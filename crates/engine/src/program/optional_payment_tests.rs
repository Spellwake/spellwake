//! Definition admission and forward-flow checks; these do not qualify runtime rules.
use super::*;
use crate::life::LifeChangeKind;
use crate::{
    characteristics::{ResolvedPartSpec, ResolvedPartsSpec},
    mana::ManaType,
    test_support::creature_types,
    token::{TokenCreation, TokenDefinition, TokenName},
};

fn optional(chooser: ResolvingPlayer) -> Instruction {
    Instruction::OptionalEffect { chooser }
}
fn offer(payer: ResolvingPlayer, paid: usize, declined: usize) -> Instruction {
    Instruction::OfferManaPayment {
        payer,
        cost: ManaCost::generic(3.into()),
        paid_destination: paid,
        declined_destination: declined,
    }
}
fn gain() -> Instruction {
    Instruction::AdjustControllerLife {
        adjustment: LifeAdjustment::Gain(1.into()),
    }
}
fn choose(slot: u16) -> Instruction {
    Instruction::ChooseCreatureType {
        into: CreatureTypeSlot::new(slot),
        excluding: BTreeSet::new(),
    }
}
fn read(slot: u16) -> Instruction {
    Instruction::DrawController {
        amount: crate::program::ValueExpr::count_battlefield(
            crate::program::ObjectFilter::any()
                .controlled()
                .of_chosen_slot(CreatureTypeSlot::new(slot)),
        ),
    }
}
fn check(target: BodyTarget, instructions: Vec<Instruction>) -> Result<(), DefinitionError> {
    ExecutableBody::new(target, instructions)
        .validate(&creature_types::selected_vocabulary().unwrap())
}
fn triggered(
    condition: TriggerSpec,
    grouping: TriggerGrouping,
    instructions: Vec<Instruction>,
) -> Program {
    let mut program = Program::artifact(None, vec![]);
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition,
        grouping,
        body: ExecutableBody::new(BodyTarget::None, instructions),
    });
    program
}

#[test]
fn optional_accepts_each_primitive_with_its_real_operand_contract() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let token = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocabulary,
    )
    .unwrap();
    let cases = [
        (
            BodyTarget::None,
            Instruction::CreateTokens {
                creations: vec![TokenCreation::one(token)],
            },
        ),
        (BodyTarget::None, gain()),
        (
            BodyTarget::SinglePlayer,
            Instruction::AdjustTargetLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            },
        ),
        (BodyTarget::None, choose(0)),
        (
            BodyTarget::SingleCreatureOrPlayer,
            Instruction::Damage { amount: 1.into() },
        ),
        (
            BodyTarget::SingleCreature,
            Instruction::CreatePreventionShield { amount: 1.into() },
        ),
        (
            BodyTarget::SingleCreature,
            Instruction::CreateDamageMultiplier { factor: 2.into() },
        ),
        (BodyTarget::SinglePermanent, Instruction::ReturnToOwnersHand),
        (
            BodyTarget::SingleSpell(crate::program::SpellFilter::any()),
            Instruction::CounterSpell,
        ),
        (
            BodyTarget::SingleSpell(crate::program::SpellFilter::copyable_instruction()),
            Instruction::CopySpell,
        ),
        (
            BodyTarget::SinglePermanent,
            Instruction::CopyTargetToSource {
                duration: ResolvedDuration::UntilEndOfTurn,
                power_toughness: None,
            },
        ),
        (
            BodyTarget::SinglePermanent,
            Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                duration: ResolvedDuration::UntilEndOfTurn,
            },
        ),
        (
            BodyTarget::SingleCreature,
            Instruction::PutPowerToughnessCounters {
                counters: PowerToughnessCounters {
                    plus_one: 1.into(),
                    minus_one: 0.into(),
                },
            },
        ),
        (
            BodyTarget::None,
            Instruction::DrawController {
                amount: crate::program::ValueExpr::Literal(1.into()),
            },
        ),
    ];
    for (target, primitive) in cases {
        assert_eq!(
            check(
                target,
                vec![optional(ResolvingPlayer::Controller), primitive.clone()]
            ),
            Ok(()),
            "optional primitive {primitive:?}"
        );
    }
}

