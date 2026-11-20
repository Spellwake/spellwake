//! Forward control instructions use the existing payload/cursor/journal owners.
use super::*;
use crate::program::{ConditionValue, ResolutionCondition};

impl EffectWindow {
    /// Exact installed trigger-count context, shared by amounts and conditions.
    pub(super) fn resolution_trigger_count(
        &self,
        controller: SeatId,
    ) -> Result<RulesInteger, EffectError> {
        self.resolution_trigger_count_at(controller, self.resolution_cursor())
    }
    pub(super) fn resolution_trigger_count_at(
        &self,
        controller: SeatId,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<RulesInteger, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::InconsistentProgram)?;
        let cursor = cursor.ok_or(EffectError::InconsistentProgram)?;
        if cursor.entry != entry.id() || entry.controller() != controller {
            return Err(EffectError::InconsistentProgram);
        }
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|value| value.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        if !matches!(entry.kind(), StackKind::TriggeredAbility { source } if source == &binding.source.object)
        {
            return Err(EffectError::InconsistentProgram);
        }
        Ok(RulesInteger::from_usize(binding.causes.len()))
    }
    fn condition_value(
        &self,
        controller: SeatId,
        value: &ConditionValue,
    ) -> Result<RulesInteger, EffectError> {
        match value {
            ConditionValue::Literal(value) => Ok(value.clone()),
            ConditionValue::ControllerLife => self
                .life
                .get(controller)
                .cloned()
                .ok_or(EffectError::InconsistentProgram),
            ConditionValue::MatchedOccurrences => self.resolution_trigger_count(controller),
        }
    }
    /// Move the sole live cursor; no instruction can jump backwards or past end.
    pub(super) fn set_instruction(&mut self, destination: usize) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let length = self
            .payloads
            .get(&cursor.entry)
            .and_then(StackPayload::instructions)
            .ok_or(EffectError::InconsistentProgram)?
            .len();
        if cursor.entry != entry.id() || destination <= cursor.instruction || destination > length {
            return Err(EffectError::InconsistentProgram);
        }
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .instruction = destination;
        Ok(())
    }
    pub(super) fn progress_control(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: &Instruction,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        let from = self
            .instruction_index()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if matches!(
            instruction,
            Instruction::BranchIfFalse { .. } | Instruction::Jump { .. }
        ) && (self.window.resolving() != Some(entry)
            || self.resolution_cursor().map(|cursor| cursor.entry) != Some(entry.id()))
        {
            return Err(EffectError::InconsistentProgram);
        }
        match instruction {
            Instruction::BranchIfFalse {
                condition,
                destination,
            } => {
                let ResolutionCondition {
                    left,
                    comparison,
                    right,
                } = condition;
                let left = self.condition_value(entry.controller(), left)?;
                let right = self.condition_value(entry.controller(), right)?;
                let result = comparison.evaluate(&left, &right);
                let next = if result {
                    from.checked_add(1)
                        .ok_or(EffectError::InconsistentProgram)?
                } else {
                    *destination
                };
                self.set_instruction(next)?;
                append
                    .records
                    .push(crate::occurrence::RulesRecord::InstructionBranch {
                        entry: entry.id(),
                        instruction: from,
                        condition: condition.clone(),
                        left,
                        right,
                        result,
                        next,
                    });
            }
            Instruction::Jump { destination } => {
                self.set_instruction(*destination)?;
                append
                    .records
                    .push(crate::occurrence::RulesRecord::InstructionJump {
                        entry: entry.id(),
                        instruction: from,
                        next: *destination,
                    });
            }
            Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::Sacrifice { .. }
            | Instruction::Discard { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::AdjustControllerLife { .. }
            | Instruction::AdjustTargetLife { .. }
            | Instruction::ChooseCreatureType { .. }
            | Instruction::Damage { .. }
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::ReturnToOwnersHand
            | Instruction::CounterSpell
            | Instruction::CopySpell
            | Instruction::CopyTargetToSource { .. }
            | Instruction::CreateContinuousEffect { .. }
            | Instruction::PutPowerToughnessCounters { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::DrawController { .. } => return Ok(false),
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::{GameCommand, GameContext, GameId, GameInput, GameState},
        mana::ManaCost,
        object_identity::CardId,
        program::{
            Comparison, CreatureTypeSlot, OrdinaryCondition, Program, TriggerGrouping,
            TriggeredAbility, TriggeredAbilityId,
        },
        test_support::creature_types,
        turn::{Step, TurnStart},
    };
    use std::{collections::BTreeSet, sync::Arc};
    const A: SeatId = SeatId(0);
    const B: SeatId = SeatId(1);
    fn paused() -> EffectWindow {
        paused_with_tail(None)
    }
    fn paused_with_tail(tail: Option<Instruction>) -> EffectWindow {
        let mut program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: crate::life::LifeAdjustment::Gain(0.into()),
                }],
            ),
        );
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::SelfCast,
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ChooseCreatureType {
                        into: CreatureTypeSlot::new(0),
                        excluding: BTreeSet::new(),
                    },
                    Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ConditionValue::MatchedOccurrences,
                            comparison: Comparison::Equal,
                            right: ConditionValue::Literal(1.into()),
                        },
                        destination: 2,
                    },
                ],
            ),
        });
        if let Some(tail) = tail {
            program.triggered[0].body.instructions.push(tail);
        }
        let seed = GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: A,
                    destination: ObjectDestination::Hand,
                    program,
                },
                SeedCard {
                    card: CardId(2),
                    owner: B,
                    destination: ObjectDestination::Battlefield { controller: B },
                    program: Program::artifact(None, vec![]),
                },
            ],
            vec![CardId(2)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            creature_types::selected_vocabulary().unwrap(),
            Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        );
        let context = GameContext::new(
            "conditional-context-unit".into(),
            seed.creature_vocabulary.rules_revision().into(),
            "literal partial".into(),
            "unit".into(),
            "unit".into(),
            seed.creature_vocabulary.id(),
            seed.name_rules.id(),
        )
        .unwrap();
        let mut game = GameState::initialize_scheduled(
            GameId(6051),
            context,
            seed,
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let source = game
            .execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        for (actor, command) in [
            (
                A,
                GameCommand::CastUntargeted {
                    spell: source,
                    payment: None,
                },
            ),
            (A, GameCommand::Pass),
            (B, GameCommand::Pass),
        ] {
            game = game
                .advance(GameInput {
                    basis: game.basis().clone(),
                    actor,
                    command,
                })
                .unwrap()
                .into_state();
        }
        assert_eq!(game.execution().instruction_index(), Some(0));
        game.execution().clone()
    }
    #[test]
    fn count_context_rejects_wrong_controller_cursor_missing_binding_and_source() {
        let state = paused();
        assert_eq!(state.resolution_trigger_count(A), Ok(1.into()));
        assert_eq!(
            state.resolution_trigger_count(B),
            Err(EffectError::InconsistentProgram)
        );
        let resolving = state.window.resolving().unwrap().id();
        let original = state.window.entries()[0].id();
        assert_ne!(original, resolving);
        let mut wrong = state.clone();
        wrong.resolution_cursor_mut().unwrap().entry = original;
        let saved = wrong.clone();
        assert_eq!(
            wrong.resolution_trigger_count(A),
            Err(EffectError::InconsistentProgram)
        );
        assert_eq!(wrong, saved);
        let mut missing = state.clone();
        let StackPayload::Instructions(payload) = missing.payloads.get_mut(&resolving).unwrap()
        else {
            panic!("instruction payload")
        };
        payload.bindings.trigger = None;
        assert_eq!(
            missing.resolution_trigger_count(A),
            Err(EffectError::InconsistentProgram)
        );
        let other = state
            .window
            .cards()
            .get(state.window.cards().current_identity(CardId(2)).unwrap())
            .unwrap()
            .clone();
        let mut foreign = state;
        let StackPayload::Instructions(payload) = foreign.payloads.get_mut(&resolving).unwrap()
        else {
            panic!("instruction payload")
        };
        payload.bindings.trigger.as_mut().unwrap().source.object = other;
        assert_eq!(
            foreign.resolution_trigger_count(A),
            Err(EffectError::InconsistentProgram)
        );
    }
    #[test]
    fn checked_control_rejects_foreign_entry_and_invalid_destinations_before_changes() {
        let state = paused();
        let foreign = state.window.entries()[0].clone();
        let actual = state.window.resolving().unwrap().clone();
        for (entry, op) in [
            (&foreign, Instruction::Jump { destination: 1 }),
            (&actual, Instruction::Jump { destination: 0 }),
            (
                &actual,
                Instruction::Jump {
                    destination: usize::MAX,
                },
            ),
        ] {
            let mut next = state.clone();
            let mut append = RulesJournal::default();
            assert!(next.progress_control(entry, &op, &mut append).is_err());
            assert_eq!(next, state);
            assert!(append.records.is_empty());
        }
    }
    #[test]
    fn exhausted_allocation_after_accepted_local_and_branch_has_no_adoptable_partial_transition() {
        use crate::token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients};
        let v = creature_types::selected_vocabulary().unwrap();
        let token = TokenDefinition::new(
            Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            TokenName::Default {
                ordered_subtypes: vec![],
            },
            &v,
        )
        .unwrap();
        let healthy = paused_with_tail(Some(Instruction::CreateTokens {
            creations: vec![TokenCreation {
                definition: token,
                amount: InstructionAmount::Literal(1.into()),
                recipients: TokenRecipients::Controller,
            }],
        }));
        let human = v.resolve_label("Human").unwrap();
        let frame = EffectDecision::CreatureType(healthy.creature_type_frame().unwrap());
        let accepted = healthy
            .resume_rule(
                healthy.revision(),
                &frame,
                A,
                DecisionReply::CreatureType(human),
                None,
            )
            .unwrap();
        assert!(accepted.append().records.iter().any(|row| matches!(
            row,
            crate::occurrence::RulesRecord::InstructionBranch {
                instruction: 1,
                result: true,
                next: 2,
                ..
            }
        )));
        let mut exhausted = healthy;
        exhausted.window.set_test_next_object_identity(u64::MAX);
        let saved = exhausted.clone();
        let frame = EffectDecision::CreatureType(exhausted.creature_type_frame().unwrap());
        let result = exhausted.resume_rule(
            exhausted.revision(),
            &frame,
            A,
            DecisionReply::CreatureType(human),
            None,
        );
        assert!(result.is_err());
        assert_eq!(exhausted, saved);
        assert!(exhausted.resolution_state().unwrap().locals().is_empty());
        assert_eq!(exhausted.instruction_index(), Some(0));
    }
}
