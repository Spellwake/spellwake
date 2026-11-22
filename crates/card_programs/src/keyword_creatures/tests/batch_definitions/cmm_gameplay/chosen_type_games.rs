//! The permanent's type declaration is reused for only its own costs, bonuses and card selections.
use super::*;
use spellwake_engine::land::EntryChoice;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_heralds_horn_reduces_its_chosen_creature_cost_and_can_reveal_the_matching_upkeep_card()
{
    let elf = current_catalogue().1.resolve_label("Elf").unwrap();
    let goblin = current_catalogue().1.resolve_label("Goblin").unwrap();
    let body = |n, zone, kind| {
        seed(
            n,
            A,
            zone,
            Program::creature(
                Some(ManaCost::generic(2.into())),
                2.into(),
                2.into(),
                [kind].into(),
            ),
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Herald's Horn"),
            body(2, ObjectDestination::Hand, elf),
            body(3, ObjectDestination::Hand, goblin),
            body(4, ObjectDestination::LibraryBottom, elf),
        ],
        &[(ManaType::Colourless, 5)],
        Step::PrecombatMain,
    );
    let choose = cast_named(&initial, &[(ManaType::Colourless, 3)]);
    for (chosen, discounted, full, eligible) in [(elf, 2, 3, true), (goblin, 3, 2, false)] {
        let live = reply(
            &choose,
            DecisionReply::EntryChoice(EntryChoice::CreatureType(chosen)),
        )
        .into_state();
        illegal(
            &live,
            A,
            GameCommand::CastUntargeted {
                spell: physical(&live, full).id(),
                payment: Some(spend(&[(ManaType::Colourless, 1)])),
            },
        );
        let discounted =
            round(cast_untargeted(&live, discounted, &[(ManaType::Colourless, 1)]).into_state())
                .into_state();
        assert_eq!(
            discounted.execution().mana(A, ManaType::Colourless),
            1.into()
        );
        let own = exile_hand::until(discounted, |s| {
            s.turn().unwrap().position().number == 3
                && s.turn().unwrap().position().step == Step::Upkeep
        });
        let look = round(own).into_state();
        let wanted = physical(&look, 4).id();
        if !eligible && look.pending_decision().is_none() {
            assert_eq!(look.execution().window().cards().library(A)[0], wanted);
            continue;
        }
        let RuleFrame::LibraryTop(frame) = look.pending_decision().unwrap().frame() else {
            panic!("expected top-card look");
        };
        assert_eq!(frame.eligible_results().contains(&wanted), eligible);
        let skip = reply(
            &look,
            DecisionReply::LibraryTop(LibraryTopReply::Search { selected: None }),
        );
        assert_eq!(
            skip.state().execution().window().cards().library(A)[0],
            wanted
        );
        if eligible {
            let found = reply(
                &look,
                DecisionReply::LibraryTop(LibraryTopReply::Search {
                    selected: Some(wanted),
                }),
            );
            assert_eq!(physical(found.state(), 4).zone(), ObjectZone::Hand(A));
            assert!(
                events(&found)
                    .flat_map(|e| &e.occurrences)
                    .any(|x| matches!(x.value, RulesOccurrence::LibraryCardRevealed { .. }))
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_icon_and_banner_boost_only_their_type_and_distinguish_looking_from_a_cast_trigger() {
    let elf = current_catalogue().1.resolve_label("Elf").unwrap();
    let goblin = current_catalogue().1.resolve_label("Goblin").unwrap();
    let body = |n, owner, zone, kind| {
        seed(
            n,
            owner,
            zone,
            Program::creature(Some(ManaCost::free()), 1.into(), 2.into(), [kind].into()),
        )
    };
    for (name, icon, cost) in [
        ("Icon of Ancestry", true, 3_i64),
        ("Vanquisher's Banner", false, 5),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                body(2, A, ObjectDestination::Battlefield { controller: A }, elf),
                body(
                    3,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    goblin,
                ),
                body(4, B, ObjectDestination::Battlefield { controller: B }, elf),
                body(5, A, ObjectDestination::Hand, elf),
                body(6, A, ObjectDestination::Hand, goblin),
                body(7, A, ObjectDestination::LibraryBottom, elf),
                body(8, A, ObjectDestination::LibraryBottom, goblin),
                named("Forest", 9, A, ObjectDestination::LibraryBottom),
            ],
            &[(ManaType::Colourless, cost + 3)],
            Step::PrecombatMain,
        );
        let choose = cast_named(&initial, &[(ManaType::Colourless, cost)]);
        let live = reply(
            &choose,
            DecisionReply::EntryChoice(EntryChoice::CreatureType(elf)),
        )
        .into_state();
        for (n, power) in [(2, 2), (3, 1), (4, 1)] {
            let actual = live
                .execution()
                .power_toughness(physical(&live, n).id())
                .unwrap()
                .unwrap()
                .derived;
            assert_eq!(
                actual,
                PowerToughness {
                    power: power.into(),
                    toughness: (power + 1).into()
                }
            );
        }
        if icon {
            let activated = command(
                &live,
                A,
                GameCommand::ActivateUntargetedAbility {
                    source: physical(&live, 1).id(),
                    ability: ActivatedAbilityId(0),
                    payment: Some(spend(&[(ManaType::Colourless, 3)])),
                },
            )
            .into_state();
            assert_eq!(physical(&activated, 1).tapped(), Some(true));
            let look = round(activated).into_state();
            let ready = look
                .supply_entropy(look.basis(), vec![0; 8])
                .unwrap()
                .into_state();
            let RuleFrame::LibraryTop(frame) = ready.pending_decision().unwrap().frame() else {
                panic!("expected three cards");
            };
            assert_eq!(frame.domain_top_first().len(), 3);
            assert_eq!(frame.eligible_results(), [physical(&ready, 7).id()]);
            for take in [false, true] {
                let found = reply(
                    &ready,
                    DecisionReply::LibraryTop(LibraryTopReply::Search {
                        selected: take.then(|| physical(&ready, 7).id()),
                    }),
                );
                assert_eq!(
                    physical(found.state(), 7).zone(),
                    if take {
                        ObjectZone::Hand(A)
                    } else {
                        ObjectZone::Library(A)
                    }
                );
                assert_eq!(
                    found.state().execution().window().cards().library(A)[0],
                    physical(found.state(), 100).id()
                );
            }
        } else {
            for (n, expected) in [(6, 1), (5, 2)] {
                let spell = cast_untargeted(&live, n, &[]).into_state();
                assert_eq!(spell.execution().window().entries().len(), expected);
                if n == 5 {
                    let drawn = round(spell).into_state();
                    assert_eq!(physical(&drawn, 7).zone(), ObjectZone::Hand(A));
                    assert_eq!(physical(&drawn, 5).zone(), ObjectZone::Stack);
                }
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_read_the_bones_still_loses_life_after_a_collector_replaces_its_draws() {
    let price = [(ManaType::Colourless, 2), (ManaType::Black, 1)];
    let state = game(
        vec![A, B],
        vec![
            hand("Read the Bones"),
            named(
                "Alms Collector",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named("Swamp", 3, A, ObjectDestination::LibraryBottom),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let scry = cast_named(&state, &price);
    let keep = [physical(&scry, 4).id(), physical(&scry, 3).id()];
    let done = reply(
        &scry,
        DecisionReply::LibraryTop(LibraryTopReply::Scry {
            top_first: keep.to_vec(),
            bottom_top_first: vec![],
        }),
    );
    assert_eq!(physical(done.state(), 4).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(done.state(), 3).zone(), ObjectZone::Library(A));
    assert_eq!(physical(done.state(), 101).zone(), ObjectZone::Hand(B));
    assert_eq!(done.state().execution().life_total(A), Some(&18.into()));
}
