//! Authored selection admission only; runtime/card qualification is separate.
use super::*;
use crate::test_support::creature_types;

fn creature_filter() -> PermanentTypeFilter {
    PermanentTypeFilter {
        required_types: BTreeSet::from([CardType::Creature]),
        forbidden_types: BTreeSet::new(),
    }
}

fn sacrifice(players: AffectedPlayers, amount: InstructionAmount) -> Instruction {
    Instruction::Sacrifice {
        players,
        amount,
        filter: creature_filter(),
    }
}

fn selected_effects(players: AffectedPlayers, amount: InstructionAmount) -> [Instruction; 2] {
    [
        sacrifice(players, amount.clone()),
        Instruction::Discard { players, amount },
    ]
}

fn gain() -> Instruction {
    Instruction::AdjustControllerLife {
        adjustment: LifeAdjustment::Gain(1.into()),
    }
}

fn copy() -> Instruction {
    Instruction::CopyTargetToSource {
        duration: ResolvedDuration::UntilEndOfTurn,
        power_toughness: None,
    }
}

fn choose() -> Instruction {
    Instruction::ChooseCreatureType {
        into: CreatureTypeSlot::new(0),
        excluding: BTreeSet::new(),
    }
}

fn branch(destination: usize) -> Instruction {
    Instruction::BranchIfFalse {
        condition: ResolutionCondition {
            left: ConditionValue::ControllerLife,
            comparison: Comparison::Greater,
            right: ConditionValue::Literal(0.into()),
        },
        destination,
    }
}

fn check(target: BodyTarget, instructions: Vec<Instruction>) -> Result<(), DefinitionError> {
    ExecutableBody::new(target, instructions)
        .validate(&creature_types::selected_vocabulary().unwrap())
}

fn check_triggered(
    target: BodyTarget,
    grouping: TriggerGrouping,
    instructions: Vec<Instruction>,
) -> Result<(), DefinitionError> {
    ExecutableBody::new(target, instructions).validate_in(
        &creature_types::selected_vocabulary().unwrap(),
        BodyContext::TriggerBound {
            condition: OrdinaryCondition::SelfCast,
            grouping,
        },
    )
}

#[test]
fn targeted_selection_consumes_only_the_exact_player_capability() {
    let target_player = AffectedPlayers::One(ResolvingPlayer::TargetPlayer);
    for effect in selected_effects(target_player, InstructionAmount::Literal(1.into())) {
        assert_eq!(
            check(BodyTarget::SinglePlayer, vec![effect.clone()]),
            Ok(())
        );
        for wrong in [
            BodyTarget::None,
            BodyTarget::SinglePermanent,
            BodyTarget::SingleCreature,
            BodyTarget::SingleCreatureOrPlayer,
            BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
        ] {
            assert_eq!(
                check(wrong, vec![effect.clone()]),
                Err(DefinitionError::ResolvingPlayerTarget)
            );
        }
    }
}

#[test]
fn nontargeted_scopes_cannot_hide_a_phantom_player_target() {
    for scope in [
        AffectedPlayers::One(ResolvingPlayer::Controller),
        AffectedPlayers::EachPlayer,
    ] {
        for effect in selected_effects(scope, InstructionAmount::Literal(1.into())) {
            assert_eq!(check(BodyTarget::None, vec![effect.clone()]), Ok(()));
            assert_eq!(
                check(BodyTarget::SinglePlayer, vec![effect]),
                Err(DefinitionError::PhantomTarget)
            );
        }
    }
}

#[test]
fn spell_controller_and_player_roles_remain_distinct_capabilities() {
    let spell_controller = AffectedPlayers::One(ResolvingPlayer::TargetSpellController);
    for effect in selected_effects(spell_controller, InstructionAmount::Literal(1.into())) {
        assert_eq!(
            check(
                BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
                vec![effect.clone()]
            ),
            Ok(())
        );
        assert_eq!(
            check(BodyTarget::SinglePlayer, vec![effect]),
            Err(DefinitionError::ResolvingPlayerTarget)
        );
    }
}

#[test]
fn target_player_uses_the_shared_optional_and_payment_role_owner() {
    for effect in [
        Instruction::OptionalEffect {
            chooser: ResolvingPlayer::TargetPlayer,
        },
        Instruction::OfferManaPayment {
            payer: ResolvingPlayer::TargetPlayer,
            cost: ManaCost::free(),
            paid_destination: 1,
            declined_destination: 2,
        },
    ] {
        assert_eq!(
            check(BodyTarget::SinglePlayer, vec![effect.clone(), gain()]),
            Ok(())
        );
        assert_eq!(
            check(BodyTarget::SingleCreatureOrPlayer, vec![effect, gain()]),
            Err(DefinitionError::ResolvingPlayerTarget)
        );
    }
}

