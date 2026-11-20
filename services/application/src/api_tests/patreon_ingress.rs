//! Controlled signed HTTP ingress; no real Patreon delivery or managed source claim.
use super::*;
use crate::patreon::{intake::IntakeConfig, test_fixture};
fn intake_config(base: &str) -> ApiConfig {
    let mut cfg = config(base);
    cfg.patreon_intake = Some(Arc::new(
        IntakeConfig::new(
            "123".into(),
            test_fixture::source()["secret"].as_str().unwrap().into(),
            "patreon_private".into(),
        )
        .unwrap(),
    ));
    cfg
}
fn captured() -> Step {
    let mut step = Step::json("/rest/v1/rpc/capture_patreon_notification", Value::Null);
    step.reply = Some(Box::new(|bytes| {
        let input: Value = serde_json::from_slice(bytes).unwrap();
        let id: Uuid = serde_json::from_value(input["p_delivery"].clone()).unwrap();
        assert!(!id.is_nil());
        let hash = input["p_capture"]["bodyHash"].as_str().unwrap();
        serde_json::to_vec(&json!({"id":id,"evidenceState":"retained","payloadRef":format!("payments/patreon/{id}/{hash}.json"),"expiresAt":"2099-01-01T00:00:00Z","processed":false})).unwrap()
    }));
    step
}
async fn deliver(
    app: &Router,
    path: &str,
    bytes: Vec<u8>,
    signatures: &[&str],
    event: &str,
) -> Response {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("x-patreon-event", event);
    for signature in signatures {
        request = request.header("x-patreon-signature", *signature);
    }
    app.clone()
        .oneshot(request.body(Body::from(bytes)).unwrap())
        .await
        .unwrap()
}
#[tokio::test]
async fn patreon_signed_ingress_uses_application_occurrences_and_ignores_unsigned_event_authority()
{
    let data = test_fixture::source();
    let bytes = data["webhookBody"].as_str().unwrap().as_bytes().to_vec();
    let sig = data["signature"].as_str().unwrap();
    let backend = Mock::new(vec![captured(), captured()]);
    let app = router(intake_config(&backend.base));
    for event in ["members:pledge:create", "invented:payment:refunded"] {
        let response = deliver(&app, "/v1/webhooks/patreon", bytes.clone(), &[sig], event).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            body(response).await,
            json!({"status":"success","data":{"received":true}})
        );
    }
    let calls = backend.finish();
    let first: Value = serde_json::from_slice(&calls[0].2).unwrap();
    let second: Value = serde_json::from_slice(&calls[1].2).unwrap();
    assert_ne!(first["p_delivery"], second["p_delivery"]);
    assert_eq!(first["p_capture"], second["p_capture"]);
    assert_eq!(first["p_capture"]["bodyHash"], data["bodySha256"]);
    assert_eq!(first["p_capture"].as_object().unwrap().len(), 7);
    let text = String::from_utf8(calls[0].2.clone()).unwrap();
    for private in [
        "Synthetic private name",
        "not-a-real-user",
        "patreon_webhook_synthetic_secret",
        "invented:payment",
    ] {
        assert!(!text.contains(private));
    }
}
#[tokio::test]
async fn patreon_ingress_rejects_missing_duplicate_wrong_signature_query_and_oversize_before_capture()
 {
    let data = test_fixture::source();
    let sig = data["signature"].as_str().unwrap();
    let bytes = data["webhookBody"].as_str().unwrap().as_bytes().to_vec();
    let backend = Mock::new(vec![]);
    let app = router(intake_config(&backend.base));
    for (path, body, signatures) in [
        ("/v1/webhooks/patreon", bytes.clone(), vec![]),
        ("/v1/webhooks/patreon", bytes.clone(), vec![sig, sig]),
        (
            "/v1/webhooks/patreon",
            bytes.clone(),
            vec!["00000000000000000000000000000000"],
        ),
        (
            "/v1/webhooks/patreon?campaignId=123",
            bytes.clone(),
            vec![sig],
        ),
        (
            "/v1/webhooks/patreon",
            vec![b'a'; crate::patreon::MAX_WEBHOOK_BODY + 1],
            vec![sig],
        ),
        ("/v1/webhooks/patreon", b"{}".to_vec(), vec![sig]),
    ] {
        let response = deliver(&app, path, body, &signatures, "members:update").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let mut cfg = intake_config(&backend.base);
    cfg.patreon_intake = Some(Arc::new(
        IntakeConfig::new(
            "other_campaign".into(),
            data["secret"].as_str().unwrap().into(),
            "patreon_private".into(),
        )
        .unwrap(),
    ));
    assert_eq!(
        deliver(
            &router(cfg),
            "/v1/webhooks/patreon",
            bytes,
            &[sig],
            "members:update"
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(backend.finish().is_empty());
}
#[tokio::test]
async fn patreon_ingress_unknown_capture_and_unconfigured_source_never_acknowledge_success() {
    let data = test_fixture::source();
    let bytes = data["webhookBody"].as_str().unwrap().as_bytes().to_vec();
    let sig = data["signature"].as_str().unwrap();
    let mut unknown = captured();
    unknown.drop_ack = true;
    let backend = Mock::new(vec![unknown]);
    let response = deliver(
        &router(intake_config(&backend.base)),
        "/v1/webhooks/patreon",
        bytes.clone(),
        &[sig],
        "members:update",
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let text = body(response).await.to_string();
    assert!(!text.contains("patreon_webhook"));
    assert!(!text.contains("Synthetic"));
    backend.finish();
    let backend = Mock::new(vec![]);
    let response = deliver(
        &router(config(&backend.base)),
        "/v1/webhooks/patreon",
        bytes,
        &[sig],
        "members:update",
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(backend.finish().is_empty());
}
