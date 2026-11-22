//! A delayed sacrifice keeps the exact incarnation even after the source leaves.
use super::*;
use spellwake_engine::{characteristics::KeywordAbility, program::CombatTriggerKeyword};
#[test]
fn decayed_instances_stack_separately_and_keep_the_departed_incarnation() {
    let mut seeds = vec![
        battlefield(1, A),
        instant(30, A, Instruction::ReturnToOwnersHand),
    ];
    seeds[0]
        .program
        .printed_keywords
        .insert(KeywordAbility::Decayed);
    keyword_triggers::add(
        &mut seeds,
        1,
        &[CombatTriggerKeyword::Decayed, CombatTriggerKeyword::Decayed],
    );
    let ready = attackers(setup(vec![A, B], seeds, Step::Untap).state());
    let mut state = keyword_triggers::place(declared(&ready, &[(1, B)]).into_state());
    assert_eq!(state.execution().window().entries().len(), 2);
    state = round(&state).into_state();
    state = round(&state).into_state();
    let old = object(&state, 1).id();
    state = keyword_damage::cast_on(&state, 30, 1);
    assert_eq!(object(&state, 1).zone(), ObjectZone::Hand(A));
    for _ in 0..30 {
        if state.turn().unwrap().position().step == Step::EndCombat {
            break;
        }
        state = round(&state).into_state();
    }
    state = keyword_triggers::place(state);
    assert_eq!(state.turn().unwrap().position().step, Step::EndCombat);
    assert_eq!(state.execution().window().entries().len(), 2);
    state = round(&state).into_state();
    state = round(&state).into_state();
    assert_ne!(object(&state, 1).id(), old);
    assert_eq!(object(&state, 1).zone(), ObjectZone::Hand(A));
}
