//! Actual HTTP transport with controlled Auth/SQL/provider envelopes; not live OAuth or managed SQL.
use super::*;
use crate::{
    patreon::{oauth::OAuthConfig, test_fixture as f},
    test_http::Credentials,
};
fn fixture() -> Value {
    f::association()
}
fn source_step(path: &str, value: Value, credential: Option<&str>) -> Step {
    let mut step = if path == "/api/oauth2/token" {
        Step::json(path, value)
    } else {
        Step::get(path.into(), 200, serde_json::to_vec(&value).unwrap())
    };
    step.credentials = credential.map_or(Credentials::Form, |key| Credentials::Bearer(key.into()));
    step
}
fn source() -> Mock {
    let data = fixture();
    Mock::new(vec![
        source_step("/api/oauth2/token", data["token"].clone(), None),
        source_step(
            &f::identity_path(),
            data["identity"].clone(),
            Some("patreon_test_user_synthetic"),
        ),
        source_step(
            &f::member_path(),
            f::source()["member"].clone(),
            Some("patreon_test_creator_synthetic"),
        ),
    ])
}
fn linked() -> Value {
    let mut value = fixture()["internal"].clone();
    value["phase"] = json!("linked");
    value["associationId"] = fixture()["status"]["associationId"].clone();
    value
}
fn claimed() -> Value {
    let mut value = fixture()["internal"].clone();
    value["phase"] = json!("claimed");
    value
}
fn active() -> (String, Value) {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = fixture()["status"]["accountId"].clone();
    (token, session)
}
fn db(path: &str, data: Value) -> Step {
    Step::json(
        &format!("/rest/v1/rpc/{path}"),
        json!({"status":"success","data":data}),
    )
}
async fn post(app: &Router, token: &str, path: &str, input: &Value) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/account/patreon/{path}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(input).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn private_objects_patreon_ack_status_terms_and_claim_arrays_never_exchange() {
    let data = fixture();
    let (token, session) = active();
    let provider = Mock::new(vec![]);
    let mut bad_terms = data["internal"].clone();
    bad_terms["serverTerms"] = crate::test_http::positional(
        &bad_terms["serverTerms"],
        &["version", "campaignId", "clientId", "redirectUri"],
    );
    let status_array = crate::test_http::positional(
        &data["internal"],
        &[
            "accountId",
            "requestId",
            "phase",
            "expiresAt",
            "generation",
            "serverTerms",
            "associationId",
        ],
    );
    let mut duplicate_terms = db("read_patreon_link", Value::Null);
    duplicate_terms.body = json!({"status":"success","data":data["internal"]})
        .to_string()
        .replace("\"version\":1", "\"version\":1,\"version\":1")
        .into_bytes();
    for (path, input, mut steps) in [
        ("complete", &data["complete"], vec![duplicate_terms]),
        (
            "start",
            &data["flow"],
            vec![db(
                "start_patreon_link",
                json!([data["flow"]["requestId"], data["start"]["expiresAt"]]),
            )],
        ),
        (
            "complete",
            &data["complete"],
            vec![db("read_patreon_link", status_array)],
        ),
        (
            "complete",
            &data["complete"],
            vec![db("read_patreon_link", bad_terms)],
        ),
        (
            "complete",
            &data["complete"],
            vec![
                db("read_patreon_link", data["internal"].clone()),
                db("claim_patreon_link", json!([true])),
            ],
        ),
    ] {
        steps.insert(
            0,
            caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        );
        let count = steps.len();
        let backend = Mock::new(steps);
        let mut cfg = config(&backend.base);
        cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
        let response = post(&router(cfg), &token, path, input).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(backend.finish().len(), count);
    }
    let mut duplicate = db("claim_patreon_link", Value::Null);
    duplicate.body =
        br#"{"status":"success","data":{"mayExchange":true,"mayExchange":true}}"#.to_vec();
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        db("read_patreon_link", data["internal"].clone()),
        duplicate,
    ]);
    let mut cfg = config(&backend.base);
    cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
    assert_eq!(
        post(&router(cfg), &token, "complete", &data["complete"])
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(backend.finish().len(), 3);
    assert!(provider.finish().is_empty());
}

