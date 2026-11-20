//! Original-session transport; actual matching, hold and revocation semantics live in DB tests.
use super::*;
#[tokio::test]
async fn prediction_commands_bind_original_session_and_exact_intent() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let target = Uuid::new_v4();
    let request = Uuid::new_v4();
    let inputs = [
        json!({"kind":"place_order","command":{"requestId":request,"marketId":target,"side":"yes","limitCost":"60","quantity":"9007199254740993"}}),
        json!({"kind":"cancel_order","command":{"requestId":request,"orderId":target}}),
    ];
    let mut steps = Vec::new();
    for _ in &inputs {
        for lookup in [false, true] {
            steps.push(caller_step(
                "/rest/v1/rpc/read_current_session",
                &token,
                session.clone(),
            ));
            steps.push(Step::json(if lookup{"/rest/v1/rpc/read_prediction_command"}else{"/rest/v1/rpc/execute_prediction_command"},if lookup{json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":target}}}})}else{json!({"status":"success","data":{"resourceId":target}})}));
        }
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for input in &inputs {
        for lookup in [false, true] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(if lookup {
                            "/v1/predictions/commands/disposition"
                        } else {
                            "/v1/predictions/commands"
                        })
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(serde_json::to_vec(input).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if lookup {
                    StatusCode::OK
                } else {
                    StatusCode::ACCEPTED
                }
            );
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
    }
    let seen = backend.finish();
    for (index, input) in inputs.iter().enumerate() {
        for lookup in 0..2 {
            assert_eq!(
                serde_json::from_slice::<Value>(&seen[index * 4 + lookup * 2 + 1].2).unwrap(),
                json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_input":input})
            );
        }
    }
}
#[tokio::test]
async fn prediction_commands_reject_actor_injection_overflow_and_foreign_cancel_ack() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let target = Uuid::new_v4();
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        Step::json(
            "/rest/v1/rpc/execute_prediction_command",
            json!({"status":"success","data":{"resourceId":Uuid::new_v4()}}),
        ),
    ]);
    let app = router(config(&backend.base));
    let base = json!({"kind":"place_order","command":{"requestId":Uuid::new_v4(),"marketId":target,"side":"yes","limitCost":"60","quantity":"1"}});
    let mut injected = base.clone();
    injected["command"]["actorId"] = json!(target);
    let mut overflow = base.clone();
    overflow["command"]["quantity"] = json!("9".repeat(78));
    for (input, status) in [
        (injected, StatusCode::BAD_REQUEST),
        (overflow, StatusCode::BAD_REQUEST),
        (
            json!({"kind":"cancel_order","command":{"requestId":Uuid::new_v4(),"orderId":target}}),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/predictions/commands")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    backend.finish();
}
