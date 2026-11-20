//! Arithmetic controls are synthetic. Native outcome tests use actual GameState
//! mechanisms with explicit genuine vocabulary, never whole card qualification.
use super::*;
#[test]
fn unknown_seat_and_turn_count_overflow_preserve_the_counter() {
    let seat = NativeSeatBinding {
        seat_id: Uuid::from_u128(77),
        native_seat: SeatId(9),
    };
    let mut activity = NativeActivity {
        seats: vec![(seat, u64::MAX)],
    };
    assert!(activity.credit_turn(SeatId(8)).is_err());
    assert!(activity.credit_turn(SeatId(9)).is_err());
    assert_eq!(activity.own_turns(seat.seat_id), Some(u64::MAX));
}
#[test]
#[ignore = "requires explicit genuine vocabulary; native root conclusion mechanism only"]
fn checked_loss_win_draw_and_concession_map_actual_root_without_required_terminal_event() {
    use crate::native_game::tests::fixture::*;
    use spellwake_engine::{
        game::{GameCommand, GameId},
        occurrence::RulesRecord,
    };
    for totals in [[0, 20], [0, 0]] {
        let mut input = seed(vec![]);
        for (row, total) in input.life.iter_mut().zip(totals) {
            row.total = total.into();
        }
        let candidate =
            GameState::initialize(GameId(90), context(&input.creature_vocabulary), input).unwrap();
        assert!(
            !candidate
                .rules_journal()
                .records
                .iter()
                .any(|record| matches!(record, RulesRecord::GameConcluded { .. }))
        );
        let state = candidate.state();
        let terminal = NativeTerminal::derive(state, &binding(state, viewer_a()))
            .unwrap()
            .unwrap();
        assert_eq!(terminal.reason(), "native_checked_losses");
        if totals[1] == 0 {
            assert_eq!(terminal.outcome(), Outcome::Draw);
            assert!(terminal.winner_seat_ids().is_empty());
            assert_eq!(terminal.losing_seat_ids().len(), 2);
        } else {
            assert_eq!(terminal.outcome(), Outcome::Win);
            assert_eq!(terminal.winner_seat_ids(), &[viewer_b()]);
            assert_eq!(terminal.losing_seat_ids(), &[viewer_a()]);
        }
    }
    let source = state(vec![]);
    assert!(
        NativeTerminal::derive(&source, &binding(&source, viewer_a()))
            .unwrap()
            .is_none()
    );
    for (actor, loser, winner) in [(A, viewer_a(), viewer_b()), (B, viewer_b(), viewer_a())] {
        let candidate = source
            .advance(GameInput {
                basis: source.basis().clone(),
                actor,
                command: GameCommand::Concede,
            })
            .unwrap();
        let terminal =
            NativeTerminal::derive(candidate.state(), &binding(candidate.state(), loser))
                .unwrap()
                .unwrap();
        assert_eq!(terminal.reason(), "native_concession");
        assert_eq!(terminal.winner_seat_ids(), &[winner]);
        assert_eq!(terminal.losing_seat_ids(), &[loser]);
        assert!(NativeTerminal::derive(candidate.state(), &binding(&source, loser)).is_err());
    }
}
#[test]
#[ignore = "requires explicit genuine vocabulary; strict frame comparison, not SQL acceptance"]
fn host_frame_must_match_native_result_and_exact_human_turn_counts() {
    use crate::admission_ports::HostCandidate;
    use crate::native_game::tests::fixture::*;
    use serde_json::{Value, json};
    use spellwake_engine::game::GameCommand;
    let source = state(vec![]);
    let next = source
        .advance(GameInput {
            basis: source.basis().clone(),
            actor: A,
            command: GameCommand::Concede,
        })
        .unwrap();
    let terminal = NativeTerminal::derive(next.state(), &binding(next.state(), viewer_a()))
        .unwrap()
        .unwrap();
    let expected = [(viewer_a(), 3), (viewer_b(), 2)];
    let body = json!({"contextId":context_id(),"stateRef":"fixture:state","factsRef":"fixture:results","events":["fixture:event"],"facts":[],"clocks":[{"seatId":viewer_a(),"clockRevision":"0","remainingMs":"0","paused":true}],"participation":[{"seatId":viewer_a(),"ownTurns":"3","reason":"native_turn_starts_v1"},{"seatId":viewer_b(),"ownTurns":"2","reason":"native_turn_starts_v1"}],"terminal":{"outcome":"win","winnerSeatIds":[viewer_b()],"winnersRef":"fixture:results","reason":"native_concession"}});
    let parse =
        |value: &Value| HostCandidate::from_bytes(&serde_json::to_vec(value).unwrap()).unwrap();
    assert!(parse(&body).matches_native_results(Some(&terminal), &expected));
    assert!(!parse(&body).matches_native_results(None, &[]));
    for (path, value) in [
        ("/terminal/outcome", json!("draw")),
        ("/terminal/winnerSeatIds", json!([viewer_a()])),
        ("/terminal/reason", json!("native_checked_losses")),
        ("/terminal/winnersRef", json!("fixture:other")),
        ("/participation/0/ownTurns", json!("4")),
        ("/participation/0/seatId", json!(Uuid::new_v4())),
        ("/participation/0/reason", json!("fixture")),
        ("/participation", json!([])),
    ] {
        let mut altered = body.clone();
        *altered.pointer_mut(path).unwrap() = value;
        // A draw carrying winners is rejected at syntax/shape validation first.
        let decoded = HostCandidate::from_bytes(&serde_json::to_vec(&altered).unwrap());
        assert!(
            decoded.is_err()
                || !decoded
                    .unwrap()
                    .matches_native_results(Some(&terminal), &expected)
        );
    }
    let mut missing = body.clone();
    missing["terminal"] = Value::Null;
    missing["participation"] = json!([]);
    assert!(!parse(&missing).matches_native_results(Some(&terminal), &expected));
    assert!(parse(&missing).matches_native_results(None, &[]));
    missing["facts"] = json!([{"eventIndex":"0","accountId":Uuid::new_v4(),"kind":"invented","revision":"v1","value":"1"}]);
    assert!(!parse(&missing).matches_native_results(None, &[]));
    assert!(!parse(&body).matches_native_results(Some(&terminal), &expected[..1]));
}

