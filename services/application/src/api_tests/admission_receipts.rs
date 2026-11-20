use super::*;

#[tokio::test]
async fn room_receipts_recover_original_outcomes_without_reading_current_room() {
    let room = Uuid::new_v4();
    let original = json!({"requestId":Uuid::new_v4(),"expectedRevision":"41"});
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let args = json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_room":room,"p_original":original});
    let accepted = |resource, revision| json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":resource,"revision":revision}}}});
    let unknown = json!({"status":"success","data":{"state":"unknown","response":null}});
    let conflict = json!({"status":"fail","data":{"request":"conflict"}});
    let revoked = json!({"status":"fail","data":{"session":"unauthorized"}});
    for (operation, rpc) in [
        ("ready", "read_qualified_room_ready_command"),
        ("force-start", "read_qualified_room_countdown_command"),
    ] {
        for (reply, expected) in [
            (accepted(room, "42"), StatusCode::OK),
            (accepted(room, "41"), StatusCode::OK),
            (unknown.clone(), StatusCode::OK),
            (conflict.clone(), StatusCode::CONFLICT),
            (revoked.clone(), StatusCode::UNAUTHORIZED),
            (
                accepted(Uuid::new_v4(), "42"),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (accepted(room, "43"), StatusCode::SERVICE_UNAVAILABLE),
        ] {
            let mut original_read = Step::json(&format!("/rest/v1/rpc/{rpc}"), reply.clone());
            original_read.expect = Some(serde_json::to_vec(&args).unwrap());
            let mock = Mock::new(vec![
                caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
                original_read,
            ]);
            let response = router(config(&mock.base))
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/v1/rooms/{room}/{operation}/disposition"))
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(original.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            if expected == StatusCode::OK {
                assert_eq!(body(response).await, reply);
            }
            mock.finish();
        }
    }
}

#[tokio::test]
async fn execute_room_commands_recover_originals_even_after_release_withdrawal() {
    let room = Uuid::new_v4();
    let original = json!({"requestId":Uuid::new_v4(),"expectedRevision":"41"});
    let ack = json!({"resourceId":room,"revision":"42"});
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let args = json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_room":room,"p_original":original});
    for (path, rpc) in [
        ("ready", "read_qualified_room_ready_command"),
        ("force-start", "read_qualified_room_countdown_command"),
    ] {
        for (state, expected) in [
            (
                json!({"state":"accepted","response":{"status":"success","data":ack}}),
                StatusCode::ACCEPTED,
            ),
            (
                json!({"state":"rejected","response":{"status":"fail","data":{"revision":"conflict"}}}),
                StatusCode::CONFLICT,
            ),
            (
                json!({"state":"unknown","response":null}),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let mut step = Step::json(
                &format!("/rest/v1/rpc/{rpc}"),
                json!({"status":"success","data":state}),
            );
            step.expect = Some(serde_json::to_vec(&args).unwrap());
            let mock = Mock::new(vec![
                caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
                step,
            ]);
            let response = router(config(&mock.base))
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/v1/rooms/{room}/{path}"))
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(original.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            if expected == StatusCode::ACCEPTED {
                assert_eq!(body(response).await, json!({"status":"success","data":ack}));
            }
            mock.finish();
        }
    }
}

#[tokio::test]
async fn between_game_receipts_bind_the_original_decision_and_project_the_optional_reservation() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let decision = Uuid::new_v4();
    let original = json!({"requestId":Uuid::new_v4(),"decisionId":decision,"selectedRevisionId":Uuid::new_v4(),"expectedRevision":"8"});
    let args = json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_original":original});
    let next = Uuid::new_v4();
    for (id, revision, reserved, expected) in [
        (decision, "9", json!(""), StatusCode::OK),
        (decision, "9", json!(next), StatusCode::OK),
        (decision, "9", json!(null), StatusCode::SERVICE_UNAVAILABLE),
        (
            decision,
            "9",
            json!("malformed"),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (decision, "8", json!(""), StatusCode::SERVICE_UNAVAILABLE),
        (
            Uuid::new_v4(),
            "9",
            json!(""),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
        let mut reply = json!({"status":"success","data":{"state":"accepted","response":{"status":"success",
            "data":{"resourceId":id,"revision":revision,"reservedGameId":reserved}}}});
        let mut step = Step::json(
            "/rest/v1/rpc/read_qualified_between_game_command",
            reply.clone(),
        );
        step.expect = Some(serde_json::to_vec(&args).unwrap());
        let mock = Mock::new(vec![
            caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
            step,
        ]);
        let response = router(config(&mock.base))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/between-game/replies/disposition")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(original.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            if reserved == json!("") {
                reply["data"]["response"]["data"]["reservedGameId"] = Value::Null;
            }
            assert_eq!(body(response).await, reply);
        }
        mock.finish();
    }
}
