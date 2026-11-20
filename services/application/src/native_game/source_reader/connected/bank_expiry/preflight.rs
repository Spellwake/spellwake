//! Genuine retained V2 bytes with explicitly synthetic accepted SQL metadata.
//! This preflight cannot authorize expiry or qualify actual SQL acceptance.
use super::*;
use crate::test_http::{Mock, Step};
use serde_json::Value;

pub(super) fn run(
    r: &Reservation,
    store: &NativeArtifactStore,
    programs: &NativeProgramSource,
    recovered: &RecoveredNativeGame,
    next: &NativeCheckpoint,
    original: &NativeSystemOriginal,
) {
    let repository = NativeSourceRepository::from_env().unwrap();
    let request = json!({"p_game":r.game_id});
    let mut source: Value = repository
        .backend
        .rpc("read_native_game_source", &request)
        .unwrap();
    let opening: Value = repository
        .backend
        .rpc("read_native_opening_source", &request)
        .unwrap();
    assert_eq!(source["data"]["acceptedRevision"], "0");
    let mut history: Value = repository.backend.rpc("read_native_transition_sources", &json!({
        "p_game":r.game_id,"p_context":r.context_id,"p_anchor_checkpoint":source["data"]["checkpoint"]["id"],"p_after_revision":null,"p_limit":32
    })).unwrap();
    let loser = next.terminal().unwrap().unwrap().losing_seat_ids()[0];
    let before = recovered
        .source()
        .clocks()
        .find(|(seat, ..)| *seat == loser)
        .unwrap();
    let future = crate::source_protocol::parse_utc_microsecond(before.4).unwrap()
        + chrono::Duration::milliseconds(1_801_000);
    let future = future.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string();
    let creator = Uuid::new_v4();
    let checkpoint = Uuid::new_v4();
    let revision = next.basis().revision;
    let sequence = recovered
        .source()
        .last_event_sequence()
        .checked_add(&Amount::from(1))
        .unwrap();
    let clock = before.2.checked_add(&Amount::from(1)).unwrap();
    source["data"]["acceptedRevision"] = json!(revision);
    source["data"]["gameState"] = json!("completed");
    source["data"]["lastEventSequence"] = json!(sequence);
    source["data"]["checkpoint"] = json!({"id":checkpoint,"acceptedRevision":revision,"stateRef":next.reference(),"clockRevision":clock,"creatorReceiptId":creator});
    for row in source["data"]["matchClocks"].as_array_mut().unwrap() {
        if row["seatId"] == json!(loser) {
            row["remainingMs"] = json!("0");
        }
        row["clockRevision"] = json!(clock);
        row["paused"] = json!(true);
        row["lastAccountedAt"] = json!(future);
    }
    history["data"]["anchor"] = json!({"gameId":r.game_id,"contextId":r.context_id,"anchorCheckpointId":checkpoint,"anchorStateRef":next.reference(),"anchorCreatorReceiptId":creator,"throughAcceptedRevision":revision,"throughEventSequence":sequence});
    let frame: Value = serde_json::from_str(
        recovered
            .prepare_bank_candidate(next)
            .unwrap()
            .original_json(),
    )
    .unwrap();
    history["data"]["groups"].as_array_mut().unwrap().push(json!({
        "checkpointId":checkpoint,"acceptedRevision":revision,"stateRef":next.reference(),"factsRef":next.prepare_results().unwrap().reference(),"creatorReceiptId":creator,"eventSchemaRevision":"native-transition-v1",
        "bankExpiry":{"originalRequestId":original.request_id(),"original":serde_json::from_str::<Value>(original.original_json()).unwrap(),"timeout":frame["bankTimeout"],"beforeClock":{"seatId":loser,"remainingMs":before.1,"clockRevision":before.2,"paused":before.3,"lastAccountedAt":before.4},"acceptedAt":future},
        "events":[{"eventId":Uuid::new_v4(),"sequence":sequence,"acceptedRevision":revision,"privatePayloadRef":next.prepare_event().unwrap().reference(),"acceptedAt":future,"creatorReceiptId":creator,"facts":[]}]
    }));
    evidence(
        "preflight-synthetic-sql.json",
        &serde_json::to_vec(&json!({"game":&source,"opening":&opening,"history":&history}))
            .unwrap(),
    );
    let mock = Mock::new(vec![
        Step::json("/rest/v1/rpc/read_native_game_source", source),
        Step::json("/rest/v1/rpc/read_native_opening_source", opening),
        Step::json("/rest/v1/rpc/read_native_transition_sources", history),
    ]);
    let repository = NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "bank-cold-preflight",
            Duration::from_secs(30),
        )
        .unwrap(),
    );
    let result = repository.recover_with_history(
        r.game_id,
        store,
        programs,
        NativeRecoveryBudget::new(100, 4_000_000).unwrap(),
    );
    if let Err(error) = &result {
        evidence(
            "preflight-error-chain.json",
            &serde_json::to_vec(&error_chain(error)).unwrap(),
        );
    }
    let cold = result.unwrap().unwrap();
    assert_eq!(cold.checkpoint().basis(), next.basis());
    assert_eq!(cold.history_evidence(), NativeHistoryEvidence::TransitionV1);
    assert_eq!(mock.finish().len(), 3);
}
