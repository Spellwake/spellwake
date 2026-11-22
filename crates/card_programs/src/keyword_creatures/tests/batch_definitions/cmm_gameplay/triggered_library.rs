//! Named library triggers with checks for ownership, timing, permissions, and frequency.
use super::*;
use spellwake_engine::effects::OptionalEffectChoice;

fn order_milled(s: GameState, nums: &[u64]) -> GameState {
    let decision = s.pending_decision().expect("owner chooses graveyard order");
    let RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame() else {
        panic!("expected ordered mill, got {:?}", decision.frame())
    };
    let order: Vec<_> = nums.iter().map(|&n| physical(&s, n).id()).collect();
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        order.iter().copied().collect()
    );
    reply(&s, DecisionReply::OrderResolutionGraveyard(order)).into_state()
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_sidisi_mills_three_on_entry_and_attack_and_creates_one_zombie_per_creature_group() {
    use ManaType::*;
    let price = [(Colourless, 1), (Black, 1), (Green, 1), (Blue, 1)];
    for attack_has_creature in [false, true] {
        let mut cards = vec![
            hand("Sidisi, Brood Tyrant"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ];
        for (n, name) in [
            (3, "Elvish Mystic"),
            (4, "Scaretiller"),
            (5, "Forest"),
            (6, "Forest"),
            (
                7,
                if attack_has_creature {
                    "Scaretiller"
                } else {
                    "Forest"
                },
            ),
            (8, "Forest"),
        ] {
            cards.push(named(name, n, A, ObjectDestination::LibraryBottom));
        }
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let entered = cast_named(&initial, &price);
        let trigger = order_milled(round(entered).into_state(), &[5, 4, 3]);
        let made = round(trigger);
        let z = tokens(&made, A, 1)[0];
        assert_creature(
            made.state(),
            z,
            ("Zombie Token", &["Zombie"], 2, 2),
            &[CardType::Creature],
            &[Haste],
        );
        let attack = attack(made.state(), 1, B);
        let milled = order_milled(round(attack).into_state(), &[6, 7, 8]);
        for n in 3..=8 {
            assert_eq!(physical(&milled, n).zone(), ObjectZone::Graveyard(A));
        }
        assert_eq!(
            milled.execution().window().entries().len(),
            usize::from(attack_has_creature)
        );
        if attack_has_creature {
            let made = round(milled);
            let count = made
                .state()
                .execution()
                .window()
                .cards()
                .iter()
                .filter(|c| c.is_token() && c.zone() == ObjectZone::Battlefield)
                .count();
            assert_eq!(count, 2);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_crawling_infestation_may_mill_and_makes_only_one_insect_per_own_turn_from_any_zone() {
    use ManaType::*;
    let price = [(Colourless, 2), (Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Crawling Infestation"),
            named("Elvish Mystic", 2, A, ObjectDestination::LibraryBottom),
            named("Scaretiller", 3, A, ObjectDestination::LibraryBottom),
            creature(4, A, 1, 1, &[]),
            creature(5, A, 1, 1, &[]),
            seed(
                6,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ),
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
    let destroy = |s: &GameState, spell, n| {
        round(targeted(s, A, spell, TargetRef::Object(physical(s, n).id())).into_state())
            .into_state()
    };
    let death = destroy(&entered, 6, 4);
    let first = round(death).into_state();
    let insects = |s: &GameState| {
        s.execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
            .map(GameObject::id)
            .collect::<Vec<_>>()
    };
    assert_eq!(insects(&first).len(), 1);
    let sheet = first
        .execution()
        .current_characteristics(insects(&first)[0])
        .unwrap()
        .unwrap();
    assert_eq!(sheet.current_colors, [Color::Green].into());
    assert_eq!(
        sheet.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Insect").unwrap()].into()
    );
    assert_eq!(
        sheet.power_toughness.unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    let second = destroy(&first, 7, 5);
    assert!(second.execution().window().entries().is_empty());
    let opponent = next_main(first.clone(), 2);
    let priority = pass(&opponent).into_state();
    let opposing_turn = destroy(&priority, 7, 5);
    assert!(opposing_turn.execution().window().entries().is_empty());
    let upkeep = exile_hand::until(first, |s| {
        let p = s.turn().unwrap().position();
        p.number == 3 && p.step == Step::Upkeep
    });
    let may = round(upkeep).into_state();
    let skip = reply(
        &may,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&skip, 2).zone(), ObjectZone::Library(A));
    let take = reply(
        &may,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let milled = order_milled(take, &[3, 2]);
    let more = round(milled).into_state();
    assert_eq!(insects(&more).len(), 2);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_nekusar_adds_a_draw_for_each_player_and_damages_only_opponents_for_each_card() {
    use ManaType::*;
    let price = [(Colourless, 2), (Blue, 1), (Black, 1), (Red, 1)];
    let mut cards = vec![hand("Nekusar, the Mindrazer")];
    for (start, seat) in [(2, A), (5, B), (8, C)] {
        for n in start..start + 3 {
            cards.push(named("Forest", n, seat, ObjectDestination::LibraryBottom));
        }
    }
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let mut state = cast_named(&initial, &price);
    for (turn, player, first, second) in [(2, B, 5, 6), (3, C, 8, 9), (4, A, 2, 3)] {
        loop {
            state = exile_hand::until(state, |s| {
                let position = s.turn().unwrap().position();
                (position.number == turn && position.step == Step::PrecombatMain)
                    || matches!(
                        s.pending_decision().map(|decision| decision.frame()),
                        Some(RuleFrame::Triggers(_))
                    )
            });
            let Some(RuleFrame::Triggers(frame)) =
                state.pending_decision().map(|decision| decision.frame())
            else {
                break;
            };
            state = reply(
                &state,
                DecisionReply::OrderTriggers(frame.instances().to_vec()),
            )
            .into_state();
        }
        for n in [first, second] {
            assert_eq!(physical(&state, n).zone(), ObjectZone::Hand(player));
        }
        assert_eq!(
            physical(&state, second + 1).zone(),
            ObjectZone::Library(player)
        );
        assert_eq!(
            state.execution().life_total(player),
            Some(&(if player == A { 20_i64 } else { 18 }).into())
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_teshar_reanimates_on_artifacts_legendaries_and_sagas_only_within_the_mana_limit() {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 1)];
    for (spell, cost, historic) in [
        ("Sol Ring", vec![(Colourless, 1)], true),
        (
            "Gorex, the Tombshell",
            vec![(Colourless, 6), (Black, 2)],
            true,
        ),
        (
            "The Binding of the Titans",
            vec![(Colourless, 1), (Green, 1)],
            true,
        ),
        ("Fervor", vec![(Colourless, 2), (Red, 1)], false),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                hand("Teshar, Ancestor's Apostle"),
                named(spell, 2, A, ObjectDestination::Hand),
                named("Pilgrim's Eye", 3, A, ObjectDestination::Graveyard),
                named("Elvish Mystic", 4, B, ObjectDestination::Graveyard),
                named("Scaretiller", 5, A, ObjectDestination::Graveyard),
                named("Sol Ring", 6, A, ObjectDestination::Graveyard),
            ],
            &[
                (Colourless, 9),
                (White, 1),
                (Black, 2),
                (Red, 1),
                (Green, 1),
            ],
            Step::PrecombatMain,
        );
        let entered = cast_named(&initial, &price);
        let cast = cast_untargeted(&entered, 2, &cost).into_state();
        if historic {
            let choose =
                |n| DecisionReply::TriggerTarget(TargetRef::Object(physical(&cast, n).id()));
            for n in [4, 5, 6] {
                illegal(&cast, A, combat_declaration(&cast, choose(n)));
            }
            let trigger = reply(&cast, choose(3)).into_state();
            let raised = round(trigger).into_state();
            assert_eq!(physical(&raised, 2).zone(), ObjectZone::Stack);
            assert_eq!(physical(&raised, 3).zone(), ObjectZone::Battlefield);
        } else {
            assert!(cast.pending_decision().is_none());
            assert_eq!(cast.execution().window().entries().len(), 1);
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_furious_rise_checks_power_at_trigger_and_resolution_and_replaces_the_previous_permission()
 {
    let price = [(ManaType::Colourless, 2), (ManaType::Red, 1)];
    let ending = |s: GameState, number| {
        exile_hand::until(s, |s| {
            let p = s.turn().unwrap().position();
            p.number == number && p.step == Step::End
        })
    };
    for power in [3, 4] {
        let initial = game(
            vec![A, B],
            vec![
                hand("Furious Rise"),
                creature(2, A, power, 3, &[]),
                seed(
                    3,
                    A,
                    ObjectDestination::LibraryBottom,
                    free_instant(
                        BodyTarget::None,
                        Instruction::GainControllerLife {
                            amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                        },
                    ),
                ),
                named("Forest", 4, A, ObjectDestination::LibraryBottom),
                named("Forest", 5, A, ObjectDestination::LibraryBottom),
                named("Island", 6, B, ObjectDestination::LibraryBottom),
                seed(
                    7,
                    A,
                    ObjectDestination::Hand,
                    free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
                ),
                creature(8, B, 9, 9, &[]),
            ],
            &price,
            Step::PrecombatMain,
        );
        let end = ending(cast_named(&initial, &price), 1);
        if power < 4 {
            assert!(end.execution().window().entries().is_empty());
            assert_eq!(physical(&end, 3).zone(), ObjectZone::Library(A));
            continue;
        }
        let removed =
            round(targeted(&end, A, 7, TargetRef::Object(physical(&end, 2).id())).into_state())
                .into_state();
        let failed = round(removed).into_state();
        assert_eq!(physical(&failed, 3).zone(), ObjectZone::Library(A));
        let exiled = round(end).into_state();
        assert_eq!(physical(&exiled, 3).zone(), ObjectZone::Exile);
        let played = round(cast_untargeted(&exiled, 3, &[]).into_state()).into_state();
        assert_eq!(played.execution().life_total(A), Some(&21.into()));
        let next = ending(exiled, 3);
        assert!(
            next.execution()
                .may_play_from_exile(physical(&next, 3).id(), A)
        );
        let replaced = round(next).into_state();
        assert_eq!(physical(&replaced, 3).zone(), ObjectZone::Exile);
        assert!(
            !replaced
                .execution()
                .may_play_from_exile(physical(&replaced, 3).id(), A)
        );
        illegal(
            &replaced,
            A,
            GameCommand::CastUntargeted {
                spell: physical(&replaced, 3).id(),
                payment: None,
            },
        );
        assert_eq!(physical(&replaced, 5).zone(), ObjectZone::Exile);
        let main = next_main(replaced, 5);
        let land = command(
            &main,
            A,
            GameCommand::PlayLand {
                land: physical(&main, 5).id(),
            },
        )
        .into_state();
        assert_eq!(physical(&land, 5).zone(), ObjectZone::Battlefield);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_twilight_prophet_keeps_the_blessing_and_each_opponent_loses_the_revealed_value_once() {
    use ManaType::*;
    let price = [(Colourless, 2), (Black, 2)];
    for ready in [false, true] {
        let mut cards = vec![
            hand("Twilight Prophet"),
            named("Scaretiller", 2, A, ObjectDestination::LibraryBottom),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget),
            ),
        ];
        for n in 4..(if ready { 13 } else { 12 }) {
            cards.push(creature(n, A, 1, 1, &[]));
        }
        let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
        let entered = cast_named(&initial, &price);
        assert_eq!(entered.execution().has_city_blessing(A), ready);
        let removed = round(
            targeted(
                &entered,
                A,
                3,
                TargetRef::Object(physical(&entered, 4).id()),
            )
            .into_state(),
        )
        .into_state();
        assert_eq!(removed.execution().has_city_blessing(A), ready);
        let upkeep = exile_hand::until(removed, |s| {
            let p = s.turn().unwrap().position();
            p.number == 4 && p.step == Step::Upkeep
        });
        if !ready {
            assert!(upkeep.execution().window().entries().is_empty());
            assert_eq!(physical(&upkeep, 2).zone(), ObjectZone::Library(A));
            continue;
        }
        let result = round(upkeep);
        assert_eq!(physical(result.state(), 2).zone(), ObjectZone::Hand(A));
        assert!(events(&result).flat_map(|e|&e.occurrences).any(|o|matches!(&o.value,RulesOccurrence::LibraryCardRevealed{object} if object.card_id()==Some(CardId(2)))));
        assert_eq!(result.state().execution().life_total(A), Some(&24.into()));
        for opponent in [B, C] {
            assert_eq!(
                result.state().execution().life_total(opponent),
                Some(&16.into())
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_descendants_fury_sacrifices_only_a_combat_contributor_and_puts_the_first_matching_creature_in_play()
 {
    use ManaType::*;
    let elf = current_catalogue().1.resolve_label("Elf").unwrap();
    let goblin = current_catalogue().1.resolve_label("Goblin").unwrap();
    let warrior = |n, kind, zone| {
        seed(
            n,
            A,
            zone,
            Program::creature(None, 2.into(), 3.into(), [kind].into()),
        )
    };
    let price = [(Colourless, 3), (Red, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Descendants' Fury"),
            warrior(2, elf, ObjectDestination::Battlefield { controller: A }),
            warrior(3, goblin, ObjectDestination::Battlefield { controller: A }),
            warrior(4, elf, ObjectDestination::Battlefield { controller: A }),
            named(
                "Fervor",
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Forest", 6, A, ObjectDestination::LibraryBottom),
            warrior(7, goblin, ObjectDestination::LibraryBottom),
            warrior(8, elf, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let attackers = combat_frame(&entered, true);
    let attacking = reply(
        &attackers,
        DecisionReply::DeclareAttackers(
            [2, 3]
                .map(|n| AttackDeclaration {
                    attacker: physical(&attackers, n).id(),
                    defender: B,
                    defending_planeswalker: None,
                })
                .to_vec(),
        ),
    )
    .into_state();
    let damage = exile_hand::until(attacking, |s| {
        s.turn().unwrap().position().step == Step::CombatDamage
    });
    assert_eq!(damage.execution().window().entries().len(), 1);
    let offer = round(damage).into_state();
    let skip = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&skip, 8).zone(), ObjectZone::Library(A));
    let choose = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let RuleFrame::ResolutionSelect(frame) = choose.pending_decision().unwrap().frame() else {
        panic!("expected contributor choice")
    };
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        [2, 3].map(|n| physical(&choose, n).id()).into()
    );
    let seeded = choose
        .supply_entropy(choose.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    for (sac, found) in [(2, 8), (3, 7)] {
        let result = reply(
            &seeded,
            DecisionReply::SelectResolution(vec![physical(&seeded, sac).id()]),
        );
        assert_eq!(
            physical(result.state(), sac).zone(),
            ObjectZone::Graveyard(A)
        );
        assert_eq!(
            physical(result.state(), found).zone(),
            ObjectZone::Battlefield
        );
        let reveal: Vec<_> = events(&result)
            .flat_map(|e| &e.occurrences)
            .filter_map(|o| match &o.value {
                RulesOccurrence::LibraryCardRevealed { object } => object.card_id(),
                _ => None,
            })
            .collect();
        assert_eq!(reveal, (6..=found).map(CardId).collect::<Vec<_>>());
        let library = result.state().execution().window().cards().library(A);
        assert_eq!(
            library[0],
            physical(result.state(), if found == 7 { 8 } else { 100 }).id()
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_windrider_wizard_can_draw_then_discard_for_each_instant_sorcery_and_wizard_cast() {
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 1)];
    let vocabulary = current_catalogue().1;
    let spell = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::GainControllerLife {
            amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
        }],
    );
    let initial = game(
        vec![A, B],
        vec![
            hand("Windrider Wizard"),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::instant(ManaCost::free(), spell.clone()),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::sorcery(ManaCost::free(), spell),
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                Program::creature(
                    Some(ManaCost::free()),
                    1.into(),
                    1.into(),
                    [vocabulary.resolve_label("Wizard").unwrap()].into(),
                ),
            ),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                Program::creature(
                    Some(ManaCost::free()),
                    1.into(),
                    1.into(),
                    [vocabulary.resolve_label("Elf").unwrap()].into(),
                ),
            ),
            named("Island", 6, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    for n in 2..=5 {
        let cast = cast_untargeted(&entered, n, &[]).into_state();
        assert_eq!(
            cast.execution().window().entries().len(),
            if n == 5 { 1 } else { 2 }
        );
        if n == 5 {
            continue;
        }
        let offer = round(cast).into_state();
        let skip = reply(
            &offer,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        )
        .into_state();
        assert_eq!(physical(&skip, 6).zone(), ObjectZone::Library(A));
        let discard = reply(
            &offer,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        )
        .into_state();
        assert_eq!(physical(&discard, 6).zone(), ObjectZone::Hand(A));
        let done = reply(
            &discard,
            DecisionReply::SelectResolution(vec![physical(&discard, 6).id()]),
        )
        .into_state();
        assert_eq!(physical(&done, 6).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(&done, n).zone(), ObjectZone::Stack);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_mindwrack_harpy_mills_each_players_three_only_at_its_controllers_beginning_of_combat()
{
    let price = [(ManaType::Colourless, 3), (ManaType::Black, 1)];
    let mut cards = vec![hand("Mindwrack Harpy")];
    for (owner, start) in [(A, 2), (B, 10), (C, 20)] {
        for n in start..start + 5 {
            cards.push(named("Forest", n, owner, ObjectDestination::LibraryBottom));
        }
    }
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let entered = cast_named(&initial, &price);
    let combat = exile_hand::until(entered, |s| {
        s.turn().unwrap().position().step == Step::BeginningCombat
    });
    let mut milled = round(combat).into_state();
    for (owner, start) in [(A, 2), (B, 10), (C, 20)] {
        assert_eq!(milled.pending_decision().unwrap().chooser(), owner);
        milled = order_milled(milled, &[start + 2, start + 1, start]);
    }
    for (owner, start) in [(A, 2), (B, 10), (C, 20)] {
        for n in start..start + 3 {
            assert_eq!(physical(&milled, n).zone(), ObjectZone::Graveyard(owner));
        }
        assert_eq!(
            physical(&milled, start + 3).zone(),
            ObjectZone::Library(owner)
        );
    }
    let other = exile_hand::until(milled, |s| {
        let p = s.turn().unwrap().position();
        p.number == 2 && p.step == Step::BeginningCombat
    });
    assert!(other.execution().window().entries().is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_inga_scrying_orders_three_and_its_death_counts_itself_and_other_simultaneous_deaths() {
    use ManaType::*;
    let price = [(Colourless, 3), (Blue, 1)];
    for third in [false, true] {
        let mut cards = vec![
            hand("Inga Rune-Eyes"),
            creature(2, B, 1, 1, &[]),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    Instruction::DestroyGroup(Box::new(
                        spellwake_engine::program::ObjectFilter::creature(),
                    )),
                ),
            ),
        ];
        if third {
            cards.push(creature(3, C, 1, 1, &[]));
        }
        for n in 5..=8 {
            cards.push(named("Island", n, A, ObjectDestination::LibraryBottom));
        }
        let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
        let entered = cast_named(&initial, &price);
        let look = round(entered).into_state();
        let ready = reply(
            &look,
            DecisionReply::LibraryTop(LibraryTopReply::Scry {
                top_first: [7, 5].map(|n| physical(&look, n).id()).to_vec(),
                bottom_top_first: vec![physical(&look, 6).id()],
            }),
        )
        .into_state();
        let order = ready.execution().window().cards().library(A);
        assert_eq!(order[0..3], [7, 5, 8].map(|n| physical(&ready, n).id()));
        let dead = round(cast_untargeted(&ready, 4, &[]).into_state()).into_state();
        assert_eq!(physical(&dead, 1).zone(), ObjectZone::Graveyard(A));
        let result = round(dead);
        assert_eq!(
            result.draw_journal().events.len(),
            if third { 3 } else { 0 }
        );
        for n in [7, 5, 8] {
            assert_eq!(
                physical(result.state(), n).zone(),
                if third {
                    ObjectZone::Hand(A)
                } else {
                    ObjectZone::Library(A)
                }
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_oath_of_jace_draws_then_discards_on_entry_and_scries_for_controlled_planeswalkers() {
    use ManaType::*;
    let price = [(Colourless, 2), (Blue, 1)];
    for has_walker in [false, true] {
        let mut cards = vec![
            hand("Oath of Jace"),
            named("Vronos, Masked Inquisitor", 2, A, ObjectDestination::Hand),
        ];
        for n in 3..=7 {
            cards.push(named("Island", n, A, ObjectDestination::LibraryBottom));
        }
        let initial = game(
            vec![A, B],
            cards,
            &[(Colourless, 5), (Blue, 3)],
            Step::PrecombatMain,
        );
        let live = if has_walker {
            round(cast_untargeted(&initial, 2, &[(Colourless, 3), (Blue, 2)]).into_state())
                .into_state()
        } else {
            initial
        };
        let entered = cast_named(&live, &price);
        let choose = round(entered).into_state();
        for n in 3..=5 {
            assert_eq!(physical(&choose, n).zone(), ObjectZone::Hand(A));
        }
        let selected = reply(
            &choose,
            DecisionReply::SelectResolution(vec![
                physical(&choose, 4).id(),
                physical(&choose, 5).id(),
            ]),
        )
        .into_state();
        let result = reply(
            &selected,
            DecisionReply::OrderResolutionGraveyard(vec![
                physical(&selected, 5).id(),
                physical(&selected, 4).id(),
            ]),
        )
        .into_state();
        for n in [4, 5] {
            assert_eq!(physical(&result, n).zone(), ObjectZone::Graveyard(A));
        }
        let upkeep = exile_hand::until(result, |s| {
            let p = s.turn().unwrap().position();
            p.number == 3 && p.step == Step::Upkeep
        });
        let look = round(upkeep).into_state();
        if has_walker {
            let RuleFrame::LibraryTop(frame) = look.pending_decision().unwrap().frame() else {
                panic!("expected one-card scry")
            };
            assert_eq!(frame.domain_top_first(), [physical(&look, 6).id()]);
            let scry = reply(
                &look,
                DecisionReply::LibraryTop(LibraryTopReply::Scry {
                    top_first: vec![],
                    bottom_top_first: vec![physical(&look, 6).id()],
                }),
            );
            assert_eq!(
                scry.state().execution().window().cards().library(A)[0],
                physical(scry.state(), 7).id()
            );
        } else {
            assert_eq!(
                look.execution().window().cards().library(A)[0],
                physical(&look, 6).id()
            );
            if let Some(d) = look.pending_decision() {
                let RuleFrame::LibraryTop(frame) = d.frame() else {
                    panic!("expected empty scry")
                };
                assert!(frame.domain_top_first().is_empty());
            }
        }
    }
}
