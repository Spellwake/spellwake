//! HTTP fixtures verify transport parsing/context; real SQL/managed tests are separate.
use super::*;

async fn send(app: &Router, token: &str, lookup: bool, raw: String) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(if lookup {
                    "/v1/quests/reroll/disposition"
                } else {
                    "/v1/quests/reroll"
                })
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(raw))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn reroll_forwards_only_original_intent_and_verified_identity_for_execute_and_recovery() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let intent = json!({"requestId":Uuid::new_v4(),"assignmentId":Uuid::new_v4()});
    let ack = json!({"resourceId":Uuid::new_v4(),"replacedId":intent["assignmentId"]});
    let expected = json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":intent["requestId"],"p_assignment":intent["assignmentId"]});
    let mut steps = vec![];
    for (lookup, result) in [
        (false, json!({"status":"success","data":ack})),
        (
            true,
            json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":ack}}}),
        ),
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        let mut step = Step::json(
            if lookup {
                "/rest/v1/rpc/read_quest_reroll_command"
            } else {
                "/rest/v1/rpc/reroll_quest_command"
            },
            result,
        );
        step.expect = Some(serde_json::to_vec(&expected).unwrap());
        steps.push(step);
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    for lookup in [false, true] {
        let response = send(&app, &token, lookup, intent.to_string()).await;
        assert_eq!(
            response.status(),
            if lookup {
                StatusCode::OK
            } else {
                StatusCode::ACCEPTED
            }
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let value = body(response).await;
        assert_eq!(
            if lookup {
                &value["data"]["response"]["data"]
            } else {
                &value["data"]
            },
            &ack
        );
    }
    assert_eq!(mock.finish().len(), 4);
}

#[tokio::test]
async fn reroll_rejects_untrusted_selection_aliases_duplicates_and_body_budgets_before_upstream() {
    let (token, _) = identity();
    let mock = Mock::new(vec![]);
    let app = router(config(&mock.base));
    let intent = json!({"requestId":Uuid::new_v4(),"assignmentId":Uuid::new_v4()});
    let mut cases = vec![
        json!([intent["requestId"], intent["assignmentId"]]).to_string(),
        "[]".into(),
        "null".into(),
        " ".repeat(32769),
        format!(
            "{{\"requestId\":{0},\"requestId\":{0},\"assignmentId\":{1}}}",
            intent["requestId"], intent["assignmentId"]
        ),
    ];
    for field in [
        "actorId",
        "accountId",
        "definitionId",
        "day",
        "rewardCoins",
        "policy",
    ] {
        let mut v = intent.clone();
        v[field] = json!("untrusted");
        cases.push(v.to_string());
    }
    for value in [
        Uuid::nil().to_string(),
        Uuid::new_v4().simple().to_string(),
        "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA".to_owned(),
    ] {
        let mut v = intent.clone();
        v["assignmentId"] = json!(value);
        cases.push(v.to_string());
    }
    for lookup in [false, true] {
        for raw in &cases {
            assert_eq!(
                send(&app, &token, lookup, raw.clone()).await.status(),
                StatusCode::BAD_REQUEST
            );
        }
    }
    for (query, authorization, content_type, expected) in [
        ("?actor=other", true, "application/json", 400),
        ("", false, "application/json", 401),
        ("", true, "text/plain", 400),
    ] {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("/v1/quests/reroll{query}"));
        if authorization {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = app
            .clone()
            .oneshot(
                builder
                    .header(header::CONTENT_TYPE, content_type)
                    .body(Body::from(intent.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), expected);
    }
    assert!(mock.finish().is_empty());
}

#[tokio::test]
async fn reroll_rejects_wrong_targets_variants_positional_records_and_undeclared_failures() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let intent = json!({"requestId":Uuid::new_v4(),"assignmentId":Uuid::new_v4()});
    let good = json!({"resourceId":Uuid::new_v4(),"replacedId":intent["assignmentId"]});
    let mut wrong = good.clone();
    wrong["replacedId"] = json!(Uuid::new_v4());
    let cases = [
        (false, json!({"status":"success","data":wrong})),
        (
            false,
            json!({"status":"success","data":{"resourceId":intent["assignmentId"],"replacedId":intent["assignmentId"]}}),
        ),
        (
            false,
            json!({"status":"success","data":[good["resourceId"],good["replacedId"]]}),
        ),
        (
            false,
            json!({"status":"success","data":{"state":"unknown","response":null}}),
        ),
        (true, json!({"status":"success","data":good})),
        (
            true,
            json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":wrong}}}),
        ),
        (
            true,
            json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"definition":"please trust this"}}}}),
        ),
        (
            false,
            json!({"status":"fail","data":{"assignment":"unavailable","request":"conflict"}}),
        ),
        (
            false,
            json!({"status":"error","message":"private source diagnostic"}),
        ),
    ];
    let mut steps = vec![];
    for (lookup, value) in &cases {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            if *lookup {
                "/rest/v1/rpc/read_quest_reroll_command"
            } else {
                "/rest/v1/rpc/reroll_quest_command"
            },
            value.clone(),
        ));
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    for (lookup, _) in cases {
        let response = send(&app, &token, lookup, intent.to_string()).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            !body(response)
                .await
                .to_string()
                .contains("private source diagnostic")
        );
    }
    assert_eq!(mock.finish().len(), 18);
}

#[tokio::test]
async fn reroll_keeps_unknown_recovery_distinct_from_expired_session_and_persisted_rejection() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("deletion_pending");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let intent = json!({"requestId":Uuid::new_v4(),"assignmentId":Uuid::new_v4()});
    let cases = [
        (
            true,
            json!({"status":"success","data":{"state":"unknown","response":null}}),
            200,
        ),
        (
            true,
            json!({"status":"success","data":{"state":"pending","response":null}}),
            200,
        ),
        (
            true,
            json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"reroll":"already used"}}}}),
            200,
        ),
        (
            false,
            json!({"status":"fail","data":{"account":"unavailable"}}),
            409,
        ),
        (
            false,
            json!({"status":"fail","data":{"session":"unauthorized"}}),
            401,
        ),
        (
            false,
            json!({"status":"fail","data":{"request":"conflict"}}),
            409,
        ),
    ];
    let mut steps = vec![];
    for (lookup, value, _) in &cases {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            if *lookup {
                "/rest/v1/rpc/read_quest_reroll_command"
            } else {
                "/rest/v1/rpc/reroll_quest_command"
            },
            value.clone(),
        ));
    }
    let mock = Mock::new(steps);
    let app = router(config(&mock.base));
    for (lookup, value, status) in cases {
        let response = send(&app, &token, lookup, intent.to_string()).await;
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(body(response).await, value);
    }
    assert_eq!(mock.finish().len(), 12);
}
