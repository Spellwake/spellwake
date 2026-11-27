//! Source-frozen selected public-root producers. Never read expected/reference results.
//! These ignored methods require explicit execution after the TT12 protocol freeze.
use super::reference_trace::Trace;
use super::*;
use crate::{
    combat::CombatRules,
    effects::SeedCard,
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId},
    program::{
        Instruction, PlayerRelation, Program, TriggerGrouping, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId,
    },
    stack_window::{StackEntryId, StackKind},
    targets::TargetRef,
    test_support::{creature_types, packet::AliasContext, projection},
};
use serde_json::json;
use std::{collections::BTreeSet, fs::OpenOptions, io::Write, sync::Arc};
const P0: SeatId = SeatId(0);
const P1: SeatId = SeatId(1);
const P2: SeatId = SeatId(2);
fn object(state: &GameState, id: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}
fn gain(amount: i64) -> Instruction {
    crate::program::Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::gain(
        crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
        crate::program::ValueExpr::Literal(amount.into()),
    )))
}
fn target_gain(amount: i64) -> Instruction {
    crate::program::Instruction::AdjustLife(Box::new(crate::toolbox::AdjustLifeSpec::gain(
        crate::toolbox::PlayerSelector::Resolving(crate::program::ResolvingPlayer::TargetPlayer),
        crate::program::ValueExpr::Literal(amount.into()),
    )))
}
fn original_spell() -> Program {
    Program::instant(
        ManaCost::one(ManaType::Blue),
        ExecutableBody::new(BodyTarget::None, vec![gain(2)]),
    )
}
fn ability(
    id: u64,
    condition: TriggerSpec,
    target: BodyTarget,
    body: Vec<Instruction>,
) -> TriggeredAbility {
    TriggeredAbility {
        id: TriggeredAbilityId(id),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(target, body),
    }
}
fn watcher_condition() -> TriggerSpec {
    crate::program::TriggerSpec::spell_cast(
        PlayerRelation::AnyParticipant,
        crate::program::SpellFilter::any(),
    )
}
fn seed(case: &str) -> (NativeCandidate, AliasContext) {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let three = matches!(case, "TR3" | "TR6");
    let players = if three {
        vec![P0, P1, P2]
    } else {
        vec![P0, P1]
    };
    let caster = if three { P1 } else { P0 };
    let mut source = if case == "TR2" {
        Program::artifact(Some(ManaCost::one(ManaType::Blue)), vec![])
    } else {
        original_spell()
    };
    if !three {
        let (target, mut body) = if case == "TR4" {
            (
                BodyTarget::single_spell(crate::program::SpellFilter::any()),
                vec![Instruction::StackOp(Box::new(
                    crate::toolbox::StackOpSpec::counter_targeted(
                        crate::toolbox::CounterDestination::Graveyard,
                    ),
                ))],
            )
        } else {
            (
                BodyTarget::SINGLE_CREATURE,
                vec![crate::program::Instruction::ZoneOp(Box::new(
                    crate::toolbox::ZoneOpSpec::move_all(
                        crate::toolbox::ObjectQuery::Targets,
                        crate::toolbox::ZoneDest::Hand,
                    ),
                ))],
            )
        };
        if matches!(case, "TR1" | "TR5") {
            body.push(gain(1));
        }
        source.triggered.push(ability(
            1,
            crate::program::TriggerSpec::self_cast(),
            target,
            body,
        ));
    }
    let mut cards = vec![SeedCard {
        card: CardId(1),
        owner: caster,
        destination: ObjectDestination::Hand,
        program: source,
    }];
    let mut names = vec![(CardId(1), "S".into())];
    if matches!(case, "TR1" | "TR5") {
        cards.push(SeedCard {
            card: CardId(2),
            owner: P1,
            destination: ObjectDestination::Battlefield { controller: P1 },
            program: Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        });
        names.push((CardId(2), "C".into()));
    }
    if case == "TR5" {
        cards.push(SeedCard {
            card: CardId(3),
            owner: P1,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::one(ManaType::Blue),
                ExecutableBody::new(
                    BodyTarget::SINGLE_CREATURE,
                    vec![crate::program::Instruction::ZoneOp(Box::new(
                        crate::toolbox::ZoneOpSpec::move_all(
                            crate::toolbox::ObjectQuery::Targets,
                            crate::toolbox::ZoneDest::Hand,
                        ),
                    ))],
                ),
            ),
        });
        names.push((CardId(3), "R".into()));
    }
    if case == "TR3" {
        let mut watcher = Program::artifact(None, vec![]);
        watcher.triggered.push(ability(
            1,
            watcher_condition(),
            BodyTarget::SINGLE_PLAYER,
            vec![target_gain(2), gain(1)],
        ));
        cards.push(SeedCard {
            card: CardId(10),
            owner: P0,
            destination: ObjectDestination::Battlefield { controller: P0 },
            program: watcher,
        });
        names.push((CardId(10), "W0".into()));
    }
    if case == "TR6" {
        for (index, player) in players.iter().enumerate() {
            let id = 10 + index as u64;
            let mut watcher = Program::artifact(None, vec![]);
            watcher.triggered.push(ability(
                1,
                watcher_condition(),
                BodyTarget::None,
                vec![gain(1)],
            ));
            watcher.triggered.push(ability(
                2,
                watcher_condition(),
                BodyTarget::SINGLE_PLAYER,
                vec![target_gain(2)],
            ));
            cards.push(SeedCard {
                card: CardId(id),
                owner: *player,
                destination: ObjectDestination::Battlefield {
                    controller: *player,
                },
                program: watcher,
            });
            names.push((CardId(id), format!("W{index}")));
        }
    }
    for player in &players {
        let island = 1000 + u64::from(player.0);
        cards.push(SeedCard {
            card: CardId(island),
            owner: *player,
            destination: ObjectDestination::Battlefield {
                controller: *player,
            },
            program: Program::land(BTreeSet::from([BasicLandType::Island.land_type()]), vec![]),
        });
        names.push((CardId(island), format!("I{}", player.0)));
        for n in 0..8 {
            let id = 2000 + u64::from(player.0) * 10 + n;
            cards.push(SeedCard {
                card: CardId(id),
                owner: *player,
                destination: ObjectDestination::LibraryBottom,
                program: Program::artifact(None, vec![]),
            });
            names.push((CardId(id), format!("P{} Library {n}", player.0)));
        }
    }
    let order = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    let seed = GameSeed::with_uniform_life(
        players,
        caster,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let context = GameContext::new(
        "targeted-reference".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "frozen-literal-partials".into(),
        "targeted-native-v1".into(),
        "prepared-main-real-Island-payment".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    let aliases = AliasContext::from_card_aliases(&names).unwrap();
    let candidate = GameState::initialize_scheduled(
        GameId(6043),
        context,
        seed,
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap();
    (candidate, aliases)
}
fn paid(
    trace: &mut Trace,
    state: &GameState,
    actor: SeatId,
    id: u64,
    target: Option<TargetRef>,
) -> NativeCandidate {
    let funded = trace.step(
        state,
        actor,
        GameCommand::ActivateMana {
            source: object(state, 1000 + u64::from(actor.0)),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        },
    );
    let payment = ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type: ManaType::Blue,
            amount: 1.into(),
        }],
    };
    let spell = object(funded.state(), id);
    let command = if let Some(target) = target {
        GameCommand::CastPaid {
            spell,
            target,
            payment,
        }
    } else {
        GameCommand::CastUntargeted {
            spell,
            payment: Some(payment),
        }
    };
    let result = trace.step(funded.state(), actor, command);
    assert!(result.mana_payment().is_some());
    assert_eq!(
        result.state().execution().mana(actor, ManaType::Blue),
        0.into()
    );
    result
}
fn choose(trace: &mut Trace, state: &GameState, target: TargetRef) -> NativeCandidate {
    let pending = state.pending_decision().unwrap();
    let RuleFrame::TriggerTarget(frame) = pending.frame() else {
        panic!("expected actual target placement")
    };
    assert!(frame.domain().contains(&target));
    trace.step(
        state,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::TriggerTarget(target),
        },
    )
}
fn resolve(trace: &mut Trace, state: &GameState) -> GameState {
    let crate::stack_window::WindowPhase::Priority(PriorityPhase::Ready { holder }) =
        state.execution().window().phase()
    else {
        panic!("normal priority required")
    };
    let players = state.execution().window().participants();
    let first = players.iter().position(|seat| *seat == holder).unwrap();
    let mut current = state.clone();
    for n in 0..players.len() {
        current = trace
            .step(
                &current,
                players[(first + n) % players.len()],
                GameCommand::Pass,
            )
            .into_state();
    }
    current
}
fn original_entry(state: &GameState) -> StackEntryId {
    state
        .execution()
        .window()
        .entries()
        .iter()
        .find(|entry| entry.kind() == &StackKind::CardSpell(object(state, 1)))
        .unwrap()
        .id()
}
fn run(case: &str) {
    let output = std::env::var_os("SPELLWAKE_TARGETED_TRIGGER_EVIDENCE_DIR")
        .expect("explicit source-frozen selected evidence directory required");
    let (initialized, aliases) = seed(case);
    let initialization = projection::candidate(&initialized, None, &aliases).unwrap();
    let mut trace = Trace {
        aliases,
        rows: vec![],
    };
    let initial = initialized.into_state();
    let caster = if matches!(case, "TR3" | "TR6") {
        P1
    } else {
        P0
    };
    let creature = matches!(case, "TR1" | "TR5").then(|| object(&initial, 2));
    let cast = paid(&mut trace, &initial, caster, 1, None);
    let mut state = cast.into_state();
    if case == "TR6" {
        for (player, definitions, target) in [(P1, [2, 1], P2), (P2, [1, 2], P0), (P0, [2, 1], P1)]
        {
            let pending = state.pending_decision().unwrap();
            let RuleFrame::Triggers(frame) = pending.frame() else {
                panic!("actual APNAP order")
            };
            assert_eq!(frame.chooser(), player);
            let order = definitions
                .into_iter()
                .map(|definition| {
                    state
                        .execution()
                        .waiting_triggers()
                        .find(|waiting| {
                            waiting.controller == player
                                && waiting.definition == TriggeredAbilityId(definition)
                        })
                        .unwrap()
                        .id
                })
                .collect();
            state = trace
                .step(
                    &state,
                    player,
                    GameCommand::Reply {
                        decision: pending.id(),
                        choice: DecisionReply::OrderTriggers(order),
                    },
                )
                .into_state();
            state = choose(&mut trace, &state, TargetRef::Player(target)).into_state();
        }
    } else if case != "TR2" {
        let target = match case {
            "TR1" | "TR5" => TargetRef::Object(creature.unwrap()),
            "TR3" => TargetRef::Player(P2),
            "TR4" => TargetRef::Stack(original_entry(&state)),
            _ => panic!("unknown selected case"),
        };
        state = choose(&mut trace, &state, target).into_state();
    }
    if case == "TR5" {
        state = trace.step(&state, P0, GameCommand::Pass).into_state();
        state = paid(
            &mut trace,
            &state,
            P1,
            3,
            Some(TargetRef::Object(creature.unwrap())),
        )
        .into_state();
    }
    for _ in 0..12 {
        if state.execution().window().entries().is_empty() {
            break;
        }
        assert!(state.pending_decision().is_none());
        state = resolve(&mut trace, &state);
    }
    assert!(state.execution().window().entries().is_empty());
    assert!(state.pending_decision().is_none());
    assert_eq!(state.execution().waiting_triggers().count(), 0);
    let life: Vec<_> = match case {
        "TR1" => vec![(P0, 23), (P1, 20)],
        "TR3" => vec![(P0, 21), (P1, 22), (P2, 22)],
        "TR5" => vec![(P0, 22), (P1, 20)],
        "TR6" => vec![(P0, 23), (P1, 25), (P2, 23)],
        _ => vec![(P0, 20), (P1, 20)],
    };
    for (player, total) in life {
        assert_eq!(state.execution().life_total(player), Some(&total.into()));
    }
    let report = json!({"schema_version":crate::test_support::current_profile::REPORT_SCHEMA_VERSION,"record_codec":"spellwake.current-native-qualification.v2","selected_protocol":"targeted-triggers-v1","scenario":case,"source_kind":"actual-public-root-native-transitions","reads_expected_or_reference":false,"initialization":initialization,"rows":trace.rows,"final":projection::state(&state,&trace.aliases).unwrap(),"scope":"Prepared main/libraries and real Island activation/explicit payment. Literal partial definitions, privileged full projections, actual receipts. No complete card/format/host/privacy/replay/runtime/capacity qualification."});
    let path = std::path::Path::new(&output).join(format!("native-{case}.json"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    serde_json::to_writer(&mut file, &report).unwrap();
    file.write_all(b"\n").unwrap();
}
#[test]
#[ignore = "explicit TT12 source-frozen selected native producer"]
fn required_creature() {
    run("TR1")
}
#[test]
#[ignore = "explicit TT12 source-frozen selected native producer"]
fn no_legal_creature() {
    run("TR2")
}
#[test]
#[ignore = "explicit TT12 source-frozen selected native producer"]
fn distinct_player_roles() {
    run("TR3")
}
#[test]
#[ignore = "explicit TT12 source-frozen selected native producer"]
fn causing_spell() {
    run("TR4")
}
#[test]
#[ignore = "explicit TT12 source-frozen selected native producer"]
fn target_lost_before_resolution() {
    run("TR5")
}
#[test]
#[ignore = "explicit TT12 source-frozen selected native producer"]
fn mixed_nonfirst_apnap() {
    run("TR6")
}
