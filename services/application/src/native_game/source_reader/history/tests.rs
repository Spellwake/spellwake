//! Adversarial SQL wire/source accounting. No actual native artifacts are claimed.
use super::super::tests::{fixture, game, repository};
use super::*;
use crate::test_http::{Mock, Step};
use serde_json::Value;
const PATH: &str = "/rest/v1/rpc/read_native_transition_sources";
fn context() -> Uuid {
    Uuid::from_u128(0x125)
}
fn reference(role: NativeArtifactRole, n: u64) -> NativeArtifactRef {
    NativeArtifactRef::for_bytes(game(), context(), role, &n.to_be_bytes()).unwrap()
}
fn selected() -> Value {
    let mut value = fixture();
    value["data"]["acceptedRevision"] = json!("1");
    value["data"]["checkpoint"]["acceptedRevision"] = json!("1");
    value["data"]["checkpoint"]["stateRef"] = json!(reference(NativeArtifactRole::State, 1));
    value["data"]["lastEventSequence"] = json!("2");
    value
}
fn header() -> Value {
    let s = selected();
    let d = &s["data"];
    json!({"gameId":game(),"contextId":context(),"anchorCheckpointId":d["checkpoint"]["id"],"anchorStateRef":d["checkpoint"]["stateRef"],"anchorCreatorReceiptId":d["checkpoint"]["creatorReceiptId"],"throughAcceptedRevision":"1","throughEventSequence":"2"})
}
fn event(sequence: u64, revision: u64, creator: Uuid) -> Value {
    json!({"eventId":Uuid::from_u128(0x150+u128::from(sequence)),"sequence":sequence.to_string(),"acceptedRevision":revision.to_string(),"privatePayloadRef":reference(NativeArtifactRole::Event,sequence),"acceptedAt":"2026-11-06T00:00:00.000000Z","creatorReceiptId":creator,"facts":[]})
}
fn pages() -> Vec<Value> {
    let h = header();
    let initial_creator = Uuid::from_u128(0x142);
    let final_creator = Uuid::from_u128(0x130);
    let first = json!({"checkpointId":Uuid::from_u128(0x141),"acceptedRevision":"0","stateRef":reference(NativeArtifactRole::State,0),"factsRef":reference(NativeArtifactRole::Facts,0),"creatorReceiptId":initial_creator,"eventSchemaRevision":null,"events":[event(0,0,initial_creator)]});
    let mut last = event(2, 1, final_creator);
    last["facts"] = json!([{"accountId":Uuid::from_u128(0x131),"kind":"fixture:count","factRevision":"fixture:v1","value":"9007199254740993"}]);
    let second = json!({"checkpointId":h["anchorCheckpointId"],"acceptedRevision":"1","stateRef":h["anchorStateRef"],"factsRef":reference(NativeArtifactRole::Facts,1),"creatorReceiptId":final_creator,"eventSchemaRevision":null,"events":[event(1,1,final_creator),last]});
    vec![
        json!({"status":"success","data":{"anchor":h,"groups":[first],"nextAfterRevision":"0"}}),
        json!({"status":"success","data":{"anchor":header(),"groups":[second],"nextAfterRevision":null}}),
    ]
}
fn execute(
    values: Vec<Value>,
    records: usize,
    maximum: u64,
    padding: bool,
) -> (
    Result<Option<NativeTransitionSources>, NativeSourceError>,
    NativeRecoveryBudget,
    u64,
) {
    let mut steps = vec![Step::json(
        "/rest/v1/rpc/read_native_game_source",
        selected(),
    )];
    let mut bytes = 0u64;
    for (index, value) in values.into_iter().enumerate() {
        let mut step = Step::json(PATH, value);
        if padding {
            let mut body = b" \n".to_vec();
            body.extend(step.body);
            body.extend(b"\t ");
            step.body = body;
        }
        bytes += u64::try_from(step.body.len()).unwrap();
        step.expect=Some(serde_json::to_vec(&json!({"p_game":game(),"p_context":context(),"p_anchor_checkpoint":header()["anchorCheckpointId"],"p_after_revision":if index==0{Value::Null}else{json!("0")},"p_limit":32})).unwrap());
        steps.push(step);
    }
    let mock = Mock::new(steps);
    let repo = repository(&mock);
    let source = repo.read(game()).unwrap().unwrap();
    let mut budget = NativeRecoveryBudget::new(records, maximum).unwrap();
    let result = repo.read_transition_sources(&source, &mut budget);
    mock.finish();
    (result, budget, bytes)
}
#[test]
fn all_pages_keep_anchor_native_revisions_event_order_and_exact_large_fact_values() {
    let values = pages();
    let raw_bytes = values
        .iter()
        .map(|v| serde_json::to_vec(v).unwrap().len() + 4)
        .sum::<usize>();
    let (result, mut budget, observed) =
        execute(values, 8, u64::try_from(raw_bytes).unwrap(), true);
    assert_eq!(observed, u64::try_from(raw_bytes).unwrap());
    let source = result.unwrap().unwrap();
    assert_eq!(source.checkpoint_count(), 2);
    assert_eq!(source.through_revision().get(), 1);
    assert_eq!(source.through_event_sequence(), &Amount::from(2));
    assert!(!source.all_claim_transition_codec());
    let facts: Vec<_> = source.facts().collect();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].value.to_string(), "9007199254740993");
    assert_eq!(facts[0].event_sequence, &Amount::from(2));
    assert_eq!(budget.remaining_bytes(), 0);
    assert!(budget.charge_work(1, 0).is_err());
}
#[test]
fn first_page_drift_gaps_bad_roles_and_missing_fields_never_request_another_page() {
    let first = pages().remove(0);
    let mut cases = Vec::new();
    for (path, value) in [
        ("/data/anchor/contextId", json!(Uuid::from_u128(999))),
        ("/data/anchor/throughEventSequence", json!("3")),
        ("/data/groups/0/acceptedRevision", json!("1")),
        (
            "/data/groups/0/stateRef",
            json!(reference(NativeArtifactRole::Facts, 0)),
        ),
        ("/data/groups/0/events/0/sequence", json!("1")),
        (
            "/data/groups/0/events/0/creatorReceiptId",
            json!(Uuid::from_u128(999)),
        ),
        ("/data/groups/0/events/0/acceptedAt", json!("2026-11-06")),
        ("/data/groups/0/eventSchemaRevision", json!("unknown")),
        ("/data/nextAfterRevision", Value::Null),
        ("/data/nextAfterRevision", json!("1")),
    ] {
        let mut v = first.clone();
        *v.pointer_mut(path).unwrap() = value;
        cases.push(v);
    }
    let mut missing = first.clone();
    missing["data"]["groups"][0]
        .as_object_mut()
        .unwrap()
        .remove("eventSchemaRevision");
    cases.push(missing);
    let mut positional = first.clone();
    positional["data"]["groups"][0] = json!([]);
    cases.push(positional);
    let mut extra = first.clone();
    extra["data"]["groups"][0]["extra"] = json!(true);
    cases.push(extra);
    for value in cases {
        assert!(execute(vec![value], 100, 1_000_000, false).0.is_err());
    }
}
#[test]
fn final_page_header_ids_cursor_and_fact_keys_are_bound_to_the_selected_prefix() {
    let base = pages();
    let last = base[1].clone();
    let mut cases = Vec::new();
    for (path, value) in [
        (
            "/data/anchor/anchorCheckpointId",
            json!(Uuid::from_u128(888)),
        ),
        ("/data/groups/0/checkpointId", json!(Uuid::from_u128(888))),
        ("/data/groups/0/acceptedRevision", json!("0")),
        (
            "/data/groups/0/creatorReceiptId",
            json!(Uuid::from_u128(0x142)),
        ),
        (
            "/data/groups/0/events/1/eventId",
            last["data"]["groups"][0]["events"][0]["eventId"].clone(),
        ),
        (
            "/data/groups/0/events/1/facts/0/accountId",
            json!(Uuid::from_u128(888)),
        ),
        ("/data/groups/0/events/1/facts/0/value", json!(3)),
        (
            "/data/groups/0/eventSchemaRevision",
            json!("native-transition-v1"),
        ),
        ("/data/nextAfterRevision", json!("1")),
    ] {
        let mut v = last.clone();
        *v.pointer_mut(path).unwrap() = value;
        cases.push(v);
    }
    let mut duplicated = last.clone();
    let fact = duplicated["data"]["groups"][0]["events"][1]["facts"][0].clone();
    duplicated["data"]["groups"][0]["events"][1]["facts"]
        .as_array_mut()
        .unwrap()
        .push(fact);
    cases.push(duplicated);
    for value in cases {
        assert!(
            execute(vec![base[0].clone(), value], 100, 1_000_000, false)
                .0
                .is_err()
        );
    }
}
#[test]
fn work_exhaustion_and_unavailable_never_return_a_partial_prefix() {
    let first = pages().remove(0);
    assert!(execute(vec![first], 2, 1_000_000, false).0.is_err());
    let unavailable = json!({"status":"fail","data":{"game":"unavailable"}});
    assert!(
        execute(vec![pages().remove(0), unavailable], 100, 1_000_000, false)
            .0
            .unwrap()
            .is_none()
    );
}

