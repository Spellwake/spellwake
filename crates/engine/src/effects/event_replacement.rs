//! Inline execution and optional-prompt coordination for `StaticReplacementAction::ReplaceWithBody` (CR 614.1).
use super::*;
use crate::{
    characteristics::{
        EvaluatedCharacteristics, ReplaceableEvent, StaticAbilityId, StaticContinuousAbility,
        StaticReplacementAction, StaticSourceCondition, WorldEvalCtx, eval_world_condition,
    },
    life::LifeAdjustment,
    occurrence::{EventCause, RulesOccurrence, StatusReason, ZoneReason},
    program::{
        AffectedPlayers, EffectSubject, ExecutableBody, Instruction, LibraryPlacement,
        MoveSubjectRef, PublicZoneDestination, ResolvingPlayer, SubjectDestination,
    },
    token::TokenRecipients,
};

/// Returns `true` iff every instruction in `body` is faithfully executed by
/// `EffectWindow::run_inline_replacement_body` without requiring stack/target state.
pub(crate) fn is_inline_replacement_body_supported(body: &ExecutableBody) -> bool {
    if body.target != BodyTarget::None || body.instructions.is_empty() {
        return false;
    }
    body.instructions.iter().all(|ins| match ins {
        Instruction::Jump { .. }
        | Instruction::BranchIfFalse { .. }
        | Instruction::GainControllerLife { .. }
        | Instruction::AdjustControllerLife { .. }
        | Instruction::AdjustPlayersLife { .. }
        | Instruction::DrainOpponents(_)
        | Instruction::DrawController { .. }
        | Instruction::DrawPlayer { .. }
        | Instruction::Mill { .. }
        | Instruction::MillEachPlayer(_)
        | Instruction::PutCountersOnSource { .. }
        | Instruction::PutChargeCounterOnSource
        | Instruction::PutBlessingCounterOnSource
        | Instruction::PutQuestCounterOnSource
        | Instruction::PutStorageCounterOnSource
        | Instruction::PutNamedCounterOnSource { .. }
        | Instruction::AddExperienceCounter
        | Instruction::AddPlayerCounter { .. }
        | Instruction::WinGame(_)
        | Instruction::LoseGame(_)
        | Instruction::TakeExtraTurn
        | Instruction::BecomeMonarch(_)
        | Instruction::DisallowDamagePreventionThisTurn
        | Instruction::GrantNoMaxHandSizeRestOfGame
        | Instruction::SetGroupTapped { .. }
        | Instruction::DestroyGroup(_)
        | Instruction::DestroyGroupWithoutRegeneration(_)
        | Instruction::ExileTopPlay { .. }
        | Instruction::ExileTopFaceDown => true,
        Instruction::PutCountersOnSubject { subject, .. } => {
            matches!(
                subject.as_ref(),
                EffectSubject::AbilitySource | EffectSubject::Matching(_)
            )
        }
        Instruction::CreateTokens { creations } => creations.iter().all(|c| {
            c.base_size.is_none()
                && matches!(
                    c.recipients,
                    TokenRecipients::Controller
                        | TokenRecipients::EachPlayer
                        | TokenRecipients::EachOpponent
                        | TokenRecipients::Player(_)
                )
        }),
        Instruction::MoveSubject {
            subject: MoveSubjectRef::AbilitySource,
            destination,
        } => matches!(
            destination,
            SubjectDestination::Exile {
                delayed_return: None
            } | SubjectDestination::Graveyard
                | SubjectDestination::Hand
                | SubjectDestination::Library(
                    LibraryPlacement::Top | LibraryPlacement::Bottom | LibraryPlacement::FromTop(_)
                )
        ),
        Instruction::MoveGroup { destination, .. } => matches!(
            destination,
            PublicZoneDestination::Hand
                | PublicZoneDestination::Exile
                | PublicZoneDestination::Graveyard
        ),
        _ => false,
    })
}

/// Event family eligible for body-based replacement lookup via `find_event_body_replacement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplacementEventKind {
    Draw,
    Scry,
    Mill,
}

/// Active replacement effect whose action executes an inline body in place of the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EventBodyReplacement {
    pub(super) source: Option<ObjectId>,
    pub(super) ability: StaticAbilityId,
    pub(super) controller: SeatId,
    pub(super) affected_player: SeatId,
    pub(super) optional: bool,
    pub(super) body: ExecutableBody,
}

/// Suspended optional life-change replacement awaiting `DecisionReply::OptionalEffect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingLifeReplacement {
    pub(super) entry: StackEntryId,
    pub(super) instruction: usize,
    pub(super) player: SeatId,
    pub(super) adjustment: LifeAdjustment,
    pub(super) candidate: EventBodyReplacement,
    pub(super) remaining: BTreeMap<SeatId, LifeAdjustment>,
    pub(super) declined: BTreeSet<(Option<ObjectId>, StaticAbilityId)>,
}

/// Result of inspecting active life replacements for a batch of player life adjustments.
#[derive(Debug, Default)]
pub(super) struct LifeReplacementOutcome {
    pub(super) mandatory: Vec<EventBodyReplacement>,
    pub(super) optional: Option<(SeatId, LifeAdjustment, EventBodyReplacement)>,
}

