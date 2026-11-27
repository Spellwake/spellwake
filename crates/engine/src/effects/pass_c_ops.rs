//! Execution handlers for Pass C orthogonal Comprehensive Rules instructions:
//! RollDie, FlipCoins, Vote, Incubate,
//! Connive / ConniveFinalize, Discover, Clash, RingTemptsYou,
//! VentureIntoDungeon, TakeInitiative, UnlockDoor, Behold, and ManifestDread.
use super::progress_rules::{RuleStepContext, StepOutcome};
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
use crate::program::{
    AffectedPlayers, DieRollMode, LibraryPlacement, MoveSubjectRef, PlayerTriggerAction,
    ResolvingPlayer, SplitHalf, SubjectDestination,
};

impl EffectWindow {
    pub(super) fn resolve_pass_c_move_subject(
        &self,
        entry: &crate::stack_window::StackEntry,
        subject: MoveSubjectRef,
    ) -> Result<Option<ObjectId>, EffectError> {
        Ok(match subject {
            MoveSubjectRef::TargetObject => self.legal_permanent_targets()?.into_iter().next(),
            MoveSubjectRef::AbilitySource => match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    Some(source.id())
                }
                StackKind::CardSpell(id) => Some(*id),
                _ => None,
            },
            MoveSubjectRef::TriggeringObject => self
                .execution_bindings(entry.id())
                .and_then(|b| b.trigger.as_ref())
                .and_then(|t| t.causes.members().first())
                .and_then(|cause| cause.subject_object_id()),
            MoveSubjectRef::AttachedHost => match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    match self.attachment_host_for_ability(source.id()) {
                        Some(TargetRef::Object(host)) => Some(host),
                        _ => None,
                    }
                }
                _ => None,
            },
            MoveSubjectRef::RevealedCard => self
                .resolution_cursor()
                .and_then(|c| c.results.revealed_top_card)
                .or_else(|| {
                    self.window
                        .cards()
                        .library(entry.controller())
                        .first()
                        .copied()
                }),
            MoveSubjectRef::CreatedTokens => self
                .resolution_cursor()
                .and_then(|c| c.last_created_tokens.first().copied()),
            MoveSubjectRef::LastMoved => self.resolution_cursor().and_then(|c| {
                c.results
                    .last_moved_objects
                    .first()
                    .copied()
                    .or(c.results.returned_target_permanent)
            }),
            MoveSubjectRef::RememberedFirst => self.legal_linked_targets()?.map(|(a, _)| a),
            MoveSubjectRef::RememberedSecond => self.legal_linked_targets()?.map(|(_, b)| b),
        })
    }

    pub(super) fn splice_replacing_current(
        &mut self,
        entry_id: StackEntryId,
        index: usize,
        ops: &[Instruction],
    ) -> Result<(), EffectError> {
        let inserted = ops.len();
        let base = isize::try_from(index).map_err(|_| EffectError::InconsistentProgram)?;
        let mut spliced = Vec::with_capacity(inserted);
        for op in ops {
            let mut cloned = op.clone();
            cloned.shift_jumps(base, 0);
            spliced.push(cloned);
        }
        let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry_id) else {
            return Err(EffectError::InconsistentProgram);
        };
        let delta = isize::try_from(inserted).map_err(|_| EffectError::InconsistentProgram)? - 1;
        for op in &mut payload.body.instructions {
            op.shift_jumps(delta, index + 1);
        }
        payload.body.instructions.splice(index..=index, spliced);
        for mode in &mut payload.modes {
            mode.shift_for_splice(index, inserted);
        }
        Ok(())
    }

    fn splice_before_current(
        &mut self,
        entry_id: StackEntryId,
        index: usize,
        ops: &[Instruction],
    ) -> Result<(), EffectError> {
        if ops.is_empty() {
            return Ok(());
        }
        let inserted = ops.len();
        let base = isize::try_from(index).map_err(|_| EffectError::InconsistentProgram)?;
        let mut spliced = Vec::with_capacity(inserted);
        for op in ops {
            let mut cloned = op.clone();
            cloned.shift_jumps(base, 0);
            spliced.push(cloned);
        }
        let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry_id) else {
            return Err(EffectError::InconsistentProgram);
        };
        let delta = isize::try_from(inserted).map_err(|_| EffectError::InconsistentProgram)?;
        for op in &mut payload.body.instructions {
            op.shift_jumps(delta, index);
        }
        payload.body.instructions.splice(index..index, spliced);
        for mode in &mut payload.modes {
            mode.shift_for_splice(index, inserted + 1);
        }
        Ok(())
    }

    pub(super) fn suspend_for_random_resolution(
        &mut self,
        chooser: SeatId,
    ) -> Result<(), EffectError> {
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
            rebound_or_buyback: false,
            cast: None,
            cursor,
            chooser,
            options: BTreeSet::new(),
        })));
        Ok(())
    }

    fn emit_keyword_action_event(
        &mut self,
        entry_id: StackEntryId,
        index: usize,
        player: SeatId,
        action: PlayerTriggerAction,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        self.emit_from(
            EventCause::Instruction {
                entry: entry_id,
                instruction: index,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::KeywordActionPerformed {
                    player,
                    action,
                }])
            },
        )
    }

    fn option_frame(
        &self,
        entry_id: StackEntryId,
        index: usize,
        chooser: SeatId,
        labels: Vec<String>,
    ) -> Result<EffectDecision, EffectError> {
        let count = u8::try_from(labels.len()).map_err(|_| EffectError::InconsistentProgram)?;
        Ok(EffectDecision::OptionIndex(OptionIndexFrame {
            revision: self.revision,
            entry: entry_id,
            instruction: index,
            chooser,
            count,
            labels,
        }))
    }

    fn object_display_label(&self, id: ObjectId, fallback: &str) -> String {
        self.copiable_program(id)
            .and_then(|p| p.names.iter().next())
            .map(|n| n.as_str().to_owned())
            .unwrap_or_else(|| fallback.to_owned())
    }

    #[inline(never)]
    pub(super) fn progress_pass_c_instruction(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        match instruction {
            Instruction::MutateObject(spec) => match &spec.op {
                crate::toolbox::ObjectMutation::BecomeMonstrous { amount } => {
                    let source_id = match entry.kind() {
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => Some(source.id()),
                        _ => None,
                    };
                    if let Some(id) = source_id
                        && self
                            .window
                            .cards()
                            .get(id)
                            .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                        && !self.characteristics.monstrous.contains(&id)
                    {
                        let count = self.resolve_value_expr(entry.controller(), amount)?;
                        let before = self.rules_snapshot(None)?;
                        if count.is_positive() {
                            let added = super::permanent_counters::effect_counter_multiplier(
                                &before.view().evaluated,
                                id,
                            )
                            .times(&count);
                            let delta = crate::characteristics::PowerToughnessCounters {
                                plus_one: added,
                                minus_one: 0.into(),
                            };
                            let _ = self.characteristics.place_counters(id, &delta);
                        }
                        self.characteristics.monstrous.insert(id);
                        self.emit_from(
                            crate::occurrence::EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            &before,
                            None,
                            ctx.append,
                            |_, _, _| {
                                Ok(vec![crate::occurrence::RulesOccurrence::BecameMonstrous {
                                    object: id,
                                    count: count.clone(),
                                }])
                            },
                        )?;
                    }
                    self.advance_instruction()?;
                    Ok(StepOutcome::Continue)
                }
                crate::toolbox::ObjectMutation::UnlockDoor { door, allow_lock } => {
                    let resolved = self
                        .eval_object_query_at(
                            entry.controller(),
                            Some(entry),
                            &spec.source,
                            self.resolution_cursor(),
                        )?
                        .into_iter()
                        .next();
                    self.execute_unlock_door_resolved(
                        ctx,
                        entry,
                        index,
                        resolved,
                        *door,
                        *allow_lock,
                    )
                }
                crate::toolbox::ObjectMutation::Enlist => {
                    let source_id = match entry.kind() {
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => Some(source.id()),
                        _ => None,
                    };
                    if let Some(src_id) = source_id
                        && self
                            .window
                            .cards()
                            .get(src_id)
                            .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                    {
                        let controller = entry.controller();
                        let evaluated = self.evaluated_characteristics()?;
                        let candidates: Vec<ObjectId> = evaluated
                            .objects
                            .values()
                            .filter(|o| {
                                o.object != src_id
                                    && o.zone == ObjectZone::Battlefield
                                    && o.controller == Some(controller)
                                    && o.is_creature()
                                    && o.tapped == Some(false)
                                    && !o.attacking
                            })
                            .map(|o| o.object)
                            .collect();
                        if !candidates.is_empty() {
                            let chosen = self
                                .resolution_cursor_mut()
                                .and_then(|c| c.chosen_option_index.take());
                            let Some(idx) = chosen else {
                                let mut labels = vec!["Decline to enlist".to_owned()];
                                for (i, id) in
                                    candidates.iter().enumerate().take(usize::from(u8::MAX) - 1)
                                {
                                    labels.push(format!(
                                        "Enlist {}",
                                        self.object_display_label(
                                            *id,
                                            &format!("Creature {}", i + 1)
                                        )
                                    ));
                                }
                                return Ok(StepOutcome::Pending(self.option_frame(
                                    entry.id(),
                                    index,
                                    controller,
                                    labels,
                                )?));
                            };
                            if idx > 0
                                && let Some(&enlisted_id) = candidates.get(usize::from(idx - 1))
                            {
                                let power = evaluated
                                    .get(enlisted_id)
                                    .and_then(|o| o.power_toughness.as_ref())
                                    .map(|pt| pt.derived.power.clone())
                                    .unwrap_or_else(|| 0.into());
                                let before = self.rules_snapshot(None)?;
                                let (updated, changes) =
                                    self.window.tap_for_effect(entry.id(), &[enlisted_id])?;
                                self.window = updated;
                                let mut tap_occs = occurrence_flow::status_occurrences(
                                    changes,
                                    crate::occurrence::StatusReason::Effect,
                                );
                                let effect = self.characteristics.add_effect(
                                    entry.id(),
                                    src_id,
                                    crate::characteristics::ResolvedParts(vec![
                                        crate::characteristics::ResolvedPart::ModifyPT(
                                            crate::characteristics::PowerToughness {
                                                power,
                                                toughness: 0.into(),
                                            },
                                        ),
                                    ]),
                                    crate::characteristics::ResolvedDuration::UntilEndOfTurn,
                                )?;
                                self.emit_from(
                                    EventCause::Instruction {
                                        entry: entry.id(),
                                        instruction: index,
                                    },
                                    &before,
                                    None,
                                    ctx.append,
                                    |old, new, _| {
                                        let appearances = characteristic_appearance_changes(
                                            old,
                                            new,
                                            effect.affected.iter().copied(),
                                        )?;
                                        tap_occs.push(RulesOccurrence::ContinuousEffectCreated {
                                            effect,
                                            appearances,
                                        });
                                        Ok(tap_occs)
                                    },
                                )?;
                            }
                        }
                    }
                    self.advance_instruction()?;
                    Ok(StepOutcome::Continue)
                }
                crate::toolbox::ObjectMutation::Connive { count } => {
                    let amount = self.resolve_value_expr(entry.controller(), count)?;
                    let subject = spec
                        .source
                        .to_move_subject()
                        .ok_or(EffectError::InconsistentProgram)?;
                    let obj_opt = self.resolve_pass_c_move_subject(entry, subject)?;
                    let Some(obj_id) = obj_opt else {
                        self.advance_instruction()?;
                        return Ok(StepOutcome::Continue);
                    };
                    if !amount.is_positive() {
                        self.advance_instruction()?;
                        return Ok(StepOutcome::Continue);
                    }
                    let subj_ctrl = self
                        .evaluated_characteristics()?
                        .get(obj_id)
                        .and_then(|c| c.controller)
                        .unwrap_or_else(|| entry.controller());
                    if let Some(cur) = self.resolution_cursor_mut() {
                        cur.results.last_nonland_discarded = 0;
                        cur.chosen_opponent = Some(subj_ctrl);
                    }
                    self.emit_keyword_action_event(
                        entry.id(),
                        index,
                        subj_ctrl,
                        PlayerTriggerAction::Connived,
                        ctx.append,
                    )?;
                    let draw_op = if subj_ctrl == entry.controller() {
                        crate::program::Instruction::DrawCards(Box::new(
                            crate::toolbox::DrawCardsSpec::simple(
                                crate::toolbox::PlayerSelector::Resolving(
                                    crate::program::ResolvingPlayer::Controller,
                                ),
                                ValueExpr::Literal(amount.clone()),
                            ),
                        ))
                    } else {
                        Instruction::DrawCards(Box::new(crate::toolbox::DrawCardsSpec {
                            players: crate::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::ChosenPlayer,
                            ),
                            amount: ValueExpr::Literal(amount.clone()),
                            bind_drawn: None,
                        }))
                    };
                    let discard_role = if subj_ctrl == entry.controller() {
                        ResolvingPlayer::Controller
                    } else {
                        ResolvingPlayer::ChosenPlayer
                    };
                    let ops = vec![
                        draw_op,
                        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::discard(
                            AffectedPlayers::One(discard_role),
                            ValueExpr::Literal(amount),
                        ))),
                        Instruction::MutateObject(Box::new(
                            crate::toolbox::MutateObjectSpec::connive_finalize(spec.source.clone()),
                        )),
                    ];
                    self.splice_replacing_current(entry.id(), index, &ops)?;
                    Ok(StepOutcome::Continue)
                }
                crate::toolbox::ObjectMutation::ConniveFinalize => {
                    let subject = spec
                        .source
                        .to_move_subject()
                        .ok_or(EffectError::InconsistentProgram)?;
                    let nonlands = self
                        .resolution_cursor()
                        .map_or(0, |c| c.results.last_nonland_discarded);
                    if nonlands > 0
                        && let Some(obj_id) = self.resolve_pass_c_move_subject(entry, subject)?
                        && self
                            .window
                            .cards()
                            .get(obj_id)
                            .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                    {
                        self.place_plus_one_counter_amount(
                            vec![obj_id],
                            i64::from(nonlands).into(),
                            EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            ctx.append,
                        )?;
                    }
                    self.advance_instruction()?;
                    Ok(StepOutcome::Continue)
                }
                _ => Ok(StepOutcome::Next(Instruction::MutateObject(spec))),
            },
            Instruction::MutatePlayer(spec) => {
                let player = spec
                    .resolving_player()
                    .unwrap_or(crate::program::ResolvingPlayer::Controller);
                match &spec.op {
                    crate::toolbox::PlayerMutation::Vote { options, secret: _ } => {
                        let initialized = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .results
                            .voters_initialized;
                        if !initialized {
                            let mut voters: Vec<SeatId> = self.window.apnap_players()?.collect();
                            if let Some(pos) = voters.iter().position(|p| *p == entry.controller())
                            {
                                voters.rotate_left(pos);
                            }
                            if let Some(cur) = self.resolution_cursor_mut() {
                                cur.results.pending_voters = voters.into();
                                cur.results.vote_counts = vec![0; options.len()];
                                cur.results.winning_vote_option = None;
                                cur.results.voters_initialized = true;
                            }
                        }
                        let next_voter = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .results
                            .pending_voters
                            .front()
                            .copied();
                        if let Some(voter) = next_voter {
                            let chosen = self
                                .resolution_cursor_mut()
                                .and_then(|c| c.chosen_option_index.take());
                            let Some(idx) = chosen else {
                                return Ok(StepOutcome::Pending(self.option_frame(
                                    entry.id(),
                                    index,
                                    voter,
                                    options.clone(),
                                )?));
                            };
                            if let Some(cur) = self.resolution_cursor_mut() {
                                cur.results.pending_voters.pop_front();
                                if let Some(slot) =
                                    cur.results.vote_counts.get_mut(usize::from(idx))
                                {
                                    *slot = slot.saturating_add(1);
                                }
                            }
                            return Ok(StepOutcome::Continue);
                        }
                        let counts = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .results
                            .vote_counts
                            .clone();
                        let max_votes = counts.iter().copied().max().unwrap_or(0);
                        let winners: Vec<usize> = counts
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| **c == max_votes && max_votes > 0)
                            .map(|(i, _)| i)
                            .collect();
                        let winning = if winners.len() == 1 {
                            u8::try_from(winners[0]).ok()
                        } else {
                            None
                        };
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.results.winning_vote_option = winning;
                            cur.results.voters_initialized = false;
                        }
                        let before = self.rules_snapshot(None)?;
                        self.emit_from(
                            EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            &before,
                            None,
                            ctx.append,
                            |_, _, _| {
                                Ok(vec![RulesOccurrence::VotesCast {
                                    options: options.clone(),
                                    counts: counts.clone(),
                                }])
                            },
                        )?;
                        self.advance_instruction()?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::Incubate { count } => {
                        let amount = self.resolve_value_expr(entry.controller(), count)?;
                        let creation = crate::dungeon::incubator_token_creation(
                            self.creature_vocabulary(),
                            amount,
                        )
                        .ok_or(EffectError::InconsistentProgram)?;
                        self.begin_token_entries(&[creation], None, false)?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::Clash => {
                        let chooser = player;
                        let cursor_snap = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .clone();
                        let actor =
                            self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                        let existing = cursor_snap.results.clash_state.clone();
                        match existing {
                            None => {
                                let opponents = self
                                    .combat_rules
                                    .opponents(self.window.participants(), actor)?;
                                let opp = match opponents.as_slice() {
                                    [] => {
                                        if let Some(cur) = self.resolution_cursor_mut() {
                                            cur.results.last_action_succeeded = false;
                                        }
                                        self.advance_instruction()?;
                                        return Ok(StepOutcome::Continue);
                                    }
                                    [only] => *only,
                                    _ => {
                                        let chosen = self
                                            .resolution_cursor_mut()
                                            .and_then(|c| c.chosen_option_index.take());
                                        let Some(idx) = chosen else {
                                            let labels = (0..opponents.len())
                                                .map(|i| format!("Opponent {}", i + 1))
                                                .collect();
                                            return Ok(StepOutcome::Pending(self.option_frame(
                                                entry.id(),
                                                index,
                                                actor,
                                                labels,
                                            )?));
                                        };
                                        *opponents
                                            .get(usize::from(idx))
                                            .ok_or(EffectError::InconsistentProgram)?
                                    }
                                };
                                let chooser_card =
                                    self.window.cards().library(actor).first().copied();
                                let opponent_card =
                                    self.window.cards().library(opp).first().copied();
                                for id in [chooser_card, opponent_card].into_iter().flatten() {
                                    if let Some(object) = self.window.cards().get(id).cloned() {
                                        let before = self.rules_snapshot(None)?;
                                        self.emit_from(
                                            EventCause::Instruction {
                                                entry: entry.id(),
                                                instruction: index,
                                            },
                                            &before,
                                            None,
                                            ctx.append,
                                            |_, _, _| {
                                                Ok(vec![RulesOccurrence::LibraryCardRevealed {
                                                    object: object.clone(),
                                                }])
                                            },
                                        )?;
                                    }
                                }
                                if let Some(cur) = self.resolution_cursor_mut() {
                                    cur.results.clash_state = Some(ClashState {
                                        chooser: actor,
                                        opponent: opp,
                                        chooser_card,
                                        opponent_card,
                                        stage: 1,
                                    });
                                }
                                Ok(StepOutcome::Continue)
                            }
                            Some(mut state) if state.stage == 1 => {
                                if let Some(card) = state.chooser_card {
                                    let chosen = self
                                        .resolution_cursor_mut()
                                        .and_then(|c| c.chosen_option_index.take());
                                    let Some(idx) = chosen else {
                                        let labels = vec![
                                            "Leave on top of library".to_owned(),
                                            "Put on bottom of library".to_owned(),
                                        ];
                                        return Ok(StepOutcome::Pending(self.option_frame(
                                            entry.id(),
                                            index,
                                            state.chooser,
                                            labels,
                                        )?));
                                    };
                                    if idx == 1 {
                                        let _ = self.move_objects_to_subject_destination(
                                            entry,
                                            index,
                                            state.chooser,
                                            &[card],
                                            SubjectDestination::Library(LibraryPlacement::Bottom),
                                            ctx.append,
                                        )?;
                                    }
                                }
                                state.stage = 2;
                                if let Some(cur) = self.resolution_cursor_mut() {
                                    cur.results.clash_state = Some(state);
                                }
                                Ok(StepOutcome::Continue)
                            }
                            Some(mut state) if state.stage == 2 => {
                                if let Some(card) = state.opponent_card {
                                    let chosen = self
                                        .resolution_cursor_mut()
                                        .and_then(|c| c.chosen_option_index.take());
                                    let Some(idx) = chosen else {
                                        let labels = vec![
                                            "Leave on top of library".to_owned(),
                                            "Put on bottom of library".to_owned(),
                                        ];
                                        return Ok(StepOutcome::Pending(self.option_frame(
                                            entry.id(),
                                            index,
                                            state.opponent,
                                            labels,
                                        )?));
                                    };
                                    if idx == 1 {
                                        let _ = self.move_objects_to_subject_destination(
                                            entry,
                                            index,
                                            state.opponent,
                                            &[card],
                                            SubjectDestination::Library(LibraryPlacement::Bottom),
                                            ctx.append,
                                        )?;
                                    }
                                }
                                state.stage = 3;
                                if let Some(cur) = self.resolution_cursor_mut() {
                                    cur.results.clash_state = Some(state);
                                }
                                Ok(StepOutcome::Continue)
                            }
                            Some(state) => {
                                let c_mv = state
                                    .chooser_card
                                    .and_then(|id| self.copiable_program(id))
                                    .map(Program::mana_value);
                                let o_mv = state
                                    .opponent_card
                                    .and_then(|id| self.copiable_program(id))
                                    .map(Program::mana_value);
                                let chooser_won = match (&c_mv, &o_mv) {
                                    (Some(c), Some(o)) => c > o,
                                    (Some(_), None) => true,
                                    _ => false,
                                };
                                let opponent_won = match (&c_mv, &o_mv) {
                                    (Some(c), Some(o)) => o > c,
                                    (None, Some(_)) => true,
                                    _ => false,
                                };
                                if let Some(cur) = self.resolution_cursor_mut() {
                                    cur.results.last_action_succeeded = chooser_won;
                                    cur.results.clash_state = None;
                                }
                                self.emit_keyword_action_event(
                                    entry.id(),
                                    index,
                                    state.chooser,
                                    PlayerTriggerAction::Clashed,
                                    ctx.append,
                                )?;
                                if chooser_won {
                                    self.emit_keyword_action_event(
                                        entry.id(),
                                        index,
                                        state.chooser,
                                        PlayerTriggerAction::WonClash,
                                        ctx.append,
                                    )?;
                                } else if opponent_won {
                                    self.emit_keyword_action_event(
                                        entry.id(),
                                        index,
                                        state.opponent,
                                        PlayerTriggerAction::WonClash,
                                        ctx.append,
                                    )?;
                                }
                                self.advance_instruction()?;
                                Ok(StepOutcome::Continue)
                            }
                        }
                    }
                    crate::toolbox::PlayerMutation::RingTemptsYou => {
                        let controller = entry.controller();
                        let evaluated = self.evaluated_characteristics()?;
                        let creatures: Vec<ObjectId> = evaluated
                            .objects
                            .values()
                            .filter(|o| {
                                o.zone == ObjectZone::Battlefield
                                    && o.controller == Some(controller)
                                    && o.is_creature()
                            })
                            .map(|o| o.object)
                            .collect();
                        let chosen_bearer = match creatures.as_slice() {
                            [] => None,
                            [only] => Some(*only),
                            _ => {
                                let chosen = self
                                    .resolution_cursor_mut()
                                    .and_then(|c| c.chosen_option_index.take());
                                let Some(idx) = chosen else {
                                    let labels = creatures
                                        .iter()
                                        .enumerate()
                                        .map(|(i, id)| {
                                            self.object_display_label(
                                                *id,
                                                &format!("Creature {}", i + 1),
                                            )
                                        })
                                        .collect();
                                    return Ok(StepOutcome::Pending(self.option_frame(
                                        entry.id(),
                                        index,
                                        controller,
                                        labels,
                                    )?));
                                };
                                creatures.get(usize::from(idx)).copied()
                            }
                        };
                        let level = self
                            .characteristics
                            .ring_level
                            .entry(controller)
                            .or_insert(0);
                        *level = level.saturating_add(1).min(4);
                        if let Some(bearer) = chosen_bearer {
                            self.characteristics.ring_bearer.insert(controller, bearer);
                        }
                        self.emit_keyword_action_event(
                            entry.id(),
                            index,
                            controller,
                            PlayerTriggerAction::RingTemptsYou,
                            ctx.append,
                        )?;
                        self.advance_instruction()?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::VentureIntoDungeon { undercity } => {
                        let undercity = *undercity;
                        let cursor_snap = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .clone();
                        let actor =
                            self.resolving_player_identity_at(player, Some(&cursor_snap))?;
                        let active = self
                            .characteristics
                            .dungeons
                            .get(&actor)
                            .and_then(|p| p.active);
                        let (dungeon, next_room) = match active {
                            None => {
                                let dungeon = if undercity {
                                    crate::characteristics::DungeonId::Undercity
                                } else {
                                    let chosen = self
                                        .resolution_cursor_mut()
                                        .and_then(|c| c.chosen_option_index.take());
                                    let Some(idx) = chosen else {
                                        let labels = crate::dungeon::STANDARD_DUNGEONS
                                            .iter()
                                            .map(|(_, name)| (*name).to_owned())
                                            .collect();
                                        return Ok(StepOutcome::Pending(self.option_frame(
                                            entry.id(),
                                            index,
                                            actor,
                                            labels,
                                        )?));
                                    };
                                    crate::dungeon::STANDARD_DUNGEONS
                                        .get(usize::from(idx))
                                        .map_or(
                                            crate::characteristics::DungeonId::LostMineOfPhandelver,
                                            |(id, _)| *id,
                                        )
                                };
                                (dungeon, 0u8)
                            }
                            Some((dungeon, current_room)) => {
                                let rooms = crate::dungeon::dungeon_rooms(
                                    dungeon,
                                    self.creature_vocabulary(),
                                );
                                let exits = rooms
                                    .get(usize::from(current_room))
                                    .map_or(&[][..], |r| r.exits);
                                if exits.is_empty() {
                                    let progress =
                                        self.characteristics.dungeons.entry(actor).or_default();
                                    progress.active = None;
                                    progress.completed = progress.completed.saturating_add(1);
                                    self.emit_keyword_action_event(
                                        entry.id(),
                                        index,
                                        actor,
                                        PlayerTriggerAction::CompletedDungeon,
                                        ctx.append,
                                    )?;
                                    return Ok(StepOutcome::Continue);
                                }
                                let next_room = if exits.len() == 1 {
                                    exits[0]
                                } else {
                                    let chosen = self
                                        .resolution_cursor_mut()
                                        .and_then(|c| c.chosen_option_index.take());
                                    let Some(idx) = chosen else {
                                        let labels = exits
                                            .iter()
                                            .map(|&r| {
                                                rooms.get(usize::from(r)).map_or_else(
                                                    || format!("Room {r}"),
                                                    |rm| rm.name.to_owned(),
                                                )
                                            })
                                            .collect();
                                        return Ok(StepOutcome::Pending(self.option_frame(
                                            entry.id(),
                                            index,
                                            actor,
                                            labels,
                                        )?));
                                    };
                                    exits.get(usize::from(idx)).copied().unwrap_or(exits[0])
                                };
                                (dungeon, next_room)
                            }
                        };
                        let rooms =
                            crate::dungeon::dungeon_rooms(dungeon, self.creature_vocabulary());
                        let room = rooms
                            .get(usize::from(next_room))
                            .ok_or(EffectError::InconsistentProgram)?;
                        let is_bottom = room.exits.is_empty();
                        let room_ops = room.body.instructions.clone();
                        let progress = self.characteristics.dungeons.entry(actor).or_default();
                        if is_bottom {
                            progress.active = None;
                            progress.completed = progress.completed.saturating_add(1);
                        } else {
                            progress.active = Some((dungeon, next_room));
                        }
                        self.emit_keyword_action_event(
                            entry.id(),
                            index,
                            actor,
                            PlayerTriggerAction::Ventured,
                            ctx.append,
                        )?;
                        if is_bottom {
                            self.emit_keyword_action_event(
                                entry.id(),
                                index,
                                actor,
                                PlayerTriggerAction::CompletedDungeon,
                                ctx.append,
                            )?;
                        }
                        if !room_ops.is_empty() {
                            self.splice_replacing_current(entry.id(), index, &room_ops)?;
                        } else {
                            self.advance_instruction()?;
                        }
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::TakeInitiative => {
                        let cursor_snap = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .clone();
                        let actor =
                            self.resolving_player_identity_at(player, Some(&cursor_snap))?;
                        self.characteristics.initiative = Some(actor);
                        self.splice_replacing_current(
                            entry.id(),
                            index,
                            &[Instruction::MutatePlayer(Box::new(
                                crate::toolbox::MutatePlayerSpec::venture_into_dungeon(
                                    player, true,
                                ),
                            ))],
                        )?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::ChooseOneLastMoved => {
                        let chooser = player;
                        let cursor_snap = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .clone();
                        let candidates = cursor_snap.results.last_moved_objects.clone();
                        if candidates.len() >= 2 {
                            let chooser_seat =
                                self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                            let chosen = self
                                .resolution_cursor_mut()
                                .and_then(|c| c.chosen_option_index.take());
                            let Some(idx) = chosen else {
                                let labels: Vec<String> = candidates
                                    .iter()
                                    .enumerate()
                                    .map(|(i, id)| {
                                        self.object_display_label(*id, &format!("Card {}", i + 1))
                                    })
                                    .collect();
                                return Ok(StepOutcome::Pending(self.option_frame(
                                    entry.id(),
                                    index,
                                    chooser_seat,
                                    labels,
                                )?));
                            };
                            if let Some(&picked) = candidates.get(usize::from(idx))
                                && let Some(cur) = self.resolution_cursor_mut()
                            {
                                cur.results.last_moved_objects = vec![picked];
                                cur.results.last_action_count = Some(RulesInteger::from(1));
                                cur.results.last_action_succeeded = true;
                            }
                        }
                        self.advance_instruction()?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::Behold { filter, optional } => {
                        let filter = **filter;
                        let optional = *optional;
                        let controller = entry.controller();
                        let evaluated = self.evaluated_characteristics()?;
                        let mut candidates: Vec<(ObjectId, bool)> = Vec::new();
                        let excluded = BTreeSet::new();
                        for o in evaluated.objects.values() {
                            if o.zone == ObjectZone::Battlefield
                                && o.controller == Some(controller)
                                && self.matches_object_filter(
                                    filter, o, controller, None, &excluded, &evaluated,
                                )
                            {
                                candidates.push((o.object, false));
                            } else if o.zone == ObjectZone::Hand(controller)
                                && self.matches_graveyard_card(filter, o)
                            {
                                candidates.push((o.object, true));
                            }
                        }
                        if candidates.is_empty() {
                            if let Some(cur) = self.resolution_cursor_mut() {
                                cur.results.last_action_succeeded = false;
                            }
                            self.advance_instruction()?;
                            return Ok(StepOutcome::Continue);
                        }
                        if !optional && candidates.len() == 1 {
                            if let Some(cur) = self.resolution_cursor_mut() {
                                cur.results.last_action_succeeded = true;
                            }
                            self.advance_instruction()?;
                            return Ok(StepOutcome::Continue);
                        }
                        let chosen = self
                            .resolution_cursor_mut()
                            .and_then(|c| c.chosen_option_index.take());
                        let Some(idx) = chosen else {
                            let mut labels: Vec<String> = candidates
                                .iter()
                                .enumerate()
                                .map(|(i, (id, from_hand))| {
                                    let name =
                                        self.object_display_label(*id, &format!("Card {}", i + 1));
                                    if *from_hand {
                                        format!("Reveal from hand: {name}")
                                    } else {
                                        format!("Choose controlled: {name}")
                                    }
                                })
                                .collect();
                            if optional {
                                labels.push("Decline to behold".to_owned());
                            }
                            return Ok(StepOutcome::Pending(self.option_frame(
                                entry.id(),
                                index,
                                controller,
                                labels,
                            )?));
                        };
                        let beheld = usize::from(idx) < candidates.len();
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.results.last_action_succeeded = beheld;
                        }
                        self.advance_instruction()?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::ManifestDread => {
                        let controller = entry.controller();
                        let top_two: Vec<ObjectId> = self
                            .window
                            .cards()
                            .library(controller)
                            .iter()
                            .take(2)
                            .copied()
                            .collect();
                        if top_two.is_empty() {
                            self.advance_instruction()?;
                            return Ok(StepOutcome::Continue);
                        }
                        let (manifest_id, graveyard_id) = if top_two.len() == 1 {
                            (top_two[0], None)
                        } else {
                            let chosen = self
                                .resolution_cursor_mut()
                                .and_then(|c| c.chosen_option_index.take());
                            let Some(idx) = chosen else {
                                let labels = top_two
                                    .iter()
                                    .enumerate()
                                    .map(|(i, id)| {
                                        format!(
                                            "Manifest: {}",
                                            self.object_display_label(
                                                *id,
                                                &format!("Card {}", i + 1)
                                            )
                                        )
                                    })
                                    .collect();
                                return Ok(StepOutcome::Pending(self.option_frame(
                                    entry.id(),
                                    index,
                                    controller,
                                    labels,
                                )?));
                            };
                            if idx == 0 {
                                (top_two[0], Some(top_two[1]))
                            } else {
                                (top_two[1], Some(top_two[0]))
                            }
                        };
                        if let Some(gy_id) = graveyard_id {
                            let _ = self.move_objects_to_subject_destination(
                                entry,
                                index,
                                controller,
                                &[gy_id],
                                SubjectDestination::Graveyard,
                                ctx.append,
                            )?;
                        }
                        self.emit_keyword_action_event(
                            entry.id(),
                            index,
                            controller,
                            PlayerTriggerAction::ManifestedDread,
                            ctx.append,
                        )?;
                        self.linked_return_work = Some(LinkedReturnWork {
                            origin: LinkedReturnOrigin::Manifest {
                                player: controller,
                                entry: entry.id(),
                                instruction: index,
                            },
                            objects: vec![manifest_id],
                            library_bottom: None,
                            wake_entry: None,
                            orders: BTreeMap::new(),
                            graveyard_orders: BTreeMap::new(),
                            choices: BTreeMap::new(),
                            auras: BTreeMap::new(),
                            replacements: None,
                        });
                        self.finish_linked_return_if_ready(ctx.append)?;
                        if let Some(frame) = self.pending_resolution_decision() {
                            return Ok(StepOutcome::Pending(frame));
                        }
                        self.advance_instruction()?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::Ripple(n) => {
                        let n = *n;
                        let controller = entry.controller();
                        let top_n: Vec<ObjectId> = self
                            .window
                            .cards()
                            .library(controller)
                            .iter()
                            .take(usize::from(n))
                            .copied()
                            .collect();
                        let source_names: BTreeSet<String> = match entry.kind() {
                            StackKind::CardSpell(id) => self
                                .copiable_program(*id)
                                .map(|p| p.names.iter().map(|nm| nm.as_str().to_owned()).collect())
                                .unwrap_or_default(),
                            StackKind::ActivatedAbility { source }
                            | StackKind::TriggeredAbility { source } => self
                                .copiable_program(source.id())
                                .map(|p| p.names.iter().map(|nm| nm.as_str().to_owned()).collect())
                                .unwrap_or_default(),
                            _ => BTreeSet::new(),
                        };
                        let mut matching = Vec::new();
                        let mut rest = Vec::new();
                        for id in top_n {
                            if let Some(object) = self.window.cards().get(id).cloned() {
                                let before = self.rules_snapshot(None)?;
                                self.emit_from(
                                    EventCause::Instruction {
                                        entry: entry.id(),
                                        instruction: index,
                                    },
                                    &before,
                                    None,
                                    ctx.append,
                                    |_, _, _| {
                                        Ok(vec![RulesOccurrence::LibraryCardRevealed {
                                            object: object.clone(),
                                        }])
                                    },
                                )?;
                            }
                            let shares_name = !source_names.is_empty()
                                && self.copiable_program(id).is_some_and(|p| {
                                    p.names.iter().any(|nm| source_names.contains(nm.as_str()))
                                });
                            if shares_name {
                                matching.push(id);
                            } else {
                                rest.push(id);
                            }
                        }
                        if !matching.is_empty() {
                            let _ = self.move_objects_to_subject_destination(
                                entry,
                                index,
                                controller,
                                &matching,
                                SubjectDestination::Exile {
                                    delayed_return: None,
                                },
                                ctx.append,
                            )?;
                            for card_id in matching {
                                self.card_play_permissions
                                    .insert(super::CardPlayPermission {
                                        zone: super::CardPlayZone::Exile,
                                        object: card_id,
                                        player: controller,
                                        duration: super::CardPlayDuration::ThroughTurn,
                                        any_color: false,
                                        free: true,
                                        cast_only: true,
                                    });
                                self.free_exiled_cards.insert((card_id, controller));
                            }
                        }
                        if !rest.is_empty() {
                            let _ = self.move_objects_to_subject_destination(
                                entry,
                                index,
                                controller,
                                &rest,
                                SubjectDestination::Library(LibraryPlacement::Bottom),
                                ctx.append,
                            )?;
                        }
                        self.advance_instruction()?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::Demonstrate => {
                        let target_id = self
                            .window
                            .entries()
                            .iter()
                            .rev()
                            .find(|e| {
                                matches!(
                                    e.kind(),
                                    StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                                )
                            })
                            .map(crate::stack_window::StackEntry::id)
                            .unwrap_or_else(|| entry.id());
                        if let Ok(copy_src) = self.capture_copy_source(target_id, 0.into()) {
                            let mut recipients = vec![entry.controller()];
                            if let Ok(opps) = self
                                .combat_rules
                                .opponents(self.window.participants(), entry.controller())
                                && let Some(&first_opp) = opps.first()
                            {
                                recipients.push(first_opp);
                            }
                            for seat in recipients {
                                let before = self.rules_snapshot(None)?;
                                let occ = self.create_copy(&copy_src, seat, vec![])?;
                                self.emit_from(
                                    EventCause::Instruction {
                                        entry: entry.id(),
                                        instruction: index,
                                    },
                                    &before,
                                    None,
                                    ctx.append,
                                    |_, _, _| Ok(vec![occ]),
                                )?;
                            }
                        }
                        self.advance_instruction()?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::RollDie {
                        sides: _,
                        count,
                        mode,
                        bands,
                    } => {
                        let cursor_snap = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .clone();
                        let roller =
                            self.resolving_player_identity_at(player, Some(&cursor_snap))?;
                        if !cursor_snap.results.die_roll_ready {
                            let raw_n = self.resolve_value_expr(entry.controller(), count)?;
                            if !raw_n.is_positive() {
                                if let Some(cur) = self.resolution_cursor_mut() {
                                    cur.results.last_die_roll = Some(0.into());
                                    cur.results.last_die_rolls.clear();
                                }
                                self.advance_instruction()?;
                                return Ok(StepOutcome::Continue);
                            }
                            self.suspend_for_random_resolution(roller)?;
                            return Ok(StepOutcome::Continue);
                        }
                        let rolls = cursor_snap.results.last_die_rolls.clone();
                        let final_result: u16 = match mode {
                            DieRollMode::Sum => {
                                rolls.iter().copied().fold(0u16, u16::saturating_add)
                            }
                            DieRollMode::KeepHighest => rolls.iter().copied().max().unwrap_or(0),
                            DieRollMode::ChooseOne => {
                                if rolls.len() <= 1 {
                                    rolls.first().copied().unwrap_or(0)
                                } else {
                                    let chosen = self
                                        .resolution_cursor_mut()
                                        .and_then(|c| c.chosen_option_index.take());
                                    let Some(idx) = chosen else {
                                        let labels =
                                            rolls.iter().map(|v| format!("Result: {v}")).collect();
                                        return Ok(StepOutcome::Pending(self.option_frame(
                                            entry.id(),
                                            index,
                                            roller,
                                            labels,
                                        )?));
                                    };
                                    rolls.get(usize::from(idx)).copied().unwrap_or(0)
                                }
                            }
                        };
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.results.die_roll_ready = false;
                            cur.results.last_die_rolls.clear();
                            cur.results.last_die_roll = Some(i64::from(final_result).into());
                        }
                        if let Some(band) = bands
                            .iter()
                            .find(|b| final_result >= b.min && final_result <= b.max)
                            && !band.body.instructions.is_empty()
                        {
                            self.splice_replacing_current(
                                entry.id(),
                                index,
                                &band.body.instructions,
                            )?;
                            return Ok(StepOutcome::Continue);
                        }
                        self.advance_instruction()?;
                        Ok(StepOutcome::Continue)
                    }
                    crate::toolbox::PlayerMutation::FlipCoins {
                        mode: _,
                        win_body,
                        lose_body,
                        lose_destination,
                    } => {
                        let cursor_snap = self
                            .resolution_cursor()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .clone();
                        let flipper =
                            self.resolving_player_identity_at(player, Some(&cursor_snap))?;
                        if !cursor_snap.results.coin_flip_ready {
                            self.suspend_for_random_resolution(flipper)?;
                            return Ok(StepOutcome::Continue);
                        }
                        let won = cursor_snap.results.last_coin_flip_won == Some(true);
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.results.coin_flip_ready = false;
                        }
                        if !won && let Some(dest) = *lose_destination {
                            self.set_instruction(dest)?;
                            return Ok(StepOutcome::Continue);
                        }
                        let chosen_body = if won {
                            win_body.as_deref()
                        } else {
                            lose_body.as_deref()
                        };
                        if let Some(body) = chosen_body
                            && !body.instructions.is_empty()
                        {
                            self.splice_replacing_current(entry.id(), index, &body.instructions)?;
                            return Ok(StepOutcome::Continue);
                        }
                        self.advance_instruction()?;
                        Ok(StepOutcome::Continue)
                    }
                    _ => Ok(StepOutcome::Next(Instruction::MutatePlayer(spec))),
                }
            }
            Instruction::StackOp(spec) => match &spec.op {
                crate::toolbox::StackOpKind::Discover { value } => {
                    let controller = entry.controller();
                    let has_state = self
                        .resolution_cursor()
                        .is_some_and(|c| c.results.discover_state.is_some());
                    if !has_state {
                        let limit = self.resolve_value_expr(controller, value)?;
                        let mut exiled = Vec::new();
                        let mut candidate = None;
                        while let Some(&top) = self.window.cards().library(controller).first() {
                            let before = self.rules_snapshot(None)?;
                            let (window, exit) = self.window.resolution_move_with_exit(
                                self.window.revision(),
                                top,
                                ObjectDestination::Exile,
                            )?;
                            let crate::stack_window::StackExit::Object(movement) = exit else {
                                return Err(EffectError::InconsistentProgram);
                            };
                            self.window = window;
                            let object = movement.after.id();
                            let program = self
                                .copiable_program(object)
                                .ok_or(EffectError::InconsistentProgram)?;
                            let eligible =
                                !program.has_type(CardType::Land) && program.mana_value() <= limit;
                            self.emit_from(
                                EventCause::Instruction {
                                    entry: entry.id(),
                                    instruction: index,
                                },
                                &before,
                                None,
                                ctx.append,
                                |prev, now, controllers| {
                                    Ok(vec![occurrence_flow::zone_occurrence_at(
                                        prev,
                                        now,
                                        &movement,
                                        ZoneReason::CascadeExile,
                                        controllers,
                                    )?])
                                },
                            )?;
                            exiled.push(object);
                            if eligible {
                                candidate = Some(object);
                                break;
                            }
                        }
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.results.discover_state = Some(DiscoverState {
                                candidate,
                                exiled,
                                awaiting_random_bottom: false,
                                bottom_order: None,
                            });
                        }
                        self.emit_keyword_action_event(
                            entry.id(),
                            index,
                            controller,
                            PlayerTriggerAction::Discovered,
                            ctx.append,
                        )?;
                        return Ok(StepOutcome::Continue);
                    }
                    let candidate = self
                        .resolution_cursor()
                        .and_then(|c| c.results.discover_state.as_ref())
                        .and_then(|d| d.candidate);
                    if let Some(card_id) = candidate {
                        let can_cast = self
                            .exiled_cast_offer(&Instruction::StackOp(Box::new(
                                crate::toolbox::StackOpSpec::cast_found_spell(card_id),
                            )))?
                            .is_some();
                        if !can_cast {
                            let _ = self.move_objects_to_subject_destination(
                                entry,
                                index,
                                controller,
                                &[card_id],
                                SubjectDestination::Hand,
                                ctx.append,
                            )?;
                            if let Some(d) = self
                                .resolution_cursor_mut()
                                .and_then(|c| c.results.discover_state.as_mut())
                            {
                                d.candidate = None;
                            }
                            return Ok(StepOutcome::Continue);
                        }
                        let chosen = self
                            .resolution_cursor_mut()
                            .and_then(|c| c.chosen_option_index.take());
                        let Some(idx) = chosen else {
                            let labels = vec![
                                "Cast without paying its mana cost".to_owned(),
                                "Put into your hand".to_owned(),
                            ];
                            return Ok(StepOutcome::Pending(self.option_frame(
                                entry.id(),
                                index,
                                controller,
                                labels,
                            )?));
                        };
                        if let Some(d) = self
                            .resolution_cursor_mut()
                            .and_then(|c| c.results.discover_state.as_mut())
                        {
                            d.candidate = None;
                        }
                        if idx == 0 {
                            self.splice_before_current(
                                entry.id(),
                                index,
                                &[Instruction::StackOp(Box::new(
                                    crate::toolbox::StackOpSpec::cast_found_spell(card_id),
                                ))],
                            )?;
                        } else {
                            let _ = self.move_objects_to_subject_destination(
                                entry,
                                index,
                                controller,
                                &[card_id],
                                SubjectDestination::Hand,
                                ctx.append,
                            )?;
                        }
                        return Ok(StepOutcome::Continue);
                    }
                    let ready_order = self
                        .resolution_cursor_mut()
                        .and_then(|c| c.results.discover_state.as_mut())
                        .and_then(|d| d.bottom_order.take());
                    let remaining: Vec<ObjectId> = if let Some(order) = ready_order {
                        order
                    } else {
                        let rem: Vec<ObjectId> = self
                            .resolution_cursor()
                            .and_then(|c| c.results.discover_state.as_ref())
                            .map(|d| {
                                d.exiled
                                    .iter()
                                    .copied()
                                    .filter(|id| {
                                        self.window
                                            .cards()
                                            .get(*id)
                                            .is_some_and(|o| o.zone() == ObjectZone::Exile)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        if rem.len() > 1 {
                            if let Some(d) = self
                                .resolution_cursor_mut()
                                .and_then(|c| c.results.discover_state.as_mut())
                            {
                                d.awaiting_random_bottom = true;
                            }
                            self.suspend_for_random_resolution(controller)?;
                            return Ok(StepOutcome::Continue);
                        }
                        rem
                    };
                    if !remaining.is_empty() {
                        let _ = self.move_objects_to_subject_destination(
                            entry,
                            index,
                            controller,
                            &remaining,
                            SubjectDestination::Library(LibraryPlacement::Bottom),
                            ctx.append,
                        )?;
                    }
                    if let Some(cur) = self.resolution_cursor_mut() {
                        cur.results.discover_state = None;
                    }
                    self.advance_instruction()?;
                    Ok(StepOutcome::Continue)
                }
                _ => Ok(StepOutcome::Next(Instruction::StackOp(spec))),
            },
            other => Ok(StepOutcome::Next(other)),
        }
    }

    fn execute_unlock_door_resolved(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        obj_opt: Option<ObjectId>,
        door: Option<crate::program::SplitHalf>,
        allow_lock: bool,
    ) -> Result<StepOutcome, EffectError> {
        let Some(obj_id) = obj_opt.filter(|id| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
        }) else {
            if let Some(cur) = self.resolution_cursor_mut() {
                cur.results.last_action_succeeded = false;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        };
        let current = self
            .characteristics
            .unlocked_doors
            .get(&obj_id)
            .copied()
            .unwrap_or_default();
        let action: Option<(SplitHalf, bool)> = if let Some(half) = door {
            let is_open = current.is_unlocked(half);
            if !is_open {
                Some((half, true))
            } else if allow_lock {
                Some((half, false))
            } else {
                None
            }
        } else {
            let mut choices: Vec<(SplitHalf, bool, String)> = Vec::new();
            if !current.left {
                choices.push((SplitHalf::Left, true, "Unlock left door".to_owned()));
            } else if allow_lock {
                choices.push((SplitHalf::Left, false, "Lock left door".to_owned()));
            }
            if !current.right {
                choices.push((SplitHalf::Right, true, "Unlock right door".to_owned()));
            } else if allow_lock {
                choices.push((SplitHalf::Right, false, "Lock right door".to_owned()));
            }
            match choices.as_slice() {
                [] => None,
                [(h, open, _)] => Some((*h, *open)),
                _ => {
                    let chosen = self
                        .resolution_cursor_mut()
                        .and_then(|c| c.chosen_option_index.take());
                    let Some(idx) = chosen else {
                        let labels = choices.iter().map(|(_, _, lbl)| lbl.clone()).collect();
                        return Ok(StepOutcome::Pending(self.option_frame(
                            entry.id(),
                            index,
                            entry.controller(),
                            labels,
                        )?));
                    };
                    choices
                        .get(usize::from(idx))
                        .map(|(h, open, _)| (*h, *open))
                }
            }
        };
        if let Some((half, unlock)) = action {
            let doors = self
                .characteristics
                .unlocked_doors
                .entry(obj_id)
                .or_default();
            let was_open = doors.is_unlocked(half);
            match half {
                SplitHalf::Left => doors.left = unlock,
                SplitHalf::Right => doors.right = unlock,
            }
            let fully_unlocked = doors.fully_unlocked();
            if unlock && !was_open {
                let before = self.rules_snapshot(None)?;
                self.emit_from(
                    EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![RulesOccurrence::DoorUnlocked {
                            object: obj_id,
                            player: entry.controller(),
                            half,
                            fully_unlocked,
                        }])
                    },
                )?;
            }
            if let Some(cur) = self.resolution_cursor_mut() {
                cur.results.last_action_succeeded = true;
            }
        } else if let Some(cur) = self.resolution_cursor_mut() {
            cur.results.last_action_succeeded = false;
        }
        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }
}