#[test]
fn later_raw_gaps_duplicates_removal_and_reordering_refuse_the_selected_prefix() {
    let original = pages();
    for kind in ["gap", "duplicate", "remove", "reorder"] {
        let mut values = original.clone();
        let events = values[1]["data"]["groups"][0]["events"]
            .as_array_mut()
            .unwrap();
        match kind {
            "gap" => events[0]["sequence"] = json!("2"),
            "duplicate" => events[1]["sequence"] = json!("1"),
            "remove" => {
                events.remove(0);
            }
            "reorder" => events.swap(0, 1),
            _ => unreachable!(),
        }
        let error = execute(values, 100, 1_000_000, false).0.err().expect(kind);
        assert!(matches!(error.0, Failure::Invalid), "{kind}");
    }
}

#[test]
fn bank_history_survives_the_actual_rpc_success_envelope_without_buffering_its_original() {
    // Synthetic SQL transport only. No elapsed-time or native conclusion claim.
    let mut values = pages();
    values[1]["data"]["groups"][0]["bankExpiry"] = json!({
        "originalRequestId":Uuid::from_u128(0x199),
        "original":{"version":"native-host-input-v1","basis":{"gameId":game(),"contextId":context(),"epoch":"0","revision":"0"},"kind":"clock","inputRef":reference(NativeArtifactRole::Input,1),"runtimeId":"a".repeat(64),"programSourceId":"b".repeat(64)},
        "timeout":{"kind":"overall_bank","seatId":Uuid::from_u128(0x131),"clockRevision":"1","proofRef":reference(NativeArtifactRole::Input,1)},
        "beforeClock":{"seatId":Uuid::from_u128(0x131),"remainingMs":"1","clockRevision":"1","paused":false,"lastAccountedAt":"2026-11-16T00:00:00.000000Z"},
        "acceptedAt":"2026-11-16T00:00:00.001000Z"
    });
    let (result, _, _) = execute(values, 100, 8_000_000, false);
    let sources = result.unwrap().unwrap();
    assert!(sources.groups[0].bank_expiry.is_none());
    assert!(sources.groups[1].bank_expiry.is_some());
}

