//! Transport/shape checks. Synthetic references do not certify artifact bytes.
use super::*;
use crate::test_http::{Mock, Step, positional};
use serde_json::Value;
const PATH: &str = "/rest/v1/rpc/read_native_opening_source";
fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}
fn fixture() -> Value {
    let artifact = |role: &str| {
        format!(
            "native-artifact-v1/{}/{}/{role}/1/{}.json",
            id(1),
            id(2),
            "a".repeat(64)
        )
    };
    json!({"status":"success","data":{
        "version":"native-opening-v1","gameId":id(1),"matchId":id(3),"contextId":id(2),
        "owner":"fixture:original-owner","originalBasis":"7","nativeEpoch":"42",
        "runtimeId":"b".repeat(64),"programSourceId":"c".repeat(64),
        "receiptId":id(4),"childReceiptId":id(5),"checkpointId":id(6),
        "opening":{"contextId":id(2),"snapshotRef":artifact("initial"),"stateRef":artifact("state"),
        "factsRef":artifact("facts"),"eventRef":artifact("event"),"contextRevision":"fixture:context",
        "qualificationRef":"fixture:SQL-only"}
    }})
}
fn repository(mock: &Mock) -> NativeSourceRepository {
    NativeSourceRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "opening-source-test",
            Duration::from_secs(2),
        )
        .unwrap(),
    )
}
#[test]
fn original_opening_preserves_native_epoch_and_original_receipt_coordinates() {
    let mut step = Step::json(PATH, fixture());
    step.expect = Some(serde_json::to_vec(&json!({"p_game":id(1)})).unwrap());
    let mock = Mock::new(vec![step]);
    let repo = repository(&mock);
    assert!(repo.opening(Uuid::nil()).is_err());
    let original = repo.opening(id(1)).unwrap().unwrap();
    assert_eq!(original.ids(), (id(3), id(1), id(2)));
    assert_eq!(original.receipt_ids(), (id(4), id(5), id(6)));
    assert_eq!(original.native_epoch().get(), 42);
    assert_eq!(original.runtime_id(), "b".repeat(64));
    assert_eq!(original.program_source_id(), "c".repeat(64));
    assert_eq!(original.original_basis().1, &Amount::from(7));
    assert_eq!(original.original_basis().0, "fixture:original-owner");
    assert!(original.artifacts().initial().role() == NativeArtifactRole::Initial);
    assert!(original.artifacts().state().role() == NativeArtifactRole::State);
    assert!(original.artifacts().facts().role() == NativeArtifactRole::Facts);
    assert!(original.artifacts().event().role() == NativeArtifactRole::Event);
    assert_eq!(original.artifacts().context_revision(), "fixture:context");
    assert_eq!(original.artifacts().qualification_ref(), "fixture:SQL-only");
    assert_eq!(mock.finish().len(), 1);
}
#[test]
fn original_opening_rejects_foreign_scope_roles_and_noncanonical_runtime_coordinates() {
    let good = fixture();
    let mut cases = Vec::new();
    for (pointer, value) in [
        ("/data/version", json!("native-opening-v2")),
        ("/data/gameId", json!(id(99))),
        ("/data/contextId", json!(id(99))),
        ("/data/opening/contextId", json!(id(99))),
        ("/data/owner", json!("x".repeat(257))),
        (
            "/data/runtimeId",
            json!(format!("sha256:{}", "b".repeat(64))),
        ),
        ("/data/programSourceId", json!("C".repeat(64))),
        ("/data/nativeEpoch", json!("18446744073709551616")),
        ("/data/nativeEpoch", json!("042")),
        ("/data/originalBasis", json!(7)),
        ("/data/receiptId", json!(Uuid::nil())),
        ("/data/childReceiptId", json!(Uuid::nil())),
        ("/data/checkpointId", json!(Uuid::nil())),
        (
            "/data/opening/stateRef",
            good["data"]["opening"]["snapshotRef"].clone(),
        ),
        (
            "/data/opening/factsRef",
            json!(format!(
                "native-artifact-v1/{}/{}/facts/1/{}.json",
                id(99),
                id(2),
                "a".repeat(64)
            )),
        ),
        (
            "/data/opening/eventRef",
            json!(format!(
                "native-artifact-v1/{}/{}/event/1/{}.json",
                id(1),
                id(99),
                "a".repeat(64)
            )),
        ),
        ("/data/opening/qualificationRef", json!("x".repeat(2049))),
    ] {
        let mut value_case = good.clone();
        *value_case.pointer_mut(pointer).unwrap() = value;
        cases.push(value_case);
    }
    for key in good["data"].as_object().unwrap().keys() {
        let mut case = good.clone();
        case["data"].as_object_mut().unwrap().remove(key);
        cases.push(case);
    }
    let mut extra = good.clone();
    extra["data"]["fenceEpoch"] = json!("42");
    cases.push(extra);
    let mut array = good.clone();
    array["data"]["opening"] = positional(
        &good["data"]["opening"],
        &[
            "contextId",
            "snapshotRef",
            "contextRevision",
            "stateRef",
            "factsRef",
            "eventRef",
            "qualificationRef",
        ],
    );
    cases.push(array);
    let mut array = good.clone();
    array["data"] = positional(
        &good["data"],
        &[
            "version",
            "matchId",
            "gameId",
            "owner",
            "originalBasis",
            "opening",
            "nativeEpoch",
            "runtimeId",
            "programSourceId",
            "childReceiptId",
            "receiptId",
            "contextId",
            "checkpointId",
        ],
    );
    cases.push(array);
    let count = cases.len();
    let mock = Mock::new(
        cases
            .into_iter()
            .map(|case| Step::json(PATH, case))
            .collect(),
    );
    let repo = repository(&mock);
    for _ in 0..count {
        assert!(repo.opening(id(1)).is_err());
    }
    assert_eq!(mock.finish().len(), count);
}
#[test]
fn original_opening_only_declared_absence_is_none_and_duplicate_fields_reject() {
    let raw = serde_json::to_string(&fixture()).unwrap();
    let duplicate = raw.replacen(
        "\"nativeEpoch\":\"42\"",
        "\"nativeEpoch\":\"42\",\"nativeEpoch\":\"42\"",
        1,
    );
    assert_ne!(raw, duplicate);
    let mut repeated = Step::json(PATH, fixture());
    repeated.body = duplicate.into_bytes();
    let mock = Mock::new(vec![
        Step::json(PATH, json!({"status":"fail","data":{"game":"unavailable"}})),
        Step::json(PATH, json!({"status":"fail","data":{"game":"stale"}})),
        Step::json(
            PATH,
            json!({"status":"fail","data":{"game":"unavailable","extra":"unavailable"}}),
        ),
        Step::json(
            PATH,
            json!({"status":"error","message":"private upstream failure"}),
        ),
        repeated,
    ]);
    let repo = repository(&mock);
    assert!(repo.opening(id(1)).unwrap().is_none());
    for _ in 0..4 {
        assert!(repo.opening(id(1)).is_err());
    }
    assert_eq!(mock.finish().len(), 5);
}

