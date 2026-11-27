//! Oracle positions bind backup to the abilities printed below that instance (CR 702.165).
use super::{KeywordAbility, TriggeredAbility, TriggeredAbilityId, static_keyword};
use spellwake_engine::program::GrowthKeyword;

pub(super) fn trigger(
    label: &str,
    following: &[String],
    id: TriggeredAbilityId,
) -> Option<TriggeredAbility> {
    let key = match label {
        "mentor" => GrowthKeyword::Mentor,
        "evolve" => GrowthKeyword::Evolve,
        "increment" => GrowthKeyword::Increment,
        _ => {
            let n = label.strip_prefix("backup ")?;
            if n.is_empty() || !n.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let printed_below = following
                .iter()
                .filter(|s| !s.starts_with("backup "))
                .map(|s| match s.as_str() {
                    "wither" => Some(KeywordAbility::Wither),
                    "infect" => Some(KeywordAbility::Infect),
                    other => static_keyword(other),
                })
                .collect::<Option<_>>()?;
            GrowthKeyword::Backup {
                amount: n.to_owned().try_into().ok()?,
                printed_below,
            }
        }
    };
    Some(key.ability(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        OracleId, TEACHING_SOURCE_SHA256,
        keyword_creatures::{CURRENT_KEYWORD_PROFILE, tests::current_catalogue},
    };
    use spellwake_engine::{
        combat::{AttackDeclaration, CombatRules},
        game::{DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
        mana::{FloatingMana, ManaPayment, ManaSpend, ManaType},
        object_identity::{CardId, ObjectDestination, ObjectId},
        priority::{PriorityPhase, SeatId},
        setup::GameSeed,
        stack_window::WindowPhase,
        targets::TargetRef,
        turn::{Step, TurnStart},
    };
    #[test]
    fn backup_uses_only_nonbackup_abilities_below_its_own_printed_position() {
        use KeywordAbility::{FirstStrike, Flying, Lifelink};
        let keys: Vec<_> = ["flying", "backup 2", "first strike", "lifelink"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(
            trigger("backup 1", &keys, TriggeredAbilityId(2)),
            Some(
                GrowthKeyword::Backup {
                    amount: 1.into(),
                    printed_below: [Flying, FirstStrike, Lifelink].into()
                }
                .ability(TriggeredAbilityId(2))
            )
        );
        assert_eq!(
            trigger("backup 2", &keys[2..], TriggeredAbilityId(3)),
            Some(
                GrowthKeyword::Backup {
                    amount: 2.into(),
                    printed_below: [FirstStrike, Lifelink].into()
                }
                .ability(TriggeredAbilityId(3))
            )
        );
        for word in ["backup", "backup -1", "backup +1", "mentor 1", "evolve x"] {
            assert_eq!(trigger(word, &[], TriggeredAbilityId(0)), None);
        }
        assert_eq!(
            trigger(
                "backup 1",
                &["when this creature dies, draw a card.".into()],
                TriggeredAbilityId(0)
            ),
            None
        );
    }
    fn id(s: &GameState, n: u64) -> ObjectId {
        s.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    }
    fn act(s: &GameState, actor: SeatId, command: GameCommand) -> GameState {
        s.advance(GameInput {
            basis: s.basis().clone(),
            actor,
            command,
        })
        .unwrap()
        .into_state()
    }
    #[track_caller]
    fn pass(s: &GameState) -> GameState {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
        else {
            panic!(
                "expected priority at {:?}; phase {:?}, pending {:?}",
                s.turn().map(|t| t.position()),
                s.execution().window().phase(),
                s.pending_decision()
            )
        };
        act(s, holder, GameCommand::Pass)
    }
    fn reply(s: &GameState, choice: DecisionReply) -> GameState {
        let p = s.pending_decision().unwrap();
        act(
            s,
            p.chooser(),
            GameCommand::Reply {
                decision: p.id(),
                choice,
            },
        )
    }
    fn resolve(s: &GameState) -> GameState {
        pass(&pass(s))
    }
    fn count(s: &GameState, n: u64) -> spellwake_contracts::RulesInteger {
        s.execution()
            .power_toughness_counters(id(s, n))
            .unwrap()
            .plus_one
    }
    #[test]
    #[ignore = "requires pinned Oracle and creature vocabulary inputs"]
    fn blade_instructor_cloudfin_raptor_and_saiba_cryptomancer_grow_through_play() {
        let (_, vocab, reg) = current_catalogue();
        let (a, b) = (SeatId(0), SeatId(1));
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let ctx = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry16".into(),
            "three-growth-abilities".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let mut cards = vec![];
        for (n, name, zone) in [
            (
                1,
                "Blade Instructor",
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                2,
                "Cloudfin Raptor",
                ObjectDestination::Battlefield { controller: a },
            ),
            (3, "Saiba Cryptomancer", ObjectDestination::Hand),
            (4, "Blade Instructor", ObjectDestination::Hand),
            (5, "Boon-Bringer Valkyrie", ObjectDestination::LibraryBottom),
        ] {
            let row = reg
                .coverage
                .cards
                .iter()
                .find(|r| r.required && r.name == name)
                .unwrap();
            cards.push(
                reg.seed(
                    &OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap(),
                    CardId(n),
                    a,
                    zone,
                )
                .unwrap(),
            );
        }
        let lands = [
            (6, "Plains"),
            (7, "Plains"),
            (8, "Plains"),
            (9, "Island"),
            (10, "Island"),
        ];
        for (n, name) in lands {
            let row = reg
                .coverage
                .cards
                .iter()
                .find(|r| r.required && r.name == name)
                .unwrap();
            cards.push(
                reg.seed(
                    &OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap(),
                    CardId(n),
                    a,
                    ObjectDestination::Battlefield { controller: a },
                )
                .unwrap(),
            );
        }
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            [1, 2, 6, 7, 8, 9, 10].into_iter().map(CardId).collect(),
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(83),
            ctx,
            seed,
            TurnStart {
                number: 1,
                step: Step::Untap,
            },
        )
        .unwrap()
        .into_state();
        while state.turn().unwrap().position().step != Step::PrecombatMain {
            state = pass(&state);
        }
        for (n, name) in lands {
            use spellwake_engine::{land::BasicLandType, mana::ManaAbilityRef};
            let kind = if name == "Plains" {
                BasicLandType::Plains
            } else {
                BasicLandType::Island
            };
            state = act(
                &state,
                a,
                GameCommand::ActivateMana {
                    source: id(&state, n),
                    ability: ManaAbilityRef::BasicLand(kind),
                },
            );
        }
        let cast = |s: &GameState, n, mana_type, amount: i64| {
            act(
                s,
                a,
                GameCommand::CastUntargeted {
                    spell: id(s, n),
                    payment: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![ManaSpend {
                            mana_type,
                            amount: amount.into(),
                        }],
                    }),
                },
            )
        };
        state = resolve(&cast(&state, 4, ManaType::White, 3));
        state = resolve(&state);
        assert_eq!(count(&state, 2), 1.into());
        state = resolve(&cast(&state, 3, ManaType::Blue, 2));
        state = reply(
            &state,
            DecisionReply::TriggerTarget(TargetRef::Object(id(&state, 2))),
        );
        state = resolve(&state);
        assert_eq!(count(&state, 2), 2.into());
        let raptor = state
            .execution()
            .current_characteristics(id(&state, 2))
            .unwrap()
            .unwrap();
        assert!(raptor.has_keyword(KeywordAbility::Hexproof));
        assert!(!raptor.has_keyword(KeywordAbility::Flash));
        while state.pending_decision().is_none() {
            state = pass(&state);
        }
        state = reply(
            &state,
            DecisionReply::DeclareAttackers(
                [1, 2]
                    .into_iter()
                    .map(|n| AttackDeclaration {
                        attacker: id(&state, n),
                        defender: b,
                        defending_planeswalker: None,
                    })
                    .collect(),
            ),
        );
        let RuleFrame::TriggerTarget(frame) = state.pending_decision().unwrap().frame() else {
            panic!("mentor target")
        };
        assert_eq!(frame.domain(), &[TargetRef::Object(id(&state, 2))]);
        state = reply(
            &state,
            DecisionReply::TriggerTarget(TargetRef::Object(id(&state, 2))),
        );
        assert_eq!(count(&resolve(&state), 2), 3.into());
    }
    #[test]
    #[ignore = "requires pinned Oracle and creature vocabulary inputs"]
    fn arcbound_worker_simic_initiate_kami_of_lunacy_and_lantern_kami_transfer() {
        use spellwake_engine::{
            effects::{OptionalEffectChoice, SeedCard},
            object_identity::ObjectZone,
            program::{BodyTarget, ExecutableBody, Program},
        };
        let (_, vocab, reg) = current_catalogue();
        let (a, b) = (SeatId(0), SeatId(1));
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let ctx = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry16".into(),
            "three-transfer-abilities".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let mut cards = Vec::new();
        for (n, name, owner, zone) in [
            (1, "Simic Initiate", a, ObjectDestination::Hand),
            (2, "Arcbound Worker", a, ObjectDestination::Hand),
            (
                3,
                "Ornithopter",
                b,
                ObjectDestination::Battlefield { controller: b },
            ),
            (
                4,
                "Kami of Lunacy",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (5, "Lantern Kami", a, ObjectDestination::Graveyard),
            (6, "Lantern Kami", b, ObjectDestination::Graveyard),
            (7, "Kami of Lunacy", a, ObjectDestination::Graveyard),
            (8, "Ornithopter", a, ObjectDestination::Graveyard),
        ] {
            let row = reg
                .coverage
                .cards
                .iter()
                .find(|r| r.required && r.name == name)
                .unwrap();
            cards.push(
                reg.seed(
                    &OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap(),
                    CardId(n),
                    owner,
                    zone,
                )
                .unwrap(),
            );
        }
        for n in [9, 10] {
            cards.push(SeedCard {
                card: CardId(n),
                owner: a,
                destination: ObjectDestination::Hand,
                program: Program::instant(
                    spellwake_engine::mana::ManaCost::free(),
                    ExecutableBody::new(
                        BodyTarget::SINGLE_CREATURE,
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
            });
        }
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(3), CardId(4)],
            vec![FloatingMana {
                player: a,
                mana_type: ManaType::Green,
                amount: 2.into(),
            }],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(84),
            ctx,
            seed,
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        for n in [1, 2] {
            state = resolve(&act(
                &state,
                a,
                GameCommand::CastUntargeted {
                    spell: id(&state, n),
                    payment: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![ManaSpend {
                            mana_type: ManaType::Green,
                            amount: 1.into(),
                        }],
                    }),
                },
            ));
        }
        let option = |s: &GameState, choice| reply(s, DecisionReply::OptionalEffect(choice));
        state = option(&resolve(&state), OptionalEffectChoice::Take);
        let zone = |s: &GameState, n| s.execution().window().cards().get(id(s, n)).unwrap().zone();
        assert_eq!(zone(&state, 1), ObjectZone::Graveyard(a));
        assert_eq!(count(&state, 2), 2.into());
        let destroy = |s: &GameState, spell, target| {
            resolve(&act(
                s,
                a,
                GameCommand::Cast {
                    spell: id(s, spell),
                    target: TargetRef::Object(id(s, target)),
                },
            ))
        };
        state = destroy(&state, 9, 2);
        let choose_only = |s: &GameState, n| {
            let RuleFrame::TriggerTarget(f) = s.pending_decision().unwrap().frame() else {
                panic!("transfer target")
            };
            let target = TargetRef::Object(id(s, n));
            assert_eq!(f.domain(), &[target]);
            reply(s, DecisionReply::TriggerTarget(target))
        };
        state = option(
            &resolve(&choose_only(&state, 3)),
            OptionalEffectChoice::Take,
        );
        assert_eq!(count(&state, 3), 2.into());
        state = destroy(&state, 10, 4);
        let chosen = resolve(&choose_only(&state, 5));
        let declined = option(&chosen, OptionalEffectChoice::Decline);
        assert_eq!(zone(&declined, 5), ObjectZone::Graveyard(a));
        state = option(&chosen, OptionalEffectChoice::Take);
        assert_eq!(zone(&state, 5), ObjectZone::Hand(a));
        assert_eq!(zone(&state, 6), ObjectZone::Graveyard(b));
    }
    #[test]
    #[ignore = "requires pinned Oracle and creature vocabulary inputs"]
    fn basilica_guards_and_basilica_screecher_extort_white_or_black_per_instance() {
        use spellwake_engine::{
            effects::ResolutionPaymentReply, land::BasicLandType, mana::ManaAbilityRef,
        };
        let (_, vocab, registry) = current_catalogue();
        let (a, b, c) = (SeatId(0), SeatId(1), SeatId(2));
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let ctx = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry16".into(),
            "three-player-extort".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let mut cards = Vec::new();
        for (n, name) in [
            (1, "Basilica Guards"),
            (2, "Basilica Screecher"),
            (3, "Plains"),
            (4, "Ornithopter"),
        ] {
            let row = registry
                .coverage
                .cards
                .iter()
                .find(|r| r.required && r.name == name)
                .unwrap();
            cards.push(
                registry
                    .seed(
                        &OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap(),
                        CardId(n),
                        a,
                        if n == 4 {
                            ObjectDestination::Hand
                        } else {
                            ObjectDestination::Battlefield { controller: a }
                        },
                    )
                    .unwrap(),
            );
        }
        let seed = GameSeed::with_uniform_life(
            vec![a, b, c],
            a,
            cards,
            vec![CardId(1), CardId(2), CardId(3)],
            [ManaType::Red, ManaType::Black]
                .into_iter()
                .map(|mana_type| FloatingMana {
                    player: a,
                    mana_type,
                    amount: 1.into(),
                })
                .collect(),
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(86),
            ctx,
            seed,
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        state = act(
            &state,
            a,
            GameCommand::CastUntargeted {
                spell: id(&state, 4),
                payment: None,
            },
        );
        let RuleFrame::Triggers(order) = state.pending_decision().unwrap().frame() else {
            panic!("two extort triggers")
        };
        assert_eq!(order.instances().len(), 2);
        state = reply(
            &state,
            DecisionReply::OrderTriggers(order.instances().to_vec()),
        );
        let paid = |color| {
            DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: color,
                    amount: 1.into(),
                }],
            }))
        };
        for (n, color) in [(1, ManaType::White), (2, ManaType::Black)] {
            state = pass(&pass(&pass(&state)));
            let p = state.pending_decision().unwrap();
            let RuleFrame::ResolutionPayment(frame) = p.frame() else {
                panic!("hybrid payment")
            };
            assert_eq!(
                frame.restricted_colors(),
                Some([ManaType::White, ManaType::Black])
            );
            let wrong = GameCommand::Reply {
                decision: p.id(),
                choice: paid(ManaType::Red),
            };
            assert!(
                state
                    .advance(GameInput {
                        basis: state.basis().clone(),
                        actor: a,
                        command: wrong
                    })
                    .is_err()
            );
            let declined = reply(
                &state,
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
            );
            assert_eq!(
                declined.execution().life_total(b),
                state.execution().life_total(b)
            );
            if n == 1 {
                state = act(
                    &state,
                    a,
                    GameCommand::ActivateMana {
                        source: id(&state, 3),
                        ability: ManaAbilityRef::BasicLand(BasicLandType::Plains),
                    },
                );
            }
            state = reply(&state, paid(color));
            assert_eq!(state.execution().life_total(a), Some(&(20 + 2 * n).into()));
            for opponent in [b, c] {
                assert_eq!(
                    state.execution().life_total(opponent),
                    Some(&(20 - n).into())
                );
            }
        }
    }
    #[test]
    #[ignore = "requires pinned Oracle and creature vocabulary inputs"]
    fn nulldrifter_and_mist_intruder_sacrifice_and_exile_the_attacked_players_cards() {
        use spellwake_engine::{
            combat::CombatDecision, mana::CastingPayment, object_identity::ObjectZone,
        };
        let (_, vocab, registry) = current_catalogue();
        let (a, b, c) = (SeatId(0), SeatId(1), SeatId(2));
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let ctx = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry16".into(),
            "eldrazi-combat".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let mut cards = vec![];
        for (n, name, owner, zone) in [
            (
                1,
                "Nulldrifter",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                2,
                "Mist Intruder",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                3,
                "Ornithopter",
                b,
                ObjectDestination::Battlefield { controller: b },
            ),
            (
                4,
                "Ornithopter",
                c,
                ObjectDestination::Battlefield { controller: c },
            ),
            (5, "Culling Drone", c, ObjectDestination::LibraryBottom),
            (6, "Nulldrifter", a, ObjectDestination::Hand),
            (7, "Culling Drone", a, ObjectDestination::LibraryBottom),
            (8, "Lantern Kami", a, ObjectDestination::LibraryBottom),
            (9, "Lantern Kami", a, ObjectDestination::LibraryBottom),
            (
                10,
                "Island",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                11,
                "Island",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                12,
                "Island",
                a,
                ObjectDestination::Battlefield { controller: a },
            ),
        ] {
            let row = registry
                .coverage
                .cards
                .iter()
                .find(|r| r.required && r.name == name)
                .unwrap();
            cards.push(
                registry
                    .seed(
                        &OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap(),
                        CardId(n),
                        owner,
                        zone,
                    )
                    .unwrap(),
            );
        }
        let seed = GameSeed::with_uniform_life(
            vec![a, b, c],
            a,
            cards,
            [1, 2, 3, 4, 10, 11, 12].into_iter().map(CardId).collect(),
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(87),
            ctx,
            seed,
            TurnStart {
                number: 1,
                step: Step::Untap,
            },
        )
        .unwrap()
        .into_state();
        while state.turn().unwrap().position().step != Step::PrecombatMain {
            state = pass(&state);
        }
        for n in [10, 11, 12] {
            state = act(
                &state,
                a,
                GameCommand::ActivateMana {
                    source: id(&state, n),
                    ability: spellwake_engine::mana::ManaAbilityRef::BasicLand(
                        spellwake_engine::land::BasicLandType::Island,
                    ),
                },
            );
        }
        let zone = |s: &GameState, n| s.execution().window().cards().get(id(s, n)).unwrap().zone();
        state = act(
            &state,
            a,
            GameCommand::CastWithResources {
                spell: id(&state, 6),
                target: None,
                payment: CastingPayment {
                    use_alternative: true,
                    mana: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![ManaSpend {
                            mana_type: ManaType::Blue,
                            amount: 3.into(),
                        }],
                    }),
                    ..Default::default()
                },
            },
        );
        for _ in 0..100 {
            if zone(&state, 5) == ObjectZone::Exile {
                assert_eq!(zone(&state, 3), ObjectZone::Graveyard(b));
                assert_eq!(zone(&state, 4), ObjectZone::Battlefield);
                assert_eq!(zone(&state, 6), ObjectZone::Graveyard(a));
                for n in [7, 8] {
                    assert_eq!(zone(&state, n), ObjectZone::Hand(a));
                }
                return;
            }
            state = if let Some(p) = state.pending_decision() {
                let choice = match p.frame() {
                    RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                        DecisionReply::DeclareAttackers(vec![
                            AttackDeclaration {
                                attacker: id(&state, 1),
                                defender: b,
                                defending_planeswalker: None,
                            },
                            AttackDeclaration {
                                attacker: id(&state, 2),
                                defender: c,
                                defending_planeswalker: None,
                            },
                        ])
                    }
                    RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                        DecisionReply::DeclareBlockers(vec![])
                    }
                    RuleFrame::ResolutionSelect(f) => {
                        assert_eq!(p.chooser(), b);
                        assert_eq!(f.domain(), &[id(&state, 3)]);
                        DecisionReply::SelectResolution(f.domain().to_vec())
                    }
                    other => panic!("unexpected decision {other:?}"),
                };
                reply(&state, choice)
            } else {
                pass(&state)
            };
        }
        panic!("ingest did not resolve");
    }
    #[test]
    #[ignore = "requires pinned Oracle and creature vocabulary inputs"]
    fn cuboid_colony_and_hungry_graffalon_increment_from_a_paid_creature_cast() {
        let (_, vocab, registry) = current_catalogue();
        let (a, b) = (SeatId(0), SeatId(1));
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let ctx = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry16".into(),
            "increment".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let mut cards = vec![];
        for (n, name, zone) in [
            (
                1,
                "Cuboid Colony",
                ObjectDestination::Battlefield { controller: a },
            ),
            (
                2,
                "Hungry Graffalon",
                ObjectDestination::Battlefield { controller: a },
            ),
            (3, "Hungry Graffalon", ObjectDestination::Hand),
        ] {
            let row = registry
                .coverage
                .cards
                .iter()
                .find(|r| r.required && r.name == name)
                .unwrap();
            cards.push(
                registry
                    .seed(
                        &OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap(),
                        CardId(n),
                        a,
                        zone,
                    )
                    .unwrap(),
            );
        }
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(1), CardId(2)],
            vec![FloatingMana {
                player: a,
                mana_type: ManaType::Green,
                amount: 4.into(),
            }],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        );
        let game = GameState::initialize_scheduled(
            GameId(88),
            ctx,
            seed,
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let mut state = act(
            &game,
            a,
            GameCommand::CastUntargeted {
                spell: id(&game, 3),
                payment: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: vec![ManaSpend {
                        mana_type: ManaType::Green,
                        amount: 4.into(),
                    }],
                }),
            },
        );
        let RuleFrame::Triggers(order) = state.pending_decision().unwrap().frame() else {
            panic!("two increment triggers")
        };
        assert_eq!(order.instances().len(), 2);
        state = reply(
            &state,
            DecisionReply::OrderTriggers(order.instances().to_vec()),
        );
        for _ in 0..3 {
            state = resolve(&state);
        }
        assert_eq!(count(&state, 1), 1.into());
        assert_eq!(count(&state, 2), 1.into());
        assert_eq!(count(&state, 3), 0.into());
    }
}
