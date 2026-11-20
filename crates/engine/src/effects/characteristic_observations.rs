//! Scoped copies of real cuts for privileged tests. No field is added to an owner.
use super::*;
use crate::{
    checks::{CheckBoundary, CheckEventId},
    object_identity::ObjectMovement,
    occurrence::RulesRecord,
};
use std::cell::RefCell;

#[derive(Debug, Clone)]
// This test-only observer retains independent actual-cut registry/movement
// snapshots. Token origin enlarges them; preserve existing capture ownership
// until a measured workload justifies changing its allocation layout.
#[allow(clippy::large_enum_variant)]
pub(super) enum Observation {
    PureEvaluation(Box<super::characteristic_reference_capture::PureEvaluationSnapshot>),
    Trajectory(Box<super::characteristic_reference_capture::TrajectoryRecord>),
    AcceptedState {
        failed_draws: BTreeSet<SeatId>,
        losses: BTreeSet<SeatId>,
    },
    DrawRequested {
        player: SeatId,
        origin: DrawOrigin,
        amount: RulesInteger,
        cards: GameObjects,
        failed_draws: BTreeSet<SeatId>,
    },
    Drawn {
        player: SeatId,
        origin: DrawOrigin,
        before: GameObjects,
        after: GameObjects,
        movement: ObjectMovement,
        failed_draws: BTreeSet<SeatId>,
    },
    DrawCompleted {
        player: SeatId,
        origin: DrawOrigin,
        journal: DrawJournal,
        cards: GameObjects,
        failed_draws: BTreeSet<SeatId>,
    },
    Published(RulesJournal),
    PublishedState {
        execution: Box<EffectWindow>,
        journal: RulesJournal,
    },
    Instruction {
        phase: &'static str,
        entry: StackEntryId,
        instruction: usize,
        operation: Option<Instruction>,
        execution: Box<EffectWindow>,
    },
    ChecksObserved {
        boundary: CheckBoundary,
        event: CheckEventId,
        cards: GameObjects,
        actions_empty: bool,
        execution: Box<EffectWindow>,
    },
}

thread_local! {
    static RECORDS: RefCell<Option<Vec<Observation>>> = const { RefCell::new(None) };
}

fn record(make: impl FnOnce() -> Observation) {
    RECORDS.with(|slot| {
        if let Some(records) = slot.borrow_mut().as_mut() {
            records.push(make());
        }
    });
}

/// Feed exact shared-route calls into this same thread-scoped observation recorder.
pub(super) fn trajectory_record(value: super::characteristic_reference_capture::TrajectoryRecord) {
    record(|| Observation::Trajectory(Box::new(value)));
}

/// Records attempted internal cuts separately from successful returned states.
pub(super) fn capture(run: impl FnOnce()) -> Vec<Observation> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            RECORDS.with(|slot| *slot.borrow_mut() = None);
        }
    }
    RECORDS.with(|slot| {
        let mut records = slot.borrow_mut();
        assert!(records.is_none(), "nested characteristic capture");
        *records = Some(Vec::new());
    });
    let _reset = Reset;
    crate::test_support::characteristic_trajectories::with_state_observer(
        |state| {
            record(|| Observation::AcceptedState {
                failed_draws: state.execution().failed_draws.clone(),
                losses: state.execution().state_based_losses.clone(),
            });
        },
        run,
    );
    RECORDS.with(|slot| slot.borrow_mut().take().unwrap())
}

pub(super) fn requested(
    state: &EffectWindow,
    player: SeatId,
    origin: DrawOrigin,
    amount: &RulesInteger,
) {
    record(|| Observation::DrawRequested {
        player,
        origin,
        amount: amount.clone(),
        cards: state.window.cards().clone(),
        failed_draws: state.failed_draws.clone(),
    });
}

pub(super) fn drawn(
    state: &EffectWindow,
    player: SeatId,
    origin: DrawOrigin,
    before: &GameObjects,
    after: &GameObjects,
    movement: &ObjectMovement,
) {
    record(|| Observation::Drawn {
        player,
        origin,
        before: before.clone(),
        after: after.clone(),
        movement: movement.clone(),
        failed_draws: state.failed_draws.clone(),
    });
}

pub(super) fn completed(
    state: &EffectWindow,
    player: SeatId,
    origin: DrawOrigin,
    journal: &DrawJournal,
) {
    record(|| Observation::DrawCompleted {
        player,
        origin,
        journal: journal.clone(),
        cards: state.window.cards().clone(),
        failed_draws: state.failed_draws.clone(),
    });
}

pub(super) fn published(state: &EffectWindow, records: &RulesJournal) {
    record(|| Observation::Published(records.clone()));
    record(|| Observation::PublishedState {
        execution: Box::new(state.clone()),
        journal: records.clone(),
    });
}

