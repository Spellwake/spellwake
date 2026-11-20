//! Public transport/order controls; actual native HTTP execution is qualified separately.
use super::*;
fn original() -> Value {
    json!({"requestId":Uuid::from_u128(3),"basis":{"gameId":Uuid::from_u128(1),"contextId":Uuid::from_u128(2),"epoch":"0","revision":"7"},"action":{"kind":"pass"}})
}
async fn send(app: &Router, token: &str, uri: &str, raw: String) -> Response {
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
#[tokio::test]
async fn native_commands_reject_body_injection_and_limits_without_upstream_work() {
    let (token, _) = identity();
    let mock = Mock::new(vec![]);
    let app = router(config(&mock.base));
    let path = format!("/v1/games/{}/commands", Uuid::from_u128(1));
    let good = original();
    let mut cases = vec![
        "null".to_owned(),
        "[]".to_owned(),
        "x".repeat(crate::player_wire::MAX_PLAYER_WIRE_BYTES + 1),
    ];
    for key in [
        "actorSeatId",
        "owner",
        "fenceEpoch",
        "candidate",
        "stateRef",
        "viewReceiptId",
        "facts",
        "clocks",
    ] {
        let mut value = good.clone();
        value[key] = json!("client-injection");
        cases.push(value.to_string());
    }
    for (pointer, value) in [
        ("/basis/gameId", json!(Uuid::new_v4())),
        ("/requestId", json!(Uuid::nil())),
        ("/basis/revision", json!("07")),
    ] {
        let mut case = good.clone();
        *case.pointer_mut(pointer).unwrap() = value;
        cases.push(case.to_string());
    }
    cases.push(good.to_string().replacen(
        "\"requestId\":",
        &format!(
            "\"requestId\":\"{}\",\"requestId\":",
            good["requestId"].as_str().unwrap()
        ),
        1,
    ));
    for raw in cases {
        let response = send(&app, &token, &path, raw).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    assert_eq!(
        send(
            &app,
            &token,
            &format!("{path}?actor=other"),
            good.to_string()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &app,
            &token,
            "/v1/games/not-a-uuid/commands",
            good.to_string()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(mock.finish().is_empty());
}
#[tokio::test]
async fn native_commands_recover_original_success_and_rejection_without_runtime_or_owner() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let original = original();
    let seat = Uuid::new_v4();
    let mut steps = vec![];
    for (state, response) in [
        (
            "accepted",
            json!({"status":"success","data":{"requestId":original["requestId"],"actorSeatId":seat,"gameId":original["basis"]["gameId"],"contextId":original["basis"]["contextId"],"epoch":"0","revision":"8"}}),
        ),
        (
            "rejected",
            json!({"status":"fail","data":{"action":"not permitted"}}),
        ),
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        let mut read = Step::json(
            "/rest/v1/rpc/read_native_human_receipts",
            json!({"status":"success","data":{"receipts":[{"actorSeatId":seat,"original":original,"state":state,"response":response}]}}),
        );
        read.expect=Some(serde_json::to_vec(&json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_query":{"gameId":original["basis"]["gameId"],"contextId":original["basis"]["contextId"],"outstanding":[original]}})).unwrap());
        steps.push(read);
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    let path = format!("/v1/games/{}/commands", Uuid::from_u128(1));
    let response = send(&app, &token, &path, original.to_string()).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let value = body(response).await;
    assert_eq!(value["data"]["basis"]["revision"], "8");
    assert_eq!(value["data"]["actorSeatId"], json!(seat));
    let response = send(&app, &token, &path, original.to_string()).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body(response).await["data"],
        json!({"action":"not permitted"})
    );
    assert_eq!(mock.finish().len(), 4);
}
#[tokio::test]
async fn native_commands_keep_gate_refusals_separate_from_rule_rejections_and_private_errors() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let mut steps = vec![];
    for data in [
        json!({"session":"unauthorized"}),
        json!({"request":"conflict"}),
        json!({"game":"unavailable"}),
        json!({"unknown":"private-canonical-ref"}),
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            "/rest/v1/rpc/read_native_human_receipts",
            json!({"status":"fail","data":data}),
        ));
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    let path = format!("/v1/games/{}/commands", Uuid::from_u128(1));
    for status in [
        StatusCode::UNAUTHORIZED,
        StatusCode::CONFLICT,
        StatusCode::NOT_FOUND,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = send(&app, &token, &path, original().to_string()).await;
        assert_eq!(response.status(), status);
        assert!(
            !body(response)
                .await
                .to_string()
                .contains("private-canonical-ref")
        );
    }
    assert_eq!(mock.finish().len(), 8);
}