#[test]
fn related_player_selection_requires_a_singular_captured_participant() {
    let related = AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer);
    for effect in selected_effects(related, InstructionAmount::Literal(1.into())) {
        assert_eq!(
            check(BodyTarget::None, vec![effect.clone()]),
            Err(DefinitionError::UnboundResolvingPlayer)
        );
        for grouping in [
            TriggerGrouping::EachOccurrence,
            TriggerGrouping::OneOrMoreByPlayer,
        ] {
            assert_eq!(
                check_triggered(BodyTarget::None, grouping, vec![effect.clone()]),
                Ok(())
            );
        }
        assert_eq!(
            check_triggered(
                BodyTarget::None,
                TriggerGrouping::OneOrMoreTogether,
                vec![effect]
            ),
            Err(DefinitionError::UnguaranteedRelatedPlayer)
        );
    }
}

#[test]
fn selection_type_filters_admit_empty_and_disjoint_sets_but_reject_overlap() {
    for filter in [
        PermanentTypeFilter {
            required_types: BTreeSet::new(),
            forbidden_types: BTreeSet::new(),
        },
        PermanentTypeFilter {
            required_types: BTreeSet::from([CardType::Artifact, CardType::Creature]),
            forbidden_types: BTreeSet::from([CardType::Land]),
        },
        PermanentTypeFilter {
            required_types: BTreeSet::new(),
            forbidden_types: BTreeSet::from([CardType::Creature]),
        },
    ] {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![Instruction::Sacrifice {
                    players: AffectedPlayers::EachPlayer,
                    amount: InstructionAmount::Literal(1.into()),
                    filter
                }]
            ),
            Ok(())
        );
    }
    assert_eq!(
        check(
            BodyTarget::None,
            vec![Instruction::Sacrifice {
                players: AffectedPlayers::EachPlayer,
                amount: InstructionAmount::Literal(0.into()),
                filter: PermanentTypeFilter {
                    required_types: BTreeSet::from([CardType::Artifact, CardType::Creature]),
                    forbidden_types: BTreeSet::from([CardType::Creature]),
                }
            }]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
}

#[test]
fn literal_counts_admit_zero_and_huge_values_without_narrowing_but_reject_negative() {
    let huge = RulesInteger::try_from(
        "999999999999999999999999999999999999999999999999999999999999999999".to_owned(),
    )
    .unwrap();
    for amount in [
        0.into(),
        1.into(),
        RulesInteger::from_usize(usize::MAX),
        huge,
    ] {
        for effect in selected_effects(
            AffectedPlayers::EachPlayer,
            InstructionAmount::Literal(amount.clone()),
        ) {
            assert_eq!(check(BodyTarget::None, vec![effect]), Ok(()));
        }
    }
    for effect in selected_effects(
        AffectedPlayers::EachPlayer,
        InstructionAmount::Literal((-1).into()),
    ) {
        assert_eq!(
            check(BodyTarget::None, vec![effect]),
            Err(DefinitionError::InvalidInstruction)
        );
    }
}

#[test]
fn selection_amounts_keep_slot_and_captured_trigger_context_requirements() {
    let chosen = InstructionAmount::ControlledPermanentsOfChosenType {
        chosen: CreatureTypeSlot::new(0),
    };
    for effect in selected_effects(AffectedPlayers::EachPlayer, chosen) {
        assert_eq!(
            check(BodyTarget::None, vec![effect.clone()]),
            Err(DefinitionError::UndefinedCreatureTypeSlot)
        );
        assert_eq!(
            check(BodyTarget::None, vec![choose(), effect.clone()]),
            Ok(())
        );
        assert_eq!(
            check(BodyTarget::None, vec![branch(2), choose(), effect]),
            Err(DefinitionError::UndefinedCreatureTypeSlot)
        );
    }
    for effect in selected_effects(
        AffectedPlayers::EachPlayer,
        InstructionAmount::MatchedOccurrences,
    ) {
        assert_eq!(
            check(BodyTarget::None, vec![effect.clone()]),
            Err(DefinitionError::UnboundTriggerAmount)
        );
        assert_eq!(
            check_triggered(
                BodyTarget::None,
                TriggerGrouping::OneOrMoreTogether,
                vec![effect]
            ),
            Ok(())
        );
    }
}

#[test]
fn selected_effects_are_not_admitted_as_optional_single_primitives() {
    for effect in selected_effects(
        AffectedPlayers::EachPlayer,
        InstructionAmount::Literal(1.into()),
    ) {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    effect
                ]
            ),
            Err(DefinitionError::UnsupportedOptionalEffect)
        );
    }
}

