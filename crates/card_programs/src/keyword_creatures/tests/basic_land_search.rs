use super::*;
use spellwake_engine::{
    effects::LibraryTopReply,
    game::{DecisionReply, RuleFrame},
    occurrence::{RulesOccurrence, RulesRecord, ZoneReason},
};

#[test]
#[ignore = "requires the dated Oracle catalogue and matching vocabulary"]
fn cmm_rampant_growth_and_font_of_fertility_search_tapped_with_real_definitions() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, number, destination| {
        let row = inventory.cards.iter().find(|row| row.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry.seed(&id, CardId(number), a, destination).unwrap()
    };
    let object = |state: &GameState, number| {
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(number))
            .unwrap()
    };
    for name in ["Rampant Growth", "Font of Fertility"] {
        let source = seed(name, 1, ObjectDestination::Hand);
        let initial_order = vec![];
        let ability = source.program.activated.first().map(|ability| ability.id);
        let game = GameState::initialize_scheduled(
            GameId(904),
            GameContext::new(
                CURRENT_KEYWORD_PROFILE.into(),
                vocabulary.rules_revision().into(),
                TEACHING_SOURCE_SHA256.into(),
                "cmm-search".into(),
                name.into(),
                vocabulary.id(),
                names.id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    source,
                    seed("Forest", 2, ObjectDestination::LibraryBottom),
                    seed("Island", 3, ObjectDestination::LibraryBottom),
                ],
                initial_order,
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Green,
                        amount: if name == "Font of Fertility" {
                            2.into()
                        } else {
                            1.into()
                        },
                    },
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
                        amount: 1.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                names.clone(),
            ),
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let game = if name == "Font of Fertility" {
            let cast = advance(
                &game,
                a,
                GameCommand::CastUntargeted {
                    spell: object(&game, 1),
                    payment: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![ManaSpend {
                            mana_type: ManaType::Green,
                            amount: 1.into(),
                        }],
                    }),
                },
            );
            let first = advance(&cast, a, GameCommand::Pass);
            let resolved = advance(&first, b, GameCommand::Pass);
            assert_eq!(
                resolved
                    .execution()
                    .window()
                    .cards()
                    .get(object(&resolved, 1))
                    .unwrap()
                    .zone(),
                ObjectZone::Battlefield
            );
            resolved
        } else {
            game
        };
        let payment = Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![
                ManaSpend {
                    mana_type: ManaType::Green,
                    amount: 1.into(),
                },
                ManaSpend {
                    mana_type: ManaType::Red,
                    amount: 1.into(),
                },
            ],
        });
        let command = if name == "Rampant Growth" {
            GameCommand::CastUntargeted {
                spell: object(&game, 1),
                payment,
            }
        } else {
            GameCommand::ActivateUntargetedAbility {
                source: object(&game, 1),
                ability: ability.unwrap(),
                payment,
            }
        };
        let paid = advance(&game, a, command);
        assert_eq!(paid.execution().mana(a, ManaType::Green), 0.into());
        assert_eq!(paid.execution().mana(a, ManaType::Red), 0.into());
        if name == "Font of Fertility" {
            assert_eq!(
                paid.execution()
                    .window()
                    .cards()
                    .get(object(&paid, 1))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(a)
            );
        }
        let first = advance(&paid, a, GameCommand::Pass);
        let pending = advance(&first, b, GameCommand::Pass);
        let pending = pending
            .supply_entropy(pending.basis(), vec![0; 8])
            .unwrap()
            .into_state();
        let RuleFrame::LibraryTop(frame) = pending.pending_decision().unwrap().frame() else {
            panic!("missing search")
        };
        let forest = object(&pending, 2);
        assert!(frame.eligible_results().contains(&forest));
        for selected in [Some(forest), None] {
            let result = pending
                .advance(GameInput {
                    basis: pending.basis().clone(),
                    actor: a,
                    command: GameCommand::Reply {
                        decision: pending.pending_decision().unwrap().id(),
                        choice: DecisionReply::LibraryTop(LibraryTopReply::Search { selected }),
                    },
                })
                .unwrap();
            assert_eq!(result.shuffle_journal().len(), 1);
            assert!(result.state().search_reveal().is_none());
            let land = result
                .state()
                .execution()
                .window()
                .cards()
                .get(object(result.state(), 2))
                .unwrap();
            if selected.is_some() {
                assert_eq!(land.zone(), ObjectZone::Battlefield);
                assert_eq!(land.controller(), Some(a));
                assert_eq!(land.tapped(), Some(true));
                assert_ne!(land.id(), forest);
                assert!(result.rules_journal().records.iter().any(|r| matches!(r, RulesRecord::Event(e) if e.occurrences.iter().any(|o| matches!(&o.value, RulesOccurrence::ZoneChange(c) if c.reason == ZoneReason::SearchToBattlefield && c.after.object.id() == land.id())))));
            } else {
                assert_eq!(land.zone(), ObjectZone::Library(a));
            }
        }
    }
}

