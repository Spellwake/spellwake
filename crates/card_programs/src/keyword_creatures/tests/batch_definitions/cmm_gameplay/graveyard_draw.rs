//! Named zone movement and draw scenarios using the current registry.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_squee_and_reassembling_skeleton_return_from_their_own_graveyards() {
    use ManaType::*;
    use spellwake_engine::effects::OptionalEffectChoice;
    for (name, price, budget) in [
        (
            "Squee, Goblin Nabob",
            vec![(Colourless, 2), (Red, 1)],
            vec![(Colourless, 2), (Red, 1)],
        ),
        (
            "Reassembling Skeleton",
            vec![(Colourless, 1), (Black, 1)],
            vec![(Colourless, 2), (Black, 2)],
        ),
    ] {
        let state = game(
            vec![A, B],
            vec![
                hand(name),
                seed(
                    2,
                    A,
                    ObjectDestination::Hand,
                    free_instant(
                        BodyTarget::SingleCreature,
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics:
                                        spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                            allow_regenerate: true,
                                        },
                                },
                            ),
                        ),
                    ),
                ),
            ],
            &budget,
            Step::PrecombatMain,
        );
        let first = cast_named(&state, &price);
        let killed =
            round(targeted(&first, A, 2, TargetRef::Object(physical(&first, 1).id())).into_state())
                .into_state();
        assert_eq!(physical(&killed, 1).zone(), ObjectZone::Graveyard(A));
        if name == "Reassembling Skeleton" {
            let action = |s: &GameState| GameCommand::ActivateUntargetedAbility {
                source: physical(s, 1).id(),
                ability: ActivatedAbilityId(0),
                payment: Some(spend(&price)),
            };
            illegal(&first, A, action(&first));
            let stacked = command(&killed, A, action(&killed)).into_state();
            let returned = round(stacked);
            let card = physical(returned.state(), 1);
            assert_eq!(
                (card.zone(), card.tapped()),
                (ObjectZone::Battlefield, Some(true))
            );
            assert_ne!(card.id(), physical(&first, 1).id());
        } else {
            let other = exile_hand::until(killed, |s| {
                s.turn().unwrap().position().number == 2
                    && s.turn().unwrap().position().step == Step::Upkeep
            });
            assert!(other.execution().window().entries().is_empty());
            let own = exile_hand::until(other, |s| {
                s.turn().unwrap().position().number == 3
                    && s.turn().unwrap().position().step == Step::Upkeep
            });
            let choose = round(own).into_state();
            for (choice, zone) in [
                (OptionalEffectChoice::Take, ObjectZone::Hand(A)),
                (OptionalEffectChoice::Decline, ObjectZone::Graveyard(A)),
            ] {
                let result = reply(&choose, DecisionReply::OptionalEffect(choice));
                assert_eq!(physical(result.state(), 1).zone(), zone);
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_hanna_and_living_lightning_recover_only_their_printed_graveyard_card_types() {
    use ManaType::*;
    for (name, price, budget) in [
        (
            "Hanna, Ship's Navigator",
            vec![(Colourless, 1), (White, 1), (Blue, 1)],
            vec![(Colourless, 2), (White, 2), (Blue, 2)],
        ),
        (
            "Living Lightning",
            vec![(Colourless, 3), (Red, 1)],
            vec![(Colourless, 3), (Red, 1)],
        ),
    ] {
        let hanna = name == "Hanna, Ship's Navigator";
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                named(
                    "Fervor",
                    2,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
                named("Sol Ring", 3, A, ObjectDestination::Graveyard),
                named("Fervor", 4, A, ObjectDestination::Graveyard),
                named("Counterspell", 5, A, ObjectDestination::Graveyard),
                named("Cultivate", 6, A, ObjectDestination::Graveyard),
                named("Forest", 7, A, ObjectDestination::Graveyard),
                named(
                    if hanna { "Sol Ring" } else { "Counterspell" },
                    8,
                    B,
                    ObjectDestination::Graveyard,
                ),
                seed(
                    9,
                    A,
                    ObjectDestination::Hand,
                    free_instant(
                        BodyTarget::SingleCreature,
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics:
                                        spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                            allow_regenerate: true,
                                        },
                                },
                            ),
                        ),
                    ),
                ),
            ],
            &budget,
            Step::PrecombatMain,
        );
        let entered = cast_named(&initial, &price);
        let targets = if hanna {
            entered
        } else {
            round(
                targeted(
                    &entered,
                    A,
                    9,
                    TargetRef::Object(physical(&entered, 1).id()),
                )
                .into_state(),
            )
            .into_state()
        };
        let choose = |n| {
            let target = TargetRef::Object(physical(&targets, n).id());
            if hanna {
                GameCommand::ActivateAbility {
                    source: physical(&targets, 1).id(),
                    ability: ActivatedAbilityId(0),
                    target,
                    payment: Some(spend(&price)),
                }
            } else {
                combat_declaration(&targets, DecisionReply::TriggerTarget(target))
            }
        };
        for n in if hanna {
            vec![5, 6, 7, 8]
        } else {
            vec![3, 4, 7, 8]
        } {
            illegal(&targets, A, choose(n));
        }
        for n in if hanna { [3, 4] } else { [5, 6] } {
            let stacked = command(&targets, A, choose(n)).into_state();
            if hanna {
                assert_eq!(physical(&stacked, 1).tapped(), Some(true));
            }
            let recovered = round(stacked);
            assert_eq!(physical(recovered.state(), n).zone(), ObjectZone::Hand(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_distant_melody_counts_only_controlled_permanents_of_the_chosen_type() {
    let price = [(ManaType::Colourless, 3), (ManaType::Blue, 1)];
    let elf = current_catalogue().1.resolve_label("Elf").unwrap();
    let goblin = current_catalogue().1.resolve_label("Goblin").unwrap();
    let initial = game(
        vec![A, B],
        vec![
            hand("Distant Melody"),
            named(
                "Elvish Mystic",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Elvish Mystic",
                3,
                B,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(
                "Elvish Mystic",
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named("Forest", 5, A, ObjectDestination::LibraryBottom),
            named("Island", 6, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let choose = cast_named(&initial, &price);
    for (kind, n) in [(elf, 2), (goblin, 0)] {
        let drawn = reply(&choose, DecisionReply::CreatureType(kind));
        assert_eq!(drawn.draw_journal().events.len(), n);
        for id in [5, 6] {
            assert_eq!(
                physical(drawn.state(), id).zone(),
                if n == 2 {
                    ObjectZone::Hand(A)
                } else {
                    ObjectZone::Library(A)
                }
            );
        }
        assert_eq!(physical(drawn.state(), 100).zone(), ObjectZone::Library(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_windfall_counts_the_largest_discard_and_draws_that_many_for_each_player() {
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 1)];
    let mut cards = vec![hand("Windfall")];
    for (n, seat) in [(2, A), (3, B), (4, B), (5, B)] {
        cards.push(seed(
            n,
            seat,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ));
    }
    for (n, seat) in [(6, A), (7, A), (8, B), (9, B), (10, C), (11, C)] {
        cards.push(seed(
            n,
            seat,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let mut resolving = cast_named(&initial, &price);
    while let Some(decision) = resolving.pending_decision() {
        let choice = match decision.frame() {
            RuleFrame::ResolutionSelect(frame) => {
                assert_eq!(frame.required(), frame.domain().len());
                DecisionReply::SelectResolution(frame.domain().to_vec())
            }
            RuleFrame::ResolutionGraveyardOrder(frame) => DecisionReply::OrderResolutionGraveyard(
                frame.domain().iter().copied().rev().collect(),
            ),
            other => panic!("unexpected windfall choice {other:?}"),
        };
        resolving = reply(&resolving, choice).into_state();
    }
    for (n, seat) in [(2, A), (3, B), (4, B), (5, B)] {
        assert_eq!(physical(&resolving, n).zone(), ObjectZone::Graveyard(seat));
    }
    for (seat, ids) in [(A, [6, 7, 100]), (B, [8, 9, 101]), (C, [10, 11, 102])] {
        for n in ids {
            assert_eq!(physical(&resolving, n).zone(), ObjectZone::Hand(seat));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_frantic_search_draws_then_discards_then_untaps_up_to_three_chosen_lands() {
    use ManaType::*;
    use spellwake_engine::land::BasicLandType;
    let mut cards = vec![
        hand("Frantic Search"),
        named("Sol Ring", 6, A, ObjectDestination::LibraryBottom),
        named("Arcane Signet", 7, A, ObjectDestination::LibraryBottom),
        named("Forest", 8, A, ObjectDestination::Hand),
    ];
    let lands = [
        ("Island", BasicLandType::Island),
        ("Mountain", BasicLandType::Mountain),
        ("Forest", BasicLandType::Forest),
        ("Plains", BasicLandType::Plains),
    ];
    for (n, (name, _)) in (2..6).zip(lands) {
        cards.push(named(
            name,
            n,
            A,
            ObjectDestination::Battlefield { controller: A },
        ));
    }
    let mut state = game(vec![A, B], cards, &[], Step::PrecombatMain);
    for (n, (_, kind)) in (2..6).zip(lands) {
        state = command(
            &state,
            A,
            GameCommand::ActivateMana {
                source: physical(&state, n).id(),
                ability: ManaAbilityRef::BasicLand(kind),
            },
        )
        .into_state();
    }
    let draw = round(cast_untargeted(&state, 1, &[(Blue, 1), (Red, 1), (Green, 1)]).into_state())
        .into_state();
    for n in [6, 7] {
        assert_eq!(physical(&draw, n).zone(), ObjectZone::Hand(A));
    }
    let discard = [physical(&draw, 7).id(), physical(&draw, 8).id()];
    let order = reply(&draw, DecisionReply::SelectResolution(discard.to_vec())).into_state();
    let choose = reply(
        &order,
        DecisionReply::OrderResolutionGraveyard(discard.to_vec()),
    )
    .into_state();
    for n in [7, 8] {
        assert_eq!(physical(&choose, n).zone(), ObjectZone::Graveyard(A));
    }
    assert_eq!(physical(&choose, 6).zone(), ObjectZone::Hand(A));
    let all = (2..6)
        .map(|n| physical(&choose, n).id())
        .collect::<Vec<_>>();
    illegal(
        &choose,
        A,
        combat_declaration(&choose, DecisionReply::SelectResolution(all.clone())),
    );
    for count in [0, 3] {
        let finish = reply(
            &choose,
            DecisionReply::SelectResolution(all[..count].to_vec()),
        );
        for (index, n) in (2..6).enumerate() {
            assert_eq!(physical(finish.state(), n).tapped(), Some(index >= count));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_path_to_exile_offers_the_exiled_creatures_controller_a_tapped_basic() {
    use spellwake_engine::effects::OptionalEffectChoice;
    let price = [(ManaType::White, 1)];
    let initial = game(
        vec![A, B, C],
        vec![
            hand("Path to Exile"),
            seed(
                2,
                C,
                ObjectDestination::Battlefield { controller: B },
                Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            ),
            named("Forest", 3, B, ObjectDestination::LibraryBottom),
            named("Island", 4, C, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let action = GameCommand::CastWithResources {
        spell: physical(&initial, 1).id(),
        target: Some(TargetRef::Object(physical(&initial, 2).id())),
        payment: CastingPayment {
            mana: Some(spend(&price)),
            ..Default::default()
        },
    };
    let offer = round(command(&initial, A, action).into_state()).into_state();
    assert_eq!(physical(&offer, 2).zone(), ObjectZone::Exile);
    assert_eq!(offer.pending_decision().unwrap().chooser(), B);
    let declined = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(physical(declined.state(), 3).zone(), ObjectZone::Library(B));
    let search = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let search = search
        .supply_entropy(search.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let RuleFrame::LibraryTop(frame) = search.pending_decision().unwrap().frame() else {
        panic!("basic search");
    };
    assert_eq!(frame.eligible_results(), [physical(&search, 3).id()]);
    let selected = reply(
        &search,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&search, 3).id()),
        }),
    );
    let land = physical(selected.state(), 3);
    assert_eq!(
        (land.zone(), land.controller(), land.tapped()),
        (ObjectZone::Battlefield, Some(B), Some(true))
    );
    assert_eq!(physical(selected.state(), 4).zone(), ObjectZone::Library(C));
    assert_eq!(selected.shuffle_journal().len(), 1);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_reality_shift_manifests_the_controllers_top_and_only_a_creature_can_turn_up() {
    use spellwake_engine::face_down::FaceDownKind;
    let price = [(ManaType::Colourless, 1), (ManaType::Blue, 1)];
    for is_creature in [false, true] {
        let top = if is_creature {
            Program::creature(Some(ManaCost::free()), 4.into(), 3.into(), BTreeSet::new())
        } else {
            Program::artifact(Some(ManaCost::free()), vec![])
        };
        let initial = game(
            vec![A, B, C],
            vec![
                hand("Reality Shift"),
                seed(
                    2,
                    C,
                    ObjectDestination::Battlefield { controller: B },
                    Program::creature(None, 2.into(), 1.into(), BTreeSet::new()),
                ),
                seed(3, B, ObjectDestination::LibraryBottom, top),
                named("Forest", 4, C, ObjectDestination::LibraryBottom),
            ],
            &price,
            Step::PrecombatMain,
        );
        let stack = command(
            &initial,
            A,
            GameCommand::CastWithResources {
                spell: physical(&initial, 1).id(),
                target: Some(TargetRef::Object(physical(&initial, 2).id())),
                payment: CastingPayment {
                    mana: Some(spend(&price)),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let manifested = round(stack).into_state();
        assert_eq!(physical(&manifested, 2).zone(), ObjectZone::Exile);
        let card = physical(&manifested, 3);
        assert_eq!(
            (card.zone(), card.controller(), card.face_down()),
            (
                ObjectZone::Battlefield,
                Some(B),
                Some(FaceDownKind::Manifest)
            )
        );
        let view = manifested
            .execution()
            .current_characteristics(card.id())
            .unwrap()
            .unwrap();
        assert!(view.current_names.is_empty());
        assert_eq!(
            view.power_toughness.unwrap().derived,
            PowerToughness {
                power: 2.into(),
                toughness: 2.into()
            }
        );
        assert_eq!(physical(&manifested, 4).zone(), ObjectZone::Library(C));
        let actor = pass(&manifested).into_state();
        let turn = GameCommand::TurnFaceUp {
            permanent: card.id(),
            ability: 0,
            payment: None,
        };
        if is_creature {
            let up = command(&actor, B, turn);
            assert_eq!(physical(up.state(), 3).face_down(), None);
            assert_eq!(
                up.state()
                    .execution()
                    .power_toughness(card.id())
                    .unwrap()
                    .unwrap()
                    .derived
                    .power,
                4.into()
            );
        } else {
            illegal(&actor, B, turn);
        }
    }
}
