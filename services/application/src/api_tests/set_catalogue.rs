//! Caller-only accepted set labels; there is no purchase/support claim in this projection.
use super::*;
#[tokio::test]
async fn set_catalogue_preserves_original_bearer_literal_query_and_nullable_code() {
    let (token, _) = identity();
    let id = Uuid::new_v4();
    let data = json!({"items":[{"id":id,"name":"Test %_ set","code":null}],"nextCursor":null});
    let backend = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_my_sets",
        &token,
        json!({"status":"success","data":data}),
    )]);
    let app = router(config(&backend.base));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sets?query=%25_")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        body(response).await,
        json!({"status":"success","data":data})
    );
    let seen = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[0].2).unwrap(),
        json!({"p_query":"%_","p_after":null,"p_limit":40})
    );
}
#[tokio::test]
async fn set_catalogue_rejects_unsupported_queries_and_inconsistent_cursors() {
    let (token, _) = identity();
    let id = Uuid::new_v4();
    let backend = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_my_sets",
        &token,
        json!({"status":"success","data":{"items":[{"id":id,"name":"Set","code":null}],"nextCursor":id}}),
    )]);
    let app = router(config(&backend.base));
    for (path, status) in [
        ("/v1/sets?limit=101", StatusCode::BAD_REQUEST),
        ("/v1/sets?owned=true", StatusCode::BAD_REQUEST),
        ("/v1/sets", StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    backend.finish();
}
