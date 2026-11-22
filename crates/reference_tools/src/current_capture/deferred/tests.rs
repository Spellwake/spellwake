//! Synthetic observer mechanism tests through actual native calls. Genuine
//! vocabulary is required; source/publication receipt metadata is synthetic.
//! These are not source-card, ordinary-report, reader or host qualification.
use super::*;
use serde_json::json;
use spellwake_engine::{
    combat::CombatRules,
    effects::{DecisionReply, SeedCard},
    game::{GameCommand, RuleFrame},
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination},
    priority::{PriorityPhase, SeatId},
    program::{AffectedPlayers, BodyTarget, ExecutableBody, Instruction, Program},
    stack_window::WindowPhase,
};
use std::{collections::BTreeSet, panic::AssertUnwindSafe, sync::Arc};

const A: SeatId = SeatId(41);
const B: SeatId = SeatId(3);

fn seed() -> (GameContext, GameSeed) {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "synthetic-deferred-caller".into(),
        vocabulary.rules_revision().into(),
        "synthetic-not-source-catalogue".into(),
        "observer-unit".into(),
        "native-mechanism-only".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let creature = || Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let cards = vec![
        SeedCard {
            card: CardId(1),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Discard {
                        players: AffectedPlayers::EachPlayer,
                        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    }],
                ),
            ),
        },
        SeedCard {
            card: CardId(2),
            owner: A,
            destination: ObjectDestination::Hand,
            program: creature(),
        },
        SeedCard {
            card: CardId(3),
            owner: B,
            destination: ObjectDestination::Hand,
            program: creature(),
        },
    ];
    (
        context,
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        ),
    )
}

fn pending_selection() -> GameState {
    assert!(!is_active());
    let (context, seed) = seed();
    let mut state = GameState::initialize(GameId(99401), context, seed)
        .unwrap()
        .into_state();
    let spell = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let input = GameInput {
        basis: state.basis().clone(),
        actor: A,
        command: GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
    };
    state = state.advance(input).unwrap().into_state();
    for _ in 0..2 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("actual ready priority required")
        };
        state = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: holder,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionSelect(_)
    ));
    state
}

fn reply(state: &GameState, valid: bool) -> GameInput {
    let pending = state.pending_decision().unwrap();
    let RuleFrame::ResolutionSelect(frame) = pending.frame() else {
        panic!()
    };
    GameInput {
        basis: state.basis().clone(),
        actor: pending.chooser(),
        command: GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::SelectResolution(if valid {
                vec![frame.domain()[0]]
            } else {
                vec![]
            }),
        },
    }
}

fn capture(scope: CaptureScope) {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let source = |path: &str| SourceRef {
        path: path.into(),
        sha256: "0".repeat(64),
        bytes: 0,
    };
    let recorder = Recorder::new_scoped(
        scope,
        &vocabulary,
        source("synthetic-input-receipt"),
        source("synthetic-publication-receipt"),
        vec![],
        RegistryWitness::NotApplicable,
    )
    .unwrap();
    ACTIVE.with(|active| {
        assert!(active.borrow().is_none());
        *active.borrow_mut() = Some(ActiveCapture {
            identity: capture_identity().unwrap(),
            profile: "resolution-selection-native".into(),
            recorder,
            points: vec![],
            calls: vec![],
            observations: BTreeMap::new(),
            record_pointers: vec![],
            sequence: 0,
            failure: None,
        });
    });
}

struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.borrow_mut().take();
        });
    }
}

fn record(rows: Vec<Value>) -> Value {
    finish_record("resolution-selection-native", json!({"accepted":rows})).unwrap()
}

fn call(value: &Value, ordinal: usize) -> &Value {
    &value["type_context_witness"]["calls"][ordinal]
}
fn completion(value: &Value, ordinal: usize) -> &Value {
    let id = call(value, ordinal)["call_id"].as_str().unwrap();
    &value["type_context_witness"]["caller_delivery"]["calls"][id]
}

