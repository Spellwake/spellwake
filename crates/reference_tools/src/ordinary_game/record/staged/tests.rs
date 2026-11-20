//! Actual ordinary recorder/policy failure paths over real native calls and the
//! genuine vocabulary. Capture receipt metadata is synthetic; no source9,
//! Report5, reader/image, card or host qualification is claimed.
use super::*;
use crate::ordinary_game::fixture::{A, B};
use serde_json::{Value, json};
use spellwake_engine::{
    draw::DrawEvent,
    game::{DecisionReply, GameCommand},
    opening::MulliganChoice,
};

fn fixture() -> Fixture {
    Fixture::new(
        CaseId::G01,
        crate::creature_types::selected_vocabulary().unwrap(),
    )
    .unwrap()
}
fn captured<T>(fixture: &Fixture, operation: impl FnOnce() -> T) -> T {
    crate::current_capture::with_selection_observer_for_test(
        &fixture.setup.seed.creature_vocabulary,
        operation,
    )
}
fn emit(recorder: &Recorder) -> Value {
    crate::current_capture::finish_record(
        "ordinary-staged-unit",
        json!({
            "observations":{"data":{"runs":[&recorder.report]}},
        }),
    )
    .unwrap()
}
fn completion(output: &Value, ordinal: usize) -> &Value {
    let call = &output["type_context_witness"]["calls"][ordinal];
    &output["type_context_witness"]["caller_delivery"]["calls"][call["call_id"].as_str().unwrap()]["completion"]
}
fn issued(policy: &mut Policy, state: &GameState) -> (GameInput, String) {
    let prepared = policy.prepare(state).unwrap().unwrap();
    let name = prepared.policy_entry_ref.clone();
    (policy.issue(state, prepared).unwrap(), name)
}

#[test]
fn prepared_policy_and_initial_record_can_be_dropped_without_mutation() {
    let fixture = fixture();
    let initial = initialize_fixture(&fixture).unwrap();
    let state = initial.state();
    let mut policy = Policy::new(&fixture).unwrap();
    let (input, _) = issued(&mut policy, state);
    let native = state.advance(input.clone()).unwrap();
    let before = format!("{policy:?}");
    drop(policy.prepare_accept(&input, &native).unwrap());
    assert_eq!(format!("{policy:?}"), before);
    policy.prepare_accept(&input, &native).unwrap().commit();
    assert_eq!(policy.consumed(), 1);

    let mut recorder = Recorder::new(fixture.case);
    let before = serde_json::to_value(&recorder.report).unwrap();
    drop(recorder.prepare_initial(&fixture, &initial).unwrap());
    assert_eq!(serde_json::to_value(&recorder.report).unwrap(), before);
    assert!(recorder.world.is_none());
    assert!(recorder.rules_journals.is_empty());
    for object in initial.state().execution().window().cards().iter() {
        assert!(recorder.history.known(&fixture, object.id()).is_err());
    }
}

#[test]
fn actual_opening_initializer_conversion_failure_keeps_native_ok_and_no_history() {
    let mut fixture = fixture();
    let direct = initialize_fixture(&fixture);
    assert!(direct.is_ok());
    let card = *fixture.aliases.keys().next().unwrap();
    fixture.aliases.remove(&card); // Real existing world conversion now fails.
    captured(&fixture, || {
        let mut recorder = Recorder::new(fixture.case);
        let error = initialize_and_install(&fixture, &mut recorder).unwrap_err();
        assert!(error.contains("unbound physical card"), "{error}");
        let trace = recorder.report.native_trace.as_ref().unwrap();
        assert_eq!(trace.initialization, direct);
        assert!(!trace.initial_installed);
        assert!(trace.replay_verified);
        assert!(recorder.report.initialization.is_none());
        assert!(recorder.rules_journals.is_empty());
        for object in direct
            .as_ref()
            .unwrap()
            .state()
            .execution()
            .window()
            .cards()
            .iter()
        {
            assert!(recorder.history.known(&fixture, object.id()).is_err());
        }
        let output = emit(&recorder);
        assert_eq!(
            completion(&output, 0),
            &json!({"stage":"RecorderPrepareFailed"})
        );
        assert_eq!(
            output["type_context_witness"]["calls"][0]["result"]["tag"],
            "Ok"
        );
        assert_eq!(output["type_context_witness"]["calls"][0]["adopted"], false);
    });
}