#[test]
fn synthetic_calendar_controls_reject_gaps_and_accept_legal_step_repetitions_without_credit() {
    // Deliberately adversarial typed transition operands, not a native game proof.
    let a = SeatId(1);
    let b = SeatId(2);
    let activity = NativeActivity {
        seats: vec![
            (
                NativeSeatBinding {
                    seat_id: Uuid::from_u128(20),
                    native_seat: a,
                },
                0,
            ),
            (
                NativeSeatBinding {
                    seat_id: Uuid::from_u128(10),
                    native_seat: b,
                },
                0,
            ),
        ],
    };
    let first = first_position(1);
    let start = TurnTransition {
        from: None,
        to: first,
        active_before: a,
        active_after: a,
    };
    let began = activity
        .follow_calendar(None, a, std::slice::from_ref(&start), Some(first), a)
        .unwrap();
    assert_eq!(began.own_turns(Uuid::from_u128(20)), Some(1));
    let mut damage = first;
    damage.step = Step::CombatDamage;
    let mut extra_damage = damage;
    extra_damage.additional_damage = true;
    let mut cleanup = first;
    cleanup.step = Step::Cleanup;
    let mut repeated = cleanup;
    repeated.cleanup_iteration = 1;
    let positions = [first, damage, extra_damage, cleanup, repeated];
    let steps: Vec<_> = positions
        .windows(2)
        .map(|pair| TurnTransition {
            from: Some(pair[0]),
            to: pair[1],
            active_before: a,
            active_after: a,
        })
        .collect();
    let within = began
        .follow_calendar(Some(first), a, &steps, Some(repeated), a)
        .unwrap();
    assert_eq!(within.own_turns(Uuid::from_u128(20)), Some(1));
    let next = first_position(2);
    let rotated = within
        .follow_calendar(
            Some(repeated),
            a,
            &[TurnTransition {
                from: Some(repeated),
                to: next,
                active_before: a,
                active_after: b,
            }],
            Some(next),
            b,
        )
        .unwrap();
    assert_eq!(rotated.own_turns(Uuid::from_u128(10)), Some(1));
    for bad in [
        TurnTransition {
            to: first_position(2),
            ..start.clone()
        },
        TurnTransition {
            active_before: b,
            ..start.clone()
        },
        TurnTransition {
            active_after: b,
            ..start.clone()
        },
        TurnTransition {
            from: Some(first),
            ..start.clone()
        },
    ] {
        assert!(
            activity
                .follow_calendar(None, a, &[bad], Some(first), a)
                .is_err()
        );
    }
    for bad in [
        TurnTransition {
            from: Some(first),
            to: first,
            active_before: a,
            active_after: a,
        },
        TurnTransition {
            from: Some(first),
            to: first_position(3),
            active_before: a,
            active_after: b,
        },
        TurnTransition {
            from: Some(first),
            to: first_position(0),
            active_before: a,
            active_after: b,
        },
        TurnTransition {
            from: Some(first),
            to: TurnPosition {
                step: Step::Draw,
                ..next
            },
            active_before: a,
            active_after: b,
        },
        TurnTransition {
            from: Some(first),
            to: damage,
            active_before: a,
            active_after: b,
        },
    ] {
        assert!(
            began
                .follow_calendar(
                    Some(first),
                    a,
                    std::slice::from_ref(&bad),
                    Some(bad.to),
                    bad.active_after
                )
                .is_err()
        );
    }
    assert!(
        activity
            .follow_calendar(None, a, &[start], None, a)
            .is_err()
    );
    assert!(
        began
            .follow_calendar(Some(first), a, &[], Some(first), b)
            .is_err()
    );
    assert!(
        activity
            .follow_calendar(None, SeatId(99), &[], None, SeatId(99))
            .is_err()
    );
    assert_eq!(activity.own_turns(Uuid::from_u128(20)), Some(0));
    assert_eq!(began.own_turns(Uuid::from_u128(20)), Some(1));
}