#[test]
#[ignore = "requires retained managed native SQL wire evidence"]
fn recorded_managed_opening_matches_the_rust_source_contract() {
    let path =
        std::env::var("SPELLWAKE_NATIVE_PREPARATION_MANAGED_EVIDENCE").expect("retained receipt");
    let evidence: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(evidence["passed"], true);
    assert_eq!(evidence["cleanupVerified"], true);
    let samples = evidence["nativeBoundaryRpc"]["wireSamples"]
        .as_array()
        .unwrap();
    let samples: Vec<_> = samples
        .iter()
        .filter(|sample| sample["operation"] == "read_native_opening_source")
        .collect();
    assert!(!samples.is_empty());
    let mock = Mock::new(
        samples
            .iter()
            .map(|sample| {
                let mut step = Step::json(PATH, sample["response"].clone());
                step.expect = Some(serde_json::to_vec(&sample["request"]).unwrap());
                step
            })
            .collect(),
    );
    let repo = repository(&mock);
    for sample in &samples {
        let game = sample["request"]["p_game"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let source = repo.opening(game).unwrap().unwrap();
        assert_eq!(source.ids().1, game);
        assert_eq!(
            source.runtime_id(),
            sample["response"]["data"]["runtimeId"].as_str().unwrap()
        );
    }
    assert_eq!(mock.finish().len(), samples.len());
}
