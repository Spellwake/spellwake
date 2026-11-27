use super::*;

fn fixture() -> (InventoryCard, FaceAst, CreatureTypeVocabulary) {
    let vocabulary =
        CreatureTypeVocabulary::new("test-vocab".into(), [0; 32], vec!["Ogre".into()]).unwrap();
    let text = "Roll a d20.\n1\u{2014}9 | Draw two cards.\n10\u{2014}19 | Scry 2, then draw two cards.\n20 | Scry 3, then draw three cards.";
    let input = CardInput {
        oracle_id: "test",
        name: "Contact Other Plane",
        layout: "normal",
        faces: vec![FaceInput {
            name: "Contact Other Plane",
            type_line: "Instant",
            oracle_text: text,
        }],
    };
    let ast = compile_card(&input, &cached_compiler_vocab(&vocabulary))
        .faces
        .remove(0);
    let card = InventoryCard {
        scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
        oracle_id: None,
        name: "Contact Other Plane".into(),
        layout: "normal".into(),
        faces: vec![spellwake_content::CatalogFace {
            name: "Contact Other Plane".into(),
            type_line: Some("Instant".into()),
            oracle_text: Some(text.into()),
            mana_cost: Some("{3}{U}".into()),
            power: None,
            toughness: None,
        }],
        legalities: BTreeMap::new(),
        eligible_formats: vec![],
        required_by_base_formats: true,
        program_evidence: "test",
    };
    (card, ast, vocabulary)
}

#[test]
fn compiled_die_roll_bands_preserve_legacy_ranges_and_bodies() {
    let (card, ast, vocabulary) = fixture();
    assert_eq!(
        ast.statics
            .iter()
            .filter(|s| matches!(s, StaticAbilityAst::CompiledDieRollBand { .. }))
            .count(),
        3
    );
    let typed = lower_face(
        &card,
        &card.faces[0],
        &ast,
        &vocabulary,
        FaceLoweringOptions::default(),
    )
    .unwrap();
    let mut legacy_ast = ast.clone();
    for item in &mut legacy_ast.statics {
        if let StaticAbilityAst::CompiledDieRollBand { roll, payload, .. } = item {
            *item = StaticAbilityAst::DieRollBand {
                roll: roll.clone(),
                payload: payload.clone(),
            };
        }
    }
    let legacy = lower_face(
        &card,
        &card.faces[0],
        &legacy_ast,
        &vocabulary,
        FaceLoweringOptions::default(),
    )
    .unwrap();
    assert_eq!(typed, legacy);
    let ProgramKind::Spell(spell) = &typed.kind else {
        panic!("spell")
    };
    let Instruction::MutatePlayer(spec) = &spell.body.instructions[0] else {
        panic!("roll")
    };
    let spellwake_engine::toolbox::PlayerMutation::RandomEvent(
        spellwake_engine::toolbox::RandomEventOp::RollDie { bands, .. },
    ) = &spec.op
    else {
        panic!("roll")
    };
    assert_eq!(
        bands.iter().map(|b| (b.min, b.max)).collect::<Vec<_>>(),
        vec![(1, 9), (10, 19), (20, 20)]
    );
    assert_eq!(
        bands
            .iter()
            .map(|b| b.body.instructions.len())
            .collect::<Vec<_>>(),
        vec![1, 2, 2]
    );
}

#[test]
fn compiled_die_roll_bands_fail_closed_without_raw_fallback() {
    let (card, original, vocabulary) = fixture();
    for variant in 0..3 {
        let mut ast = original.clone();
        let StaticAbilityAst::CompiledDieRollBand { roll, body, .. } = &mut ast.statics[0] else {
            panic!("compiled band")
        };
        match variant {
            0 => body.clear(),
            1 => *roll = "19-10".into(),
            _ => body.push(InstructionAst::Draw {
                player: PlayerRefAst::You,
                count: Box::new(ValueExprAst::DistinctCountAmong {
                    category: "unsupported-die-band-value".into(),
                    filter: Box::new(
                        spellwake_oracle_compiler::filter::parse_object_filter(
                            "creatures you control",
                            &cached_compiler_vocab(&vocabulary),
                        )
                        .unwrap(),
                    ),
                }),
                up_to: false,
            }),
        }
        assert!(
            lower_face(
                &card,
                &card.faces[0],
                &ast,
                &vocabulary,
                FaceLoweringOptions::default()
            )
            .is_err(),
            "variant {variant}"
        );
    }
}