#[test]
fn actual_selection_only_success_installs_once_with_returned_empty_append() {
    let _reset = Reset;
    let mut state = pending_selection();
    let source = state.clone();
    let input = reply(&state, true);
    let direct = source.advance(input.clone());
    capture(CaptureScope::ResolutionSelection11);
    let (native, ticket) = advance_deferred(&state, input.clone(), true);
    assert_eq!(native, direct);
    assert!(native.as_ref().unwrap().rules_journal().records.is_empty());
    let mut accepted = vec![];
    ticket
        .install(
            native.unwrap(),
            GameStateSlot::Advance(&mut state),
            "/accepted/0",
            || {
                accepted.push(json!({"synthetic_observer_row":0}));
            },
        )
        .unwrap();
    let output = record(accepted);
    assert_eq!(output["schema_version"], 11);
    assert_eq!(
        output["record_codec"],
        "spellwake.current-native-qualification.v2"
    );
    assert_eq!(
        output["type_context_witness"]["schema"],
        "spellwake.current-type-context-witness.v9"
    );
    assert_eq!(call(&output, 0)["adopted"], true);
    assert_eq!(call(&output, 0)["append"]["tag"], "RulesJournal");
    assert_eq!(completion(&output, 0)["completion"]["stage"], "Installed");
    assert_eq!(
        completion(&output, 0)["completion"]["accepted_append_pointer"],
        "/type_context_witness/calls/0/append"
    );
    assert_eq!(call(&output, 0).as_object().unwrap().len(), 9);
    assert_eq!(source.advance(input).unwrap().into_state(), state);
    assert_eq!(
        output["type_context_witness"]["calls"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "completion adds no native call"
    );
}

#[test]
fn returned_nonadopted_success_is_retained_and_next_call_uses_same_saved_source() {
    let _reset = Reset;
    let mut state = pending_selection();
    let source = state.clone();
    let input = reply(&state, true);
    capture(CaptureScope::ResolutionSelection11);
    let (first, ticket) = advance_deferred(&state, input.clone(), false);
    ticket
        .not_installed(NonInstallation::CallerDeclined)
        .unwrap();
    assert_eq!(state, source);
    let (second, ticket) = advance_deferred(&state, input.clone(), true);
    assert_eq!(first, second);
    ticket
        .install(
            second.unwrap(),
            GameStateSlot::Advance(&mut state),
            "/accepted/0",
            || {},
        )
        .unwrap();
    let output = record(vec![json!({"synthetic_observer_row":0})]);
    assert_eq!(call(&output, 0)["adopted"], false);
    assert!(call(&output, 0)["after_point"].is_string());
    assert_eq!(call(&output, 0)["result"]["tag"], "Ok");
    assert_eq!(
        completion(&output, 0)["completion"],
        json!({"stage":"CallerDeclined"})
    );
    assert_eq!(call(&output, 1)["adopted"], true);
    let mut replay = source;
    for (index, saved) in [input.clone(), input].into_iter().enumerate() {
        let actual = replay.advance(saved);
        assert_eq!(actual, first);
        if call(&output, index)["adopted"] == true {
            replay = actual.unwrap().into_state();
        }
    }
    assert_eq!(replay, state);
}

#[test]
fn native_error_retains_its_typed_result_and_never_becomes_a_caller_failure() {
    let _reset = Reset;
    let state = pending_selection();
    let source = state.clone();
    let input = reply(&state, false);
    let direct = state.advance(input.clone());
    assert!(direct.is_err());
    capture(CaptureScope::ResolutionSelection11);
    let (native, ticket) = advance_deferred(&state, input, true);
    assert_eq!(native, direct);
    ticket
        .not_installed(NonInstallation::PolicyAcceptFailed)
        .unwrap();
    let output = record(vec![]);
    assert_eq!(state, source);
    assert_eq!(call(&output, 0)["adopted"], false);
    assert!(call(&output, 0)["after_point"].is_null());
    assert_eq!(
        call(&output, 0)["append"],
        json!({"tag":"NoReturnedTransition","reason":"NativeError"})
    );
    assert_eq!(
        completion(&output, 0)["completion"],
        json!({"stage":"NativeError"})
    );
}

#[test]
fn explicit_preinstall_stops_keep_returned_success_without_installed_rows() {
    // These exercise ticket stages, not actual ordinary Recorder/Policy failures.
    let _reset = Reset;
    let state = pending_selection();
    for (reason, stage) in [
        (
            NonInstallation::RecorderPrepareFailed,
            "RecorderPrepareFailed",
        ),
        (NonInstallation::PolicyAcceptFailed, "PolicyAcceptFailed"),
        (
            NonInstallation::CallerAbortedBeforeInstall,
            "CallerAbortedBeforeInstall",
        ),
    ] {
        capture(CaptureScope::ResolutionSelection11);
        let (native, ticket) = advance_deferred(&state, reply(&state, true), true);
        assert!(native.is_ok());
        ticket.not_installed(reason).unwrap();
        let output = record(vec![]);
        assert_eq!(call(&output, 0)["result"]["tag"], "Ok");
        assert_eq!(call(&output, 0)["adopted"], false);
        assert_eq!(completion(&output, 0)["completion"], json!({"stage":stage}));
    }
}

#[test]
fn preinstall_unwind_and_dropped_error_keep_their_actual_native_outcomes() {
    let _reset = Reset;
    let state = pending_selection();
    for valid in [true, false] {
        capture(CaptureScope::ResolutionSelection11);
        let actual = state.advance(reply(&state, valid));
        let unwind = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let (native, _ticket) = advance_deferred(&state, reply(&state, valid), true);
            assert_eq!(native, actual);
            panic!("explicit synthetic preinstall unwind");
        }));
        assert!(unwind.is_err());
        active(|capture| {
            let id = &capture.calls[0];
            let output = capture.recorder.caller_delivery_value()?;
            assert_eq!(
                output["calls"][id]["completion"]["stage"],
                if valid {
                    "CallerAbortedBeforeInstall"
                } else {
                    "NativeError"
                }
            );
            assert_eq!(
                capture.recorder.retained_call(id).unwrap()["adopted"],
                false
            );
            assert!(capture.failure.as_ref().unwrap().contains("unwound"));
            Ok(())
        })
        .unwrap();
        assert!(finish_record("resolution-selection-native", json!({})).is_err());
    }
}