#[test]
fn optional_rejects_missing_effect_and_each_control_wrapper() {
    assert_eq!(
        check(
            BodyTarget::None,
            vec![optional(ResolvingPlayer::Controller)]
        ),
        Err(DefinitionError::MissingOptionalEffect)
    );
    let branch = Instruction::BranchIfFalse {
        condition: ResolutionCondition {
            left: crate::program::ValueExpr::PlayerStat(
                crate::program::PlayerStatExpr::ControllerLife,
            ),
            comparison: Comparison::Equal,
            right: crate::program::ValueExpr::Literal(20.into()),
        },
        destination: 3,
    };
    for wrapper in [
        branch,
        Instruction::Jump { destination: 3 },
        optional(ResolvingPlayer::Controller),
        offer(ResolvingPlayer::Controller, 3, 3),
    ] {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![
                    optional(ResolvingPlayer::Controller),
                    wrapper.clone(),
                    gain()
                ]
            ),
            Err(DefinitionError::UnsupportedOptionalEffect),
            "wrapper {wrapper:?}"
        );
    }
}

#[test]
fn optional_preserves_the_following_primitives_validation() {
    assert_eq!(
        check(
            BodyTarget::None,
            vec![
                optional(ResolvingPlayer::Controller),
                Instruction::DrawController {
                    amount: crate::program::ValueExpr::Literal((-1).into())
                }
            ]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
    assert_eq!(
        check(
            BodyTarget::SinglePlayer,
            vec![
                optional(ResolvingPlayer::Controller),
                Instruction::ReturnToOwnersHand
            ]
        ),
        Err(DefinitionError::OperandFamily)
    );
}

#[test]
fn payment_checks_both_forward_destinations_including_body_end() {
    for (paid, declined) in [(1, 1), (1, 2), (2, 1), (2, 2)] {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![offer(ResolvingPlayer::Controller, paid, declined), gain()]
            ),
            Ok(())
        );
    }
    for destination in [0, 3, usize::MAX] {
        for (paid, declined) in [(destination, 2), (2, destination)] {
            assert_eq!(
                check(
                    BodyTarget::None,
                    vec![offer(ResolvingPlayer::Controller, paid, declined), gain()]
                ),
                Err(DefinitionError::InvalidControlFlow)
            );
        }
    }
    assert_eq!(
        check(
            BodyTarget::None,
            vec![gain(), offer(ResolvingPlayer::Controller, 1, 2)]
        ),
        Err(DefinitionError::InvalidControlFlow)
    );
    assert_eq!(
        check(
            BodyTarget::None,
            vec![gain(), offer(ResolvingPlayer::Controller, 2, 1)]
        ),
        Err(DefinitionError::InvalidControlFlow)
    );
}

#[test]
fn payment_uses_existing_cost_validity_including_explicit_zero() {
    for cost in [
        ManaCost::free(),
        ManaCost::generic(3.into()),
        ManaCost::specific(ManaType::Blue, 0.into()),
        ManaCost::specific(ManaType::Blue, 2.into()),
    ] {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![Instruction::OfferManaPayment {
                    payer: ResolvingPlayer::Controller,
                    cost,
                    paid_destination: 1,
                    declined_destination: 1,
                }]
            ),
            Ok(())
        );
    }
    for cost in [
        ManaCost::generic((-1).into()),
        ManaCost::specific(ManaType::Blue, (-1).into()),
    ] {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![Instruction::OfferManaPayment {
                    payer: ResolvingPlayer::Controller,
                    cost,
                    paid_destination: 1,
                    declined_destination: 1,
                }]
            ),
            Err(DefinitionError::InvalidResolutionPaymentCost)
        );
    }
}

#[test]
fn related_player_requires_actual_trigger_context_for_both_instruction_forms() {
    for instructions in [
        vec![optional(ResolvingPlayer::UniqueRelatedPlayer), gain()],
        vec![offer(ResolvingPlayer::UniqueRelatedPlayer, 1, 1)],
    ] {
        assert_eq!(
            check(BodyTarget::None, instructions),
            Err(DefinitionError::UnboundResolvingPlayer)
        );
    }
}

