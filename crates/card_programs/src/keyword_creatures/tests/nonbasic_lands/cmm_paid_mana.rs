//! Dated CMM paid-tap mana cards through the real scheduled game.
use super::*;

fn allocate(kind: ManaType, amount: i64) -> ManaPayment {
    ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type: kind,
            amount: amount.into(),
        }],
    }
}
fn paid(s: &GameState, kind: ManaType, amount: i64) -> GameCommand {
    GameCommand::ActivateManaWithPayment {
        source: id(s, 1),
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        payment: allocate(kind, amount),
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_five_paid_tap_mana_cards_use_land_mana_and_reject_illegal_payments() {
    use ManaType::{Blue, Green, Red, White};
    for (name, land, output) in [
        ("Azorius Signet", false, [White, Blue]),
        ("Boros Signet", false, [Red, White]),
        ("Izzet Signet", false, [Blue, Red]),
        ("Skycloud Expanse", true, [White, Blue]),
        ("Sungrass Prairie", true, [Green, White]),
    ] {
        let (inventory, _, registry) = current_catalogue();
        let row = inventory
            .cards
            .iter()
            .find(|r| r.name == name && r.required_by_base_formats)
            .unwrap();
        assert!(
            row.faces[0]
                .oracle_text
                .as_deref()
                .unwrap()
                .starts_with("{1}, {T}: Add "),
            "{name}"
        );
        let oracle = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        assert!(
            registry
                .seed(&oracle, CardId(30), A, ObjectDestination::Hand)
                .is_ok(),
            "{name}"
        );
        let initial = game(name, &[("Forest", A)], 2, 20);
        let ready = if land {
            play(&initial)
        } else {
            let cast = advance(
                &initial,
                A,
                GameCommand::CastUntargeted {
                    spell: id(&initial, 1),
                    payment: payment(2),
                },
            );
            resolve(&cast)
        };
        assert!(!tapped(&ready), "{name}");
        let saved = ready.clone();
        assert!(
            ready
                .advance(GameInput {
                    basis: ready.basis().clone(),
                    actor: A,
                    command: GameCommand::ActivateMana {
                        source: id(&ready, 1),
                        ability: ManaAbilityRef::Printed(ManaAbilityId(0))
                    },
                })
                .is_err()
        );
        assert!(
            ready
                .advance(GameInput {
                    basis: ready.basis().clone(),
                    actor: A,
                    command: paid(&ready, Blue, 1)
                })
                .is_err()
        );
        assert!(
            ready
                .advance(GameInput {
                    basis: ready.basis().clone(),
                    actor: A,
                    command: paid(&ready, ManaType::Colourless, 2)
                })
                .is_err()
        );
        assert_eq!(ready, saved, "invalid payment changed {name}");
        let forest = advance(
            &ready,
            A,
            GameCommand::ActivateMana {
                source: id(&ready, 10),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
            },
        );
        assert_eq!(forest.execution().mana(A, Green), 1.into());
        let activated = advance(&forest, A, paid(&forest, Green, 1));
        assert!(tapped(&activated), "{name}");
        for color in ManaType::ALL {
            let expected = forest
                .execution()
                .mana(A, color)
                .minus(&i64::from(color == Green).into())
                .plus(&i64::from(output.contains(&color)).into());
            assert_eq!(
                activated.execution().mana(A, color),
                expected,
                "{name}: {color:?}"
            );
        }
        reject(&activated, paid(&activated, ManaType::Colourless, 1));
        let opponent_priority = advance(&ready, A, GameCommand::Pass);
        let snapshot = opponent_priority.clone();
        assert!(
            opponent_priority
                .advance(GameInput {
                    basis: opponent_priority.basis().clone(),
                    actor: B,
                    command: paid(&opponent_priority, ManaType::Colourless, 1)
                })
                .is_err()
        );
        assert_eq!(opponent_priority, snapshot);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_signet_converts_land_mana_to_pay_another_spell_once() {
    let (_, vocabulary, registry) = current_catalogue();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        format!(
            "keyword-gameplay-registry{}",
            registry.coverage.registry_version
        ),
        "paid-mana-casting".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let cards = vec![
        seed(
            "Azorius Signet",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        seed(
            "Forest",
            10,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        seed("Boros Signet", 20, A, ObjectDestination::Hand),
        seed("Forest", 2, A, ObjectDestination::LibraryBottom),
        seed("Forest", 3, B, ObjectDestination::LibraryBottom),
    ];
    let initial = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![CardId(1), CardId(10)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let game = GameState::initialize_scheduled(
        GameId(911),
        context,
        initial,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let forest = advance(
        &game,
        A,
        GameCommand::ActivateMana {
            source: id(&game, 10),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    );
    let filtered = advance(&forest, A, paid(&forest, ManaType::Green, 1));
    assert_eq!(filtered.execution().mana(A, ManaType::Green), 0.into());
    let payment = ManaPayment {
        from_lots: Default::default(),
        spend: vec![
            ManaSpend {
                mana_type: ManaType::White,
                amount: 1.into(),
            },
            ManaSpend {
                mana_type: ManaType::Blue,
                amount: 1.into(),
            },
        ],
    };
    let cast = advance(
        &filtered,
        A,
        GameCommand::CastUntargeted {
            spell: id(&filtered, 20),
            payment: Some(payment),
        },
    );
    assert_eq!(cast.execution().mana(A, ManaType::White), 0.into());
    assert_eq!(cast.execution().mana(A, ManaType::Blue), 0.into());
    assert_eq!(
        cast.execution()
            .window()
            .cards()
            .get(id(&cast, 20))
            .unwrap()
            .zone(),
        ObjectZone::Stack
    );
    let resolved = resolve(&cast);
    assert_eq!(
        resolved
            .execution()
            .window()
            .cards()
            .get(id(&resolved, 20))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
}
