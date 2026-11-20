//! Actual bounded HTTP for logical delivery; not actual Temporal or live Patreon qualification.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
fn task() -> PatreonRefreshTask {
    PatreonRefreshTask::new(association(), Uuid::new_v4()).unwrap()
}
fn recovery(value: Value) -> Step {
    Step::json("/rest/v1/rpc/read_patreon_refresh_result", value)
}
fn discovery(value: Value) -> Step {
    Step::json("/rest/v1/rpc/read_patreon_refresh_work", value)
}

#[test]
fn discovery_allocates_only_application_ids_and_preserves_bounded_cursor_contract() {
    let provider = Mock::new(vec![]);
    let backend = Mock::new(vec![
        discovery(json!([])),
        discovery(json!([association()])),
    ]);
    let service = service(&backend.base, &provider.base);
    assert!(service.discover(None).unwrap().is_none());
    let cursor = Uuid::from_u128(1);
    let task = service.discover(Some(cursor)).unwrap().unwrap();
    assert_eq!(task.association_id(), association());
    assert!(!task.request_id().is_nil());
    assert_eq!(
        serde_json::to_value(task).unwrap(),
        json!({"associationId":association(),"requestId":task.request_id()})
    );
    let calls = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[0].2).unwrap(),
        json!({"p_campaign":"123","p_after":null,"p_limit":1})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[1].2).unwrap(),
        json!({"p_campaign":"123","p_after":cursor,"p_limit":1})
    );
    provider.finish();
}
#[test]
fn malformed_noncanonical_or_nonadvancing_discovery_never_generates_delivery() {
    for value in [
        Value::Null,
        json!({}),
        json!(1),
        json!([association(), Uuid::new_v4()]),
        json!([Uuid::nil()]),
        json!([association().simple().to_string()]),
        json!([association().to_string().to_uppercase()]),
        json!([42]),
    ] {
        let provider = Mock::new(vec![]);
        let backend = Mock::new(vec![discovery(value)]);
        assert!(
            service(&backend.base, &provider.base)
                .discover(None)
                .is_err()
        );
        backend.finish();
        provider.finish();
    }
    for cursor in [association(), Uuid::from_u128(u128::MAX)] {
        let provider = Mock::new(vec![]);
        let backend = Mock::new(vec![discovery(json!([association()]))]);
        assert!(
            service(&backend.base, &provider.base)
                .discover(Some(cursor))
                .is_err()
        );
        backend.finish();
        provider.finish();
    }
    let provider = Mock::new(vec![]);
    let backend = Mock::new(vec![]);
    assert!(
        service(&backend.base, &provider.base)
            .discover(Some(Uuid::nil()))
            .is_err()
    );
    backend.finish();
    provider.finish();
}
#[test]
fn recorded_task_is_strict_object_only_and_recovery_short_circuits_all_provider_work() {
    let task = task();
    let value = serde_json::to_value(task).unwrap();
    for bad in [
        json!([association(), task.request_id()]),
        json!({"associationId":association(),"requestId":Uuid::nil()}),
        json!({"associationId":association(),"requestId":task.request_id(),"memberId":"private"}),
        json!({"associationId":association()}),
    ] {
        assert!(serde_json::from_value::<PatreonRefreshTask>(bad).is_err());
    }
    let raw = serde_json::to_string(&value).unwrap().replacen(
        "\"associationId\":",
        &format!("\"associationId\":\"{}\",\"associationId\":", association()),
        1,
    );
    assert!(serde_json::from_str::<PatreonRefreshTask>(&raw).is_err());
    assert!(PatreonRefreshTask::new(Uuid::nil(), Uuid::new_v4()).is_err());
    assert!(PatreonRefreshTask::new(association(), Uuid::nil()).is_err());
    let provider = Mock::new(vec![]);
    let backend = Mock::new(vec![recovery(result("applied"))]);
    assert_eq!(
        serde_json::to_value(
            service(&backend.base, &provider.base)
                .deliver(&task)
                .unwrap()
        )
        .unwrap(),
        result("applied")
    );
    let calls = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&calls[0].2).unwrap(),
        json!({"p_association":association(),"p_request":task.request_id()})
    );
    provider.finish();
}
#[test]
fn malformed_or_foreign_recovery_never_authorizes_provider_work() {
    let mut foreign = result("applied");
    foreign["associationId"] = json!(Uuid::new_v4());
    let mut no_source = result("applied");
    no_source["sourceEventId"] = Value::Null;
    let mut missing = result("applied");
    missing.as_object_mut().unwrap().remove("sourceEventId");
    for value in [
        json!({}),
        json!([association(), Uuid::new_v4(), "applied"]),
        json!(1),
        foreign,
        no_source,
        missing,
        result("unavailable"),
        result("superseded"),
    ] {
        let provider = Mock::new(vec![]);
        let backend = Mock::new(vec![recovery(value)]);
        assert!(
            service(&backend.base, &provider.base)
                .deliver(&task())
                .is_err()
        );
        backend.finish();
        provider.finish();
    }
    let mut duplicate = recovery(result("applied"));
    duplicate.body = serde_json::to_string(&result("applied"))
        .unwrap()
        .replacen(
            "\"outcome\":\"applied\"",
            "\"outcome\":\"applied\",\"outcome\":\"applied\"",
            1,
        )
        .into_bytes();
    let provider = Mock::new(vec![]);
    let backend = Mock::new(vec![duplicate]);
    assert!(
        service(&backend.base, &provider.base)
            .deliver(&task())
            .is_err()
    );
    backend.finish();
    provider.finish();
    let mut unavailable = recovery(Value::Null);
    unavailable.status = 503;
    let provider = Mock::new(vec![]);
    let backend = Mock::new(vec![unavailable]);
    assert!(
        service(&backend.base, &provider.base)
            .deliver(&task())
            .is_err()
    );
    backend.finish();
    provider.finish();
}
#[test]
fn logical_delivery_uses_recorded_request_and_recovers_unknown_or_conflicting_commit() {
    for failure in ["conflict", "unknown", "malformed"] {
        let provider = super::provider();
        let mut failed = commit(Value::Null);
        if failure == "conflict" {
            failed.status = 409;
        } else if failure == "unknown" {
            failed.drop_ack = true;
        } else {
            failed.body = b"{}".to_vec();
        }
        let backend = Mock::new(vec![
            recovery(Value::Null),
            prepare(ticket()),
            failed,
            recovery(result("applied")),
        ]);
        let task = task();
        assert_eq!(
            serde_json::to_value(
                service(&backend.base, &provider.base)
                    .deliver(&task)
                    .unwrap()
            )
            .unwrap(),
            result("applied")
        );
        let calls = backend.finish();
        let input: Value = serde_json::from_slice(&calls[2].2).unwrap();
        assert_eq!(input["p_request"], json!(task.request_id()));
        assert_eq!(input["p_ticket"], ticket());
        assert_eq!(calls[0].2, calls[3].2);
        assert_eq!(provider.finish().len(), 1);
    }
}
#[test]
fn reconstructed_service_recovers_same_task_after_unknown_commit_and_unknown_recovery() {
    let provider = super::provider();
    let mut failed = commit(Value::Null);
    failed.drop_ack = true;
    let mut lost = recovery(Value::Null);
    lost.drop_ack = true;
    let backend = Mock::new(vec![
        recovery(Value::Null),
        prepare(ticket()),
        failed,
        lost,
        recovery(result("applied")),
    ]);
    let task = task();
    let recorded = serde_json::to_vec(&task).unwrap();
    let first = service(&backend.base, &provider.base);
    assert!(first.deliver(&task).is_err());
    drop(first);
    let restored: PatreonRefreshTask = serde_json::from_slice(&recorded).unwrap();
    assert_eq!(
        serde_json::to_value(
            service(&backend.base, &provider.base)
                .deliver(&restored)
                .unwrap()
        )
        .unwrap(),
        result("applied")
    );
    let calls = backend.finish();
    assert_eq!(calls[0].2, calls[3].2);
    assert_eq!(calls[0].2, calls[4].2);
    assert_eq!(provider.finish().len(), 1);
}
#[test]
fn absent_acceptance_retry_observes_fresh_ticket_and_source_under_the_same_logical_request() {
    let mut empty = f::source()["member"].clone();
    empty["data"]["relationships"]["currently_entitled_tiers"]["data"] = json!([]);
    let provider = Mock::new(vec![
        source_step(f::source()["member"].clone()),
        source_step(empty),
    ]);
    let mut failed = commit(Value::Null);
    failed.status = 409;
    let mut fresh = ticket();
    fresh["generation"] = json!("2");
    fresh["expectedSourceId"] = json!(Uuid::new_v4());
    let backend = Mock::new(vec![
        recovery(Value::Null),
        prepare(ticket()),
        failed,
        recovery(Value::Null),
        recovery(Value::Null),
        prepare(fresh.clone()),
        commit(result("applied")),
    ]);
    let task = task();
    let service = service(&backend.base, &provider.base);
    assert!(service.deliver(&task).is_err());
    assert!(matches!(
        service.deliver(&task).unwrap(),
        PatreonRefreshOutcome::Applied { .. }
    ));
    let calls = backend.finish();
    let first: Value = serde_json::from_slice(&calls[2].2).unwrap();
    let second: Value = serde_json::from_slice(&calls[6].2).unwrap();
    assert_eq!(first["p_request"], json!(task.request_id()));
    assert_eq!(first["p_request"], second["p_request"]);
    assert_eq!(first["p_ticket"], ticket());
    assert_eq!(second["p_ticket"], fresh);
    assert_ne!(
        first["p_proof"]["memberHash"],
        second["p_proof"]["memberHash"]
    );
    assert_eq!(second["p_proof"]["entitledTierIds"], json!([]));
    assert_eq!(provider.finish().len(), 2);
}
#[test]
fn absent_failed_or_malformed_post_commit_recovery_preserves_original_backend_error() {
    for response in [Value::Null, json!({}), result("superseded")] {
        let provider = super::provider();
        let mut failed = commit(Value::Null);
        failed.status = 409;
        let backend = Mock::new(vec![
            recovery(Value::Null),
            prepare(ticket()),
            failed,
            recovery(response),
        ]);
        let error = service(&backend.base, &provider.base)
            .deliver(&task())
            .err()
            .unwrap();
        assert!(matches!(error.0, Failure::Backend(BackendError::Request)));
        assert_eq!(
            error.to_string(),
            "patreon membership refresh is unresolved"
        );
        backend.finish();
        provider.finish();
    }
}
#[test]
fn negative_dispositions_and_foreign_campaign_preserve_fresh_preparation_rules() {
    let provider = Mock::new(vec![]);
    let backend = Mock::new(vec![recovery(Value::Null), prepare(Value::Null)]);
    assert_eq!(
        serde_json::to_value(
            service(&backend.base, &provider.base)
                .deliver(&task())
                .unwrap()
        )
        .unwrap(),
        result("unavailable")
    );
    backend.finish();
    provider.finish();
    let mut foreign = ticket();
    foreign["campaignId"] = json!("other");
    let provider = Mock::new(vec![]);
    let backend = Mock::new(vec![recovery(Value::Null), prepare(foreign)]);
    assert!(
        service(&backend.base, &provider.base)
            .deliver(&task())
            .is_err()
    );
    backend.finish();
    provider.finish();
    for phase in ["superseded", "unavailable"] {
        let provider = super::provider();
        let backend = Mock::new(vec![
            recovery(Value::Null),
            prepare(ticket()),
            commit(result(phase)),
        ]);
        assert_eq!(
            serde_json::to_value(
                service(&backend.base, &provider.base)
                    .deliver(&task())
                    .unwrap()
            )
            .unwrap(),
            result(phase)
        );
        backend.finish();
        provider.finish();
    }
}
#[test]
fn observed_stop_fences_every_later_network_group_without_aborting_inflight_acknowledgment() {
    for boundary in ["before", "recovery", "prepare", "provider", "commit"] {
        let stopped = Arc::new(AtomicBool::new(boundary == "before"));
        let signal = stopped.clone();
        let target = boundary.to_owned();
        let mut steps = vec![];
        if boundary != "before" {
            steps.push(recovery(Value::Null));
        }
        if matches!(boundary, "prepare" | "provider" | "commit") {
            steps.push(prepare(ticket()));
        }
        if boundary == "commit" {
            let mut failed = commit(Value::Null);
            failed.drop_ack = true;
            steps.push(failed);
        }
        let backend = Mock::new_with_observer(steps, move |path| {
            if (target == "recovery" && path.ends_with("read_patreon_refresh_result"))
                || (target == "prepare" && path.ends_with("read_patreon_refresh"))
                || (target == "commit" && path.ends_with("commit_patreon_refresh"))
            {
                signal.store(true, Ordering::Release);
            }
        });
        let signal = stopped.clone();
        let on_provider = boundary == "provider";
        let provider = Mock::new_with_observer(
            if matches!(boundary, "provider" | "commit") {
                vec![source_step(f::source()["member"].clone())]
            } else {
                vec![]
            },
            move |_| {
                if on_provider {
                    signal.store(true, Ordering::Release);
                }
            },
        );
        let service = service(&backend.base, &provider.base).with_stop(stopped);
        if boundary == "before" {
            assert!(service.discover(None).is_err());
        }
        assert!(service.deliver(&task()).is_err(), "{boundary}");
        backend.finish();
        provider.finish();
    }
}