#[test]
#[ignore = "requires the dated Oracle catalogue and matching vocabulary"]
fn cmm_bauble_and_expanse_pay_tap_sacrifice_and_search_tapped() {
    use spellwake_engine::occurrence::{RulesOccurrence, RulesRecord, ZoneReason};
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, number, destination| {
        let row = inventory.cards.iter().find(|row| row.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry.seed(&id, CardId(number), a, destination).unwrap()
    };
    let object = |state: &GameState, number| {
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(number))
            .unwrap()
    };
    for name in ["Wayfarer's Bauble", "Terramorphic Expanse"] {
        let source = seed(name, 1, ObjectDestination::Hand);
        let ability = source.program.activated[0].id;
        let game = GameState::initialize_scheduled(
            GameId(905),
            GameContext::new(
                CURRENT_KEYWORD_PROFILE.into(),
                vocabulary.rules_revision().into(),
                TEACHING_SOURCE_SHA256.into(),
                "cmm-tap-sacrifice".into(),
                name.into(),
                vocabulary.id(),
                names.id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    source,
                    seed("Forest", 2, ObjectDestination::LibraryBottom),
                    seed("Island", 3, ObjectDestination::LibraryBottom),
                ],
                vec![],
                vec![FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 3.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                names.clone(),
            ),
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let game = if name == "Wayfarer's Bauble" {
            let paid = advance(
                &game,
                a,
                GameCommand::CastUntargeted {
                    spell: object(&game, 1),
                    payment: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![ManaSpend {
                            mana_type: ManaType::Red,
                            amount: 1.into(),
                        }],
                    }),
                },
            );
            let first = advance(&paid, a, GameCommand::Pass);
            advance(&first, b, GameCommand::Pass)
        } else {
            advance(
                &game,
                a,
                GameCommand::PlayLand {
                    land: object(&game, 1),
                },
            )
        };
        let source = object(&game, 1);
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(source)
                .unwrap()
                .tapped(),
            Some(false)
        );
        let payment = if name == "Wayfarer's Bauble" {
            Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Red,
                    amount: 2.into(),
                }],
            })
        } else {
            None
        };
        let paid = game
            .advance(GameInput {
                basis: game.basis().clone(),
                actor: a,
                command: GameCommand::ActivateUntargetedAbility {
                    source,
                    ability,
                    payment,
                },
            })
            .unwrap();
        assert_eq!(
            paid.state()
                .execution()
                .window()
                .cards()
                .get(object(paid.state(), 1))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(a)
        );
        let occurrences: Vec<_> = paid
            .rules_journal()
            .records
            .iter()
            .filter_map(|r| match r {
                RulesRecord::Event(e) => Some(e),
                _ => None,
            })
            .flat_map(|e| e.occurrences.iter().map(|o| &o.value))
            .collect();
        let tapped = occurrences.iter().position(|o| matches!(o, RulesOccurrence::StatusChange { change, .. } if change.before.id() == source && change.after.tapped() == Some(true))).unwrap();
        let sacrificed = occurrences.iter().position(|o| matches!(o, RulesOccurrence::ZoneChange(change) if change.reason == ZoneReason::SacrificeCost && change.before.object.id() == source && change.before.object.tapped() == Some(true))).unwrap();
        assert!(tapped < sacrificed);
        let first = advance(paid.state(), a, GameCommand::Pass);
        let pending = advance(&first, b, GameCommand::Pass);
        let pending = pending
            .supply_entropy(pending.basis(), vec![0; 8])
            .unwrap()
            .into_state();
        let forest = object(&pending, 2);
        for selected in [Some(forest), None] {
            let result = pending
                .advance(GameInput {
                    basis: pending.basis().clone(),
                    actor: a,
                    command: GameCommand::Reply {
                        decision: pending.pending_decision().unwrap().id(),
                        choice: DecisionReply::LibraryTop(LibraryTopReply::Search { selected }),
                    },
                })
                .unwrap();
            assert_eq!(result.shuffle_journal().len(), 1);
            assert!(result.state().search_reveal().is_none());
            let current = result
                .state()
                .execution()
                .window()
                .cards()
                .get(object(result.state(), 2))
                .unwrap();
            if selected.is_some() {
                assert_eq!(current.zone(), ObjectZone::Battlefield);
                assert_eq!(current.tapped(), Some(true));
            } else {
                assert_eq!(current.zone(), ObjectZone::Library(a));
            }
            assert_eq!(
                result
                    .state()
                    .execution()
                    .window()
                    .cards()
                    .get(object(result.state(), 1))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(a)
            );
        }
    }
}

