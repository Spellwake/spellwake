//! Cast choices, conditional resolution and grouped effects.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_cyclonic_rift_and_vandalblast_replace_the_single_target_with_each_eligible_opposing_permanent_when_overloaded()
 {
    use ManaType::*;
    for (name, normal, overloaded, color, rift) in [
        ("Cyclonic Rift", 1, 6, Blue, true),
        ("Vandalblast", 0, 4, Red, false),
    ] {
        let mut protected = seed(4, C, battlefield(C), Program::artifact(None, vec![]));
        protected
            .program
            .printed_keywords
            .insert(KeywordAbility::Hexproof);
        let initial = game(
            vec![A, B, C],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                named("Sol Ring", 2, A, battlefield(A)),
                named("Sol Ring", 3, B, battlefield(B)),
                protected,
                creature(5, B, 2, 2, &[]),
                named("Island", 6, C, battlefield(C)),
            ],
            &[(Colourless, overloaded), (color, 1)],
            Step::PrecombatMain,
        );
        let cmd = |many: bool, n: Option<u64>| {
            let generic = if many { overloaded } else { normal };
            let mut price = vec![(color, 1)];
            if generic > 0 {
                price.push((Colourless, generic));
            }
            GameCommand::CastWithResources {
                spell: physical(&initial, 1).id(),
                target: n.map(|id| marker(&initial, id)),
                payment: CastingPayment {
                    use_alternative: many,
                    mana: Some(spend(&price)),
                    ..Default::default()
                },
            }
        };
        for n in [2, 4, 6] {
            illegal(&initial, A, cmd(false, Some(n)));
        }
        if !rift {
            illegal(&initial, A, cmd(false, Some(5)));
        }
        let one = round(command(&initial, A, cmd(false, Some(3))).into_state()).into_state();
        assert_eq!(
            physical(&one, 3).zone(),
            if rift {
                ObjectZone::Hand(B)
            } else {
                ObjectZone::Graveyard(B)
            }
        );
        assert_eq!(physical(&one, 4).zone(), ObjectZone::Battlefield);
        illegal(&initial, A, cmd(true, Some(3)));
        let mass = round(command(&initial, A, cmd(true, None)).into_state()).into_state();
        for (n, owner) in [(3, B), (4, C)] {
            assert_eq!(
                physical(&mass, n).zone(),
                if rift {
                    ObjectZone::Hand(owner)
                } else {
                    ObjectZone::Graveyard(owner)
                }
            );
        }
        assert_eq!(
            physical(&mass, 5).zone(),
            if rift {
                ObjectZone::Hand(B)
            } else {
                ObjectZone::Battlefield
            }
        );
        assert_eq!(physical(&mass, 2).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(&mass, 6).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_cleansing_nova_resolves_exactly_one_mode_across_all_controllers_and_leaves_indestructible_permanents()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 2)];
    let cards = vec![
        named("Cleansing Nova", 1, A, ObjectDestination::Hand),
        creature(2, A, 1, 1, &[]),
        named("Sol Ring", 3, B, battlefield(B)),
        seed(4, A, battlefield(A), Program::enchantment(None)),
        creature(5, B, 1, 1, &[KeywordAbility::Indestructible]),
    ];
    let state = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let cast = |indices: &[u8]| GameCommand::CastWithResources {
        spell: physical(&state, 1).id(),
        target: None,
        payment: CastingPayment {
            modes: indices
                .iter()
                .map(|&mode| spellwake_engine::mana::CastMode {
                    mode,
                    target: None,
                    targets: vec![],
                })
                .collect(),
            mana: Some(spend(&price)),
            ..Default::default()
        },
    };
    illegal(&state, A, cast(&[0, 1]));
    for mode in [0, 1] {
        let done = round(command(&state, A, cast(&[mode])).into_state()).into_state();
        for (n, owner) in [(2, A), (3, B), (4, A)] {
            let destroyed = (n == 2) == (mode == 0);
            assert_eq!(
                physical(&done, n).zone(),
                if destroyed {
                    ObjectZone::Graveyard(owner)
                } else {
                    ObjectZone::Battlefield
                }
            );
        }
        assert_eq!(physical(&done, 5).zone(), ObjectZone::Battlefield);
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_toxic_deluge_pays_chosen_life_on_cast_and_applies_that_exact_penalty_until_cleanup_including_indestructible_creatures()
 {
    use ManaType::*;
    let price = [(Colourless, 2), (Black, 1)];
    let start = game(
        vec![A, B],
        vec![
            named("Toxic Deluge", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 3, &[KeywordAbility::Indestructible]),
            creature(3, B, 4, 5, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let announce = |x: i64| GameCommand::CastWithResources {
        spell: physical(&start, 1).id(),
        target: None,
        payment: CastingPayment {
            chosen_x: Some(x.into()),
            mana: Some(spend(&price)),
            ..Default::default()
        },
    };
    illegal(&start, A, announce(21));
    for x in [0, 3] {
        let paid = command(&start, A, announce(x)).into_state();
        assert_eq!(paid.execution().life_total(A), Some(&(20 - x).into()));
        stats(&paid, physical(&paid, 3).id(), 4, 5);
        let done = round(paid).into_state();
        stats(&done, physical(&done, 3).id(), 4 - x, 5 - x);
        assert_eq!(
            physical(&done, 2).zone(),
            if x == 0 {
                ObjectZone::Battlefield
            } else {
                ObjectZone::Graveyard(A)
            }
        );
        let expired = main_step(done, 2);
        stats(&expired, physical(&expired, 3).id(), 4, 5);
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sulfurous_blast_damages_each_player_and_creature_using_the_phase_in_which_it_was_cast() {
    use ManaType::*;
    let price = [(Colourless, 2), (Red, 2)];
    for (step, amount) in [
        (Step::PrecombatMain, 3),
        (Step::PostcombatMain, 3),
        (Step::BeginningCombat, 2),
    ] {
        let start = game(
            vec![A, B, C],
            vec![
                named("Sulfurous Blast", 1, A, ObjectDestination::Hand),
                creature(2, A, 1, 4, &[]),
                creature(3, B, 1, 3, &[]),
                creature(4, C, 1, 5, &[]),
            ],
            &price,
            step,
        );
        let done = cast_card(&start, 1, &price);
        for seat in [A, B, C] {
            assert_eq!(
                done.execution().life_total(seat),
                Some(&(20 - amount).into())
            );
        }
        for n in [2, 4] {
            assert_eq!(
                done.execution()
                    .battlefield_marked_damage(physical(&done, n).id()),
                Some(amount.into())
            );
        }
        assert_eq!(
            physical(&done, 3).zone(),
            if amount == 3 {
                ObjectZone::Graveyard(B)
            } else {
                ObjectZone::Battlefield
            }
        );
    }
}

fn paid_target(s: &GameState, spell: u64, creature: u64, price: &[(ManaType, i64)]) -> GameCommand {
    GameCommand::CastPaid {
        spell: physical(s, spell).id(),
        target: marker(s, creature),
        payment: spend(price),
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_return_to_dust_exiles_the_first_artifact_or_enchantment_and_offers_a_second_only_from_the_casters_main_phase()
 {
    use ManaType::*;
    let price = [(Colourless, 2), (White, 2)];
    for step in [Step::PrecombatMain, Step::BeginningCombat] {
        let start = game(
            vec![A, B],
            vec![
                named("Return to Dust", 1, A, ObjectDestination::Hand),
                named("Sol Ring", 2, A, battlefield(A)),
                seed(3, B, battlefield(B), Program::enchantment(None)),
                creature(4, B, 1, 1, &[]),
            ],
            &price,
            step,
        );
        for ids in [&[2, 2][..], &[2, 4][..]] {
            illegal(&start, A, multiple(&start, 1, ids, &price));
        }
        let pair = round(command(&start, A, multiple(&start, 1, &[2, 3], &price)).into_state())
            .into_state();
        assert_eq!(physical(&pair, 2).zone(), ObjectZone::Exile);
        if step == Step::PrecombatMain {
            for take in [false, true] {
                let done = super::super::equipment::optional(&pair, take);
                assert_eq!(
                    physical(&done, 3).zone(),
                    if take {
                        ObjectZone::Exile
                    } else {
                        ObjectZone::Battlefield
                    }
                );
            }
        } else {
            assert!(pair.pending_decision().is_none());
            assert_eq!(physical(&pair, 3).zone(), ObjectZone::Battlefield);
        }
        let single =
            round(command(&start, A, multiple(&start, 1, &[3], &price)).into_state()).into_state();
        assert_eq!(physical(&single, 3).zone(), ObjectZone::Exile);
        assert_eq!(physical(&single, 2).zone(), ObjectZone::Battlefield);
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_temur_battle_rage_checks_the_casters_power_at_resolution_and_double_strike_and_trample_expire()
 {
    use ManaType::*;
    let price = [(Colourless, 1), (Red, 1)];
    for power in [3, 4] {
        let start = game(
            vec![A, B],
            vec![
                named("Temur Battle Rage", 1, A, ObjectDestination::Hand),
                creature(2, B, 2, 2, &[]),
                creature(3, A, power, 4, &[]),
                creature(4, B, 8, 8, &[]),
                super::super::super::cmm_gameplay::remove_spell(5, A),
            ],
            &price,
            Step::PrecombatMain,
        );
        let cast = command(&start, A, paid_target(&start, 1, 2, &price)).into_state();
        let lost = round(targeted(&cast, A, 5, marker(&cast, 3)).into_state()).into_state();
        let after = round(lost).into_state();
        let host = physical(&after, 2).id();
        assert!(sheet(&after, host).has_keyword(KeywordAbility::DoubleStrike));
        assert!(!sheet(&after, host).has_keyword(KeywordAbility::Trample));
        let done = round(cast).into_state();
        assert!(sheet(&done, host).has_keyword(KeywordAbility::DoubleStrike));
        assert_eq!(
            sheet(&done, host).has_keyword(KeywordAbility::Trample),
            power == 4
        );
        let expired = main_step(done, 2);
        for key in [KeywordAbility::DoubleStrike, KeywordAbility::Trample] {
            assert!(!sheet(&expired, host).has_keyword(key));
        }
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_supernatural_stamina_uses_a_death_trigger_returns_tapped_to_its_owner_and_expires_with_its_power_bonus()
 {
    use ManaType::*;
    let price = [(Black, 1)];
    let mut stolen = creature(2, B, 2, 3, &[]);
    stolen.destination = battlefield(A);
    let start = game(
        vec![A, B],
        vec![
            named("Supernatural Stamina", 1, A, ObjectDestination::Hand),
            stolen,
            super::super::super::cmm_gameplay::remove_spell(3, A),
            super::super::super::cmm_gameplay::remove_spell(4, B),
        ],
        &price,
        Step::PrecombatMain,
    );
    let given =
        round(command(&start, A, paid_target(&start, 1, 2, &price)).into_state()).into_state();
    let old = physical(&given, 2).id();
    stats(&given, old, 4, 3);
    let died = round(targeted(&given, A, 3, marker(&given, 2)).into_state()).into_state();
    assert_eq!(physical(&died, 2).zone(), ObjectZone::Graveyard(B));
    let back = round(died).into_state();
    let card = physical(&back, 2);
    assert_eq!(card.zone(), ObjectZone::Battlefield);
    assert_eq!(card.controller(), Some(B));
    assert_eq!(card.tapped(), Some(true));
    assert_ne!(card.id(), old);
    stats(&back, card.id(), 2, 3);
    let next = main_step(given, 2);
    stats(&next, old, 2, 3);
    let gone = round(targeted(&next, B, 4, marker(&next, 2)).into_state()).into_state();
    assert_eq!(physical(&gone, 2).zone(), ObjectZone::Graveyard(B));
    assert!(gone.execution().window().entries().is_empty());
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_tragic_slip_uses_any_creature_death_that_turn_including_a_death_after_the_spell_was_cast() {
    use ManaType::*;
    let price = [(Black, 1)];
    let start = game(
        vec![A, B],
        vec![
            named("Tragic Slip", 1, A, ObjectDestination::Hand),
            creature(2, B, 15, 14, &[]),
            creature(3, B, 1, 1, &[]),
            super::super::super::cmm_gameplay::remove_spell(4, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(&start, A, paid_target(&start, 1, 2, &price)).into_state();
    let normal = round(cast.clone()).into_state();
    let host = physical(&normal, 2).id();
    stats(&normal, host, 14, 13);
    let reset = main_step(normal, 2);
    stats(&reset, host, 15, 14);
    let died = round(targeted(&cast, A, 4, marker(&cast, 3)).into_state()).into_state();
    assert_eq!(physical(&died, 3).zone(), ObjectZone::Graveyard(B));
    let stronger = round(died).into_state();
    stats(&stronger, host, 2, 1);
    let expired = main_step(stronger, 2);
    stats(&expired, host, 15, 14);
}
