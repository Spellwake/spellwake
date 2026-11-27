//! Wave 14 literal Oracle selector and object-filter behavior trajectories.
use super::engine_trajectories as trajectories;
use super::*;
use spellwake_contracts::names::{EnglishCardName, ObjectNames};
use spellwake_engine::{
    game::GameCommand,
    mana::ManaPayment,
    object_identity::{CardId, ObjectDestination, ObjectZone},
};
use trajectories::{
    P0, card, current, object, pass_pair, permanent, pt, spell, submit, vocabulary, world,
};

fn make_card(
    name: &str,
    type_line: &str,
    mana_cost: &str,
    power: Option<&str>,
    toughness: Option<&str>,
    text: &str,
) -> InventoryCard {
    InventoryCard {
        scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
        oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
        name: name.into(),
        layout: "normal".into(),
        faces: vec![spellwake_content::CatalogFace {
            name: name.into(),
            type_line: Some(type_line.into()),
            oracle_text: Some(text.into()),
            mana_cost: Some(mana_cost.into()),
            power: power.map(str::to_owned),
            toughness: toughness.map(str::to_owned),
        }],
        legalities: BTreeMap::new(),
        eligible_formats: Vec::new(),
        required_by_base_formats: true,
        program_evidence: "wave14-pinned-source-test",
    }
}

#[test]
fn wave14_cross_category_or_selector_buffs_food_and_golem_creatures_only() {
    let vocab = vocabulary();
    let brenard = compile_with_reason(
        &make_card(
            "Ginger Sculptor Static Test",
            "Legendary Creature \u{2014} Elf Artificer",
            "{1}{G}{W}{U}",
            Some("3"),
            Some("3"),
            "Each creature you control that's a Food or a Golem gets +2/+2 and has trample.",
        ),
        &vocab,
    )
    .expect("Food or Golem cross-category selector compiles");

    let mut food_creature = permanent(
        &vocab,
        &[CardType::Artifact, CardType::Creature],
        Some((1, 1)),
        &[],
    );
    food_creature
        .artifact_subtypes
        .insert(ArtifactSubtype::Food);

    let golem = permanent(
        &vocab,
        &[CardType::Artifact, CardType::Creature],
        Some((2, 2)),
        &["Golem"],
    );
    let bear = permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]);

    let game = world(
        vocab.clone(),
        vec![
            card(1, brenard, true),
            card(2, food_creature, true),
            card(3, golem, true),
            card(4, bear, true),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );
    assert_eq!(
        current(&game, object(&game, 2))
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pt(3, 3)
    );
    assert!(current(&game, object(&game, 2)).has_keyword(KeywordAbility::Trample));
    assert_eq!(
        current(&game, object(&game, 3))
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pt(4, 4)
    );
    assert!(current(&game, object(&game, 3)).has_keyword(KeywordAbility::Trample));
    assert_eq!(
        current(&game, object(&game, 4))
            .power_toughness
            .as_ref()
            .unwrap()
            .derived,
        pt(2, 2)
    );
    assert!(!current(&game, object(&game, 4)).has_keyword(KeywordAbility::Trample));
}

#[test]
fn wave14_ambiguous_multi_subtype_without_or_marker_fails_closed() {
    let vocab = vocabulary();
    // Stage 1 marks plural "and" subtype lists ("Mounts and Vehicles") with SubtypesMatchAny so
    // Miriam grants hexproof to both Mounts and Vehicles (and not Bears), while adjacent compound
    // subtypes ("Eldrazi Spawn") without an explicit disjunction still fail closed.
    let miriam = compile_with_reason(
        &make_card(
            "Miriam, Herd Whisperer",
            "Legendary Creature \u{2014} Human Druid",
            "{G}{W}",
            Some("3"),
            Some("2"),
            "During your turn, Mounts and Vehicles you control have hexproof.\nWhenever a Mount or Vehicle you control attacks, put a +1/+1 counter on it.",
        ),
        &vocab,
    )
    .expect("Miriam plural 'and' multi-subtype compiles with SubtypesMatchAny");
    let mount = permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Mount"]);
    let mut vehicle = permanent(&vocab, &[CardType::Artifact], None, &[]);
    vehicle.artifact_subtypes.insert(ArtifactSubtype::Vehicle);
    let bear = permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]);
    let game = world(
        vocab.clone(),
        vec![
            card(1, miriam, true),
            card(2, mount, true),
            card(3, vehicle, true),
            card(4, bear, true),
        ],
        vec![CardId(1), CardId(2), CardId(3), CardId(4)],
    );
    assert!(current(&game, object(&game, 2)).has_keyword(KeywordAbility::Hexproof));
    assert!(current(&game, object(&game, 3)).has_keyword(KeywordAbility::Hexproof));
    assert!(!current(&game, object(&game, 4)).has_keyword(KeywordAbility::Hexproof));

    let broodwarden_err = compile_with_reason(
        &make_card(
            "Broodwarden",
            "Creature \u{2014} Eldrazi Drone",
            "{3}{G}{G}",
            Some("4"),
            Some("4"),
            "Eldrazi Spawn creatures you control get +2/+1.",
        ),
        &vocab,
    )
    .expect_err(
        "Broodwarden compound multi-subtype must fail closed until Stage 1 distinguishes it",
    );
    assert_eq!(broodwarden_err, "oracle:selector-multi-subtypes");
}

