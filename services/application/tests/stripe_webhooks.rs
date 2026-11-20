//! Synthetic delivery/source tests. No live Stripe account or customer is used.
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use spellwake_application::stripe::{
    MAX_SIGNATURE_HEADER, MAX_WEBHOOK_BODY, StripeError, StripeEventKind, StripeWebhookVerifier,
};
use std::{collections::BTreeMap, error::Error};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fixture {
    scope: String,
    sdk_version: String,
    sdk_git_head: String,
    sdk_integrity: String,
    sdk_files: BTreeMap<String, String>,
    generator_sha256: String,
    timestamp: i64,
    secret: String,
    api_version: String,
    vectors: Vec<Vector>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Vector {
    name: String,
    body: String,
    header: String,
    body_sha256: String,
    sdk_accepted: bool,
}
fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/stripe-webhooks.json")).unwrap()
}
fn verifier(f: &Fixture, live: bool) -> StripeWebhookVerifier {
    StripeWebhookVerifier::new(f.secret.clone(), f.api_version.clone(), live).unwrap()
}
fn signed(f: &Fixture, body: &[u8], timestamp: i64) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(f.secret.as_bytes()).unwrap();
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let signature: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("t={timestamp},v1={signature}")
}
fn rejected(f: &Fixture, body: &[u8]) -> StripeError {
    match verifier(f, false).verify(body, &signed(f, body, f.timestamp), f.timestamp) {
        Err(error) => error,
        Ok(_) => panic!("invalid synthetic notification was accepted"),
    }
}

#[test]
fn retained_official_sdk_vectors_verify_exact_unchanged_bytes() {
    let f = fixture();
    assert_eq!(
        f.scope,
        "synthetic-official-sdk-delivery-vectors-not-live-provider-evidence"
    );
    assert_eq!(f.sdk_version, "23.1.0");
    assert_eq!(f.sdk_git_head, "6d5ed967d602224ac3056dc3f0befbb55487ee50");
    assert_eq!(
        f.sdk_integrity,
        "sha512-fVUfMSfTPoukfoV//MUKAx2+YxyKmYaYtnAL9VkfnHfaPNQylFdWZMD+zgUFerI8rjbaiER8Vzj2XEUQ2AhAsQ=="
    );
    assert_eq!(f.sdk_files.len(), 2);
    assert_eq!(f.generator_sha256.len(), 64);
    assert_eq!(f.vectors.len(), 4);
    for vector in &f.vectors {
        assert!(vector.sdk_accepted, "{}", vector.name);
        let raw: Value = serde_json::from_str(&vector.body).unwrap();
        let v = verifier(&f, raw["livemode"].as_bool().unwrap());
        let event = v
            .verify(vector.body.as_bytes(), &vector.header, f.timestamp)
            .unwrap();
        assert_eq!(event.event_id(), raw["id"].as_str().unwrap());
        assert_eq!(event.event_type(), raw["type"].as_str().unwrap());
        assert_eq!(event.created().timestamp(), 1); // Old event, fresh delivery.
        assert_eq!(event.api_version(), f.api_version);
        assert_eq!(event.is_live(), raw["livemode"].as_bool().unwrap());
        assert_eq!(event.body_sha256(), vector.body_sha256);
        let mut changed = vector.body.as_bytes().to_vec();
        changed.push(b' ');
        assert!(matches!(
            v.verify(&changed, &vector.header, f.timestamp),
            Err(StripeError::Signature)
        ));
    }
    assert!(f.vectors[1].body.contains("café / 雪"));
    assert!(f.vectors[1].body.contains("\r\n"));
    assert_eq!(
        verifier(&f, false)
            .verify(
                f.vectors[3].body.as_bytes(),
                &f.vectors[3].header,
                f.timestamp
            )
            .unwrap()
            .signal(),
        None
    );
}