#[test]
fn actual_native_initializer_error_has_no_installed_record_and_full_error_replay() {
    let mut fixture = fixture();
    fixture.setup.seed.seats[1] = A;
    let direct = initialize_fixture(&fixture);
    assert!(direct.is_err());
    captured(&fixture, || {
        let mut recorder = Recorder::new(fixture.case);
        assert!(initialize_and_install(&fixture, &mut recorder).is_err());
        let trace = recorder.report.native_trace.as_ref().unwrap();
        assert_eq!(trace.initialization, direct);
        assert!(!trace.initial_installed);
        assert!(trace.replay_verified);
        assert!(recorder.report.initialization.is_none());
        let output = emit(&recorder);
        assert_eq!(completion(&output, 0), &json!({"stage":"NativeError"}));
        assert!(output["type_context_witness"]["calls"][0]["after_point"].is_null());
    });
}

#[test]
fn late_record_failure_does_not_publish_counterfactual_draw_and_retry_uses_original_input() {
    let fixture = fixture();
    captured(&fixture, || {
        let mut recorder = Recorder::new(fixture.case);
        let mut policy = Policy::new(&fixture).unwrap();
        let mut state = initialize_and_install(&fixture, &mut recorder).unwrap();
        for _ in 0..128 {
            let (input, name) = issued(&mut policy, &state);
            let direct = state.advance(input.clone()).unwrap();
            if let Some(DrawEvent::Drawn { after, .. }) = direct
                .draw_journal()
                .events
                .iter()
                .find(|event| matches!(event, DrawEvent::Drawn { .. }))
            {
                let new_id = after.id();
                assert!(recorder.history.known(&fixture, new_id).is_err());
                let before = state.clone();
                let accepted = recorder.report.accepted.len();
                let journals = recorder.rules_journals.clone();
                let consumed = policy.consumed();
                let world = recorder.world.take(); // Fails after local history/world preparation.
                let error = advance_and_install(
                    &fixture,
                    &mut recorder,
                    &mut policy,
                    &mut state,
                    input.clone(),
                    name.clone(),
                )
                .unwrap_err();
                assert_eq!(error, "recorder lacks initial world");
                assert_eq!(state, before);
                assert_eq!(policy.consumed(), consumed);
                assert_eq!(recorder.report.accepted.len(), accepted);
                assert_eq!(recorder.rules_journals, journals);
                assert!(recorder.history.known(&fixture, new_id).is_err());
                let trace = recorder.report.native_trace.as_ref().unwrap();
                let failed = trace.calls.len() - 1;
                assert_eq!(trace.calls[failed].returned, Ok(direct.clone()));
                assert!(trace.calls[failed].installed_ordinal.is_none());
                assert!(trace.replay_verified);

                recorder.world = world;
                advance_and_install(
                    &fixture,
                    &mut recorder,
                    &mut policy,
                    &mut state,
                    input,
                    name,
                )
                .unwrap();
                assert_eq!(state, direct.into_state());
                assert!(recorder.history.known(&fixture, new_id).is_ok());
                verify_trace(&fixture, &mut recorder, Some(&state)).unwrap();
                let trace = recorder.report.native_trace.as_ref().unwrap();
                assert_eq!(
                    trace.calls[failed].returned,
                    trace.calls[failed + 1].returned
                );
                assert_eq!(
                    trace.calls[failed + 1].installed_ordinal,
                    Some(accepted + 1)
                );
                let output = emit(&recorder);
                assert_eq!(
                    completion(&output, failed + 1)["stage"],
                    "RecorderPrepareFailed"
                );
                assert_eq!(completion(&output, failed + 2)["stage"], "Installed");
                assert!(
                    completion(&output, failed + 1)
                        .get("accepted_record_pointer")
                        .is_none()
                );
                return;
            }
            advance_and_install(
                &fixture,
                &mut recorder,
                &mut policy,
                &mut state,
                input,
                name,
            )
            .unwrap();
        }
        panic!("actual G01 policy did not reach its ordinary draw");
    });
}