#[tokio::test]
async fn private_objects_patreon_status_keeps_required_null_terms_distinct_from_missing() {
    let data = fixture();
    let (token, session) = active();
    let mut unknown = data["internal"].clone();
    unknown["phase"] = json!("unknown");
    for field in ["expiresAt", "generation", "serverTerms", "associationId"] {
        unknown[field] = Value::Null;
    }
    let mut missing = unknown.clone();
    missing.as_object_mut().unwrap().remove("serverTerms");
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", unknown),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        db("read_patreon_link", missing),
    ]);
    // Status recovery needs no provider credentials or activation.
    let app = router(config(&backend.base));
    let response = post(&app, &token, "status", &data["flow"]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await["data"]["phase"], "unknown");
    assert_eq!(
        post(&app, &token, "status", &data["flow"]).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(backend.finish().len(), 4);
}
#[tokio::test]
async fn patreon_start_complete_status_and_replay_keep_one_exchange_and_private_source() {
    let data = fixture();
    let (token, session) = active();
    let provider = source();
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db(
            "start_patreon_link",
            json!({"resourceId":data["flow"]["requestId"],"expiresAt":data["start"]["expiresAt"]}),
        ),
        db("read_patreon_link", data["internal"].clone()),
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", data["internal"].clone()),
        db("claim_patreon_link", json!({"mayExchange":true})),
        db(
            "complete_patreon_link",
            json!({"resourceId":data["status"]["associationId"]}),
        ),
        db("read_patreon_link", linked()),
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", linked()),
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", linked()),
    ]);
    let mut cfg = config(&backend.base);
    cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
    let app = router(cfg);
    for (path, input, expected, status) in [
        ("start", &data["flow"], &data["start"], StatusCode::ACCEPTED),
        (
            "complete",
            &data["complete"],
            &data["status"],
            StatusCode::OK,
        ),
        ("status", &data["flow"], &data["status"], StatusCode::OK),
        (
            "complete",
            &data["complete"],
            &data["status"],
            StatusCode::OK,
        ),
    ] {
        let response = post(&app, &token, path, input).await;
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            body(response).await,
            json!({"status":"success","data":expected})
        );
    }
    let calls = backend.finish();
    let complete = serde_json::from_slice::<Value>(&calls[6].2).unwrap();
    assert_eq!(complete["p_subject"], session["data"]["subjectId"]);
    assert_eq!(complete["p_session"], session["data"]["sessionId"]);
    assert_eq!(complete["p_proof"]["userId"], "456");
    assert_eq!(complete["p_proof"]["memberId"], "member-abc");
    assert_eq!(complete["p_proof"]["campaignId"], "123");
    assert_eq!(complete["p_proof"]["entitledTierIds"], json!(["789"]));
    let provider_calls = provider.finish();
    assert_eq!(provider_calls.len(), 3);
    let form = reqwest::Url::parse(&format!(
        "https://example.invalid/?{}",
        String::from_utf8(provider_calls[0].2.clone()).unwrap()
    ))
    .unwrap()
    .query_pairs()
    .into_owned()
    .collect::<BTreeMap<_, _>>();
    assert_eq!(form["code"], data["complete"]["code"].as_str().unwrap());
    assert_eq!(form["client_secret"], "patreon_test_client_secret");
    assert_eq!(
        form["redirect_uri"],
        data["internal"]["serverTerms"]["redirectUri"]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        complete["p_proof"]["identityHash"],
        crate::provider_json::fingerprint(&serde_json::to_vec(&data["identity"]).unwrap())
    );
    assert_eq!(
        complete["p_proof"]["memberHash"],
        crate::provider_json::fingerprint(&serde_json::to_vec(&f::source()["member"]).unwrap())
    );
    for call in calls
        .iter()
        .filter(|v| !v.0.ends_with("read_current_session"))
    {
        let text = String::from_utf8(call.2.clone()).unwrap();
        for forbidden in [
            "patreon_test_code",
            "patreon_test_user_synthetic",
            "patreon_test_creator_synthetic",
            "patreon_test_client_secret",
            data["flow"]["stateKey"].as_str().unwrap(),
        ] {
            assert!(!text.contains(forbidden));
        }
    }
}
#[tokio::test]
async fn patreon_unknown_claim_ack_and_subsequent_observations_never_exchange_again() {
    let data = fixture();
    let (token, session) = active();
    let provider = Mock::new(vec![]);
    let mut unknown = db("claim_patreon_link", json!({"mayExchange":true}));
    unknown.drop_ack = true;
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", data["internal"].clone()),
        unknown,
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", claimed()),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        db("read_patreon_link", claimed()),
    ]);
    let mut cfg = config(&backend.base);
    cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
    let app = router(cfg);
    assert_eq!(
        post(&app, &token, "complete", &data["complete"])
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    for (path, input) in [("status", &data["flow"]), ("complete", &data["complete"])] {
        let response = post(&app, &token, path, input).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await["data"]["phase"], "claimed");
    }
    assert_eq!(
        backend
            .finish()
            .iter()
            .filter(|v| v.0.ends_with("claim_patreon_link"))
            .count(),
        1
    );
    assert!(provider.finish().is_empty());
}
#[tokio::test]
async fn patreon_lost_completion_ack_recovers_from_owned_status_without_provider_config() {
    let data = fixture();
    let (token, session) = active();
    let provider = source();
    let mut unknown = db(
        "complete_patreon_link",
        json!({"resourceId":data["status"]["associationId"]}),
    );
    unknown.drop_ack = true;
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", data["internal"].clone()),
        db("claim_patreon_link", json!({"mayExchange":true})),
        unknown,
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", linked()),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        db("read_patreon_link", linked()),
    ]);
    let mut cfg = config(&backend.base);
    cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
    let app = router(cfg);
    assert_eq!(
        post(&app, &token, "complete", &data["complete"])
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let no_config = router(config(&backend.base));
    for (path, input) in [("status", &data["flow"]), ("complete", &data["complete"])] {
        let response = post(&no_config, &token, path, input).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await["data"], data["status"]);
    }
    assert_eq!(
        backend
            .finish()
            .iter()
            .filter(|v| v.0.ends_with("complete_patreon_link"))
            .count(),
        1
    );
    assert_eq!(provider.finish().len(), 3);
}