#[test]
fn wave14_wave_goodbye_bounces_only_creatures_without_plus_one_counter() {
    let vocab = vocabulary();
    let wave_goodbye = compile_with_reason(
        &make_card(
            "Wave Goodbye",
            "Sorcery",
            "{2}{U}{U}",
            None,
            None,
            "Return each creature without a +1/+1 counter on it to its owner's hand.",
        ),
        &vocab,
    )
    .expect("Wave Goodbye compiles");
    let mut wave_free = wave_goodbye;
    if let spellwake_engine::program::ProgramKind::Spell(def) = &mut wave_free.kind {
        def.cost = ManaCost::free();
    }

    let put_counter_spell = spell(
        10,
        BodyTarget::SINGLE_CREATURE,
        vec![super::toolbox_emit::put_counters_on_subject(
            spellwake_engine::program::EffectSubject::TargetObject,
            CounterKind::PlusOne,
            ValueExpr::literal(1),
        )],
    );

    let game = world(
        vocab.clone(),
        vec![
            card(
                1,
                permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]),
                true,
            ),
            card(
                2,
                permanent(&vocab, &[CardType::Creature], Some((3, 3)), &["Beast"]),
                true,
            ),
            put_counter_spell,
            card(11, wave_free, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    // Put a +1/+1 counter on Bear (CardId 1).
    let bear_id = object(&game, 1);
    let cast_counter = submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: object(&game, 10),
            target: spellwake_engine::targets::TargetRef::Object(bear_id),
            payment: ManaPayment::default(),
        },
    );
    let game = pass_pair(cast_counter.state()).into_state();
    assert!(
        current(&game, object(&game, 1)).has_plus_one_counter,
        "Bear should have a +1/+1 counter before Wave Goodbye resolves"
    );

    // Cast Wave Goodbye: Beast (no +1/+1 counter) returns to hand; Bear (+1/+1 counter) stays.
    let cast_wave = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 11),
            payment: Some(ManaPayment::default()),
        },
    );
    let game = pass_pair(cast_wave.state()).into_state();

    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(object(&game, 1))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(object(&game, 2))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0)
    );
}

