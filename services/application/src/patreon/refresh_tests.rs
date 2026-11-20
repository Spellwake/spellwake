//! Actual bounded HTTP with controlled SQL/provider envelopes, not live source or managed SQL.
use super::*;
use crate::{
    patreon::test_fixture as f,
    test_http::{Credentials, Mock, Step},
};
use serde_json::{Value, json};
fn association() -> Uuid {
    Uuid::parse_str("12345678-1234-4234-9234-123456789abc").unwrap()
}
fn ticket() -> Value {
    json!({"version":1,"accountId":"22345678-1234-4234-9234-123456789abc","associationId":association(),"userId":"456","campaignId":"123","memberId":"member-abc","expectedSourceId":"32345678-1234-4234-9234-123456789abc","anchorEventId":"42345678-1234-4234-9234-123456789abc","generation":"1"})
}
fn result(outcome: &str) -> Value {
    json!({"associationId":association(),"sourceEventId":if outcome=="applied"{json!("52345678-1234-4234-9234-123456789abc")}else{Value::Null},"outcome":outcome})
}
fn source_step(value: Value) -> Step {
    let mut step = Step::get(f::member_path(), 200, serde_json::to_vec(&value).unwrap());
    step.credentials = Credentials::Bearer("patreon_test_creator_synthetic".into());
    step
}
fn provider() -> Mock {
    Mock::new(vec![source_step(f::source()["member"].clone())])
}
fn prepare(value: Value) -> Step {
    Step::json("/rest/v1/rpc/read_patreon_refresh", value)
}
fn commit(value: Value) -> Step {
    Step::json("/rest/v1/rpc/commit_patreon_refresh", value)
}
fn service(backend: &str, provider: &str) -> PatreonRefreshService {
    PatreonRefreshService::new(
        BackendConfig::new(backend, "sb_secret_test_only").unwrap(),
        PatreonConfig::fixture(provider),
    )
    .unwrap()
}
#[test]
fn current_refresh_selects_exact_bound_source_without_payment_or_profile_fields() {
    let provider = provider();
    let backend = Mock::new(vec![prepare(ticket()), commit(result("applied"))]);
    let service = service(&backend.base, &provider.base);
    let attempt = service.observe(association()).unwrap().unwrap();
    let outcome = service.commit(&attempt).unwrap();
    assert_eq!(serde_json::to_value(outcome).unwrap(), result("applied"));
    let calls = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[0].2).unwrap(),
        json!({"p_association":association()})
    );
    let input: Value = serde_json::from_slice(&calls[1].2).unwrap();
    assert_eq!(input["p_ticket"], ticket());
    assert!(Uuid::parse_str(input["p_request"].as_str().unwrap()).is_ok());
    assert_eq!(
        input["p_proof"],
        json!({"version":1,"source":"patreon-v2-current-member-v1","campaignId":"123","userId":"456","memberId":"member-abc","memberHash":crate::provider_json::fingerprint(&serde_json::to_vec(&f::source()["member"]).unwrap()),"entitledTierIds":["789"]})
    );
    let text = String::from_utf8(calls[1].2.clone()).unwrap();
    for forbidden in [
        "lastCharge",
        "last_charge",
        "patron_status",
        "Paid",
        "identityHash",
        "patreon_test_creator",
        "full_name",
    ] {
        assert!(!text.contains(forbidden));
    }
    assert_eq!(provider.finish().len(), 1);
}
#[test]
fn lost_commit_ack_replays_identical_attempt_without_another_provider_read() {
    let provider = provider();
    let mut unknown = commit(result("applied"));
    unknown.drop_ack = true;
    let backend = Mock::new(vec![prepare(ticket()), unknown, commit(result("applied"))]);
    let service = service(&backend.base, &provider.base);
    let attempt = service.observe(association()).unwrap().unwrap();
    assert!(service.commit(&attempt).is_err());
    assert!(matches!(
        service.commit(&attempt).unwrap(),
        PatreonRefreshOutcome::Applied { .. }
    ));
    let calls = backend.finish();
    assert_eq!(calls[1].2, calls[2].2);
    assert_eq!(provider.finish().len(), 1);
}
#[test]
fn superseded_requires_new_ticket_and_fresh_source_read_for_a_new_attempt() {
    let mut updated = ticket();
    updated["expectedSourceId"] = json!("62345678-1234-4234-9234-123456789abc");
    updated["generation"] = json!("2");
    let mut empty = f::source()["member"].clone();
    empty["data"]["relationships"]["currently_entitled_tiers"]["data"] = json!([]);
    let provider = Mock::new(vec![
        source_step(f::source()["member"].clone()),
        source_step(empty),
    ]);
    let backend = Mock::new(vec![
        prepare(ticket()),
        commit(result("superseded")),
        prepare(updated.clone()),
        commit(result("applied")),
    ]);
    let service = service(&backend.base, &provider.base);
    let first = service.observe(association()).unwrap().unwrap();
    assert!(matches!(
        service.commit(&first).unwrap(),
        PatreonRefreshOutcome::Superseded { .. }
    ));
    let second = service.observe(association()).unwrap().unwrap();
    service.commit(&second).unwrap();
    let calls = backend.finish();
    let first: Value = serde_json::from_slice(&calls[1].2).unwrap();
    let second: Value = serde_json::from_slice(&calls[3].2).unwrap();
    assert_ne!(first["p_request"], second["p_request"]);
    assert_eq!(second["p_ticket"], updated);
    assert_eq!(second["p_proof"]["entitledTierIds"], json!([]));
    assert_eq!(provider.finish().len(), 2);
}
#[test]
fn null_ticket_and_invalid_input_never_contact_provider_or_commit() {
    let provider = Mock::new(vec![]);
    let backend = Mock::new(vec![prepare(Value::Null)]);
    let service = service(&backend.base, &provider.base);
    assert!(service.observe(Uuid::nil()).is_err());
    assert!(service.observe(association()).unwrap().is_none());
    backend.finish();
    assert!(provider.finish().is_empty());
}
#[test]
fn malformed_or_foreign_ticket_is_rejected_before_provider_dispatch() {
    let mut cases = vec![
        json!({}),
        json!([]),
        json!({"status":"success","data":ticket()}),
    ];
    let t = ticket();
    cases.push(json!([
        1,
        t["accountId"],
        t["associationId"],
        t["userId"],
        t["campaignId"],
        t["memberId"],
        t["expectedSourceId"],
        t["anchorEventId"],
        t["generation"]
    ]));
    for (field, value) in [
        ("associationId", json!(Uuid::new_v4())),
        ("accountId", json!(Uuid::nil())),
        (
            "expectedSourceId",
            json!("32345678123442349234123456789abc"),
        ),
        (
            "anchorEventId",
            json!("42345678-1234-4234-9234-123456789ABC"),
        ),
        ("generation", json!("01")),
        ("generation", json!("9223372036854775808")),
        ("generation", json!("0")),
        ("generation", json!(1)),
        ("campaignId", json!("other_campaign")),
        ("userId", json!("é")),
        ("memberId", json!("x".repeat(129))),
        ("version", json!(2)),
    ] {
        let mut v = ticket();
        v[field] = value;
        cases.push(v);
    }
    let mut extra = ticket();
    extra["tierId"] = json!(Uuid::new_v4());
    cases.push(extra);
    let mut missing = ticket();
    missing.as_object_mut().unwrap().remove("memberId");
    cases.push(missing);
    for value in cases {
        let provider = Mock::new(vec![]);
        let backend = Mock::new(vec![prepare(value)]);
        assert!(
            service(&backend.base, &provider.base)
                .observe(association())
                .is_err()
        );
        backend.finish();
        assert!(provider.finish().is_empty());
    }
    let provider = Mock::new(vec![]);
    let raw = serde_json::to_string(&ticket()).unwrap().replacen(
        "\"version\":1",
        "\"version\":1,\"version\":1",
        1,
    );
    let mut step = prepare(Value::Null);
    step.body = raw.into_bytes();
    let backend = Mock::new(vec![step]);
    assert!(
        service(&backend.base, &provider.base)
            .observe(association())
            .is_err()
    );
    backend.finish();
    provider.finish();
}
#[test]
fn malformed_or_mismatched_commit_results_never_confirm_success() {
    let mut cases = vec![
        json!({}),
        Value::Null,
        json!({"status":"success","data":result("applied")}),
    ];
    cases.push(json!([
        association(),
        result("applied")["sourceEventId"],
        "applied"
    ]));
    cases.push(json!([association(), null, "superseded"]));
    for (field, value) in [
        ("associationId", json!(Uuid::new_v4())),
        ("sourceEventId", Value::Null),
        ("sourceEventId", json!(Uuid::nil())),
        ("sourceEventId", json!("52345678123442349234123456789abc")),
        ("outcome", json!("pending")),
    ] {
        let mut v = result("applied");
        v[field] = value;
        cases.push(v);
    }
    let mut wrong_negative = result("superseded");
    wrong_negative["sourceEventId"] = json!(Uuid::new_v4());
    cases.push(wrong_negative);
    let mut missing = result("applied");
    missing.as_object_mut().unwrap().remove("sourceEventId");
    cases.push(missing);
    let mut extra = result("applied");
    extra["tierId"] = json!(Uuid::new_v4());
    cases.push(extra);
    for value in cases {
        let provider = provider();
        let backend = Mock::new(vec![prepare(ticket()), commit(value)]);
        let service = service(&backend.base, &provider.base);
        let attempt = service.observe(association()).unwrap().unwrap();
        assert!(service.commit(&attempt).is_err());
        backend.finish();
        provider.finish();
    }
    let provider = provider();
    let raw = serde_json::to_string(&result("applied")).unwrap().replacen(
        "\"outcome\":\"applied\"",
        "\"outcome\":\"applied\",\"outcome\":\"applied\"",
        1,
    );
    let mut step = commit(Value::Null);
    step.body = raw.into_bytes();
    let backend = Mock::new(vec![prepare(ticket()), step]);
    let service = service(&backend.base, &provider.base);
    let attempt = service.observe(association()).unwrap().unwrap();
    assert!(service.commit(&attempt).is_err());
    backend.finish();
    provider.finish();
}
#[test]
fn source_failure_missing_entitlement_or_foreign_identity_never_becomes_cancellation() {
    for case in [
        "404",
        "redirect",
        "missing",
        "foreign_user",
        "foreign_campaign",
        "wrong_member",
    ] {
        let mut value = f::source()["member"].clone();
        match case {
            "missing" => {
                value["data"]["relationships"]
                    .as_object_mut()
                    .unwrap()
                    .remove("currently_entitled_tiers");
            }
            "foreign_user" => {
                value["data"]["relationships"]["user"]["data"]["id"] = json!("other_user")
            }
            "foreign_campaign" => {
                value["data"]["relationships"]["campaign"]["data"]["id"] = json!("other_campaign")
            }
            "wrong_member" => value["data"]["id"] = json!("other_member"),
            _ => {}
        }
        let mut source = source_step(value);
        if case == "404" {
            source.status = 404;
        }
        if case == "redirect" {
            source.status = 302;
            source
                .response_headers
                .push(("Location".into(), "/unexpected".into()));
        }
        let provider = Mock::new(vec![source]);
        let backend = Mock::new(vec![prepare(ticket())]);
        let error = service(&backend.base, &provider.base)
            .observe(association())
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "patreon membership refresh is unresolved"
        );
        assert!(error.source().is_some());
        assert_eq!(format!("{error:?}"), error.to_string());
        backend.finish();
        assert_eq!(provider.finish().len(), 1);
    }
}
#[test]
fn an_attempt_cannot_be_committed_to_another_backend_origin() {
    let provider = provider();
    let original = Mock::new(vec![prepare(ticket())]);
    let first = service(&original.base, &provider.base);
    let attempt = first.observe(association()).unwrap().unwrap();
    let foreign = Mock::new(vec![]);
    let second = service(&foreign.base, &provider.base);
    assert!(second.commit(&attempt).is_err());
    original.finish();
    assert!(foreign.finish().is_empty());
    provider.finish();
}