#[test]
fn postinstall_unwind_preserves_installed_state_and_sticky_observer_failure() {
    let _reset = Reset;
    let mut state = pending_selection();
    let input = reply(&state, true);
    let expected = state.advance(input.clone()).unwrap();
    capture(CaptureScope::ResolutionSelection11);
    let unwind = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let (native, ticket) = advance_deferred(&state, input, true);
        ticket
            .install(
                native.unwrap(),
                GameStateSlot::Advance(&mut state),
                "/accepted/0",
                || panic!("explicit synthetic postinstall commit unwind"),
            )
            .unwrap();
    }));
    assert!(unwind.is_err());
    assert_eq!(state, expected.into_state());
    active(|capture| {
        let output = capture.recorder.caller_delivery_value()?;
        let id = &capture.calls[0];
        assert_eq!(output["calls"][id]["completion"]["stage"], "Installed");
        assert_eq!(capture.recorder.retained_call(id).unwrap()["adopted"], true);
        assert!(capture.failure.as_ref().unwrap().contains("unwound"));
        Ok(())
    })
    .unwrap();
    assert!(finish_record("resolution-selection-native", json!({"accepted":[{}]})).is_err());
}

#[test]
fn wrong_source_or_full_candidate_cannot_be_installed_and_does_not_commit() {
    let _reset = Reset;
    let source = pending_selection();
    for wrong_candidate in [true, false] {
        capture(CaptureScope::ResolutionSelection11);
        let (native, ticket) = advance_deferred(&source, reply(&source, true), true);
        let native = native.unwrap();
        let successor = native.state().clone();
        let mut slot = if wrong_candidate {
            source.clone()
        } else {
            successor.clone()
        };
        let before = slot.clone();
        let candidate = if wrong_candidate {
            successor.advance(reply(&successor, true)).unwrap()
        } else {
            native
        };
        let mut committed = false;
        assert!(
            ticket
                .install(
                    candidate,
                    GameStateSlot::Advance(&mut slot),
                    "/accepted/0",
                    || {
                        committed = true;
                    }
                )
                .is_err()
        );
        assert!(!committed);
        assert_eq!(slot, before);
        assert!(finish_record("resolution-selection-native", json!({})).is_err());
    }
}

