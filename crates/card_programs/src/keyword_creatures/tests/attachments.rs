use super::*;
use spellwake_engine::{
    activated::ActivatedAbilityId, object_identity::ObjectId, targets::TargetRef,
};

#[test]
#[ignore = "requires the dated saved metadata/gzip and source-qualified vocabulary"]
fn holy_strength_and_bonesplitter_apply_to_the_selected_raging_goblin() {
    let vocabulary =
        vocabulary::load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap();
    let inventory = spellwake_content::snapshot::load(
        &input("SPELLWAKE_CATALOGUE_METADATA"),
        &input("SPELLWAKE_CATALOGUE_INPUT"),
    )
    .unwrap();
    let registry = Registry::from_keyword_gameplay_inventory(&inventory, &vocabulary).unwrap();
    let (a, b) = (SeatId(0), SeatId(1));
    let seed_card = |n, name, owner, zone| {
        let row = inventory.cards.iter().find(|c| c.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry.seed(&id, CardId(n), owner, zone).unwrap()
    };
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        KEYWORD_GAMEPLAY_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "keyword-gameplay-registry13".into(),
        "named-attachments".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let plains = spellwake_engine::land::BasicLandType::Plains;
    let seed = GameSeed::with_uniform_life(
        vec![a, b],
        a,
        vec![
            seed_card(
                1,
                "Raging Goblin",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            seed_card(2, "Holy Strength", a, ObjectDestination::Hand),
            seed_card(3, "Bonesplitter", a, ObjectDestination::Hand),
            seed_card(4, "Raging Goblin", a, ObjectDestination::LibraryBottom),
            seed_card(5, "Raging Goblin", b, ObjectDestination::LibraryBottom),
            seed_card(6, "Darksteel Garrison", a, ObjectDestination::Hand),
            seed_card(11, "Rabbit Battery", a, ObjectDestination::Hand),
            seed_card(8, "Skinwing", a, ObjectDestination::Hand),
            seed_card(9, "Mirran Bardiche", a, ObjectDestination::Hand),
            seed_card(10, "Monk's Fist", a, ObjectDestination::Hand),
            SeedCard {
                card: CardId(7),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: Program::land(BTreeSet::from([plains.land_type()]), vec![]),
            },
        ],
        vec![CardId(1), CardId(7)],
        vec![
            FloatingMana {
                player: a,
                mana_type: ManaType::Red,
                amount: 3.into(),
            },
            FloatingMana {
                player: a,
                mana_type: ManaType::White,
                amount: 19.into(),
            },
        ],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary.clone(),
        names,
    );
    let mut state = GameState::initialize_scheduled(
        GameId(13),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let object = |s: &GameState, n| -> ObjectId {
        s.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    let goblin = object(&state, 1);
    let pay = |amount: i64| {
        Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::White,
                amount: amount.into(),
            }],
        })
    };
    let resolve = |s: &GameState| advance(&advance(s, a, GameCommand::Pass), b, GameCommand::Pass);
    state = advance(
        &state,
        a,
        GameCommand::Cast {
            spell: object(&state, 2),
            target: TargetRef::Object(goblin),
        },
    );
    state = resolve(&state);
    assert_eq!(
        state.execution().attached_to(object(&state, 2)),
        Some(TargetRef::Object(goblin))
    );
    state = advance(
        &state,
        a,
        GameCommand::CastUntargeted {
            spell: object(&state, 3),
            payment: pay(1),
        },
    );
    state = resolve(&state);
    assert_eq!(state.execution().attached_to(object(&state, 3)), None);
    state = advance(
        &state,
        a,
        GameCommand::ActivateAbility {
            source: object(&state, 3),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(goblin),
            payment: pay(1),
        },
    );
    state = resolve(&state);
    assert_eq!(
        state.execution().attached_to(object(&state, 3)),
        Some(TargetRef::Object(goblin))
    );
    let pt = state
        .execution()
        .power_toughness(goblin)
        .unwrap()
        .unwrap()
        .derived;
    assert_eq!((pt.power, pt.toughness), (4.into(), 3.into()));
    state = advance(
        &state,
        a,
        GameCommand::CastUntargeted {
            spell: object(&state, 6),
            payment: pay(2),
        },
    );
    state = resolve(&state);
    let land = object(&state, 7);
    let fortify = |target| GameCommand::ActivateAbility {
        source: object(&state, 6),
        ability: ActivatedAbilityId(1),
        target: TargetRef::Object(target),
        payment: pay(3),
    };
    assert!(
        state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: a,
                command: fortify(goblin)
            })
            .is_err()
    );
    state = advance(&state, a, fortify(land));
    state = resolve(&state);
    assert_eq!(state.execution().mana(a, ManaType::White), 11.into());
    assert!(
        state
            .execution()
            .current_characteristics(land)
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Indestructible)
    );
    state = advance(
        &state,
        a,
        GameCommand::ActivateMana {
            source: land,
            ability: spellwake_engine::mana::ManaAbilityRef::BasicLand(plains),
        },
    );
    assert_eq!(state.execution().mana(a, ManaType::White), 12.into());
    if let Some(decision) = state.pending_decision() {
        state = advance(
            &state,
            decision.chooser(),
            GameCommand::Reply {
                decision: decision.id(),
                choice: spellwake_engine::game::DecisionReply::TriggerTarget(TargetRef::Object(
                    goblin,
                )),
            },
        );
    }
    assert_eq!(state.execution().window().entries().len(), 1);
    state = resolve(&state);
    let pt = state
        .execution()
        .power_toughness(goblin)
        .unwrap()
        .unwrap()
        .derived;
    assert_eq!((pt.power, pt.toughness), (5.into(), 4.into()));

    use spellwake_engine::characteristics::Color;
    for (card, price, name, color, subtypes, pt, ability) in [
        (
            8,
            4,
            "Phyrexian Germ Token",
            Some(Color::Black),
            vec!["Phyrexian", "Germ"],
            (2, 2),
            Some(KeywordAbility::Flying),
        ),
        (
            9,
            5,
            "Rebel Token",
            Some(Color::Red),
            vec!["Rebel"],
            (4, 3),
            Some(KeywordAbility::Vigilance),
        ),
        (
            10,
            2,
            "Hero Token",
            None,
            vec!["Hero", "Monk"],
            (2, 1),
            None,
        ),
    ] {
        state = advance(
            &state,
            a,
            GameCommand::CastUntargeted {
                spell: object(&state, card),
                payment: pay(price),
            },
        );
        state = resolve(&state);
        let equipment = object(&state, card);
        assert_eq!(state.execution().attached_to(equipment), None);
        assert_eq!(state.execution().window().entries().len(), 1);
        state = resolve(&state);
        let Some(TargetRef::Object(token)) = state.execution().attached_to(equipment) else {
            panic!("missing bearer of {card}")
        };
        let view = state
            .execution()
            .current_characteristics(token)
            .unwrap()
            .unwrap();
        assert_eq!(
            view.current_names
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>(),
            [name]
        );
        assert_eq!(view.current_colors, color.into_iter().collect());
        assert_eq!(
            view.current_creature_subtypes,
            subtypes
                .iter()
                .map(|label| vocabulary.resolve_label(label).unwrap())
                .collect()
        );
        assert_eq!(view.controller, Some(a));
        let actual = state
            .execution()
            .power_toughness(token)
            .unwrap()
            .unwrap()
            .derived;
        assert_eq!((actual.power, actual.toughness), (pt.0.into(), pt.1.into()));
        if let Some(ability) = ability {
            assert!(view.has_keyword(ability));
        }
    }

    let red = || {
        Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Red,
                amount: 1.into(),
            }],
        })
    };
    state = advance(
        &state,
        a,
        GameCommand::CastUntargeted {
            spell: object(&state, 11),
            payment: red(),
        },
    );
    state = resolve(&state);
    let battery = object(&state, 11);
    let Some(TargetRef::Object(hero)) = state.execution().attached_to(object(&state, 10)) else {
        panic!("missing equipped Hero")
    };
    let detach = || GameCommand::ActivateUntargetedAbility {
        source: battery,
        ability: ActivatedAbilityId(2),
        payment: red(),
    };
    let attach = |host| GameCommand::ActivateAbility {
        source: battery,
        ability: ActivatedAbilityId(1),
        target: TargetRef::Object(host),
        payment: red(),
    };
    let attempt = |state: &GameState, command| {
        state.advance(GameInput {
            basis: state.basis().clone(),
            actor: a,
            command,
        })
    };
    assert!(attempt(&state, detach()).is_err());
    assert!(attempt(&state, attach(battery)).is_err());
    assert!(
        state
            .execution()
            .current_characteristics(battery)
            .unwrap()
            .unwrap()
            .is_creature()
    );
    state = advance(&state, a, attach(hero));
    state = resolve(&state);
    let view = state
        .execution()
        .current_characteristics(battery)
        .unwrap()
        .unwrap();
    assert!(!view.is_creature());
    assert!(view.current_creature_subtypes.is_empty());
    assert!(view.has_type(CardType::Artifact));
    assert!(
        state
            .execution()
            .current_characteristics(hero)
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Haste)
    );
    let actual = state
        .execution()
        .power_toughness(hero)
        .unwrap()
        .unwrap()
        .derived;
    assert_eq!((actual.power, actual.toughness), (3.into(), 2.into()));
    state = advance(&state, a, detach());
    state = resolve(&state);
    let view = state
        .execution()
        .current_characteristics(battery)
        .unwrap()
        .unwrap();
    assert!(view.is_creature());
    assert!(
        view.current_creature_subtypes
            .contains(&vocabulary.resolve_label("Rabbit").unwrap())
    );
    assert_eq!(state.execution().attached_to(battery), None);
    assert!(
        !state
            .execution()
            .current_characteristics(hero)
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Haste)
    );
    assert_eq!(state.execution().mana(a, ManaType::Red), 0.into());
}
