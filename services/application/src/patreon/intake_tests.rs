//! Controlled private HTTP/Storage envelopes; not actual managed Storage or provider delivery.
use super::*;
use crate::{
    patreon::test_fixture,
    test_http::{Mock, Step},
};
use serde_json::{Value, json};
fn body() -> Vec<u8> {
    test_fixture::source()["webhookBody"]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec()
}
fn signature() -> String {
    test_fixture::source()["signature"].as_str().unwrap().into()
}
fn key(id: Uuid) -> String {
    path(id, test_fixture::source()["bodySha256"].as_str().unwrap()).unwrap()
}
fn ticket(id: Uuid, state: &str, expired: bool) -> Value {
    json!({"id":id,"evidenceState":state,"payloadRef":key(id),"expiresAt":if expired{"2000-01-01T00:00:00Z"}else{"2099-01-01T00:00:00Z"},"processed":false})
}
fn service(base: &str) -> IntakeService {
    IntakeService::new(
        BackendConfig::new(base, "sb_secret_test_only").unwrap(),
        Arc::new(
            IntakeConfig::new(
                "123".into(),
                test_fixture::source()["secret"].as_str().unwrap().into(),
                "patreon_private".into(),
            )
            .unwrap(),
        ),
    )
    .unwrap()
}

