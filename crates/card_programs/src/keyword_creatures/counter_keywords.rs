//! Creature definitions for CR 702.32, 702.63, 702.79 and 702.93.
use spellwake_engine::program::{CounterKeyword as K, TriggeredAbility, TriggeredAbilityId};

pub(super) fn adapt(
    card: &super::InventoryCard,
    vocabulary: &super::CreatureTypeVocabulary,
) -> Option<super::Program> {
    use super::*;
    use spellwake_engine::activated::SourceCost;
    use spellwake_engine::program::TriggerGrouping;

    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let (cost, amount) = lines.next()?.split_once(": Adapt ")?;
    let amount = amount.strip_suffix('.')?.parse::<u16>().ok()?;
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
    program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(0),
        mana_cost: fixed_cost(cost)?,
        source_cost: SourceCost::None,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Adapt(amount)]),
    });
    match lines.next() {
        None => {}
        Some(
            "Whenever one or more +1/+1 counters are put on this creature, it gains double strike until end of turn.",
        ) if lines.next().is_none() => {
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(0),
                condition: spellwake_engine::program::TriggerSpec::counters_changed(
                    spellwake_engine::program::ObjectFilter::self_object(),
                    spellwake_engine::program::CounterTriggerKind::PlusOnePlaced {
                        by_controller_only: false,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                            spellwake_engine::program::EffectSubject::AbilitySource,
                            spellwake_engine::program::ValueExpr::literal(0),
                            spellwake_engine::program::ValueExpr::literal(0),
                            BTreeSet::from([KeywordAbility::DoubleStrike]),
                            BTreeSet::new(),
                        ),
                    ],
                ),
            });
        }
        Some(_) => return None,
    }
    program.validate(vocabulary).ok()?;
    Some(program)
}
pub(super) fn triggers(label: &str, id: TriggeredAbilityId) -> Option<Vec<TriggeredAbility>> {
    let number = |s: &str| {
        (!s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
    };
    let key = match label {
        "persist" => K::Persist,
        "undying" => K::Undying,
        "vanishing" => K::Vanishing(None),
        _ => {
            let (name, n) = label.split_once(' ')?;
            match name {
                "fading" => K::Fading(number(n)?),
                "vanishing" => K::Vanishing(Some(number(n)?)),
                _ => return None,
            }
        }
    };
    let mut abilities = vec![key.ability(id)];
    if matches!(key, K::Vanishing(_)) {
        abilities.push(K::VanishingLastCounter.ability(TriggeredAbilityId(id.0.checked_add(1)?)));
    }
    Some(abilities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        OracleId, TEACHING_SOURCE_SHA256,
        keyword_creatures::{
            CURRENT_KEYWORD_PROFILE,
            tests::{current_catalogue, fourth_catalogue},
        },
    };
    use spellwake_engine::{
        combat::{CombatDecision, CombatRules},
        effects::SeedCard,
        game::{DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
        mana::{FloatingMana, ManaCost, ManaPayment, ManaSpend, ManaType},
        object_identity::{CardId, ObjectDestination, ObjectZone},
        priority::{PriorityPhase, SeatId},
        program::{BodyTarget, ExecutableBody, Program},
        setup::GameSeed,
        stack_window::WindowPhase,
        targets::TargetRef,
        turn::{Step, TurnStart},
    };

    #[test]
    fn numbered_and_unnumbered_vanishing_get_two_distinct_abilities() {
        let id = TriggeredAbilityId(3);
        for (label, upkeep) in [
            ("vanishing", K::Vanishing(None)),
            ("vanishing 0", K::Vanishing(Some(0))),
            ("vanishing 5", K::Vanishing(Some(5))),
        ] {
            assert_eq!(
                triggers(label, id).unwrap(),
                vec![
                    upkeep.ability(id),
                    K::VanishingLastCounter.ability(TriggeredAbilityId(4))
                ]
            );
        }
        for bad in [
            "fading",
            "fading -1",
            "vanishing +3",
            "persist 1",
            "undying 0",
        ] {
            assert!(triggers(bad, id).is_none());
        }
    }

    fn apply(s: &GameState, actor: SeatId, command: GameCommand) -> GameState {
        s.advance(GameInput {
            basis: s.basis().clone(),
            actor,
            command,
        })
        .unwrap()
        .into_state()
    }
    fn settle(mut s: GameState) -> GameState {
        while !s.execution().window().entries().is_empty() {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                s.execution().window().phase()
            else {
                panic!("priority")
            };
            s = apply(&s, holder, GameCommand::Pass);
        }
        s
    }
    fn object(s: &GameState, n: u64) -> spellwake_engine::object_identity::ObjectId {
        s.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    }

    #[test]
    #[ignore = "requires pinned Oracle and creature vocabulary inputs"]
    fn ridgeback_wurm_goblin_and_wolf_apply_their_printed_counter_rules() {
        let (_, vocab, registry) = fourth_catalogue();
        play_counter_keyword_game(registry, vocab);
    }

    #[test]
    #[ignore = "requires pinned Oracle and creature vocabulary inputs"]
    fn skyshroud_ridgeback_fades_only_on_its_controllers_upkeep() {
        let (_, vocab, registry) = current_catalogue();
        play_counter_keyword_game(registry, vocab);
    }

    fn play_counter_keyword_game(
        registry: &crate::Registry,
        vocab: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    ) {
        let (a, b) = (SeatId(0), SeatId(1));
        let mut cards = vec![];
        let mut named = |n, name: &str, owner, zone| {
            let row = registry
                .coverage
                .cards
                .iter()
                .find(|r| r.name == name)
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
        };
        named(1, "Skyshroud Ridgeback", a, ObjectDestination::Hand);
        named(2, "Waning Wurm", a, ObjectDestination::Hand);
        named(
            3,
            "Putrid Goblin",
            a,
            ObjectDestination::Battlefield { controller: a },
        );
        named(
            4,
            "Young Wolf",
            b,
            ObjectDestination::Battlefield { controller: b },
        );
        for n in 10..18 {
            named(
                n,
                "Young Wolf",
                if n % 2 == 0 { a } else { b },
                ObjectDestination::LibraryBottom,
            );
        }
        for n in [5, 6] {
            cards.push(SeedCard {
                card: CardId(n),
                owner: a,
                destination: ObjectDestination::Hand,
                program: Program::instant(
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
                ),
            });
        }
        let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            format!(
                "keyword-gameplay-registry{}",
                registry.coverage().registry_version
            ),
            "four-counter-keywords".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let pool = [(ManaType::Green, 1i64), (ManaType::Black, 4)];
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(3), CardId(4)],
            pool.into_iter()
                .map(|(mana_type, n)| FloatingMana {
                    player: a,
                    mana_type,
                    amount: n.into(),
                })
                .collect(),
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(79),
            context,
            seed,
            TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        for (n, (mana_type, amount)) in [(1, pool[0]), (2, pool[1])] {
            state = settle(apply(
                &state,
                a,
                GameCommand::CastUntargeted {
                    spell: object(&state, n),
                    payment: Some(ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![ManaSpend {
                            mana_type,
                            amount: amount.into(),
                        }],
                    }),
                },
            ));
        }
        assert_eq!(
            state.execution().fade_counters()[&object(&state, 1)],
            2.into()
        );
        assert_eq!(
            state.execution().time_counters()[&object(&state, 2)],
            2.into()
        );
        for (spell, target) in [(5, 3), (6, 4)] {
            state = settle(apply(
                &state,
                a,
                GameCommand::Cast {
                    spell: object(&state, spell),
                    target: TargetRef::Object(object(&state, target)),
                },
            ));
        }
        assert_eq!(
            state
                .execution()
                .power_toughness_counters(object(&state, 3))
                .unwrap()
                .minus_one,
            1.into()
        );
        assert_eq!(
            state
                .execution()
                .power_toughness_counters(object(&state, 4))
                .unwrap()
                .plus_one,
            1.into()
        );
        let ridgeback = object(&state, 1);
        let mut checked_draw_steps = std::collections::BTreeSet::new();
        for _ in 0..320 {
            let zone = |n| {
                state
                    .execution()
                    .window()
                    .cards()
                    .get(object(&state, n))
                    .unwrap()
                    .zone()
            };
            let turn = state.turn().unwrap().position();
            if turn.step == Step::Draw && (2..=6).contains(&turn.number) {
                let expected = match turn.number {
                    2 => 2,
                    3 | 4 => 1,
                    _ => 0,
                };
                assert_eq!(zone(1), ObjectZone::Battlefield);
                assert_eq!(
                    state
                        .execution()
                        .fade_counters()
                        .get(&ridgeback)
                        .cloned()
                        .unwrap_or_else(|| 0.into()),
                    expected.into(),
                );
                checked_draw_steps.insert(turn.number);
            }
            if zone(1) == ObjectZone::Graveyard(a) {
                assert_eq!(checked_draw_steps, (2..=6).collect());
                assert_eq!(zone(2), ObjectZone::Graveyard(a));
                assert_eq!(state.turn().unwrap().position().number, 7);
                return;
            }
            if let Some(p) = state.pending_decision() {
                let choice = match p.frame() {
                    RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                        DecisionReply::DeclareAttackers(vec![])
                    }
                    RuleFrame::Triggers(order) => {
                        DecisionReply::OrderTriggers(order.instances().to_vec())
                    }
                    f => panic!("unexpected {f:?}"),
                };
                state = apply(
                    &state,
                    p.chooser(),
                    GameCommand::Reply {
                        decision: p.id(),
                        choice,
                    },
                );
            } else {
                let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                    state.execution().window().phase()
                else {
                    panic!("priority")
                };
                state = apply(&state, holder, GameCommand::Pass);
            }
        }
        panic!("did not reach final upkeep");
    }
}