#[test]
fn direct_history_envelope_stays_closed_and_failure_fields_keep_unique_keys() {
    let good = pages().remove(0);
    let text = serde_json::to_string(&good).unwrap();
    let duplicate_status = text.replacen(
        "\"status\":\"success\"",
        "\"status\":\"success\",\"status\":\"success\"",
        1,
    );
    let duplicate_data = text.replacen("\"data\":", "\"data\":{},\"data\":", 1);
    for raw in [
        duplicate_status,
        duplicate_data,
        "{\"status\":\"success\",\"data\":[]}".into(),
    ] {
        assert!(decode_page_response(raw.as_bytes()).is_err());
    }
    let mut extra = good;
    extra["extra"] = json!(true);
    assert!(decode_page_response(&serde_json::to_vec(&extra).unwrap()).is_err());
    assert!(matches!(
        decode_page_response(br#"{"status":"fail","data":{"game":"unavailable"}}"#).unwrap(),
        ApiResponse::Fail { .. }
    ));
    assert!(
        decode_page_response(
            br#"{"status":"fail","data":{"game":"unavailable","game":"unavailable"}}"#
        )
        .is_err()
    );
    assert!(matches!(
        decode_page_response(br#"{"status":"error","message":"unavailable","code":"fixture"}"#)
            .unwrap(),
        ApiResponse::Error { .. }
    ));
}
