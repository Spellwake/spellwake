//! Source-qualified Sliver keyword grants and gameplay.
use super::*;
use spellwake_engine::{
    combat::{AttackDeclaration, BlockDeclaration, CombatDecision},
    game::{DecisionReply, NativeCandidate, RuleFrame},
    object_identity::ObjectId,
    program::{BodyTarget, ExecutableBody, Instruction},
    targets::TargetRef,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn object(game: &GameState, n: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn act(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
    .unwrap()
}
fn pass(game: &GameState) -> GameState {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("priority")
    };
    act(game, holder, GameCommand::Pass).into_state()
}
fn round(game: &GameState) -> GameState {
    pass(&pass(game))
}
fn reply(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    let pending = game.pending_decision().unwrap();
    act(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
}
fn seed(card: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination,
        program,
    }
}
fn named(name: &str, destination: ObjectDestination) -> SeedCard {
    let (inventory, _, registry) = current_catalogue();
    let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
    registry
        .seed(
            &OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap(),
            CardId(1),
            A,
            destination,
        )
        .unwrap()
}
fn world(
    name: &str,
    source_zone: ObjectDestination,
    extra: Vec<SeedCard>,
    mana: Vec<FloatingMana>,
    step: Step,
) -> GameState {
    let (_, vocabulary, registry) = current_catalogue();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        format!(
            "keyword-gameplay-registry{}",
            registry.coverage.registry_version
        ),
        "sliver-keyword-games".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let mut cards = vec![named(name, source_zone)];
    cards.extend(extra);
    for player in [A, B] {
        cards.push(seed(
            100 + u64::from(player.0),
            player,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let battlefield = cards
        .iter()
        .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
        .map(|c| c.card)
        .collect();
    let initial = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        battlefield,
        mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    GameState::initialize_scheduled(GameId(902), context, initial, TurnStart { number: 1, step })
        .unwrap()
        .into_state()
}
fn frame(game: &GameState, attackers: bool) -> GameState {
    let mut state = game.clone();
    for _ in 0..30 {
        match state.pending_decision().map(|p| p.frame()) {
            Some(RuleFrame::Combat(CombatDecision::Attackers(_))) if attackers => return state,
            Some(RuleFrame::Combat(CombatDecision::Blockers(_))) if !attackers => return state,
            Some(f) => panic!("unexpected frame {f:?}"),
            None => state = pass(&state),
        }
    }
    panic!("combat frame missing")
}
fn sliver(power: i64, toughness: i64) -> Program {
    let (_, v, _) = current_catalogue();
    Program::creature(
        None,
        power.into(),
        toughness.into(),
        [v.resolve_label("Sliver").unwrap()].into(),
    )
}
fn card_zone(game: &GameState, n: u64) -> ObjectZone {
    game.execution()
        .window()
        .cards()
        .get(object(game, n))
        .unwrap()
        .zone()
}
fn creature_has(game: &GameState, n: u64, k: KeywordAbility) -> bool {
    game.execution()
        .current_characteristics(object(game, n))
        .unwrap()
        .unwrap()
        .has_keyword(k)
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sliver_keyword_definitions_cast_and_track_control_and_source_departure() {
    use KeywordAbility::{
        Deathtouch, DoubleStrike, FirstStrike, Flying, Haste, Indestructible, Lifelink, Menace,
        Shroud, Vigilance,
    };
    let cases: &[(&str, &[KeywordAbility], bool)] = &[
        ("Blur Sliver", &[Haste], false),
        ("Bonescythe Sliver", &[DoubleStrike], false),
        ("Cloudshredder Sliver", &[Flying, Haste], false),
        ("Galerider Sliver", &[Flying], false),
        ("Sentinel Sliver", &[Vigilance], false),
        ("Syphon Sliver", &[Lifelink], false),
        ("Striking Sliver", &[FirstStrike], false),
        ("Venom Sliver", &[Deathtouch], false),
        ("Sliver Hivelord", &[Indestructible], false),
        ("Winged Sliver", &[Flying], true),
        ("Two-Headed Sliver", &[Menace], true),
        ("Crystalline Sliver", &[Shroud], true),
    ];
    for &(name, keywords, all) in cases {
        let source = named(name, ObjectDestination::Hand);
        let cost = source
            .program
            .permanent()
            .unwrap()
            .casting_cost
            .clone()
            .unwrap();
        let mut mana = cost
            .required
            .iter()
            .map(|(&mana_type, amount)| FloatingMana {
                player: A,
                mana_type,
                amount: amount.clone(),
            })
            .collect::<Vec<_>>();
        if cost.generic.is_positive() {
            mana.push(FloatingMana {
                player: A,
                mana_type: ManaType::Colourless,
                amount: cost.generic.clone(),
            });
        }
        let mut spend = cost
            .required
            .iter()
            .map(|(&mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.clone(),
            })
            .collect::<Vec<_>>();
        if cost.generic.is_positive() {
            spend.push(ManaSpend {
                mana_type: ManaType::Colourless,
                amount: cost.generic.clone(),
            });
        }
        let removal = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SingleCreature,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(
                    ),
                ],
            ),
        );
        let g = world(
            name,
            ObjectDestination::Hand,
            vec![
                seed(
                    2,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    sliver(4, 4),
                ),
                seed(
                    3,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    sliver(4, 4),
                ),
                seed(
                    4,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    Program::creature(None, 4.into(), 4.into(), BTreeSet::new()),
                ),
                seed(5, A, ObjectDestination::Hand, removal),
            ],
            mana,
            Step::PrecombatMain,
        );
        let cast = act(
            &g,
            A,
            GameCommand::CastUntargeted {
                spell: object(&g, 1),
                payment: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend,
                }),
            },
        )
        .into_state();
        let entered = round(&cast);
        assert_eq!(card_zone(&entered, 1), ObjectZone::Battlefield, "{name}");
        let row = current_catalogue()
            .0
            .cards
            .iter()
            .find(|row| row.name == name)
            .unwrap();
        let face = &row.faces[0];
        assert_eq!(
            cost,
            fixed_cost(face.mana_cost.as_deref().unwrap()).unwrap(),
            "{name}"
        );
        let sheet = entered
            .execution()
            .current_characteristics(object(&entered, 1))
            .unwrap()
            .unwrap();
        assert_eq!(
            sheet
                .current_names
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>(),
            [name]
        );
        assert_eq!(sheet.current_types, [CardType::Creature].into());
        let expected_colors = cost
            .required
            .keys()
            .filter_map(|mana| match mana {
                ManaType::White => Some(Color::White),
                ManaType::Blue => Some(Color::Blue),
                ManaType::Black => Some(Color::Black),
                ManaType::Red => Some(Color::Red),
                ManaType::Green => Some(Color::Green),
                ManaType::Colourless => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(sheet.current_colors, expected_colors, "{name}");
        assert_eq!(
            sheet.current_creature_subtypes,
            [current_catalogue().1.resolve_label("Sliver").unwrap()].into()
        );
        let stats = sheet.power_toughness.unwrap().derived;
        assert_eq!(stats.power, number(face.power.as_deref()).unwrap().into());
        assert_eq!(
            stats.toughness,
            number(face.toughness.as_deref()).unwrap().into()
        );
        let legendary = face.type_line.as_deref().unwrap().starts_with("Legendary ");
        assert_eq!(
            sheet.current_supertypes.contains(&Supertype::Legendary),
            legendary
        );
        for &k in keywords {
            assert!(creature_has(&entered, 1, k), "{name} source {k:?}");
            assert!(creature_has(&entered, 2, k), "{name} own {k:?}");
            assert_eq!(creature_has(&entered, 3, k), all, "{name} opponent {k:?}");
            assert!(!creature_has(&entered, 4, k), "{name} non-Sliver {k:?}");
        }
        if !keywords.contains(&Shroud) {
            let s = act(
                &entered,
                A,
                GameCommand::Cast {
                    spell: object(&entered, 5),
                    target: TargetRef::Object(object(&entered, 1)),
                },
            )
            .into_state();
            let removed = round(&s);
            assert_eq!(card_zone(&removed, 1), ObjectZone::Hand(A));
            for &k in keywords {
                assert!(!creature_has(&removed, 2, k), "{name} lingering {k:?}");
                assert!(
                    !creature_has(&removed, 3, k),
                    "{name} opponent lingering {k:?}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sliver_keyword_grants_change_combat_and_destruction_rules() {
    use KeywordAbility::{
        Deathtouch, DoubleStrike, FirstStrike, Flying, Haste, Indestructible, Lifelink, Menace,
        Vigilance,
    };
    let cases = [
        ("Blur Sliver", Haste),
        ("Bonescythe Sliver", DoubleStrike),
        ("Cloudshredder Sliver", Flying),
        ("Cloudshredder Sliver", Haste),
        ("Galerider Sliver", Flying),
        ("Sentinel Sliver", Vigilance),
        ("Syphon Sliver", Lifelink),
        ("Striking Sliver", FirstStrike),
        ("Venom Sliver", Deathtouch),
        ("Sliver Hivelord", Indestructible),
        ("Winged Sliver", Flying),
        ("Two-Headed Sliver", Menace),
    ];
    for (name, keyword) in cases {
        if keyword == Indestructible {
            let destroy = Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
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
            );
            let game = world(
                name,
                ObjectDestination::Battlefield { controller: A },
                vec![
                    seed(
                        2,
                        A,
                        ObjectDestination::Battlefield { controller: A },
                        sliver(2, 4),
                    ),
                    seed(3, A, ObjectDestination::Hand, destroy),
                ],
                vec![],
                Step::PrecombatMain,
            );
            let spell = act(
                &game,
                A,
                GameCommand::Cast {
                    spell: object(&game, 3),
                    target: TargetRef::Object(object(&game, 2)),
                },
            )
            .into_state();
            let resolved = round(&spell);
            assert_eq!(card_zone(&resolved, 2), ObjectZone::Battlefield, "{name}");
            continue;
        }
        let newly_cast = keyword == Haste;
        let mut peer = sliver(2, 4);
        if newly_cast {
            let spellwake_engine::program::ProgramKind::Permanent(definition) = &mut peer.kind
            else {
                panic!("creature")
            };
            definition.casting_cost = Some(ManaCost::free());
            peer.mana.push(spellwake_engine::mana::ManaAbility {
                id: spellwake_engine::mana::ManaAbilityId(0),
                output: spellwake_engine::mana::ManaOutput::Fixed(
                    [(ManaType::Green, 1.into())].into(),
                ),
            });
        }
        let control_peer = peer.clone();
        let defender = if keyword == FirstStrike {
            Program::creature(None, 8.into(), 1.into(), BTreeSet::new())
        } else {
            Program::creature(None, 0.into(), 8.into(), BTreeSet::new())
        };
        let blocker_with_reach = Program::creature(None, 0.into(), 8.into(), BTreeSet::new())
            .with_printed_keywords([KeywordAbility::Reach]);
        let extra = vec![
            seed(
                2,
                A,
                if newly_cast {
                    ObjectDestination::Hand
                } else {
                    ObjectDestination::Battlefield { controller: A }
                },
                peer,
            ),
            seed(
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
                defender,
            ),
            seed(
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
                blocker_with_reach,
            ),
        ];
        let mut game = world(
            name,
            ObjectDestination::Battlefield { controller: A },
            extra,
            vec![],
            if newly_cast {
                Step::PrecombatMain
            } else {
                Step::Untap
            },
        );
        if newly_cast {
            let cast = act(
                &game,
                A,
                GameCommand::CastUntargeted {
                    spell: object(&game, 2),
                    payment: None,
                },
            )
            .into_state();
            game = round(&cast);
            let ability = spellwake_engine::mana::ManaAbilityRef::Printed(
                spellwake_engine::mana::ManaAbilityId(0),
            );
            let tap = GameCommand::ActivateMana {
                source: object(&game, 2),
                ability,
            };
            let produced = act(&game, A, tap).into_state();
            assert_eq!(
                produced.execution().mana(A, ManaType::Green),
                1.into(),
                "{name} grants immediate tap"
            );
            let control = world(
                "Galerider Sliver",
                ObjectDestination::Battlefield { controller: A },
                vec![seed(2, A, ObjectDestination::Hand, control_peer)],
                vec![],
                Step::PrecombatMain,
            );
            let control_cast = act(
                &control,
                A,
                GameCommand::CastUntargeted {
                    spell: object(&control, 2),
                    payment: None,
                },
            )
            .into_state();
            let control = round(&control_cast);
            let no_haste = GameCommand::ActivateMana {
                source: object(&control, 2),
                ability,
            };
            assert!(
                control
                    .advance(GameInput {
                        basis: control.basis().clone(),
                        actor: A,
                        command: no_haste
                    })
                    .is_err(),
                "new creature without haste cannot tap"
            );
        }
        let attack = frame(&game, true);
        let attacker = object(&attack, 2);
        let declared = reply(
            &attack,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker,
                defender: B,
                defending_planeswalker: None,
            }]),
        )
        .into_state();
        assert_eq!(
            declared
                .execution()
                .window()
                .cards()
                .get(attacker)
                .unwrap()
                .tapped()
                .unwrap(),
            keyword != Vigilance,
            "{name} attack tap"
        );
        let blockers = frame(&declared, false);
        let one = DecisionReply::DeclareBlockers(vec![BlockDeclaration {
            blocker: object(&blockers, 3),
            attacker,
        }]);
        if keyword == Flying || keyword == Menace {
            let pending = blockers.pending_decision().unwrap();
            assert!(
                blockers
                    .advance(GameInput {
                        basis: blockers.basis().clone(),
                        actor: pending.chooser(),
                        command: GameCommand::Reply {
                            decision: pending.id(),
                            choice: one
                        }
                    })
                    .is_err(),
                "{name} must reject one ground blocker"
            );
        }
        if keyword == Flying {
            let reach = vec![BlockDeclaration {
                blocker: object(&blockers, 4),
                attacker,
            }];
            let _accepted = reply(&blockers, DecisionReply::DeclareBlockers(reach));
        }
        if keyword == Menace {
            let both = vec![
                BlockDeclaration {
                    blocker: object(&blockers, 3),
                    attacker,
                },
                BlockDeclaration {
                    blocker: object(&blockers, 4),
                    attacker,
                },
            ];
            let _accepted = reply(&blockers, DecisionReply::DeclareBlockers(both));
        }
        let choices = if keyword == FirstStrike || keyword == Deathtouch {
            vec![BlockDeclaration {
                blocker: object(&blockers, 3),
                attacker,
            }]
        } else {
            vec![]
        };
        let blocked = reply(&blockers, DecisionReply::DeclareBlockers(choices)).into_state();
        let mut after = round(&blocked);
        if keyword == FirstStrike {
            assert_eq!(card_zone(&after, 3), ObjectZone::Graveyard(B), "{name}");
            assert_eq!(card_zone(&after, 2), ObjectZone::Battlefield, "{name}");
        } else if keyword == Deathtouch {
            assert_eq!(card_zone(&after, 3), ObjectZone::Graveyard(B), "{name}");
        } else if keyword == DoubleStrike {
            assert_eq!(
                after.execution().life_total(B),
                Some(&18.into()),
                "first strike {name}"
            );
            after = round(&after);
            assert_eq!(
                after.execution().life_total(B),
                Some(&16.into()),
                "regular strike {name}"
            );
        } else if keyword == Lifelink {
            assert_eq!(after.execution().life_total(A), Some(&22.into()), "{name}");
            assert_eq!(after.execution().life_total(B), Some(&18.into()), "{name}");
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn sliver_keyword_compiler_rejects_unrepresented_extra_rules() {
    let (inventory, vocabulary, _) = current_catalogue();
    let original = inventory
        .cards
        .iter()
        .find(|r| r.name == "Galerider Sliver")
        .unwrap();
    for text in [
        "Sliver creatures you control have flying.\nDraw a card.",
        "Sliver creatures you control have flying until end of turn.",
        "Sliver creatures you control have flying and ward {1}.",
        "All Sliver creatures have flying. Draw a card.",
    ] {
        let mut card = original.clone();
        card.faces[0].oracle_text = Some(text.into());
        assert!(
            static_grants::sliver_keywords(&card, &vocabulary).is_none(),
            "{text}"
        );
    }
    let firewake = inventory
        .cards
        .iter()
        .find(|r| r.name == "Firewake Sliver")
        .unwrap();
    assert!(static_grants::sliver_keywords(firewake, &vocabulary).is_none());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn crystalline_sliver_grants_shroud_to_noncreature_slivers_and_both_players() {
    let (_, vocabulary, _) = current_catalogue();
    let mut kindred = Program::artifact(None, vec![]);
    kindred.card_types.insert(CardType::Kindred);
    kindred
        .creature_subtypes
        .insert(vocabulary.resolve_label("Sliver").unwrap());
    let target = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![
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
            ],
        ),
    );
    let sacrifice = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::sacrifice(
                    spellwake_engine::program::AffectedPlayers::One(
                        spellwake_engine::program::ResolvingPlayer::Controller,
                    ),
                    spellwake_engine::program::ValueExpr::Literal(1.into()),
                    spellwake_engine::program::PermanentTypeFilter {
                        any_types: Default::default(),
                        required_types: [CardType::Creature].into(),
                        forbidden_types: BTreeSet::new(),
                        object_filter: None,
                    },
                ),
            ))],
        ),
    );
    let state = world(
        "Crystalline Sliver",
        ObjectDestination::Battlefield { controller: A },
        vec![
            seed(
                2,
                B,
                ObjectDestination::Battlefield { controller: B },
                kindred,
            ),
            seed(
                3,
                B,
                ObjectDestination::Battlefield { controller: B },
                Program::artifact(None, vec![]),
            ),
            seed(4, A, ObjectDestination::Hand, target),
            seed(5, A, ObjectDestination::Hand, sacrifice),
        ],
        vec![],
        Step::PrecombatMain,
    );
    assert!(creature_has(&state, 1, KeywordAbility::Shroud));
    assert!(creature_has(&state, 2, KeywordAbility::Shroud));
    assert!(!creature_has(&state, 3, KeywordAbility::Shroud));
    for target in [1, 2] {
        let command = GameCommand::Cast {
            spell: object(&state, 4),
            target: TargetRef::Object(object(&state, target)),
        };
        assert!(
            state
                .advance(GameInput {
                    basis: state.basis().clone(),
                    actor: A,
                    command
                })
                .is_err()
        );
    }
    let cast = act(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 4),
            target: TargetRef::Object(object(&state, 3)),
        },
    )
    .into_state();
    let done = round(&cast);
    assert_eq!(card_zone(&done, 3), ObjectZone::Graveyard(B));
    let cast = act(
        &done,
        A,
        GameCommand::CastUntargeted {
            spell: object(&done, 5),
            payment: None,
        },
    )
    .into_state();
    let selecting = round(&cast);
    let source = object(&selecting, 1);
    let after = reply(&selecting, DecisionReply::SelectResolution(vec![source])).into_state();
    assert_eq!(card_zone(&after, 1), ObjectZone::Graveyard(A));
    assert!(!creature_has(&after, 2, KeywordAbility::Shroud));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn sliver_creature_grants_follow_host_type_changes() {
    use spellwake_engine::characteristics::{
        ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, SubtypeOperation, TypeChangeSpec,
        TypeOperation,
    };
    let effect = |types: BTreeSet<CardType>| {
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                            land_subtypes: SubtypeOperation::Keep,
                            supertypes: TypeOperation::Keep,
                            types: TypeOperation::Set(types),
                            subtypes: SubtypeOperation::Keep,
                        })]),
                        ResolvedDuration::NoStatedDuration,
                    ),
                ],
            ),
        )
    };
    let state = world(
        "Winged Sliver",
        ObjectDestination::Battlefield { controller: A },
        vec![
            seed(
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
                sliver(3, 3),
            ),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                effect([CardType::Artifact, CardType::Kindred].into()),
            ),
            seed(
                4,
                A,
                ObjectDestination::Hand,
                effect([CardType::Creature, CardType::Kindred].into()),
            ),
        ],
        vec![],
        Step::PrecombatMain,
    );
    assert!(creature_has(&state, 2, KeywordAbility::Flying));
    let cast = act(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 3),
            target: TargetRef::Object(object(&state, 2)),
        },
    )
    .into_state();
    let artifact = round(&cast);
    let sheet = artifact
        .execution()
        .current_characteristics(object(&artifact, 2))
        .unwrap()
        .unwrap();
    assert!(!sheet.has_type(CardType::Creature));
    assert!(!sheet.has_keyword(KeywordAbility::Flying));
    assert!(
        sheet
            .current_creature_subtypes
            .contains(&current_catalogue().1.resolve_label("Sliver").unwrap())
    );
    let cast = act(
        &artifact,
        A,
        GameCommand::Cast {
            spell: object(&artifact, 4),
            target: TargetRef::Object(object(&artifact, 2)),
        },
    )
    .into_state();
    let restored = round(&cast);
    assert!(creature_has(&restored, 2, KeywordAbility::Flying));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn sliver_hivelord_obeys_legend_rule_despite_indestructible() {
    use spellwake_engine::checks::CheckDecision;
    let original = named(
        "Sliver Hivelord",
        ObjectDestination::Battlefield { controller: A },
    );
    let mana = [
        ManaType::White,
        ManaType::Blue,
        ManaType::Black,
        ManaType::Red,
        ManaType::Green,
    ]
    .into_iter()
    .map(|mana_type| FloatingMana {
        player: A,
        mana_type,
        amount: 1.into(),
    })
    .collect();
    let state = world(
        "Sliver Hivelord",
        ObjectDestination::Hand,
        vec![seed(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            original.program,
        )],
        mana,
        Step::PrecombatMain,
    );
    let payment = ManaPayment {
        from_lots: Default::default(),
        spend: [
            ManaType::White,
            ManaType::Blue,
            ManaType::Black,
            ManaType::Red,
            ManaType::Green,
        ]
        .into_iter()
        .map(|mana_type| ManaSpend {
            mana_type,
            amount: 1.into(),
        })
        .collect(),
    };
    let cast = act(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: Some(payment),
        },
    )
    .into_state();
    let choice = round(&cast);
    let RuleFrame::Checks(CheckDecision::Legend(frame)) =
        choice.pending_decision().unwrap().frame()
    else {
        panic!("legend choice")
    };
    assert_eq!(frame.conflicts().len(), 1);
    let conflict = frame.conflicts()[0].id();
    let done = reply(
        &choice,
        DecisionReply::KeepLegend {
            conflict,
            keep: object(&choice, 2),
        },
    )
    .into_state();
    assert_eq!(card_zone(&done, 1), ObjectZone::Graveyard(A));
    assert_eq!(card_zone(&done, 2), ObjectZone::Battlefield);
    assert!(creature_has(&done, 2, KeywordAbility::Indestructible));
}
