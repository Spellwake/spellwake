//! Configuration acknowledgments retain the exact resulting revision and original intent.
use super::*;
fn intent() -> Value {
    json!({"requestId":Uuid::new_v4(),"expectedRevision":"9007199254740993","profileId":Uuid::new_v4(),"structure":"bo1","teams":[{"slotId":Uuid::new_v4(),"teamRef":"  🐉  "}]})
}
#[tokio::test]
async fn configuration_forwards_verified_session_and_preserves_labels_and_exact_revision() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let room = Uuid::new_v4();
    let input = intent();
    let ack = json!({"resourceId":room,"revision":"9007199254740994"});
    let mock = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/configure_room_command",
            json!({"status":"success","data":ack}),
        ),
    ]);
    let response = router(config(&mock.base))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/rooms/{room}/configuration"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(body(response).await, json!({"status":"success","data":ack}));
    let seen = mock.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[1].2).unwrap(),
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":input["requestId"],"p_room":room,"p_expected":input["expectedRevision"],"p_profile":input["profileId"],"p_structure":input["structure"],"p_teams":input["teams"]})
    );
}
#[tokio::test]
async fn configuration_lookup_rejects_wrong_target_revision_and_missing_revision() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let room = Uuid::new_v4();
    let mut steps = vec![];
    for ack in [
        json!({"resourceId":room,"revision":"9007199254740993"}),
        json!({"resourceId":room,"revision":"9007199254740995"}),
        json!({"resourceId":Uuid::new_v4(),"revision":"9007199254740994"}),
        json!({"resourceId":room}),
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json("/rest/v1/rpc/read_room_configuration_command",json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":ack}}})));
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    for expected in [
        StatusCode::OK,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/rooms/{room}/configuration/disposition"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(intent().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    mock.finish();
}
#[tokio::test]
async fn configuration_rejects_malformed_paths_and_authority_fields_before_backend_io() {
    let (token, _) = identity();
    let mock = Mock::new(vec![]);
    let app = router(config(&mock.base));
    for (path, input) in [
        ("%FF/configuration".to_owned(), intent()),
        ("not-uuid/configuration/disposition".into(), intent()),
        (format!("{}/configuration", Uuid::new_v4()), {
            let mut v = intent();
            v["qualification"] = json!("client-claim");
            v
        }),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/rooms/{path}"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body(response).await["status"], "fail");
    }
    mock.finish();
}
