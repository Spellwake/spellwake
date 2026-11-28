//! Named casting, cycling and channel options, including triggered rewards.
use super::*;
use spellwake_engine::{activated::SourceCost, effects::OptionalEffectChoice, land::BasicLandType};

fn cycle_card(s: &GameState, n: u64, price: &[(ManaType, i64)]) -> GameCommand {
    let source = physical(s, n).id();
    let ability = s
        .execution()
        .copiable_program(source)
        .unwrap()
        .activated
        .iter()
        .find(|ability| {
            ability.source_cost
                == SourceCost::HandDiscardSelf(
                    spellwake_engine::activated::HandDiscardKeyword::Cycling,
                )
        })
        .expect("printed cycling ability")
        .id;
    GameCommand::Cycle {
        source,
        ability,
        payment: Some(spend(price)),
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_ash_barrens_and_twisted_abomination_typecycle_for_their_printed_land_filters() {
    use ManaType::*;
    for (name, price, choice) in [("Ash Barrens", 1, 3), ("Twisted Abomination", 2, 4)] {
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                named("Forest", 3, A, ObjectDestination::LibraryBottom),
                named("Swamp", 4, A, ObjectDestination::LibraryBottom),
                named("Sol Ring", 5, A, ObjectDestination::LibraryBottom),
            ],
            &[(Colourless, price)],
            Step::PrecombatMain,
        );
        let activated =
            command(&initial, A, cycle_card(&initial, 1, &[(Colourless, price)])).into_state();
        assert_eq!(physical(&activated, 1).zone(), ObjectZone::Graveyard(A));
        let choice_frame = round(activated).into_state();
        let ready = choice_frame
            .supply_entropy(choice_frame.basis(), vec![0; 8])
            .unwrap()
            .into_state();
        let RuleFrame::LibraryTop(frame) = ready.pending_decision().unwrap().frame() else {
            panic!("expected land search");
        };
        let expected = if name == "Ash Barrens" {
            vec![3, 4]
        } else {
            vec![4]
        };
        assert_eq!(
            frame
                .eligible_results()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            expected
                .into_iter()
                .map(|n| physical(&ready, n).id())
                .collect()
        );
        let none = reply(
            &ready,
            DecisionReply::LibraryTop(LibraryTopReply::Search { selected: None }),
        );
        assert_eq!(none.shuffle_journal().len(), 1);
        let result = reply(
            &ready,
            DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: Some(physical(&ready, choice).id()),
            }),
        );
        assert_eq!(physical(result.state(), choice).zone(), ObjectZone::Hand(A));
        assert!(result.draw_journal().events.is_empty());
        assert_eq!(result.shuffle_journal().len(), 1);
        if name == "Ash Barrens" {
            let land = command(
                &initial,
                A,
                GameCommand::PlayLand {
                    land: physical(&initial, 1).id(),
                },
            )
            .into_state();
            let tap = command(
                &land,
                A,
                GameCommand::ActivateMana {
                    source: physical(&land, 1).id(),
                    ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
                },
            );
            assert_eq!(
                tap.state().execution().mana(A, Colourless),
                (price + 1).into()
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_twisted_abomination_can_regenerate_once_before_a_second_destruction() {
    use ManaType::*;
    let price = [(Colourless, 5), (Black, 1)];
    let mut cards = vec![hand("Twisted Abomination")];
    for n in [2, 3] {
        cards.push(seed(
            n,
            A,
            ObjectDestination::Hand,
            free_instant(
                BodyTarget::SINGLE_CREATURE,
                crate::keyword_creatures::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    ),
                ),
            ),
        ));
    }
    let state = game(
        vec![A, B],
        cards,
        &[(Colourless, 5), (Black, 2)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&state, &price);
    let id = physical(&entered, 1).id();
    let shield = command(
        &entered,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: id,
            ability: ActivatedAbilityId(1),
            payment: Some(spend(&[(Black, 1)])),
        },
    );
    let shield = round(shield.into_state()).into_state();
    let first = round(targeted(&shield, A, 2, TargetRef::Object(id)).into_state()).into_state();
    assert_eq!(
        (physical(&first, 1).id(), physical(&first, 1).tapped()),
        (id, Some(true))
    );
    let second = round(targeted(&first, A, 3, TargetRef::Object(id)).into_state());
    assert_eq!(physical(second.state(), 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_krosan_tusker_searches_before_its_cycling_draw_and_can_be_cast_as_a_creature() {
    use ManaType::*;
    let initial = game(
        vec![A, B],
        vec![
            hand("Krosan Tusker"),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
        ],
        &[(Colourless, 5), (Green, 2)],
        Step::PrecombatMain,
    );
    let hardcast = cast_named(&initial, &[(Colourless, 5), (Green, 2)]);
    assert_eq!(
        hardcast
            .execution()
            .power_toughness(physical(&hardcast, 1).id())
            .unwrap()
            .unwrap()
            .derived,
        PowerToughness {
            power: 6.into(),
            toughness: 5.into()
        }
    );
    let stack = command(
        &initial,
        A,
        cycle_card(&initial, 1, &[(Colourless, 2), (Green, 1)]),
    )
    .into_state();
    assert_eq!(physical(&stack, 1).zone(), ObjectZone::Graveyard(A));
    let optional = round(stack).into_state();
    let no = reply(
        &optional,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    let normal = round(no);
    assert_eq!(physical(normal.state(), 2).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(normal.state(), 100).zone(), ObjectZone::Library(A));
    let search = reply(
        &optional,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let ready = search
        .supply_entropy(search.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let found = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&ready, 2).id()),
        }),
    );
    assert!(found.draw_journal().events.is_empty());
    assert_eq!(physical(found.state(), 2).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(found.state(), 100).zone(), ObjectZone::Library(A));
    let drawn = round(found.into_state());
    assert_eq!(physical(drawn.state(), 100).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_greater_tanuki_channels_for_a_tapped_basic_or_casts_and_tramples() {
    use ManaType::*;
    use spellwake_engine::combat::CombatDamageAssignment;
    let spell = [(Colourless, 4), (Green, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Greater Tanuki"),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
            named(
                "Fervor",
                3,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(4, B, 1, 1, &[]),
        ],
        &spell,
        Step::PrecombatMain,
    );
    let channel = command(
        &initial,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: physical(&initial, 1).id(),
            ability: ActivatedAbilityId(0),
            payment: Some(spend(&[(Colourless, 2), (Green, 1)])),
        },
    )
    .into_state();
    assert_eq!(physical(&channel, 1).zone(), ObjectZone::Graveyard(A));
    let choice = round(channel).into_state();
    let ready = choice
        .supply_entropy(choice.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let found = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&ready, 2).id()),
        }),
    );
    assert_eq!(
        (
            physical(found.state(), 2).zone(),
            physical(found.state(), 2).tapped()
        ),
        (ObjectZone::Battlefield, Some(true))
    );
    assert_eq!(found.shuffle_journal().len(), 1);
    let entered = cast_named(&initial, &spell);
    let blocks = blockers(&attack(&entered, 1, B));
    let distribution = round(reply(&blocks, block_choice(&blocks, 4, 1)).into_state()).into_state();
    let source = physical(&distribution, 1).id();
    let result = reply(
        &distribution,
        DecisionReply::AssignCombatDamage(vec![
            CombatDamageAssignment {
                source,
                target: DamageRecipient::Creature(physical(&distribution, 4).id()),
                amount: 1.into(),
            },
            CombatDamageAssignment {
                source,
                target: DamageRecipient::Player(B),
                amount: 5.into(),
            },
        ]),
    );
    assert_eq!(result.state().execution().life_total(B), Some(&15.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_windcaller_aven_flies_and_cycling_grants_flying_before_the_draw_until_cleanup() {
    use ManaType::*;
    let price = [(Colourless, 4), (Blue, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Windcaller Aven"),
            creature(2, B, 1, 1, &[]),
            creature(3, B, 1, 1, &[Reach]),
            creature(4, A, 2, 2, &[]),
            named(
                "Fervor",
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    check_flying_blocks(&entered, 2, 3);
    let stacked = command(&initial, A, cycle_card(&initial, 1, &[(Blue, 1)])).into_state();
    let subject = physical(&stacked, 4).id();
    let stack = reply(
        &stacked,
        DecisionReply::TriggerTarget(TargetRef::Object(subject)),
    )
    .into_state();
    let flying = round(stack).into_state();
    let has_flying = |s: &GameState| {
        s.execution()
            .current_characteristics(subject)
            .unwrap()
            .unwrap()
            .keywords
            .contains(&Flying)
    };
    assert!(has_flying(&flying));
    assert_eq!(physical(&flying, 100).zone(), ObjectZone::Library(A));
    let drawn = round(flying).into_state();
    assert_eq!(physical(&drawn, 100).zone(), ObjectZone::Hand(A));
    let defend = blockers(&attack(&drawn, 4, B));
    illegal(
        &defend,
        B,
        combat_declaration(&defend, block_choice(&defend, 2, 4)),
    );
    let next = next_main(drawn, 2);
    assert!(!has_flying(&next));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_vizier_of_tumbling_sands_untaps_another_permanent_or_untaps_before_its_cycling_draw() {
    use ManaType::*;
    let spell = [(Colourless, 2), (Blue, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Vizier of Tumbling Sands"),
            named(
                "Fervor",
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
        ],
        &spell,
        Step::PrecombatMain,
    );
    let tapped = command(
        &initial,
        A,
        GameCommand::ActivateMana {
            source: physical(&initial, 3).id(),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    )
    .into_state();
    let entered = cast_named(&tapped, &spell);
    let source = physical(&entered, 1).id();
    let target = physical(&entered, 3).id();
    let activate = |t| GameCommand::ActivatePermanentTargets {
        source,
        ability: ActivatedAbilityId(1),
        targets: vec![TargetRef::Object(t)],
        payment: None,
    };
    illegal(&entered, A, activate(source));
    let activated = command(&entered, A, activate(target)).into_state();
    assert_eq!(physical(&activated, 1).tapped(), Some(true));
    let done = round(activated);
    assert_eq!(physical(done.state(), 3).tapped(), Some(false));
    let cycled = command(
        &tapped,
        A,
        cycle_card(&tapped, 1, &[(Colourless, 1), (Blue, 1)]),
    )
    .into_state();
    let target = TargetRef::Object(physical(&cycled, 3).id());
    let trigger = reply(
        &cycled,
        DecisionReply::TriggerPermanentTargets(vec![target]),
    )
    .into_state();
    let untapped = round(trigger).into_state();
    assert_eq!(physical(&untapped, 3).tapped(), Some(false));
    assert_eq!(physical(&untapped, 100).zone(), ObjectZone::Library(A));
    let drawn = round(untapped);
    assert_eq!(physical(drawn.state(), 100).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_cast_out_flashes_in_and_releases_the_opponents_nonland_or_can_cycle_for_one_white() {
    use ManaType::*;
    let price = [(Colourless, 3), (White, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Cast Out"),
            named(
                "Sol Ring",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            named(
                "Sol Ring",
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
                free_instant(
                    BodyTarget::SINGLE_PERMANENT,
                    crate::keyword_creatures::toolbox_emit::zone_op(
                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            spellwake_engine::toolbox::ZoneDest::Graveyard {
                                semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                    allow_regenerate: true,
                                },
                            },
                        ),
                    ),
                ),
            ),
        ],
        &price,
        Step::BeginningCombat,
    );
    let entered = cast_named(&initial, &price);
    let choose = |n| DecisionReply::TriggerTarget(TargetRef::Object(physical(&entered, n).id()));
    for n in [3, 4] {
        illegal(&entered, A, combat_declaration(&entered, choose(n)));
    }
    let exiled = round(reply(&entered, choose(2)).into_state()).into_state();
    assert_eq!(physical(&exiled, 2).zone(), ObjectZone::Exile);
    let returned =
        round(targeted(&exiled, A, 5, TargetRef::Object(physical(&exiled, 1).id())).into_state())
            .into_state();
    assert_eq!(physical(&returned, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(
        (
            physical(&returned, 2).zone(),
            physical(&returned, 2).controller()
        ),
        (ObjectZone::Battlefield, Some(B))
    );
    let cycling = command(&initial, A, cycle_card(&initial, 1, &[(White, 1)])).into_state();
    assert_eq!(physical(&cycling, 1).zone(), ObjectZone::Graveyard(A));
    let drawn = round(cycling).into_state();
    assert_eq!(physical(&drawn, 100).zone(), ObjectZone::Hand(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_slice_and_dice_damages_every_creature_for_four_or_may_deal_one_before_its_cycling_draw()
 {
    use ManaType::*;
    let price = [(Colourless, 4), (Red, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Slice and Dice"),
            creature(2, A, 1, 3, &[]),
            creature(3, B, 1, 4, &[]),
            creature(4, B, 1, 5, &[]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let spell = cast_named(&initial, &price);
    for (n, owner) in [(2, A), (3, B)] {
        assert_eq!(physical(&spell, n).zone(), ObjectZone::Graveyard(owner));
    }
    assert_eq!(
        spell
            .execution()
            .battlefield_marked_damage(physical(&spell, 4).id()),
        Some(4.into())
    );
    let cycled = command(
        &initial,
        A,
        cycle_card(&initial, 1, &[(Colourless, 2), (Red, 1)]),
    )
    .into_state();
    let offer = round(cycled).into_state();
    for (choice, damage) in [
        (OptionalEffectChoice::Take, 1_i64),
        (OptionalEffectChoice::Decline, 0),
    ] {
        let outcome = reply(&offer, DecisionReply::OptionalEffect(choice)).into_state();
        assert_eq!(physical(&outcome, 100).zone(), ObjectZone::Library(A));
        for n in [2, 3, 4] {
            assert_eq!(
                outcome
                    .execution()
                    .battlefield_marked_damage(physical(&outcome, n).id()),
                Some(damage.into())
            );
        }
        let drawn = round(outcome).into_state();
        assert_eq!(physical(&drawn, 100).zone(), ObjectZone::Hand(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_decree_of_pain_counts_destroyed_creatures_and_cycling_shrinks_even_indestructible_creatures()
 {
    use ManaType::*;
    use spellwake_engine::activated::ActivatedAbility;
    let price = [(Colourless, 6), (Black, 2)];
    let mut fragile = creature(2, A, 1, 2, &[]);
    fragile.program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![crate::keyword_creatures::toolbox_emit::regenerate_source()],
        ),
    });
    let initial = game(
        vec![A, B],
        vec![
            hand("Decree of Pain"),
            fragile,
            creature(3, B, 2, 3, &[]),
            creature(4, B, 1, 2, &[Indestructible]),
            named("Island", 5, A, ObjectDestination::LibraryBottom),
            named("Forest", 6, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let protected = round(
        command(
            &initial,
            A,
            GameCommand::ActivateUntargetedAbility {
                source: physical(&initial, 2).id(),
                ability: ActivatedAbilityId(0),
                payment: None,
            },
        )
        .into_state(),
    )
    .into_state();
    let destroyed = cast_named(&protected, &price);
    for (n, owner) in [(2, A), (3, B)] {
        assert_eq!(physical(&destroyed, n).zone(), ObjectZone::Graveyard(owner));
    }
    assert_eq!(physical(&destroyed, 4).zone(), ObjectZone::Battlefield);
    for n in [5, 6] {
        assert_eq!(physical(&destroyed, n).zone(), ObjectZone::Hand(A));
    }
    assert_eq!(physical(&destroyed, 100).zone(), ObjectZone::Library(A));
    let cycled = command(
        &initial,
        A,
        cycle_card(&initial, 1, &[(Colourless, 3), (Black, 2)]),
    )
    .into_state();
    let shrunk = round(cycled).into_state();
    for (n, owner) in [(2, A), (4, B)] {
        assert_eq!(physical(&shrunk, n).zone(), ObjectZone::Graveyard(owner));
    }
    let size = |s: &GameState| {
        s.execution()
            .power_toughness(physical(s, 3).id())
            .unwrap()
            .unwrap()
            .derived
    };
    assert_eq!(
        size(&shrunk),
        PowerToughness {
            power: 0.into(),
            toughness: 1.into()
        }
    );
    assert_eq!(physical(&shrunk, 5).zone(), ObjectZone::Library(A));
    let drawn = round(shrunk).into_state();
    assert_eq!(physical(&drawn, 5).zone(), ObjectZone::Hand(A));
    assert_eq!(
        size(&next_main(drawn, 2)),
        PowerToughness {
            power: 2.into(),
            toughness: 3.into()
        }
    );
}