#[test]
fn delivery_tolerance_is_symmetric_bounded_and_distinct_from_event_creation() {
    let f = fixture();
    let v = verifier(&f, false);
    let body = f.vectors[0].body.as_bytes();
    for offset in [-300, 0, 300] {
        assert!(
            v.verify(body, &signed(&f, body, f.timestamp + offset), f.timestamp)
                .is_ok()
        );
    }
    for offset in [-301, 301] {
        assert!(matches!(
            v.verify(body, &signed(&f, body, f.timestamp + offset), f.timestamp),
            Err(StripeError::Signature)
        ));
    }
    for now in [-1, i64::MIN, i64::MAX] {
        assert!(matches!(
            v.verify(body, &f.vectors[0].header, now),
            Err(StripeError::Clock)
        ));
    }
    for timestamp in [i64::MIN, i64::MAX] {
        assert!(matches!(
            v.verify(body, &signed(&f, body, timestamp), f.timestamp),
            Err(StripeError::Signature)
        ));
    }
}

#[test]
fn malformed_headers_wrong_secrets_and_non_v1_signatures_never_authenticate() {
    let f = fixture();
    let v = verifier(&f, false);
    let body = f.vectors[0].body.as_bytes();
    let zero = "0".repeat(64);
    for header in [
        format!("v1={zero}"),
        format!("t={},v0={zero}", f.timestamp),
        format!("t={0},t={0},v1={zero}", f.timestamp),
        format!("t=0{},v1={zero}", f.timestamp),
        format!("t=+{},v1={zero}", f.timestamp),
        format!("t=18446744073709551615,v1={zero}"),
        format!("t=1.0,v1={zero}"),
        format!("t={},v1={}", f.timestamp, "g".repeat(64)),
        format!("t={},v1=aa", f.timestamp),
        format!("t={},v1={zero},", f.timestamp),
        format!("t={},v1={zero}\r\n", f.timestamp),
        format!("t={},v1={zero},snow=雪", f.timestamp),
        format!("t={},broken,v1={zero}", f.timestamp),
        format!("t={},v1={zero},x=a=b", f.timestamp),
        format!("t={},v1={zero},x=has space", f.timestamp),
    ] {
        assert!(matches!(
            v.verify(body, &header, f.timestamp),
            Err(StripeError::Signature)
        ));
    }
    let other = StripeWebhookVerifier::new(
        "whsec_other_synthetic_secret_only".into(),
        f.api_version.clone(),
        false,
    )
    .unwrap();
    assert!(matches!(
        other.verify(body, &f.vectors[0].header, f.timestamp),
        Err(StripeError::Signature)
    ));
    let malformed = b"not JSON, but no parsing before authentication";
    assert!(matches!(
        v.verify(
            malformed,
            &format!("t={},v1={zero}", f.timestamp),
            f.timestamp
        ),
        Err(StripeError::Signature)
    ));
    assert!(matches!(
        v.verify(malformed, &signed(&f, malformed, f.timestamp), f.timestamp),
        Err(StripeError::Envelope(_))
    ));
}

#[test]
fn multiple_v1_candidates_and_component_limits_are_explicit() {
    let f = fixture();
    let v = verifier(&f, false);
    let body = f.vectors[0].body.as_bytes();
    let zero = "0".repeat(64);
    let good = f.vectors[0].header.split(",v1=").nth(1).unwrap();
    assert!(
        v.verify(
            body,
            &format!("t={}, v0=old,\tv1={zero}, v1={good}", f.timestamp),
            f.timestamp
        )
        .is_ok()
    );
    let eight = format!(
        "t={},{}v1={good}",
        f.timestamp,
        format!("v1={zero},").repeat(7)
    );
    assert!(v.verify(body, &eight, f.timestamp).is_ok());
    assert!(matches!(
        v.verify(body, &format!("{eight},v1={good}"), f.timestamp),
        Err(StripeError::Signature)
    ));
    let thirty_two = format!(
        "{},{}",
        f.vectors[0].header,
        "x=ignored,".repeat(30).trim_end_matches(',')
    );
    assert!(v.verify(body, &thirty_two, f.timestamp).is_ok());
    assert!(matches!(
        v.verify(body, &format!("{thirty_two},x=ignored"), f.timestamp),
        Err(StripeError::Signature)
    ));
    let component = format!(",{}={}", "x".repeat(32), "a".repeat(256));
    let mut exact = format!("{}{}", f.vectors[0].header, component.repeat(27));
    let last_value = MAX_SIGNATURE_HEADER - exact.len() - 34;
    assert!((1..=256).contains(&last_value));
    exact.push_str(&format!(",{}={}", "x".repeat(32), "a".repeat(last_value)));
    assert_eq!(exact.len(), MAX_SIGNATURE_HEADER);
    assert!(v.verify(body, &exact, f.timestamp).is_ok());
}

