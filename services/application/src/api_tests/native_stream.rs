//! Strict transport/identity mechanics; genuine stream execution has owned tests.
use super::native_views::send;
use super::*;
fn query() -> Value {
    json!({"gameId":Uuid::from_u128(1),"contextId":Uuid::from_u128(2),"cursor":null})
}
#[tokio::test]
async fn stream_rejects_bad_scope_unknown_fields_duplicates_and_oversize_before_backend() {
    let (token, _) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let path = format!("/v1/games/{}/stream", Uuid::from_u128(1));
    let mut cases = vec![
        "[]".to_owned(),
        "null".to_owned(),
        "x".repeat(crate::player_wire::MAX_PLAYER_WIRE_BYTES + 1),
    ];
    for (pointer, value) in [
        ("/gameId", json!(Uuid::new_v4())),
        ("/contextId", json!(Uuid::nil())),
        ("/cursor", json!({})),
    ] {
        let mut v = query();
        *v.pointer_mut(pointer).unwrap() = value;
        cases.push(v.to_string());
    }
    let mut extra = query();
    extra["viewerSeatId"] = json!(Uuid::new_v4());
    cases.push(extra.to_string());
    let mut missing = query();
    missing.as_object_mut().unwrap().remove("cursor");
    cases.push(missing.to_string());
    cases.push(query().to_string().replacen(
        "\"cursor\":null",
        "\"cursor\":null,\"cursor\":null",
        1,
    ));
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
async fn stream_requires_original_bearer_canonical_path_and_registered_account() {
    let (token, session) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let path = format!("/v1/games/{}/stream", Uuid::from_u128(1));
    let response = app
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
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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
            "/v1/games/not-a-uuid/stream",
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
#[tokio::test]
async fn stream_missing_native_runtime_is_a_service_error_after_actual_identity_lookup() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let backend = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_current_session",
        &token,
        session,
    )]);
    let response = send(
        &router(config(&backend.base)),
        &token,
        &format!("/v1/games/{}/stream", Uuid::from_u128(1)),
        query().to_string(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body(response).await["status"], "error");
    assert_eq!(backend.finish().len(), 1);
}

fn state(base: &str, workers: usize, streams: usize) -> ApiState {
    ApiState {
        config: Arc::new(config(base)),
        permits: Arc::new(Semaphore::new(workers)),
        stream_permits: Arc::new(Semaphore::new(streams)),
    }
}
async fn direct(state: ApiState, token: &str) -> Response {
    native_stream_routes::read(
        State(state),
        Ok(Path(Uuid::from_u128(1).to_string())),
        Request::builder()
            .method("POST")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(query().to_string()))
            .unwrap(),
    )
    .await
}
#[tokio::test]
async fn stream_lifetime_and_work_admission_are_separate_and_release_on_preflight_failures() {
    let (token, session) = identity();
    let mock = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_current_session",
        &token,
        session,
    )]);
    let state = state(&mock.base, 1, 1);
    let lifetime = state.stream_permits.clone().try_acquire_owned().unwrap();
    assert_eq!(
        direct(state.clone(), &token).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(state.permits.available_permits(), 1);
    drop(lifetime);
    let work = state.permits.clone().try_acquire_owned().unwrap();
    assert_eq!(
        direct(state.clone(), &token).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(state.stream_permits.available_permits(), 1);
    drop(work);
    assert_eq!(
        direct(state.clone(), &token).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(state.stream_permits.available_permits(), 1);
    assert_eq!(state.permits.available_permits(), 1);
    assert_eq!(mock.finish().len(), 1);
}
