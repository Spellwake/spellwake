use super::*;
use crate::{
    controller_possession::ControllerCredential,
    test_http::{Mock, Step},
};
use std::sync::Arc;
const READ: &str = "/rest/v1/rpc/read_controller_ai_ante_original";
const CONTEXT: &str = "/rest/v1/rpc/read_controller_ai_ante_context";
const SUBMIT: &str = "/rest/v1/rpc/submit_controller_ai_ante";
fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
fn source() -> AiAnteStake {
    AiAnteStake {
        request_id: id(1),
        match_id: id(2),
        seat_id: id(3),
        expected_terms_revision: 0.into(),
        coins: 100.into(),
    }
}
fn install(
    store: &ControllerSecretStore,
    grant: u128,
) -> (ControllerCredential, PossessedController) {
    let token = store
        .install(
            id(grant),
            id(5),
            2.into(),
            format!("private:ante:{grant}").parse().unwrap(),
        )
        .unwrap();
    let proof = store.authenticate(id(grant), &token).unwrap().unwrap();
    (token, proof)
}
fn actor(grant: u128) -> Value {
    json!({"seatId":id(3),"kind":"ai","accountId":null,"grantId":id(grant),"bindingId":id(5),"bindingEpoch":"2"})
}
fn ack() -> Value {
    json!({"resourceId":id(6),"termsRevision":"1","termsHash":"a".repeat(64)})
}
fn reply(state: &str, grant: u128) -> Value {
    let response = match state {
        "accepted" => json!({"status":"success","data":ack()}),
        "rejected" => json!({"status":"fail","data":{"terms":"stale or no longer offered"}}),
        _ => Value::Null,
    };
    json!({"status":"success","data":{"actor":actor(grant),"original":source(),"disposition":{"state":state,"response":response}}})
}
fn context() -> Value {
    json!({"status":"success","data":{"actor":actor(4),"context":{"matchId":id(2),"seatId":id(3),"sponsorId":id(7),"matchStartedAt":"2026-11-12T23:59:59.123456Z","commitmentId":null,"exposureId":null}}})
}
fn repo(mock: &Mock) -> ControllerAnteRepository {
    ControllerAnteRepository::new(
        BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "ante-test",
            Duration::from_secs(2),
        )
        .unwrap(),
    )
}
#[test]
fn original_first_pending_and_terminal_recovery_never_reads_context_or_resubmits() {
    for state in ["pending", "accepted", "rejected"] {
        let store = ControllerSecretStore::default();
        let (token, proof) = install(&store, 4);
        let mut step = Step::json(READ, reply(state, 4));
        step.expect=Some(serde_json::to_vec(&json!({"p_grant":id(4),"p_epoch":"2","p_secret_ref":"private:ante:4","p_source":source()})).unwrap());
        let mock = Mock::new(vec![step]);
        let result = repo(&mock).execute(&store, &proof, &source()).unwrap();
        assert!(
            matches!(result,ApiResponse::Success{data} if !matches!(data,CommandDisposition::Unknown))
        );
        let captured = mock.finish();
        assert_eq!(captured.len(), 1);
        assert!(!String::from_utf8_lossy(&captured[0].2).contains(&token.expose_for_delivery()));
    }
}
#[test]
fn fresh_context_supplies_original_day_and_submit_source_only_once() {
    let store = ControllerSecretStore::default();
    let (_, proof) = install(&store, 4);
    let mock = Mock::new(vec![
        Step::json(READ, reply("unknown", 4)),
        Step::json(CONTEXT, context()),
        Step::json(SUBMIT, reply("accepted", 4)),
    ]);
    assert!(matches!(
        repo(&mock).execute(&store, &proof, &source()).unwrap(),
        ApiResponse::Success {
            data: CommandDisposition::Accepted { .. }
        }
    ));
    let captured = mock.finish();
    assert_eq!(captured.len(), 3);
    let payload: Value = serde_json::from_slice(&captured[2].2).unwrap();
    assert_eq!(payload["p_source"], json!(source()));
    assert_eq!(payload.as_object().unwrap().len(), 4);
    assert!(!String::from_utf8_lossy(&captured[2].2).contains("sponsor"));
    let mock = Mock::new(vec![Step::json(CONTEXT, context())]);
    let ApiResponse::Success { data } = repo(&mock).context(&store, &proof, id(2)).unwrap() else {
        panic!()
    };
    assert_eq!(data.match_started_at.date_naive().to_string(), "2026-11-12");
    mock.finish();
}
#[test]
fn uncertain_ack_recovers_exact_original_without_second_mutation() {
    for mode in 0..3 {
        let store = ControllerSecretStore::default();
        let (_, proof) = install(&store, 4);
        let mut lost = Step::json(SUBMIT, reply("accepted", 4));
        match mode {
            0 => lost.drop_ack = true,
            1 => lost.body = b"{\"status\":".to_vec(),
            _ => lost.status = 503,
        }
        let mock = Mock::new(vec![
            Step::json(READ, reply("unknown", 4)),
            Step::json(CONTEXT, context()),
            lost,
            Step::json(READ, reply("accepted", 4)),
        ]);
        assert!(matches!(
            repo(&mock).execute(&store, &proof, &source()).unwrap(),
            ApiResponse::Success {
                data: CommandDisposition::Accepted { .. }
            }
        ));
        assert_eq!(
            mock.finish()
                .iter()
                .filter(|(path, _, _)| path == SUBMIT)
                .count(),
            1
        );
    }
}
#[test]
fn uncertain_ack_with_unknown_recovery_preserves_safe_error_and_cause() {
    let store = ControllerSecretStore::default();
    let (_, proof) = install(&store, 4);
    let mut lost = Step::json(SUBMIT, Value::Null);
    lost.status = 503;
    let mock = Mock::new(vec![
        Step::json(READ, reply("unknown", 4)),
        Step::json(CONTEXT, context()),
        lost,
        Step::json(READ, reply("unknown", 4)),
    ]);
    let error = repo(&mock).execute(&store, &proof, &source()).unwrap_err();
    assert!(error.source().is_some());
    assert_eq!(
        error.to_string(),
        "controller ante source is unavailable or inconsistent"
    );
    mock.finish();
}
#[test]
fn foreign_store_revoked_proof_and_invalid_original_cannot_dispatch() {
    let store = ControllerSecretStore::default();
    let (_, proof) = install(&store, 4);
    let mock = Mock::new(vec![]);
    let repository = repo(&mock);
    assert!(matches!(
        repository
            .execute(&ControllerSecretStore::default(), &proof, &source())
            .unwrap(),
        ApiResponse::Fail { .. }
    ));
    let mut bad = source();
    bad.coins = 301.into();
    assert!(repository.execute(&store, &proof, &bad).is_err());
    store.revoke(id(4)).unwrap();
    assert!(matches!(
        repository.context(&store, &proof, id(2)).unwrap(),
        ApiResponse::Fail { .. }
    ));
    mock.finish();
}
#[test]
fn local_revocation_encloses_success_denial_decode_and_transport_outcomes() {
    for path in [READ, CONTEXT, SUBMIT] {
        for mode in 0..4 {
            let store = Arc::new(ControllerSecretStore::default());
            let (_, proof) = install(&store, 4);
            let revoke = store.clone();
            let mut step = Step::json(path, Value::Null);
            if mode == 3 {
                step.status = 503;
            }
            step.reply = Some(Box::new(move |_| {
                revoke.revoke(id(4)).unwrap();
                match mode {
                    0 => serde_json::to_vec(&if path == CONTEXT {
                        context()
                    } else {
                        reply("accepted", 4)
                    })
                    .unwrap(),
                    1 => {
                        serde_json::to_vec(&json!({"status":"fail","data":{"actor":"unavailable"}}))
                            .unwrap()
                    }
                    _ => b"not-json".to_vec(),
                }
            }));
            let mock = Mock::new(vec![step]);
            let repository = repo(&mock);
            let unavailable = if path == CONTEXT {
                matches!(repository.context(&store,&proof,id(2)).unwrap(),ApiResponse::Fail{data} if data.get("actor").is_some_and(|v|v=="unavailable"))
            } else {
                matches!(repository.original_rpc(&store,&proof,&source(),path==SUBMIT).unwrap(),ApiResponse::Fail{data} if data.get("actor").is_some_and(|v|v=="unavailable"))
            };
            assert!(unavailable);
            mock.finish();
        }
    }
}
#[test]
fn same_seat_replacement_recovers_historical_original_with_new_proof() {
    let store = ControllerSecretStore::default();
    let (_, old) = install(&store, 4);
    store.revoke(id(4)).unwrap();
    let (_, new) = install(&store, 8);
    let mock = Mock::new(vec![Step::json(READ, reply("accepted", 8))]);
    let repository = repo(&mock);
    assert!(matches!(
        repository.original(&store, &old, &source()).unwrap(),
        ApiResponse::Fail { .. }
    ));
    assert!(matches!(
        repository.execute(&store, &new, &source()).unwrap(),
        ApiResponse::Success {
            data: CommandDisposition::Accepted { .. }
        }
    ));
    mock.finish();
}
#[test]
fn strict_original_actor_and_disposition_correlations_reject_drift() {
    let mut cases = Vec::new();
    for (pointer, value) in [
        ("/data/actor/grantId", json!(id(8))),
        ("/data/actor/bindingId", json!(id(8))),
        ("/data/actor/bindingEpoch", json!("3")),
        ("/data/actor/kind", json!("human")),
        ("/data/actor/accountId", json!(id(7))),
        ("/data/actor/seatId", json!(id(8))),
        ("/data/original/coins", json!("101")),
        ("/data/original/requestId", json!(id(8))),
        ("/data/original/matchId", json!(id(8))),
        ("/data/original/seatId", json!(id(8))),
        ("/data/disposition/response/data/termsRevision", json!("2")),
        (
            "/data/disposition/response/data/termsHash",
            json!("A".repeat(64)),
        ),
        (
            "/data/disposition/response/data/resourceId",
            json!(Uuid::nil()),
        ),
        ("/data/disposition/state", json!("pending")),
    ] {
        let mut v = reply("accepted", 4);
        *v.pointer_mut(pointer).unwrap() = value;
        cases.push(v);
    }
    let mut extra = reply("accepted", 4);
    extra["data"]["original"]["sponsorId"] = json!(id(7));
    cases.push(extra);
    cases.extend([
        json!({"status":"fail","data":{"session":"unauthorized"}}),
        json!({"status":"error","message":"unsafe"}),
        json!({"status":"fail","data":{"terms":"stale or no longer offered"}}),
    ]);
    let store = ControllerSecretStore::default();
    let (_, proof) = install(&store, 4);
    let mock = Mock::new(cases.iter().map(|v| Step::json(READ, v.clone())).collect());
    let repository = repo(&mock);
    for _ in cases {
        assert!(repository.original(&store, &proof, &source()).is_err());
    }
    mock.finish();
}
#[test]
fn context_requires_exact_scope_nullable_identity_and_canonical_timestamp() {
    let mut cases = Vec::new();
    for (pointer, value) in [
        ("/data/context/matchId", json!(id(8))),
        ("/data/context/seatId", json!(id(8))),
        ("/data/context/sponsorId", json!(Uuid::nil())),
        ("/data/context/exposureId", json!(id(9))),
        (
            "/data/context/matchStartedAt",
            json!("2026-11-12T23:59:59Z"),
        ),
        (
            "/data/context/matchStartedAt",
            json!("2026-11-12T23:59:59.123456+00:00"),
        ),
    ] {
        let mut v = context();
        *v.pointer_mut(pointer).unwrap() = value;
        cases.push(v);
    }
    for key in ["commitmentId", "exposureId"] {
        let mut v = context();
        v["data"]["context"].as_object_mut().unwrap().remove(key);
        cases.push(v);
    }
    let mut v = context();
    v["data"]["context"] = json!([]);
    cases.push(v);
    let store = ControllerSecretStore::default();
    let (_, proof) = install(&store, 4);
    let mock = Mock::new(
        cases
            .iter()
            .map(|v| Step::json(CONTEXT, v.clone()))
            .collect(),
    );
    let repository = repo(&mock);
    for _ in cases {
        assert!(repository.context(&store, &proof, id(2)).is_err());
    }
    mock.finish();
}
#[test]
fn raw_duplicate_positional_and_budget_boundaries_are_refused() {
    let good = reply("accepted", 4);
    let raw = serde_json::to_string(&good).unwrap();
    let mut duplicate = Step::json(READ, good.clone());
    duplicate.body = raw
        .replace("\"coins\":", "\"coins\":\"100\",\"coins\":")
        .into_bytes();
    let mut positional = good.clone();
    positional["data"]["actor"] = json!([id(3), "ai", null, id(4), id(5), "2"]);
    let mut oversized = Step::json(READ, good);
    oversized.body.extend(vec![b' '; LIMIT as usize]);
    let store = ControllerSecretStore::default();
    let (_, proof) = install(&store, 4);
    let mock = Mock::new(vec![duplicate, Step::json(READ, positional), oversized]);
    let repository = repo(&mock);
    for _ in 0..3 {
        assert!(repository.original(&store, &proof, &source()).is_err());
    }
    mock.finish();
}
#[test]
fn known_commitment_mismatch_cannot_escape_through_uncertain_recovery() {
    let store = ControllerSecretStore::default();
    let (_, proof) = install(&store, 4);
    let mut selected = context();
    selected["data"]["context"]["commitmentId"] = json!(id(9));
    let mock = Mock::new(vec![
        Step::json(READ, reply("unknown", 4)),
        Step::json(CONTEXT, selected),
        Step::json(SUBMIT, reply("accepted", 4)),
        Step::json(READ, reply("accepted", 4)),
    ]);
    assert!(repo(&mock).execute(&store, &proof, &source()).is_err());
    mock.finish();
}