#[tokio::test]
async fn patreon_lost_token_response_spends_claim_without_repeating_exchange() {
    let data = fixture();
    let (token, session) = active();
    let mut exchange = source_step("/api/oauth2/token", data["token"].clone(), None);
    exchange.drop_ack = true;
    let provider = Mock::new(vec![exchange]);
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", data["internal"].clone()),
        db("claim_patreon_link", json!({"mayExchange":true})),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        db("read_patreon_link", claimed()),
    ]);
    let mut cfg = config(&backend.base);
    cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
    let app = router(cfg);
    let response = post(&app, &token, "complete", &data["complete"]).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let response = post(&app, &token, "complete", &data["complete"]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await["data"]["phase"], "claimed");
    assert_eq!(backend.finish().len(), 5);
    assert_eq!(provider.finish().len(), 1);
}

#[tokio::test]
async fn patreon_invalid_token_or_redirect_never_reaches_identity_or_completion() {
    let data = fixture();
    let mut expired = data["token"].clone();
    expired["expires_in"] = json!(0);
    let mut wrong_scope = data["token"].clone();
    wrong_scope["scope"] = json!("campaigns");
    let mut wrong_kind = data["token"].clone();
    wrong_kind["token_type"] = json!("Basic");
    let mut redirect = source_step("/api/oauth2/token", json!({}), None);
    redirect.status = 302;
    redirect
        .response_headers
        .push(("Location".into(), "/unexpected-redirect".into()));
    let mut invalid_grant =
        source_step("/api/oauth2/token", json!({"error":"invalid_grant"}), None);
    invalid_grant.status = 400;
    let cases = vec![
        source_step("/api/oauth2/token", expired, None),
        source_step("/api/oauth2/token", wrong_scope, None),
        source_step("/api/oauth2/token", wrong_kind, None),
        redirect,
        invalid_grant,
    ];
    for exchange in cases {
        let (token, session) = active();
        let provider = Mock::new(vec![exchange]);
        let backend = Mock::new(vec![
            caller_step("/rest/v1/rpc/read_current_session", &token, session),
            db("read_patreon_link", data["internal"].clone()),
            db("claim_patreon_link", json!({"mayExchange":true})),
        ]);
        let mut cfg = config(&backend.base);
        cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
        let response = post(&router(cfg), &token, "complete", &data["complete"]).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let response = body(response).await.to_string();
        assert!(!response.contains("patreon_test"));
        assert!(!response.contains("invalid_grant"));
        assert_eq!(backend.finish().len(), 3);
        assert_eq!(provider.finish().len(), 1);
    }
}