impl EffectWindow {
    /// True if any card definition or active temporary rule defines a `ReplaceWithBody` draw replacement.
    pub(super) fn has_draw_body_replacement_source(&self) -> bool {
        let is_draw_body = |ab: &StaticContinuousAbility| {
            matches!(
                ab,
                StaticContinuousAbility::EventReplacement {
                    event: ReplaceableEvent::Draw { .. },
                    action: StaticReplacementAction::ReplaceWithBody { .. },
                    ..
                }
            )
        };
        self.temporary_rules.iter().any(|active| {
            !active.consumed
                && matches!(
                    &active.rule,
                    crate::program::TemporaryRuleKind::TemporaryStatic(ab) if is_draw_body(ab)
                )
        }) || self.programs.values().any(|program| {
            program
                .continuous
                .iter()
                .chain(&program.additional_battlefield_abilities)
                .any(is_draw_body)
        })
    }

    /// Pending optional decision for a suspended life-change body replacement, if any.
    pub(super) fn pending_life_replacement_decision(&self) -> Option<EffectDecision> {
        let work = self.pending_life_replacement.as_ref()?;
        Some(EffectDecision::OptionalEffect(
            OptionalEffectFrame::event_replacement(self.revision, work.candidate.affected_player),
        ))
    }

    /// Resume a suspended optional life-change replacement after `DecisionReply::OptionalEffect`.
    pub(super) fn resume_life_replacement(
        &self,
        expected: u64,
        frame: &OptionalEffectFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_life_replacement_decision().as_ref()
            != Some(&EffectDecision::OptionalEffect(frame.clone()))
        {
            return Err(EffectError::WrongOptionalEffectFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::OptionalEffect(choice) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if !matches!(
            choice,
            OptionalEffectChoice::Take | OptionalEffectChoice::Decline
        ) {
            return Err(EffectError::IllegalOptionalEffect);
        }
        let mut next = self.clone();
        next.revision = revision;
        let Some(work) = next.pending_life_replacement.take() else {
            return Err(EffectError::WrongOptionalEffectFrame);
        };
        let mut append = RulesJournal::default();
        let mut remaining = work.remaining;
        let mut declined = work.declined;
        match choice {
            OptionalEffectChoice::Take => {
                next.execute_inline_replacement_body(
                    &work.candidate,
                    EventCause::Instruction {
                        entry: work.entry,
                        instruction: work.instruction,
                    },
                    &mut append,
                )?;
            }
            OptionalEffectChoice::Decline => {
                declined.insert((work.candidate.source, work.candidate.ability));
                remaining.insert(work.player, work.adjustment);
            }
            _ => return Err(EffectError::IllegalOptionalEffect),
        }
        if !remaining.is_empty() {
            next.adjust_players_at_cut_with_declined(
                work.entry,
                work.instruction,
                remaining,
                declined,
                &mut append,
            )?;
        }
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )
    }

    /// Locate the first active `ReplaceWithBody` replacement matching `kind` for `player`
    /// that has not already applied to this event (CR 614.5) or been declined.
    pub(super) fn find_event_body_replacement(
        &self,
        evaluated: &EvaluatedCharacteristics,
        kind: ReplacementEventKind,
        player: SeatId,
        excluded: &BTreeSet<(Option<ObjectId>, StaticAbilityId)>,
    ) -> Result<Option<EventBodyReplacement>, EffectError> {
        let opponents = self
            .combat_rules
            .opponents(self.window.participants(), player)?;
        let matches_event = |event: &ReplaceableEvent| -> Option<AffectedPlayers> {
            match (kind, event) {
                (ReplacementEventKind::Draw, ReplaceableEvent::Draw { player: rel })
                | (ReplacementEventKind::Scry, ReplaceableEvent::Scry { player: rel })
                | (ReplacementEventKind::Mill, ReplaceableEvent::Mill { player: rel }) => {
                    Some(*rel)
                }
                _ => None,
            }
        };
        let relation_applies = |rel: AffectedPlayers, ctrl: SeatId| match rel {
            AffectedPlayers::One(ResolvingPlayer::Controller) => player == ctrl,
            AffectedPlayers::EachOpponent | AffectedPlayers::EachOtherPlayer => {
                opponents.contains(&ctrl)
            }
            AffectedPlayers::EachPlayer | AffectedPlayers::One(_) => true,
        };

        for perm in evaluated
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield)
        {
            let Some(ctrl) = perm.controller else {
                continue;
            };
            for ab in perm
                .printed_text
                .continuous
                .iter()
                .chain(&perm.printed_text.additional_battlefield_abilities)
            {
                let active = perm.abilities.continuous == Some(ab.id())
                    || perm
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ab.id());
                if !active {
                    continue;
                }
                let StaticContinuousAbility::EventReplacement {
                    id,
                    event,
                    condition,
                    action: StaticReplacementAction::ReplaceWithBody { optional, body },
                } = ab
                else {
                    continue;
                };
                let key = (Some(perm.object), *id);
                if self.active_event_replacements.contains(&key) || excluded.contains(&key) {
                    continue;
                }
                let Some(rel) = matches_event(event) else {
                    continue;
                };
                if !relation_applies(rel, ctrl) {
                    continue;
                }
                let cond_ok = condition.as_deref().is_none_or(|cond| match cond {
                    StaticSourceCondition::Eval(rc) => {
                        let ctx = WorldEvalCtx {
                            active_player: self.window.active_player(),
                            participants: self.window.participants(),
                            life: Some(&self.life),
                            combat: self.combat.as_ref(),
                            cards: self.window.cards(),
                            programs: &self.programs,
                            state: &self.characteristics,
                            objects: &evaluated.objects,
                            source: perm.object,
                            controller: ctrl,
                            name_rules: &self.name_rules,
                            trigger_subject: None,
                            related_player: Some(player),
                            defending_player: None,
                        };
                        eval_world_condition(&ctx, rc)
                    }
                    _ => true,
                });
                if !cond_ok {
                    continue;
                }
                return Ok(Some(EventBodyReplacement {
                    source: Some(perm.object),
                    ability: *id,
                    controller: ctrl,
                    affected_player: player,
                    optional: *optional,
                    body: (**body).clone(),
                }));
            }
        }