#[test]
fn wave14_oil_gorger_troll_requires_exact_oil_counter_not_plus_one() {
    let vocab = vocabulary();
    let mut troll = compile_with_reason(
        &make_card(
            "Oil-Gorger Troll",
            "Creature \u{2014} Phyrexian Troll Warrior",
            "{3}{G}",
            Some("3"),
            Some("4"),
            "When this creature enters, you gain 3 life. Then if you control a permanent with an oil counter on it, draw a card.",
        ),
        &vocab,
    )
    .expect("Oil-Gorger Troll compiles");
    if let spellwake_engine::program::ProgramKind::Permanent(def) = &mut troll.kind {
        def.casting_cost = Some(ManaCost::free());
    }

    let put_plus_one = spell(
        10,
        BodyTarget::SINGLE_CREATURE,
        vec![super::toolbox_emit::put_counters_on_subject(
            spellwake_engine::program::EffectSubject::TargetObject,
            CounterKind::PlusOne,
            ValueExpr::literal(1),
        )],
    );
    let put_oil = spell(
        11,
        BodyTarget::SINGLE_CREATURE,
        vec![super::toolbox_emit::put_counters_on_subject(
            spellwake_engine::program::EffectSubject::TargetObject,
            CounterKind::Oil,
            ValueExpr::literal(1),
        )],
    );

    let base_cards = |troll_prog: Program| {
        vec![
            card(
                1,
                permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]),
                true,
            ),
            card(2, troll_prog, false),
            put_plus_one.clone(),
            put_oil.clone(),
            spellwake_engine::effects::SeedCard {
                card: CardId(20),
                owner: P0,
                destination: ObjectDestination::LibraryTop,
                program: permanent(&vocab, &[CardType::Creature], Some((1, 1)), &["Elf"]),
            },
        ]
    };

    // Case 1: Bear has a +1/+1 counter (NOT oil) -> Troll gains 3 life, does NOT draw CardId 20.
    let game1 = world(vocab.clone(), base_cards(troll.clone()), vec![CardId(1)]);
    let c1 = submit(
        &game1,
        P0,
        GameCommand::CastPaid {
            spell: object(&game1, 10),
            target: spellwake_engine::targets::TargetRef::Object(object(&game1, 1)),
            payment: ManaPayment::default(),
        },
    );
    let game1 = pass_pair(c1.state()).into_state();
    let cast_troll1 = submit(
        &game1,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game1, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let game1 = pass_pair(cast_troll1.state()).into_state();
    let game1 = pass_pair(&game1).into_state();
    assert_eq!(game1.execution().life_total(P0), Some(&23.into()));
    assert_eq!(
        game1
            .execution()
            .window()
            .cards()
            .get(object(&game1, 20))
            .unwrap()
            .zone(),
        ObjectZone::Library(P0),
        "Plus-one counter must not satisfy oil counter check"
    );

    // Case 2: an oil counter exists -> Troll gains 3 life AND draws CardId 20.
    let game2 = world(vocab.clone(), base_cards(troll), vec![CardId(1)]);
    let c2 = submit(
        &game2,
        P0,
        GameCommand::CastPaid {
            spell: object(&game2, 11),
            target: spellwake_engine::targets::TargetRef::Object(object(&game2, 1)),
            payment: ManaPayment::default(),
        },
    );
    let game2 = pass_pair(c2.state()).into_state();
    let cast_troll2 = submit(
        &game2,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game2, 2),
            payment: Some(ManaPayment::default()),
        },
    );
    let game2 = pass_pair(cast_troll2.state()).into_state();
    let game2 = pass_pair(&game2).into_state();
    assert_eq!(game2.execution().life_total(P0), Some(&23.into()));
    assert_eq!(
        game2
            .execution()
            .window()
            .cards()
            .get(object(&game2, 20))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0),
        "Oil counter must satisfy oil counter check and draw"
    );
}

