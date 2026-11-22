//! Drawing cards from entering permanents and controlled battlefield characteristics.
use super::*;
use spellwake_engine::{characteristics::PowerToughnessCounters, effects::CounterKind};

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_armorcraft_judge_counts_own_counter_bearing_creatures_once_each() {
    let price = [(ManaType::Colourless, 3), (ManaType::Green, 1)];
    let mut cards = vec![
        hand("Armorcraft Judge"),
        creature(2, A, 2, 2, &[]),
        creature(3, A, 2, 2, &[]),
        creature(4, B, 2, 2, &[]),
        named("Forest", 5, A, ObjectDestination::LibraryBottom),
        named("Plains", 6, A, ObjectDestination::LibraryBottom),
    ];
    for (n, count) in [(7, 1), (8, 2), (9, 1)] {
        cards.push(seed(
            n,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SingleCreature,
                Instruction::PutPowerToughnessCounters {
                    counters: PowerToughnessCounters {
                        plus_one: count.into(),
                        minus_one: 0.into(),
                    },
                },
            ),
        ));
    }
    let before = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let zero = round(cast_named(&before, &price));
    assert!(zero.draw_journal().events.is_empty());
    let mut state = before;
    for (spell, recipient) in [(7, 2), (8, 3), (9, 4)] {
        let id = physical(&state, recipient).id();
        state = round(targeted(&state, A, spell, TargetRef::Object(id)).into_state()).into_state();
    }
    let result = round(cast_named(&state, &price));
    assert_eq!(result.draw_journal().events.len(), 2);
    for n in [5, 6] {
        assert_eq!(physical(result.state(), n).zone(), ObjectZone::Hand(A));
    }
    assert_eq!(physical(result.state(), 100).zone(), ObjectZone::Library(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_phyrexian_gargantua_and_prophetic_prism_draw_on_entry_and_pay_their_costs() {
    use ManaType::*;
    for (name, price, count) in [
        ("Phyrexian Gargantua", vec![(Colourless, 4), (Black, 2)], 2),
        ("Prophetic Prism", vec![(Colourless, 2)], 1),
    ] {
        let mut budget = price.clone();
        if name == "Prophetic Prism" {
            budget[0].1 += 1;
        }
        let before = game(
            vec![A, B],
            vec![
                hand(name),
                named("Island", 2, A, ObjectDestination::LibraryBottom),
                named("Mountain", 3, A, ObjectDestination::LibraryBottom),
            ],
            &budget,
            Step::PrecombatMain,
        );
        let result = round(cast_named(&before, &price));
        assert_eq!(result.draw_journal().events.len(), count);
        assert_eq!(physical(result.state(), 2).zone(), ObjectZone::Hand(A));
        if name == "Phyrexian Gargantua" {
            assert_eq!(physical(result.state(), 3).zone(), ObjectZone::Hand(A));
            assert_eq!(result.state().execution().life_total(A), Some(&18.into()));
            continue;
        }
        let draw = result.into_state();
        assert_eq!(physical(&draw, 3).zone(), ObjectZone::Library(A));
        let choice = command(
            &draw,
            A,
            GameCommand::ActivateManaWithPayment {
                source: physical(&draw, 1).id(),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
                payment: spend(&[(Colourless, 1)]),
            },
        )
        .into_state();
        for color in [White, Blue, Black, Red, Green] {
            let mana = reply(&choice, DecisionReply::ManaColor(color)).into_state();
            assert_eq!(mana.execution().mana(A, Colourless), 0.into());
            assert_eq!(mana.execution().mana(A, color), 1.into());
            assert_eq!(physical(&mana, 1).tapped(), Some(true));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_tatyova_eidolon_and_setessan_champion_draw_only_for_their_entry_conditions() {
    use ManaType::*;
    for (name, price, self_draw, land) in [
        (
            "Tatyova, Benthic Druid",
            vec![(Colourless, 3), (Green, 1), (Blue, 1)],
            false,
            true,
        ),
        (
            "Eidolon of Blossoms",
            vec![(Colourless, 2), (Green, 2)],
            true,
            false,
        ),
        (
            "Setessan Champion",
            vec![(Colourless, 2), (Green, 1)],
            false,
            false,
        ),
    ] {
        let before = game(
            vec![A, B],
            vec![
                hand(name),
                named("Forest", 2, A, ObjectDestination::Hand),
                seed(
                    3,
                    A,
                    ObjectDestination::Hand,
                    Program::enchantment(Some(ManaCost::free())),
                ),
                seed(
                    4,
                    A,
                    ObjectDestination::Hand,
                    Program::artifact(Some(ManaCost::free()), vec![]),
                ),
                named("Island", 5, A, ObjectDestination::LibraryBottom),
                named("Plains", 6, A, ObjectDestination::LibraryBottom),
            ],
            &price,
            Step::PrecombatMain,
        );
        let entered = cast_named(&before, &price);
        assert_eq!(
            entered.execution().window().entries().len(),
            usize::from(self_draw)
        );
        let entered = if self_draw {
            round(entered).into_state()
        } else {
            entered
        };
        let artifact = round(cast_untargeted(&entered, 4, &[]).into_state()).into_state();
        assert!(artifact.execution().window().entries().is_empty());
        let played = command(
            &artifact,
            A,
            GameCommand::PlayLand {
                land: physical(&artifact, 2).id(),
            },
        )
        .into_state();
        assert_eq!(
            played.execution().window().entries().len(),
            usize::from(land)
        );
        let landed = if land {
            round(played).into_state()
        } else {
            played
        };
        let enchanted = round(cast_untargeted(&landed, 3, &[]).into_state()).into_state();
        assert_eq!(
            enchanted.execution().window().entries().len(),
            usize::from(!land)
        );
        let end = if land {
            enchanted
        } else {
            round(enchanted).into_state()
        };
        assert_eq!(physical(&end, 5).zone(), ObjectZone::Hand(A));
        assert_eq!(
            physical(&end, 6).zone(),
            if self_draw {
                ObjectZone::Hand(A)
            } else {
                ObjectZone::Library(A)
            }
        );
        assert_eq!(
            end.execution().life_total(A),
            Some(&(if land { 21_i64 } else { 20 }).into())
        );
        assert_eq!(
            end.execution()
                .counters_of_kind(physical(&end, 1).id(), CounterKind::PlusOne),
            i64::from(name == "Setessan Champion").into()
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_abundance_allows_a_failed_search_without_an_actual_draw() {
    use spellwake_engine::effects::{DrawReplacementChoice, DrawReplacementDomain};
    let price = [(ManaType::Colourless, 2), (ManaType::Green, 2)];
    let before = game(
        vec![A, B],
        vec![
            hand("Abundance"),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    Instruction::DrawController {
                        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    },
                ),
            ),
            named("Sol Ring", 3, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&before, &price);
    let offer = round(cast_untargeted(&entered, 2, &[]).into_state()).into_state();
    let choose = reply(
        &offer,
        DecisionReply::DrawReplacement(DrawReplacementChoice::Land),
    )
    .into_state();
    let RuleFrame::DrawReplacement(frame) = choose.pending_decision().unwrap().frame() else {
        panic!("expected remaining cards");
    };
    let DrawReplacementDomain::Bottom(domain) = &frame.domain else {
        panic!("expected bottom order");
    };
    assert_eq!(
        domain,
        [physical(&choose, 3).id(), physical(&choose, 100).id()].as_slice()
    );
    let order = domain.iter().copied().rev().collect::<Vec<_>>();
    let done = reply(
        &choose,
        DecisionReply::DrawReplacement(DrawReplacementChoice::Bottom(order.clone())),
    );
    assert!(done.draw_journal().events.is_empty());
    assert_eq!(done.state().execution().window().cards().library(A), order);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_alms_collector_leaves_single_draws_and_allows_the_affected_player_to_choose_a_collector()
 {
    use spellwake_engine::effects::{DrawReplacementChoice, DrawReplacementDomain};
    let mut cards = vec![
        named(
            "Alms Collector",
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        named(
            "Alms Collector",
            2,
            C,
            ObjectDestination::Battlefield { controller: C },
        ),
    ];
    for (n, count) in [(3, 1), (4, 3)] {
        cards.push(seed(
            n,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SinglePlayer,
                Instruction::DrawTargetPlayer {
                    amount: spellwake_engine::program::ValueExpr::Literal(count.into()),
                },
            ),
        ));
    }
    let state = game(vec![A, B, C], cards, &[], Step::PrecombatMain);
    let one = round(targeted(&state, A, 3, TargetRef::Player(B)).into_state());
    assert_eq!(one.draw_journal().events.len(), 1);
    assert_eq!(physical(one.state(), 101).zone(), ObjectZone::Hand(B));
    let offer = round(targeted(&state, A, 4, TargetRef::Player(B)).into_state()).into_state();
    let RuleFrame::DrawReplacement(frame) = offer.pending_decision().unwrap().frame() else {
        panic!("expected collector choice");
    };
    assert_eq!(frame.chooser, B);
    let DrawReplacementDomain::Collector(collectors) = &frame.domain else {
        panic!("expected affected player's alternatives");
    };
    assert_eq!(
        collectors.iter().copied().collect::<BTreeSet<_>>(),
        [A, C].into()
    );
    for (selected, skipped) in [(A, C), (C, A)] {
        let done = reply(
            &offer,
            DecisionReply::DrawReplacement(DrawReplacementChoice::Collector(selected)),
        );
        assert_eq!(done.draw_journal().events.len(), 2);
        for seat in [B, selected] {
            assert_eq!(
                physical(done.state(), 100 + u64::from(seat.0)).zone(),
                ObjectZone::Hand(seat)
            );
        }
        assert_eq!(
            physical(done.state(), 100 + u64::from(skipped.0)).zone(),
            ObjectZone::Library(skipped)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_deploy_the_gatewatch_and_animal_magnetism_handle_no_battlefield_selection() {
    let deploy = [(ManaType::Colourless, 4), (ManaType::White, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Deploy the Gatewatch"),
            named(
                "Vronos, Masked Inquisitor",
                2,
                A,
                ObjectDestination::LibraryBottom,
            ),
        ],
        &deploy,
        Step::PrecombatMain,
    );
    let select = cast_named(&initial, &deploy);
    let select = select
        .supply_entropy(select.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let none = reply(
        &select,
        DecisionReply::LibraryTop(LibraryTopReply::SearchBattlefield {
            selected_in_timestamp_order: vec![],
        }),
    );
    assert_eq!(
        none.state().execution().window().cards().library(A).len(),
        2
    );
    assert_eq!(physical(none.state(), 2).zone(), ObjectZone::Library(A));

    let animal = [(ManaType::Colourless, 4), (ManaType::Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Animal Magnetism"),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
        ],
        &animal,
        Step::PrecombatMain,
    );
    let select = cast_named(&initial, &animal);
    let done = reply(
        &select,
        DecisionReply::OrderResolutionGraveyard(vec![
            physical(&select, 100).id(),
            physical(&select, 2).id(),
        ]),
    );
    for n in [2, 100] {
        assert_eq!(physical(done.state(), n).zone(), ObjectZone::Graveyard(A));
    }
}
