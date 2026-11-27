use super::*;
use spellwake_engine::{
    combat::CombatDecision,
    effects::LibraryTopReply,
    game::{DecisionReply, RuleFrame},
    mana::{CastingPayment, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput},
    program::{BodyTarget, ExileAction, ProgramKind},
    targets::TargetRef,
};

fn oid(game: &GameState, n: u64) -> spellwake_engine::object_identity::ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
pub(super) fn until(mut state: GameState, done: impl Fn(&GameState) -> bool) -> GameState {
    for _ in 0..750 {
        if done(&state) {
            return state;
        }
        if let Some(p) = state.pending_decision() {
            let choice = match p.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                RuleFrame::LibraryTop(frame) => DecisionReply::LibraryTop(LibraryTopReply::Scry {
                    top_first: frame.domain_top_first().to_vec(),
                    bottom_top_first: vec![],
                }),
                RuleFrame::Checks(spellwake_engine::checks::CheckDecision::CleanupDiscard(
                    frame,
                )) => DecisionReply::Discard(
                    frame
                        .domain()
                        .iter()
                        .rev()
                        .take(frame.required())
                        .copied()
                        .collect(),
                ),
                RuleFrame::TimestampOrder(frame) => DecisionReply::TimestampOrder(
                    frame.members().iter().map(|m| m.member).collect(),
                ),
                RuleFrame::ResolutionSelect(frame) => DecisionReply::SelectResolution(
                    frame
                        .domain()
                        .iter()
                        .take(frame.required())
                        .copied()
                        .collect(),
                ),
                RuleFrame::OptionalEffect(frame)
                    if frame.exile_cast().is_some_and(|offer| {
                        offer.ability == spellwake_engine::effects::ExileCastAbility::Cascade
                    }) =>
                {
                    DecisionReply::OptionalEffect(
                        spellwake_engine::effects::OptionalEffectChoice::Decline,
                    )
                }
                frame => panic!("unexpected {frame:?}"),
            };
            state = advance(
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
                panic!("expected priority");
            };
            state = advance(&state, holder, GameCommand::Pass);
        }
    }
    panic!("failed to reach game boundary")
}

