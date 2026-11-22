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
fn afflict_remembers_the_defending_player_once_per_instance_across_removal() {
    let mut seeds = vec![
        battlefield(1, A),
        battlefield(3, B),
        battlefield(4, B),
        instant(5, A, Instruction::ReturnToOwnersHand),
    ];
    add(&mut seeds, 1, &[Afflict(2.into()), Afflict(3.into())]);
    let ready = attackers(setup(vec![A, B, C], seeds, Step::Untap).state());
    let attack = declared(&ready, &[(1, B)]);
    let blocking = round(attack.state());
    let state = place(blocked(blocking.state(), &[(3, 1), (4, 1)]).into_state());
    assert_eq!(state.execution().window().entries().len(), 2);
    let departed = cast_on(&state, 5, 1);
    let first = round(&departed);
    let second = round(first.state());
    for (seat, life) in [(A, 20), (B, 15), (C, 20)] {
        assert_eq!(
            second.state().execution().life_total(seat),
            Some(&life.into())
        );
    }
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

#[test]
fn training_looks_only_at_other_declared_attackers_and_does_not_recheck_power() {
    let mut seeds = vec![
        combat_creature(1, A, 2, 4, &[]),
        combat_creature(2, A, 2, 4, &[]),
        combat_creature(3, A, 3, 4, &[]),
        instant(4, A, Instruction::ReturnToOwnersHand),
    ];
    add(&mut seeds, 1, &[Training, Training]);
    let ready = attackers(setup(vec![A, B, C], seeds, Step::Untap).state());
    let equal = declared(&ready, &[(1, B), (2, B)]);
    assert!(equal.state().execution().window().entries().is_empty());
    let stronger = place(declared(&ready, &[(1, B), (2, B), (3, C)]).into_state());
    assert_eq!(stronger.execution().window().entries().len(), 2);
    let left = cast_on(&stronger, 4, 3);
    let first = round(&left);
    let second = round(first.state());
    assert_eq!(counters(second.state(), 1), 2.into());
}

#[test]
fn melee_counts_distinct_attacked_opponents_even_when_their_only_attacker_leaves() {
    let mut seeds = vec![
        battlefield(1, A),
        battlefield(2, A),
        battlefield(3, A),
        instant(4, A, Instruction::ReturnToOwnersHand),
    ];
    add(&mut seeds, 1, &[Melee, Melee]);
    let ready = attackers(setup(vec![A, B, C], seeds, Step::Untap).state());
    let state = place(declared(&ready, &[(1, B), (2, B), (3, C)]).into_state());
    assert_eq!(state.execution().window().entries().len(), 2);
    let departed = cast_on(&state, 4, 3);
    let first = round(&departed);
    let second = round(first.state());
    let stats = second
        .state()
        .execution()
        .creature(object(second.state(), 1).id())
        .unwrap()
        .unwrap();
    assert_eq!((stats.power, stats.toughness), (6.into(), 8.into()));
    assert_eq!(counters(second.state(), 1), 0.into());
}

#[test]
fn renown_checks_both_times_and_only_the_first_resolving_instance_makes_it_renowned() {
    for amount in [0i64, 2] {
        let mut seeds = vec![
            combat_creature(1, A, 2, 5, &[DoubleStrike]),
            instant(2, A, Instruction::ReturnToOwnersHand),
        ];
        add(
            &mut seeds,
            1,
            &[Renown(amount.into()), Renown(amount.into())],
        );
        let pending = place(round(&blocks(seeds, &[])).into_state());
        let id = object(&pending, 1).id();
        assert_eq!(pending.execution().window().entries().len(), 2);
        assert!(pending.execution().renowned_objects().is_empty());
        let first = round(&pending);
        assert!(first.state().execution().renowned_objects().contains(&id));
        assert!(first.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|row|matches!(row.value,RulesOccurrence::BecameRenowned{object} if object==id)))));
        let second = round(first.state());
        assert_eq!(counters(second.state(), 1), amount.into());
        let hit_again = round(second.state());
        assert!(hit_again.state().execution().window().entries().is_empty());
        assert_eq!(
            hit_again.state().execution().life_total(B),
            Some(&(16 - amount).into())
        );
        let left = cast_on(hit_again.state(), 2, 1);
        assert!(left.execution().renowned_objects().is_empty());
        assert_ne!(object(&left, 1).id(), id);
        let left_first = cast_on(&pending, 2, 1);
        let first = round(&left_first);
        let second = round(first.state());
        assert!(second.state().execution().renowned_objects().is_empty());
    }
}
