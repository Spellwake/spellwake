//! Source-backed Commander Masters cases for casting choices and multi-player zone changes.
use super::*;
use spellwake_engine::{
    effects::{CounterKind, OptionalEffectChoice},
    face_down::FaceDownKind,
    land::{BasicLandType, LandType},
    program::SpellSubtype,
};

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_beanstalk_casts_both_spells_and_counts_its_lands() {
    use ManaType::{Colourless, Green};
    let state = game(
        vec![A, B],
        vec![
            hand("Beanstalk Giant // Fertile Footsteps"),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
        ],
        &[(Colourless, 8), (Green, 2)],
        Step::PrecombatMain,
    );
    let adventure = |state: &GameState| GameCommand::CastWithResources {
        spell: physical(state, 1).id(),
        target: None,
        payment: CastingPayment {
            adventure: true,
            mana: Some(spend(&[(Colourless, 2), (Green, 1)])),
            ..Default::default()
        },
    };
    let stacked = command(&state, A, adventure(&state)).into_state();
    let actual = stacked
        .execution()
        .copiable_program(physical(&stacked, 1).id())
        .unwrap();
    assert!(actual.has_type(CardType::Sorcery));
    assert!(actual.spell_subtypes.contains(&SpellSubtype::Adventure));
    assert_eq!(actual.mana_value(), 3.into());
    let searching = round(stacked).into_state();
    let resolved = reply(
        &searching,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&searching, 2).id()),
        }),
    )
    .into_state();
    assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&resolved, 2).tapped(), Some(false));
    assert_eq!(physical(&resolved, 1).zone(), ObjectZone::Exile);
    assert!(
        resolved
            .execution()
            .may_play_from_exile(physical(&resolved, 1).id(), A)
    );
    illegal(&resolved, A, adventure(&resolved));
    let giant = round(cast_untargeted(&resolved, 1, &[(Colourless, 6), (Green, 1)]).into_state())
        .into_state();
    let view = giant
        .execution()
        .current_characteristics(physical(&giant, 1).id())
        .unwrap()
        .unwrap();
    assert!(view.is_creature());
    let pt = view.power_toughness.unwrap().derived;
    assert_eq!((pt.power, pt.toughness), (1.into(), 1.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_yedora_returns_stolen_creature_to_its_owner_as_a_forest() {
    let price = [(ManaType::Colourless, 4), (ManaType::Green, 1)];
    let state = game(
        vec![A, B],
        vec![
            hand("Yedora, Grave Gardener"),
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: A },
                Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            ),
            seed(
                3,
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
    let entered = cast_named(&state, &price);
    let removed = round(
        targeted(
            &entered,
            A,
            3,
            TargetRef::Object(physical(&entered, 2).id()),
        )
        .into_state(),
    )
    .into_state();
    assert_eq!(physical(&removed, 2).zone(), ObjectZone::Graveyard(B));
    let choice = round(removed).into_state();
    let declined = reply(
        &choice,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(physical(&declined, 2).zone(), ObjectZone::Graveyard(B));
    let returned = reply(
        &choice,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let forest = physical(&returned, 2);
    assert_eq!(forest.controller(), Some(B));
    assert_eq!(forest.face_down(), Some(FaceDownKind::Forest));
    let view = returned
        .execution()
        .current_characteristics(forest.id())
        .unwrap()
        .unwrap();
    assert_eq!(view.current_types, [CardType::Land].into());
    assert_eq!(view.current_land_subtypes, [LandType::Forest].into());
    assert!(view.current_names.is_empty());
    assert!(
        returned
            .execution()
            .manifest_face_up_cost(forest.id())
            .is_none()
    );
    let give_priority = pass(&returned).into_state();
    let tapped = command(
        &give_priority,
        B,
        GameCommand::ActivateMana {
            source: forest.id(),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    );
    assert_eq!(
        tapped.state().execution().mana(B, ManaType::Green),
        1.into()
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_desecrate_reality_checks_distinct_opponents_parity_and_paid_mana() {
    use ManaType::{Blue, Colourless};
    for colorless in [2, 3] {
        let price = [(Colourless, colorless), (Blue, 7 - colorless)];
        let artifact = |number, owner, zone, value| {
            seed(
                number,
                owner,
                zone,
                Program::artifact(Some(ManaCost::generic(value)), vec![]),
            )
        };
        let state = game(
            vec![A, B, C],
            vec![
                hand("Desecrate Reality"),
                artifact(
                    2,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    0.into(),
                ),
                artifact(
                    3,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    2.into(),
                ),
                artifact(
                    4,
                    C,
                    ObjectDestination::Battlefield { controller: C },
                    1.into(),
                ),
                artifact(5, A, ObjectDestination::Graveyard, 1.into()),
                artifact(6, A, ObjectDestination::Graveyard, 2.into()),
            ],
            &price,
            Step::PrecombatMain,
        );
        let spell = |ids: &[u64]| GameCommand::CastWithResources {
            spell: physical(&state, 1).id(),
            target: None,
            payment: CastingPayment {
                permanent_targets: ids
                    .iter()
                    .map(|&id| TargetRef::Object(physical(&state, id).id()))
                    .collect(),
                mana: Some(spend(&price)),
                ..Default::default()
            },
        };
        illegal(&state, A, spell(&[2, 3]));
        illegal(&state, A, spell(&[4]));
        let mut resolved = round(command(&state, A, spell(&[2])).into_state()).into_state();
        assert_eq!(physical(&resolved, 2).zone(), ObjectZone::Exile);
        if colorless == 3 {
            let RuleFrame::ResolutionSelect(frame) = resolved.pending_decision().unwrap().frame()
            else {
                panic!("expected graveyard choice")
            };
            assert_eq!(frame.domain(), &[physical(&resolved, 5).id()]);
            assert_eq!(frame.minimum(), 1);
            resolved = reply(
                &resolved,
                DecisionReply::SelectResolution(vec![physical(&resolved, 5).id()]),
            )
            .into_state();
            assert_eq!(physical(&resolved, 5).zone(), ObjectZone::Battlefield);
        } else {
            assert!(resolved.pending_decision().is_none());
            assert_eq!(physical(&resolved, 5).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_promise_of_loyalty_collects_choices_before_sacrifices_and_limits_attack() {
    let price = [(ManaType::Colourless, 4), (ManaType::White, 1)];
    let state = game(
        vec![A, B, C],
        vec![
            hand("Promise of Loyalty"),
            creature(2, A, 1, 2, &[]),
            creature(3, A, 1, 2, &[]),
            creature(4, B, 1, 2, &[]),
            creature(5, B, 1, 2, &[Indestructible]),
        ],
        &price,
        Step::PrecombatMain,
    );
    let first = round(cast_untargeted(&state, 1, &price).into_state()).into_state();
    let next = reply(
        &first,
        DecisionReply::SelectResolution(vec![physical(&first, 2).id()]),
    )
    .into_state();
    assert_eq!(next.pending_decision().unwrap().chooser(), B);
    assert_eq!(
        next.execution().prior_vow_choices(),
        [(A, physical(&next, 2).id())]
    );
    assert_eq!(
        next.execution()
            .counters_of_kind(physical(&next, 2).id(), CounterKind::Vow),
        0.into()
    );
    let finished = reply(
        &next,
        DecisionReply::SelectResolution(vec![physical(&next, 4).id()]),
    );
    let sacrifices: Vec<_> = events(&finished).map(|event| event.occurrences.iter().filter(|row|
        matches!(&row.value, RulesOccurrence::ZoneChange(move_) if matches!(move_.reason,ZoneReason::Sacrifice {..}))
    ).count()).filter(|count| *count > 0).collect();
    assert_eq!(sacrifices, [2]);
    let state = finished.into_state();
    assert_eq!(physical(&state, 3).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&state, 5).zone(), ObjectZone::Graveyard(B));
    for id in [2, 4] {
        assert_eq!(
            state
                .execution()
                .counters_of_kind(physical(&state, id).id(), CounterKind::Vow),
            1.into()
        );
    }
    let second_turn = next_main(state, 2);
    let combat = combat_frame(&second_turn, true);
    let RuleFrame::Combat(CombatDecision::Attackers(frame)) =
        combat.pending_decision().unwrap().frame()
    else {
        panic!("expected declaration")
    };
    let restrictions = &frame.forbidden_defenders()[&physical(&combat, 4).id()];
    assert!(restrictions.contains(&TargetRef::Player(A)));
    assert!(!restrictions.contains(&TargetRef::Player(C)));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_blink_spells_use_the_printed_controllers_and_return_time() {
    for delayed in [false, true] {
        let (name, price) = if delayed {
            (
                "Semester's End",
                [(ManaType::Colourless, 3), (ManaType::White, 1)],
            )
        } else {
            (
                "Ghostly Flicker",
                [(ManaType::Colourless, 2), (ManaType::Blue, 1)],
            )
        };
        let state = game(
            vec![A, B],
            vec![
                hand(name),
                creature(2, A, 2, 2, &[]),
                seed(
                    3,
                    B,
                    ObjectDestination::Battlefield { controller: A },
                    Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
                ),
            ],
            &price,
            Step::PrecombatMain,
        );
        let ids = [physical(&state, 2).id(), physical(&state, 3).id()];
        let cast = command(
            &state,
            A,
            GameCommand::CastWithResources {
                spell: physical(&state, 1).id(),
                target: None,
                payment: CastingPayment {
                    permanent_targets: ids.into_iter().map(TargetRef::Object).collect(),
                    mana: Some(spend(&price)),
                    ..Default::default()
                },
            },
        );
        let mut resolved = round(cast.into_state()).into_state();
        if delayed {
            for id in [2, 3] {
                assert_eq!(physical(&resolved, id).zone(), ObjectZone::Exile);
            }
            let end = exile_hand::until(resolved, |game| {
                game.turn().unwrap().position().step == Step::End
                    && !game.execution().window().entries().is_empty()
            });
            resolved = round(end).into_state();
        }
        if let Some(RuleFrame::LinkedReturnOrder(frame)) =
            resolved.pending_decision().map(|p| p.frame())
        {
            resolved = reply(
                &resolved,
                DecisionReply::LinkedReturnOrder(frame.objects().to_vec()),
            )
            .into_state();
        }
        for (index, id) in [2, 3].into_iter().enumerate() {
            let card = physical(&resolved, id);
            assert_ne!(card.id(), ids[index]);
            assert_eq!(
                card.controller(),
                Some(if delayed && id == 3 { B } else { A })
            );
            assert_eq!(
                resolved
                    .execution()
                    .counters_of_kind(card.id(), CounterKind::PlusOne),
                i64::from(delayed).into()
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_minds_aglow_accepts_each_players_mana_then_draws_the_combined_total() {
    use spellwake_engine::effects::ResolutionPaymentReply;
    let state = game(
        vec![A, B],
        vec![
            hand("Minds Aglow"),
            named(
                "Forest",
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
            ),
            seed(
                3,
                A,
                ObjectDestination::LibraryTop,
                Program::artifact(None, vec![]),
            ),
            seed(
                4,
                B,
                ObjectDestination::LibraryTop,
                Program::artifact(None, vec![]),
            ),
        ],
        &[(ManaType::Blue, 1), (ManaType::Colourless, 1)],
        Step::PrecombatMain,
    );
    let first = round(cast_untargeted(&state, 1, &[(ManaType::Blue, 1)]).into_state()).into_state();
    let skipped = reply(
        &first,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    )
    .into_state();
    let nobody = reply(
        &skipped,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
    );
    assert!(nobody.draw_journal().events.is_empty());
    assert_eq!(physical(nobody.state(), 3).zone(), ObjectZone::Library(A));
    let second = reply(
        &first,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[(
            ManaType::Colourless,
            1,
        )]))),
    )
    .into_state();
    assert_eq!(second.pending_decision().unwrap().chooser(), B);
    assert_eq!(physical(&second, 3).zone(), ObjectZone::Library(A));
    let floated = command(
        &second,
        B,
        GameCommand::ActivateMana {
            source: physical(&second, 2).id(),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    )
    .into_state();
    let result = reply(
        &floated,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[(
            ManaType::Green,
            1,
        )]))),
    )
    .into_state();
    for (seat, first, second) in [(A, 3, 100), (B, 4, 101)] {
        assert_eq!(physical(&result, first).zone(), ObjectZone::Hand(seat));
        assert_eq!(physical(&result, second).zone(), ObjectZone::Hand(seat));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_gorex_casts_with_a_graveyard_card_and_recovers_that_card_on_death() {
    let price = [(ManaType::Colourless, 4), (ManaType::Black, 2)];
    let state = game(
        vec![A, B],
        vec![
            hand("Gorex, the Tombshell"),
            seed(
                2,
                A,
                ObjectDestination::Graveyard,
                Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            ),
            seed(
                3,
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
            seed(
                4,
                A,
                ObjectDestination::Exile,
                Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &state,
        A,
        GameCommand::CastWithResources {
            spell: physical(&state, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&price)),
                objects: vec![CastResource::ExileCreatureForReduction(
                    physical(&state, 2).id(),
                )],
                ..Default::default()
            },
        },
    );
    assert_eq!(physical(cast.state(), 2).zone(), ObjectZone::Exile);
    let entered = round(cast.into_state()).into_state();
    let dead = round(
        targeted(
            &entered,
            A,
            3,
            TargetRef::Object(physical(&entered, 1).id()),
        )
        .into_state(),
    )
    .into_state();
    let returned = round(dead).into_state();
    assert_eq!(physical(&returned, 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(physical(&returned, 2).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(&returned, 4).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_isareth_binds_paid_x_to_a_reflexive_target_and_replaces_its_departure() {
    use spellwake_engine::effects::ResolutionPaymentReply;
    let price = [(ManaType::Colourless, 1), (ManaType::Black, 2)];
    let state = game(
        vec![A, B],
        vec![
            hand("Isareth the Awakener"),
            seed(
                2,
                A,
                ObjectDestination::Graveyard,
                Program::creature(
                    Some(ManaCost::generic(1.into())),
                    2.into(),
                    2.into(),
                    BTreeSet::new(),
                ),
            ),
            seed(
                3,
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
            named(
                "Forest",
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let main = next_main(cast_named(&state, &price), 3);
    let attacked = attack(&main, 1, B);
    let payment = round(attacked).into_state();
    let floated = command(
        &payment,
        A,
        GameCommand::ActivateMana {
            source: physical(&payment, 4).id(),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
        },
    )
    .into_state();
    let paid = reply(
        &floated,
        DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(spend(&[(
            ManaType::Green,
            1,
        )]))),
    )
    .into_state();
    let subject = physical(&paid, 2).id();
    let targeted = reply(
        &paid,
        DecisionReply::TriggerTarget(TargetRef::Object(subject)),
    )
    .into_state();
    assert_eq!(physical(&targeted, 2).zone(), ObjectZone::Graveyard(A));
    let returned = round(targeted).into_state();
    let body = physical(&returned, 2).id();
    assert_eq!(physical(&returned, 2).zone(), ObjectZone::Battlefield);
    assert_eq!(
        returned
            .execution()
            .counters_of_kind(body, CounterKind::Corpse),
        1.into()
    );
    let removed =
        round(super::targeted(&returned, A, 3, TargetRef::Object(body)).into_state()).into_state();
    assert_eq!(physical(&removed, 2).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_days_undoing_shuffles_each_players_hand_and_graveyard_and_ends_its_turn() {
    use spellwake_engine::draw::DrawEvent;
    let mut cards = vec![hand("Day's Undoing")];
    for (owner, base) in [(A, 10), (B, 30)] {
        for offset in 0..8 {
            let zone = match offset {
                0 => ObjectDestination::Graveyard,
                1 => ObjectDestination::Hand,
                _ => ObjectDestination::LibraryBottom,
            };
            cards.push(seed(
                base + offset,
                owner,
                zone,
                Program::artifact(None, vec![]),
            ));
        }
    }
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 1)];
    let state = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let stacked = cast_untargeted(&state, 1, &price).into_state();
    let ready = stacked
        .supply_entropy(stacked.basis(), vec![0; 32])
        .unwrap()
        .into_state();
    let resolution = round(ready);
    assert_eq!(
        resolution
            .shuffle_journal()
            .iter()
            .map(|shuffle| shuffle.player)
            .collect::<Vec<_>>(),
        [A, B]
    );
    for (owner, base) in [(A, 10), (B, 30)] {
        for card in [base, base + 1] {
            assert_ne!(
                physical(&state, card).id(),
                physical(resolution.state(), card).id()
            );
        }
        assert_eq!(
            resolution
                .draw_journal()
                .events
                .iter()
                .filter(|draw| matches!(draw, DrawEvent::Drawn { player, .. } if *player == owner))
                .count(),
            7
        );
    }
    assert_eq!(physical(resolution.state(), 1).zone(), ObjectZone::Exile);
    assert!(events(&resolution).any(|event| {
        event
            .occurrences
            .iter()
            .any(|occ| matches!(&occ.value, RulesOccurrence::StackExiledToEndTurn { .. }))
    }));
    let position = resolution.state().turn().unwrap().position();
    assert!(position.number > 1 || position.step == Step::Cleanup);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_abundance_replaces_a_draw_with_a_land_and_orders_the_other_cards() {
    use spellwake_engine::effects::{DrawReplacementChoice, DrawReplacementDomain};
    let price = [(ManaType::Colourless, 2), (ManaType::Green, 2)];
    let filler = |id| {
        seed(
            id,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        )
    };
    let state = game(
        vec![A, B],
        vec![
            hand("Abundance"),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::None,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_controller(
                        spellwake_engine::program::ValueExpr::Literal(1.into()),
                    ),
                ),
            ),
            filler(3),
            filler(4),
            named("Forest", 5, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let ready = cast_named(&state, &price);
    let choice = round(cast_untargeted(&ready, 2, &[]).into_state()).into_state();
    let plain = reply(
        &choice,
        DecisionReply::DrawReplacement(DrawReplacementChoice::Draw),
    );
    assert_eq!(physical(plain.state(), 3).zone(), ObjectZone::Hand(A));
    let nonland = reply(
        &choice,
        DecisionReply::DrawReplacement(DrawReplacementChoice::Nonland),
    );
    assert_eq!(physical(nonland.state(), 3).zone(), ObjectZone::Hand(A));
    assert!(nonland.draw_journal().events.is_empty());
    let reveal = reply(
        &choice,
        DecisionReply::DrawReplacement(DrawReplacementChoice::Land),
    )
    .into_state();
    let RuleFrame::DrawReplacement(frame) = reveal.pending_decision().unwrap().frame() else {
        panic!("expected bottom order");
    };
    let DrawReplacementDomain::Bottom(domain) = &frame.domain else {
        panic!("expected revealed nonlands");
    };
    assert_eq!(
        domain,
        &[physical(&choice, 3).id(), physical(&choice, 4).id()]
    );
    let bottom = vec![domain[1], domain[0]];
    let obtained = reply(
        &reveal,
        DecisionReply::DrawReplacement(DrawReplacementChoice::Bottom(bottom.clone())),
    );
    assert_eq!(physical(obtained.state(), 5).zone(), ObjectZone::Hand(A));
    assert!(obtained.draw_journal().events.is_empty());
    assert!(
        obtained
            .state()
            .execution()
            .window()
            .cards()
            .library(A)
            .ends_with(&bottom)
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_alms_collector_replaces_an_opponents_multiple_draw_once() {
    use spellwake_engine::draw::DrawEvent;
    let price = [(ManaType::Colourless, 3), (ManaType::White, 1)];
    let state = game(
        vec![A, B],
        vec![
            hand("Alms Collector"),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                free_instant(
                    BodyTarget::SINGLE_PLAYER,
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::draw_target_player(
                        spellwake_engine::program::ValueExpr::Literal(3.into()),
                    ),
                ),
            ),
            seed(
                3,
                B,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let drawing = targeted(&state, A, 2, TargetRef::Player(B)).into_state();
    let collector = cast_named(&drawing, &price);
    assert_eq!(physical(&collector, 1).zone(), ObjectZone::Battlefield);
    assert_eq!(physical(&collector, 2).zone(), ObjectZone::Stack);
    let result = round(collector);
    let drawn: Vec<_> = result
        .draw_journal()
        .events
        .iter()
        .map(|event| match event {
            DrawEvent::Drawn { player, .. } => *player,
            _ => panic!("unexpected empty draw"),
        })
        .collect();
    assert_eq!(drawn, [A, B]);
    assert_eq!(physical(result.state(), 100).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(result.state(), 3).zone(), ObjectZone::Hand(B));
    assert_eq!(physical(result.state(), 101).zone(), ObjectZone::Library(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_fact_or_fiction_and_animal_magnetism_bind_each_choice_to_its_player() {
    use spellwake_engine::effects::PileChoice;
    for (name, price, choosing_creature) in [
        (
            "Fact or Fiction",
            [(ManaType::Colourless, 3), (ManaType::Blue, 1)],
            false,
        ),
        (
            "Animal Magnetism",
            [(ManaType::Colourless, 4), (ManaType::Green, 1)],
            true,
        ),
    ] {
        let card = |id| {
            seed(
                id,
                A,
                ObjectDestination::LibraryBottom,
                Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            )
        };
        let initial = game(
            vec![A, B, C],
            vec![hand(name), card(2), card(3)],
            &price,
            Step::PrecombatMain,
        );
        let shown = round(cast_untargeted(&initial, 1, &price).into_state());
        assert_eq!(
            events(&shown)
                .flat_map(|event| &event.occurrences)
                .filter(|row| matches!(row.value, RulesOccurrence::LibraryCardRevealed { .. }))
                .count(),
            3
        );
        let decision = reply(shown.state(), DecisionReply::OpponentChoice(C)).into_state();
        let RuleFrame::RevealedPiles(frame) = decision.pending_decision().unwrap().frame() else {
            panic!("expected opponent's choice");
        };
        assert_eq!(frame.chooser(), C);
        let left = physical(&initial, 2).id();
        let right = physical(&initial, 3).id();
        let rest = physical(&initial, 100).id();
        if choosing_creature {
            assert_eq!(frame.choice(), &PileChoice::Creature(vec![left, right]));
            let order = reply(&decision, DecisionReply::SelectResolution(vec![right])).into_state();
            let result = reply(
                &order,
                DecisionReply::OrderResolutionGraveyard(vec![rest, left]),
            )
            .into_state();
            assert_eq!(physical(&result, 3).zone(), ObjectZone::Battlefield);
            assert_eq!(physical(&result, 2).zone(), ObjectZone::Graveyard(A));
            assert_eq!(physical(&result, 100).zone(), ObjectZone::Graveyard(A));
        } else {
            assert_eq!(frame.choice(), &PileChoice::Divide(vec![left, right, rest]));
            let parted =
                reply(&decision, DecisionReply::SelectResolution(vec![left, rest])).into_state();
            assert_eq!(parted.pending_decision().unwrap().chooser(), A);
            let result = reply(&parted, DecisionReply::ChoosePile(1)).into_state();
            assert_eq!(physical(&result, 2).zone(), ObjectZone::Hand(A));
            assert_eq!(physical(&result, 100).zone(), ObjectZone::Hand(A));
            assert_eq!(physical(&result, 3).zone(), ObjectZone::Graveyard(A));
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_matter_reshaper_can_put_the_same_revealed_card_into_either_zone() {
    let price = [(ManaType::Colourless, 3)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Matter Reshaper"),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
            seed(
                3,
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
    let alive = cast_named(&initial, &price);
    let dead =
        round(targeted(&alive, A, 3, TargetRef::Object(physical(&alive, 1).id())).into_state())
            .into_state();
    assert_eq!(physical(&dead, 1).zone(), ObjectZone::Graveyard(A));
    let choice = round(dead).into_state();
    for (selected, zone) in [
        (None, ObjectZone::Hand(A)),
        (Some(physical(&choice, 2).id()), ObjectZone::Battlefield),
    ] {
        let got = reply(
            &choice,
            DecisionReply::LibraryTop(LibraryTopReply::Search { selected }),
        )
        .into_state();
        assert_eq!(physical(&got, 2).zone(), zone);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_bribery_searches_and_shuffles_the_opponents_library() {
    let price = [(ManaType::Colourless, 3), (ManaType::Blue, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Bribery"),
            seed(
                2,
                B,
                ObjectDestination::LibraryBottom,
                Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: Some(TargetRef::Player(B)),
            payment: CastingPayment {
                mana: Some(spend(&price)),
                ..Default::default()
            },
        },
    );
    let choice = round(cast.into_state()).into_state();
    assert_eq!(choice.pending_decision().unwrap().chooser(), A);
    let ready = choice
        .supply_entropy(choice.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let missed = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::Search { selected: None }),
    );
    assert_eq!(physical(missed.state(), 2).zone(), ObjectZone::Library(B));
    assert_eq!(missed.shuffle_journal()[0].player, B);
    let result = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&ready, 2).id()),
        }),
    );
    assert_eq!(
        result
            .shuffle_journal()
            .iter()
            .map(|r| r.player)
            .collect::<Vec<_>>(),
        [B]
    );
    let stolen = physical(result.state(), 2);
    assert_eq!(
        (stolen.zone(), stolen.owner(), stolen.controller()),
        (ObjectZone::Battlefield, B, Some(A))
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_deploy_the_gatewatch_selects_two_walkers_and_bottoms_the_rest() {
    let price = [(ManaType::Colourless, 4), (ManaType::White, 2)];
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
            named(
                "Chandra, Legacy of Fire",
                3,
                A,
                ObjectDestination::LibraryBottom,
            ),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let choice = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
    let ready = choice
        .supply_entropy(choice.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let a = physical(&ready, 2).id();
    let b = physical(&ready, 3).id();
    let result = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::SearchBattlefield {
            selected_in_timestamp_order: vec![b, a],
        }),
    )
    .into_state();
    for id in [2, 3] {
        let walker = physical(&result, id);
        assert_eq!(walker.zone(), ObjectZone::Battlefield);
        assert!(
            result
                .execution()
                .counters_of_kind(walker.id(), CounterKind::Loyalty)
                .is_positive()
        );
    }
    assert_eq!(
        result.execution().window().cards().library(A),
        &[physical(&result, 100).id(), physical(&result, 4).id()]
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_search_spells_find_a_legal_card_and_apply_the_printed_destination() {
    use ManaType::*;
    for (spell, price, card_name, destination, tapped) in [
        (
            "Three Visits",
            vec![(Colourless, 1), (Green, 1)],
            "Forest",
            ObjectZone::Battlefield,
            Some(false),
        ),
        (
            "Nature's Lore",
            vec![(Colourless, 1), (Green, 1)],
            "Forest",
            ObjectZone::Battlefield,
            Some(false),
        ),
        (
            "Farseek",
            vec![(Colourless, 1), (Green, 1)],
            "Island",
            ObjectZone::Battlefield,
            Some(true),
        ),
        (
            "Rampant Growth",
            vec![(Colourless, 1), (Green, 1)],
            "Mountain",
            ObjectZone::Battlefield,
            Some(true),
        ),
        (
            "Demonic Tutor",
            vec![(Colourless, 1), (Black, 1)],
            "Forest",
            ObjectZone::Hand(A),
            None,
        ),
        (
            "Steelshaper's Gift",
            vec![(White, 1)],
            "Sword of the Animist",
            ObjectZone::Hand(A),
            None,
        ),
        (
            "Personal Tutor",
            vec![(Blue, 1)],
            "Cultivate",
            ObjectZone::Library(A),
            None,
        ),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                hand(spell),
                named("Plains", 3, A, ObjectDestination::LibraryBottom),
                named(card_name, 2, A, ObjectDestination::LibraryBottom),
            ],
            &price,
            Step::PrecombatMain,
        );
        let choice = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
        let ready = choice
            .supply_entropy(choice.basis(), vec![0; 8])
            .unwrap()
            .into_state();
        let target = physical(&ready, 2).id();
        let RuleFrame::LibraryTop(frame) = ready.pending_decision().unwrap().frame() else {
            panic!("{spell} search");
        };
        assert!(frame.eligible_results().contains(&target), "{spell}");
        let result = reply(
            &ready,
            DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: Some(target),
            }),
        );
        let card = physical(result.state(), 2);
        assert_eq!(
            (card.zone(), card.tapped()),
            (destination, tapped),
            "{spell}"
        );
        assert_eq!(result.shuffle_journal().len(), 1, "{spell}");
        if destination == ObjectZone::Library(A) {
            assert_eq!(
                result.state().execution().window().cards().library(A)[0],
                card.id()
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_search_spells_assign_multiple_results_and_shuffle() {
    use ManaType::*;
    for (spell, price) in [
        ("Cultivate", vec![(Colourless, 2), (Green, 1)]),
        ("Kodama's Reach", vec![(Colourless, 2), (Green, 1)]),
        ("Skyshroud Claim", vec![(Colourless, 3), (Green, 1)]),
        ("Final Parting", vec![(Colourless, 3), (Black, 2)]),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                hand(spell),
                named("Forest", 2, A, ObjectDestination::LibraryBottom),
                named("Forest", 3, A, ObjectDestination::LibraryBottom),
            ],
            &price,
            Step::PrecombatMain,
        );
        let choice = round(cast_untargeted(&initial, 1, &price).into_state()).into_state();
        let ready = choice
            .supply_entropy(choice.basis(), vec![0; 8])
            .unwrap()
            .into_state();
        let a = physical(&ready, 2).id();
        let b = physical(&ready, 3).id();
        let (request, zone_a, zone_b, tap_a) = match spell {
            "Skyshroud Claim" => (
                LibraryTopReply::SearchBattlefield {
                    selected_in_timestamp_order: vec![a, b],
                },
                ObjectZone::Battlefield,
                ObjectZone::Battlefield,
                Some(false),
            ),
            "Final Parting" => (
                LibraryTopReply::SearchHandGraveyard {
                    hand: Some(a),
                    graveyard: Some(b),
                },
                ObjectZone::Hand(A),
                ObjectZone::Graveyard(A),
                None,
            ),
            _ => (
                LibraryTopReply::SearchSplit {
                    battlefield: Some(a),
                    hand: Some(b),
                },
                ObjectZone::Battlefield,
                ObjectZone::Hand(A),
                Some(true),
            ),
        };
        let result = reply(&ready, DecisionReply::LibraryTop(request));
        assert_eq!(physical(result.state(), 2).zone(), zone_a, "{spell}");
        assert_eq!(physical(result.state(), 3).zone(), zone_b, "{spell}");
        assert_eq!(physical(result.state(), 2).tapped(), tap_a, "{spell}");
        assert_eq!(result.shuffle_journal().len(), 1, "{spell}");
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_zero_targets_resolve_semesters_end_and_continue_desecrate_reality() {
    for (name, price, adamant) in [
        (
            "Semester's End",
            vec![(ManaType::Colourless, 3), (ManaType::White, 1)],
            false,
        ),
        ("Desecrate Reality", vec![(ManaType::Colourless, 7)], true),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                hand(name),
                seed(
                    2,
                    A,
                    ObjectDestination::Graveyard,
                    Program::artifact(Some(ManaCost::generic(1.into())), vec![]),
                ),
                creature(3, A, 2, 2, &[]),
                creature(4, B, 2, 2, &[]),
            ],
            &price,
            Step::PrecombatMain,
        );
        let stacked = cast_untargeted(&initial, 1, &price).into_state();
        let mut result = round(stacked).into_state();
        if adamant {
            result = reply(
                &result,
                DecisionReply::SelectResolution(vec![physical(&result, 2).id()]),
            )
            .into_state();
            assert_eq!(physical(&result, 2).zone(), ObjectZone::Battlefield);
        }
        assert_eq!(physical(&result, 1).zone(), ObjectZone::Graveyard(A));
        assert_eq!(physical(&result, 3).id(), physical(&initial, 3).id());
        assert_eq!(physical(&result, 4).id(), physical(&initial, 4).id());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_divergent_transformations_discounts_and_reveals_for_each_controller() {
    let price = [(ManaType::Colourless, 4), (ManaType::Red, 1)];
    let mut cards = vec![
        hand("Divergent Transformations"),
        creature(2, A, 1, 2, &[]),
        creature(3, B, 2, 1, &[]),
    ];
    for (owner, base) in [(A, 10), (B, 20)] {
        cards.push(seed(
            base,
            owner,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
        cards.push(seed(
            base + 1,
            owner,
            ObjectDestination::LibraryBottom,
            Program::creature(None, 3.into(), 3.into(), BTreeSet::new()),
        ));
    }
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let cast = command(
        &initial,
        A,
        GameCommand::CastWithResources {
            spell: physical(&initial, 1).id(),
            target: None,
            payment: CastingPayment {
                mana: Some(spend(&price)),
                permanent_targets: vec![
                    TargetRef::Object(physical(&initial, 2).id()),
                    TargetRef::Object(physical(&initial, 3).id()),
                ],
                ..Default::default()
            },
        },
    )
    .into_state();
    let ready = cast
        .supply_entropy(cast.basis(), vec![0; 32])
        .unwrap()
        .into_state();
    let resolved = round(ready);
    assert_eq!(
        resolved
            .shuffle_journal()
            .iter()
            .map(|r| r.player)
            .collect::<Vec<_>>(),
        [A, B]
    );
    for (id, owner, base) in [(2, A, 10), (3, B, 20)] {
        assert_eq!(physical(resolved.state(), id).zone(), ObjectZone::Exile);
        assert_eq!(
            physical(resolved.state(), base).zone(),
            ObjectZone::Library(owner)
        );
        assert_eq!(
            physical(resolved.state(), base + 1).controller(),
            Some(owner)
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_wake_the_dead_observes_combat_x_and_next_end_step_sacrifice() {
    let mut cards = vec![
        named("Wake the Dead", 1, B, ObjectDestination::Hand),
        seed(
            2,
            B,
            ObjectDestination::Graveyard,
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        ),
    ];
    for id in 3..=5 {
        cards.push(named(
            "Swamp",
            id,
            B,
            ObjectDestination::Battlefield { controller: B },
        ));
    }
    let attempt = |s: &GameState, x: i64| GameCommand::CastWithResources {
        spell: physical(s, 1).id(),
        target: None,
        payment: CastingPayment {
            chosen_x: Some(x.into()),
            mana: Some(spend(&[(ManaType::Black, 2 + x)])),
            permanent_targets: vec![TargetRef::Object(physical(s, 2).id())],
            ..Default::default()
        },
    };
    for step in [Step::PrecombatMain, Step::BeginningCombat] {
        let initial = game(vec![A, B], cards.clone(), &[], step);
        let mut ready = pass(&initial).into_state();
        for id in 3..=5 {
            ready = command(
                &ready,
                B,
                GameCommand::ActivateMana {
                    source: physical(&ready, id).id(),
                    ability: ManaAbilityRef::BasicLand(BasicLandType::Swamp),
                },
            )
            .into_state();
        }
        if step == Step::PrecombatMain {
            illegal(&ready, B, attempt(&ready, 1));
            continue;
        }
        illegal(&ready, B, attempt(&ready, 0));
        let returned = round(command(&ready, B, attempt(&ready, 1)).into_state()).into_state();
        assert_eq!(physical(&returned, 2).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(&returned, 2).controller(), Some(B));
        let end = exile_hand::until(returned, |s| {
            s.turn().unwrap().position().step == Step::End
                && !s.execution().window().entries().is_empty()
        });
        let done = round(end).into_state();
        assert_eq!(physical(&done, 2).zone(), ObjectZone::Graveyard(B));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_blink_preserves_land_artifact_and_planeswalker_return_rules() {
    for (name, price, delayed) in [
        (
            "Ghostly Flicker",
            vec![(ManaType::Colourless, 2), (ManaType::Blue, 1)],
            false,
        ),
        (
            "Semester's End",
            vec![(ManaType::Colourless, 3), (ManaType::White, 1)],
            true,
        ),
    ] {
        let dest = ObjectDestination::Battlefield { controller: A };
        let cards = if delayed {
            vec![
                hand(name),
                named("Vronos, Masked Inquisitor", 2, A, ObjectDestination::Hand),
            ]
        } else {
            vec![
                hand(name),
                named("Forest", 2, A, dest),
                seed(3, A, dest, Program::artifact(None, vec![])),
            ]
        };
        let initial = if delayed {
            let game = game(
                vec![A, B],
                cards,
                &[
                    (ManaType::Colourless, 6),
                    (ManaType::Blue, 2),
                    (ManaType::White, 1),
                ],
                Step::PrecombatMain,
            );
            round(
                cast_untargeted(&game, 2, &[(ManaType::Colourless, 3), (ManaType::Blue, 2)])
                    .into_state(),
            )
            .into_state()
        } else {
            game(vec![A, B], cards, &price, Step::PrecombatMain)
        };
        let loyalty = initial
            .execution()
            .counters_of_kind(physical(&initial, 2).id(), CounterKind::Loyalty);
        let target_numbers = if delayed { vec![2] } else { vec![2, 3] };
        let stacked = command(
            &initial,
            A,
            GameCommand::CastWithResources {
                spell: physical(&initial, 1).id(),
                target: None,
                payment: CastingPayment {
                    permanent_targets: target_numbers
                        .iter()
                        .map(|&id| TargetRef::Object(physical(&initial, id).id()))
                        .collect(),
                    mana: Some(spend(&price)),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let mut result = round(stacked).into_state();
        if delayed {
            assert_eq!(physical(&result, 2).zone(), ObjectZone::Exile);
            let end = exile_hand::until(result, |g| {
                g.turn().unwrap().position().step == Step::End
                    && !g.execution().window().entries().is_empty()
            });
            result = round(end).into_state();
            assert_eq!(
                result
                    .execution()
                    .counters_of_kind(physical(&result, 2).id(), CounterKind::Loyalty),
                loyalty.plus(&1.into())
            );
        }
        if let Some(RuleFrame::LinkedReturnOrder(frame)) =
            result.pending_decision().map(|p| p.frame())
        {
            result = reply(
                &result,
                DecisionReply::LinkedReturnOrder(frame.objects().to_vec()),
            )
            .into_state();
        }
        for number in target_numbers {
            assert_eq!(physical(&result, number).zone(), ObjectZone::Battlefield);
            assert_ne!(
                physical(&result, number).id(),
                physical(&initial, number).id()
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_creature_entry_searches_apply_the_printed_filters_and_optional_choices() {
    use ManaType::*;
    for (card, price, target_name, optional) in [
        (
            "Spellseeker",
            vec![(Colourless, 2), (Blue, 1)],
            "Demonic Tutor",
            true,
        ),
        (
            "Fierce Empath",
            vec![(Colourless, 2), (Green, 1)],
            "Rune-Scarred Demon",
            true,
        ),
        ("Pilgrim's Eye", vec![(Colourless, 3)], "Plains", true),
        (
            "Rune-Scarred Demon",
            vec![(Colourless, 5), (Black, 2)],
            "Forest",
            false,
        ),
    ] {
        let initial = game(
            vec![A, B],
            vec![
                hand(card),
                named(target_name, 2, A, ObjectDestination::LibraryBottom),
            ],
            &price,
            Step::PrecombatMain,
        );
        let entered = cast_named(&initial, &price);
        let mut prompt = round(entered).into_state();
        if optional {
            let decline = reply(
                &prompt,
                DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
            );
            assert_eq!(physical(decline.state(), 2).zone(), ObjectZone::Library(A));
            assert!(decline.shuffle_journal().is_empty());
            prompt = reply(
                &prompt,
                DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
            )
            .into_state();
        }
        let target = physical(&prompt, 2).id();
        let RuleFrame::LibraryTop(frame) = prompt.pending_decision().unwrap().frame() else {
            panic!("expected search for {card}");
        };
        assert!(frame.eligible_results().contains(&target), "{card}");
        assert_eq!(
            frame
                .eligible_results()
                .contains(&physical(&prompt, 100).id()),
            !optional,
            "{card}"
        );
        let ready = prompt
            .supply_entropy(prompt.basis(), vec![0; 8])
            .unwrap()
            .into_state();
        let got = reply(
            &ready,
            DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: Some(target),
            }),
        );
        assert_eq!(physical(got.state(), 2).zone(), ObjectZone::Hand(A));
        assert_eq!(got.shuffle_journal().len(), 1);
        if !optional {
            assert!(got.state().search_reveal().is_none());
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_abundant_harvest_finds_either_kind_and_bottoms_the_other_cards() {
    let price = [(ManaType::Green, 1)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Abundant Harvest"),
            seed(
                2,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let cast = cast_untargeted(&initial, 1, &price).into_state();
    let ready = cast
        .supply_entropy(cast.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let prompt = round(ready).into_state();
    for (land, found) in [(true, 4), (false, 2)] {
        let result = reply(&prompt, DecisionReply::ChooseLandKind(land));
        assert_eq!(physical(result.state(), found).zone(), ObjectZone::Hand(A));
        assert_eq!(
            result.state().execution().window().cards().library(A).len(),
            3
        );
        if land {
            let ids = result.state().execution().window().cards().library(A);
            assert_eq!(
                ids,
                &[
                    physical(result.state(), 100).id(),
                    physical(result.state(), 3).id(),
                    physical(result.state(), 2).id()
                ]
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_gonti_exiles_privately_and_casts_after_gonti_leaves_with_other_mana() {
    let price = [(ManaType::Colourless, 2), (ManaType::Black, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Gonti, Lord of Luxury"),
            seed(
                2,
                B,
                ObjectDestination::LibraryBottom,
                Program::creature(
                    Some(ManaCost::one(ManaType::White)),
                    1.into(),
                    2.into(),
                    BTreeSet::new(),
                ),
            ),
            named("Island", 3, B, ObjectDestination::LibraryBottom),
            named("Swamp", 4, B, ObjectDestination::LibraryBottom),
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
        &[(ManaType::Colourless, 3), (ManaType::Black, 2)],
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let targeted = reply(&entered, DecisionReply::TriggerTarget(TargetRef::Player(B))).into_state();
    let prompt = round(targeted).into_state();
    let ready = prompt
        .supply_entropy(prompt.basis(), vec![0; 9])
        .unwrap()
        .into_state();
    let chosen = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&ready, 2).id()),
        }),
    )
    .into_state();
    let hidden = physical(&chosen, 2).id();
    assert_eq!(chosen.execution().face_down_exile_viewer(hidden), Some(A));
    assert!(chosen.execution().may_cast_from_face_down_exile(hidden, A));
    assert!(!chosen.execution().may_cast_from_face_down_exile(hidden, B));
    let departed = round(
        super::targeted(&chosen, A, 5, TargetRef::Object(physical(&chosen, 1).id())).into_state(),
    )
    .into_state();
    let cast = cast_untargeted(&departed, 2, &[(ManaType::Colourless, 1)]);
    let played = round(cast.into_state()).into_state();
    assert_eq!(physical(&played, 2).controller(), Some(A));
    assert_eq!(physical(&played, 2).owner(), B);
    assert!(played.execution().face_down_exile_viewer(hidden).is_none());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_oreskos_explorer_counts_players_with_more_lands_and_reveals_plains() {
    let price = [(ManaType::Colourless, 1), (ManaType::White, 1)];
    for higher in [0, 1, 2] {
        let mut cards = vec![
            hand("Oreskos Explorer"),
            named("Plains", 2, A, ObjectDestination::LibraryBottom),
            named("Plains", 3, A, ObjectDestination::LibraryBottom),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
        ];
        for (index, seat) in [B, C].into_iter().take(higher).enumerate() {
            cards.push(named(
                "Island",
                10 + index as u64,
                seat,
                ObjectDestination::Battlefield { controller: seat },
            ));
        }
        let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
        let entered = cast_named(&initial, &price);
        let prompt = round(entered).into_state();
        let ready = prompt
            .supply_entropy(prompt.basis(), vec![0; 10])
            .unwrap()
            .into_state();
        let RuleFrame::LibraryTop(frame) = ready.pending_decision().unwrap().frame() else {
            panic!("expected Plains search");
        };
        assert_eq!(frame.search_batch_limit(), higher);
        assert!(!frame.eligible_results().contains(&physical(&ready, 4).id()));
        let selected = frame
            .eligible_results()
            .iter()
            .copied()
            .take(higher)
            .collect();
        let result = reply(
            &ready,
            DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
                selected,
                colors: vec![],
            }),
        );
        let found = [2, 3]
            .into_iter()
            .filter(|&id| physical(result.state(), id).zone() == ObjectZone::Hand(A))
            .count();
        assert_eq!(found, higher);
        assert_eq!(result.shuffle_journal().len(), 1);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_tooth_and_nail_casts_each_mode_and_entwines_the_search_before_entry() {
    use spellwake_engine::mana::CastMode;
    for selected_modes in [vec![0], vec![1], vec![0, 1]] {
        let search = selected_modes.contains(&0);
        let put = selected_modes.contains(&1);
        let entwined = search && put;
        let price = [
            (ManaType::Colourless, if entwined { 7 } else { 5 }),
            (ManaType::Green, 2),
        ];
        let creature = |id, zone| {
            seed(
                id,
                A,
                zone,
                Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            )
        };
        let initial = game(
            vec![A, B],
            vec![
                hand("Tooth and Nail"),
                creature(2, ObjectDestination::LibraryBottom),
                creature(3, ObjectDestination::Hand),
                creature(4, ObjectDestination::Hand),
            ],
            &price,
            Step::PrecombatMain,
        );
        let modes = selected_modes
            .into_iter()
            .map(|mode| CastMode {
                mode,
                target: None,
                targets: vec![],
            })
            .collect();
        let cast = command(
            &initial,
            A,
            GameCommand::CastWithResources {
                spell: physical(&initial, 1).id(),
                target: None,
                payment: CastingPayment {
                    modes,
                    additional: if entwined {
                        [(0, 1.into())].into()
                    } else {
                        BTreeMap::new()
                    },
                    mana: Some(spend(&price)),
                    ..Default::default()
                },
            },
        )
        .into_state();
        let mut current = round(cast).into_state();
        if search {
            current = current
                .supply_entropy(current.basis(), vec![0; 8])
                .unwrap()
                .into_state();
            current = reply(
                &current,
                DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
                    selected: vec![physical(&current, 2).id()],
                    colors: vec![],
                }),
            )
            .into_state();
            assert_eq!(physical(&current, 2).zone(), ObjectZone::Hand(A));
        }
        if put {
            let first = if search { 2 } else { 3 };
            let cards = vec![physical(&current, first).id(), physical(&current, 4).id()];
            current = reply(&current, DecisionReply::SelectResolution(cards)).into_state();
            if let Some(RuleFrame::LinkedReturnOrder(frame)) =
                current.pending_decision().map(|p| p.frame())
            {
                current = reply(
                    &current,
                    DecisionReply::LinkedReturnOrder(frame.objects().to_vec()),
                )
                .into_state();
            }
            assert_eq!(physical(&current, first).zone(), ObjectZone::Battlefield);
            assert_eq!(physical(&current, 4).zone(), ObjectZone::Battlefield);
        }
        assert_eq!(physical(&current, 1).zone(), ObjectZone::Graveyard(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_braids_offers_the_upkeep_player_only_artifacts_creatures_and_lands() {
    let price = [(ManaType::Colourless, 2), (ManaType::Blue, 2)];
    let mut cards = vec![hand("Braids, Conjurer Adept")];
    for (id, prog) in [
        (2, Program::artifact(None, vec![])),
        (
            3,
            Program::creature(None, 3.into(), 2.into(), BTreeSet::new()),
        ),
        (4, Program::land([LandType::Forest].into(), vec![])),
        (5, Program::enchantment(None)),
    ] {
        cards.push(seed(id, B, ObjectDestination::Hand, prog));
    }
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let ready = cast_named(&initial, &price);
    let upkeep = exile_hand::until(ready, |s| {
        s.turn().unwrap().position().number == 2
            && s.turn().unwrap().position().step == Step::Upkeep
            && !s.execution().window().entries().is_empty()
    });
    let prompt = round(upkeep).into_state();
    let decision = prompt.pending_decision().unwrap();
    assert_eq!(decision.chooser(), B);
    let RuleFrame::ResolutionSelect(frame) = decision.frame() else {
        panic!("expected hand choices");
    };
    assert!(!frame.domain().contains(&physical(&prompt, 5).id()));
    for id in [2, 3, 4] {
        let result = reply(
            &prompt,
            DecisionReply::SelectResolution(vec![physical(&prompt, id).id()]),
        );
        assert_eq!(physical(result.state(), id).zone(), ObjectZone::Battlefield);
        assert_eq!(physical(result.state(), id).controller(), Some(B));
    }
    let declined = reply(&prompt, DecisionReply::SelectResolution(vec![]));
    assert_eq!(physical(declined.state(), 2).zone(), ObjectZone::Hand(B));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_composer_counts_enchantments_at_resolution_and_puts_the_choice_in_tapped() {
    let price = [(ManaType::Colourless, 1), (ManaType::Green, 1)];
    for before in [4, 5] {
        let mut cards = vec![
            hand("Composer of Spring"),
            seed(
                2,
                A,
                ObjectDestination::Hand,
                Program::enchantment(Some(ManaCost::free())),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            ),
            named("Forest", 4, A, ObjectDestination::Hand),
        ];
        for id in 0..before {
            cards.push(seed(
                10 + id,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::enchantment(None),
            ));
        }
        let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
        let ready = cast_named(&initial, &price);
        let triggered = round(cast_untargeted(&ready, 2, &[]).into_state()).into_state();
        let prompt = round(triggered).into_state();
        let RuleFrame::ResolutionSelect(frame) = prompt.pending_decision().unwrap().frame() else {
            panic!("expected hand choice");
        };
        assert_eq!(
            frame.domain().contains(&physical(&prompt, 3).id()),
            before == 5
        );
        assert!(frame.domain().contains(&physical(&prompt, 4).id()));
        let declined = reply(&prompt, DecisionReply::SelectResolution(vec![]));
        assert_eq!(physical(declined.state(), 4).zone(), ObjectZone::Hand(A));
        let chosen = if before == 5 { 3 } else { 4 };
        let effect = reply(
            &prompt,
            DecisionReply::SelectResolution(vec![physical(&prompt, chosen).id()]),
        );
        assert_eq!(
            physical(effect.state(), chosen).zone(),
            ObjectZone::Battlefield
        );
        assert_eq!(physical(effect.state(), chosen).tapped(), Some(true));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_days_undoing_keeps_the_active_turn_when_cast_by_another_player() {
    let mut cards = vec![
        named("Day's Undoing", 1, B, ObjectDestination::Hand),
        named(
            "Vedalken Orrery",
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
        ),
    ];
    for id in 3..=5 {
        cards.push(named(
            "Island",
            id,
            B,
            ObjectDestination::Battlefield { controller: B },
        ));
    }
    for (player, base) in [(A, 10), (B, 30)] {
        for n in 0..7 {
            cards.push(seed(
                base + n,
                player,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ));
        }
    }
    let initial = game(vec![A, B], cards, &[], Step::PrecombatMain);
    let mut ready = pass(&initial).into_state();
    for id in 3..=5 {
        ready = command(
            &ready,
            B,
            GameCommand::ActivateMana {
                source: physical(&ready, id).id(),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
            },
        )
        .into_state();
    }
    let cast = command(
        &ready,
        B,
        GameCommand::CastUntargeted {
            spell: physical(&ready, 1).id(),
            payment: Some(spend(&[(ManaType::Blue, 3)])),
        },
    )
    .into_state();
    let funded = cast
        .supply_entropy(cast.basis(), vec![0; 32])
        .unwrap()
        .into_state();
    let result = round(funded);
    assert_eq!(physical(result.state(), 1).zone(), ObjectZone::Graveyard(B));
    assert_eq!(
        result.state().turn().unwrap().position(),
        initial.turn().unwrap().position()
    );
    assert_eq!(result.shuffle_journal().len(), 2);
    assert_eq!(result.draw_journal().events.len(), 14);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_for_the_ancestors_chooses_a_type_and_repeats_the_effect_with_flashback() {
    let (_, vocabulary, _) = current_catalogue();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let goblin = vocabulary.resolve_label("Goblin").unwrap();
    let mut cards = vec![hand("For the Ancestors")];
    for (n, kind) in [(2, elf), (3, goblin), (4, elf)] {
        cards.push(seed(
            n,
            A,
            ObjectDestination::LibraryBottom,
            Program::creature(None, 2.into(), 2.into(), [kind].into()),
        ));
    }
    let mut state = game(
        vec![A, B],
        cards,
        &[(ManaType::Colourless, 5), (ManaType::Green, 2)],
        Step::PrecombatMain,
    );
    for (price, kind, targets, zone) in [
        (2, elf, vec![2, 4], ObjectZone::Graveyard(A)),
        (3, goblin, vec![3], ObjectZone::Exile),
    ] {
        let cast = cast_untargeted(
            &state,
            1,
            &[(ManaType::Colourless, price), (ManaType::Green, 1)],
        )
        .into_state();
        let choose = round(cast).into_state();
        let select = reply(
            &choose,
            DecisionReply::PromptChoice(
                spellwake_engine::toolbox::PromptChoiceReply::CreatureType(kind),
            ),
        )
        .into_state();
        let ready = select
            .supply_entropy(select.basis(), vec![0; 9])
            .unwrap()
            .into_state();
        let target_ids: Vec<_> = targets.iter().map(|&n| physical(&ready, n).id()).collect();
        let RuleFrame::LibraryTop(frame) = ready.pending_decision().unwrap().frame() else {
            panic!("expected library choice");
        };
        assert_eq!(
            frame
                .eligible_results()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>(),
            target_ids.iter().copied().collect()
        );
        state = reply(
            &ready,
            DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
                selected: target_ids,
                colors: vec![],
            }),
        )
        .into_state();
        for n in targets {
            assert_eq!(physical(&state, n).zone(), ObjectZone::Hand(A));
        }
        assert_eq!(physical(&state, 1).zone(), zone);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_matter_reshaper_puts_expensive_permanents_and_nonpermanents_in_hand() {
    for revealed in ["Vronos, Masked Inquisitor", "Demonic Tutor"] {
        let price = [(ManaType::Colourless, 3)];
        let initial = game(
            vec![A, B],
            vec![
                hand("Matter Reshaper"),
                named(revealed, 2, A, ObjectDestination::LibraryBottom),
                seed(
                    3,
                    A,
                    ObjectDestination::Hand,
                    free_instant(
                        BodyTarget::SINGLE_CREATURE,
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
            &price,
            Step::PrecombatMain,
        );
        let alive = cast_named(&initial, &price);
        let dead =
            round(targeted(&alive, A, 3, TargetRef::Object(physical(&alive, 1).id())).into_state())
                .into_state();
        let result = round(dead);
        assert!(result.state().pending_decision().is_none());
        assert_eq!(physical(result.state(), 2).zone(), ObjectZone::Hand(A));
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_sandstone_oracle_draws_only_the_chosen_opponents_excess_hand_size() {
    let price = [(ManaType::Colourless, 7)];
    let mut cards = vec![
        hand("Sandstone Oracle"),
        creature(6, B, 2, 2, &[]),
        creature(7, B, 2, 2, &[Reach]),
    ];
    for (id, seat, zone) in [
        (2, A, ObjectDestination::LibraryBottom),
        (3, B, ObjectDestination::Hand),
        (4, B, ObjectDestination::Hand),
    ] {
        cards.push(seed(id, seat, zone, Program::artifact(None, vec![])));
    }
    let initial = game(vec![A, B, C], cards, &price, Step::PrecombatMain);
    let entered = cast_named(&initial, &price);
    let prompt = round(entered).into_state();
    for (seat, expected) in [(B, 2), (C, 0)] {
        let done = reply(&prompt, DecisionReply::OpponentChoice(seat));
        assert_eq!(done.draw_journal().events.len(), expected);
        assert_eq!(
            physical(done.state(), 2).zone() == ObjectZone::Hand(A),
            expected > 0
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_hoarding_dragon_links_its_optional_search_only_to_its_own_death() {
    let price = [(ManaType::Colourless, 3), (ManaType::Red, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Hoarding Dragon"),
            seed(
                2,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            seed(
                3,
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
            seed(
                4,
                A,
                ObjectDestination::Exile,
                Program::artifact(None, vec![]),
            ),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let offer = round(entered).into_state();
    let bypassed = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(physical(bypassed.state(), 2).zone(), ObjectZone::Library(A));
    let choice = reply(
        &offer,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    )
    .into_state();
    let ready = choice
        .supply_entropy(choice.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    let bound = reply(
        &ready,
        DecisionReply::LibraryTop(LibraryTopReply::Search {
            selected: Some(physical(&ready, 2).id()),
        }),
    )
    .into_state();
    assert_eq!(physical(&bound, 2).zone(), ObjectZone::Exile);
    let dead =
        round(targeted(&bound, A, 3, TargetRef::Object(physical(&bound, 1).id())).into_state())
            .into_state();
    let choice = round(dead).into_state();
    let kept = reply(
        &choice,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(physical(kept.state(), 2).zone(), ObjectZone::Exile);
    let got = reply(
        &choice,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(physical(got.state(), 2).zone(), ObjectZone::Hand(A));
    assert_eq!(physical(got.state(), 4).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_land_tax_checks_opposing_land_counts_and_finds_three_basic_lands() {
    let price = [(ManaType::White, 1)];
    for ahead in [false, true] {
        let mut cards = vec![
            hand("Land Tax"),
            named("Plains", 2, A, ObjectDestination::LibraryBottom),
            named("Forest", 3, A, ObjectDestination::LibraryBottom),
            named("Island", 4, A, ObjectDestination::LibraryBottom),
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
        let upkeep = exile_hand::until(entered, |s| {
            s.turn().unwrap().position().number == 3
                && s.turn().unwrap().position().step == Step::Upkeep
        });
        if !ahead {
            assert!(upkeep.execution().window().entries().is_empty());
            continue;
        }
        let choose = round(upkeep).into_state();
        let declined = reply(
            &choose,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        );
        assert!(declined.shuffle_journal().is_empty());
        let search = reply(
            &choose,
            DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        )
        .into_state();
        let ready = search
            .supply_entropy(search.basis(), vec![0; 8])
            .unwrap()
            .into_state();
        let ids = [2, 3, 4]
            .into_iter()
            .map(|n| physical(&ready, n).id())
            .collect();
        let result = reply(
            &ready,
            DecisionReply::LibraryTop(LibraryTopReply::SearchWithEntryColors {
                selected: ids,
                colors: vec![],
            }),
        );
        for n in 2..=4 {
            assert_eq!(physical(result.state(), n).zone(), ObjectZone::Hand(A));
        }
        assert_eq!(result.state().search_reveals().len(), 3);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_guff_rewrites_history_shuffles_for_owners_then_offers_casts_to_controllers() {
    let price = [(ManaType::Colourless, 2), (ManaType::Red, 1)];
    let permanent = |n, controller| {
        seed(
            n,
            B,
            ObjectDestination::Battlefield { controller },
            Program::artifact(Some(ManaCost::generic(4.into())), vec![]),
        )
    };
    let initial = game(
        vec![A, B],
        vec![
            hand("Guff Rewrites History"),
            permanent(2, A),
            permanent(3, B),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
            named("Sol Ring", 5, A, ObjectDestination::LibraryBottom),
            named("Swamp", 6, B, ObjectDestination::LibraryBottom),
            named("Sol Ring", 7, B, ObjectDestination::LibraryBottom),
        ],
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
                mana: Some(spend(&price)),
                permanent_targets: vec![
                    TargetRef::Object(physical(&initial, 2).id()),
                    TargetRef::Object(physical(&initial, 3).id()),
                ],
                ..Default::default()
            },
        },
    )
    .into_state();
    let ready = cast
        .supply_entropy(cast.basis(), vec![4, 3, 2, 1, 0, 0, 0, 0])
        .unwrap()
        .into_state();
    let first = round(ready);
    assert_eq!(
        first
            .shuffle_journal()
            .iter()
            .map(|r| r.player)
            .collect::<Vec<_>>(),
        [B]
    );
    assert_eq!(first.state().pending_decision().unwrap().chooser(), A);
    let RuleFrame::OptionalEffect(first_offer) = first.state().pending_decision().unwrap().frame()
    else {
        panic!("expected free cast");
    };
    assert_eq!(
        first_offer.exile_cast().unwrap().spell,
        physical(first.state(), 5).id()
    );
    let accepted = reply(
        first.state(),
        DecisionReply::ImpulseCast {
            target: None,
            payment: CastingPayment::default(),
            mana_abilities: vec![],
        },
    )
    .into_state();
    assert_eq!(accepted.pending_decision().unwrap().chooser(), B);
    let RuleFrame::OptionalEffect(second_offer) = accepted.pending_decision().unwrap().frame()
    else {
        panic!("expected second free cast");
    };
    let second = second_offer.exile_cast().unwrap().spell;
    assert_eq!(
        accepted
            .execution()
            .window()
            .cards()
            .get(second)
            .unwrap()
            .owner(),
        B
    );
    let declined = reply(
        &accepted,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    )
    .into_state();
    assert_eq!(
        declined
            .execution()
            .window()
            .cards()
            .get(second)
            .unwrap()
            .zone(),
        ObjectZone::Exile
    );
    let done = round(declined).into_state();
    assert_eq!(physical(&done, 5).controller(), Some(A));
    assert_eq!(physical(&done, 4).zone(), ObjectZone::Library(A));
    assert_eq!(physical(&done, 1).zone(), ObjectZone::Graveyard(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_mystic_forge_casts_artifacts_and_colorless_spells_from_top_and_exiles_for_life() {
    let price = [(ManaType::Colourless, 4)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Mystic Forge"),
            named("Sol Ring", 2, A, ObjectDestination::LibraryBottom),
            seed(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Program::creature(
                    Some(ManaCost::generic(1.into())),
                    2.into(),
                    2.into(),
                    BTreeSet::new(),
                ),
            ),
            named("Island", 4, A, ObjectDestination::LibraryBottom),
        ],
        &[(ManaType::Colourless, 6)],
        Step::PrecombatMain,
    );
    let mut state = cast_named(&initial, &price);
    for id in [2, 3] {
        let cut = state.execution().evaluated_characteristics().unwrap();
        let own = state.execution().library_top_access(&cut, A, A);
        assert!(own.visible && own.playable);
        assert!(!state.execution().library_top_access(&cut, A, B).visible);
        state = round(cast_untargeted(&state, id, &[(ManaType::Colourless, 1)]).into_state())
            .into_state();
        assert_eq!(physical(&state, id).zone(), ObjectZone::Battlefield);
    }
    let cut = state.execution().evaluated_characteristics().unwrap();
    assert!(!state.execution().library_top_access(&cut, A, A).playable);
    illegal(
        &state,
        A,
        GameCommand::PlayLand {
            land: physical(&state, 4).id(),
        },
    );
    let source = physical(&state, 1).id();
    let activated = command(
        &state,
        A,
        GameCommand::ActivateUntargetedAbility {
            source,
            ability: ActivatedAbilityId(0),
            payment: None,
        },
    );
    assert_eq!(
        activated.state().execution().life_total(A),
        Some(&19.into())
    );
    let resolved = round(activated.into_state());
    assert_eq!(physical(resolved.state(), 4).zone(), ObjectZone::Exile);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_courser_reveals_the_top_plays_a_land_and_gains_life_on_entry() {
    let price = [(ManaType::Colourless, 1), (ManaType::Green, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Courser of Kruphix"),
            named("Forest", 2, A, ObjectDestination::LibraryBottom),
            named("Mountain", 3, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let cut = entered.execution().evaluated_characteristics().unwrap();
    assert!(entered.execution().library_top_access(&cut, A, B).visible);
    illegal(
        &entered,
        A,
        GameCommand::PlayLand {
            land: physical(&entered, 3).id(),
        },
    );
    let land = command(
        &entered,
        A,
        GameCommand::PlayLand {
            land: physical(&entered, 2).id(),
        },
    )
    .into_state();
    assert_eq!(physical(&land, 2).zone(), ObjectZone::Battlefield);
    let resolved = round(land).into_state();
    assert_eq!(resolved.execution().life_total(A), Some(&21.into()));
    illegal(
        &resolved,
        A,
        GameCommand::PlayLand {
            land: physical(&resolved, 3).id(),
        },
    );
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_demonlord_belzenlok_repeats_for_high_mana_values_and_damages_for_nonlands() {
    let price = [(ManaType::Colourless, 4), (ManaType::Black, 2)];
    let initial = game(
        vec![A, B],
        vec![
            hand("Demonlord Belzenlok"),
            named("Swamp", 2, A, ObjectDestination::LibraryBottom),
            named(
                "Vronos, Masked Inquisitor",
                3,
                A,
                ObjectDestination::LibraryBottom,
            ),
            named("Forest", 4, A, ObjectDestination::LibraryBottom),
            named("Sol Ring", 5, A, ObjectDestination::LibraryBottom),
        ],
        &price,
        Step::PrecombatMain,
    );
    let entered = cast_named(&initial, &price);
    let finished = round(entered).into_state();
    for id in [2, 4] {
        assert_eq!(physical(&finished, id).zone(), ObjectZone::Exile);
    }
    for id in [3, 5] {
        assert_eq!(physical(&finished, id).zone(), ObjectZone::Hand(A));
    }
    assert_eq!(finished.execution().life_total(A), Some(&18.into()));
    assert_eq!(physical(&finished, 100).zone(), ObjectZone::Library(A));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_zones_realmwalker_uses_its_own_entry_type_to_cast_the_actual_top_creature() {
    use spellwake_engine::land::EntryChoice;
    let (_, vocabulary, _) = current_catalogue();
    let elf = vocabulary.resolve_label("Elf").unwrap();
    let goblin = vocabulary.resolve_label("Goblin").unwrap();
    let price = [(ManaType::Colourless, 2), (ManaType::Green, 1)];
    let mut cards = vec![hand("Realmwalker")];
    for (n, kind) in [(2, elf), (3, goblin)] {
        cards.push(seed(
            n,
            A,
            ObjectDestination::LibraryBottom,
            Program::creature(Some(ManaCost::free()), 1.into(), 1.into(), [kind].into()),
        ));
    }
    let initial = game(vec![A, B], cards, &price, Step::PrecombatMain);
    let choose = cast_named(&initial, &price);
    let entered = reply(
        &choose,
        DecisionReply::EntryChoice(EntryChoice::CreatureType(elf)),
    )
    .into_state();
    let cut = entered.execution().evaluated_characteristics().unwrap();
    let own = entered.execution().library_top_access(&cut, A, A);
    assert!(own.visible && own.playable);
    assert!(
        cut.get(physical(&entered, 1).id())
            .unwrap()
            .current_creature_subtypes
            .contains(&goblin)
    );
    assert!(!entered.execution().library_top_access(&cut, A, B).visible);
    let elf_cast = round(cast_untargeted(&entered, 2, &[]).into_state()).into_state();
    assert_eq!(physical(&elf_cast, 2).controller(), Some(A));
    let cut = elf_cast.execution().evaluated_characteristics().unwrap();
    assert!(!elf_cast.execution().library_top_access(&cut, A, A).playable);
    illegal(
        &elf_cast,
        A,
        GameCommand::CastUntargeted {
            spell: physical(&elf_cast, 3).id(),
            payment: None,
        },
    );
}
