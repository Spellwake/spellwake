//! Whole-Saga games follow the same permanent through the three printed chapters.
use super::*;
use spellwake_engine::effects::CounterKind;

fn chapter(state: &GameState, n: i64) {
    let saga = physical(state, 1);
    assert_eq!(saga.zone(), ObjectZone::Battlefield);
    assert_eq!(
        state
            .execution()
            .counters_of_kind(saga.id(), CounterKind::Lore),
        n.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_binding_the_old_gods_destroys_an_opposing_nonland_finds_a_forest_and_grants_deathtouch()
 {
    use ManaType::*;
    let price = [(Colourless, 2), (Black, 1), (Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Binding the Old Gods"),
            creature(2, A, 1, 5, &[]),
            creature(3, B, 1, 5, &[]),
            named(
                "Sol Ring",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Forest",
                5,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named("Island", 6, A, ObjectDestination::LibraryBottom),
            named("Forest", 7, A, ObjectDestination::LibraryBottom),
            named("Mountain", 8, A, ObjectDestination::LibraryBottom),
            named("Island", 9, B, ObjectDestination::LibraryBottom),
            named("Mountain", 10, B, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let first = cast_named(&initial, &price);
    chapter(&first, 1);
    let target = |n| DecisionReply::TriggerTarget(TargetRef::Object(physical(&first, n).id()));
    for n in [2, 5] {
        illegal(&first, A, combat_declaration(&first, target(n)));
    }
    let destroyed = round(reply(&first, target(4)).into_state()).into_state();
    assert_eq!(physical(&destroyed, 4).zone(), ObjectZone::Graveyard(B));
    let second = next_main(destroyed, 3);
    chapter(&second, 2);
    let choice = round(second).into_state();
    let ready = choice
        .supply_entropy(choice.basis(), vec![0; 12])
        .unwrap()
        .into_state();
    let RuleFrame::LibraryTop(frame) = ready.pending_decision().unwrap().frame() else {
        panic!("expected Forest search")
    };
    assert_eq!(frame.eligible_results(), [physical(&ready, 7).id()]);
    let found = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&ready, 7).id()),
        }),
    );
    assert_eq!(found.shuffle_journal().len(), 1);
    assert_eq!(
        (
            physical(found.state(), 7).zone(),
            physical(found.state(), 7).tapped()
        ),
        (ObjectZone::Battlefield, Some(true))
    );
    let third = next_main(found.into_state(), 5);
    chapter(&third, 3);
    let result = round(third).into_state();
    let venom = |s: &GameState, n| {
        s.execution()
            .current_characteristics(physical(s, n).id())
            .unwrap()
            .unwrap()
            .has_keyword(Deathtouch)
    };
    assert!(venom(&result, 2));
    assert!(!venom(&result, 3));
    assert_eq!(physical(&result, 1).zone(), ObjectZone::Graveyard(A));
    assert!(!venom(&next_main(result, 6), 2));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_binding_of_the_titans_mills_all_players_exiles_zero_to_two_and_recovers_an_own_creature_or_land()
 {
    use ManaType::*;
    let price = [(Colourless, 1), (Green, 1)];
    let mut cards = vec![hand("The Binding of the Titans")];
    for (seat, start) in [(A, 2), (B, 8)] {
        for (offset, name) in ["Elvish Mystic", "Forest", "Scaretiller", "Island", "Forest"]
            .into_iter()
            .enumerate()
        {
            cards.push(named(
                name,
                start + offset as u64,
                seat,
                ObjectDestination::LibraryBottom,
            ));
        }
    }
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let first = cast_named(&initial, &price);
    chapter(&first, 1);
    let mut milled = round(first).into_state();
    for (seat, start) in [(A, 2), (B, 8)] {
        let RuleFrame::ResolutionGraveyardOrder(frame) = milled.pending_decision().unwrap().frame()
        else {
            panic!("expected each owner's three milled cards")
        };
        assert_eq!(frame.chooser(), seat);
        let order = (start..start + 3)
            .rev()
            .map(|n| physical(&milled, n).id())
            .collect();
        milled = reply(&milled, DecisionReply::OrderResolutionGraveyard(order)).into_state();
    }
    for (seat, start) in [(A, 2), (B, 8)] {
        for n in start..start + 3 {
            assert_eq!(physical(&milled, n).zone(), ObjectZone::Graveyard(seat));
        }
    }
    let second = next_main(milled, 3);
    chapter(&second, 2);
    for (targets, gain) in [(vec![], 0_i64), (vec![9], 0), (vec![2, 8], 2)] {
        let ts = targets
            .iter()
            .map(|&n| TargetRef::Object(physical(&second, n).id()))
            .collect();
        let selected = reply(&second, DecisionReply::TriggerPermanentTargets(ts)).into_state();
        let exiled = round(selected).into_state();
        assert_eq!(exiled.execution().life_total(A), Some(&(20 + gain).into()));
        for n in targets {
            assert_eq!(physical(&exiled, n).zone(), ObjectZone::Exile);
        }
        let third = next_main(exiled, 5);
        chapter(&third, 3);
        for n in [3, 4] {
            let chose = reply(
                &third,
                DecisionReply::TriggerTarget(TargetRef::Object(physical(&third, n).id())),
            )
            .into_state();
            let end = round(chose).into_state();
            assert_eq!(physical(&end, n).zone(), ObjectZone::Hand(A));
            assert_eq!(physical(&end, 1).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_mending_of_dominaria_mills_and_recovers_twice_then_returns_all_own_lands_before_shuffling()
 {
    use ManaType::*;
    let price = [(Colourless, 3), (Green, 2)];
    let mut cards = vec![
        hand("The Mending of Dominaria"),
        named("Elvish Mystic", 2, A, ObjectDestination::LibraryBottom),
        named("Forest", 3, A, ObjectDestination::LibraryBottom),
        named("Island", 4, A, ObjectDestination::LibraryBottom),
        named("Scaretiller", 5, A, ObjectDestination::LibraryBottom),
        named("Mountain", 6, A, ObjectDestination::LibraryBottom),
        named("Island", 7, A, ObjectDestination::LibraryBottom),
        named("Island", 8, B, ObjectDestination::Graveyard),
        named("Sol Ring", 9, A, ObjectDestination::Graveyard),
    ];
    cards.push(named("Swamp", 10, B, ObjectDestination::LibraryBottom));
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let mut saga = cast_named(&initial, &price);
    for (number, ids, take) in [(1, [2, 3], true), (2, [5, 6], false)] {
        chapter(&saga, number);
        let ordering = round(saga).into_state();
        let choice = reply(
            &ordering,
            DecisionReply::OrderResolutionGraveyard(
                ids.map(|n| physical(&ordering, n).id()).to_vec(),
            ),
        )
        .into_state();
        let RuleFrame::ResolutionSelect(frame) = choice.pending_decision().unwrap().frame() else {
            panic!("expected optional creature return")
        };
        assert_eq!(frame.domain(), [physical(&choice, ids[0]).id()]);
        let result = reply(
            &choice,
            DecisionReply::SelectResolution(if take {
                vec![physical(&choice, ids[0]).id()]
            } else {
                vec![]
            }),
        )
        .into_state();
        assert_eq!(
            physical(&result, ids[0]).zone(),
            if take {
                ObjectZone::Hand(A)
            } else {
                ObjectZone::Graveyard(A)
            }
        );
        saga = next_main(result, if number == 1 { 3 } else { 5 });
    }
    chapter(&saga, 3);
    let ready = saga
        .supply_entropy(saga.basis(), vec![0; 30])
        .unwrap()
        .into_state();
    let mut result = round(ready);
    while let Some(decision) = result.state().pending_decision() {
        let choice = match decision.frame() {
            RuleFrame::LinkedReturnOrder(frame) => {
                DecisionReply::LinkedReturnOrder(frame.objects().to_vec())
            }
            RuleFrame::TimestampOrder(frame) => {
                DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect())
            }
            other => panic!("unexpected land return choice {other:?}"),
        };
        result = reply(result.state(), choice);
    }
    let state = result.state();
    assert_eq!(result.shuffle_journal().len(), 1);
    for n in [3, 6] {
        assert_eq!(physical(state, n).zone(), ObjectZone::Battlefield);
    }
    for n in [5, 9] {
        assert_eq!(physical(state, n).zone(), ObjectZone::Library(A));
    }
    assert_eq!(physical(state, 8).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(state, 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_eldest_reborn_makes_each_opponent_sacrifice_and_discard_then_returns_a_creature_or_walker()
 {
    use ManaType::*;
    use spellwake_engine::mana::{ManaAbility, ManaOutput};
    let price = [(Colourless, 4), (Black, 1)];
    let land = |n, seat, color, neutral: i64, colored: i64| {
        seed(
            n,
            seat,
            ObjectDestination::Battlefield { controller: seat },
            Program::land(
                Default::default(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed(
                        [(color, colored.into()), (Colourless, neutral.into())].into(),
                    ),
                }],
            ),
        )
    };
    let mut cards = vec![
        hand("The Eldest Reborn"),
        named("Vronos, Masked Inquisitor", 2, B, ObjectDestination::Hand),
        creature(3, B, 1, 1, &[]),
        creature(4, C, 1, 1, &[]),
        creature(5, A, 1, 1, &[]),
        land(6, A, Black, 4_i64, 1_i64),
        land(7, B, Blue, 3, 2),
        named("Sol Ring", 8, B, ObjectDestination::Hand),
        named("Forest", 9, C, ObjectDestination::Hand),
        named("Elvish Mystic", 10, A, ObjectDestination::Graveyard),
    ];
    for (seat, start) in [(A, 20), (B, 30), (C, 40)] {
        for n in start..start + 5 {
            cards.push(named("Island", n, seat, ObjectDestination::LibraryBottom));
        }
    }
    let initial = game(vec![A, B, C], cards, &[], Step::PrecombatMain);
    let b = next_main(initial, 2);
    let b = command(
        &b,
        B,
        GameCommand::ActivateMana {
            source: physical(&b, 7).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let b = round(
        command(
            &b,
            B,
            GameCommand::CastUntargeted {
                spell: physical(&b, 2).id(),
                payment: Some(spend(&[(Colourless, 3), (Blue, 2)])),
            },
        )
        .into_state(),
    )
    .into_state();
    let a = next_main(b, 4);
    let a = command(
        &a,
        A,
        GameCommand::ActivateMana {
            source: physical(&a, 6).id(),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    )
    .into_state();
    let saga = cast_named(&a, &price);
    chapter(&saga, 1);
    let choose = round(saga).into_state();
    for sacrifice in [2, 3] {
        let RuleFrame::ResolutionSelect(frame) = choose.pending_decision().unwrap().frame() else {
            panic!("expected sacrifice")
        };
        assert_eq!(frame.chooser(), B);
        assert_eq!(
            frame.domain().iter().copied().collect::<BTreeSet<_>>(),
            [2, 3].map(|n| physical(&choose, n).id()).into()
        );
        let c = reply(
            &choose,
            DecisionReply::SelectResolution(vec![physical(&choose, sacrifice).id()]),
        )
        .into_state();
        assert_eq!(c.pending_decision().unwrap().chooser(), C);
        let sacrificed = reply(
            &c,
            DecisionReply::SelectResolution(vec![physical(&c, 4).id()]),
        )
        .into_state();
        assert_eq!(
            physical(&sacrificed, sacrifice).zone(),
            ObjectZone::Graveyard(B)
        );
        assert_eq!(physical(&sacrificed, 4).zone(), ObjectZone::Graveyard(C));
        assert_eq!(physical(&sacrificed, 5).zone(), ObjectZone::Battlefield);
        let second = next_main(sacrificed, 7);
        chapter(&second, 2);
        let mut discard = round(second).into_state();
        for (seat, n) in [(B, 8), (C, 9)] {
            assert_eq!(discard.pending_decision().unwrap().chooser(), seat);
            discard = reply(
                &discard,
                DecisionReply::SelectResolution(vec![physical(&discard, n).id()]),
            )
            .into_state();
        }
        for (seat, n) in [(B, 8), (C, 9)] {
            assert_eq!(physical(&discard, n).zone(), ObjectZone::Graveyard(seat));
        }
        let third = next_main(discard, 10);
        chapter(&third, 3);
        for n in [sacrifice, 10] {
            let target = TargetRef::Object(physical(&third, n).id());
            let end = round(reply(&third, DecisionReply::TriggerTarget(target)).into_state())
                .into_state();
            assert_eq!(
                (physical(&end, n).zone(), physical(&end, n).controller()),
                (ObjectZone::Battlefield, Some(A))
            );
            assert_eq!(physical(&end, 1).zone(), ObjectZone::Graveyard(A));
        }
    }
}

pub(super) fn check_chapter_token(
    s: &GameState,
    id: ObjectId,
    color: Color,
    size: i64,
    types: &[&str],
    keywords: &[KeywordAbility],
) {
    let v = s.execution().current_characteristics(id).unwrap().unwrap();
    assert_eq!(v.current_colors, [color].into());
    assert_eq!(v.current_types, [CardType::Creature].into());
    assert_eq!(
        v.current_creature_subtypes,
        types
            .iter()
            .map(|n| current_catalogue().1.resolve_label(n).unwrap())
            .collect()
    );
    assert_eq!(v.keywords, keywords.iter().copied().collect());
    assert_eq!(
        v.power_toughness.unwrap().derived,
        PowerToughness {
            power: size.into(),
            toughness: size.into()
        }
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_love_song_of_night_and_day_reads_ahead_and_resolves_only_its_chosen_and_later_chapters()
 {
    use ManaType::*;
    use spellwake_engine::land::EntryChoice;
    let price = [(Colourless, 2), (White, 1)];
    let mut cards = vec![
        hand("Love Song of Night and Day"),
        creature(2, A, 1, 3, &[]),
        creature(3, B, 1, 3, &[]),
    ];
    for (owner, start) in [(A, 4), (B, 10)] {
        for n in start..start + 5 {
            cards.push(named("Forest", n, owner, ObjectDestination::LibraryBottom));
        }
    }
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let entry = cast_named(&initial, &price);
    for start in [1u8, 2, 3] {
        let mut state = reply(
            &entry,
            DecisionReply::EntryChoice(EntryChoice::Chapter(start)),
        )
        .into_state();
        chapter(&state, i64::from(start));
        if start == 1 {
            illegal(
                &state,
                A,
                combat_declaration(&state, DecisionReply::TriggerTarget(TargetRef::Player(A))),
            );
            let selected =
                reply(&state, DecisionReply::TriggerTarget(TargetRef::Player(B))).into_state();
            let drawn = round(selected).into_state();
            for (seat, nums) in [(A, [4, 5]), (B, [10, 11])] {
                for n in nums {
                    assert_eq!(physical(&drawn, n).zone(), ObjectZone::Hand(seat));
                }
            }
            state = next_main(drawn, 3);
        } else {
            assert_eq!(physical(&state, 4).zone(), ObjectZone::Library(A));
        }
        if start < 3 {
            let bird = round(state);
            let id = tokens(&bird, A, 1)[0];
            check_chapter_token(bird.state(), id, Color::White, 1, &["Bird"], &[Flying]);
            state = next_main(bird.into_state(), if start == 1 { 5 } else { 3 });
        } else {
            assert!(
                state
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .all(|o| !o.is_token())
            );
        }
        chapter(&state, 3);
        for picks in [vec![], vec![2, 3]] {
            let selected = reply(
                &state,
                DecisionReply::TriggerPermanentTargets(
                    picks
                        .iter()
                        .map(|&n| TargetRef::Object(physical(&state, n).id()))
                        .collect(),
                ),
            )
            .into_state();
            let end = round(selected).into_state();
            for n in [2, 3] {
                assert_eq!(
                    end.execution()
                        .counters_of_kind(physical(&end, n).id(), CounterKind::PlusOne),
                    (if picks.contains(&n) { 1_i64 } else { 0 }).into()
                );
            }
            assert_eq!(physical(&end, 1).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_battle_for_bretagard_creates_two_distinct_warriors_and_copies_only_differently_named_tokens()
 {
    use ManaType::*;
    let price = [(Colourless, 1), (Green, 1), (White, 1)];
    let saga = hand("Battle for Bretagard");
    let make_human = saga.program.triggered[0].body.clone();
    let initial = game(
        vec![A, B],
        vec![
            saga,
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::instant(ManaCost::free(), make_human),
            ),
            named("Forest", 3, A, ObjectDestination::LibraryBottom),
            named("Forest", 4, B, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let one = cast_named(&initial, &price);
    chapter(&one, 1);
    let human = round(one);
    let first = tokens(&human, A, 1)[0];
    check_chapter_token(
        human.state(),
        first,
        Color::White,
        1,
        &["Human", "Warrior"],
        &[],
    );
    let repeat = round(cast_untargeted(human.state(), 2, &[]).into_state()).into_state();
    let other = repeat
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| o.is_token() && o.id() != first && o.zone() == ObjectZone::Battlefield)
        .unwrap()
        .id();
    let two = next_main(repeat, 3);
    chapter(&two, 2);
    let elf = round(two).into_state();
    let elf_id = elf
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| {
            o.is_token()
                && o.id() != first
                && o.id() != other
                && o.zone() == ObjectZone::Battlefield
        })
        .unwrap()
        .id();
    check_chapter_token(&elf, elf_id, Color::Green, 1, &["Elf", "Warrior"], &[]);
    let three = next_main(elf, 5);
    chapter(&three, 3);
    let choose = round(three).into_state();
    illegal(
        &choose,
        A,
        combat_declaration(&choose, DecisionReply::SelectResolution(vec![first, other])),
    );
    for indices in [vec![], vec![first, elf_id]] {
        let mut copied =
            reply(&choose, DecisionReply::SelectResolution(indices.clone())).into_state();
        if let Some(RuleFrame::TimestampOrder(frame)) = copied.pending_decision().map(|d| d.frame())
        {
            copied = reply(
                &copied,
                DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect()),
            )
            .into_state();
        }
        let all = copied
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
            .map(GameObject::id)
            .collect::<Vec<_>>();
        assert_eq!(all.len(), 3 + indices.len());
        if !indices.is_empty() {
            let count = |color| {
                all.iter()
                    .filter(|&&id| {
                        copied
                            .execution()
                            .current_characteristics(id)
                            .unwrap()
                            .unwrap()
                            .current_colors
                            .contains(&color)
                    })
                    .count()
            };
            assert_eq!(count(Color::White), 3);
            assert_eq!(count(Color::Green), 2);
        }
        assert_eq!(physical(&copied, 1).zone(), ObjectZone::Graveyard(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_battle_at_the_helvault_limits_each_players_exiles_and_returns_both_chapters_cohorts() {
    use ManaType::*;
    let price = [(Colourless, 4), (White, 2)];
    let mut cards = vec![
        hand("Battle at the Helvault"),
        creature(2, A, 1, 2, &[]),
        creature(3, A, 1, 2, &[]),
        creature(4, B, 1, 2, &[]),
        creature(5, B, 1, 2, &[]),
        named(
            "Forest",
            6,
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
    ];
    for (seat, start) in [(A, 7), (B, 10)] {
        for n in start..start + 2 {
            cards.push(named("Island", n, seat, ObjectDestination::LibraryBottom));
        }
    }
    let setup = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let start = cast_named(&setup, &price);
    let select = |s: &GameState, ns: &[u64]| {
        DecisionReply::TriggerPermanentTargets(
            ns.iter()
                .map(|&n| TargetRef::Object(physical(s, n).id()))
                .collect(),
        )
    };
    for ids in [vec![1], vec![6], vec![2, 3]] {
        illegal(&start, A, combat_declaration(&start, select(&start, &ids)));
    }
    let empty = round(reply(&start, select(&start, &[])).into_state()).into_state();
    assert_eq!(physical(&empty, 2).zone(), ObjectZone::Battlefield);
    let first = round(reply(&start, select(&start, &[2, 4])).into_state()).into_state();
    let next = next_main(first, 3);
    let second = round(reply(&next, select(&next, &[3, 5])).into_state()).into_state();
    for n in 2..=5 {
        assert_eq!(physical(&second, n).zone(), ObjectZone::Exile);
    }
    let last = next_main(second, 5);
    let mut ended = round(last).into_state();
    while let Some(d) = ended.pending_decision() {
        let pick = match d.frame() {
            RuleFrame::LinkedReturnOrder(frame) => {
                DecisionReply::LinkedReturnOrder(frame.objects().to_vec())
            }
            RuleFrame::TimestampOrder(frame) => {
                DecisionReply::TimestampOrder(frame.members().iter().map(|m| m.member).collect())
            }
            other => panic!("unexpected Helvault return {other:?}"),
        };
        ended = reply(&ended, pick).into_state();
    }
    assert_eq!(physical(&ended, 1).zone(), ObjectZone::Graveyard(A));
    for (n, seat) in [(2, A), (3, A), (4, B), (5, B)] {
        assert_eq!(
            (physical(&ended, n).zone(), physical(&ended, n).controller()),
            (ObjectZone::Battlefield, Some(seat))
        );
    }
    let avacyn = ended
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .unwrap()
        .id();
    check_chapter_token(
        &ended,
        avacyn,
        Color::White,
        8,
        &["Angel"],
        &[Flying, Vigilance, Indestructible],
    );
    let profile = ended
        .execution()
        .current_characteristics(avacyn)
        .unwrap()
        .unwrap();
    assert!(profile.current_names.iter().any(|n| n.as_str() == "Avacyn"));
    assert!(
        profile
            .current_supertypes
            .contains(&spellwake_engine::program::Supertype::Legendary)
    );
}
