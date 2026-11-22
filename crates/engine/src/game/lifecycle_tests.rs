//! Prepared lifecycle units and finite real root inputs, separate from full games.
#[path = "selection_scheduler_tests.rs"]
mod selection_scheduler_tests;
use super::*;
use crate::{
    effects::{Program, SeedCard},
    mana::{FloatingMana, ManaAbility, ManaAbilityId, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::{
        EventCause, OpeningMovePhase, OpeningShufflePhase, RulesEvent, RulesOccurrence,
        RulesRecord, ZoneReason,
    },
    program::{
        BodyTarget, ExecutableBody, Instruction, PlayerRelation, TriggerSpec, TriggeredAbility,
        TriggeredAbilityId,
    },
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const C: SeatId = SeatId(2);

fn context() -> GameContext {
    GameContext::new(
        "lifecycle-units".into(),
        "cr-20260925".into(),
        "literal-fixtures".into(),
        "ordinary-occurrences".into(),
        "unrestricted-ffa".into(),
        crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr-20260925").id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn land(kind: ManaType) -> Program {
    Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(BTreeMap::from([(kind, 1.into())])),
        }],
    )
}
fn libraries(seats: &[SeatId]) -> Vec<SeedCard> {
    seats
        .iter()
        .flat_map(|seat| {
            (0..8).map(move |n| SeedCard {
                card: CardId(1000 + u64::from(seat.0) * 100 + n),
                owner: *seat,
                destination: ObjectDestination::LibraryBottom,
                program: land(ManaType::White),
            })
        })
        .collect()
}
fn observer(id: u64, owner: SeatId, condition: TriggerSpec) -> SeedCard {
    let mut program = Program::artifact(None, vec![]);
    program.triggered.push(TriggeredAbility {
        grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
        id: TriggeredAbilityId(1),
        condition,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: crate::program::ValueExpr::Literal(1.into()),
            }],
        ),
    });
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller: owner },
        program,
    }
}
fn scheduled(
    seats: Vec<SeatId>,
    active: SeatId,
    step: Step,
    cards: Vec<SeedCard>,
    mana: Vec<FloatingMana>,
) -> NativeCandidate {
    GameState::initialize_scheduled(
        GameId(1001),
        context(),
        {
            // Fixture premise: seeded BF timestamps follow this authored card sequence.
            let initial_battlefield_order = cards
                .iter()
                .filter(|card| {
                    matches!(
                        card.destination,
                        crate::object_identity::ObjectDestination::Battlefield { .. }
                    )
                })
                .map(|card| card.card)
                .collect();
            GameSeed::with_uniform_life(
                seats,
                active,
                cards,
                initial_battlefield_order,
                mana,
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr-20260925"),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        TurnStart { number: 1, step },
    )
    .unwrap()
}
fn accept(
    state: &mut GameState,
    actor: SeatId,
    command: GameCommand,
    rows: &mut Vec<RulesRecord>,
) -> NativeCandidate {
    let value = state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        })
        .unwrap();
    rows.extend(value.rules_journal().records.clone());
    *state = value.state().clone();
    value
}
fn round(state: &mut GameState, actors: &[SeatId], rows: &mut Vec<RulesRecord>) {
    for &actor in actors {
        accept(state, actor, GameCommand::Pass, rows);
    }
}
fn reply(
    state: &mut GameState,
    actor: SeatId,
    choice: DecisionReply,
    rows: &mut Vec<RulesRecord>,
) -> NativeCandidate {
    let decision = state.pending_decision().unwrap().id();
    accept(state, actor, GameCommand::Reply { decision, choice }, rows)
}
fn events(rows: &[RulesRecord]) -> Vec<&RulesEvent> {
    rows.iter()
        .filter_map(|row| match row {
            RulesRecord::Event(value) => Some(value),
            _ => None,
        })
        .collect()
}
fn hand(state: &GameState, owner: SeatId) -> Vec<ObjectId> {
    state
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|card| card.zone() == ObjectZone::Hand(owner))
        .map(|card| card.id())
        .collect()
}