#[test]
fn foreign_missing_and_duplicate_completion_prevent_qualification() {
    let _reset = Reset;
    let source = pending_selection();
    capture(CaptureScope::ResolutionSelection11);
    let (_, ticket) = advance_deferred(&source, reply(&source, true), true);
    assert!(
        finish_record("resolution-selection-native", json!({}))
            .unwrap_err()
            .contains("not completed")
    );
    capture(CaptureScope::ResolutionSelection11);
    assert!(
        ticket
            .not_installed(NonInstallation::CallerDeclined)
            .unwrap_err()
            .contains("another capture")
    );
    assert!(finish_record("resolution-selection-native", json!({})).is_err());

    capture(CaptureScope::ResolutionSelection11);
    let (_, ticket) = advance_deferred(&source, reply(&source, true), false);
    ticket
        .not_installed(NonInstallation::CallerDeclined)
        .unwrap();
    let duplicate = active(|capture| {
        let id = &capture.calls[0];
        capture
            .recorder
            .complete_caller_delivery(id, CallerCompletion::CallerDeclined)
    });
    assert!(
        duplicate
            .as_ref()
            .unwrap_err()
            .contains("already completed")
    );
    remember(duplicate);
    assert!(finish_record("resolution-selection-native", json!({})).is_err());
}

#[test]
fn distinct_installed_calls_cannot_claim_the_same_accepted_record_row() {
    let _reset = Reset;
    let mut state = pending_selection();
    capture(CaptureScope::ResolutionSelection11);
    let (native, ticket) = advance_deferred(&state, reply(&state, true), true);
    ticket
        .install(
            native.unwrap(),
            GameStateSlot::Advance(&mut state),
            "/accepted/0",
            || {},
        )
        .unwrap();
    let before = state.clone();
    let (native, ticket) = advance_deferred(&state, reply(&state, true), true);
    let error = ticket
        .install(
            native.unwrap(),
            GameStateSlot::Advance(&mut state),
            "/accepted/0",
            || panic!("duplicate accepted row must fail before committing"),
        )
        .unwrap_err();
    assert!(error.contains("reuses an accepted record pointer"));
    assert_eq!(state, before);
    assert!(finish_record("resolution-selection-native", json!({"accepted":[{}]})).is_err());
}

#[test]
fn caught_invocation_unwind_cannot_fabricate_a_returned_native_result() {
    let _reset = Reset;
    capture(CaptureScope::ResolutionSelection11);
    let unwind = std::panic::catch_unwind(|| {
        let _guard = InvocationGuard::new();
        panic!("explicit invocation-guard unit; no actual engine panic injected");
    });
    assert!(unwind.is_err());
    active(|capture| {
        assert!(capture.calls.is_empty());
        assert_eq!(
            capture.failure.as_deref(),
            Some("native/observer invocation unwound before completion")
        );
        Ok(())
    })
    .unwrap();
    assert!(finish_record("resolution-selection-native", json!({})).is_err());
}

#[test]
fn reserved_witness_pointer_cannot_survive_final_witness_replacement() {
    let _reset = Reset;
    let mut state = pending_selection();
    let before = state.clone();
    capture(CaptureScope::ResolutionSelection11);
    let (native, ticket) = advance_deferred(&state, reply(&state, true), true);
    let pointer = "/type_context_witness/accepted/0";
    let result = ticket.install(
        native.unwrap(),
        GameStateSlot::Advance(&mut state),
        pointer,
        || {},
    );
    match result {
        Ok(()) => {
            let output = finish_record(
                "resolution-selection-native",
                json!({
                    "type_context_witness":{"accepted":[{}]},
                }),
            )
            .unwrap();
            assert!(
                output.pointer(pointer).is_some(),
                "Installed pointer passed the input-record check but dangles after witness replacement"
            );
        }
        Err(error) => {
            assert!(error.contains("reserved"), "wrong rejection path: {error}");
            assert_eq!(
                state, before,
                "reserved paths must reject before actual installation"
            );
            assert!(finish_record("resolution-selection-native", json!({})).is_err());
        }
    }
}