#[test]
fn intake_and_startup_bounds_are_enforced_before_parsing() {
    let f = fixture();
    let v = verifier(&f, false);
    for secret in [
        "".into(),
        "sk_test_wrong_family".into(),
        "whsec_short".into(),
        "whsec_unsafe\nvalue".into(),
        "whsec_éprivate".into(),
        "a".repeat(513),
    ] {
        assert!(matches!(
            StripeWebhookVerifier::new(secret, f.api_version.clone(), false),
            Err(StripeError::Configuration)
        ));
    }
    for version in [
        "".into(),
        "x".repeat(65),
        "version with space".into(),
        "version?token=private".into(),
    ] {
        assert!(matches!(
            StripeWebhookVerifier::new(f.secret.clone(), version, false),
            Err(StripeError::Configuration)
        ));
    }
    let body = f.vectors[0].body.as_bytes();
    for (input, header) in [
        (Vec::new(), f.vectors[0].header.clone()),
        (
            vec![b' '; MAX_WEBHOOK_BODY + 1],
            f.vectors[0].header.clone(),
        ),
        (body.to_vec(), String::new()),
        (body.to_vec(), "x".repeat(MAX_SIGNATURE_HEADER + 1)),
    ] {
        assert!(matches!(
            v.verify(&input, &header, f.timestamp),
            Err(StripeError::Intake)
        ));
    }
    let mut big: Value = serde_json::from_slice(body).unwrap();
    big["padding"] = json!("");
    let empty = serde_json::to_vec(&big).unwrap();
    big["padding"] = json!("a".repeat(MAX_WEBHOOK_BODY - empty.len()));
    let exact = serde_json::to_vec(&big).unwrap();
    assert_eq!(exact.len(), MAX_WEBHOOK_BODY);
    assert!(
        v.verify(&exact, &signed(&f, &exact, f.timestamp), f.timestamp)
            .is_ok()
    );
}

#[test]
fn supported_signals_require_exact_object_classes_and_ids_without_payment_decisions() {
    let f = fixture();
    let v = verifier(&f, false);
    let base: Value = serde_json::from_str(&f.vectors[0].body).unwrap();
    for (event_type, object, id, expected) in [
        (
            "checkout.session.completed",
            "checkout.session",
            "cs_synthetic",
            StripeEventKind::CheckoutCompleted,
        ),
        (
            "checkout.session.async_payment_succeeded",
            "checkout.session",
            "cs_synthetic",
            StripeEventKind::CheckoutAsyncSucceeded,
        ),
        (
            "checkout.session.async_payment_failed",
            "checkout.session",
            "cs_synthetic",
            StripeEventKind::CheckoutAsyncFailed,
        ),
        (
            "checkout.session.expired",
            "checkout.session",
            "cs_synthetic",
            StripeEventKind::CheckoutExpired,
        ),
        (
            "charge.succeeded",
            "charge",
            "ch_synthetic",
            StripeEventKind::ChargeSucceeded,
        ),
        (
            "charge.refunded",
            "charge",
            "ch_synthetic",
            StripeEventKind::ChargeRefunded,
        ),
        (
            "charge.updated",
            "charge",
            "ch_synthetic",
            StripeEventKind::ChargeUpdated,
        ),
        (
            "refund.created",
            "refund",
            "re_synthetic",
            StripeEventKind::RefundCreated,
        ),
        (
            "refund.updated",
            "refund",
            "re_synthetic",
            StripeEventKind::RefundUpdated,
        ),
        (
            "refund.failed",
            "refund",
            "re_synthetic",
            StripeEventKind::RefundFailed,
        ),
        (
            "charge.dispute.created",
            "dispute",
            "du_synthetic",
            StripeEventKind::DisputeCreated,
        ),
        (
            "charge.dispute.updated",
            "dispute",
            "du_synthetic",
            StripeEventKind::DisputeUpdated,
        ),
        (
            "charge.dispute.closed",
            "dispute",
            "du_synthetic",
            StripeEventKind::DisputeClosed,
        ),
        (
            "charge.dispute.funds_withdrawn",
            "dispute",
            "du_synthetic",
            StripeEventKind::DisputeFundsWithdrawn,
        ),
        (
            "charge.dispute.funds_reinstated",
            "dispute",
            "du_synthetic",
            StripeEventKind::DisputeFundsReinstated,
        ),
    ] {
        let mut event = base.clone();
        event["type"] = json!(event_type);
        event["data"]["object"] = json!({"id":id,"object":object,"payment_status":"paid","metadata":{"accountId":"untrusted"}});
        let body = serde_json::to_vec(&event).unwrap();
        assert_eq!(
            v.verify(&body, &signed(&f, &body, f.timestamp), f.timestamp)
                .unwrap()
                .signal(),
            Some((expected, id))
        );
        for target in [
            json!({"id":id,"object":"wrong"}),
            json!({"id":"pi_wrongFamily","object":object}),
            json!({"id":null,"object":object}),
            json!({"id":"cs_","object":object}),
        ] {
            event["data"]["object"] = target;
            let body = serde_json::to_vec(&event).unwrap();
            assert!(matches!(rejected(&f, &body), StripeError::Contract));
        }
    }
}