#[test]
fn pinned_e14_die_roll_regressions_lower_completely() {
    let vocabulary = CreatureTypeVocabulary::new(
        "test-vocab".into(),
        [0; 32],
        vec![
            "Beholder".into(),
            "Boar".into(),
            "Cleric".into(),
            "Devil".into(),
            "Djinn".into(),
            "Dog".into(),
            "Druid".into(),
            "Elemental".into(),
            "Elf".into(),
            "Goat".into(),
            "Goblin".into(),
            "Halfling".into(),
            "Horror".into(),
            "Human".into(),
            "Knight".into(),
            "Ogre".into(),
            "Ox".into(),
            "Rogue".into(),
            "Shaman".into(),
            "Soldier".into(),
            "Sorcerer".into(),
            "Tiefling".into(),
            "Troll".into(),
            "Warlock".into(),
            "Warrior".into(),
            "Wizard".into(),
        ],
    )
    .unwrap();
    for (name, type_line, cost, power, toughness, text) in [
        (
            "Arcane Investigator",
            "Creature \u{2014} Elf Wizard",
            "{1}{U}",
            Some("2"),
            Some("1"),
            "Search the Room \u{2014} {5}{U}: Roll a d20.\n1\u{2014}9 | Draw a card.\n10\u{2014}20 | Look at the top three cards of your library. Put one of them into your hand and the rest on the bottom of your library in any order.",
        ),
        (
            "Chaos Channeler",
            "Creature \u{2014} Human Shaman",
            "{2}{R}{R}",
            Some("4"),
            Some("3"),
            "Wild Magic Surge \u{2014} Whenever this creature attacks, roll a d20.\n1\u{2014}9 | Exile the top card of your library. You may play it this turn.\n10\u{2014}19 | Exile the top two cards of your library. You may play them this turn.\n20 | Exile the top three cards of your library. You may play them this turn.",
        ),
        (
            "Component Pouch",
            "Artifact",
            "{3}",
            None,
            None,
            "{T}, Remove a component counter from this artifact: Add two mana of different colors.\n{T}: Roll a d20.\n1\u{2014}9 | Put a component counter on this artifact.\n10\u{2014}20 | Put two component counters on this artifact.",
        ),
        (
            "Contact Other Plane",
            "Instant",
            "{3}{U}",
            None,
            None,
            "Roll a d20.\n1\u{2014}9 | Draw two cards.\n10\u{2014}19 | Scry 2, then draw two cards.\n20 | Scry 3, then draw three cards.",
        ),
        (
            "Contraband Livestock",
            "Instant",
            "{1}{W}",
            None,
            None,
            "Exile target creature, then roll a d20.\n1\u{2014}9 | Its controller creates a 4/4 green Ox creature token.\n10\u{2014}19 | Its controller creates a 2/2 green Boar creature token.\n20 | Its controller creates a 0/1 white Goat creature token.",
        ),
        (
            "Diviner's Portent",
            "Instant",
            "{X}{U}{U}{U}",
            None,
            None,
            "Roll a d20 and add the number of cards in your hand.\n1\u{2014}14 | Draw X cards.\n15+ | Scry X, then draw X cards.",
        ),
        (
            "Djinni Windseer",
            "Creature \u{2014} Djinn",
            "{3}{U}",
            Some("3"),
            Some("3"),
            "Flying\nWhen this creature enters, roll a d20.\n1\u{2014}9 | Scry 1.\n10\u{2014}19 | Scry 2.\n20 | Scry 3.",
        ),
        (
            "Earth-Cult Elemental",
            "Creature \u{2014} Elemental",
            "{4}{R}{R}",
            Some("6"),
            Some("6"),
            "Siege Monster \u{2014} When this creature enters, roll a d20.\n1\u{2014}9 | Each player sacrifices a permanent of their choice.\n10\u{2014}19 | Each opponent sacrifices a permanent of their choice.\n20 | Each opponent sacrifices two permanents of their choice.",
        ),
        (
            "Farideh's Fireball",
            "Instant",
            "{3}{R}{R}",
            None,
            None,
            "Farideh's Fireball deals 5 damage to target creature or planeswalker. Roll a d20.\n1\u{2014}9 | Farideh's Fireball deals 2 damage to each player.\n10\u{2014}20 | Farideh's Fireball deals 2 damage to each opponent.",
        ),
        (
            "Gale's Redirection",
            "Instant",
            "{3}{U}{U}",
            None,
            None,
            "Exile target spell, then roll a d20 and add that spell's mana value.\n1\u{2014}14 | You may cast the exiled card for as long as it remains exiled, and you may spend mana as though it were mana of any color to cast that spell.\n15+ | You may cast the exiled card without paying its mana cost for as long as it remains exiled.",
        ),
        (
            "Goblin Morningstar",
            "Artifact \u{2014} Equipment",
            "{1}{R}",
            None,
            None,
            "Equipped creature gets +1/+0 and has trample.\nEquip {1}{R}\nWhen this Equipment enters, roll a d20.\n1\u{2014}9 | Create a 1/1 red Goblin creature token.\n10\u{2014}20 | Create a 1/1 red Goblin creature token, then attach this Equipment to it.",
        ),
        (
            "Herald of Hadar",
            "Creature \u{2014} Human Warlock",
            "{4}{B}",
            Some("4"),
            Some("4"),
            "Circle of Death \u{2014} {5}{B}: Roll a d20.\n1\u{2014}9 | Each opponent loses 2 life.\n10\u{2014}19 | Each opponent loses 2 life and you gain 2 life.\n20 | Each opponent loses 2 life and you gain 2 life. Create two Treasure tokens. (They're artifacts with \"{T}, Sacrifice this token: Add one mana of any color.\")",
        ),
        (
            "Hoarding Ogre",
            "Creature \u{2014} Ogre",
            "{3}{R}",
            Some("3"),
            Some("3"),
            "Whenever this creature attacks, roll a d20.\n1\u{2014}9 | Create a Treasure token. (It's an artifact with \"{T}, Sacrifice this token: Add one mana of any color.\")\n10\u{2014}19 | Create two Treasure tokens.\n20 | Create three Treasure tokens.",
        ),
        (
            "Iron Mastiff",
            "Artifact Creature \u{2014} Dog",
            "{4}",
            Some("4"),
            Some("4"),
            "Whenever this creature attacks, roll a d20 for each player being attacked and ignore all but the highest roll.\n1\u{2014}9 | This creature deals damage equal to its power to you.\n10\u{2014}19 | This creature deals damage equal to its power to defending player.\n20 | This creature deals damage equal to its power to each opponent.",
        ),
        (
            "Lightfoot Rogue",
            "Creature \u{2014} Halfling Rogue",
            "{1}{B}",
            Some("2"),
            Some("1"),
            "Sneak Attack \u{2014} Whenever this creature attacks, roll a d20.\n1\u{2014}9 | This creature gains deathtouch until end of turn.\n10\u{2014}19 | It gets +1/+0 and gains deathtouch until end of turn.\n20 | It gets +3/+0 and gains first strike and deathtouch until end of turn.",
        ),
        (
            "Loathsome Troll",
            "Creature \u{2014} Troll",
            "{3}{G}{G}",
            Some("6"),
            Some("2"),
            "{3}{G}: Roll a d20. Activate only if this card is in your graveyard.\n1\u{2014}9 | Put this card on top of your library.\n10\u{2014}19 | Return this card to your hand.\n20 | Return this card to the battlefield tapped.",
        ),
        (
            "Mathise, Surge Channeler",
            "Legendary Creature \u{2014} Human Elf Shaman Sorcerer",
            "{2}{U}",
            Some("1"),
            Some("1"),
            "Whenever you cast an instant or sorcery spell with mana value 3 or greater, roll a d20.\n1-9 | Each player draws a card.\n10-19 | You draw a card.\n20 | Copy that spell. You may choose new targets for the copy.",
        ),
        (
            "Myrkul's Edict",
            "Sorcery",
            "{1}{B}",
            None,
            None,
            "Roll a d20.\n1\u{2014}9 | Choose an opponent. That player sacrifices a creature of their choice.\n10\u{2014}19 | Each opponent sacrifices a creature of their choice.\n20 | Each opponent sacrifices a creature with the greatest power among creatures that player controls.",
        ),
        (
            "Nothic",
            "Creature \u{2014} Horror",
            "{4}{B}",
            Some("4"),
            Some("3"),
            "Weird Insight \u{2014} When this creature dies, roll a d20.\n1\u{2014}9 | You draw a card and you lose 1 life.\n10\u{2014}19 | You draw two cards and you lose 2 life.\n20 | You draw seven cards and you lose 7 life.",
        ),
        (
            "Overwhelming Encounter",
            "Sorcery",
            "{3}{G}{G}",
            None,
            None,
            "Creatures you control gain vigilance and trample until end of turn. Roll a d20.\n1\u{2014}9 | Creatures you control get +2/+2 until end of turn.\n10\u{2014}19 | Put two +1/+1 counters on each creature you control.\n20 | Put four +1/+1 counters on each creature you control.",
        ),
        (
            "Recruitment Drive",
            "Sorcery",
            "{2}{W}",
            None,
            None,
            "Roll a d20.\n1\u{2014}9 | Create two 1/1 white Soldier creature tokens.\n10\u{2014}19 | Create two 2/2 white Knight creature tokens.\n20 | Create three 2/2 white Knight creature tokens.",
        ),
        (
            "Sylvan Shepherd",
            "Creature \u{2014} Human Druid",
            "{2}{G}",
            Some("2"),
            Some("3"),
            "Vigilance\nWhenever this creature attacks, roll a d20.\n1\u{2014}9 | You gain 1 life.\n10\u{2014}19 | You gain 2 life.\n20 | You gain 5 life.",
        ),
    ] {
        let card = InventoryCard {
            scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
            oracle_id: None,
            name: name.into(),
            layout: "normal".into(),
            faces: vec![spellwake_content::CatalogFace {
                name: name.into(),
                type_line: Some(type_line.into()),
                oracle_text: Some(text.into()),
                mana_cost: Some(cost.into()),
                power: power.map(str::to_owned),
                toughness: toughness.map(str::to_owned),
            }],
            legalities: BTreeMap::new(),
            eligible_formats: vec![],
            required_by_base_formats: true,
            program_evidence: "test",
        };
        compile_with_reason(&card, &vocabulary).unwrap_or_else(|reason| panic!("{name}: {reason}"));
    }
}
