use super::*;
use serde_json::{Value, json};
fn id() -> Uuid {
    Uuid::new_v4()
}
fn proof() -> SourceProof {
    " fixture:private ".parse().unwrap()
}
fn stamp() -> DateTime<Utc> {
    "2026-11-04T12:00:00.123456Z".parse().unwrap()
}
fn bytes(value: Value) -> Vec<u8> {
    serde_json::to_vec(&value).unwrap()
}
fn pause() -> PauseGame {
    PauseGame {
        game_id: id(),
        owner_ref: proof(),
        epoch: 1.into(),
        operational_revision: 2.into(),
        incident_at: stamp(),
        kind: IncidentKind::Storage,
        proof_ref: proof(),
        request_id: id(),
    }
}
fn recovery(original: &PauseGame) -> Value {
    json!({"status":"success","data":{"resourceId":original.game_id,"checkpointId":id(),"epoch":"2","operationalRevision":"3","clockRevision":"4"}})
}
#[test]
fn game_source_decodes_required_nullable_clock_and_checked_original_frontier() {
    let original = pause();
    let wire = recovery(&original);
    let accepted = original.decode_response(&bytes(wire.clone())).unwrap();
    let ApiResponse::Success { data } = accepted else {
        panic!("expected success")
    };
    assert!(data.matches_source(data.checkpoint_id, Some(&4.into())));
    assert!(!data.matches_source(id(), Some(&4.into())));
    assert!(!data.matches_source(data.checkpoint_id, None));
    let mut changed = wire.clone();
    changed["data"]["clockRevision"] = Value::Null;
    assert!(original.decode_response(&bytes(changed.clone())).is_ok());
    changed["data"]
        .as_object_mut()
        .unwrap()
        .remove("clockRevision");
    assert!(original.decode_response(&bytes(changed)).is_err());
    for (field, value) in [
        ("resourceId", json!(id())),
        ("epoch", json!("3")),
        ("operationalRevision", json!("2")),
        ("clockRevision", json!("0")),
        ("clockRevision", json!(0)),
        ("checkpointId", json!(Uuid::nil())),
        (
            "checkpointId",
            json!("AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"),
        ),
        ("epoch", json!("02")),
        ("epoch", json!("9".repeat(79))),
        ("private", json!("unexpected")),
    ] {
        let mut changed = wire.clone();
        changed["data"][field] = value;
        assert!(
            original.decode_response(&bytes(changed)).is_err(),
            "{field}"
        );
    }
    let checkpoint = data.checkpoint_id;
    let restore = RestoreGame {
        game_id: original.game_id,
        epoch: 1.into(),
        operational_revision: 2.into(),
        checkpoint_id: checkpoint,
        new_owner_ref: proof(),
        restoration_ref: proof(),
        request_id: id(),
    };
    let mut restored = wire;
    restored["data"]["checkpointId"] = json!(checkpoint);
    assert!(restore.decode_response(&bytes(restored.clone())).is_ok());
    restored["data"]["checkpointId"] = json!(id());
    assert!(restore.decode_response(&bytes(restored)).is_err());
}
#[test]
fn raw_objects_duplicates_and_safe_errors_retain_boundary() {
    use std::error::Error;
    let p = pause();
    for wire in [
        format!(r#"{{"status":"success","data":["{}","{}","2","3",null]}}"#,p.game_id,id()),
        format!(r#"{{"status":"success","data":{{"resourceId":"{}","resourceId":"{}","checkpointId":"{}","epoch":"2","operationalRevision":"3","clockRevision":null}}}}"#,p.game_id,p.game_id,id()),
        r#"{"status":"fail","data":{"basis":"stale or unavailable","basis":"stale or unavailable"}}"#.to_owned(),
        r#"{"status":"error","message":"PRIVATE_CANARY"}"#.to_owned(),
        r#"{"status":"success","data":null}"#.to_owned(),
        r#"[]"#.to_owned(),
    ] { assert!(p.decode_response(wire.as_bytes()).is_err()); }
    let error = p.decode_response(b"{PRIVATE_CANARY").err().unwrap();
    assert!(error.source().is_some());
    assert!(!format!("{error:?} {error}").contains("PRIVATE_CANARY"));
}
#[test]
fn input_normalization_rejects_lossy_time_owner_nil_and_overflow_success() {
    let mut p = pause();
    let wire = recovery(&p);
    p.incident_at = p.incident_at.with_nanosecond(123456789).unwrap();
    assert!(p.decode_response(&bytes(wire.clone())).is_err());
    p.incident_at = stamp();
    p.owner_ref = "🂡".repeat(257).parse().unwrap();
    assert!(p.decode_response(&bytes(wire.clone())).is_err());
    p.owner_ref = format!("{}{}", " ".repeat(1000), "🂡".repeat(256))
        .parse()
        .unwrap();
    assert!(p.decode_response(&bytes(wire.clone())).is_ok());
    p.epoch = "9".repeat(78).parse().unwrap();
    assert!(p.decode_response(&bytes(wire.clone())).is_err());
    // The original maximum is a valid envelope; a stored fail can still replay.
    assert!(
        p.decode_response(br#"{"status":"fail","data":{"basis":"stale or unavailable"}}"#)
            .is_ok()
    );
    p.request_id = Uuid::nil();
    assert!(p.decode_response(&bytes(wire)).is_err());
    assert_eq!(IncidentKind::ServerProcess.as_str(), "server_process");
    assert_eq!(IncidentKind::Storage.as_str(), "storage");
}
#[test]
fn match_source_binds_both_generations_and_exact_declared_failures() {
    let p = PauseMatch {
        match_id: id(),
        basis: 2.into(),
        availability_basis: 3.into(),
        incident_at: stamp(),
        kind: IncidentKind::Storage,
        proof_ref: proof(),
        request_id: id(),
    };
    let incident = id();
    let wire = json!({"status":"success","data":{"resourceId":p.match_id,"incidentId":incident,"revision":"3","availabilityRevision":"4"}});
    assert!(p.decode_response(&bytes(wire.clone())).is_ok());
    for (key, value) in [
        ("revision", json!("4")),
        ("availabilityRevision", json!("3")),
        ("incidentId", Value::Null),
        ("resourceId", json!(id())),
    ] {
        let mut w = wire.clone();
        w["data"][key] = value;
        assert!(p.decode_response(&bytes(w)).is_err());
    }
    let r = ResumeMatch {
        match_id: p.match_id,
        basis: 3.into(),
        availability_basis: 4.into(),
        incident_id: incident,
        proof_ref: proof(),
        request_id: id(),
    };
    assert!(r.decode_response(&bytes(json!({"status":"success","data":{"resourceId":p.match_id,"revision":"4","availabilityRevision":"5"}}))).is_ok());
    assert!(r.decode_response(&bytes(wire)).is_err());
    for (key, message) in [
        ("basis", "stale or unavailable match phase"),
        ("clock", "outside inactive phase boundary"),
    ] {
        assert!(
            p.decode_response(&bytes(json!({"status":"fail","data":{key:message}})))
                .is_ok()
        );
    }
    assert!(
        r.decode_response(
            br#"{"status":"fail","data":{"basis":"stale or unavailable match incident"}}"#
        )
        .is_ok()
    );
    assert!(
        p.decode_response(
            br#"{"status":"fail","data":{"basis":"stale or unavailable match incident"}}"#
        )
        .is_err()
    );
    assert!(r.decode_response(br#"{"status":"fail","data":{"basis":"stale or unavailable match incident","clock":"outside inactive phase boundary"}}"#).is_err());
    let g = pause();
    for key in ["basis", "incidentAt"] {
        let message = if key == "basis" {
            "stale or unavailable"
        } else {
            "predates durable clock boundary"
        };
        assert!(
            g.decode_response(&bytes(json!({"status":"fail","data":{key:message}})))
                .is_ok()
        );
    }
    assert!(
        g.decode_response(br#"{"status":"fail","data":{}}"#)
            .is_err()
    );
}
#[test]
fn terminal_and_publication_are_success_only_with_distinct_resource_ids() {
    let t = FinalizeUnrecoverableMatch {
        match_id: id(),
        basis: 0.into(),
        availability_basis: 1.into(),
        incident_id: id(),
        failure_ref: proof(),
    };
    let result = id();
    let wire = json!({"status":"success","data":{"resourceId":t.match_id,"terminalResultId":result,"revision":"1","availabilityRevision":"2","phase":"terminal"}});
    assert_eq!(
        t.decode_response(&bytes(wire.clone()))
            .unwrap()
            .terminal_result_id,
        result
    );
    for (field, value) in [
        ("phase", json!("between_games")),
        ("terminalResultId", Value::Null),
        ("availabilityRevision", json!("1")),
    ] {
        let mut w = wire.clone();
        w["data"][field] = value;
        assert!(t.decode_response(&bytes(w)).is_err());
    }
    let p = PublishReplay {
        game_id: id(),
        revision: 0.into(),
        initial_checkpoint_id: id(),
        through_event_sequence: 0.into(),
        artifact_ref: proof(),
        visibility_index_ref: proof(),
        schema_revision: proof(),
        qualification_ref: proof(),
    };
    let manifest = id();
    let w = json!({"status":"success","data":{"resourceId":manifest,"gameId":p.game_id,"throughSequence":"0","revision":"0"}});
    assert_eq!(
        p.decode_response(&bytes(w.clone())).unwrap().resource_id,
        manifest
    );
    for (field, value) in [
        ("gameId", json!(id())),
        ("throughSequence", json!("1")),
        ("revision", json!("1")),
        ("resourceId", json!(Uuid::nil())),
    ] {
        let mut changed = w.clone();
        changed["data"][field] = value;
        assert!(p.decode_response(&bytes(changed)).is_err());
    }
    for w in [
        br#"{"status":"fail","data":{"basis":"stale"}}"#.as_slice(),
        br#"{"status":"error","message":"unavailable"}"#,
    ] {
        assert!(t.decode_response(w).is_err());
        assert!(p.decode_response(w).is_err());
    }
}
