//! Iterative resolution and combat-origin damage driver, split into stack-isolated phases.
use super::*;

pub(super) struct RuleStepContext<'a> {
    pub(super) journal: &'a mut DamageJournal,
    pub(super) draws: &'a mut DrawJournal,
    pub(super) append: &'a mut RulesJournal,
}

// Pending(EffectDecision) holds decision frames while Next(Instruction) shrinks during Toolbox convergence.
#[allow(clippy::large_enum_variant)]
pub(super) enum StepOutcome {
    Continue,
    ProgressLookTop,
    Pending(EffectDecision),
    Complete(RuleCompletion),
    Next(Instruction),
}

impl EffectWindow {
    /// One iterative driver serves resolution and combat-origin damage. Returning
    /// from one damage instruction never recursively completes the whole program.
    #[inline(never)]
    pub(super) fn progress_rules(
        mut self,
        mut journal: DamageJournal,
        declarations: Vec<CombatDeclarationRecord>,
        assignments: Vec<CombatAssignmentRecord>,
        mut draws: DrawJournal,
        mut append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        loop {
            self.settle_departed_payer(&mut append)?;
            if let Some(frame) = self.pending_life_replacement_decision() {
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame,
                        journal,
                        declarations,
                        assignments,
                        draws,
                    },
                    append,
                ));
            }
            if self.random_resolution_pending() {
                let frame = self
                    .pending_resolution_decision()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame,
                        journal,
                        declarations,
                        assignments,
                        draws,
                    },
                    append,
                ));
            }
            if let Some(frame) = self.pending_mana_color().cloned() {
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame: EffectDecision::ManaColor(frame),
                        journal,
                        declarations,
                        assignments,
                        draws,
                    },
                    append,
                ));
            }
            if matches!(self.pending_work, Some(PendingWork::ManaSacrifice(_)))
                && self.linked_return_work.is_some()
            {
                self.finish_linked_return_if_ready(&mut append)?;
                if let Some(frame) = self.linked_return_decision()? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                if self.linked_return_work.is_some() {
                    return Err(EffectError::InconsistentProgram);
                }
            }
            if matches!(self.pending_work, Some(PendingWork::ManaSacrifice(_)))
                && self.linked_return_work.is_none()
            {
                match self.advance_sacrifice_mana(&mut append)? {
                    ManaActivationResult::Pending { frame } => {
                        return Ok(RuleProgress::new(
                            self,
                            RuleResult::Pending {
                                checks: CheckJournal::default(),
                                frame: EffectDecision::ManaColor(frame),
                                journal,
                                declarations,
                                assignments,
                                draws,
                            },
                            append,
                        ));
                    }
                    ManaActivationResult::PendingRule { frame } => {
                        return Ok(RuleProgress::new(
                            self,
                            RuleResult::Pending {
                                checks: CheckJournal::default(),
                                frame,
                                journal,
                                declarations,
                                assignments,
                                draws,
                            },
                            append,
                        ));
                    }
                    ManaActivationResult::Complete { .. } => {
                        return self.finish_mana_continuation(
                            journal,
                            declarations,
                            assignments,
                            draws,
                            append,
                        );
                    }
                }
            }
            if let Some(frame) = self.linked_return_decision()? {
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame,
                        journal,
                        declarations,
                        assignments,
                        draws,
                    },
                    append,
                ));
            }
            if matches!(self.pending_work, Some(PendingWork::GraveyardShuffle(_))) {
                if let Some(frame) = self.progress_graveyard_shuffle()? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::GraveyardExile(_))) {
                if let Some(frame) = self.progress_graveyard_exile(&mut append)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::SearchEntry(_))) {
                if let Some(frame) = self.progress_search_entry(&mut append)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::BattlefieldEntry(_))) {
                if let Some(frame) = self.progress_battlefield_entry(&mut append)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::LibraryTop(_))) {
                if let Some(frame) = self.progress_library_top(&mut append)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::ResolutionSelection(_))) {
                if let Some(frame) = self.progress_resolution_selection(&mut append)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::TokenEntry(_))) {
                if let Some(frame) = self.progress_token_entries(&mut append)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::Damage(_))) {
                match self.progress_damage(&mut journal, &mut append)? {
                    DamageProgressStep::Pending(frame) => {
                        return Ok(RuleProgress::new(
                            self,
                            RuleResult::Pending {
                                checks: CheckJournal::default(),
                                frame: frame.into_frame(),
                                journal,
                                declarations,
                                assignments,
                                draws,
                            },
                            append,
                        ));
                    }
                    DamageProgressStep::NextBatch => continue,
                    DamageProgressStep::ResolutionContinued => {
                        #[cfg(test)]
                        characteristic_observations::damage_instruction_completed(&self);
                        continue;
                    }
                    DamageProgressStep::ManaComplete => {
                        return self.finish_mana_continuation(
                            journal,
                            declarations,
                            assignments,
                            draws,
                            append,
                        );
                    }
                    DamageProgressStep::ManaChoice(frame) => {
                        return Ok(RuleProgress::new(
                            self,
                            RuleResult::Pending {
                                checks: CheckJournal::default(),
                                frame: EffectDecision::ManaColor(frame),
                                journal,
                                declarations,
                                assignments,
                                draws,
                            },
                            append,
                        ));
                    }
                    DamageProgressStep::CombatComplete {
                        additional_step_required,
                    } => {
                        return Ok(RuleProgress::new(
                            self,
                            RuleResult::Complete {
                                checks: CheckJournal::default(),
                                completion: RuleCompletion::TurnWork(WorkCompleted::CombatDamage {
                                    additional_step_required,
                                }),
                                journal,
                                declarations,
                                assignments,
                                permanent_entry: None,
                                draws,
                            },
                            append,
                        ));
                    }
                }
            }
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongPhase)?
                .clone();
            let cursor = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?;
            if cursor.entry != entry.id() {
                return Err(EffectError::WrongResolutionFrame);
            }
            let index = cursor.instruction;
            let program = self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::instructions)
                .ok_or(EffectError::InconsistentProgram)?;
            let instruction_count = program.len();
            if index == instruction_count {
                if let Some(frame) = self.begin_rebound_buyback_choice(&entry)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                self.complete_at_cut(
                    &entry,
                    crate::occurrence::EntryCompletion::Resolved,
                    self.entry_exit(&entry, true),
                    &mut append,
                )?;
                self.pending_work = None;
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Complete {
                        checks: CheckJournal::default(),
                        completion: RuleCompletion::Resolution,
                        journal,
                        declarations,
                        assignments,
                        permanent_entry: None,
                        draws,
                    },
                    append,
                ));
            }
            let instruction = program
                .get(index)
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            let next_instruction = program.get(index + 1).cloned();
            if self.mode_target_illegal(entry.id(), index) && instruction.uses_mode_target() {
                self.advance_instruction()?;
                continue;
            }
            if entry.targets().targets().is_empty()
                && self
                    .payloads
                    .get(&entry.id())
                    .and_then(StackPayload::body)
                    .is_some_and(ExecutableBody::is_up_to_one_target)
                && instruction.uses_mode_target()
                && !matches!(instruction, Instruction::BranchCondition { .. })
            {
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = false;
                }
                self.advance_instruction()?;
                continue;
            }
            let mut ctx = RuleStepContext {
                journal: &mut journal,
                draws: &mut draws,
                append: &mut append,
            };
            macro_rules! run_phase {
                ($step:expr) => {
                    match $step? {
                        StepOutcome::Continue => continue,
                        StepOutcome::ProgressLookTop => {
                            return self.progress_look_top(
                                journal,
                                declarations,
                                assignments,
                                draws,
                                append,
                            );
                        }
                        StepOutcome::Pending(frame) => {
                            return Ok(RuleProgress::new(
                                self,
                                RuleResult::Pending {
                                    checks: CheckJournal::default(),
                                    frame,
                                    journal,
                                    declarations,
                                    assignments,
                                    draws,
                                },
                                append,
                            ));
                        }
                        StepOutcome::Complete(completion) => {
                            return Ok(RuleProgress::new(
                                self,
                                RuleResult::Complete {
                                    checks: CheckJournal::default(),
                                    completion,
                                    journal,
                                    declarations,
                                    assignments,
                                    permanent_entry: None,
                                    draws,
                                },
                                append,
                            ));
                        }
                        StepOutcome::Next(next) => next,
                    }
                };
            }
            let instruction = run_phase!(self.progress_phase_1(
                &mut ctx,
                &entry,
                index,
                instruction_count,
                next_instruction.as_ref(),
                instruction,
            ));
            let instruction =
                run_phase!(self.progress_phase_2(&mut ctx, &entry, index, instruction));
            let instruction =
                run_phase!(self.progress_phase_3(&mut ctx, &entry, index, instruction));
            let instruction =
                run_phase!(self.progress_phase_4(&mut ctx, &entry, index, instruction));
            let instruction =
                run_phase!(self.progress_phase_5(&mut ctx, &entry, index, instruction));
            let instruction =
                run_phase!(self.progress_phase_6(&mut ctx, &entry, index, instruction));
            let _ = run_phase!(self.progress_phase_final(&mut ctx, entry, index, instruction,));
        }
    }

    #[inline(never)]
    // Context-passing phase helper extracted from progress_rules to isolate stack frames.
    #[allow(clippy::too_many_arguments)]
    fn progress_phase_1(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        instruction_count: usize,
        next_instruction: Option<&Instruction>,
        mut instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if let Instruction::PromptChoice(spec) = &mut instruction
            && let Some((cost, _, _)) = spec.as_mana_payment_mut()
            && let Some(keyword) = self
                .execution_bindings(entry.id())
                .and_then(|b| b.trigger.as_ref())
                .and_then(|t| t.upkeep_keyword)
        {
            let Some(price) = self.upkeep_price(keyword, cost, entry, index, ctx.append)? else {
                self.set_instruction(index + 2)?;
                return Ok(StepOutcome::Continue);
            };
            *cost = price;
        }
        if let Some((object, controller, reason)) =
            self.keyword_entry_candidate(&instruction, entry)?
        {
            if !self.settled_entry_destinations.contains_key(&object) {
                self.begin_battlefield_entry_replacement(object, controller, reason)?;
                return Ok(StepOutcome::Continue);
            }
            let destination = self.settled_entry_destinations[&object];
            if destination != crate::program::PublicZoneDestination::Battlefield {
                self.settled_entry_destinations.remove(&object);
                let physical = destination
                    .physical(Some(controller))
                    .ok_or(EffectError::InconsistentProgram)?;
                let current = self
                    .window
                    .cards()
                    .get(object)
                    .ok_or(EffectError::UnknownObject)?;
                let remains = matches!(
                    (current.zone(), physical),
                    (ObjectZone::Hand(_), ObjectDestination::Hand)
                        | (ObjectZone::Graveyard(_), ObjectDestination::Graveyard)
                );
                if !remains {
                    let before = self.rules_snapshot(None)?;
                    let (window, exit) = self.window.resolution_move_with_exit(
                        self.window.revision(),
                        object,
                        physical,
                    )?;
                    self.window = window;
                    self.emit_from(
                        crate::occurrence::EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |old, current, controllers| {
                            Ok(vec![exit_occurrence_at(
                                old,
                                current,
                                exit,
                                reason,
                                controllers,
                            )?])
                        },
                    )?;
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::ObjectMutation::Unearth
                        | crate::toolbox::ObjectMutation::Ninjutsu
                        | crate::toolbox::ObjectMutation::ResolveCounterKeyword(
                            crate::program::CounterKeyword::Persist
                                | crate::program::CounterKeyword::Undying
                        )
                )
        ) && let Some(frame) = self.entry_choice_frame()?
        {
            return Ok(StepOutcome::Pending(EffectDecision::EntryChoice(frame)));
        }
        #[cfg(test)]
        characteristic_observations::instruction(
            self,
            "Before",
            entry.id(),
            index,
            Some(&instruction),
        );
        if matches!(instruction, Instruction::OptionalSequence { .. })
            && matches!(
                next_instruction,
                Some(Instruction::MutateObject(spec))
                    if matches!(
                        spec.op,
                        crate::toolbox::ObjectMutation::ResolveTransfer(
                            crate::program::TransferKeyword::Graft(_)
                        )
                    )
            )
            && self.graft_pair(entry.id()).is_none()
        {
            self.set_instruction(index + 2)?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::PlayerMutation::RequireCondition(
                        crate::program::InterveningCondition::CastFromGraveyard
                    )
                )
        ) {
            if entry.cast_from_graveyard {
                self.advance_instruction()?;
            } else {
                self.set_instruction(instruction_count)?;
            }
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::StackOp(spec)
                if matches!(spec.op, crate::toolbox::StackOpKind::CopyCapturedAbility)
        ) {
            let source = self.delayed_ability_source(entry.id())?;
            if !source.entry.targets().targets().is_empty() {
                let frame = EffectDecision::CopyTargets(self.copy_resolution_frame()?);
                return Ok(StepOutcome::Pending(frame));
            }
            self.finish_delayed_ability_copy(entry, &source, vec![], ctx.append)?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::StackOp(spec)
                if matches!(spec.op, crate::toolbox::StackOpKind::CopyOnCast(_))
        ) {
            let Instruction::StackOp(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::StackOpKind::CopyOnCast(kind) = spec.op else {
                unreachable!();
            };
            if let Some(frame) = self.progress_cast_copies(entry, kind, index, ctx.append)? {
                return Ok(StepOutcome::Pending(frame));
            }
            return Ok(StepOutcome::Continue);
        }
        if self.begin_look_top(&instruction, ctx.append)? {
            if let Some(frame) = self.look_top_frame() {
                return Ok(StepOutcome::Pending(frame));
            }
            if matches!(self.pending_work, Some(PendingWork::LookTop(_))) {
                return Ok(StepOutcome::ProgressLookTop);
            }
            return Ok(StepOutcome::Continue);
        }
        if self.begin_library_top(&instruction)? {
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::MutateObject(ref spec) = instruction
            && let crate::toolbox::ObjectMutation::ScheduleArtifactReturn(object) = spec.op
        {
            let StackKind::Emblem { emblem } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            self.triggers
                .delay_emblem_return(*emblem, entry.controller(), object);
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::PlayerMutation::RequireCondition(
                        crate::program::InterveningCondition::ReturnSourceOnBattlefield
                    )
                )
        ) {
            let StackKind::TriggeredAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            if self
                .window
                .cards()
                .get(source.id())
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            {
                self.advance_instruction()?;
            } else {
                self.set_instruction(instruction_count)?;
            }
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ZoneOp(ref spec) = instruction
            && let crate::toolbox::ZoneDest::Delayed(
                crate::toolbox::DelayedZoneAction::ReturnGraveyardCard(object),
            ) = spec.destination
        {
            if let Some(player) = self.delayed_graveyard_return_player(object) {
                self.begin_graveyard_returns(
                    entry,
                    index,
                    player,
                    vec![object],
                    false,
                    ctx.append,
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ZoneOp(ref spec) = instruction
            && let crate::toolbox::ZoneDest::Delayed(
                crate::toolbox::DelayedZoneAction::ReturnExiledTargetCohort {
                    next_end_step,
                    owner_controls,
                },
            ) = spec.destination
        {
            let Some(PendingWork::Resolution(cursor)) = &mut self.pending_work else {
                return Err(EffectError::WrongResolutionFrame);
            };
            let objects = std::mem::take(&mut cursor.impulse.flickered);
            if next_end_step {
                self.delay_spell_return_with_counters(entry, objects)?;
            } else {
                let origin = LinkedReturnOrigin::ChosenExile {
                    controller: (!owner_controls).then(|| entry.controller()),
                    tapped: false,
                    entry: entry.id(),
                    instruction: index,
                };
                self.begin_linked_cohort_return(origin, objects, ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::ZoneOp(spec)
                if matches!(
                    spec.destination,
                    crate::toolbox::ZoneDest::Delayed(
                        crate::toolbox::DelayedZoneAction::ReturnExileWithCounters(_)
                    )
                )
        ) {
            let Instruction::ZoneOp(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::ZoneDest::Delayed(
                crate::toolbox::DelayedZoneAction::ReturnExileWithCounters(objects),
            ) = spec.destination
            else {
                unreachable!();
            };
            self.begin_linked_cohort_return(
                LinkedReturnOrigin::ExileWithCounters {
                    entry: entry.id(),
                    instruction: index,
                },
                objects,
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::ZoneOp(spec)
                if matches!(
                    spec.destination,
                    crate::toolbox::ZoneDest::Delayed(
                        crate::toolbox::DelayedZoneAction::ReturnExile(_)
                    )
                )
        ) {
            let Instruction::ZoneOp(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::ZoneDest::Delayed(crate::toolbox::DelayedZoneAction::ReturnExile(
                objects,
            )) = spec.destination
            else {
                unreachable!();
            };
            self.begin_linked_cohort_return(LinkedReturnOrigin::Exile, objects, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        Ok(StepOutcome::Next(instruction))
    }

    #[inline(never)]
    fn progress_phase_2(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if matches!(
            &instruction,
            Instruction::StackOp(spec)
                if matches!(spec.op, crate::toolbox::StackOpKind::ChooseFoundSpell)
        ) {
            if let Some(frame) = self.found_spell_selection()? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::ZoneOp(spec)
                if matches!(
                    spec.destination,
                    crate::toolbox::ZoneDest::Delayed(
                        crate::toolbox::DelayedZoneAction::PutHandPermanentsOntoBattlefield { .. }
                    )
                )
        ) {
            if let Some(frame) = self.hand_entry_selection()? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(spec.op, crate::toolbox::ObjectMutation::ReturnEarthbentCard)
        ) {
            let binding = self
                .execution_bindings(entry.id())
                .and_then(|bindings| bindings.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let [member] = binding.causes.members() else {
                return Err(EffectError::InconsistentProgram);
            };
            let crate::occurrence::TriggerRelated::ZoneChange { after, .. } = &member.related
            else {
                return Err(EffectError::InconsistentProgram);
            };
            let card_id = after.object.id();
            if self.window.cards().get(card_id).is_some_and(|card| {
                matches!(card.zone(), ObjectZone::Graveyard(_) | ObjectZone::Exile)
            }) {
                self.move_objects_to_subject_destination_ids(
                    entry,
                    index,
                    entry.controller(),
                    &[card_id],
                    crate::program::SubjectDestination::Battlefield {
                        tapped: true,
                        attacking: false,
                        transformed: false,
                        under_owner_control: true,
                        with_counter: None,
                        attach_to_self: false,
                    },
                    ctx.append,
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::ZoneOp(spec)
                if matches!(
                    spec.destination,
                    crate::toolbox::ZoneDest::Delayed(
                        crate::toolbox::DelayedZoneAction::ReturnChosenXGraveyardCreatures
                    )
                )
        ) {
            if self.linked_return_work.is_some() {
                return Err(EffectError::InconsistentProgram);
            }
            let player = entry.controller();
            let objects = self
                .legal_permanent_targets()?
                .into_iter()
                .collect::<Vec<_>>();
            if !objects.is_empty() {
                self.linked_return_work = Some(LinkedReturnWork {
                    library_bottom: None,
                    origin: LinkedReturnOrigin::Graveyard {
                        player: Some(player),
                        controller: None,
                        subtype_counters: None,
                        entry_additions: crate::program::EntryAdditions::default(),
                        entry: entry.id(),
                        instruction: index,
                        tapped: false,
                        forest: false,
                        corpse: false,
                    },
                    wake_entry: Some(entry.id()),
                    objects,
                    orders: BTreeMap::new(),
                    graveyard_orders: BTreeMap::new(),
                    choices: BTreeMap::new(),
                    auras: BTreeMap::new(),
                    replacements: None,
                });
                self.finish_linked_return_if_ready(ctx.append)?;
            } else {
                self.delay_spell_sacrifice(entry.id(), player, vec![])?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::PlayerMutation::SacrificeRandomUntilLand
                )
        ) && self
            .resolution_cursor()
            .is_some_and(|cursor| cursor.random_selection.is_none())
        {
            let objects = self.random_move_candidates(&instruction)?;
            match objects.as_slice() {
                [] => self.advance_instruction()?,
                [only] => {
                    self.resolution_cursor_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .random_selection = Some(*only);
                }
                _ => {
                    let cursor = self
                        .resolution_cursor()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .clone();
                    self.pending_work =
                        Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
                            cursor,
                            chooser: entry.controller(),
                            options: BTreeSet::new(),
                            rebound_or_buyback: false,
                            cast: None,
                        })));
                }
            }
            return Ok(StepOutcome::Continue);
        }

        if let Instruction::DrawCards(spec) = &instruction
            && let crate::program::ValueExpr::PlayerStat(
                crate::program::PlayerStatExpr::CardsShortOfHandSize(required),
            ) = &spec.amount
            && self
                .window
                .cards()
                .iter()
                .filter(|o| o.zone() == ObjectZone::Hand(entry.controller()))
                .count()
                >= usize::from(*required)
        {
            self.complete_at_cut(
                entry,
                crate::occurrence::EntryCompletion::InterveningIfFalse,
                None,
                ctx.append,
            )?;
            self.payloads.remove(&entry.id());
            self.pending_work = None;
            return Ok(StepOutcome::Complete(RuleCompletion::Resolution));
        }
        let instruction = match self.progress_toolbox_instruction(ctx, entry, index, instruction)? {
            StepOutcome::Next(rest) => rest,
            other => return Ok(other),
        };
        let instruction = match self.progress_pass_c_instruction(ctx, entry, index, instruction)? {
            StepOutcome::Next(rest) => rest,
            other => return Ok(other),
        };
        if matches!(
            &instruction,
            Instruction::MutatePlayer(spec)
                if matches!(spec.op, crate::toolbox::PlayerMutation::OfferAnyPlayer { .. })
        ) {
            let Instruction::MutatePlayer(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::PlayerSelector::Group(players) = spec.players else {
                return Err(EffectError::InconsistentProgram);
            };
            let crate::toolbox::PlayerMutation::OfferAnyPlayer {
                cost_body,
                paid_body,
                declined_body,
            } = spec.op
            else {
                unreachable!();
            };
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
                    cur.results.last_action_succeeded = false;
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
                    cur.results.loop_player = None;
                }
                if let Some(declined) = declined_body
                    && !declined.instructions.is_empty()
                {
                    self.splice_replacing_current_op(entry.id(), index, &declined.instructions)?;
                } else {
                    self.advance_instruction()?;
                }
                return Ok(StepOutcome::Continue);
            };
            let chosen = self
                .resolution_cursor_mut()
                .and_then(|c| c.chosen_option_index.take());
            let Some(idx) = chosen else {
                let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                    revision: self.revision,
                    entry: entry.id(),
                    instruction: index,
                    chooser: actor,
                    count: 2,
                    labels: vec!["Accept".to_owned(), "Decline".to_owned()],
                });
                return Ok(StepOutcome::Pending(frame));
            };
            if idx == 0 {
                let mut combined = cost_body.instructions;
                if !paid_body.instructions.is_empty() {
                    let combined_end = combined.len() + 1 + paid_body.instructions.len();
                    combined.push(Instruction::BranchCondition {
                        condition: Box::new(crate::toolbox::ConditionExpr::Resolution(Box::new(
                            crate::program::ResolutionCondition {
                                left: ValueExpr::event(
                                    crate::program::EventValueKind::LastActionSucceeded,
                                ),
                                comparison: crate::program::Comparison::GreaterOrEqual,
                                right: ValueExpr::literal(1),
                            },
                        ))),
                        destination: combined_end,
                    });
                    let shift = isize::try_from(combined.len())
                        .map_err(|_| EffectError::InconsistentProgram)?;
                    for mut op in paid_body.instructions {
                        op.shift_jumps(shift, 0);
                        combined.push(op);
                    }
                }
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.pending_option_players.clear();
                    cur.results.loop_player = Some(actor);
                    cur.chosen_opponent = Some(actor);
                }
                if combined.is_empty() {
                    if let Some(cur) = self.resolution_cursor_mut() {
                        cur.results.option_players_initialized = false;
                        cur.results.loop_player = None;
                    }
                    self.advance_instruction()?;
                } else {
                    if let Some(StackPayload::Instructions(payload)) =
                        self.payloads.get_mut(&entry.id())
                        && let Some(Instruction::MutatePlayer(spec)) =
                            payload.body.instructions.get_mut(index)
                        && let crate::toolbox::PlayerMutation::OfferAnyPlayer {
                            declined_body, ..
                        } = &mut spec.op
                    {
                        *declined_body = None;
                    }
                    self.splice_before_current_op(entry.id(), index, &combined)?;
                }
                return Ok(StepOutcome::Continue);
            }
            if let Some(cur) = self.resolution_cursor_mut() {
                cur.results.pending_option_players.pop_front();
            }
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(spec.op, crate::toolbox::ObjectMutation::CreateTokenCopy(_))
        ) {
            let Instruction::MutateObject(spec) = instruction else {
                unreachable!();
            };
            let source = spec
                .source
                .to_move_subject()
                .ok_or(EffectError::InconsistentProgram)?;
            let crate::toolbox::ObjectMutation::CreateTokenCopy(copy) = spec.op else {
                unreachable!();
            };
            let crate::toolbox::TokenCopySpec {
                count,
                recipients,
                enters_tapped,
                haste,
                nonlegendary,
                extra_types,
                extra_subtypes,
                extra_keywords,
                override_pt,
                extra_colors,
                override_colors,
                strip_mana_cost,
                extra_activated,
                extra_triggered,
                extra_static,
                extra_mana,
                extra_counters,
                keep_name,
                ..
            } = *copy;
            let resolved_count = self
                .resolve_value_expr(entry.controller(), &count)?
                .max(0.into())
                .to_usize()
                .unwrap_or(0);
            let donor_id = self.resolve_pass_c_move_subject(entry, source)?;
            if resolved_count > 0
                && let Some(donor) = donor_id
                && let Some(mut program) = self
                    .copiable_program(donor)
                    .or_else(|| self.departed_copy_sources.get(&donor))
                    .cloned()
                && program.permanent().is_some()
            {
                if nonlegendary {
                    program
                        .supertypes
                        .remove(&crate::program::Supertype::Legendary);
                }
                if haste {
                    program.printed_keywords.insert(KeywordAbility::Haste);
                }
                program
                    .printed_keywords
                    .extend(extra_keywords.iter().copied());
                program.card_types.extend(extra_types.iter().copied());
                program
                    .creature_subtypes
                    .extend(extra_subtypes.iter().copied());
                if let Some((p, t)) = override_pt
                    && let crate::program::ProgramKind::Permanent(perm) = &mut program.kind
                {
                    perm.printed_pt = Some(crate::characteristics::PrintedPowerToughness {
                        power: crate::characteristics::PrintedStat::Number(p),
                        toughness: crate::characteristics::PrintedStat::Number(t),
                    });
                    let is_pt_cda = |ab: &crate::characteristics::StaticContinuousAbility| {
                        matches!(
                            ab,
                            crate::characteristics::StaticContinuousAbility::CharacteristicDefining {
                                parts,
                                ..
                            } if parts.0.iter().any(|part| matches!(
                                part,
                                crate::characteristics::DefiningPart::PowerToughness { .. }
                            ))
                        )
                    };
                    if program.continuous.as_ref().is_some_and(is_pt_cda) {
                        program.continuous = None;
                    }
                    program
                        .additional_battlefield_abilities
                        .retain(|ab| !is_pt_cda(ab));
                }
                if let Some(colors) = override_colors {
                    program.color_indicator = Some(colors.clone());
                    program.effect_defined_colors = Some(colors);
                } else if !extra_colors.is_empty() {
                    let mut merged = program.defined_colors();
                    merged.extend(extra_colors.iter().copied());
                    program.color_indicator = Some(merged.clone());
                    program.effect_defined_colors = Some(merged);
                }
                if strip_mana_cost {
                    if let crate::program::ProgramKind::Permanent(perm) = &mut program.kind {
                        perm.casting_cost = None;
                    }
                    program.x_mana_symbols = 0;
                    program.hybrid_symbols.clear();
                }
                if keep_name {
                    let source_obj = match entry.kind() {
                        StackKind::CardSpell(id) => Some(*id),
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => Some(source.id()),
                        _ => None,
                    };
                    if let Some(src_names) = source_obj
                        .and_then(|src| self.copiable_program(src))
                        .map(|prog| prog.names.clone())
                    {
                        program.names = src_names;
                    }
                }
                for mut ability in extra_activated {
                    let next = program
                        .activated
                        .iter()
                        .try_fold(0_u64, |candidate, a| {
                            a.id.0.checked_add(1).map(|n| n.max(candidate))
                        })
                        .ok_or(EffectError::InconsistentProgram)?;
                    ability.id = crate::activated::ActivatedAbilityId(next);
                    program.activated.push(ability);
                }
                for mut mana_ability in extra_mana {
                    let next = program
                        .mana
                        .iter()
                        .try_fold(0_u64, |candidate, m| {
                            m.id.0.checked_add(1).map(|n| n.max(candidate))
                        })
                        .ok_or(EffectError::InconsistentProgram)?;
                    mana_ability.id = crate::mana::ManaAbilityId(next);
                    program.mana.push(mana_ability);
                }
                for mut trigger in extra_triggered {
                    let next = program
                        .triggered
                        .iter()
                        .try_fold(0_u64, |candidate, t| {
                            t.id.0.checked_add(1).map(|n| n.max(candidate))
                        })
                        .ok_or(EffectError::InconsistentProgram)?;
                    trigger.id = crate::program::TriggeredAbilityId(next);
                    program.triggered.push(trigger);
                }
                for extra_stat in extra_static {
                    let next = program
                        .continuous
                        .iter()
                        .chain(&program.additional_battlefield_abilities)
                        .try_fold(0_u64, |candidate, ab| {
                            ab.id().0.checked_add(1).map(|n| n.max(candidate))
                        })
                        .ok_or(EffectError::InconsistentProgram)?;
                    program.additional_battlefield_abilities.push(
                        extra_stat.with_component_id(crate::characteristics::StaticAbilityId(next)),
                    );
                }
                for (kind, count) in extra_counters {
                    if count > 0 {
                        program
                            .entry_counter_abilities
                            .push(super::entry_copy::fixed_entry_counter(kind, count));
                    }
                }
                let controller = entry.controller();
                let mut groups = Vec::new();
                use crate::token::TokenRecipients;
                if recipients == TokenRecipients::DestroyedPermanentControllers {
                    let mut counts: BTreeMap<SeatId, usize> = BTreeMap::new();
                    if let Some(cursor) = self.resolution_cursor() {
                        for &seat in &cursor.results.last_moved_controllers {
                            *counts.entry(seat).or_insert(0) += 1;
                        }
                    }
                    for (creator, mult) in counts {
                        let total = resolved_count.saturating_mul(mult);
                        if total > 0 {
                            groups.push((creator, total, program.clone(), enters_tapped));
                        }
                    }
                } else {
                    let prior_controller = match recipients {
                        TokenRecipients::CombatDamageSourceController => {
                            self.combat_damage_source_controller()?
                        }
                        TokenRecipients::RememberedTargetController => Some(
                            self.resolution_cursor()
                                .and_then(|cursor| cursor.remembered_permanent.as_ref())
                                .ok_or(EffectError::InconsistentProgram)?
                                .0,
                        ),
                        TokenRecipients::ExiledTargetController => Some(
                            self.resolution_cursor()
                                .and_then(|cursor| cursor.exiled_target.as_ref())
                                .ok_or(EffectError::InconsistentProgram)?
                                .controller,
                        ),
                        TokenRecipients::Player(role) => Some(self.resolving_player(role)?),
                        TokenRecipients::Controller
                        | TokenRecipients::EachPlayer
                        | TokenRecipients::EachOpponent
                        | TokenRecipients::DestroyedPermanentControllers => None,
                    };
                    for creator in
                        self.window
                            .participants()
                            .iter()
                            .copied()
                            .filter(|seat| match recipients {
                                TokenRecipients::CombatDamageSourceController
                                | TokenRecipients::RememberedTargetController
                                | TokenRecipients::ExiledTargetController
                                | TokenRecipients::Player(_) => Some(*seat) == prior_controller,
                                TokenRecipients::Controller => *seat == controller,
                                TokenRecipients::EachPlayer => true,
                                TokenRecipients::EachOpponent => *seat != controller,
                                TokenRecipients::DestroyedPermanentControllers => false,
                            })
                    {
                        groups.push((creator, resolved_count, program.clone(), enters_tapped));
                    }
                }
                if !groups.is_empty() {
                    self.begin_materialized_token_entries(groups, None, false)?;
                    return Ok(StepOutcome::Continue);
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutatePlayer(spec)
                if matches!(spec.op, crate::toolbox::PlayerMutation::BindResolutionX(_))
        ) {
            let Instruction::MutatePlayer(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::PlayerMutation::BindResolutionX(expr) = spec.op else {
                unreachable!();
            };
            let value = self
                .resolve_value_expr(entry.controller(), &expr)?
                .max(0.into());
            self.window.set_resolving_chosen_x(value)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::ObjectMutation::CreateAuraTokenAttached { .. }
                )
        ) {
            let Instruction::MutateObject(spec) = instruction else {
                unreachable!();
            };
            let host = spec
                .source
                .to_move_subject()
                .ok_or(EffectError::InconsistentProgram)?;
            let crate::toolbox::ObjectMutation::CreateAuraTokenAttached {
                definition,
                up_to_one,
            } = spec.op
            else {
                unreachable!();
            };
            if up_to_one
                && host == crate::program::MoveSubjectRef::TargetObject
                && self.instruction_target(entry).is_err()
            {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            self.begin_materialized_token_entries(
                vec![(entry.controller(), 1, definition.program().clone(), false)],
                None,
                false,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ScheduleDelayedEventTrigger {
            trigger,
            body,
            expires,
            once,
        } = instruction
        {
            if let Some(resolved_expires) = self.resolve_duration(entry, expires)? {
                let snapshot = self.rules_snapshot(None)?;
                let source_id = match entry.kind() {
                    StackKind::CardSpell(id) => *id,
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => source.id(),
                    _ => return Err(EffectError::InconsistentProgram),
                };
                let appearance = snapshot
                    .view()
                    .appearance(source_id)
                    .or_else(|| self.departed_reflexive_sources.get(&source_id).cloned())
                    .or_else(|| {
                        self.execution_bindings(entry.id())
                            .and_then(|b| b.trigger.as_ref())
                            .map(|t| t.source.clone())
                    })
                    .ok_or(EffectError::InconsistentProgram)?;
                self.triggers.delay_event_trigger(
                    appearance,
                    entry.controller(),
                    *trigger,
                    *body,
                    resolved_expires,
                    once,
                );
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if self.begin_resolution_selection(&instruction, ctx.append)? {
            return Ok(StepOutcome::Continue);
        }
        if self.begin_resolution_choice(&instruction, ctx.append)? {
            // An unavailable free cast (for example, a missing target) simply finishes.
            let Some(frame) = self.pending_resolution_decision() else {
                return Ok(StepOutcome::Continue);
            };
            return Ok(StepOutcome::Pending(frame));
        }
        if self.progress_control(entry, &instruction, ctx.append)? {
            #[cfg(test)]
            characteristic_observations::instruction(self, "Completed", entry.id(), index, None);
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::SearchLibrary { filter } = &instruction {
            let searcher = match filter.searcher {
                crate::program::ResolvingPlayer::Controller => entry.controller(),
                other => self.resolving_player(other)?,
            };
            if self.player_cant_search_libraries(searcher)? {
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = false;
                    cursor.results.last_action_count = Some(0.into());
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let frame = EffectDecision::LibraryTop(self.search_frame(&instruction)?);
            return Ok(StepOutcome::Pending(frame));
        }
        Ok(StepOutcome::Next(instruction))
    }

    #[inline(never)]
    fn progress_phase_3(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if matches!(
            &instruction,
            Instruction::MutatePlayer(spec)
                if matches!(spec.op, crate::toolbox::PlayerMutation::ExtortDrain)
        ) {
            self.drain_opponents(entry.id(), index, entry.controller(), &1.into(), ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutatePlayer(spec)
                if matches!(spec.op, crate::toolbox::PlayerMutation::RequireCondition(_))
        ) {
            let Instruction::MutatePlayer(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::PlayerMutation::RequireCondition(cond) = spec.op else {
                unreachable!();
            };
            use crate::program::InterveningCondition as IC;
            let holds = match cond {
                IC::Monarch { opponent } => self.characteristics.monarch.is_some_and(|monarch| {
                    if opponent {
                        monarch != entry.controller()
                    } else {
                        monarch == entry.controller()
                    }
                }),
                IC::NoOtherControlledCreatures => {
                    let cut = self.rules_snapshot(None)?;
                    let StackKind::TriggeredAbility { source } = entry.kind() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    !cut.view()
                        .evaluated
                        .has_other_controlled_creatures(entry.controller(), source.id())
                }
                IC::ControlledCreatureCount(count) => {
                    let cut = self.rules_snapshot(None)?;
                    cut.view()
                        .evaluated
                        .controlled_creature_count(entry.controller())
                        == usize::from(count)
                }
                IC::SourceLoyalty => self.loyalty_transfer_source(entry)?.is_some(),
                IC::OpponentLifeLoss(n) => self
                    .rules_snapshot(None)?
                    .view()
                    .opponent_lost_life(entry.controller(), n),
                IC::SourceQuestCounters(n) => {
                    let StackKind::TriggeredAbility { source } = entry.kind() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let count = if self
                        .window
                        .cards()
                        .get(source.id())
                        .is_some_and(|o| o.zone() == ObjectZone::Battlefield)
                    {
                        self.counters_of_kind(source.id(), CounterKind::Quest)
                    } else {
                        self.departed_quest_counts
                            .get(&source.id())
                            .cloned()
                            .unwrap_or_else(|| 0.into())
                    };
                    count >= i64::from(n).into()
                }
                IC::NoLoyaltyActivation => !self
                    .characteristics
                    .loyalty_activators
                    .contains(&entry.controller()),
                IC::OwnCommander => self
                    .rules_snapshot(None)?
                    .view()
                    .controls_own_commander(entry.controller()),
                IC::GreatestArtifact => self
                    .rules_snapshot(None)?
                    .view()
                    .controls_greatest_artifact(entry.controller()),
                IC::OpponentMoreLands => self
                    .rules_snapshot(None)?
                    .view()
                    .opponent_has_more_lands(entry.controller()),
                IC::CastFromGraveyard | IC::ReturnSourceOnBattlefield => {
                    return Err(EffectError::InconsistentProgram);
                }
            };
            if !holds {
                self.complete_at_cut(
                    entry,
                    crate::occurrence::EntryCompletion::InterveningIfFalse,
                    None,
                    ctx.append,
                )?;
                self.pending_work = None;
                return Ok(StepOutcome::Complete(RuleCompletion::Resolution));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(spec.op, crate::toolbox::ObjectMutation::ResolveTransfer(_))
        ) {
            let Instruction::MutateObject(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::ObjectMutation::ResolveTransfer(key) = spec.op else {
                unreachable!();
            };
            self.resolve_transfer(key, entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ScheduleDelayedStepTrigger {
            step,
            own_step_only,
            body,
        } = instruction
        {
            let snapshot = self.rules_snapshot(None)?;
            let source_id = match entry.kind() {
                StackKind::CardSpell(id) => *id,
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    source.id()
                }
                _ => return Err(EffectError::InconsistentProgram),
            };
            let appearance = snapshot
                .view()
                .appearance(source_id)
                .or_else(|| self.departed_reflexive_sources.get(&source_id).cloned())
                .or_else(|| {
                    self.execution_bindings(entry.id())
                        .and_then(|b| b.trigger.as_ref())
                        .map(|t| t.source.clone())
                })
                .ok_or(EffectError::InconsistentProgram)?;
            let mut materialized_body = body;
            if let Some(created) = self
                .resolution_cursor()
                .map(|c| c.last_created_tokens.clone())
                && !created.is_empty()
            {
                materialize_created_tokens_in_body(&mut materialized_body, &created);
            }
            self.triggers.delay_step_body(
                appearance,
                entry.controller(),
                step,
                own_step_only,
                materialized_body,
            );
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::EmitReflexiveTrigger { body } = instruction {
            let snapshot = self.rules_snapshot(None)?;
            let source_id = match entry.kind() {
                StackKind::CardSpell(id) => *id,
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    source.id()
                }
                _ => return Err(EffectError::InconsistentProgram),
            };
            let appearance = snapshot
                .view()
                .appearance(source_id)
                .or_else(|| self.departed_reflexive_sources.get(&source_id).cloned())
                .or_else(|| {
                    self.execution_bindings(entry.id())
                        .and_then(|b| b.trigger.as_ref())
                        .map(|t| t.source.clone())
                })
                .ok_or(EffectError::InconsistentProgram)?;
            let before_waiting: BTreeSet<_> = self.triggers.waiting().keys().copied().collect();
            let reflexive_ctx =
                self.resolution_cursor()
                    .map(|cursor| super::ReflexiveTriggerContext {
                        env: cursor.env.clone(),
                        last_sacrificed_creature: cursor.results.last_sacrificed_creature.clone(),
                        last_sacrificed_mv: cursor.results.last_sacrificed_mv.clone(),
                        resolution_x: entry.chosen_x().cloned(),
                        discarded_types: cursor.results.discarded_types.clone(),
                        exiled_target: cursor.exiled_target.clone(),
                        last_moved_objects: cursor.results.last_moved_objects.clone(),
                        last_moved_controllers: cursor.results.last_moved_controllers.clone(),
                        last_action_count: cursor.results.last_action_count.clone(),
                    });
            self.emit_from(
                crate::occurrence::EventCause::Instruction {
                    entry: entry.id(),
                    instruction: index,
                },
                &snapshot,
                None,
                ctx.append,
                |_, _, _| {
                    Ok(vec![crate::occurrence::RulesOccurrence::ReflexiveTrigger {
                        source: Box::new(appearance.clone()),
                        controller: entry.controller(),
                        body: body.clone(),
                        chosen_x: entry.chosen_x().cloned(),
                    }])
                },
            )?;
            if let Some(captured) = reflexive_ctx {
                let new_ids: Vec<_> = self
                    .triggers
                    .waiting()
                    .keys()
                    .copied()
                    .filter(|id| !before_waiting.contains(id))
                    .collect();
                for tid in new_ids {
                    self.pending_reflexive_context.insert(tid, captured.clone());
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::PlayerMutation::RevealTopTwoPiles { .. }
                )
        ) {
            let player = entry.controller();
            if let Some(frame) =
                self.progress_revealed_piles(entry.id(), index, player, ctx.append)?
            {
                return Ok(StepOutcome::Pending(frame));
            }
            if self.random_resolution_pending() {
                return Ok(StepOutcome::Continue);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(spec.op, crate::toolbox::ObjectMutation::Myriad)
        ) {
            if let Some(frame) = self.progress_myriad(entry)? {
                return Ok(StepOutcome::Pending(EffectDecision::MyriadDefender(frame)));
            }
            return Ok(StepOutcome::Continue);
        }
        Ok(StepOutcome::Next(instruction))
    }

    #[inline(never)]
    fn progress_phase_4(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if let Instruction::ZoneOp(ref spec) = instruction
            && let crate::toolbox::ZoneDest::Delayed(
                crate::toolbox::DelayedZoneAction::ReturnGraveyardToHand(object),
            ) = spec.destination
        {
            if self
                .window
                .cards()
                .get(object)
                .is_none_or(|card| card.zone() != ObjectZone::Graveyard(card.owner()))
            {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            if !self.settled_entry_destinations.contains_key(&object) {
                self.begin_hand_return_replacement(object, entry.controller())?;
                return Ok(StepOutcome::Continue);
            }
            let destination = self
                .settled_entry_destinations
                .remove(&object)
                .ok_or(EffectError::InconsistentProgram)?;
            if destination != crate::program::PublicZoneDestination::Graveyard {
                let physical = destination
                    .physical(None)
                    .ok_or(EffectError::InconsistentProgram)?;
                let before = self.rules_snapshot(None)?;
                let (window, exit) = self.window.resolution_move_with_exit(
                    self.window.revision(),
                    object,
                    physical,
                )?;
                self.window = window;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |old, current, controllers| {
                        Ok(vec![exit_occurrence_at(
                            old,
                            current,
                            exit,
                            crate::occurrence::ZoneReason::ReturnToOwnersHand,
                            controllers,
                        )?])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        Ok(StepOutcome::Next(instruction))
    }

    #[inline(never)]
    fn progress_phase_5(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(spec.op, crate::toolbox::ObjectMutation::Ninjutsu)
        ) {
            if let Some((object, _, _)) = self.keyword_entry_candidate(&instruction, entry)? {
                self.settled_entry_destinations.remove(&object);
            }
            self.resolve_ninjutsu(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::StackOp(spec)
                if matches!(spec.op, crate::toolbox::StackOpKind::EndStepCast { .. })
        ) {
            let Instruction::StackOp(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::StackOpKind::EndStepCast {
                kind,
                permanent,
                turn,
            } = spec.op
            else {
                unreachable!();
            };
            self.resolve_end_step_cast(entry, index, kind, permanent, turn, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::MutateObject(ref spec) = instruction
            && let crate::toolbox::ObjectMutation::ResolveGrowth(ref keyword) = spec.op
        {
            self.resolve_growth(keyword, entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::ObjectMutation::ResolveCounterKeyword(_)
                )
        ) {
            if let Some((object, _, _)) = self.keyword_entry_candidate(&instruction, entry)? {
                self.settled_entry_destinations.remove(&object);
            }
            let Instruction::MutateObject(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::ObjectMutation::ResolveCounterKeyword(keyword) = spec.op else {
                unreachable!();
            };
            self.resolve_counter_keyword(keyword, entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::MutateObject(ref spec) = instruction
            && let crate::toolbox::ObjectMutation::ResolveKeywordTrigger(
                crate::program::CombatTriggerKeyword::Mobilize(amount),
            ) = &spec.op
        {
            let warrior = crate::program::mobilize_warrior(&self.creature_vocabulary)
                .map_err(|_| EffectError::InconsistentProgram)?;
            let mut creation = crate::token::TokenCreation::one(warrior);
            creation.amount = ValueExpr::Literal(amount.clone());
            creation.enters_tapped = true;
            self.begin_token_entries(&[creation], None, true)?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::MutateObject(ref spec) = instruction
            && let crate::toolbox::ObjectMutation::ResolveKeywordTrigger(keyword) = &spec.op
        {
            self.resolve_keyword_trigger(keyword, entry.id(), index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::ObjectMutation::Unearth
                        | crate::toolbox::ObjectMutation::ExileUnearthed(_)
                )
        ) {
            if let Some((object, _, _)) = self.keyword_entry_candidate(&instruction, entry)? {
                self.settled_entry_destinations.remove(&object);
            }
            self.resolve_unearth(entry, index, &instruction, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::ObjectMutation::RememberPermanentTarget
                )
        ) {
            let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            let controller = self
                .current_characteristics(object)?
                .and_then(|view| view.controller)
                .ok_or(EffectError::InconsistentProgram)?;
            let value = self
                .permanent_mana_value(object)
                .ok_or(EffectError::InconsistentProgram)?;
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .remembered_permanent = Some((controller, value));
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        Ok(StepOutcome::Next(instruction))
    }

    #[inline(never)]
    fn progress_phase_6(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::ObjectMutation::ResolvePowerAbility(_)
                )
        ) {
            let Instruction::MutateObject(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::ObjectMutation::ResolvePowerAbility(kind) = spec.op else {
                unreachable!();
            };
            self.resolve_power_ability(entry, kind, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            &instruction,
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::ObjectMutation::CreateTokenAndAttach { .. }
                )
        ) {
            let Instruction::MutateObject(spec) = instruction else {
                unreachable!();
            };
            let crate::toolbox::ObjectMutation::CreateTokenAndAttach { definition } = spec.op
            else {
                unreachable!();
            };
            if self
                .resolution_cursor()
                .is_some_and(|cursor| cursor.chosen_token_equipment.is_some())
            {
                if let Some(frame) = self.token_equipment_selection()? {
                    return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                        frame,
                    )));
                }
                let cursor = self
                    .resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                cursor.chosen_token_equipment = None;
                cursor.last_created_tokens.clear();
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let StackKind::TriggeredAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            self.begin_token_entries(
                &[crate::token::TokenCreation::one(*definition)],
                Some(source.id()),
                false,
            )?;
            return Ok(StepOutcome::Continue);
        }
        // CollectEvidence and Blight are handled interactively via `begin_resolution_selection`.
        if matches!(
            &instruction,
            Instruction::StackOp(spec) if matches!(spec.op, crate::toolbox::StackOpKind::Encore)
        ) {
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let definition = source
                .base_program(&self.programs)
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), entry.controller())?;
            let groups = opponents
                .iter()
                .map(|_| (entry.controller(), 1, definition.clone(), false))
                .collect();
            self.begin_materialized_token_entries(groups, None, false)?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateTokens { creations } = instruction {
            self.begin_token_entries(&creations, None, false)?;
            return Ok(StepOutcome::Continue);
        }
        Ok(StepOutcome::Next(instruction))
    }

    #[inline(never)]
    fn progress_phase_final(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: crate::stack_window::StackEntry,
        index: usize,
        instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        let _ = index;
        let bound = self.instruction_target(&entry)?;
        let target = bound.choice().target;
        if !self.operation_available(&instruction, target)? {
            self.advance_instruction()?;
            #[cfg(test)]
            characteristic_observations::instruction(
                self,
                "SkippedUnavailable",
                entry.id(),
                index,
                None,
            );
            return Ok(StepOutcome::Continue);
        }
        if let Some(frame) = self.graveyard_aura_entry_frame()? {
            if frame.domain.is_empty() {
                self.settled_entry_destinations.remove(&frame.object);
                self.prospective_entry_copies.remove(&frame.object);
                self.prospective_entry_controllers.remove(&frame.object);
                self.prospective_devour_counters.remove(&frame.object);
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            return Ok(StepOutcome::Pending(EffectDecision::LinkedAuraChoice(
                frame,
            )));
        }
        if let Some(frame) = self.entry_choice_frame()? {
            return Ok(StepOutcome::Pending(EffectDecision::EntryChoice(frame)));
        }
        match instruction {
            Instruction::StackOp(ref spec) if spec.is_interactive_copy_or_retarget() => {
                let is_change_only = matches!(
                    spec.op,
                    crate::toolbox::StackOpKind::Retarget {
                        single_target_only: true
                    }
                );
                let TargetRef::Stack(original) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let source = self
                    .window
                    .entries()
                    .iter()
                    .find(|spell| spell.id() == original)
                    .ok_or(EffectError::InconsistentProgram)?;
                if is_change_only && !self.can_change_only_target(source)? {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                if source.targets().targets().is_empty() {
                    #[cfg(test)]
                    let copying_entry = entry.id();
                    let controller = entry.controller();
                    self.apply_instruction(
                        entry,
                        instruction,
                        Some((controller, Vec::new())),
                        ctx.journal,
                        ctx.append,
                    )?;
                    self.advance_instruction()?;
                    #[cfg(test)]
                    characteristic_observations::instruction(
                        self,
                        "Completed",
                        copying_entry,
                        index,
                        None,
                    );
                    return Ok(StepOutcome::Continue);
                }
                return Ok(StepOutcome::Pending(EffectDecision::CopyTargets(
                    self.copy_resolution_frame()?,
                )));
            }
            other => {
                #[cfg(test)]
                let observed_entry = entry.id();
                self.apply_instruction(entry, other, None, ctx.journal, ctx.append)?;
                self.advance_instruction()?;
                #[cfg(test)]
                characteristic_observations::instruction(
                    self,
                    "Completed",
                    observed_entry,
                    index,
                    None,
                );
            }
        }
        Ok(StepOutcome::Continue)
    }

    pub(super) fn apply_turn_prevention_spec(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        spec: &crate::damage::TurnPreventionSpec,
    ) -> Result<StepOutcome, EffectError> {
        use crate::damage::{
            ActiveTurnPrevention, ActiveTurnPreventionKind, DamageSourceId,
            PreventionRecipientSpec, PreventionSourceSpec, ResolvedPreventionRecipient,
            ResolvedPreventionSource,
        };
        let ability_source_id = match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                Some(source.id())
            }
            _ => None,
        };
        let resolved_source = match &spec.source {
            PreventionSourceSpec::Any => ResolvedPreventionSource::Any,
            PreventionSourceSpec::AbilitySource => {
                let id = ability_source_id.ok_or(EffectError::InconsistentProgram)?;
                ResolvedPreventionSource::ExactSources(BTreeSet::from([DamageSourceId::Object(id)]))
            }
            PreventionSourceSpec::TargetPermanent => {
                let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                ResolvedPreventionSource::ExactSources(BTreeSet::from([DamageSourceId::Object(
                    target,
                )]))
            }
            PreventionSourceSpec::TargetSpell => {
                let TargetRef::Stack(target_entry_id) =
                    self.instruction_target(entry)?.choice().target
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let mut sources = BTreeSet::new();
                if let Some(target_entry) = self
                    .window
                    .entries()
                    .iter()
                    .find(|e| e.id() == target_entry_id)
                {
                    match target_entry.kind() {
                        StackKind::CardSpell(obj_id) => {
                            sources.insert(DamageSourceId::Object(*obj_id));
                        }
                        StackKind::SpellCopy { .. } => {
                            sources.insert(DamageSourceId::SpellCopy(target_entry_id));
                        }
                        _ => return Err(EffectError::InconsistentProgram),
                    }
                }
                if sources.is_empty() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                ResolvedPreventionSource::ExactSources(sources)
            }
            PreventionSourceSpec::AllPermanentTargets => {
                let selected: BTreeSet<DamageSourceId> = self
                    .legal_permanent_targets()?
                    .into_iter()
                    .map(DamageSourceId::Object)
                    .collect();
                if selected.is_empty() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                ResolvedPreventionSource::ExactSources(selected)
            }
            PreventionSourceSpec::ChosenSource(filter) => {
                let Some(chosen) = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_damage_source.take())
                else {
                    let sources = self.legal_damage_sources(*filter)?;
                    if sources.is_empty() {
                        self.advance_instruction()?;
                        return Ok(StepOutcome::Continue);
                    }
                    let frame = EffectDecision::DamageSourceChoice(DamageSourceChoiceFrame {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: entry.controller(),
                        sources,
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                ResolvedPreventionSource::ExactSources(BTreeSet::from([chosen]))
            }
            PreventionSourceSpec::ChosenColor => {
                let Some(mana_type) = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_effect_color.take())
                else {
                    let frame = EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: entry.controller(),
                        colors: mana_domains::colored_mana(),
                        split: None,
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                let color = mana_type
                    .color()
                    .ok_or(EffectError::SelectionOutsideDomain)?;
                ResolvedPreventionSource::Color(color)
            }
            PreventionSourceSpec::Filter(filter) => ResolvedPreventionSource::Filter(*filter),
            PreventionSourceSpec::CreaturesOtherThanTarget => {
                let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                ResolvedPreventionSource::CreaturesOtherThan(target)
            }
            PreventionSourceSpec::CreaturesOtherThanAbilitySource => {
                let id = ability_source_id.ok_or(EffectError::InconsistentProgram)?;
                ResolvedPreventionSource::CreaturesOtherThan(id)
            }
        };
        let resolved_recipient = match spec.recipient {
            PreventionRecipientSpec::Any => ResolvedPreventionRecipient::Any,
            PreventionRecipientSpec::Controller => ResolvedPreventionRecipient::Controller,
            PreventionRecipientSpec::ControllerAndControlledCreatures => {
                ResolvedPreventionRecipient::ControllerAndControlledCreatures
            }
            PreventionRecipientSpec::ControllerAndControlledPermanents => {
                ResolvedPreventionRecipient::ControllerAndControlledPermanents
            }
            PreventionRecipientSpec::ControlledCreatures => {
                ResolvedPreventionRecipient::ControlledCreatures
            }
            PreventionRecipientSpec::AllPlayers => ResolvedPreventionRecipient::AllPlayers,
            PreventionRecipientSpec::AllCreatures => ResolvedPreventionRecipient::AllCreatures,
            PreventionRecipientSpec::AbilitySource => {
                let id = ability_source_id.ok_or(EffectError::InconsistentProgram)?;
                ResolvedPreventionRecipient::ExactTarget(TargetRef::Object(id))
            }
            PreventionRecipientSpec::Target => {
                let target = self.instruction_target(entry)?.choice().target;
                ResolvedPreventionRecipient::ExactTarget(target)
            }
        };
        let before = self.rules_snapshot(None)?;
        self.damage_state.turn_preventions.insert(
            (entry.id(), index),
            ActiveTurnPrevention {
                controller: entry.controller(),
                kind: ActiveTurnPreventionKind::Spec {
                    combat_only: spec.combat_only,
                    source: resolved_source,
                    recipient: resolved_recipient,
                    both_directions: spec.both_directions,
                },
            },
        );
        self.emit_from(
            crate::occurrence::EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            },
            &before,
            None,
            ctx.append,
            |_, _, _| {
                Ok(vec![
                    crate::occurrence::RulesOccurrence::TurnPreventionSpecCreated {
                        creator: entry.id(),
                        instruction: index,
                        controller: entry.controller(),
                        spec: Box::new(spec.clone()),
                    },
                ])
            },
        )?;
        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }

    pub(super) fn install_temporary_rule(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        rule_val: crate::program::TemporaryRuleKind,
        duration: crate::characteristics::ResolvedDuration,
    ) -> Result<(), EffectError> {
        let rule = Box::new(rule_val);
        use crate::program::TemporaryRuleKind;
        if let Some(resolved_dur) = self.resolve_duration(entry, duration)? {
            let source_object = match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    Some(source.id())
                }
                StackKind::CardSpell(id) => Some(*id),
                _ => None,
            };
            let bound_target = self
                .instruction_target(entry)
                .ok()
                .map(|t| t.choice().target);
            let second_bound_target = entry.targets().targets().get(1).map(|t| t.choice().target);
            let source_damage = self.resolution_damage_source(entry).ok();
            let source_entry = Some(entry.id());
            let mut rule = *rule;
            // CR 611.2c: lock ChosenX eagerly when creating a temporary static rule.
            if let TemporaryRuleKind::TemporaryStatic(ability)
            | TemporaryRuleKind::ConditionalTemporaryStatic { ability, .. } = &mut rule
                && let Ok(resolved_x) =
                    self.resolve_value_expr(entry.controller(), &ValueExpr::chosen_x())
            {
                substitute_chosen_x_in_static(ability, &resolved_x);
            }
            match rule {
                TemporaryRuleKind::DamageCantBePrevented => {
                    self.damage_state.prevention_disallowed_this_turn = true;
                }
                TemporaryRuleKind::PreventAllCombatDamage => {
                    self.damage_state.turn_preventions.insert(
                        (entry.id(), index),
                        crate::damage::ActiveTurnPrevention {
                            controller: entry.controller(),
                            kind: crate::damage::ActiveTurnPreventionKind::Legacy {
                                filter: crate::damage::TurnPreventionFilter::Combat,
                                protected_source: None,
                            },
                        },
                    );
                }
                TemporaryRuleKind::ExileIfDies { subject } => {
                    let affected = self.effect_group(entry, *subject)?;
                    for id in affected {
                        self.damage_exile_until_cleanup
                            .entry(id)
                            .or_default()
                            .insert((entry.id(), index));
                    }
                }
                TemporaryRuleKind::ExileIfLeavesBattlefield { subject } => {
                    let affected: BTreeSet<ObjectId> =
                        self.effect_group(entry, *subject)?.into_iter().collect();
                    if !affected.is_empty() {
                        let id = self.next_temp_rule_id;
                        self.next_temp_rule_id = self.next_temp_rule_id.saturating_add(1);
                        self.temporary_rules.push(super::ActiveTemporaryRule {
                            id,
                            controller: entry.controller(),
                            source_object,
                            source_damage: source_damage.clone(),
                            source_entry,
                            rule: TemporaryRuleKind::ExileIfLeavesBattlefield { subject },
                            duration: resolved_dur,
                            resolved_players: BTreeSet::new(),
                            resolved_objects: affected,
                            target: bound_target,
                            second_target: second_bound_target,
                            consumed: false,
                            spell_cast_baselines: BTreeMap::new(),
                        });
                    }
                }
                TemporaryRuleKind::PreventNextDamageExpr {
                    amount,
                    source,
                    recipient,
                    combat_only,
                } => {
                    let resolved = self.resolve_value_expr(entry.controller(), &amount)?;
                    if resolved.is_positive() {
                        let id = self.next_temp_rule_id;
                        self.next_temp_rule_id = self.next_temp_rule_id.saturating_add(1);
                        self.temporary_rules.push(super::ActiveTemporaryRule {
                            id,
                            controller: entry.controller(),
                            source_object,
                            source_damage: source_damage.clone(),
                            source_entry,
                            rule: TemporaryRuleKind::PreventNextDamage {
                                amount: resolved,
                                source,
                                recipient,
                                combat_only,
                            },
                            duration: resolved_dur,
                            resolved_players: BTreeSet::new(),
                            resolved_objects: BTreeSet::new(),
                            target: bound_target,
                            second_target: second_bound_target,
                            consumed: false,
                            spell_cast_baselines: BTreeMap::new(),
                        });
                    }
                }
                TemporaryRuleKind::SpellCostReductionExpr {
                    filter,
                    amount,
                    next_only,
                } => {
                    let resolved = self.resolve_value_expr(entry.controller(), &amount)?;
                    if resolved.is_positive() {
                        let generic = resolved
                            .to_usize()
                            .map_or(u16::MAX, |v| u16::try_from(v).unwrap_or(u16::MAX));
                        let id = self.next_temp_rule_id;
                        self.next_temp_rule_id = self.next_temp_rule_id.saturating_add(1);
                        self.temporary_rules.push(super::ActiveTemporaryRule {
                            id,
                            controller: entry.controller(),
                            source_object,
                            source_damage: source_damage.clone(),
                            source_entry,
                            rule: TemporaryRuleKind::SpellCostReduction {
                                filter,
                                generic,
                                next_only,
                            },
                            duration: resolved_dur,
                            resolved_players: BTreeSet::new(),
                            resolved_objects: BTreeSet::new(),
                            target: None,
                            second_target: None,
                            consumed: false,
                            spell_cast_baselines: BTreeMap::new(),
                        });
                    }
                }
                TemporaryRuleKind::TemporaryStatic(ability)
                    if matches!(
                        ability.as_ref(),
                        crate::characteristics::StaticContinuousAbility::Battlefield { .. }
                    ) =>
                {
                    self.characteristics.add_temporary_static(
                        entry.controller(),
                        source_object,
                        resolved_dur,
                        *ability,
                    )?;
                }
                TemporaryRuleKind::RedirectNextDamageExpr {
                    source,
                    source_is_target,
                    from,
                    to,
                    combat_only,
                    amount,
                    one_shot,
                } => {
                    let resolved = self.resolve_value_expr(entry.controller(), &amount)?;
                    if resolved.is_positive() {
                        let resolved_objects = match bound_target {
                            Some(TargetRef::Object(obj)) => BTreeSet::from([obj]),
                            _ => BTreeSet::new(),
                        };
                        let id = self.next_temp_rule_id;
                        self.next_temp_rule_id = self.next_temp_rule_id.saturating_add(1);
                        self.temporary_rules.push(super::ActiveTemporaryRule {
                            id,
                            controller: entry.controller(),
                            source_object,
                            source_damage: source_damage.clone(),
                            source_entry,
                            rule: TemporaryRuleKind::RedirectNextDamage {
                                source,
                                source_is_target,
                                from,
                                to,
                                combat_only,
                                amount: Some(resolved),
                                one_shot,
                            },
                            duration: resolved_dur,
                            resolved_players: BTreeSet::new(),
                            resolved_objects,
                            target: bound_target,
                            second_target: second_bound_target,
                            consumed: false,
                            spell_cast_baselines: BTreeMap::new(),
                        });
                    }
                }
                TemporaryRuleKind::RedirectNextDamage {
                    source,
                    source_is_target,
                    from,
                    to,
                    combat_only,
                    amount,
                    one_shot,
                } => {
                    let resolved_objects = match bound_target {
                        Some(TargetRef::Object(obj)) => BTreeSet::from([obj]),
                        _ => BTreeSet::new(),
                    };
                    let id = self.next_temp_rule_id;
                    self.next_temp_rule_id = self.next_temp_rule_id.saturating_add(1);
                    self.temporary_rules.push(super::ActiveTemporaryRule {
                        id,
                        controller: entry.controller(),
                        source_object,
                        source_damage: source_damage.clone(),
                        source_entry,
                        rule: TemporaryRuleKind::RedirectNextDamage {
                            source,
                            source_is_target,
                            from,
                            to,
                            combat_only,
                            amount,
                            one_shot,
                        },
                        duration: resolved_dur,
                        resolved_players: BTreeSet::new(),
                        resolved_objects,
                        target: bound_target,
                        second_target: second_bound_target,
                        consumed: false,
                        spell_cast_baselines: BTreeMap::new(),
                    });
                }
                other => {
                    let cursor = self.resolution_cursor().cloned();
                    let resolved_players: BTreeSet<SeatId> = match &other {
                        TemporaryRuleKind::PlayerCantGainLife { players }
                        | TemporaryRuleKind::CantCastSpells { players, .. }
                        | TemporaryRuleKind::CantCastFromZone { players, .. }
                        | TemporaryRuleKind::CantActivateAbilities { players, .. }
                        | TemporaryRuleKind::CantPlayLands { players }
                        | TemporaryRuleKind::PlayerGrantKeyword { players, .. }
                        | TemporaryRuleKind::SpellCostIncrease { players, .. }
                        | TemporaryRuleKind::CastMaxMoreSpellsThisTurn { players, .. }
                        | TemporaryRuleKind::ExileSpellOnResolve { players, .. } => self
                            .selection_players(*players, entry.controller(), cursor.as_ref())?
                            .into_iter()
                            .collect(),
                        TemporaryRuleKind::AdditionalLandPlay { player, .. }
                        | TemporaryRuleKind::FilteredFlash { player, .. }
                        | TemporaryRuleKind::ZoneCastPermission { player, .. }
                        | TemporaryRuleKind::NextSpellAffinity { player, .. } => {
                            [self.resolving_player_identity_at(*player, cursor.as_ref())?]
                                .into_iter()
                                .collect()
                        }
                        _ => BTreeSet::new(),
                    };
                    let resolved_objects = match &other {
                        crate::program::TemporaryRuleKind::ZoneCastPermission {
                            zone: crate::program::SourceZoneFilter::LastMoved,
                            ..
                        } => cursor
                            .as_ref()
                            .map(|c| {
                                c.results
                                    .last_moved_objects
                                    .iter()
                                    .copied()
                                    .collect::<BTreeSet<_>>()
                            })
                            .unwrap_or_default(),
                        _ => BTreeSet::new(),
                    };
                    let spell_cast_baselines =
                        if matches!(other, TemporaryRuleKind::CastMaxMoreSpellsThisTurn { .. }) {
                            resolved_players
                                .iter()
                                .map(|&seat| {
                                    (
                                        seat,
                                        self.characteristics
                                            .spells_cast_this_turn
                                            .get(&seat)
                                            .copied()
                                            .unwrap_or(0),
                                    )
                                })
                                .collect()
                        } else {
                            BTreeMap::new()
                        };
                    let id = self.next_temp_rule_id;
                    self.next_temp_rule_id = self.next_temp_rule_id.saturating_add(1);
                    self.temporary_rules.push(super::ActiveTemporaryRule {
                        id,
                        controller: entry.controller(),
                        source_object,
                        source_damage,
                        source_entry,
                        rule: other,
                        duration: resolved_dur,
                        resolved_players,
                        resolved_objects,
                        target: bound_target,
                        second_target: second_bound_target,
                        consumed: false,
                        spell_cast_baselines,
                    });
                }
            }
        }
        Ok(())
    }

    pub(super) fn apply_combat_action(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        affected: &[ObjectId],
        kind: crate::program::CombatActionKind,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        use crate::program::CombatActionKind;
        match kind {
            CombatActionKind::MustAttack => {
                for &object in affected {
                    let count = self.creatures_required_to_attack.entry(object).or_default();
                    *count = count
                        .checked_add(1)
                        .ok_or(EffectError::InconsistentProgram)?;
                }
            }
            CombatActionKind::MustBlock => {
                for &object in affected {
                    let count = self.must_block_this_turn.entry(object).or_default();
                    *count = count
                        .checked_add(1)
                        .ok_or(EffectError::InconsistentProgram)?;
                }
            }
            CombatActionKind::MustBeBlocked => {
                for &object in affected {
                    let count = self.must_be_blocked_this_turn.entry(object).or_default();
                    *count = count
                        .checked_add(1)
                        .ok_or(EffectError::InconsistentProgram)?;
                }
            }
            CombatActionKind::Goad => {
                let goader = entry.controller();
                for &object in affected {
                    self.goaded.entry(object).or_default().insert(goader);
                }
            }
            CombatActionKind::RemoveFromCombat | CombatActionKind::UntapAndRemoveFromCombat => {
                if kind == CombatActionKind::UntapAndRemoveFromCombat {
                    let changing: Vec<ObjectId> = affected
                        .iter()
                        .copied()
                        .filter(|id| {
                            self.window.cards().get(*id).and_then(GameObject::tapped) == Some(true)
                        })
                        .collect();
                    if !changing.is_empty() {
                        let before = self.rules_snapshot(None)?;
                        let occurrences = self.untap_for_effect_with_stun(entry.id(), &changing)?;
                        self.emit_from(
                            crate::occurrence::EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            &before,
                            None,
                            append,
                            |_, _, _| Ok(occurrences),
                        )?;
                    }
                }
                if let Some(combat) = &mut self.combat {
                    combat.reconcile(|combatant, _| !affected.contains(&combatant.object()));
                }
            }
            CombatActionKind::BecomeBlocked => {
                if let Some(combat) = &mut self.combat {
                    for &object in affected {
                        combat.mark_attacker_blocked(object);
                    }
                }
            }
            CombatActionKind::PreventCombatDamageByAndTo => {
                for &object in affected {
                    let id = self.next_temp_rule_id;
                    self.next_temp_rule_id = self.next_temp_rule_id.saturating_add(1);
                    self.temporary_rules.push(super::ActiveTemporaryRule {
                        id,
                        controller: entry.controller(),
                        source_object: Some(object),
                        source_damage: None,
                        source_entry: None,
                        target: None,
                        second_target: None,
                        rule: crate::program::TemporaryRuleKind::TemporaryStatic(Box::new(
                            crate::characteristics::StaticContinuousAbility::DamageRule {
                                id: crate::characteristics::StaticAbilityId(id),
                                rule: crate::characteristics::StaticDamageRule::PreventSourceCombat,
                            },
                        )),
                        duration: crate::characteristics::ResolvedDuration::UntilEndOfTurn,
                        resolved_players: BTreeSet::new(),
                        resolved_objects: BTreeSet::from([object]),
                        consumed: false,
                        spell_cast_baselines: BTreeMap::new(),
                    });
                }
            }
        }
        Ok(())
    }

    pub(in crate::effects) fn execute_add_mana(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        recipient: SeatId,
        mana_yield: &crate::program::EffectManaYield,
    ) -> Result<StepOutcome, EffectError> {
        use crate::program::EffectManaYield;
        let (inner_yield, retain_until_cleanup) = match mana_yield {
            EffectManaYield::RetainedUntilCleanup(inner) => (inner.as_ref(), true),
            other => (other, false),
        };
        let mana_lot_source = retain_until_cleanup
            .then(|| {
                self.execution_bindings(entry.id())
                    .and_then(|bindings| bindings.trigger.as_ref())
                    .map(|t| t.source.clone())
                    .or_else(|| {
                        let sid = match entry.kind() {
                            StackKind::ActivatedAbility { source }
                            | StackKind::TriggeredAbility { source } => Some(source.id()),
                            StackKind::CardSpell(id) => Some(*id),
                            _ => None,
                        }?;
                        self.rules_snapshot(None)
                            .ok()
                            .and_then(|snap| snap.view().appearance(sid))
                    })
            })
            .flatten();
        let produced: BTreeMap<_, _> = match inner_yield {
            EffectManaYield::RetainedUntilCleanup(_) => {
                return Err(EffectError::InconsistentProgram);
            }
            EffectManaYield::RestrictedChoice {
                colors,
                amount,
                restriction,
            } => {
                let qty = self.resolve_value_expr(entry.controller(), amount)?;
                if !qty.is_positive() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let color = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.chosen_effect_color);
                let Some(color) = color else {
                    let frame = EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: recipient,
                        colors: colors.clone(),
                        split: None,
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                if !colors.contains(&color) {
                    return Err(EffectError::InconsistentProgram);
                }
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_effect_color = None;
                let before_snapshot = self.rules_snapshot(None)?;
                let player = recipient;
                let prior = self.mana(player, color);
                let after = prior.plus(&qty);
                self.mana.insert((player, color), after.clone());
                let key = (player, color, restriction.clone());
                let previous = self
                    .restricted_mana
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| 0.into());
                self.restricted_mana.insert(key, previous.plus(&qty));
                if let Some(source) = mana_lot_source {
                    let retained: BTreeMap<_, _> = [(color, qty.clone())].into();
                    self.record_mana_lots(
                        source,
                        player,
                        &retained,
                        crate::mana::ManaSpendEffect::RetainUntilCleanup,
                    )?;
                }
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before_snapshot,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ManaAddedByEffect {
                                entry: entry.id(),
                                player,
                                produced: [(color, qty.clone())].into(),
                                before: [(color, prior)].into(),
                                after: [(color, after)].into(),
                            },
                        ])
                    },
                )?;
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            EffectManaYield::RestrictedFixed {
                bundle,
                restriction,
            } => {
                let before_snapshot = self.rules_snapshot(None)?;
                let player = recipient;
                let mut before_map = BTreeMap::new();
                let mut after_map = BTreeMap::new();
                for (&color, amount) in bundle {
                    let prior = self.mana(player, color);
                    let after = prior.plus(amount);
                    self.mana.insert((player, color), after.clone());
                    let key = (player, color, restriction.clone());
                    let previous = self
                        .restricted_mana
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| 0.into());
                    self.restricted_mana.insert(key, previous.plus(amount));
                    before_map.insert(color, prior);
                    after_map.insert(color, after);
                }
                if let Some(source) = mana_lot_source {
                    self.record_mana_lots(
                        source,
                        player,
                        bundle,
                        crate::mana::ManaSpendEffect::RetainUntilCleanup,
                    )?;
                }
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before_snapshot,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ManaAddedByEffect {
                                entry: entry.id(),
                                player,
                                produced: bundle.clone(),
                                before: before_map,
                                after: after_map,
                            },
                        ])
                    },
                )?;
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            EffectManaYield::RestrictedCounted {
                kind,
                amount,
                restriction,
            } => {
                let qty = self.resolve_value_expr(entry.controller(), amount)?;
                if !qty.is_positive() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let before_snapshot = self.rules_snapshot(None)?;
                let player = recipient;
                let prior = self.mana(player, *kind);
                let after = prior.plus(&qty);
                self.mana.insert((player, *kind), after.clone());
                let key = (player, *kind, restriction.clone());
                let previous = self
                    .restricted_mana
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| 0.into());
                self.restricted_mana.insert(key, previous.plus(&qty));
                if let Some(source) = mana_lot_source {
                    let retained: BTreeMap<_, _> = [(*kind, qty.clone())].into();
                    self.record_mana_lots(
                        source,
                        player,
                        &retained,
                        crate::mana::ManaSpendEffect::RetainUntilCleanup,
                    )?;
                }
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before_snapshot,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ManaAddedByEffect {
                                entry: entry.id(),
                                player,
                                produced: [(*kind, qty.clone())].into(),
                                before: [(*kind, prior)].into(),
                                after: [(*kind, after)].into(),
                            },
                        ])
                    },
                )?;
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            EffectManaYield::Choice { colors, amount } => {
                let resolved = self.resolve_value_expr(entry.controller(), amount)?;
                if !resolved.is_positive() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let color = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.chosen_effect_color);
                let Some(color) = color else {
                    let frame = EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: recipient,
                        colors: colors.clone(),
                        split: None,
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                if !colors.contains(&color) {
                    return Err(EffectError::InconsistentProgram);
                }
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_effect_color = None;
                [(color, resolved)].into()
            }
            EffectManaYield::ChoiceBundle { options } => {
                let chosen = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_option_index.take());
                let Some(idx) = chosen else {
                    let count = u8::try_from(options.len())
                        .map_err(|_| EffectError::InconsistentProgram)?;
                    let labels = options
                        .iter()
                        .map(|bundle| {
                            let mut s = String::from("Add ");
                            for (mana_type, qty) in bundle {
                                let sym = match mana_type {
                                    crate::mana::ManaType::White => "{W}",
                                    crate::mana::ManaType::Blue => "{U}",
                                    crate::mana::ManaType::Black => "{B}",
                                    crate::mana::ManaType::Red => "{R}",
                                    crate::mana::ManaType::Green => "{G}",
                                    crate::mana::ManaType::Colourless => "{C}",
                                };
                                let n = qty.to_usize().unwrap_or(1);
                                for _ in 0..n {
                                    s.push_str(sym);
                                }
                            }
                            s
                        })
                        .collect();
                    let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: recipient,
                        count,
                        labels,
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                options
                    .get(usize::from(idx))
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram)?
            }
            EffectManaYield::LandProducible { controller, amount } => {
                let resolved = self.resolve_value_expr(entry.controller(), amount)?;
                if !resolved.is_positive() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let players: BTreeSet<SeatId> = self
                    .selection_players(*controller, recipient, Some(cursor))?
                    .into_iter()
                    .collect();
                let colors = self.land_colors_for_players(&players)?;
                if colors.is_empty() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let color = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.chosen_effect_color);
                let Some(color) = color else {
                    let frame = EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: recipient,
                        colors,
                        split: None,
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                if !colors.contains(&color) {
                    return Err(EffectError::InconsistentProgram);
                }
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_effect_color = None;
                [(color, resolved)].into()
            }
            EffectManaYield::CommanderIdentity { amount } => {
                let resolved = self.resolve_value_expr(entry.controller(), amount)?;
                if !resolved.is_positive() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let colors = self.commander_mana_colors(recipient)?;
                if colors.is_empty() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let color = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.chosen_effect_color);
                let Some(color) = color else {
                    let frame = EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: recipient,
                        colors,
                        split: None,
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                if !colors.contains(&color) {
                    return Err(EffectError::InconsistentProgram);
                }
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_effect_color = None;
                [(color, resolved)].into()
            }
            EffectManaYield::SourceChosenColor { amount } => {
                let resolved = self.resolve_value_expr(entry.controller(), amount)?;
                let source_id = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                };
                let chosen = source_id.and_then(|id| self.entry_color_choices.get(&id).copied());
                if !resolved.is_positive() || chosen.is_none() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let Some(color) = chosen else {
                    return Err(EffectError::InconsistentProgram);
                };
                [(color, resolved)].into()
            }
            EffectManaYield::DistinctColors { amount } => {
                let resolved = self
                    .resolve_value_expr(entry.controller(), amount)?
                    .max(0.into())
                    .min(5.into());
                let needed = resolved.to_usize().unwrap_or(0);
                if needed == 0 {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                if let Some(color) = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_effect_color.take())
                {
                    self.resolution_cursor_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .chosen_distinct_colors
                        .push(color);
                }
                let picked = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_distinct_colors
                    .clone();
                if picked.len() < needed {
                    let remaining: BTreeSet<ManaType> = mana_domains::colored_mana()
                        .into_iter()
                        .filter(|c| !picked.contains(c))
                        .collect();
                    let frame = EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: recipient,
                        colors: remaining,
                        split: None,
                    });
                    return Ok(StepOutcome::Pending(frame));
                }
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_distinct_colors
                    .clear();
                picked.into_iter().map(|c| (c, 1.into())).collect()
            }
            EffectManaYield::RetainedUntilEndOfCombat { kind, amount } => {
                let resolved = self.resolve_value_expr(entry.controller(), amount)?;
                if !resolved.is_positive() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let produced: BTreeMap<_, _> = [(*kind, resolved)].into();
                let source = self
                    .execution_bindings(entry.id())
                    .and_then(|bindings| bindings.trigger.as_ref())
                    .map(|t| t.source.clone())
                    .or_else(|| {
                        let sid = match entry.kind() {
                            StackKind::ActivatedAbility { source }
                            | StackKind::TriggeredAbility { source } => Some(source.id()),
                            StackKind::CardSpell(id) => Some(*id),
                            _ => None,
                        }?;
                        self.rules_snapshot(None)
                            .ok()
                            .and_then(|snap| snap.view().appearance(sid))
                    });
                if let Some(source) = source {
                    self.record_mana_lots(
                        source,
                        recipient,
                        &produced,
                        crate::mana::ManaSpendEffect::RetainUntilEndOfCombat,
                    )?;
                }
                produced
            }
            EffectManaYield::CombatDamageUntilCleanup(kind) => {
                let damage = self.resolve_value_expr(
                    entry.controller(),
                    &ValueExpr::event(EventValueKind::CombatDamageToPlayer),
                )?;
                let source = self
                    .execution_bindings(entry.id())
                    .and_then(|bindings| bindings.trigger.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?
                    .source
                    .clone();
                let produced = [(*kind, damage)].into();
                self.record_mana_lots(
                    source,
                    recipient,
                    &produced,
                    crate::mana::ManaSpendEffect::RetainUntilCleanup,
                )?;
                produced
            }
            EffectManaYield::Counted { kind, amount } => {
                let resolved = self.resolve_value_expr(entry.controller(), amount)?;
                if !resolved.is_positive() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                [(*kind, resolved)].into()
            }
            EffectManaYield::Split {
                first,
                second,
                amount,
            } => {
                let total = self.resolve_value_expr(entry.controller(), amount)?;
                if !total.is_positive() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let Some(portion) = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_mana_split.take())
                else {
                    let frame = EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: recipient,
                        colors: [*first, *second].into(),
                        split: Some((*first, *second, total)),
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                let remainder = total.minus(&portion);
                [(*first, portion), (*second, remainder)]
                    .into_iter()
                    .filter(|(_, amount)| amount.is_positive())
                    .collect()
            }
            EffectManaYield::Fixed(produced) => produced.clone(),
        };
        if let Some(source) = mana_lot_source
            && !produced.is_empty()
        {
            self.record_mana_lots(
                source,
                recipient,
                &produced,
                crate::mana::ManaSpendEffect::RetainUntilCleanup,
            )?;
        }
        let before_snapshot = self.rules_snapshot(None)?;
        let player = recipient;
        let before: BTreeMap<_, _> = produced
            .keys()
            .map(|kind| (*kind, self.mana(player, *kind)))
            .collect();
        for (kind, amount) in &produced {
            let current = self.mana(player, *kind);
            self.mana.insert((player, *kind), current.plus(amount));
        }
        let after: BTreeMap<_, _> = produced
            .keys()
            .map(|kind| (*kind, self.mana(player, *kind)))
            .collect();
        self.emit_from(
            crate::occurrence::EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            },
            &before_snapshot,
            None,
            ctx.append,
            |_, _, _| {
                Ok(vec![
                    crate::occurrence::RulesOccurrence::ManaAddedByEffect {
                        entry: entry.id(),
                        player,
                        produced,
                        before,
                        after,
                    },
                ])
            },
        )?;
        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }

    fn splice_before_current_op(
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

    fn splice_replacing_current_op(
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

    pub(super) fn remove_counters_from_objects(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        objects: &[ObjectId],
        kind: Option<CounterKind>,
        amount: Option<&ValueExpr>,
        append: &mut RulesJournal,
    ) -> Result<RulesInteger, EffectError> {
        let per_object_cap = match amount {
            Some(expr) => {
                let cap = self.resolve_value_expr(entry.controller(), expr)?;
                if !cap.is_positive() {
                    return Ok(0.into());
                }
                Some(cap)
            }
            None => None,
        };
        let before_snapshot = self.rules_snapshot(None)?;
        let mut total_removed = RulesInteger::from(0);
        let mut occurrences = Vec::new();
        for &object in objects {
            if !self
                .window
                .cards()
                .get(object)
                .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
            {
                continue;
            }
            let kinds: Vec<CounterKind> = match kind {
                Some(ck) => vec![ck],
                None => self
                    .characteristics
                    .present_permanent_counter_kinds(object)
                    .into_iter()
                    .collect(),
            };
            let mut remaining = per_object_cap.clone();
            for ck in kinds {
                if remaining.as_ref().is_some_and(|r| !r.is_positive()) {
                    break;
                }
                let current_count = self.counters_of_kind(object, ck);
                if !current_count.is_positive() {
                    continue;
                }
                let to_remove = match &remaining {
                    Some(r) if r < &current_count => r.clone(),
                    _ => current_count.clone(),
                };
                if !to_remove.is_positive() {
                    continue;
                }
                match ck {
                    CounterKind::PlusOne | CounterKind::MinusOne => {
                        let before = self.characteristics.counters(object);
                        let delta = crate::characteristics::PowerToughnessCounters {
                            plus_one: if ck == CounterKind::PlusOne {
                                to_remove.clone()
                            } else {
                                0.into()
                            },
                            minus_one: if ck == CounterKind::MinusOne {
                                to_remove.clone()
                            } else {
                                0.into()
                            },
                        };
                        let after = self
                            .characteristics
                            .take_power_toughness_counters(object, &delta)
                            .map_err(|_| EffectError::InconsistentProgram)?;
                        occurrences.push(
                            crate::occurrence::RulesOccurrence::PermanentCounterChanged {
                                object,
                                kind: ck,
                                before: if ck == CounterKind::PlusOne {
                                    before.plus_one
                                } else {
                                    before.minus_one
                                },
                                after: if ck == CounterKind::PlusOne {
                                    after.plus_one
                                } else {
                                    after.minus_one
                                },
                            },
                        );
                    }
                    other => {
                        let after = current_count.minus(&to_remove);
                        if let Some(map) = self.characteristics.named_permanent_counters_mut(other)
                        {
                            if after.is_positive() {
                                map.insert(object, after.clone());
                            } else {
                                map.remove(&object);
                                if other.keyword_counter_ability().is_some() {
                                    self.characteristics
                                        .clear_keyword_counter_grant(object, other);
                                }
                            }
                            occurrences.push(if other == CounterKind::Loyalty {
                                crate::occurrence::RulesOccurrence::LoyaltyCountersChanged {
                                    object,
                                    before: current_count,
                                    after,
                                }
                            } else {
                                crate::occurrence::RulesOccurrence::PermanentCounterChanged {
                                    object,
                                    kind: other,
                                    before: current_count,
                                    after,
                                }
                            });
                        }
                    }
                }
                total_removed = total_removed.plus(&to_remove);
                if let Some(r) = &mut remaining {
                    *r = r.minus(&to_remove);
                }
            }
        }
        if !occurrences.is_empty() {
            self.emit_from(
                crate::occurrence::EventCause::Instruction {
                    entry: entry.id(),
                    instruction: index,
                },
                &before_snapshot,
                None,
                append,
                |_, _, _| Ok(occurrences),
            )?;
        }
        Ok(total_removed)
    }

    fn player_cant_search_libraries(&self, player: SeatId) -> Result<bool, EffectError> {
        let relation_matches = |relation: crate::program::PlayerRelation,
                                controller: SeatId|
         -> Result<bool, EffectError> {
            Ok(match relation {
                crate::program::PlayerRelation::Controller => player == controller,
                crate::program::PlayerRelation::Opponent => self
                    .combat_rules
                    .opponents(self.window.participants(), controller)?
                    .contains(&player),
                crate::program::PlayerRelation::AnyParticipant => true,
            })
        };
        let evaluated = self.evaluated_characteristics()?;
        for active in &self.temporary_rules {
            if let Some(ability) = self.active_temporary_static(active, &evaluated)
                && let crate::characteristics::StaticContinuousAbility::Restriction {
                    rule: crate::characteristics::StaticRestriction::PlayersCantSearchLibraries(rel),
                    ..
                } = ability
                && relation_matches(*rel, active.controller)?
            {
                return Ok(true);
            }
        }
        for source in evaluated
            .objects
            .values()
            .filter(|permanent| permanent.zone == ObjectZone::Battlefield)
        {
            let Some(controller) = source.controller else {
                continue;
            };
            if Self::permanent_has_restriction(source, |rule| {
                matches!(
                    rule,
                    crate::characteristics::StaticRestriction::PlayersCantSearchLibraries(rel)
                        if relation_matches(*rel, controller).unwrap_or(false)
                )
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn substitute_chosen_x_in_expr(expr: &mut ValueExpr, resolved_x: &RulesInteger) {
    match expr {
        ValueExpr::EventValue(crate::program::EventValueKind::ChosenX) => {
            *expr = ValueExpr::Literal(resolved_x.clone());
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            substitute_chosen_x_in_expr(inner, resolved_x);
        }
        ValueExpr::Add(left, right) | ValueExpr::Sub(left, right) => {
            substitute_chosen_x_in_expr(left, resolved_x);
            substitute_chosen_x_in_expr(right, resolved_x);
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            substitute_chosen_x_in_expr(left, resolved_x);
            substitute_chosen_x_in_expr(right, resolved_x);
            substitute_chosen_x_in_expr(then_value, resolved_x);
            substitute_chosen_x_in_expr(else_value, resolved_x);
        }
        _ => {}
    }
}

fn substitute_chosen_x_in_condition(
    cond: &mut crate::program::ResolutionCondition,
    resolved_x: &RulesInteger,
) {
    substitute_chosen_x_in_expr(&mut cond.left, resolved_x);
    substitute_chosen_x_in_expr(&mut cond.right, resolved_x);
}

fn substitute_chosen_x_in_numeric_expr(
    expr: &mut crate::characteristics::NumericExpression,
    resolved_x: &RulesInteger,
) {
    if let crate::characteristics::NumericExpression::Eval(inner) = expr {
        substitute_chosen_x_in_expr(inner, resolved_x);
    }
}

fn substitute_chosen_x_in_static(
    ability: &mut crate::characteristics::StaticContinuousAbility,
    resolved_x: &RulesInteger,
) {
    use crate::characteristics::{
        CastPermissionRule, DefiningPart, StaticContinuousAbility, StaticDamageOp,
        StaticDamageRule, StaticModifierRule, StaticPart, StaticReplacementAction,
    };
    match ability {
        StaticContinuousAbility::Battlefield { parts, .. } => {
            for part in &mut parts.0 {
                match part {
                    StaticPart::SetPT(pair) | StaticPart::ModifyPT(pair) => {
                        substitute_chosen_x_in_numeric_expr(&mut pair.power, resolved_x);
                        substitute_chosen_x_in_numeric_expr(&mut pair.toughness, resolved_x);
                    }
                    StaticPart::SetPower(expr) | StaticPart::SetToughness(expr) => {
                        substitute_chosen_x_in_numeric_expr(expr, resolved_x);
                    }
                    _ => {}
                }
            }
        }
        StaticContinuousAbility::CharacteristicDefining { parts, .. } => {
            for part in &mut parts.0 {
                if let DefiningPart::PowerToughness { power, toughness } = part {
                    if let Some(p) = power {
                        substitute_chosen_x_in_numeric_expr(p, resolved_x);
                    }
                    if let Some(t) = toughness {
                        substitute_chosen_x_in_numeric_expr(t, resolved_x);
                    }
                }
            }
        }
        StaticContinuousAbility::DamageRule { rule, .. } => {
            if let StaticDamageRule::ModifyDamage { op, .. }
            | StaticDamageRule::ConditionalModifyDamage { op, .. } = rule
                && let StaticDamageOp::AddExpr(expr) | StaticDamageOp::PreventUpToExpr(expr) = op
            {
                substitute_chosen_x_in_expr(expr, resolved_x);
            }
        }
        StaticContinuousAbility::EventReplacement {
            action: StaticReplacementAction::ModifyAmount { add, .. },
            ..
        } => {
            substitute_chosen_x_in_expr(add, resolved_x);
        }
        StaticContinuousAbility::Modifier { rule, .. } => match rule {
            StaticModifierRule::OthersEnterWithCountersExpr { amount, .. } => {
                substitute_chosen_x_in_expr(amount, resolved_x);
            }
            StaticModifierRule::ConditionalModifyLifeGain { condition, .. } => {
                substitute_chosen_x_in_condition(condition, resolved_x);
            }
            _ => {}
        },
        StaticContinuousAbility::CastPermission {
            permission: CastPermissionRule::GraveyardSelfConditional { condition },
            ..
        } => {
            substitute_chosen_x_in_condition(condition, resolved_x);
        }
        _ => {}
    }
}

fn materialize_created_tokens_in_query(
    query: &mut crate::toolbox::ObjectQuery,
    created: &[ObjectId],
) {
    use crate::toolbox::ObjectQuery;
    match query {
        ObjectQuery::CreatedTokens => *query = ObjectQuery::Fixed(created.to_vec()),
        ObjectQuery::Filtered { source, .. } | ObjectQuery::Take { source, .. } => {
            materialize_created_tokens_in_query(source, created);
        }
        ObjectQuery::Union(qs) => {
            for q in qs {
                materialize_created_tokens_in_query(q, created);
            }
        }
        ObjectQuery::Difference(a, b) | ObjectQuery::Intersect(a, b) => {
            materialize_created_tokens_in_query(a, created);
            materialize_created_tokens_in_query(b, created);
        }
        _ => {}
    }
}

fn materialize_created_tokens_in_body(
    body: &mut crate::program::ExecutableBody,
    created: &[ObjectId],
) {
    for op in &mut body.instructions {
        match op {
            Instruction::ZoneOp(spec) => {
                materialize_created_tokens_in_query(&mut spec.source, created);
            }
            Instruction::MutateObject(spec) => {
                materialize_created_tokens_in_query(&mut spec.source, created);
            }
            Instruction::ApplyContinuous(spec) => {
                if let Some(sel) = &mut spec.selector {
                    materialize_created_tokens_in_query(sel, created);
                }
            }
            Instruction::SetObjectTag { query, .. } => {
                materialize_created_tokens_in_query(query, created);
            }
            Instruction::ForEach { query, body, .. } => {
                materialize_created_tokens_in_query(query, created);
                materialize_created_tokens_in_body(body, created);
            }
            Instruction::ForEachPlayerQuery { body, .. } => {
                materialize_created_tokens_in_body(body, created);
            }
            _ => {}
        }
    }
}
