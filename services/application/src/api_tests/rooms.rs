//! Room protocol tests supplement canonical real-Postgres transaction checks.
use super::*;

#[tokio::test]
async fn room_commands_forward_only_verified_session_and_exact_original_terms() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let room = Uuid::new_v4();
    let request_id = Uuid::new_v4();
    let input = json!({"kind":"select_deck","command":{"requestId":request_id,"roomId":room,"expectedRevision":"9007199254740993","deckRevisionId":null}});
    let mock = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/select_room_deck_command",
            json!({"status":"success","data":{"resourceId":room}}),
        ),
    ]);
    let response = router(config(&mock.base))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/rooms/commands")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let seen = mock.finish();
    let sent: Value = serde_json::from_slice(&seen[1].2).unwrap();
    assert_eq!(
        sent,
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":request_id,"p_room":room,"p_expected":"9007199254740993","p_revision":null})
    );
}
#[tokio::test]
async fn room_lookup_preserves_original_intent_and_checks_accepted_target_and_safe_failures() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let room = Uuid::new_v4();
    let input = json!({"kind":"leave","command":{"requestId":Uuid::new_v4(),"roomId":room,"expectedRevision":"1"}});
    let mut steps = vec![];
    for data in [
        json!({"state":"unknown","response":null}),
        json!({"state":"accepted","response":{"status":"success","data":{"resourceId":room}}}),
        json!({"state":"accepted","response":{"status":"success","data":{"resourceId":Uuid::new_v4()}}}),
        json!({"state":"rejected","response":{"status":"fail","data":{"queue":"already reserved"}}}),
        json!({"state":"rejected","response":{"status":"fail","data":{"diagnostic":"private-secret"}}}),
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            "/rest/v1/rpc/read_room_command",
            json!({"status":"success","data":data}),
        ));
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    for expected in [
        StatusCode::OK,
        StatusCode::OK,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::OK,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/rooms/commands/disposition")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(!body(response).await.to_string().contains("private-secret"));
    }
    let seen = mock.finish();
    for request in seen.iter().skip(1).step_by(2) {
        let sent: Value = serde_json::from_slice(&request.2).unwrap();
        assert_eq!(
            sent,
            json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_input":input})
        );
    }
}
#[tokio::test]
async fn room_intents_reject_missing_nullable_revision_duplicate_keys_and_authority_fields_before_io()
 {
    let mock = Mock::new(vec![]);
    let app = router(config(&mock.base));
    let (token, _) = identity();
    let input = json!({"kind":"select_deck","command":{"requestId":Uuid::new_v4(),"roomId":Uuid::new_v4(),"expectedRevision":"0","deckRevisionId":null}});
    let mut missing = input.clone();
    missing["command"]
        .as_object_mut()
        .unwrap()
        .remove("deckRevisionId");
    let mut injected = input.clone();
    injected["command"]["actorId"] = json!(Uuid::new_v4());
    let mut numeric = input.clone();
    numeric["command"]["expectedRevision"] = json!(1);
    let duplicate = input.to_string().replacen(
        "\"expectedRevision\":\"0\"",
        "\"expectedRevision\":\"0\",\"expectedRevision\":\"0\"",
        1,
    );
    for wire in [
        missing.to_string(),
        injected.to_string(),
        numeric.to_string(),
        duplicate,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/rooms/commands")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(wire))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    mock.finish();
}

#[tokio::test]
async fn room_detail_forwards_bearer_target_and_original_slot_page_and_rejects_foreign_selection() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let room = Uuid::new_v4();
    let after = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let detail = json!({"accountId":owner,"room":{"id":room,"hostId":owner,"formatId":Uuid::new_v4(),"profileId":Uuid::new_v4(),"mode":"open","competitive":false,"visibility":"public","state":"lobby","revision":"0","structure":"bo1","configurationId":Uuid::new_v4(),"targetSeatCount":null,"countdownAt":null},"slots":{"items":[{"id":Uuid::new_v4(),"accountId":owner,"seatIndex":"7","controllerKind":"human","ready":false,"deckRevisionId":Uuid::new_v4()}],"nextCursor":null}});
    let mut private = detail.clone();
    private["slots"]["items"][0]["accountId"] = json!(Uuid::new_v4());
    let mock = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        caller_step(
            "/rest/v1/rpc/read_visible_room",
            &token,
            json!({"status":"success","data":detail}),
        ),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        caller_step(
            "/rest/v1/rpc/read_visible_room",
            &token,
            json!({"status":"success","data":private}),
        ),
    ]);
    let app = router(config(&mock.base));
    for expected in [StatusCode::OK, StatusCode::SERVICE_UNAVAILABLE] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/rooms/{room}?afterSlot={after}&slotLimit=100"))
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
    for request in seen.iter().skip(1).step_by(2) {
        assert_eq!(
            serde_json::from_slice::<Value>(&request.2).unwrap(),
            json!({"p_room":room,"p_after":after,"p_limit":100})
        );
    }
}

#[tokio::test]
async fn malformed_room_path_uses_standard_fail_without_backend_io() {
    let mock = Mock::new(vec![]);
    let app = router(config(&mock.base));
    for raw in [
        "%FF",
        "not-a-uuid",
        "00000000-0000-0000-0000-000000000000",
        "ABCDEFAB-CDEF-ABCD-EFAB-CDEFABCDEFAB",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/rooms/{raw}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            body(response).await,
            json!({"status":"fail","data":{"room":"invalid"}})
        );
    }
    mock.finish();
}

#[tokio::test]
async fn retained_dependency_cause_stays_out_of_public_http_errors() {
    let (token, _) = identity();
    let mock = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_current_session",
        &token,
        json!("private-dependency-sentinel"),
    )]);
    let response = router(config(&mock.base))
        .oneshot(
            Request::builder()
                .uri("/v1/session")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        body(response).await,
        json!({"status":"error","message":"Application service is temporarily unavailable","code":"unavailable"})
    );
    mock.finish();
}
