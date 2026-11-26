//! Source-bound mana production, costs and activation restrictions.
use super::*;
use spellwake_engine::characteristics::KeywordAbility::{Flying, Haste, Trample};
use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};

fn land_play(state: &GameState, id: u64) -> GameState {
    command(
        state,
        A,
        GameCommand::PlayLand {
            land: physical(state, id).id(),
        },
    )
    .into_state()
}
fn tap_for_mana(state: &GameState, id: u64, ability: u64) -> GameCommand {
    GameCommand::ActivateMana {
        source: physical(state, id).id(),
        ability: ManaAbilityRef::Printed(ManaAbilityId(ability)),
    }
}
fn assert_colors(state: &GameState, choices: &[ManaType], damage: i64) {
    let RuleFrame::ManaColor(frame) = state.pending_decision().unwrap().frame() else {
        panic!("choose a mana color");
    };
    assert_eq!(frame.colors(), &choices.iter().copied().collect());
    for &color in choices {
        let done = reply(state, DecisionReply::ManaColor(color)).into_state();
        assert!(done.execution().window().entries().is_empty());
        assert_eq!(done.execution().mana(A, color), 1.into());
        assert_eq!(done.execution().life_total(A), Some(&(20 - damage).into()));
        assert_eq!(physical(&done, 1).tapped(), Some(true));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_talismans_produce_colorless_without_damage_or_one_printed_color_with_damage() {
    for (name, colors) in [
        ("Talisman of Conviction", [Red, White]),
        ("Talisman of Creativity", [Blue, Red]),
        ("Talisman of Progress", [White, Blue]),
    ] {
        let initial = game(
            vec![A, B],
            vec![named(name, 1, A, ObjectDestination::Hand)],
            &[(Colourless, 2)],
            Step::PrecombatMain,
        );
        let entered =
            round(cast_untargeted(&initial, 1, &[(Colourless, 2)]).into_state()).into_state();
        let fixed = command(&entered, A, tap_for_mana(&entered, 1, 0)).into_state();
        assert_eq!(fixed.execution().mana(A, Colourless), 1.into());
        assert_eq!(fixed.execution().life_total(A), Some(&20.into()));
        illegal(&fixed, A, tap_for_mana(&fixed, 1, 1));
        let choosing = command(&entered, A, tap_for_mana(&entered, 1, 1)).into_state();
        assert_colors(&choosing, &colors, 1);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_tainted_lands_add_colorless_freely_and_offer_their_colors_only_with_a_controlled_swamp() {
    for (name, colors) in [
        ("Tainted Field", [White, Black]),
        ("Tainted Wood", [Black, Green]),
    ] {
        for owner in [A, B] {
            let initial = game(
                vec![A, B],
                vec![
                    named(name, 1, A, ObjectDestination::Hand),
                    named(
                        "Swamp",
                        2,
                        owner,
                        ObjectDestination::Battlefield { controller: owner },
                    ),
                ],
                &[],
                Step::PrecombatMain,
            );
            let land = land_play(&initial, 1);
            let fixed = command(&land, A, tap_for_mana(&land, 1, 0)).into_state();
            assert_eq!(fixed.execution().mana(A, Colourless), 1.into());
            let choose = tap_for_mana(&land, 1, 1);
            if owner == A {
                assert_colors(&command(&land, A, choose).into_state(), &colors, 0);
            } else {
                illegal(&land, A, choose);
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_wastes_adds_colorless_and_prismatic_lens_pays_one_to_filter_into_any_color() {
    let state = game(
        vec![A, B],
        vec![named("Wastes", 1, A, ObjectDestination::Hand)],
        &[],
        Step::PrecombatMain,
    );
    let ready = land_play(&state, 1);
    let done = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
    assert_eq!(done.execution().mana(A, Colourless), 1.into());
    assert_eq!(physical(&done, 1).tapped(), Some(true));
    illegal(&done, A, tap_for_mana(&done, 1, 0));
    let start = game(
        vec![A, B],
        vec![named("Prismatic Lens", 1, A, ObjectDestination::Hand)],
        &[(Colourless, 3)],
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&start, 1, &[(Colourless, 2)]).into_state()).into_state();
    let fixed = command(&entered, A, tap_for_mana(&entered, 1, 0)).into_state();
    assert_eq!(fixed.execution().mana(A, Colourless), 2.into());
    illegal(&entered, A, tap_for_mana(&entered, 1, 1));
    let choosing = command(
        &entered,
        A,
        GameCommand::ActivateManaWithPayment {
            source: physical(&entered, 1).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            payment: spend(&[(Colourless, 1)]),
        },
    )
    .into_state();
    assert_eq!(choosing.execution().mana(A, Colourless), 0.into());
    assert_colors(&choosing, &[White, Blue, Black, Red, Green], 0);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_bonders_enclave_and_sea_gate_wreckage_require_the_correct_condition_to_tap_and_draw() {
    for (name, empty_required) in [("Bonders' Enclave", false), ("Sea Gate Wreckage", true)] {
        for enabled in [false, true] {
            let mut cards = vec![named(name, 1, A, ObjectDestination::Hand)];
            if !empty_required {
                cards.extend([
                    creature(2, A, if enabled { 4 } else { 3 }, 2, &[]),
                    creature(3, B, 9, 9, &[]),
                ]);
            }
            if empty_required && !enabled {
                cards.push(seed(
                    2,
                    A,
                    ObjectDestination::Hand,
                    Program::artifact(None, vec![]),
                ));
            }
            let start = game(
                vec![A, B],
                cards,
                &[(Colourless, 3), (Green, 3)],
                Step::PrecombatMain,
            );
            let played = land_play(&start, 1);
            let fixed = command(&played, A, tap_for_mana(&played, 1, 0)).into_state();
            assert_eq!(fixed.execution().mana(A, Colourless), 4.into());
            let draw = activation(&played, 1, 0, &[(Colourless, 3)]);
            if !enabled {
                illegal(&played, A, draw);
                continue;
            }
            if empty_required {
                illegal(&played, A, activation(&played, 1, 0, &[(Green, 3)]));
            }
            let before = hand_size(&played, A);
            let paid = command(&played, A, draw).into_state();
            assert_eq!(physical(&paid, 1).tapped(), Some(true));
            assert_eq!(paid.execution().mana(A, Colourless), 0.into());
            let done = round(paid).into_state();
            assert_eq!(hand_size(&done, A), before + 1);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_basic_lands_enter_untapped_and_each_intrinsic_ability_adds_its_own_color() {
    use spellwake_engine::land::BasicLandType;
    for (name, subtype, color) in [
        ("Plains", BasicLandType::Plains, White),
        ("Island", BasicLandType::Island, Blue),
        ("Swamp", BasicLandType::Swamp, Black),
        ("Mountain", BasicLandType::Mountain, Red),
        ("Forest", BasicLandType::Forest, Green),
    ] {
        let initial = game(
            vec![A, B],
            vec![named(name, 1, A, ObjectDestination::Hand)],
            &[],
            Step::PrecombatMain,
        );
        let land = land_play(&initial, 1);
        let source = physical(&land, 1).id();
        let properties = sheet(&land, source);
        assert!(properties.current_supertypes.contains(&Supertype::Basic));
        assert!(
            properties
                .current_land_subtypes
                .contains(&subtype.land_type())
        );
        assert_eq!(physical(&land, 1).tapped(), Some(false));
        let add = GameCommand::ActivateMana {
            source,
            ability: ManaAbilityRef::BasicLand(subtype),
        };
        let done = command(&land, A, add.clone()).into_state();
        for kind in ManaType::ALL {
            assert_eq!(
                done.execution().mana(A, kind),
                if kind == color { 1.into() } else { 0.into() }
            );
        }
        assert_eq!(physical(&done, 1).tapped(), Some(true));
        illegal(&done, A, add);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_temple_of_the_false_god_counts_its_own_lands_at_the_five_land_threshold() {
    for without_temple in [3, 4] {
        let mut cards = vec![
            named("Temple of the False God", 1, A, ObjectDestination::Hand),
            named(
                "Forest",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
        ];
        cards.extend((3..3 + without_temple).map(|id| {
            named(
                "Forest",
                id,
                A,
                ObjectDestination::Battlefield { controller: A },
            )
        }));
        let start = game(vec![A, B], cards, &[], Step::PrecombatMain);
        let land = land_play(&start, 1);
        let add = tap_for_mana(&land, 1, 0);
        if without_temple < 4 {
            illegal(&land, A, add);
        } else {
            let done = command(&land, A, add).into_state();
            assert_eq!(done.execution().mana(A, Colourless), 2.into());
            assert_eq!(physical(&done, 1).tapped(), Some(true));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_firemind_vessel_enters_tapped_then_makes_exactly_two_distinct_colors() {
    let colors = [White, Blue, Black, Red, Green];
    let start = game(
        vec![A, B],
        vec![named("Firemind Vessel", 1, A, ObjectDestination::Hand)],
        &[(Colourless, 4)],
        Step::PrecombatMain,
    );
    let entered = round(cast_untargeted(&start, 1, &[(Colourless, 4)]).into_state()).into_state();
    let pay = |s: &GameState, kinds: Vec<(ManaType, i64)>| GameCommand::ActivateManaWithBundle {
        source: physical(s, 1).id(),
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        bundle: kinds.into_iter().map(|(k, n)| (k, n.into())).collect(),
    };
    assert_eq!(physical(&entered, 1).tapped(), Some(true));
    illegal(&entered, A, pay(&entered, vec![(Red, 1), (Blue, 1)]));
    let ready = exile_hand::until(entered, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    for invalid in [vec![(Red, 2)], vec![(Colourless, 1), (Blue, 1)]] {
        illegal(&ready, A, pay(&ready, invalid));
    }
    for (i, &a) in colors.iter().enumerate() {
        for &b in &colors[i + 1..] {
            let done = command(&ready, A, pay(&ready, vec![(a, 1), (b, 1)])).into_state();
            assert_eq!(physical(&done, 1).tapped(), Some(true));
            for k in ManaType::ALL {
                assert_eq!(
                    done.execution().mana(A, k),
                    if k == a || k == b { 1.into() } else { 0.into() }
                );
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ilysian_caryatid_and_sanctum_weaver_count_their_controllers_permanents_for_any_color() {
    let colors = [White, Blue, Black, Red, Green];
    for (name, weaver) in [("Ilysian Caryatid", false), ("Sanctum Weaver", true)] {
        for stronger in [false, true] {
            let mut cards = vec![
                named(name, 1, A, ObjectDestination::Hand),
                creature(2, A, if stronger { 4 } else { 3 }, 2, &[]),
                creature(3, B, 7, 7, &[]),
                seed(
                    4,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    Program::enchantment(None),
                ),
            ];
            if stronger {
                cards.extend((5..7).map(|id| {
                    seed(
                        id,
                        A,
                        ObjectDestination::Battlefield { controller: A },
                        Program::enchantment(None),
                    )
                }));
            }
            let start = game(
                vec![A, B],
                cards,
                &[(Colourless, 1), (Green, 1)],
                Step::PrecombatMain,
            );
            let entered =
                round(cast_untargeted(&start, 1, &[(Colourless, 1), (Green, 1)]).into_state())
                    .into_state();
            illegal(&entered, A, tap_for_mana(&entered, 1, 0));
            let ready = exile_hand::until(entered, |s| {
                s.turn().unwrap().position().number == 3
                    && s.turn().unwrap().position().step == Step::PrecombatMain
            });
            let choosing = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
            let RuleFrame::ManaColor(frame) = choosing.pending_decision().unwrap().frame() else {
                panic!("choose a color");
            };
            assert_eq!(frame.colors(), &colors.into());
            let amount: i64 = if stronger {
                if weaver { 3 } else { 2 }
            } else {
                1
            };
            for color in colors {
                let done = reply(&choosing, DecisionReply::ManaColor(color)).into_state();
                assert_eq!(done.execution().mana(A, color), amount.into());
                assert_eq!(physical(&done, 1).tapped(), Some(true));
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_reveal_lands_check_both_types_from_their_controllers_hand_and_offer_their_printed_colors() {
    for (name, types, colors) in [
        ("Fortified Village", ["Forest", "Plains"], [Green, White]),
        ("Frostboil Snarl", ["Island", "Mountain"], [Blue, Red]),
        ("Furycalm Snarl", ["Mountain", "Plains"], [Red, White]),
        ("Necroblossom Snarl", ["Swamp", "Forest"], [Black, Green]),
        ("Port Town", ["Plains", "Island"], [White, Blue]),
        ("Shineshadow Snarl", ["Plains", "Swamp"], [White, Black]),
    ] {
        let start = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                named(types[0], 2, A, ObjectDestination::Hand),
                named(types[1], 3, A, ObjectDestination::Hand),
                named("Wastes", 4, A, ObjectDestination::Hand),
                named(types[0], 5, B, ObjectDestination::Hand),
            ],
            &[],
            Step::PrecombatMain,
        );
        let reveal = |which: Option<u64>| GameCommand::PlayLandWithEntryReveal {
            land: physical(&start, 1).id(),
            reveal: which.map(|id| physical(&start, id).id()),
        };
        for id in [4, 5] {
            illegal(&start, A, reveal(Some(id)));
        }
        let tapped = command(&start, A, reveal(None)).into_state();
        assert_eq!(physical(&tapped, 1).tapped(), Some(true));
        illegal(&tapped, A, tap_for_mana(&tapped, 1, 0));
        for id in [2, 3] {
            let receipt = command(&start, A, reveal(Some(id)));
            let ready = receipt.into_state();
            assert_eq!(physical(&ready, id).zone(), ObjectZone::Hand(A));
            assert_eq!(physical(&ready, 1).tapped(), Some(false));
            assert_colors(
                &command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state(),
                &colors,
                0,
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_thriving_lands_enter_tapped_and_add_the_printed_color_or_the_other_color_chosen_on_entry() {
    let colors = [White, Blue, Black, Red, Green];
    for (name, fixed) in [
        ("Thriving Bluff", Red),
        ("Thriving Grove", Green),
        ("Thriving Heath", White),
        ("Thriving Isle", Blue),
        ("Thriving Moor", Black),
    ] {
        let start = game(
            vec![A, B],
            vec![named(name, 1, A, ObjectDestination::Hand)],
            &[],
            Step::PrecombatMain,
        );
        let play = |color| GameCommand::PlayLandWithEntryColor {
            land: physical(&start, 1).id(),
            color,
        };
        for invalid in [fixed, Colourless] {
            illegal(&start, A, play(invalid));
        }
        for other in colors.into_iter().filter(|c| *c != fixed) {
            let entered = command(&start, A, play(other)).into_state();
            assert_eq!(physical(&entered, 1).tapped(), Some(true));
            illegal(&entered, A, tap_for_mana(&entered, 1, 0));
            let ready = exile_hand::until(entered, |s| {
                s.turn().unwrap().position().number == 3
                    && s.turn().unwrap().position().step == Step::PrecombatMain
            });
            let choice = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
            assert_colors(&choice, &[fixed, other], 0);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_bounce_lands_return_one_controlled_land_and_tap_for_their_exact_mana_bundle() {
    for (name, colors) in [
        ("Golgari Rot Farm", [Black, Green]),
        ("Guildless Commons", [Colourless, Colourless]),
        ("Orzhov Basilica", [White, Black]),
        ("Selesnya Sanctuary", [Green, White]),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                named(
                    "Forest",
                    2,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
                named(
                    "Island",
                    3,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                ),
            ],
            &[],
            Step::PrecombatMain,
        );
        let entered = land_play(&initial, 1);
        assert_eq!(physical(&entered, 1).tapped(), Some(true));
        let choosing = round(entered).into_state();
        let RuleFrame::ResolutionSelect(frame) = choosing.pending_decision().unwrap().frame()
        else {
            panic!("choose a controlled land to return");
        };
        assert_eq!(
            frame.domain().iter().copied().collect::<BTreeSet<_>>(),
            [physical(&choosing, 1).id(), physical(&choosing, 2).id()].into()
        );
        let self_return = reply(
            &choosing,
            DecisionReply::SelectResolution(vec![physical(&choosing, 1).id()]),
        )
        .into_state();
        assert_eq!(physical(&self_return, 1).zone(), ObjectZone::Hand(A));
        let returned = reply(
            &choosing,
            DecisionReply::SelectResolution(vec![physical(&choosing, 2).id()]),
        )
        .into_state();
        assert_eq!(physical(&returned, 2).zone(), ObjectZone::Hand(A));
        let ready = exile_hand::until(returned, |s| {
            s.turn().unwrap().position().number == 3
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        let produced = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
        for color in ManaType::ALL {
            assert_eq!(
                produced.execution().mana(A, color),
                (colors.iter().filter(|c| **c == color).count() as i64).into()
            );
        }
        assert_eq!(physical(&produced, 1).tapped(), Some(true));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_krosan_verge_and_myriad_landscape_pay_sacrifice_and_find_their_distinct_land_pairs() {
    use spellwake_engine::effects::LibraryTopReply;
    for (name, matching_basics) in [("Krosan Verge", false), ("Myriad Landscape", true)] {
        let start = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                named(
                    "Sol Ring",
                    2,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
                seed(
                    3,
                    A,
                    ObjectDestination::LibraryBottom,
                    Program::artifact(None, vec![]),
                ),
                named("Forest", 4, A, ObjectDestination::LibraryBottom),
                named("Forest", 5, A, ObjectDestination::LibraryBottom),
                named("Plains", 6, A, ObjectDestination::LibraryBottom),
                named("Taiga", 7, A, ObjectDestination::LibraryBottom),
                named("Tundra", 8, A, ObjectDestination::LibraryBottom),
            ],
            &[],
            Step::PrecombatMain,
        );
        let played = land_play(&start, 1);
        assert_eq!(physical(&played, 1).tapped(), Some(true));
        illegal(&played, A, tap_for_mana(&played, 1, 0));
        let ready = exile_hand::until(played, |s| {
            s.turn().unwrap().position().number == 3
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        let mana = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
        assert_eq!(mana.execution().mana(A, Colourless), 1.into());
        let funded = command(&ready, A, tap_for_mana(&ready, 2, 0)).into_state();
        let paid = command(&funded, A, activation(&funded, 1, 0, &[(Colourless, 2)])).into_state();
        assert_eq!(physical(&paid, 1).zone(), ObjectZone::Graveyard(A));
        let search = round(paid).into_state();
        let RuleFrame::LibraryTop(frame) = search.pending_decision().unwrap().frame() else {
            panic!("search for land pair");
        };
        assert_eq!(
            frame
                .eligible_results()
                .contains(&physical(&search, 7).id()),
            !matching_basics
        );
        let ready = search
            .supply_entropy(search.basis(), vec![0; 16])
            .unwrap()
            .into_state();
        let choice = |selected: &[u64]| {
            DecisionReply::LibraryTop(LibraryTopReply::SearchBattlefield {
                selected_in_timestamp_order: selected
                    .iter()
                    .map(|n| physical(&ready, *n).id())
                    .collect(),
            })
        };
        let illegal_pair = if matching_basics { [4, 6] } else { [4, 5] };
        illegal(
            &ready,
            A,
            GameCommand::Reply {
                decision: ready.pending_decision().unwrap().id(),
                choice: choice(&illegal_pair),
            },
        );
        let pair = if matching_basics { [4, 5] } else { [7, 8] };
        for n in 0..=2 {
            let result = reply(&ready, choice(&pair[..n]));
            assert_eq!(result.shuffle_journal().len(), 1);
            for (index, &id) in pair.iter().enumerate() {
                let object = physical(result.state(), id);
                if index < n {
                    assert_eq!(object.zone(), ObjectZone::Battlefield);
                    assert_eq!(object.tapped(), Some(true));
                } else {
                    assert_eq!(object.zone(), ObjectZone::Library(A));
                }
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_arch_of_orazca_keeps_the_citys_blessing_after_falling_below_ten_permanents() {
    for count in [8, 9] {
        let mut cards = vec![
            named("Arch of Orazca", 1, A, ObjectDestination::Hand),
            creature(2, B, 1, 1, &[]),
            remove_spell(3, A),
        ];
        cards.extend((5..5 + count).map(|n| creature(n, A, 1, 1, &[])));
        let start = game(vec![A, B], cards, &[(Colourless, 5)], Step::PrecombatMain);
        let land = land_play(&start, 1);
        assert_eq!(land.execution().has_city_blessing(A), count == 9);
        let add = command(&land, A, tap_for_mana(&land, 1, 0)).into_state();
        assert_eq!(add.execution().mana(A, Colourless), 6.into());
        if count == 8 {
            illegal(&land, A, activation(&land, 1, 0, &[(Colourless, 5)]));
            continue;
        }
        let below =
            round(targeted(&land, A, 3, TargetRef::Object(physical(&land, 5).id())).into_state())
                .into_state();
        assert!(below.execution().has_city_blessing(A));
        let before = hand_size(&below, A);
        let paid = command(&below, A, activation(&below, 1, 0, &[(Colourless, 5)])).into_state();
        assert_eq!(physical(&paid, 1).tapped(), Some(true));
        assert_eq!(paid.execution().mana(A, Colourless), 0.into());
        let done = round(paid).into_state();
        assert_eq!(hand_size(&done, A), before + 1);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_tomb_of_the_spirit_dragon_counts_only_its_controllers_colorless_creatures() {
    let start = game(
        vec![A, B],
        vec![
            named("Tomb of the Spirit Dragon", 1, A, ObjectDestination::Hand),
            creature(2, A, 1, 1, &[]),
            creature(3, A, 1, 1, &[]),
            creature(4, B, 1, 1, &[]),
            named(
                "Blood Aspirant",
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(
                6,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::artifact(None, vec![]),
            ),
        ],
        &[(Colourless, 2)],
        Step::PrecombatMain,
    );
    let land = land_play(&start, 1);
    let fixed = command(&land, A, tap_for_mana(&land, 1, 0)).into_state();
    assert_eq!(fixed.execution().mana(A, Colourless), 3.into());
    let paid = command(&land, A, activation(&land, 1, 1, &[(Colourless, 2)])).into_state();
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    let result = round(paid).into_state();
    assert_eq!(result.execution().life_total(A), Some(&22.into()));
    assert_eq!(result.execution().life_total(B), Some(&20.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mage_ring_network_stores_counters_or_removes_any_available_number_for_colorless_mana() {
    use spellwake_engine::effects::CounterKind;
    let mut cards = draw_pile();
    cards.extend([
        named("Mage-Ring Network", 1, A, ObjectDestination::Hand),
        named(
            "Sol Ring",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        seed(
            3,
            B,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ),
    ]);
    let start = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let mut land = land_play(&start, 1);
    let source = physical(&land, 1).id();
    let basic = command(&land, A, tap_for_mana(&land, 1, 0)).into_state();
    assert_eq!(basic.execution().mana(A, Colourless), 1.into());
    for (turn, count) in [(1, 1), (3, 2)] {
        let ready = exile_hand::until(land, |s| {
            s.turn().unwrap().position().number == turn
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        let funded = command(&ready, A, tap_for_mana(&ready, 2, 0)).into_state();
        let paid = command(&funded, A, activation(&funded, 1, 2, &[(Colourless, 1)])).into_state();
        land = round(paid).into_state();
        assert_eq!(
            land.execution()
                .counters_of_kind(source, CounterKind::Storage),
            count.into()
        );
        assert_eq!(physical(&land, 1).tapped(), Some(true));
    }
    let ready = exile_hand::until(land, |s| {
        s.turn().unwrap().position().number == 5
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let remove = |n: i64| GameCommand::ActivateManaWithBundle {
        source,
        ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        bundle: if n == 0 {
            Default::default()
        } else {
            [(Colourless, n.into())].into()
        },
    };
    illegal(&ready, A, remove(3));
    for n in 0..=2 {
        let done = command(&ready, A, remove(n)).into_state();
        assert_eq!(done.execution().mana(A, Colourless), n.into());
        assert_eq!(
            done.execution()
                .counters_of_kind(source, CounterKind::Storage),
            (2 - n).into()
        );
        assert_eq!(physical(&done, 1).tapped(), Some(true));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_chosen_type_mana_sources_limit_their_colors_to_matching_creature_spells_and_abilities() {
    use spellwake_engine::{
        activated::ActivationOrigin,
        land::EntryChoice,
        mana::{ManaRestriction, RestrictedManaSpend},
    };
    let elf = current_catalogue().1.resolve_label("Elf").unwrap();
    for (name, is_land, ability_ok) in [
        ("Unclaimed Territory", true, false),
        ("Secluded Courtyard", true, true),
        ("Pillar of Origins", false, false),
    ] {
        let mut elf_program =
            Program::creature(fixed_cost("{1}"), 1.into(), 1.into(), [elf].into());
        elf_program.activated.push(ActivatedAbility {
            id: ActivatedAbilityId(0),
            mana_cost: ManaCost::generic(1.into()),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
                }],
            ),
        });
        let start = game(
            vec![A, B],
            vec![
                named(name, 1, A, ObjectDestination::Hand),
                seed(2, A, ObjectDestination::Hand, elf_program.clone()),
                seed(
                    3,
                    A,
                    ObjectDestination::Hand,
                    Program::creature(fixed_cost("{1}"), 1.into(), 1.into(), Default::default()),
                ),
                seed(
                    4,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    elf_program,
                ),
            ],
            if is_land { &[] } else { &[(Colourless, 2)] },
            Step::PrecombatMain,
        );
        let ask = if is_land {
            land_play(&start, 1)
        } else {
            round(cast_untargeted(&start, 1, &[(Colourless, 2)]).into_state()).into_state()
        };
        let ready = reply(
            &ask,
            DecisionReply::EntryChoice(EntryChoice::CreatureType(elf)),
        )
        .into_state();
        if is_land {
            let colorless = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
            assert_eq!(
                colorless.execution().unrestricted_mana(A, Colourless),
                1.into()
            );
        }
        let choose = command(&ready, A, tap_for_mana(&ready, 1, 1)).into_state();
        assert_colors(&choose, &[White, Blue, Black, Red, Green], 0);
        let funded = reply(&choose, DecisionReply::ManaColor(Red)).into_state();
        let restriction = if ability_ok {
            ManaRestriction::CreatureSpellOrAbilityOfType(Some(elf))
        } else {
            ManaRestriction::CreatureSpellOfType(Some(elf))
        };
        let amount = RestrictedManaSpend {
            mana_type: Red,
            restriction,
            amount: 1.into(),
        };
        assert_eq!(funded.execution().unrestricted_mana(A, Red), 0.into());
        let cast = |id| GameCommand::CastWithResources {
            spell: physical(&funded, id).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&[(Red, 1)])),
                restricted_mana: vec![amount.clone()],
                ..Default::default()
            },
        };
        illegal(&funded, A, cast(3));
        let matching = round(command(&funded, A, cast(2)).into_state()).into_state();
        assert_eq!(physical(&matching, 2).zone(), ObjectZone::Battlefield);
        let activate = GameCommand::ActivateWithRestrictedMana {
            source: physical(&funded, 4).id(),
            ability: ActivatedAbilityId(0),
            granting_effect: None,
            origin: ActivationOrigin::Untargeted,
            chosen_x: None,
            payment: Some(spend(&[(Red, 1)])),
            restricted_mana: vec![amount],
        };
        if ability_ok {
            let gained = round(command(&funded, A, activate).into_state()).into_state();
            assert_eq!(gained.execution().life_total(A), Some(&21.into()));
        } else {
            illegal(&funded, A, activate);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_chromatic_lantern_grants_each_controlled_land_all_colors_and_removes_the_grant_on_departure()
{
    let initial = game(
        vec![A, B],
        vec![
            named("Chromatic Lantern", 1, A, ObjectDestination::Hand),
            named(
                "Wastes",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Island",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(4, A, 1, 1, &[]),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
        ],
        &[(Colourless, 3)],
        Step::PrecombatMain,
    );
    let ready = round(cast_untargeted(&initial, 1, &[(Colourless, 3)]).into_state()).into_state();
    let key = *sheet(&ready, physical(&ready, 2).id())
        .granted_any_color_mana
        .first()
        .unwrap();
    for n in [3, 4] {
        assert!(
            sheet(&ready, physical(&ready, n).id())
                .granted_any_color_mana
                .is_empty()
        );
    }
    let colors = [White, Blue, Black, Red, Green];
    for (n, ability) in [
        (1, ManaAbilityRef::Printed(ManaAbilityId(0))),
        (2, ManaAbilityRef::GrantedAnyColor(key)),
    ] {
        let choosing = command(
            &ready,
            A,
            GameCommand::ActivateMana {
                source: physical(&ready, n).id(),
                ability,
            },
        )
        .into_state();
        let RuleFrame::ManaColor(frame) = choosing.pending_decision().unwrap().frame() else {
            panic!("all color choices");
        };
        assert_eq!(frame.colors(), &colors.into());
        for color in colors {
            let done = reply(&choosing, DecisionReply::ManaColor(color)).into_state();
            assert_eq!(done.execution().mana(A, color), 1.into());
            assert_eq!(physical(&done, n).tapped(), Some(true));
        }
    }
    let destroyed =
        round(targeted(&ready, A, 5, TargetRef::Object(physical(&ready, 1).id())).into_state())
            .into_state();
    assert!(
        sheet(&destroyed, physical(&destroyed, 2).id())
            .granted_any_color_mana
            .is_empty()
    );
    illegal(
        &destroyed,
        A,
        GameCommand::ActivateMana {
            source: physical(&destroyed, 2).id(),
            ability: ManaAbilityRef::GrantedAnyColor(key),
        },
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_eldrazi_temple_and_shrine_of_the_forsaken_gods_enforce_their_land_and_spending_restrictions()
{
    use spellwake_engine::{
        activated::ActivationOrigin,
        mana::{ManaRestriction, RestrictedManaSpend},
    };
    let eldrazi = current_catalogue().1.resolve_label("Eldrazi").unwrap();
    for (name, requirement, rule) in [
        (
            "Eldrazi Temple",
            0,
            ManaRestriction::ColorlessEldraziSpellOrAbility,
        ),
        (
            "Shrine of the Forsaken Gods",
            6,
            ManaRestriction::ColorlessSpell,
        ),
    ] {
        for other_lands in [0, 6] {
            let mut ordinary =
                Program::creature(fixed_cost("{2}"), 1.into(), 1.into(), [eldrazi].into());
            ordinary.activated.push(ActivatedAbility {
                id: ActivatedAbilityId(0),
                mana_cost: ManaCost::generic(2.into()),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustControllerLife {
                        adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
                    }],
                ),
            });
            let mut colored = ordinary.clone();
            colored.effect_defined_colors = Some([Color::Blue].into());
            let mut cards = vec![
                named(name, 1, A, ObjectDestination::Hand),
                seed(2, A, ObjectDestination::Hand, ordinary.clone()),
                seed(
                    3,
                    A,
                    ObjectDestination::Hand,
                    Program::artifact(fixed_cost("{2}"), vec![]),
                ),
                seed(4, A, ObjectDestination::Hand, colored.clone()),
                seed(
                    5,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    ordinary,
                ),
                seed(
                    6,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    colored,
                ),
            ];
            cards.extend((10..10 + other_lands).map(|n| {
                named(
                    "Wastes",
                    n,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                )
            }));
            cards.push(named(
                "Wastes",
                20,
                B,
                ObjectDestination::Battlefield { controller: B },
            ));
            let start = game(vec![A, B], cards, &[], Step::PrecombatMain);
            let ready = land_play(&start, 1);
            let single = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
            assert_eq!(
                single.execution().unrestricted_mana(A, Colourless),
                1.into()
            );
            if other_lands < requirement {
                illegal(&ready, A, tap_for_mana(&ready, 1, 1));
                continue;
            }
            let funded = command(&ready, A, tap_for_mana(&ready, 1, 1)).into_state();
            assert_eq!(funded.execution().mana(A, Colourless), 2.into());
            assert_eq!(
                funded.execution().unrestricted_mana(A, Colourless),
                0.into()
            );
            let amount = RestrictedManaSpend {
                mana_type: Colourless,
                restriction: rule.clone(),
                amount: 2.into(),
            };
            let cast = |n| GameCommand::CastWithResources {
                spell: physical(&funded, n).id(),
                target: None,
                payment: CastingPayment {
                    mana: Some(spend(&[(Colourless, 2)])),
                    restricted_mana: vec![amount.clone()],
                    ..Default::default()
                },
            };
            illegal(&funded, A, cast(4));
            let allowed = round(command(&funded, A, cast(2)).into_state()).into_state();
            assert_eq!(physical(&allowed, 2).zone(), ObjectZone::Battlefield);
            if rule == ManaRestriction::ColorlessSpell {
                let artifact = round(command(&funded, A, cast(3)).into_state()).into_state();
                assert_eq!(physical(&artifact, 3).zone(), ObjectZone::Battlefield);
            } else {
                illegal(&funded, A, cast(3));
            }
            let activate = |n| GameCommand::ActivateWithRestrictedMana {
                source: physical(&funded, n).id(),
                ability: ActivatedAbilityId(0),
                granting_effect: None,
                origin: ActivationOrigin::Untargeted,
                chosen_x: None,
                payment: Some(spend(&[(Colourless, 2)])),
                restricted_mana: vec![amount.clone()],
            };
            illegal(&funded, A, activate(6));
            if rule == ManaRestriction::ColorlessEldraziSpellOrAbility {
                let done = round(command(&funded, A, activate(5)).into_state()).into_state();
                assert_eq!(done.execution().life_total(A), Some(&21.into()));
            } else {
                illegal(&funded, A, activate(5));
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_jeweled_lotus_taps_and_sacrifices_for_three_of_one_color_restricted_to_its_owners_commander()
{
    use spellwake_engine::mana::{ManaRestriction, RestrictedManaSpend};
    let game = partner_mana_game(
        "Jeweled Lotus",
        vec![seed(
            2,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(fixed_cost("{2}"), vec![]),
        )],
    );
    let lotus = round(cast_untargeted(&game, 1, &[]).into_state()).into_state();
    let choose = command(&lotus, A, tap_for_mana(&lotus, 1, 0)).into_state();
    let RuleFrame::ManaColor(frame) = choose.pending_decision().unwrap().frame() else {
        panic!("choose one mana color");
    };
    let colors = [White, Blue, Black, Red, Green];
    assert_eq!(frame.colors(), &colors.into());
    for color in colors {
        let funded = reply(&choose, DecisionReply::ManaColor(color)).into_state();
        assert_eq!(physical(&funded, 1).zone(), ObjectZone::Graveyard(A));
        assert_eq!(funded.execution().mana(A, color), 3.into());
        assert_eq!(funded.execution().unrestricted_mana(A, color), 0.into());
        let cast = |n| GameCommand::CastWithResources {
            spell: physical(&funded, n).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&[(color, 2)])),
                restricted_mana: vec![RestrictedManaSpend {
                    mana_type: color,
                    restriction: ManaRestriction::CommanderSpell,
                    amount: 2.into(),
                }],
                ..Default::default()
            },
        };
        illegal(&funded, A, cast(2));
        if color == White {
            let commander = round(command(&funded, A, cast(99)).into_state()).into_state();
            assert_eq!(physical(&commander, 99).zone(), ObjectZone::Battlefield);
            assert_eq!(commander.execution().mana(A, color), 1.into());
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spectral_searchlight_allows_each_chosen_player_to_choose_and_receive_their_own_mana() {
    let start = game(
        vec![A, B, C],
        vec![named("Spectral Searchlight", 1, A, ObjectDestination::Hand)],
        &[(Colourless, 3)],
        Step::PrecombatMain,
    );
    let ready = round(cast_untargeted(&start, 1, &[(Colourless, 3)]).into_state()).into_state();
    for player in [A, B, C] {
        let choose = command(
            &ready,
            A,
            GameCommand::ActivateManaForPlayer {
                source: physical(&ready, 1).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
                recipient: player,
            },
        )
        .into_state();
        let pending = choose.pending_decision().unwrap();
        assert_eq!(pending.chooser(), player);
        let RuleFrame::ManaColor(frame) = pending.frame() else {
            panic!("recipient's mana color");
        };
        let colors = [White, Blue, Black, Red, Green];
        assert_eq!(frame.colors(), &colors.into());
        for color in colors {
            let received = reply(&choose, DecisionReply::ManaColor(color)).into_state();
            assert_eq!(physical(&received, 1).tapped(), Some(true));
            assert!(received.execution().window().entries().is_empty());
            for seat in [A, B, C] {
                assert_eq!(
                    received.execution().mana(seat, color),
                    i64::from(seat == player).into()
                );
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_honor_worn_shaku_untaps_by_tapping_only_an_untapped_controlled_legendary_permanent() {
    let start = game(
        vec![A, B],
        vec![
            named("Honor-Worn Shaku", 1, A, ObjectDestination::Hand),
            named(
                "Yargle, Glutton of Urborg",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Yargle, Glutton of Urborg",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(4, A, 1, 1, &[]),
            named(
                "The Great Henge",
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[(Colourless, 3)],
        Step::PrecombatMain,
    );
    let cast = round(cast_untargeted(&start, 1, &[(Colourless, 3)]).into_state()).into_state();
    let tapped = command(&cast, A, tap_for_mana(&cast, 1, 0)).into_state();
    assert_eq!(tapped.execution().mana(A, Colourless), 1.into());
    let untap = |n| GameCommand::ActivateWithCreatures {
        source: physical(&tapped, 1).id(),
        ability: ActivatedAbilityId(0),
        creatures: vec![physical(&tapped, n).id()],
    };
    for id in [1, 3, 4] {
        illegal(&tapped, A, untap(id));
    }
    let noncreature = round(command(&tapped, A, untap(5)).into_state()).into_state();
    assert_eq!(physical(&noncreature, 5).tapped(), Some(true));
    assert_eq!(physical(&noncreature, 1).tapped(), Some(false));
    let paid = command(&tapped, A, untap(2)).into_state();
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    assert_eq!(physical(&paid, 2).tapped(), Some(true));
    illegal(&paid, A, untap(2));
    let ready = round(paid).into_state();
    assert_eq!(physical(&ready, 1).tapped(), Some(false));
    let again = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
    assert_eq!(again.execution().mana(A, Colourless), 2.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_war_room_charges_life_for_the_union_of_its_controllers_commander_colors_before_drawing() {
    use spellwake_engine::mana::{ManaAbility, ManaOutput};
    let mut rock = Program::artifact(Some(ManaCost::free()), vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed([(Colourless, 3.into())].into()),
    });
    let start = partner_mana_game(
        "War Room",
        vec![seed(2, A, ObjectDestination::LibraryBottom, rock)],
    );
    let cast = round(cast_untargeted(&start, 2, &[]).into_state()).into_state();
    let land = land_play(&cast, 1);
    let plain = command(&land, A, tap_for_mana(&land, 1, 0)).into_state();
    assert_eq!(plain.execution().mana(A, Colourless), 1.into());
    let funded = command(&land, A, tap_for_mana(&land, 2, 0)).into_state();
    let hand = hand_size(&funded, A);
    let paid = command(&funded, A, activation(&funded, 1, 0, &[(Colourless, 3)])).into_state();
    assert_eq!(paid.execution().life_total(A), Some(&38.into()));
    assert_eq!(paid.execution().life_total(B), Some(&40.into()));
    assert_eq!(paid.execution().mana(A, Colourless), 0.into());
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    assert_eq!(hand_size(&paid, A), hand);
    let done = round(paid).into_state();
    assert_eq!(hand_size(&done, A), hand + 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rogues_passage_and_suspicious_bookcase_make_one_creature_unblockable_until_cleanup() {
    use spellwake_engine::characteristics::KeywordAbility::Defender;
    for (name, land, price) in [
        ("Rogue's Passage", true, 4),
        ("Suspicious Bookcase", false, 3),
    ] {
        let mut cards = draw_pile();
        cards.extend([
            named(name, 1, A, ObjectDestination::Hand),
            creature(2, A, 3, 2, &[]),
            creature(3, B, 2, 2, &[]),
            named(
                "Fervor",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(5, A, 1, 1, &[]),
        ]);
        let budget = price + if land { 0 } else { 2 };
        let start = game(
            vec![A, B],
            cards,
            &[(Colourless, budget)],
            Step::PrecombatMain,
        );
        let ready = if land {
            land_play(&start, 1)
        } else {
            round(cast_untargeted(&start, 1, &[(Colourless, 2)]).into_state()).into_state()
        };
        if land {
            let tapped = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
            assert_eq!(tapped.execution().mana(A, Colourless), (price + 1).into());
        } else {
            assert!(sheet(&ready, physical(&ready, 1).id()).has_keyword(Defender));
            let attack = combat_frame(&ready, true);
            illegal(
                &attack,
                A,
                GameCommand::Reply {
                    decision: attack.pending_decision().unwrap().id(),
                    choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                        attacker: physical(&attack, 1).id(),
                        defender: B,
                        defending_planeswalker: None,
                    }]),
                },
            );
        }
        let effect = |id| GameCommand::ActivateAbility {
            source: physical(&ready, 1).id(),
            ability: ActivatedAbilityId(0),
            target: TargetRef::Object(physical(&ready, id).id()),
            payment: Some(spend(&[(Colourless, price)])),
        };
        illegal(&ready, A, effect(4));
        let their_creature = round(command(&ready, A, effect(3)).into_state()).into_state();
        assert!(
            sheet(&their_creature, physical(&their_creature, 3).id())
                .blocking_restrictions
                .contains(&spellwake_engine::characteristics::BlockingRestriction::CannotBeBlocked)
        );
        let ours = round(command(&ready, A, effect(2)).into_state()).into_state();
        assert_eq!(physical(&ours, 1).tapped(), Some(true));
        assert!(
            sheet(&ours, physical(&ours, 2).id())
                .blocking_restrictions
                .contains(&spellwake_engine::characteristics::BlockingRestriction::CannotBeBlocked)
        );
        assert!(
            !sheet(&ours, physical(&ours, 5).id())
                .blocking_restrictions
                .contains(&spellwake_engine::characteristics::BlockingRestriction::CannotBeBlocked)
        );
        let attack = combat_frame(&ours, true);
        let attacking = reply(
            &attack,
            DecisionReply::DeclareAttackers(
                [2, 5]
                    .into_iter()
                    .map(|n| AttackDeclaration {
                        attacker: physical(&attack, n).id(),
                        defender: B,
                        defending_planeswalker: None,
                    })
                    .collect(),
            ),
        )
        .into_state();
        let blocks = combat_frame(&attacking, false);
        illegal(
            &blocks,
            B,
            GameCommand::Reply {
                decision: blocks.pending_decision().unwrap().id(),
                choice: DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                    blocker: physical(&blocks, 3).id(),
                    attacker: physical(&blocks, 2).id(),
                }]),
            },
        );
        let clear = reply(
            &blocks,
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker: physical(&blocks, 3).id(),
                attacker: physical(&blocks, 5).id(),
            }]),
        )
        .into_state();
        let future = exile_hand::until(clear, |s| {
            s.turn().unwrap().position().number == 2
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        assert_eq!(future.execution().life_total(B), Some(&17.into()));
        assert!(
            !sheet(&future, physical(&future, 2).id())
                .blocking_restrictions
                .contains(&spellwake_engine::characteristics::BlockingRestriction::CannotBeBlocked)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_arcane_lighthouse_suppresses_opposing_hexproof_and_shroud_through_cleanup_including_later_grants()
 {
    use spellwake_engine::{
        characteristics::KeywordAbility::{Hexproof, Shroud},
        program::{EffectSubject, ObjectFilter},
    };
    let start = game(
        vec![A, B, C],
        vec![
            named("Arcane Lighthouse", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[Hexproof, Shroud]),
            creature(3, B, 2, 2, &[Hexproof]),
            creature(4, C, 2, 2, &[Shroud]),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    Instruction::ModifyGroupUntilEndOfTurn {
                        group: Box::new(EffectSubject::Matching(ObjectFilter::creature())),
                        power_toughness: None,
                        keywords: [Hexproof, Shroud].into(),
                    },
                ),
            ),
            remove_spell(6, A),
        ],
        &[(Colourless, 1)],
        Step::PrecombatMain,
    );
    let land = land_play(&start, 1);
    let mana = command(&land, A, tap_for_mana(&land, 1, 0)).into_state();
    assert_eq!(mana.execution().mana(A, Colourless), 2.into());
    let target = |s: &GameState, n| GameCommand::Cast {
        spell: physical(s, 6).id(),
        target: TargetRef::Object(physical(s, n).id()),
    };
    for n in [3, 4] {
        illegal(&land, A, target(&land, n));
    }
    let paid = command(&land, A, activation(&land, 1, 0, &[(Colourless, 1)])).into_state();
    assert_eq!(physical(&paid, 1).tapped(), Some(true));
    let resolved = round(paid).into_state();
    let granted = round(cast_untargeted(&resolved, 5, &[]).into_state()).into_state();
    for k in [Hexproof, Shroud] {
        assert!(sheet(&granted, physical(&granted, 2).id()).has_keyword(k));
        for n in [3, 4] {
            assert!(!sheet(&granted, physical(&granted, n).id()).has_keyword(k));
        }
    }
    for n in [3, 4] {
        let destroyed = round(command(&granted, A, target(&granted, n)).into_state()).into_state();
        assert_eq!(
            physical(&destroyed, n).zone(),
            ObjectZone::Graveyard(if n == 3 { B } else { C })
        );
    }
    let expired = exile_hand::until(granted, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    assert!(sheet(&expired, physical(&expired, 3).id()).has_keyword(Hexproof));
    assert!(sheet(&expired, physical(&expired, 4).id()).has_keyword(Shroud));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_forsaken_monument_buffs_its_colorless_creatures_adds_one_extra_colorless_and_gains_life_per_colorless_cast()
 {
    let initial = game(
        vec![A, B],
        vec![
            named("Forsaken Monument", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 3, &[]),
            creature(3, B, 2, 3, &[]),
            named(
                "Blood Aspirant",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Sol Ring",
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Forest",
                6,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(
                7,
                A,
                ObjectDestination::Hand,
                Program::artifact(Some(ManaCost::free()), vec![]),
            ),
            seed(
                8,
                A,
                ObjectDestination::Hand,
                Program::creature(fixed_cost("{G}"), 1.into(), 1.into(), Default::default()),
            ),
        ],
        &[(Colourless, 5)],
        Step::PrecombatMain,
    );
    let ready = round(cast_untargeted(&initial, 1, &[(Colourless, 5)]).into_state()).into_state();
    assert_size(&ready, physical(&ready, 2).id(), 4, 5);
    assert_size(&ready, physical(&ready, 3).id(), 2, 3);
    assert_size(&ready, physical(&ready, 4).id(), 1, 1);
    let trigger = command(&ready, A, tap_for_mana(&ready, 5, 0)).into_state();
    let RuleFrame::ManaColor(color) = trigger.pending_decision().unwrap().frame() else {
        panic!("additional colorless mana");
    };
    assert_eq!(color.colors(), &[Colourless].into());
    let extra = reply(&trigger, DecisionReply::ManaColor(Colourless)).into_state();
    assert_eq!(extra.execution().mana(A, Colourless), 3.into());
    let green = command(
        &ready,
        A,
        GameCommand::ActivateMana {
            source: physical(&ready, 6).id(),
            ability: ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Forest),
        },
    )
    .into_state();
    assert_eq!(green.execution().mana(A, Green), 1.into());
    assert_eq!(green.execution().mana(A, Colourless), 0.into());
    let colored = round(cast_untargeted(&green, 8, &[(Green, 1)]).into_state()).into_state();
    assert!(colored.execution().window().entries().is_empty());
    assert_eq!(colored.execution().life_total(A), Some(&20.into()));
    let cast = cast_untargeted(&ready, 7, &[]).into_state();
    assert_eq!(cast.execution().window().entries().len(), 2);
    let life = round(cast).into_state();
    assert_eq!(life.execution().life_total(A), Some(&22.into()));
    assert_eq!(life.execution().life_total(B), Some(&20.into()));
    let artifact = round(life).into_state();
    assert_eq!(physical(&artifact, 7).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_the_great_henge_uses_the_greatest_power_for_its_price_then_produces_mana_life_and_nontoken_entry_benefits()
 {
    let mut cards = draw_pile();
    cards.extend([
        named("The Great Henge", 1, A, ObjectDestination::Hand),
        creature(2, A, 5, 5, &[]),
        creature(3, A, 2, 2, &[]),
        creature(4, B, 9, 9, &[]),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            Program::creature(
                Some(ManaCost::free()),
                1.into(),
                1.into(),
                Default::default(),
            ),
        ),
        remove_spell(6, A),
        named(
            "Carrier Thrall",
            7,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ]);
    let start = game(
        vec![A, B],
        cards,
        &[(Colourless, 2), (Green, 2)],
        Step::PrecombatMain,
    );
    illegal(
        &start,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&start, 1).id(),
            payment: Some(spend(&[(Green, 2)])),
        },
    );
    let ready =
        round(cast_untargeted(&start, 1, &[(Colourless, 2), (Green, 2)]).into_state()).into_state();
    let funded = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
    assert_eq!(funded.execution().mana(A, Green), 2.into());
    assert_eq!(funded.execution().life_total(A), Some(&22.into()));
    assert_eq!(physical(&funded, 1).tapped(), Some(true));
    let death =
        round(targeted(&funded, A, 6, TargetRef::Object(physical(&funded, 7).id())).into_state())
            .into_state();
    let created = round(death);
    let token = tokens(&created, A, 1)[0];
    let after_token = created.into_state();
    assert_eq!(sheet(&after_token, token).plus_one_counters, 0.into());
    assert!(after_token.execution().window().entries().is_empty());
    let new = round(cast_untargeted(&after_token, 5, &[]).into_state()).into_state();
    assert_eq!(new.execution().window().entries().len(), 1);
    let hand = hand_size(&new, A);
    let done = round(new).into_state();
    assert_eq!(
        sheet(&done, physical(&done, 5).id()).plus_one_counters,
        1.into()
    );
    assert_size(&done, physical(&done, 5).id(), 2, 2);
    assert_eq!(hand_size(&done, A), hand + 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_omnath_locus_of_mana_retains_only_green_across_steps_and_phases_and_changes_size_as_it_is_spent()
 {
    let start = game(
        vec![A, B],
        vec![
            named("Omnath, Locus of Mana", 1, A, ObjectDestination::Hand),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::creature(fixed_cost("{G}"), 1.into(), 1.into(), Default::default()),
            ),
            remove_spell(3, A),
        ],
        &[(Green, 4), (Colourless, 2), (Red, 2)],
        Step::PrecombatMain,
    );
    let entered =
        round(cast_untargeted(&start, 1, &[(Green, 1), (Colourless, 2)]).into_state()).into_state();
    assert_size(&entered, physical(&entered, 1).id(), 4, 4);
    let paid = round(cast_untargeted(&entered, 2, &[(Green, 1)]).into_state()).into_state();
    assert_size(&paid, physical(&paid, 1).id(), 3, 3);
    let combat = exile_hand::until(paid, |s| {
        s.turn().unwrap().position().step == Step::BeginningCombat
    });
    assert_eq!(combat.execution().mana(A, Green), 2.into());
    assert_eq!(combat.execution().mana(A, Red), 0.into());
    let later = exile_hand::until(combat, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(later.execution().mana(A, Green), 2.into());
    assert_size(&later, physical(&later, 1).id(), 3, 3);
    let gone =
        round(targeted(&later, A, 3, TargetRef::Object(physical(&later, 1).id())).into_state())
            .into_state();
    let main = exile_hand::until(gone, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(main.execution().mana(A, Green), 0.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_miraris_wake_buffs_only_controlled_creatures_and_adds_one_of_the_mana_types_a_controlled_land_produced()
 {
    use spellwake_engine::land::BasicLandType;
    let initial = game(
        vec![A, B],
        vec![
            named("Mirari's Wake", 1, A, ObjectDestination::Hand),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            named(
                "Selesnya Sanctuary",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Forest",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Sol Ring",
                6,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[(Colourless, 3), (Green, 1), (White, 1)],
        Step::PrecombatMain,
    );
    let field = round(
        cast_untargeted(&initial, 1, &[(Colourless, 3), (Green, 1), (White, 1)]).into_state(),
    )
    .into_state();
    assert_size(&field, physical(&field, 2).id(), 3, 3);
    assert_size(&field, physical(&field, 3).id(), 2, 2);
    let choose = command(&field, A, tap_for_mana(&field, 4, 0)).into_state();
    let RuleFrame::ManaColor(frame) = choose.pending_decision().unwrap().frame() else {
        panic!("land's two produced mana types");
    };
    assert_eq!(frame.colors(), &[Green, White].into());
    for bonus in [Green, White] {
        let funded = reply(&choose, DecisionReply::ManaColor(bonus)).into_state();
        for color in [Green, White] {
            assert_eq!(
                funded.execution().mana(A, color),
                (1 + i64::from(color == bonus)).into()
            );
        }
    }
    let rock = command(&field, A, tap_for_mana(&field, 6, 0)).into_state();
    assert_eq!(rock.execution().mana(A, Colourless), 2.into());
    assert!(rock.pending_decision().is_none());
    let priority = pass(&field).into_state();
    let forest = command(
        &priority,
        B,
        GameCommand::ActivateMana {
            source: physical(&priority, 5).id(),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    )
    .into_state();
    assert_eq!(forest.execution().mana(B, Green), 1.into());
    assert!(forest.pending_decision().is_none());
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_ruins_of_oran_rief_enters_tapped_then_adds_colorless_or_counters_a_colorless_creature_from_this_turn()
 {
    let make = || {
        Program::creature(
            Some(ManaCost::free()),
            1.into(),
            1.into(),
            Default::default(),
        )
    };
    let mut colored = make();
    colored.color_indicator = Some([Color::Green].into());
    let mut cards = draw_pile();
    cards.extend([
        named("Ruins of Oran-Rief", 1, A, ObjectDestination::Hand),
        seed(2, A, ObjectDestination::Hand, make()),
        seed(3, A, ObjectDestination::Hand, colored),
        creature(4, A, 1, 1, &[]),
    ]);
    let start = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let land = land_play(&start, 1);
    assert_eq!(physical(&land, 1).tapped(), Some(true));
    illegal(&land, A, tap_for_mana(&land, 1, 0));
    let ready = exile_hand::until(land, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let mana = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
    assert_eq!(mana.execution().mana(A, Colourless), 1.into());
    let first = round(cast_untargeted(&ready, 2, &[]).into_state()).into_state();
    let all = round(cast_untargeted(&first, 3, &[]).into_state()).into_state();
    let target = |n| GameCommand::ActivateAbility {
        source: physical(&all, 1).id(),
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(physical(&all, n).id()),
        payment: None,
    };
    for n in [1, 3, 4] {
        illegal(&all, A, target(n));
    }
    let done = round(command(&all, A, target(2)).into_state()).into_state();
    assert_eq!(
        sheet(&done, physical(&done, 2).id()).plus_one_counters,
        1.into()
    );
    assert_eq!(physical(&done, 1).tapped(), Some(true));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_tyrite_sanctum_permanently_adds_god_to_a_legend_then_can_sacrifice_itself_for_an_indestructible_counter()
 {
    use spellwake_engine::{
        effects::CounterKind,
        mana::{ManaAbility, ManaOutput},
    };
    let mut rock = Program::artifact(None, vec![]);
    rock.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::Fixed([(Colourless, 4.into())].into()),
    });
    let mut cards = draw_pile();
    cards.extend([
        named("Tyrite Sanctum", 1, A, ObjectDestination::Hand),
        named(
            "Yargle, Glutton of Urborg",
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
        creature(3, A, 1, 1, &[]),
        seed(4, A, ObjectDestination::Battlefield { controller: A }, rock),
        remove_spell(5, A),
    ]);
    let start = game(vec![A, B], cards, &[(Colourless, 2)], Step::PrecombatMain);
    let field = land_play(&start, 1);
    let basic = command(&field, A, tap_for_mana(&field, 1, 0)).into_state();
    assert_eq!(basic.execution().mana(A, Colourless), 3.into());
    let activate = |s: &GameState, id, ability, n| GameCommand::ActivateAbility {
        source: physical(s, 1).id(),
        ability: ActivatedAbilityId(ability),
        target: TargetRef::Object(physical(s, id).id()),
        payment: Some(spend(&[(Colourless, n)])),
    };
    illegal(&field, A, activate(&field, 3, 0, 2));
    let raised = round(command(&field, A, activate(&field, 2, 0, 2)).into_state()).into_state();
    let vocab = current_catalogue().1;
    let target = physical(&raised, 2).id();
    for kind in ["God", "Frog", "Spirit"] {
        assert!(
            sheet(&raised, target)
                .current_creature_subtypes
                .contains(&vocab.resolve_label(kind).unwrap())
        );
    }
    assert_eq!(sheet(&raised, target).plus_one_counters, 1.into());
    let next = exile_hand::until(raised, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let funded = command(&next, A, tap_for_mana(&next, 4, 0)).into_state();
    illegal(&funded, A, activate(&funded, 3, 1, 4));
    let offered = command(&funded, A, activate(&funded, 2, 1, 4)).into_state();
    assert_eq!(physical(&offered, 1).zone(), ObjectZone::Graveyard(A));
    let shielded = round(offered).into_state();
    assert_eq!(
        shielded
            .execution()
            .counters_of_kind(target, CounterKind::Indestructible),
        1.into()
    );
    let spared =
        round(targeted(&shielded, A, 5, TargetRef::Object(target)).into_state()).into_state();
    assert_eq!(physical(&spared, 2).zone(), ObjectZone::Battlefield);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_forge_of_heroes_counters_a_commander_that_entered_this_turn_according_to_its_current_types()
{
    use spellwake_engine::{
        effects::CounterKind,
        mana::{ManaAbility, ManaOutput},
    };
    for (name, price, kind) in [
        (
            "Keleth, Sunmane Familiar",
            vec![(Colourless, 1), (White, 1)],
            CounterKind::PlusOne,
        ),
        (
            "Teferi, Temporal Archmage",
            vec![(Colourless, 4), (Blue, 2)],
            CounterKind::Loyalty,
        ),
    ] {
        let mut rock = Program::artifact(Some(ManaCost::free()), vec![]);
        rock.mana.push(ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Fixed(price.iter().map(|&(c, n)| (c, n.into())).collect()),
        });
        let start = commander_opening(
            vec![
                named("Forge of Heroes", 1, A, ObjectDestination::LibraryBottom),
                seed(2, A, ObjectDestination::LibraryBottom, rock),
                seed(
                    3,
                    A,
                    ObjectDestination::LibraryBottom,
                    Program::creature(
                        Some(ManaCost::free()),
                        1.into(),
                        1.into(),
                        Default::default(),
                    ),
                ),
                named(name, 100, A, ObjectDestination::Command),
                named("Verdeloth the Ancient", 200, B, ObjectDestination::Command),
            ],
            None,
        );
        let land = land_play(&start, 1);
        let plain = command(&land, A, tap_for_mana(&land, 1, 0)).into_state();
        assert_eq!(plain.execution().mana(A, Colourless), 1.into());
        let rock = round(cast_untargeted(&land, 2, &[]).into_state()).into_state();
        let funded = command(&rock, A, tap_for_mana(&rock, 2, 0)).into_state();
        let creature = round(cast_untargeted(&funded, 3, &[]).into_state()).into_state();
        let paid = round(cast_untargeted(&creature, 100, &price).into_state()).into_state();
        let choose = |s: &GameState, n| GameCommand::ActivatePermanentTargets {
            source: physical(s, 1).id(),
            ability: ActivatedAbilityId(0),
            targets: vec![TargetRef::Object(physical(s, n).id())],
            payment: None,
        };
        illegal(&paid, A, choose(&paid, 3));
        let commander = physical(&paid, 100).id();
        let before = paid.execution().counters_of_kind(commander, kind);
        let done = round(command(&paid, A, choose(&paid, 100)).into_state()).into_state();
        assert_eq!(
            done.execution().counters_of_kind(commander, kind),
            before.plus(&1.into())
        );
        let next = exile_hand::until(paid, |s| {
            s.turn().unwrap().position().number == 3
                && s.turn().unwrap().position().step == Step::PrecombatMain
        });
        illegal(&next, A, choose(&next, 100));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_crash_of_rhino_beetles_and_molimo_recalculate_their_size_from_only_their_controllers_lands()
{
    for (name, price, small, large) in [
        (
            "Crash of Rhino Beetles",
            vec![(Colourless, 4), (Green, 1)],
            5,
            15,
        ),
        (
            "Molimo, Maro-Sorcerer",
            vec![(Colourless, 4), (Green, 3)],
            9,
            10,
        ),
    ] {
        let mut cards = vec![
            named(name, 1, A, ObjectDestination::Hand),
            named("Forest", 2, A, ObjectDestination::Hand),
            named(
                "Island",
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
            ),
        ];
        cards.extend((21..30).map(|n| {
            named(
                "Forest",
                n,
                A,
                ObjectDestination::Battlefield { controller: A },
            )
        }));
        let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let entered = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
        let id = physical(&entered, 1).id();
        assert!(sheet(&entered, id).has_keyword(Trample));
        assert_size(&entered, id, small, small);
        let ten = land_play(&entered, 2);
        assert_size(&ten, id, large, large);
        let nine =
            round(targeted(&ten, A, 4, TargetRef::Object(physical(&ten, 2).id())).into_state())
                .into_state();
        assert_size(&nine, id, small, small);
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_filigree_attendant_counts_itself_and_other_controlled_artifacts_and_recalculates_when_an_artifact_leaves()
 {
    let cards = vec![
        named("Filigree Attendant", 1, A, ObjectDestination::Hand),
        seed(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(None, vec![]),
        ),
        seed(
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::artifact(None, vec![]),
        ),
        seed(
            4,
            A,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::free()), vec![]),
        ),
        seed(
            5,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
        ),
    ];
    let price = [(Colourless, 2), (Blue, 2)];
    let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let id = physical(&field, 1).id();
    assert!(sheet(&field, id).has_keyword(Flying));
    assert_size(&field, id, 2, 3);
    let added = round(cast_untargeted(&field, 4, &[]).into_state()).into_state();
    assert_size(&added, id, 3, 3);
    let gone =
        round(targeted(&added, A, 5, TargetRef::Object(physical(&added, 2).id())).into_state())
            .into_state();
    assert_size(&gone, id, 2, 3);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_rampaging_brontodon_counts_its_controllers_lands_on_attack_resolution_and_loses_the_bonus_at_cleanup()
 {
    let mut cards = draw_pile();
    cards.extend([
        named("Rampaging Brontodon", 1, A, ObjectDestination::Hand),
        named(
            "Forest",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named(
            "Forest",
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
        seed(
            5,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
        ),
    ]);
    let price = [(Colourless, 5), (Green, 2)];
    let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = round(cast_untargeted(&start, 1, &price).into_state()).into_state();
    let id = physical(&field, 1).id();
    assert!(sheet(&field, id).has_keyword(Trample));
    assert_size(&field, id, 7, 7);
    let attacks = exile_hand::until(field, |s| {
        s.turn().unwrap().position().number == 3
            && matches!(
                s.pending_decision().map(|p| p.frame()),
                Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
            )
    });
    let declared = reply(
        &attacks,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: id,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let cut = round(
        targeted(
            &declared,
            A,
            5,
            TargetRef::Object(physical(&declared, 2).id()),
        )
        .into_state(),
    )
    .into_state();
    let buff = round(cut).into_state();
    assert_size(&buff, id, 8, 8);
    let cleanup = exile_hand::until(buff, |s| s.turn().unwrap().position().number == 4);
    assert_size(&cleanup, id, 7, 7);
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_dryad_of_the_ilysian_grove_grants_one_more_land_play_and_all_five_basic_types_without_removing_other_mana_abilities()
 {
    use spellwake_engine::land::BasicLandType;
    use spellwake_engine::mana::{ManaAbilityId, ManaAbilityRef};
    let mut cards = vec![
        named("Dryad of the Ilysian Grove", 1, A, ObjectDestination::Hand),
        named(
            "Bonders' Enclave",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named(
            "Bonders' Enclave",
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
    ];
    cards.extend((4..7).map(|n| named("Forest", n, A, ObjectDestination::Hand)));
    let cost = [(Colourless, 2), (Green, 1)];
    let start = game(vec![A, B], cards, &cost, Step::PrecombatMain);
    let first = land_play(&start, 4);
    illegal(
        &first,
        A,
        GameCommand::PlayLand {
            land: physical(&first, 5).id(),
        },
    );
    let dryad = round(cast_untargeted(&first, 1, &cost).into_state()).into_state();
    let two = land_play(&dryad, 5);
    illegal(
        &two,
        A,
        GameCommand::PlayLand {
            land: physical(&two, 6).id(),
        },
    );
    assert!(
        sheet(&two, physical(&two, 3).id())
            .current_land_subtypes
            .is_empty()
    );
    for (ty, color) in [
        (BasicLandType::Plains, White),
        (BasicLandType::Island, Blue),
        (BasicLandType::Swamp, Black),
        (BasicLandType::Mountain, Red),
        (BasicLandType::Forest, Green),
    ] {
        for n in [2, 4, 5] {
            assert!(
                sheet(&two, physical(&two, n).id())
                    .current_land_subtypes
                    .contains(&ty.land_type())
            );
        }
        let mana = command(
            &two,
            A,
            GameCommand::ActivateMana {
                source: physical(&two, 2).id(),
                ability: ManaAbilityRef::BasicLand(ty),
            },
        )
        .into_state();
        assert_eq!(mana.execution().mana(A, color), 1.into());
    }
    let printed = command(
        &two,
        A,
        GameCommand::ActivateMana {
            source: physical(&two, 2).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    assert_eq!(printed.execution().mana(A, Colourless), 1.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_wayward_swordtooth_allows_a_second_land_play_and_can_attack_or_block_only_after_earning_the_citys_blessing()
 {
    let mut cards = draw_pile();
    cards.extend([
        named("Wayward Swordtooth", 1, A, ObjectDestination::Hand),
        named("Forest", 2, A, ObjectDestination::Hand),
        named("Forest", 3, A, ObjectDestination::Hand),
        creature(4, A, 1, 2, &[]),
        creature(5, B, 1, 1, &[Haste]),
        seed(
            6,
            A,
            ObjectDestination::Hand,
            free_instant(BodyTarget::SinglePermanent, Instruction::DestroyTarget),
        ),
    ]);
    cards.extend((25..31).map(|n| {
        named(
            "Forest",
            n,
            A,
            ObjectDestination::Battlefield { controller: A },
        )
    }));
    let cost = [(Colourless, 2), (Green, 1)];
    let start = game(vec![A, B], cards, &cost, Step::PrecombatMain);
    let field = round(cast_untargeted(&start, 1, &cost).into_state()).into_state();
    let first = land_play(&field, 2);
    assert!(!first.execution().has_city_blessing(A));
    let second = land_play(&first, 3);
    assert!(second.execution().has_city_blessing(A));
    let blessed =
        round(targeted(&second, A, 6, TargetRef::Object(physical(&second, 2).id())).into_state())
            .into_state();
    assert!(blessed.execution().has_city_blessing(A));
    for (state, allowed) in [(first, false), (blessed, true)] {
        let id = physical(&state, 1).id();
        let other = exile_hand::until(state, |s| {
            s.turn().unwrap().position().number == 2
                && matches!(
                    s.pending_decision().map(|p| p.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                )
        });
        let declared = reply(
            &other,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: physical(&other, 5).id(),
                defender: A,
                defending_planeswalker: None,
            }]),
        )
        .into_state();
        let blocks = combat_frame(&declared, false);
        let RuleFrame::Combat(CombatDecision::Blockers(frame)) =
            blocks.pending_decision().unwrap().frame()
        else {
            panic!("block declarations")
        };
        assert_eq!(frame.eligible_blockers().contains(&id), allowed);
        let unblocked = reply(&blocks, DecisionReply::DeclareBlockers(vec![])).into_state();
        let own = exile_hand::until(unblocked, |s| {
            s.turn().unwrap().position().number == 3
                && matches!(
                    s.pending_decision().map(|p| p.frame()),
                    Some(RuleFrame::Combat(CombatDecision::Attackers(_)))
                )
        });
        let RuleFrame::Combat(CombatDecision::Attackers(frame)) =
            own.pending_decision().unwrap().frame()
        else {
            unreachable!()
        };
        assert_eq!(frame.eligible_attackers().contains(&id), allowed);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_interplanar_beacon_makes_colorless_or_two_different_walker_restricted_colors_and_gains_life_for_a_walker_cast()
 {
    use spellwake_engine::mana::{ManaRestriction, RestrictedManaSpend};
    let start = game(
        vec![A, B],
        vec![
            named("Interplanar Beacon", 1, A, ObjectDestination::Hand),
            named("Ugin, the Ineffable", 2, A, ObjectDestination::Hand),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::artifact(fixed_cost("{6}"), vec![]),
            ),
        ],
        &[(Colourless, 5)],
        Step::PrecombatMain,
    );
    let field = land_play(&start, 1);
    let plain = command(&field, A, tap_for_mana(&field, 1, 0)).into_state();
    assert_eq!(plain.execution().unrestricted_mana(A, Colourless), 6.into());
    let nonwalker = cast_untargeted(&plain, 3, &[(Colourless, 6)]).into_state();
    assert_eq!(nonwalker.execution().window().entries().len(), 1);
    let activate = |pairs: &[(ManaType, i64)]| GameCommand::ActivateManaWithRestrictedPayment {
        source: physical(&field, 1).id(),
        ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        payment: spend(&[(Colourless, 1)]),
        restricted_mana: vec![],
        bundle: Some(pairs.iter().map(|&(color, n)| (color, n.into())).collect()),
    };
    illegal(&field, A, activate(&[(Blue, 2)]));
    illegal(&field, A, activate(&[(Blue, 1), (Colourless, 1)]));
    let funded = command(&field, A, activate(&[(Blue, 1), (Red, 1)])).into_state();
    for color in [Red, Blue] {
        assert_eq!(funded.execution().mana(A, color), 1.into());
        assert_eq!(funded.execution().unrestricted_mana(A, color), 0.into());
    }
    let cast = |n| GameCommand::CastWithResources {
        spell: physical(&funded, n).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(&[(Colourless, 4), (Red, 1), (Blue, 1)])),
            restricted_mana: [Red, Blue]
                .into_iter()
                .map(|color| RestrictedManaSpend {
                    mana_type: color,
                    restriction: ManaRestriction::PlaneswalkerSpell,
                    amount: 1.into(),
                })
                .collect(),
            ..Default::default()
        },
    };
    illegal(&funded, A, cast(3));
    let walker = command(&funded, A, cast(2)).into_state();
    let gain = round(walker).into_state();
    assert_eq!(gain.execution().life_total(A), Some(&21.into()));
    assert_eq!(physical(&gain, 2).zone(), ObjectZone::Stack);
    assert_eq!(
        physical(&round(gain).into_state(), 2).zone(),
        ObjectZone::Battlefield
    );
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_renowned_weaponsmith_mana_only_pays_for_artifact_spells_or_abilities_and_its_search_offers_exactly_the_two_names()
 {
    use spellwake_engine::{
        activated::ActivationOrigin,
        effects::LibraryTopReply,
        mana::{ManaRestriction, RestrictedManaSpend},
    };
    let ability = ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::generic(2.into()),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
            }],
        ),
    };
    let mut ordinary = Program::creature(fixed_cost("{2}"), 2.into(), 2.into(), Default::default());
    ordinary.activated.push(ability.clone());
    let artifact = Program::artifact(fixed_cost("{2}"), vec![ability]);
    let mut cards = draw_pile();
    cards.extend([
        named(
            "Renowned Weaponsmith",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        seed(2, A, ObjectDestination::Hand, ordinary.clone()),
        seed(3, A, ObjectDestination::Hand, artifact.clone()),
        seed(
            4,
            A,
            ObjectDestination::Battlefield { controller: A },
            ordinary,
        ),
        seed(
            5,
            A,
            ObjectDestination::Battlefield { controller: A },
            artifact,
        ),
        named("Heart-Piercer Bow", 6, A, ObjectDestination::LibraryBottom),
        named("Vial of Dragonfire", 7, A, ObjectDestination::LibraryBottom),
        named("Sol Ring", 8, A, ObjectDestination::LibraryBottom),
        named(
            "Island",
            9,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ]);
    let opening = game(vec![A, B], cards, &[], Step::PrecombatMain);
    illegal(&opening, A, tap_for_mana(&opening, 1, 0));
    let turn = exile_hand::until(opening, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let ready = command(
        &turn,
        A,
        GameCommand::ActivateMana {
            source: physical(&turn, 9).id(),
            ability: ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Island),
        },
    )
    .into_state();
    let funded = command(&ready, A, tap_for_mana(&ready, 1, 0)).into_state();
    assert_eq!(funded.execution().mana(A, Colourless), 2.into());
    assert_eq!(
        funded.execution().unrestricted_mana(A, Colourless),
        0.into()
    );
    let restricted = RestrictedManaSpend {
        mana_type: Colourless,
        restriction: ManaRestriction::ArtifactSpellOrAbility,
        amount: 2.into(),
    };
    let cast = |n| GameCommand::CastWithResources {
        spell: physical(&funded, n).id(),
        target: None,
        payment: CastingPayment {
            mana: Some(spend(&[(Colourless, 2)])),
            restricted_mana: vec![restricted.clone()],
            ..Default::default()
        },
    };
    illegal(&funded, A, cast(2));
    let made = round(command(&funded, A, cast(3)).into_state()).into_state();
    assert_eq!(physical(&made, 3).zone(), ObjectZone::Battlefield);
    let pay = |n| GameCommand::ActivateWithRestrictedMana {
        source: physical(&funded, n).id(),
        ability: ActivatedAbilityId(0),
        granting_effect: None,
        origin: ActivationOrigin::Untargeted,
        chosen_x: None,
        payment: Some(spend(&[(Colourless, 2)])),
        restricted_mana: vec![restricted.clone()],
    };
    illegal(&funded, A, pay(4));
    assert_eq!(
        round(command(&funded, A, pay(5)).into_state())
            .state()
            .execution()
            .life_total(A),
        Some(&21.into())
    );
    let searching =
        round(command(&ready, A, activation(&ready, 1, 1, &[(Blue, 1)])).into_state()).into_state();
    let RuleFrame::LibraryTop(search) = searching.pending_decision().unwrap().frame() else {
        panic!("named search")
    };
    let wanted = [physical(&searching, 6).id(), physical(&searching, 7).id()];
    assert_eq!(
        search
            .eligible_results()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        wanted.into()
    );
    let entropy = searching
        .supply_entropy(searching.basis(), vec![0; 32])
        .unwrap()
        .into_state();
    for n in [6, 7] {
        let found = reply(
            &entropy,
            DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: Some(physical(&entropy, n).id()),
            }),
        )
        .into_state();
        assert_eq!(physical(&found, n).zone(), ObjectZone::Hand(A));
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_mobilized_district_counts_controlled_legendary_creatures_and_planeswalkers_and_animates_only_through_cleanup()
 {
    use spellwake_engine::characteristics::KeywordAbility::Vigilance;
    let price = [(Colourless, 8), (White, 3)];
    let mut cards = draw_pile();
    cards.extend([
        named("Mobilized District", 1, A, ObjectDestination::Hand),
        named("Keleth, Sunmane Familiar", 2, A, ObjectDestination::Hand),
        named("Elspeth, Sun's Champion", 3, A, ObjectDestination::Hand),
        named(
            "Yargle, Glutton of Urborg",
            4,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
        creature(5, A, 2, 2, &[]),
    ]);
    let start = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let field = land_play(&start, 1);
    let plain = command(&field, A, tap_for_mana(&field, 1, 0)).into_state();
    assert_eq!(plain.execution().mana(A, Colourless), 9.into());
    let legendary =
        round(cast_untargeted(&field, 2, &[(Colourless, 1), (White, 1)]).into_state()).into_state();
    let walker = round(cast_untargeted(&legendary, 3, &[(Colourless, 4), (White, 2)]).into_state())
        .into_state();
    for (s, required) in [(field, 4), (legendary, 3), (walker, 2)] {
        illegal(&s, A, activation(&s, 1, 0, &[(Colourless, required - 1)]));
        let body =
            round(command(&s, A, activation(&s, 1, 0, &[(Colourless, required)])).into_state())
                .into_state();
        let id = physical(&body, 1).id();
        let animated = sheet(&body, id);
        assert!(animated.has_type(CardType::Land));
        assert!(animated.is_creature());
        assert!(animated.has_keyword(Vigilance));
        assert!(
            animated
                .current_creature_subtypes
                .contains(&current_catalogue().1.resolve_label("Citizen").unwrap())
        );
        assert_size(&body, id, 3, 3);
        let morning = exile_hand::until(body, |s| s.turn().unwrap().position().number == 2);
        let again = sheet(&morning, id);
        assert!(again.has_type(CardType::Land));
        assert!(!again.is_creature());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_neheb_adds_red_for_all_opponents_life_lost_including_afflict_when_blocked_and_ignores_its_controllers_loss()
 {
    let ping = |n| free_instant(BodyTarget::SinglePlayer, Instruction::Damage { amount: n });
    let mut cards = draw_pile();
    cards.extend([
        named(
            "Neheb, the Eternal",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        creature(2, B, 0, 7, &[]),
        seed(3, A, ObjectDestination::Hand, ping(2.into())),
        seed(4, A, ObjectDestination::Hand, ping(1.into())),
    ]);
    let opening = game(vec![A, B, C], cards, &[], Step::PrecombatMain);
    let start = exile_hand::until(opening, |s| {
        s.turn().unwrap().position().number == 4
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let hurt = round(targeted(&start, A, 3, TargetRef::Player(C)).into_state()).into_state();
    let self_hurt = round(targeted(&hurt, A, 4, TargetRef::Player(A)).into_state()).into_state();
    let attacks = combat_frame(&self_hurt, true);
    let id = physical(&attacks, 1).id();
    let declare = reply(
        &attacks,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: id,
            defender: B,
            defending_planeswalker: None,
        }]),
    )
    .into_state();
    let blocks = combat_frame(&declare, false);
    for blocked in [false, true] {
        let declarations = if blocked {
            vec![BlockDeclaration {
                blocker: physical(&blocks, 2).id(),
                attacker: id,
            }]
        } else {
            vec![]
        };
        let chosen = reply(&blocks, DecisionReply::DeclareBlockers(declarations)).into_state();
        assert_eq!(
            chosen.execution().window().entries().len(),
            usize::from(blocked)
        );
        let combat = if blocked {
            let afflicted = round(chosen).into_state();
            assert_eq!(afflicted.execution().life_total(B), Some(&17.into()));
            afflicted
        } else {
            chosen
        };
        let main = exile_hand::until(combat, |s| {
            s.turn().unwrap().position().step == Step::PostcombatMain
        });
        let done = round(main).into_state();
        assert_eq!(
            done.execution().mana(A, Red),
            (if blocked { 5 } else { 6 }).into()
        );
        assert_eq!(done.execution().life_total(A), Some(&19.into()));
        assert_eq!(done.execution().life_total(C), Some(&18.into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_selvala_offers_the_strictly_greatest_entrants_controller_a_draw_and_makes_a_chosen_bundle_from_only_her_controllers_greatest_power()
 {
    use spellwake_engine::effects::OptionalEffectChoice;
    let mut cards = draw_pile();
    cards.extend([
        named(
            "Selvala, Heart of the Wilds",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        creature(2, A, 4, 4, &[]),
        creature(3, B, 7, 3, &[]),
    ]);
    for (n, owner, power) in [(4, A, 7), (5, A, 8), (6, B, 8)] {
        let mut card = Program::creature(
            Some(ManaCost::free()),
            power.into(),
            2.into(),
            Default::default(),
        );
        card.printed_keywords.insert(KeywordAbility::Flash);
        cards.push(seed(n, owner, ObjectDestination::Hand, card));
    }
    cards.push(named(
        "Forest",
        7,
        A,
        ObjectDestination::Battlefield { controller: A },
    ));
    cards.push(seed(
        8,
        B,
        ObjectDestination::LibraryBottom,
        Program::artifact(None, vec![]),
    ));
    let opening = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let turn = exile_hand::until(opening, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::PrecombatMain
    });
    let field = command(
        &turn,
        A,
        GameCommand::ActivateMana {
            source: physical(&turn, 7).id(),
            ability: ManaAbilityRef::BasicLand(spellwake_engine::land::BasicLandType::Forest),
        },
    )
    .into_state();
    let convert = |parts: &[(ManaType, i64)]| GameCommand::ActivateManaWithRestrictedPayment {
        source: physical(&field, 1).id(),
        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        payment: spend(&[(Green, 1)]),
        restricted_mana: vec![],
        bundle: Some(parts.iter().map(|&(color, n)| (color, n.into())).collect()),
    };
    for bad in [vec![(Red, 7)], vec![(Red, 3)], vec![(Colourless, 4)]] {
        illegal(&field, A, convert(&bad));
    }
    let mana = command(&field, A, convert(&[(White, 1), (Blue, 1), (Red, 2)])).into_state();
    for (kind, n) in [(Green, 0), (White, 1), (Blue, 1), (Red, 2)] {
        assert_eq!(mana.execution().mana(A, kind), n.into());
    }
    for (n, actor, draw) in [(4, A, false), (5, A, true), (6, B, true)] {
        let base = if actor == A {
            field.clone()
        } else {
            pass(&field).into_state()
        };
        let entering = command(
            &base,
            actor,
            GameCommand::CastUntargeted {
                spell: physical(&base, n).id(),
                payment: None,
            },
        )
        .into_state();
        let triggered = round(entering).into_state();
        let before = hand_size(&triggered, actor);
        let resolved = round(triggered).into_state();
        if draw {
            assert_eq!(resolved.pending_decision().unwrap().chooser(), actor);
            for (choice, count) in [
                (OptionalEffectChoice::Take, 1),
                (OptionalEffectChoice::Decline, 0),
            ] {
                let end = reply(&resolved, DecisionReply::OptionalEffect(choice)).into_state();
                assert_eq!(hand_size(&end, actor), before + count);
            }
        } else {
            assert!(resolved.pending_decision().is_none());
            assert_eq!(hand_size(&resolved, actor), before);
        }
    }
}
