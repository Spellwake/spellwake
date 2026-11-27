//! Authored selection admission only; runtime/card qualification is separate.
use super::*;
use crate::test_support::creature_types;

fn creature_filter() -> PermanentTypeFilter {
    PermanentTypeFilter {
        any_types: Default::default(),
        required_types: BTreeSet::from([CardType::Creature]),
        forbidden_types: BTreeSet::new(),
        object_filter: None,
    }
}

fn sacrifice(players: AffectedPlayers, amount: ValueExpr) -> Instruction {
    Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::sacrifice(
        players,
        amount,
        creature_filter(),
    )))
}

fn selected_effects(players: AffectedPlayers, amount: ValueExpr) -> [Instruction; 2] {
    [
        sacrifice(players, amount.clone()),
        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::discard(
            players, amount,
        ))),
    ]
}

fn gain() -> Instruction {
    crate::program::Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::gain(
        crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
        crate::program::ValueExpr::Literal(1.into()),
    )))
}

fn copy() -> Instruction {
    Instruction::ApplyContinuous(Box::new(crate::toolbox::ApplyContinuousSpec::become_copy(
        crate::toolbox::ObjectQuery::Source,
        crate::toolbox::ObjectQuery::Targets,
        ResolvedDuration::UntilEndOfTurn,
        crate::program::CopyExceptions {
            override_pt: None,
            ..Default::default()
        },
    )))
}

fn choose() -> Instruction {
    Instruction::PromptChoice(Box::new(
        crate::toolbox::PromptChoiceSpec::choose_creature_type(
            CreatureTypeSlot::new(0),
            BTreeSet::new(),
        ),
    ))
}

fn branch(destination: usize) -> Instruction {
    Instruction::BranchCondition {
        condition: Box::new(crate::toolbox::ConditionExpr::Resolution(Box::new(
            ResolutionCondition {
                left: crate::program::ValueExpr::PlayerStat(
                    crate::program::PlayerStatExpr::ControllerLife,
                ),
                comparison: Comparison::Greater,
                right: crate::program::ValueExpr::Literal(0.into()),
            },
        ))),
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
            condition: &crate::program::TriggerSpec::self_cast(),
            grouping,
        },
    )
}

#[test]
fn targeted_selection_consumes_only_the_exact_player_capability() {
    let target_player = AffectedPlayers::One(ResolvingPlayer::TargetPlayer);
    for effect in selected_effects(target_player, crate::program::ValueExpr::Literal(1.into())) {
        assert_eq!(
            check(BodyTarget::SINGLE_PLAYER, vec![effect.clone()]),
            Ok(())
        );
        for wrong in [
            BodyTarget::None,
            BodyTarget::SINGLE_PERMANENT,
            BodyTarget::SINGLE_CREATURE,
            BodyTarget::SINGLE_CREATURE_OR_PLAYER,
            BodyTarget::single_spell(crate::program::SpellFilter::any()),
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
        for effect in selected_effects(scope, crate::program::ValueExpr::Literal(1.into())) {
            assert_eq!(check(BodyTarget::None, vec![effect.clone()]), Ok(()));
            assert_eq!(
                check(BodyTarget::SINGLE_PLAYER, vec![effect]),
                Err(DefinitionError::PhantomTarget)
            );
        }
    }
}

#[test]
fn spell_controller_and_player_roles_remain_distinct_capabilities() {
    let spell_controller = AffectedPlayers::One(ResolvingPlayer::TargetSpellController);
    for effect in selected_effects(
        spell_controller,
        crate::program::ValueExpr::Literal(1.into()),
    ) {
        assert_eq!(
            check(
                BodyTarget::single_spell(crate::program::SpellFilter::any()),
                vec![effect.clone()]
            ),
            Ok(())
        );
        assert_eq!(
            check(BodyTarget::SINGLE_PLAYER, vec![effect]),
            Err(DefinitionError::ResolvingPlayerTarget)
        );
    }
}

#[test]
fn target_player_uses_the_shared_optional_and_payment_role_owner() {
    for effect in [
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::TargetPlayer,
            instructions: 1,
        },
        Instruction::PromptChoice(Box::new(
            crate::toolbox::PromptChoiceSpec::offer_dynamic_mana_payment(
                ResolvingPlayer::TargetPlayer,
                ManaCost::free(),
                crate::program::ValueExpr::Literal(0.into()),
                1,
                2,
            ),
        )),
    ] {
        assert_eq!(
            check(BodyTarget::SINGLE_PLAYER, vec![effect.clone(), gain()]),
            Ok(())
        );
        assert_eq!(
            check(BodyTarget::SINGLE_CREATURE_OR_PLAYER, vec![effect, gain()]),
            Err(DefinitionError::ResolvingPlayerTarget)
        );
    }
}