#[test]
#[ignore = "requires explicit genuine vocabulary; candidate basis and surviving multiplayer roster"]
fn foreign_candidate_base_and_nonterminal_multiplayer_loss_do_not_create_results_or_turns() {
    use crate::native_game::tests::fixture::*;
    use spellwake_engine::game::{GameCommand, GameId};
    let source = state(vec![]);
    let candidate = source
        .advance(GameInput {
            basis: source.basis().clone(),
            actor: A,
            command: GameCommand::Concede,
        })
        .unwrap();
    let activity = NativeActivity {
        seats: binding(&source, viewer_a())
            .seats
            .iter()
            .map(|seat| (*seat, 0))
            .collect(),
    };
    let wrong = GameState::initialize(GameId(91), context(&vocabulary()), seed(vec![])).unwrap();
    assert!(activity.advance(wrong.state(), &candidate).is_err());
    let original_context = &source.basis().context;
    let foreign_context = spellwake_engine::game::GameContext::new(
        "foreign-context".into(),
        original_context.rules().into(),
        original_context.catalogue().into(),
        original_context.programs().into(),
        original_context.profile().into(),
        original_context.creature_vocabulary_id(),
        original_context.name_rules_id(),
    )
    .unwrap();
    let foreign = GameState::initialize(GameId(90), foreign_context, seed(vec![])).unwrap();
    assert!(activity.advance(foreign.state(), &candidate).is_err());
    assert!(activity.advance(candidate.state(), &candidate).is_err());
    let prepared = GameState::initialize_scheduled(
        GameId(90),
        context(&vocabulary()),
        seed(vec![]),
        spellwake_engine::turn::TurnStart {
            number: 99,
            step: Step::PrecombatMain,
        },
    )
    .unwrap();
    assert!(
        NativeActivity::initial(&prepared, &binding(prepared.state(), viewer_a()).seats).is_err()
    );
    let third = SeatId(99);
    let mut input = seed(vec![]);
    input.seats.push(third);
    input.life.push(spellwake_engine::setup::StartingLife {
        player: third,
        total: 0.into(),
    });
    let continued =
        GameState::initialize(GameId(90), context(&input.creature_vocabulary), input).unwrap();
    assert!(continued.state().pending_eliminations().is_empty());
    assert_eq!(
        continued.state().execution().window().participants(),
        &[A, B]
    );
    assert!(continued.state().original_participants().contains(&third));
    assert!(continued.state().outcome().is_none());
    let mut roster = binding(&source, viewer_a()).seats;
    roster.push(NativeSeatBinding {
        seat_id: Uuid::from_u128(104),
        native_seat: third,
    });
    let bound = NativeBinding::new(
        context_id(),
        continued.state().basis().clone(),
        roster,
        viewer_a(),
    )
    .unwrap();
    assert!(
        NativeTerminal::derive(continued.state(), &bound)
            .unwrap()
            .is_none()
    );
    assert!(
        NativeTerminal::derive(continued.state(), &binding(continued.state(), viewer_a())).is_err()
    );
}
