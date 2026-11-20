//! Private wire mechanics only. SQL/native qualification has independent evidence.
use super::*;
use crate::test_http::{Mock, Step, positional};
use serde_json::Value;
const PATH: &str = "/rest/v1/rpc/read_native_room_source";
fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}
fn fixture() -> Value {
    json!({"status":"success","data":{"sourceHash":"a".repeat(64),"terms":{
        "version":"native-room-source-v1",
        "room":{"id":id(1),"revision":"0","hostId":id(2),"state":"lobby","configurationId":id(3),"profileId":id(4),"formatId":id(5),"mode":"open","competitive":false,"structure":"bo1","countdownAt":null},
        "profile":{"id":id(4),"key":"fixture","revisionRef":"fixture:profile","rulesRef":"fixture:rules","planarPoolRef":null,"minimumSeats":"2","seatCapacity":"2","qualificationRef":"fixture:SQL-only"},
        "release":{"id":id(6),"releaseRef":"fixture:release","kernelRef":"fixture:kernel","rulesRef":"fixture:rules","programManifestRef":"fixture:manifest","evidenceRef":"fixture:SQL-only"},
        "legalityRevisionId":id(7),
        "slots":[{"slotId":id(8),"seatIndex":"0","accountId":id(2),"sponsorId":null,"controllerKind":"human","teamRef":null,"ready":false,"sourceStatus":"available","selectedRevisionId":id(9),
            "entries":[{"entryId":id(10),"definitionId":id(11),"oracleRef":"fixture:oracle","definitionRevisionId":id(12),"definitionRevisionRef":"fixture:body","rulesTextRef":"fixture:rules","programRef":"fixture:program","unlimitedBasicLand":true,"printing":null,"section":"main","quantity":"1000"}]},
            {"slotId":id(13),"seatIndex":"1","accountId":null,"sponsorId":id(2),"controllerKind":"ai","teamRef":null,"ready":false,"sourceStatus":"unselected","selectedRevisionId":null,"entries":null}]
    }}})
}
fn between_fixture() -> Value {
    let shared = fixture();
    json!({"status":"success","data":{"sourceHash":"b".repeat(64),"terms":{
        "version":"native-between-source-v1",
        "decision":{"id":id(30),"matchId":id(20),"seatId":id(21),"afterGameId":id(22),"basisRevision":"3","clockRevision":"5","deadlineRevision":"0","deadlineAt":null,"state":"pending"},
        "match":{"id":id(20),"revision":"4","mode":"open","formatId":id(5),"profileId":id(4),"legalityRevisionId":id(7),"releaseId":id(6),"availabilityRevision":"0","availability":"available","structure":"bo3","startedAt":"2026-11-06T00:00:00.000000Z"},
        "profile":shared["data"]["terms"]["profile"],"release":shared["data"]["terms"]["release"],
        "originalRevisionId":id(9),"originalSnapshotRef":"fixture:immutable-original-pool","selectedRevisionId":id(29),"entries":shared["data"]["terms"]["slots"][0]["entries"]
    }}})
}
pub(in crate::native_game) fn reserved_fixture(successor: bool) -> Value {
    let shared = fixture();
    let seats:Vec<Value>=(0..2).map(|index|json!({"seatId":id(40+index),"seatIndex":index.to_string(),"accountId":if index==0{Some(id(2))}else{None},"controllerKind":if index==0{"human"}else{"ai"},"sponsorId":if index==0{None}else{Some(id(2))},"originalRevisionId":id(9),"originalSnapshotRef":"fixture:original","selectedRevisionId":if successor{id(29)}else{id(9)},"selectionDecisionId":if successor{Some(id(60+index))}else{None},"selectionReplyRef":if successor{Some("fixture:answered-selection")}else{None},"entries":shared["data"]["terms"]["slots"][0]["entries"]})).collect();
    json!({"status":"success","data":{"version":"native-admission-source-v1","gameId":id(35),"matchId":id(20),"roomId":id(1),"gameIndex":if successor{"42"}else{"0"},"matchRevision":"6","mode":"open","formatId":id(5),"legalityRevisionId":id(7),"competitive":false,"structure":if successor{"bo3"}else{"bo1"},"admissionQualificationRef":"fixture:admission","profile":shared["data"]["terms"]["profile"],"release":shared["data"]["terms"]["release"],"seats":seats}})
}
#[test]
fn between_and_reserved_sources_preserve_original_pool_selection_and_draw_successors() {
    let mut between_step = Step::json("/rest/v1/rpc/read_native_between_source", between_fixture());
    between_step.expect = Some(
        serde_json::to_vec(
            &json!({"p_decision":id(30),"p_basis":"4","p_selected_revision":id(29)}),
        )
        .unwrap(),
    );
    let mock = Mock::new(vec![
        between_step,
        Step::json(
            "/rest/v1/rpc/read_native_admission_source",
            reserved_fixture(false),
        ),
        Step::json(
            "/rest/v1/rpc/read_native_admission_source",
            reserved_fixture(true),
        ),
    ]);
    let repo = repository(&mock);
    assert!(
        repo.between(Uuid::nil(), &Amount::from(4u64), id(29))
            .is_err()
    );
    assert!(repo.reserved_game(Uuid::nil()).is_err());
    let selected = repo
        .between(id(30), &Amount::from(4u64), id(29))
        .unwrap()
        .unwrap();
    assert_eq!(selected.terms().original_revision_id, id(9));
    assert_eq!(selected.terms().selected_revision_id, id(29));
    assert_eq!(
        selected.terms().original_snapshot_ref.as_str(),
        "fixture:immutable-original-pool"
    );
    let first = repo.reserved_game(id(35)).unwrap().unwrap();
    assert!(first.metadata().game_index.is_zero());
    assert!(first.metadata().seats[0].selection_decision_id.is_none());
    let later = repo.reserved_game(id(35)).unwrap().unwrap();
    assert_eq!(later.metadata().game_index, Amount::from(42u64));
    assert_eq!(later.metadata().seats[0].selected_revision_id, id(29));
    assert_eq!(later.metadata().seats[0].original_revision_id, id(9));
    assert_eq!(mock.finish().len(), 3);
}
#[test]
fn between_source_rejects_wrong_scope_frontier_and_deadline_shape() {
    let good = between_fixture();
    let path = "/rest/v1/rpc/read_native_between_source";
    let mut cases = Vec::new();
    for (pointer, value) in [
        ("/data/terms/decision/id", json!(id(99))),
        ("/data/terms/decision/matchId", json!(id(99))),
        ("/data/terms/decision/basisRevision", json!("5")),
        ("/data/terms/decision/state", json!("answered")),
        ("/data/terms/match/revision", json!("5")),
        ("/data/terms/match/profileId", json!(id(99))),
        ("/data/terms/match/releaseId", json!(id(99))),
        ("/data/terms/match/availability", json!("recovering")),
        ("/data/terms/match/structure", json!("bo1")),
        ("/data/terms/match/startedAt", json!("2026-11-06")),
        ("/data/terms/decision/deadlineRevision", json!("1")),
        (
            "/data/terms/decision/deadlineAt",
            json!("2026-11-06T01:00:00.000000Z"),
        ),
        ("/data/terms/selectedRevisionId", json!(id(99))),
        ("/data/terms/entries", json!([])),
    ] {
        let mut changed = good.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        cases.push(changed);
    }
    let mut positional_match = good.clone();
    positional_match["data"]["terms"]["match"] = positional(
        &good["data"]["terms"]["match"],
        &[
            "id",
            "revision",
            "mode",
            "formatId",
            "profileId",
            "legalityRevisionId",
            "releaseId",
            "availabilityRevision",
            "availability",
            "structure",
            "startedAt",
        ],
    );
    cases.push(positional_match);
    let mut missing = good.clone();
    missing["data"]["terms"]["decision"]
        .as_object_mut()
        .unwrap()
        .remove("deadlineAt");
    cases.push(missing);
    let count = cases.len();
    let mock = Mock::new(
        cases
            .into_iter()
            .map(|value| Step::json(path, value))
            .collect(),
    );
    let repo = repository(&mock);
    for _ in 0..count {
        assert!(repo.between(id(30), &Amount::from(4u64), id(29)).is_err());
    }
    assert_eq!(mock.finish().len(), count);
}
#[test]
fn reserved_source_rejects_impossible_roster_or_selection_without_a_three_game_cap() {
    let good = reserved_fixture(false);
    let mut cases = Vec::new();
    let path = "/rest/v1/rpc/read_native_admission_source";
    for (pointer, value) in [
        ("/data/gameId", json!(id(99))),
        ("/data/version", json!("native-admission-source-v2")),
        ("/data/gameIndex", json!("1")),
        ("/data/competitive", json!(true)),
        ("/data/seats/1/seatId", json!(id(40))),
        ("/data/seats/1/seatIndex", json!("0")),
        ("/data/seats/1/seatIndex", json!("2")),
        ("/data/seats/0/accountId", Value::Null),
        ("/data/seats/0/sponsorId", json!(id(2))),
        ("/data/seats/1/sponsorId", Value::Null),
        ("/data/seats/0/selectedRevisionId", json!(id(29))),
        ("/data/seats/0/selectionDecisionId", json!(id(60))),
        ("/data/seats/0/entries", json!([])),
    ] {
        let mut changed = good.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        cases.push(changed);
    }
    for field in ["selectionDecisionId", "selectionReplyRef"] {
        let mut changed = reserved_fixture(true);
        changed["data"]["seats"][0][field] = Value::Null;
        cases.push(changed);
        let mut changed = good.clone();
        changed["data"]["seats"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        cases.push(changed);
    }
    let mut positional_seat = good.clone();
    positional_seat["data"]["seats"][0] = positional(
        &good["data"]["seats"][0],
        &[
            "seatId",
            "seatIndex",
            "accountId",
            "controllerKind",
            "sponsorId",
            "originalRevisionId",
            "originalSnapshotRef",
            "selectedRevisionId",
            "selectionDecisionId",
            "selectionReplyRef",
            "entries",
        ],
    );
    cases.push(positional_seat);
    let count = cases.len();
    let mock = Mock::new(
        cases
            .into_iter()
            .map(|value| Step::json(path, value))
            .collect(),
    );
    let repo = repository(&mock);
    for _ in 0..count {
        assert!(repo.reserved_game(id(35)).is_err());
    }
    assert_eq!(mock.finish().len(), count);
}
#[test]
fn preparation_source_absence_and_failure_vocabularies_are_family_specific() {
    let between = "/rest/v1/rpc/read_native_between_source";
    let reserved = "/rest/v1/rpc/read_native_admission_source";
    let mock = Mock::new(vec![
        Step::json(
            between,
            json!({"status":"fail","data":{"source":"unavailable"}}),
        ),
        Step::json(
            between,
            json!({"status":"fail","data":{"game":"unavailable"}}),
        ),
        Step::json(
            reserved,
            json!({"status":"fail","data":{"game":"unavailable"}}),
        ),
        Step::json(
            reserved,
            json!({"status":"fail","data":{"source":"unavailable"}}),
        ),
    ]);
    let repo = repository(&mock);
    assert!(
        repo.between(id(30), &Amount::from(4u64), id(29))
            .unwrap()
            .is_none()
    );
    assert!(repo.between(id(30), &Amount::from(4u64), id(29)).is_err());
    assert!(repo.reserved_game(id(35)).unwrap().is_none());
    assert!(repo.reserved_game(id(35)).is_err());
    assert_eq!(mock.finish().len(), 4);
}
fn repository(mock: &Mock) -> NativePreparationRepository {
    NativePreparationRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "native-preparation-test",
            Duration::from_secs(1),
        )
        .unwrap(),
    )
}
fn read(
    repository: &NativePreparationRepository,
) -> Result<Option<NativeRoomSource>, NativePreparationError> {
    repository.room(id(1), &Amount::from(0u64), id(6), id(7))
}
#[test]
fn exact_source_preserves_unselected_opponent_nullable_printing_and_maximum_quantity() {
    let mut step = Step::json(PATH, fixture());
    step.expect = Some(
        serde_json::to_vec(
            &json!({"p_room":id(1),"p_basis":"0","p_release":id(6),"p_legality":id(7)}),
        )
        .unwrap(),
    );
    let mock = Mock::new(vec![step]);
    let repo = repository(&mock);
    assert!(
        repo.room(Uuid::nil(), &Amount::from(0u64), id(6), id(7))
            .is_err()
    );
    let source = read(&repo).unwrap().unwrap();
    assert_eq!(source.source_hash(), "a".repeat(64));
    let entries = source.terms().slots[0].entries.as_ref().unwrap();
    assert_eq!(entries[0].quantity, Amount::from(1000u64));
    assert!(entries[0].printing.is_none());
    assert!(matches!(
        source.terms().slots[1].source_status,
        PreparationStatus::Unselected
    ));
    assert_eq!(mock.finish().len(), 1);
}
#[test]
fn partial_room_and_unsupported_selected_deck_remain_observations_not_qualification() {
    let mut partial = fixture();
    partial["data"]["terms"]["slots"]
        .as_array_mut()
        .unwrap()
        .pop();
    let mut unsupported = fixture();
    unsupported["data"]["terms"]["slots"][0]["sourceStatus"] = json!("unsupported");
    unsupported["data"]["terms"]["slots"][0]["entries"] = Value::Null;
    let mock = Mock::new(vec![
        Step::json(PATH, partial),
        Step::json(PATH, unsupported),
    ]);
    let repo = repository(&mock);
    assert_eq!(read(&repo).unwrap().unwrap().terms().slots.len(), 1);
    assert!(matches!(
        read(&repo).unwrap().unwrap().terms().slots[0].source_status,
        PreparationStatus::Unsupported
    ));
    assert_eq!(mock.finish().len(), 2);
}
#[test]
fn room_source_has_its_own_bounded_two_mib_transport_without_widening_other_rpcs() {
    let mut large = fixture();
    let text = format!("{}fixture:rules", " ".repeat(1_100_000));
    large["data"]["terms"]["profile"]["rulesRef"] = json!(text);
    let mut oversized = fixture();
    oversized["data"]["terms"]["profile"]["rulesRef"] = json!(format!(
        "{}fixture:rules",
        " ".repeat(MAX_PREPARATION_SOURCE_BYTES as usize)
    ));
    let mock = Mock::new(vec![
        Step::json(PATH, large.clone()),
        Step::json(PATH, large),
        Step::json(PATH, oversized),
    ]);
    let repo = repository(&mock);
    let source = read(&repo).unwrap().unwrap();
    assert_eq!(source.terms().profile.rules_ref.as_str(), text);
    let ordinary = BackendClient::new(
        BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
        "ordinary-boundary-test",
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(
        ordinary
            .rpc::<Value>("read_native_room_source", &json!({}))
            .is_err()
    );
    assert!(read(&repo).is_err());
    assert_eq!(mock.finish().len(), 3);
}
#[test]
fn selected_printing_metadata_is_preserved_without_substituting_another_printing() {
    let mut value = fixture();
    value["data"]["terms"]["slots"][0]["entries"][0]["printing"] = json!({"id":id(70),"externalPrintingRef":"fixture:exact-printing","finish":"foil","setId":id(71)});
    let mock = Mock::new(vec![Step::json(PATH, value)]);
    let repo = repository(&mock);
    let source = read(&repo).unwrap().unwrap();
    let printing = source.terms().slots[0].entries.as_ref().unwrap()[0]
        .printing
        .as_ref()
        .unwrap();
    assert_eq!(printing.id, id(70));
    assert_eq!(
        printing.external_printing_ref.as_str(),
        "fixture:exact-printing"
    );
    assert_eq!(printing.finish, "foil");
    assert_eq!(mock.finish().len(), 1);
}
#[test]
fn corruption_matrix_rejects_foreign_scope_actor_slot_cardinality_and_noncanonical_values() {
    let good = fixture();
    let mut cases = Vec::new();
    for (path, value) in [
        ("/data/sourceHash", json!("A".repeat(64))),
        ("/data/terms/version", json!("native-room-source-v2")),
        ("/data/terms/room/id", json!(id(99))),
        ("/data/terms/room/revision", json!("1")),
        ("/data/terms/release/id", json!(id(99))),
        ("/data/terms/legalityRevisionId", json!(id(99))),
        ("/data/terms/profile/id", json!(id(99))),
        ("/data/terms/profile/minimumSeats", json!("1")),
        ("/data/terms/profile/seatCapacity", json!("7")),
        (
            "/data/terms/room/countdownAt",
            json!("2026-11-06T00:00:00.000000Z"),
        ),
        ("/data/terms/room/state", json!("countdown")),
        ("/data/terms/room/competitive", json!(true)),
        ("/data/terms/slots/0/accountId", Value::Null),
        ("/data/terms/slots/0/sponsorId", json!(id(2))),
        ("/data/terms/slots/1/accountId", json!(id(99))),
        ("/data/terms/slots/1/sponsorId", Value::Null),
        ("/data/terms/slots/0/selectedRevisionId", Value::Null),
        ("/data/terms/slots/0/entries", Value::Null),
        ("/data/terms/slots/0/entries/0/quantity", json!("0")),
        ("/data/terms/slots/0/entries/0/quantity", json!("1001")),
        ("/data/terms/slots/0/entries/0/quantity", json!(1000)),
        ("/data/terms/slots/0/entries/0/quantity", json!("01000")),
        ("/data/terms/slots/1/slotId", json!(id(8))),
        ("/data/terms/slots/1/seatIndex", json!("0")),
        ("/data/terms/slots/1/seatIndex", json!("2")),
    ] {
        let mut changed = good.clone();
        *changed.pointer_mut(path).unwrap() = value;
        cases.push(changed);
    }
    let mut sum = good.clone();
    let mut extra = sum["data"]["terms"]["slots"][0]["entries"][0].clone();
    extra["entryId"] = json!(id(90));
    extra["quantity"] = json!("1");
    sum["data"]["terms"]["slots"][0]["entries"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    cases.push(sum);
    for path in [
        "/data/terms/slots/0/entries/0",
        "/data/terms/slots/1",
        "/data/terms/profile",
    ] {
        let mut changed = good.clone();
        changed
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(if path.ends_with("/0") {
                "printing"
            } else if path.ends_with("/1") {
                "entries"
            } else {
                "planarPoolRef"
            });
        cases.push(changed);
    }
    let count = cases.len();
    let mock = Mock::new(
        cases
            .into_iter()
            .map(|value| Step::json(PATH, value))
            .collect(),
    );
    let repo = repository(&mock);
    for _ in 0..count {
        assert!(read(&repo).is_err());
    }
    assert_eq!(mock.finish().len(), count);
}
#[test]
fn nested_records_reject_positional_encodings_at_every_boundary() {
    let good = fixture();
    let mut cases = Vec::new();
    for (path, fields) in [
        ("/data", vec!["sourceHash", "terms"]),
        (
            "/data/terms",
            vec![
                "version",
                "room",
                "profile",
                "release",
                "legalityRevisionId",
                "slots",
            ],
        ),
        (
            "/data/terms/room",
            vec![
                "id",
                "revision",
                "hostId",
                "state",
                "configurationId",
                "profileId",
                "formatId",
                "mode",
                "competitive",
                "structure",
                "countdownAt",
            ],
        ),
        (
            "/data/terms/profile",
            vec![
                "id",
                "key",
                "revisionRef",
                "rulesRef",
                "planarPoolRef",
                "minimumSeats",
                "seatCapacity",
                "qualificationRef",
            ],
        ),
        (
            "/data/terms/release",
            vec![
                "id",
                "releaseRef",
                "kernelRef",
                "rulesRef",
                "programManifestRef",
                "evidenceRef",
            ],
        ),
        (
            "/data/terms/slots/0",
            vec![
                "slotId",
                "seatIndex",
                "accountId",
                "sponsorId",
                "controllerKind",
                "teamRef",
                "ready",
                "sourceStatus",
                "selectedRevisionId",
                "entries",
            ],
        ),
        (
            "/data/terms/slots/0/entries/0",
            vec![
                "entryId",
                "definitionId",
                "oracleRef",
                "definitionRevisionId",
                "definitionRevisionRef",
                "rulesTextRef",
                "programRef",
                "unlimitedBasicLand",
                "printing",
                "section",
                "quantity",
            ],
        ),
    ] {
        let mut changed = good.clone();
        let array = positional(changed.pointer(path).unwrap(), &fields);
        *changed.pointer_mut(path).unwrap() = array;
        cases.push(changed);
    }
    let mut printing = good.clone();
    printing["data"]["terms"]["slots"][0]["entries"][0]["printing"] =
        json!([id(70), "fixture:printing", "nonfoil", id(71)]);
    cases.push(printing);
    let count = cases.len();
    let mock = Mock::new(
        cases
            .into_iter()
            .map(|value| Step::json(PATH, value))
            .collect(),
    );
    let repo = repository(&mock);
    for _ in 0..count {
        assert!(read(&repo).is_err());
    }
    assert_eq!(mock.finish().len(), count);
}
#[test]
fn unavailable_is_distinct_from_unknown_fail_transport_and_duplicate_source_fields() {
    let raw = serde_json::to_string(&fixture()).unwrap();
    let mut steps = vec![Step::json(
        PATH,
        json!({"status":"fail","data":{"source":"unavailable"}}),
    )];
    for key in ["sourceHash", "slotId", "quantity"] {
        let value = if key == "sourceHash" {
            json!("a".repeat(64))
        } else if key == "slotId" {
            json!(id(8))
        } else {
            json!("1000")
        };
        let mut step = Step::json(PATH, Value::Null);
        let needle = format!("\"{key}\":");
        step.body = raw
            .replacen(&needle, &format!("{needle}{value},{needle}"), 1)
            .into_bytes();
        steps.push(step);
    }
    steps.push(Step::json(
        PATH,
        json!({"status":"fail","data":{"source":"unavailable","extra":"invalid"}}),
    ));
    let mut fault = Step::json(PATH, json!({"private":"not-a-source"}));
    fault.status = 500;
    steps.push(fault);
    let count = steps.len();
    let mock = Mock::new(steps);
    let repo = repository(&mock);
    assert!(read(&repo).unwrap().is_none());
    for _ in 1..count {
        assert!(read(&repo).is_err());
    }
    assert_eq!(mock.finish().len(), count);
}

#[test]
#[ignore = "requires explicit retained managed native-boundary evidence"]
fn recorded_managed_room_responses_match_the_rust_preparation_contract() {
    let path = std::env::var("SPELLWAKE_NATIVE_PREPARATION_MANAGED_EVIDENCE")
        .expect("explicit managed receipt path");
    let value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(value["passed"], true);
    assert_eq!(value["cleanupVerified"], true);
    let mut checked = 0;
    for sample in value["nativeBoundaryRpc"]["wireSamples"]
        .as_array()
        .unwrap()
    {
        if sample["operation"] != "read_native_room_source" {
            continue;
        }
        let query = &sample["request"];
        let response: ObjectResponse<RoomRecord> =
            crate::decode_wire_object(&serde_json::to_vec(&sample["response"]).unwrap()).unwrap();
        let ApiResponse::Success { data } = response.0 else {
            panic!("expected positive source");
        };
        let amount: Amount = query["p_basis"].as_str().unwrap().parse().unwrap();
        let uuid = |key: &str| query[key].as_str().unwrap().parse::<Uuid>().unwrap();
        assert!(data.valid(
            uuid("p_room"),
            &amount,
            uuid("p_release"),
            uuid("p_legality")
        ));
        checked += 1;
    }
    assert!(checked >= 2);
}