#[test]
#[ignore = "requires the dated Oracle catalogue and matching vocabulary"]
fn cmm_omen_and_solemn_offer_optional_entry_search() {
    use spellwake_engine::effects::OptionalEffectChoice;
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, number, destination| {
        let row = inventory.cards.iter().find(|row| row.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry.seed(&id, CardId(number), a, destination).unwrap()
    };
    let object = |state: &GameState, number| {
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(number))
            .unwrap()
    };
    for name in ["Omen of the Hunt", "Solemn Simulacrum"] {
        let game = GameState::initialize_scheduled(
            GameId(906),
            GameContext::new(
                CURRENT_KEYWORD_PROFILE.into(),
                vocabulary.rules_revision().into(),
                TEACHING_SOURCE_SHA256.into(),
                "cmm-entry-search".into(),
                name.into(),
                vocabulary.id(),
                names.id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    seed(name, 1, ObjectDestination::Hand),
                    seed("Forest", 2, ObjectDestination::LibraryBottom),
                    seed("Island", 3, ObjectDestination::LibraryBottom),
                ],
                vec![],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
                        amount: 4.into(),
                    },
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Green,
                        amount: 1.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                names.clone(),
            ),
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let spend = if name == "Omen of the Hunt" {
            vec![
                ManaSpend {
                    mana_type: ManaType::Red,
                    amount: 2.into(),
                },
                ManaSpend {
                    mana_type: ManaType::Green,
                    amount: 1.into(),
                },
            ]
        } else {
            vec![ManaSpend {
                mana_type: ManaType::Red,
                amount: 4.into(),
            }]
        };
        let cast = advance(
            &game,
            a,
            GameCommand::CastUntargeted {
                spell: object(&game, 1),
                payment: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend,
                }),
            },
        );
        let first = advance(&cast, a, GameCommand::Pass);
        let entered = advance(&first, b, GameCommand::Pass);
        let first = advance(&entered, a, GameCommand::Pass);
        let offered = advance(&first, b, GameCommand::Pass);
        assert!(matches!(
            offered.pending_decision().unwrap().frame(),
            RuleFrame::OptionalEffect(_)
        ));
        let reply = |choice| GameInput {
            basis: offered.basis().clone(),
            actor: a,
            command: GameCommand::Reply {
                decision: offered.pending_decision().unwrap().id(),
                choice: DecisionReply::OptionalEffect(choice),
            },
        };
        let declined = offered
            .advance(reply(OptionalEffectChoice::Decline))
            .unwrap();
        assert!(declined.shuffle_journal().is_empty());
        assert!(declined.state().pending_decision().is_none());
        assert_eq!(
            declined
                .state()
                .execution()
                .window()
                .cards()
                .get(object(declined.state(), 2))
                .unwrap()
                .zone(),
            ObjectZone::Library(a)
        );
        let taken = offered.advance(reply(OptionalEffectChoice::Take)).unwrap();
        let searching = taken
            .state()
            .supply_entropy(taken.state().basis(), vec![0; 8])
            .unwrap()
            .into_state();
        assert!(matches!(
            searching.pending_decision().unwrap().frame(),
            RuleFrame::LibraryTop(_)
        ));
        let forest = object(&searching, 2);
        let done = searching
            .advance(GameInput {
                basis: searching.basis().clone(),
                actor: a,
                command: GameCommand::Reply {
                    decision: searching.pending_decision().unwrap().id(),
                    choice: DecisionReply::LibraryTop(LibraryTopReply::Search {
                        selected: Some(forest),
                    }),
                },
            })
            .unwrap();
        assert_eq!(done.shuffle_journal().len(), 1);
        let land = done
            .state()
            .execution()
            .window()
            .cards()
            .get(object(done.state(), 2))
            .unwrap();
        assert_eq!(land.zone(), ObjectZone::Battlefield);
        assert_eq!(land.tapped(), Some(true));
        assert!(done.state().search_reveal().is_none());
    }
}

