//! Cast permissions granted while looking at or exiling another card.
use super::*;

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_melek_copies_a_library_spell_and_changes_the_copy_target() {
    use spellwake_engine::effects::CopyTargetChoice;
    let price = [
        (ManaType::Colourless, 4),
        (ManaType::Blue, 1),
        (ManaType::Red, 1),
    ];
    let initial = game(
        vec![A, B],
        vec![
            hand("Melek, Izzet Paragon"),
            seed(
                2,
                A,
                ObjectDestination::LibraryBottom,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
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
            creature(3, B, 1, 1, &[]),
            creature(4, B, 2, 2, &[]),
            seed(
                5,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_CREATURE,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
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
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let cut = entered.execution().evaluated_characteristics().unwrap();
    assert!(entered.execution().library_top_access(&cut, A, B).visible);
    let hand_cast = targeted(
        &entered,
        A,
        5,
        TargetRef::Object(physical(&entered, 3).id()),
    );
    assert_eq!(hand_cast.state().execution().window().entries().len(), 1);
    let from_library = targeted(
        &entered,
        A,
        2,
        TargetRef::Object(physical(&entered, 3).id()),
    )
    .into_state();
    assert_eq!(from_library.execution().window().entries().len(), 2);
    let offer = round(from_library).into_state();
    let copied = reply(
        &offer,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Object(
            physical(&offer, 4).id(),
        ))),
    )
    .into_state();
    let first = round(copied).into_state();
    assert_eq!(physical(&first, 4).zone(), ObjectZone::Graveyard(B));
    assert_eq!(physical(&first, 3).zone(), ObjectZone::Battlefield);
    let done = round(first).into_state();
    assert_eq!(physical(&done, 3).zone(), ObjectZone::Graveyard(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_yennett_draws_on_decline_or_even_value_and_casts_an_odd_spell_free() {
    let price = [
        (ManaType::Colourless, 2),
        (ManaType::White, 1),
        (ManaType::Blue, 1),
        (ManaType::Black, 1),
    ];
    for (name, odd) in [("Sol Ring", true), ("Arcane Signet", false)] {
        let mut cards = vec![
            hand("Yennett, Cryptic Sovereign"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named(name, 3, A, ObjectDestination::LibraryBottom),
        ];
        cards.push(creature(4, B, 2, 2, &[]));
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let entered = cast_named(&initial, &price);
        let declared = attack(&entered, 1, B);
        assert_eq!(physical(&declared, 1).tapped(), Some(false));
        let outcome = round(declared).into_state();
        if odd {
            let draw = reply(
                &outcome,
                DecisionReply::OptionalEffect(
                    spellwake_engine::effects::OptionalEffectChoice::Decline,
                ),
            );
            assert_eq!(physical(draw.state(), 3).zone(), ObjectZone::Hand(A));
            let cast = reply(
                &outcome,
                DecisionReply::ImpulseCast {
                    target: None,
                    payment: CastingPayment::default(),
                    mana_abilities: vec![],
                },
            );
            assert_eq!(physical(cast.state(), 3).zone(), ObjectZone::Stack);
            assert_eq!(
                physical(round(cast.into_state()).state(), 3).zone(),
                ObjectZone::Battlefield
            );
        } else {
            assert_eq!(physical(&outcome, 3).zone(), ObjectZone::Hand(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_kaho_searches_for_instants_and_casts_only_its_linked_card_at_x() {
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 2)];
    let instant = |value: i64| {
        Program::instant(
            ManaCost::generic(value.into()),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller,
                        ),
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                ],
            ),
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Kaho, Minamo Historian"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            seed(3, A, ObjectDestination::LibraryBottom, instant(1)),
            seed(4, A, ObjectDestination::LibraryBottom, instant(2)),
            seed(5, A, ObjectDestination::Exile, instant(1)),
            named("Sol Ring", 6, A, ObjectDestination::LibraryBottom),
        ],
        &[(ManaType::Colourless, 3), (ManaType::Blue, 2)],
        Step::PrecombatMain,
    );
    let search = round(cast_named(&initial, &price)).into_state();
    let ready = search
        .supply_entropy(search.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let select = |state: &GameState, ids: &[u64]| {
        DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
            selected: ids.iter().map(|&n| physical(state, n).id()).collect(),
            colors: vec![],
        })
    };
    let empty = reply(&ready, select(&ready, &[]));
    assert_eq!(physical(empty.state(), 3).zone(), ObjectZone::Library(A));
    assert_eq!(empty.shuffle_journal().len(), 1);
    illegal(&ready, A, combat_declaration(&ready, select(&ready, &[6])));
    let found = reply(&ready, select(&ready, &[3, 4])).into_state();
    assert_eq!(physical(&found, 3).zone(), ObjectZone::Exile);
    assert_eq!(physical(&found, 4).zone(), ObjectZone::Exile);
    let activated = command(
        &found,
        A,
        GameCommand::ActivateWithRestrictedMana {
            source: physical(&found, 1).id(),
            ability: ActivatedAbilityId(0),
            granting_effect: None,
            origin: spellwake_engine::activated::ActivationOrigin::Untargeted,
            chosen_x: Some(1.into()),
            payment: Some(spend(&[(ManaType::Colourless, 1)])),
            restricted_mana: vec![],
        },
    );
    assert_eq!(physical(activated.state(), 1).tapped(), Some(true));
    let selection = round(activated.into_state()).into_state();
    let RuleFrame::ResolutionSelect(frame) = selection.pending_decision().unwrap().frame() else {
        panic!("expected matching linked instant")
    };
    assert_eq!(frame.domain(), [physical(&selection, 3).id()]);
    let skipped = reply(&selection, DecisionReply::SelectResolution(vec![]));
    assert_eq!(physical(skipped.state(), 3).zone(), ObjectZone::Exile);
    let offer = reply(
        &selection,
        DecisionReply::SelectResolution(vec![physical(&selection, 3).id()]),
    )
    .into_state();
    let cast = reply(
        &offer,
        DecisionReply::ImpulseCast {
            target: None,
            payment: CastingPayment::default(),
            mana_abilities: vec![],
        },
    );
    let resolved = round(cast.into_state()).into_state();
    assert_eq!(physical(&resolved, 3).zone(), ObjectZone::Graveyard(A));
    assert_eq!(resolved.execution().life_total(A), Some(&21.into()));
    for n in [4, 5] {
        assert_eq!(physical(&resolved, n).zone(), ObjectZone::Exile);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_cunning_rhetoric_lets_the_defender_play_an_exiled_land_or_spend_any_color() {
    use spellwake_engine::land::BasicLandType;
    let price = [(ManaType::Colourless, 2), (ManaType::Black, 1)];
    for land in [false, true] {
        let stolen = if land {
            named("Island", 4, B, ObjectDestination::LibraryBottom)
        } else {
            seed(
                4,
                B,
                ObjectDestination::LibraryBottom,
                Program::creature(
                    Some(ManaCost::one(ManaType::White)),
                    1.into(),
                    1.into(),
                    BTreeSet::new(),
                ),
            )
        };
        let initial = game(
            vec![A, B],
            vec![
                hand("Cunning Rhetoric"),
                creature(2, B, 1, 1, &[]),
                named("Mountain", 3, B, ObjectDestination::LibraryBottom),
                stolen,
                named(
                    "Forest",
                    5,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                ),
            ],
            &price,
            Step::PrecombatMain,
        );
        let defending = next_main(cast_named(&initial, &price), 2);
        assert_eq!(physical(&defending, 3).zone(), ObjectZone::Hand(B));
        let attack_declared = attack(&defending, 2, A);
        let exiled = round(attack_declared).into_state();
        assert_eq!(physical(&exiled, 4).zone(), ObjectZone::Exile);
        let main = next_main(exiled, 3);
        let played = if land {
            command(
                &main,
                A,
                GameCommand::PlayLand {
                    land: physical(&main, 4).id(),
                },
            )
            .into_state()
        } else {
            let mana = command(
                &main,
                A,
                GameCommand::ActivateMana {
                    source: physical(&main, 5).id(),
                    ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
                },
            )
            .into_state();
            round(cast_untargeted(&mana, 4, &[(ManaType::Green, 1)]).into_state()).into_state()
        };
        let card = physical(&played, 4);
        assert_eq!(
            (card.zone(), card.owner(), card.controller()),
            (ObjectZone::Battlefield, B, Some(A))
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_torrential_gearhulk_flashes_in_casts_a_graveyard_instant_and_exiles_it() {
    let price = [(ManaType::Colourless, 4), (ManaType::Blue, 2)];
    let gain = |n: i64| {
        crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
            spellwake_engine::toolbox::PlayerSelector::Resolving(
                spellwake_engine::program::ResolvingPlayer::Controller,
            ),
            spellwake_engine::program::ValueExpr::Literal(n.into()),
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Torrential Gearhulk"),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::None, gain(1)),
            ),
            seed(
                3,
                A,
                ObjectDestination::Graveyard,
                Program::instant(
                    ManaCost::generic(4.into()),
                    ExecutableBody::new(BodyTarget::None, vec![gain(3)]),
                ),
            ),
            seed(
                4,
                B,
                ObjectDestination::Graveyard,
                free_instant(BodyTarget::None, gain(1)),
            ),
            named("Sol Ring", 5, A, ObjectDestination::Graveyard),
        ],
        &price,
        Step::PrecombatMain,
    );
    let original = cast_untargeted(&initial, 2, &[]).into_state();
    let entered = cast_named(&original, &price);
    let choose = |n| DecisionReply::TriggerTarget(TargetRef::Object(physical(&entered, n).id()));
    for n in [4, 5] {
        illegal(&entered, A, combat_declaration(&entered, choose(n)));
    }
    let declared = reply(&entered, choose(3)).into_state();
    let offer = round(declared).into_state();
    let declined = reply(
        &offer,
        DecisionReply::OptionalEffect(spellwake_engine::effects::OptionalEffectChoice::Decline),
    );
    assert_eq!(
        physical(declined.state(), 3).zone(),
        ObjectZone::Graveyard(A)
    );
    let cast = reply(
        &offer,
        DecisionReply::ImpulseCast {
            target: None,
            payment: CastingPayment::default(),
            mana_abilities: vec![],
        },
    );
    assert_eq!(physical(cast.state(), 3).zone(), ObjectZone::Stack);
    let resolved = round(cast.into_state()).into_state();
    assert_eq!(physical(&resolved, 3).zone(), ObjectZone::Exile);
    assert_eq!(resolved.execution().life_total(A), Some(&23.into()));
    assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Stack);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_scaretiller_tap_offers_both_land_modes_and_enters_the_chosen_land_tapped() {
    use spellwake_engine::mana::CastMode;
    let price = [(ManaType::Colourless, 4)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Scaretiller"),
            named(
                "Fervor",
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            named("Swamp", 3, A, ObjectDestination::Hand),
            named("Forest", 4, A, ObjectDestination::Graveyard),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let choose = attack(&entered, 1, B);
    let mode = |index, target| {
        DecisionReply::TriggerModes(vec![CastMode {
            mode: index,
            target,
            targets: vec![],
        }])
    };
    let hand_mode = reply(&choose, mode(0, None)).into_state();
    let hand_offer = round(hand_mode).into_state();
    let declined = reply(&hand_offer, DecisionReply::SelectResolution(vec![]));
    assert_eq!(physical(declined.state(), 3).zone(), ObjectZone::Hand(A));
    let accepted = reply(
        &hand_offer,
        DecisionReply::SelectResolution(vec![physical(&hand_offer, 3).id()]),
    );
    let from_hand = physical(accepted.state(), 3);
    assert_eq!(
        (from_hand.zone(), from_hand.tapped()),
        (ObjectZone::Battlefield, Some(true))
    );
    let grave_mode = reply(
        &choose,
        mode(1, Some(TargetRef::Object(physical(&choose, 4).id()))),
    )
    .into_state();
    let returned = round(grave_mode);
    let from_graveyard = physical(returned.state(), 4);
    assert_eq!(
        (from_graveyard.zone(), from_graveyard.tapped()),
        (ObjectZone::Battlefield, Some(true))
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_taigam_skips_draw_selects_the_upkeep_hand_card_and_exiles_the_rest_to_shrink() {
    use spellwake_engine::activated::ActivationOrigin;
    use spellwake_engine::land::BasicLandType;
    let price = [
        (ManaType::Colourless, 3),
        (ManaType::Blue, 1),
        (ManaType::Black, 1),
    ];
    let initial = game(
        vec![A, B],
        vec![
            hand("Taigam, Sidisi's Hand"),
            named("Island", 2, A, ObjectDestination::LibraryBottom),
            named("Mountain", 3, A, ObjectDestination::LibraryBottom),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
            named(
                "Swamp",
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
            creature(6, B, 3, 3, &[]),
            named("Plains", 7, B, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let upkeep = exile_hand::until(entered, |s| {
        s.turn().unwrap().position().number == 3
            && s.turn().unwrap().position().step == Step::Upkeep
    });
    let select = round(upkeep).into_state();
    let order = reply(
        &select,
        DecisionReply::SelectResolution(vec![physical(&select, 3).id()]),
    )
    .into_state();
    let resolved = reply(
        &order,
        DecisionReply::OrderResolutionGraveyard(vec![
            physical(&order, 4).id(),
            physical(&order, 2).id(),
        ]),
    )
    .into_state();
    assert_eq!(physical(&resolved, 3).zone(), ObjectZone::Hand(A));
    for n in [2, 4] {
        assert_eq!(physical(&resolved, n).zone(), ObjectZone::Graveyard(A));
    }
    let main = next_main(resolved, 3);
    assert_eq!(physical(&main, 100).zone(), ObjectZone::Library(A));
    let mana = command(
        &main,
        A,
        GameCommand::ActivateMana {
            source: physical(&main, 5).id(),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Swamp),
        },
    )
    .into_state();
    let pay = |objects| GameCommand::ActivateWithRestrictedMana {
        source: physical(&mana, 1).id(),
        ability: ActivatedAbilityId(0),
        granting_effect: None,
        origin: ActivationOrigin::SelectedCost {
            target: Some(TargetRef::Object(physical(&mana, 6).id())),
            objects,
        },
        chosen_x: None,
        payment: Some(spend(&[(ManaType::Black, 1)])),
        restricted_mana: vec![],
    };
    illegal(&mana, A, pay(vec![physical(&mana, 3).id()]));
    let activated = command(
        &mana,
        A,
        pay(vec![physical(&mana, 2).id(), physical(&mana, 4).id()]),
    )
    .into_state();
    assert_eq!(physical(&activated, 1).tapped(), Some(true));
    for n in [2, 4] {
        assert_eq!(physical(&activated, n).zone(), ObjectZone::Exile);
    }
    let reduced = round(activated).into_state();
    let pt = |s: &GameState| {
        s.execution()
            .power_toughness(physical(s, 6).id())
            .unwrap()
            .unwrap()
            .derived
    };
    assert_eq!(
        pt(&reduced),
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    let expired = next_main(reduced, 4);
    assert_eq!(
        pt(&expired),
        PowerToughness {
            power: 3.into(),
            toughness: 3.into()
        }
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_aminatous_augury_spends_each_exiled_type_once_and_the_permission_expires() {
    let price = [(ManaType::Colourless, 6), (ManaType::Blue, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Aminatou's Augury"),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
            named(
                "Ornithopter of Paradise",
                3,
                A,
                ObjectDestination::LibraryBottom,
            ),
            named("Sol Ring", 4, A, ObjectDestination::LibraryBottom),
            named("Elvish Mystic", 5, A, ObjectDestination::LibraryBottom),
            named("Fervor", 6, A, ObjectDestination::LibraryBottom),
            named("Mountain", 7, A, ObjectDestination::LibraryBottom),
            named("Arcane Signet", 8, A, ObjectDestination::LibraryBottom),
            named("Island", 9, A, ObjectDestination::LibraryBottom),
            named("Swamp", 10, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let selection = cast_named(&initial, &price);
    for n in 2..=9 {
        assert_eq!(physical(&selection, n).zone(), ObjectZone::Exile);
    }
    assert_eq!(physical(&selection, 10).zone(), ObjectZone::Library(A));
    let declined = reply(&selection, DecisionReply::SelectResolution(vec![]));
    assert_eq!(physical(declined.state(), 2).zone(), ObjectZone::Exile);
    let main = reply(
        &selection,
        DecisionReply::SelectResolution(vec![physical(&selection, 2).id()]),
    )
    .into_state();
    assert_eq!(physical(&main, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&main, 7).zone(), ObjectZone::Exile);
    let cast = |state: &GameState, n, kind| GameCommand::CastWithResources {
        spell: physical(state, n).id(),
        target: None,
        payment: CastingPayment {
            exile_cast_type: Some(kind),
            ..Default::default()
        },
    };
    let first =
        round(command(&main, A, cast(&main, 3, CardType::Creature)).into_state()).into_state();
    illegal(&first, A, cast(&first, 5, CardType::Creature));
    let second =
        round(command(&first, A, cast(&first, 4, CardType::Artifact)).into_state()).into_state();
    illegal(&second, A, cast(&second, 8, CardType::Artifact));
    for n in [3, 4] {
        assert_eq!(physical(&second, n).zone(), ObjectZone::Battlefield);
    }
    assert_eq!(
        second
            .execution()
            .free_exile_types(physical(&second, 6).id(), A),
        [CardType::Enchantment]
    );
    let expired = next_main(second, 2);
    assert!(
        expired
            .execution()
            .free_exile_types(physical(&expired, 6).id(), A)
            .is_empty()
    );
}