#[test]
fn related_player_selection_requires_a_singular_captured_participant() {
    let related = AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer);
    for effect in selected_effects(related, crate::program::ValueExpr::Literal(1.into())) {
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
            any_types: Default::default(),
            required_types: BTreeSet::new(),
            forbidden_types: BTreeSet::new(),
            object_filter: None,
        },
        PermanentTypeFilter {
            any_types: Default::default(),
            required_types: BTreeSet::from([CardType::Artifact, CardType::Creature]),
            forbidden_types: BTreeSet::from([CardType::Land]),
            object_filter: None,
        },
        PermanentTypeFilter {
            any_types: Default::default(),
            required_types: BTreeSet::new(),
            forbidden_types: BTreeSet::from([CardType::Creature]),
            object_filter: None,
        },
    ] {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::sacrifice(
                        AffectedPlayers::EachPlayer,
                        crate::program::ValueExpr::Literal(1.into()),
                        filter,
                    ),
                ))]
            ),
            Ok(())
        );
    }
    assert_eq!(
        check(
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                crate::toolbox::ZoneOpSpec::sacrifice(
                    AffectedPlayers::EachPlayer,
                    crate::program::ValueExpr::Literal(0.into()),
                    PermanentTypeFilter {
                        any_types: Default::default(),
                        required_types: BTreeSet::from([CardType::Artifact, CardType::Creature]),
                        forbidden_types: BTreeSet::from([CardType::Creature]),
                        object_filter: None,
                    },
                ),
            ))]
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
            crate::program::ValueExpr::Literal(amount.clone()),
        ) {
            assert_eq!(check(BodyTarget::None, vec![effect]), Ok(()));
        }
    }
    for effect in selected_effects(
        AffectedPlayers::EachPlayer,
        crate::program::ValueExpr::Literal((-1).into()),
    ) {
        assert_eq!(
            check(BodyTarget::None, vec![effect]),
            Err(DefinitionError::InvalidInstruction)
        );
    }
}

#[test]
fn selection_amounts_keep_slot_and_captured_trigger_context_requirements() {
    let chosen = crate::program::ValueExpr::count_battlefield(
        crate::program::ObjectFilter::any()
            .controlled()
            .of_chosen_slot(CreatureTypeSlot::new(0)),
    );
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
        crate::program::ValueExpr::event(crate::program::EventValueKind::MatchedOccurrences),
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
        crate::program::ValueExpr::Literal(1.into()),
    ) {
        assert_eq!(
            check(
                BodyTarget::None,
                vec![
                    Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 1,
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
        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::discard(
            AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
            crate::program::ValueExpr::Literal(1.into()),
        ))),
        sacrifice(
            AffectedPlayers::EachPlayer,
            crate::program::ValueExpr::Literal((-1).into()),
        ),
        sacrifice(
            AffectedPlayers::EachPlayer,
            crate::program::ValueExpr::event(crate::program::EventValueKind::MatchedOccurrences),
        ),
        Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::sacrifice(
            AffectedPlayers::EachPlayer,
            crate::program::ValueExpr::Literal(1.into()),
            PermanentTypeFilter {
                any_types: Default::default(),
                required_types: BTreeSet::from([CardType::Creature]),
                forbidden_types: BTreeSet::from([CardType::Creature]),
                object_filter: None,
            },
        ))),
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
            BodyTarget::SINGLE_PLAYER,
            vec![
                Instruction::Jump { destination: 2 },
                Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::discard(
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    crate::program::ValueExpr::Literal(1.into()),
                ))),
            ]
        ),
        Err(DefinitionError::PhantomTarget)
    );
}

