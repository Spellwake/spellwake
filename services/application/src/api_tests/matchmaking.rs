//! Caller protocol fixtures; real PostgreSQL/managed Auth paths have separate qualification.
use super::*;
fn intent() -> Value {
    json!({"requestId":Uuid::new_v4(),"mode":"collection","competitive":false,"alternatives":[{"deckRevisionId":Uuid::new_v4(),"choices":[{"profileId":Uuid::new_v4(),"configurationKey":"  duel  ","maximumAiSeats":"1","maximumSponsoredAiSeats":"0"}]}]})
}
#[tokio::test]
async fn enqueue_forwards_original_consent_and_keeps_both_acknowledgment_ids() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let entry = Uuid::new_v4();
    let intent_id = Uuid::new_v4();
    let input = intent();
    let mock = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/enqueue_matchmaking_command",
            json!({"status":"success","data":{"resourceId":entry,"intentId":intent_id}}),
        ),
    ]);
    let response = router(config(&mock.base))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/queue")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        body(response).await,
        json!({"status":"success","data":{"resourceId":entry,"intentId":intent_id}})
    );
    let seen = mock.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[1].2).unwrap(),
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":input["requestId"],"p_mode":input["mode"],"p_competitive":input["competitive"],"p_alternatives":input["alternatives"]})
    );
}
#[tokio::test]
async fn enqueue_recovery_requires_both_ids_and_filters_unknown_private_failures() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let mut steps = vec![];
    let responses = [
        json!({"status":"success","data":{"state":"unknown","response":null}}),
        json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":Uuid::new_v4()}}}}),
        json!({"status":"fail","data":{"intent":"invalid envelope"}}),
        json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"provider":"private-sentinel"}}}}),
    ];
    for response in responses {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            "/rest/v1/rpc/read_matchmaking_command",
            response,
        ));
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    for expected in [
        StatusCode::OK,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::CONFLICT,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/queue/disposition")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(intent().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(
            !body(response)
                .await
                .to_string()
                .contains("private-sentinel")
        );
    }
    mock.finish();
}
#[tokio::test]
async fn profile_configuration_read_binds_original_bearer_target_and_ordinal_page() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let profile = Uuid::new_v4();
    let format = Uuid::new_v4();
    let first = json!({"accountId":owner,"profileId":profile,"items":[{"index":"1","key":"duel","formatId":format,"seatCount":"2","structure":"bo1"}],"nextIndex":"1"});
    let mut bad = first.clone();
    bad["items"][0]["index"] = json!("2");
    bad["nextIndex"] = json!("2");
    let mock = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        caller_step(
            "/rest/v1/rpc/read_table_profile_configurations",
            &token,
            json!({"status":"success","data":first}),
        ),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        caller_step(
            "/rest/v1/rpc/read_table_profile_configurations",
            &token,
            json!({"status":"success","data":bad}),
        ),
    ]);
    let app = router(config(&mock.base));
    for expected in [StatusCode::OK, StatusCode::SERVICE_UNAVAILABLE] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/table-profiles/{profile}/configurations?afterIndex=0&limit=1"
                    ))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let seen = mock.finish();
    for request in seen.iter().skip(1).step_by(2) {
        assert_eq!(
            serde_json::from_slice::<Value>(&request.2).unwrap(),
            json!({"p_profile":profile,"p_after":0,"p_limit":1})
        );
    }
}
#[tokio::test]
async fn queue_intake_rejects_roster_authority_open_ranked_duplicate_decks_and_missing_nulls_before_io()
 {
    let (token, _) = identity();
    let mock = Mock::new(vec![]);
    let app = router(config(&mock.base));
    let mut roster = intent();
    roster["humanSeats"] = json!([]);
    let mut ranked = intent();
    ranked["mode"] = json!("open");
    ranked["competitive"] = json!(true);
    let mut duplicate = intent();
    let original = duplicate["alternatives"][0].clone();
    duplicate["alternatives"]
        .as_array_mut()
        .unwrap()
        .push(original);
    for input in [roster, ranked, duplicate] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/queue")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/table-profiles/%FF/configurations")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body(response).await["status"], "fail");
    mock.finish();
}