        for active in &self.temporary_rules {
            if active.consumed {
                continue;
            }
            let crate::program::TemporaryRuleKind::TemporaryStatic(ab) = &active.rule else {
                continue;
            };
            let StaticContinuousAbility::EventReplacement {
                id,
                event,
                condition,
                action: StaticReplacementAction::ReplaceWithBody { optional, body },
            } = ab.as_ref()
            else {
                continue;
            };
            let key = (active.source_object, *id);
            if self.active_event_replacements.contains(&key) || excluded.contains(&key) {
                continue;
            }
            let Some(rel) = matches_event(event) else {
                continue;
            };
            if !relation_applies(rel, active.controller) {
                continue;
            }
            let cond_ok = condition.as_deref().is_none_or(|cond| match cond {
                StaticSourceCondition::Eval(rc) => active.source_object.is_some_and(|src| {
                    let ctx = WorldEvalCtx {
                        active_player: self.window.active_player(),
                        participants: self.window.participants(),
                        life: Some(&self.life),
                        combat: self.combat.as_ref(),
                        cards: self.window.cards(),
                        programs: &self.programs,
                        state: &self.characteristics,
                        objects: &evaluated.objects,
                        source: src,
                        controller: active.controller,
                        name_rules: &self.name_rules,
                        trigger_subject: None,
                        related_player: Some(player),
                        defending_player: None,
                    };
                    eval_world_condition(&ctx, rc)
                }),
                _ => true,
            });
            if !cond_ok {
                continue;
            }
            return Ok(Some(EventBodyReplacement {
                source: active.source_object,
                ability: *id,
                controller: active.controller,
                affected_player: player,
                optional: *optional,
                body: (**body).clone(),
            }));
        }

