//! Execution handlers for Pass C orthogonal Comprehensive Rules instructions:
//! RollDie, FlipCoins, ChooseValue, ChooseOptionForPlayers, Vote, Incubate,
//! Connive / ConniveFinalize, Discover, Clash, RingTemptsYou,
//! VentureIntoDungeon, TakeInitiative, UnlockDoor, Behold, and ManifestDread.
use super::progress_rules::{RuleStepContext, StepOutcome};
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
use crate::program::{
    AffectedPlayers, DieRollMode, LibraryPlacement, MoveSubjectRef, PlayerTriggerAction,
    ResolutionChoiceDomain, ResolvingPlayer, SplitHalf, SubjectDestination,
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
                .window
                .cards()
                .library(entry.controller())
                .first()
                .copied(),
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

    fn resolve_pass_c_move_subjects(
        &self,
        entry: &crate::stack_window::StackEntry,
        subject: MoveSubjectRef,
    ) -> Result<Vec<ObjectId>, EffectError> {
        match subject {
            MoveSubjectRef::CreatedTokens => Ok(self
                .resolution_cursor()
                .map(|c| c.last_created_tokens.clone())
                .unwrap_or_default()),
            MoveSubjectRef::LastMoved => Ok(self
                .resolution_cursor()
                .map(|c| {
                    if !c.results.last_moved_objects.is_empty() {
                        c.results.last_moved_objects.clone()
                    } else {
                        c.results.returned_target_permanent.into_iter().collect()
                    }
                })
                .unwrap_or_default()),
            other => Ok(self
                .resolve_pass_c_move_subject(entry, other)?
                .into_iter()
                .collect()),
        }
    }

    fn splice_replacing_current(
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

    fn suspend_for_random_resolution(&mut self, chooser: SeatId) -> Result<(), EffectError> {
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
            Instruction::RollDie {
                player,
                sides: _,
                count,
                mode,
                bands,
            } => {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let roller = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
                if !cursor_snap.results.die_roll_ready {
                    let raw_n = self.resolve_value_expr(entry.controller(), &count)?;
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
                    DieRollMode::Sum => rolls.iter().copied().fold(0u16, u16::saturating_add),
                    DieRollMode::KeepHighest => rolls.iter().copied().max().unwrap_or(0),
                    DieRollMode::ChooseOne => {
                        if rolls.len() <= 1 {
                            rolls.first().copied().unwrap_or(0)
                        } else {
                            let chosen = self
                                .resolution_cursor_mut()
                                .and_then(|c| c.chosen_option_index.take());
                            let Some(idx) = chosen else {
                                let labels = rolls.iter().map(|v| format!("Result: {v}")).collect();
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
                    self.splice_replacing_current(entry.id(), index, &band.body.instructions)?;
                    return Ok(StepOutcome::Continue);
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::FlipCoins {
                player,
                mode: _,
                win_body,
                lose_body,
            } => {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let flipper = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
                if !cursor_snap.results.coin_flip_ready {
                    self.suspend_for_random_resolution(flipper)?;
                    return Ok(StepOutcome::Continue);
                }
                let won = cursor_snap.results.last_coin_flip_won == Some(true);
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.coin_flip_ready = false;
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
            Instruction::ChooseValue { chooser, domain } => {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                match domain {
                    ResolutionChoiceDomain::Color => {
                        let chosen = self
                            .resolution_cursor_mut()
                            .and_then(|c| c.chosen_option_index.take());
                        let Some(idx) = chosen else {
                            let labels = ["White", "Blue", "Black", "Red", "Green"]
                                .into_iter()
                                .map(str::to_owned)
                                .collect();
                            return Ok(StepOutcome::Pending(self.option_frame(
                                entry.id(),
                                index,
                                actor,
                                labels,
                            )?));
                        };
                        let mana = match idx {
                            0 => ManaType::White,
                            1 => ManaType::Blue,
                            2 => ManaType::Black,
                            3 => ManaType::Red,
                            _ => ManaType::Green,
                        };
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.chosen_effect_color = Some(mana);
                        }
                    }
                    ResolutionChoiceDomain::Player { opponents_only } => {
                        let seats: Vec<SeatId> = if opponents_only {
                            self.combat_rules
                                .opponents(self.window.participants(), actor)?
                        } else {
                            self.window.participants().to_vec()
                        };
                        match seats.as_slice() {
                            [] => {}
                            [only] => {
                                if let Some(cur) = self.resolution_cursor_mut() {
                                    cur.chosen_opponent = Some(*only);
                                }
                            }
                            _ => {
                                let chosen = self
                                    .resolution_cursor_mut()
                                    .and_then(|c| c.chosen_option_index.take());
                                let Some(idx) = chosen else {
                                    let labels = (0..seats.len())
                                        .map(|i| format!("Player {}", i + 1))
                                        .collect();
                                    return Ok(StepOutcome::Pending(self.option_frame(
                                        entry.id(),
                                        index,
                                        actor,
                                        labels,
                                    )?));
                                };
                                if let Some(&seat) = seats.get(usize::from(idx))
                                    && let Some(cur) = self.resolution_cursor_mut()
                                {
                                    cur.chosen_opponent = Some(seat);
                                }
                            }
                        }
                    }
                    ResolutionChoiceDomain::CardType(allowed) => {
                        let types = if allowed.is_empty() {
                            vec![
                                CardType::Artifact,
                                CardType::Creature,
                                CardType::Enchantment,
                                CardType::Instant,
                                CardType::Land,
                                CardType::Planeswalker,
                                CardType::Sorcery,
                            ]
                        } else {
                            allowed
                        };
                        let chosen = self
                            .resolution_cursor_mut()
                            .and_then(|c| c.chosen_option_index.take());
                        let Some(idx) = chosen else {
                            let labels = types.iter().map(|t| format!("{t:?}")).collect();
                            return Ok(StepOutcome::Pending(self.option_frame(
                                entry.id(),
                                index,
                                actor,
                                labels,
                            )?));
                        };
                        if let Some(&ct) = types.get(usize::from(idx))
                            && let Some(cur) = self.resolution_cursor_mut()
                        {
                            cur.results.chosen_card_type = Some(ct);
                        }
                    }
                    ResolutionChoiceDomain::BasicLandType => {
                        let types = [
                            crate::land::LandType::Plains,
                            crate::land::LandType::Island,
                            crate::land::LandType::Swamp,
                            crate::land::LandType::Mountain,
                            crate::land::LandType::Forest,
                        ];
                        let chosen = self
                            .resolution_cursor_mut()
                            .and_then(|c| c.chosen_option_index.take());
                        let Some(idx) = chosen else {
                            let labels = ["Plains", "Island", "Swamp", "Mountain", "Forest"]
                                .into_iter()
                                .map(str::to_owned)
                                .collect();
                            return Ok(StepOutcome::Pending(self.option_frame(
                                entry.id(),
                                index,
                                actor,
                                labels,
                            )?));
                        };
                        if let Some(&lt) = types.get(usize::from(idx))
                            && let Some(cur) = self.resolution_cursor_mut()
                        {
                            cur.results.chosen_basic_land_type = Some(lt);
                        }
                    }
                    ResolutionChoiceDomain::Number { min, max } => {
                        let chosen = self
                            .resolution_cursor_mut()
                            .and_then(|c| c.chosen_option_index.take());
                        let Some(idx) = chosen else {
                            let labels = (min..=max).map(|n| n.to_string()).collect();
                            return Ok(StepOutcome::Pending(self.option_frame(
                                entry.id(),
                                index,
                                actor,
                                labels,
                            )?));
                        };
                        let value = min.saturating_add(u16::from(idx)).min(max);
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.results.chosen_number = Some(i64::from(value).into());
                        }
                    }
                    ResolutionChoiceDomain::LabeledOption(labels) => {
                        let chosen = self
                            .resolution_cursor_mut()
                            .and_then(|c| c.chosen_option_index.take());
                        let Some(idx) = chosen else {
                            return Ok(StepOutcome::Pending(self.option_frame(
                                entry.id(),
                                index,
                                actor,
                                labels,
                            )?));
                        };
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.results.chosen_option_index = Some(idx);
                        }
                    }
                    ResolutionChoiceDomain::CardName { nonland_only } => {
                        let mut names = BTreeSet::new();
                        for obj in self.window.cards().iter() {
                            if let Some(card_id) = obj.card_id()
                                && let Some(program) = self.programs.get(&card_id)
                            {
                                if nonland_only && program.has_type(CardType::Land) {
                                    continue;
                                }
                                for name in program.names.iter() {
                                    names.insert(name.as_str().to_owned());
                                }
                            }
                        }
                        let mut labels: Vec<String> = names.into_iter().take(254).collect();
                        labels.push("Other card name".to_owned());
                        let chosen = self
                            .resolution_cursor_mut()
                            .and_then(|c| c.chosen_option_index.take());
                        let Some(idx) = chosen else {
                            return Ok(StepOutcome::Pending(self.option_frame(
                                entry.id(),
                                index,
                                actor,
                                labels,
                            )?));
                        };
                        if let Some(selected_name) = labels.get(usize::from(idx)).cloned() {
                            if let Some(cur) = self.resolution_cursor_mut() {
                                cur.results.chosen_card_name = Some(selected_name.clone());
                            }
                            if let StackKind::ActivatedAbility { source }
                            | StackKind::TriggeredAbility { source } = entry.kind()
                            {
                                self.characteristics
                                    .named_cards
                                    .insert(source.id(), selected_name);
                            }
                        }
                    }
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::ChooseOptionForPlayers { players, options } => {
                let initialized = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .results
                    .option_players_initialized;
                if !initialized {
                    let cursor_snap = self
                        .resolution_cursor()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .clone();
                    let seats =
                        self.selection_players(players, entry.controller(), Some(&cursor_snap))?;
                    if let Some(cur) = self.resolution_cursor_mut() {
                        cur.results.pending_option_players = seats.into();
                        cur.results.option_players_initialized = true;
                    }
                }
                let next_actor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .results
                    .pending_option_players
                    .front()
                    .copied();
                let Some(actor) = next_actor else {
                    if let Some(cur) = self.resolution_cursor_mut() {
                        cur.results.option_players_initialized = false;
                    }
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                };
                let chosen = self
                    .resolution_cursor_mut()
                    .and_then(|c| c.chosen_option_index.take());
                let Some(idx) = chosen else {
                    let labels = options.iter().map(|b| b.label.clone()).collect();
                    return Ok(StepOutcome::Pending(self.option_frame(
                        entry.id(),
                        index,
                        actor,
                        labels,
                    )?));
                };
                let branch = options
                    .get(usize::from(idx))
                    .ok_or(EffectError::InconsistentProgram)?;
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.pending_option_players.pop_front();
                    cur.chosen_opponent = Some(actor);
                    cur.results.chosen_option_index = Some(idx);
                }
                if !branch.body.instructions.is_empty() {
                    self.splice_before_current(entry.id(), index, &branch.body.instructions)?;
                }
                Ok(StepOutcome::Continue)
            }
            Instruction::Vote { options, secret: _ } => {
                let initialized = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .results
                    .voters_initialized;
                if !initialized {
                    let mut voters: Vec<SeatId> = self.window.apnap_players()?.collect();
                    if let Some(pos) = voters.iter().position(|p| *p == entry.controller()) {
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
                            options,
                        )?));
                    };
                    if let Some(cur) = self.resolution_cursor_mut() {
                        cur.results.pending_voters.pop_front();
                        if let Some(slot) = cur.results.vote_counts.get_mut(usize::from(idx)) {
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
            Instruction::Incubate { count } => {
                let amount = self.resolve_value_expr(entry.controller(), &count)?;
                let creation =
                    crate::dungeon::incubator_token_creation(self.creature_vocabulary(), amount)
                        .ok_or(EffectError::InconsistentProgram)?;
                self.begin_token_entries(&[creation], None, false)?;
                Ok(StepOutcome::Continue)
            }
            Instruction::Connive { subject, count } => {
                let amount = self.resolve_value_expr(entry.controller(), &count)?;
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
                    Instruction::DrawController {
                        amount: ValueExpr::Literal(amount.clone()),
                    }
                } else {
                    Instruction::DrawPlayer {
                        players: AffectedPlayers::One(ResolvingPlayer::ChosenPlayer),
                        amount: ValueExpr::Literal(amount.clone()),
                    }
                };
                let discard_role = if subj_ctrl == entry.controller() {
                    ResolvingPlayer::Controller
                } else {
                    ResolvingPlayer::ChosenPlayer
                };
                let ops = vec![
                    draw_op,
                    Instruction::Discard {
                        players: AffectedPlayers::One(discard_role),
                        amount: ValueExpr::Literal(amount),
                    },
                    Instruction::ConniveFinalize { subject },
                ];
                self.splice_replacing_current(entry.id(), index, &ops)?;
                Ok(StepOutcome::Continue)
            }
            Instruction::ConniveFinalize { subject } => {
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
            Instruction::Discover { value } => {
                let controller = entry.controller();
                let has_state = self
                    .resolution_cursor()
                    .is_some_and(|c| c.results.discover_state.is_some());
                if !has_state {
                    let limit = self.resolve_value_expr(controller, &value)?;
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
                        .exiled_cast_offer(&Instruction::CastFoundSpell(card_id))?
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
                            &[Instruction::CastFoundSpell(card_id)],
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
            Instruction::Clash { chooser } => {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
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
                        let chooser_card = self.window.cards().library(actor).first().copied();
                        let opponent_card = self.window.cards().library(opp).first().copied();
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
            Instruction::RingTemptsYou => {
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
                                    self.object_display_label(*id, &format!("Creature {}", i + 1))
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
            Instruction::VentureIntoDungeon { player, undercity } => {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let actor = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
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
                        let rooms =
                            crate::dungeon::dungeon_rooms(dungeon, self.creature_vocabulary());
                        let exits = rooms
                            .get(usize::from(current_room))
                            .map_or(&[][..], |r| r.exits);
                        if exits.is_empty() {
                            let progress = self.characteristics.dungeons.entry(actor).or_default();
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
                let rooms = crate::dungeon::dungeon_rooms(dungeon, self.creature_vocabulary());
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
            Instruction::TakeInitiative { player } => {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let actor = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
                self.characteristics.initiative = Some(actor);
                self.splice_replacing_current(
                    entry.id(),
                    index,
                    &[Instruction::VentureIntoDungeon {
                        player,
                        undercity: true,
                    }],
                )?;
                Ok(StepOutcome::Continue)
            }
            Instruction::UnlockDoor {
                subject,
                door,
                allow_lock,
            } => {
                let obj_opt = self.resolve_pass_c_move_subject(entry, subject)?;
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
                                let labels =
                                    choices.iter().map(|(_, _, lbl)| lbl.clone()).collect();
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
            Instruction::Behold { filter, optional } => {
                let controller = entry.controller();
                let evaluated = self.evaluated_characteristics()?;
                let mut candidates: Vec<(ObjectId, bool)> = Vec::new();
                let excluded = BTreeSet::new();
                for o in evaluated.objects.values() {
                    if o.zone == ObjectZone::Battlefield
                        && o.controller == Some(controller)
                        && self.matches_object_filter(
                            *filter, o, controller, None, &excluded, &evaluated,
                        )
                    {
                        candidates.push((o.object, false));
                    } else if o.zone == ObjectZone::Hand(controller)
                        && self.matches_graveyard_card(*filter, o)
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
                            let name = self.object_display_label(*id, &format!("Card {}", i + 1));
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
            Instruction::ManifestDread => {
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
                                    self.object_display_label(*id, &format!("Card {}", i + 1))
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
            Instruction::Monstrosity { amount } => {
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
                    let count = self.resolve_value_expr(entry.controller(), &amount)?;
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
            Instruction::Fight { first, second } => {
                let first_objs = self.resolve_pass_c_move_subjects(entry, *first)?;
                let second_objs = self.resolve_pass_c_move_subjects(entry, *second)?;
                let mut pairs = Vec::new();
                for &a in &first_objs {
                    for &b in &second_objs {
                        if a != b {
                            pairs.push((a, b));
                        }
                    }
                }
                let inputs = self.fight_damage_inputs(&pairs)?;
                if inputs.is_empty() {
                    self.advance_instruction()?;
                } else {
                    self.install_damage_batch(
                        DamageOrigin::Resolution {
                            entry: entry.id(),
                            instruction: index,
                        },
                        DamageKind::Noncombat,
                        inputs,
                        ctx.journal,
                    )?;
                }
                Ok(StepOutcome::Continue)
            }
            Instruction::Exchange(spec) => {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                match spec {
                    crate::program::ExchangeSpec::Control { first, second } => {
                        let a_opt = self.resolve_pass_c_move_subject(entry, *first)?;
                        let b_opt = self.resolve_pass_c_move_subject(entry, *second)?;
                        if let (Some(a), Some(b)) = (a_opt, b_opt)
                            && a != b
                        {
                            let before = self.rules_snapshot(None)?;
                            let current = &before.view().evaluated;
                            let ctrl_a = current
                                .get(a)
                                .filter(|o| o.zone == ObjectZone::Battlefield)
                                .and_then(|o| o.controller);
                            let ctrl_b = current
                                .get(b)
                                .filter(|o| o.zone == ObjectZone::Battlefield)
                                .and_then(|o| o.controller);
                            if let (Some(ca), Some(cb)) = (ctrl_a, ctrl_b)
                                && ca != cb
                            {
                                let eff_a = self.characteristics.add_effect(
                                    entry.id(),
                                    a,
                                    crate::characteristics::ResolvedParts(vec![
                                        crate::characteristics::ResolvedPart::Control(cb),
                                    ]),
                                    crate::characteristics::ResolvedDuration::NoStatedDuration,
                                )?;
                                let eff_b = self.characteristics.add_effect(
                                    entry.id(),
                                    b,
                                    crate::characteristics::ResolvedParts(vec![
                                        crate::characteristics::ResolvedPart::Control(ca),
                                    ]),
                                    crate::characteristics::ResolvedDuration::NoStatedDuration,
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
                                        let app_a = characteristic_appearance_changes(
                                            old,
                                            new,
                                            eff_a.affected.iter().copied(),
                                        )?;
                                        let app_b = characteristic_appearance_changes(
                                            old,
                                            new,
                                            eff_b.affected.iter().copied(),
                                        )?;
                                        Ok(vec![
                                            RulesOccurrence::ContinuousEffectCreated {
                                                effect: eff_a,
                                                appearances: app_a,
                                            },
                                            RulesOccurrence::ContinuousEffectCreated {
                                                effect: eff_b,
                                                appearances: app_b,
                                            },
                                        ])
                                    },
                                )?;
                            }
                        }
                    }
                    crate::program::ExchangeSpec::LifeTotals { first, second } => {
                        let p1 = self.resolving_player_identity_at(first, Some(&cursor_snap))?;
                        let p2 = self.resolving_player_identity_at(second, Some(&cursor_snap))?;
                        if p1 != p2
                            && let (Some(l1), Some(l2)) =
                                (self.life.get(p1).cloned(), self.life.get(p2).cloned())
                        {
                            let evaluated = self.evaluated_characteristics()?;
                            let prohibited = self.life_gain_prohibitions(&evaluated)?;
                            let blocked = (l2 > l1 && prohibited.contains(&p1))
                                || (l1 > l2 && prohibited.contains(&p2));
                            if !blocked {
                                self.adjust_players_at_cut(
                                    entry.id(),
                                    index,
                                    BTreeMap::from([
                                        (p1, crate::life::LifeAdjustment::Set(l2)),
                                        (p2, crate::life::LifeAdjustment::Set(l1)),
                                    ]),
                                    ctx.append,
                                )?;
                            }
                        }
                    }
                    crate::program::ExchangeSpec::LifeWithStat {
                        player,
                        object,
                        stat,
                    } => {
                        let seat = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
                        if let Some(obj_id) = self.resolve_pass_c_move_subject(entry, *object)? {
                            let before = self.rules_snapshot(None)?;
                            let current = &before.view().evaluated;
                            let stats_opt = current
                                .get(obj_id)
                                .filter(|o| o.zone == ObjectZone::Battlefield && o.is_creature())
                                .and_then(|o| o.power_toughness.as_ref())
                                .map(|pt| (pt.derived.power.clone(), pt.derived.toughness.clone()));
                            if let (Some((pwr, tgh)), Some(life)) =
                                (stats_opt, self.life.get(seat).cloned())
                            {
                                let use_power = stat == crate::program::ObjectStatKind::Power;
                                let stat_val = if use_power { pwr } else { tgh.clone() };
                                let blocked = stat_val > life
                                    && self.life_gain_prohibitions(current)?.contains(&seat);
                                if !blocked {
                                    let part = if use_power {
                                        crate::characteristics::ResolvedPart::SetPT(
                                            crate::characteristics::PowerToughness {
                                                power: life,
                                                toughness: tgh,
                                            },
                                        )
                                    } else {
                                        crate::characteristics::ResolvedPart::SetToughness(life)
                                    };
                                    let effect = self.characteristics.add_effect(
                                        entry.id(),
                                        obj_id,
                                        crate::characteristics::ResolvedParts(vec![part]),
                                        crate::characteristics::ResolvedDuration::NoStatedDuration,
                                    )?;
                                    let life_changes = self
                                        .life
                                        .apply_adjustments(
                                            &[(seat, crate::life::LifeAdjustment::Set(stat_val))]
                                                .into(),
                                        )
                                        .map_err(|_| EffectError::InconsistentProgram)?;
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
                                            let mut occs = life_changes
                                                .into_iter()
                                                .map(RulesOccurrence::LifeChanged)
                                                .collect::<Vec<_>>();
                                            occs.push(RulesOccurrence::ContinuousEffectCreated {
                                                effect,
                                                appearances,
                                            });
                                            Ok(occs)
                                        },
                                    )?;
                                }
                            }
                        }
                    }
                    crate::program::ExchangeSpec::Zones { player, from, to } => {
                        let seat = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
                        let collect_zone = |z: crate::program::ExchangeZone| -> Vec<ObjectId> {
                            match z {
                                crate::program::ExchangeZone::Hand => self
                                    .window
                                    .cards()
                                    .iter()
                                    .filter(|c| c.zone() == ObjectZone::Hand(seat))
                                    .map(GameObject::id)
                                    .collect(),
                                crate::program::ExchangeZone::Graveyard => self
                                    .window
                                    .cards()
                                    .iter()
                                    .filter(|c| c.zone() == ObjectZone::Graveyard(seat))
                                    .map(GameObject::id)
                                    .collect(),
                                crate::program::ExchangeZone::Library => {
                                    self.window.cards().library(seat).to_vec()
                                }
                                crate::program::ExchangeZone::Exile => self
                                    .window
                                    .cards()
                                    .iter()
                                    .filter(|c| c.zone() == ObjectZone::Exile && c.owner() == seat)
                                    .map(GameObject::id)
                                    .collect(),
                            }
                        };
                        let to_dest = |z: crate::program::ExchangeZone| -> SubjectDestination {
                            match z {
                                crate::program::ExchangeZone::Hand => SubjectDestination::Hand,
                                crate::program::ExchangeZone::Graveyard => {
                                    SubjectDestination::Graveyard
                                }
                                crate::program::ExchangeZone::Library => {
                                    SubjectDestination::Library(LibraryPlacement::Top)
                                }
                                crate::program::ExchangeZone::Exile => SubjectDestination::Exile {
                                    delayed_return: None,
                                },
                            }
                        };
                        let from_cards = collect_zone(from);
                        let to_cards = collect_zone(to);
                        if !from_cards.is_empty() {
                            let _ = self.move_objects_to_subject_destination(
                                entry,
                                index,
                                seat,
                                &from_cards,
                                to_dest(to),
                                ctx.append,
                            )?;
                        }
                        if !to_cards.is_empty() {
                            let _ = self.move_objects_to_subject_destination(
                                entry,
                                index,
                                seat,
                                &to_cards,
                                to_dest(from),
                                ctx.append,
                            )?;
                        }
                    }
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::CopyStackObject {
                target,
                count,
                may_choose_new_targets: _,
            } => {
                let copies = self
                    .resolve_value_expr(entry.controller(), &count)?
                    .max(0.into())
                    .to_usize()
                    .unwrap_or(0)
                    .min(64);
                let target_entry_id = match target {
                    crate::program::StackCopyTarget::ResolvingSpell => Some(entry.id()),
                    crate::program::StackCopyTarget::TargetSpell
                    | crate::program::StackCopyTarget::TargetAbility
                    | crate::program::StackCopyTarget::TargetSpellOrAbility => self
                        .instruction_target(entry)
                        .ok()
                        .and_then(|bound| match bound.choice().target {
                            TargetRef::Stack(id) => Some(id),
                            _ => None,
                        }),
                    crate::program::StackCopyTarget::TriggeringSpell
                    | crate::program::StackCopyTarget::TriggeringAbility => self
                        .window
                        .entries()
                        .last()
                        .map(crate::stack_window::StackEntry::id),
                };
                if copies > 0
                    && let Some(target_id) = target_entry_id
                    && let Some(target_entry) = self
                        .window
                        .entries()
                        .iter()
                        .chain(self.window.resolving())
                        .find(|e| e.id() == target_id)
                        .cloned()
                {
                    let is_spell = matches!(
                        target_entry.kind(),
                        StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                    );
                    let copy_prohibited = is_spell
                        && self
                            .window
                            .spell_object(&target_entry)
                            .and_then(|obj| self.copiable_program(obj))
                            .is_some_and(|program| program.cannot_be_copied);
                    if !copy_prohibited {
                        for _ in 0..copies {
                            let before = self.rules_snapshot(None)?;
                            let occ = if is_spell {
                                let Ok(copy_src) = self.capture_copy_source(target_id, 0.into())
                                else {
                                    break;
                                };
                                self.create_copy(&copy_src, entry.controller(), vec![])?
                            } else if let Some(payload) = self.payloads.get(&target_id).cloned() {
                                self.create_ability_copy(
                                    &target_entry,
                                    payload,
                                    entry.controller(),
                                    vec![],
                                )?
                            } else {
                                break;
                            };
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
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::ExileGroupFromZone {
                players,
                zone,
                filter,
            } => {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let seats =
                    self.selection_players(players, entry.controller(), Some(&cursor_snap))?;
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let source_id = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    StackKind::CardSpell(id) => Some(*id),
                    _ => None,
                };
                let targets: Vec<ObjectId> = self
                    .window
                    .cards()
                    .iter()
                    .filter(|obj| {
                        let zone_ok = match zone {
                            crate::program::SourceZoneFilter::Hand => {
                                matches!(obj.zone(), ObjectZone::Hand(p) if seats.contains(&p))
                            }
                            crate::program::SourceZoneFilter::Graveyard => {
                                matches!(obj.zone(), ObjectZone::Graveyard(p) if seats.contains(&p))
                            }
                            crate::program::SourceZoneFilter::HandOrGraveyard => {
                                matches!(obj.zone(), ObjectZone::Hand(p) | ObjectZone::Graveyard(p) if seats.contains(&p))
                            }
                            crate::program::SourceZoneFilter::OpponentGraveyard => {
                                matches!(obj.zone(), ObjectZone::Graveyard(p) if p != entry.controller())
                            }
                            crate::program::SourceZoneFilter::AnyGraveyard => {
                                matches!(obj.zone(), ObjectZone::Graveyard(_))
                            }
                            crate::program::SourceZoneFilter::Exile => {
                                obj.zone() == ObjectZone::Exile && seats.contains(&obj.owner())
                            }
                            crate::program::SourceZoneFilter::OpponentExile => {
                                obj.zone() == ObjectZone::Exile
                                    && obj.owner() != entry.controller()
                            }
                            crate::program::SourceZoneFilter::Battlefield => {
                                obj.zone() == ObjectZone::Battlefield
                                    && self.window.cards().phased_out_under(obj.id()).is_none()
                                    && evaluated
                                        .get(obj.id())
                                        .and_then(|v| v.controller)
                                        .is_some_and(|c| seats.contains(&c))
                            }
                            crate::program::SourceZoneFilter::YourLibrary => {
                                matches!(obj.zone(), ObjectZone::Library(p) if seats.contains(&p))
                            }
                            crate::program::SourceZoneFilter::LinkedExile => {
                                obj.zone() == ObjectZone::Exile
                                    && source_id.is_some_and(|src| {
                                        self.linked_exiles
                                            .get(&src)
                                            .is_some_and(|l| l.contains(&obj.id()))
                                    })
                            }
                        };
                        zone_ok
                            && evaluated.get(obj.id()).is_some_and(|view| {
                                self.matches_object_filter(
                                    *filter,
                                    view,
                                    entry.controller(),
                                    source_id,
                                    &excluded,
                                    &evaluated,
                                )
                            })
                    })
                    .map(GameObject::id)
                    .collect();
                let before_controllers: Vec<SeatId> = targets
                    .iter()
                    .filter_map(|&oid| {
                        self.window
                            .cards()
                            .get(oid)
                            .and_then(|o| o.controller().or(Some(o.owner())))
                    })
                    .collect();
                let moved = if targets.is_empty() {
                    0
                } else {
                    self.move_objects_to_subject_destination(
                        entry,
                        index,
                        entry.controller(),
                        &targets,
                        SubjectDestination::Exile {
                            delayed_return: None,
                        },
                        ctx.append,
                    )?
                };
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.last_action_succeeded = moved > 0;
                    cur.results.last_action_count = Some(RulesInteger::from_usize(moved));
                    cur.results.last_moved_objects = targets;
                    cur.results.last_moved_controllers = before_controllers;
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::SacrificeSubject { subject } => {
                let objs: Vec<ObjectId> = self
                    .effect_group(entry, *subject)?
                    .into_iter()
                    .filter(|id| {
                        self.window
                            .cards()
                            .get(*id)
                            .is_some_and(|o| o.zone() == ObjectZone::Battlefield)
                    })
                    .collect();
                let before_controllers: Vec<SeatId> = objs
                    .iter()
                    .filter_map(|&oid| {
                        self.window
                            .cards()
                            .get(oid)
                            .and_then(|o| o.controller().or(Some(o.owner())))
                    })
                    .collect();
                let moved = if objs.is_empty() {
                    0
                } else {
                    self.move_objects_to_subject_destination(
                        entry,
                        index,
                        entry.controller(),
                        &objs,
                        SubjectDestination::Graveyard,
                        ctx.append,
                    )?
                };
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.last_action_succeeded = moved > 0;
                    cur.results.last_action_count = Some(RulesInteger::from_usize(moved));
                    cur.results.last_moved_objects = objs;
                    cur.results.last_moved_controllers = before_controllers;
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::DestroyChosen {
                chooser,
                filter,
                count,
                allow_regeneration: _,
            } => {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                let limit = self
                    .resolve_value_expr(entry.controller(), &count)?
                    .max(0.into())
                    .to_usize()
                    .unwrap_or(0);
                let evaluated = self.evaluated_characteristics()?;
                let excluded = BTreeSet::new();
                let source_id = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    StackKind::CardSpell(id) => Some(*id),
                    _ => None,
                };
                let candidates: Vec<ObjectId> = evaluated
                    .objects
                    .values()
                    .filter(|o| {
                        o.zone == ObjectZone::Battlefield
                            && self.window.cards().phased_out_under(o.object).is_none()
                            && self.matches_object_filter(
                                *filter,
                                o,
                                entry.controller(),
                                source_id,
                                &excluded,
                                &evaluated,
                            )
                    })
                    .map(|o| o.object)
                    .collect();
                let chosen_objs: Vec<ObjectId> = if limit == 0 || candidates.is_empty() {
                    Vec::new()
                } else if candidates.len() <= limit {
                    candidates
                } else {
                    let chosen = self
                        .resolution_cursor_mut()
                        .and_then(|c| c.chosen_option_index.take());
                    let Some(idx) = chosen else {
                        let labels = candidates
                            .iter()
                            .enumerate()
                            .take(usize::from(u8::MAX))
                            .map(|(i, id)| {
                                self.object_display_label(*id, &format!("Permanent {}", i + 1))
                            })
                            .collect();
                        return Ok(StepOutcome::Pending(self.option_frame(
                            entry.id(),
                            index,
                            actor,
                            labels,
                        )?));
                    };
                    let mut picked = Vec::new();
                    if let Some(&first_pick) = candidates.get(usize::from(idx)) {
                        picked.push(first_pick);
                    }
                    for &cand in &candidates {
                        if picked.len() >= limit {
                            break;
                        }
                        if !picked.contains(&cand) {
                            picked.push(cand);
                        }
                    }
                    picked
                };
                let destroyable: Vec<ObjectId> = chosen_objs
                    .into_iter()
                    .filter(|id| {
                        evaluated.get(*id).is_some_and(|o| {
                            !o.has_keyword(crate::characteristics::KeywordAbility::Indestructible)
                        })
                    })
                    .collect();
                let before_controllers: Vec<SeatId> = destroyable
                    .iter()
                    .filter_map(|&oid| {
                        self.window
                            .cards()
                            .get(oid)
                            .and_then(|o| o.controller().or(Some(o.owner())))
                    })
                    .collect();
                let moved = if destroyable.is_empty() {
                    0
                } else {
                    self.move_objects_to_subject_destination(
                        entry,
                        index,
                        entry.controller(),
                        &destroyable,
                        SubjectDestination::Graveyard,
                        ctx.append,
                    )?
                };
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.last_action_succeeded = moved > 0;
                    cur.results.last_action_count = Some(RulesInteger::from_usize(moved));
                    cur.results.last_moved_objects = destroyable;
                    cur.results.last_moved_controllers = before_controllers;
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::CastLinkedFaceDownFree => {
                let source_id = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    StackKind::CardSpell(id) => Some(*id),
                    _ => None,
                };
                if let Some(src) = source_id {
                    let linked: Vec<ObjectId> = self
                        .linked_exiles
                        .get(&src)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .chain(
                            self.characteristics
                                .imprinted_cards
                                .get(&src)
                                .cloned()
                                .unwrap_or_default(),
                        )
                        .collect();
                    if let Some(card_id) = linked.into_iter().find(|id| {
                        self.window
                            .cards()
                            .get(*id)
                            .is_some_and(|c| c.zone() == ObjectZone::Exile)
                    }) {
                        self.card_play_permissions
                            .insert(super::CardPlayPermission {
                                zone: super::CardPlayZone::Exile,
                                object: card_id,
                                player: entry.controller(),
                                duration: super::CardPlayDuration::ThroughTurn,
                                any_color: false,
                                free: true,
                                cast_only: false,
                            });
                        self.free_exiled_cards.insert((card_id, entry.controller()));
                    }
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::Ripple(n) => {
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
            Instruction::Demonstrate => {
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
            Instruction::Enlist => {
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
                                    self.object_display_label(*id, &format!("Creature {}", i + 1))
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
            other => Ok(StepOutcome::Next(other)),
        }
    }
}