#[test]
fn real_policy_receipt_failure_discards_fully_prepared_record_without_fake_game_error() {
    let fixture = fixture();
    captured(&fixture, || {
        let mut recorder = Recorder::new(fixture.case);
        let mut state = initialize_and_install(&fixture, &mut recorder).unwrap();
        let mut policy = Policy::new(&fixture).unwrap();
        let prepared = policy.prepare(&state).unwrap().unwrap();
        let input = prepared.input.clone();
        let name = prepared.policy_entry_ref.clone();
        let mut differently_issued = prepared;
        let GameCommand::Reply { choice, .. } = &mut differently_issued.input.command else {
            panic!()
        };
        *choice = DecisionReply::Mulligan(MulliganChoice::Mulligan);
        let alternative = policy.issue(&state, differently_issued).unwrap();
        assert_ne!(input, alternative);
        let direct = state.advance(input.clone()).unwrap();
        let before = state.clone();
        let policy_before = format!("{policy:?}");
        let error = advance_and_install(
            &fixture,
            &mut recorder,
            &mut policy,
            &mut state,
            input,
            name,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "accepted input differs from the exact issued policy input"
        );
        assert_eq!(state, before);
        assert_eq!(format!("{policy:?}"), policy_before);
        assert!(recorder.report.accepted.is_empty());
        assert_eq!(recorder.rules_journals.len(), 1);
        let trace = recorder.report.native_trace.as_ref().unwrap();
        assert_eq!(trace.calls[0].returned, Ok(direct));
        assert!(trace.calls[0].installed_ordinal.is_none());
        assert!(trace.replay_verified);
        let output = emit(&recorder);
        assert_eq!(
            completion(&output, 1),
            &json!({"stage":"PolicyAcceptFailed"})
        );
        assert_eq!(
            output["type_context_witness"]["calls"][1]["result"]["tag"],
            "Ok"
        );
    });
}

#[test]
fn actual_native_error_retains_source_policy_and_complete_typed_error() {
    let fixture = fixture();
    captured(&fixture, || {
        let mut recorder = Recorder::new(fixture.case);
        let mut state = initialize_and_install(&fixture, &mut recorder).unwrap();
        let mut policy = Policy::new(&fixture).unwrap();
        let (mut input, name) = issued(&mut policy, &state);
        input.actor = B;
        let direct = state.advance(input.clone());
        assert!(direct.is_err());
        let source = state.clone();
        let policy_before = format!("{policy:?}");
        assert!(
            advance_and_install(
                &fixture,
                &mut recorder,
                &mut policy,
                &mut state,
                input,
                name
            )
            .is_err()
        );
        assert_eq!(state, source);
        assert_eq!(format!("{policy:?}"), policy_before);
        let trace = recorder.report.native_trace.as_ref().unwrap();
        assert_eq!(trace.calls[0].returned, direct);
        assert!(trace.replay_verified);
        let output = emit(&recorder);
        assert_eq!(completion(&output, 1), &json!({"stage":"NativeError"}));
    });
}

#[test]
fn complete_existing_fixture_uses_finalized_delivery_and_preserves_report_bytes() {
    let fixture = fixture();
    let old = super::super::run_case(fixture.case, fixture.setup.seed.creature_vocabulary.clone());
    assert!(old.completed(), "{:?}", old.failure);
    captured(&fixture, || {
        let mut recorder = Recorder::new(fixture.case);
        execute_fixture(&fixture, &mut recorder).unwrap();
        assert!(recorder.report.completed());
        let trace = recorder.report.native_trace.as_ref().unwrap();
        assert!(trace.replay_verified);
        assert!(trace.initial_installed);
        assert_eq!(
            trace
                .calls
                .iter()
                .filter(|call| call.installed_ordinal.is_some())
                .count(),
            recorder.report.accepted.len()
        );
        assert_eq!(
            serde_json::to_vec(&recorder.report).unwrap(),
            serde_json::to_vec(&old).unwrap()
        );
        let output = emit(&recorder);
        assert_eq!(completion(&output, 0)["stage"], "Installed");
    });
}

