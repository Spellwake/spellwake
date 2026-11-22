//! No-commander games have an empty commander identity.
use super::*;

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn cmm_commander_identity_mana_taps_without_producing_when_no_commander() {
    for (name, is_land, price) in [
        ("Command Tower", true, 0),
        ("Arcane Signet", false, 2),
        ("Commander's Sphere", false, 3),
    ] {
        let game = game(name, &[], 2, 20);
        let ready = if is_land {
            play(&game)
        } else {
            let cast = advance(
                &game,
                A,
                GameCommand::CastUntargeted {
                    spell: id(&game, 1),
                    payment: payment(price),
                },
            );
            resolve(&cast)
        };
        let before = ManaType::ALL.map(|kind| ready.execution().mana(A, kind));
        let result = advance(
            &ready,
            A,
            GameCommand::ActivateMana {
                source: id(&ready, 1),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        );
        assert!(tapped(&result), "{name}");
        assert!(result.pending_decision().is_none(), "{name}");
        assert_eq!(
            ManaType::ALL.map(|kind| result.execution().mana(A, kind)),
            before,
            "{name}"
        );
        reject(
            &result,
            GameCommand::ActivateMana {
                source: id(&result, 1),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        );
    }
}