#[test]
fn initializer_and_advance_share_one_actual_installation_handoff() {
    let _reset = Reset;
    for scheduled in [false, true] {
        let (context, seed) = seed();
        capture(CaptureScope::ResolutionSelection11);
        let (native, ticket) = if scheduled {
            initialize_scheduled_deferred(
                GameId(99402),
                context,
                seed,
                TurnStart {
                    number: 2,
                    step: spellwake_engine::turn::Step::PrecombatMain,
                },
                true,
            )
        } else {
            initialize_deferred(GameId(99402), context, seed, true)
        };
        let expected = native.clone().unwrap();
        let mut slot = None;
        ticket
            .install(
                native.unwrap(),
                GameStateSlot::Initialize(&mut slot),
                "/initialization",
                || {},
            )
            .unwrap();
        assert_eq!(slot, Some(expected.into_state()));
        let output =
            finish_record("resolution-selection-native", json!({"initialization":{}})).unwrap();
        assert_eq!(call(&output, 0)["adopted"], true);
        assert_eq!(
            call(&output, 0)["replay"]["source_snapshot_before_invocation"],
            "InputOnlyForInitializer"
        );
        assert_eq!(completion(&output, 0)["completion"]["stage"], "Installed");
    }
}

#[test]
fn inactive_deferred_calls_preserve_native_results_and_installation() {
    let _reset = Reset;
    let mut state = pending_selection();
    let input = reply(&state, true);
    let expected = state.advance(input.clone());
    let (native, ticket) = advance_deferred(&state, input, true);
    assert_eq!(native, expected);
    ticket
        .install(
            native.unwrap(),
            GameStateSlot::Advance(&mut state),
            "/accepted/0",
            || {},
        )
        .unwrap();
    assert_eq!(state, expected.unwrap().into_state());
    assert!(!is_active());
}

#[test]
fn observer_failure_is_sticky_and_does_not_replace_result_or_erase_installation() {
    let _reset = Reset;
    let mut state = pending_selection();
    let input = reply(&state, true);
    let expected = state.advance(input.clone());
    capture(CaptureScope::ResolutionSelection11);
    remember(Err("first synthetic observer failure".into()));
    let (native, ticket) = advance_deferred(&state, input, true);
    assert_eq!(native, expected);
    ticket
        .install(
            native.unwrap(),
            GameStateSlot::Advance(&mut state),
            "/accepted/0",
            || {},
        )
        .unwrap();
    assert_eq!(state, expected.unwrap().into_state());
    assert_eq!(
        finish_record("resolution-selection-native", json!({"accepted":[{}]})).unwrap_err(),
        "first synthetic observer failure"
    );
}

#[test]
fn old_capture_epoch_keeps_masked_adoption_and_has_no_new_witness_member() {
    let _reset = Reset;
    let (context, mut seed) = seed();
    seed.cards.retain(|card| card.card != CardId(1));
    let source = GameState::initialize(GameId(99403), context, seed)
        .unwrap()
        .into_state();
    for (adopt, valid) in [(false, true), (true, true), (true, false)] {
        capture(CaptureScope::Current10);
        let input = GameInput {
            basis: source.basis().clone(),
            actor: if valid { A } else { B },
            command: GameCommand::Pass,
        };
        assert_eq!(
            advance_with_adoption(&source, input.clone(), adopt),
            source.advance(input)
        );
        let output = record(vec![]);
        assert_eq!(output["schema_version"], 10);
        assert_eq!(
            output["type_context_witness"]["schema"],
            "spellwake.current-type-context-witness.v8"
        );
        assert!(
            output["type_context_witness"]
                .get("caller_delivery")
                .is_none()
        );
        assert_eq!(call(&output, 0)["adopted"], adopt && valid);
    }
}
