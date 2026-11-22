//! Source-qualified current-subtype tests for the original Urza lands.
use super::*;

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn cmm_urza_lands_check_both_controlled_land_subtype_pairs() {
    let ability = ManaAbilityRef::Printed(ManaAbilityId(0));
    for (name, first, second, amount) in [
        ("Urza's Mine", "Urza's Power Plant", "Urza's Tower", 2),
        ("Urza's Power Plant", "Urza's Mine", "Urza's Tower", 2),
        ("Urza's Tower", "Urza's Mine", "Urza's Power Plant", 3),
    ] {
        for (extras, expected) in [
            (vec![], 1),
            (vec![(first, A)], 1),
            (vec![(first, A), (second, B)], 1),
            (vec![(first, A), (second, A)], amount),
        ] {
            let state = play(&game(name, &extras, 2, 20));
            assert!(!tapped(&state), "{name}");
            let before = state.execution().mana(A, ManaType::Colourless);
            let result = advance(&state, A, production(&state, ability));
            assert!(tapped(&result), "{name}");
            assert_eq!(
                result.execution().mana(A, ManaType::Colourless),
                before.plus(&expected.into()),
                "{name}"
            );
            reject(&result, production(&result, ability));
        }
    }
}
