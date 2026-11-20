use super::*;
use chrono::Timelike;
use serde_json::{Value, json};
fn id() -> Uuid {
    Uuid::new_v4()
}
fn proof() -> SourceProof {
    "fixture:proof".parse().unwrap()
}
fn time() -> DateTime<Utc> {
    "2026-11-04T12:00:00.123456Z".parse().unwrap()
}
fn bytes(v: Value) -> Vec<u8> {
    serde_json::to_vec(&v).unwrap()
}
fn schedule() -> ScheduleDeadline {
    ScheduleDeadline {
        decision_id: id(),
        match_revision: 4.into(),
        deadline_revision: 0.into(),
        clock_revision: 2.into(),
        availability_revision: 0.into(),
        profile_id: id(),
        policy_ref: proof(),
        due_at: time(),
        qualification_ref: proof(),
        request_id: id(),
    }
}
fn schedule_ack(p: &ScheduleDeadline) -> Value {
    json!({"status":"success","data":{"resourceId":p.decision_id,"deadlineRevision":"1","dueAt":due_text(p.due_at).unwrap()}})
}
fn expiry() -> ExpireDeadline {
    ExpireDeadline {
        decision_id: id(),
        deadline_revision: 1.into(),
        selected_revision_id: id(),
        reply_ref: proof(),
        qualification_ref: proof(),
        request_id: id(),
    }
}
fn expiry_ack(p: &ExpireDeadline) -> Value {
    json!({"status":"success","data":{"resourceId":p.decision_id,"replyReceiptId":id(),"deadlineRevision":"1","revision":"42","reservedGameId":""}})
}
#[test]
fn schedules_freeze_microsecond_time_and_original_generation_budget() {
    let mut p = schedule();
    let w = schedule_ack(&p);
    assert_eq!(due_text(p.due_at).unwrap(), "2026-11-04T12:00:00.123456Z");
    assert!(p.decode_response(&bytes(w.clone())).is_ok());
    p.deadline_revision = 1.into();
    let mut changed = w.clone();
    changed["data"]["deadlineRevision"] = json!("2");
    assert!(p.decode_response(&bytes(changed)).is_err());
    assert!(
        p.decode_response(br#"{"status":"fail","data":{"decision":"unavailable"}}"#)
            .is_ok()
    );
    p.deadline_revision = 0.into();
    p.due_at = p.due_at.with_nanosecond(123456789).unwrap();
    assert!(p.original_terms_octets().is_none());
    assert!(p.decode_response(&bytes(w.clone())).is_err());
    p.due_at = time();
    let initial = p.original_terms_octets().unwrap();
    p.policy_ref = format!(
        "{}fixture:proof",
        " ".repeat(MAX_DEADLINE_TERMS_BYTES - initial)
    )
    .parse()
    .unwrap();
    assert_eq!(p.original_terms_octets(), Some(MAX_DEADLINE_TERMS_BYTES));
    assert!(p.decode_response(&bytes(w.clone())).is_ok());
    p.policy_ref = format!(" {}", p.policy_ref.as_str()).parse().unwrap();
    assert_eq!(
        p.original_terms_octets(),
        Some(MAX_DEADLINE_TERMS_BYTES + 1)
    );
    assert!(p.decode_response(&bytes(w)).is_err());
    assert!(due_text("+010000-01-01T00:00:00Z".parse().unwrap()).is_none());
}
#[test]
fn selection_required_empty_uuid_and_qualification_are_exact() {
    let p = BetweenGameReply {
        decision_id: id(),
        basis: 3.into(),
        selected_revision_id: id(),
        reply_ref: proof(),
        qualification_ref: proof(),
        request_id: id(),
    };
    let w = json!({"status":"success","data":{"resourceId":p.decision_id,"revision":"4","reservedGameId":"","qualificationRef":p.qualification_ref.as_str()}});
    assert!(p.decode_response(&bytes(w.clone())).is_ok());
    let mut next = w.clone();
    next["data"]["reservedGameId"] = json!(id());
    assert!(p.decode_response(&bytes(next)).is_ok());
    for (field, value) in [
        ("reservedGameId", Value::Null),
        ("reservedGameId", json!(Uuid::nil())),
        (
            "reservedGameId",
            json!("AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"),
        ),
        ("reservedGameId", json!(" ")),
        ("revision", json!("3")),
        ("qualificationRef", json!("other")),
        ("resourceId", json!(id())),
        ("revision", json!(4)),
        ("private", json!("no")),
    ] {
        let mut changed = w.clone();
        changed["data"][field] = value;
        assert!(p.decode_response(&bytes(changed)).is_err(), "{field}");
    }
    let mut missing = w;
    missing["data"]
        .as_object_mut()
        .unwrap()
        .remove("reservedGameId");
    assert!(p.decode_response(&bytes(missing)).is_err());
    let d = expiry();
    let w = expiry_ack(&d);
    let ApiResponse::Success { data } = d.decode_response(&bytes(w.clone())).unwrap() else {
        panic!("expected success")
    };
    assert!(data.matches_source(data.reply_receipt_id, &41.into()));
    assert!(!data.matches_source(data.reply_receipt_id, &1.into()));
    let mut next = w;
    next["data"]["reservedGameId"] = json!(id());
    assert!(d.decode_response(&bytes(next)).is_ok());
    assert!(
        p.decode_response(
            br#"{"status":"fail","data":{"deadline":"qualified cutoff has elapsed"}}"#
        )
        .is_ok()
    );
    assert!(
        d.decode_response(
            br#"{"status":"fail","data":{"deadline":"qualified cutoff has elapsed"}}"#
        )
        .is_err()
    );
    for &(key, message) in SELECTION_FAILURES {
        assert!(
            p.decode_response(&bytes(json!({"status":"fail","data":{key:message}})))
                .is_ok()
        );
        assert!(
            d.decode_response(&bytes(json!({"status":"fail","data":{key:message}})))
                .is_ok()
        );
    }
}
#[test]
fn expiry_budget_and_raw_shape_failures_never_become_source_success() {
    let mut p = expiry();
    let w = expiry_ack(&p);
    let initial = p.original_terms_octets().unwrap();
    p.reply_ref = format!(
        "{}fixture:proof",
        " ".repeat(MAX_DEADLINE_TERMS_BYTES - initial)
    )
    .parse()
    .unwrap();
    assert_eq!(p.original_terms_octets(), Some(MAX_DEADLINE_TERMS_BYTES));
    assert!(p.decode_response(&bytes(w.clone())).is_ok());
    p.reply_ref = format!(" {}", p.reply_ref.as_str()).parse().unwrap();
    assert!(p.decode_response(&bytes(w)).is_err());
    let p = expiry();
    for raw in [
        format!(r#"{{"status":"success","data":["{}","{}","1","42",""]}}"#,p.decision_id,id()),
        format!(r#"{{"status":"success","data":{{"resourceId":"{}","resourceId":"{}","replyReceiptId":"{}","deadlineRevision":"1","revision":"42","reservedGameId":""}}}}"#,p.decision_id,p.decision_id,id()),
        r#"{"status":"fail","data":{"basis":"stale or unavailable between-game decision","basis":"stale or unavailable between-game decision"}}"#.to_owned(),
        r#"{"status":"error","message":"PRIVATE_CANARY"}"#.to_owned(),
        r#"[]"#.to_owned(),
    ] {assert!(p.decode_response(raw.as_bytes()).is_err());}
    let mut p = expiry();
    p.deadline_revision = 0.into();
    assert!(p.decode_response(&bytes(expiry_ack(&p))).is_err());
}
fn timer(decision: Uuid) -> Value {
    json!({"version":"between-deadline-snapshot-v1","decisionId":decision,"matchId":id(),"seatId":id(),"afterGameId":id(),"openedAt":"2026-11-04T11:00:00Z","basisRevision":"2","clockRevision":"3","policyRef":"fixture:policy","revision":"1","originReceiptId":id(),"sourceReceiptId":id(),"availabilityRevision":"0","accountedAt":"2026-11-04T12:30:00Z","dueAt":"2026-11-04T12:00:00Z","remainingUs":null})
}
#[test]
fn due_page_orders_ids_without_inventing_as_of_clock_or_wire_cursor() {
    let mut ids = [id(), id()];
    ids.sort();
    let query = DueDeadlines {
        after: None,
        limit: 2,
    };
    // accountedAt intentionally follows observedAt: source SELECT can see a concurrent arm.
    let w = json!({"observedAt":"2026-11-04T12:00:01Z","items":[timer(ids[0]),timer(ids[1])]});
    let page = query.decode_response(&bytes(w.clone())).unwrap();
    assert_eq!(page.continuation(&query), Some(ids[1]));
    let after = DueDeadlines {
        after: Some(ids[0]),
        limit: 2,
    };
    assert!(after.decode_response(&bytes(w.clone())).is_err());
    assert!(page.continuation(&after).is_none());
    let small = DueDeadlines {
        after: None,
        limit: 1,
    };
    assert!(small.decode_response(&bytes(w.clone())).is_err());
    for (key, value) in [
        ("revision", json!("0")),
        ("remainingUs", json!("1")),
        ("dueAt", json!("2026-11-04T13:00:00Z")),
        ("version", json!("wrong")),
        ("decisionId", json!(ids[1])),
    ] {
        let mut changed = w.clone();
        changed["items"][0][key] = value;
        assert!(query.decode_response(&bytes(changed)).is_err(), "{key}");
    }
    let mut missing = w.clone();
    missing["items"][0]
        .as_object_mut()
        .unwrap()
        .remove("remainingUs");
    assert!(query.decode_response(&bytes(missing)).is_err());
    let mut extra = w;
    extra["nextCursor"] = json!(ids[1]);
    assert!(query.decode_response(&bytes(extra)).is_err());
    for raw in [
        r#"{"observedAt":"2026-11-04T12:00:01Z","items":[[]]}"#,
        r#"{"observedAt":"2026-11-04T12:00:01Z","items":null}"#,
    ] {
        assert!(query.decode_response(raw.as_bytes()).is_err());
    }
    let empty = query
        .decode_response(br#"{"observedAt":"2026-11-04T12:00:01Z","items":[]}"#)
        .unwrap();
    assert!(empty.continuation(&query).is_none());
}