#[test]
#[ignore = "requires the dated Oracle catalogue and matching vocabulary"]
fn cmm_omen_scry_and_solemn_death_draw() {
    use spellwake_engine::{effects::OptionalEffectChoice, targets::TargetRef};
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, n, dest| {
        let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry.seed(&id, CardId(n), a, dest).unwrap()
    };
    let object = |g: &GameState, n| {
        g.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    for name in ["Omen of the Hunt", "Solemn Simulacrum"] {
        let source = seed(name, 1, ObjectDestination::Battlefield { controller: a });
        let killer = SeedCard {
            card: CardId(4),
            owner: b,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SinglePermanent,
                    vec![
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
                    ],
                ),
            ),
        };
        let game = GameState::initialize_scheduled(
            GameId(907),
            GameContext::new(
                CURRENT_KEYWORD_PROFILE.into(),
                vocabulary.rules_revision().into(),
                TEACHING_SOURCE_SHA256.into(),
                "cmm-search-secondary".into(),
                name.into(),
                vocabulary.id(),
                names.id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    source,
                    seed("Forest", 2, ObjectDestination::LibraryBottom),
                    seed("Island", 3, ObjectDestination::LibraryBottom),
                    killer,
                ],
                vec![CardId(1)],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
                        amount: 2.into(),
                    },
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Green,
                        amount: 1.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                names.clone(),
            ),
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        if name == "Omen of the Hunt" {
            let paid = advance(
                &game,
                a,
                GameCommand::ActivateUntargetedAbility {
                    source: object(&game, 1),
                    ability: ActivatedAbilityId(0),
                    payment: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![
                            ManaSpend {
                                mana_type: ManaType::Red,
                                amount: 2.into(),
                            },
                            ManaSpend {
                                mana_type: ManaType::Green,
                                amount: 1.into(),
                            },
                        ],
                    }),
                },
            );
            assert_eq!(
                paid.execution()
                    .window()
                    .cards()
                    .get(object(&paid, 1))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(a)
            );
            let first = advance(&paid, a, GameCommand::Pass);
            let pending = advance(&first, b, GameCommand::Pass);
            let RuleFrame::LibraryTop(frame) = pending.pending_decision().unwrap().frame() else {
                panic!("expected scry")
            };
            let top = frame.domain_top_first().to_vec();
            assert_eq!(top.len(), 2);
            let result = pending
                .advance(GameInput {
                    basis: pending.basis().clone(),
                    actor: a,
                    command: GameCommand::Reply {
                        decision: pending.pending_decision().unwrap().id(),
                        choice: DecisionReply::LibraryTop(LibraryTopReply::Scry {
                            top_first: vec![top[1]],
                            bottom_top_first: vec![top[0]],
                        }),
                    },
                })
                .unwrap();
            assert!(result.state().pending_decision().is_none());
            assert_eq!(
                result.state().execution().window().cards().library(a),
                &[top[1], top[0]]
            );
        } else {
            let priority = advance(&game, a, GameCommand::Pass);
            let cast = advance(
                &priority,
                b,
                GameCommand::Cast {
                    spell: object(&priority, 4),
                    target: TargetRef::Object(object(&priority, 1)),
                },
            );
            let first = advance(&cast, b, GameCommand::Pass);
            let killed = advance(&first, a, GameCommand::Pass);
            assert_eq!(
                killed
                    .execution()
                    .window()
                    .cards()
                    .get(object(&killed, 1))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(a)
            );
            let first = advance(&killed, a, GameCommand::Pass);
            let pending = advance(&first, b, GameCommand::Pass);
            assert!(matches!(
                pending.pending_decision().unwrap().frame(),
                RuleFrame::OptionalEffect(_)
            ));
            for choice in [OptionalEffectChoice::Take, OptionalEffectChoice::Decline] {
                let result = pending
                    .advance(GameInput {
                        basis: pending.basis().clone(),
                        actor: a,
                        command: GameCommand::Reply {
                            decision: pending.pending_decision().unwrap().id(),
                            choice: DecisionReply::OptionalEffect(choice),
                        },
                    })
                    .unwrap();
                assert_eq!(
                    result.draw_journal().events.len(),
                    usize::from(choice == OptionalEffectChoice::Take)
                );
            }
        }
    }
}