#[test]
fn private_objects_reject_capture_and_expiry_arrays_before_storage_actions() {
    let id = Uuid::new_v4();
    let value = crate::test_http::positional(
        &ticket(id, "pending", false),
        &[
            "id",
            "evidenceState",
            "payloadRef",
            "expiresAt",
            "processed",
        ],
    );
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/capture_patreon_notification",
        value,
    )]);
    assert!(
        service(&mock.base)
            .ingest(&body(), &signature(), id)
            .is_err()
    );
    assert_eq!(mock.finish().len(), 1);
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/begin_patreon_evidence_expiry",
        json!([key(id), test_fixture::source()["bodySha256"]]),
    )]);
    assert!(service(&mock.base).evidence.expire(id).is_err());
    assert_eq!(mock.finish().len(), 1);
}
fn bucket() -> Step {
    Step::get(
        "/storage/v1/bucket/patreon_private".into(),
        200,
        serde_json::to_vec(&json!({"id":"patreon_private","public":false})).unwrap(),
    )
}
fn object(id: Uuid, status: u16, bytes: Vec<u8>) -> Step {
    Step::get(
        format!(
            "/storage/v1/object/authenticated/patreon_private/{}",
            key(id)
        ),
        status,
        bytes,
    )
}
fn capture_step(id: Uuid, state: &str, expired: bool) -> Step {
    Step::json(
        "/rest/v1/rpc/capture_patreon_notification",
        ticket(id, state, expired),
    )
}
fn begin(id: Uuid) -> Step {
    Step::json(
        "/rest/v1/rpc/begin_patreon_evidence_expiry",
        json!({"payloadRef":key(id),"bodyHash":test_fixture::source()["bodySha256"]}),
    )
}
fn deletion() -> Step {
    let mut step = Step::json("/storage/v1/object/patreon_private", json!([]));
    step.method = "DELETE";
    step
}
#[test]
fn authenticated_intake_retains_exact_bytes_before_confirmation_and_normalizes_only_routing() {
    let id = Uuid::new_v4();
    let bytes = body();
    let mut upload = Step::json(
        &format!("/storage/v1/object/patreon_private/{}", key(id)),
        Value::Null,
    );
    upload.expect = Some(bytes.clone());
    let backend = Mock::new(vec![
        capture_step(id, "pending", false),
        bucket(),
        object(id, 404, vec![]),
        upload,
        object(id, 200, bytes.clone()),
        Step::json("/rest/v1/rpc/confirm_patreon_evidence", json!(true)),
    ]);
    service(&backend.base)
        .ingest(&bytes, &signature(), id)
        .unwrap();
    let calls = backend.finish();
    let input: Value = serde_json::from_slice(&calls[0].2).unwrap();
    assert_eq!(input["p_delivery"], json!(id));
    assert_eq!(
        input["p_capture"],
        json!({"version":1,"kind":"webhook","source":"patreon-v2-member-notification-v1","campaignId":"123","userId":"456","memberId":"member-abc","bodyHash":test_fixture::source()["bodySha256"]})
    );
    assert_eq!(calls[3].1["x-upsert"], "false");
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[5].2).unwrap(),
        json!({"p_event":id,"p_hash":test_fixture::source()["bodySha256"]})
    );
    for i in [0, 5] {
        let text = String::from_utf8(calls[i].2.clone()).unwrap();
        assert!(!text.contains("Synthetic private name"));
        assert!(!text.contains("not-a-real-user"));
        assert!(!text.contains("patreon_webhook_synthetic_secret"));
    }
}
#[test]
fn lost_upload_ack_uses_exact_readback_and_mismatch_never_confirms() {
    for matches in [true, false] {
        let id = Uuid::new_v4();
        let mut upload = Step::json(
            &format!("/storage/v1/object/patreon_private/{}", key(id)),
            Value::Null,
        );
        upload.drop_ack = true;
        let mut steps = vec![
            capture_step(id, "pending", false),
            bucket(),
            object(id, 404, vec![]),
            upload,
            object(
                id,
                200,
                if matches {
                    body()
                } else {
                    b"wrong source bytes".to_vec()
                },
            ),
        ];
        if matches {
            steps.push(Step::json(
                "/rest/v1/rpc/confirm_patreon_evidence",
                json!(true),
            ));
        }
        let backend = Mock::new(steps);
        assert_eq!(
            service(&backend.base)
                .ingest(&body(), &signature(), id)
                .is_ok(),
            matches
        );
        backend.finish();
    }
}
#[test]
fn capture_unknown_ack_requires_same_attempt_identity_and_retained_replay_has_no_upload() {
    let id = Uuid::new_v4();
    let mut unknown = capture_step(id, "pending", false);
    unknown.drop_ack = true;
    let backend = Mock::new(vec![unknown, capture_step(id, "retained", false)]);
    let intake = service(&backend.base);
    assert!(intake.ingest(&body(), &signature(), id).is_err());
    intake.ingest(&body(), &signature(), id).unwrap();
    let calls = backend.finish();
    assert_eq!(calls[0].2, calls[1].2);
}
#[test]
fn source_ticket_scope_state_precision_and_extra_fields_are_checked_before_storage() {
    let id = Uuid::new_v4();
    let mut wrong_id = ticket(id, "pending", false);
    wrong_id["id"] = json!(Uuid::new_v4());
    let mut wrong_path = ticket(id, "pending", false);
    wrong_path["payloadRef"] = json!(key(Uuid::new_v4()));
    let mut bad_time = ticket(id, "pending", false);
    bad_time["expiresAt"] = json!("2099-01-01T00:00:00.000000001Z");
    let mut extra = ticket(id, "pending", false);
    extra["accountId"] = json!(Uuid::new_v4());
    for value in [
        wrong_id,
        wrong_path,
        bad_time,
        extra,
        ticket(id, "none", false),
    ] {
        let backend = Mock::new(vec![Step::json(
            "/rest/v1/rpc/capture_patreon_notification",
            value,
        )]);
        assert!(
            service(&backend.base)
                .ingest(&body(), &signature(), id)
                .is_err()
        );
        backend.finish();
    }
}
#[test]
fn bad_authentication_or_nil_occurrence_has_no_backend_effect() {
    let backend = Mock::new(vec![]);
    let intake = service(&backend.base);
    assert!(
        intake
            .ingest(&body(), &"0".repeat(32), Uuid::new_v4())
            .is_err()
    );
    assert!(intake.ingest(&body(), &signature(), Uuid::nil()).is_err());
    assert!(intake.ingest(b"{}", &signature(), Uuid::new_v4()).is_err());
    assert!(backend.finish().is_empty());
}
#[test]
fn due_or_expired_intake_skips_storage_confirmation_and_deletion() {
    for state in ["pending", "retained", "expired"] {
        let id = Uuid::new_v4();
        let backend = Mock::new(vec![capture_step(id, state, true)]);
        service(&backend.base)
            .ingest(&body(), &signature(), id)
            .unwrap();
        assert_eq!(backend.finish().len(), 1);
    }
}

#[test]
fn expiry_binds_ticket_to_requested_source_and_does_not_finish_a_missing_ticket() {
    let id = Uuid::new_v4();
    for value in [
        Value::Null,
        json!({"payloadRef":key(Uuid::new_v4()),"bodyHash":test_fixture::source()["bodySha256"]}),
        json!({"payloadRef":key(id),"bodyHash":"not-a-hash"}),
    ] {
        let missing = value.is_null();
        let backend = Mock::new(vec![Step::json(
            "/rest/v1/rpc/begin_patreon_evidence_expiry",
            value,
        )]);
        assert_eq!(service(&backend.base).evidence.expire(id).is_ok(), missing);
        backend.finish();
    }
}

