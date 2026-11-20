//! HTTP transport tests complement real PostgreSQL and managed-realm acceptance.
use super::*;
#[tokio::test]
async fn game_chat_write_derives_subject_session_and_preserves_original_text() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    let match_id = Uuid::new_v4();
    let message = Uuid::new_v4();
    let request_id = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/send_game_message_command",
            json!({"status":"success","data":{"resourceId":message}}),
        ),
    ]);
    let response = router(config(&backend.base))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/matches/{match_id}/chat/messages"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"requestId":request_id,"body":" Hello 🎲 "}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let seen = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[1].2).unwrap(),
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":request_id,"p_match":match_id,"p_body":" Hello 🎲 "})
    );
}
#[tokio::test]
async fn notification_ack_recovery_rejects_foreign_ack_and_private_errors() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let mut steps = vec![];
    for data in [
        json!({"status":"success","data":{"state":"unknown","response":null}}),
        json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":Uuid::new_v4()}}}}),
        json!({"status":"fail","data":{"internal":"private-sentinel"}}),
        json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"request":"conflict"}}}}),
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json("/rest/v1/rpc/read_notifications_command", data));
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
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
                    .uri("/v1/notifications/read/disposition")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"requestId":Uuid::new_v4(),"notificationIds":[Uuid::new_v4()]})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert!(
            !body(response)
                .await
                .to_string()
                .contains("private-sentinel")
        );
    }
    backend.finish();
}
#[tokio::test]
async fn chat_notification_inputs_reject_invalid_paths_cursors_and_forged_fields_before_backend() {
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let match_id = Uuid::new_v4();
    for uri in [
        "/v1/matches/%FF/chat".to_owned(),
        format!("/v1/matches/{match_id}/chat?beforeId={match_id}"),
        "/v1/notifications?limit=101".to_owned(),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::AUTHORIZATION, "Bearer placeholder")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/matches/{match_id}/chat/messages"))
                .header(header::AUTHORIZATION, "Bearer placeholder")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"requestId":Uuid::new_v4(),"body":"hello","senderId":Uuid::new_v4()})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    backend.finish();
}