#[test]
fn actual_postinstall_capture_failure_keeps_native_policy_history_and_installed_ticket() {
    let fixture = fixture();
    captured(&fixture, || {
        let mut recorder = Recorder::new(fixture.case);
        let mut state = initialize_and_install(&fixture, &mut recorder).unwrap();
        let mut policy = Policy::new(&fixture).unwrap();
        let (input, name) = issued(&mut policy, &state);
        let direct = state.advance(input.clone()).unwrap();
        let old_state = state.clone();
        let old_journals = recorder.rules_journals.len();
        let old_consumed = policy.consumed();
        crate::current_capture::exhaust_next_observation_for_test();
        advance_and_install(
            &fixture,
            &mut recorder,
            &mut policy,
            &mut state,
            input.clone(),
            name,
        )
        .unwrap();
        assert_ne!(state, old_state);
        assert_eq!(state, direct.clone().into_state());
        assert_eq!(policy.consumed(), old_consumed + 1);
        assert_eq!(recorder.report.accepted.len(), 1);
        assert_eq!(recorder.rules_journals.len(), old_journals + 1);
        assert_eq!(recorder.rules_journals.last(), Some(direct.rules_journal()));
        assert_eq!(recorder.report.accepted[0].ordinal, 1);
        assert!(
            equal(
                recorder.world.as_ref().unwrap(),
                &view::world(&fixture, &recorder.history, &state).unwrap()
            )
            .unwrap()
        );
        for object in state.execution().window().cards().iter() {
            recorder.history.known(&fixture, object.id()).unwrap();
        }
        verify_trace(&fixture, &mut recorder, Some(&state)).unwrap();
        let trace = recorder.report.native_trace.as_ref().unwrap();
        assert_eq!(trace.calls[0].input, input);
        assert_eq!(trace.calls[0].returned, Ok(direct));
        assert_eq!(trace.calls[0].installed_ordinal, Some(1));
        assert!(trace.replay_verified);
        let delivery = crate::current_capture::caller_delivery_for_test();
        let calls = delivery["calls"].as_object().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(
            calls
                .values()
                .all(|call| call["completion"]["stage"] == "Installed")
        );
        assert!(
            calls
                .values()
                .any(|call| call["completion"]["accepted_record_pointer"]
                    == "/observations/data/runs/0/accepted/0")
        );
        let error = crate::current_capture::finish_record(
            "ordinary-staged-unit",
            json!({"observations":{"data":{"runs":[&recorder.report]}}}),
        )
        .unwrap_err();
        assert_eq!(error, "qualification producer label exhausted");
    });
}