#[test]
#[ignore = "requires the dated Oracle source and matching vocabulary"]
fn real_foretell_and_suspend_cards_pay_and_cast_from_actual_exile() {
    let (inventory, vocab, registry) = fourth_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let mut tested = BTreeSet::new();
    for row in inventory
        .cards
        .iter()
        .filter(|r| r.required_by_base_formats)
    {
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        let Some(bound) = registry.resolve(&id) else {
            continue;
        };
        let Some(action) = &bound.instruction.exile_action else {
            continue;
        };
        let raw = without_reminder(row.faces[0].oracle_text.as_deref().unwrap()).unwrap();
        let (special, cost, time) = match action {
            ExileAction::Foretell(cost) => {
                let price = raw
                    .lines()
                    .map(str::trim)
                    .find_map(|s| s.strip_prefix("Foretell "))
                    .unwrap();
                assert_eq!(cost, &fixed_cost(price).unwrap(), "{}", row.name);
                (ManaCost::generic(2.into()), cost.clone(), None)
            }
            ExileAction::Suspend { counters, price } => {
                let (n, symbols) = raw
                    .lines()
                    .map(str::trim)
                    .find_map(|s| s.strip_prefix("Suspend "))
                    .unwrap()
                    .split_once('—')
                    .unwrap();
                assert_eq!(*counters, n.parse::<u16>().unwrap(), "{}", row.name);
                assert_eq!(price, &fixed_cost(symbols).unwrap(), "{}", row.name);
                (price.clone(), ManaCost::free(), Some(*counters))
            }
        };
        let funding = |cost: &ManaCost| {
            let mut pool = cost.required.clone();
            if cost.generic.is_positive() {
                let current = pool.entry(ManaType::Colourless).or_insert_with(|| 0.into());
                *current = current.plus(&cost.generic);
            }
            pool
        };
        let mana = funding(&special)
            .into_iter()
            .map(|(mana_type, amount)| FloatingMana {
                player: a,
                mana_type,
                amount,
            })
            .collect();
        let payment = |cost: &ManaCost| {
            Some(ManaPayment {
                from_lots: Default::default(),
                spend: funding(cost)
                    .into_iter()
                    .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                    .collect(),
            })
        };
        let mut cards = vec![
            registry
                .seed(&id, CardId(1), a, ObjectDestination::Hand)
                .unwrap(),
        ];
        let vanilla = Program::creature(None, 1.into(), 30.into(), BTreeSet::new());
        let mut add = |id, owner, destination, program| {
            cards.push(SeedCard {
                card: CardId(id),
                owner,
                destination,
                program,
            })
        };
        add(
            2,
            b,
            ObjectDestination::Battlefield { controller: b },
            vanilla.clone(),
        );
        let rate = funding(&cost);
        let land = Program::land(
            BTreeSet::new(),
            if rate.is_empty() {
                vec![]
            } else {
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed(rate),
                }]
            },
        );
        add(3, a, ObjectDestination::Battlefield { controller: a }, land);
        add(
            4,
            a,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::free()), vec![]),
        );
        for n in 0..time.map_or(5, |count| u64::from(count) + 1) {
            for (offset, owner) in [(10, a), (30, b)] {
                add(
                    offset + n,
                    owner,
                    ObjectDestination::LibraryBottom,
                    vanilla.clone(),
                );
            }
        }
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context = GameContext::new(
            "keyword-gameplay-current".into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "keyword-gameplay-registry15".into(),
            "exile-special-actions".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(2), CardId(3)],
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab.clone(),
            names,
        );
        let mut state = GameState::initialize_scheduled(
            GameId(15),
            context,
            seed,
            TurnStart {
                number: 1,
                step: Step::PostcombatMain,
            },
        )
        .unwrap()
        .into_state();
        let source = oid(&state, 1);
        let command = if time.is_some() {
            GameCommand::Suspend {
                source,
                payment: payment(&special),
            }
        } else {
            GameCommand::Foretell {
                source,
                payment: payment(&special),
            }
        };
        state = advance(&state, a, command);
        let exiled = oid(&state, 1);
        let plan = match &bound.instruction.kind {
            ProgramKind::Spell(s) => s.body.target,
            _ => BodyTarget::None,
        };
        if let Some(n) = time {
            assert_eq!(
                state.execution().time_counters().get(&exiled),
                Some(&i64::from(n).into())
            );
            state = until(
                state,
                |s| matches!(s.pending_decision().map(|p|p.frame()),Some(RuleFrame::OptionalEffect(f)) if f.suspended_cast().is_some()),
            );
            assert_eq!(
                state.turn().unwrap().position().number,
                1 + u64::from(n) * 2,
                "{}",
                row.name
            );
            let p = state.pending_decision().unwrap();
            let target = match plan {
                BodyTarget::None => None,
                BodyTarget::SINGLE_CREATURE => Some(TargetRef::Object(oid(&state, 2))),
                other => panic!("{} target {other:?}", row.name),
            };
            state = advance(
                &state,
                a,
                GameCommand::Reply {
                    decision: p.id(),
                    choice: DecisionReply::SuspendCast {
                        target,
                        payment: CastingPayment::default(),
                    },
                },
            );
        } else {
            assert_eq!(state.execution().foretold_on(exiled), Some(1));
            state = until(state, |s| {
                s.turn().unwrap().position().number == 3
                    && s.turn().unwrap().position().step == Step::PrecombatMain
            });
            if cost != ManaCost::free() {
                state = advance(
                    &state,
                    a,
                    GameCommand::ActivateMana {
                        source: oid(&state, 3),
                        ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
                    },
                );
            }
            let target = match plan {
                BodyTarget::None => None,
                BodyTarget::SINGLE_CREATURE => Some(TargetRef::Object(oid(&state, 2))),
                BodyTarget::Spell {
                    include_abilities: false,
                    ..
                } => {
                    state = advance(
                        &state,
                        a,
                        GameCommand::CastUntargeted {
                            spell: oid(&state, 4),
                            payment: None,
                        },
                    );
                    Some(TargetRef::Stack(
                        state.execution().window().entries()[0].id(),
                    ))
                }
                other => panic!("{} target {other:?}", row.name),
            };
            let spell = oid(&state, 1);
            let command = if let Some(target) = target {
                GameCommand::CastPaid {
                    spell,
                    target,
                    payment: payment(&cost).unwrap(),
                }
            } else {
                GameCommand::CastUntargeted {
                    spell,
                    payment: payment(&cost),
                }
            };
            state = advance(&state, a, command);
        }
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(oid(&state, 1))
                .unwrap()
                .zone(),
            ObjectZone::Stack
        );
        let hand_count = |s: &GameState| {
            s.execution()
                .window()
                .cards()
                .iter()
                .filter(|card| card.zone() == ObjectZone::Hand(a))
                .count()
        };
        let previous_hand = hand_count(&state);
        let expected = if matches!(bound.instruction.kind, ProgramKind::Spell(_)) {
            ObjectZone::Graveyard(a)
        } else {
            ObjectZone::Battlefield
        };
        state = until(state, |s| {
            s.execution()
                .window()
                .cards()
                .get(oid(s, 1))
                .unwrap()
                .zone()
                == expected
        });
        if expected == ObjectZone::Battlefield && time.is_some() {
            assert!(
                state
                    .execution()
                    .current_characteristics(oid(&state, 1))
                    .unwrap()
                    .unwrap()
                    .has_keyword(KeywordAbility::Haste)
            );
        }
        match row.name.as_str() {
            "Heroes Remembered" => assert_eq!(state.execution().life_total(a), Some(&40.into())),
            "Shivan Meteor" => assert_eq!(
                state.execution().battlefield_marked_damage(oid(&state, 2)),
                Some(13.into())
            ),
            "Behold the Multiverse" => assert_eq!(hand_count(&state), previous_hand + 2),
            "Saw It Coming" => assert_eq!(
                state
                    .execution()
                    .window()
                    .cards()
                    .get(oid(&state, 4))
                    .unwrap()
                    .zone(),
                ObjectZone::Graveyard(a)
            ),
            "Mammoth Growth" | "Kaya's Onslaught" => {
                let bonus = if row.name == "Mammoth Growth" { 4 } else { 1 };
                let target = state
                    .execution()
                    .current_characteristics(oid(&state, 2))
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    target.has_keyword(KeywordAbility::DoubleStrike),
                    row.name == "Kaya's Onslaught"
                );
                let actual = target.power_toughness.unwrap().derived;
                assert_eq!(
                    (actual.power, actual.toughness),
                    ((1 + bonus).into(), (30 + bonus).into())
                );
            }
            _ => {}
        }
        tested.insert(row.name.clone());
    }
    for name in [
        "Scorn Effigy",
        "Mammoth Growth",
        "Saw It Coming",
        "Durkwood Baloth",
        "Heroes Remembered",
        "Giant Dustwasp",
    ] {
        assert!(tested.contains(name), "{name}");
    }
    eprintln!(
        "{} real Foretell/Suspend cards: {}",
        tested.len(),
        tested.into_iter().collect::<Vec<_>>().join(", ")
    );
}