#[tokio::test]
async fn patreon_absent_membership_differs_from_unresolved_or_foreign_identity() {
    let data = fixture();
    let mut absent = data["identity"].clone();
    absent["data"]["relationships"]["memberships"]["data"] = json!([]);
    let mut missing = data["identity"].clone();
    missing["data"]["relationships"]
        .as_object_mut()
        .unwrap()
        .remove("memberships");
    let mut unresolved = data["identity"].clone();
    unresolved["included"] = json!([]);
    let mut foreign = data["identity"].clone();
    foreign["included"][0]["relationships"]["user"]["data"]["id"] = json!("someone_else");
    let mut duplicate_resource = data["identity"].clone();
    duplicate_resource["included"]
        .as_array_mut()
        .unwrap()
        .push(data["identity"]["included"][0].clone());
    let mut duplicate_reference = data["identity"].clone();
    duplicate_reference["data"]["relationships"]["memberships"]["data"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"member","id":"member-abc"}));
    let mut ambiguous = data["identity"].clone();
    let mut other_member = data["identity"]["included"][0].clone();
    other_member["id"] = json!("member-def");
    ambiguous["included"]
        .as_array_mut()
        .unwrap()
        .push(other_member);
    ambiguous["data"]["relationships"]["memberships"]["data"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"member","id":"member-def"}));
    for (identity, expected) in [
        (absent, StatusCode::CONFLICT),
        (missing, StatusCode::SERVICE_UNAVAILABLE),
        (unresolved, StatusCode::SERVICE_UNAVAILABLE),
        (foreign, StatusCode::SERVICE_UNAVAILABLE),
        (duplicate_resource, StatusCode::SERVICE_UNAVAILABLE),
        (duplicate_reference, StatusCode::SERVICE_UNAVAILABLE),
        (ambiguous, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let (token, session) = active();
        let provider = Mock::new(vec![
            source_step("/api/oauth2/token", data["token"].clone(), None),
            source_step(
                &f::identity_path(),
                identity,
                Some("patreon_test_user_synthetic"),
            ),
        ]);
        let backend = Mock::new(vec![
            caller_step("/rest/v1/rpc/read_current_session", &token, session),
            db("read_patreon_link", data["internal"].clone()),
            db("claim_patreon_link", json!({"mayExchange":true})),
        ]);
        let mut cfg = config(&backend.base);
        cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
        let response = post(&router(cfg), &token, "complete", &data["complete"]).await;
        assert_eq!(response.status(), expected);
        if expected == StatusCode::CONFLICT {
            assert_eq!(
                body(response).await,
                json!({"status":"fail","data":{"membership":"unavailable"}})
            );
        }
        assert_eq!(backend.finish().len(), 3);
        assert_eq!(provider.finish().len(), 2);
    }
}

