//! Equipment tokens and ordered library selections through the public game root.
use super::super::declarations::{attacks, combat_on, invalid};
use super::additional_walkers::permanent;
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_nahiri_lithomancer_equips_her_kor_puts_equipment_from_either_zone_and_creates_an_indestructible_blade()
 {
    use ManaType::*;
    verify_commander_deck("Nahiri, the Lithomancer", "Plains");
    let ready = with_loyalty_for_ultimate(
        "Nahiri, the Lithomancer",
        &[(Colourless, 3), (White, 2)],
        3,
        10,
        vec![
            named("Bonesplitter", 2, A, battlefield(A)),
            named("Bonesplitter", 3, A, ObjectDestination::Hand),
            named("Bonesplitter", 4, A, ObjectDestination::Graveyard),
            named("Bonesplitter", 5, B, ObjectDestination::Graveyard),
            creature(6, A, 2, 2, &[]),
            seed(
                7,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
        ],
    );
    let created = round(command(&ready, A, activate(&ready, 1, 0, &[])).into_state());
    let token = tokens(&created, A, 1)[0];
    let offer = created.into_state();
    let sheet = offer
        .execution()
        .current_characteristics(token)
        .unwrap()
        .unwrap();
    assert_eq!(sheet.current_colors, [Color::White].into());
    assert_eq!(
        sheet.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    let vocab = current_catalogue().1;
    assert_eq!(
        sheet.current_creature_subtypes,
        ["Kor", "Soldier"]
            .map(|name| vocab.resolve_label(name).unwrap())
            .into()
    );
    for attach in [false, true] {
        let chosen = reply(
            &offer,
            DecisionReply::SelectResolution(if attach {
                vec![physical(&offer, 2).id()]
            } else {
                vec![]
            }),
        )
        .into_state();
        let chosen = if attach {
            reply(&chosen, DecisionReply::SelectResolution(vec![token])).into_state()
        } else {
            chosen
        };
        assert_eq!(
            chosen.execution().attached_to(physical(&chosen, 2).id()),
            attach.then_some(TargetRef::Object(token))
        );
    }
    let offer = round(command(&ready, A, activate(&ready, 1, 1, &[])).into_state()).into_state();
    invalid(
        &offer,
        A,
        DecisionReply::SelectResolution(vec![physical(&offer, 5).id()]),
    );
    let no = reply(&offer, DecisionReply::SelectResolution(vec![])).into_state();
    assert_eq!(physical(&no, 4).zone(), ObjectZone::Graveyard(A));
    for n in [3, 4] {
        let found = reply(
            &offer,
            DecisionReply::SelectResolution(vec![physical(&offer, n).id()]),
        )
        .into_state();
        assert_eq!(physical(&found, n).zone(), ObjectZone::Battlefield);
    }
    let forged = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state());
    let blade = tokens(&forged, A, 1)[0];
    let forged = forged.into_state();
    let view = forged
        .execution()
        .current_characteristics(blade)
        .unwrap()
        .unwrap();
    assert!(
        view.current_names
            .iter()
            .any(|n| n.as_str() == "Stoneforged Blade")
    );
    assert!(view.current_colors.is_empty());
    assert!(view.has_type(CardType::Artifact) && view.has_keyword(KeywordAbility::Indestructible));
    let safe = round(targeted(&forged, A, 7, TargetRef::Object(blade)).into_state()).into_state();
    assert!(safe.execution().window().cards().get(blade).is_some());
    let equip = GameCommand::ActivateAbility {
        source: blade,
        ability: ActivatedAbilityId(1),
        target: permanent(&safe, 6),
        payment: None,
    };
    let wield = round(command(&safe, A, equip).into_state()).into_state();
    assert_eq!(
        wield.execution().attached_to(blade),
        Some(permanent(&wield, 6))
    );
    let view = wield
        .execution()
        .current_characteristics(physical(&wield, 6).id())
        .unwrap()
        .unwrap();
    assert_eq!(
        view.power_toughness.as_ref().unwrap().derived,
        PowerToughness {
            power: 7.into(),
            toughness: 7.into()
        }
    );
    assert!(view.has_keyword(KeywordAbility::DoubleStrike));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_jace_architect_weakens_opposing_attacks_and_resolves_piles_owned_libraries_and_free_spells_separately()
 {
    use ManaType::*;
    let cards = vec![
        creature(2, B, 3, 3, &[]),
        creature(3, C, 3, 3, &[]),
        named("Sol Ring", 10, A, ObjectDestination::LibraryBottom),
        named("Island", 11, A, ObjectDestination::LibraryBottom),
        named("Forest", 12, A, ObjectDestination::LibraryBottom),
        named("Swamp", 13, A, ObjectDestination::LibraryBottom),
        named("Elvish Mystic", 20, B, ObjectDestination::LibraryBottom),
        named("Forest", 21, B, ObjectDestination::LibraryBottom),
        named("Plains", 30, C, ObjectDestination::LibraryBottom),
    ];
    let base = with_loyalty_for_ultimate(
        "Jace, Architect of Thought",
        &[(Colourless, 2), (Blue, 2)],
        4,
        8,
        cards,
    );
    let ready = base;
    let watch = round(command(&ready, A, activate(&ready, 1, 0, &[])).into_state()).into_state();
    let b = combat_on(watch, 2);
    let swing = reply(&b, attacks(&b, &[(2, C, None)])).into_state();
    let shrunk = round(swing).into_state();
    let power = |s: &GameState, n| {
        s.execution()
            .current_characteristics(physical(s, n).id())
            .unwrap()
            .unwrap()
            .power_toughness
            .unwrap()
            .derived
            .power
    };
    assert_eq!(power(&shrunk, 2), 2.into());
    let c = combat_on(shrunk, 3);
    assert_eq!(power(&c, 2), 3.into());
    assert_eq!(c.execution().life_total(C), Some(&18.into()));
    let again = reply(&c, attacks(&c, &[(3, A, None)])).into_state();
    let hit = round(again).into_state();
    assert_eq!(power(&hit, 3), 2.into());
    let next = combat_on(hit, 5);
    let normal = reply(&next, attacks(&next, &[(2, C, None)])).into_state();
    assert!(normal.execution().window().entries().is_empty());
    assert_eq!(power(&normal, 2), 3.into());
    let pile = round(command(&ready, A, activate(&ready, 1, 1, &[])).into_state()).into_state();
    let assign = reply(&pile, DecisionReply::OpponentChoice(C)).into_state();
    assert_eq!(assign.pending_decision().unwrap().chooser(), C);
    let divided = reply(
        &assign,
        DecisionReply::SelectResolution(vec![physical(&assign, 10).id()]),
    )
    .into_state();
    assert_eq!(divided.pending_decision().unwrap().chooser(), A);
    let bottom = reply(&divided, DecisionReply::ChoosePile(1)).into_state();
    let done = reply(
        &bottom,
        DecisionReply::OrderResolutionGraveyard(vec![
            physical(&bottom, 12).id(),
            physical(&bottom, 11).id(),
        ]),
    )
    .into_state();
    assert_eq!(physical(&done, 10).zone(), ObjectZone::Hand(A));
    let library = done.execution().window().cards().library(A);
    assert_eq!(
        &library[library.len() - 2..],
        &[physical(&done, 12).id(), physical(&done, 11).id()]
    );
    let other = reply(&divided, DecisionReply::ChoosePile(2)).into_state();
    for n in [11, 12] {
        assert_eq!(physical(&other, n).zone(), ObjectZone::Hand(A));
    }
    let mut search =
        round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    search = super::library_walkers::entropy(search);
    for (pick, land) in [(Some(10), 11), (Some(20), 21), (None, 30)] {
        let offer = |n: Option<u64>| {
            DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Search {
                selected: n.map(|id| physical(&search, id).id()),
            })
        };
        invalid(&search, A, offer(Some(land)));
        let found = reply(&search, offer(pick));
        assert_eq!(found.shuffle_journal().len(), 1);
        search = found.into_state();
        if let Some(id) = pick {
            assert_eq!(physical(&search, id).zone(), ObjectZone::Exile);
        }
    }
    let done = reply(&search, DecisionReply::SelectResolution(vec![])).into_state();
    assert!(done.execution().window().entries().is_empty());
    let chosen = reply(
        &search,
        DecisionReply::SelectResolution(vec![physical(&search, 20).id()]),
    )
    .into_state();
    let stack = reply(
        &chosen,
        DecisionReply::ImpulseCast {
            target: None,
            payment: Default::default(),
            mana_abilities: vec![],
        },
    )
    .into_state();
    let stopped = reply(&stack, DecisionReply::SelectResolution(vec![])).into_state();
    let cast = round(stopped).into_state();
    assert_eq!(physical(&cast, 20).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&cast, 20).owner(), B);
    assert_eq!(physical(&cast, 20).controller(), Some(A));
    assert_eq!(physical(&cast, 10).zone(), ObjectZone::Exile);
}