#[test]
fn unknown_confirmation_ack_recovers_retained_source_without_uploading_again() {
    let id = Uuid::new_v4();
    let mut unknown = Step::json("/rest/v1/rpc/confirm_patreon_evidence", json!(true));
    unknown.drop_ack = true;
    let backend = Mock::new(vec![
        capture_step(id, "pending", false),
        bucket(),
        object(id, 200, body()),
        unknown,
        capture_step(id, "retained", false),
    ]);
    let intake = service(&backend.base);
    assert!(intake.ingest(&body(), &signature(), id).is_err());
    intake.ingest(&body(), &signature(), id).unwrap();
    let calls = backend.finish();
    assert_eq!(calls[0].2, calls[4].2);
}
#[test]
fn expiry_requires_private_bucket_and_exact_positive_absence_after_lost_delete_ack() {
    for (metadata, absence, expected) in [
        (
            json!({"id":"patreon_private","public":false}),
            json!({"code":"NoSuchKey","statusCode":"404"}),
            true,
        ),
        (
            json!({"id":"patreon_private","public":false}),
            json!({"error":"not_found","statusCode":"404"}),
            false,
        ),
        (
            json!({"id":"patreon_private","public":true}),
            Value::Null,
            false,
        ),
    ] {
        let id = Uuid::new_v4();
        let is_private = metadata["public"] == false;
        let mut steps = vec![
            begin(id),
            Step::get(
                "/storage/v1/bucket/patreon_private".into(),
                200,
                serde_json::to_vec(&metadata).unwrap(),
            ),
        ];
        if is_private {
            let mut delete = deletion();
            delete.drop_ack = true;
            steps.push(delete);
            steps.push(object(id, 400, serde_json::to_vec(&absence).unwrap()));
        }
        if expected {
            steps.push(Step::no_content(
                "/rest/v1/rpc/finish_patreon_evidence_expiry",
            ));
        }
        let backend = Mock::new(steps);
        assert_eq!(service(&backend.base).evidence.expire(id).is_ok(), expected);
        backend.finish();
    }
}

#[test]
fn evidence_work_checks_page_scope_and_keeps_failed_sources_for_the_next_sweep() {
    let id = Uuid::new_v4();
    for ids in [json!([Uuid::nil()]), json!([id, id])] {
        let backend = Mock::new(vec![Step::json("/rest/v1/rpc/read_patreon_work", ids)]);
        assert!(service(&backend.base).evidence.work(None).is_err());
        backend.finish();
    }
    let backend = Mock::new(vec![Step::json(
        "/rest/v1/rpc/read_patreon_work",
        json!([id]),
    )]);
    assert!(service(&backend.base).evidence.work(Some(id)).is_err());
    backend.finish();
    let mut fail = Step::json("/rest/v1/rpc/begin_patreon_evidence_expiry", Value::Null);
    fail.status = 503;
    let backend = Mock::new(vec![
        Step::json("/rest/v1/rpc/read_patreon_work", json!([id])),
        fail,
        Step::json("/rest/v1/rpc/read_patreon_work", json!([])),
        Step::json("/rest/v1/rpc/read_patreon_work", json!([id])),
        Step::json("/rest/v1/rpc/begin_patreon_evidence_expiry", Value::Null),
    ]);
    let evidence = service(&backend.base).evidence;
    assert_eq!(evidence.work(None).unwrap(), Some(id));
    assert_eq!(evidence.work(Some(id)).unwrap(), None);
    assert_eq!(evidence.work(None).unwrap(), Some(id));
    let calls = backend.finish();
    for (index, after) in [(0, Value::Null), (2, json!(id)), (3, Value::Null)] {
        assert_eq!(
            serde_json::from_slice::<Value>(&calls[index].2).unwrap(),
            json!({"p_kind":"evidence","p_after":after,"p_limit":1,"p_bucket":"patreon_private"})
        );
    }
}
#[test]
fn evidence_stop_prevents_new_groups_after_a_returning_begin_request() {
    let id = Uuid::new_v4();
    let flag = Arc::new(AtomicBool::new(false));
    let callback_flag = flag.clone();
    let backend = Mock::new_with_observer(vec![begin(id)], move |_| {
        callback_flag.store(true, Ordering::Release);
    });
    let evidence = EvidenceService::new(
        BackendConfig::new(&backend.base, "sb_secret_test_only").unwrap(),
        "patreon_private".into(),
        flag,
    )
    .unwrap();
    assert!(evidence.expire(id).is_err());
    assert_eq!(backend.finish().len(), 1);
    let backend = Mock::new(vec![]);
    let evidence = EvidenceService::new(
        BackendConfig::new(&backend.base, "sb_secret_test_only").unwrap(),
        "patreon_private".into(),
        Arc::new(AtomicBool::new(true)),
    )
    .unwrap();
    assert!(evidence.work(None).is_err());
    assert!(backend.finish().is_empty());
}
