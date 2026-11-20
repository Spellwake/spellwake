use super::*;
fn id() -> Uuid {
    Uuid::new_v4()
}
fn bytes(value: Value) -> Vec<u8> {
    serde_json::to_vec(&value).unwrap()
}
fn proof() -> Value {
    json!({"family":"patreon-financial-v1","occurrenceRef":"provider:occurrence","memberRef":"provider:member","paymentRef":"provider:payment","tierRef":"provider:tier","initiatedAt":"2026-10-01T00:00:00.000000Z","paidAt":"2026-10-01T00:00:00.000000Z","occurredAt":"2026-10-01T00:00:00.000000Z","state":"paid","amountMinor":"500","currency":"usd","bodyHash":"a".repeat(64),"evidenceRef":"fixture:private"})
}
fn observation(value: Value) -> Result<FinancialObservation, SourceResponseError> {
    FinancialObservation::from_bytes("opaque:delivery", &bytes(value))
}
#[test]
fn financial_proof_preserves_original_opaque_terms_and_exact_time_grammar() {
    let value = proof();
    let original = observation(value.clone()).unwrap();
    assert_eq!(original.original_terms(), value);
    assert_eq!(original.delivery_ref(), "opaque:delivery");
    for (key, value) in [
        ("family", json!("current-member")),
        ("amountMinor", json!("0")),
        ("amountMinor", json!(500)),
        ("amountMinor", json!("01")),
        ("amountMinor", json!("1".repeat(31))),
        ("currency", json!("USD")),
        ("bodyHash", json!("A".repeat(64))),
        ("state", json!("active")),
        ("tierRef", json!(" ")),
        ("memberRef", json!("🂡".repeat(257))),
        ("private", json!("extra")),
    ] {
        let mut changed = proof();
        changed[key] = value;
        assert!(observation(changed).is_err(), "{key}");
    }
    for invalid in [
        "2026-10-01T00:00:00Z",
        "2026-10-01T00:00:00.000000+00:00",
        "2026-10-01T00:00:00.0000001Z",
        "2026-02-30T00:00:00.000000Z",
        "0000-10-01T00:00:00.000000Z",
        "2026-10-01T00:00:60.000000Z",
    ] {
        let mut changed = proof();
        changed["initiatedAt"] = json!(invalid);
        assert!(observation(changed).is_err(), "{invalid}");
    }
    let mut changed = proof();
    changed["initiatedAt"] = json!("2026-10-02T00:00:00.000000Z");
    assert!(observation(changed).is_err());
    let mut changed = proof();
    changed["occurredAt"] = json!("2026-10-02T00:00:00.000000Z");
    assert!(observation(changed.clone()).is_err());
    changed["state"] = json!("refunded");
    assert!(observation(changed).is_ok());
    assert!(FinancialObservation::from_bytes(&"d".repeat(201), &bytes(proof())).is_err());
}
#[test]
fn financial_source_budget_preserves_padding_unicode_and_rejects_raw_duplicates() {
    let mut value = proof();
    value["evidenceRef"] = json!("fixture:🂡\n\"\\");
    let initial = observation(value.clone())
        .unwrap()
        .original_terms_octets()
        .unwrap();
    value["evidenceRef"] = json!(format!(
        "{}{}",
        " ".repeat(MAX_FINANCIAL_PROOF_BYTES - initial),
        value["evidenceRef"].as_str().unwrap()
    ));
    assert_eq!(
        observation(value.clone()).unwrap().original_terms_octets(),
        Some(MAX_FINANCIAL_PROOF_BYTES)
    );
    value["evidenceRef"] = json!(format!(" {}", value["evidenceRef"].as_str().unwrap()));
    assert!(observation(value).is_err());
    let wire = String::from_utf8(bytes(proof())).unwrap().replacen(
        "{",
        "{\"family\":\"patreon-financial-v1\",",
        1,
    );
    assert!(FinancialObservation::from_bytes("d", wire.as_bytes()).is_err());
    assert!(FinancialObservation::from_bytes("d", b"[]").is_err());
}
#[test]
fn financial_bare_results_bind_conditional_null_and_historical_payment_state() {
    let paid = observation(proof()).unwrap();
    let payment = id();
    for disposition in ["applied", "noop"] {
        let ack = paid
            .decode_response(&bytes(
                json!({"paymentId":payment,"disposition":disposition,"state":"paid"}),
            ))
            .unwrap();
        assert!(ack.matches_payment(Some(payment)));
        assert!(!ack.matches_payment(None));
        assert!(!ack.matches_payment(Some(id())));
    }
    let ack = paid
        .decode_response(&bytes(
            json!({"paymentId":null,"disposition":"ineligible_after_deletion","state":"paid"}),
        ))
        .unwrap();
    assert!(ack.matches_payment(None));
    assert!(!ack.matches_payment(Some(payment)));
    for state in ["refunded", "reversed"] {
        assert!(
            paid.decode_response(&bytes(
                json!({"paymentId":payment,"disposition":"noop","state":state})
            ))
            .is_ok()
        );
    }
    let mut proof = proof();
    proof["state"] = json!("refunded");
    let refunded = observation(proof).unwrap();
    assert!(
        refunded
            .decode_response(&bytes(
                json!({"paymentId":payment,"disposition":"noop","state":"paid"})
            ))
            .is_err()
    );
    assert!(
        refunded
            .decode_response(&bytes(
                json!({"paymentId":payment,"disposition":"noop","state":"reversed"})
            ))
            .is_ok()
    );
    for bad in [
        json!({"paymentId":null,"disposition":"applied","state":"paid"}),
        json!({"paymentId":payment,"disposition":"ineligible_after_deletion","state":"paid"}),
        json!({"paymentId":payment,"disposition":"applied","state":"reversed"}),
        json!({"disposition":"ineligible_after_deletion","state":"paid"}),
        json!({"paymentId":Uuid::nil(),"disposition":"noop","state":"paid"}),
        json!({"paymentId":payment,"disposition":"duplicate","state":"paid"}),
        json!({"status":"success","data":{"paymentId":payment,"disposition":"applied","state":"paid"}}),
    ] {
        assert!(paid.decode_response(&bytes(bad)).is_err());
    }
    assert!(paid.decode_response(br#"{"paymentId":null,"disposition":"ineligible_after_deletion","state":"paid","state":"paid"}"#).is_err());
    assert!(
        paid.decode_response(br#"[null,"ineligible_after_deletion","paid"]"#)
            .is_err()
    );
}
#[test]
fn notifications_bind_event_and_known_nullable_historical_context_exactly() {
    let command = ProcessNotification { event_id: id() };
    let association = id();
    let refresh = id();
    for (disposition, a, r) in [
        ("refreshed", Some(association), Some(refresh)),
        ("ignored_unlinked", None, None),
        ("ignored_inactive", Some(association), None),
        ("ignored_stale_member", Some(association), None),
    ] {
        let ack=command.decode_response(&bytes(json!({"eventId":command.event_id,"disposition":disposition,"associationId":a,"refreshSourceId":r}))).unwrap();
        assert!(ack.matches_source(&NotificationContext {
            association_id: a,
            refresh_source_id: r
        }));
        assert!(!ack.matches_source(&NotificationContext {
            association_id: Some(id()),
            refresh_source_id: r
        }));
        assert!(!ack.matches_source(&NotificationContext {
            association_id: a,
            refresh_source_id: Some(id())
        }));
    }
    for bad in [
        json!({"eventId":id(),"disposition":"ignored_unlinked","associationId":null,"refreshSourceId":null}),
        json!({"eventId":command.event_id,"disposition":"refreshed","associationId":association,"refreshSourceId":null}),
        json!({"eventId":command.event_id,"disposition":"ignored_unlinked","associationId":association,"refreshSourceId":null}),
        json!({"eventId":command.event_id,"disposition":"ignored_inactive","associationId":null,"refreshSourceId":null}),
        json!({"eventId":command.event_id,"disposition":"ignored_stale_member","associationId":association}),
        json!({"eventId":command.event_id,"disposition":"ignored_stale_member","associationId":association,"refreshSourceId":Uuid::nil()}),
    ] {
        assert!(command.decode_response(&bytes(bad)).is_err());
    }
    assert!(
        command
            .decode_response(
                format!(r#"["{}","ignored_unlinked",null,null]"#, command.event_id).as_bytes()
            )
            .is_err()
    );
    assert!(command.decode_response(format!(r#"{{"eventId":"{}","disposition":"ignored_unlinked","associationId":null,"refreshSourceId":null,"refreshSourceId":null}}"#,command.event_id).as_bytes()).is_err());
}

#[test]
fn source_enums_are_strings_never_externally_tagged_objects_or_arrays() {
    for (key, tag) in [("family", "patreon-financial-v1"), ("state", "paid")] {
        for value in [json!({tag:null}), json!([tag]), json!(null), json!(true)] {
            let mut changed = proof();
            changed[key] = value;
            assert!(observation(changed).is_err());
        }
    }
    let original = observation(proof()).unwrap();
    for (key, tag) in [("disposition", "applied"), ("state", "paid")] {
        let mut changed = json!({"paymentId":id(),"disposition":"applied","state":"paid"});
        changed[key] = json!({tag:null});
        assert!(original.decode_response(&bytes(changed)).is_err());
    }
    let original = ProcessNotification { event_id: id() };
    let changed = json!({"eventId":original.event_id,"disposition":{"ignored_unlinked":null},"associationId":null,"refreshSourceId":null});
    assert!(original.decode_response(&bytes(changed)).is_err());
}
#[test]
fn source_reference_scalar_bounds_and_time_order_are_explicit() {
    for (key, max) in [
        ("occurrenceRef", 256),
        ("memberRef", 256),
        ("paymentRef", 256),
        ("tierRef", 256),
    ] {
        let mut changed = proof();
        changed[key] = json!(format!("{}{}", " ".repeat(1000), "🂡".repeat(max)));
        assert!(observation(changed).is_ok());
        let mut changed = proof();
        changed[key] = json!("🂡".repeat(max + 1));
        assert!(observation(changed).is_err());
        let mut changed = proof();
        changed[key] = json!("private\0ref");
        assert!(observation(changed).is_err());
    }
    let delivery = format!("{}{}", " ".repeat(1000), "🂡".repeat(200));
    assert!(FinancialObservation::from_bytes(&delivery, &bytes(proof())).is_ok());
    assert!(FinancialObservation::from_bytes(&"🂡".repeat(201), &bytes(proof())).is_err());
    assert!(FinancialObservation::from_bytes("x\0y", &bytes(proof())).is_err());
    let mut changed = proof();
    changed["state"] = json!("refunded");
    changed["paidAt"] = json!("2026-10-02T00:00:00.000000Z");
    assert!(observation(changed).is_err());
}