/// Operational instruction cuts are diagnostic observations, never new RulesEvents.
pub(super) fn instruction(
    state: &EffectWindow,
    phase: &'static str,
    entry: StackEntryId,
    index: usize,
    operation: Option<&Instruction>,
) {
    record(|| Observation::Instruction {
        phase,
        entry,
        instruction: index,
        operation: operation.cloned(),
        execution: Box::new(state.clone()),
    });
}

pub(super) fn damage_instruction_completed(state: &EffectWindow) {
    if let Some(cursor) = state.resolution_state()
        && let Some(index) = cursor.instruction().checked_sub(1)
    {
        instruction(state, "Completed", cursor.entry(), index, None);
    }
}

pub(super) fn checks_observed(
    state: &EffectWindow,
    boundary: CheckBoundary,
    event: CheckEventId,
    actions_empty: bool,
) {
    record(|| Observation::ChecksObserved {
        boundary,
        event,
        cards: state.window.cards().clone(),
        actions_empty,
        execution: Box::new(state.clone()),
    });
}

fn assert_no_failed_draws(records: &[Observation]) {
    let mut accepted = 0;
    for record in records {
        match record {
            Observation::AcceptedState {
                failed_draws,
                losses,
            } => {
                accepted += 1;
                assert!(failed_draws.is_empty());
                assert!(losses.is_empty());
            }
            Observation::DrawRequested { failed_draws, .. }
            | Observation::Drawn { failed_draws, .. }
            | Observation::DrawCompleted { failed_draws, .. } => {
                assert!(failed_draws.is_empty());
            }
            _ => {}
        }
    }
    assert!(accepted > 0);
}