#[test]
fn exact_version_mode_ids_and_creation_shape_are_checked_after_signature() {
    let f = fixture();
    let base: Value = serde_json::from_str(&f.vectors[0].body).unwrap();
    for (field, bad) in [
        ("api_version", json!("2026-09-30.clover")),
        ("livemode", json!(true)),
        ("object", json!("v2.core.event")),
        ("id", json!("pi_wrong")),
        ("id", json!("evt_")),
        ("id", json!("evt_unsafe/path")),
        ("id", json!("evt_".to_owned() + &"a".repeat(252))),
        ("type", json!("charge.SUCCEEDED")),
        ("created", json!(-1)),
        ("created", json!(i64::MAX)),
        ("created", json!(1.5)),
    ] {
        let mut bad_event = base.clone();
        bad_event[field] = bad;
        assert!(matches!(
            rejected(&f, &serde_json::to_vec(&bad_event).unwrap()),
            StripeError::Contract | StripeError::Envelope(_)
        ));
    }
}

#[test]
fn documented_dispute_object_routes_with_current_du_identifier() {
    let f = fixture();
    let mut event: Value = serde_json::from_str(&f.vectors[0].body).unwrap();
    // Current official dispute object/retrieve example, not live customer data:
    // https://docs.stripe.com/api/disputes/object
    event["type"] = json!("charge.dispute.funds_reinstated");
    event["data"]["object"] = json!({"id":"du_1MtJUT2eZvKYlo2CNaw2HvEv","object":"dispute"});
    let body = serde_json::to_vec(&event).unwrap();
    assert_eq!(
        verifier(&f, false)
            .verify(&body, &signed(&f, &body, f.timestamp), f.timestamp)
            .unwrap()
            .signal(),
        Some((
            StripeEventKind::DisputeFundsReinstated,
            "du_1MtJUT2eZvKYlo2CNaw2HvEv"
        ))
    );
    event["data"]["object"]["id"] = json!("dp_previouslyGuessedSyntheticPrefix");
    assert!(matches!(
        rejected(&f, &serde_json::to_vec(&event).unwrap()),
        StripeError::Contract
    ));
}

fn assert_private_errors(error: &StripeError) {
    let forbidden = ["PRIVATE_STRIPE_CANARY_56891", "9456789123456789"];
    let mut source: Option<&dyn Error> = Some(error);
    for _ in 0..8 {
        let Some(current) = source else {
            return;
        };
        for formatted in [current.to_string(), format!("{current:?}")] {
            for canary in forbidden {
                assert!(
                    !formatted.contains(canary),
                    "private synthetic canary leaked"
                );
            }
        }
        source = current.source();
    }
    panic!("unexpected error source cycle");
}

