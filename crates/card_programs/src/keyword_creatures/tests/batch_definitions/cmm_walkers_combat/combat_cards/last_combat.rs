//! Multiplayer attack destinations and power-based damage.
use super::*;
fn stock(cards: &mut Vec<SeedCard>) {
    for (owner, base) in [(A, 60), (B, 65), (C, 70)] {
        for id in base..base + 4 {
            cards.push(named("Island", id, owner, ObjectDestination::LibraryBottom));
        }
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_coveted_peacock_flies_and_optionally_goads_only_a_creature_of_its_defender_until_its_controllers_next_turn()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 2)];
    let mut cards = vec![
        named("Coveted Peacock", 1, A, ObjectDestination::Hand),
        creature(2, B, 2, 3, &[]),
        creature(3, C, 2, 3, &[]),
    ];
    stock(&mut cards);
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let field = cast_card(&initial, 1, &price);
    assert!(sheet(&field, physical(&field, 1).id()).has_keyword(KeywordAbility::Flying));
    let combat = combat_on(field, 4);
    let target = reply(&combat, attacks(&combat, &[(1, B, None)])).into_state();
    invalid(&target, A, DecisionReply::TriggerTarget(marker(&target, 3)));
    let voted = reply(&target, DecisionReply::TriggerTarget(marker(&target, 2))).into_state();
    let offer = round(voted).into_state();
    let skipped = super::super::equipment::optional(&offer, false);
    let next = combat_on(skipped, 5);
    let _ = reply(&next, attacks(&next, &[]));
    let goaded = super::super::equipment::optional(&offer, true);
    let forced = combat_on(goaded, 5);
    invalid(&forced, B, attacks(&forced, &[]));
    invalid(&forced, B, attacks(&forced, &[(2, A, None)]));
    let obeyed = reply(&forced, attacks(&forced, &[(2, C, None)])).into_state();
    let expired = combat_on(obeyed, 8);
    let _ = reply(&expired, attacks(&expired, &[]));
    let _ = reply(&expired, attacks(&expired, &[(2, A, None)]));
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_frontier_warmonger_groups_attackers_by_defender_and_grants_menace_to_any_player_attacking_its_opponents()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Red, 1)];
    let jace = [(Colourless, 1), (Blue, 2)];
    let mut blue = super::super::loyalty::mana_source(11, &jace);
    blue.owner = B;
    blue.destination = battlefield(B);
    let mut cards = vec![
        named("Frontier Warmonger", 1, A, ObjectDestination::Hand),
        creature(2, A, 1, 2, &[KeywordAbility::Haste]),
        creature(3, A, 2, 3, &[KeywordAbility::Haste]),
        creature(4, B, 1, 3, &[]),
        creature(5, B, 2, 3, &[]),
        creature(6, C, 1, 4, &[]),
        named("Jace Beleren", 7, A, ObjectDestination::Hand),
        named("Jace Beleren", 8, B, ObjectDestination::Hand),
        blue,
    ];
    stock(&mut cards);
    let initial = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 4), (Red, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let field = cast_card(&initial, 1, &price);
    let walker = cast_card(&field, 7, &jace);
    let combat = combat_on(walker, 1);
    let attack = reply(&combat, attacks(&combat, &[(2, B, None), (3, B, None)])).into_state();
    assert_eq!(attack.execution().window().entries().len(), 1);
    let menacing = round(attack).into_state();
    for n in [2, 3] {
        assert!(sheet(&menacing, physical(&menacing, n).id()).has_keyword(KeywordAbility::Menace));
    }
    let block = blockers(menacing);
    invalid(&block, B, blocks(&block, &[(4, 2)]));
    let _ = reply(&block, blocks(&block, &[(4, 2), (5, 2)]));
    let next = next_main(reply(&block, blocks(&block, &[])).into_state(), 2);
    assert!(!sheet(&next, physical(&next, 2).id()).has_keyword(KeywordAbility::Menace));
    let blue = command(
        &next,
        B,
        GameCommand::ActivateMana {
            source: physical(&next, 11).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let enemywalker = round(
        command(
            &blue,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&blue, 8).id(),
                payment: Some(spend(&jace)),
            },
        )
        .into_state(),
    )
    .into_state();
    let b = combat_on(enemywalker, 2);
    let both = reply(&b, attacks(&b, &[(4, A, Some(7)), (5, C, None)])).into_state();
    assert_eq!(both.execution().window().entries().len(), 1);
    let grant = round(both).into_state();
    for (n, menace) in [(4, false), (5, true)] {
        assert_eq!(
            sheet(&grant, physical(&grant, n).id()).has_keyword(KeywordAbility::Menace),
            menace
        );
    }
    let again = combat_on(grant, 4);
    let vs_walker = reply(&again, attacks(&again, &[(2, B, Some(8))])).into_state();
    let granted = round(vs_walker).into_state();
    assert!(sheet(&granted, physical(&granted, 2).id()).has_keyword(KeywordAbility::Menace));
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spikeshot_goblin_pays_red_and_taps_to_deal_its_power_at_resolution_to_each_commander_masters_target_kind()
 {
    use ManaType::*;
    let goblin = named("Spikeshot Goblin", 1, A, ObjectDestination::Hand);
    let ability = goblin.program.activated[0].id;
    let mut cards = vec![
        goblin,
        named("Bonesplitter", 2, A, battlefield(A)),
        creature(3, B, 1, 4, &[]),
        named("Jace Beleren", 4, A, ObjectDestination::Hand),
        named("Mountain", 5, A, battlefield(A)),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_PERMANENT,
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    ),
                ),
            ),
        ),
    ];
    stock(&mut cards);
    let initial = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 4), (Red, 1), (Blue, 2)],
        Step::PrecombatMain,
    );
    let cast = cast_card(&initial, 1, &[(Colourless, 2), (Red, 1)]);
    let walker = cast_card(&cast, 4, &[(Colourless, 1), (Blue, 2)]);
    let armed = wield(&walker, physical(&walker, 2).id(), 1, 1);
    let ready = next_main(armed, 4);
    let mana = command(
        &ready,
        A,
        GameCommand::ActivateMana {
            source: physical(&ready, 5).id(),
            ability: ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Mountain),
        },
    )
    .into_state();
    let goblin = physical(&mana, 1).id();
    let ability_to = |t| GameCommand::ActivateAbility {
        source: goblin,
        ability,
        target: t,
        payment: Some(spend(&[(Red, 1)])),
    };
    for t in [TargetRef::Player(B), marker(&mana, 3), marker(&mana, 4)] {
        let aimed = command(&mana, A, ability_to(t)).into_state();
        assert_eq!(physical(&aimed, 1).tapped(), Some(true));
        assert_eq!(aimed.execution().mana(A, Red), 0.into());
        let hit = round(aimed.clone());
        assert_eq!(hit.damage_journal().applied.len(), 1);
        let row = &hit.damage_journal().applied[0];
        assert_eq!(row.source.id(), DamageSourceId::Object(goblin));
        assert_eq!(row.amount, 3.into());
        match t {
            TargetRef::Player(_) => {
                assert_eq!(hit.state().execution().life_total(B), Some(&17.into()))
            }
            _ if t == marker(&mana, 3) => assert_eq!(
                hit.state()
                    .execution()
                    .battlefield_marked_damage(physical(hit.state(), 3).id()),
                Some(3.into())
            ),
            _ => assert_eq!(physical(hit.state(), 4).zone(), ObjectZone::Graveyard(A)),
        }
        if t == TargetRef::Player(B) {
            let dropped =
                round(targeted(&aimed, A, 6, marker(&aimed, 2)).into_state()).into_state();
            assert_eq!(
                round(dropped).state().execution().life_total(B),
                Some(&19.into())
            );
        }
    }
}
