//! Regression and contract tests for audit remediation and UI-surface completion routes.
use super::*;

#[test]
fn json_content_type_accepts_charset_and_rejects_ambiguous_headers() {
    let mut headers = HeaderMap::new();
    assert!(!is_json_content_type(&headers));
    for valid in [
        "application/json",
        "application/json; charset=utf-8",
        "APPLICATION/JSON; charset=UTF-8",
    ] {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(valid));
        assert!(is_json_content_type(&headers), "expected valid: {valid}");
    }
    for invalid in [
        "text/plain",
        "application/json, text/plain",
        "application/json; invalid_param",
        "application/json; ",
    ] {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(invalid));
        assert!(
            !is_json_content_type(&headers),
            "expected invalid: {invalid}"
        );
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.append(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    assert!(!is_json_content_type(&headers));
}

#[tokio::test]
async fn room_detail_accepts_both_camel_case_slot_limit_and_snake_case_alias() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    let room_id = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);

    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        caller_step(
            "/rest/v1/rpc/read_visible_room",
            &token,
            json!({"status": "fail", "data": {"room": "unavailable"}}),
        ),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        caller_step(
            "/rest/v1/rpc/read_visible_room",
            &token,
            json!({"status": "fail", "data": {"room": "unavailable"}}),
        ),
    ]);
    let app = router(config(&backend.base));
    for qs in ["slotLimit=100", "slot_limit=100"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/rooms/{room_id}?{qs}"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Both query spellings pass validation and reach the room lookup (404, not 400).
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "qs={qs}");
    }
    assert_eq!(backend.finish().len(), 4);
}

#[tokio::test]
async fn ui_surface_routes_relay_authenticated_rpc_and_enforce_boundaries() {
    let (token, _) = identity();
    let room_id = Uuid::new_v4();
    let account_id = Uuid::new_v4();
    let printing_id = Uuid::new_v4();

    let backend = Mock::new(vec![
        caller_step(
            "/rest/v1/rpc/read_room_game",
            &token,
            json!({
                "status": "success",
                "data": {
                    "state": "waiting",
                    "accountId": account_id,
                    "roomId": room_id,
                    "roomState": "lobby"
                }
            }),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_patreon_status",
            &token,
            json!({
                "status": "success",
                "data": {
                    "accountId": account_id,
                    "linked": true,
                    "active": true,
                    "supporter": true,
                    "tier": { "id": Uuid::new_v4(), "name": "Mythic", "rank": 3 }
                }
            }),
        ),
        caller_step(
            "/rest/v1/rpc/read_card_metadata",
            &token,
            json!({
                "status": "success",
                "data": { "printings": [], "definitions": [] }
            }),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_limited_draft",
            &token,
            json!({
                "status": "success",
                "data": {
                    "draftId": Uuid::new_v4(),
                    "step": 0,
                    "complete": false,
                    "picked": false,
                    "pack": [],
                    "pool": []
                }
            }),
        ),
        caller_step(
            "/rest/v1/rpc/read_league_leaderboard",
            &token,
            json!({
                "status": "success",
                "data": {
                    "seasonId": null,
                    "formatId": null,
                    "offset": 0,
                    "total": 0,
                    "items": []
                }
            }),
        ),
    ]);
    let app = router(config(&backend.base));

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/rooms/{room_id}/game"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body(resp).await["data"]["state"], "waiting");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/account/patreon")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body(resp).await["data"]["supporter"], true);

    // Charset parameter on Content-Type is accepted across POST routes.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/catalogue/metadata")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from(
                    json!({"printingIds": [printing_id]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/rooms/{room_id}/draft"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/league/leaderboard?limit=25")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(backend.finish().len(), 5);
}

#[tokio::test]
async fn sensitive_unauthenticated_route_is_rate_limited_per_client_ip() {
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let mut saw_429 = false;
    for _ in 0..25 {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/account/deletion/observe")
                    .header("fly-client-ip", "203.0.113.42")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        if resp.status() == StatusCode::TOO_MANY_REQUESTS {
            saw_429 = true;
            assert_eq!(
                body(resp).await,
                json!({"status": "fail", "data": {"rate": "limited"}})
            );
            break;
        }
    }
    assert!(
        saw_429,
        "expected 429 rate limit on repeated observe probes"
    );
    assert!(backend.finish().is_empty());
}
