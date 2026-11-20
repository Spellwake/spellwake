//! HTTP fixtures check transport and strict protocol; actual SQL is tested separately.
use super::*;

#[tokio::test]
async fn import_uses_original_session_and_returns_only_both_confirmed_ids() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let resource = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let mock = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/import_deck_command",
            json!({"status":"success","data":{"resourceId":resource,"revisionId":revision}}),
        ),
    ]);
    let input = json!({"requestId":Uuid::new_v4(),"name":"Resolved import","mode":"collection","formatId":Uuid::new_v4(),"entries":[]});
    let response = router(config(&mock.base))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/decks/import")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        body(response).await,
        json!({"status":"success","data":{"resourceId":resource,"revisionId":revision}})
    );
    let seen = mock.finish();
    let forwarded: Value = serde_json::from_slice(&seen[1].2).unwrap();
    assert_eq!(
        forwarded,
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_input":input})
    );
}
#[tokio::test]
async fn import_recovery_validates_state_specific_two_id_success_and_rejects_private_failures() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let id = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let responses = vec![
        json!({"status":"success","data":{"state":"unknown","response":null}}),
        json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":id,"revisionId":revision}}}}),
        json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":id}}}}),
        json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"private":"secret_provider_data"}}}}),
    ];
    let mut steps = vec![];
    for response in responses {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            "/rest/v1/rpc/read_deck_import_command",
            response,
        ));
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    let input = json!({"requestId":Uuid::new_v4(),"name":"Import","mode":"open","formatId":Uuid::new_v4(),"entries":[]});
    for expected in [
        StatusCode::OK,
        StatusCode::OK,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/decks/import/disposition")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(
            !body(response)
                .await
                .to_string()
                .contains("secret_provider_data")
        );
    }
    mock.finish();
}
#[tokio::test]
async fn private_headers_cover_early_failure_and_import_rejects_queries_before_io() {
    let mock = Mock::new(vec![]);
    let app = router(config(&mock.base));
    for (path, method) in [
        ("/v1/account", "GET"),
        ("/v1/inventory", "GET"),
        ("/v1/decks/import?actor=forged", "POST"),
        ("/v1/decks/import", "POST"),
        ("/v1/unknown", "GET"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_client_error());
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    mock.finish();
}
