//! Admission of live target-spell controller reads, not positive LKI support.
use super::*;
use crate::test_support::creature_types;

fn spell_target() -> BodyTarget {
    BodyTarget::SingleSpell(SpellTargetFilter::AnySpell)
}

fn gain() -> Instruction {
    Instruction::AdjustControllerLife {
        adjustment: LifeAdjustment::Gain(1.into()),
    }
}

fn discard(player: ResolvingPlayer, amount: i64) -> Instruction {
    Instruction::Discard {
        players: AffectedPlayers::One(player),
        amount: InstructionAmount::Literal(amount.into()),
    }
}

fn sacrifice(player: ResolvingPlayer, amount: i64) -> Instruction {
    Instruction::Sacrifice {
        players: AffectedPlayers::One(player),
        amount: InstructionAmount::Literal(amount.into()),
        filter: PermanentTypeFilter {
            required_types: BTreeSet::from([CardType::Creature]),
            forbidden_types: BTreeSet::new(),
        },
    }
}

fn optional(player: ResolvingPlayer) -> Instruction {
    Instruction::OptionalEffect { chooser: player }
}

fn payment(player: ResolvingPlayer, destination: usize) -> Instruction {
    Instruction::OfferManaPayment {
        payer: player,
        cost: ManaCost::free(),
        paid_destination: destination,
        declined_destination: destination,
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

#[test]
fn countered_target_spell_controller_requires_unimplemented_lki() {
    // CR 701.6a removes the bound spell from the stack. The live-entry role
    // cannot supply CR 608.2h controller LKI for this later discard.
    assert_eq!(
        check(
            spell_target(),
            vec![
                Instruction::CounterSpell,
                discard(ResolvingPlayer::TargetSpellController, 1),
            ],
        ),
        Err(DefinitionError::InvalidInstruction)
    );
}

#[test]
fn all_shared_role_consumers_reject_after_possible_target_countering() {
    for instructions in [
        vec![
            Instruction::CounterSpell,
            optional(ResolvingPlayer::TargetSpellController),
            gain(),
        ],
        vec![
            Instruction::CounterSpell,
            payment(ResolvingPlayer::TargetSpellController, 2),
        ],
        vec![
            Instruction::CounterSpell,
            sacrifice(ResolvingPlayer::TargetSpellController, 1),
        ],
        vec![
            Instruction::CounterSpell,
            discard(ResolvingPlayer::TargetSpellController, 1),
        ],
    ] {
        assert_eq!(
            check(spell_target(), instructions),
            Err(DefinitionError::InvalidInstruction)
        );
    }
}

#[test]
fn zero_selection_amount_does_not_invent_a_departed_spell_controller() {
    for operation in [
        sacrifice(ResolvingPlayer::TargetSpellController, 0),
        discard(ResolvingPlayer::TargetSpellController, 0),
    ] {
        assert_eq!(
            check(spell_target(), vec![Instruction::CounterSpell, operation]),
            Err(DefinitionError::InvalidInstruction)
        );
    }
}

#[test]
fn role_before_countering_and_copy_without_departure_remain_admitted() {
    for instructions in [
        vec![
            optional(ResolvingPlayer::TargetSpellController),
            gain(),
            Instruction::CounterSpell,
        ],
        vec![
            payment(ResolvingPlayer::TargetSpellController, 1),
            Instruction::CounterSpell,
        ],
        vec![
            sacrifice(ResolvingPlayer::TargetSpellController, 1),
            Instruction::CounterSpell,
        ],
        vec![
            discard(ResolvingPlayer::TargetSpellController, 1),
            Instruction::CounterSpell,
        ],
        vec![
            Instruction::CopySpell,
            discard(ResolvingPlayer::TargetSpellController, 1),
        ],
    ] {
        assert_eq!(check(spell_target(), instructions), Ok(()));
    }
}

#[test]
fn an_intervening_nondeparture_does_not_clear_countering_history() {
    assert_eq!(
        check(
            spell_target(),
            vec![
                Instruction::CounterSpell,
                Instruction::CopySpell,
                discard(ResolvingPlayer::TargetSpellController, 1),
            ],
        ),
        Err(DefinitionError::InvalidInstruction)
    );
}

#[test]
fn possible_countering_at_branch_and_optional_joins_rejects_later_roles() {
    for instructions in [
        vec![
            branch(2),
            Instruction::CounterSpell,
            discard(ResolvingPlayer::TargetSpellController, 1),
        ],
        vec![
            optional(ResolvingPlayer::Controller),
            Instruction::CounterSpell,
            discard(ResolvingPlayer::TargetSpellController, 1),
        ],
        vec![
            Instruction::OfferManaPayment {
                payer: ResolvingPlayer::Controller,
                cost: ManaCost::free(),
                paid_destination: 1,
                declined_destination: 2,
            },
            Instruction::CounterSpell,
            discard(ResolvingPlayer::TargetSpellController, 1),
        ],
    ] {
        assert_eq!(
            check(spell_target(), instructions),
            Err(DefinitionError::InvalidInstruction)
        );
    }
}

#[test]
fn disjoint_or_unreachable_countering_does_not_poison_a_live_role_path() {
    assert_eq!(
        check(
            spell_target(),
            vec![
                branch(3),
                Instruction::CounterSpell,
                Instruction::Jump { destination: 4 },
                discard(ResolvingPlayer::TargetSpellController, 1),
                gain(),
            ],
        ),
        Ok(())
    );
    assert_eq!(
        check(
            spell_target(),
            vec![
                Instruction::Jump { destination: 2 },
                Instruction::CounterSpell,
                discard(ResolvingPlayer::TargetSpellController, 1),
            ],
        ),
        Ok(())
    );
}

#[test]
fn other_player_roles_and_body_locals_survive_target_countering() {
    let chosen = CreatureTypeSlot::new(0);
    for operation in [
        discard(ResolvingPlayer::Controller, 1),
        sacrifice(ResolvingPlayer::Controller, 1),
        Instruction::Discard {
            players: AffectedPlayers::EachPlayer,
            amount: InstructionAmount::Literal(1.into()),
        },
        Instruction::Sacrifice {
            players: AffectedPlayers::EachPlayer,
            amount: InstructionAmount::Literal(1.into()),
            filter: PermanentTypeFilter {
                required_types: BTreeSet::from([CardType::Creature]),
                forbidden_types: BTreeSet::new(),
            },
        },
        optional(ResolvingPlayer::Controller),
        payment(ResolvingPlayer::Controller, 3),
    ] {
        assert_eq!(
            check(
                spell_target(),
                vec![Instruction::CounterSpell, operation, gain()],
            ),
            Ok(())
        );
    }
    assert_eq!(
        check(
            spell_target(),
            vec![
                Instruction::ChooseCreatureType {
                    into: chosen,
                    excluding: BTreeSet::new(),
                },
                Instruction::CounterSpell,
                Instruction::DrawController {
                    amount: InstructionAmount::ControlledPermanentsOfChosenType { chosen },
                },
            ],
        ),
        Ok(())
    );
}

#[test]
fn permanent_donor_movement_does_not_mark_target_spell_departure() {
    assert_eq!(
        check(
            spell_target(),
            vec![
                sacrifice(ResolvingPlayer::Controller, 1),
                discard(ResolvingPlayer::TargetSpellController, 1),
            ],
        ),
        Ok(())
    );
}

#[test]
fn trigger_bound_related_player_keeps_its_context_contract_after_countering() {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let body = ExecutableBody::new(
        spell_target(),
        vec![
            Instruction::CounterSpell,
            discard(ResolvingPlayer::UniqueRelatedPlayer, 1),
        ],
    );
    for grouping in [
        TriggerGrouping::EachOccurrence,
        TriggerGrouping::OneOrMoreByPlayer,
    ] {
        assert_eq!(
            body.validate_in(
                &vocabulary,
                BodyContext::TriggerBound {
                    condition: OrdinaryCondition::SelfCast,
                    grouping,
                },
            ),
            Ok(())
        );
    }
    assert_eq!(
        body.validate_in(
            &vocabulary,
            BodyContext::TriggerBound {
                condition: OrdinaryCondition::SelfCast,
                grouping: TriggerGrouping::OneOrMoreTogether,
            },
        ),
        Err(DefinitionError::UnguaranteedRelatedPlayer)
    );
}

#[test]
fn exact_target_capability_and_unreachable_context_errors_remain_precise() {
    assert_eq!(
        check(
            spell_target(),
            vec![
                Instruction::CounterSpell,
                discard(ResolvingPlayer::TargetPlayer, 1),
            ],
        ),
        Err(DefinitionError::ResolvingPlayerTarget)
    );
    assert_eq!(
        check(
            BodyTarget::None,
            vec![
                Instruction::Jump { destination: 2 },
                discard(ResolvingPlayer::TargetSpellController, 1),
                gain(),
            ],
        ),
        Err(DefinitionError::ResolvingPlayerTarget)
    );
    assert_eq!(
        check(
            spell_target(),
            vec![
                Instruction::CounterSpell,
                discard(ResolvingPlayer::UniqueRelatedPlayer, 1),
            ],
        ),
        Err(DefinitionError::UnboundResolvingPlayer)
    );
}

#[test]
fn independent_departed_permanent_donor_guard_still_rejects_copy_lki() {
    assert_eq!(
        check(
            BodyTarget::SinglePermanent,
            vec![
                Instruction::ReturnToOwnersHand,
                Instruction::CopyTargetToSource {
                    duration: ResolvedDuration::UntilEndOfTurn,
                    power_toughness: None,
                },
            ],
        ),
        Err(DefinitionError::InvalidInstruction)
    );
}