#[test]
fn unreachable_selection_still_validates_roles_amounts_and_filter() {
    let invalid_effects = [
        Instruction::Discard {
            players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
            amount: InstructionAmount::Literal(1.into()),
        },
        sacrifice(
            AffectedPlayers::EachPlayer,
            InstructionAmount::Literal((-1).into()),
        ),
        sacrifice(
            AffectedPlayers::EachPlayer,
            InstructionAmount::MatchedOccurrences,
        ),
        Instruction::Sacrifice {
            players: AffectedPlayers::EachPlayer,
            amount: InstructionAmount::Literal(1.into()),
            filter: PermanentTypeFilter {
                required_types: BTreeSet::from([CardType::Creature]),
                forbidden_types: BTreeSet::from([CardType::Creature]),
            },
        },
    ];
    for (effect, error) in invalid_effects.into_iter().zip([
        DefinitionError::ResolvingPlayerTarget,
        DefinitionError::InvalidInstruction,
        DefinitionError::UnboundTriggerAmount,
        DefinitionError::InvalidInstruction,
    ]) {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![Instruction::Jump { destination: 2 }, effect]
            ),
            Err(error)
        );
    }
    assert_eq!(
        check(
            BodyTarget::SinglePlayer,
            vec![
                Instruction::Jump { destination: 2 },
                Instruction::Discard {
                    players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    amount: InstructionAmount::Literal(1.into()),
                }
            ]
        ),
        Err(DefinitionError::PhantomTarget)
    );
}

#[test]
fn possible_sacrifice_departure_rejects_straight_line_and_joined_copy_donors() {
    let positive = sacrifice(
        AffectedPlayers::One(ResolvingPlayer::Controller),
        InstructionAmount::Literal(1.into()),
    );
    assert_eq!(
        check(BodyTarget::SinglePermanent, vec![positive.clone(), copy()]),
        Err(DefinitionError::InvalidInstruction)
    );
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![branch(2), positive, copy()]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
    let nonliteral = sacrifice(
        AffectedPlayers::EachPlayer,
        InstructionAmount::ControlledPermanentsOfChosenType {
            chosen: CreatureTypeSlot::new(0),
        },
    );
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![choose(), nonliteral, copy()]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
    assert_eq!(
        check_triggered(
            BodyTarget::SinglePermanent,
            TriggerGrouping::EachOccurrence,
            vec![
                sacrifice(
                    AffectedPlayers::EachPlayer,
                    InstructionAmount::MatchedOccurrences
                ),
                copy()
            ]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
}

#[test]
fn sacrifice_transfer_does_not_leak_to_disjoint_or_unreachable_copy_paths() {
    let positive = sacrifice(
        AffectedPlayers::EachPlayer,
        InstructionAmount::Literal(1.into()),
    );
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                branch(3),
                positive.clone(),
                Instruction::Jump { destination: 4 },
                copy(),
                gain()
            ]
        ),
        Ok(())
    );
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                Instruction::Jump { destination: 2 },
                positive.clone(),
                copy()
            ]
        ),
        Ok(())
    );
    assert_eq!(
        check(BodyTarget::SinglePermanent, vec![copy(), positive]),
        Ok(())
    );
}

#[test]
fn exact_zero_sacrifice_preserves_both_false_and_true_incoming_movement_facts() {
    let zero = sacrifice(
        AffectedPlayers::EachPlayer,
        InstructionAmount::Literal(0.into()),
    );
    assert_eq!(
        check(BodyTarget::SinglePermanent, vec![zero.clone(), copy()]),
        Ok(())
    );
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![Instruction::ReturnToOwnersHand, zero.clone(), copy()]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                branch(2),
                sacrifice(
                    AffectedPlayers::EachPlayer,
                    InstructionAmount::Literal(1.into())
                ),
                zero,
                copy()
            ]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
}

#[test]
fn discard_does_not_invalidate_a_battlefield_copy_donor() {
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                Instruction::Discard {
                    players: AffectedPlayers::EachPlayer,
                    amount: InstructionAmount::Literal(3.into()),
                },
                copy()
            ]
        ),
        Ok(())
    );
}

#[test]
fn selected_body_shapes_keep_targeted_life_loss_and_each_player_trigger_continuations() {
    let targeted = AffectedPlayers::One(ResolvingPlayer::TargetPlayer);
    assert_eq!(
        check(
            BodyTarget::SinglePlayer,
            vec![
                sacrifice(targeted, InstructionAmount::Literal(1.into())),
                Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Lose(1.into()),
                }
            ]
        ),
        Ok(())
    );
    assert_eq!(
        check_triggered(
            BodyTarget::None,
            TriggerGrouping::EachOccurrence,
            vec![sacrifice(
                AffectedPlayers::EachPlayer,
                InstructionAmount::Literal(1.into())
            )]
        ),
        Ok(())
    );
}
