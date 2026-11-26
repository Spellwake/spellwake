//! Named planeswalker games covering loyalty choices, combat and delayed effects.
use super::super::declarations::{attacks, blocks, combat_on, invalid};
use super::*;

fn targeted_loyalty(s: &GameState, card: u64, index: u64, target: TargetRef) -> GameCommand {
    GameCommand::ActivateAbility {
        source: physical(s, card).id(),
        ability: ActivatedAbilityId(index),
        target,
        payment: None,
    }
}
pub(super) fn permanent(s: &GameState, n: u64) -> TargetRef {
    TargetRef::Object(physical(s, n).id())
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_gideon_jura_requires_attacks_on_him_destroys_only_tapped_creatures_and_prevents_damage_when_animated()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 2)];
    let mut tapper = creature(2, A, 2, 2, &[KeywordAbility::Haste]);
    tapper
        .program
        .mana
        .push(spellwake_engine::mana::ManaAbility {
            id: ManaAbilityId(0),
            output: spellwake_engine::mana::ManaOutput::Fixed([(Green, 1.into())].into()),
        });
    let game = game(
        vec![A, B, C],
        vec![
            named("Gideon Jura", 1, A, ObjectDestination::Hand),
            tapper,
            creature(3, B, 2, 3, &[]),
            creature(4, C, 2, 3, &[]),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SingleAnyTarget,
                    Instruction::Damage { amount: 8.into() },
                ),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let gideon = round(cast_untargeted(&game, 1, &price).into_state()).into_state();
    assert_eq!(loyalty(&gideon, 1), 6);
    illegal(
        &gideon,
        A,
        targeted_loyalty(&gideon, 1, 0, TargetRef::Player(A)),
    );
    let marked = round(
        command(
            &gideon,
            A,
            targeted_loyalty(&gideon, 1, 0, TargetRef::Player(B)),
        )
        .into_state(),
    )
    .into_state();
    let b = combat_on(marked, 2);
    for declarations in [vec![], vec![(3, A, None)], vec![(3, C, None)]] {
        invalid(&b, B, attacks(&b, &declarations));
    }
    let hit = reply(&b, attacks(&b, &[(3, A, Some(1))])).into_state();
    let c = combat_on(hit, 3);
    assert_eq!(loyalty(&c, 1), 6);
    let free = reply(&c, attacks(&c, &[(4, B, None)])).into_state();
    assert_eq!(physical(&free, 4).tapped(), Some(true));
    illegal(
        &gideon,
        A,
        targeted_loyalty(&gideon, 1, 1, permanent(&gideon, 3)),
    );
    let tapped = tap_source(&gideon, 2);
    let dead = round(
        command(
            &tapped,
            A,
            targeted_loyalty(&tapped, 1, 1, permanent(&tapped, 2)),
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(loyalty(&dead, 1), 4);
    assert_eq!(physical(&dead, 2).zone(), ObjectZone::Graveyard(A));
    let animated =
        round(command(&gideon, A, activate(&gideon, 1, 2, &[])).into_state()).into_state();
    let obj = physical(&animated, 1).id();
    let face = animated
        .execution()
        .current_characteristics(obj)
        .unwrap()
        .unwrap();
    assert!(face.is_creature() && face.has_type(CardType::Planeswalker));
    assert_eq!(
        face.current_creature_subtypes,
        ["Human", "Soldier"]
            .map(|name| current_catalogue().1.resolve_label(name).unwrap())
            .into()
    );
    assert_eq!(
        face.power_toughness.unwrap().derived,
        PowerToughness {
            power: 6.into(),
            toughness: 6.into()
        }
    );
    let safe = round(targeted(&animated, A, 5, TargetRef::Object(obj)).into_state()).into_state();
    assert_eq!(loyalty(&safe, 1), 6);
    let next = exile_hand::until(safe, |s| s.turn().unwrap().position().number == 2);
    assert!(
        !next
            .execution()
            .current_characteristics(obj)
            .unwrap()
            .unwrap()
            .is_creature()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_vronos_phases_only_other_own_walkers_bounces_one_per_opponent_and_permanently_animates_its_artifact()
 {
    use ManaType::*;
    let jace = [(Colourless, 1), (Blue, 2)];
    let wanderer = [(Colourless, 3), (White, 1)];
    let base = with_loyalty_for_ultimate(
        "Vronos, Masked Inquisitor",
        &[(Colourless, 3), (Blue, 2)],
        5,
        7,
        vec![
            named("Jace Beleren", 2, A, ObjectDestination::Hand),
            named("The Wanderer", 3, A, ObjectDestination::Hand),
            mana_source(4, &[(Colourless, 4), (Blue, 2), (White, 1)]),
            seed(5, A, battlefield(A), Program::artifact(None, vec![])),
            seed(6, C, battlefield(B), Program::artifact(None, vec![])),
            creature(7, B, 2, 3, &[]),
            creature(8, C, 2, 3, &[]),
            creature(9, A, 2, 3, &[]),
            named("Plains", 10, B, battlefield(B)),
            seed(
                11,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
        ],
    );
    let jace = round(cast_untargeted(&tap_source(&base, 4), 2, &jace).into_state()).into_state();
    let ready = round(cast_untargeted(&jace, 3, &wanderer).into_state()).into_state();
    illegal(&ready, A, selected_from(&ready, 0, &[1]));
    illegal(&ready, A, selected_from(&ready, 0, &[5]));
    for targets in [&[][..], &[2, 3][..]] {
        let waiting =
            round(command(&ready, A, selected_from(&ready, 0, targets)).into_state()).into_state();
        let captured: Vec<_> = [2, 3].map(|n| physical(&ready, n).id()).into();
        let hidden = exile_hand::until(waiting, |s| s.turn().unwrap().position().number == 2);
        for (&n, &object) in [2, 3].iter().zip(&captured) {
            assert_eq!(
                hidden.execution().window().cards().phased_out_under(object),
                targets.contains(&n).then_some(A)
            );
        }
        let back = exile_hand::until(hidden, |s| {
            s.turn().unwrap().position().number == 4
                && s.turn().unwrap().position().step == Step::Upkeep
        });
        for (n, id) in [2, 3].into_iter().zip(captured) {
            assert_eq!(physical(&back, n).id(), id);
        }
    }
    for invalid in [&[6, 7][..], &[5][..], &[10][..]] {
        illegal(&ready, A, selected_from(&ready, 1, invalid));
    }
    let without =
        round(command(&ready, A, selected_from(&ready, 1, &[])).into_state()).into_state();
    assert_eq!(physical(&without, 6).zone(), ObjectZone::Battlefield);
    let bounced =
        round(command(&ready, A, selected_from(&ready, 1, &[6, 8])).into_state()).into_state();
    assert_eq!(physical(&bounced, 6).zone(), ObjectZone::Hand(C));
    assert_eq!(physical(&bounced, 8).zone(), ObjectZone::Hand(C));
    let ultimate = |s: &GameState, id| targeted_loyalty(s, 1, 2, permanent(s, id));
    illegal(&ready, A, ultimate(&ready, 6));
    let artifact = round(command(&ready, A, ultimate(&ready, 5)).into_state()).into_state();
    assert_eq!(physical(&artifact, 1).zone(), ObjectZone::Graveyard(A));
    let ward = round(targeted(&artifact, A, 11, permanent(&artifact, 5)).into_state()).into_state();
    let attack = combat_on(ward, 4);
    let sheet = attack
        .execution()
        .current_characteristics(physical(&attack, 5).id())
        .unwrap()
        .unwrap();
    assert!(sheet.is_creature() && sheet.has_type(CardType::Artifact));
    assert!(
        sheet.has_keyword(KeywordAbility::Vigilance)
            && sheet.has_keyword(KeywordAbility::Indestructible)
    );
    assert_eq!(
        sheet.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Construct").unwrap()].into()
    );
    assert_eq!(
        sheet.power_toughness.unwrap().derived,
        PowerToughness {
            power: 9.into(),
            toughness: 9.into()
        }
    );
    let swinging = reply(&attack, attacks(&attack, &[(5, B, None), (9, B, None)])).into_state();
    assert_eq!(physical(&swinging, 5).tapped(), Some(false));
    let block = combat_frame(&swinging, false);
    invalid(&block, B, blocks(&block, &[(7, 5)]));
    let blocked = reply(&block, blocks(&block, &[(7, 9)])).into_state();
    let end = exile_hand::until(blocked, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(end.execution().life_total(B), Some(&11.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sarkhan_makes_and_animates_dragons_and_each_dragon_hits_attackers_of_controller_or_walkers()
{
    use ManaType::*;
    let price = [(Colourless, 3), (Red, 2)];
    let blue = [(Colourless, 1), (Blue, 2)];
    let game = game(
        vec![A, B, C],
        vec![
            named("Sarkhan the Masterless", 1, A, ObjectDestination::Hand),
            named("Jace Beleren", 2, A, ObjectDestination::Hand),
            mana_source(3, &blue),
            creature(4, B, 2, 1, &[]),
            creature(5, B, 2, 1, &[]),
            creature(6, B, 2, 1, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let sarkhan = round(cast_untargeted(&game, 1, &price).into_state()).into_state();
    let ready =
        round(cast_untargeted(&tap_source(&sarkhan, 3), 2, &blue).into_state()).into_state();
    let dragon = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state());
    let token = tokens(&dragon, A, 1)[0];
    let dragon_type = current_catalogue().1.resolve_label("Dragon").unwrap();
    let sheet = dragon
        .state()
        .execution()
        .current_characteristics(token)
        .unwrap()
        .unwrap();
    assert_eq!(sheet.current_creature_subtypes, [dragon_type].into());
    assert_eq!(sheet.current_colors, [Color::Red].into());
    assert!(sheet.has_keyword(KeywordAbility::Flying));
    assert_eq!(
        sheet.power_toughness.unwrap().derived,
        PowerToughness {
            power: 4.into(),
            toughness: 4.into()
        }
    );
    let b = combat_on(dragon.into_state(), 2);
    let declared = reply(
        &b,
        attacks(&b, &[(4, A, None), (5, A, Some(2)), (6, C, None)]),
    )
    .into_state();
    let RuleFrame::Triggers(order) = declared.pending_decision().unwrap().frame() else {
        panic!("matched two attackers");
    };
    assert_eq!(order.instances().len(), 2);
    let placed = reply(
        &declared,
        DecisionReply::OrderTriggers(order.instances().to_vec()),
    )
    .into_state();
    let end = exile_hand::until(placed, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    for n in [4, 5] {
        assert_eq!(physical(&end, n).zone(), ObjectZone::Graveyard(B));
    }
    assert_eq!(physical(&end, 6).zone(), ObjectZone::Battlefield);
    assert_eq!(end.execution().life_total(C), Some(&18.into()));
    assert_eq!(loyalty(&end, 2), 3);
    let animated = round(command(&ready, A, activate(&ready, 1, 1, &[])).into_state()).into_state();
    for n in [1, 2] {
        let sheet = animated
            .execution()
            .current_characteristics(physical(&animated, n).id())
            .unwrap()
            .unwrap();
        assert!(sheet.is_creature() && !sheet.has_type(CardType::Planeswalker));
        assert_eq!(sheet.current_creature_subtypes, [dragon_type].into());
        assert_eq!(sheet.current_colors, [Color::Red].into());
        assert!(sheet.has_keyword(KeywordAbility::Flying));
        assert_eq!(
            sheet.power_toughness.unwrap().derived,
            PowerToughness {
                power: 4.into(),
                toughness: 4.into()
            }
        );
    }
    let next = exile_hand::until(animated, |s| s.turn().unwrap().position().number == 2);
    for n in [1, 2] {
        assert!(
            next.execution()
                .current_characteristics(physical(&next, n).id())
                .unwrap()
                .unwrap()
                .has_type(CardType::Planeswalker)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_chandra_legacy_counts_controlled_walkers_for_mana_and_damage_and_limits_exile_permission_to_the_turn()
 {
    use ManaType::*;
    let price = [(Colourless, 4), (Red, 1)];
    let blue = [(Colourless, 1), (Blue, 2)];
    let game = game(
        vec![A, B, C],
        vec![
            named("Chandra, Legacy of Fire", 1, A, ObjectDestination::Hand),
            named("Jace Beleren", 2, A, ObjectDestination::Hand),
            mana_source(3, &blue),
            named("Plains", 4, A, ObjectDestination::LibraryBottom),
            seed(
                5,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
            seed(
                6,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let chandra = round(cast_untargeted(&game, 1, &price).into_state()).into_state();
    let only = round(command(&chandra, A, activate(&chandra, 1, 1, &[])).into_state()).into_state();
    assert_eq!(only.execution().mana(A, Red), 1.into());
    let ready =
        round(cast_untargeted(&tap_source(&chandra, 3), 2, &blue).into_state()).into_state();
    let mana = round(command(&ready, A, activate(&ready, 1, 1, &[])).into_state()).into_state();
    assert_eq!(mana.execution().mana(A, Red), 2.into());
    let after = exile_hand::until(mana, |s| s.turn().unwrap().position().number == 2);
    for opponent in [B, C] {
        assert_eq!(after.execution().life_total(opponent), Some(&18.into()));
    }
    assert_eq!(after.execution().life_total(A), Some(&20.into()));
    let offer = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    let pick = |ids: &[u64]| {
        DecisionReply::SelectResolution(ids.iter().map(|&n| physical(&offer, n).id()).collect())
    };
    let zero = reply(&offer, pick(&[])).into_state();
    assert_eq!(loyalty(&zero, 1), 3);
    assert_eq!(physical(&zero, 4).zone(), ObjectZone::Library(A));
    let exiled = reply(&offer, pick(&[1, 2])).into_state();
    for n in [1, 2] {
        assert_eq!(loyalty(&exiled, n), 2);
    }
    for n in [4, 5] {
        assert_eq!(physical(&exiled, n).zone(), ObjectZone::Exile);
    }
    let land = command(
        &exiled,
        A,
        GameCommand::PlayLand {
            land: physical(&exiled, 4).id(),
        },
    )
    .into_state();
    assert_eq!(physical(&land, 4).zone(), ObjectZone::Battlefield);
    let spell = round(cast_untargeted(&land, 5, &[]).into_state()).into_state();
    assert_eq!(physical(&spell, 5).zone(), ObjectZone::Battlefield);
    let old = exile_hand::until(exiled, |s| {
        s.turn().unwrap().position().number == 4
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    for n in [4, 5] {
        assert_eq!(physical(&old, n).zone(), ObjectZone::Exile);
    }
    illegal(
        &old,
        A,
        GameCommand::PlayLand {
            land: physical(&old, 4).id(),
        },
    );
    illegal(
        &old,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&old, 5).id(),
            payment: None,
        },
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_jace_mirror_mage_kicker_creates_an_independent_nonlegendary_walker_and_each_loyalty_ability_resolves()
 {
    use ManaType::*;
    for kicked in [false, true] {
        let price = [(Colourless, if kicked { 3 } else { 1 }), (Blue, 2)];
        let start = game(
            vec![A, B],
            vec![
                named("Jace, Mirror Mage", 1, A, ObjectDestination::Hand),
                named("Sol Ring", 2, A, ObjectDestination::LibraryBottom),
                named("Plains", 3, A, ObjectDestination::LibraryBottom),
            ],
            &price,
            Step::PrecombatMain,
        );
        let cast = command(
            &start,
            A,
            GameCommand::CastWithResources {
                spell: physical(&start, 1).id(),
                target: None,
                payment: CastingPayment {
                    mana: Some(spend(&price)),
                    additional: if kicked {
                        [(0, 1.into())].into()
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            },
        );
        let entry = round(cast.into_state()).into_state();
        assert_eq!(loyalty(&entry, 1), 4);
        let ready = if kicked {
            round(entry).into_state()
        } else {
            entry
        };
        let copies: Vec<_> = ready
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
            .map(|o| o.id())
            .collect();
        assert_eq!(copies.len(), usize::from(kicked));
        if let Some(&copy) = copies.first() {
            let sheet = ready
                .execution()
                .current_characteristics(copy)
                .unwrap()
                .unwrap();
            assert!(sheet.has_type(CardType::Planeswalker));
            assert!(
                !sheet
                    .current_supertypes
                    .contains(&spellwake_engine::program::Supertype::Legendary)
            );
            assert_eq!(
                ready.execution().loyalty_counters().get(&copy),
                Some(&1.into())
            );
            let paid = round(
                command(
                    &ready,
                    A,
                    GameCommand::ActivateUntargetedAbility {
                        source: copy,
                        ability: ActivatedAbilityId(3),
                        payment: None,
                    },
                )
                .into_state(),
            )
            .into_state();
            assert_eq!(physical(&paid, 2).zone(), ObjectZone::Hand(A));
            assert!(paid.execution().window().cards().get(copy).is_none());
            assert_eq!(loyalty(&paid, 1), 4);
        }
        let drawn = round(command(&ready, A, activate(&ready, 1, 3, &[])).into_state());
        assert_eq!(loyalty(drawn.state(), 1), 3);
        assert_eq!(physical(drawn.state(), 2).zone(), ObjectZone::Hand(A));
        assert!(
            events(&drawn)
                .flat_map(|event| &event.occurrences)
                .any(|row| matches!(row.value, RulesOccurrence::HandCardRevealed { .. }))
        );
        let scry = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
        assert_eq!(loyalty(&scry, 1), 5);
        let library = scry.execution().window().cards().library(A);
        assert_eq!(library[0], physical(&scry, 2).id());
        assert_eq!(library[1], physical(&scry, 3).id());
        let result = reply(
            &scry,
            DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Scry {
                top_first: vec![library[1]],
                bottom_top_first: vec![library[0]],
            }),
        )
        .into_state();
        let reordered = result.execution().window().cards().library(A);
        assert_eq!(reordered.first(), Some(&physical(&result, 3).id()));
        assert_eq!(reordered.last(), Some(&physical(&result, 2).id()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_commodore_guff_counters_another_walker_creates_a_restricted_mana_wizard_and_counts_walkers_for_each_opponent()
 {
    use ManaType::*;
    use spellwake_engine::mana::{ManaRestriction, RestrictedManaSpend};
    verify_commander_deck("Commodore Guff", "Plains");
    let price = [(Colourless, 1), (Blue, 1), (Red, 1), (White, 1)];
    let blue = [(Colourless, 1), (Blue, 2)];
    let start = game(
        vec![A, B, C],
        vec![
            named("Commodore Guff", 1, A, ObjectDestination::Hand),
            named("Jace Beleren", 2, A, ObjectDestination::Hand),
            mana_source(3, &blue),
            seed(
                4,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                5,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            named("Sol Ring", 6, A, ObjectDestination::Hand),
            named("Chandra, Legacy of Fire", 7, A, ObjectDestination::Hand),
            mana_source(8, &[(Colourless, 4)]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let guff = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let ready = round(cast_untargeted(&tap_source(&guff, 3), 2, &blue).into_state()).into_state();
    let drawn = round(command(&ready, A, activate(&ready, 1, 2, &[])).into_state()).into_state();
    assert_eq!(loyalty(&drawn, 1), 2);
    for id in [4, 5] {
        assert_eq!(physical(&drawn, id).zone(), ObjectZone::Hand(A));
    }
    for seat in [B, C] {
        assert_eq!(drawn.execution().life_total(seat), Some(&18.into()));
    }
    let wizard = round(command(&ready, A, activate(&ready, 1, 1, &[])).into_state());
    let token = tokens(&wizard, A, 1)[0];
    let wizard = wizard.into_state();
    let view = wizard
        .execution()
        .current_characteristics(token)
        .unwrap()
        .unwrap();
    assert_eq!(view.current_colors, [Color::Red].into());
    assert_eq!(
        view.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Wizard").unwrap()].into()
    );
    assert_eq!(
        view.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    let end = exile_hand::until(wizard, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::TriggerTarget(_))
        )
    });
    invalid(&end, A, DecisionReply::TriggerTarget(permanent(&end, 1)));
    let target = reply(&end, DecisionReply::TriggerTarget(permanent(&end, 2))).into_state();
    let given = round(target).into_state();
    assert_eq!(loyalty(&given, 2), 4);
    assert_eq!(loyalty(&given, 1), 6);
    let main = exile_hand::until(given, |s| {
        s.turn().unwrap().position().number == 4
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let red = command(
        &main,
        A,
        GameCommand::ActivateMana {
            source: token,
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let restriction = ManaRestriction::PlaneswalkerSpell;
    assert_eq!(
        red.execution()
            .restricted_mana()
            .get(&(A, Red, restriction.clone())),
        Some(&1.into())
    );
    let cast = |s: &GameState, n, generic| GameCommand::CastWithResources {
        spell: physical(s, n).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(if generic {
                &[(Colourless, 4), (Red, 1)]
            } else {
                &[(Red, 1)]
            })),
            restricted_mana: vec![RestrictedManaSpend {
                mana_type: Red,
                restriction: restriction.clone(),
                amount: 1.into(),
            }],
            ..Default::default()
        },
    };
    illegal(&red, A, cast(&red, 6, false));
    let mana = tap_source(&red, 8);
    let next = round(command(&mana, A, cast(&mana, 7, true)).into_state()).into_state();
    assert_eq!(loyalty(&next, 7), 3);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_chandra_awakened_inferno_resists_countering_grants_separate_emblems_and_exiles_only_damaged_permanents()
 {
    use ManaType::*;
    let price = [(Colourless, 4), (Red, 2)];
    let elemental = Program::creature(
        None,
        2.into(),
        2.into(),
        [current_catalogue().1.resolve_label("Elemental").unwrap()].into(),
    );
    let game = game(
        vec![A, B, C],
        vec![
            named("Chandra, Awakened Inferno", 1, A, ObjectDestination::Hand),
            named("Counterspell", 2, A, ObjectDestination::Hand),
            seed(3, B, battlefield(B), elemental),
            creature(4, B, 2, 3, &[]),
            creature(5, C, 6, 6, &[]),
            creature(6, A, 1, 2, &[]),
            seed(
                7,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
            named("Jace Beleren", 8, A, ObjectDestination::Hand),
            mana_source(9, &[(Colourless, 1), (Blue, 2)]),
        ],
        &[(Colourless, 4), (Red, 2), (Blue, 2)],
        Step::PrecombatMain,
    );
    let spell = cast_untargeted(&game, 1, &price).into_state();
    let entry = spell.execution().window().entries()[0].id();
    let counter = command(
        &spell,
        A,
        GameCommand::CastPaid {
            spell: physical(&spell, 2).id(),
            target: TargetRef::Stack(entry),
            payment: spend(&[(Blue, 2)]),
        },
    )
    .into_state();
    let survived = round(counter).into_state();
    assert_eq!(physical(&survived, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&survived, 1).zone(), ObjectZone::Stack);
    let walker = round(survived).into_state();
    let emblems =
        round(command(&walker, A, activate(&walker, 1, 1, &[])).into_state()).into_state();
    assert_eq!(emblems.execution().emblems().count(), 2);
    assert_eq!(loyalty(&emblems, 1), 8);
    let during = exile_hand::until(emblems, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_eq!(during.execution().life_total(B), Some(&19.into()));
    assert_eq!(during.execution().life_total(C), Some(&20.into()));
    let later = exile_hand::until(during, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert_eq!(later.execution().life_total(C), Some(&19.into()));
    assert_eq!(later.execution().life_total(A), Some(&20.into()));
    let swept = round(command(&walker, A, activate(&walker, 1, 2, &[])).into_state()).into_state();
    for (n, zone) in [
        (3, ObjectZone::Battlefield),
        (4, ObjectZone::Graveyard(B)),
        (5, ObjectZone::Battlefield),
        (6, ObjectZone::Graveyard(A)),
    ] {
        assert_eq!(physical(&swept, n).zone(), zone);
    }
    let marked = |s: &GameState, n| {
        s.execution()
            .battlefield_marked_damage(physical(s, n).id())
            .unwrap_or_else(|| 0.into())
    };
    assert_eq!(marked(&swept, 5), 3.into());
    assert_eq!(marked(&swept, 3), 0.into());
    let fire = |s: &GameState, n, x: i64| GameCommand::ActivateLoyaltyX {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(3),
        chosen_x: x.into(),
        target: permanent(s, n),
        payment: None,
    };
    illegal(&walker, A, fire(&walker, 4, 7));
    let dead = round(command(&walker, A, fire(&walker, 4, 3)).into_state()).into_state();
    assert_eq!(physical(&dead, 4).zone(), ObjectZone::Exile);
    for (x, zone) in [(0, ObjectZone::Graveyard(C)), (1, ObjectZone::Exile)] {
        let hit = round(command(&walker, A, fire(&walker, 5, x)).into_state()).into_state();
        assert_eq!(marked(&hit, 5), x.into());
        let destroyed = round(targeted(&hit, A, 7, permanent(&hit, 5)).into_state()).into_state();
        assert_eq!(physical(&destroyed, 5).zone(), zone);
    }
    let jace = round(
        cast_untargeted(&tap_source(&walker, 9), 8, &[(Colourless, 1), (Blue, 2)]).into_state(),
    )
    .into_state();
    let exile = round(command(&jace, A, fire(&jace, 8, 3)).into_state()).into_state();
    assert_eq!(physical(&exile, 8).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_chandra_torch_of_defiance_casts_or_burns_adds_red_damages_creatures_and_keeps_a_spell_cast_emblem()
 {
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice;
    let base = with_loyalty_for_ultimate(
        "Chandra, Torch of Defiance",
        &[(Colourless, 2), (Red, 2)],
        4,
        7,
        vec![
            seed(
                2,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
            creature(3, B, 3, 4, &[]),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
        ],
    );
    let impulse = round(command(&base, A, activate(&base, 1, 0, &[])).into_state()).into_state();
    assert_eq!(physical(&impulse, 2).zone(), ObjectZone::Exile);
    let decline = reply(
        &impulse,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    for seat in [B, C] {
        assert_eq!(decline.execution().life_total(seat), Some(&18.into()));
    }
    let cast = reply(
        &impulse,
        DecisionReply::ImpulseCast {
            target: None,
            payment: Default::default(),
            mana_abilities: vec![],
        },
    )
    .into_state();
    let resolved = round(cast).into_state();
    assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Battlefield);
    for seat in [B, C] {
        assert_eq!(resolved.execution().life_total(seat), Some(&20.into()));
    }
    let mana = round(command(&base, A, activate(&base, 1, 1, &[])).into_state()).into_state();
    assert_eq!(mana.execution().mana(A, Red), 2.into());
    illegal(
        &base,
        A,
        targeted_loyalty(&base, 1, 2, TargetRef::Player(B)),
    );
    let burned =
        round(command(&base, A, targeted_loyalty(&base, 1, 2, permanent(&base, 3))).into_state())
            .into_state();
    assert_eq!(physical(&burned, 3).zone(), ObjectZone::Graveyard(B));
    let emblem = round(command(&base, A, activate(&base, 1, 3, &[])).into_state()).into_state();
    assert_eq!(physical(&emblem, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(emblem.execution().emblems().count(), 1);
    let choosing = cast_untargeted(&emblem, 4, &[]).into_state();
    for target in [TargetRef::Player(C), permanent(&choosing, 3)] {
        let stack = reply(&choosing, DecisionReply::TriggerTarget(target)).into_state();
        let done = round(stack).into_state();
        if target == TargetRef::Player(C) {
            assert_eq!(done.execution().life_total(C), Some(&15.into()));
        } else {
            assert_eq!(physical(&done, 3).zone(), ObjectZone::Graveyard(B));
        }
        assert_eq!(physical(&done, 4).zone(), ObjectZone::Stack);
    }
}