#[test]
fn actual_upkeep_entry_places_two_own_staff_bodies_without_prepared_untap_event() {
    let mut cards = libraries(&[A, B]);
    for (id, owner) in [(1, A), (2, A), (3, B)] {
        cards.push(observer(
            id,
            owner,
            crate::program::TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller),
        ));
    }
    let initial = scheduled(vec![A, B], A, Step::Untap, cards, vec![]);
    let mut rows = initial.rules_journal().records.clone();
    let mut state = initial.into_state();
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    let RuleFrame::Triggers(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.chooser(), A);
    let ordered = frame.instances().iter().rev().copied().collect();
    reply(
        &mut state,
        A,
        DecisionReply::OrderTriggers(ordered),
        &mut rows,
    );
    assert_eq!(state.execution().window().entries().len(), 2);
    round(&mut state, &[A, B], &mut rows);
    round(&mut state, &[A, B], &mut rows);
    assert_eq!(hand(&state, A).len(), 2);
    assert_eq!(hand(&state, B).len(), 0);
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    let steps = events(&rows)
        .into_iter()
        .filter_map(|event| match event.cause {
            EventCause::StepBegan { position } => Some(position.step),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(steps, vec![Step::Upkeep]);
}

#[test]
fn waiting_only_cleanup_enters_priority_once_and_real_repeat_has_a_new_iteration() {
    let mut cards = libraries(&[A, B]);
    cards.push(observer(
        1,
        A,
        crate::program::TriggerSpec::step_began(Step::Cleanup, PlayerRelation::Controller),
    ));
    let initial = scheduled(vec![A, B], A, Step::Untap, cards, vec![]);
    let mut rows = initial.rules_journal().records.clone();
    let mut state = initial.into_state();
    for _ in 0..7 {
        round(&mut state, &[A, B], &mut rows);
    }
    assert_eq!(state.turn().unwrap().position().step, Step::Cleanup);
    assert_eq!(state.turn().unwrap().position().cleanup_iteration, 0);
    assert_eq!(state.execution().window().entries().len(), 1);
    assert!(state.pending_decision().is_none());
    round(&mut state, &[A, B], &mut rows);
    assert_eq!(hand(&state, A).len(), 1);
    round(&mut state, &[A, B], &mut rows);
    assert_eq!(state.turn().unwrap().position().cleanup_iteration, 1);
    assert_eq!(state.execution().window().entries().len(), 1);
    let cleanups = events(&rows)
        .into_iter()
        .filter_map(|event| match event.cause {
            EventCause::StepBegan { position } if position.step == Step::Cleanup => {
                Some((event.id, position.cleanup_iteration))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        cleanups.iter().map(|row| row.1).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_ne!(cleanups[0].0, cleanups[1].0);
    assert!(events(&rows).iter().all(|event| !matches!(event.cause, EventCause::StepBegan { position } if position.step == Step::Draw)));
}

#[test]
fn discard_draw_does_not_repeat_cleanup_work_or_step_entry_in_the_same_iteration() {
    let mut cards = libraries(&[A, B]);
    cards.push(observer(
        1,
        A,
        crate::program::TriggerSpec::player_action(
            PlayerRelation::Controller,
            crate::program::PlayerTriggerAction::Discarded,
        ),
    ));
    for n in 0..8 {
        cards.push(SeedCard {
            card: CardId(20 + n),
            owner: A,
            destination: ObjectDestination::Hand,
            program: land(ManaType::White),
        });
    }
    let initial = scheduled(vec![A, B], A, Step::Untap, cards, vec![]);
    let mut rows = initial.rules_journal().records.clone();
    let mut state = initial.into_state();
    for _ in 0..7 {
        round(&mut state, &[A, B], &mut rows);
    }
    let discarded = hand(&state, A)[7];
    reply(
        &mut state,
        A,
        DecisionReply::Discard(vec![discarded]),
        &mut rows,
    );
    assert_eq!(state.execution().window().entries().len(), 1);
    round(&mut state, &[A, B], &mut rows);
    assert_eq!(hand(&state, A).len(), 8);
    assert!(state.pending_decision().is_none());
    assert_eq!(state.turn().unwrap().position().cleanup_iteration, 0);
    assert_eq!(events(&rows).iter().filter(|event| matches!(event.cause, EventCause::StepBegan { position } if position.step == Step::Cleanup)).count(), 1);
    let discards = events(&rows).iter().flat_map(|event| &event.occurrences).filter(|row| matches!(&row.value,
        RulesOccurrence::ZoneChange(change) if matches!(change.reason, ZoneReason::Discard { player: A }))).count();
    assert_eq!(discards, 1);
}

fn prepared_pools() -> Vec<FloatingMana> {
    [
        (C, ManaType::Red, 1),
        (C, ManaType::Blue, 2),
        (B, ManaType::Green, 3),
        (B, ManaType::White, 1),
        (A, ManaType::Black, 0),
        (A, ManaType::Colourless, 0),
    ]
    .into_iter()
    .map(|(player, mana_type, amount)| FloatingMana {
        player,
        mana_type,
        amount: amount.into(),
    })
    .collect()
}

#[test]
fn prepared_cleanup_rotation_clears_one_shared_event_in_old_active_apnap() {
    // Disclosed prepared resources: ordinary End would already have emptied these pools.
    let initial = scheduled(
        vec![B, C, A],
        C,
        Step::Cleanup,
        libraries(&[B, C, A]),
        prepared_pools(),
    );
    let values = events(&initial.rules_journal().records);
    let pools = values
        .iter()
        .filter(|event| matches!(event.cause, EventCause::ManaPoolsEmptied { .. }))
        .collect::<Vec<_>>();
    assert_eq!(pools.len(), 1);
    assert_eq!(
        pools[0]
            .occurrences
            .iter()
            .map(|row| row.id.index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    let losses = pools[0]
        .occurrences
        .iter()
        .map(|row| match &row.value {
            RulesOccurrence::ManaPoolEmptied { player, previous } => (*player, previous.clone()),
            _ => panic!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        losses,
        vec![
            (
                C,
                BTreeMap::from([(ManaType::Blue, 2.into()), (ManaType::Red, 1.into())])
            ),
            (
                B,
                BTreeMap::from([(ManaType::White, 1.into()), (ManaType::Green, 3.into())])
            )
        ]
    );
    let state = initial.state();
    assert_eq!(state.execution().window().active_player(), A);
    assert_eq!(state.turn().unwrap().position().number, 2);
    for player in [A, B, C] {
        for kind in ManaType::ALL {
            assert_eq!(state.execution().mana(player, kind), 0.into());
        }
    }
    assert!(matches!(
        values[0].cause,
        EventCause::ManaPoolsEmptied { .. }
    ));
    let steps = values
        .iter()
        .filter_map(|event| match event.cause {
            EventCause::StepBegan { position } => Some(position.step),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(steps, vec![Step::Untap, Step::Upkeep]);
}

#[test]
fn same_cleanup_priority_keeps_resources_until_the_real_repeated_step() {
    let mut cards = libraries(&[B, C, A]);
    cards.push(observer(
        1,
        C,
        crate::program::TriggerSpec::player_action(
            PlayerRelation::Controller,
            crate::program::PlayerTriggerAction::Discarded,
        ),
    ));
    for n in 0..8 {
        cards.push(SeedCard {
            card: CardId(20 + n),
            owner: C,
            destination: ObjectDestination::Hand,
            program: land(ManaType::White),
        });
    }
    let initial = scheduled(vec![B, C, A], C, Step::Cleanup, cards, prepared_pools());
    let mut rows = initial.rules_journal().records.clone();
    let mut state = initial.into_state();
    let selected = hand(&state, C)[7];
    reply(
        &mut state,
        C,
        DecisionReply::Discard(vec![selected]),
        &mut rows,
    );
    assert_eq!(state.execution().mana(C, ManaType::Blue), 2.into());
    assert!(events(&rows).iter().all(|event| !matches!(
        event.cause,
        EventCause::ManaPoolsEmptied { .. } | EventCause::StepBegan { .. }
    )));
    round(&mut state, &[C, A, B], &mut rows);
    assert_eq!(state.execution().mana(C, ManaType::Blue), 2.into());
    round(&mut state, &[C, A, B], &mut rows);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Checks(_)
    ));
    assert_eq!(state.turn().unwrap().position().cleanup_iteration, 1);
    assert_eq!(
        events(&rows)
            .iter()
            .filter(|event| matches!(event.cause, EventCause::ManaPoolsEmptied { .. }))
            .count(),
        1
    );
    assert_eq!(events(&rows).iter().filter(|event| matches!(event.cause, EventCause::StepBegan { position } if position.step == Step::Cleanup && position.cleanup_iteration == 1)).count(), 1);
}

#[test]
fn actual_empty_context_combat_end_precedes_pool_loss_and_postcombat_entry() {
    let mut cards = libraries(&[A, B]);
    cards.push(SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: land(ManaType::Green),
    });
    let initial = scheduled(vec![A, B], A, Step::EndCombat, cards, vec![]);
    let mut rows = Vec::new();
    let mut state = initial.into_state();
    let source = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    accept(
        &mut state,
        A,
        GameCommand::ActivateMana {
            source,
            ability: crate::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
        &mut rows,
    );
    rows.clear();
    round(&mut state, &[A, B], &mut rows);
    let values = events(&rows);
    assert_eq!(values.len(), 3);
    assert!(matches!(values[0].cause, EventCause::CombatEnded { .. }));
    assert!(matches!(
        values[0].occurrences[0].value,
        RulesOccurrence::CombatEnded { previous: None }
    ));
    assert!(matches!(
        values[1].cause,
        EventCause::ManaPoolsEmptied { .. }
    ));
    assert!(
        matches!(values[2].cause, EventCause::StepBegan { position } if position.step == Step::PostcombatMain)
    );
}

fn opening() -> NativeCandidate {
    GameState::initialize_opening(
        GameId(1002),
        context(),
        OpeningSetup {
            seed: GameSeed::with_uniform_life(
                vec![A, B],
                A,
                libraries(&[A, B]),
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr-20260925"),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
            entropy: EntropyTape::new((0..4).flat_map(|_| [7, 6, 5, 4, 3, 2, 1]).collect()),
        },
    )
    .unwrap()
}

#[test]
fn opening_keep_has_exactly_eighteen_events_and_real_first_untap_entry() {
    let initial = opening();
    let mut rows = initial.rules_journal().records.clone();
    assert_eq!(events(&rows).len(), 16);
    let mut state = initial.into_state();
    reply(
        &mut state,
        A,
        DecisionReply::Mulligan(MulliganChoice::Keep),
        &mut rows,
    );
    reply(
        &mut state,
        B,
        DecisionReply::Mulligan(MulliganChoice::Keep),
        &mut rows,
    );
    let values = events(&rows);
    assert_eq!(values.len(), 18);
    assert!(matches!(
        values[0].cause,
        EventCause::OpeningShuffle {
            player: A,
            phase: OpeningShufflePhase::Initial
        }
    ));
    assert!(matches!(
        values[1].cause,
        EventCause::OpeningShuffle {
            player: B,
            phase: OpeningShufflePhase::Initial
        }
    ));
    assert!(
        matches!(values[16].cause, EventCause::StepBegan { position } if position.step == Step::Untap)
    );
    assert!(
        matches!(values[17].cause, EventCause::StepBegan { position } if position.step == Step::Upkeep)
    );
}

#[test]
fn opening_return_and_charged_bottom_are_whole_cohorts_with_late_failure_rollback() {
    let initial = opening();
    let mut state = initial.into_state();
    let mut rows = Vec::new();
    reply(
        &mut state,
        A,
        DecisionReply::Mulligan(MulliganChoice::Mulligan),
        &mut rows,
    );
    let clocks = (
        state.execution().revision(),
        state.execution().window().revision(),
        state.execution().window().cards().revision(),
    );
    let redrawn = reply(
        &mut state,
        B,
        DecisionReply::Mulligan(MulliganChoice::Mulligan),
        &mut rows,
    );
    assert_eq!(state.execution().revision(), clocks.0 + 6);
    assert_eq!(state.execution().window().revision(), clocks.1 + 6);
    assert_eq!(state.execution().window().cards().revision(), clocks.2 + 30);
    let returns = events(&redrawn.rules_journal().records)
        .into_iter()
        .filter(|event| {
            matches!(
                event.cause,
                EventCause::OpeningCardsMoved {
                    phase: OpeningMovePhase::ReturnForMulligan,
                    ..
                }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(returns.len(), 1);
    assert_eq!(returns[0].occurrences.len(), 14);
    assert_eq!(
        events(&redrawn.rules_journal().records)
            .iter()
            .filter(|event| matches!(
                event.cause,
                EventCause::OpeningShuffle {
                    phase: OpeningShufflePhase::Mulligan,
                    ..
                }
            ))
            .count(),
        2
    );
    let a = hand(&state, A)[6];
    let staged = reply(&mut state, A, DecisionReply::Bottom(vec![a]), &mut rows);
    assert!(staged.rules_journal().records.is_empty());
    let b = hand(&state, B)[6];
    let clocks = (
        state.execution().revision(),
        state.execution().window().revision(),
        state.execution().window().cards().revision(),
    );
    let mut fault = state.clone();
    fault
        .execution
        .set_test_object_allocator(u64::MAX - 1, true);
    let saved = fault.clone();
    assert!(
        fault
            .advance(GameInput {
                basis: fault.basis().clone(),
                actor: B,
                command: GameCommand::Reply {
                    decision: fault.pending_decision().unwrap().id(),
                    choice: DecisionReply::Bottom(vec![b])
                }
            })
            .is_err()
    );
    assert_eq!(fault, saved);
    assert_eq!(hand(&fault, A).len(), 7);
    assert_eq!(hand(&fault, B).len(), 7);
    let committed = reply(&mut state, B, DecisionReply::Bottom(vec![b]), &mut rows);
    assert_eq!(state.execution().revision(), clocks.0 + 2);
    assert_eq!(state.execution().window().revision(), clocks.1 + 2);
    assert_eq!(state.execution().window().cards().revision(), clocks.2 + 2);
    let values = events(&committed.rules_journal().records);
    assert_eq!(values.len(), 1);
    assert!(matches!(
        values[0].cause,
        EventCause::OpeningCardsMoved {
            phase: OpeningMovePhase::ChargedBottom,
            ..
        }
    ));
    let moved = values[0]
        .occurrences
        .iter()
        .map(|row| match &row.value {
            RulesOccurrence::ZoneChange(change) => {
                assert!(matches!(
                    change.reason,
                    ZoneReason::Opening(OpeningMovePhase::ChargedBottom)
                ));
                change.before.object.id()
            }
            _ => panic!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(moved, vec![a, b]);
}

#[test]
fn real_turn_start_stamp_precedes_step_capture_and_actual_untap_status() {
    use crate::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
    let mut cards = libraries(&[A, B]);
    let mut source = observer(
        1,
        A,
        crate::program::TriggerSpec::step_began(Step::Untap, PlayerRelation::Controller),
    );
    source.program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: crate::mana::ManaCost::free(),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Damage { amount: 1.into() }],
        ),
    });
    cards.push(source);
    cards.push(SeedCard {
        card: CardId(2),
        owner: B,
        destination: ObjectDestination::Battlefield { controller: B },
        program: Program::creature(
            None,
            1.into(),
            100.into(),
            std::collections::BTreeSet::new(),
        ),
    });
    let initial = scheduled(vec![A, B], A, Step::Untap, cards, vec![]);
    let mut rows = initial.rules_journal().records.clone();
    let mut state = initial.into_state();
    let source = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let target = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(2))
        .unwrap();
    accept(
        &mut state,
        A,
        GameCommand::ActivateAbility {
            source,
            ability: ActivatedAbilityId(1),
            target: crate::targets::TargetRef::Object(target),
            payment: None,
        },
        &mut rows,
    );
    round(&mut state, &[A, B], &mut rows);
    for _ in 0..7 {
        round(&mut state, &[A, B], &mut rows);
    }
    for _ in 0..4 {
        round(&mut state, &[B, A], &mut rows);
    }
    reply(
        &mut state,
        B,
        DecisionReply::DeclareAttackers(vec![]),
        &mut rows,
    );
    for _ in 0..4 {
        round(&mut state, &[B, A], &mut rows);
    }
    assert_eq!(state.turn().unwrap().position().number, 3);
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    let captures = rows
        .iter()
        .filter_map(|row| match row {
            RulesRecord::TriggerDiscovered(value) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].source.object.id(), source);
    assert_eq!(captures[0].source.object.tapped(), Some(true));
    assert_eq!(
        captures[0]
            .source
            .object
            .has_continuous_control_since_turn_start(),
        Some(true)
    );
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(false)
    );
    let values = events(&rows);
    let untap = values
        .iter()
        .find(|event| matches!(event.cause, EventCause::UntapTurn { active: A }))
        .unwrap();
    let [change] = untap.occurrences.as_slice() else {
        panic!()
    };
    let RulesOccurrence::StatusChange { change, .. } = &change.value else {
        panic!()
    };
    assert_eq!(change.before.id(), source);
    assert_eq!(
        change.before.has_continuous_control_since_turn_start(),
        Some(true)
    );
    assert_eq!(
        change.after.has_continuous_control_since_turn_start(),
        Some(true)
    );
    assert_eq!(change.before.tapped(), Some(true));
    assert_eq!(change.after.tapped(), Some(false));
    let step = values
        .iter()
        .find(|event| event.id == captures[0].causes.event())
        .unwrap();
    assert!(
        matches!(step.cause, EventCause::StepBegan { position } if position.step == Step::Untap && position.number == 3)
    );
    assert!(step.id.sequence() < untap.id.sequence());
    assert_eq!(values.iter().filter(|event| matches!(event.cause, EventCause::Draw { origin: DrawOrigin::Turn { position }, player: B } if position.number == 2 && position.step == Step::Draw)).count(), 1);
}