#[test]
fn each_current_condition_proves_related_participant_only_with_singular_grouping() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let conditions = [
        crate::program::TriggerSpec::self_cast(),
        crate::program::TriggerSpec::spell_cast(
            PlayerRelation::Opponent,
            crate::program::SpellFilter::any(),
        ),
        crate::program::TriggerSpec::player_action(
            PlayerRelation::AnyParticipant,
            crate::program::PlayerTriggerAction::TokenCreated,
        ),
        crate::program::TriggerSpec::life_changed(
            LifeChangeKind::Gain,
            PlayerRelation::AnyParticipant,
        ),
        crate::program::TriggerSpec::enters(crate::program::ObjectFilter::creature()),
        crate::program::TriggerSpec::leaves_battlefield(crate::program::ObjectFilter::creature()),
        crate::program::TriggerSpec::dies(crate::program::ObjectFilter::creature()),
        crate::program::TriggerSpec::becomes_tapped(crate::program::ObjectFilter::self_object()),
        crate::program::TriggerSpec::player_action(
            PlayerRelation::AnyParticipant,
            crate::program::PlayerTriggerAction::Discarded,
        ),
        crate::program::TriggerSpec::step_began(Step::Upkeep, PlayerRelation::AnyParticipant),
    ];
    for condition in conditions {
        for instructions in [
            vec![optional(ResolvingPlayer::UniqueRelatedPlayer), gain()],
            vec![offer(ResolvingPlayer::UniqueRelatedPlayer, 1, 1)],
        ] {
            for (grouping, expected) in [
                (TriggerGrouping::EachOccurrence, Ok(())),
                (TriggerGrouping::OneOrMoreByPlayer, Ok(())),
                (
                    TriggerGrouping::OneOrMoreTogether,
                    Err(DefinitionError::UnguaranteedRelatedPlayer),
                ),
            ] {
                assert_eq!(
                    triggered(condition.clone(), grouping, instructions.clone())
                        .validate(&vocabulary),
                    expected,
                    "condition {condition:?}, grouping {grouping:?}"
                );
            }
        }
    }
}

#[test]
fn controller_and_existing_trigger_scalars_keep_together_admission() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let instructions = vec![
        offer(ResolvingPlayer::Controller, 1, 1),
        Instruction::BranchIfFalse {
            condition: ResolutionCondition {
                left: crate::program::ValueExpr::event(
                    crate::program::EventValueKind::MatchedOccurrences,
                ),
                comparison: Comparison::Greater,
                right: crate::program::ValueExpr::Literal(0.into()),
            },
            destination: 4,
        },
        optional(ResolvingPlayer::Controller),
        Instruction::DrawController {
            amount: crate::program::ValueExpr::event(
                crate::program::EventValueKind::MatchedOccurrences,
            ),
        },
    ];
    assert_eq!(
        triggered(
            crate::program::TriggerSpec::dies(crate::program::ObjectFilter::creature()),
            TriggerGrouping::OneOrMoreTogether,
            instructions
        )
        .validate(&vocabulary),
        Ok(())
    );
}

#[test]
fn spell_controller_requires_single_spell_and_consumes_the_target() {
    for target in [
        BodyTarget::SingleSpell(crate::program::SpellFilter::any()),
        BodyTarget::SingleSpell(crate::program::SpellFilter::copyable_instruction()),
    ] {
        assert_eq!(
            check(
                target,
                vec![optional(ResolvingPlayer::TargetSpellController), gain()]
            ),
            Ok(())
        );
        assert_eq!(
            check(
                target,
                vec![offer(ResolvingPlayer::TargetSpellController, 1, 1)]
            ),
            Ok(())
        );
        assert_eq!(
            check(target, vec![offer(ResolvingPlayer::Controller, 1, 1)]),
            Err(DefinitionError::PhantomTarget)
        );
    }
    for target in [
        BodyTarget::None,
        BodyTarget::SinglePermanent,
        BodyTarget::SingleCreature,
        BodyTarget::SinglePlayer,
        BodyTarget::SingleCreatureOrPlayer,
    ] {
        for instructions in [
            vec![optional(ResolvingPlayer::TargetSpellController), gain()],
            vec![offer(ResolvingPlayer::TargetSpellController, 1, 1)],
        ] {
            assert_eq!(
                check(target, instructions),
                Err(DefinitionError::ResolvingPlayerTarget)
            );
        }
    }
}

