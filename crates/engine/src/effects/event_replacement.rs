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
        ResolvingPlayer,
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
        | Instruction::SetPlayersLife { .. }
        | Instruction::DrainOpponents(_)
        | Instruction::DrawController { .. }
        | Instruction::DrawCards(_)
        | Instruction::Mill { .. }
        | Instruction::MillEachPlayer(_)
        | Instruction::AddExperienceCounter
        | Instruction::AddPlayerCounter { .. }
        | Instruction::WinGame(_)
        | Instruction::LoseGame(_)
        | Instruction::TakeExtraTurn
        | Instruction::BecomeMonarch(_)
        | Instruction::DisallowDamagePreventionThisTurn
        | Instruction::ExileTopPlay { .. } => true,
        Instruction::ZoneOp(spec) => {
            matches!(spec.selection, crate::toolbox::ZoneSelection::All)
                && (matches!(
                    (&spec.source, &spec.destination),
                    (
                        crate::toolbox::ObjectQuery::Source,
                        crate::toolbox::ZoneDest::Exile {
                            delayed_return: None,
                            ..
                        } | crate::toolbox::ZoneDest::Graveyard { .. }
                            | crate::toolbox::ZoneDest::Hand
                            | crate::toolbox::ZoneDest::Library {
                                position: crate::toolbox::LibraryOrderPos::Top
                                    | crate::toolbox::LibraryOrderPos::Bottom
                                    | crate::toolbox::LibraryOrderPos::NthFromTop(_)
                            }
                    ) | (
                        crate::toolbox::ObjectQuery::ZoneScan {
                            zone: crate::toolbox::QueryZone::Battlefield,
                            ..
                        },
                        crate::toolbox::ZoneDest::Graveyard { .. }
                            | crate::toolbox::ZoneDest::Hand
                            | crate::toolbox::ZoneDest::Exile {
                                delayed_return: None,
                                ..
                            }
                    ) | (
                        crate::toolbox::ObjectQuery::ZoneScan {
                            zone: crate::toolbox::QueryZone::Library(_),
                            ..
                        },
                        crate::toolbox::ZoneDest::Exile {
                            face_down: true,
                            ..
                        }
                    )
                ))
        }
        Instruction::RemoveCountersFromPermanent {
            target_self: true,
            kind: Some(_),
            ..
        } => true,
        Instruction::MutateObject(spec) => {
            let valid_src = match &spec.source {
                crate::toolbox::ObjectQuery::Source
                | crate::toolbox::ObjectQuery::ZoneScan {
                    zone: crate::toolbox::QueryZone::Battlefield,
                    ..
                } => true,
                crate::toolbox::ObjectQuery::EffectSubject(subject) => matches!(
                    subject.as_ref(),
                    EffectSubject::AbilitySource | EffectSubject::Matching(_)
                ),
                _ => false,
            };
            matches!(spec.selection, crate::toolbox::ZoneSelection::All)
                && valid_src
                && matches!(
                    &spec.op,
                    crate::toolbox::ObjectMutation::AddCounters { .. }
                        | crate::toolbox::ObjectMutation::SetTapped(_)
                        | crate::toolbox::ObjectMutation::RemoveCounters { kind: Some(_), .. }
                        | crate::toolbox::ObjectMutation::ClearCounters { kind: Some(_) }
                )
        }
        Instruction::ApplyContinuous(spec) => matches!(
            &spec.payload,
            crate::toolbox::ContinuousPayload::TemporaryRule(rule)
                if matches!(
                    rule.as_ref(),
                    crate::program::TemporaryRuleKind::TemporaryStatic(ability)
                        if matches!(
                            ability.as_ref(),
                            StaticContinuousAbility::DrawRule {
                                rule: crate::characteristics::StaticDrawRule::NoMaximumHandSize,
                                ..
                            }
                        )
                )
        ),
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
                    crate::program::TemporaryRuleKind::TemporaryStatic(ab)
                        | crate::program::TemporaryRuleKind::OneShotEventReplacement(ab)
                        | crate::program::TemporaryRuleKind::ConditionalTemporaryStatic {
                            ability: ab,
                            ..
                        } if is_draw_body(ab)
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
                let cond_ok = condition.as_deref().is_none_or(|cond| {
                    self.eval_static_source_condition_at(
                        evaluated,
                        cond,
                        Some(perm.object),
                        ctrl,
                        Some(player),
                    )
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
            let Some(ab) = self.active_temporary_static(active, evaluated) else {
                continue;
            };
            let StaticContinuousAbility::EventReplacement {
                id,
                event,
                condition,
                action: StaticReplacementAction::ReplaceWithBody { optional, body },
            } = ab
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
            let cond_ok = condition.as_deref().is_none_or(|cond| {
                self.eval_static_source_condition_at(
                    evaluated,
                    cond,
                    active.source_object,
                    active.controller,
                    Some(player),
                )
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

    /// Evaluate a [`StaticSourceCondition`] against an already-evaluated characteristic cut.
    pub(super) fn eval_static_source_condition_at(
        &self,
        evaluated: &EvaluatedCharacteristics,
        condition: &StaticSourceCondition,
        source: Option<ObjectId>,
        controller: SeatId,
        related_player: Option<SeatId>,
    ) -> bool {
        use crate::characteristics::Color;
        use StaticSourceCondition::*;
        match condition {
            ControllerLifeAtMost(limit) => self
                .life
                .get(controller)
                .is_some_and(|life| *life <= i64::from(*limit).into()),
            ControllerLifeAtLeast(limit) => self
                .life
                .get(controller)
                .is_some_and(|life| *life >= i64::from(*limit).into()),
            ControllerControlsSubtype(kind) | OpponentControlsSubtype(kind) => {
                let opponents = matches!(condition, OpponentControlsSubtype(_));
                evaluated.objects.values().any(|card| {
                    card.zone == ObjectZone::Battlefield
                        && card
                            .controller
                            .is_some_and(|owner| (owner != controller) == opponents)
                        && card.current_creature_subtypes.contains(kind)
                })
            }
            OpponentsTurn => self.window.active_player() != controller,
            ControllersTurn => self.window.active_player() == controller,
            HasKeyword(keyword) => source
                .and_then(|s| evaluated.get(s))
                .is_some_and(|o| o.has_keyword(*keyword)),
            Delirium => {
                let mut types = BTreeSet::new();
                for card in evaluated
                    .objects
                    .values()
                    .filter(|c| c.zone == ObjectZone::Graveyard(controller))
                {
                    types.extend(card.current_types.iter().copied());
                }
                types.len() >= 4
            }
            Metalcraft => {
                evaluated
                    .objects
                    .values()
                    .filter(|c| {
                        c.zone == ObjectZone::Battlefield
                            && c.controller == Some(controller)
                            && c.has_type(CardType::Artifact)
                    })
                    .count()
                    >= 3
            }
            Threshold => {
                evaluated
                    .objects
                    .values()
                    .filter(|c| c.zone == ObjectZone::Graveyard(controller))
                    .count()
                    >= 7
            }
            ControlsCardType { kind, minimum } => {
                evaluated
                    .objects
                    .values()
                    .filter(|c| {
                        c.zone == ObjectZone::Battlefield
                            && c.controller == Some(controller)
                            && c.has_type(*kind)
                    })
                    .count()
                    >= usize::from(*minimum)
            }
            GraveyardCountAtLeast { types, minimum } => {
                evaluated
                    .objects
                    .values()
                    .filter(|c| {
                        c.zone == ObjectZone::Graveyard(controller)
                            && types.matches_any(&c.current_types)
                    })
                    .count()
                    >= usize::from(*minimum)
            }
            HandCountAtMost(limit) => {
                evaluated
                    .objects
                    .values()
                    .filter(|c| c.zone == ObjectZone::Hand(controller))
                    .count()
                    <= usize::from(*limit)
            }
            HandCountAtLeast(limit) => {
                evaluated
                    .objects
                    .values()
                    .filter(|c| c.zone == ObjectZone::Hand(controller))
                    .count()
                    >= usize::from(*limit)
            }
            SourceCounterAtLeast { kind, minimum } => source.is_some_and(|s| {
                self.characteristics.permanent_counter_count(s, *kind) >= i64::from(*minimum).into()
            }),
            SourceAnyCounterAtLeast(minimum) => source.is_some_and(|s| {
                self.characteristics.total_permanent_counters(s) >= i64::from(*minimum).into()
            }),
            SourceIsEquipped => source.is_some_and(|s| {
                evaluated.objects.values().any(|equipment| {
                    equipment.zone == ObjectZone::Battlefield
                        && equipment.attached_to == Some(TargetRef::Object(s))
                        && equipment.has_type(CardType::Artifact)
                        && equipment
                            .current_artifact_subtypes
                            .contains(&crate::program::ArtifactSubtype::Equipment)
                })
            }),
            SourceIsEnchanted => source.is_some_and(|s| {
                evaluated.objects.values().any(|aura| {
                    aura.zone == ObjectZone::Battlefield
                        && aura.attached_to == Some(TargetRef::Object(s))
                        && aura.has_type(CardType::Enchantment)
                })
            }),
            AttachedHostMatches(selector) => source
                .and_then(|s| evaluated.get(s))
                .and_then(|src| match src.attached_to {
                    Some(TargetRef::Object(host)) => Some(host),
                    _ => None,
                })
                .is_some_and(|host| {
                    crate::characteristics::selector_matches_permanent(
                        self.window.cards(),
                        evaluated,
                        source.unwrap_or(ObjectId::SENTINEL),
                        controller,
                        selector,
                        host,
                        &self.name_rules,
                    )
                }),
            ControlsExactCount { selector, count } => {
                let mut sel = selector.as_ref().clone();
                if sel.controller == crate::characteristics::ControllerSelector::Any {
                    sel.controller = crate::characteristics::ControllerSelector::SourceController;
                }
                crate::characteristics::count_permanents(
                    self.window.cards(),
                    evaluated,
                    source.unwrap_or(ObjectId::SENTINEL),
                    controller,
                    &sel,
                    &self.name_rules,
                ) == i64::from(*count).into()
            }
            SourceIsTapped => source
                .and_then(|s| evaluated.get(s))
                .is_some_and(|o| o.tapped == Some(true)),
            SourceIsUntapped => source
                .and_then(|s| evaluated.get(s))
                .is_some_and(|o| o.tapped == Some(false)),
            SourceClassLevelAtLeast(min) => source.is_some_and(|s| {
                self.characteristics
                    .class_levels
                    .get(&s)
                    .copied()
                    .unwrap_or(1)
                    >= *min
            }),
            SourceIsSolved => {
                source.is_some_and(|s| self.characteristics.solved_cases.contains(&s))
            }
            SourceEnteredThisTurn => {
                source.is_some_and(|s| self.characteristics.entered_objects_this_turn.contains(&s))
            }
            SourcePaired => {
                source.is_some_and(|s| self.characteristics.paired_creatures.contains_key(&s))
            }
            SourceIsPrepared => source.is_some_and(|s| self.characteristics.prepared.contains(&s)),
            EntryOptionChosen(expected) => source.is_some_and(|s| {
                self.characteristics.entry_option_choices.get(&s) == Some(expected)
            }),
            NonlandPermanentsEnteredThisTurnAtLeast(min) => {
                let total = self
                    .characteristics
                    .permanents_entered_this_turn
                    .get(&controller)
                    .copied()
                    .unwrap_or(0);
                let lands = self
                    .characteristics
                    .entered_types_this_turn
                    .get(&controller)
                    .and_then(|by_type| by_type.get(&CardType::Land))
                    .copied()
                    .unwrap_or(0);
                total.saturating_sub(lands) >= *min
            }
            AnyLibraryAtMost(max) => self
                .window
                .participants()
                .iter()
                .any(|&seat| self.window.cards().library(seat).len() <= usize::from(*max)),
            AnyPlayerHandCountAtMost(max) => self.window.participants().iter().any(|&seat| {
                evaluated
                    .objects
                    .values()
                    .filter(|c| c.zone == ObjectZone::Hand(seat))
                    .count()
                    <= usize::from(*max)
            }),
            MostCommonPermanentColor { color, or_tied } => {
                let mut counts = [0usize; 5];
                let idx = |c: Color| match c {
                    Color::White => 0,
                    Color::Blue => 1,
                    Color::Black => 2,
                    Color::Red => 3,
                    Color::Green => 4,
                };
                for perm in evaluated
                    .objects
                    .values()
                    .filter(|p| p.zone == ObjectZone::Battlefield)
                {
                    for &c in &perm.current_colors {
                        counts[idx(c)] += 1;
                    }
                }
                let target = counts[idx(*color)];
                let max_other = counts
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| i != idx(*color))
                    .map(|(_, &n)| n)
                    .max()
                    .unwrap_or(0);
                if *or_tied {
                    target >= max_other
                } else {
                    target > max_other
                }
            }
            ControllerHasCityBlessing => self.characteristics.city_blessing.contains(&controller),
            ControllerGainedLifeThisTurn => self
                .characteristics
                .life_gained_this_turn
                .get(&controller)
                .is_some_and(RulesInteger::is_positive),
            ControllerLostLifeThisTurn => self
                .characteristics
                .life_lost_this_turn
                .get(&controller)
                .is_some_and(RulesInteger::is_positive),
            OpponentLostLifeThisTurn => self
                .characteristics
                .life_lost_this_turn
                .iter()
                .any(|(&seat, amount)| seat != controller && amount.is_positive()),
            ControllerAttackedThisTurn => self
                .characteristics
                .attacked_this_turn_players
                .contains(&controller),
            ControllerCastSpellsThisTurn(min) => {
                self.characteristics
                    .spells_cast_this_turn
                    .get(&controller)
                    .copied()
                    .unwrap_or(0)
                    >= *min
            }
            ControllerDiscardedThisTurn => {
                self.characteristics
                    .cards_discarded_this_turn
                    .get(&controller)
                    .copied()
                    .unwrap_or(0)
                    >= 1
            }
            ControllerSacrificedThisTurn => {
                self.characteristics
                    .permanents_sacrificed_this_turn
                    .get(&controller)
                    .copied()
                    .unwrap_or(0)
                    >= 1
            }
            PermanentLeftBattlefieldThisTurn => {
                self.characteristics
                    .permanents_left_battlefield_this_turn
                    .get(&controller)
                    .copied()
                    .unwrap_or(0)
                    >= 1
            }
            CreatureEnteredUnderControlThisTurn => {
                self.characteristics
                    .creatures_entered_this_turn
                    .get(&controller)
                    .copied()
                    .unwrap_or(0)
                    >= 1
            }
            DescendedThisTurn => self
                .characteristics
                .descended_this_turn
                .contains(&controller),
            CommittedCrimeThisTurn => self
                .characteristics
                .committed_crime_this_turn
                .contains(&controller),
            ControllerDrewCardsThisTurn(min) => {
                self.characteristics
                    .cards_drawn_this_turn_counts
                    .get(&controller)
                    .copied()
                    .unwrap_or(0)
                    >= *min
            }
            ControlsMatching { selector, minimum } => {
                let mut sel = selector.as_ref().clone();
                if sel.controller == crate::characteristics::ControllerSelector::Any {
                    sel.controller = crate::characteristics::ControllerSelector::SourceController;
                }
                crate::characteristics::count_permanents(
                    self.window.cards(),
                    evaluated,
                    source.unwrap_or(ObjectId::SENTINEL),
                    controller,
                    &sel,
                    &self.name_rules,
                ) >= i64::from(*minimum).into()
            }
            OpponentControlsMatching { selector, minimum } => {
                let mut sel = selector.as_ref().clone();
                if sel.controller == crate::characteristics::ControllerSelector::Any {
                    sel.controller = crate::characteristics::ControllerSelector::Opponents;
                }
                crate::characteristics::count_permanents(
                    self.window.cards(),
                    evaluated,
                    source.unwrap_or(ObjectId::SENTINEL),
                    controller,
                    &sel,
                    &self.name_rules,
                ) >= i64::from(*minimum).into()
            }
            Eval(rc) => {
                let ctx = WorldEvalCtx {
                    active_player: self.window.active_player(),
                    step: self.current_step,
                    participants: self.window.participants(),
                    life: Some(&self.life),
                    combat: self.combat.as_ref(),
                    cards: self.window.cards(),
                    programs: &self.programs,
                    state: &self.characteristics,
                    objects: &evaluated.objects,
                    source: source.unwrap_or(ObjectId::SENTINEL),
                    controller,
                    name_rules: &self.name_rules,
                    trigger_subject: None,
                    related_player,
                    defending_player: None,
                };
                eval_world_condition(&ctx, rc)
            }
            RoomDoorUnlocked(half) => source
                .and_then(|s| self.characteristics.unlocked_doors.get(&s))
                .is_some_and(|doors| doors.is_unlocked(*half)),
            RoomFullyUnlocked => source
                .and_then(|s| self.characteristics.unlocked_doors.get(&s))
                .is_some_and(|doors| doors.fully_unlocked()),
            Not(inner) => !self.eval_static_source_condition_at(
                evaluated,
                inner,
                source,
                controller,
                related_player,
            ),
            And(a, b) => {
                self.eval_static_source_condition_at(
                    evaluated,
                    a,
                    source,
                    controller,
                    related_player,
                ) && self.eval_static_source_condition_at(
                    evaluated,
                    b,
                    source,
                    controller,
                    related_player,
                )
            }
            Or(a, b) => {
                self.eval_static_source_condition_at(
                    evaluated,
                    a,
                    source,
                    controller,
                    related_player,
                ) || self.eval_static_source_condition_at(
                    evaluated,
                    b,
                    source,
                    controller,
                    related_player,
                )
            }
        }
    }

    /// Execute an inline replacement body without putting an object on the stack (CR 614.1),
    /// guarding `(rep.source, rep.ability)` against CR 614.5 self-reapplication.
    pub(super) fn execute_inline_replacement_body(
        &mut self,
        rep: &EventBodyReplacement,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.execute_inline_replacement_body_with_amount(rep, None, cause, append)
    }

    /// Execute an inline replacement body with an optional event amount (such as damage prevented),
    /// exposing it to `EventValueKind::DamagePreventedThisWay` and `EventValueKind::LastActionCount`.
    pub(super) fn execute_inline_replacement_body_with_amount(
        &mut self,
        rep: &EventBodyReplacement,
        event_amount: Option<RulesInteger>,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let key = (rep.source, rep.ability);
        if !self.active_event_replacements.insert(key) {
            return Ok(());
        }
        for active in &mut self.temporary_rules {
            if !active.consumed
                && active.source_object == rep.source
                && matches!(
                    &active.rule,
                    crate::program::TemporaryRuleKind::OneShotEventReplacement(ab)
                        if ab.id() == rep.ability
                )
            {
                active.consumed = true;
                break;
            }
        }
        let result = self.run_inline_replacement_body(rep, event_amount.as_ref(), cause, append);
        self.active_event_replacements.remove(&key);
        result
    }

    /// Check active `ReplaceableEvent::LoseGame` replacements (static, temporary, or one-shot)
    /// for `player`. Returns `Ok(true)` if the loss was replaced (and any inline replacement body
    /// executed), or `Ok(false)` if the player still loses.
    pub(super) fn try_replace_player_loss(
        &mut self,
        evaluated: &EvaluatedCharacteristics,
        player: SeatId,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        let opponents = self
            .combat_rules
            .opponents(self.window.participants(), player)?;
        let relation_applies = |rel: AffectedPlayers, ctrl: SeatId| match rel {
            AffectedPlayers::One(ResolvingPlayer::Controller) => player == ctrl,
            AffectedPlayers::EachOpponent | AffectedPlayers::EachOtherPlayer => {
                opponents.contains(&ctrl)
            }
            AffectedPlayers::EachPlayer | AffectedPlayers::One(_) => true,
        };
        struct LoseGameReplacementMatch {
            source: Option<ObjectId>,
            ability: StaticAbilityId,
            controller: SeatId,
            action: StaticReplacementAction,
            one_shot_idx: Option<usize>,
        }
        let mut found: Option<LoseGameReplacementMatch> = None;
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
                if let StaticContinuousAbility::EventReplacement {
                    id,
                    event: ReplaceableEvent::LoseGame { player: rel },
                    condition,
                    action,
                } = ab
                {
                    let key = (Some(perm.object), *id);
                    if self.active_event_replacements.contains(&key)
                        || !relation_applies(*rel, ctrl)
                    {
                        continue;
                    }
                    let cond_ok = condition.as_deref().is_none_or(|cond| {
                        self.eval_static_source_condition_at(
                            evaluated,
                            cond,
                            Some(perm.object),
                            ctrl,
                            Some(player),
                        )
                    });
                    if cond_ok {
                        found = Some(LoseGameReplacementMatch {
                            source: Some(perm.object),
                            ability: *id,
                            controller: ctrl,
                            action: action.clone(),
                            one_shot_idx: None,
                        });
                        break;
                    }
                }
            }
            if found.is_some() {
                break;
            }
        }
        if found.is_none() {
            for (idx, active) in self.temporary_rules.iter().enumerate() {
                let Some(ab) = self.active_temporary_static(active, evaluated) else {
                    continue;
                };
                if let StaticContinuousAbility::EventReplacement {
                    id,
                    event: ReplaceableEvent::LoseGame { player: rel },
                    condition,
                    action,
                } = ab
                {
                    let key = (active.source_object, *id);
                    if self.active_event_replacements.contains(&key)
                        || !relation_applies(*rel, active.controller)
                    {
                        continue;
                    }
                    let cond_ok = condition.as_deref().is_none_or(|cond| {
                        self.eval_static_source_condition_at(
                            evaluated,
                            cond,
                            active.source_object,
                            active.controller,
                            Some(player),
                        )
                    });
                    if cond_ok {
                        let one_shot_idx = matches!(
                            &active.rule,
                            crate::program::TemporaryRuleKind::OneShotEventReplacement(_)
                        )
                        .then_some(idx);
                        found = Some(LoseGameReplacementMatch {
                            source: active.source_object,
                            ability: *id,
                            controller: active.controller,
                            action: action.clone(),
                            one_shot_idx,
                        });
                        break;
                    }
                }
            }
        }
        let Some(LoseGameReplacementMatch {
            source,
            ability,
            controller,
            action,
            one_shot_idx,
        }) = found
        else {
            return Ok(false);
        };
        if let Some(idx) = one_shot_idx {
            self.temporary_rules[idx].consumed = true;
        }
        match action {
            StaticReplacementAction::Skip => Ok(true),
            StaticReplacementAction::ReplaceWithBody { body, .. } => {
                let rep = EventBodyReplacement {
                    source,
                    ability,
                    controller,
                    affected_player: player,
                    optional: false,
                    body: (*body).clone(),
                };
                self.execute_inline_replacement_body(&rep, cause, append)?;
                Ok(true)
            }
            StaticReplacementAction::ModifyAmount { .. } => Ok(false),
        }
    }

    fn resolve_inline_value_expr(
        &self,
        rep: &EventBodyReplacement,
        expr: &crate::program::ValueExpr,
        event_amount: Option<&RulesInteger>,
    ) -> Result<RulesInteger, EffectError> {
        use crate::program::{EventValueKind, ValueExpr};
        match expr {
            ValueExpr::EventValue(EventValueKind::DamagePreventedThisWay) => {
                Ok(event_amount.cloned().unwrap_or_else(|| 0.into()))
            }
            ValueExpr::EventValue(EventValueKind::LastActionCount) if event_amount.is_some() => {
                Ok(event_amount.cloned().unwrap_or_else(|| 0.into()))
            }
            ValueExpr::Scale { expr, factor } => Ok(self
                .resolve_inline_value_expr(rep, expr, event_amount)?
                .times(factor)),
            ValueExpr::DivConst {
                expr,
                divisor,
                round_up,
            } => {
                let value = self.resolve_inline_value_expr(rep, expr, event_amount)?;
                let d = usize::from(divisor.get());
                if d == 2 {
                    Ok(if *round_up {
                        value.nonnegative_half_rounded_up()
                    } else {
                        value.nonnegative_half_rounded_down()
                    })
                } else if let Some(n) = value.to_usize() {
                    let result = if *round_up { n.div_ceil(d) } else { n / d };
                    Ok(RulesInteger::from_usize(result))
                } else {
                    Err(EffectError::InconsistentProgram)
                }
            }
            ValueExpr::Add(a, b) => {
                let lhs = self.resolve_inline_value_expr(rep, a, event_amount)?;
                let rhs = self.resolve_inline_value_expr(rep, b, event_amount)?;
                Ok(lhs.plus(&rhs))
            }
            ValueExpr::Sub(a, b) => {
                let lhs = self.resolve_inline_value_expr(rep, a, event_amount)?;
                let rhs = self.resolve_inline_value_expr(rep, b, event_amount)?;
                Ok(lhs.minus(&rhs))
            }
            ValueExpr::Conditional {
                left,
                comparison,
                right,
                then_value,
                else_value,
            } => {
                let lhs = self.resolve_inline_value_expr(rep, left, event_amount)?;
                let rhs = self.resolve_inline_value_expr(rep, right, event_amount)?;
                if comparison.evaluate(&lhs, &rhs) {
                    self.resolve_inline_value_expr(rep, then_value, event_amount)
                } else {
                    self.resolve_inline_value_expr(rep, else_value, event_amount)
                }
            }
            _ => self.resolve_value_expr_with_source(rep.controller, expr, None, rep.source),
        }
    }

    fn resolve_replacement_player(
        &self,
        role: ResolvingPlayer,
        rep: &EventBodyReplacement,
    ) -> Result<SeatId, EffectError> {
        match role {
            ResolvingPlayer::Controller => Ok(rep.controller),
            ResolvingPlayer::ActivePlayer => Ok(self.window.active_player()),
            ResolvingPlayer::EnchantedPlayer => Ok(rep
                .source
                .and_then(|s| self.characteristics.attachments.get(&s).copied())
                .and_then(|att| match att {
                    TargetRef::Player(seat) => Some(seat),
                    _ => None,
                })
                .unwrap_or(rep.affected_player)),
            ResolvingPlayer::AttachedHostController => Ok(rep
                .source
                .and_then(|s| self.characteristics.attachments.get(&s).copied())
                .and_then(|att| match att {
                    TargetRef::Player(seat) => Some(seat),
                    TargetRef::Object(host) => self
                        .window
                        .cards()
                        .get(host)
                        .and_then(GameObject::controller),
                    TargetRef::Stack(_) => None,
                })
                .unwrap_or(rep.affected_player)),
            ResolvingPlayer::SourceChosenPlayer => Ok(rep
                .source
                .and_then(|s| self.characteristics.entry_player_choices.get(&s).copied())
                .unwrap_or(rep.affected_player)),
            ResolvingPlayer::UniqueRelatedPlayer
            | ResolvingPlayer::TargetPlayer
            | ResolvingPlayer::TargetController
            | ResolvingPlayer::TargetOwner
            | ResolvingPlayer::CombatDamageSourceController
            | ResolvingPlayer::TriggerSubjectController
            | ResolvingPlayer::TriggerSubjectOwner => Ok(rep.affected_player),
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
        event_amount: Option<&RulesInteger>,
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
                    let left =
                        self.resolve_inline_value_expr(rep, &condition.left, event_amount)?;
                    let right =
                        self.resolve_inline_value_expr(rep, &condition.right, event_amount)?;
                    if !condition.comparison.evaluate(&left, &right) {
                        pc = destination;
                        continue;
                    }
                }
                Instruction::GainControllerLife { amount } => {
                    let val = self
                        .resolve_inline_value_expr(rep, &amount, event_amount)?
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
                        .resolve_inline_value_expr(rep, &amount, event_amount)?
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
                Instruction::SetPlayersLife { players, amount } => {
                    let val = self.resolve_inline_value_expr(rep, &amount, event_amount)?;
                    let seats = self.resolve_replacement_players(players, rep)?;
                    let adj = LifeAdjustment::Set(val);
                    let map = seats.into_iter().map(|s| (s, adj.clone())).collect();
                    self.adjust_players_at_cut(synthetic_entry, synthetic_index, map, append)?;
                }
                Instruction::DrainOpponents(amount) => {
                    let val = self
                        .resolve_inline_value_expr(rep, &amount, event_amount)?
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
                        .resolve_inline_value_expr(rep, &amount, event_amount)?
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
                Instruction::DrawCards(spec) => {
                    let val = self
                        .resolve_inline_value_expr(rep, &spec.amount, event_amount)?
                        .max(0.into());
                    if val.is_positive() {
                        let seats: Vec<SeatId> = match spec.players {
                            crate::toolbox::PlayerSelector::Resolving(role)
                            | crate::toolbox::PlayerSelector::Group(AffectedPlayers::One(role)) => {
                                vec![self.resolve_replacement_player(role, rep)?]
                            }
                            crate::toolbox::PlayerSelector::Group(AffectedPlayers::EachPlayer) => {
                                self.window.apnap_players()?.collect()
                            }
                            crate::toolbox::PlayerSelector::Group(
                                AffectedPlayers::EachOpponent,
                            ) => self
                                .window
                                .apnap_players()?
                                .filter(|seat| *seat != rep.controller)
                                .collect(),
                            _ => Vec::new(),
                        };
                        for seat in seats {
                            if self.window.participants().contains(&seat) {
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
                }
                Instruction::MutateObject(spec) => {
                    let targets: Vec<ObjectId> = match &spec.source {
                        crate::toolbox::ObjectQuery::Source => rep.source.into_iter().collect(),
                        crate::toolbox::ObjectQuery::EffectSubject(subject) => {
                            self.inline_resolve_subject(*subject.as_ref(), rep)?
                        }
                        crate::toolbox::ObjectQuery::ZoneScan {
                            zone: crate::toolbox::QueryZone::Battlefield,
                            filter,
                            ..
                        } => {
                            let evaluated = self.evaluated_characteristics()?;
                            let excluded = BTreeSet::new();
                            evaluated
                                .objects
                                .values()
                                .filter(|p| {
                                    p.zone == ObjectZone::Battlefield
                                        && self.matches_object_filter(
                                            **filter,
                                            p,
                                            rep.controller,
                                            rep.source,
                                            &excluded,
                                            &evaluated,
                                        )
                                })
                                .map(|p| p.object)
                                .collect()
                        }
                        _ => Vec::new(),
                    };
                    match &spec.op {
                        crate::toolbox::ObjectMutation::AddCounters { kind, amount } => {
                            let val = self
                                .resolve_inline_value_expr(rep, amount, event_amount)?
                                .max(0.into());
                            if val.is_positive() {
                                self.inline_put_permanent_counters(
                                    &targets,
                                    *kind,
                                    &val,
                                    cause.clone(),
                                    append,
                                )?;
                            }
                        }
                        crate::toolbox::ObjectMutation::SetTapped(tapped) => {
                            let changing: Vec<ObjectId> = targets
                                .into_iter()
                                .filter(|id| {
                                    self.window.cards().get(*id).and_then(GameObject::tapped)
                                        == Some(!*tapped)
                                })
                                .collect();
                            if !changing.is_empty() {
                                let before = self.rules_snapshot(None)?;
                                let occurrences = if *tapped {
                                    let (window, changes) = self
                                        .window
                                        .record_inline_replacement_status(&changing, true)?;
                                    self.window = window;
                                    occurrence_flow::status_occurrences(
                                        changes,
                                        StatusReason::Effect,
                                    )
                                } else {
                                    let mut untapping = Vec::with_capacity(changing.len());
                                    let mut stun_occurrences = Vec::new();
                                    for id in changing {
                                        if self
                                            .counters_of_kind(id, CounterKind::Stun)
                                            .is_positive()
                                        {
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
                                        occurrence_flow::status_occurrences(
                                            changes,
                                            StatusReason::Effect,
                                        )
                                    };
                                    occs.extend(stun_occurrences);
                                    occs
                                };
                                self.emit_from(cause.clone(), &before, None, append, |_, _, _| {
                                    Ok(occurrences)
                                })?;
                            }
                        }
                        crate::toolbox::ObjectMutation::RemoveCounters {
                            kind: Some(kind),
                            amount,
                        } => {
                            let val = self
                                .resolve_inline_value_expr(rep, amount, event_amount)?
                                .max(0.into());
                            if val.is_positive() {
                                self.inline_remove_permanent_counters(
                                    &targets,
                                    *kind,
                                    Some(&val),
                                    cause.clone(),
                                    append,
                                )?;
                            }
                        }
                        crate::toolbox::ObjectMutation::ClearCounters { kind: Some(kind) } => {
                            self.inline_remove_permanent_counters(
                                &targets,
                                *kind,
                                None,
                                cause.clone(),
                                append,
                            )?;
                        }
                        _ => {}
                    }
                }
                Instruction::ApplyContinuous(spec) => {
                    if let crate::toolbox::ContinuousPayload::TemporaryRule(rule) = &spec.payload
                        && let crate::program::TemporaryRuleKind::TemporaryStatic(ability) =
                            rule.as_ref()
                        && matches!(
                            ability.as_ref(),
                            StaticContinuousAbility::DrawRule {
                                rule: crate::characteristics::StaticDrawRule::NoMaximumHandSize,
                                ..
                            }
                        )
                    {
                        self.characteristics
                            .no_max_hand_size_players
                            .insert(rep.controller);
                    }
                }
                Instruction::Mill { player, amount } => {
                    let seat = self.resolve_replacement_player(player, rep)?;
                    let val = self
                        .resolve_inline_value_expr(rep, &amount, event_amount)?
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
                Instruction::RemoveCountersFromPermanent {
                    target_self: true,
                    kind: Some(kind),
                    amount,
                } => {
                    if let Some(source) = rep.source {
                        let val = self
                            .resolve_inline_value_expr(rep, &amount, event_amount)?
                            .max(0.into());
                        if val.is_positive() {
                            self.inline_remove_permanent_counters(
                                &[source],
                                kind,
                                Some(&val),
                                cause.clone(),
                                append,
                            )?;
                        }
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
                        .resolve_inline_value_expr(rep, &amount, event_amount)?
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
                    self.inline_create_tokens(
                        &creations,
                        rep,
                        event_amount,
                        cause.clone(),
                        append,
                    )?;
                }
                Instruction::WinGame(player) => {
                    let winner = self.resolve_replacement_player(player, rep)?;
                    let evaluated = self.evaluated_characteristics()?;
                    if self.window.participants().contains(&winner)
                        && !self.player_cannot_win(&evaluated, winner)?
                    {
                        self.won_by_effect =
                            Some((winner, rep.source.unwrap_or(ObjectId::SENTINEL)));
                    }
                }
                Instruction::LoseGame(players) => {
                    let evaluated = self.evaluated_characteristics()?;
                    let mut losing = BTreeSet::new();
                    for seat in self.resolve_replacement_players(players, rep)? {
                        if self.window.participants().contains(&seat)
                            && !self.player_cannot_lose(&evaluated, seat, false)?
                        {
                            losing.insert(seat);
                        }
                    }
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
                Instruction::ZoneOp(spec) => {
                    let evaluated = self.evaluated_characteristics()?;
                    let excluded = BTreeSet::new();
                    let targets: Vec<ObjectId> = match &spec.source {
                        crate::toolbox::ObjectQuery::Source => rep
                            .source
                            .filter(|s| {
                                self.window
                                    .cards()
                                    .get(*s)
                                    .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                            })
                            .into_iter()
                            .collect(),
                        crate::toolbox::ObjectQuery::ZoneScan {
                            zone: crate::toolbox::QueryZone::Battlefield,
                            filter,
                            ..
                        } => evaluated
                            .objects
                            .values()
                            .filter(|p| {
                                p.zone == ObjectZone::Battlefield
                                    && self.matches_object_filter(
                                        **filter,
                                        p,
                                        rep.controller,
                                        rep.source,
                                        &excluded,
                                        &evaluated,
                                    )
                            })
                            .map(|p| p.object)
                            .collect(),
                        crate::toolbox::ObjectQuery::ZoneScan {
                            zone: crate::toolbox::QueryZone::Library(_),
                            ..
                        } => self
                            .window
                            .cards()
                            .library(rep.controller)
                            .first()
                            .copied()
                            .into_iter()
                            .collect(),
                        _ => Vec::new(),
                    };
                    match &spec.destination {
                        crate::toolbox::ZoneDest::Exile {
                            face_down,
                            link_to_source,
                            ..
                        } => {
                            for id in targets {
                                if let Some(moved) = self.inline_move_object(
                                    id,
                                    ObjectDestination::Exile,
                                    ZoneReason::ExiledByEffect,
                                    cause.clone(),
                                    append,
                                )? {
                                    if *face_down {
                                        self.face_down_exile.insert(moved, rep.controller);
                                    }
                                    if *link_to_source && let Some(src) = rep.source {
                                        self.linked_exiles.entry(src).or_default().push(moved);
                                    }
                                }
                            }
                        }
                        crate::toolbox::ZoneDest::Graveyard { semantics } => {
                            let is_destroy = matches!(
                                semantics,
                                crate::toolbox::GraveyardSemantics::Destroy { .. }
                            );
                            let reason = match semantics {
                                crate::toolbox::GraveyardSemantics::Destroy { .. } => {
                                    ZoneReason::Destroyed
                                }
                                crate::toolbox::GraveyardSemantics::Sacrifice => {
                                    ZoneReason::Sacrifice {
                                        player: rep.controller,
                                    }
                                }
                                _ => ZoneReason::PutLibraryCohortIntoGraveyard,
                            };
                            for id in targets {
                                if is_destroy
                                    && evaluated.get(id).is_some_and(|p| {
                                        p.has_keyword(KeywordAbility::Indestructible)
                                    })
                                {
                                    continue;
                                }
                                let _ = self.inline_move_object(
                                    id,
                                    ObjectDestination::Graveyard,
                                    reason,
                                    cause.clone(),
                                    append,
                                )?;
                            }
                        }
                        crate::toolbox::ZoneDest::Hand => {
                            for id in targets {
                                let _ = self.inline_move_object(
                                    id,
                                    ObjectDestination::Hand,
                                    ZoneReason::ReturnToOwnersHand,
                                    cause.clone(),
                                    append,
                                )?;
                            }
                        }
                        crate::toolbox::ZoneDest::Library { position } => {
                            let (dest, pl) = match position {
                                crate::toolbox::LibraryOrderPos::Top => {
                                    (ObjectDestination::LibraryTop, LibraryPlacement::Top)
                                }
                                crate::toolbox::LibraryOrderPos::Bottom
                                | crate::toolbox::LibraryOrderPos::BottomRandom
                                | crate::toolbox::LibraryOrderPos::ShuffleInto => {
                                    (ObjectDestination::LibraryBottom, LibraryPlacement::Bottom)
                                }
                                crate::toolbox::LibraryOrderPos::NthFromTop(expr) => {
                                    let n = self
                                        .resolve_value_expr_with_source(
                                            rep.controller,
                                            expr,
                                            None,
                                            rep.source,
                                        )?
                                        .to_usize()
                                        .unwrap_or(0);
                                    let off = u8::try_from(n).unwrap_or(u8::MAX);
                                    (
                                        ObjectDestination::LibraryFromTop(off),
                                        LibraryPlacement::FromTop(off),
                                    )
                                }
                                crate::toolbox::LibraryOrderPos::ChoiceTopOrBottom { .. } => {
                                    return Err(EffectError::InconsistentProgram);
                                }
                            };
                            for id in targets {
                                let _ = self.inline_move_object(
                                    id,
                                    dest,
                                    ZoneReason::PutIntoOwnersLibrary(pl),
                                    cause.clone(),
                                    append,
                                )?;
                            }
                        }
                        _ => {}
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

    fn inline_remove_permanent_counters(
        &mut self,
        objects: &[ObjectId],
        kind: CounterKind,
        amount: Option<&RulesInteger>,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if objects.is_empty() || amount.is_some_and(|a| !a.is_positive()) {
            return Ok(());
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
            let current_count = self.counters_of_kind(object, kind);
            if !current_count.is_positive() {
                continue;
            }
            let to_remove = match amount {
                Some(cap) if cap < &current_count => cap.clone(),
                _ => current_count.clone(),
            };
            if !to_remove.is_positive() {
                continue;
            }
            match kind {
                CounterKind::PlusOne | CounterKind::MinusOne => {
                    let prev = self.characteristics.counters(object);
                    let delta = PowerToughnessCounters {
                        plus_one: if kind == CounterKind::PlusOne {
                            to_remove.clone()
                        } else {
                            0.into()
                        },
                        minus_one: if kind == CounterKind::MinusOne {
                            to_remove.clone()
                        } else {
                            0.into()
                        },
                    };
                    let after = self
                        .characteristics
                        .take_power_toughness_counters(object, &delta)
                        .map_err(|_| EffectError::InconsistentProgram)?;
                    events.push(RulesOccurrence::PermanentCounterChanged {
                        object,
                        kind,
                        before: if kind == CounterKind::PlusOne {
                            prev.plus_one
                        } else {
                            prev.minus_one
                        },
                        after: if kind == CounterKind::PlusOne {
                            after.plus_one
                        } else {
                            after.minus_one
                        },
                    });
                }
                other => {
                    let after = current_count.minus(&to_remove);
                    if let Some(map) = self.characteristics.named_permanent_counters_mut(other) {
                        if after.is_positive() {
                            map.insert(object, after.clone());
                        } else {
                            map.remove(&object);
                            if other.keyword_counter_ability().is_some() {
                                self.characteristics
                                    .clear_keyword_counter_grant(object, other);
                            }
                        }
                        events.push(match other {
                            CounterKind::Loyalty => RulesOccurrence::LoyaltyCountersChanged {
                                object,
                                before: current_count,
                                after,
                            },
                            CounterKind::Charge => RulesOccurrence::ChargeCountersChanged {
                                object,
                                before: current_count,
                                after,
                            },
                            CounterKind::Suspect => RulesOccurrence::SuspectCountersChanged {
                                object,
                                before: current_count,
                                after,
                            },
                            _ => RulesOccurrence::PermanentCounterChanged {
                                object,
                                kind: other,
                                before: current_count,
                                after,
                            },
                        });
                    }
                }
            }
        }
        if !events.is_empty() {
            self.emit_from(cause, &before, None, append, |_, _, _| Ok(events))?;
        }
        Ok(())
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
        if self.player_counter_prevented(&before.view().evaluated, player, kind)? {
            return Ok(());
        }
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
        event_amount: Option<&RulesInteger>,
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
                .resolve_inline_value_expr(rep, &creation.amount, event_amount)?
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
                if adjusted > 0 {
                    for (bonus_prog, bonus_count) in
                        self.bonus_tokens_for_creation(&before.view().evaluated, creator, adjusted)?
                    {
                        for _ in 0..bonus_count {
                            inputs.push((creator, bonus_prog.clone()));
                            tapped_flags.push(false);
                        }
                    }
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
