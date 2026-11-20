//! Public HTTP contracts; artwork source approval and managed Auth have separate receipts.
use super::*;
#[tokio::test]
async fn profile_art_edit_uses_original_session_exact_numbers_and_owner_ack() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    let image = Uuid::new_v4();
    let request = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let art = json!({"printingId":image,"cropX":0.10000000000000002,"cropY":0.3333333333333333,"scale":f64::MAX});
    let mut steps = vec![];
    for ack in [owner, Uuid::new_v4()] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            "/rest/v1/rpc/change_profile_art_command",
            json!({"status":"success","data":{"resourceId":ack}}),
        ));
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for expected in [StatusCode::ACCEPTED, StatusCode::SERVICE_UNAVAILABLE] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/account/profile")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"requestId":request,"expected":null,"cardArtProfile":art})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let seen = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[1].2).unwrap(),
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":request,"p_expected":null,"p_art":art})
    );
}
#[tokio::test]
async fn public_profile_read_binds_original_viewer_target_and_retired_privacy() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    let target = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let good = json!({"viewerAccountId":owner,"person":{"id":target,"username":"Deleted player","isDeleted":true},"cardArtProfile":null,"supporter":false});
    let mut bad = good.clone();
    bad["supporter"] = json!(true);
    let mut foreign = good.clone();
    foreign["person"]["id"] = json!(Uuid::new_v4());
    let mut steps = vec![];
    for data in [good, bad, foreign] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(caller_step(
            "/rest/v1/rpc/read_public_profile_art",
            &token,
            json!({"status":"success","data":data}),
        ));
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for expected in [
        StatusCode::OK,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/players/{target}/profile"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let seen = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[1].2).unwrap(),
        json!({"p_player":target})
    );
}
#[tokio::test]
async fn profile_art_requires_complete_nullable_objects_and_safe_paths() {
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    for payload in [
        json!({"requestId":Uuid::new_v4(),"cardArtProfile":null}),
        json!({"requestId":Uuid::new_v4(),"expected":null,"cardArtProfile":[]}),
        json!({"requestId":Uuid::new_v4(),"expected":null,"cardArtProfile":null,"owner":Uuid::new_v4()}),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/account/profile")
                    .header(header::AUTHORIZATION, "Bearer placeholder")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    for uri in [
        "/v1/players/%FF/profile",
        "/v1/account/profile?actorId=forged",
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
    backend.finish();
}
