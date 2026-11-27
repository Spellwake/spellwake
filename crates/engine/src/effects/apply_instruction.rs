//! Atomic single-instruction execution cuts during stack resolution.
use super::*;

impl EffectWindow {
    #[inline(never)]
    pub(super) fn apply_instruction(
        &mut self,
        entry: crate::stack_window::StackEntry,
        instruction: Instruction,
        copy_choice: Option<(SeatId, Vec<TargetChoice>)>,
        _journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
        let index = self
            .instruction_index()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let bound = self.instruction_target(&entry)?;
        let choice = bound.choice();
        let target = self
            .resolution_cursor()
            .and_then(|c| c.results.returned_target_permanent)
            .map(TargetRef::Object)
            .unwrap_or(choice.target);
        let before = self.rules_snapshot(None)?;
        // Function-local transient cut enum; consumed immediately without heap storage.
        #[allow(clippy::large_enum_variant)]
        enum InstructionCut {
            Ready(RulesOccurrence),
            Exit {
                exit: crate::stack_window::StackExit,
                reason: ZoneReason,
            },
        }
        let mut cause = EventCause::Instruction {
            entry: entry.id(),
            instruction: index,
        };
        let receipt = match instruction {
            Instruction::OptionalSequence { .. }
            | Instruction::Jump { .. }
            | Instruction::ZoneOp(_)
            | Instruction::DrawCards(_)
            | Instruction::MutateObject(_)
            | Instruction::AdjustLife(_)
            | Instruction::DealDamage(_)
            | Instruction::MutatePlayer(_)
            | Instruction::ApplyContinuous(_)
            | Instruction::PromptChoice(_)
            | Instruction::BranchCondition { .. }
            | Instruction::ForEach { .. }
            | Instruction::ForEachPlayerQuery { .. }
            | Instruction::BindEnv { .. }
            | Instruction::SetObjectTag { .. }
            | Instruction::UpdateStateTrack { .. }
            | Instruction::SideDeckOp(_)
            | Instruction::ScheduleDelayedEventTrigger { .. }
            | Instruction::RepeatLoop { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::ScheduleDelayedStepTrigger { .. }
            | Instruction::EmitReflexiveTrigger { .. }
            | Instruction::FixedTargetClause(_)
            | Instruction::SearchLibrary { .. } => {
                return Err(EffectError::InconsistentProgram);
            }
            Instruction::StackOp(spec) if !spec.requires_stack_target() => {
                return Err(EffectError::InconsistentProgram);
            }
            Instruction::StackOp(spec) => {
                use crate::toolbox::{CounterDestination, StackOpKind};
                match &spec.op {
                    StackOpKind::Counter { destination, .. } => {
                        let TargetRef::Stack(countered) = target else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let view = before.view();
                        let target_type_set = match self
                            .window
                            .entries()
                            .iter()
                            .find(|candidate| candidate.id() == countered)
                            .map(|candidate| candidate.kind())
                        {
                            Some(StackKind::CardSpell(id)) => view
                                .evaluated
                                .get(*id)
                                .map(|properties| &properties.current_types),
                            Some(StackKind::SpellCopy { .. }) => {
                                self.stack_copiable_card_types(countered)
                            }
                            _ => None,
                        }
                        .cloned()
                        .unwrap_or_default();
                        self.resolution_cursor_mut()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .results
                            .counter_target_types = target_type_set;
                        if self.spell_cannot_be_countered_by(countered, Some(&entry))? {
                            self.resolution_cursor_mut()
                                .ok_or(EffectError::WrongResolutionFrame)?
                                .results
                                .last_action_succeeded = false;
                            return Ok(());
                        }
                        if self
                            .window
                            .entries()
                            .iter()
                            .find(|candidate| candidate.id() == countered)
                            .is_some_and(|candidate| {
                                !matches!(
                                    candidate.kind(),
                                    StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                                )
                            })
                        {
                            let (window, exit) = self.window.counter_captured_entry(countered)?;
                            if exit.is_some() {
                                return Err(EffectError::InconsistentProgram);
                            }
                            self.window = window;
                            self.payloads.remove(&countered);
                            self.resolution_cursor_mut()
                                .ok_or(EffectError::WrongResolutionFrame)?
                                .results
                                .last_action_succeeded = true;
                            self.emit_from(
                                EventCause::CounterSpell {
                                    entry: entry.id(),
                                    instruction: index,
                                    target: countered,
                                },
                                &before,
                                None,
                                append,
                                |_, _, _| {
                                    Ok(vec![RulesOccurrence::AbilityCountered { entry: countered }])
                                },
                            )?;
                            return Ok(());
                        }
                        let controller = self
                            .window
                            .entries()
                            .iter()
                            .find(|spell| spell.id() == countered)
                            .ok_or(EffectError::InconsistentProgram)?
                            .controller();
                        self.resolution_cursor_mut()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .results
                            .countered_spell_controller = Some((countered, controller));
                        let temp_exile_countered = self.temporary_rules.iter().any(|active| {
                            let crate::program::TemporaryRuleKind::ExileReplacement {
                                filter,
                                trigger: crate::program::TemporaryExileTrigger::SpellResolves,
                                ..
                            } = &active.rule
                            else {
                                return false;
                            };
                            if !active.resolved_players.contains(&controller) {
                                return false;
                            }
                            match filter {
                                None => true,
                                Some(f) => match self
                                    .window
                                    .entries()
                                    .iter()
                                    .find(|spell| spell.id() == countered)
                                    .map(|spell| spell.kind())
                                {
                                    Some(StackKind::CardSpell(obj)) => {
                                        before.view().evaluated.get(*obj).is_some_and(|view| {
                                            self.matches_object_filter(
                                                *f,
                                                view,
                                                active.controller,
                                                active.source_object,
                                                &BTreeSet::new(),
                                                &before.view().evaluated,
                                            )
                                        })
                                    }
                                    _ => true,
                                },
                            }
                        });
                        let dest = if matches!(destination, CounterDestination::Exile)
                            || temp_exile_countered
                        {
                            ObjectDestination::Exile
                        } else if let CounterDestination::Library { top } = *destination {
                            if top {
                                ObjectDestination::LibraryTop
                            } else {
                                ObjectDestination::LibraryBottom
                            }
                        } else {
                            ObjectDestination::Graveyard
                        };
                        let (window, exit) =
                            self.window.record_targeted_resolution_move_with_exit(
                                self.window.revision(),
                                choice.id,
                                dest,
                            )?;
                        self.window = window;
                        self.resolution_cursor_mut()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .results
                            .last_action_succeeded = true;
                        cause = EventCause::CounterSpell {
                            entry: entry.id(),
                            instruction: index,
                            target: countered,
                        };
                        InstructionCut::Exit {
                            exit,
                            reason: ZoneReason::Countered,
                        }
                    }
                    StackOpKind::GainControl => {
                        let TargetRef::Stack(spell) = target else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let controller = entry.controller();
                        let previous = self.window.gain_spell_control(spell, controller)?;
                        if previous == controller {
                            return Ok(());
                        }
                        InstructionCut::Ready(RulesOccurrence::SpellControllerChanged {
                            entry: spell,
                            before: previous,
                            after: controller,
                        })
                    }
                    StackOpKind::Retarget { .. } => {
                        let (actor, changes) =
                            copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                        if actor != entry.controller() {
                            return Err(EffectError::WrongChooser);
                        }
                        let TargetRef::Stack(original) = target else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        if changes.is_empty() {
                            return Ok(());
                        }
                        let subjects = |window: &crate::stack_window::StackWindow| -> Result<Vec<TargetChoice>, EffectError> {
                            Ok(window
                                .entries()
                                .iter()
                                .find(|s| s.id() == original)
                                .ok_or(EffectError::InconsistentProgram)?
                                .targets()
                                .targets()
                                .iter()
                                .map(|b| b.choice())
                                .collect())
                        };
                        let before = subjects(&self.window)?;
                        self.window.change_targets(original, changes)?;
                        let after = subjects(&self.window)?;
                        if before == after {
                            return Ok(());
                        }
                        InstructionCut::Ready(RulesOccurrence::StackTargetsChanged {
                            entry: original,
                            before,
                            after,
                        })
                    }
                    StackOpKind::Copy {
                        ability_mode: true, ..
                    } => {
                        let (actor, changes) =
                            copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                        if actor != entry.controller() {
                            return Err(EffectError::WrongChooser);
                        }
                        let TargetRef::Stack(original) = target else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let source = self
                            .window
                            .entries()
                            .iter()
                            .find(|s| s.id() == original)
                            .ok_or(EffectError::InconsistentProgram)?
                            .clone();
                        let payload = self
                            .payloads
                            .get(&original)
                            .cloned()
                            .ok_or(EffectError::InconsistentProgram)?;
                        InstructionCut::Ready(
                            self.create_ability_copy(&source, payload, actor, changes)?,
                        )
                    }
                    StackOpKind::Copy {
                        ability_mode: false,
                        ..
                    } => {
                        let (actor, selected) =
                            copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                        if actor != entry.controller() {
                            return Err(EffectError::WrongChooser);
                        }
                        let TargetRef::Stack(original) = target else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let snapshot = self.capture_copy_source(original, 0.into())?;
                        InstructionCut::Ready(self.create_copy(&snapshot, actor, selected)?)
                    }
                    _ => return Err(EffectError::InconsistentProgram),
                }
            }
        };
        self.emit_from(
            cause,
            &before,
            None,
            append,
            |previous, current, controllers| {
                let occurrence = match receipt {
                    InstructionCut::Ready(value) => value,
                    InstructionCut::Exit { exit, reason } => {
                        exit_occurrence_at(previous, current, exit, reason, controllers)?
                    }
                };
                Ok(vec![occurrence])
            },
        )?;
        Ok(())
    }

    pub(super) fn materialize_resolved_parts(
        &self,
        entry: &crate::stack_window::StackEntry,
        parts: &crate::characteristics::ResolvedPartsSpec,
        locals: &BTreeMap<crate::program::CreatureTypeSlot, CreatureTypeId>,
        target_owner: SeatId,
        source_object: Option<ObjectId>,
    ) -> Result<crate::characteristics::ResolvedParts, EffectError> {
        let cursor = self.resolution_cursor();
        let chosen_color = cursor
            .and_then(|c| c.chosen_effect_color)
            .and_then(crate::characteristics::Color::from_mana);
        let chosen_basic_land_type = cursor
            .and_then(|c| c.results.chosen_basic_land_type)
            .and_then(crate::land::LandType::basic);
        let opponents = self
            .combat_rules
            .opponents(self.window.participants(), entry.controller())?;
        let subject_object = parts.0.iter().find_map(|spec| {
            if let crate::characteristics::ResolvedPartSpec::MustBlockSubject(subj) = spec {
                self.resolve_pass_c_move_subject(entry, *subj)
                    .ok()
                    .flatten()
            } else {
                None
            }
        });
        Ok(parts.materialize_full(
            locals,
            entry.controller(),
            target_owner,
            source_object,
            chosen_color,
            chosen_basic_land_type,
            &opponents,
            subject_object,
            |expr| {
                self.resolve_value_expr(entry.controller(), expr)
                    .map_err(|_| {
                        crate::characteristics::CharacteristicInvariantError::NoEligibleComponent
                    })
            },
        )?)
    }
}