#[test]
#[ignore = "requires the dated Oracle catalogue and matching vocabulary"]
fn cmm_omen_flash_casts_on_opponents_turn() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let row = inventory
        .cards
        .iter()
        .find(|r| r.name == "Omen of the Hunt")
        .unwrap();
    let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
    let source = registry
        .seed(&id, CardId(1), a, ObjectDestination::Hand)
        .unwrap();
    assert!(
        source
            .program
            .printed_keywords
            .contains(&KeywordAbility::Flash)
    );
    let game = GameState::initialize_scheduled(
        GameId(908),
        GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "cmm-omen-flash".into(),
            "opponent-turn".into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap(),
        GameSeed::with_uniform_life(
            vec![a, b],
            b,
            vec![source],
            vec![],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 2.into(),
                },
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Green,
                    amount: 1.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        ),
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let ready = advance(&game, b, GameCommand::Pass);
    let source = ready
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let cast = advance(
        &ready,
        a,
        GameCommand::CastUntargeted {
            spell: source,
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![
                    ManaSpend {
                        mana_type: ManaType::Red,
                        amount: 2.into(),
                    },
                    ManaSpend {
                        mana_type: ManaType::Green,
                        amount: 1.into(),
                    },
                ],
            }),
        },
    );
    let first = advance(&cast, a, GameCommand::Pass);
    let resolved = advance(&first, b, GameCommand::Pass);
    let current = resolved
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    assert_eq!(
        resolved
            .execution()
            .window()
            .cards()
            .get(current)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
}