#[test]
fn all_typed_and_object_shape_errors_preserve_safe_sources_without_private_values() {
    let f = fixture();
    let base: Value = serde_json::from_str(&f.vectors[0].body).unwrap();
    let shapes = [
        json!("PRIVATE_STRIPE_CANARY_56891"),
        json!(9456789123456789_i64),
        json!(false),
        json!(1.25),
        Value::Null,
        json!(["PRIVATE_STRIPE_CANARY_56891"]),
    ];
    for shape in &shapes {
        for path in ["", "/data", "/data/object"] {
            let mut body = base.clone();
            *body.pointer_mut(path).unwrap() = shape.clone();
            let error = rejected(&f, &serde_json::to_vec(&body).unwrap());
            assert_private_errors(&error);
            assert!(matches!(error, StripeError::Envelope(_)));
            assert!(error.source().is_some());
        }
    }
    for path in [
        "/id",
        "/object",
        "/type",
        "/created",
        "/livemode",
        "/api_version",
        "/data/object/id",
        "/data/object/object",
    ] {
        for bad in [
            json!({"private":"PRIVATE_STRIPE_CANARY_56891"}),
            json!([9456789123456789_i64]),
            json!(9456789123456789_i64),
            json!("PRIVATE_STRIPE_CANARY_56891"),
        ] {
            let mut body = base.clone();
            *body.pointer_mut(path).unwrap() = bad;
            assert_private_errors(&rejected(&f, &serde_json::to_vec(&body).unwrap()));
        }
    }
    for raw in [
        "{\"id\":\"PRIVATE_STRIPE_CANARY_56891\",broken}",
        "{\"created\":9456789123456789,broken}",
        "{\"id\":\"PRIVATE_STRIPE_CANARY_56891\\q\"}",
    ] {
        let error = rejected(&f, raw.as_bytes());
        assert_private_errors(&error);
        assert!(error.source().is_some());
    }
}

#[test]
fn duplicate_selected_keys_reject_but_additive_private_vendor_fields_are_ignored() {
    let f = fixture();
    let base = &f.vectors[0].body;
    for (needle, replacement) in [
        (
            "\"id\":\"evt_spellwakeSynthetic1\"",
            "\"id\":\"PRIVATE_STRIPE_CANARY_56891\",\"id\":\"evt_spellwakeSynthetic1\"",
        ),
        (
            "\"livemode\":false",
            "\"livemode\":false,\"livemode\":false",
        ),
        ("\"data\":{", "\"data\":{},\"data\":{"),
        (
            "\"object\":{\"id\":\"cs_test_spellwakeSynthetic\"",
            "\"object\":{},\"object\":{\"id\":\"cs_test_spellwakeSynthetic\"",
        ),
        (
            "\"id\":\"cs_test_spellwakeSynthetic\"",
            "\"id\":\"PRIVATE_STRIPE_CANARY_56891\",\"id\":\"cs_test_spellwakeSynthetic\"",
        ),
        (
            "\"object\":\"checkout.session\"",
            "\"object\":\"charge\",\"object\":\"checkout.session\"",
        ),
    ] {
        assert!(base.contains(needle));
        let raw = base.replace(needle, replacement);
        let error = rejected(&f, raw.as_bytes());
        assert!(matches!(error, StripeError::Envelope(_)));
        assert_private_errors(&error);
    }
    let mut event: Value = serde_json::from_str(base).unwrap();
    event["new_vendor_field"] =
        json!({"private":"PRIVATE_STRIPE_CANARY_56891","numeric":9456789123456789_i64});
    event["data"]["new_vendor_field"] = event["new_vendor_field"].clone();
    event["data"]["object"]["billing_details"] = event["new_vendor_field"].clone();
    let raw = serde_json::to_vec(&event).unwrap();
    let notification = verifier(&f, false)
        .verify(&raw, &signed(&f, &raw, f.timestamp), f.timestamp)
        .unwrap();
    assert_eq!(
        notification.signal(),
        Some((
            StripeEventKind::CheckoutCompleted,
            "cs_test_spellwakeSynthetic"
        ))
    );
}
