//! Commander Masters spell cases through paid casts and their actual target windows.
use super::*;

fn cast_at(state: &GameState, price: &[(ManaType, i64)], target: TargetRef) -> GameState {
    let through_stack = command(
        state,
        A,
        GameCommand::CastPaid {
            spell: physical(state, 1).id(),
            target,
            payment: spend(price),
        },
    )
    .into_state();
    assert_eq!(physical(&through_stack, 1).zone(), ObjectZone::Stack);
    for &(kind, _) in price {
        assert_eq!(through_stack.execution().mana(A, kind), 0.into());
    }
    round(through_stack).into_state()
}

fn cannot_cast_at(state: &GameState, price: &[(ManaType, i64)], target: TargetRef) {
    illegal(
        state,
        A,
        GameCommand::CastPaid {
            spell: physical(state, 1).id(),
            target,
            payment: spend(price),
        },
    );
    assert_eq!(physical(state, 1).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_gift_and_resculpt_create_the_receiving_players_printed_tokens() {
    for (name, price, zone, token_name, colors, subtype, size) in [
        (
            "Generous Gift",
            vec![(ManaType::Colourless, 2), (ManaType::White, 1)],
            ObjectZone::Graveyard(B),
            "Elephant Token",
            vec![Color::Green],
            "Elephant",
            3,
        ),
        (
            "Resculpt",
            vec![(ManaType::Colourless, 1), (ManaType::Blue, 1)],
            ObjectZone::Exile,
            "Elemental Token",
            vec![Color::Blue, Color::Red],
            "Elemental",
            4,
        ),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                named(
                    "Fencing Ace",
                    2,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                ),
                named(
                    "Ornithopter of Paradise",
                    3,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                ),
                named(
                    "Forest",
                    4,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                ),
                named(
                    "Darksteel Ingot",
                    5,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                ),
            ],
            &price,
            Step::PrecombatMain,
        );
        for victim in [2, 3] {
            let after = cast_at(
                &initial,
                &price,
                TargetRef::Object(physical(&initial, victim).id()),
            );
            assert_eq!(physical(&after, victim).zone(), zone);
            assert_eq!(physical(&after, 1).zone(), ObjectZone::Graveyard(A));
            let found = after
                .execution()
                .window()
                .cards()
                .iter()
                .filter(|object| object.is_token() && object.zone() == ObjectZone::Battlefield)
                .collect::<Vec<_>>();
            let [token] = found.as_slice() else {
                panic!("{name}: expected one token");
            };
            assert_eq!(token.owner(), B);
            assert_eq!(token.controller(), Some(B));
            let view = after
                .execution()
                .current_characteristics(token.id())
                .unwrap()
                .unwrap();
            assert_eq!(
                view.current_names.iter().next().unwrap().as_str(),
                token_name
            );
            assert_eq!(
                view.current_creature_subtypes,
                [current_catalogue().1.resolve_label(subtype).unwrap()].into()
            );
            assert_eq!(view.current_colors, colors.iter().copied().collect());
            assert_eq!(
                view.power_toughness.unwrap().derived,
                PowerToughness {
                    power: size.into(),
                    toughness: size.into(),
                }
            );
        }
        if name == "Resculpt" {
            cannot_cast_at(
                &initial,
                &price,
                TargetRef::Object(physical(&initial, 4).id()),
            );
            let exiled = cast_at(
                &initial,
                &price,
                TargetRef::Object(physical(&initial, 5).id()),
            );
            assert_eq!(physical(&exiled, 5).zone(), ObjectZone::Exile);
        } else {
            let land_destroyed = cast_at(
                &initial,
                &price,
                TargetRef::Object(physical(&initial, 4).id()),
            );
            assert_eq!(
                physical(&land_destroyed, 4).zone(),
                ObjectZone::Graveyard(B)
            );
            let indestructible = cast_at(
                &initial,
                &price,
                TargetRef::Object(physical(&initial, 5).id()),
            );
            assert_eq!(physical(&indestructible, 5).zone(), ObjectZone::Battlefield);
            assert_eq!(
                indestructible
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|o| o.is_token() && o.controller() == Some(B))
                    .count(),
                1
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_feed_the_swarm_loses_the_owned_targets_mana_value_even_if_it_survives() {
    let price = [(ManaType::Colourless, 1), (ManaType::Black, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Feed the Swarm"),
            named(
                "Fencing Ace",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Spirited Companion",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Darksteel Ingot",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            seed(
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::enchantment(Some(ManaCost::generic(4.into()))),
            ),
            named(
                "Zetalpa, Primal Dawn",
                6,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    for id in [3, 4] {
        cannot_cast_at(
            &initial,
            &price,
            TargetRef::Object(physical(&initial, id).id()),
        );
    }
    for (id, resulting_zone, life) in [
        (2, ObjectZone::Graveyard(B), 18),
        (5, ObjectZone::Graveyard(B), 16),
        (6, ObjectZone::Battlefield, 12),
    ] {
        let result = cast_at(
            &initial,
            &price,
            TargetRef::Object(physical(&initial, id).id()),
        );
        assert_eq!(physical(&result, id).zone(), resulting_zone);
        assert_eq!(result.execution().life_total(A), Some(&life.into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_swords_to_plowshares_exiles_a_creature_and_gains_life_for_its_controller() {
    let price = [(ManaType::White, 1)];
    let mut stolen = creature(2, B, 5, 5, &[Indestructible]);
    stolen.owner = A;
    let initial = game(
        vec![A, B],
        vec![
            hand("Swords to Plowshares"),
            stolen,
            creature(3, A, -2, 1, &[]),
            named(
                "Forest",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    cannot_cast_at(
        &initial,
        &price,
        TargetRef::Object(physical(&initial, 4).id()),
    );
    let stolen_result = cast_at(
        &initial,
        &price,
        TargetRef::Object(physical(&initial, 2).id()),
    );
    assert_eq!(physical(&stolen_result, 2).zone(), ObjectZone::Exile);
    assert_eq!(stolen_result.execution().life_total(A), Some(&20.into()));
    assert_eq!(stolen_result.execution().life_total(B), Some(&25.into()));
    let negative = cast_at(
        &initial,
        &price,
        TargetRef::Object(physical(&initial, 3).id()),
    );
    assert_eq!(physical(&negative, 3).zone(), ObjectZone::Exile);
    assert_eq!(negative.execution().life_total(A), Some(&20.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_broken_wings_targets_artifacts_enchantments_or_creatures_with_flying() {
    let price = [(ManaType::Colourless, 2), (ManaType::Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Broken Wings"),
            creature(2, B, 3, 2, &[Flying]),
            creature(3, B, 3, 2, &[Reach]),
            named(
                "Mind Stone",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            seed(
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::enchantment(None),
            ),
            named(
                "Darksteel Ingot",
                6,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    cannot_cast_at(
        &initial,
        &price,
        TargetRef::Object(physical(&initial, 3).id()),
    );
    for target in [2, 4, 5, 6] {
        let outcome = cast_at(
            &initial,
            &price,
            TargetRef::Object(physical(&initial, target).id()),
        );
        assert_eq!(
            physical(&outcome, target).zone(),
            if target == 6 {
                ObjectZone::Battlefield
            } else {
                ObjectZone::Graveyard(B)
            }
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_abrade_selects_and_resolves_each_mode_with_its_own_target_domain() {
    let price = [(ManaType::Colourless, 1), (ManaType::Red, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Abrade"),
            creature(2, B, 2, 3, &[]),
            creature(3, B, 2, 3, &[Indestructible]),
            named(
                "Mind Stone",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Darksteel Ingot",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            remove_spell(6, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let mode = |target, mode| GameCommand::CastWithResources {
        spell: physical(&initial, 1).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(&price)),
            modes: vec![spellwake_engine::mana::CastMode {
                mode,
                target: Some(TargetRef::Object(physical(&initial, target).id())),
                targets: vec![],
            }],
            ..Default::default()
        },
    };
    illegal(&initial, A, mode(2, 1));
    illegal(&initial, A, mode(4, 0));
    for (id, option, destination) in [
        (2, 0, ObjectZone::Graveyard(B)),
        (3, 0, ObjectZone::Battlefield),
        (4, 1, ObjectZone::Graveyard(B)),
        (5, 1, ObjectZone::Battlefield),
    ] {
        let stacked = command(&initial, A, mode(id, option)).into_state();
        assert_eq!(physical(&stacked, 1).zone(), ObjectZone::Stack);
        let result = round(stacked).into_state();
        assert_eq!(physical(&result, id).zone(), destination);
        assert_eq!(physical(&result, 1).zone(), ObjectZone::Graveyard(A));
        assert_eq!(result.execution().life_total(B), Some(&20.into()));
        if id == 3 {
            assert_eq!(
                result
                    .execution()
                    .battlefield_marked_damage(physical(&result, 3).id()),
                Some(3.into())
            );
        }
    }
    let cast = command(&initial, A, mode(2, 0)).into_state();
    let target = physical(&cast, 2).id();
    let removed = round(targeted(&cast, A, 6, TargetRef::Object(target)).into_state()).into_state();
    let fizzled = round(removed);
    assert_eq!(
        physical(fizzled.state(), 1).zone(),
        ObjectZone::Graveyard(A)
    );
    assert!(
        !events(&fizzled)
            .flat_map(|event| &event.occurrences)
            .any(|row| matches!(row.value, RulesOccurrence::DamageDealt { .. }))
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_swift_response_requires_a_tapped_creature_at_announcement_and_resolution() {
    let initial = game(
        vec![A, B],
        vec![
            hand("Swift Response"),
            creature(2, A, 2, 2, &[Haste]),
            creature(3, B, 2, 2, &[]),
            named(
                "Forest",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Plains",
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                change_one_permanent(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::set_targets_tapped(
                        false,
                    ),
                ),
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let mut attacked = attack(&initial, 2, B);
    for (id, land) in [
        (4, spellwake_engine::land::BasicLandType::Forest),
        (5, spellwake_engine::land::BasicLandType::Plains),
    ] {
        attacked = command(
            &attacked,
            A,
            GameCommand::ActivateMana {
                source: physical(&attacked, id).id(),
                ability: ManaAbilityRef::BasicLand(land),
            },
        )
        .into_state();
    }
    let price = [(ManaType::Green, 1), (ManaType::White, 1)];
    cannot_cast_at(
        &attacked,
        &price,
        TargetRef::Object(physical(&attacked, 3).id()),
    );
    let waiting = command(
        &attacked,
        A,
        GameCommand::CastPaid {
            spell: physical(&attacked, 1).id(),
            target: TargetRef::Object(physical(&attacked, 2).id()),
            payment: spend(&price),
        },
    )
    .into_state();
    let untapped = respond_to_creature(&waiting, 6, 2);
    let survived = round(untapped).into_state();
    assert_eq!(physical(&survived, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&survived, 2).tapped(), Some(false));

    let removed = cast_at(
        &attacked,
        &price,
        TargetRef::Object(physical(&attacked, 2).id()),
    );
    assert_eq!(physical(&removed, 2).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&removed, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&removed, 3).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_decimate_requires_all_four_printed_targets_but_still_destroys_the_remaining_ones() {
    let price = [
        (ManaType::Colourless, 2),
        (ManaType::Red, 1),
        (ManaType::Green, 1),
    ];
    let initial = game(
        vec![A, B],
        vec![
            hand("Decimate"),
            named(
                "Darksteel Ingot",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(3, B, 2, 2, &[]),
            seed(
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::enchantment(None),
            ),
            named(
                "Forest",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            remove_spell(6, A),
        ],
        &price,
        Step::PrecombatMain,
    );
    let select = |range: &[u64]| GameCommand::CastWithResources {
        spell: physical(&initial, 1).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(&price)),
            fixed_targets: range
                .iter()
                .map(|&id| TargetRef::Object(physical(&initial, id).id()))
                .collect(),
            ..Default::default()
        },
    };
    illegal(&initial, A, select(&[2, 3, 4]));
    illegal(&initial, A, select(&[2, 3, 4, 3]));
    let pending = command(&initial, A, select(&[2, 3, 4, 5])).into_state();
    let target = physical(&pending, 3).id();
    let changed =
        round(targeted(&pending, A, 6, TargetRef::Object(target)).into_state()).into_state();
    let ordering = round(changed).into_state();
    let decision = ordering.pending_decision().unwrap();
    let RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame() else {
        panic!("owner must order the two destroyed permanents");
    };
    assert_eq!(decision.chooser(), B);
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        [physical(&ordering, 4).id(), physical(&ordering, 5).id()].into(),
    );
    let damage = reply(
        &ordering,
        DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec()),
    )
    .into_state();
    assert_eq!(physical(&damage, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&damage, 3).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&damage, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&damage, 5).zone(), ObjectZone::Graveyard(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ram_through_deals_source_damage_and_assigns_only_trample_excess_to_the_player() {
    let price = [(ManaType::Colourless, 1), (ManaType::Green, 1)];
    for (trample, opponent_life) in [(true, 17), (false, 20)] {
        let mut dealer = creature(2, A, 5, 5, &[Lifelink]);
        if trample {
            dealer.program.printed_keywords.insert(Trample);
        }
        let initial = game(
            vec![A, B],
            vec![
                hand("Ram Through"),
                dealer,
                creature(3, B, 2, 2, &[]),
                remove_spell(4, A),
            ],
            &price,
            Step::PrecombatMain,
        );
        let target = |id| TargetRef::Object(physical(&initial, id).id());
        let announce = |dealer, recipient| GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: None,
            payment: CastingPayment {
                fixed_targets: vec![target(dealer), target(recipient)],
                mana: Some(spend(&price)),
                ..Default::default()
            },
        };
        illegal(&initial, A, announce(3, 2));
        let cast = command(&initial, A, announce(2, 3)).into_state();
        let after = round(cast.clone()).into_state();
        assert_eq!(physical(&after, 2).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(&after, 3).zone(), ObjectZone::Graveyard(B));
        assert_eq!(after.execution().life_total(A), Some(&25.into()));
        assert_eq!(after.execution().life_total(B), Some(&opponent_life.into()));

        let removed = round(targeted(&cast, A, 4, target(3)).into_state()).into_state();
        let no_damage = round(removed).into_state();
        assert_eq!(no_damage.execution().life_total(A), Some(&20.into()));
        assert_eq!(no_damage.execution().life_total(B), Some(&20.into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_inferno_titan_keeps_the_announced_damage_shares_when_one_target_is_gone() {
    use spellwake_engine::activated::ActivatedAbilityId;
    let price = [(ManaType::Colourless, 4), (ManaType::Red, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Inferno Titan"),
            creature(2, B, 2, 2, &[]),
            remove_spell(3, A),
        ],
        &[(ManaType::Colourless, 4), (ManaType::Red, 3)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let victim = TargetRef::Object(physical(&entered, 2).id());
    let choose = |targets| DecisionReply::TriggerPermanentTargets(targets);
    illegal(&entered, A, combat_declaration(&entered, choose(vec![])));
    illegal(
        &entered,
        A,
        combat_declaration(&entered, choose(vec![victim, victim])),
    );
    let all_three = reply(
        &entered,
        choose(vec![TargetRef::Player(A), TargetRef::Player(B), victim]),
    )
    .into_state();
    let divided = round(all_three).into_state();
    assert_eq!(divided.execution().life_total(A), Some(&19.into()));
    assert_eq!(divided.execution().life_total(B), Some(&19.into()));
    assert_eq!(
        divided
            .execution()
            .battlefield_marked_damage(physical(&divided, 2).id()),
        Some(1.into()),
    );
    let announced = reply(&entered, choose(vec![TargetRef::Player(B), victim])).into_state();
    let removed = round(targeted(&announced, A, 3, victim).into_state()).into_state();
    let hit = round(removed).into_state();
    assert_eq!(hit.execution().life_total(B), Some(&18.into()));
    assert_eq!(physical(&hit, 2).zone(), ObjectZone::Graveyard(B));

    let titan = physical(&hit, 1).id();
    let paid = command(
        &hit,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: titan,
            ability: ActivatedAbilityId(0),
            payment: Some(spend(&[(ManaType::Red, 1)])),
        },
    )
    .into_state();
    let pumped = round(paid).into_state();
    assert_eq!(
        pumped
            .execution()
            .power_toughness(titan)
            .unwrap()
            .unwrap()
            .derived
            .power,
        7.into()
    );
    let following = next_main(pumped, 3);
    let attacking = attack(&following, 1, B);
    let stacked = reply(&attacking, choose(vec![TargetRef::Player(B)])).into_state();
    let resolved = round(stacked).into_state();
    assert_eq!(resolved.execution().life_total(B), Some(&15.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_savage_beating_only_casts_in_own_combat_and_entwine_combines_both_modes() {
    use ManaType::*;
    use spellwake_engine::mana::CastMode;
    let price = [(Colourless, 4), (Red, 3)];
    let battlefield = || {
        vec![
            hand("Savage Beating"),
            creature(2, A, 2, 2, &[Haste]),
            creature(3, B, 2, 2, &[]),
        ]
    };
    let outside = game(vec![A, B], battlefield(), &price, Step::PrecombatMain);
    let announce = |state: &GameState, modes: &[u8], entwined| GameCommand::CastWithResources {
        spell: physical(state, 1).id(),
        target: None,
        payment: CastingPayment {
            modes: modes
                .iter()
                .copied()
                .map(|mode| CastMode {
                    mode,
                    target: None,
                    targets: vec![],
                })
                .collect(),
            additional: if entwined {
                [(0, 1.into())].into()
            } else {
                Default::default()
            },
            mana: Some(spend(if entwined {
                &price
            } else {
                &[(Colourless, 3), (Red, 2)]
            })),
            ..Default::default()
        },
    };
    illegal(&outside, A, announce(&outside, &[0], false));
    let entering_combat = game(vec![A, B], battlefield(), &price, Step::DeclareAttackers);
    let attacking = attack(&entering_combat, 2, B);
    assert_eq!(physical(&attacking, 2).tapped(), Some(true));
    illegal(&attacking, A, announce(&attacking, &[0, 1], false));
    let has_strike = |state: &GameState, id| {
        state
            .execution()
            .current_characteristics(physical(state, id).id())
            .unwrap()
            .unwrap()
            .keywords
            .contains(&DoubleStrike)
    };
    for (modes, entwined, double, untapped) in [
        (&[0][..], false, true, false),
        (&[1][..], false, false, true),
        (&[0, 1][..], true, true, true),
    ] {
        let paid = command(&attacking, A, announce(&attacking, modes, entwined)).into_state();
        let result = round(paid).into_state();
        assert_eq!(has_strike(&result, 2), double);
        assert!(!has_strike(&result, 3));
        assert_eq!(physical(&result, 2).tapped(), Some(!untapped));
        if untapped {
            let next_combat = exile_hand::until(result, |state| {
                state.turn().unwrap().position().step == Step::BeginningCombat
            });
            assert_eq!(next_combat.turn().unwrap().position().number, 1);
            assert_eq!(has_strike(&next_combat, 2), double);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_urzas_ruinous_blast_checks_a_controlled_legend_then_exiles_nonland_nonlegends() {
    use ManaType::*;
    let price = [(Colourless, 4), (White, 1)];
    let boards = |own| {
        vec![
            hand("Urza's Ruinous Blast"),
            named(
                "Akiri, Fearless Voyager",
                2,
                own,
                ObjectDestination::Battlefield { controller: own },
            ),
            named(
                "Darksteel Ingot",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Forest",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(5, B, 4, 4, &[]),
        ]
    };
    let no_legend = game(vec![A, B], boards(B), &price, Step::PrecombatMain);
    illegal(
        &no_legend,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&no_legend, 1).id(),
            payment: Some(spend(&price)),
        },
    );
    let live_legend = game(vec![A, B], boards(A), &price, Step::PrecombatMain);
    let resolved = cast_named(&live_legend, &price);
    for (id, zone) in [
        (1, ObjectZone::Graveyard(A)),
        (2, ObjectZone::Battlefield),
        (3, ObjectZone::Exile),
        (4, ObjectZone::Battlefield),
        (5, ObjectZone::Exile),
    ] {
        assert_eq!(physical(&resolved, id).zone(), zone);
    }

    let mut with_walker = boards(B);
    with_walker.push(named("The Wanderer", 6, A, ObjectDestination::Hand));
    let fresh = game(
        vec![A, B],
        with_walker,
        &[(Colourless, 7), (White, 2)],
        Step::PrecombatMain,
    );
    let controlled =
        round(cast_untargeted(&fresh, 6, &[(Colourless, 3), (White, 1)]).into_state()).into_state();
    let blast = cast_named(&controlled, &price);
    assert_eq!(physical(&blast, 6).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&blast, 3).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_exsanguinate_batches_opponent_losses_before_applying_the_linked_gain() {
    use ManaType::*;
    use spellwake_engine::life::LifeChangeKind::{Gain, Loss};
    for (x, lives) in [(0, [20, 20, 20]), (4, [28, 16, 16])] {
        let price: Vec<_> = [(Black, 2), (Colourless, x)]
            .into_iter()
            .filter(|&(_, amount)| amount > 0)
            .collect();
        let initial = game(
            vec![A, B, C],
            vec![hand("Exsanguinate")],
            &price,
            Step::PrecombatMain,
        );
        let cast = command(
            &initial,
            A,
            GameCommand::CastWithResources {
                spell: physical(&initial, 1).id(),
                target: None,
                payment: CastingPayment {
                    chosen_x: Some(x.into()),
                    mana: Some(spend(&price)),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let resolved = round(cast);
        assert_eq!(
            physical(resolved.state(), 1).zone(),
            ObjectZone::Graveyard(A)
        );
        for (seat, life) in [A, B, C].into_iter().zip(lives) {
            assert_eq!(
                resolved.state().execution().life_total(seat),
                Some(&life.into())
            );
        }
        let changes: Vec<_> = events(&resolved)
            .filter_map(|event| {
                let group: BTreeMap<_, _> = event
                    .occurrences
                    .iter()
                    .filter_map(|row| match &row.value {
                        RulesOccurrence::LifeChanged(change) => Some((change.player, change.kind)),
                        _ => None,
                    })
                    .collect();
                (!group.is_empty()).then_some(group)
            })
            .collect();
        if x == 0 {
            assert!(changes.is_empty());
        } else {
            assert_eq!(
                changes,
                [
                    BTreeMap::from([(B, Loss), (C, Loss)]),
                    BTreeMap::from([(A, Gain)]),
                ],
            );
        }
    }
}

fn change_one_permanent(instruction: Instruction) -> Program {
    free_instant(BodyTarget::SingleCreature, instruction)
}

fn respond_to_creature(state: &GameState, spell: u64, creature: u64) -> GameState {
    let response = GameCommand::CastWithResources {
        spell: physical(state, spell).id(),
        target: None,
        payment: CastingPayment {
            permanent_targets: vec![TargetRef::Object(physical(state, creature).id())],
            ..Default::default()
        },
    };
    round(command(state, A, response).into_state()).into_state()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_removal_reads_target_power_control_and_flying_at_resolution() {
    use ManaType::*;
    use spellwake_engine::characteristics::{AbilityChange, AbilityRemoval};
    for (name, cost, destination, token_count, own_life) in [
        (
            "Generous Gift",
            vec![(Colourless, 2), (White, 1)],
            ObjectZone::Graveyard(A),
            1,
            20,
        ),
        (
            "Resculpt",
            vec![(Colourless, 1), (Blue, 1)],
            ObjectZone::Exile,
            1,
            20,
        ),
        (
            "Swords to Plowshares",
            vec![(White, 1)],
            ObjectZone::Exile,
            0,
            26,
        ),
        (
            "Broken Wings",
            vec![(Colourless, 2), (Green, 1)],
            ObjectZone::Battlefield,
            0,
            20,
        ),
    ] {
        let mut target = creature(2, B, 2, 2, &[Flying]);
        target.owner = A;
        let parts = if name == "Broken Wings" {
            vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                AbilityRemoval::All,
            ))]
        } else {
            vec![
                ResolvedPartSpec::Control(ControlRecipient::TargetOwner),
                ResolvedPartSpec::ModifyPT(PowerToughness {
                    power: 4.into(),
                    toughness: 0.into(),
                }),
            ]
        };
        let setup = game(
            vec![A, B],
            vec![
                hand(name),
                target,
                seed(
                    3,
                    A,
                    ObjectDestination::Hand,
                    change_one_permanent(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            ResolvedPartsSpec(parts),
                            ResolvedDuration::UntilEndOfTurn,
                        ),
                    ),
                ),
            ],
            &cost,
            Step::PrecombatMain,
        );
        let announced = GameCommand::CastPaid {
            spell: physical(&setup, 1).id(),
            target: TargetRef::Object(physical(&setup, 2).id()),
            payment: spend(&cost),
        };
        let cast = command(&setup, A, announced).into_state();
        let changed = respond_to_creature(&cast, 3, 2);
        if name == "Broken Wings" {
            assert!(
                !changed
                    .execution()
                    .current_characteristics(physical(&changed, 2).id())
                    .unwrap()
                    .unwrap()
                    .has_keyword(Flying)
            );
        } else {
            assert_eq!(physical(&changed, 2).controller(), Some(A));
        }
        let result = round(changed).into_state();
        assert_eq!(physical(&result, 2).zone(), destination, "{name}");
        assert_eq!(
            result.execution().life_total(A),
            Some(&own_life.into()),
            "{name}"
        );
        assert_eq!(result.execution().life_total(B), Some(&20.into()), "{name}");
        let tokens = result
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|object| object.is_token() && object.zone() == ObjectZone::Battlefield)
            .collect::<Vec<_>>();
        assert_eq!(tokens.len(), token_count, "{name}");
        assert!(
            tokens.iter().all(|token| token.controller() == Some(A)),
            "{name}"
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rise_of_the_eldrazi_survives_a_counter_destroys_draws_four_and_gives_an_extra_turn() {
    use ManaType::*;
    let price = [(Colourless, 12)];
    let mut cards = vec![
        hand("Rise of the Eldrazi"),
        creature(2, B, 3, 3, &[]),
        seed(
            3,
            B,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                        spellwake_engine::toolbox::CounterDestination::Graveyard,
                    ),
                )),
            ),
        ),
    ];
    cards.extend((10..14).map(|id| named("Forest", id, B, ObjectDestination::LibraryBottom)));
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let subject = TargetRef::Object(physical(&initial, 2).id());
    let announce = |targets| GameCommand::CastWithResources {
        spell: physical(&initial, 1).id(),
        target: None,
        payment: CastingPayment {
            fixed_targets: targets,
            mana: Some(spend(&price)),
            ..Default::default()
        },
    };
    illegal(&initial, A, announce(vec![TargetRef::Player(B), subject]));
    let stacked = command(&initial, A, announce(vec![subject, TargetRef::Player(B)])).into_state();
    assert_eq!(stacked.execution().mana(A, Colourless), 0.into());
    let entry = stacked.execution().window().entries()[0].id();
    let their_priority = pass(&stacked).into_state();
    let counter = targeted(&their_priority, B, 3, TargetRef::Stack(entry)).into_state();
    let safe = round(counter).into_state();
    assert_eq!(safe.execution().window().entries()[0].id(), entry);
    assert_eq!(physical(&safe, 1).zone(), ObjectZone::Stack);
    assert_eq!(physical(&safe, 3).zone(), ObjectZone::Graveyard(B));
    let resolved = round(safe).into_state();
    assert_eq!(physical(&resolved, 1).zone(), ObjectZone::Exile);
    assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Graveyard(B));
    for id in 10..14 {
        assert_eq!(physical(&resolved, id).zone(), ObjectZone::Hand(B));
    }
    assert_eq!(physical(&resolved, 101).zone(), ObjectZone::Library(B));
    let extra = next_main(resolved, 2);
    assert_eq!(extra.execution().window().active_player(), A);
}