#[test]
fn actual_postinitialization_policy_failure_finalizes_full_native_trace() {
    let mut fixture = fixture();
    fixture.initialization = FixtureInitialization::Scheduled(spellwake_engine::turn::TurnStart {
        number: 1,
        step: spellwake_engine::turn::Step::PrecombatMain,
    });
    static_inputs(&fixture).unwrap();
    let direct = initialize_fixture(&fixture).unwrap();
    let expected = Policy::new(&fixture)
        .unwrap()
        .prepare(direct.state())
        .unwrap_err();
    assert!(expected.ends_with(": unexpected opening/turn/step boundary"));
    captured(&fixture, || {
        let mut recorder = Recorder::new(fixture.case);
        let error = execute_fixture(&fixture, &mut recorder).unwrap_err();
        assert_eq!(error, expected);
        assert!(!recorder.report.completed());
        assert!(recorder.report.initialization.is_some());
        assert!(recorder.report.accepted.is_empty());
        assert_eq!(
            recorder.rules_journals,
            vec![direct.rules_journal().clone()]
        );
        let trace = recorder.report.native_trace.as_ref().unwrap();
        assert_eq!(trace.initialization, Ok(direct));
        assert!(trace.initial_installed);
        assert!(trace.calls.is_empty());
        assert!(
            trace.replay_verified,
            "post-initialization policy error skipped live-state native replay"
        );
        let output = emit(&recorder);
        assert_eq!(completion(&output, 0)["stage"], "Installed");
        assert_eq!(
            output["type_context_witness"]["calls"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            output["type_context_witness"]["calls"][0]["result"]["tag"],
            "Ok"
        );
    });
}

#[test]
fn failed_run_preserves_original_policy_diagnostic_and_verified_installed_initializer() {
    let mut fixture = fixture();
    fixture.initialization = FixtureInitialization::Scheduled(spellwake_engine::turn::TurnStart {
        number: 1,
        step: spellwake_engine::turn::Step::PrecombatMain,
    });
    let direct = initialize_fixture(&fixture).unwrap();
    let expected = Policy::new(&fixture)
        .unwrap()
        .prepare(direct.state())
        .unwrap_err();
    let vocabulary = fixture.setup.seed.creature_vocabulary.clone();
    crate::current_capture::with_selection_observer_for_test(&vocabulary, || {
        let report = run_fixture_at(fixture, 0);
        assert_eq!(report.failure.as_ref(), Some(&expected));
        assert!(!report.completed());
        assert!(report.rerun.is_none());
        assert!(report.completion.is_none());
        let trace = report.native_trace.as_ref().unwrap();
        assert_eq!(trace.initialization, Ok(direct));
        assert!(trace.initial_installed);
        assert!(trace.calls.is_empty());
        assert!(trace.replay_verified);
        let output = crate::current_capture::finish_record(
            "ordinary-staged-unit",
            json!({"observations":{"data":{"runs":[&report]}}}),
        )
        .unwrap();
        assert_eq!(completion(&output, 0)["stage"], "Installed");
        assert_eq!(
            output["observations"]["data"]["runs"][0]["failure"],
            expected
        );
        assert_eq!(
            output["type_context_witness"]["calls"][0]["result"]["tag"],
            "Ok"
        );
    });
}

#[test]
fn corrupted_delivery_replay_is_sticky_without_replacing_real_policy_error_or_native_result() {
    let mut fixture = fixture();
    fixture.initialization = FixtureInitialization::Scheduled(spellwake_engine::turn::TurnStart {
        number: 1,
        step: spellwake_engine::turn::Step::PrecombatMain,
    });
    captured(&fixture, || {
        let mut recorder = Recorder::new(fixture.case);
        let state = initialize_and_install(&fixture, &mut recorder).unwrap();
        let expected = Policy::new(&fixture).unwrap().prepare(&state).unwrap_err();
        verify_trace(&fixture, &mut recorder, Some(&state)).unwrap();
        let original = recorder
            .report
            .native_trace
            .as_ref()
            .unwrap()
            .initialization
            .clone();
        // Explicit negative delivery-metadata corruption. The actual native
        // Result, installed state and qualification caller metadata stay intact.
        recorder
            .report
            .native_trace
            .as_mut()
            .unwrap()
            .initial_installed = false;
        let error =
            finalize_execution(&fixture, &mut recorder, &state, true, Err(expected.clone()))
                .unwrap_err();
        assert_eq!(error, expected);
        assert!(!recorder.report.completed());
        let trace = recorder.report.native_trace.as_ref().unwrap();
        assert_eq!(trace.initialization, original);
        assert!(!trace.replay_verified);
        assert!(trace.calls.is_empty());
        let delivery = crate::current_capture::caller_delivery_for_test();
        assert!(
            delivery["calls"]
                .as_object()
                .unwrap()
                .values()
                .all(|call| call["completion"]["stage"] == "Installed")
        );
        let replay_error = "saved native delivery replay final full state differs";
        // Even an otherwise successful caller return cannot mark a corrupt
        // native trace complete. This is a negative gate control, not a game.
        assert_eq!(
            finalize_execution(&fixture, &mut recorder, &state, true, Ok(())).unwrap_err(),
            replay_error
        );
        assert!(!recorder.report.completed());
        assert_eq!(
            recorder
                .report
                .native_trace
                .as_ref()
                .unwrap()
                .initialization,
            original
        );
        assert_eq!(
            crate::current_capture::finish_record(
                "ordinary-staged-unit",
                json!({"observations":{"data":{"runs":[&recorder.report]}}}),
            )
            .unwrap_err(),
            replay_error
        );
    });
}
