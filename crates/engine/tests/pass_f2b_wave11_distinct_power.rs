#![forbid(unsafe_code)]
//! Distinct current-power counting and intervening-if rechecks for coven.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    characteristics::{ResolvedDuration, ResolvedPartSpec},
    effects::SeedCard,
    game::{GameCommand, GameState, NativeCandidate},
    mana::ManaPayment,
    object_identity::CardId,
    program::{BodyTarget, ObjectFilter, ObjectStatKind, Program, StatAggregate, ValueExpr},
    targets::TargetRef,
};
use std::collections::BTreeSet;
use trajectories::*;

fn distinct_powers(filter: ObjectFilter) -> ValueExpr {
    ValueExpr::aggregate(filter, ObjectStatKind::Power, StatAggregate::CountDistinct)
}

fn count_spell(id: u64, filter: ObjectFilter) -> SeedCard {
    spell(
        id,
        BodyTarget::None,
        vec![spellwake_engine::program::Instruction::AdjustLife(
            Box::new(spellwake_engine::toolbox::AdjustLifeSpec::gain(
                spellwake_engine::toolbox::PlayerSelector::Resolving(
                    spellwake_engine::program::ResolvingPlayer::Controller,
                ),
                distinct_powers(filter),
            )),
        )],
    )
}

fn creature(power: RulesInteger) -> Program {
    Program::creature(None, power, 5.into(), BTreeSet::new())
}

fn cast_untargeted(game: &GameState, id: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(game, id),
            payment: Some(ManaPayment::default()),
        },
    );
    pass_pair(cast.state())
}

fn cast_pump(game: &GameState, id: u64, target: u64) -> NativeCandidate {
    let cast = submit(
        game,
        P0,
        GameCommand::CastPaid {
            spell: object(game, id),
            target: TargetRef::Object(object(game, target)),
            payment: ManaPayment::default(),
        },
    );
    pass_pair(cast.state())
}

#[test]
fn distinct_power_counts_exact_signed_values_and_current_layered_power() {
    let huge = RulesInteger::from(i64::MAX).times(&RulesInteger::from(i64::MAX));
    let filter = ObjectFilter::creature().controlled();
    let game = world(
        vocabulary(),
        vec![
            card(1, creature((-2).into()), true),
            card(2, creature((-2).into()), true),
            card(3, creature(0.into()), true),
            card(4, creature(2.into()), true),
            card(5, creature(huge), true),
            controlled(6, P1, P1, creature(9.into())),
            card(7, Program::enchantment(None), true),
            count_spell(10, filter),
            count_spell(11, filter),
            spell(
                12,
                BodyTarget::SingleCreature,
                vec![effect(
                    vec![ResolvedPartSpec::ModifyPT(pt(2, 0))],
                    ResolvedDuration::UntilEndOfTurn,
                )],
            ),
        ],
        (1..=7).map(CardId).collect(),
    );
    let initial = cast_untargeted(&game, 10);
    assert_eq!(initial.state().execution().life_total(P0), Some(&24.into()));
    let changed = cast_pump(initial.state(), 12, 3);
    assert_eq!(
        current(changed.state(), object(changed.state(), 3))
            .power_toughness
            .unwrap()
            .derived
            .power,
        2.into()
    );
    let recounted = cast_untargeted(changed.state(), 11);
    assert_eq!(
        recounted.state().execution().life_total(P0),
        Some(&27.into())
    );
}

#[test]
fn distinct_power_omits_objects_without_power_and_counts_empty_as_zero() {
    let game = world(
        vocabulary(),
        vec![
            card(1, Program::enchantment(None), true),
            count_spell(10, ObjectFilter::any()),
        ],
        vec![CardId(1)],
    );
    let counted = cast_untargeted(&game, 10);
    assert_eq!(counted.state().execution().life_total(P0), Some(&20.into()));
}