#[test]
fn possible_sacrifice_departure_rejects_straight_line_and_joined_copy_donors() {
    let positive = sacrifice(
        AffectedPlayers::One(ResolvingPlayer::Controller),
        crate::program::ValueExpr::Literal(1.into()),
    );
    assert_eq!(
        check(BodyTarget::SINGLE_PERMANENT, vec![positive.clone(), copy()]),
        Err(DefinitionError::InvalidInstruction)
    );
    assert_eq!(
        check(
            BodyTarget::SINGLE_PERMANENT,
            vec![branch(2), positive, copy()]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
    let nonliteral = sacrifice(
        AffectedPlayers::EachPlayer,
        crate::program::ValueExpr::count_battlefield(
            crate::program::ObjectFilter::any()
                .controlled()
                .of_chosen_slot(CreatureTypeSlot::new(0)),
        ),
    );
    assert_eq!(
        check(
            BodyTarget::SINGLE_PERMANENT,
            vec![choose(), nonliteral, copy()]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
    assert_eq!(
        check_triggered(
            BodyTarget::SINGLE_PERMANENT,
            TriggerGrouping::EachOccurrence,
            vec![
                sacrifice(
                    AffectedPlayers::EachPlayer,
                    crate::program::ValueExpr::event(
                        crate::program::EventValueKind::MatchedOccurrences
                    )
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
        crate::program::ValueExpr::Literal(1.into()),
    );
    assert_eq!(
        check(
            BodyTarget::SINGLE_PERMANENT,
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
            BodyTarget::SINGLE_PERMANENT,
            vec![
                Instruction::Jump { destination: 2 },
                positive.clone(),
                copy()
            ]
        ),
        Ok(())
    );
    assert_eq!(
        check(BodyTarget::SINGLE_PERMANENT, vec![copy(), positive]),
        Ok(())
    );
}

#[test]
fn exact_zero_sacrifice_preserves_both_false_and_true_incoming_movement_facts() {
    let zero = sacrifice(
        AffectedPlayers::EachPlayer,
        crate::program::ValueExpr::Literal(0.into()),
    );
    assert_eq!(
        check(BodyTarget::SINGLE_PERMANENT, vec![zero.clone(), copy()]),
        Ok(())
    );
    assert_eq!(
        check(
            BodyTarget::SINGLE_PERMANENT,
            vec![
                crate::program::Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::move_all(
                        crate::toolbox::ObjectQuery::Targets,
                        crate::toolbox::ZoneDest::Hand
                    )
                )),
                zero.clone(),
                copy()
            ]
        ),
        Err(DefinitionError::InvalidInstruction)
    );
    assert_eq!(
        check(
            BodyTarget::SINGLE_PERMANENT,
            vec![
                branch(2),
                sacrifice(
                    AffectedPlayers::EachPlayer,
                    crate::program::ValueExpr::Literal(1.into())
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
            BodyTarget::SINGLE_PERMANENT,
            vec![
                Instruction::ZoneOp(Box::new(crate::toolbox::ZoneOpSpec::discard(
                    AffectedPlayers::EachPlayer,
                    crate::program::ValueExpr::Literal(3.into()),
                ))),
                copy()
            ]
        ),
        Ok(())
    );
}