        Ok(None)
    }

    /// Execute an inline replacement body without putting an object on the stack (CR 614.1),
    /// guarding `(rep.source, rep.ability)` against CR 614.5 self-reapplication.
    pub(super) fn execute_inline_replacement_body(
        &mut self,
        rep: &EventBodyReplacement,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let key = (rep.source, rep.ability);
        if !self.active_event_replacements.insert(key) {
            return Ok(());
        }
        let result = self.run_inline_replacement_body(rep, cause, append);
        self.active_event_replacements.remove(&key);
        result
    }

    fn resolve_replacement_player(
        &self,
        role: ResolvingPlayer,
        rep: &EventBodyReplacement,
    ) -> Result<SeatId, EffectError> {
        match role {
            ResolvingPlayer::Controller => Ok(rep.controller),
            ResolvingPlayer::ActivePlayer => Ok(self.window.active_player()),
            ResolvingPlayer::UniqueRelatedPlayer
            | ResolvingPlayer::TargetPlayer
            | ResolvingPlayer::TargetController
            | ResolvingPlayer::TargetOwner => Ok(rep.affected_player),
            _ => Ok(rep.controller),
        }
    }

    fn resolve_replacement_players(
        &self,
        players: AffectedPlayers,
        rep: &EventBodyReplacement,
    ) -> Result<Vec<SeatId>, EffectError> {
        match players {
            AffectedPlayers::One(role) => {
                let seat = self.resolve_replacement_player(role, rep)?;
                Ok(self
                    .window
                    .participants()
                    .contains(&seat)
                    .then_some(seat)
                    .into_iter()
                    .collect())
            }
            AffectedPlayers::EachPlayer => Ok(self.window.apnap_players()?.collect()),
            AffectedPlayers::EachOpponent | AffectedPlayers::EachOtherPlayer => {
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), rep.controller)?;
                Ok(self
                    .window
                    .apnap_players()?
                    .filter(|seat| opponents.contains(seat))
                    .collect())
            }
        }
    }

    fn run_inline_replacement_body(
        &mut self,
        rep: &EventBodyReplacement,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let synthetic_entry = match &cause {
            EventCause::Instruction { entry, .. } => *entry,
            _ => StackEntryId::entry_replacement_creator(),
        };
        let synthetic_index = match &cause {
            EventCause::Instruction { instruction, .. } => *instruction,
            _ => 0,
        };
        let mut pc = 0usize;
        let mut steps = 0usize;
        while pc < rep.body.instructions.len() && steps < 256 && self.won_by_effect.is_none() {
            steps += 1;
            let instruction = rep.body.instructions[pc].clone();
            match instruction {
                Instruction::Jump { destination } => {
                    pc = destination;
                    continue;
                }
                Instruction::BranchIfFalse {
                    condition,
                    destination,
                } => {
                    let left = self.resolve_value_expr_with_source(
                        rep.controller,
                        &condition.left,
                        None,
                        rep.source,
                    )?;
                    let right = self.resolve_value_expr_with_source(
                        rep.controller,
                        &condition.right,
                        None,
                        rep.source,
                    )?;
                    if !condition.comparison.evaluate(&left, &right) {
                        pc = destination;
                        continue;
                    }
                }
                Instruction::GainControllerLife { amount } => {
                    let val = self
                        .resolve_value_expr_with_source(rep.controller, &amount, None, rep.source)?
                        .max(0.into());
                    self.adjust_players_at_cut(
                        synthetic_entry,
                        synthetic_index,
                        BTreeMap::from([(rep.controller, LifeAdjustment::Gain(val))]),
                        append,
                    )?;
                }
                Instruction::AdjustControllerLife { adjustment } => {
                    self.adjust_players_at_cut(
                        synthetic_entry,
                        synthetic_index,
                        BTreeMap::from([(rep.controller, adjustment)]),
                        append,
                    )?;
                }
                Instruction::AdjustPlayersLife {
                    players,
                    gain,
                    amount,
                } => {
                    let val = self
                        .resolve_value_expr_with_source(rep.controller, &amount, None, rep.source)?
                        .max(0.into());
                    let seats = self.resolve_replacement_players(players, rep)?;
                    let adj = if gain {
                        LifeAdjustment::Gain(val)
                    } else {
                        LifeAdjustment::Lose(val)
                    };
                    let map = seats.into_iter().map(|s| (s, adj.clone())).collect();
                    self.adjust_players_at_cut(synthetic_entry, synthetic_index, map, append)?;
                }
                Instruction::DrainOpponents(amount) => {
                    let val = self
                        .resolve_value_expr_with_source(rep.controller, &amount, None, rep.source)?
                        .max(0.into());
                    self.drain_opponents(
                        synthetic_entry,
                        synthetic_index,
                        rep.controller,
                        &val,
                        append,
                    )?;
                }
                Instruction::DrawController { amount } => {
                    let val = self
                        .resolve_value_expr_with_source(rep.controller, &amount, None, rep.source)?
                        .max(0.into());
                    if val.is_positive() && self.window.participants().contains(&rep.controller) {
                        let _ = self.draw_at_cuts(
                            rep.controller,
                            val,
                            DrawOrigin::Instruction {
                                entry: synthetic_entry,
                                instruction: synthetic_index,
                            },
                            append,
                        )?;
                    }
                }
                Instruction::DrawPlayer { players, amount } => {
                    let val = self
                        .resolve_value_expr_with_source(rep.controller, &amount, None, rep.source)?
                        .max(0.into());
                    if val.is_positive() {
                        for seat in self.resolve_replacement_players(players, rep)? {
                            let _ = self.draw_at_cuts(
                                seat,
                                val.clone(),
                                DrawOrigin::Instruction {
                                    entry: synthetic_entry,
                                    instruction: synthetic_index,
                                },
                                append,
                            )?;
                        }
                    }
                }
                Instruction::Mill { player, amount } => {
                    let seat = self.resolve_replacement_player(player, rep)?;
                    let val = self
                        .resolve_value_expr_with_source(rep.controller, &amount, None, rep.source)?
                        .max(0.into());
                    self.inline_mill_player(seat, &val, cause.clone(), append)?;
                }
                Instruction::MillEachPlayer(amount) => {
                    let val = RulesInteger::from(i64::from(amount));
                    let seats: Vec<SeatId> = self.window.apnap_players()?.collect();
                    for seat in seats {
                        self.inline_mill_player(seat, &val, cause.clone(), append)?;
                    }
                }
                Instruction::PutCountersOnSource { amount } => {
                    if let Some(source) = rep.source
                        && self
                            .window
                            .cards()
                            .get(source)
                            .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                    {
                        self.place_plus_one_counter_amount(
                            vec![source],
                            amount,
                            cause.clone(),
                            append,
                        )?;
                    }
                }
                Instruction::PutChargeCounterOnSource => {
                    if let Some(source) = rep.source {
                        self.inline_put_permanent_counters(
                            &[source],
                            CounterKind::Charge,
                            &1.into(),
                            cause.clone(),
                            append,
                        )?;
                    }
                }
                Instruction::PutBlessingCounterOnSource => {
                    if let Some(source) = rep.source {
                        self.inline_put_permanent_counters(
                            &[source],
                            CounterKind::Blessing,
                            &1.into(),
                            cause.clone(),
                            append,
                        )?;
                    }
                }
                Instruction::PutQuestCounterOnSource => {
                    if let Some(source) = rep.source {
                        self.inline_put_permanent_counters(
                            &[source],
                            CounterKind::Quest,
                            &1.into(),
                            cause.clone(),
                            append,
                        )?;
                    }
                }
                Instruction::PutStorageCounterOnSource => {
                    if let Some(source) = rep.source {
                        self.inline_put_permanent_counters(
                            &[source],
                            CounterKind::Storage,
                            &1.into(),
                            cause.clone(),
                            append,
                        )?;
                    }
                }
                Instruction::PutNamedCounterOnSource { kind, amount } => {
                    if let Some(source) = rep.source {
                        self.inline_put_permanent_counters(
                            &[source],
                            kind,
                            &i64::from(amount).into(),
                            cause.clone(),
                            append,
                        )?;
                    }
                }
                Instruction::PutCountersOnSubject {
                    subject,
                    kind,
                    amount,
                } => {
                    let val = self
                        .resolve_value_expr_with_source(rep.controller, &amount, None, rep.source)?
                        .max(0.into());
                    if val.is_positive() {
                        let objects = self.inline_resolve_subject(*subject, rep)?;
                        self.inline_put_permanent_counters(
                            &objects,
                            kind,
                            &val,
                            cause.clone(),
                            append,
                        )?;
                    }
                }
                Instruction::AddExperienceCounter => {
                    self.inline_add_player_counter(
                        rep.controller,
                        CounterKind::Experience,
                        &1.into(),
                        cause.clone(),
                        append,
                    )?;
                }
                Instruction::AddPlayerCounter {
                    players,
                    kind,
                    amount,
                } => {
                    let val = self
                        .resolve_value_expr_with_source(rep.controller, &amount, None, rep.source)?
                        .max(0.into());
                    if val.is_positive() {
                        for seat in self.resolve_replacement_players(players, rep)? {
                            self.inline_add_player_counter(
                                seat,
                                kind,
                                &val,
                                cause.clone(),
                                append,
                            )?;
                        }
                    }
                }
                Instruction::CreateTokens { creations } => {
                    self.inline_create_tokens(&creations, rep, cause.clone(), append)?;
                }
                Instruction::WinGame(player) => {
                    let winner = self.resolve_replacement_player(player, rep)?;
                    if self.window.participants().contains(&winner) {
                        self.won_by_effect =
                            Some((winner, rep.source.unwrap_or(ObjectId::SENTINEL)));
                    }
                }
                Instruction::LoseGame(players) => {
                    let losing: BTreeSet<SeatId> = self
                        .resolve_replacement_players(players, rep)?
                        .into_iter()
                        .filter(|seat| self.window.participants().contains(seat))
                        .collect();
                    if !losing.is_empty() {
                        let before = self.rules_snapshot(None)?;
                        let mut loss_occs = Vec::with_capacity(losing.len());
                        for &seat in &losing {
                            self.state_based_losses.insert(seat);
                            loss_occs.push(RulesOccurrence::PlayerLost {
                                player: seat,
                                reasons: [crate::occurrence::PlayerLossReason::Effect].into(),
                            });
                        }
                        self.emit_from(cause.clone(), &before, None, append, |_, _, _| {
                            Ok(loss_occs)
                        })?;
                    }
                }
                Instruction::TakeExtraTurn => {
                    self.extra_turns.push(rep.controller);
                    let before = self.rules_snapshot(None)?;
                    self.emit_from(cause.clone(), &before, None, append, |_, _, _| {
                        Ok(vec![RulesOccurrence::ExtraTurnCreated {
                            player: rep.controller,
                        }])
                    })?;
                }
                Instruction::BecomeMonarch(player) => {
                    self.become_monarch(
                        synthetic_entry,
                        synthetic_index,
                        player.unwrap_or(rep.controller),
                        append,
                    )?;
                }
                Instruction::DisallowDamagePreventionThisTurn => {
                    self.damage_state.prevention_disallowed_this_turn = true;
                }
                Instruction::GrantNoMaxHandSizeRestOfGame => {
                    self.characteristics
                        .no_max_hand_size_players
                        .insert(rep.controller);
                }
                Instruction::SetGroupTapped { filter, tapped } => {
                    let evaluated = self.evaluated_characteristics()?;
                    let excluded = BTreeSet::new();
                    let changing: Vec<ObjectId> = evaluated
                        .objects
                        .values()
                        .filter(|p| {
                            p.zone == ObjectZone::Battlefield
                                && self.matches_object_filter(
                                    *filter,
                                    p,
                                    rep.controller,
                                    rep.source,
                                    &excluded,
                                    &evaluated,
                                )
                                && self
                                    .window
                                    .cards()
                                    .get(p.object)
                                    .and_then(GameObject::tapped)
                                    == Some(!tapped)
                        })
                        .map(|p| p.object)
                        .collect();
                    if !changing.is_empty() {
                        let before = self.rules_snapshot(None)?;
                        let occurrences = if tapped {
                            let (window, changes) = self
                                .window
                                .record_inline_replacement_status(&changing, true)?;
                            self.window = window;
                            occurrence_flow::status_occurrences(changes, StatusReason::Effect)
                        } else {
                            let mut untapping = Vec::with_capacity(changing.len());
                            let mut stun_occurrences = Vec::new();
                            for id in changing {
                                if self.counters_of_kind(id, CounterKind::Stun).is_positive() {
                                    let prev = self.counters_of_kind(id, CounterKind::Stun);
                                    let after = prev.minus(&1.into());
                                    if let Some(map) = self
                                        .characteristics
                                        .named_permanent_counters_mut(CounterKind::Stun)
                                    {
                                        if after.is_positive() {
                                            map.insert(id, after.clone());
                                        } else {
                                            map.remove(&id);
                                        }
                                    }
                                    stun_occurrences.push(
                                        RulesOccurrence::PermanentCounterChanged {
                                            object: id,
                                            kind: CounterKind::Stun,
                                            before: prev,
                                            after,
                                        },
                                    );
                                } else {
                                    untapping.push(id);
                                }
                            }
                            let mut occs = if untapping.is_empty() {
                                Vec::new()
                            } else {
                                let (window, changes) = self
                                    .window
                                    .record_inline_replacement_status(&untapping, false)?;
                                self.window = window;
                                occurrence_flow::status_occurrences(changes, StatusReason::Effect)
                            };
                            occs.extend(stun_occurrences);
                            occs
                        };
                        self.emit_from(cause.clone(), &before, None, append, |_, _, _| {
                            Ok(occurrences)
                        })?;
                    }
                }
                Instruction::MoveSubject {
                    subject: MoveSubjectRef::AbilitySource,
                    destination,
                } => {
                    if let Some(source) = rep.source
                        && self
                            .window
                            .cards()
                            .get(source)
                            .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                    {
                        let (dest, reason) = match destination {
                            SubjectDestination::Exile {
                                delayed_return: None,
                            } => (ObjectDestination::Exile, ZoneReason::ExiledByEffect),
                            SubjectDestination::Graveyard => (
                                ObjectDestination::Graveyard,
                                ZoneReason::Sacrifice {
                                    player: rep.controller,
                                },
                            ),
                            SubjectDestination::Hand => {
                                (ObjectDestination::Hand, ZoneReason::ReturnToOwnersHand)
                            }
                            SubjectDestination::Library(placement) => {
                                let dest = match placement {
                                    LibraryPlacement::Top => ObjectDestination::LibraryTop,
                                    LibraryPlacement::Bottom => ObjectDestination::LibraryBottom,
                                    LibraryPlacement::FromTop(n) => {
                                        ObjectDestination::LibraryFromTop(n)
                                    }
                                    LibraryPlacement::ChoiceTopOrBottom { .. } => {
                                        return Err(EffectError::InconsistentProgram);
                                    }
                                };
                                (dest, ZoneReason::PutIntoOwnersLibrary(placement))
                            }
                            _ => return Err(EffectError::InconsistentProgram),
                        };
                        let _ =
                            self.inline_move_object(source, dest, reason, cause.clone(), append)?;
                    }
                }
                Instruction::DestroyGroup(filter)
                | Instruction::DestroyGroupWithoutRegeneration(filter) => {
                    let evaluated = self.evaluated_characteristics()?;
                    let excluded = BTreeSet::new();
                    let targets: Vec<ObjectId> = evaluated
                        .objects
                        .values()
                        .filter(|p| {
                            p.zone == ObjectZone::Battlefield
                                && !p.has_keyword(KeywordAbility::Indestructible)
                                && self.matches_object_filter(
                                    *filter,
                                    p,
                                    rep.controller,
                                    rep.source,
                                    &excluded,
                                    &evaluated,
                                )
                        })
                        .map(|p| p.object)
                        .collect();
                    for id in targets {
                        let _ = self.inline_move_object(
                            id,
                            ObjectDestination::Graveyard,
                            ZoneReason::Destroyed,
                            cause.clone(),
                            append,
                        )?;
                    }
                }
                Instruction::MoveGroup { group, destination } => {
                    let evaluated = self.evaluated_characteristics()?;
                    let excluded = BTreeSet::new();
                    let targets: Vec<ObjectId> = evaluated
                        .objects
                        .values()
                        .filter(|p| {
                            p.zone == ObjectZone::Battlefield
                                && self.matches_object_filter(
                                    *group,
                                    p,
                                    rep.controller,
                                    rep.source,
                                    &excluded,
                                    &evaluated,
                                )
                        })
                        .map(|p| p.object)
                        .collect();
                    let (dest, reason) = match destination {
                        PublicZoneDestination::Hand => {
                            (ObjectDestination::Hand, ZoneReason::ReturnToOwnersHand)
                        }
                        PublicZoneDestination::Exile => {
                            (ObjectDestination::Exile, ZoneReason::ExiledByEffect)
                        }
                        PublicZoneDestination::Graveyard => (
                            ObjectDestination::Graveyard,
                            ZoneReason::Sacrifice {
                                player: rep.controller,
                            },
                        ),
                        PublicZoneDestination::Battlefield
                        | PublicZoneDestination::Library
                        | PublicZoneDestination::Command => {
                            return Err(EffectError::InconsistentProgram);
                        }
                    };
                    for id in targets {
                        let _ = self.inline_move_object(id, dest, reason, cause.clone(), append)?;
                    }
                }
                Instruction::ExileTopPlay {
                    count,
                    library_owner,
                    duration,
                    cast_only,
                    free,
                    any_color,
                    face_down,
                    grant_play,
                } => {
                    let val = self
                        .resolve_value_expr_with_source(rep.controller, &count, None, rep.source)?
                        .max(0.into());
                    let n = val.to_usize().unwrap_or(0);
                    if n > 0 {
                        for seat in self.resolve_replacement_players(library_owner, rep)? {
                            let top: Vec<ObjectId> = self
                                .window
                                .cards()
                                .library(seat)
                                .iter()
                                .take(n)
                                .copied()
                                .collect();
                            for id in top {
                                if let Some(exiled_id) = self.inline_move_object(
                                    id,
                                    ObjectDestination::Exile,
                                    ZoneReason::ExiledByEffect,
                                    cause.clone(),
                                    append,
                                )? {
                                    if face_down {
                                        self.face_down_exile.insert(exiled_id, rep.controller);
                                    }
                                    if grant_play {
                                        if cast_only
                                            && self
                                                .copiable_program(exiled_id)
                                                .is_some_and(|p| p.has_type(CardType::Land))
                                        {
                                            continue;
                                        }
                                        let permission = (exiled_id, rep.controller);
                                        if duration
                                            == crate::program::ImpulsePlayDuration::UntilNextFromSource
                                            && let Some(source) = rep.source
                                        {
                                            self.exile_play_until_next.insert(source, permission);
                                            if free {
                                                self.free_exiled_cards.insert(permission);
                                            }
                                            continue;
                                        }
                                        let mapped_duration = match duration {
                                            crate::program::ImpulsePlayDuration::ThisTurn => {
                                                CardPlayDuration::ThroughTurn
                                            }
                                            crate::program::ImpulsePlayDuration::UntilEndOfNextTurn => {
                                                if self.window.active_player() == rep.controller {
                                                    CardPlayDuration::ThroughNextOwnTurnSkipCurrent(
                                                        rep.controller,
                                                    )
                                                } else {
                                                    CardPlayDuration::ThroughNextOwnTurn(
                                                        rep.controller,
                                                    )
                                                }
                                            }
                                            crate::program::ImpulsePlayDuration::WhileExiled
                                            | crate::program::ImpulsePlayDuration::UntilNextFromSource => {
                                                CardPlayDuration::WhileInZone
                                            }
                                        };
                                        self.card_play_permissions.insert(CardPlayPermission {
                                            zone: CardPlayZone::Exile,
                                            object: exiled_id,
                                            player: rep.controller,
                                            duration: mapped_duration,
                                            any_color,
                                            free,
                                            cast_only,
                                        });
                                        if free {
                                            self.free_exiled_cards.insert(permission);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Instruction::ExileTopFaceDown => {
                    let top = self.window.cards().library(rep.controller).first().copied();
                    if let Some(id) = top
                        && let Some(exiled_id) = self.inline_move_object(
                            id,
                            ObjectDestination::Exile,
                            ZoneReason::ExiledByEffect,
                            cause.clone(),
                            append,
                        )?
                    {
                        self.face_down_exile.insert(exiled_id, rep.controller);
                    }
                }
                _ => return Err(EffectError::InconsistentProgram),
            }
            pc += 1;
        }
        Ok(())
    }

    fn inline_resolve_subject(
        &self,
        subject: EffectSubject,
        rep: &EventBodyReplacement,
    ) -> Result<Vec<ObjectId>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        match subject {
            EffectSubject::AbilitySource => Ok(rep
                .source
                .filter(|id| {
                    evaluated
                        .get(*id)
                        .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                })
                .into_iter()
                .collect()),
            EffectSubject::Matching(filter) => {
                let excluded = BTreeSet::new();
                Ok(evaluated
                    .objects
                    .values()
                    .filter(|p| {
                        p.zone == ObjectZone::Battlefield
                            && self.matches_object_filter(
                                filter,
                                p,
                                rep.controller,
                                rep.source,
                                &excluded,
                                &evaluated,
                            )
                    })
                    .map(|p| p.object)
                    .collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn inline_mill_player(
        &mut self,
        player: SeatId,
        amount: &RulesInteger,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if !amount.is_positive() || !self.window.participants().contains(&player) {
            return Ok(());
        }
        let adjusted =
            self.adjusted_library_action_amount(LibraryActionKind::Mill, player, amount.clone())?;
        let count = adjusted
            .to_usize()
            .unwrap_or(usize::MAX)
            .min(self.window.cards().library(player).len());
        if count == 0 {
            return Ok(());
        }
        let cards: Vec<ObjectId> = self.window.cards().library(player)[..count].to_vec();
        for id in cards {
            self.inline_move_object(
                id,
                ObjectDestination::Graveyard,
                ZoneReason::Mill { player },
                cause.clone(),
                append,
            )?;
        }
        Ok(())
    }

    fn inline_move_object(
        &mut self,
        id: ObjectId,
        destination: ObjectDestination,
        reason: ZoneReason,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<Option<ObjectId>, EffectError> {
        if self.window.cards().get(id).is_none() {
            return Ok(None);
        }
        let before = self.rules_snapshot(None)?;
        let (window, movement) = self
            .window
            .record_inline_replacement_move(id, destination)?;
        self.window = window;
        let Some(movement) = movement else {
            return Ok(None);
        };
        let moved_id = movement.after.id();
        self.emit_from(cause, &before, None, append, |old, new, controllers| {
            Ok(vec![zone_occurrence_at(
                old,
                new,
                &movement,
                reason,
                controllers,
            )?])
        })?;
        Ok(Some(moved_id))
    }

    fn inline_put_permanent_counters(
        &mut self,
        objects: &[ObjectId],
        kind: CounterKind,
        amount: &RulesInteger,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if !amount.is_positive() || objects.is_empty() {
            return Ok(());
        }
        if kind == CounterKind::PlusOne {
            return self.place_plus_one_counter_amount(
                objects.to_vec(),
                amount.clone(),
                cause,
                append,
            );
        }
        let before = self.rules_snapshot(None)?;
        let mut events = Vec::new();
        for &object in objects {
            if !self
                .window
                .cards()
                .get(object)
                .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
            {
                continue;
            }
            let mult = super::permanent_counters::effect_counter_multiplier(
                &before.view().evaluated,
                object,
            );
            let added = amount.times(&mult);
            if !added.is_positive() {
                continue;
            }
            if kind == CounterKind::MinusOne {
                let prev = self.characteristics.counters(object);
                let delta = PowerToughnessCounters {
                    plus_one: 0.into(),
                    minus_one: added,
                };
                self.characteristics.place_counters(object, &delta)?;
                let after = self.characteristics.counters(object);
                events.push(RulesOccurrence::CountersPlaced {
                    object,
                    before: prev,
                    added: delta,
                    after,
                    before_appearance: Box::new(
                        before
                            .view()
                            .appearance(object)
                            .ok_or(EffectError::InconsistentProgram)?,
                    ),
                    after_appearance: Box::new(
                        self.rules_snapshot(None)?
                            .view()
                            .appearance(object)
                            .ok_or(EffectError::InconsistentProgram)?,
                    ),
                });
            } else if let Some(map) = self.characteristics.named_permanent_counters_mut(kind) {
                let prev = map.get(&object).cloned().unwrap_or_else(|| 0.into());
                let after = prev.plus(&added);
                map.insert(object, after.clone());
                events.push(match kind {
                    CounterKind::Charge => RulesOccurrence::ChargeCountersChanged {
                        object,
                        before: prev,
                        after,
                    },
                    CounterKind::Suspect => RulesOccurrence::SuspectCountersChanged {
                        object,
                        before: prev,
                        after,
                    },
                    _ => RulesOccurrence::PermanentCounterChanged {
                        object,
                        kind,
                        before: prev,
                        after,
                    },
                });
            }
        }
        if !events.is_empty() {
            self.emit_from(cause, &before, None, append, |_, _, _| Ok(events))?;
        }
        Ok(())
    }

    fn inline_add_player_counter(
        &mut self,
        player: SeatId,
        kind: CounterKind,
        amount: &RulesInteger,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if !amount.is_positive() || !self.window.participants().contains(&player) {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let prev = self.player_counters_of_kind(player, kind);
        let after = prev.plus(amount);
        match kind {
            CounterKind::Poison => {
                self.poison.insert(player, after.clone());
            }
            CounterKind::Experience => {
                self.characteristics
                    .experience
                    .insert(player, after.clone());
            }
            other => {
                self.characteristics
                    .player_counters
                    .entry(other)
                    .or_default()
                    .insert(player, after.clone());
            }
        }
        self.emit_from(cause, &before, None, append, |_, _, _| {
            Ok(vec![RulesOccurrence::PlayerCounterChanged {
                player,
                kind,
                before: prev,
                after,
            }])
        })
    }

    fn inline_create_tokens(
        &mut self,
        creations: &[crate::token::TokenCreation],
        rep: &EventBodyReplacement,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let mut inputs = Vec::new();
        let mut tapped_flags = Vec::new();
        for creation in creations {
            let recipients = match creation.recipients {
                TokenRecipients::Controller => vec![rep.controller],
                TokenRecipients::EachPlayer => self.window.apnap_players()?.collect(),
                TokenRecipients::EachOpponent => {
                    let opponents = self
                        .combat_rules
                        .opponents(self.window.participants(), rep.controller)?;
                    self.window
                        .apnap_players()?
                        .filter(|s| opponents.contains(s))
                        .collect()
                }
                TokenRecipients::Player(role) => vec![self.resolve_replacement_player(role, rep)?],
                _ => vec![rep.controller],
            };
            let raw_count = self
                .resolve_value_expr_with_source(rep.controller, &creation.amount, None, rep.source)?
                .max(0.into())
                .to_usize()
                .unwrap_or(0);
            for creator in recipients {
                if !self.window.participants().contains(&creator) {
                    continue;
                }
                let adjusted = self.adjusted_token_count(
                    &before.view().evaluated,
                    creator,
                    raw_count,
                    Some(creation.definition.program()),
                )?;
                for _ in 0..adjusted {
                    inputs.push((creator, creation.definition.program().clone()));
                    tapped_flags.push(creation.enters_tapped);
                }
            }
        }
        if inputs.is_empty() {
            return Ok(());
        }
        let (window, objects) = self.window.record_token_creations(
            self.window.revision(),
            inputs,
            &self.creature_vocabulary,
        )?;
        self.window = window;
        let ids: Vec<ObjectId> = objects.iter().map(GameObject::id).collect();
        for (&id, &tapped) in ids.iter().zip(&tapped_flags) {
            if tapped {
                self.window.enter_tapped(id)?;
            }
        }
        self.characteristics.register_battlefield_entries(&ids)?;
        for &id in &ids {
            self.apply_entry_counters(&before, id, PowerToughnessCounters::default(), 0)?;
        }
        self.emit_from(cause, &before, None, append, |_, current, _| {
            objects
                .iter()
                .map(|object| {
                    Ok(RulesOccurrence::TokenCreated {
                        creator: object.owner(),
                        after: Box::new(
                            current
                                .appearance(object.id())
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    })
                })
                .collect()
        })
    }
}