#[tokio::test]
async fn patreon_denied_exchange_claim_observes_existing_phase_without_provider_work() {
    let data = fixture();
    for (observed, expected) in [
        (claimed(), StatusCode::OK),
        (linked(), StatusCode::OK),
        (data["internal"].clone(), StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let (token, session) = active();
        let provider = Mock::new(vec![]);
        let backend = Mock::new(vec![
            caller_step("/rest/v1/rpc/read_current_session", &token, session),
            db("read_patreon_link", data["internal"].clone()),
            db("claim_patreon_link", json!({"mayExchange":false})),
            db("read_patreon_link", observed.clone()),
        ]);
        let mut cfg = config(&backend.base);
        cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
        let response = post(&router(cfg), &token, "complete", &data["complete"]).await;
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            assert_eq!(body(response).await["data"]["phase"], observed["phase"]);
        }
        assert_eq!(backend.finish().len(), 4);
        assert!(provider.finish().is_empty());
    }
}

#[tokio::test]
async fn patreon_wrong_read_scope_or_changed_server_terms_cannot_claim_or_exchange() {
    let data = fixture();
    let mut foreign = data["internal"].clone();
    foreign["accountId"] = json!("11111111-1111-4111-8111-111111111111");
    let mut request = data["internal"].clone();
    request["requestId"] = json!("11111111-1111-4111-8111-111111111111");
    let mut missing = data["internal"].clone();
    missing.as_object_mut().unwrap().remove("associationId");
    let mut generation = data["internal"].clone();
    generation["generation"] = json!("01");
    let mut changed = data["internal"].clone();
    changed["serverTerms"]["clientId"] = json!("older_client");
    for observed in [foreign, request, missing, generation, changed] {
        let (token, session) = active();
        let provider = Mock::new(vec![]);
        let backend = Mock::new(vec![
            caller_step("/rest/v1/rpc/read_current_session", &token, session),
            db("read_patreon_link", observed),
        ]);
        let mut cfg = config(&backend.base);
        cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
        let response = post(&router(cfg), &token, "complete", &data["complete"]).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(backend.finish().len(), 2);
        assert!(provider.finish().is_empty());
    }
}

#[tokio::test]
async fn patreon_invalid_public_terms_are_rejected_before_auth_or_provider_work() {
    let data = fixture();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let mut foreign_actor = data["complete"].clone();
    foreign_actor["accountId"] = data["status"]["accountId"].clone();
    let mut provider = data["flow"].clone();
    provider["campaignId"] = json!("123");
    let mut nil = data["flow"].clone();
    nil["requestId"] = json!(Uuid::nil());
    let mut bad_key = data["flow"].clone();
    bad_key["stateKey"] = json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB");
    let mut short_code = data["complete"].clone();
    short_code["code"] = json!("short");
    for (path, input) in [
        ("complete", foreign_actor),
        ("start", provider),
        ("status", nil),
        ("start", bad_key),
        ("complete", short_code),
        ("status?campaignId=123", data["flow"].clone()),
    ] {
        let response = post(&app, "test_invalid_before_auth", path, &input).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    assert!(backend.finish().is_empty());
}

#[tokio::test]
async fn patreon_creator_read_must_confirm_the_same_user_and_campaign() {
    let data = fixture();
    for (relationship, replacement) in [("user", "foreign_user"), ("campaign", "foreign_campaign")]
    {
        let (token, session) = active();
        let mut member = f::source()["member"].clone();
        member["data"]["relationships"][relationship]["data"]["id"] = json!(replacement);
        let provider = Mock::new(vec![
            source_step("/api/oauth2/token", data["token"].clone(), None),
            source_step(
                &f::identity_path(),
                data["identity"].clone(),
                Some("patreon_test_user_synthetic"),
            ),
            source_step(
                &f::member_path(),
                member,
                Some("patreon_test_creator_synthetic"),
            ),
        ]);
        let backend = Mock::new(vec![
            caller_step("/rest/v1/rpc/read_current_session", &token, session),
            db("read_patreon_link", data["internal"].clone()),
            db("claim_patreon_link", json!({"mayExchange":true})),
        ]);
        let mut cfg = config(&backend.base);
        cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
        let response = post(&router(cfg), &token, "complete", &data["complete"]).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(backend.finish().len(), 3);
        assert_eq!(provider.finish().len(), 3);
    }
}

#[tokio::test]
async fn patreon_final_authority_denial_does_not_acknowledge_provider_success() {
    let data = fixture();
    for (failure, expected) in [
        (json!({"session":"unauthorized"}), StatusCode::UNAUTHORIZED),
        (json!({"request":"expired"}), StatusCode::CONFLICT),
        (json!({"proof":"invalid"}), StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let (token, session) = active();
        let provider = source();
        let backend = Mock::new(vec![
            caller_step("/rest/v1/rpc/read_current_session", &token, session),
            db("read_patreon_link", data["internal"].clone()),
            db("claim_patreon_link", json!({"mayExchange":true})),
            Step::json(
                "/rest/v1/rpc/complete_patreon_link",
                json!({"status":"fail","data":failure}),
            ),
        ]);
        let mut cfg = config(&backend.base);
        cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
        let response = post(&router(cfg), &token, "complete", &data["complete"]).await;
        assert_eq!(response.status(), expected);
        assert_ne!(body(response).await["status"], "success");
        assert_eq!(backend.finish().len(), 4);
        assert_eq!(provider.finish().len(), 3);
    }
}

#[tokio::test]
async fn patreon_pending_deletion_recovers_linked_result_but_cannot_begin_new_work() {
    let data = fixture();
    let (token, mut session) = active();
    session["data"]["state"] = json!("deletion_pending");
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        db("read_patreon_link", linked()),
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        db("read_patreon_link", data["internal"].clone()),
    ]);
    let app = router(config(&backend.base));
    let response = post(&app, &token, "complete", &data["complete"]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await["data"], data["status"]);
    for (path, input) in [("start", &data["flow"]), ("complete", &data["complete"])] {
        let response = post(&app, &token, path, input).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            body(response).await,
            json!({"status":"fail","data":{"account":"unavailable"}})
        );
    }
    assert_eq!(backend.finish().len(), 5);
}