#[test]
fn unreachable_new_instructions_still_validate_context_syntax_and_cost() {
    for (instruction, expected) in [
        (
            optional(ResolvingPlayer::Controller),
            DefinitionError::MissingOptionalEffect,
        ),
        (
            offer(ResolvingPlayer::UniqueRelatedPlayer, 2, 2),
            DefinitionError::UnboundResolvingPlayer,
        ),
        (
            offer(ResolvingPlayer::TargetSpellController, 2, 2),
            DefinitionError::ResolvingPlayerTarget,
        ),
        (
            offer(ResolvingPlayer::Controller, 1, 2),
            DefinitionError::InvalidControlFlow,
        ),
        (
            Instruction::OfferManaPayment {
                payer: ResolvingPlayer::Controller,
                cost: ManaCost::generic((-1).into()),
                paid_destination: 2,
                declined_destination: 2,
            },
            DefinitionError::InvalidResolutionPaymentCost,
        ),
    ] {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![Instruction::Jump { destination: 2 }, instruction]
            ),
            Err(expected)
        );
    }
    assert_eq!(
        check(
            BodyTarget::None,
            vec![
                Instruction::Jump { destination: 3 },
                optional(ResolvingPlayer::UniqueRelatedPlayer),
                gain()
            ]
        ),
        Err(DefinitionError::UnboundResolvingPlayer)
    );
    assert_eq!(
        check(
            BodyTarget::None,
            vec![
                Instruction::Jump { destination: 3 },
                optional(ResolvingPlayer::Controller),
                Instruction::Jump { destination: 3 }
            ]
        ),
        Err(DefinitionError::UnsupportedOptionalEffect)
    );
    assert_eq!(
        check(
            BodyTarget::SingleSpell(crate::program::SpellFilter::any()),
            vec![
                Instruction::Jump { destination: 2 },
                offer(ResolvingPlayer::TargetSpellController, 2, 2)
            ]
        ),
        Err(DefinitionError::PhantomTarget)
    );
}

#[test]
fn optional_skip_never_defines_a_later_local_and_retains_possible_duplicates() {
    assert_eq!(
        check(
            BodyTarget::None,
            vec![optional(ResolvingPlayer::Controller), choose(0), read(0)]
        ),
        Err(DefinitionError::UndefinedCreatureTypeSlot)
    );
    assert_eq!(
        check(
            BodyTarget::None,
            vec![optional(ResolvingPlayer::Controller), choose(0), choose(0)]
        ),
        Err(DefinitionError::DuplicateCreatureTypeSlot)
    );
    assert_eq!(
        check(
            BodyTarget::None,
            vec![optional(ResolvingPlayer::Controller), choose(0), gain()]
        ),
        Ok(())
    );
    assert_eq!(
        check(
            BodyTarget::None,
            vec![
                choose(0),
                optional(ResolvingPlayer::Controller),
                gain(),
                read(0)
            ]
        ),
        Ok(())
    );
}

#[test]
fn either_payment_successor_can_skip_assignment_but_same_slot_branch_merge_is_valid() {
    for (paid, declined) in [(1, 2), (2, 1)] {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![
                    offer(ResolvingPlayer::Controller, paid, declined),
                    choose(0),
                    read(0)
                ]
            ),
            Err(DefinitionError::UndefinedCreatureTypeSlot)
        );
        assert_eq!(
            check(
                BodyTarget::None,
                vec![
                    offer(ResolvingPlayer::Controller, paid, declined),
                    choose(0),
                    choose(0)
                ]
            ),
            Err(DefinitionError::DuplicateCreatureTypeSlot)
        );
    }
    for (paid, declined) in [(1, 3), (3, 1)] {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![
                    offer(ResolvingPlayer::Controller, paid, declined),
                    choose(0),
                    Instruction::Jump { destination: 4 },
                    choose(0),
                    read(0)
                ]
            ),
            Ok(())
        );
        assert_eq!(
            check(
                BodyTarget::None,
                vec![
                    offer(ResolvingPlayer::Controller, paid, declined),
                    choose(0),
                    Instruction::Jump { destination: 4 },
                    choose(1),
                    read(0)
                ]
            ),
            Err(DefinitionError::UndefinedCreatureTypeSlot)
        );
    }
}

#[test]
fn optional_and_either_payment_path_preserve_possible_moved_donor() {
    let copy = || Instruction::CopyTargetToSource {
        duration: ResolvedDuration::UntilEndOfTurn,
        power_toughness: None,
    };
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                optional(ResolvingPlayer::Controller),
                Instruction::ReturnToOwnersHand,
                copy()
            ]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
    for (paid, declined) in [(1, 2), (2, 1)] {
        assert_eq!(
            check(
                BodyTarget::SinglePermanent,
                vec![
                    offer(ResolvingPlayer::Controller, paid, declined),
                    Instruction::ReturnToOwnersHand,
                    copy()
                ]
            ),
            Err(DefinitionError::InvalidInstruction)
        );
    }
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                offer(ResolvingPlayer::Controller, 1, 3),
                Instruction::ReturnToOwnersHand,
                Instruction::Jump { destination: 4 },
                copy(),
                gain()
            ]
        ),
        Ok(())
    );
}