#[test]
#[ignore = "requires the dated Oracle catalogue and matching vocabulary"]
fn cmm_natures_lore_and_three_visits_find_basic_or_nonbasic_forest() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, n, dest| {
        let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry.seed(&id, CardId(n), a, dest).unwrap()
    };
    let object = |g: &GameState, n| {
        g.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    for name in ["Nature's Lore", "Three Visits"] {
        let game = GameState::initialize_scheduled(
            GameId(909),
            GameContext::new(
                CURRENT_KEYWORD_PROFILE.into(),
                vocabulary.rules_revision().into(),
                TEACHING_SOURCE_SHA256.into(),
                "cmm-forest-search".into(),
                name.into(),
                vocabulary.id(),
                names.id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    seed(name, 1, ObjectDestination::Hand),
                    seed("Forest", 2, ObjectDestination::LibraryBottom),
                    seed("Cinder Glade", 3, ObjectDestination::LibraryBottom),
                    seed("Island", 4, ObjectDestination::LibraryBottom),
                ],
                vec![],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Green,
                        amount: 1.into(),
                    },
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
                        amount: 1.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                names.clone(),
            ),
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let cast = advance(
            &game,
            a,
            GameCommand::CastUntargeted {
                spell: object(&game, 1),
                payment: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: vec![
                        ManaSpend {
                            mana_type: ManaType::Green,
                            amount: 1.into(),
                        },
                        ManaSpend {
                            mana_type: ManaType::Red,
                            amount: 1.into(),
                        },
                    ],
                }),
            },
        );
        let first = advance(&cast, a, GameCommand::Pass);
        let pending = advance(&first, b, GameCommand::Pass);
        let pending = pending
            .supply_entropy(pending.basis(), vec![0; 8])
            .unwrap()
            .into_state();
        let RuleFrame::LibraryTop(frame) = pending.pending_decision().unwrap().frame() else {
            panic!("search")
        };
        assert_eq!(
            frame.eligible_results(),
            &[object(&pending, 2), object(&pending, 3)]
        );
        for (card, tapped) in [(2, false), (3, true)] {
            let selected = object(&pending, card);
            let done = pending
                .advance(GameInput {
                    basis: pending.basis().clone(),
                    actor: a,
                    command: GameCommand::Reply {
                        decision: pending.pending_decision().unwrap().id(),
                        choice: DecisionReply::LibraryTop(LibraryTopReply::Search {
                            selected: Some(selected),
                        }),
                    },
                })
                .unwrap();
            let land = done
                .state()
                .execution()
                .window()
                .cards()
                .get(object(done.state(), card))
                .unwrap();
            assert_eq!(land.zone(), ObjectZone::Battlefield);
            assert_eq!(land.tapped(), Some(tapped));
            assert!(done.state().search_reveal().is_none());
            assert_eq!(done.shuffle_journal().len(), 1);
        }
        let none = pending
            .advance(GameInput {
                basis: pending.basis().clone(),
                actor: a,
                command: GameCommand::Reply {
                    decision: pending.pending_decision().unwrap().id(),
                    choice: DecisionReply::LibraryTop(LibraryTopReply::Search { selected: None }),
                },
            })
            .unwrap();
        assert_eq!(none.shuffle_journal().len(), 1);
    }
}

