//! Typed owner projections reject foreign owners and malformed cursor context.
use super::*;
#[tokio::test]
async fn quest_reads_forward_the_original_bearer_and_reject_foreign_owner_response() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let valid = json!({"status":"success","data":{"accountId":owner,"utcDay":"2026-11-04","daily":null,"items":[],"nextCursor":null}});
    let mut foreign = valid.clone();
    foreign["data"]["accountId"] = json!(Uuid::new_v4());
    let mut missing = valid.clone();
    missing["data"].as_object_mut().unwrap().remove("daily");
    let mut steps = vec![];
    for response in [valid, foreign, missing] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(caller_step("/rest/v1/rpc/read_my_quests", &token, response));
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    for expected in [
        StatusCode::OK,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/quests?limit=2")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let seen = mock.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[1].2).unwrap(),
        json!({"p_after":null,"p_limit":2})
    );
}
#[tokio::test]
async fn progression_queries_reject_actor_and_invalid_filters_before_requesting_authority() {
    let (token, _) = identity();
    let mock = Mock::new(vec![]);
    let app = router(config(&mock.base));
    for path in [
        "/v1/quests?accountId=foreign",
        "/v1/ranks?limit=0",
        "/v1/ranks?formatId=00000000-0000-0000-0000-000000000000",
        "/v1/seasons?limit=101",
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
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    mock.finish();
}
