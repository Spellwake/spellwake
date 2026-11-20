//! Transport failures before private native work; genuine delivery is qualified separately.
use super::*;

pub(super) async fn send(app: &Router, token: &str, uri: &str, raw: String) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(raw))
                .unwrap(),
        )
        .await
        .unwrap()
}
fn query() -> Value {
    json!({"gameId":Uuid::from_u128(1),"contextId":Uuid::from_u128(2)})
}
#[tokio::test]
async fn native_views_reject_foreign_duplicate_nil_extra_and_oversize_bodies_before_backend() {
    let (token, _) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let path = format!("/v1/games/{}/view", Uuid::from_u128(1));
    let cases = [
        "[]".to_owned(),
        "null".to_owned(),
        "x".repeat(crate::player_wire::MAX_PLAYER_WIRE_BYTES+1),
        json!({"gameId":Uuid::from_u128(3),"contextId":Uuid::from_u128(2)}).to_string(),
        json!({"gameId":Uuid::from_u128(1),"contextId":Uuid::nil()}).to_string(),
        json!({"gameId":Uuid::from_u128(1),"contextId":Uuid::from_u128(2),"viewerSeatId":Uuid::from_u128(4)}).to_string(),
        json!({"gameId":Uuid::from_u128(1),"contextId":Uuid::from_u128(2),"disclosure":"completed"}).to_string(),
        format!(r#"{{"gameId":"{}","contextId":"{}","contextId":"{}"}}"#,Uuid::from_u128(1),Uuid::from_u128(2),Uuid::from_u128(2)),
    ];
    for raw in cases {
        let response = send(&app, &token, &path, raw).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(body(response).await["status"], "fail");
    }
    assert_eq!(
        send(
            &app,
            &token,
            &format!("{path}?viewer=owner"),
            query().to_string()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(backend.finish().is_empty());
}
#[tokio::test]
async fn native_views_resolve_actor_before_returning_missing_runtime_and_never_claim_success() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::from_u128(5));
    let backend = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_current_session",
        &token,
        session,
    )]);
    let app = router(config(&backend.base));
    let response = send(
        &app,
        &token,
        &format!("/v1/games/{}/view", Uuid::from_u128(1)),
        query().to_string(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body(response).await["status"], "error");
    assert_eq!(backend.finish().len(), 1);
}

#[tokio::test]
async fn native_views_require_credentials_canonical_path_and_registered_account() {
    let (token, session) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let path = format!("/v1/games/{}/view", Uuid::from_u128(1));
    let anonymous = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(query().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        send(&app, "invalid-token", &path, query().to_string())
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &app,
            &token,
            "/v1/games/not-a-uuid/view",
            query().to_string()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(backend.finish().is_empty());
    let backend = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_current_session",
        &token,
        session,
    )]);
    let response = send(
        &router(config(&backend.base)),
        &token,
        &path,
        query().to_string(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body(response).await["data"],
        json!({"account":"unavailable"})
    );
    assert_eq!(backend.finish().len(), 1);
}