fn tap_route(mana: bool) {
    use crate::test_support::characteristic_trajectories::{P0, P1, new_tap};
    let records = capture(|| new_tap(mana));
    assert_no_failed_draws(&records);
    let requests = records
        .iter()
        .filter_map(|record| match record {
            Observation::DrawRequested {
                player,
                origin,
                amount,
                ..
            } => Some((*player, *origin, amount.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    for ((player, origin, amount), expected_player, number) in
        [(&requests[0], P1, 2), (&requests[1], P0, 3)]
    {
        assert_eq!(*player, expected_player);
        assert_eq!(*amount, 1.into());
        let DrawOrigin::Turn { position } = origin else {
            panic!("scheduled draw")
        };
        assert_eq!(position.number, number);
        assert_eq!(position.step, crate::turn::Step::Draw);
    }
    let completions = records
        .iter()
        .filter_map(|record| match record {
            Observation::DrawCompleted {
                player,
                origin,
                journal,
                cards,
                ..
            } => Some((*player, *origin, journal, cards)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 2);
    for ((player, origin, journal, cards), request, expected_card) in [
        (&completions[0], &requests[0], CardId(6)),
        (&completions[1], &requests[1], CardId(5)),
    ] {
        assert_eq!((*player, *origin), (request.0, request.1));
        assert_eq!(journal.events.len(), 1);
        let DrawEvent::Drawn {
            player: drawn_player,
            before,
            after,
        } = &journal.events[0]
        else {
            panic!("no empty draw hidden by checks")
        };
        assert_eq!(*drawn_player, *player);
        assert_eq!(
            before.card_id().expect("physical fixture card"),
            expected_card
        );
        assert_eq!(
            after.card_id().expect("physical fixture card"),
            expected_card
        );
        assert_eq!(after.zone(), ObjectZone::Hand(*player));
        assert_eq!(cards.get(after.id()), Some(after));
    }
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(record, Observation::Drawn { .. }))
            .count(),
        2
    );
}

#[test]
fn cl_observe_tap_new_mana() {
    tap_route(true);
}

#[test]
fn cl_observe_tap_new_pump() {
    tap_route(false);
}

#[test]
fn cl_observe_chosen_count_draw_cuts() {
    use crate::test_support::characteristic_trajectories::{P0, chosen_count_draw_cuts};
    let records = capture(chosen_count_draw_cuts);
    assert_no_failed_draws(&records);
    let requests = records
        .iter()
        .filter_map(|record| match record {
            Observation::DrawRequested {
                player,
                origin,
                amount,
                cards,
                ..
            } => Some((*player, *origin, amount.clone(), cards)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests.iter().map(|r| r.2.clone()).collect::<Vec<_>>(),
        vec![2.into(), 1.into()]
    );
    let draws = records
        .iter()
        .filter_map(|record| match record {
            Observation::Drawn {
                player,
                origin,
                before,
                after,
                movement,
                ..
            } => Some((*player, *origin, before, after, movement)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(draws.len(), 3);
    let mut entry = None;
    for (index, (player, origin, before, after, movement)) in draws.iter().enumerate() {
        assert_eq!(*player, P0);
        let DrawOrigin::Instruction {
            entry: actual_entry,
            instruction,
        } = origin
        else {
            panic!("actual body draw origin")
        };
        assert_eq!(*instruction, [2, 2, 4][index]);
        assert_eq!(*entry.get_or_insert(*actual_entry), *actual_entry);
        let before_cards = before
            .iter()
            .map(|card| (card.card_id().expect("physical fixture card"), card))
            .collect::<BTreeMap<_, _>>();
        let after_cards = after
            .iter()
            .map(|card| (card.card_id().expect("physical fixture card"), card))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            before_cards.keys().copied().collect::<Vec<_>>(),
            (1..=7).map(CardId).collect::<Vec<_>>()
        );
        assert_eq!(
            before_cards.keys().collect::<Vec<_>>(),
            after_cards.keys().collect::<Vec<_>>()
        );
        let drawn_card = CardId(4 + index as u64);
        assert_eq!(
            movement.before.card_id().expect("physical fixture card"),
            drawn_card
        );
        assert_eq!(movement.before.zone(), ObjectZone::Library(P0));
        assert_eq!(movement.after.zone(), ObjectZone::Hand(P0));
        assert_eq!(movement.after.owner(), P0);
        assert_eq!(movement.after.controller(), None);
        assert_ne!(movement.before.id(), movement.after.id());
        assert_eq!(before_cards[&drawn_card], &movement.before);
        assert_eq!(after_cards[&drawn_card], &movement.after);
        for (card, original) in &before_cards {
            if *card != drawn_card {
                assert_eq!(*original, after_cards[card]);
            }
        }
        let library = |cards: &GameObjects| {
            cards
                .library(P0)
                .iter()
                .map(|id| {
                    cards
                        .get(*id)
                        .unwrap()
                        .card_id()
                        .expect("physical fixture card")
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            library(before),
            (4 + index as u64..=6).map(CardId).collect::<Vec<_>>()
        );
        assert_eq!(
            library(after),
            (5 + index as u64..=6).map(CardId).collect::<Vec<_>>()
        );
        assert_eq!(before_cards[&CardId(7)].zone(), ObjectZone::Stack);
        if index > 0 {
            assert_eq!(*before, draws[index - 1].3);
        }
    }
    assert_eq!(requests[0].3, draws[0].2);
    assert_eq!(requests[1].3, draws[2].2);
    let completions = records
        .iter()
        .filter_map(|record| match record {
            Observation::DrawCompleted {
                player,
                origin,
                journal,
                cards,
                ..
            } => Some((*player, *origin, journal, cards)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 2);
    for (index, last_draw) in [1, 2].into_iter().enumerate() {
        assert_eq!(
            (completions[index].0, completions[index].1),
            (requests[index].0, requests[index].1)
        );
        assert_eq!(completions[index].2.events.len(), [2, 1][index]);
        assert_eq!(completions[index].3, draws[last_draw].3);
    }
}

#[test]
fn cl_observe_cleanup_simultaneous() {
    use crate::test_support::characteristic_trajectories::cleanup_simultaneous;
    let records = capture(cleanup_simultaneous);
    assert_no_failed_draws(&records);
    assert!(
        !records
            .iter()
            .any(|r| matches!(r, Observation::DrawRequested { .. }))
    );
    let mut cuts = Vec::new();
    let mut cleanup_position = None;
    for record in &records {
        match record {
            Observation::Published(journal) => {
                for record in &journal.records {
                    if let RulesRecord::Event(event) = record {
                        match event.cause {
                            crate::occurrence::EventCause::CleanupDiscard { .. } => {
                                cuts.push("discard")
                            }
                            crate::occurrence::EventCause::CleanupCharacteristics { position } => {
                                cuts.push("combined");
                                cleanup_position = Some(position);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Observation::ChecksObserved {
                boundary: CheckBoundary::Cleanup(position),
                event,
                cards,
                actions_empty,
                ..
            } => {
                cuts.push("checks");
                assert_eq!(Some(*position), cleanup_position);
                assert_eq!(event.pass(), 1);
                assert!(*actions_empty);
                let x = cards
                    .iter()
                    .find(|c| c.card_id().expect("physical fixture card") == CardId(1))
                    .unwrap();
                assert_eq!(x.zone(), ObjectZone::Battlefield);
                assert_eq!(
                    cards
                        .iter()
                        .find(|c| c.card_id().expect("physical fixture card") == CardId(9))
                        .unwrap()
                        .zone(),
                    ObjectZone::Graveyard(SeatId(0))
                );
            }
            _ => {}
        }
    }
    assert_eq!(cuts, vec!["discard", "combined", "checks"]);
}

/// Lazily clone a pure fixture only while this existing recorder is active.
pub(super) fn pure_evaluation(
    make: impl FnOnce() -> super::characteristic_reference_capture::PureEvaluationSnapshot,
) {
    record(|| Observation::PureEvaluation(Box::new(make())));
}
