//! Literal Oracle condition trajectories, independent of parser condition structure.
use super::engine_trajectories as trajectories;
use super::*;
use spellwake_engine::{game::GameCommand, mana::ManaPayment, object_identity::CardId};
use trajectories::{P0, card, current, object, pass_pair, pt, spell, submit, vocabulary, world};

// Exact source text from the pinned 20261029210158 catalogue.
fn oracle_card(elenda: bool) -> InventoryCard {
    let (name, oracle_id, kind, cost, power, toughness, text) = if elenda {
        (
            "Elenda, Saint of Dusk",
            "bfd614d5-4c7f-416b-9cb2-bb167c0893a0",
            "Legendary Creature \u{2014} Vampire Knight",
            "{2}{W}{B}",
            "4",
            "4",
            "Lifelink, hexproof from instants\nAs long as your life total is greater than your starting life total, Elenda gets +1/+1 and has menace. Elenda gets an additional +5/+5 as long as your life total is at least 10 greater than your starting life total.",
        )
    } else {
        (
            "Tek",
            "f85aef9b-b6ad-4716-b48b-5e26b474a59d",
            "Artifact Creature \u{2014} Dragon",
            "{5}",
            "2",
            "2",
            "This creature gets +0/+2 as long as you control a Plains, has flying as long as you control an Island, gets +2/+0 as long as you control a Swamp, has first strike as long as you control a Mountain, and has trample as long as you control a Forest.",
        )
    };
    InventoryCard {
        scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
        oracle_id: Some(oracle_id.into()),
        name: name.into(),
        layout: "normal".into(),
        faces: vec![spellwake_content::CatalogFace {
            name: name.into(),
            type_line: Some(kind.into()),
            oracle_text: Some(text.into()),
            mana_cost: Some(cost.into()),
            power: Some(power.into()),
            toughness: Some(toughness.into()),
        }],
        legalities: BTreeMap::new(),
        eligible_formats: Vec::new(),
        required_by_base_formats: true,
        program_evidence: "pinned-source-test",
    }
}

#[test]
fn wave12_elenda_life_thresholds_are_separate_and_cumulative() {
    let vocabulary = vocabulary();
    let program =
        compile_with_reason(&oracle_card(true), &vocabulary).expect("Elenda source compiles");
    for starting_life in [20, 40] {
        let mut game = trajectories::world_with_starting_life(
            vocabulary.clone(),
            vec![
                card(1, program.clone(), true),
                spell(
                    10,
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(1),
                    }],
                ),
                spell(
                    11,
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(8),
                    }],
                ),
                spell(
                    12,
                    BodyTarget::None,
                    vec![Instruction::GainControllerLife {
                        amount: ValueExpr::literal(1),
                    }],
                ),
            ],
            vec![CardId(1)],
            starting_life,
        );
        for (spell_id, life, expected_pt, menace) in [
            (None, starting_life, (4, 4), false),
            (Some(10), starting_life + 1, (5, 5), true),
            (Some(11), starting_life + 9, (5, 5), true),
            (Some(12), starting_life + 10, (10, 10), true),
        ] {
            if let Some(id) = spell_id {
                let cast = submit(
                    &game,
                    P0,
                    GameCommand::CastUntargeted {
                        spell: object(&game, id),
                        payment: Some(ManaPayment::default()),
                    },
                );
                game = pass_pair(cast.state()).into_state();
            }
            assert_eq!(game.execution().life_total(P0), Some(&life.into()));
            let elenda = current(&game, object(&game, 1));
            assert_eq!(
                elenda.power_toughness.as_ref().unwrap().derived,
                pt(expected_pt.0, expected_pt.1),
                "life {life}"
            );
            assert_eq!(
                elenda.has_keyword(KeywordAbility::Menace),
                menace,
                "life {life}"
            );
            assert!(elenda.has_keyword(KeywordAbility::Lifelink));
        }
    }
}

#[test]
fn wave12_tek_each_land_gate_works_without_other_land_types() {
    let vocabulary = vocabulary();
    let program =
        compile_with_reason(&oracle_card(false), &vocabulary).expect("Tek source compiles");
    for (land, expected_pt, keyword) in [
        (None, (2, 2), None),
        (Some(LandType::Plains), (2, 4), None),
        (Some(LandType::Island), (2, 2), Some(KeywordAbility::Flying)),
        (Some(LandType::Swamp), (4, 2), None),
        (
            Some(LandType::Mountain),
            (2, 2),
            Some(KeywordAbility::FirstStrike),
        ),
        (
            Some(LandType::Forest),
            (2, 2),
            Some(KeywordAbility::Trample),
        ),
    ] {
        let mut cards = vec![card(1, program.clone(), true)];
        let mut order = vec![CardId(1)];
        if let Some(land) = land {
            cards.push(card(2, Program::land(BTreeSet::from([land]), vec![]), true));
            order.push(CardId(2));
        }
        let game = world(vocabulary.clone(), cards, order);
        let tek = current(&game, object(&game, 1));
        assert_eq!(
            tek.power_toughness.as_ref().unwrap().derived,
            pt(expected_pt.0, expected_pt.1),
            "{land:?}"
        );
        for checked in [
            KeywordAbility::Flying,
            KeywordAbility::FirstStrike,
            KeywordAbility::Trample,
        ] {
            assert_eq!(
                tek.has_keyword(checked),
                keyword == Some(checked),
                "{land:?}: {checked:?}"
            );
        }
    }
    let all_lands = BTreeSet::from([
        LandType::Plains,
        LandType::Island,
        LandType::Swamp,
        LandType::Mountain,
        LandType::Forest,
    ]);
    let game = world(
        vocabulary,
        vec![
            card(1, program, true),
            card(2, Program::land(all_lands, vec![]), true),
        ],
        vec![CardId(1), CardId(2)],
    );
    let tek = current(&game, object(&game, 1));
    assert_eq!(tek.power_toughness.as_ref().unwrap().derived, pt(4, 4));
    for keyword in [
        KeywordAbility::Flying,
        KeywordAbility::FirstStrike,
        KeywordAbility::Trample,
    ] {
        assert!(tek.has_keyword(keyword));
    }
}