#[test]
fn wave14_charmed_stray_exact_name_buffs_only_named_other_creatures() {
    let vocab = vocabulary();
    let mut stray = compile_with_reason(
        &make_card(
            "Charmed Stray",
            "Creature \u{2014} Cat",
            "{W}",
            Some("1"),
            Some("1"),
            "Lifelink\nWhen Charmed Stray enters, put a +1/+1 counter on each other creature you control named Charmed Stray.",
        ),
        &vocab,
    )
    .expect("Charmed Stray compiles");
    stray.names = ObjectNames::single(EnglishCardName::new("Charmed Stray".into()).unwrap());
    if let spellwake_engine::program::ProgramKind::Permanent(def) = &mut stray.kind {
        def.casting_cost = Some(ManaCost::free());
    }

    let mut other_stray = permanent(&vocab, &[CardType::Creature], Some((1, 1)), &["Cat"]);
    other_stray.names = ObjectNames::single(EnglishCardName::new("Charmed Stray".into()).unwrap());
    let mut bear = permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]);
    bear.names = ObjectNames::single(EnglishCardName::new("Grizzly Bears".into()).unwrap());

    let game = world(
        vocab.clone(),
        vec![
            card(1, other_stray, true),
            card(2, bear, true),
            card(3, stray, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let game = pass_pair(cast.state()).into_state();
    let game = pass_pair(&game).into_state();
    assert_eq!(
        current(&game, object(&game, 1)).plus_one_counters,
        1.into(),
        "Other creature named Charmed Stray gets +1/+1 counter"
    );
    assert_eq!(
        current(&game, object(&game, 2)).plus_one_counters,
        0.into(),
        "Creature with another name does not get counter"
    );
    assert_eq!(
        current(&game, object(&game, 3)).plus_one_counters,
        0.into(),
        "Entering Charmed Stray excludes itself"
    );

    // Also verify NameFilterAst::Exact (lowercase Stage 1 exact name) matches mixed-case ObjectNames.
    let mut exact_spell = compile_with_reason(
        &make_card(
            "Stray Rally",
            "Sorcery",
            "{W}",
            None,
            None,
            "Put a +1/+1 counter on each creature you control named Charmed Stray.",
        ),
        &vocab,
    )
    .expect("Exact-name spell compiles");
    if let spellwake_engine::program::ProgramKind::Spell(def) = &mut exact_spell.kind {
        def.cost = ManaCost::free();
    }
    let mut named_cat = permanent(&vocab, &[CardType::Creature], Some((1, 1)), &["Cat"]);
    named_cat.names = ObjectNames::single(EnglishCardName::new("Charmed Stray".into()).unwrap());
    let mut named_bear = permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]);
    named_bear.names = ObjectNames::single(EnglishCardName::new("Grizzly Bears".into()).unwrap());
    let game2 = world(
        vocab.clone(),
        vec![
            card(1, named_cat, true),
            card(2, named_bear, true),
            card(3, exact_spell, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let cast2 = submit(
        &game2,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game2, 3),
            payment: Some(ManaPayment::default()),
        },
    );
    let game2 = pass_pair(cast2.state()).into_state();
    assert_eq!(
        current(&game2, object(&game2, 1)).plus_one_counters,
        1.into()
    );
    assert_eq!(
        current(&game2, object(&game2, 2)).plus_one_counters,
        0.into()
    );
}

#[test]
fn wave14_six_subtype_disjunction_and_five_subtype_forbidden_list_execute() {
    let vocab = vocabulary();
    let sanctuary = compile_with_reason(
        &make_card(
            "Animal Sanctuary",
            "Land",
            "",
            None,
            None,
            "{T}: Add {C}.\n{2}, {T}: Put a +1/+1 counter on target Bird, Cat, Dog, Goat, Ox, or Snake.",
        ),
        &vocab,
    )
    .expect("Animal Sanctuary 6-subtype AnyOfList compiles");
    assert!(
        !sanctuary.activated.is_empty(),
        "Animal Sanctuary should produce activated abilities"
    );

    let mut bounce_non_sea = compile_with_reason(
        &make_card(
            "Sea Monsters Bounce Test",
            "Sorcery",
            "{U}",
            None,
            None,
            "Return each creature that isn't a Kraken, Leviathan, Merfolk, Octopus, or Serpent to its owner's hand.",
        ),
        &vocab,
    )
    .expect("5-subtype NoneOfList compiles");
    if let spellwake_engine::program::ProgramKind::Spell(def) = &mut bounce_non_sea.kind {
        def.cost = ManaCost::free();
    }

    let game = world(
        vocab.clone(),
        vec![
            card(
                1,
                permanent(&vocab, &[CardType::Creature], Some((5, 5)), &["Serpent"]),
                true,
            ),
            card(
                2,
                permanent(&vocab, &[CardType::Creature], Some((2, 2)), &["Bear"]),
                true,
            ),
            card(10, bounce_non_sea, false),
        ],
        vec![CardId(1), CardId(2)],
    );
    let cast = submit(
        &game,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&game, 10),
            payment: Some(ManaPayment::default()),
        },
    );
    let game = pass_pair(cast.state()).into_state();
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(object(&game, 1))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield,
        "Serpent is excluded by 5-subtype NoneOfList and stays on battlefield"
    );
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(object(&game, 2))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P0),
        "Bear is not in the 5-subtype NoneOfList and returns to hand"
    );
}

#[test]
fn wave14_bloomburrow_four_subtypes_and_background_compile_and_dies_non_pt_counter_fails_closed() {
    let vocab = vocabulary();
    compile_with_reason(
        &make_card(
            "Valley Mightcaller",
            "Creature \u{2014} Frog",
            "{G}",
            Some("1"),
            Some("1"),
            "Trample\nWhenever another Frog, Rabbit, Raccoon, or Squirrel you control enters, put a +1/+1 counter on this creature.",
        ),
        &vocab,
    )
    .expect("Valley Mightcaller 4-subtype AnyOfList compiles");

    compile_with_reason(
        &make_card(
            "Swarmyard",
            "Land",
            "",
            None,
            None,
            "{T}: Add {C}.\n{T}: Regenerate target Insect, Rat, Spider, or Squirrel.",
        ),
        &vocab,
    )
    .expect("Swarmyard 4-subtype AnyOfList compiles");

    compile_with_reason(
        &make_card(
            "Raised by Giants",
            "Legendary Enchantment \u{2014} Background",
            "{5}{G}",
            None,
            None,
            "Commander creatures you own have base power and toughness 10/10 and are Giants in addition to their other types.",
        ),
        &vocab,
    )
    .expect("Raised by Giants Background selector compiles");

    let err = compile_with_reason(
        &make_card(
            "Dies Non-PT Counter Guard",
            "Enchantment",
            "{1}{B}",
            None,
            None,
            "Whenever a creature with an oil counter on it dies, draw a card.",
        ),
        &vocab,
    )
    .expect_err("Non-PT counter filter on battlefield-departure trigger must fail closed");
    assert_eq!(err, "oracle:unsupported-object-filter");
}
