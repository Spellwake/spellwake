//! Dated hybrid filter land payment and output bundle games.
use super::*;

#[test]
#[ignore = "requires dated Oracle catalogue and creature vocabulary"]
fn cmm_hybrid_filter_lands_pay_either_color_and_choose_each_bundle() {
    use ManaType::{Blue, Red, White};
    for (name, first_land, second_land, first, second) in [
        ("Cascade Bluffs", "Island", "Mountain", Blue, Red),
        ("Mystic Gate", "Plains", "Island", White, Blue),
        ("Rugged Prairie", "Mountain", "Plains", Red, White),
    ] {
        let ready = play(&game(name, &[(first_land, A), (second_land, A)], 2, 20));
        assert!(!tapped(&ready));
        let printed = ManaAbilityRef::Printed(ManaAbilityId(1));
        let command =
            |state: &GameState,
             payment: ManaType,
             bundle: BTreeMap<ManaType, spellwake_contracts::RulesInteger>| {
                GameCommand::ActivateHybridMana {
                    source: id(state, 1),
                    ability: printed,
                    payment: ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![ManaSpend {
                            mana_type: payment,
                            amount: 1.into(),
                        }],
                    },
                    bundle,
                }
            };
        let paid_colorless = command(
            &ready,
            ManaType::Colourless,
            BTreeMap::from([(first, 2.into())]),
        );
        reject(&ready, paid_colorless);
        let normal = advance(
            &ready,
            A,
            production(&ready, ManaAbilityRef::Printed(ManaAbilityId(0))),
        );
        assert_eq!(normal.execution().mana(A, ManaType::Colourless), 5.into());
        assert!(tapped(&normal));
        for (land, kind) in [(10, first), (11, second)] {
            let basic = match kind {
                White => BasicLandType::Plains,
                Blue => BasicLandType::Island,
                Red => BasicLandType::Mountain,
                _ => unreachable!(),
            };
            let funded = advance(
                &ready,
                A,
                GameCommand::ActivateMana {
                    source: id(&ready, land),
                    ability: ManaAbilityRef::BasicLand(basic),
                },
            );
            let saved = funded.clone();
            for wrong in [
                BTreeMap::from([(first, 1.into())]),
                BTreeMap::from([(ManaType::Green, 2.into())]),
                BTreeMap::from([(first, 3.into())]),
            ] {
                reject(&funded, command(&funded, kind, wrong));
            }
            assert_eq!(funded, saved, "{name}: bad bundle mutated state");
            for (a, b) in [(2, 0), (1, 1), (0, 2)] {
                let mut bundle = BTreeMap::new();
                if a > 0 {
                    bundle.insert(first, a.into());
                }
                if b > 0 {
                    bundle.insert(second, b.into());
                }
                let result = advance(&funded, A, command(&funded, kind, bundle));
                assert_eq!(result.execution().mana(A, first), a.into(), "{name}");
                assert_eq!(result.execution().mana(A, second), b.into(), "{name}");
                assert!(tapped(&result));
                reject(
                    &result,
                    command(&result, kind, BTreeMap::from([(first, 2.into())])),
                );
            }
        }
    }
}
