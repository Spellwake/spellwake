#![allow(dead_code, unused_imports)]
//! CR 702.105, 702.112, 702.121, 702.130 and 702.149.
use super::keyword_damage::{blocks, cast_on, combat_creature};
use super::keyword_triggers::{add, place};
use super::*;
use spellwake_engine::{
    characteristics::KeywordAbility::DoubleStrike,
    life::LifeAdjustment,
    occurrence::{RulesOccurrence, RulesRecord},
    program::CombatTriggerKeyword::*,
};

fn counters(state: &GameState, id: u64) -> spellwake_contracts::RulesInteger {
    state
        .execution()
        .power_toughness_counters(object(state, id).id())
        .unwrap()
        .plus_one
}
fn lose_life(id: u64) -> SeedCard {
    let mut spell = instant(id, A, Instruction::DestroyTarget);
    spell.program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePlayer,
            vec![Instruction::AdjustTargetLife {
                adjustment: LifeAdjustment::Lose(1.into()),
            }],
        ),
    );
    spell
}
fn lower(state: &GameState, id: u64, player: SeatId) -> GameState {
    let spell = act(
        state,
        A,
        GameCommand::Cast {
            spell: object(state, id).id(),
            target: TargetRef::Player(player),
        },
    );
    round(spell.state()).into_state()
}

#[test]
fn dethrone_checks_all_life_totals_at_declaration_and_keeps_a_tied_trigger() {
    let mut seeds = vec![battlefield(1, A), lose_life(2)];
    add(&mut seeds, 1, &[Dethrone, Dethrone]);
    let mut original = setup(vec![A, B, C], seeds, Step::Untap).into_state();
    while original.turn().unwrap().position().step != Step::PrecombatMain {
        original = round(&original).into_state();
    }
    let ready = attackers(&original);
    let tied = place(declared(&ready, &[(1, B)]).into_state());
    assert_eq!(tied.execution().window().entries().len(), 2);
    let lowered = lower(&tied, 2, B);
    let one = round(&lowered);
    let two = round(one.state());
    assert_eq!(counters(two.state(), 1), 2.into());
    let lowered = lower(&original, 2, B);
    let attack = declared(&attackers(&lowered), &[(1, B)]);
    assert!(attack.state().execution().window().entries().is_empty());
    let top = declared(&attackers(&lowered), &[(1, C)]);
    assert_eq!(
        place(top.into_state()).execution().window().entries().len(),
        2
    );
}