#[test]
fn valid_negative_dispositions_keep_required_null_source_and_do_not_retry_automatically() {
    for phase in ["superseded", "unavailable"] {
        let provider = provider();
        let backend = Mock::new(vec![prepare(ticket()), commit(result(phase))]);
        let service = service(&backend.base, &provider.base);
        let attempt = service.observe(association()).unwrap().unwrap();
        let outcome = service.commit(&attempt).unwrap();
        assert_eq!(serde_json::to_value(outcome).unwrap(), result(phase));
        assert_eq!(backend.finish().len(), 2);
        assert_eq!(provider.finish().len(), 1);
    }
}

#[path = "refresh_work_tests.rs"]
mod work;

#[test]
fn historical_refresh_recovery_rejects_nested_object_and_array_enum_aliases() {
    let task = PatreonRefreshTask::new(association(), Uuid::new_v4()).unwrap();
    for bad in [json!({"applied":null}), json!(["applied"])] {
        let mut malformed = result("applied");
        malformed["outcome"] = bad;
        let backend = Mock::new(vec![Step::json(
            "/rest/v1/rpc/read_patreon_refresh_result",
            malformed,
        )]);
        let provider = Mock::new(vec![]);
        assert!(
            service(&backend.base, &provider.base)
                .recover(&task)
                .is_err()
        );
        assert_eq!(backend.finish().len(), 1);
        assert!(provider.finish().is_empty());
    }
}
