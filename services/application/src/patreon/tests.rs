use super::test_fixture::{member_path, source as fixture};
use super::*;
use crate::test_http::{Mock, Step};
use client::{PatreonClient, PatreonConfig, UserAccessToken};
use serde_json::{Value, json};
fn signature(body: &[u8]) -> String {
    let mut mac =
        Hmac::<Md5>::new_from_slice(fixture()["secret"].as_str().unwrap().as_bytes()).unwrap();
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn verifier() -> PatreonWebhookVerifier {
    PatreonWebhookVerifier::new(fixture()["secret"].as_str().unwrap().into(), "123".into()).unwrap()
}
#[test]
fn exact_raw_bytes_match_independent_hmac_and_replays_have_no_invented_freshness() {
    let f = fixture();
    let body = f["webhookBody"].as_str().unwrap().as_bytes();
    let signal = verifier()
        .verify(body, f["signature"].as_str().unwrap())
        .unwrap();
    assert_eq!(signal.member_id(), "member-abc");
    assert_eq!(signal.user_id(), "456");
    assert_eq!(signal.campaign_id(), "123");
    assert_eq!(signal.body_sha256(), f["bodySha256"].as_str().unwrap());
    assert_eq!(
        verifier()
            .verify(body, &f["signature"].as_str().unwrap().to_uppercase())
            .unwrap()
            .body_sha256(),
        signal.body_sha256()
    );
    let mut changed = body.to_vec();
    changed.push(b' ');
    assert!(matches!(
        verifier().verify(&changed, f["signature"].as_str().unwrap()),
        Err(PatreonError::Signature)
    ));
    // RFC2202 HMAC-MD5 test case 1: correct MAC reaches JSON validation, not signature rejection.
    let rfc = PatreonWebhookVerifier::new("x".repeat(16), "123".into()).unwrap();
    let rfc = PatreonWebhookVerifier {
        secret: String::from_utf8(vec![0x0b; 16]).unwrap(),
        ..rfc
    };
    assert!(matches!(
        rfc.verify(b"Hi There", "9294727a3638bb1c13f48ef8158bfc9d"),
        Err(PatreonError::Json(_))
    ));
}
#[test]
fn notification_intake_rejects_wrong_campaign_relationships_and_unsafe_shapes() {
    let f = fixture();
    let body = f["webhookBody"].as_str().unwrap().as_bytes();
    for bad in [
        "",
        "0",
        "0d".repeat(17).as_str(),
        "gggggggggggggggggggggggggggggggg",
        "éééééééééééééééé",
    ] {
        assert!(matches!(
            verifier().verify(body, bad),
            Err(PatreonError::Signature)
        ));
    }
    for (pointer, value) in [
        ("/data/type", json!("post")),
        ("/data/id", json!("../private")),
        ("/data/relationships/campaign/data/id", json!("other")),
        ("/data/relationships/user/data/type", json!("campaign")),
        ("/data/relationships/user/data/id", json!("")),
        ("/data/relationships/user/data", Value::Null),
    ] {
        let mut changed: Value = serde_json::from_slice(body).unwrap();
        *changed.pointer_mut(pointer).unwrap() = value;
        let bytes = serde_json::to_vec(&changed).unwrap();
        assert!(verifier().verify(&bytes, &signature(&bytes)).is_err());
    }
    assert!(matches!(
        verifier().verify(&vec![b'x'; MAX_WEBHOOK_BODY + 1], ""),
        Err(PatreonError::Intake)
    ));
    assert!(PatreonWebhookVerifier::new("short".into(), "123".into()).is_err());
}
fn page(value: &Value) -> Value {
    json!({"data":[value["data"]],"included":value["included"],"meta":{"pagination":{"cursors":{"next":null}}}})
}
fn parse(value: &Value) -> Result<client::MemberObservation, PatreonError> {
    let mut page = source::parse_page(&serde_json::to_vec(&page(value)).unwrap(), "123", None)?;
    let selected = page.members.remove(0);
    if selected.current.user_id != "456" || selected.current.member_id != "member-abc" {
        return Err(PatreonError::Contract);
    }
    Ok(selected)
}
#[test]
fn tier_entitlement_and_payment_observations_remain_separate_and_exact() {
    let mut value = fixture()["member"].clone();
    let observation = parse(&value).unwrap();
    assert_eq!(observation.current.entitled_tier_ids, vec!["789"]);
    assert_eq!(observation.pledge_history.len(), 1);
    let payment = &observation.pledge_history[0];
    assert_eq!(payment.event_id, "event-1");
    assert_eq!(payment.tier_id, "789");
    assert_eq!(payment.amount_cents, 500);
    assert_eq!(payment.date.timestamp_subsec_nanos(), 123456000);
    assert_eq!(payment.payment_status, client::PaymentStatus::Paid);
    assert_eq!(payment.kind, client::PledgeKind::Subscription);
    value["data"]["relationships"]["pledge_history"]["data"] = json!([]);
    value["included"] = json!([]);
    assert!(parse(&value).unwrap().pledge_history.is_empty()); // Current entitlement never invents a payment.
    value = fixture()["member"].clone();
    for (raw, expected) in [
        ("Declined", client::PaymentStatus::Declined),
        ("Pending", client::PaymentStatus::Pending),
        ("Refunded", client::PaymentStatus::Refunded),
        (
            "Refunded by Patreon",
            client::PaymentStatus::RefundedByPatreon,
        ),
        ("Fraud", client::PaymentStatus::Fraud),
        (
            "Partially Refunded",
            client::PaymentStatus::PartiallyRefunded,
        ),
        ("Free Trial", client::PaymentStatus::FreeTrial),
        ("Refund Pending", client::PaymentStatus::RefundPending),
        ("Refund Declined", client::PaymentStatus::RefundDeclined),
    ] {
        value["included"][0]["attributes"]["payment_status"] = json!(raw);
        assert_eq!(
            parse(&value).unwrap().pledge_history[0].payment_status,
            expected
        );
    }
}
#[test]
fn observations_reject_foreign_lineage_ambiguous_references_and_private_scalar_errors() {
    for (pointer, value) in [
        ("/data/relationships/user/data/id", json!("other")),
        ("/data/relationships/campaign/data/id", json!("other")),
        ("/included/0/relationships/patron/data/id", json!("other")),
        ("/included/0/relationships/campaign/data/id", json!("other")),
        ("/included/0/relationships/tier/data/id", json!("other")),
        ("/included/0/attributes/tier_id", json!("other")),
        ("/included/0/attributes/amount_cents", json!(500.5)),
        ("/included/0/attributes/currency_code", json!("usd")),
        (
            "/included/0/attributes/date",
            json!("2026-11-02T12:00:00.123456789Z"),
        ),
        (
            "/included/0/attributes/payment_status",
            json!("private-secret-sentinel"),
        ),
        (
            "/data/attributes/last_charge_status",
            json!(["private-secret-sentinel"]),
        ),
        ("/data/relationships/pledge_history/data", json!([])),
    ] {
        let mut changed = fixture()["member"].clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        // Empty history is an explicit lack of selected history, not an error or a payment.
        if pointer.ends_with("pledge_history/data") {
            assert!(parse(&changed).unwrap().pledge_history.is_empty());
            continue;
        }
        let error = parse(&changed).err().unwrap();
        assert!(!format!("{error:?}").contains("private-secret-sentinel"));
        if let Some(source) = error.source() {
            assert!(!source.to_string().contains("private-secret-sentinel"));
        }
    }
    let mut duplicate = fixture()["member"].clone();
    let item = duplicate["included"][0].clone();
    duplicate["included"].as_array_mut().unwrap().push(item);
    assert!(parse(&duplicate).is_err());
    let body = serde_json::to_string(&page(&fixture()["member"])).unwrap();
    let duplicate_scalar = body.replace(
        "\"amount_cents\":500",
        "\"amount_cents\":500,\"amount_cents\":999",
    );
    assert_ne!(body, duplicate_scalar);
    assert!(source::parse_page(duplicate_scalar.as_bytes(), "123", None).is_err());
    let mut absent = fixture()["member"].clone();
    absent["included"] = json!([]);
    assert!(parse(&absent).is_err());
    let mut missing = fixture()["member"].clone();
    missing["data"]["relationships"]
        .as_object_mut()
        .unwrap()
        .remove("pledge_history");
    assert!(parse(&missing).is_err());
}

#[test]
fn identity_and_member_requests_use_distinct_tokens_without_backend_headers_or_redirects() {
    let f = fixture();
    let identity =
        json!({"data":{"type":"user","id":"456","attributes":{"email":"ignored@example.invalid"}}});
    let mut steps = vec![
        Step::get(
            "/api/oauth2/v2/identity".into(),
            200,
            serde_json::to_vec(&identity).unwrap(),
        ),
        Step::get(
            member_path(),
            200,
            serde_json::to_vec(&f["member"]).unwrap(),
        ),
    ];
    steps[0].credentials =
        crate::test_http::Credentials::Bearer("patreon_test_user_synthetic".into());
    steps[1].credentials =
        crate::test_http::Credentials::Bearer("patreon_test_creator_synthetic".into());
    let mock = Mock::new(steps);
    let client = PatreonClient::new(PatreonConfig::fixture(&mock.base)).unwrap();
    let user = UserAccessToken::new("patreon_test_user_synthetic".into()).unwrap();
    assert_eq!(client.identity(&user).unwrap().user_id(), "456");
    assert_eq!(
        client.member("member-abc", "456").unwrap().member_id,
        "member-abc"
    );
    let calls = mock.finish();
    for (index, expected) in [
        (0, "Bearer patreon_test_user_synthetic"),
        (1, "Bearer patreon_test_creator_synthetic"),
    ] {
        assert_eq!(calls[index].1.get("authorization").unwrap(), expected);
        assert!(!calls[index].1.contains_key("apikey"));
        assert!(!calls[index].1.contains_key("stripe-version"));
        assert_eq!(
            calls[index].1.get("user-agent").unwrap(),
            "Spellwake - Membership Sync"
        );
        assert!(calls[index].2.is_empty());
    }
    let mut redirect = Step::get("/api/oauth2/v2/identity".into(), 302, Vec::new());
    redirect
        .response_headers
        .push(("Location".into(), "https://example.invalid/steal".into()));
    redirect.credentials =
        crate::test_http::Credentials::Bearer("patreon_test_user_synthetic".into());
    let mock = Mock::new(vec![redirect]);
    let client = PatreonClient::new(PatreonConfig::fixture(&mock.base)).unwrap();
    assert!(matches!(
        client.identity(&user),
        Err(PatreonError::Status(302))
    ));
    mock.finish();
}

#[test]
fn source_reads_enforce_response_bounds_status_and_safe_network_causes() {
    let user = UserAccessToken::new("patreon_test_user_synthetic".into()).unwrap();
    for status in [401, 404, 429, 500] {
        let mut step = Step::get(
            "/api/oauth2/v2/identity".into(),
            status,
            b"private-error-body".to_vec(),
        );
        step.credentials =
            crate::test_http::Credentials::Bearer("patreon_test_user_synthetic".into());
        let mock = Mock::new(vec![step]);
        let client = PatreonClient::new(PatreonConfig::fixture(&mock.base)).unwrap();
        assert!(
            matches!(client.identity(&user), Err(PatreonError::Status(code)) if code == status)
        );
        assert_eq!(mock.finish().len(), 1);
    }
    let mut step = Step::get(
        "/api/oauth2/v2/identity".into(),
        200,
        vec![b' '; 1024 * 1024 + 1],
    );
    step.credentials = crate::test_http::Credentials::Bearer("patreon_test_user_synthetic".into());
    let mock = Mock::new(vec![step]);
    let client = PatreonClient::new(PatreonConfig::fixture(&mock.base)).unwrap();
    assert!(matches!(client.identity(&user), Err(PatreonError::Body(_))));
    mock.finish();
    let mut step = Step::get("/api/oauth2/v2/identity".into(), 200, Vec::new());
    step.credentials = crate::test_http::Credentials::Bearer("patreon_test_user_synthetic".into());
    step.drop_ack = true;
    let mock = Mock::new(vec![step]);
    let client = PatreonClient::new(PatreonConfig::fixture(&mock.base)).unwrap();
    let error = client.identity(&user).err().unwrap();
    let PatreonError::Network(ref network) = error else {
        panic!("expected unknown network acknowledgment")
    };
    assert!(network.url().is_none());
    assert!(error.source().is_some());
    assert!(!error.to_string().contains("127.0.0.1"));
    mock.finish();
    let mut oversized = fixture()["member"].clone();
    oversized["data"]["relationships"]["pledge_history"]["data"] = Value::Array(
        (0..1001)
            .map(|i| json!({"type":"pledge-event","id":format!("event-{i}")}))
            .collect(),
    );
    assert!(parse(&oversized).is_err());
}

#[test]
fn current_membership_is_independent_from_unrequested_history_and_campaign_pages_are_explicit() {
    let mut current = fixture()["member"].clone();
    current["data"]["relationships"]
        .as_object_mut()
        .unwrap()
        .remove("pledge_history");
    current.as_object_mut().unwrap().remove("included");
    let selected = source::parse_member(
        &serde_json::to_vec(&current).unwrap(),
        "member-abc",
        "456",
        "123",
    )
    .unwrap();
    assert_eq!(selected.entitled_tier_ids, vec!["789"]);
    assert!(
        source::parse_member(
            &serde_json::to_vec(&current).unwrap(),
            "member-abc",
            "other",
            "123"
        )
        .is_err()
    );
    let mut payload = page(&fixture()["member"]);
    payload["meta"]["pagination"]["cursors"]["next"] = json!("opaque/+=?cursor");
    let after = "previous/+=?cursor";
    let mut expected =
        reqwest::Url::parse("http://127.0.0.1/api/oauth2/v2/campaigns/123/members").unwrap();
    expected.query_pairs_mut().append_pair("include","campaign,user,currently_entitled_tiers,pledge_history")
        .append_pair("fields[member]","patron_status,last_charge_status,last_charge_date,campaign,user,currently_entitled_tiers,pledge_history")
        .append_pair("fields[pledge-event]","amount_cents,currency_code,date,payment_status,pledge_payment_status,tier_id,type,campaign,patron,tier")
        .append_pair("page[count]","25").append_pair("page[cursor]",after);
    let mut step = Step::get(
        format!("{}?{}", expected.path(), expected.query().unwrap()),
        200,
        serde_json::to_vec(&payload).unwrap(),
    );
    step.credentials =
        crate::test_http::Credentials::Bearer("patreon_test_creator_synthetic".into());
    let mock = Mock::new(vec![step]);
    let client = PatreonClient::new(PatreonConfig::fixture(&mock.base)).unwrap();
    let result = client.campaign_members(Some(after)).unwrap();
    assert_eq!(result.next_cursor.as_deref(), Some("opaque/+=?cursor"));
    assert_eq!(result.members[0].current.member_id, "member-abc");
    assert_eq!(result.members[0].pledge_history[0].event_id, "event-1");
    mock.finish();
    assert!(
        source::parse_page(
            &serde_json::to_vec(&payload).unwrap(),
            "123",
            Some("opaque/+=?cursor")
        )
        .is_err()
    );
    payload["meta"]["pagination"]["cursors"]["next"] = Value::Null;
    payload["links"] = json!({"next":"https://example.invalid/do-not-follow"});
    assert!(source::parse_page(&serde_json::to_vec(&payload).unwrap(), "123", None).is_err());
    payload.as_object_mut().unwrap().remove("links");
    let data = payload["data"][0].clone();
    payload["data"].as_array_mut().unwrap().push(data.clone());
    assert!(source::parse_page(&serde_json::to_vec(&payload).unwrap(), "123", None).is_err());
    let empty = json!({"data":[],"meta":{"pagination":{"cursors":{}}}});
    assert!(
        source::parse_page(&serde_json::to_vec(&empty).unwrap(), "123", None)
            .unwrap()
            .members
            .is_empty()
    );
    let mut null = empty;
    null["included"] = Value::Null;
    assert!(source::parse_page(&serde_json::to_vec(&null).unwrap(), "123", None).is_err());
    payload["data"] = Value::Array(vec![data; 26]);
    assert!(source::parse_page(&serde_json::to_vec(&payload).unwrap(), "123", None).is_err());
}
