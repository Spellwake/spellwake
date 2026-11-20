//! Public repository regression with a genuine native chain and strict source/Storage mocks.
//! SQL source responses are controlled fixtures, not actual accepted timeout proof.
use super::*;
use crate::native_game::initial_source::tests::{CheckpointFixture, ids, source, spec};
use crate::test_http::Step;

pub(super) fn recover(
    f: &CheckpointFixture,
) -> Result<Option<RecoveredNativeGame>, NativeSourceError> {
    recover_metadata(f, |_| {})
}
pub(super) fn recover_metadata(
    f: &CheckpointFixture,
    edit: impl FnOnce(&mut Value),
) -> Result<Option<RecoveredNativeGame>, NativeSourceError> {
    let programs = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let initial =
        NativeCheckpoint::initial(prepare_initial(ids(), programs, spec()).unwrap()).unwrap();
    assert!(initial.reference() == &f.states[0].0);
    let b = f.tip.basis();
    let matched = Uuid::from_u128(0xa1);
    let context_revision = "fixture:bank-state-only";
    let seats: Vec<Value> = f.tip.roster.iter().enumerate().map(|(index, seat)| json!({
        "seatId":seat.seat_id,"seatIndex":index.to_string(),"accountId":seat.seat_id,"controllerKind":"human",
        "sourceRevisionId":Uuid::from_u128(0x1000+u128::try_from(index).unwrap()),"deckSnapshotRef":"fixture:original-copy"
    })).collect();
    let waiter = f
        .tip
        .state()
        .pending_decision()
        .map(|v| v.chooser())
        .or_else(|| match f.tip.state().execution().window().phase() {
            spellwake_engine::stack_window::WindowPhase::Priority(
                spellwake_engine::priority::PriorityPhase::Ready { holder },
            ) if f.tip.state().outcome().is_none() => Some(holder),
            _ => None,
        });
    let clocks: Vec<Value> = f
        .tip
        .roster
        .iter()
        .map(|seat| {
            json!({"seatId":seat.seat_id,
        "policyRef":"bo3-overall-bank-v1","remainingMs":"1700000","clockRevision":"4","paused":waiter != Some(seat.native_seat),
        "lastAccountedAt":"2026-11-16T00:00:00.001400Z"})
        })
        .collect();
    let mut metadata = json!({"status":"success","data":{"version":"native-game-source-v1",
        "gameId":b.game_id,"matchId":matched,"contextId":b.context_id,"releaseId":ids().release_id,"profileId":ids().profile_id,
        "gameState":if f.tip.state().outcome().is_some(){"completed"}else{"active"},"matchState":"active","matchAvailability":"available",
        "acceptedRevision":b.revision,"contextRevision":context_revision,"initialRef":f.tip.initial_reference(),
        "checkpoint":{"id":Uuid::from_u128(0xa2),"acceptedRevision":b.revision,"stateRef":f.tip.reference(),"clockRevision":"4","creatorReceiptId":Uuid::from_u128(0xa3)},
        "operational":{"ownerRef":"fixture:owner","fenceEpoch":"1","revision":"0","availability":"available","recoveryCheckpointId":null},
        "seats":seats,"matchClocks":clocks,"lastEventSequence":b.revision}});
    edit(&mut metadata);
    let opening = json!({"status":"success","data":{"version":"native-opening-v1","matchId":matched,"gameId":b.game_id,
        "owner":"fixture:owner","originalBasis":"0","opening":{"contextId":b.context_id,"snapshotRef":initial.initial_reference(),
        "contextRevision":context_revision,"stateRef":initial.reference(),"factsRef":initial.prepare_results().unwrap().reference(),
        "eventRef":initial.prepare_event().unwrap().reference(),"qualificationRef":"fixture:mechanism-only",
        "eventSchemaRevision":"native-transition-v1","clockObservation":initial.opening_clock_observation().unwrap()},
        "nativeEpoch":initial.basis().epoch,"runtimeId":native_runtime_identity(),"programSourceId":programs.identity().hex(),
        "childReceiptId":Uuid::from_u128(0xa4),"receiptId":Uuid::from_u128(0xa5),"contextId":b.context_id,"checkpointId":Uuid::from_u128(0xa6)}});
    let mut steps = vec![
        Step::json("/rest/v1/rpc/read_native_game_source", metadata),
        Step::json("/rest/v1/rpc/read_native_opening_source", opening),
    ];
    // Existing basic recovery reads original once for opening correlation, then
    // load_checkpoint reads the complete State chain and original source again.
    steps.extend(checkpoint_read_steps(
        initial.initial_reference(),
        &f.original,
    ));
    for (reference, bytes) in f.states.iter().rev() {
        steps.extend(checkpoint_read_steps(reference, bytes));
    }
    steps.extend(checkpoint_read_steps(
        initial.initial_reference(),
        &f.original,
    ));
    for (reference, bytes) in &f.inputs {
        steps.extend(checkpoint_read_steps(reference, bytes));
    }
    let count = steps.len();
    let mock = Mock::new(steps);
    let repository = NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "bank-recovery-test",
            Duration::from_secs(2),
        )
        .unwrap(),
    );
    let recovered = repository.recover(
        b.game_id,
        &checkpoint_store(&mock),
        programs,
        NativeRecoveryBudget::new(100, 100_000_000).unwrap(),
    );
    assert_eq!(
        mock.finish().len(),
        count,
        "public recovery must consume the complete exact original input chain"
    );
    recovered
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; public recovery with mocked SQL/Storage"]
fn public_state_only_recovery_keeps_ordinary_inputs_and_refuses_bank_before_delivery() {
    let mut f = checkpoint_fixture(None);
    let ordinary = recover(&f).unwrap().unwrap();
    assert_eq!(
        ordinary.history_evidence(),
        NativeHistoryEvidence::StateOnly
    );
    assert_eq!(ordinary.checkpoint().state(), f.tip.state());
    let (next, _, bytes) = cut_from(&f);
    let input = next.retained_input().unwrap().2.clone();
    f.inputs.push((input, bytes));
    f.states
        .push((next.reference().clone(), next.original_bytes().to_vec()));
    f.candidates.push(next.candidate().clone());
    f.tip = next;
    let error = match recover(&f) {
        Err(error) => error,
        Ok(_) => panic!("bank state escaped basic recovery"),
    };
    // Native decoding/input failures keep a nested cause. The final source-scope
    // refusal has none, proving this exercised the post-reconstruction gate.
    assert!(std::error::Error::source(&error).is_none());
}
