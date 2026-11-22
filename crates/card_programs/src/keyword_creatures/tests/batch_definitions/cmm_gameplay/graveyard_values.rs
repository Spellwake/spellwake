//! Graveyard counts evaluated while casting, resolving and applying continuous effects.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_carrion_grub_mills_four_and_uses_the_greatest_creature_power_in_its_own_graveyard() {
    let price = [(ManaType::Colourless, 3), (ManaType::Black, 1)];
    let monster = |n, seat, zone, power: i64| {
        seed(
            n,
            seat,
            zone,
            Program::creature(None, power.into(), 3.into(), BTreeSet::new()),
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Carrion Grub"),
            monster(2, A, ObjectDestination::Graveyard, 2),
            monster(3, B, ObjectDestination::Graveyard, 9),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
            monster(5, A, ObjectDestination::LibraryBottom, 4),
            named("Island", 6, A, ObjectDestination::LibraryBottom),
            monster(7, A, ObjectDestination::LibraryBottom, 3),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let power = |s: &GameState| {
        s.execution()
            .power_toughness(physical(s, 1).id())
            .unwrap()
            .unwrap()
            .derived
            .power
    };
    assert_eq!(power(&entered), 2.into());
    let order = round(entered).into_state();
    let rule = order.pending_decision().unwrap().frame();
    let RuleFrame::ResolutionGraveyardOrder(frame) = rule else {
        panic!("expected mill order, got {rule:?}")
    };
    let result = reply(
        &order,
        DecisionReply::OrderResolutionGraveyard(frame.domain().iter().copied().rev().collect()),
    );
    assert_eq!(power(result.state()), 4.into());
    for n in 4..=7 {
        assert_eq!(physical(result.state(), n).zone(), ObjectZone::Graveyard(A));
    }
    assert_eq!(physical(result.state(), 100).zone(), ObjectZone::Library(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_cryptic_serpent_reduces_only_generic_mana_for_instants_and_sorceries_in_its_graveyard()
{
    use ManaType::*;
    let state = game(
        vec![A, B],
        vec![
            hand("Cryptic Serpent"),
            named("Counterspell", 2, A, ObjectDestination::Graveyard),
            named("Cultivate", 3, A, ObjectDestination::Graveyard),
            named("Sol Ring", 4, A, ObjectDestination::Graveyard),
            named("Counterspell", 5, B, ObjectDestination::Graveyard),
        ],
        &[(Colourless, 5), (Blue, 2)],
        Step::PrecombatMain,
    );
    let pay = |cost: &[(ManaType, i64)]| GameCommand::CastUntargeted {
        spell: physical(&state, 1).id(),
        payment: Some(spend(cost)),
    };
    illegal(&state, A, pay(&[(Colourless, 2), (Blue, 2)]));
    illegal(&state, A, pay(&[(Colourless, 4), (Blue, 1)]));
    let entered = cast_named(&state, &[(Colourless, 3), (Blue, 2)]);
    assert_eq!(physical(&entered, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(entered.execution().mana(A, Colourless), 2.into());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_corpse_augur_counts_the_target_players_creature_cards_on_resolution() {
    let price = [(ManaType::Colourless, 3), (ManaType::Black, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Corpse Augur"),
            named("Elvish Mystic", 2, B, ObjectDestination::Graveyard),
            named("Scaretiller", 3, B, ObjectDestination::Graveyard),
            named("Sol Ring", 4, B, ObjectDestination::Graveyard),
            named("Island", 5, A, ObjectDestination::LibraryBottom),
            named("Plains", 6, A, ObjectDestination::LibraryBottom),
            seed(
                7,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let trigger = round(
        targeted(
            &entered,
            A,
            7,
            TargetRef::Object(physical(&entered, 1).id()),
        )
        .into_state(),
    )
    .into_state();
    for (seat, n) in [(A, 1_i64), (B, 2), (C, 0)] {
        let stack = reply(
            &trigger,
            DecisionReply::TriggerTarget(TargetRef::Player(seat)),
        )
        .into_state();
        let result = round(stack);
        assert_eq!(
            result.draw_journal().events.len(),
            usize::try_from(n).unwrap()
        );
        assert_eq!(
            result.state().execution().life_total(A),
            Some(&(20_i64 - n).into())
        );
        assert_eq!(result.state().execution().life_total(B), Some(&20.into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_deep_analysis_draws_for_its_target_and_pays_life_only_for_flashback() {
    use ManaType::*;
    let normal = [(Colourless, 3), (Blue, 1)];
    let flashback = [(Colourless, 1), (Blue, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Deep Analysis"),
            named("Forest", 2, B, ObjectDestination::LibraryBottom),
            named("Mountain", 3, A, ObjectDestination::LibraryBottom),
        ],
        &[(Colourless, 4), (Blue, 2)],
        Step::PrecombatMain,
    );
    let mut state = initial;
    for (price, target, life, card, zone) in [
        (&normal, B, 20, 2, ObjectZone::Graveyard(A)),
        (&flashback, A, 17, 3, ObjectZone::Exile),
    ] {
        let stack = command(
            &state,
            A,
            GameCommand::CastWithResources {
                spell: physical(&state, 1).id(),
                target: Some(TargetRef::Player(target)),
                payment: CastingPayment {
                    mana: Some(spend(price)),
                    ..Default::default()
                },
            },
        )
        .into_state();
        assert_eq!(stack.execution().life_total(A), Some(&life.into()));
        let drawn = round(stack);
        assert_eq!(drawn.draw_journal().events.len(), 2);
        assert_eq!(
            physical(drawn.state(), card).zone(),
            ObjectZone::Hand(target)
        );
        assert_eq!(physical(drawn.state(), 1).zone(), zone);
        state = drawn.into_state();
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_faithless_looting_draws_before_each_discard_and_exiles_after_flashback() {
    use ManaType::*;
    let mut cards = vec![
        hand("Faithless Looting"),
        named("Sol Ring", 6, A, ObjectDestination::Hand),
    ];
    for n in 2..=5 {
        cards.push(named("Island", n, A, ObjectDestination::LibraryBottom));
    }
    let mut state = game(
        vec![A, B],
        cards,
        &[(Colourless, 2), (Red, 2)],
        Step::PrecombatMain,
    );
    for (price, arrivals, sent, zone) in [
        (vec![(Red, 1)], [2, 3], [3, 6], ObjectZone::Graveyard(A)),
        (
            vec![(Colourless, 2), (Red, 1)],
            [4, 5],
            [2, 5],
            ObjectZone::Exile,
        ),
    ] {
        let choose = round(cast_untargeted(&state, 1, &price).into_state()).into_state();
        for n in arrivals {
            assert_eq!(physical(&choose, n).zone(), ObjectZone::Hand(A));
        }
        let ids = sent.map(|n| physical(&choose, n).id());
        let order = reply(&choose, DecisionReply::SelectResolution(ids.to_vec())).into_state();
        state = reply(
            &order,
            DecisionReply::OrderResolutionGraveyard(ids.to_vec()),
        )
        .into_state();
        for n in sent {
            assert_eq!(physical(&state, n).zone(), ObjectZone::Graveyard(A));
        }
        assert_eq!(physical(&state, 1).zone(), zone);
    }
    assert_eq!(physical(&state, 4).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_cartographers_hawk_returns_on_combat_damage_to_the_opponent_with_more_lands() {
    use spellwake_engine::effects::OptionalEffectChoice;
    let price = [(ManaType::Colourless, 1), (ManaType::White, 1)];
    for ahead in [false, true] {
        let mut cards = vec![
            hand("Cartographer's Hawk"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Plains", 3, A, ObjectDestination::LibraryBottom),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
        ];
        if ahead {
            cards.push(named(
                "Swamp",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ));
        }
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let entered = cast_named(&initial, &price);
        let declared = attack(&entered, 1, B);
        let hit = exile_hand::until(declared, |s| {
            s.execution().life_total(B) == Some(&18.into())
        });
        if !ahead {
            assert!(hit.execution().window().entries().is_empty());
            assert_eq!(physical(&hit, 1).zone(), ObjectZone::Battlefield);
            continue;
        }
        let returned = round(hit).into_state();
        assert_eq!(physical(&returned, 1).zone(), ObjectZone::Hand(A));
        let decline = reply(
            &returned,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        );
        assert_eq!(physical(decline.state(), 3).zone(), ObjectZone::Library(A));
        let search = reply(
            &returned,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        )
        .into_state();
        let search = search
            .supply_entropy(search.basis(), vec![0; 8])
            .unwrap()
            .into_state();
        let RuleFrame::LibraryTop(frame) = search.pending_decision().unwrap().frame() else {
            panic!("expected plains search");
        };
        assert_eq!(frame.eligible_results(), [physical(&search, 3).id()]);
        let land = reply(
            &search,
            DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: Some(physical(&search, 3).id()),
            }),
        );
        assert_eq!(physical(land.state(), 3).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(land.state(), 3).tapped(), Some(true));
        assert_eq!(land.shuffle_journal().len(), 1);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_drown_in_sorrow_weakens_all_creatures_then_scries_and_expires_at_cleanup() {
    let price = [(ManaType::Colourless, 1), (ManaType::Black, 2)];
    let before = game(
        vec![A, B],
        vec![
            hand("Drown in Sorrow"),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 3, 4, &[]),
            named("Swamp", 4, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let scry = cast_named(&before, &price);
    let top = physical(&scry, 4).id();
    for keep in [true, false] {
        let reply_scry = if keep {
            LibraryTopReply::Scry {
                top_first: vec![top],
                bottom_top_first: vec![],
            }
        } else {
            LibraryTopReply::Scry {
                top_first: vec![],
                bottom_top_first: vec![top],
            }
        };
        let finish = reply(&scry, DecisionReply::LibraryTop(reply_scry)).into_state();
        let expected = if keep { 4 } else { 100 };
        assert_eq!(
            finish.execution().window().cards().library(A)[0],
            physical(&finish, expected).id()
        );
        assert_eq!(physical(&finish, 2).zone(), ObjectZone::Graveyard(A));
        let pt = |s: &GameState| {
            s.execution()
                .power_toughness(physical(s, 3).id())
                .unwrap()
                .unwrap()
                .derived
        };
        assert_eq!(
            pt(&finish),
            PowerToughness {
                power: 1.into(),
                toughness: 2.into()
            }
        );
        let after = next_main(finish, 2);
        assert_eq!(
            pt(&after),
            PowerToughness {
                power: 3.into(),
                toughness: 4.into()
            }
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_fists_of_flame_counts_its_own_draw_and_earlier_draws_and_grants_trample_until_cleanup()
{
    let price = [(ManaType::Colourless, 1), (ManaType::Red, 1)];
    let before = game(
        vec![A, B],
        vec![
            hand("Fists of Flame"),
            creature(2, A, 2, 2, &[]),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    Instruction::DrawController {
                        amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                    },
                ),
            ),
            named("Mountain", 4, A, ObjectDestination::LibraryBottom),
            named("Forest", 5, A, ObjectDestination::LibraryBottom),
            named("Island", 6, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    for previous in [false, true] {
        let state = if previous {
            round(cast_untargeted(&before, 3, &[]).into_state()).into_state()
        } else {
            before.clone()
        };
        let target = physical(&state, 2).id();
        let spell = command(
            &state,
            A,
            GameCommand::CastWithResources {
                spell: physical(&state, 1).id(),
                target: Some(TargetRef::Object(target)),
                payment: CastingPayment {
                    mana: Some(spend(&price)),
                    ..Default::default()
                },
            },
        );
        let done = round(spell.into_state()).into_state();
        assert_eq!(
            physical(&done, if previous { 6 } else { 4 }).zone(),
            ObjectZone::Hand(A)
        );
        let sheet = done
            .execution()
            .current_characteristics(target)
            .unwrap()
            .unwrap();
        assert!(sheet.keywords.contains(&Trample));
        assert_eq!(
            sheet.power_toughness.unwrap().derived.power,
            if previous { 5.into() } else { 3.into() }
        );
        let after = next_main(done, 2);
        let sheet = after
            .execution()
            .current_characteristics(target)
            .unwrap()
            .unwrap();
        assert!(!sheet.keywords.contains(&Trample));
        assert_eq!(sheet.power_toughness.unwrap().derived.power, 2.into());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_shelter_restricts_the_target_and_protects_it_from_the_chosen_color_until_cleanup() {
    use ManaType::*;
    let price = [(Colourless, 1), (White, 1)];
    let before = game(
        vec![A, B],
        vec![
            hand("Shelter"),
            creature(2, A, 2, 2, &[]),
            creature(3, B, 2, 2, &[]),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::one(Red),
                    ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::DestroyTarget],
                    ),
                ),
            ),
        ],
        &[(Colourless, 1), (White, 1), (Red, 1)],
        Step::PrecombatMain,
    );
    let cast =
        |s: &GameState, card, target, budget: &[(ManaType, i64)]| GameCommand::CastWithResources {
            spell: physical(s, card).id(),
            target: Some(TargetRef::Object(physical(s, target).id())),
            payment: CastingPayment {
                mana: Some(spend(budget)),
                ..Default::default()
            },
        };
    illegal(&before, A, cast(&before, 1, 3, &price));
    let choose = round(command(&before, A, cast(&before, 1, 2, &price)).into_state()).into_state();
    for (color, ability) in [
        (White, ProtectionFromWhite),
        (Blue, ProtectionFromBlue),
        (Black, ProtectionFromBlack),
        (Red, ProtectionFromRed),
        (Green, ProtectionFromGreen),
    ] {
        let result = reply(&choose, DecisionReply::ManaColor(color)).into_state();
        assert_eq!(physical(&result, 100).zone(), ObjectZone::Hand(A));
        let target = physical(&result, 2).id();
        assert!(
            result
                .execution()
                .current_characteristics(target)
                .unwrap()
                .unwrap()
                .keywords
                .contains(&ability)
        );
        let red_spell = cast(&result, 4, 2, &[(Red, 1)]);
        if color == Red {
            illegal(&result, A, red_spell);
        } else {
            command(&result, A, red_spell);
        }
        let after = next_main(result, 2);
        assert!(
            !after
                .execution()
                .current_characteristics(target)
                .unwrap()
                .unwrap()
                .keywords
                .contains(&ability)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_cyclops_electromancer_and_lotleth_giant_count_only_their_own_relevant_graveyard_cards()
{
    use ManaType::*;
    for (name, price, targets_creature) in [
        (
            "Cyclops Electromancer",
            vec![(Colourless, 4), (Red, 1)],
            true,
        ),
        ("Lotleth Giant", vec![(Colourless, 6), (Black, 1)], false),
    ] {
        for count in [0_i64, 2] {
            let mut cards = vec![
                hand(name),
                creature(2, A, 1, 4, &[]),
                creature(3, B, 1, 4, &[]),
                named("Sol Ring", 4, A, ObjectDestination::Graveyard),
                named("Counterspell", 5, B, ObjectDestination::Graveyard),
                named("Scaretiller", 6, B, ObjectDestination::Graveyard),
                named(
                    if targets_creature {
                        "Scaretiller"
                    } else {
                        "Counterspell"
                    },
                    7,
                    A,
                    ObjectDestination::Graveyard,
                ),
            ];
            if count > 0 {
                for (n, name) in if targets_creature {
                    [(8, "Counterspell"), (9, "Cultivate")]
                } else {
                    [(8, "Elvish Mystic"), (9, "Scaretiller")]
                } {
                    cards.push(named(name, n, A, ObjectDestination::Graveyard));
                }
            }
            let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
            let arrived = cast_named(&initial, &price);
            let (good, bad) = if targets_creature {
                (
                    TargetRef::Object(physical(&arrived, 3).id()),
                    TargetRef::Object(physical(&arrived, 2).id()),
                )
            } else {
                (TargetRef::Player(B), TargetRef::Player(A))
            };
            illegal(
                &arrived,
                A,
                combat_declaration(&arrived, DecisionReply::TriggerTarget(bad)),
            );
            let result = round(reply(&arrived, DecisionReply::TriggerTarget(good)).into_state())
                .into_state();
            assert_eq!(
                result.execution().life_total(B),
                Some(&(20 - if targets_creature { 0 } else { count }).into())
            );
            assert_eq!(
                result
                    .execution()
                    .battlefield_marked_damage(physical(&result, 3).id()),
                Some((if targets_creature { count } else { 0 }).into())
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_rise_from_the_tides_counts_only_its_controllers_instants_and_sorceries_for_tapped_zombies()
 {
    let price = [(ManaType::Colourless, 5), (ManaType::Blue, 1)];
    for count in [0usize, 2] {
        let mut cards = vec![
            hand("Rise from the Tides"),
            named("Sol Ring", 2, A, ObjectDestination::Graveyard),
            named("Counterspell", 3, B, ObjectDestination::Graveyard),
        ];
        if count > 0 {
            cards.push(named("Counterspell", 4, A, ObjectDestination::Graveyard));
            cards.push(named("Cultivate", 5, A, ObjectDestination::Graveyard));
        }
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let mut step = round(cast_untargeted(&initial, 1, &price).into_state());
        if let Some(RuleFrame::TimestampOrder(frame)) =
            step.state().pending_decision().map(|d| d.frame())
        {
            step = reply(
                step.state(),
                DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
            );
        }
        let result = step.state();
        let zombies = result
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
            .collect::<Vec<_>>();
        assert_eq!(zombies.len(), count);
        for zombie in zombies {
            assert_eq!(zombie.controller(), Some(A));
            assert_eq!(zombie.tapped(), Some(true));
            assert_creature(
                result,
                zombie.id(),
                ("Zombie Token", &["Zombie"], 2, 2),
                &[CardType::Creature],
                &[],
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_ravaging_blaze_deals_x_to_the_target_and_its_controller_only_after_spell_mastery() {
    use ManaType::*;
    for mastery in [false, true] {
        let price = [(Colourless, 3), (Red, 2)];
        let mut cards = vec![
            hand("Ravaging Blaze"),
            creature(2, B, 1, 5, &[]),
            named("Sol Ring", 3, A, ObjectDestination::Graveyard),
            named("Counterspell", 4, B, ObjectDestination::Graveyard),
            named("Cultivate", 5, A, ObjectDestination::Graveyard),
        ];
        if mastery {
            cards.push(named("Counterspell", 6, A, ObjectDestination::Graveyard));
        }
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let cast = |target| GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: Some(target),
            payment: CastingPayment {
                mana: Some(spend(&price)),
                chosen_x: Some(3.into()),
                ..Default::default()
            },
        };
        illegal(&initial, A, cast(TargetRef::Player(B)));
        let result = round(
            command(
                &initial,
                A,
                cast(TargetRef::Object(physical(&initial, 2).id())),
            )
            .into_state(),
        )
        .into_state();
        assert_eq!(
            result
                .execution()
                .battlefield_marked_damage(physical(&result, 2).id()),
            Some(3.into())
        );
        assert_eq!(
            result.execution().life_total(B),
            Some(&(if mastery { 17_i64 } else { 20 }).into())
        );
        assert_eq!(result.execution().life_total(A), Some(&20.into()));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_magus_of_the_wheel_pays_and_sacrifices_before_everyone_discards_and_draws_seven() {
    use ManaType::*;
    let price = [(Colourless, 2), (Red, 1)];
    let mut cards = vec![
        hand("Magus of the Wheel"),
        named(
            "Fervor",
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ];
    for (n, seat) in [(3, A), (4, B), (5, B)] {
        cards.push(seed(
            n,
            seat,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ));
    }
    for (start, seat) in [(10, A), (20, B), (30, C)] {
        for n in start..(start + 7) {
            cards.push(seed(
                n,
                seat,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ));
        }
    }
    let initial = game(
        vec![A, B, C],
        cards,
        &[(Colourless, 3), (Red, 2)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let activated = command(
        &entered,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: physical(&entered, 1).id(),
            ability: ActivatedAbilityId(0),
            payment: Some(spend(&[(Colourless, 1), (Red, 1)])),
        },
    )
    .into_state();
    assert_eq!(physical(&activated, 1).zone(), ObjectZone::Graveyard(A));
    let mut state = round(activated).into_state();
    while let Some(d) = state.pending_decision() {
        let choice = match d.frame() {
            RuleFrame::ResolutionSelect(frame) => {
                assert_eq!(frame.required(), frame.domain().len());
                DecisionReply::SelectResolution(frame.domain().to_vec())
            }
            RuleFrame::ResolutionGraveyardOrder(frame) => {
                DecisionReply::OrderResolutionGraveyard(frame.domain().to_vec())
            }
            other => panic!("unexpected wheel choice {other:?}"),
        };
        state = reply(&state, choice).into_state();
    }
    for (n, seat) in [(3, A), (4, B), (5, B)] {
        assert_eq!(physical(&state, n).zone(), ObjectZone::Graveyard(seat));
    }
    for (start, seat) in [(10, A), (20, B), (30, C)] {
        for n in start..(start + 7) {
            assert_eq!(physical(&state, n).zone(), ObjectZone::Hand(seat));
        }
        assert_eq!(
            physical(&state, 100 + u64::from(seat.0)).zone(),
            ObjectZone::Library(seat)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_mire_triton_mills_two_before_gaining_life_and_deathtouch_kills_a_larger_creature() {
    use ManaType::*;
    let price = [(Colourless, 1), (Black, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Mire Triton"),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
            named("Island", 3, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let arrived = cast_named(&initial, &price);
    let choose = round(arrived).into_state();
    assert_eq!(choose.execution().life_total(A), Some(&20.into()));
    let result = reply(
        &choose,
        DecisionReply::OrderResolutionGraveyard(vec![
            physical(&choose, 3).id(),
            physical(&choose, 2).id(),
        ]),
    )
    .into_state();
    for n in [2, 3] {
        assert_eq!(physical(&result, n).zone(), ObjectZone::Graveyard(A));
    }
    assert_eq!(result.execution().life_total(A), Some(&22.into()));
    assert_eq!(physical(&result, 100).zone(), ObjectZone::Library(A));
    let fighting = game(
        vec![A, B],
        vec![
            named(
                "Mire Triton",
                1,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(2, A, 1, 8, &[]),
            named(
                "Fervor",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let defending = blockers(&attack(&fighting, 2, B));
    let blocked = reply(&defending, block_choice(&defending, 1, 2)).into_state();
    let end = exile_hand::until(blocked, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(physical(&end, 1).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&end, 2).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_nyx_weaver_mills_at_its_own_upkeep_exiles_as_a_cost_and_blocks_flying_with_reach() {
    use ManaType::*;
    let price = [(Colourless, 1), (Black, 1), (Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Nyx Weaver"),
            named("Forest", 2, A, ObjectDestination::Graveyard),
            named("Counterspell", 3, A, ObjectDestination::Graveyard),
            named("Forest", 4, B, ObjectDestination::Graveyard),
            named("Forest", 5, A, ObjectDestination::LibraryBottom),
            named("Island", 6, A, ObjectDestination::LibraryBottom),
        ],
        &[(Colourless, 2), (Black, 2), (Green, 2)],
        Step::PrecombatMain,
    );
    let arrived = cast_named(&initial, &price);
    let activate = |n| GameCommand::ActivateAbility {
        source: physical(&arrived, 1).id(),
        ability: ActivatedAbilityId(0),
        target: TargetRef::Object(physical(&arrived, n).id()),
        payment: Some(spend(&price)),
    };
    illegal(&arrived, A, activate(4));
    for n in [2, 3] {
        let stacked = command(&arrived, A, activate(n)).into_state();
        assert_eq!(physical(&stacked, 1).zone(), ObjectZone::Exile);
        assert_eq!(physical(&stacked, n).zone(), ObjectZone::Graveyard(A));
        let returned = round(stacked);
        assert_eq!(physical(returned.state(), n).zone(), ObjectZone::Hand(A));
    }
    let upkeep = exile_hand::until(arrived, |s| {
        let p = s.turn().unwrap().position();
        p.number == 3 && p.step == Step::Upkeep
    });
    let choosing = round(upkeep).into_state();
    let result = reply(
        &choosing,
        DecisionReply::OrderResolutionGraveyard(vec![
            physical(&choosing, 6).id(),
            physical(&choosing, 5).id(),
        ]),
    )
    .into_state();
    for n in [5, 6] {
        assert_eq!(physical(&result, n).zone(), ObjectZone::Graveyard(A));
    }
    let fighting = game(
        vec![A, B],
        vec![
            named(
                "Nyx Weaver",
                1,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            creature(2, A, 1, 2, &[Flying]),
            creature(3, B, 2, 3, &[]),
            named(
                "Fervor",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &[],
        Step::PrecombatMain,
    );
    let defending = blockers(&attack(&fighting, 2, B));
    illegal(
        &defending,
        B,
        combat_declaration(&defending, block_choice(&defending, 3, 2)),
    );
    let blocked = reply(&defending, block_choice(&defending, 1, 2)).into_state();
    let end = exile_hand::until(blocked, |s| {
        s.turn().unwrap().position().step == Step::EndCombat
    });
    assert_eq!(physical(&end, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&end, 2).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_nessian_wanderer_can_reveal_a_land_from_three_and_randomly_bottoms_the_rest() {
    use ManaType::*;
    let price = [(Colourless, 1), (Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Nessian Wanderer"),
            named("Fervor", 2, A, ObjectDestination::Hand),
            named("Forest", 3, A, ObjectDestination::LibraryBottom),
            named("Elvish Mystic", 4, A, ObjectDestination::LibraryBottom),
            named("Sol Ring", 5, A, ObjectDestination::LibraryBottom),
            named("Sol Ring", 6, A, ObjectDestination::Hand),
        ],
        &[(Colourless, 4), (Green, 1), (Red, 1)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let rock = round(cast_untargeted(&entered, 6, &[(Colourless, 1)]).into_state()).into_state();
    assert!(rock.execution().window().entries().is_empty());
    let enchant =
        round(cast_untargeted(&rock, 2, &[(Colourless, 2), (Red, 1)]).into_state()).into_state();
    let choice = round(enchant).into_state();
    let ready = choice
        .supply_entropy(choice.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let RuleFrame::LibraryTop(frame) = ready.pending_decision().unwrap().frame() else {
        panic!("expected a private look")
    };
    assert_eq!(frame.domain_top_first().len(), 3);
    assert_eq!(frame.eligible_results(), [physical(&ready, 3).id()]);
    for take in [false, true] {
        let result = reply(
            &ready,
            DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: take.then(|| physical(&ready, 3).id()),
            }),
        );
        let state = result.state();
        assert_eq!(
            physical(state, 3).zone(),
            if take {
                ObjectZone::Hand(A)
            } else {
                ObjectZone::Library(A)
            }
        );
        assert_eq!(
            state.execution().window().cards().library(A)[0],
            physical(state, 100).id()
        );
        assert_eq!(
            events(&result)
                .flat_map(|e| &e.occurrences)
                .any(|o| matches!(o.value, RulesOccurrence::LibraryCardRevealed { .. })),
            take
        );
        let actual = state.execution().window().cards().library(A)[1..]
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut expected = BTreeSet::from([physical(state, 4).id(), physical(state, 5).id()]);
        if !take {
            expected.insert(physical(state, 3).id());
        }
        assert_eq!(actual, expected);
    }
}