#[tokio::test]
async fn patreon_start_replay_preserves_frozen_url_expiry_and_hashed_terms() {
    let data = fixture();
    let (token, session) = active();
    let provider = Mock::new(vec![]);
    let mut steps = Vec::new();
    for _ in 0..2 {
        steps.extend([
            caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
            db("start_patreon_link", json!({"resourceId":data["flow"]["requestId"],"expiresAt":data["start"]["expiresAt"]})),
            db("read_patreon_link", data["internal"].clone()),
        ]);
    }
    let backend = Mock::new(steps);
    let mut cfg = config(&backend.base);
    cfg.patreon = Some(Arc::new(OAuthConfig::fixture(&provider.base)));
    let app = router(cfg);
    for _ in 0..2 {
        let response = post(&app, &token, "start", &data["flow"]).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(body(response).await["data"], data["start"]);
    }
    let calls = backend.finish();
    assert_eq!(calls[1].2, calls[4].2);
    let terms = serde_json::from_slice::<Value>(&calls[1].2).unwrap();
    assert_eq!(terms["p_terms"], data["internal"]["serverTerms"]);
    assert_eq!(
        terms["p_state_hash"],
        crate::provider_json::fingerprint(&[0; 32])
    );
    assert!(provider.finish().is_empty());
}

#[tokio::test]
async fn patreon_terminal_or_superseded_initial_disposition_cannot_reexchange() {
    let data = fixture();
    for phase in ["expired", "superseded"] {
        let (token, session) = active();
        let mut observed = data["internal"].clone();
        observed["phase"] = json!(phase);
        let backend = Mock::new(vec![
            caller_step("/rest/v1/rpc/read_current_session", &token, session),
            db("read_patreon_link", observed),
        ]);
        // No configuration is needed to return an already established disposition.
        let response = post(
            &router(config(&backend.base)),
            &token,
            "complete",
            &data["complete"],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await["data"]["phase"], phase);
        assert_eq!(backend.finish().len(), 2);
    }
}
