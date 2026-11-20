//! Source preflight and publication bind original caller and immutable qualified evidence reference.
use super::*;
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/prediction-publication.json"
    ))
    .unwrap()
}
#[tokio::test]
async fn publication_http_uses_original_source_reader_and_session_command_bridge() {
    let f = fixture();
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = f["source"]["accountId"].clone();
    let printing = f["command"]["command"]["printingId"].as_str().unwrap();
    let created = Uuid::new_v4();
    let backend = Mock::new(vec![
        caller_step(
            "/rest/v1/rpc/read_my_prediction_publication_source",
            &token,
            json!({"status":"success","data":f["source"]}),
        ),
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/execute_prediction_command",
            json!({"status":"success","data":{"resourceId":created}}),
        ),
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/read_prediction_command",
            json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":created}}}}),
        ),
    ]);
    let app = router(config(&backend.base));
    let source = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/predictions/printings/{printing}/source"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(source.status(), StatusCode::OK);
    assert_eq!(source.headers()[header::CACHE_CONTROL], "no-store");
    for lookup in [false, true] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(if lookup {
                        "/v1/predictions/commands/disposition"
                    } else {
                        "/v1/predictions/commands"
                    })
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&f["command"]).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if lookup {
                StatusCode::OK
            } else {
                StatusCode::ACCEPTED
            }
        );
    }
    let seen = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[0].2).unwrap(),
        json!({"p_printing":printing})
    );
    for index in [2, 4] {
        assert_eq!(
            serde_json::from_slice::<Value>(&seen[index].2).unwrap(),
            json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_input":f["command"]})
        );
    }
}
#[tokio::test]
async fn publication_source_distinguishes_unavailable_evidence_from_bad_scoped_response() {
    let f = fixture();
    let (token, _) = identity();
    let printing = f["command"]["command"]["printingId"].as_str().unwrap();
    let mut wrong = f["source"].clone();
    wrong["source"]["printingId"] = json!(Uuid::new_v4());
    let backend = Mock::new(
        [
            json!({"status":"fail","data":{"price":"unavailable"}}),
            json!({"status":"fail","data":{"printing":"unavailable"}}),
            json!({"status":"success","data":wrong}),
        ]
        .into_iter()
        .map(|value| {
            caller_step(
                "/rest/v1/rpc/read_my_prediction_publication_source",
                &token,
                value,
            )
        })
        .collect(),
    );
    let app = router(config(&backend.base));
    for status in [
        StatusCode::CONFLICT,
        StatusCode::NOT_FOUND,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/predictions/printings/{printing}/source"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    backend.finish();
}

#[tokio::test]
async fn preparation_requires_empty_explicit_body_and_rejects_unavailable_source_without_dispatch()
{
    let (token, _) = identity();
    let printing = Uuid::new_v4();
    let backend = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_my_prediction_probe_basis",
        &token,
        json!({"status":"fail","data":{"price":"unavailable"}}),
    )]);
    let app = router(config(&backend.base));
    for (payload, query, status) in [
        ("{\"slot\":\"2026-11-02\"}", "", StatusCode::BAD_REQUEST),
        ("null", "", StatusCode::BAD_REQUEST),
        ("[]", "", StatusCode::BAD_REQUEST),
        ("{}", "?actorId=other", StatusCode::BAD_REQUEST),
        ("{}", "", StatusCode::CONFLICT),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/predictions/printings/{printing}/source{query}"
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let seen = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[0].2).unwrap(),
        json!({"p_printing":printing})
    );
}
#[tokio::test]
#[ignore = "requires tools/test_local_price_workflow.py owned Temporal namespace; controlled Auth protocol"]
async fn real_temporal_preparation_api_reconciles_queued_work_and_rechecks_delivery_authority() {
    let namespace = std::env::var("SPELLWAKE_TEST_TEMPORAL_NAMESPACE").unwrap();
    let suffix = namespace.strip_prefix("spellwake-price-test-").unwrap();
    assert!(suffix.len() == 32 && suffix.bytes().all(|b| b.is_ascii_hexdigit()));
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    let printing = Uuid::new_v4();
    let external = Uuid::new_v4();
    session["data"]["accountId"] = json!(owner);
    session["data"]["state"] = json!("active");
    let basis = json!({"status":"success","data":{"accountId":owner,"basis":{"printingId":printing,"externalPrintingId":external,"slot":"1900-01-03"}}});
    let backend = Mock::new(vec![
        caller_step(
            "/rest/v1/rpc/read_my_prediction_probe_basis",
            &token,
            basis.clone(),
        ),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        caller_step("/rest/v1/rpc/read_my_prediction_probe_basis", &token, basis),
        caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            json!({"status":"fail","data":{"session":"unauthorized"}}),
        ),
    ]);
    let mut configuration = config(&backend.base);
    configuration.temporal = Some(
        crate::temporal_runtime::TemporalConfig::new("http://127.0.0.1:57233", &namespace, None)
            .unwrap(),
    );
    let app = router(configuration);
    for expected in [StatusCode::ACCEPTED, StatusCode::UNAUTHORIZED] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/predictions/printings/{printing}/source"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        if expected == StatusCode::ACCEPTED {
            assert_eq!(
                body(response).await,
                json!({"status":"success","data":{"accountId":owner,"printingId":printing,"slot":"1900-01-03","state":"requested"}})
            );
        }
    }
    backend.finish();
}