#[test]
#[ignore = "requires the dated Oracle catalogue and matching vocabulary"]
fn cmm_farseek_and_tapped_fetchlands_find_their_printed_subtypes() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, n, dest| {
        let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry.seed(&id, CardId(n), a, dest).unwrap()
    };
    let object = |g: &GameState, n| {
        g.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    for (name, eligible) in [
        ("Farseek", vec![3, 4, 5, 6]),
        ("Flood Plain", vec![3, 4]),
        ("Grasslands", vec![2, 3]),
        ("Mountain Valley", vec![2, 6]),
        ("Rocky Tar Pit", vec![5, 6]),
    ] {
        let initialize = |destination, initial_order| {
            GameState::initialize_scheduled(
                GameId(910),
                GameContext::new(
                    CURRENT_KEYWORD_PROFILE.into(),
                    vocabulary.rules_revision().into(),
                    TEACHING_SOURCE_SHA256.into(),
                    "cmm-subtype-search".into(),
                    name.into(),
                    vocabulary.id(),
                    names.id(),
                )
                .unwrap(),
                GameSeed::with_uniform_life(
                    vec![a, b],
                    a,
                    vec![
                        seed(name, 1, destination),
                        seed("Forest", 2, ObjectDestination::LibraryBottom),
                        seed("Plains", 3, ObjectDestination::LibraryBottom),
                        seed("Island", 4, ObjectDestination::LibraryBottom),
                        seed("Swamp", 5, ObjectDestination::LibraryBottom),
                        seed("Mountain", 6, ObjectDestination::LibraryBottom),
                    ],
                    initial_order,
                    vec![
                        FloatingMana {
                            player: a,
                            mana_type: ManaType::Green,
                            amount: 1.into(),
                        },
                        FloatingMana {
                            player: a,
                            mana_type: ManaType::Red,
                            amount: 1.into(),
                        },
                    ],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    vocabulary.clone(),
                    names.clone(),
                ),
                TurnStart {
                    number: 3,
                    step: Step::PrecombatMain,
                },
            )
            .unwrap()
            .into_state()
        };
        let game = if name == "Farseek" {
            initialize(ObjectDestination::Hand, vec![])
        } else {
            let hand = initialize(ObjectDestination::Hand, vec![]);
            let played = advance(
                &hand,
                a,
                GameCommand::PlayLand {
                    land: object(&hand, 1),
                },
            );
            let source = object(&played, 1);
            assert_eq!(
                played
                    .execution()
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(true)
            );
            let saved = played.clone();
            assert!(
                played
                    .advance(GameInput {
                        basis: played.basis().clone(),
                        actor: a,
                        command: GameCommand::ActivateUntargetedAbility {
                            source,
                            ability: ActivatedAbilityId(0),
                            payment: None
                        }
                    })
                    .is_err()
            );
            assert_eq!(played, saved);
            initialize(
                ObjectDestination::Battlefield { controller: a },
                vec![CardId(1)],
            )
        };
        let command = if name == "Farseek" {
            GameCommand::CastUntargeted {
                spell: object(&game, 1),
                payment: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: vec![
                        ManaSpend {
                            mana_type: ManaType::Green,
                            amount: 1.into(),
                        },
                        ManaSpend {
                            mana_type: ManaType::Red,
                            amount: 1.into(),
                        },
                    ],
                }),
            }
        } else {
            GameCommand::ActivateUntargetedAbility {
                source: object(&game, 1),
                ability: ActivatedAbilityId(0),
                payment: None,
            }
        };
        let paid = advance(&game, a, command);
        if name != "Farseek" {
            assert_eq!(
                paid.execution()
                    .window()
                    .cards()
                    .get(object(&paid, 1))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(a)
            );
        }
        let first = advance(&paid, a, GameCommand::Pass);
        let pending = advance(&first, b, GameCommand::Pass);
        let pending = pending
            .supply_entropy(pending.basis(), vec![0; 16])
            .unwrap()
            .into_state();
        let RuleFrame::LibraryTop(frame) = pending.pending_decision().unwrap().frame() else {
            panic!("search")
        };
        assert_eq!(
            frame.eligible_results(),
            eligible
                .iter()
                .map(|n| object(&pending, *n))
                .collect::<Vec<_>>()
        );
        for selected in [Some(object(&pending, eligible[0])), None] {
            let done = pending
                .advance(GameInput {
                    basis: pending.basis().clone(),
                    actor: a,
                    command: GameCommand::Reply {
                        decision: pending.pending_decision().unwrap().id(),
                        choice: DecisionReply::LibraryTop(LibraryTopReply::Search { selected }),
                    },
                })
                .unwrap();
            assert_eq!(done.shuffle_journal().len(), 1);
            assert!(done.state().search_reveal().is_none());
            if selected.is_some() {
                let land = done
                    .state()
                    .execution()
                    .window()
                    .cards()
                    .get(object(done.state(), eligible[0]))
                    .unwrap();
                assert_eq!(land.zone(), ObjectZone::Battlefield);
                assert_eq!(land.tapped(), Some(name == "Farseek"));
            }
        }
    }
}
