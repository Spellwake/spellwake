//! Iterative resolution and combat-origin damage driver, split into stack-isolated phases.
use super::*;

pub(super) struct RuleStepContext<'a> {
    pub(super) journal: &'a mut DamageJournal,
    pub(super) draws: &'a mut DrawJournal,
    pub(super) append: &'a mut RulesJournal,
}

pub(super) enum StepOutcome {
    Continue,
    ProgressLookTop,
    Pending(EffectDecision),
    Complete(RuleCompletion),
    Next(Instruction),
}

macro_rules! draw_cards_step {
    ($self:expr, $ctx:expr, $player:expr, $amount:expr, $origin:expr, $append:expr $(,)?) => {{
        match $self.draw_with_replacements($player, $amount, $origin, $append)? {
            Ok(completed) => {
                if $self.won_by_effect.is_some() {
                    $ctx.draws.append(completed);
                    return Ok(StepOutcome::Complete(RuleCompletion::GameEnded));
                }
                completed
            }
            Err(frame) => {
                $ctx.draws.append($self.take_draw_receipts());
                return Ok(StepOutcome::Pending(frame));
            }
        }
    }};
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
            if matches!(self.pending_work, Some(PendingWork::ImpulseReplacement(_))) {
                if let Some(frame) = self.progress_impulse_replacement(&mut append)? {
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
            let mut power_dealer: Option<crate::damage::DamageSource> = None;
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
                &mut power_dealer,
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
            let instruction =
                run_phase!(self.progress_phase_7(&mut ctx, &entry, index, instruction));
            let _ = run_phase!(self.progress_phase_final(
                &mut ctx,
                entry,
                index,
                instruction,
                power_dealer
            ));
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
        power_dealer: &mut Option<crate::damage::DamageSource>,
    ) -> Result<StepOutcome, EffectError> {
        if let Instruction::TransformSubtype(kind) = instruction {
            if let Some(order) = self.resolution_transform_order(kind)? {
                return Ok(StepOutcome::Pending(EffectDecision::TransformOrder(order)));
            }
            self.transform_subtype(entry, index, kind, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::GrantFlashback(subject) = instruction {
            use crate::program::FlashbackSubject;
            let player = entry.controller();
            let candidates = match subject {
                FlashbackSubject::Target => {
                    let TargetRef::Object(card) = self.instruction_target(entry)?.choice().target
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    vec![card]
                }
                FlashbackSubject::ControllerGraveyard => self
                    .window
                    .cards()
                    .iter()
                    .filter(|card| card.zone() == ObjectZone::Graveyard(player))
                    .map(GameObject::id)
                    .collect(),
            };
            let evaluated = self.evaluated_characteristics()?;
            self.granted_flashback
                .extend(candidates.into_iter().filter(|id| {
                    evaluated.get(*id).is_some_and(|card| {
                        card.zone == ObjectZone::Graveyard(player)
                            && (card.has_type(CardType::Instant)
                                || card.has_type(CardType::Sorcery))
                    })
                }));
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DamageFromPower(source) = instruction {
            let Some((dealer, power)) = self.damage_dealer_power(entry, source)? else {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            };
            if source == crate::program::PowerDamageSource::TargetCreatureToAbilitySource {
                if let Some(target) = self.ability_source_damage_recipient(entry)? {
                    self.install_damage_batch(
                        DamageOrigin::Resolution {
                            entry: entry.id(),
                            instruction: index,
                        },
                        DamageKind::Noncombat,
                        vec![DamageInput {
                            source: dealer,
                            target,
                            amount: power,
                        }],
                        ctx.journal,
                    )?;
                } else {
                    self.advance_instruction()?;
                }
                return Ok(StepOutcome::Continue);
            }
            *power_dealer = Some(dealer);
            instruction = Instruction::Damage { amount: power };
        }
        if let Instruction::DamageAmount(amount)
        | Instruction::DamageAmountThenExileIfDies(amount) = &instruction
        {
            let amount = self.resolve_value_expr(entry.controller(), amount)?;
            if !amount.is_positive() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            instruction = Instruction::Damage { amount };
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
            instruction,
            Instruction::Unearth
                | Instruction::Ninjutsu
                | Instruction::ExileUpToTwoGraveyardTargetsGainLife
                | Instruction::ReturnUpToOneOwnGraveyardCreatureToHand
                | Instruction::ReturnSourceGraveyardToBattlefield { .. }
                | Instruction::ResolveCounterKeyword(
                    crate::program::CounterKeyword::Persist
                        | crate::program::CounterKeyword::Undying
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
        if matches!(
            instruction,
            Instruction::OptionalSequence { .. } | Instruction::OptionalEffect { .. }
        ) && matches!(
            next_instruction,
            Some(Instruction::ResolveTransfer(
                crate::program::TransferKeyword::Graft(_)
            ))
        ) && self.graft_pair(entry.id()).is_none()
        {
            self.set_instruction(index + 2)?;
            return Ok(StepOutcome::Continue);
        }
        if let (
            Instruction::OfferManaPayment { cost, .. },
            Some(Instruction::SacrificeUpkeepSource(keyword)),
        ) = (&mut instruction, next_instruction)
        {
            let Some(price) = self.upkeep_price(*keyword, cost, entry, index, ctx.append)? else {
                self.set_instruction(index + 2)?;
                return Ok(StepOutcome::Continue);
            };
            *cost = price;
        }
        if instruction
            == Instruction::RequireCondition(
                crate::program::InterveningCondition::CastFromGraveyard,
            )
        {
            if entry.cast_from_graveyard {
                self.advance_instruction()?;
            } else {
                self.set_instruction(instruction_count)?;
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CopyForOtherCreatures {
            self.start_creature_copies(entry)?;
            if self.progress_copy_run(entry, index, ctx.append)? {
                let frame = EffectDecision::CopyTargets(self.copy_resolution_frame()?);
                return Ok(StepOutcome::Pending(frame));
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CopyResolvingSpell {
            self.start_self_copy(entry.id())?;
            if self.progress_copy_run(entry, index, ctx.append)? {
                let frame = EffectDecision::CopyTargets(self.copy_resolution_frame()?);
                return Ok(StepOutcome::Pending(frame));
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::WatchNextSpellOrLoyaltyTwice {
            let colors = self.targeting_entry_colors(entry, &self.evaluated_characteristics()?);
            self.triggers
                .watch_next_spell_or_loyalty(entry.controller(), entry.id(), colors);
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::WatchNextLoyaltyCopy {
            self.watch_ability_copy(entry, crate::occurrence::AbilityCopyWatch::NextLoyalty)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CopyCapturedAbility {
            let source = self.delayed_ability_source(entry.id())?;
            if !source.entry.targets().targets().is_empty() {
                let frame = EffectDecision::CopyTargets(self.copy_resolution_frame()?);
                return Ok(StepOutcome::Pending(frame));
            }
            self.finish_delayed_ability_copy(entry, &source, vec![], ctx.append)?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CopyOnCast(kind) = instruction {
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
        if let Instruction::ScheduleArtifactReturn(object) = instruction {
            let StackKind::Emblem { emblem } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            self.triggers
                .delay_emblem_return(*emblem, entry.controller(), object);
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ScheduleGraveyardReturnAtEndStep(rule) = instruction {
            self.schedule_graveyard_return(entry, rule)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction
            == Instruction::RequireCondition(
                crate::program::InterveningCondition::ReturnSourceOnBattlefield,
            )
        {
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
        if instruction == Instruction::ReturnSacrificedCardToBattlefield {
            let binding = self
                .execution_bindings(entry.id())
                .and_then(|value| value.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let [member] = binding.causes.members() else {
                return Err(EffectError::InconsistentProgram);
            };
            let crate::occurrence::TriggerRelated::ZoneChange { after, .. } = &member.related
            else {
                return Err(EffectError::InconsistentProgram);
            };
            let object = after.object.id();
            let zone = after.object.zone();
            if matches!(
                zone,
                ObjectZone::Graveyard(_) | ObjectZone::Exile | ObjectZone::Command
            ) && self
                .window
                .cards()
                .get(object)
                .is_some_and(|card| card.zone() == zone && card.card_id().is_some())
                && self
                    .copiable_program(object)
                    .is_some_and(|card| card.permanent().is_some())
            {
                self.linked_return_work = Some(LinkedReturnWork {
                    origin: LinkedReturnOrigin::Sacrifice {
                        zone,
                        controller: entry.controller(),
                        entry: entry.id(),
                        instruction: index,
                    },
                    library_bottom: None,
                    wake_entry: None,
                    objects: vec![object],
                    orders: BTreeMap::new(),
                    graveyard_orders: BTreeMap::new(),
                    choices: BTreeMap::new(),
                    auras: BTreeMap::new(),
                    replacements: None,
                });
                self.finish_linked_return_if_ready(ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReturnCardsDestroyedThisWay {
            let cards = std::mem::take(
                &mut self
                    .resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .results
                    .destroyed_cards_to_graveyard,
            );
            self.begin_linked_cohort_return(
                LinkedReturnOrigin::Graveyard {
                    player: None,
                    controller: Some(entry.controller()),
                    subtype_counters: None,
                    entry_additions: crate::program::EntryAdditions::default(),
                    entry: entry.id(),
                    instruction: index,
                    tapped: false,
                    forest: false,
                    corpse: false,
                },
                cards,
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ReturnCapturedDyingCreature
                | Instruction::ReturnCapturedDyingCreatureWithTypeAndColor { .. }
        ) {
            let entry_additions = match instruction {
                Instruction::ReturnCapturedDyingCreatureWithTypeAndColor { subtype, color } => {
                    crate::program::EntryAdditions {
                        creature_subtype: Some(subtype),
                        color: Some(color),
                    }
                }
                _ => crate::program::EntryAdditions::default(),
            };
            if let Some((card, owner)) = self.captured_graveyard_card()? {
                let origin = LinkedReturnOrigin::Graveyard {
                    player: Some(owner),
                    controller: Some(entry.controller()),
                    subtype_counters: None,
                    entry_additions,
                    entry: entry.id(),
                    instruction: index,
                    tapped: false,
                    forest: false,
                    corpse: false,
                };
                self.begin_linked_cohort_return(origin, vec![card], ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ReturnGraveyardCreatureWithSubtypeCounters { subtype, amount } =
            instruction
        {
            let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            let origin = LinkedReturnOrigin::Graveyard {
                player: Some(entry.controller()),
                controller: None,
                subtype_counters: Some((subtype, amount)),
                entry_additions: crate::program::EntryAdditions::default(),
                entry: entry.id(),
                instruction: index,
                tapped: false,
                forest: false,
                corpse: false,
            };
            self.begin_linked_cohort_return(origin, vec![object], ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReturnDyingCreatureAsForest {
            if let Some((card, owner)) = self.captured_graveyard_card()? {
                self.begin_graveyard_returns(entry, index, owner, vec![card], false, ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReturnDyingSourceTapped {
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
            let object = after.object.id();
            let player = after.object.owner();
            if self.window.cards().get(object).is_some_and(|card| {
                card.zone() == ObjectZone::Graveyard(player) && card.card_id().is_some()
            }) && self
                .copiable_program(object)
                .is_some_and(|card| card.permanent().is_some())
            {
                self.begin_graveyard_returns(entry, index, player, vec![object], true, ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ReturnDelayedGraveyardCardTransformed(object) = instruction {
            if let Some(player) = self.delayed_transformed_return_player(object) {
                self.begin_linked_cohort_return(
                    LinkedReturnOrigin::GraveyardTransformed {
                        player,
                        controller: entry.controller(),
                        entry: entry.id(),
                        instruction: index,
                    },
                    vec![object],
                    ctx.append,
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ReturnDelayedGraveyardCard(object) = instruction {
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
        if let Instruction::ReturnExiledTargetCohort {
            next_end_step,
            owner_controls,
        } = instruction
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
        if let Instruction::ReturnDelayedExileWithCounters(objects) = instruction {
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
        if let Instruction::ReturnDelayedExile(objects) = instruction {
            self.begin_linked_cohort_return(LinkedReturnOrigin::Exile, objects, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::WatchSmallCreatureCombatDraw(_) | Instruction::WatchTargetCombatDamageDraw
        ) {
            let subject = if let Instruction::WatchSmallCreatureCombatDraw(limit) = instruction {
                crate::occurrence::CombatDrawSubject::ControlledPowerAtMost(limit)
            } else {
                let TargetRef::Object(id) = self.instruction_target(entry)?.choice().target else {
                    return Err(EffectError::InconsistentProgram);
                };
                crate::occurrence::CombatDrawSubject::Exact(id)
            };
            let source = match entry.kind() {
                StackKind::SpellCopy { .. } => crate::occurrence::CombatDrawSource::SpellCopy(
                    entry.id(),
                    self.stack_copy_colors(entry.id())
                        .cloned()
                        .unwrap_or_default(),
                ),
                kind => {
                    let id = match kind {
                        StackKind::CardSpell(id) => *id,
                        StackKind::ActivatedAbility { source } => source.id(),
                        _ => return Err(EffectError::InconsistentProgram),
                    };
                    let appearance = self
                        .rules_snapshot(None)?
                        .view()
                        .appearance(id)
                        .or_else(|| self.departed_reflexive_sources.get(&id).cloned())
                        .ok_or(EffectError::InconsistentProgram)?;
                    crate::occurrence::CombatDrawSource::Physical(Box::new(appearance))
                }
            };
            self.triggers
                .watch_combat_draw(entry.controller(), source, subject);
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ChooseMatchingHandSpellToCast {
            let candidates = self
                .window
                .cards()
                .iter()
                .filter(|card| card.zone() == ObjectZone::Hand(entry.controller()))
                .filter(|card| self.copiable_program(card.id()).is_some())
                .map(GameObject::id)
                .collect();
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .found_spells = candidates;
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.body.instructions[index] = Instruction::ChooseFoundSpell;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ChooseOwnCommanderToCast {
            let candidates = self
                .window
                .cards()
                .iter()
                .filter(|card| {
                    self.commander_in_command(card, entry.controller())
                        .is_some()
                })
                .map(GameObject::id)
                .collect();
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .found_spells = candidates;
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.body.instructions[index] = Instruction::ChooseFoundSpell;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::SearchEachLibraryForFreeCast {
            let participants = self.window.participants().to_vec();
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            let search = participants
                .iter()
                .map(|player| Instruction::SearchLibrary {
                    filter: Box::new(
                        crate::program::LibrarySearch::to_hand(
                            crate::program::SearchCardMatcher::Nonland,
                        )
                        .with_source(crate::program::SearchSource::PlayerLibrary(*player))
                        .with_destination(crate::program::SearchDestination::Exile {
                            link: crate::program::SearchExileLink::FreeCastCohort,
                        })
                        .with_reveal(false),
                    ),
                });
            payload.expand_instruction(
                index,
                false,
                search.chain(std::iter::repeat_n(
                    Instruction::ChooseFoundSpell,
                    participants.len(),
                )),
            );
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CastLinkedSpellMatchingX {
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let cost = entry.chosen_x().ok_or(EffectError::InconsistentProgram)?;
            let candidates = self
                .linked_search_results
                .get(&source.id())
                .into_iter()
                .flatten()
                .copied()
                .filter(|&id| {
                    self.window
                        .cards()
                        .get(id)
                        .is_some_and(|card| card.zone() == ObjectZone::Exile)
                        && self
                            .copiable_program(id)
                            .is_some_and(|program| program.mana_value() == *cost)
                })
                .collect();
            self.resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .found_spells = candidates;
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.body.instructions[index] = Instruction::ChooseFoundSpell;
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
        if let Instruction::ExileMilledCreatures { maximum } = instruction {
            if let Some(frame) = self.milled_creature_frame(maximum)? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.begin_selected_graveyard_exile(vec![])?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::VoteForGraveyardCards { ref types } = instruction {
            if let Some(frame) = self.graveyard_vote_frame(types)? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            if !self.return_vote_winners()? {
                self.advance_instruction()?;
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ChooseFoundSpell {
            if let Some(frame) = self.found_spell_selection()? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ChooseMeldPartner(ref mate) = instruction {
            if let Some(frame) = self.meld_selection(mate)? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::MeldExiledPair(ref back) = instruction {
            self.finish_meld(back, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::PairWithCreature { entrant_only } = instruction {
            if let Some(frame) = self.soulbond_selection(entrant_only)? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::MayTurnControlledCreatureFaceUp {
            if let Some(frame) = self.turn_face_up_selection()? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::EquipNewToken {
            if let Some(frame) = self.token_equipment_selection()? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ReturnOddGraveyardPermanentIfPaid { .. }
                | Instruction::PutGraveyardCreatureAtMostX
                | Instruction::PutLandFromExiledCohort
                | Instruction::PutTargetOwnedExiledLandsOntoBattlefield
                | Instruction::PutEquipmentFromHandOrGraveyard
                | Instruction::PutHandPermanentsOntoBattlefield { .. }
                | Instruction::PutHandPermanentFiltered { .. }
        ) {
            if let Some(frame) = self.hand_entry_selection()? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::MoveGraveyardGroup {
            filter,
            all_graveyards,
            to_battlefield: true,
            tapped,
        } = &instruction
        {
            let objects: Vec<_> = self
                .graveyard_cards_matching(entry.controller(), filter, *all_graveyards)?
                .into_iter()
                .collect();
            if objects.is_empty() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            self.linked_return_work = Some(LinkedReturnWork {
                origin: LinkedReturnOrigin::Graveyard {
                    player: (!*all_graveyards).then_some(entry.controller()),
                    controller: Some(entry.controller()),
                    subtype_counters: None,
                    entry_additions: crate::program::EntryAdditions::default(),
                    entry: entry.id(),
                    instruction: index,
                    tapped: *tapped,
                    forest: false,
                    corpse: false,
                },
                library_bottom: None,
                wake_entry: None,
                objects,
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
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ManifestTop { player, amount } = &instruction {
            let player = self.resolving_player(*player)?;
            let remaining = match self
                .resolution_cursor()
                .and_then(|cursor| cursor.manifest_remaining.clone())
            {
                Some(count) => count,
                None => self.resolve_value_expr(entry.controller(), amount)?,
            };
            let next_card = self.window.cards().library(player).first().copied();
            if let Some(card) = next_card
                && remaining.is_positive()
            {
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .manifest_remaining = Some(remaining.minus(&1.into()));
                self.linked_return_work = Some(LinkedReturnWork {
                    origin: LinkedReturnOrigin::Manifest {
                        player,
                        entry: entry.id(),
                        instruction: index,
                    },
                    objects: vec![card],
                    library_bottom: None,
                    wake_entry: None,
                    orders: BTreeMap::new(),
                    graveyard_orders: BTreeMap::new(),
                    choices: BTreeMap::new(),
                    auras: BTreeMap::new(),
                    replacements: None,
                });
                self.finish_linked_return_if_ready(ctx.append)?;
            } else {
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .manifest_remaining = None;
                self.advance_instruction()?;
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CreateReflexiveGraveyardReturn {
            let amount = self
                .resolution_cursor()
                .and_then(|cur| cur.impulse.contributions.clone())
                .ok_or(EffectError::InconsistentProgram)?;
            let source = self
                .execution_bindings(entry.id())
                .and_then(|b| b.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?
                .source
                .clone();
            let before = self.rules_snapshot(None)?;
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
                        crate::occurrence::RulesOccurrence::GraveyardReturnPaid {
                            source: Box::new(source),
                            controller: entry.controller(),
                            mana_value: amount,
                        },
                    ])
                },
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ReturnGraveyardCreatureWithCorpse(_)
        ) {
            let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::IllegalTarget);
            };
            self.begin_graveyard_returns(
                entry,
                index,
                entry.controller(),
                vec![object],
                false,
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReturnRandomLinkedExile {
            let objects = self.exiled_search_results()?;
            if objects.len() > 1 {
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
            } else if !self.begin_exile_cohort_to_hand(objects)? {
                self.advance_instruction()?;
            }
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ReturnCardsExiledWithLink(link) = instruction {
            let StackKind::TriggeredAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let candidates = self
                .independent_exile_links
                .get(&(source.id(), link))
                .into_iter()
                .flatten()
                .copied()
                .collect();
            self.begin_linked_cohort_return(
                LinkedReturnOrigin::ChosenExile {
                    controller: None,
                    tapped: false,
                    entry: entry.id(),
                    instruction: index,
                },
                candidates,
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ReturnCardsExiledWithLinkExt {
            group,
            under_controller_control,
            tapped,
        } = instruction
        {
            let source_id = match entry.kind() {
                StackKind::TriggeredAbility { source } | StackKind::ActivatedAbility { source } => {
                    source.id()
                }
                StackKind::CardSpell(id) => *id,
                _ => return Err(EffectError::InconsistentProgram),
            };
            let candidates = self
                .independent_exile_links
                .get(&(source_id, group))
                .into_iter()
                .flatten()
                .copied()
                .collect();
            self.begin_linked_cohort_return(
                LinkedReturnOrigin::ChosenExile {
                    controller: under_controller_control.then(|| entry.controller()),
                    tapped,
                    entry: entry.id(),
                    instruction: index,
                },
                candidates,
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ExileSubjectUntilSourceLeaves { subject } = instruction {
            let objects: Vec<ObjectId> = self.effect_group(entry, *subject)?.into_iter().collect();
            let before_controllers: Vec<SeatId> = objects
                .iter()
                .filter_map(|&oid| {
                    self.window
                        .cards()
                        .get(oid)
                        .and_then(|o| o.controller().or(Some(o.owner())))
                })
                .collect();
            let new_ids = self.move_objects_to_subject_destination_ids(
                entry,
                index,
                entry.controller(),
                &objects,
                crate::program::SubjectDestination::Exile {
                    delayed_return: Some(crate::program::DelayedReturnTiming::UntilSourceLeaves),
                },
                ctx.append,
            )?;
            let moved_count = new_ids.len();
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = moved_count > 0;
                cursor.results.last_action_count = Some(RulesInteger::from_usize(moved_count));
                cursor.results.last_moved_objects = new_ids;
                cursor.results.last_moved_controllers = if moved_count > 0 {
                    before_controllers
                } else {
                    Vec::new()
                };
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::DrawGame {
            let participants = self.window.participants().to_vec();
            for seat in participants {
                self.state_based_losses.insert(seat);
            }
            return Ok(StepOutcome::Complete(RuleCompletion::GameEnded));
        }
        if let Instruction::TurnFaceUpSubject { subject } = instruction {
            let targets: Vec<ObjectId> = self
                .effect_group(entry, *subject)?
                .into_iter()
                .filter(|&id| {
                    self.window.cards().get(id).is_some_and(|card| {
                        card.zone() == ObjectZone::Battlefield && card.face_down().is_some()
                    })
                })
                .collect();
            let mut turned = 0usize;
            for id in targets {
                let before = self.rules_snapshot(None)?;
                let Some(card) = self.window.cards().get(id).cloned() else {
                    continue;
                };
                if card.zone() != ObjectZone::Battlefield || card.face_down().is_none() {
                    continue;
                }
                let underlying = self
                    .printed_face(&card)
                    .ok_or(EffectError::InconsistentProgram)?;
                let instant_or_sorcery = underlying.has_type(CardType::Instant)
                    || underlying.has_type(CardType::Sorcery);
                if !instant_or_sorcery {
                    self.window.set_orientation(id, None)?;
                    turned += 1;
                }
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |old, new, _| {
                        Ok(vec![if instant_or_sorcery {
                            crate::occurrence::RulesOccurrence::FaceDownRevealed { object: card }
                        } else {
                            crate::occurrence::RulesOccurrence::TurnedFaceUpByEffect {
                                before: Box::new(
                                    old.appearance(id).ok_or(EffectError::InconsistentProgram)?,
                                ),
                                after: Box::new(
                                    new.appearance(id).ok_or(EffectError::InconsistentProgram)?,
                                ),
                            }
                        }])
                    },
                )?;
            }
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = turned > 0;
                cursor.results.last_action_count = Some(RulesInteger::from_usize(turned));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::MoveCounters {
            from,
            to,
            kind,
            all,
            any_number,
            amount,
        } = instruction
        {
            let from_opt = self.resolve_pass_c_move_subject(entry, from)?;
            let to_opt = self.resolve_pass_c_move_subject(entry, to)?;
            let both_valid = from_opt.zip(to_opt).filter(|(f, t)| {
                f != t
                    && self
                        .window
                        .cards()
                        .get(*f)
                        .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                    && self
                        .window
                        .cards()
                        .get(*t)
                        .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
            });
            if let Some((from_id, to_id)) = both_valid {
                let evaluated = self.evaluated_characteristics()?;
                let mut present_kinds: Vec<CounterKind> = match kind {
                    Some(ck) => {
                        if self.counters_of_kind(from_id, ck).is_positive() {
                            vec![ck]
                        } else {
                            Vec::new()
                        }
                    }
                    None => self
                        .characteristics
                        .present_permanent_counter_kinds(from_id)
                        .into_iter()
                        .collect(),
                };
                present_kinds
                    .retain(|&ck| !self.permanent_counter_prevented(&evaluated, to_id, ck));
                if !present_kinds.is_empty() {
                    if all {
                        let mut moved = BTreeMap::new();
                        for &ck in &present_kinds {
                            let removed = self.remove_counters_from_objects(
                                entry,
                                index,
                                &[from_id],
                                Some(ck),
                                None,
                                ctx.append,
                            )?;
                            if removed.is_positive() {
                                moved.insert(ck, removed);
                            }
                        }
                        if !moved.is_empty() {
                            self.place_moved_counters(
                                entry.id(),
                                index,
                                to_id,
                                &moved,
                                ctx.append,
                            )?;
                        }
                    } else if any_number {
                        return Ok(StepOutcome::Pending(EffectDecision::CounterRemoval(
                            self.counter_removal_choice()?,
                        )));
                    } else if let Some(ck) = kind {
                        let cap = match amount.as_ref() {
                            Some(expr) => self.resolve_value_expr(entry.controller(), expr)?,
                            None => 1.into(),
                        };
                        if cap.is_positive() {
                            let removed = self.remove_counters_from_objects(
                                entry,
                                index,
                                &[from_id],
                                Some(ck),
                                Some(&ValueExpr::Literal(cap)),
                                ctx.append,
                            )?;
                            if removed.is_positive() {
                                let mut moved = BTreeMap::new();
                                moved.insert(ck, removed);
                                self.place_moved_counters(
                                    entry.id(),
                                    index,
                                    to_id,
                                    &moved,
                                    ctx.append,
                                )?;
                            }
                        }
                    } else if present_kinds.len() == 1 {
                        let ck = present_kinds[0];
                        let cap = match amount.as_ref() {
                            Some(expr) => self.resolve_value_expr(entry.controller(), expr)?,
                            None => 1.into(),
                        };
                        if cap.is_positive() {
                            let removed = self.remove_counters_from_objects(
                                entry,
                                index,
                                &[from_id],
                                Some(ck),
                                Some(&ValueExpr::Literal(cap)),
                                ctx.append,
                            )?;
                            if removed.is_positive() {
                                let mut moved = BTreeMap::new();
                                moved.insert(ck, removed);
                                self.place_moved_counters(
                                    entry.id(),
                                    index,
                                    to_id,
                                    &moved,
                                    ctx.append,
                                )?;
                            }
                        }
                    } else {
                        let cap = match amount.as_ref() {
                            Some(expr) => self.resolve_value_expr(entry.controller(), expr)?,
                            None => 1.into(),
                        };
                        if cap.is_positive() {
                            return Ok(StepOutcome::Pending(EffectDecision::CounterRemoval(
                                self.counter_removal_choice()?,
                            )));
                        }
                    }
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReturnLinkedSearchCards {
            let objects = self.exiled_search_results()?;
            if self.begin_exile_cohort_to_hand(objects)? {
                return Ok(StepOutcome::Continue);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(instruction, Instruction::ReturnGraveyardTargetsTapped(_)) {
            let objects = self
                .legal_permanent_targets()?
                .into_iter()
                .collect::<Vec<_>>();
            if !objects.is_empty() {
                self.begin_graveyard_returns(
                    entry,
                    index,
                    entry.controller(),
                    objects,
                    true,
                    ctx.append,
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReturnChosenXGraveyardCreatures {
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
        if let Instruction::ReturnGraveyardCohort(returning) = instruction {
            use crate::program::{GraveyardReturnFilter as Filter, ObjectFilter};
            let player = (!returning.from_all_graveyards).then_some(entry.controller());
            let current = self.evaluated_characteristics()?;
            let objects = self
                .window
                .cards()
                .iter()
                .filter(|card| {
                    matches!(card.zone(), ObjectZone::Graveyard(owner)
                        if player.is_none_or(|player| owner == player))
                        && card.card_id().is_some()
                })
                .filter(|card| {
                    current
                        .get(card.id())
                        .is_some_and(|value| match returning.filter {
                            Filter::Permanent => {
                                self.matches_graveyard_card(ObjectFilter::permanent(), value)
                            }
                            Filter::CardType(kind) => value.has_type(kind),
                            Filter::CreatureSubtype(kind) => {
                                value.is_creature()
                                    && self.matches_graveyard_card(
                                        ObjectFilter::any().with_subtype(kind),
                                        value,
                                    )
                            }
                        })
                })
                .map(GameObject::id)
                .collect();
            self.begin_linked_cohort_return(
                LinkedReturnOrigin::Graveyard {
                    player,
                    controller: returning
                        .under_resolving_players_control
                        .then_some(entry.controller()),
                    subtype_counters: None,
                    entry_additions: returning.additions,
                    entry: entry.id(),
                    instruction: index,
                    tapped: returning.tapped,
                    forest: false,
                    corpse: false,
                },
                objects,
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CopyOtherControlledNontokensAsEntryHost {
            self.copy_controlled_creatures_as_entry_host(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CopyOtherTargetAsArtifact {
            if let Some((receiver, donor)) = self.legal_linked_targets()? {
                let before = self.rules_snapshot(None)?;
                let mut copied = self
                    .copiable_program(donor)
                    .ok_or(EffectError::InconsistentProgram)?
                    .clone();
                copied.card_types.insert(CardType::Artifact);
                let effect = self.characteristics.add_effect(
                    entry.id(),
                    receiver,
                    crate::characteristics::ResolvedParts(vec![
                        crate::characteristics::ResolvedPart::Copy(Box::new(copied)),
                    ]),
                    crate::characteristics::ResolvedDuration::UntilEndOfTurn,
                )?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |old, new, _| {
                        let appearances = occurrence_flow::characteristic_appearance_changes(
                            old,
                            new,
                            effect.affected.iter().copied(),
                        )?;
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                effect,
                                appearances,
                            },
                        ])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExileUntilOtherTargetLeaves
            && self.legal_linked_targets()?.is_none()
        {
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::DiscardOneAtRandom(_) | Instruction::SacrificeRandomUntilLand
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
        if let Instruction::ReturnRandomGraveyardCards {
            ref filter, count, ..
        } = instruction
            && self
                .resolution_cursor()
                .is_some_and(|cursor| cursor.results.sampled_graveyard_cards.is_none())
        {
            let candidates =
                self.random_graveyard_candidates(entry.controller(), filter.as_deref().copied())?;
            if candidates.len() <= usize::from(count.get()) {
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .results
                    .sampled_graveyard_cards = Some(candidates.into_iter().collect());
            } else {
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
                return Ok(StepOutcome::Continue);
            }
        }
        if matches!(
            instruction,
            Instruction::ReturnRandomGraveyardCards {
                to_battlefield: true,
                ..
            }
        ) {
            let objects = self
                .resolution_cursor_mut()
                .and_then(|cursor| cursor.results.sampled_graveyard_cards.take())
                .ok_or(EffectError::InconsistentProgram)?;
            if self.linked_return_work.is_some() {
                return Err(EffectError::InconsistentProgram);
            }
            if !objects.is_empty() {
                self.linked_return_work = Some(LinkedReturnWork {
                    library_bottom: None,
                    origin: LinkedReturnOrigin::Graveyard {
                        player: Some(entry.controller()),
                        controller: None,
                        subtype_counters: None,
                        entry_additions: crate::program::EntryAdditions::default(),
                        entry: entry.id(),
                        instruction: index,
                        tapped: false,
                        forest: false,
                        corpse: false,
                    },
                    wake_entry: None,
                    objects: objects.into_iter().collect(),
                    orders: BTreeMap::new(),
                    graveyard_orders: BTreeMap::new(),
                    choices: BTreeMap::new(),
                    auras: BTreeMap::new(),
                    replacements: None,
                });
                self.finish_linked_return_if_ready(ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::DestroyTarget | Instruction::DestroyTargetWithoutRegeneration
        ) {
            let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            let previous = self.current_characteristics(target)?;
            let results = &mut self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .results;
            results.destroyed_target_controller = None;
            results.destruction_target_controller =
                previous.as_ref().and_then(|card| card.controller);
            results.destruction_target_toughness = previous
                .as_ref()
                .and_then(|card| card.power_toughness.as_ref())
                .map(|stats| stats.derived.toughness.clone());
            results.destruction_target_subtypes = previous
                .map(|card| card.current_creature_subtypes)
                .unwrap_or_default();
        }
        let instruction = match self.progress_pass_c_instruction(ctx, entry, index, instruction)? {
            StepOutcome::Next(rest) => rest,
            other => return Ok(other),
        };
        if let Instruction::ChooseOption {
            chooser,
            ref options,
        } = instruction
        {
            let chosen = self
                .resolution_cursor_mut()
                .and_then(|cursor| cursor.chosen_option_index.take());
            let Some(chosen_idx) = chosen else {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                let count =
                    u8::try_from(options.len()).map_err(|_| EffectError::InconsistentProgram)?;
                let labels = options.iter().map(|b| b.label.clone()).collect();
                let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                    revision: self.revision,
                    entry: entry.id(),
                    instruction: index,
                    chooser: actor,
                    count,
                    labels,
                });
                return Ok(StepOutcome::Pending(frame));
            };
            let branch = options
                .get(usize::from(chosen_idx))
                .ok_or(EffectError::InconsistentProgram)?;
            let inserted = branch.body.instructions.len();
            let base = isize::try_from(index).map_err(|_| EffectError::InconsistentProgram)?;
            let mut spliced = Vec::with_capacity(inserted);
            for op in &branch.body.instructions {
                let mut cloned = op.clone();
                cloned.shift_jumps(base, 0);
                spliced.push(cloned);
            }
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            let delta =
                isize::try_from(inserted).map_err(|_| EffectError::InconsistentProgram)? - 1;
            for op in &mut payload.body.instructions {
                op.shift_jumps(delta, index + 1);
            }
            payload.body.instructions.splice(index..=index, spliced);
            for mode in &mut payload.modes {
                mode.shift_for_splice(index, inserted);
            }
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::PutTargetInLibrary(
            crate::program::LibraryPlacement::ChoiceTopOrBottom { chooser },
        ) = instruction
        {
            let chosen = self
                .resolution_cursor_mut()
                .and_then(|cursor| cursor.chosen_option_index.take());
            let Some(chosen_idx) = chosen else {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                    revision: self.revision,
                    entry: entry.id(),
                    instruction: index,
                    chooser: actor,
                    count: 2,
                    labels: vec![
                        "Put on top of library".to_owned(),
                        "Put on bottom of library".to_owned(),
                    ],
                });
                return Ok(StepOutcome::Pending(frame));
            };
            let concrete = if chosen_idx == 0 {
                crate::program::LibraryPlacement::Top
            } else {
                crate::program::LibraryPlacement::Bottom
            };
            let resolved_op = Instruction::PutTargetInLibrary(concrete);
            if self.begin_resolution_selection(&resolved_op, ctx.append)? {
                return Ok(StepOutcome::Continue);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ForEachPlayer { players, body } = instruction {
            let initialized = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .results
                .option_players_initialized;
            if !initialized {
                if body.instructions.is_empty() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let seats =
                    self.selection_players(players, entry.controller(), Some(&cursor_snap))?;
                if seats.is_empty() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.pending_option_players = seats.into();
                    cur.results.option_players_initialized = true;
                }
            }
            let next_seat = self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .results
                .pending_option_players
                .pop_front();
            if let Some(seat) = next_seat {
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.loop_player = Some(seat);
                }
                self.splice_before_current_op(entry.id(), index, &body.instructions)?;
                return Ok(StepOutcome::Continue);
            }
            if let Some(cur) = self.resolution_cursor_mut() {
                cur.results.option_players_initialized = false;
                cur.results.loop_player = None;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::OfferAnyPlayer {
            players,
            cost_body,
            paid_body,
            declined_body,
        } = instruction
        {
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
                    combined.push(Instruction::BranchIfFalse {
                        condition: crate::program::ResolutionCondition {
                            left: ValueExpr::event(
                                crate::program::EventValueKind::LastActionSucceeded,
                            ),
                            comparison: crate::program::Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        },
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
                        && let Some(Instruction::OfferAnyPlayer { declined_body, .. }) =
                            payload.body.instructions.get_mut(index)
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
        if let Instruction::DealDamageToAnnouncedTargets { amount, .. } = instruction {
            let resolved = self.resolve_value_expr(entry.controller(), &amount)?;
            if !resolved.is_positive() {
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = false;
                    cursor.results.last_action_count = Some(0.into());
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let assessment = self
                .window
                .target_assessment()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let source = self.resolution_damage_source(entry)?;
            let current = self.evaluated_characteristics()?;
            let mut inputs = Vec::new();
            for result in assessment.results() {
                if result.legality != crate::targets::TargetLegality::Legal {
                    continue;
                }
                let recipient = match result.choice.target {
                    TargetRef::Player(player) if self.window.participants().contains(&player) => {
                        Some(DamageRecipient::Player(player))
                    }
                    TargetRef::Object(object) => current
                        .get(object)
                        .filter(|view| view.zone == ObjectZone::Battlefield)
                        .and_then(|view| view.damage_recipient()),
                    TargetRef::Player(_) | TargetRef::Stack(_) => None,
                };
                if let Some(target) = recipient {
                    inputs.push(DamageInput {
                        source: source.clone(),
                        target,
                        amount: resolved.clone(),
                    });
                }
            }
            if inputs.is_empty() {
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = false;
                    cursor.results.last_action_count = Some(0.into());
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = true;
                cursor.results.last_action_count = Some(RulesInteger::from_usize(inputs.len()));
                cursor.results.dealt_damage_this_resolution = true;
            }
            if let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
                && let Some(slot) = payload.body.instructions.get_mut(index)
            {
                *slot = Instruction::DamageOrderedTargets {
                    first: resolved.clone(),
                    others: resolved,
                };
            }
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::RemoveCountersFromSubject {
            subject,
            kind,
            amount,
        } = instruction
        {
            let objects: Vec<ObjectId> = self.effect_group(entry, *subject)?.into_iter().collect();
            let total = self.remove_counters_from_objects(
                entry,
                index,
                &objects,
                kind,
                amount.as_ref(),
                ctx.append,
            )?;
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = total.is_positive();
                cursor.results.last_action_count = Some(total);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::MoveGroupToLibrary {
            group,
            mut placement,
        } = instruction
        {
            if let crate::program::LibraryPlacement::ChoiceTopOrBottom { chooser } = placement {
                let chosen = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_option_index.take());
                let Some(chosen_idx) = chosen else {
                    let cursor_snap = self
                        .resolution_cursor()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .clone();
                    let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                    let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: actor,
                        count: 2,
                        labels: vec![
                            "Put on top of library".to_owned(),
                            "Put on bottom of library".to_owned(),
                        ],
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                placement = if chosen_idx == 0 {
                    crate::program::LibraryPlacement::Top
                } else {
                    crate::program::LibraryPlacement::Bottom
                };
            }
            let mut objects: Vec<ObjectId> = self
                .effect_group(entry, crate::program::EffectSubject::Matching(group))?
                .into_iter()
                .collect();
            if placement == crate::program::LibraryPlacement::BottomRandom {
                match self.suspend_bottom_random_if_needed(entry, objects)? {
                    Some(ordered) => objects = ordered,
                    None => return Ok(StepOutcome::Continue),
                }
            }
            if !objects.is_empty() {
                let before_controllers: Vec<SeatId> = objects
                    .iter()
                    .filter_map(|&oid| {
                        self.window
                            .cards()
                            .get(oid)
                            .and_then(|o| o.controller().or(Some(o.owner())))
                    })
                    .collect();
                let moved_ids = self.move_objects_to_subject_destination_ids(
                    entry,
                    index,
                    entry.controller(),
                    &objects,
                    crate::program::SubjectDestination::Library(placement),
                    ctx.append,
                )?;
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = !moved_ids.is_empty();
                    cursor.results.last_action_count =
                        Some(RulesInteger::from_usize(moved_ids.len()));
                    cursor.results.last_moved_objects = moved_ids;
                    cursor.results.last_moved_controllers = before_controllers;
                }
            } else if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = false;
                cursor.results.last_action_count = Some(0.into());
                cursor.results.last_moved_objects.clear();
                cursor.results.last_moved_controllers.clear();
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ExileLibraryTopLinked {
            player,
            amount,
            group,
            face_down,
        } = instruction
        {
            let cursor_snap = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .clone();
            let seat = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
            let requested = self
                .resolve_value_expr(entry.controller(), &amount)?
                .max(0.into())
                .to_usize()
                .unwrap_or(0);
            let cards: Vec<ObjectId> = if self.window.participants().contains(&seat) {
                let library = self.window.cards().library(seat);
                library[..requested.min(library.len())].to_vec()
            } else {
                Vec::new()
            };
            if cards.is_empty() {
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = false;
                    cursor.results.last_action_count = Some(0.into());
                    cursor.results.last_moved_objects.clear();
                    cursor.results.moved_library_types.clear();
                    cursor.results.revealed_top_types.clear();
                    cursor.results.revealed_top_card = None;
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let moves: Vec<(ObjectId, ObjectDestination)> = cards
                .iter()
                .map(|&id| (id, ObjectDestination::Exile))
                .collect();
            let before = self.rules_snapshot(None)?;
            let (window, movements) = self.window.record_linked_return_moves(&moves)?;
            self.window = window;
            let source_id = match entry.kind() {
                StackKind::TriggeredAbility { source } | StackKind::ActivatedAbility { source } => {
                    Some(source.id())
                }
                StackKind::CardSpell(id) => Some(*id),
                _ => None,
            };
            let exiled_ids: Vec<ObjectId> = movements
                .iter()
                .filter(|m| m.after.zone() == ObjectZone::Exile)
                .flat_map(|m| m.arrivals())
                .collect();
            for &new_id in &exiled_ids {
                if face_down {
                    self.face_down_exile.insert(new_id, entry.controller());
                }
                if let Some(src) = source_id {
                    self.independent_exile_links
                        .entry((src, group))
                        .or_default()
                        .insert(new_id);
                }
            }
            let visible = self.evaluated_characteristics()?;
            let moved_types: BTreeSet<CardType> = movements
                .iter()
                .filter(|m| m.after.zone() == ObjectZone::Exile)
                .filter_map(|m| visible.get(m.after.id()))
                .flat_map(|view| view.current_types.iter().copied())
                .collect();
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_moved_objects = exiled_ids.clone();
                cursor.results.last_action_succeeded = !exiled_ids.is_empty();
                cursor.results.last_action_count = Some(RulesInteger::from_usize(exiled_ids.len()));
                cursor.results.moved_library_types = moved_types.clone();
                cursor.results.revealed_top_types = moved_types;
                cursor.results.revealed_top_card = exiled_ids.first().copied();
            }
            self.emit_from(
                crate::occurrence::EventCause::Instruction {
                    entry: entry.id(),
                    instruction: index,
                },
                &before,
                None,
                ctx.append,
                |previous, current, controllers| {
                    movements
                        .iter()
                        .map(|m| {
                            occurrence_flow::zone_occurrence_at(
                                previous,
                                current,
                                m,
                                crate::occurrence::ZoneReason::ExiledByEffect,
                                controllers,
                            )
                        })
                        .collect()
                },
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::MoveGraveyardGroupOwned {
            players,
            filter,
            mut destination,
        } = instruction
        {
            if let crate::program::SubjectDestination::Library(
                crate::program::LibraryPlacement::ChoiceTopOrBottom { chooser },
            ) = destination
            {
                let chosen = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_option_index.take());
                let Some(chosen_idx) = chosen else {
                    let cursor_snap = self
                        .resolution_cursor()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .clone();
                    let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                    let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: actor,
                        count: 2,
                        labels: vec![
                            "Put on top of library".to_owned(),
                            "Put on bottom of library".to_owned(),
                        ],
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                destination = crate::program::SubjectDestination::Library(if chosen_idx == 0 {
                    crate::program::LibraryPlacement::Top
                } else {
                    crate::program::LibraryPlacement::Bottom
                });
            }
            let cursor_snap = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .clone();
            let seats = self.selection_players(players, entry.controller(), Some(&cursor_snap))?;
            let evaluated = self.evaluated_characteristics()?;
            let excluded = BTreeSet::new();
            let mut objects = Vec::new();
            for seat in seats {
                for &id in self.window.cards().graveyard(seat) {
                    if evaluated.get(id).is_some_and(|card| {
                        filter.matches_static_characteristics(card, &excluded, || {
                            self.copiable_program(id).map(Program::mana_value)
                        })
                    }) {
                        objects.push(id);
                    }
                }
            }
            if matches!(
                destination,
                crate::program::SubjectDestination::Library(
                    crate::program::LibraryPlacement::BottomRandom
                )
            ) {
                match self.suspend_bottom_random_if_needed(entry, objects)? {
                    Some(ordered) => objects = ordered,
                    None => return Ok(StepOutcome::Continue),
                }
            }
            if !objects.is_empty() {
                let before_controllers: Vec<SeatId> = objects
                    .iter()
                    .filter_map(|&oid| {
                        self.window
                            .cards()
                            .get(oid)
                            .and_then(|o| o.controller().or(Some(o.owner())))
                    })
                    .collect();
                let moved_ids = self.move_objects_to_subject_destination_ids(
                    entry,
                    index,
                    entry.controller(),
                    &objects,
                    destination,
                    ctx.append,
                )?;
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = !moved_ids.is_empty();
                    cursor.results.last_action_count =
                        Some(RulesInteger::from_usize(moved_ids.len()));
                    cursor.results.last_moved_objects = moved_ids;
                    cursor.results.last_moved_controllers = before_controllers;
                }
            } else if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = false;
                cursor.results.last_action_count = Some(0.into());
                cursor.results.last_moved_objects.clear();
                cursor.results.last_moved_controllers.clear();
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DealDamageFrom {
            source,
            recipients,
            amount,
        } = instruction
        {
            use crate::program::{DamageAmountSpec, DamageRecipientsRef, DamageSourceRef};
            let current = self.evaluated_characteristics()?;
            let legal_target = |t: &&crate::targets::BoundTarget| {
                self.window.target_assessment().is_none_or(|assessment| {
                    assessment
                        .get(t.choice().id)
                        .is_some_and(|row| row.legality == crate::targets::TargetLegality::Legal)
                })
            };
            let source_ids: Vec<ObjectId> = match *source {
                DamageSourceRef::AbilitySource => match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => vec![source.id()],
                    StackKind::CardSpell(id) => vec![*id],
                    _ => Vec::new(),
                },
                DamageSourceRef::TargetObject => self
                    .instruction_target(entry)
                    .ok()
                    .filter(legal_target)
                    .and_then(|t| match t.choice().target {
                        TargetRef::Object(id) => Some(id),
                        _ => None,
                    })
                    .into_iter()
                    .collect(),
                DamageSourceRef::TriggeringObject => self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .map(|t| {
                        t.causes
                            .members()
                            .iter()
                            .filter_map(|m| m.subject_object_id())
                            .collect()
                    })
                    .unwrap_or_default(),
                DamageSourceRef::AttachedHost => match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => match self.attached_to(source.id())
                    {
                        Some(TargetRef::Object(host)) => vec![host],
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                },
                DamageSourceRef::Matching(filter) => self
                    .effect_group(entry, crate::program::EffectSubject::Matching(filter))?
                    .into_iter()
                    .collect(),
                DamageSourceRef::LastMoved => self
                    .resolution_cursor()
                    .map(|c| {
                        if !c.results.last_moved_objects.is_empty() {
                            c.results.last_moved_objects.clone()
                        } else {
                            c.results.returned_target_permanent.into_iter().collect()
                        }
                    })
                    .unwrap_or_default(),
                DamageSourceRef::CreatedTokens => self
                    .resolution_cursor()
                    .map(|c| c.last_created_tokens.clone())
                    .unwrap_or_default(),
                DamageSourceRef::AllPermanentTargets => {
                    if entry.targets().targets().is_empty() {
                        Vec::new()
                    } else {
                        self.legal_permanent_targets()?.into_iter().collect()
                    }
                }
            };
            let fixed_targets: Option<Vec<DamageRecipient>> = match recipients.as_ref() {
                DamageRecipientsRef::Target => Some(
                    self.instruction_target(entry)
                        .ok()
                        .filter(legal_target)
                        .and_then(|t| match t.choice().target {
                            TargetRef::Player(p) => Some(DamageRecipient::Player(p)),
                            TargetRef::Object(id) => {
                                current.get(id).and_then(|v| v.damage_recipient())
                            }
                            TargetRef::Stack(_) => None,
                        })
                        .into_iter()
                        .collect(),
                ),
                DamageRecipientsRef::TargetController => Some(
                    self.instruction_target(entry)
                        .ok()
                        .filter(legal_target)
                        .and_then(|t| match t.choice().target {
                            TargetRef::Object(id) => current
                                .get(id)
                                .and_then(|v| v.controller)
                                .or_else(|| {
                                    self.resolution_cursor()
                                        .and_then(|c| c.results.target_lki.get(&id))
                                        .and_then(|lki| lki.controller)
                                })
                                .or_else(|| {
                                    self.resolution_cursor()
                                        .and_then(|c| c.results.destroyed_target_controller)
                                })
                                .map(DamageRecipient::Player),
                            TargetRef::Player(p) => Some(DamageRecipient::Player(p)),
                            TargetRef::Stack(sid) => self
                                .window
                                .entries()
                                .iter()
                                .find(|e| e.id() == sid)
                                .map(|e| DamageRecipient::Player(e.controller()))
                                .or_else(|| {
                                    self.resolution_cursor()
                                        .and_then(|c| c.results.countered_spell_controller)
                                        .filter(|(id, _)| *id == sid)
                                        .map(|(_, p)| DamageRecipient::Player(p))
                                }),
                        })
                        .into_iter()
                        .collect(),
                ),
                DamageRecipientsRef::AbilitySource => Some(match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => current
                        .get(source.id())
                        .and_then(|v| v.damage_recipient())
                        .into_iter()
                        .collect(),
                    _ => Vec::new(),
                }),
                DamageRecipientsRef::TriggeringObject => Some(
                    self.execution_bindings(entry.id())
                        .and_then(|b| b.trigger.as_ref())
                        .map(|t| {
                            t.causes
                                .members()
                                .iter()
                                .filter_map(|m| m.subject_object_id())
                                .filter_map(|id| current.get(id).and_then(|v| v.damage_recipient()))
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                DamageRecipientsRef::AttachedHost => Some(match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => match self.attached_to(source.id())
                    {
                        Some(TargetRef::Object(host)) => current
                            .get(host)
                            .and_then(|v| v.damage_recipient())
                            .into_iter()
                            .collect(),
                        Some(TargetRef::Player(p)) => vec![DamageRecipient::Player(p)],
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                }),
                DamageRecipientsRef::Group(group) => {
                    Some(self.group_damage_recipients(*group, entry.controller())?)
                }
                DamageRecipientsRef::LastMoved => Some(
                    self.resolution_cursor()
                        .map(|c| {
                            let ids: Vec<ObjectId> = if !c.results.last_moved_objects.is_empty() {
                                c.results.last_moved_objects.clone()
                            } else {
                                c.results.returned_target_permanent.into_iter().collect()
                            };
                            ids.into_iter()
                                .filter_map(|id| current.get(id).and_then(|v| v.damage_recipient()))
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                DamageRecipientsRef::CreatedTokens => Some(
                    self.resolution_cursor()
                        .map(|c| {
                            c.last_created_tokens
                                .iter()
                                .filter_map(|&id| {
                                    current.get(id).and_then(|v| v.damage_recipient())
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                DamageRecipientsRef::OtherCreaturesAndEachPlayer
                | DamageRecipientsRef::EachOtherPlayer
                | DamageRecipientsRef::EachOtherOpponent => None,
            };
            if source_ids.is_empty() || fixed_targets.as_ref().is_some_and(Vec::is_empty) {
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = false;
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let fixed_amount = match amount.as_ref() {
                DamageAmountSpec::Expr(expr) => {
                    Some(self.resolve_value_expr(entry.controller(), expr)?)
                }
                DamageAmountSpec::SourcePower | DamageAmountSpec::SourceToughness => None,
            };
            let mut inputs = Vec::new();
            for src_id in source_ids {
                let src_view = current.get(src_id);
                let lki = self
                    .resolution_cursor()
                    .and_then(|c| c.results.target_lki.get(&src_id));
                let src_card = self.window.cards().get(src_id).cloned().or_else(|| {
                    lki.map(|info| {
                        GameObject::synthetic_emblem(info.controller.unwrap_or(info.owner))
                    })
                });
                let Some(src_card) = src_card else {
                    continue;
                };
                let src_controller = src_view
                    .and_then(|v| v.controller)
                    .or_else(|| lki.and_then(|info| info.controller.or(Some(info.owner))))
                    .unwrap_or_else(|| entry.controller());
                if !self.window.participants().contains(&src_controller) {
                    continue;
                }
                let dmg_amount = match amount.as_ref() {
                    DamageAmountSpec::Expr(_) => fixed_amount.clone().unwrap_or_else(|| 0.into()),
                    DamageAmountSpec::SourcePower => src_view
                        .and_then(|v| v.power_toughness.as_ref())
                        .map(|pt| pt.derived.power.clone())
                        .or_else(|| lki.and_then(|info| info.power.clone()))
                        .unwrap_or_else(|| 0.into()),
                    DamageAmountSpec::SourceToughness => src_view
                        .and_then(|v| v.power_toughness.as_ref())
                        .map(|pt| pt.derived.toughness.clone())
                        .or_else(|| lki.and_then(|info| info.toughness.clone()))
                        .unwrap_or_else(|| 0.into()),
                };
                if !dmg_amount.is_positive() {
                    continue;
                }
                let dmg_source = DamageSource::Object {
                    object: src_card.clone(),
                    controller: src_controller,
                };
                if let Some(target_list) = &fixed_targets {
                    for recipient in target_list {
                        inputs.push(DamageInput {
                            source: dmg_source.clone(),
                            target: *recipient,
                            amount: dmg_amount.clone(),
                        });
                    }
                } else {
                    let per_source: Vec<DamageRecipient> = match recipients.as_ref() {
                        DamageRecipientsRef::OtherCreaturesAndEachPlayer => {
                            let mut list: Vec<DamageRecipient> = self
                                .window
                                .participants()
                                .iter()
                                .copied()
                                .map(DamageRecipient::Player)
                                .collect();
                            list.extend(
                                current
                                    .objects
                                    .values()
                                    .filter(|o| {
                                        o.zone == ObjectZone::Battlefield
                                            && o.is_creature()
                                            && o.object != src_id
                                    })
                                    .map(|o| DamageRecipient::Creature(o.object)),
                            );
                            list
                        }
                        DamageRecipientsRef::EachOtherPlayer => self
                            .window
                            .participants()
                            .iter()
                            .copied()
                            .filter(|&p| p != src_controller)
                            .map(DamageRecipient::Player)
                            .collect(),
                        DamageRecipientsRef::EachOtherOpponent => self
                            .combat_rules
                            .opponents(self.window.participants(), src_controller)?
                            .into_iter()
                            .map(DamageRecipient::Player)
                            .collect(),
                        _ => Vec::new(),
                    };
                    for recipient in per_source {
                        inputs.push(DamageInput {
                            source: dmg_source.clone(),
                            target: recipient,
                            amount: dmg_amount.clone(),
                        });
                    }
                }
            }
            if inputs.is_empty() {
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = false;
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = true;
                cursor.results.dealt_damage_this_resolution = true;
            }
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateTemporaryRule { rule, duration } = instruction {
            use crate::program::TemporaryRuleKind;
            if let Some(resolved_dur) = self.resolve_duration(entry, duration)? {
                let source_object = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    StackKind::CardSpell(id) => Some(*id),
                    _ => None,
                };
                let bound_target = self
                    .instruction_target(entry)
                    .ok()
                    .map(|t| t.choice().target);
                let second_bound_target =
                    entry.targets().targets().get(1).map(|t| t.choice().target);
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
                            (
                                entry.controller(),
                                crate::damage::TurnPreventionFilter::Combat,
                                None,
                            ),
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
                        });
                    }
                    other => {
                        let cursor = self.resolution_cursor().cloned();
                        let resolved_players: BTreeSet<SeatId> = match &other {
                            TemporaryRuleKind::PlayerCantGainLife { players }
                            | TemporaryRuleKind::CantCastSpells { players, .. }
                            | TemporaryRuleKind::CantActivateAbilities { players, .. }
                            | TemporaryRuleKind::CantPlayLands { players }
                            | TemporaryRuleKind::PlayerGrantKeyword { players, .. }
                            | TemporaryRuleKind::SpellCostIncrease { players, .. }
                            | TemporaryRuleKind::ExileSpellOnResolve { players, .. } => self
                                .selection_players(*players, entry.controller(), cursor.as_ref())?
                                .into_iter()
                                .collect(),
                            TemporaryRuleKind::AdditionalLandPlay { player, .. }
                            | TemporaryRuleKind::FilteredFlash { player, .. }
                            | TemporaryRuleKind::ZoneCastPermission { player, .. } => {
                                [self.resolving_player_identity_at(*player, cursor.as_ref())?]
                                    .into_iter()
                                    .collect()
                            }
                            _ => BTreeSet::new(),
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
                            resolved_objects: BTreeSet::new(),
                            target: bound_target,
                            second_target: second_bound_target,
                            consumed: false,
                        });
                    }
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AttachPermanent { attachment, host } = instruction {
            let resolve_ref = |subj: crate::program::MoveSubjectRef| -> Option<TargetRef> {
                match subj {
                    crate::program::MoveSubjectRef::TargetObject => self
                        .instruction_target(entry)
                        .ok()
                        .map(|t| t.choice().target),
                    crate::program::MoveSubjectRef::AbilitySource => match entry.kind() {
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => {
                            Some(TargetRef::Object(source.id()))
                        }
                        _ => None,
                    },
                    crate::program::MoveSubjectRef::TriggeringObject => self
                        .execution_bindings(entry.id())
                        .and_then(|b| b.trigger.as_ref())
                        .and_then(|t| t.causes.members().first())
                        .and_then(|m| m.subject_object_id())
                        .map(TargetRef::Object),
                    crate::program::MoveSubjectRef::AttachedHost => match entry.kind() {
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => self.attached_to(source.id()),
                        _ => None,
                    },
                    crate::program::MoveSubjectRef::RevealedCard => None,
                    crate::program::MoveSubjectRef::CreatedTokens => self
                        .resolution_cursor()
                        .and_then(|c| c.last_created_tokens.first().copied())
                        .map(TargetRef::Object),
                    crate::program::MoveSubjectRef::LastMoved => self
                        .resolution_cursor()
                        .and_then(|c| {
                            c.results
                                .last_moved_objects
                                .first()
                                .copied()
                                .or(c.results.returned_target_permanent)
                        })
                        .map(TargetRef::Object),
                    crate::program::MoveSubjectRef::RememberedFirst => self
                        .legal_linked_targets()
                        .ok()
                        .flatten()
                        .map(|(a, _)| TargetRef::Object(a)),
                    crate::program::MoveSubjectRef::RememberedSecond => self
                        .legal_linked_targets()
                        .ok()
                        .flatten()
                        .map(|(_, b)| TargetRef::Object(b)),
                }
            };
            if let (Some(TargetRef::Object(att_id)), Some(host_ref)) =
                (resolve_ref(attachment), resolve_ref(host))
            {
                let before = self.rules_snapshot(None)?;
                if let Some(change) = self.attach_if_legal(entry.id(), att_id, host_ref)? {
                    self.emit_from(
                        crate::occurrence::EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |_, _, _| Ok(vec![change]),
                    )?;
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::SkipNextUntap { subject } = instruction {
            let affected = self.effect_group(entry, *subject)?;
            self.skip_next_controller_untap.extend(affected);
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::GrantCardPlayPermission {
            player,
            subject,
            duration,
            cast_only,
            free,
            any_color,
        } = instruction
        {
            let cursor_snap = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .clone();
            let grantee = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
            let maybe_obj = match subject {
                crate::program::MoveSubjectRef::TargetObject => self
                    .instruction_target(entry)
                    .ok()
                    .and_then(|t| match t.choice().target {
                        TargetRef::Object(id) => Some(id),
                        _ => None,
                    }),
                crate::program::MoveSubjectRef::AbilitySource => match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    StackKind::CardSpell(id) => Some(*id),
                    _ => None,
                },
                crate::program::MoveSubjectRef::TriggeringObject => self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .and_then(|t| t.causes.members().first())
                    .and_then(|m| m.subject_object_id()),
                crate::program::MoveSubjectRef::AttachedHost => match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => {
                        match self.attachment_host_for_ability(source.id()) {
                            Some(TargetRef::Object(host)) => Some(host),
                            _ => None,
                        }
                    }
                    _ => None,
                },
                crate::program::MoveSubjectRef::RevealedCard => {
                    cursor_snap.impulse.exiled.or_else(|| {
                        self.window
                            .cards()
                            .library(entry.controller())
                            .first()
                            .copied()
                    })
                }
                crate::program::MoveSubjectRef::CreatedTokens => {
                    cursor_snap.last_created_tokens.first().copied()
                }
                crate::program::MoveSubjectRef::LastMoved => cursor_snap
                    .results
                    .last_moved_objects
                    .first()
                    .copied()
                    .or(cursor_snap.results.returned_target_permanent),
                crate::program::MoveSubjectRef::RememberedFirst => {
                    self.legal_linked_targets()?.map(|(a, _)| a)
                }
                crate::program::MoveSubjectRef::RememberedSecond => {
                    self.legal_linked_targets()?.map(|(_, b)| b)
                }
            };
            if let Some(obj_id) = maybe_obj
                && let Some(card) = self.window.cards().get(obj_id)
            {
                let zone_opt = match card.zone() {
                    ObjectZone::Exile => Some(super::CardPlayZone::Exile),
                    ObjectZone::Graveyard(_) => Some(super::CardPlayZone::Graveyard),
                    ObjectZone::Hand(_) => Some(super::CardPlayZone::Hand),
                    _ => None,
                };
                if let Some(zone) = zone_opt {
                    let is_land = self
                        .copiable_program(obj_id)
                        .is_some_and(|p| p.has_type(CardType::Land));
                    if !(cast_only && is_land) {
                        let mapped_duration = match duration {
                            crate::program::ImpulsePlayDuration::ThisTurn => {
                                super::CardPlayDuration::ThroughTurn
                            }
                            crate::program::ImpulsePlayDuration::UntilEndOfNextTurn => {
                                if self.window.active_player() == grantee {
                                    super::CardPlayDuration::ThroughNextOwnTurnSkipCurrent(grantee)
                                } else {
                                    super::CardPlayDuration::ThroughNextOwnTurn(grantee)
                                }
                            }
                            crate::program::ImpulsePlayDuration::WhileExiled
                            | crate::program::ImpulsePlayDuration::UntilNextFromSource => {
                                super::CardPlayDuration::WhileInZone
                            }
                        };
                        self.card_play_permissions
                            .insert(super::CardPlayPermission {
                                zone,
                                object: obj_id,
                                player: grantee,
                                duration: mapped_duration,
                                any_color,
                                free,
                                cast_only,
                            });
                        if free && zone == super::CardPlayZone::Exile {
                            self.free_exiled_cards.insert((obj_id, grantee));
                        }
                    }
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::RequireCombatAction { subject, kind } = instruction {
            let affected: Vec<ObjectId> = self.effect_group(entry, *subject)?.into_iter().collect();
            self.apply_combat_action(entry, index, &affected, kind, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateTokenCopy {
            source,
            count,
            tapped,
            grant_keywords,
            nonlegendary,
            override_pt,
            override_colors,
            add_subtypes,
            add_card_types,
            strip_mana_cost,
            recipients,
            ..
        } = instruction
        {
            let resolved_count = self
                .resolve_value_expr(entry.controller(), &count)?
                .max(0.into())
                .to_usize()
                .unwrap_or(0);
            let donor_id = match source {
                crate::program::MoveSubjectRef::TargetObject => self
                    .instruction_target(entry)
                    .ok()
                    .and_then(|t| match t.choice().target {
                        TargetRef::Object(id) => Some(id),
                        _ => None,
                    }),
                crate::program::MoveSubjectRef::AbilitySource => match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                },
                crate::program::MoveSubjectRef::TriggeringObject => self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .and_then(|t| t.causes.members().first())
                    .and_then(|m| m.subject_object_id()),
                crate::program::MoveSubjectRef::AttachedHost => match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => match self.attached_to(source.id())
                    {
                        Some(TargetRef::Object(host)) => Some(host),
                        _ => None,
                    },
                    _ => None,
                },
                crate::program::MoveSubjectRef::RevealedCard => None,
                crate::program::MoveSubjectRef::CreatedTokens => self
                    .resolution_cursor()
                    .and_then(|c| c.last_created_tokens.first().copied()),
                crate::program::MoveSubjectRef::LastMoved => {
                    self.resolution_cursor().and_then(|c| {
                        c.results
                            .last_moved_objects
                            .first()
                            .copied()
                            .or(c.results.returned_target_permanent)
                    })
                }
                crate::program::MoveSubjectRef::RememberedFirst => {
                    self.legal_linked_targets()?.map(|(a, _)| a)
                }
                crate::program::MoveSubjectRef::RememberedSecond => {
                    self.legal_linked_targets()?.map(|(_, b)| b)
                }
            };
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
                program
                    .printed_keywords
                    .extend(grant_keywords.iter().copied());
                if let Some((p, t)) = override_pt
                    && let crate::program::ProgramKind::Permanent(perm) = &mut program.kind
                {
                    perm.printed_pt = Some(crate::characteristics::PrintedPowerToughness {
                        power: crate::characteristics::PrintedStat::Number(i64::from(p).into()),
                        toughness: crate::characteristics::PrintedStat::Number(i64::from(t).into()),
                    });
                }
                if let Some(colors) = override_colors {
                    program.color_indicator = Some(colors.clone());
                    program.effect_defined_colors = Some(colors);
                }
                program
                    .creature_subtypes
                    .extend(add_subtypes.iter().copied());
                program.card_types.extend(add_card_types.iter().copied());
                if strip_mana_cost {
                    if let crate::program::ProgramKind::Permanent(perm) = &mut program.kind {
                        perm.casting_cost = None;
                    }
                    program.x_mana_symbols = 0;
                    program.hybrid_symbols.clear();
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
                            groups.push((creator, total, program.clone(), tapped));
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
                        groups.push((creator, resolved_count, program.clone(), tapped));
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
        if let Instruction::CreateTokenCopyExt {
            source,
            count,
            recipients,
            enters_tapped,
            enters_attacking,
            haste,
            sacrifice_at_end_step,
            exile_at_end_step,
            nonlegendary,
            extra_types,
            extra_subtypes,
            extra_keywords,
            override_pt,
            extra_colors,
            ..
        } = instruction
        {
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
                    if matches!(
                        &program.continuous,
                        Some(crate::characteristics::StaticContinuousAbility::CharacteristicDefining {
                            parts,
                            ..
                        }) if parts.0.iter().any(|part| matches!(
                            part,
                            crate::characteristics::DefiningPart::PowerToughness { .. }
                        ))
                    ) {
                        program.continuous = None;
                    }
                }
                if !extra_colors.is_empty() {
                    let mut merged = program.defined_colors();
                    merged.extend(extra_colors.iter().copied());
                    program.color_indicator = Some(merged.clone());
                    program.effect_defined_colors = Some(merged);
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
                    if (enters_attacking || sacrifice_at_end_step || exile_at_end_step)
                        && let Some(StackPayload::Instructions(payload)) =
                            self.payloads.get_mut(&entry.id())
                        && let Some(slot) = payload.body.instructions.get_mut(index)
                    {
                        *slot = Instruction::CreateTokenCopy {
                            source,
                            count: ValueExpr::Literal(0.into()),
                            tapped: enters_tapped,
                            attacking: enters_attacking,
                            sacrifice_at_end_step,
                            exile_at_end_step,
                            grant_keywords: BTreeSet::new(),
                            nonlegendary: false,
                            override_pt: None,
                            override_colors: None,
                            add_subtypes: BTreeSet::new(),
                            add_card_types: BTreeSet::new(),
                            strip_mana_cost: false,
                            recipients,
                        };
                    }
                    self.begin_materialized_token_entries(groups, None, false)?;
                    return Ok(StepOutcome::Continue);
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::BindResolutionX(expr) = instruction {
            let value = self
                .resolve_value_expr(entry.controller(), &expr)?
                .max(0.into());
            self.window.set_resolving_chosen_x(value)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::PutResolvingSpellInLibrary(mut placement) = instruction {
            if let crate::program::LibraryPlacement::ChoiceTopOrBottom { chooser } = placement {
                let chosen = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_option_index.take());
                let Some(chosen_idx) = chosen else {
                    let cursor_snap = self
                        .resolution_cursor()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .clone();
                    let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                    let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: actor,
                        count: 2,
                        labels: vec![
                            "Put on top of library".to_owned(),
                            "Put on bottom of library".to_owned(),
                        ],
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                placement = if chosen_idx == 0 {
                    crate::program::LibraryPlacement::Top
                } else {
                    crate::program::LibraryPlacement::Bottom
                };
            }
            if let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id()) {
                payload.library_on_resolve = Some(placement);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::PutSubjectIntoLibraryFromTop { subject, offset } = instruction {
            let resolved_offset = self
                .resolve_value_expr(entry.controller(), &offset)?
                .max(0.into())
                .to_usize()
                .unwrap_or(0);
            let offset_u8 = u8::try_from(resolved_offset).unwrap_or(u8::MAX);
            let target_objs = self.resolve_move_subject_objects(entry, subject)?;
            if !target_objs.is_empty() {
                let before_controllers: Vec<SeatId> = target_objs
                    .iter()
                    .filter_map(|&oid| {
                        self.window
                            .cards()
                            .get(oid)
                            .and_then(|o| o.controller().or(Some(o.owner())))
                    })
                    .collect();
                let moved_ids = self.move_objects_to_subject_destination_ids(
                    entry,
                    index,
                    entry.controller(),
                    &target_objs,
                    crate::program::SubjectDestination::Library(
                        crate::program::LibraryPlacement::FromTop(offset_u8),
                    ),
                    ctx.append,
                )?;
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = !moved_ids.is_empty();
                    cursor.results.last_action_count =
                        Some(RulesInteger::from_usize(moved_ids.len()));
                    cursor.results.last_moved_objects = moved_ids;
                    cursor.results.last_moved_controllers = before_controllers;
                }
            } else if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = false;
                cursor.results.last_action_count = Some(0.into());
                cursor.results.last_moved_objects.clear();
                cursor.results.last_moved_controllers.clear();
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::SetDayNight(clock) = instruction {
            let previous = self.day_night();
            if previous != clock {
                let before = self.rules_snapshot(None)?;
                let (_, faces) = self.reconcile_day_night(Some(clock), &before.view().evaluated)?;
                self.transform_orders.clear();
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |old, new, _| Self::day_change_occurrences(previous, clock, &faces, old, new),
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateAuraTokenAttached {
            definition,
            host,
            up_to_one,
        } = instruction
        {
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
        if let Instruction::MoveSubject {
            subject,
            mut destination,
        } = instruction
        {
            if let crate::program::SubjectDestination::Library(
                crate::program::LibraryPlacement::ChoiceTopOrBottom { chooser },
            ) = destination
            {
                let chosen = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_option_index.take());
                let Some(chosen_idx) = chosen else {
                    let cursor_snap = self
                        .resolution_cursor()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .clone();
                    let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                    let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: actor,
                        count: 2,
                        labels: vec![
                            "Put on top of library".to_owned(),
                            "Put on bottom of library".to_owned(),
                        ],
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                destination = crate::program::SubjectDestination::Library(if chosen_idx == 0 {
                    crate::program::LibraryPlacement::Top
                } else {
                    crate::program::LibraryPlacement::Bottom
                });
            }
            let target_objs = self.resolve_move_subject_objects(entry, subject)?;
            if !target_objs.is_empty() {
                let before_controllers: Vec<SeatId> = target_objs
                    .iter()
                    .filter_map(|&oid| {
                        self.window
                            .cards()
                            .get(oid)
                            .and_then(|o| o.controller().or(Some(o.owner())))
                    })
                    .collect();
                let moved_ids = self.move_objects_to_subject_destination_ids(
                    entry,
                    index,
                    entry.controller(),
                    &target_objs,
                    destination,
                    ctx.append,
                )?;
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = !moved_ids.is_empty();
                    cursor.results.last_action_count =
                        Some(RulesInteger::from_usize(moved_ids.len()));
                    cursor.results.last_moved_objects = moved_ids;
                    cursor.results.last_moved_controllers = before_controllers;
                }
            } else if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = false;
                cursor.results.last_action_count = Some(0.into());
                cursor.results.last_moved_objects.clear();
                cursor.results.last_moved_controllers.clear();
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::SelectZoneCardsToZone {
            player,
            source_zone,
            ref filter,
            ref min,
            ref max,
            mut destination,
            reveal,
        } = instruction
            && let crate::program::SubjectDestination::Library(
                crate::program::LibraryPlacement::ChoiceTopOrBottom { chooser },
            ) = destination
        {
            let chosen = self
                .resolution_cursor_mut()
                .and_then(|cursor| cursor.chosen_option_index.take());
            let Some(chosen_idx) = chosen else {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let actor = self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                    revision: self.revision,
                    entry: entry.id(),
                    instruction: index,
                    chooser: actor,
                    count: 2,
                    labels: vec![
                        "Put on top of library".to_owned(),
                        "Put on bottom of library".to_owned(),
                    ],
                });
                return Ok(StepOutcome::Pending(frame));
            };
            destination = crate::program::SubjectDestination::Library(if chosen_idx == 0 {
                crate::program::LibraryPlacement::Top
            } else {
                crate::program::LibraryPlacement::Bottom
            });
            let resolved_op = Instruction::SelectZoneCardsToZone {
                player,
                source_zone,
                filter: filter.clone(),
                min: min.clone(),
                max: max.clone(),
                destination,
                reveal,
            };
            if self.begin_resolution_selection(&resolved_op, ctx.append)? {
                return Ok(StepOutcome::Continue);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if self.begin_resolution_selection(&instruction, ctx.append)? {
            return Ok(StepOutcome::Continue);
        }
        if matches!(instruction, Instruction::ImpulseCastOrDamage(_))
            && self
                .resolution_cursor()
                .is_some_and(|cursor| !cursor.impulse.started)
            && self.begin_impulse(entry, index, false)?
        {
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReturnTokenLinkedExile {
            let binding = self
                .execution_bindings(entry.id())
                .and_then(|bindings| bindings.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let [member] = binding.causes.members() else {
                return Err(EffectError::InconsistentProgram);
            };
            let crate::occurrence::TriggerRelated::ZoneChange { before, .. } = &member.related
            else {
                return Err(EffectError::InconsistentProgram);
            };
            let token = before.object.id();
            let exiled = self.token_linked_exiles.remove(&token).flatten();
            if let Some(object) = exiled
                && self.begin_linked_exile_to_hand(object)?
            {
                return Ok(StepOutcome::Continue);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ScheduleCreatedTokenSacrifice {
            let cursor = self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let tokens = std::mem::take(&mut cursor.last_created_tokens);
            if !tokens.is_empty() {
                self.delay_spell_sacrifice(entry.id(), entry.controller(), tokens)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ScheduleCreatedTokenExile(step) = instruction {
            let cursor = self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let tokens = std::mem::take(&mut cursor.last_created_tokens);
            if !tokens.is_empty() {
                self.delay_entry_exile(entry.id(), entry.controller(), step, tokens)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RegisterTokenLinkedExile {
            let cursor = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let tokens = cursor.last_created_tokens.clone();
            let exiled = cursor.impulse.exiled;
            if !tokens.is_empty() {
                let StackKind::ActivatedAbility { source } = entry.kind() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let source_id = source.id();
                let snapshot = self.rules_snapshot(None)?;
                let appearance = snapshot
                    .view()
                    .appearance(source_id)
                    .or_else(|| self.departed_reflexive_sources.get(&source_id).cloned())
                    .ok_or(EffectError::InconsistentProgram)?;
                for token in tokens {
                    self.triggers
                        .delay_token_leave(token, appearance.clone(), entry.controller());
                    self.token_linked_exiles.insert(token, exiled);
                }
                // Other abilities of this source may still need its last appearance.
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ExileTopForFreePlay
                | Instruction::ExileTargetUntilNonlandForFreeCast
                | Instruction::ExileTopForEachSpellType(_)
                | Instruction::ExileTopFreeNoncreatureSpells(_)
                | Instruction::ExileRelatedTopForCastThisTurn
                | Instruction::ExileRelatedTopForPlay
                | Instruction::ExileTopForPlay { .. }
                | Instruction::ExileTopPlay { .. }
        ) {
            let count = match &instruction {
                Instruction::ExileTopForEachSpellType(count)
                | Instruction::ExileTopFreeNoncreatureSpells(count) => usize::from(*count),
                Instruction::ExileTopPlay { count, .. } => self
                    .resolve_value_expr(entry.controller(), count)?
                    .max(0.into())
                    .min(RulesInteger::from_usize(1024))
                    .to_usize()
                    .ok_or(EffectError::InconsistentProgram)?,
                _ => 1,
            };
            if count == 0 || !self.begin_exile_top_for_play(entry, index, count)? {
                self.advance_instruction()?;
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExileTopRepeatOnPermanentEntry {
            self.progress_permanent_exile_loop(entry, index, ctx.append)?;
            if let Some(frame) = self.pending_resolution_decision() {
                return Ok(StepOutcome::Pending(frame));
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExileTopFaceDown {
            if !self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .impulse
                .started
                && self.begin_impulse(entry, index, true)?
            {
                return Ok(StepOutcome::Continue);
            }
            self.advance_instruction()?;
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
        if matches!(
            instruction,
            Instruction::SearchLibrary { .. } | Instruction::ExileFromTargetLibrary
        ) {
            let searcher = match &instruction {
                Instruction::SearchLibrary { filter } => match filter.searcher {
                    crate::program::ResolvingPlayer::Controller => entry.controller(),
                    other => self.resolving_player(other)?,
                },
                _ => entry.controller(),
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
        if let Instruction::AttachOnEntry { entering_equipment } = instruction {
            let (equipment, host) = self.entry_attachment(entering_equipment)?;
            let before = self.rules_snapshot(None)?;
            if let Some(event) =
                self.attach_at_cut(entry.id(), equipment, host, &before.view().evaluated)?
            {
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| Ok(vec![event]),
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::UntapControlledLands {
            let before = self.rules_snapshot(None)?;
            let lands = before
                .view()
                .evaluated
                .objects
                .values()
                .filter(|object| {
                    object.zone == ObjectZone::Battlefield
                        && object.controller == Some(entry.controller())
                        && object.has_type(CardType::Land)
                        && self
                            .window
                            .cards()
                            .get(object.object)
                            .and_then(GameObject::tapped)
                            == Some(true)
                })
                .map(|object| object.object)
                .collect::<Vec<_>>();
            if !lands.is_empty() {
                let occurrences = self.untap_for_effect_with_stun(entry.id(), &lands)?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| Ok(occurrences),
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DamageCapturedBlocker(amount) = instruction {
            let binding = self
                .execution_bindings(entry.id())
                .and_then(|bindings| bindings.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let [member] = binding.causes.members() else {
                return Err(EffectError::InconsistentProgram);
            };
            let crate::occurrence::TriggerRelated::Combatant(blocker) = member.related else {
                return Err(EffectError::InconsistentProgram);
            };
            if self
                .current_characteristics(blocker)?
                .is_some_and(|view| view.zone == ObjectZone::Battlefield && view.is_creature())
            {
                let source = self.resolution_damage_source(entry)?;
                self.install_damage_batch(
                    DamageOrigin::Resolution {
                        entry: entry.id(),
                        instruction: index,
                    },
                    DamageKind::Noncombat,
                    vec![DamageInput {
                        source,
                        target: DamageRecipient::Creature(blocker),
                        amount: i64::from(amount).into(),
                    }],
                    ctx.journal,
                )?;
            } else {
                self.advance_instruction()?;
            }
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ControlledSubtypeDamagesAttacker(kind) = &instruction {
            let binding = self
                .execution_bindings(entry.id())
                .and_then(|b| b.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let [member] = binding.causes.members() else {
                return Err(EffectError::InconsistentProgram);
            };
            let crate::occurrence::TriggerRelated::Combatant(attacker) = member.related else {
                return Err(EffectError::InconsistentProgram);
            };
            let current = self.evaluated_characteristics()?;
            let recipient = current
                .get(attacker)
                .filter(|value| value.zone == ObjectZone::Battlefield)
                .and_then(|value| {
                    if value.is_creature() {
                        Some(DamageRecipient::Creature(attacker))
                    } else if value.has_type(CardType::Planeswalker) {
                        Some(DamageRecipient::Planeswalker(attacker))
                    } else {
                        None
                    }
                });
            if let Some(target) = recipient {
                let inputs = current
                    .objects
                    .values()
                    .filter(|value| {
                        value.zone == ObjectZone::Battlefield
                            && value.controller == Some(entry.controller())
                            && value.current_creature_subtypes.contains(kind)
                    })
                    .map(|value| {
                        Ok(DamageInput {
                            source: DamageSource::Object {
                                object: self
                                    .window
                                    .cards()
                                    .get(value.object)
                                    .ok_or(EffectError::UnknownObject)?
                                    .clone(),
                                controller: entry.controller(),
                            },
                            target,
                            amount: 1.into(),
                        })
                    })
                    .collect::<Result<Vec<_>, EffectError>>()?;
                if !inputs.is_empty() {
                    self.install_damage_batch(
                        DamageOrigin::Resolution {
                            entry: entry.id(),
                            instruction: index,
                        },
                        DamageKind::Noncombat,
                        inputs,
                        ctx.journal,
                    )?;
                    return Ok(StepOutcome::Continue);
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ChangeControlledPlaneswalkers { parts, duration }
        | Instruction::ChangeGroup {
            parts, duration, ..
        } = &instruction
        {
            let before = self.rules_snapshot(None)?;
            let affected: BTreeSet<_> = match &instruction {
                Instruction::ChangeGroup { group, .. } => self
                    .effect_group(entry, *group.as_ref())?
                    .into_iter()
                    .collect(),
                _ => before
                    .view()
                    .evaluated
                    .objects
                    .values()
                    .filter(|value| {
                        value.zone == ObjectZone::Battlefield
                            && value.has_type(CardType::Planeswalker)
                            && value.controller == Some(entry.controller())
                    })
                    .map(|value| value.object)
                    .collect(),
            };
            if !affected.is_empty() {
                let Some(resolved_duration) = self.resolve_duration(entry, *duration)? else {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                };
                let locals = self
                    .resolution_state()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .locals()
                    .clone();
                let source_object = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                };
                let needs_per_owner = parts.0.iter().any(|p| {
                    matches!(
                        p,
                        crate::characteristics::ResolvedPartSpec::Control(
                            crate::characteristics::ControlRecipient::TargetOwner
                        )
                    )
                });
                let groups: Vec<(SeatId, BTreeSet<ObjectId>)> = if needs_per_owner {
                    let mut by_owner: BTreeMap<SeatId, BTreeSet<ObjectId>> = BTreeMap::new();
                    for id in affected {
                        if let Some(card) = self.window.cards().get(id) {
                            by_owner.entry(card.owner()).or_default().insert(id);
                        }
                    }
                    by_owner.into_iter().collect()
                } else {
                    vec![(entry.controller(), affected)]
                };
                let mut created_effects = Vec::new();
                for (owner_seat, group_ids) in groups {
                    let materialized = self.materialize_resolved_parts(
                        entry,
                        parts,
                        &locals,
                        owner_seat,
                        source_object,
                    )?;
                    let effect = self.characteristics.add_effect_for_objects(
                        entry.id(),
                        group_ids,
                        materialized,
                        resolved_duration,
                    )?;
                    created_effects.push(effect);
                }
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |old, new, _| {
                        let mut occs = Vec::with_capacity(created_effects.len());
                        for effect in created_effects {
                            let appearances = occurrence_flow::characteristic_appearance_changes(
                                old,
                                new,
                                effect.affected.iter().copied(),
                            )?;
                            occs.push(
                                crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                    effect,
                                    appearances,
                                },
                            );
                        }
                        Ok(occs)
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ChangeAbilitySource { .. } | Instruction::GiveControlOfAbilitySource
        ) {
            let (StackKind::TriggeredAbility { source } | StackKind::ActivatedAbility { source }) =
                entry.kind()
            else {
                return Err(EffectError::InconsistentProgram);
            };
            if self
                .window
                .cards()
                .get(source.id())
                .is_some_and(|object| object.zone() == ObjectZone::Battlefield)
            {
                let before = self.rules_snapshot(None)?;
                let locals = self
                    .resolution_state()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .locals()
                    .clone();
                let (materialized, duration) = match &instruction {
                    Instruction::ChangeAbilitySource { parts, duration } => (
                        self.materialize_resolved_parts(
                            entry,
                            parts,
                            &locals,
                            source.owner(),
                            Some(source.id()),
                        )?,
                        *duration,
                    ),
                    Instruction::GiveControlOfAbilitySource => {
                        let recipient =
                            self.resolving_player(crate::program::ResolvingPlayer::TargetPlayer)?;
                        (
                            crate::characteristics::ResolvedParts(vec![
                                crate::characteristics::ResolvedPart::Control(recipient),
                            ]),
                            crate::characteristics::ResolvedDuration::NoStatedDuration,
                        )
                    }
                    _ => return Err(EffectError::InconsistentProgram),
                };
                let effect = self.characteristics.add_effect(
                    entry.id(),
                    source.id(),
                    materialized,
                    duration,
                )?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |old, new, _| {
                        let appearances = occurrence_flow::characteristic_appearance_changes(
                            old,
                            new,
                            effect.affected.iter().copied(),
                        )?;
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                effect,
                                appearances,
                            },
                        ])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::UntapAndControlCreatures {
            self.untap_and_control_creatures(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateArmyIfAbsent { army, subtype } = instruction {
            if self.controlled_armies(entry.controller(), army)?.is_empty() {
                self.create_army(entry.controller(), army, subtype)?;
            } else {
                self.advance_instruction()?;
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::DoubleCountersOnTargets {
            if entry.targets().targets().is_empty() {
                self.advance_instruction()?;
            } else {
                self.double_target_counters(ctx.append)?;
            }
            return Ok(StepOutcome::Continue);
        }
        if matches!(instruction, Instruction::RemoveCreatureCounters { .. }) {
            let choice = self.counter_removal_choice()?;
            if choice.domain().is_empty() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            return Ok(StepOutcome::Pending(EffectDecision::CounterRemoval(choice)));
        }
        if instruction == Instruction::Proliferate {
            let choice = self.proliferate_choice()?;
            if choice.domain().is_empty() {
                self.proliferate(&choice, vec![], ctx.append)?;
                return Ok(StepOutcome::Continue);
            }
            return Ok(StepOutcome::Pending(EffectDecision::Proliferate(choice)));
        }
        if instruction == Instruction::ChooseTypeThenDestroyUnchosen {
            self.expand_type_choices_for_table(entry.id(), index)?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ChooseCreatureType { .. }
                | Instruction::ChooseCreatureTypeForPlayer { .. }
        ) {
            let frame = EffectDecision::CreatureType(self.creature_type_frame()?);
            return Ok(StepOutcome::Pending(frame));
        }
        if instruction == Instruction::LinkedPlayerLosesDamageAmount {
            self.lose_life_for_linked_damage(entry.id(), index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ExtortDrain | Instruction::DrainOpponents(_)
        ) {
            let amount = match &instruction {
                Instruction::DrainOpponents(amount) => {
                    self.resolve_value_expr(entry.controller(), amount)?
                }
                _ => 1.into(),
            };
            self.drain_opponents(entry.id(), index, entry.controller(), &amount, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::BecomeMonarch(player) = instruction {
            self.become_monarch(
                entry.id(),
                index,
                player.unwrap_or(entry.controller()),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(instruction, Instruction::TapEnchantedCreature) {
            let source = self
                .execution_bindings(entry.id())
                .and_then(|b| b.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?
                .source
                .object
                .id();
            let current = self.evaluated_characteristics()?;
            let host = if current
                .get(source)
                .is_some_and(|v| v.zone == ObjectZone::Battlefield)
            {
                self.attached_to(source)
            } else {
                self.growth_last_known
                    .get(&source)
                    .and_then(|v| v.attached_to)
            };
            if let Some(TargetRef::Object(host)) = host
                && current
                    .get(host)
                    .is_some_and(|v| v.zone == ObjectZone::Battlefield)
                && self.window.cards().get(host).and_then(GameObject::tapped) == Some(false)
            {
                let before = self.rules_snapshot(None)?;
                let (window, changes) = self.window.tap_for_effect(entry.id(), &[host])?;
                self.window = window;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(occurrence_flow::status_occurrences(
                            changes,
                            crate::occurrence::StatusReason::Effect,
                        ))
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ChoosePlaneswalkerTypeForCopies {
            let frame = EffectDecision::PlaneswalkerType(PlaneswalkerTypeFrame {
                revision: self.revision,
                entry: entry.id(),
                instruction: index,
                chooser: entry.controller(),
            });
            return Ok(StepOutcome::Pending(frame));
        }
        if instruction == Instruction::ChooseAttackDirection {
            let frame = EffectDecision::AttackDirection(AttackDirectionFrame {
                revision: self.revision,
                entry: entry.id(),
                instruction: index,
                chooser: entry.controller(),
            });
            return Ok(StepOutcome::Pending(frame));
        }
        if instruction == Instruction::RequireAttackSourceNextTurn {
            let TargetRef::Player(opponent) = self.instruction_target(entry)?.choice().target
            else {
                return Err(EffectError::InconsistentProgram);
            };
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            self.next_turn_attack_targets
                .entry(opponent)
                .or_default()
                .push(source.id());
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::AdditionalLoyaltyActivation {
            let value = self
                .characteristics
                .extra_loyalty_activations
                .entry(entry.controller())
                .or_insert_with(|| 0.into());
            *value = value.plus(&1.into());
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DrawController {
            amount: ValueExpr::PlayerStat(PlayerStatExpr::CardsShortOfHandSize(required)),
        } = &instruction
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
        if instruction == Instruction::MoveLoyaltyCounterToEnteringPermanent {
            self.move_loyalty_to_entering_permanent(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::RequireCondition(cond) = instruction {
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
        if let Instruction::ResolveTransfer(key) = instruction {
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
            self.triggers.delay_step_body(
                appearance,
                entry.controller(),
                step,
                own_step_only,
                body,
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
                        last_sacrificed_creature: cursor.results.last_sacrificed_creature.clone(),
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
        if instruction == Instruction::CreateReflexiveDiscardDamage {
            let amount = self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .discarded_nonland_mana_value
                .take();
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            if let Some(amount) = amount {
                let source_id = source.id();
                let snapshot = self.rules_snapshot(None)?;
                let appearance = snapshot
                    .view()
                    .appearance(source_id)
                    .or_else(|| self.departed_reflexive_sources.get(&source_id).cloned())
                    .ok_or(EffectError::InconsistentProgram)?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &snapshot,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ReflexiveDiscardDamage {
                                source: Box::new(appearance),
                                controller: entry.controller(),
                                amount,
                            },
                        ])
                    },
                )?;
            }
            // Occurrence publication prunes last-known appearances after the last ability.
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::DivideTargetPermanentsAndSacrificePile {
            if let Some(frame) = self.progress_permanent_piles(entry)? {
                return Ok(StepOutcome::Pending(frame));
            }
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::LookTopOneRestGraveyard { .. }
                | Instruction::RevealTopHandRestGraveyard { .. }
                | Instruction::LookTopOneRestBottom { .. }
                | Instruction::LookTopChoose { .. }
                | Instruction::LookTopChooseExt { .. }
                | Instruction::RevealUntilMatch { .. }
                | Instruction::RevealUntilMatchExt { .. }
                | Instruction::LookTopCreatureSharingAttachedSubtype { .. }
                | Instruction::RevealTopCreaturesAndLandsRestBottom { .. }
                | Instruction::RevealHandAndBottomPiles
                | Instruction::RevealDividePiles
                | Instruction::RevealOpponentChoosesCreature
                | Instruction::RevealToNameAfterSourceShuffle(_)
        ) {
            let player = if let Instruction::RevealToNameAfterSourceShuffle(_) = instruction {
                self.resolution_cursor()
                    .and_then(|cursor| cursor.results.shuffled_source_owner)
            } else if let Instruction::LookTopChoose { library_owner, .. }
            | Instruction::LookTopChooseExt { library_owner, .. }
            | Instruction::RevealUntilMatchExt { library_owner, .. } = instruction
            {
                let cursor_snap = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                Some(self.resolving_player_identity_at(library_owner, Some(&cursor_snap))?)
            } else {
                Some(entry.controller())
            };
            if let Some(player) = player
                && let Some(frame) =
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
        if instruction == Instruction::DrawToChosenOpponentHandSize {
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), entry.controller())?;
            let opponent = match opponents.as_slice() {
                [] => {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                [only] => *only,
                _ => {
                    match self
                        .resolution_cursor_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .chosen_opponent
                        .take()
                    {
                        Some(player) => player,
                        None => {
                            let frame = EffectDecision::OpponentChoice(OpponentChoiceFrame {
                                revision: self.revision,
                                entry: entry.id(),
                                instruction: index,
                                chooser: entry.controller(),
                                opponents,
                            });
                            return Ok(StepOutcome::Pending(frame));
                        }
                    }
                }
            };
            let hand_size = |player| {
                self.window
                    .cards()
                    .iter()
                    .filter(|card| card.zone() == ObjectZone::Hand(player))
                    .count()
            };
            let difference = self
                .requested_draw_count(entry.controller())
                .and_then(|n| n.to_usize())
                .unwrap_or_else(|| {
                    hand_size(opponent).saturating_sub(hand_size(entry.controller()))
                });
            if difference > 0 {
                ctx.draws.append(draw_cards_step!(
                    self,
                    ctx,
                    entry.controller(),
                    RulesInteger::from_usize(difference),
                    DrawOrigin::Instruction {
                        entry: entry.id(),
                        instruction: index
                    },
                    ctx.append
                ));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReselectAttackDefender {
            if let Some(domain) = self.attack_defender_frame()? {
                let frame = EffectDecision::ReselectAttackDefender(domain);
                return Ok(StepOutcome::Pending(frame));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::SetBlockDeclarations(rule) = instruction {
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), entry.controller())?;
            if let Some(combat) = &mut self.combat {
                match rule {
                    crate::program::BlockDeclarationRule::EachOpponentOneBlocker => {
                        combat.one_blocker_for.extend(opponents)
                    }
                    crate::program::BlockDeclarationRule::ControllerChooses => {
                        combat.blocking_chooser = Some(entry.controller())
                    }
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::PhaseOtherPlaneswalkersNextEndStep {
            let objects = self.legal_permanent_targets()?;
            if !objects.is_empty() {
                let StackKind::ActivatedAbility { source } = entry.kind() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let before = self.rules_snapshot(None)?;
                let appearance = before
                    .view()
                    .appearance(source.id())
                    .or_else(|| self.departed_reflexive_sources.get(&source.id()).cloned())
                    .ok_or(EffectError::InconsistentProgram)?;
                self.triggers.delay_phasing(
                    appearance,
                    entry.controller(),
                    objects.into_iter().collect(),
                );
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        let phasing_roots = match &instruction {
            Instruction::PhaseOutObjects(objects) => {
                Some(std::borrow::Cow::Borrowed(objects.as_slice()))
            }
            Instruction::PhaseOut(crate::program::PhasingSubject::Target) => Some(
                std::borrow::Cow::Owned(self.legal_permanent_targets()?.into_iter().collect()),
            ),
            Instruction::PhaseOut(crate::program::PhasingSubject::Source) => {
                let (StackKind::TriggeredAbility { source }
                | StackKind::ActivatedAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                Some(std::borrow::Cow::Owned(vec![source.id()]))
            }
            Instruction::PhaseOut(crate::program::PhasingSubject::ControlledLands) => {
                Some(std::borrow::Cow::Owned(
                    self.evaluated_characteristics()?
                        .objects
                        .values()
                        .filter(|object| {
                            object.zone == ObjectZone::Battlefield
                                && object.controller == Some(entry.controller())
                                && object.has_type(CardType::Land)
                        })
                        .map(|object| object.object)
                        .collect(),
                ))
            }
            _ => None,
        };
        if let Some(objects) = phasing_roots {
            let before = self.rules_snapshot(None)?;
            let changed = self
                .window
                .phase_out(&objects, &self.characteristics.attachments)?;
            if !changed.is_empty() {
                if let Some(combat) = &mut self.combat {
                    combat.reconcile(|combatant, _| {
                        changed.binary_search(&combatant.object()).is_err()
                    });
                }
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![crate::occurrence::RulesOccurrence::Phased {
                            objects: changed,
                            out: true,
                        }])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::WeakenOpposingAttackersUntilNextTurn {
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let before = self.rules_snapshot(None)?;
            let appearance = before
                .view()
                .appearance(source.id())
                .or_else(|| self.departed_reflexive_sources.get(&source.id()).cloned())
                .ok_or(EffectError::InconsistentProgram)?;
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), entry.controller())?
                .into_iter()
                .collect();
            self.triggers
                .weaken_opposing_attackers(appearance, entry.controller(), opponents);
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::WeakenDelayedAttacker(object) = instruction {
            if self
                .window
                .cards()
                .get(object)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            {
                self.modify_objects(
                    entry,
                    index,
                    [object].into(),
                    Some(crate::characteristics::PowerToughness {
                        power: (-1).into(),
                        toughness: 0.into(),
                    }),
                    BTreeSet::new(),
                    ctx.append,
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::Myriad {
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
        mut instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if let Instruction::LoseLifeAndScryForSubtype(subtype) = instruction {
            let count = self
                .resolve_value_expr(entry.controller(), &ValueExpr::controlled_subtype(subtype))?;
            if !count.is_positive() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.expand_instruction(
                index,
                false,
                [
                    Instruction::AdjustEachOpponentLife {
                        adjustment: crate::life::LifeAdjustment::Lose(count.clone()),
                        controller_adjustment: None,
                    },
                    Instruction::Scry {
                        player: crate::program::ResolvingPlayer::Controller,
                        amount: ValueExpr::Literal(count),
                    },
                ],
            );
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ExileForBlackToken { .. } | Instruction::ExileForSubtypeToken(_)
        ) {
            self.begin_selected_graveyard_exile(
                self.legal_permanent_targets()?.into_iter().collect(),
            )?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CopyCreatureExiledForAbility {
            let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id()) else {
                return Err(EffectError::InconsistentProgram);
            };
            let definition = payload
                .exiled_activation_copy
                .as_deref()
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            self.begin_materialized_token_entries(
                vec![(entry.controller(), 1, definition, false)],
                None,
                false,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CopyEntrantAsArtifact {
            let trigger = self
                .execution_bindings(entry.id())
                .and_then(|bindings| bindings.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            if let Some(mut copied) = self.trigger_entrant_definition(trigger)?.cloned() {
                copied.card_types.insert(CardType::Artifact);
                self.begin_materialized_token_entries(
                    vec![(entry.controller(), 1, copied, false)],
                    None,
                    false,
                )?;
            } else {
                self.advance_instruction()?;
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReplaceSourceCreatedTokens {
            let source = self
                .execution_bindings(entry.id())
                .and_then(|bindings| bindings.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?
                .source
                .object
                .id();
            let created = std::mem::take(
                &mut self
                    .resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .last_created_tokens,
            );
            let tracked = self.source_created_tokens.entry(source).or_default();
            let earlier = tracked
                .iter()
                .copied()
                .filter(|token| !created.contains(token))
                .collect();
            tracked.extend(created);
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.body.instructions[index] = Instruction::ExileDelayedObjects(earlier);
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExileCombatDamagedCreature {
            let trigger = self
                .execution_bindings(entry.id())
                .and_then(|binding| binding.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let [member] = trigger.causes.members() else {
                return Err(EffectError::InconsistentProgram);
            };
            let crate::occurrence::TriggerRelated::Combatant(creature) = member.related else {
                return Err(EffectError::InconsistentProgram);
            };
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.body.instructions[index] = Instruction::ExileDelayedObjects(vec![creature]);
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::GiveEquippedCreatureToUpkeepPlayer {
            if let Some(host) = self.equipped_creature_for_control()? {
                let player = self.captured_related_player()?;
                self.modify_objects_for_duration(
                    entry,
                    index,
                    [host].into(),
                    crate::characteristics::ResolvedParts(vec![
                        crate::characteristics::ResolvedPart::Control(player),
                    ]),
                    crate::characteristics::ResolvedDuration::UntilEndOfTurn,
                    ctx.append,
                )?;
                if self.window.cards().get(host).and_then(GameObject::tapped) == Some(true) {
                    let before = self.rules_snapshot(None)?;
                    let occurrences = self.untap_for_effect_with_stun(entry.id(), &[host])?;
                    self.emit_from(
                        crate::occurrence::EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |_, _, _| Ok(occurrences),
                    )?;
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CopyEntrantOnceEachTurn {
            self.copy_entering_permanent_once()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CreateNonlegendaryLoyaltyCopy {
            let source = self
                .execution_bindings(entry.id())
                .and_then(|b| b.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?
                .source
                .object
                .id();
            let mut copied = self
                .copiable_program(source)
                .or_else(|| self.departed_copy_sources.get(&source))
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            copied
                .supertypes
                .remove(&crate::program::Supertype::Legendary);
            copied.printed_loyalty = Some(1.into());
            self.begin_materialized_token_entries(
                vec![(entry.controller(), 1, copied, false)],
                None,
                false,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::DrawRevealRemoveLoyalty {
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let source = source.id();
            let drawn = draw_cards_step!(
                self,
                ctx,
                entry.controller(),
                1.into(),
                DrawOrigin::Instruction {
                    entry: entry.id(),
                    instruction: index
                },
                ctx.append
            );
            for event in &drawn.events {
                let crate::draw::DrawEvent::Drawn { after: object, .. } = event else {
                    continue;
                };
                let snapshot = self.rules_snapshot(None)?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &snapshot,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![crate::occurrence::RulesOccurrence::HandCardRevealed {
                            object: object.clone(),
                        }])
                    },
                )?;
                let amount = self
                    .copiable_program(object.id())
                    .ok_or(EffectError::InconsistentProgram)?
                    .mana_value();
                if let Some(old) = self.characteristics.loyalty.get(&source).cloned() {
                    let new = old.minus(&amount).max(0.into());
                    if new != old {
                        let snapshot = self.rules_snapshot(None)?;
                        self.characteristics.loyalty.insert(source, new.clone());
                        self.emit_from(
                            crate::occurrence::EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            &snapshot,
                            None,
                            ctx.append,
                            |_, _, _| {
                                Ok(vec![
                                    crate::occurrence::RulesOccurrence::LoyaltyCountersChanged {
                                        object: source,
                                        before: old,
                                        after: new,
                                    },
                                ])
                            },
                        )?;
                    }
                }
            }
            ctx.draws.append(drawn);
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RemoveLoyaltyExileTopForPlay {
            let selected = self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .chosen_loyalty_removals
                .take();
            let Some(selected) = selected else {
                let evaluated = self.evaluated_characteristics()?;
                let objects = evaluated
                    .objects
                    .values()
                    .filter(|value| {
                        value.zone == ObjectZone::Battlefield
                            && value.controller == Some(entry.controller())
                    })
                    .filter(|value| {
                        self.characteristics
                            .loyalty
                            .get(&value.object)
                            .is_some_and(RulesInteger::is_positive)
                    })
                    .map(|value| value.object)
                    .collect::<Vec<_>>();
                let frame = EffectDecision::LoyaltyRemoval(LoyaltyRemovalChoice {
                    revision: self.revision,
                    entry: entry.id(),
                    instruction: index,
                    chooser: entry.controller(),
                    objects,
                });
                return Ok(StepOutcome::Pending(frame));
            };
            let before = self.rules_snapshot(None)?;
            let mut changes = Vec::new();
            for object in &selected {
                let old = self
                    .characteristics
                    .loyalty
                    .get(object)
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram)?;
                if !old.is_positive() {
                    return Err(EffectError::InconsistentProgram);
                }
                let new = old.minus(&1.into());
                if new.is_positive() {
                    self.characteristics.loyalty.insert(*object, new.clone());
                } else {
                    self.characteristics.loyalty.remove(object);
                }
                changes.push((*object, old, new));
            }
            if !changes.is_empty() {
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(changes
                            .into_iter()
                            .map(|(object, before, after)| {
                                crate::occurrence::RulesOccurrence::LoyaltyCountersChanged {
                                    object,
                                    before,
                                    after,
                                }
                            })
                            .collect())
                    },
                )?;
            }
            if self.begin_exile_top_for_play(entry, index, selected.len())? {
                return Ok(StepOutcome::Continue);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::GrantProtectionChosenColor(recipients) = instruction {
            let Some(color) = self
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
            use crate::characteristics::Color;
            let color = Color::from_mana(color).ok_or(EffectError::InconsistentProgram)?;
            let keyword = match color {
                Color::White => KeywordAbility::ProtectionFromWhite,
                Color::Blue => KeywordAbility::ProtectionFromBlue,
                Color::Black => KeywordAbility::ProtectionFromBlack,
                Color::Red => KeywordAbility::ProtectionFromRed,
                Color::Green => KeywordAbility::ProtectionFromGreen,
            };
            if recipients == crate::program::ProtectionRecipients::ControllerAndPermanents {
                self.grant_controller_protection(entry, index, color, keyword, ctx.append)?;
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            if recipients == crate::program::ProtectionRecipients::AbilitySource {
                if let StackKind::ActivatedAbility { source }
                | StackKind::TriggeredAbility { source } = entry.kind()
                    && self
                        .window
                        .cards()
                        .get(source.id())
                        .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                {
                    let parts = crate::characteristics::ResolvedParts(vec![
                        crate::characteristics::ResolvedPart::Abilities(
                            crate::characteristics::AbilityChange::grant_keywords([keyword]),
                        ),
                    ]);
                    self.characteristics.add_effect(
                        entry.id(),
                        source.id(),
                        parts,
                        crate::characteristics::ResolvedDuration::UntilEndOfTurn,
                    )?;
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            if let crate::program::ProtectionRecipients::Matching(filter) = recipients {
                let evaluated = self.evaluated_characteristics()?;
                let ability_source = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                };
                let affected: BTreeSet<ObjectId> = evaluated
                    .objects
                    .values()
                    .filter(|p| {
                        p.zone == ObjectZone::Battlefield
                            && self.matches_object_filter(
                                *filter,
                                p,
                                entry.controller(),
                                ability_source,
                                &BTreeSet::new(),
                                &evaluated,
                            )
                    })
                    .map(|p| p.object)
                    .collect();
                if !affected.is_empty() {
                    let parts = crate::characteristics::ResolvedParts(vec![
                        crate::characteristics::ResolvedPart::Abilities(
                            crate::characteristics::AbilityChange::grant_keywords([keyword]),
                        ),
                    ]);
                    self.characteristics.add_effect_for_objects(
                        entry.id(),
                        affected,
                        parts,
                        crate::characteristics::ResolvedDuration::UntilEndOfTurn,
                    )?;
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            instruction = Instruction::CreateContinuousEffect {
                parts: crate::characteristics::ResolvedPartsSpec(vec![
                    crate::characteristics::ResolvedPartSpec::Abilities(
                        crate::characteristics::AbilityChange::grant_keywords([keyword]),
                    ),
                ]),
                duration: crate::characteristics::ResolvedDuration::UntilEndOfTurn,
            };
        }
        if let Instruction::GrantChosenProtection {
            recipients,
            duration,
        } = instruction
        {
            let Some(color) = self
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
            let Some(resolved_dur) = self.resolve_duration(entry, duration)? else {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            };
            use crate::characteristics::Color;
            let color = Color::from_mana(color).ok_or(EffectError::InconsistentProgram)?;
            let keyword = match color {
                Color::White => KeywordAbility::ProtectionFromWhite,
                Color::Blue => KeywordAbility::ProtectionFromBlue,
                Color::Black => KeywordAbility::ProtectionFromBlack,
                Color::Red => KeywordAbility::ProtectionFromRed,
                Color::Green => KeywordAbility::ProtectionFromGreen,
            };
            let parts = crate::characteristics::ResolvedParts(vec![
                crate::characteristics::ResolvedPart::Abilities(
                    crate::characteristics::AbilityChange::grant_keywords([keyword]),
                ),
            ]);
            match recipients {
                crate::program::ProtectionRecipients::ControllerAndPermanents => {
                    let before = self.rules_snapshot(None)?;
                    let player = entry.controller();
                    let affected = self.effect_group(
                        entry,
                        crate::program::EffectSubject::Matching(
                            crate::program::ObjectFilter::any().controlled(),
                        ),
                    )?;
                    let effect = if affected.is_empty() {
                        None
                    } else {
                        Some(self.characteristics.add_effect_for_objects(
                            entry.id(),
                            affected,
                            parts,
                            resolved_dur,
                        )?)
                    };
                    self.characteristics
                        .player_color_protection
                        .entry(player)
                        .or_default()
                        .insert(color);
                    self.emit_from(
                        crate::occurrence::EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |old, current, _| {
                            let mut rows = Vec::new();
                            if let Some(effect) = effect {
                                let appearances = characteristic_appearance_changes(
                                    old,
                                    current,
                                    effect.affected.iter().copied(),
                                )?;
                                rows.push(
                                    crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                        effect,
                                        appearances,
                                    },
                                );
                            }
                            rows.push(
                                crate::occurrence::RulesOccurrence::PlayerProtectionGranted {
                                    player,
                                    color,
                                },
                            );
                            Ok(rows)
                        },
                    )?;
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                crate::program::ProtectionRecipients::AbilitySource => {
                    if let StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } = entry.kind()
                        && self
                            .window
                            .cards()
                            .get(source.id())
                            .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                    {
                        let before = self.rules_snapshot(None)?;
                        let effect = self.characteristics.add_effect(
                            entry.id(),
                            source.id(),
                            parts,
                            resolved_dur,
                        )?;
                        self.emit_from(
                            crate::occurrence::EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            &before,
                            None,
                            ctx.append,
                            |old, current, _| {
                                let appearances = characteristic_appearance_changes(
                                    old,
                                    current,
                                    effect.affected.iter().copied(),
                                )?;
                                Ok(vec![
                                    crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                        effect,
                                        appearances,
                                    },
                                ])
                            },
                        )?;
                    }
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                crate::program::ProtectionRecipients::Matching(filter) => {
                    let evaluated = self.evaluated_characteristics()?;
                    let ability_source = match entry.kind() {
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => Some(source.id()),
                        _ => None,
                    };
                    let affected: BTreeSet<ObjectId> = evaluated
                        .objects
                        .values()
                        .filter(|p| {
                            p.zone == ObjectZone::Battlefield
                                && self.matches_object_filter(
                                    *filter,
                                    p,
                                    entry.controller(),
                                    ability_source,
                                    &BTreeSet::new(),
                                    &evaluated,
                                )
                        })
                        .map(|p| p.object)
                        .collect();
                    if !affected.is_empty() {
                        let before = self.rules_snapshot(None)?;
                        let effect = self.characteristics.add_effect_for_objects(
                            entry.id(),
                            affected,
                            parts,
                            resolved_dur,
                        )?;
                        self.emit_from(
                            crate::occurrence::EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            &before,
                            None,
                            ctx.append,
                            |old, current, _| {
                                let appearances = characteristic_appearance_changes(
                                    old,
                                    current,
                                    effect.affected.iter().copied(),
                                )?;
                                Ok(vec![
                                    crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                        effect,
                                        appearances,
                                    },
                                ])
                            },
                        )?;
                    }
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                crate::program::ProtectionRecipients::TargetPermanent => {
                    instruction = Instruction::CreateContinuousEffect {
                        parts: crate::characteristics::ResolvedPartsSpec(vec![
                            crate::characteristics::ResolvedPartSpec::Abilities(
                                crate::characteristics::AbilityChange::grant_keywords([keyword]),
                            ),
                        ]),
                        duration: resolved_dur,
                    };
                }
            }
        }
        if matches!(
            instruction,
            Instruction::AbandonSourceScheme | Instruction::TabletopHonorAction { .. }
        ) {
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AddMana(ref mana_yield) = instruction {
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
                        let frame =
                            EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                                revision: self.revision,
                                entry: entry.id(),
                                instruction: index,
                                chooser: entry.controller(),
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
                    let player = entry.controller();
                    let prior = self.mana(player, color);
                    let after = prior.plus(&qty);
                    self.mana.insert((player, color), after.clone());
                    let key = (player, color, *restriction);
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
                    let player = entry.controller();
                    let mut before_map = BTreeMap::new();
                    let mut after_map = BTreeMap::new();
                    for (&color, amount) in bundle {
                        let prior = self.mana(player, color);
                        let after = prior.plus(amount);
                        self.mana.insert((player, color), after.clone());
                        let key = (player, color, *restriction);
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
                        let frame =
                            EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                                revision: self.revision,
                                entry: entry.id(),
                                instruction: index,
                                chooser: entry.controller(),
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
                            chooser: entry.controller(),
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
                        .selection_players(*controller, entry.controller(), Some(cursor))?
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
                        let frame =
                            EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                                revision: self.revision,
                                entry: entry.id(),
                                instruction: index,
                                chooser: entry.controller(),
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
                    let colors = self.commander_mana_colors(entry.controller())?;
                    if colors.is_empty() {
                        self.advance_instruction()?;
                        return Ok(StepOutcome::Continue);
                    }
                    let color = self
                        .resolution_cursor()
                        .and_then(|cursor| cursor.chosen_effect_color);
                    let Some(color) = color else {
                        let frame =
                            EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                                revision: self.revision,
                                entry: entry.id(),
                                instruction: index,
                                chooser: entry.controller(),
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
                    let chosen =
                        source_id.and_then(|id| self.entry_color_choices.get(&id).copied());
                    if !resolved.is_positive() || chosen.is_none() {
                        self.advance_instruction()?;
                        return Ok(StepOutcome::Continue);
                    }
                    let Some(color) = chosen else { unreachable!() };
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
                        let frame =
                            EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                                revision: self.revision,
                                entry: entry.id(),
                                instruction: index,
                                chooser: entry.controller(),
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
                            entry.controller(),
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
                        entry.controller(),
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
                        let frame =
                            EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                                revision: self.revision,
                                entry: entry.id(),
                                instruction: index,
                                chooser: entry.controller(),
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
                    entry.controller(),
                    &produced,
                    crate::mana::ManaSpendEffect::RetainUntilCleanup,
                )?;
            }
            let before_snapshot = self.rules_snapshot(None)?;
            let player = entry.controller();
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
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateOpponentEmblems(ref definition) = instruction {
            self.resolve_create_opponent_emblems(entry, index, definition.clone(), ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateEmblem(ref definition) = instruction {
            self.resolve_create_emblem(entry, index, definition.clone(), ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::IncreaseSpeed {
            self.resolve_speed(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ReturnSourceGraveyardToBattlefield { tapped } = instruction {
            if let Some((object, _, _)) = self.keyword_entry_candidate(&instruction, entry)? {
                self.settled_entry_destinations.remove(&object);
                self.return_exact_source_graveyard(entry, index, object, tapped, ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ReturnUpToOneRecentGraveyardCard
                | Instruction::ReturnSourceGraveyardToHand
                | Instruction::ReturnDelayedGraveyardToHand(_)
        ) {
            let object = if let Instruction::ReturnDelayedGraveyardToHand(object) = instruction {
                object
            } else if instruction == Instruction::ReturnSourceGraveyardToHand {
                let (StackKind::TriggeredAbility { source }
                | StackKind::ActivatedAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                source.id()
            } else {
                let Some(target) = entry.targets().targets().first() else {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                };
                let TargetRef::Object(id) = target.choice().target else {
                    return Err(EffectError::InconsistentProgram);
                };
                id
            };
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
        if instruction == Instruction::RevealCreatureOfSacrificedType {
            self.begin_kin_reveal(entry, index, ctx.append)?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExileNonlandForFormerControllers {
            self.progress_library_rewrite(entry, index)?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RevealCreatureForEachTargetController {
            self.progress_creature_reveals(ctx.append)?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::EndControllerTurn {
            if entry.controller() != self.window.active_player() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let before = self.rules_snapshot(None)?;
            self.triggers.clear_waiting_at_end_turn();
            let entries = self
                .window
                .entries()
                .iter()
                .map(|entry| entry.id())
                .collect();
            let (window, exits) = self.window.exile_stack_to_end_turn(entry.id())?;
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
                    let mut events =
                        vec![crate::occurrence::RulesOccurrence::StackExiledToEndTurn { entries }];
                    for exit in exits {
                        events.push(exit_occurrence_at(
                            old,
                            current,
                            exit,
                            crate::occurrence::ZoneReason::ExiledByEffect,
                            controllers,
                        )?);
                    }
                    Ok(events)
                },
            )?;
            self.pending_work = None;
            self.prune_payloads();
            self.ending_turn = true;
            self.additional_combats = 0;
            return Ok(StepOutcome::Complete(RuleCompletion::Resolution));
        }
        if matches!(
            instruction,
            Instruction::ShuffleEachHandAndGraveyard
                | Instruction::ShuffleOwnLibrary
                | Instruction::ShuffleOwnGraveyardIntoLibrary
        ) {
            self.begin_graveyard_shuffle()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ShufflePlayerLibrary(player) = instruction {
            self.begin_player_library_shuffle(player)?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ShuffleSubjectIntoLibrary { subject, zone } = instruction {
            self.begin_subject_into_library_shuffle(entry, *subject, zone)?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::GainControlOfPermanent {
            subject,
            controller,
            duration,
        } = instruction
        {
            if let Some(resolved_dur) = self.resolve_duration(entry, duration)? {
                let affected: BTreeSet<ObjectId> =
                    self.effect_group(entry, *subject)?.into_iter().collect();
                if !affected.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let groups: Vec<(SeatId, BTreeSet<ObjectId>)> = if controller
                        == crate::program::ResolvingPlayer::TargetOwner
                    {
                        let mut by_owner: BTreeMap<SeatId, BTreeSet<ObjectId>> = BTreeMap::new();
                        for id in affected {
                            if let Some(card) = self.window.cards().get(id) {
                                by_owner.entry(card.owner()).or_default().insert(id);
                            }
                        }
                        by_owner.into_iter().collect()
                    } else {
                        let new_ctrl = self.resolving_player(controller)?;
                        vec![(new_ctrl, affected)]
                    };
                    let mut created = Vec::new();
                    for (new_ctrl, ids) in groups {
                        let effect = self.characteristics.add_effect_for_objects(
                            entry.id(),
                            ids,
                            crate::characteristics::ResolvedParts(vec![
                                crate::characteristics::ResolvedPart::Control(new_ctrl),
                            ]),
                            resolved_dur,
                        )?;
                        created.push(effect);
                    }
                    self.emit_from(
                        crate::occurrence::EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |old, new, _| {
                            let mut occs = Vec::with_capacity(created.len());
                            for effect in created {
                                let appearances =
                                    occurrence_flow::characteristic_appearance_changes(
                                        old,
                                        new,
                                        effect.affected.iter().copied(),
                                    )?;
                                occs.push(
                                    crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                        effect,
                                        appearances,
                                    },
                                );
                            }
                            Ok(occs)
                        },
                    )?;
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::GiveFixedTargetControl { duration } = instruction {
            if let Some(resolved_dur) = self.resolve_duration(entry, duration)?
                && let Some((recipient, permanent)) =
                    self.legal_linked_player_and_object_targets()?
                && self
                    .window
                    .cards()
                    .get(permanent)
                    .is_some_and(|obj| obj.zone() == ObjectZone::Battlefield)
            {
                let before = self.rules_snapshot(None)?;
                let effect = self.characteristics.add_effect(
                    entry.id(),
                    permanent,
                    crate::characteristics::ResolvedParts(vec![
                        crate::characteristics::ResolvedPart::Control(recipient),
                    ]),
                    resolved_dur,
                )?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |old, new, _| {
                        let appearances = occurrence_flow::characteristic_appearance_changes(
                            old,
                            new,
                            effect.affected.iter().copied(),
                        )?;
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                effect,
                                appearances,
                            },
                        ])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::BecomeCopy {
            receiver,
            donor,
            duration,
            exceptions,
        } = instruction
        {
            let donor_id = match donor {
                crate::program::MoveSubjectRef::TargetObject => self
                    .instruction_target(entry)
                    .ok()
                    .and_then(|bound| match bound.choice().target {
                        TargetRef::Object(id) => Some(id),
                        _ => None,
                    }),
                crate::program::MoveSubjectRef::AbilitySource => match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    StackKind::CardSpell(id) => Some(*id),
                    _ => None,
                },
                crate::program::MoveSubjectRef::TriggeringObject => self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .and_then(|t| t.causes.members().first())
                    .and_then(|m| m.subject_object_id()),
                crate::program::MoveSubjectRef::AttachedHost => match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => {
                        match self.attachment_host_for_ability(source.id()) {
                            Some(TargetRef::Object(host)) => Some(host),
                            _ => None,
                        }
                    }
                    _ => None,
                },
                crate::program::MoveSubjectRef::RevealedCard => self
                    .resolution_cursor()
                    .and_then(|c| c.impulse.exiled.or(c.results.revealed_top_card)),
                crate::program::MoveSubjectRef::CreatedTokens => self
                    .resolution_cursor()
                    .and_then(|c| c.last_created_tokens.first().copied()),
                crate::program::MoveSubjectRef::LastMoved => {
                    self.resolution_cursor().and_then(|c| {
                        c.results
                            .last_moved_objects
                            .first()
                            .copied()
                            .or(c.results.returned_target_permanent)
                    })
                }
                crate::program::MoveSubjectRef::RememberedFirst => {
                    self.legal_linked_targets()?.map(|(a, _)| a)
                }
                crate::program::MoveSubjectRef::RememberedSecond => {
                    self.legal_linked_targets()?.map(|(_, b)| b)
                }
            };
            if let Some(resolved_dur) = self.resolve_duration(entry, duration)?
                && let Some(donor_id) = donor_id
                && let Some(base) = self.copiable_program(donor_id).cloned()
            {
                let receivers: Vec<ObjectId> = self
                    .effect_group(entry, *receiver)?
                    .into_iter()
                    .filter(|id| {
                        self.window
                            .cards()
                            .get(*id)
                            .is_some_and(|obj| obj.zone() == ObjectZone::Battlefield)
                    })
                    .collect();
                if !receivers.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let mut created = Vec::new();
                    for recv_id in receivers {
                        let mut copied = base.clone();
                        apply_copy_exceptions(
                            &mut copied,
                            &exceptions,
                            self.copiable_program(recv_id),
                        );
                        let effect = self.characteristics.add_effect(
                            entry.id(),
                            recv_id,
                            crate::characteristics::ResolvedParts(vec![
                                crate::characteristics::ResolvedPart::Copy(Box::new(copied)),
                            ]),
                            resolved_dur,
                        )?;
                        created.push(effect);
                    }
                    self.emit_from(
                        crate::occurrence::EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |old, new, _| {
                            let mut occs = Vec::with_capacity(created.len());
                            for effect in created {
                                let appearances =
                                    occurrence_flow::characteristic_appearance_changes(
                                        old,
                                        new,
                                        effect.affected.iter().copied(),
                                    )?;
                                occs.push(
                                    crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                        effect,
                                        appearances,
                                    },
                                );
                            }
                            Ok(occs)
                        },
                    )?;
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::BecomeCopyFixedTargets {
            duration,
            exceptions,
        } = instruction
        {
            if let Some(resolved_dur) = self.resolve_duration(entry, duration)?
                && let Some((recv_id, donor_id)) = self.legal_linked_targets()?
                && self
                    .window
                    .cards()
                    .get(recv_id)
                    .is_some_and(|obj| obj.zone() == ObjectZone::Battlefield)
                && let Some(mut copied) = self.copiable_program(donor_id).cloned()
            {
                let before = self.rules_snapshot(None)?;
                apply_copy_exceptions(&mut copied, &exceptions, self.copiable_program(recv_id));
                let effect = self.characteristics.add_effect(
                    entry.id(),
                    recv_id,
                    crate::characteristics::ResolvedParts(vec![
                        crate::characteristics::ResolvedPart::Copy(Box::new(copied)),
                    ]),
                    resolved_dur,
                )?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |old, new, _| {
                        let appearances = occurrence_flow::characteristic_appearance_changes(
                            old,
                            new,
                            effect.affected.iter().copied(),
                        )?;
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                effect,
                                appearances,
                            },
                        ])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DoublePowerToughnessSubject {
            subject,
            power,
            toughness,
            duration,
        } = instruction
        {
            if let Some(resolved_dur) = self.resolve_duration(entry, duration)? {
                let objects = self.effect_group(entry, *subject)?;
                let evaluated = self.evaluated_characteristics()?;
                for object in objects {
                    if let Some(pt) = evaluated
                        .get(object)
                        .filter(|o| o.zone == ObjectZone::Battlefield)
                        .and_then(|o| o.power_toughness.clone())
                    {
                        let dp = if power { pt.derived.power } else { 0.into() };
                        let dt = if toughness {
                            pt.derived.toughness
                        } else {
                            0.into()
                        };
                        self.modify_objects_for_duration(
                            entry,
                            index,
                            [object].into(),
                            crate::characteristics::ResolvedParts(vec![
                                crate::characteristics::ResolvedPart::ModifyPT(
                                    crate::characteristics::PowerToughness {
                                        power: dp,
                                        toughness: dt,
                                    },
                                ),
                            ]),
                            resolved_dur,
                            ctx.append,
                        )?;
                    }
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DoubleCountersOnSubject { subject, kind } = instruction {
            let objects: Vec<ObjectId> = self.effect_group(entry, *subject)?.into_iter().collect();
            self.double_subject_counters(objects, kind, ctx.append)?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::WinGame(player) = instruction {
            let winner = self.resolving_player(player)?;
            let evaluated = self.evaluated_characteristics()?;
            if self.window.participants().contains(&winner)
                && !self.player_cannot_win(&evaluated, winner)?
            {
                let source = match entry.kind() {
                    StackKind::CardSpell(id) => *id,
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => source.id(),
                    StackKind::SpellCopy { .. }
                    | StackKind::Emblem { .. }
                    | StackKind::DelayedCopy { .. }
                    | StackKind::Monarch
                    | StackKind::Speed => ObjectId::SENTINEL,
                };
                self.won_by_effect = Some((winner, source));
                return Ok(StepOutcome::Complete(RuleCompletion::GameEnded));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::LoseGame(players) = instruction {
            let evaluated = self.evaluated_characteristics()?;
            let mut losing = BTreeSet::new();
            for seat in
                self.selection_players(players, entry.controller(), self.resolution_cursor())?
            {
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
                    loss_occs.push(crate::occurrence::RulesOccurrence::PlayerLost {
                        player: seat,
                        reasons: [crate::occurrence::PlayerLossReason::Effect].into(),
                    });
                }
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| Ok(loss_occs),
                )?;
                let surviving = self
                    .window
                    .participants()
                    .iter()
                    .filter(|seat| !self.state_based_losses.contains(seat))
                    .count();
                if surviving <= 1 {
                    return Ok(StepOutcome::Complete(RuleCompletion::GameEnded));
                }
                self.apply_player_departures(&losing, None, ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ShuffleTargetIntoLibrary {
            self.begin_target_into_library_shuffle(entry)?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::RevealPlayerHand(player) = instruction {
            let seat = self.resolving_player(player)?;
            let cards: Vec<GameObject> = self
                .window
                .cards()
                .iter()
                .filter(|obj| obj.zone() == ObjectZone::Hand(seat))
                .cloned()
                .collect();
            if !cards.is_empty() {
                let before = self.rules_snapshot(None)?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(cards
                            .iter()
                            .cloned()
                            .map(
                                |object| crate::occurrence::RulesOccurrence::HandCardRevealed {
                                    object,
                                },
                            )
                            .collect())
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::GrantGraveyardCastAndWatchAbilities {
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let TargetRef::Object(card) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            if self
                .window
                .cards()
                .get(card)
                .is_some_and(|object| matches!(object.zone(), ObjectZone::Graveyard(_)))
            {
                self.card_play_permissions.insert(CardPlayPermission {
                    zone: CardPlayZone::Graveyard,
                    object: card,
                    player: entry.controller(),
                    duration: CardPlayDuration::ThroughTurn,
                    any_color: false,
                    free: false,
                    cast_only: true,
                });
                self.triggers.watch_graveyard_cast(
                    entry.controller(),
                    entry.id(),
                    source.id(),
                    card,
                );
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::GainCastCardAbilities {
            recipient,
            ref donor,
        } = instruction
        {
            use crate::characteristics::{ResolvedDuration, ResolvedPart, ResolvedParts};
            if self
                .window
                .cards()
                .get(recipient)
                .is_some_and(|object| object.zone() == ObjectZone::Battlefield)
                && self.window.cards().phased_out_under(recipient).is_none()
            {
                let before = self.rules_snapshot(None)?;
                let observed = before
                    .view()
                    .evaluated
                    .get(donor.object)
                    .filter(|card| card.zone == ObjectZone::Stack)
                    .cloned();
                let ability = crate::characteristics::StaticAbilityId(
                    self.characteristics.next_effect_sequence(),
                );
                let inherited = crate::characteristics::inherit_activated_abilities(
                    recipient,
                    ability,
                    [observed.as_ref().unwrap_or(donor)],
                );
                if !inherited.activated.is_empty() || !inherited.mana.is_empty() {
                    let effect = self.characteristics.add_effect(
                        entry.id(),
                        recipient,
                        ResolvedParts(vec![ResolvedPart::InheritedAbilities(Box::new(inherited))]),
                        ResolvedDuration::UntilEndOfTurn,
                    )?;
                    self.emit_from(
                        crate::occurrence::EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |old, new, _| {
                            let appearances = occurrence_flow::characteristic_appearance_changes(
                                old,
                                new,
                                effect.affected.iter().copied(),
                            )?;
                            Ok(vec![
                                crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                    effect,
                                    appearances,
                                },
                            ])
                        },
                    )?;
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ShuffleAbilitySourceIntoLibrary {
            self.begin_ability_source_shuffle(entry)?;
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
        mut instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if matches!(instruction, Instruction::ShuffleTargetGraveyardCards { .. }) {
            self.begin_targeted_graveyard_shuffle()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExileAllGraveyards {
            self.begin_graveyard_exile()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ExilePlayerGraveyards(players) = instruction {
            let seats =
                self.selection_players(players, entry.controller(), self.resolution_cursor())?;
            self.begin_players_graveyard_exile(&seats)?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::RemoveCountersFromPermanent {
            target_self,
            kind,
            ref amount,
        } = instruction
        {
            let resolved = self.resolve_value_expr(entry.controller(), amount)?;
            let target_id = if target_self {
                match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                }
            } else if let Ok(target) = self.instruction_target(entry) {
                match target.choice().target {
                    TargetRef::Object(id) => Some(id),
                    TargetRef::Player(_) | TargetRef::Stack(_) => None,
                }
            } else {
                None
            };
            if resolved.is_positive()
                && let Some(object) = target_id
                && self
                    .window
                    .cards()
                    .get(object)
                    .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
            {
                if kind.is_none() {
                    let mut present_kinds: Vec<(CounterKind, RulesInteger)> = Vec::new();
                    let mut total_avail = RulesInteger::from(0);
                    for ck in self.characteristics.present_permanent_counter_kinds(object) {
                        let cnt = self.counters_of_kind(object, ck);
                        if cnt.is_positive() {
                            total_avail = total_avail.plus(&cnt);
                            present_kinds.push((ck, cnt));
                        }
                    }
                    if present_kinds.len() > 1 && total_avail > resolved {
                        let choice = self.counter_removal_choice()?;
                        if !choice.domain().is_empty() {
                            return Ok(StepOutcome::Pending(EffectDecision::CounterRemoval(
                                choice,
                            )));
                        }
                    }
                    if present_kinds.len() > 1 && total_avail <= resolved {
                        let choice = self.counter_removal_choice()?;
                        let all_selected = choice.domain().clone();
                        self.remove_counters(&choice, &all_selected, ctx.append)?;
                        self.advance_instruction()?;
                        return Ok(StepOutcome::Continue);
                    }
                }
                let chosen_kind = kind.or_else(|| {
                    self.characteristics
                        .present_permanent_counter_kinds(object)
                        .into_iter()
                        .next()
                });
                if let Some(ck) = chosen_kind {
                    let current_count = self.counters_of_kind(object, ck);
                    let to_remove = if resolved < current_count {
                        resolved
                    } else {
                        current_count.clone()
                    };
                    if to_remove.is_positive() {
                        let before_snapshot = self.rules_snapshot(None)?;
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
                                            crate::occurrence::RulesOccurrence::PermanentCounterChanged {
                                                object,
                                                kind: ck,
                                                before: if ck == CounterKind::PlusOne {
                                                    before.plus_one.clone()
                                                } else {
                                                    before.minus_one.clone()
                                                },
                                                after: if ck == CounterKind::PlusOne {
                                                    after.plus_one.clone()
                                                } else {
                                                    after.minus_one.clone()
                                                },
                                            },
                                        ])
                                    },
                                )?;
                            }
                            other => {
                                let after = current_count.minus(&to_remove);
                                if let Some(map) =
                                    self.characteristics.named_permanent_counters_mut(other)
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
                                                crate::occurrence::RulesOccurrence::PermanentCounterChanged {
                                                    object,
                                                    kind: other,
                                                    before: current_count.clone(),
                                                    after: after.clone(),
                                                },
                                            ])
                                        },
                                    )?;
                                }
                            }
                        }
                        if let Some(cursor) = self.resolution_cursor_mut() {
                            cursor.results.last_action_succeeded = true;
                        }
                    }
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::Ninjutsu {
            if let Some((object, _, _)) = self.keyword_entry_candidate(&instruction, entry)? {
                self.settled_entry_destinations.remove(&object);
            }
            self.resolve_ninjutsu(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::EndStepCast {
            kind,
            permanent,
            turn,
        } = instruction
        {
            self.resolve_end_step_cast(entry, index, kind, permanent, turn, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ResolveGrowth(ref keyword) = instruction {
            self.resolve_growth(keyword, entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ResolveCounterKeyword(keyword) = instruction {
            if let Some((object, _, _)) = self.keyword_entry_candidate(&instruction, entry)? {
                self.settled_entry_destinations.remove(&object);
            }
            self.resolve_counter_keyword(keyword, entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ResolveKeywordTrigger(crate::program::CombatTriggerKeyword::Mobilize(
            amount,
        )) = &instruction
        {
            let warrior = crate::program::mobilize_warrior(&self.creature_vocabulary)
                .map_err(|_| EffectError::InconsistentProgram)?;
            let mut creation = crate::token::TokenCreation::one(warrior);
            creation.amount = ValueExpr::Literal(amount.clone());
            creation.enters_tapped = true;
            self.begin_token_entries(&[creation], None, true)?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ResolveKeywordTrigger(keyword) = &instruction {
            self.resolve_keyword_trigger(keyword, entry.id(), index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::Unearth | Instruction::ExileUnearthed(_)
        ) {
            if let Some((object, _, _)) = self.keyword_entry_candidate(&instruction, entry)? {
                self.settled_entry_destinations.remove(&object);
            }
            self.resolve_unearth(entry, index, &instruction, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CounterWardSource {
            self.counter_ward_source(entry.id(), index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::CounterTriggeringStackObject {
            self.counter_triggering_stack_object(entry.id(), index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::BoostSourceByCastManaValue {
            self.boost_source_by_cast_mana_value(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::BoostLoneAttacker {
            self.boost_lone_attacker(entry.id(), index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateTokensToFight(definition) = &instruction {
            let definition = definition
                .materialize(&self.creature_vocabulary)
                .map_err(|_| EffectError::InconsistentProgram)?;
            let opponents = self.effect_group(
                entry,
                crate::program::EffectSubject::Matching(
                    crate::program::ObjectFilter::creature().opposing(),
                ),
            )?;
            let groups = opponents
                .iter()
                .map(|_| (entry.controller(), 1, definition.clone(), false))
                .collect();
            self.begin_materialized_token_entries(groups, None, false)?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::FightFixedTargets | Instruction::FightSourceWithTarget
        ) {
            let pairs = if instruction == Instruction::FightFixedTargets {
                self.legal_linked_targets()?
                    .filter(|(first, second)| first != second)
            } else {
                let (StackKind::ActivatedAbility { source }
                | StackKind::TriggeredAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                Some((source.id(), target))
            };
            let inputs = self.fight_damage_inputs(pairs.as_slice())?;
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
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::FightPairedCreatures(pairs) = &instruction {
            let inputs = self.fight_damage_inputs(pairs)?;
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
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DistributeCountersAmongTargets { kind, total } = instruction {
            let assessment = self
                .window
                .target_assessment()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let division = match self.payloads.get(&entry.id()) {
                Some(StackPayload::Instructions(bound)) => bound.target_division.clone(),
                _ => BTreeMap::new(),
            };
            let total_targets = entry.targets().targets().len();
            let mut amounts = Vec::new();
            for row in assessment.results() {
                if row.legality != crate::targets::TargetLegality::Legal {
                    continue;
                }
                let TargetRef::Object(object) = row.choice.target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let share = division.get(&row.choice.id).copied().unwrap_or_else(|| {
                    if total_targets <= 1 {
                        total
                    } else {
                        let base = total / u16::try_from(total_targets).unwrap_or(1);
                        let rem = total % u16::try_from(total_targets).unwrap_or(1);
                        if row.choice.id == TargetId(0) {
                            base.saturating_add(rem)
                        } else {
                            base
                        }
                    }
                });
                if share > 0 {
                    amounts.push((object, i64::from(share).into()));
                }
            }
            self.place_divided_counters(
                kind,
                amounts,
                crate::occurrence::EventCause::Instruction {
                    entry: entry.id(),
                    instruction: index,
                },
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(instruction, Instruction::DistributePlusOneCounters { .. }) {
            let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id()) else {
                return Err(EffectError::InconsistentProgram);
            };
            let assessment = self
                .window
                .target_assessment()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let amounts = assessment
                .results()
                .iter()
                .filter(|row| row.legality == crate::targets::TargetLegality::Legal)
                .map(|row| {
                    let TargetRef::Object(object) = row.choice.target else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let share = *payload
                        .target_division
                        .get(&row.choice.id)
                        .ok_or(EffectError::InconsistentProgram)?;
                    Ok((object, i64::from(share).into()))
                })
                .collect::<Result<Vec<_>, EffectError>>()?;
            self.place_divided_plus_one_counters(
                amounts,
                crate::occurrence::EventCause::Instruction {
                    entry: entry.id(),
                    instruction: index,
                },
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DivideDamageAmongTargets { total } = instruction {
            let assessment = self
                .window
                .target_assessment()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let source = self.resolution_damage_source(entry)?;
            let division = match self.payloads.get(&entry.id()) {
                Some(StackPayload::Instructions(bound)) => bound.target_division.clone(),
                _ => BTreeMap::new(),
            };
            let total_targets = entry.targets().targets().len();
            let mut inputs = Vec::new();
            for result in assessment.results() {
                if result.legality != crate::targets::TargetLegality::Legal {
                    continue;
                }
                let share = division.get(&result.choice.id).copied().unwrap_or_else(|| {
                    if total_targets <= 1 {
                        total
                    } else {
                        let base = total / u16::try_from(total_targets).unwrap_or(1);
                        let rem = total % u16::try_from(total_targets).unwrap_or(1);
                        if result.choice.id == TargetId(0) {
                            base.saturating_add(rem)
                        } else {
                            base
                        }
                    }
                });
                if share == 0 {
                    continue;
                }
                let target = match result.choice.target {
                    TargetRef::Player(player) => DamageRecipient::Player(player),
                    TargetRef::Object(object) => self
                        .current_characteristics(object)?
                        .and_then(|view| view.damage_recipient())
                        .ok_or(EffectError::IllegalTarget)?,
                    TargetRef::Stack(_) => return Err(EffectError::InconsistentProgram),
                };
                inputs.push(DamageInput {
                    source: source.clone(),
                    target,
                    amount: i64::from(share).into(),
                });
            }
            if inputs.is_empty() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::DamageOrderedTargets { .. } | Instruction::DivideThreeDamageAmongTargets
        ) {
            let assessment = self
                .window
                .target_assessment()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let source = self.resolution_damage_source(entry)?;
            let (first, others) =
                if let Instruction::DamageOrderedTargets { first, others } = instruction {
                    (first, others)
                } else {
                    let allocated_to_first = match entry.targets().targets().len() {
                        1 => 3,
                        2 => 2,
                        3 => 1,
                        _ => return Err(EffectError::InconsistentProgram),
                    };
                    (allocated_to_first.into(), 1.into())
                };
            let mut inputs = Vec::new();
            for result in assessment.results() {
                if result.legality != crate::targets::TargetLegality::Legal {
                    continue;
                }
                let target = match result.choice.target {
                    TargetRef::Player(player) => DamageRecipient::Player(player),
                    TargetRef::Object(object) => self
                        .current_characteristics(object)?
                        .and_then(|view| view.damage_recipient())
                        .ok_or(EffectError::IllegalTarget)?,
                    TargetRef::Stack(_) => return Err(EffectError::InconsistentProgram),
                };
                inputs.push(DamageInput {
                    source: source.clone(),
                    target,
                    amount: if result.choice.id == TargetId(0) {
                        first.clone()
                    } else {
                        others.clone()
                    },
                });
            }
            if inputs.is_empty() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExileResolvingSpell {
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.exile_on_resolve = true;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReturnResolvingSpellToHand {
            let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
            else {
                return Err(EffectError::InconsistentProgram);
            };
            payload.return_to_hand_on_resolve = true;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::Ascend {
            let controller = entry.controller();
            if !self.has_city_blessing(controller) {
                let evaluated = self.evaluated_characteristics()?;
                let controlled_count = evaluated
                    .objects
                    .values()
                    .filter(|p| {
                        p.zone == ObjectZone::Battlefield && p.controller == Some(controller)
                    })
                    .take(10)
                    .count();
                if controlled_count >= 10 {
                    let before = self.rules_snapshot(None)?;
                    self.characteristics.city_blessing.insert(controller);
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
                                crate::occurrence::RulesOccurrence::CityBlessingGained {
                                    player: controller,
                                },
                            ])
                        },
                    )?;
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::AdditionalCombatAfterUntappingAttackers
                | Instruction::AdditionalCombat { .. }
        ) {
            let can_schedule_combat = self.current_step.is_some_and(|step| {
                matches!(
                    step.phase(),
                    crate::turn::Phase::Beginning
                        | crate::turn::Phase::PrecombatMain
                        | crate::turn::Phase::Combat
                        | crate::turn::Phase::PostcombatMain
                )
            });
            let mut attackers = match instruction {
                Instruction::AdditionalCombat {
                    untap: crate::program::CombatUntapGroup::None,
                } => Vec::new(),
                Instruction::AdditionalCombat {
                    untap: crate::program::CombatUntapGroup::ControlledCreatures,
                } => self
                    .evaluated_characteristics()?
                    .objects
                    .values()
                    .filter(|p| {
                        p.zone == ObjectZone::Battlefield
                            && p.controller == Some(entry.controller())
                            && p.is_creature()
                    })
                    .map(|p| p.object)
                    .collect::<Vec<_>>(),
                Instruction::AdditionalCombat {
                    untap: crate::program::CombatUntapGroup::SourceAndControlledSubtype(subtype),
                } => {
                    let StackKind::TriggeredAbility { source } = entry.kind() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    self.evaluated_characteristics()?
                        .objects
                        .values()
                        .filter(|p| {
                            p.zone == ObjectZone::Battlefield
                                && (p.object == source.id()
                                    || (p.controller == Some(entry.controller())
                                        && p.current_creature_subtypes.contains(&subtype)))
                        })
                        .map(|p| p.object)
                        .collect()
                }
                _ => {
                    if matches!(instruction,Instruction::AdditionalCombat {untap:crate::program::CombatUntapGroup::AttackersIfSourceAttacksHighestLife}) {
                            let StackKind::TriggeredAbility {source}=entry.kind() else {return Err(EffectError::InconsistentProgram);};
                            let defending = self.combat.as_ref().and_then(|combat|combat.attacker(source.id())).map(|member|(member.defender(),member.defending_planeswalker()))
                                .or_else(||self.departed_attack_defenders.as_ref().and_then(|old|old.get(&source.id()).copied()));
                            let qualifies=defending.filter(|(_,walker)|walker.is_none()).and_then(|(seat,_)|self.life.get(seat)).is_some_and(|defending|
                                    self.window.participants().iter().all(|&p|self.life.get(p).is_some_and(|total|total<=defending)));
                            if !qualifies {self.advance_instruction()?; return Ok(StepOutcome::Continue);}
                        }
                    self.combat
                        .as_ref()
                        .into_iter()
                        .flat_map(|combat| combat.attackers())
                        .map(|member| member.combatant().object())
                        .collect()
                }
            };
            attackers.retain(|id| {
                self.window.cards().get(*id).and_then(GameObject::tapped) == Some(true)
            });
            let before = self.rules_snapshot(None)?;
            let mut values = self.untap_for_effect_with_stun(entry.id(), &attackers)?;
            if can_schedule_combat {
                self.additional_combats = self
                    .additional_combats
                    .checked_add(1)
                    .ok_or(EffectError::InconsistentProgram)?;
                values.push(crate::occurrence::RulesOccurrence::AdditionalCombatCreated);
            }
            if !values.is_empty() {
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| Ok(values),
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::DisallowDamagePreventionThisTurn {
            self.damage_state.prevention_disallowed_this_turn = true;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::GrantNoMaxHandSizeRestOfGame {
            self.characteristics
                .no_max_hand_size_players
                .insert(entry.controller());
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::TakeExtraTurn {
            let before = self.rules_snapshot(None)?;
            let player = entry.controller();
            let current = &before.view().evaluated;
            let matches_affected = |rel: &crate::program::AffectedPlayers, ctrl: SeatId| match rel {
                crate::program::AffectedPlayers::One(
                    crate::program::ResolvingPlayer::Controller,
                ) => player == ctrl,
                crate::program::AffectedPlayers::EachOpponent
                | crate::program::AffectedPlayers::EachOtherPlayer => player != ctrl,
                crate::program::AffectedPlayers::EachPlayer => true,
                crate::program::AffectedPlayers::One(_) => true,
            };
            let skip_extra_turn = current.objects.values().any(|perm| {
                let Some(ctrl) = perm.controller else {
                    return false;
                };
                perm.zone == ObjectZone::Battlefield
                    && perm.has_battlefield_rule(|ab| {
                        matches!(
                            ab,
                            crate::characteristics::StaticContinuousAbility::EventReplacement {
                                event: crate::characteristics::ReplaceableEvent::ExtraTurn {
                                    player: rel
                                },
                                action: crate::characteristics::StaticReplacementAction::Skip,
                                ..
                            } if matches_affected(rel, ctrl)
                        )
                    })
            }) || self.temporary_rules.iter().any(|active| {
                self.active_temporary_static(active, current)
                    .is_some_and(|ab| {
                        matches!(
                            ab,
                            crate::characteristics::StaticContinuousAbility::EventReplacement {
                                event: crate::characteristics::ReplaceableEvent::ExtraTurn {
                                    player: rel
                                },
                                action: crate::characteristics::StaticReplacementAction::Skip,
                                ..
                            } if matches_affected(rel, active.controller)
                        )
                    })
            });
            if !skip_extra_turn {
                self.extra_turns.push(player);
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![crate::occurrence::RulesOccurrence::ExtraTurnCreated {
                            player,
                        }])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::PreventDamageThisTurn(filter) = instruction {
            let before = self.rules_snapshot(None)?;
            let source = match filter {
                crate::damage::TurnPreventionFilter::ToTarget
                | crate::damage::TurnPreventionFilter::CombatFromCreaturesOtherThanTarget => {
                    let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Some(target)
                }
                _ => match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                },
            };
            self.damage_state
                .turn_preventions
                .insert((entry.id(), index), (entry.controller(), filter, source));
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
                        crate::occurrence::RulesOccurrence::TurnPreventionCreated {
                            creator: entry.id(),
                            instruction: index,
                            controller: entry.controller(),
                            filter,
                        },
                    ])
                },
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::DrawAndLoseLifeForTargetGraveyardCreatures {
            let player = self.resolving_player(crate::program::ResolvingPlayer::TargetPlayer)?;
            let current = self.evaluated_characteristics()?;
            let amount = self
                .requested_draw_count(entry.controller())
                .unwrap_or_else(|| {
                    RulesInteger::from_usize(
                        current
                            .objects
                            .values()
                            .filter(|card| {
                                card.zone == ObjectZone::Graveyard(player)
                                    && card.is_creature()
                                    && self
                                        .window
                                        .cards()
                                        .get(card.object)
                                        .is_some_and(|object| object.card_id().is_some())
                            })
                            .count(),
                    )
                });
            ctx.draws.append(draw_cards_step!(
                self,
                ctx,
                entry.controller(),
                amount.clone(),
                DrawOrigin::Instruction {
                    entry: entry.id(),
                    instruction: index
                },
                ctx.append
            ));
            self.adjust_life_at_cut(
                entry.id(),
                index,
                entry.controller(),
                &crate::life::LifeAdjustment::Lose(amount),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::DrawEachPlayerForPayments {
            let paid = self
                .resolution_cursor()
                .and_then(|cur| cur.impulse.contributions.clone())
                .unwrap_or_else(|| 0.into());
            instruction = Instruction::DrawEachPlayer {
                amount: ValueExpr::Literal(paid),
            };
        }
        if let Instruction::DrawEachPlayer { ref amount }
        | Instruction::DrawControllerAndTarget { ref amount } = instruction
        {
            let target = if matches!(instruction, Instruction::DrawControllerAndTarget { .. }) {
                Some(self.resolving_player(crate::program::ResolvingPlayer::TargetPlayer)?)
            } else {
                None
            };
            let amount = self.resolve_value_expr(entry.controller(), amount)?;
            let players = self
                .window
                .apnap_players()?
                .filter(|player| {
                    target.is_none() || Some(*player) == target || *player == entry.controller()
                })
                .collect::<Vec<_>>();
            for player in players {
                ctx.draws.append(draw_cards_step!(
                    self,
                    ctx,
                    player,
                    amount.clone(),
                    DrawOrigin::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    ctx.append
                ));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DrawPlayersControllingName(ref name) = instruction {
            let evaluated = self.evaluated_characteristics()?;
            let players = self
                .window
                .apnap_players()?
                .filter(|player| {
                    evaluated.objects.values().any(|object| {
                        object.zone == ObjectZone::Battlefield
                            && object.controller == Some(*player)
                            && self.name_rules.has_name(&object.current_names, name)
                    })
                })
                .collect::<Vec<_>>();
            for player in players {
                ctx.draws.append(draw_cards_step!(
                    self,
                    ctx,
                    player,
                    1.into(),
                    DrawOrigin::Instruction {
                        entry: entry.id(),
                        instruction: index
                    },
                    ctx.append
                ));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::DrawHighestEntrantController
                | Instruction::DrawCombatDamageSourceController
                | Instruction::DrawRelatedPlayer
        ) {
            let player = if instruction == Instruction::DrawRelatedPlayer {
                let player = self.captured_related_player()?;
                self.window
                    .participants()
                    .contains(&player)
                    .then_some(player)
            } else if instruction == Instruction::DrawHighestEntrantController {
                self.highest_entrant_controller()?
            } else {
                self.combat_damage_source_controller()?
            };
            if let Some(player) = player {
                ctx.draws.append(draw_cards_step!(
                    self,
                    ctx,
                    player,
                    1.into(),
                    DrawOrigin::Instruction {
                        entry: entry.id(),
                        instruction: index
                    },
                    ctx.append
                ));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DrawPlayer {
            players,
            ref amount,
        } = instruction
        {
            let cursor_snap = self.resolution_cursor().cloned();
            let recipients =
                self.selection_players(players, entry.controller(), cursor_snap.as_ref())?;
            let count = self.resolve_value_expr(entry.controller(), amount)?;
            for recipient in recipients {
                ctx.draws.append(draw_cards_step!(
                    self,
                    ctx,
                    recipient,
                    count.clone(),
                    DrawOrigin::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    ctx.append,
                ));
            }
            self.advance_instruction()?;
            #[cfg(test)]
            characteristic_observations::instruction(self, "Completed", entry.id(), index, None);
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DrawController { ref amount }
        | Instruction::DrawTargetPlayer { ref amount } = instruction
        {
            let recipient = if matches!(instruction, Instruction::DrawTargetPlayer { .. }) {
                self.resolving_player(crate::program::ResolvingPlayer::TargetPlayer)?
            } else {
                entry.controller()
            };
            let amount = self.resolve_value_expr(entry.controller(), amount)?;
            ctx.draws.append(draw_cards_step!(
                self,
                ctx,
                recipient,
                amount,
                DrawOrigin::Instruction {
                    entry: entry.id(),
                    instruction: index,
                },
                ctx.append,
            ));
            self.advance_instruction()?;
            #[cfg(test)]
            characteristic_observations::instruction(self, "Completed", entry.id(), index, None);
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::RegenerateTarget | Instruction::RegenerateSource
        ) {
            self.create_regeneration_shield(
                entry,
                index,
                instruction == Instruction::RegenerateTarget,
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::RegenerateControlledSubtype(kind) = instruction {
            let evaluated = self.evaluated_characteristics()?;
            let subjects = evaluated
                .objects
                .values()
                .filter(|permanent| {
                    permanent.zone == ObjectZone::Battlefield
                        && permanent.is_creature()
                        && permanent.controller == Some(entry.controller())
                        && permanent.current_creature_subtypes.contains(&kind)
                })
                .map(|permanent| permanent.object)
                .collect();
            self.create_regeneration_shields(entry.id(), index, subjects, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RevealTopAndRemoveAttackerIfForest {
            self.reveal_top_against_attacker(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RemoveTargetFromCombat {
            let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            if let Some(combat) = &mut self.combat {
                combat.reconcile(|combatant, _| combatant.object() != object);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RequireTargetAttack {
            let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            let count = self.creatures_required_to_attack.entry(object).or_default();
            *count = count
                .checked_add(1)
                .ok_or(EffectError::InconsistentProgram)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::GoadOtherPlayersCreatures | Instruction::GoadTarget
        ) {
            self.goad_creatures(
                entry,
                index,
                instruction == Instruction::GoadTarget,
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RememberPermanentTarget {
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
        if instruction == Instruction::ControllerLosesRememberedManaValue {
            let (_, value) = self
                .resolution_cursor()
                .and_then(|c| c.remembered_permanent.clone())
                .ok_or(EffectError::InconsistentProgram)?;
            self.adjust_life_at_cut(
                entry.id(),
                index,
                entry.controller(),
                &crate::life::LifeAdjustment::Lose(value),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExiledTargetControllerGainsPower {
            let remembered = self
                .resolution_cursor()
                .and_then(|cursor| cursor.exiled_target.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let gain = remembered
                .power
                .clone()
                .ok_or(EffectError::InconsistentProgram)?
                .max(0.into());
            self.adjust_life_at_cut(
                entry.id(),
                index,
                remembered.controller,
                &crate::life::LifeAdjustment::Gain(gain),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RepeatOpponentLifeLoss {
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), entry.controller())?;
            let adjustments = opponents
                .into_iter()
                .filter_map(|player| {
                    self.characteristics
                        .life_lost_this_turn
                        .get(&player)
                        .map(|loss| (player, crate::life::LifeAdjustment::Lose(loss.clone())))
                })
                .collect();
            self.adjust_players_at_cut(entry.id(), index, adjustments, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::OpponentsLoseAndControllerGains(ref amount) = instruction {
            let amount = self.resolve_value_expr(entry.controller(), amount)?;
            self.adjust_each_opponent_life_at_cut(
                entry.id(),
                index,
                entry.controller(),
                &crate::life::LifeAdjustment::Lose(amount.clone()),
                Some(&crate::life::LifeAdjustment::Gain(amount)),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AdjustPlayersLife {
            players,
            gain,
            amount,
        } = &instruction
        {
            let resolved = self.resolve_value_expr(entry.controller(), amount)?;
            if resolved.is_positive() {
                let adj = if *gain {
                    crate::life::LifeAdjustment::Gain(resolved)
                } else {
                    crate::life::LifeAdjustment::Lose(resolved)
                };
                let cursor = self.resolution_cursor();
                let seats = self.selection_players(*players, entry.controller(), cursor)?;
                let adjustments: BTreeMap<_, _> =
                    seats.into_iter().map(|p| (p, adj.clone())).collect();
                self.adjust_players_at_cut(entry.id(), index, adjustments, ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AdjustEachOpponentLife {
            adjustment,
            controller_adjustment,
        } = &instruction
        {
            self.adjust_each_opponent_life_at_cut(
                entry.id(),
                index,
                entry.controller(),
                adjustment,
                controller_adjustment.as_ref(),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RevealSharedTypeCreatureCastOrBottom {
            let cursor = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?;
            if !cursor.impulse.cast
                && let Some(id) = cursor.impulse.revealed_library_card
                && self.window.cards().library(entry.controller()).first() == Some(&id)
            {
                self.bottom_remaining_looked_at(entry.controller(), entry.id(), &[id])?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RevealOddCastOrDraw {
            let state = &self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .impulse;
            if !state.decided {
                return Err(EffectError::InconsistentProgram);
            }
            if !state.cast {
                ctx.draws.append(draw_cards_step!(
                    self,
                    ctx,
                    entry.controller(),
                    1.into(),
                    DrawOrigin::Instruction {
                        entry: entry.id(),
                        instruction: index
                    },
                    ctx.append
                ));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ImpulseCastOrDamage(amount) = &instruction {
            let impulse = &self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .impulse;
            if !impulse.decided {
                return Err(EffectError::InconsistentProgram);
            }
            if impulse.cast {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let targets = self.group_damage_recipients(
                crate::program::DamageGroup::Opponents,
                entry.controller(),
            )?;
            if targets.is_empty() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let source = self.resolution_damage_source(entry)?;
            let inputs = targets
                .into_iter()
                .map(|target| DamageInput {
                    source: source.clone(),
                    target,
                    amount: amount.clone(),
                })
                .collect();
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DrawAndDamageOpponents { amount } = &instruction {
            let amount = match self.requested_draw_count(entry.controller()) {
                Some(original) => original,
                None => self.resolve_value_expr(entry.controller(), amount)?,
            };
            if !amount.is_positive() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            ctx.draws.append(draw_cards_step!(
                self,
                ctx,
                entry.controller(),
                amount.clone(),
                DrawOrigin::Instruction {
                    entry: entry.id(),
                    instruction: index
                },
                ctx.append
            ));
            let targets = self.group_damage_recipients(
                crate::program::DamageGroup::Opponents,
                entry.controller(),
            )?;
            if targets.is_empty() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let source = self.resolution_damage_source(entry)?;
            let inputs = targets
                .into_iter()
                .map(|target| DamageInput {
                    source: source.clone(),
                    target,
                    amount: amount.clone(),
                })
                .collect();
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
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
        mut instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if matches!(
            instruction,
            Instruction::TargetDamagesItsControllersOtherCreatures
                | Instruction::OtherCreaturesReturnPowerDamage
        ) {
            let returning = instruction == Instruction::OtherCreaturesReturnPowerDamage;
            let inputs = self.cohort_power_damage(entry, returning)?;
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
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ReflectAttachedCreatureDamage {
            if let Some(input) = self.reflected_attached_damage(entry.id())? {
                self.install_damage_batch(
                    DamageOrigin::Resolution {
                        entry: entry.id(),
                        instruction: index,
                    },
                    DamageKind::Noncombat,
                    vec![input],
                    ctx.journal,
                )?;
            } else {
                self.advance_instruction()?;
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::DamagePlayersHalfLife {
            let source = self.resolution_damage_source(entry)?;
            let mut inputs = Vec::new();
            for &player in self.window.participants() {
                let amount = self
                    .life
                    .get(player)
                    .ok_or(EffectError::InconsistentProgram)?
                    .nonnegative_half_rounded_down();
                if amount.is_positive() {
                    inputs.push(DamageInput {
                        source: source.clone(),
                        target: DamageRecipient::Player(player),
                        amount,
                    });
                }
            }
            if inputs.is_empty() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::DamageDamagedPlayersCreatures {
            let player = self.captured_related_player()?;
            let amount = self.resolve_value_expr(
                entry.controller(),
                &ValueExpr::event(EventValueKind::CombatDamageToPlayer),
            )?;
            let source = self.resolution_damage_source(entry)?;
            let inputs: Vec<_> = self
                .evaluated_characteristics()?
                .objects
                .values()
                .filter(|object| {
                    object.zone == ObjectZone::Battlefield
                        && object.is_creature()
                        && object.controller == Some(player)
                })
                .map(|object| DamageInput {
                    source: source.clone(),
                    target: DamageRecipient::Creature(object.object),
                    amount: amount.clone(),
                })
                .collect();
            if inputs.is_empty() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ExileNonlandsToHandAndDamage { .. } = instruction {
            let Some(amount) = self.progress_nonland_search(entry, index)? else {
                return Ok(StepOutcome::Continue);
            };
            if amount == 0 {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            instruction = Instruction::DamageGroup {
                group: crate::program::DamageGroup::Controller,
                amount: RulesInteger::from_usize(amount),
            };
        }
        if let Instruction::DamageOpponentsAmount(ref amount)
        | Instruction::DamageRelatedPlayerAmount(ref amount) = instruction
        {
            use crate::program::DamageGroup;
            let group = if matches!(instruction, Instruction::DamageRelatedPlayerAmount(_)) {
                DamageGroup::RelatedPlayer
            } else {
                DamageGroup::Opponents
            };
            let resolved = self.resolve_value_expr(entry.controller(), amount)?;
            if !resolved.is_positive() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            instruction = Instruction::DamageGroup {
                group,
                amount: resolved,
            };
        }
        if let Instruction::DamageGroupAmount { group, ref amount } = instruction {
            let resolved = self.resolve_value_expr(entry.controller(), amount)?;
            if !resolved.is_positive() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            instruction = Instruction::DamageGroup {
                group,
                amount: resolved,
            };
        }
        if let Instruction::DoublePowerToughness {
            target_self,
            power,
            toughness,
            duration,
        } = instruction
        {
            let target_id = if target_self {
                match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                }
            } else if let Ok(target) = self.instruction_target(entry) {
                match target.choice().target {
                    TargetRef::Object(id) => Some(id),
                    TargetRef::Player(_) | TargetRef::Stack(_) => None,
                }
            } else {
                None
            };
            if let Some(object) = target_id
                && let Some(pt) = self
                    .evaluated_characteristics()?
                    .get(object)
                    .filter(|o| o.zone == ObjectZone::Battlefield)
                    .and_then(|o| o.power_toughness.clone())
            {
                let dp = if power { pt.derived.power } else { 0.into() };
                let dt = if toughness {
                    pt.derived.toughness
                } else {
                    0.into()
                };
                if let Some(resolved_dur) = self.resolve_duration(entry, duration)? {
                    self.modify_objects_for_duration(
                        entry,
                        index,
                        [object].into(),
                        crate::characteristics::ResolvedParts(vec![
                            crate::characteristics::ResolvedPart::ModifyPT(
                                crate::characteristics::PowerToughness {
                                    power: dp,
                                    toughness: dt,
                                },
                            ),
                        ]),
                        resolved_dur,
                        ctx.append,
                    )?;
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AddCounterAndDischargeOnResolution(ordinal) = instruction {
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id()) else {
                return Err(EffectError::InconsistentProgram);
            };
            let ability = payload
                .activated_ability
                .clone()
                .ok_or(EffectError::InconsistentProgram)?;
            let resolutions = self
                .ability_resolutions_this_turn
                .entry((source.id(), ability))
                .or_default();
            *resolutions = resolutions.saturating_add(1);
            let discharge = *resolutions == u16::from(ordinal);
            if self
                .window
                .cards()
                .get(source.id())
                .is_some_and(|o| o.zone() == ObjectZone::Battlefield)
            {
                self.add_source_counters(
                    source.id(),
                    entry.id(),
                    index,
                    1.into(),
                    resolution_choices::CounterPlacement::Ordinary,
                    ctx.append,
                )?;
                if discharge {
                    let before = self.rules_snapshot(None)?;
                    let amount = self.characteristics.take_all_plus_one_counters(source.id());
                    if amount.is_positive() {
                        self.emit_from(crate::occurrence::EventCause::Instruction {entry:entry.id(),instruction:index},&before,None,ctx.append,|old,new,_| Ok(vec![crate::occurrence::RulesOccurrence::PowerToughnessCountersRemoved {
                                object:source.id(),before:old.characteristics.counters(source.id()),after:new.characteristics.counters(source.id()),
                            }]))?;
                        let dealer = self.resolution_damage_source(entry)?;
                        let inputs = self
                            .group_damage_recipients(
                                crate::program::DamageGroup::CreaturesAndPlayers,
                                entry.controller(),
                            )?
                            .into_iter()
                            .map(|target| DamageInput {
                                source: dealer.clone(),
                                target,
                                amount: amount.clone(),
                            })
                            .collect();
                        self.install_damage_batch(
                            DamageOrigin::Resolution {
                                entry: entry.id(),
                                instruction: index,
                            },
                            DamageKind::Noncombat,
                            inputs,
                            ctx.journal,
                        )?;
                        return Ok(StepOutcome::Continue);
                    }
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreatureDealsPowerDamageToOther { trample_overflow } = instruction {
            let inputs = self.linked_creature_power_damage(trample_overflow)?;
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
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DamageCreatureAtSpellMastery { amount, minimum } = &instruction {
            let amount = self.resolve_value_expr(entry.controller(), amount)?;
            if !amount.is_positive() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            let recipient = DamageRecipient::Creature(target);
            let source = self.resolution_damage_source(entry)?;
            let mut inputs = vec![DamageInput {
                source: source.clone(),
                target: recipient,
                amount: amount.clone(),
            }];
            let eligible_cards = self.resolve_value_expr(
                entry.controller(),
                &ValueExpr::count_graveyard(
                    crate::program::ResolvingPlayer::Controller,
                    ObjectFilter::instant_or_sorcery(),
                ),
            )?;
            if eligible_cards >= i64::from(*minimum).into() {
                let controller = self
                    .evaluated_characteristics()?
                    .get(target)
                    .and_then(|value| value.controller)
                    .ok_or(EffectError::InconsistentProgram)?;
                inputs.push(DamageInput {
                    source,
                    target: DamageRecipient::Player(controller),
                    amount,
                });
            }
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DamageTargetControllersCreatures {
            amount,
            include_target,
            ..
        } = &instruction
        {
            let quantity = self.resolve_value_expr(entry.controller(), amount)?;
            let mut inputs = Vec::new();
            if quantity.is_positive() {
                let source = self.resolution_damage_source(entry)?;
                let target = self.instruction_target(entry)?.choice().target;
                inputs = self
                    .target_controller_damage_recipients(target, *include_target)?
                    .into_iter()
                    .map(|target| DamageInput {
                        source: source.clone(),
                        target,
                        amount: quantity.clone(),
                    })
                    .collect();
            }
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
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DamageGroup { group, amount } = &instruction {
            let mut targets = self.group_damage_recipients(*group, entry.controller())?;
            if *group == crate::program::DamageGroup::RelatedPlayerOrTheirPlaneswalker
                && targets.len() > 1
            {
                let recipients: Vec<_> = targets
                    .iter()
                    .map(|recipient| match *recipient {
                        DamageRecipient::Player(player) => TargetRef::Player(player),
                        DamageRecipient::Creature(object)
                        | DamageRecipient::Planeswalker(object)
                        | DamageRecipient::Battle(object) => TargetRef::Object(object),
                    })
                    .collect();
                let Some(chosen) = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.chosen_damage_recipient)
                else {
                    let frame = EffectDecision::DamageRecipientChoice(DamageRecipientChoiceFrame {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: entry.controller(),
                        recipients,
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                let selected = recipients
                    .iter()
                    .position(|recipient| *recipient == chosen)
                    .ok_or(EffectError::IllegalTarget)?;
                targets = vec![targets[selected]];
            }
            if targets.is_empty() {
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            let source = self.resolution_damage_source(entry)?;
            let inputs = targets
                .into_iter()
                .map(|target| DamageInput {
                    source: source.clone(),
                    target,
                    amount: amount.clone(),
                })
                .collect();
            self.install_damage_batch(
                DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                },
                DamageKind::Noncombat,
                inputs,
                ctx.journal,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::UntapAbilitySource | Instruction::TapAbilitySource
        ) {
            let tap = instruction == Instruction::TapAbilitySource;
            let (StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source }) =
                entry.kind()
            else {
                return Err(EffectError::InconsistentProgram);
            };
            if self
                .window
                .cards()
                .get(source.id())
                .and_then(GameObject::tapped)
                == Some(!tap)
            {
                let old = self.rules_snapshot(None)?;
                let occurrences = if tap {
                    let (updated, changes) =
                        self.window.tap_for_effect(entry.id(), &[source.id()])?;
                    self.window = updated;
                    occurrence_flow::status_occurrences(
                        changes,
                        crate::occurrence::StatusReason::Effect,
                    )
                } else {
                    self.untap_for_effect_with_stun(entry.id(), &[source.id()])?
                };
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &old,
                    None,
                    ctx.append,
                    |_, _, _| Ok(occurrences),
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AbilitySourceControllerLosesLife(amount) = instruction {
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let player = self
                .current_characteristics(source.id())?
                .and_then(|permanent| permanent.controller)
                .or_else(|| {
                    self.departed_reflexive_sources
                        .get(&source.id())
                        .and_then(|last| last.object.controller())
                })
                .ok_or(EffectError::InconsistentProgram)?;
            self.adjust_life_at_cut(
                entry.id(),
                index,
                player,
                &crate::life::LifeAdjustment::Lose(i64::from(amount).into()),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::RelatedPlayerLosesLife(amount) = instruction {
            let player = self.captured_related_player()?;
            if self.window.participants().contains(&player) {
                self.adjust_life_at_cut(
                    entry.id(),
                    index,
                    player,
                    &crate::life::LifeAdjustment::Lose(i64::from(amount).into()),
                    ctx.append,
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RelatedPlayerLosesHalfLife {
            let player = self.captured_related_player()?;
            if self.window.participants().contains(&player) {
                let life = self
                    .life
                    .get(player)
                    .ok_or(EffectError::InconsistentProgram)?;
                let amount = life.nonnegative_half_rounded_up();
                self.adjust_life_at_cut(
                    entry.id(),
                    index,
                    player,
                    &crate::life::LifeAdjustment::Lose(amount),
                    ctx.append,
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::DrainTargetAndGain(amount) = &instruction {
            let TargetRef::Player(player) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            let quantity = self.resolve_value_expr(entry.controller(), amount)?;
            if quantity.is_positive() {
                self.drain_target_and_gain(entry, index, player, quantity, ctx.append)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExchangeLifeWithSourceToughness {
            self.exchange_life_with_source_toughness(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::GainControllerLife { amount } = &instruction {
            let gain = self
                .resolve_value_expr(entry.controller(), amount)?
                .max(0.into());
            self.adjust_life_at_cut(
                entry.id(),
                index,
                entry.controller(),
                &crate::life::LifeAdjustment::Gain(gain),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AdjustControllerLife { adjustment } = instruction {
            self.adjust_life_at_cut(
                entry.id(),
                index,
                entry.controller(),
                &adjustment,
                ctx.append,
            )?;
            self.advance_instruction()?;
            #[cfg(test)]
            characteristic_observations::instruction(self, "Completed", entry.id(), index, None);
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::SetControlledCreatureSizeToHand {
            use crate::characteristics::{PowerToughness, ResolvedPart, ResolvedParts};
            let controller = entry.controller();
            let count = RulesInteger::from_usize(
                self.window
                    .cards()
                    .iter()
                    .filter(|card| card.zone() == ObjectZone::Hand(controller))
                    .count(),
            );
            let affected = self
                .evaluated_characteristics()?
                .objects
                .values()
                .filter(|object| {
                    object.zone == ObjectZone::Battlefield
                        && object.controller == Some(controller)
                        && object.is_creature()
                })
                .map(|object| object.object)
                .collect();
            self.modify_objects_until_cleanup(
                entry,
                index,
                affected,
                ResolvedParts(vec![ResolvedPart::SetPT(PowerToughness {
                    power: count.clone(),
                    toughness: count,
                })]),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ModifyGroupByAmount {
            group,
            scale,
            amount,
            keywords,
        } = &instruction
        {
            let n = self.resolve_value_expr(entry.controller(), amount)?;
            instruction = Instruction::ModifyGroupUntilEndOfTurn {
                group: group.clone(),
                keywords: keywords.clone(),
                power_toughness: Some(crate::characteristics::PowerToughness {
                    power: scale.power.times(&n),
                    toughness: scale.toughness.times(&n),
                }),
            };
        }
        if let Instruction::ModifyGroupUntilEndOfTurn {
            group,
            power_toughness,
            keywords,
        } = instruction
        {
            self.modify_group(entry, index, *group, power_toughness, keywords, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::BoostSource {
            power_toughness,
            keywords,
        } = instruction
        {
            self.resolve_source_boost(entry, index, power_toughness, keywords, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ResolvePowerAbility(kind) = instruction {
            self.resolve_power_ability(entry, kind, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::BoostSaddledAttacker {
            adjustment,
            keyword,
        } = instruction
        {
            self.boost_saddled_attacker(entry, adjustment, keyword, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::ModifyAnyNumberOfPermanents { parts, duration } = &instruction {
            let selected = if entry.targets().targets().is_empty() {
                BTreeSet::new()
            } else {
                self.legal_permanent_targets()?
            };
            if !selected.is_empty() {
                // Target-owner-dependent control changes aren't admitted by this group operand.
                let locals = self
                    .resolution_state()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .locals()
                    .clone();
                let source_object = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                };
                let materialized = self.materialize_resolved_parts(
                    entry,
                    parts,
                    &locals,
                    entry.controller(),
                    source_object,
                )?;
                let before = self.rules_snapshot(None)?;
                let effect = self.characteristics.add_effect_for_objects(
                    entry.id(),
                    selected,
                    materialized,
                    *duration,
                )?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
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
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                effect,
                                appearances,
                            },
                        ])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::TapTargetWhileControllingSource {
            let TargetRef::Object(target) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            let StackKind::TriggeredAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let source = source.id();
            if let Some(epoch) = self
                .window
                .cards()
                .get(source)
                .filter(|card| {
                    card.zone() == ObjectZone::Battlefield
                        && card.controller() == Some(entry.controller())
                })
                .and_then(GameObject::control_epoch)
            {
                self.untap_locks.insert((target, source, epoch));
            }
            if self.window.cards().get(target).and_then(GameObject::tapped) == Some(false) {
                let before = self.rules_snapshot(None)?;
                let (window, changes) = self.window.tap_for_effect(entry.id(), &[target])?;
                self.window = window;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(occurrence_flow::status_occurrences(
                            changes,
                            crate::occurrence::StatusReason::Effect,
                        ))
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        Ok(StepOutcome::Next(instruction))
    }

    #[inline(never)]
    fn progress_phase_7(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        if matches!(
            instruction,
            Instruction::UntapControlledCreatures
                | Instruction::TapAttackersAndSkipNextUntap
                | Instruction::SetGroupTapped { .. }
                | Instruction::SetSubjectTapped { .. }
        ) {
            use crate::program::{EffectSubject, ObjectFilter};
            let (tap, skip_untap, group) = match &instruction {
                Instruction::TapAttackersAndSkipNextUntap => (
                    true,
                    true,
                    EffectSubject::Matching(ObjectFilter::creature().attacking()),
                ),
                Instruction::UntapControlledCreatures => (
                    false,
                    false,
                    EffectSubject::Matching(ObjectFilter::creature().controlled()),
                ),
                Instruction::SetGroupTapped { filter, tapped } => {
                    (*tapped, false, EffectSubject::Matching(*filter.as_ref()))
                }
                Instruction::SetSubjectTapped { subject, tapped } => {
                    (*tapped, false, *subject.as_ref())
                }
                _ => return Err(EffectError::InconsistentProgram),
            };
            let affected = self.effect_group(entry, group)?;
            if skip_untap {
                self.skip_next_controller_untap
                    .extend(affected.iter().copied());
            }
            let changing = affected
                .into_iter()
                .filter(|object| {
                    self.window
                        .cards()
                        .get(*object)
                        .and_then(GameObject::tapped)
                        == Some(!tap)
                })
                .collect::<Vec<_>>();
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = !changing.is_empty();
                cursor.results.last_action_count = Some(RulesInteger::from_usize(changing.len()));
            }
            if !changing.is_empty() {
                let before = self.rules_snapshot(None)?;
                let occurrences = if tap {
                    let (window, changes) = self.window.tap_for_effect(entry.id(), &changing)?;
                    self.window = window;
                    occurrence_flow::status_occurrences(
                        changes,
                        crate::occurrence::StatusReason::Effect,
                    )
                } else {
                    self.untap_for_effect_with_stun(entry.id(), &changing)?
                };
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| Ok(occurrences),
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AffectPermanentTargets { action, .. } = &instruction {
            let mut selected = self.legal_permanent_targets()?;
            let cause = crate::occurrence::EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            };
            match action {
                crate::program::PermanentTargetAction::ToggleTap => {
                    // The optional no-op covers either declining or choosing its current status.
                    if let Some(object) = selected.first().copied() {
                        let before = self.rules_snapshot(None)?;
                        let occurrences =
                            if self.window.cards().get(object).and_then(GameObject::tapped)
                                == Some(true)
                            {
                                self.untap_for_effect_with_stun(entry.id(), &[object])?
                            } else {
                                let (window, changes) =
                                    self.window.tap_for_effect(entry.id(), &[object])?;
                                self.window = window;
                                occurrence_flow::status_occurrences(
                                    changes,
                                    crate::occurrence::StatusReason::Effect,
                                )
                            };
                        self.emit_from(cause, &before, None, ctx.append, |_, _, _| {
                            Ok(occurrences)
                        })?;
                    }
                }
                crate::program::PermanentTargetAction::Tap
                | crate::program::PermanentTargetAction::TapAndSkipNextUntap
                | crate::program::PermanentTargetAction::Untap => {
                    let tap = *action != crate::program::PermanentTargetAction::Untap;
                    if *action == crate::program::PermanentTargetAction::TapAndSkipNextUntap {
                        self.skip_next_controller_untap
                            .extend(selected.iter().copied());
                    }
                    let changing = selected
                        .into_iter()
                        .filter(|id| {
                            self.window.cards().get(*id).and_then(GameObject::tapped) == Some(!tap)
                        })
                        .collect::<Vec<_>>();
                    if !changing.is_empty() {
                        let before = self.rules_snapshot(None)?;
                        let occurrences = if tap {
                            let (window, changes) =
                                self.window.tap_for_effect(entry.id(), &changing)?;
                            self.window = window;
                            occurrence_flow::status_occurrences(
                                changes,
                                crate::occurrence::StatusReason::Effect,
                            )
                        } else {
                            self.untap_for_effect_with_stun(entry.id(), &changing)?
                        };
                        self.emit_from(cause, &before, None, ctx.append, |_, _, _| {
                            Ok(occurrences)
                        })?;
                    }
                }
                crate::program::PermanentTargetAction::PutCountersByCurrentType => {
                    let current = self.evaluated_characteristics()?;
                    let creatures = selected
                        .iter()
                        .copied()
                        .filter(|id| current.get(*id).is_some_and(|p| p.is_creature()))
                        .collect();
                    let planeswalkers = selected
                        .iter()
                        .copied()
                        .filter(|id| {
                            current
                                .get(*id)
                                .is_some_and(|p| p.has_type(CardType::Planeswalker))
                        })
                        .collect();
                    self.place_creature_and_loyalty_counters(
                        creatures,
                        planeswalkers,
                        cause,
                        ctx.append,
                    )?;
                }
                crate::program::PermanentTargetAction::SkipNextUntap => {
                    self.skip_next_controller_untap
                        .extend(selected.iter().copied());
                }
                crate::program::PermanentTargetAction::PutCounters { kind, amount } => {
                    let resolved = self.resolve_value_expr(entry.controller(), amount)?;
                    if resolved.is_positive() && !selected.is_empty() {
                        let before = self.rules_snapshot(None)?;
                        let mut occurrences = Vec::new();
                        for object in selected {
                            if !self
                                .window
                                .cards()
                                .get(object)
                                .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                            {
                                continue;
                            }
                            if matches!(*kind, CounterKind::PlusOne | CounterKind::MinusOne) {
                                let raw = if *kind == CounterKind::PlusOne {
                                    crate::characteristics::PowerToughnessCounters {
                                        plus_one: resolved.clone(),
                                        minus_one: 0.into(),
                                    }
                                } else {
                                    crate::characteristics::PowerToughnessCounters {
                                        plus_one: 0.into(),
                                        minus_one: resolved.clone(),
                                    }
                                };
                                let counters = self.adjusted_pt_counters(
                                    &before.view().evaluated,
                                    object,
                                    raw,
                                    None,
                                )?;
                                if !counters.valid_placement() {
                                    continue;
                                }
                                let previous = self.characteristics.counters(object);
                                self.characteristics.place_counters(object, &counters)?;
                                let after_pt = self.characteristics.counters(object);
                                occurrences.push(
                                    crate::occurrence::RulesOccurrence::PermanentCounterChanged {
                                        object,
                                        kind: *kind,
                                        before: if *kind == CounterKind::PlusOne {
                                            previous.plus_one
                                        } else {
                                            previous.minus_one
                                        },
                                        after: if *kind == CounterKind::PlusOne {
                                            after_pt.plus_one
                                        } else {
                                            after_pt.minus_one
                                        },
                                    },
                                );
                            } else {
                                let old = self.counters_of_kind(object, *kind);
                                let added = super::permanent_counters::effect_counter_multiplier(
                                    &before.view().evaluated,
                                    object,
                                )
                                .times(&resolved);
                                let after = old.plus(&added);
                                self.characteristics
                                    .named_permanent_counters_mut(*kind)
                                    .ok_or(EffectError::InconsistentProgram)?
                                    .insert(object, after.clone());
                                if kind.keyword_counter_ability().is_some() {
                                    self.characteristics.refresh_keyword_counter_grant(
                                        entry.id(),
                                        object,
                                        *kind,
                                    )?;
                                }
                                occurrences.push(if *kind == CounterKind::Loyalty {
                                    crate::occurrence::RulesOccurrence::LoyaltyCountersChanged {
                                        object,
                                        before: old,
                                        after,
                                    }
                                } else {
                                    crate::occurrence::RulesOccurrence::PermanentCounterChanged {
                                        object,
                                        kind: *kind,
                                        before: old,
                                        after,
                                    }
                                });
                            }
                        }
                        if !occurrences.is_empty() {
                            self.emit_from(cause, &before, None, ctx.append, |_, _, _| {
                                Ok(occurrences)
                            })?;
                        }
                    }
                }
                crate::program::PermanentTargetAction::PutPlusOneCounter
                | crate::program::PermanentTargetAction::PutPlusOneCounterOnOthers => {
                    self.place_plus_one_counters(selected.into_iter().collect(), cause, ctx.append)?
                }
                crate::program::PermanentTargetAction::Modify { parts, duration }
                | crate::program::PermanentTargetAction::ModifyWithPartner { parts, duration } => {
                    if let Some(target) = selected.first().copied()
                        && let Some(resolved_dur) = self.resolve_duration(entry, *duration)?
                    {
                        if matches!(
                            action,
                            crate::program::PermanentTargetAction::ModifyWithPartner { .. }
                        ) && let Some(partner) =
                            self.characteristics.paired_creatures.get(&target)
                        {
                            selected.insert(*partner);
                        }
                        let locals = self
                            .resolution_state()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .locals()
                            .clone();
                        let owner = self
                            .window
                            .cards()
                            .get(target)
                            .ok_or(EffectError::InconsistentProgram)?
                            .owner();
                        let source_object = match entry.kind() {
                            StackKind::ActivatedAbility { source }
                            | StackKind::TriggeredAbility { source } => Some(source.id()),
                            _ => None,
                        };
                        let materialized = self.materialize_resolved_parts(
                            entry,
                            parts,
                            &locals,
                            owner,
                            source_object,
                        )?;
                        let before = self.rules_snapshot(None)?;
                        let effect = self.characteristics.add_effect_for_objects(
                            entry.id(),
                            selected,
                            materialized,
                            resolved_dur,
                        )?;
                        self.emit_from(
                            cause,
                            &before,
                            None,
                            ctx.append,
                            |previous, current, _| {
                                let appearances = characteristic_appearance_changes(
                                    previous,
                                    current,
                                    effect.affected.iter().copied(),
                                )?;
                                Ok(vec![
                                    crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                        effect,
                                        appearances,
                                    },
                                ])
                            },
                        )?;
                    }
                }
                crate::program::PermanentTargetAction::RequireCombatAction(kind) => {
                    let selected_vec: Vec<ObjectId> = selected.iter().copied().collect();
                    self.apply_combat_action(entry, index, &selected_vec, *kind, ctx.append)?;
                }
                crate::program::PermanentTargetAction::MoveTo(destination) => {
                    let mut resolved_dest = *destination;
                    if let crate::program::SubjectDestination::Library(
                        crate::program::LibraryPlacement::ChoiceTopOrBottom { chooser },
                    ) = resolved_dest
                    {
                        let chosen = self
                            .resolution_cursor_mut()
                            .and_then(|cursor| cursor.chosen_option_index.take());
                        let Some(chosen_idx) = chosen else {
                            let cursor_snap = self
                                .resolution_cursor()
                                .ok_or(EffectError::WrongResolutionFrame)?
                                .clone();
                            let actor =
                                self.resolving_player_identity_at(chooser, Some(&cursor_snap))?;
                            let frame = EffectDecision::OptionIndex(OptionIndexFrame {
                                revision: self.revision,
                                entry: entry.id(),
                                instruction: index,
                                chooser: actor,
                                count: 2,
                                labels: vec![
                                    "Put on top of library".to_owned(),
                                    "Put on bottom of library".to_owned(),
                                ],
                            });
                            return Ok(StepOutcome::Pending(frame));
                        };
                        resolved_dest =
                            crate::program::SubjectDestination::Library(if chosen_idx == 0 {
                                crate::program::LibraryPlacement::Top
                            } else {
                                crate::program::LibraryPlacement::Bottom
                            });
                    }
                    if !selected.is_empty() {
                        let selected_vec: Vec<ObjectId> = selected.iter().copied().collect();
                        let moved_ids = self.move_objects_to_subject_destination_ids(
                            entry,
                            index,
                            entry.controller(),
                            &selected_vec,
                            resolved_dest,
                            ctx.append,
                        )?;
                        if let Some(cursor) = self.resolution_cursor_mut() {
                            cursor.results.last_action_succeeded = !moved_ids.is_empty();
                            cursor.results.last_action_count =
                                Some(RulesInteger::from_usize(moved_ids.len()));
                            cursor.results.last_moved_objects = moved_ids;
                        }
                    }
                }
                crate::program::PermanentTargetAction::Damage(expr) => {
                    let resolved = self.resolve_value_expr(entry.controller(), expr)?;
                    if resolved.is_positive() && !selected.is_empty() {
                        let source = self.resolution_damage_source(entry)?;
                        let current = self.evaluated_characteristics()?;
                        let mut inputs = Vec::new();
                        for id in selected {
                            if let Some(target) = current
                                .get(id)
                                .filter(|view| view.zone == ObjectZone::Battlefield)
                                .and_then(|view| view.damage_recipient())
                            {
                                inputs.push(DamageInput {
                                    source: source.clone(),
                                    target,
                                    amount: resolved.clone(),
                                });
                            }
                        }
                        if !inputs.is_empty() {
                            if let Some(cursor) = self.resolution_cursor_mut() {
                                cursor.results.last_action_succeeded = true;
                                cursor.results.last_action_count =
                                    Some(RulesInteger::from_usize(inputs.len()));
                                cursor.results.dealt_damage_this_resolution = true;
                            }
                            if let Some(StackPayload::Instructions(payload)) =
                                self.payloads.get_mut(&entry.id())
                                && let Some(slot) = payload.body.instructions.get_mut(index)
                            {
                                *slot = Instruction::DamageOrderedTargets {
                                    first: resolved.clone(),
                                    others: resolved,
                                };
                            }
                            self.install_damage_batch(
                                DamageOrigin::Resolution {
                                    entry: entry.id(),
                                    instruction: index,
                                },
                                DamageKind::Noncombat,
                                inputs,
                                ctx.journal,
                            )?;
                            return Ok(StepOutcome::Continue);
                        }
                    }
                    if let Some(cursor) = self.resolution_cursor_mut() {
                        cursor.results.last_action_succeeded = false;
                        cursor.results.last_action_count = Some(0.into());
                    }
                }
                crate::program::PermanentTargetAction::RemoveCounters { kind, amount } => {
                    let selected_vec: Vec<ObjectId> = selected.iter().copied().collect();
                    let total = self.remove_counters_from_objects(
                        entry,
                        index,
                        &selected_vec,
                        *kind,
                        amount.as_ref(),
                        ctx.append,
                    )?;
                    if let Some(cursor) = self.resolution_cursor_mut() {
                        cursor.results.last_action_succeeded = total.is_positive();
                        cursor.results.last_action_count = Some(total);
                    }
                }
                crate::program::PermanentTargetAction::Exile
                | crate::program::PermanentTargetAction::Destroy
                | crate::program::PermanentTargetAction::ReturnToHand => {
                    return Err(EffectError::InconsistentProgram);
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::AddExperienceCounter {
            let before = self.rules_snapshot(None)?;
            let value = self.give_experience_counter(entry.controller());
            self.emit_from(
                crate::occurrence::EventCause::Instruction {
                    entry: entry.id(),
                    instruction: index,
                },
                &before,
                None,
                ctx.append,
                |_, _, _| Ok(vec![value]),
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AddPlayerCounter {
            players,
            kind,
            amount,
        } = instruction
        {
            let resolved = self.resolve_value_expr(entry.controller(), &amount)?;
            let cursor_snapshot = self.resolution_cursor().cloned();
            let recipients =
                self.selection_players(players, entry.controller(), cursor_snapshot.as_ref())?;
            let source_id = match entry.kind() {
                StackKind::CardSpell(id) => crate::damage::DamageSourceId::Object(*id),
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    crate::damage::DamageSourceId::Object(source.id())
                }
                StackKind::Emblem { emblem, .. } => crate::damage::DamageSourceId::Emblem(*emblem),
                StackKind::SpellCopy { .. }
                | StackKind::Speed
                | StackKind::Monarch
                | StackKind::DelayedCopy { .. } => {
                    crate::damage::DamageSourceId::SpellCopy(entry.id())
                }
            };
            let before = self.rules_snapshot(None)?;
            let mut occurrences = Vec::new();
            for player in recipients {
                if let Some(occ) = self.give_player_counters(
                    player,
                    kind,
                    &resolved,
                    entry.id(),
                    source_id,
                    entry.controller(),
                )? {
                    occurrences.push(occ);
                }
            }
            if !occurrences.is_empty() {
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| Ok(occurrences),
                )?;
            }
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = resolved.is_positive();
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ControlManaSourceThroughNextTurn {
            let binding = self
                .execution_bindings(entry.id())
                .and_then(|b| b.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let [member] = binding.causes.members() else {
                return Err(EffectError::InconsistentProgram);
            };
            let crate::occurrence::TriggerRelated::ManaSource(object) = member.related else {
                return Err(EffectError::InconsistentProgram);
            };
            if self
                .window
                .cards()
                .get(object)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            {
                let before = self.rules_snapshot(None)?;
                let effect = self.characteristics.add_effect(
                    entry.id(),
                    object,
                    crate::characteristics::ResolvedParts(vec![
                        crate::characteristics::ResolvedPart::Control(entry.controller()),
                    ]),
                    crate::characteristics::ResolvedDuration::ThroughNextTurn {
                        player: entry.controller(),
                        created_in_turn: self.current_turn_number,
                    },
                )?;
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
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
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::ContinuousEffectCreated {
                                effect,
                                appearances,
                            },
                        ])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::SuppressGroupKeywords { group, keywords } = &instruction {
            let affected = self.effect_group(entry, **group)?;
            self.modify_objects_until_cleanup(
                entry,
                index,
                affected,
                crate::characteristics::ResolvedParts(vec![
                    crate::characteristics::ResolvedPart::Abilities(
                        crate::characteristics::AbilityChange {
                            suppressed_keywords: keywords.clone(),
                            ..Default::default()
                        },
                    ),
                ]),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::GrantControllerFlashUntilCleanup {
            self.flash_through_cleanup.insert(entry.controller());
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RequireBlocksAgainstTargetThisTurn {
            let target = self.instruction_target(entry)?;
            let TargetRef::Object(attacker) = target.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            let requirements = self.must_be_blocked_this_turn.entry(attacker).or_default();
            *requirements = requirements
                .checked_add(1)
                .ok_or(EffectError::InconsistentProgram)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::RequireOpponentsCreaturesBlockThisTurn {
            for object in self.effect_group(
                entry,
                crate::program::EffectSubject::Matching(
                    crate::program::ObjectFilter::creature().opposing(),
                ),
            )? {
                let requirements = self.must_block_this_turn.entry(object).or_default();
                *requirements = requirements
                    .checked_add(1)
                    .ok_or(EffectError::InconsistentProgram)?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::TransformSourceAndAttachToTarget {
            let target = self.instruction_target(entry)?.choice().target;
            if let Some(source) = self.available_transform_source(entry)? {
                self.transform_ability_source(entry, index, ctx.append)?;
                let before = self.rules_snapshot(None)?;
                if let Some(attachment) = self.attach_if_legal(entry.id(), source, target)? {
                    self.emit_from(
                        crate::occurrence::EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |_, _, _| Ok(vec![attachment]),
                    )?;
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::TransformSource {
            self.transform_ability_source(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::AdvanceClassLevel(level) = instruction {
            if let StackKind::ActivatedAbility { source } = entry.kind() {
                let id = source.id();
                if self
                    .window
                    .cards()
                    .get(id)
                    .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                {
                    let before = self.rules_snapshot(None)?;
                    self.characteristics.class_levels.insert(id, level);
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
                                crate::occurrence::RulesOccurrence::ClassLevelReached {
                                    object: id,
                                    level,
                                },
                            ])
                        },
                    )?;
                }
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::SolveSourceCase {
            let source_id = match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    Some(source.id())
                }
                _ => None,
            };
            if let Some(id) = source_id
                && self
                    .window
                    .cards()
                    .get(id)
                    .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                && !self.characteristics.solved_cases.contains(&id)
            {
                let before = self.rules_snapshot(None)?;
                self.characteristics.solved_cases.insert(id);
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![crate::occurrence::RulesOccurrence::CaseSolved {
                            object: id,
                        }])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::BecomePreparedSource {
            let source_id = match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    Some(source.id())
                }
                _ => None,
            };
            if let Some(id) = source_id
                && self
                    .window
                    .cards()
                    .get(id)
                    .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            {
                self.characteristics.prepared.insert(id);
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::PutCounterOnRelatedPermanent {
            self.put_counter_on_related_permanent(entry, index, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ClearSourceCounters(_)
                | Instruction::PutChosenXChargeCounters
                | Instruction::PutCountersOnControlledCreatures
                | Instruction::PutCountersOnControlledSubtype(_)
                | Instruction::PutCountersOnControlledCreaturesAndGrant(_)
                | Instruction::PutCountersOnOtherCreatures
                | Instruction::PutCreatureAndOtherPlaneswalkerCounters
                | Instruction::PutChargeCounterOnSource
                | Instruction::PutBlessingCounterOnSource
                | Instruction::PutNamedCounterOnSource { .. }
                | Instruction::PutCountersOnSubject { .. }
                | Instruction::PutQuestCounterOnSource
                | Instruction::PutStorageCounterOnSource
        ) {
            self.place_ability_counters(entry, index, &instruction, ctx.append)?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::Adapt(amount) = instruction {
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let object = source.id();
            if self
                .window
                .cards()
                .get(object)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                && !self.characteristics.counters(object).plus_one.is_positive()
            {
                self.place_plus_one_counter_amount(
                    vec![object],
                    i64::from(amount).into(),
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    ctx.append,
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::PutCountersOnSource { amount } = instruction {
            let (StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source }) =
                entry.kind()
            else {
                return Err(EffectError::InconsistentProgram);
            };
            let source = source.id();
            if self
                .window
                .cards()
                .get(source)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            {
                self.add_source_counters(
                    source,
                    entry.id(),
                    index,
                    amount,
                    resolution_choices::CounterPlacement::Ordinary,
                    ctx.append,
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::UnattachEquipmentAndProtectHost {
            if let Some(frame) = self.equipment_protection_frame()? {
                return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                    frame,
                )));
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::Unattach {
            let (StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source }) =
                entry.kind()
            else {
                return Err(EffectError::InconsistentProgram);
            };
            let source = source.id();
            let cut = self.rules_snapshot(None)?;
            if self.window.cards().phased_out_under(source).is_none()
                && let Some(host) = self.characteristics.detach(source)
            {
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &cut,
                    None,
                    ctx.append,
                    |_, _, _| {
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::AttachmentChanged {
                                source,
                                before: Some(host),
                                after: None,
                            },
                        ])
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateTokenAndAttach { definition } = instruction {
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
        if let Instruction::PayControllerLife(n) = instruction {
            self.pay_life_at_cut(
                entry.controller(),
                i64::from(n).into(),
                crate::occurrence::EventCause::Instruction {
                    entry: entry.id(),
                    instruction: index,
                },
                None,
                ctx.append,
            )?;
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = true;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::PayControllerEnergy(n) = instruction {
            let actor = entry.controller();
            let before = self.player_counters_of_kind(actor, CounterKind::Energy);
            let cost = RulesInteger::from(i64::from(n));
            if before >= cost {
                let after = before.minus(&cost);
                let before_snapshot = self.rules_snapshot(None)?;
                let map = self
                    .characteristics
                    .player_counters
                    .entry(CounterKind::Energy)
                    .or_default();
                if after.is_positive() {
                    map.insert(actor, after.clone());
                } else {
                    map.remove(&actor);
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
                            crate::occurrence::RulesOccurrence::PlayerCounterChanged {
                                player: actor,
                                kind: CounterKind::Energy,
                                before: before.clone(),
                                after: after.clone(),
                            },
                        ])
                    },
                )?;
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = true;
                }
            } else if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = false;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::PayLife { player, ref amount } = instruction {
            let cursor_snap = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .clone();
            let actor = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
            let cost = self
                .resolve_value_expr(entry.controller(), amount)?
                .max(0.into());
            let evaluated = self.evaluated_characteristics()?;
            if !cost.is_positive()
                || (!evaluated.life_and_creature_costs_prohibited()
                    && self.life.get(actor).is_some_and(|total| *total >= cost))
            {
                if cost.is_positive() {
                    self.pay_life_at_cut(
                        actor,
                        cost,
                        crate::occurrence::EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        None,
                        ctx.append,
                    )?;
                }
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = true;
                }
            } else if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = false;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::PayEnergy { player, ref amount } = instruction {
            let cursor_snap = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .clone();
            let actor = self.resolving_player_identity_at(player, Some(&cursor_snap))?;
            let cost = self
                .resolve_value_expr(entry.controller(), amount)?
                .max(0.into());
            let before = self.player_counters_of_kind(actor, CounterKind::Energy);
            if before >= cost {
                if cost.is_positive() {
                    let after = before.minus(&cost);
                    let before_snapshot = self.rules_snapshot(None)?;
                    let map = self
                        .characteristics
                        .player_counters
                        .entry(CounterKind::Energy)
                        .or_default();
                    if after.is_positive() {
                        map.insert(actor, after.clone());
                    } else {
                        map.remove(&actor);
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
                                crate::occurrence::RulesOccurrence::PlayerCounterChanged {
                                    player: actor,
                                    kind: CounterKind::Energy,
                                    before: before.clone(),
                                    after: after.clone(),
                                },
                            ])
                        },
                    )?;
                }
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = true;
                }
            } else if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = false;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::ExertAbilitySource {
            let source_id = match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    Some(source.id())
                }
                _ => None,
            };
            let succeeded = if let Some(id) = source_id
                && self
                    .window
                    .cards()
                    .get(id)
                    .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
            {
                let before = self.rules_snapshot(None)?;
                self.skip_next_controller_untap.insert(id);
                self.characteristics.exerted_objects_this_turn.insert(id);
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
                            crate::occurrence::RulesOccurrence::KeywordActionPerformed {
                                player: entry.controller(),
                                action: crate::program::PlayerTriggerAction::Exerted,
                            },
                        ])
                    },
                )?;
                true
            } else {
                false
            };
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = succeeded;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        // CollectEvidence and Blight are handled interactively via `begin_resolution_selection`.
        if instruction == Instruction::CopyCreatureAsToken {
            let TargetRef::Object(donor) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            if self.legal_permanent_targets()?.contains(&donor) {
                let definition = self
                    .copiable_program(donor)
                    .ok_or(EffectError::InconsistentProgram)?
                    .clone();
                self.begin_materialized_token_entries(
                    vec![(entry.controller(), 1, definition, false)],
                    None,
                    false,
                )?;
            } else {
                self.advance_instruction()?;
            }
            return Ok(StepOutcome::Continue);
        }
        if instruction == Instruction::Encore {
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
        if instruction == Instruction::GrantNewTokensHaste {
            let tokens = std::mem::take(
                &mut self
                    .resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .last_created_tokens,
            );
            self.modify_objects(
                entry,
                index,
                tokens.into_iter().collect(),
                None,
                [KeywordAbility::Haste].into(),
                ctx.append,
            )?;
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }
        if let Instruction::CreateTokens { creations }
        | Instruction::CreateAttackingTokens { creations, .. } = instruction
        {
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
        power_dealer: Option<crate::damage::DamageSource>,
    ) -> Result<StepOutcome, EffectError> {
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
        if instruction == Instruction::ReturnToOwnersHand
            && let TargetRef::Object(object) = target
        {
            if !self.settled_entry_destinations.contains_key(&object) {
                self.begin_hand_return_replacement(object, entry.controller())?;
                return Ok(StepOutcome::Continue);
            }
            let destination = self
                .settled_entry_destinations
                .remove(&object)
                .ok_or(EffectError::InconsistentProgram)?;
            if destination != crate::program::PublicZoneDestination::Hand {
                let physical = destination
                    .physical(None)
                    .ok_or(EffectError::InconsistentProgram)?;
                let current = self
                    .window
                    .cards()
                    .get(object)
                    .ok_or(EffectError::UnknownObject)?;
                let remains = matches!(
                    (current.zone(), physical),
                    (ObjectZone::Graveyard(_), ObjectDestination::Graveyard)
                );
                if !remains {
                    let before = self.rules_snapshot(None)?;
                    let (window, exit) = self.window.record_targeted_resolution_move_with_exit(
                        self.window.revision(),
                        bound.choice().id,
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
        }
        if matches!(
            instruction,
            Instruction::ReturnOwnGraveyardCreatureToBattlefield
                | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
                | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
                | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker
        ) && let TargetRef::Object(object) = target
            && !self.settled_entry_destinations.contains_key(&object)
        {
            self.begin_battlefield_entry_replacement(
                object,
                entry.controller(),
                crate::occurrence::ZoneReason::ReturnFromGraveyardToBattlefield,
            )?;
            return Ok(StepOutcome::Continue);
        }
        if matches!(
            instruction,
            Instruction::ReturnOwnGraveyardCreatureToBattlefield
                | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
                | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
                | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker
        ) && let TargetRef::Object(object) = target
            && let Some(destination) = self.settled_entry_destinations.get(&object).copied()
            && destination != crate::program::PublicZoneDestination::Battlefield
        {
            self.settled_entry_destinations.remove(&object);
            let physical = destination
                .physical(Some(entry.controller()))
                .ok_or(EffectError::InconsistentProgram)?;
            if physical != ObjectDestination::Graveyard {
                let before = self.rules_snapshot(None)?;
                let (window, exit) = self.window.record_targeted_resolution_move_with_exit(
                    self.window.revision(),
                    bound.choice().id,
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
                            crate::occurrence::ZoneReason::ReturnFromGraveyardToBattlefield,
                            controllers,
                        )?])
                    },
                )?;
            }
            self.advance_instruction()?;
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
            Instruction::CopyAbility
            | Instruction::CopySpell
            | Instruction::ChooseNewTargets
            | Instruction::ChangeOnlyTarget => {
                let TargetRef::Stack(original) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let source = self
                    .window
                    .entries()
                    .iter()
                    .find(|spell| spell.id() == original)
                    .ok_or(EffectError::InconsistentProgram)?;
                if instruction == Instruction::ChangeOnlyTarget
                    && !self.can_change_only_target(source)?
                {
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
                let frame = EffectDecision::CopyTargets(self.copy_resolution_frame()?);
                return Ok(StepOutcome::Pending(frame));
            }
            Instruction::Damage { amount } => {
                let target = match target {
                    TargetRef::Object(target) => self
                        .current_characteristics(target)?
                        .and_then(|view| view.damage_recipient())
                        .ok_or(EffectError::IllegalTarget)?,
                    TargetRef::Player(player) => DamageRecipient::Player(player),
                    TargetRef::Stack(_) => return Err(EffectError::InconsistentProgram),
                };
                let source = match power_dealer {
                    Some(source) => source,
                    None => self.resolution_damage_source(&entry)?,
                };
                let origin = DamageOrigin::Resolution {
                    entry: entry.id(),
                    instruction: index,
                };
                self.install_damage_batch(
                    origin,
                    DamageKind::Noncombat,
                    vec![DamageInput {
                        source,
                        target,
                        amount,
                    }],
                    ctx.journal,
                )?;
            }
            Instruction::AdjustTargetLife { adjustment } => {
                let TargetRef::Player(player) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                self.adjust_life_at_cut(entry.id(), index, player, &adjustment, ctx.append)?;
                self.advance_instruction()?;
                #[cfg(test)]
                characteristic_observations::instruction(
                    self,
                    "Completed",
                    entry.id(),
                    index,
                    None,
                );
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

    fn apply_combat_action(
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
                    });
                }
            }
        }
        Ok(())
    }

    fn resolve_move_subject_objects(
        &self,
        entry: &crate::stack_window::StackEntry,
        subject: crate::program::MoveSubjectRef,
    ) -> Result<Vec<ObjectId>, EffectError> {
        Ok(match subject {
            crate::program::MoveSubjectRef::TargetObject => {
                self.legal_permanent_targets()?.into_iter().collect()
            }
            crate::program::MoveSubjectRef::AbilitySource => match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    vec![source.id()]
                }
                StackKind::CardSpell(id) => vec![*id],
                _ => Vec::new(),
            },
            crate::program::MoveSubjectRef::TriggeringObject => self
                .execution_bindings(entry.id())
                .and_then(|b| b.trigger.as_ref())
                .and_then(|t| t.causes.members().first())
                .and_then(|cause| cause.subject_object_id())
                .into_iter()
                .collect(),
            crate::program::MoveSubjectRef::AttachedHost => match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    match self.attachment_host_for_ability(source.id()) {
                        Some(TargetRef::Object(host)) => vec![host],
                        _ => Vec::new(),
                    }
                }
                _ => Vec::new(),
            },
            crate::program::MoveSubjectRef::RevealedCard => self
                .window
                .cards()
                .library(entry.controller())
                .first()
                .copied()
                .into_iter()
                .collect(),
            crate::program::MoveSubjectRef::CreatedTokens => self
                .resolution_cursor()
                .map(|c| c.last_created_tokens.clone())
                .unwrap_or_default(),
            crate::program::MoveSubjectRef::LastMoved => self
                .resolution_cursor()
                .map(|c| {
                    if !c.results.last_moved_objects.is_empty() {
                        c.results.last_moved_objects.clone()
                    } else {
                        c.results.returned_target_permanent.into_iter().collect()
                    }
                })
                .unwrap_or_default(),
            crate::program::MoveSubjectRef::RememberedFirst => self
                .legal_linked_targets()?
                .map(|(a, _)| a)
                .into_iter()
                .collect(),
            crate::program::MoveSubjectRef::RememberedSecond => self
                .legal_linked_targets()?
                .map(|(_, b)| b)
                .into_iter()
                .collect(),
        })
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

    fn suspend_bottom_random_if_needed(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        objects: Vec<ObjectId>,
    ) -> Result<Option<Vec<ObjectId>>, EffectError> {
        if let Some(mut piles) = self
            .resolution_cursor_mut()
            .and_then(|c| c.revealed_piles.take())
            && let Some(ordered) = piles.bottom_order.take()
        {
            return Ok(Some(ordered));
        }
        let mut owner_counts: BTreeMap<SeatId, usize> = BTreeMap::new();
        for &id in &objects {
            if let Some(card) = self.window.cards().get(id) {
                *owner_counts.entry(card.owner()).or_insert(0) += 1;
            }
        }
        if owner_counts.values().all(|&count| count <= 1) {
            return Ok(Some(objects));
        }
        let mut work = revealed_piles::RevealedPiles::new(objects.clone());
        work.pending_random_bottom = Some(objects);
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .revealed_piles = Some(Box::new(work));
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
            cursor,
            chooser: entry.controller(),
            options: BTreeSet::new(),
            rebound_or_buyback: false,
            cast: None,
        })));
        Ok(None)
    }

    fn remove_counters_from_objects(
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

fn apply_copy_exceptions(
    copied: &mut Program,
    exceptions: &crate::program::CopyExceptions,
    receiver_base: Option<&Program>,
) {
    if exceptions.nonlegendary {
        copied
            .supertypes
            .remove(&crate::program::Supertype::Legendary);
    }
    if let Some(orig) = receiver_base {
        if exceptions.keep_name {
            copied.names = orig.names.clone();
        }
        if exceptions.keep_types {
            copied.card_types.extend(orig.card_types.iter().copied());
            copied
                .creature_subtypes
                .extend(orig.creature_subtypes.iter().copied());
            copied
                .artifact_subtypes
                .extend(orig.artifact_subtypes.iter().copied());
            copied
                .enchantment_subtypes
                .extend(orig.enchantment_subtypes.iter().copied());
            copied
                .land_subtypes
                .extend(orig.land_subtypes.iter().copied());
        }
        if exceptions.keep_abilities {
            copied
                .printed_keywords
                .extend(orig.printed_keywords.iter().copied());
            let mut next_trig = copied
                .triggered
                .iter()
                .map(|t| t.id.0)
                .max()
                .map_or(0, |m| m.saturating_add(1));
            for tr in &orig.triggered {
                let mut cloned = tr.clone();
                cloned.id = crate::program::TriggeredAbilityId(next_trig);
                next_trig = next_trig.saturating_add(1);
                copied.triggered.push(cloned);
            }
            let mut next_act = copied
                .activated
                .iter()
                .map(|a| a.id.0)
                .max()
                .map_or(0, |m| m.saturating_add(1));
            for act in &orig.activated {
                let mut cloned = act.clone();
                cloned.id = crate::activated::ActivatedAbilityId(next_act);
                next_act = next_act.saturating_add(1);
                copied.activated.push(cloned);
            }
        }
    }
    copied
        .card_types
        .extend(exceptions.extra_types.iter().copied());
    copied
        .artifact_subtypes
        .extend(exceptions.extra_artifact_subtypes.iter().copied());
    copied
        .creature_subtypes
        .extend(exceptions.extra_creature_subtypes.iter().copied());
    copied
        .printed_keywords
        .extend(exceptions.extra_keywords.iter().copied());
    if let Some(pt) = &exceptions.override_pt
        && let ProgramKind::Permanent(permanent) = &mut copied.kind
    {
        permanent.printed_pt = Some(pt.clone().into());
        if matches!(
            &copied.continuous,
            Some(crate::characteristics::StaticContinuousAbility::CharacteristicDefining {
                parts,
                ..
            }) if parts
                .0
                .iter()
                .any(|part| matches!(part, crate::characteristics::DefiningPart::PowerToughness { .. }))
        ) {
            copied.continuous = None;
        }
    }
    if !exceptions.extra_triggered.is_empty() {
        let mut next_trig = copied
            .triggered
            .iter()
            .map(|t| t.id.0)
            .max()
            .map_or(0, |m| m.saturating_add(1));
        for tr in &exceptions.extra_triggered {
            let mut cloned = tr.clone();
            cloned.id = crate::program::TriggeredAbilityId(next_trig);
            next_trig = next_trig.saturating_add(1);
            copied.triggered.push(cloned);
        }
    }
    if !exceptions.extra_activated.is_empty() {
        let mut next_act = copied
            .activated
            .iter()
            .map(|a| a.id.0)
            .max()
            .map_or(0, |m| m.saturating_add(1));
        for act in &exceptions.extra_activated {
            let mut cloned = act.clone();
            cloned.id = crate::activated::ActivatedAbilityId(next_act);
            next_act = next_act.saturating_add(1);
            copied.activated.push(cloned);
        }
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
                if let StaticPart::SetPT(pair) | StaticPart::ModifyPT(pair) = part {
                    substitute_chosen_x_in_numeric_expr(&mut pair.power, resolved_x);
                    substitute_chosen_x_in_numeric_expr(&mut pair.toughness, resolved_x);
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
