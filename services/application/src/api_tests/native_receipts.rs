//! Router mechanics only; owned managed Auth/DB qualification is separate.
use super::*;
fn query() -> Value {
    json!({"gameId":Uuid::from_u128(1),"contextId":Uuid::from_u128(2),"outstanding":[{"requestId":Uuid::from_u128(3),"basis":{"gameId":Uuid::from_u128(1),"contextId":Uuid::from_u128(2),"epoch":"0","revision":"7"},"action":{"kind":"pass"}}]})
}
async fn send(app: &Router, token: &str, raw: String) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/games/{}/receipts", Uuid::from_u128(1)))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(raw))
                .unwrap(),
        )
        .await
        .unwrap()
}
#[tokio::test]
async fn native_receipts_use_verified_identity_and_preserve_historical_ack_without_game_execution()
{
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let input = query();
    let seat = Uuid::new_v4();
    let ack = json!({"requestId":Uuid::from_u128(3),"actorSeatId":seat,"gameId":input["gameId"],"contextId":input["contextId"],"epoch":"0","revision":"8"});
    let mut step = Step::json(
        "/rest/v1/rpc/read_native_human_receipts",
        json!({"status":"success","data":{"receipts":[{"actorSeatId":seat,"original":input["outstanding"][0],"state":"accepted","response":{"status":"success","data":ack}}]}}),
    );
    step.expect=Some(serde_json::to_vec(&json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_query":input})).unwrap());
    let mock = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        step,
    ]);
    let app = router(config(&mock.base));
    let response = send(&app, &token, input.to_string()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let value = body(response).await;
    assert_eq!(
        value["data"]["receipts"][0]["disposition"]["response"]["data"]["basis"]["revision"],
        "8"
    );
    assert_eq!(
        value["data"]["receipts"][0]["original"],
        input["outstanding"][0]
    );
    assert_eq!(mock.finish().len(), 2);
}
#[tokio::test]
async fn native_receipts_reject_foreign_invalid_duplicate_and_over_budget_original_queries_before_upstream()
 {
    let (token, _) = identity();
    let mock = Mock::new(vec![]);
    let app = router(config(&mock.base));
    let good = query();
    let mut cases = vec![
        "[]".to_owned(),
        "x".repeat(crate::player_wire::MAX_PLAYER_WIRE_BYTES + 1),
    ];
    for (pointer, value) in [
        ("/gameId", json!(Uuid::new_v4())),
        ("/outstanding/0/basis/contextId", json!(Uuid::new_v4())),
        ("/outstanding/0/basis/revision", json!("07")),
    ] {
        let mut case = good.clone();
        *case.pointer_mut(pointer).unwrap() = value;
        cases.push(case.to_string());
    }
    let mut injected = good.clone();
    injected["actorId"] = json!(Uuid::new_v4());
    cases.push(injected.to_string());
    let mut duplicate = good.clone();
    duplicate["outstanding"]
        .as_array_mut()
        .unwrap()
        .push(good["outstanding"][0].clone());
    cases.push(duplicate.to_string());
    let raw = good.to_string();
    let repeated = raw.replacen("\"outstanding\":", "\"outstanding\":[],\"outstanding\":", 1);
    assert_ne!(raw, repeated);
    cases.push(repeated);
    for raw in cases {
        assert_eq!(
            send(&app, &token, raw).await.status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert!(mock.finish().is_empty());
}
#[tokio::test]
async fn native_receipts_map_auth_absence_conflict_and_unknown_upstream_failures() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let mut steps = vec![];
    for fields in [
        json!({"session":"unauthorized"}),
        json!({"game":"unavailable"}),
        json!({"request":"conflict"}),
        json!({"unknown":"private diagnostic"}),
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            "/rest/v1/rpc/read_native_human_receipts",
            json!({"status":"fail","data":fields}),
        ));
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    for expected in [
        StatusCode::UNAUTHORIZED,
        StatusCode::NOT_FOUND,
        StatusCode::CONFLICT,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = send(&app, &token, query().to_string()).await;
        assert_eq!(response.status(), expected);
        assert!(
            !body(response)
                .await
                .to_string()
                .contains("private diagnostic")
        );
    }
    assert_eq!(mock.finish().len(), 8);
}
