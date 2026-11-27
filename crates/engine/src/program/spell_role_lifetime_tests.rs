//! Admission of live and retained target-spell controller reads.
use super::*;
use crate::test_support::creature_types;

fn spell_target() -> BodyTarget {
    BodyTarget::SingleSpell(crate::program::SpellFilter::any())
}

fn gain() -> Instruction {
    crate::program::Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::gain(
        crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
        crate::program::ValueExpr::Literal(1.into()),
    )))
}

fn discard(player: ResolvingPlayer, amount: i64) -> Instruction {
    Instruction::Discard {
        players: AffectedPlayers::One(player),
        amount: crate::program::ValueExpr::Literal(amount.into()),
    }
}

fn sacrifice(player: ResolvingPlayer, amount: i64) -> Instruction {
    Instruction::Sacrifice {
        players: AffectedPlayers::One(player),
        amount: crate::program::ValueExpr::Literal(amount.into()),
        filter: PermanentTypeFilter {
            any_types: Default::default(),
            required_types: BTreeSet::from([CardType::Creature]),
            forbidden_types: BTreeSet::new(),
            object_filter: None,
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
            left: crate::program::ValueExpr::PlayerStat(
                crate::program::PlayerStatExpr::ControllerLife,
            ),
            comparison: Comparison::Greater,
            right: crate::program::ValueExpr::Literal(0.into()),
        },
        destination,
    }
}

fn check(target: BodyTarget, instructions: Vec<Instruction>) -> Result<(), DefinitionError> {
    ExecutableBody::new(target, instructions)
        .validate(&creature_types::selected_vocabulary().unwrap())
}

#[test]
fn countered_target_spell_controller_uses_retained_controller() {
    // CR 608.2h reads the controller as the countered spell last existed.
    assert_eq!(
        check(
            spell_target(),
            vec![
                Instruction::CounterSpell,
                discard(ResolvingPlayer::TargetSpellController, 1),
            ],
        ),
        Ok(())
    );
}

#[test]
fn shared_player_roles_accept_retained_controllers() {
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
        assert_eq!(check(spell_target(), instructions), Ok(()));
    }
}

#[test]
fn zero_selection_after_countering_is_admitted() {
    for operation in [
        sacrifice(ResolvingPlayer::TargetSpellController, 0),
        discard(ResolvingPlayer::TargetSpellController, 0),
    ] {
        assert_eq!(
            check(spell_target(), vec![Instruction::CounterSpell, operation]),
            Ok(())
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
fn copy_after_attempted_countering_is_valid_for_a_remaining_spell() {
    assert_eq!(
        check(
            spell_target(),
            vec![
                Instruction::CounterSpell,
                Instruction::CopySpell,
                discard(ResolvingPlayer::TargetSpellController, 1),
            ],
        ),
        Ok(())
    );
}

#[test]
fn live_and_retained_controller_paths_join() {
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
        assert_eq!(check(spell_target(), instructions), Ok(()));
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
                    condition: &crate::program::TriggerSpec::self_cast(),
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
                condition: &crate::program::TriggerSpec::self_cast(),
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
                crate::program::Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::move_all(
                        crate::toolbox::ObjectQuery::Targets,
                        crate::toolbox::ZoneDest::Hand
                    )
                )),
                Instruction::CopyTargetToSource {
                    duration: ResolvedDuration::UntilEndOfTurn,
                    power_toughness: None,
                },
            ],
        ),
        Err(DefinitionError::InvalidInstruction)
    );
}
