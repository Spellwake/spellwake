//! Router-level ambiguous content-type rejection, without upstream work.
use super::*;

// Every registered JSON POST, including exact-command recovery endpoints.
pub(super) const JSON_POST_ROUTES: &[&str] = &[
    "/v1/games/{game}/receipts",
    "/v1/games/{game}/view",
    "/v1/matches/{match}/ante",
    "/v1/seasons/{season}/ballot",
    "/v1/rooms/{room}/ai",
    "/v1/matches/{match}/ante/disposition",
    "/v1/ante/{commitment}/accept",
    "/v1/ante/{commitment}/accept/disposition",
    "/v1/ante/{commitment}/cancel",
    "/v1/ante/{commitment}/cancel/disposition",
    "/v1/seasons/{season}/ballot/disposition",
    "/v1/rooms/{room}/ai/disposition",
    "/v1/ai-slots/{slot}/bootstrap",
    "/v1/ai-slots/{slot}/bootstrap/disposition",
    "/v1/ai-slots/{slot}/remove",
    "/v1/ai-slots/{slot}/remove/disposition",
    "/v1/between-game/replies/disposition",
    "/v1/rooms/{room}/ready",
    "/v1/rooms/{room}/force-start",
    "/v1/rooms/{room}/ready/disposition",
    "/v1/rooms/{room}/force-start/disposition",
    "/v1/rooms/{room}/unready",
    "/v1/rooms/{room}/unready/disposition",
    "/v1/quests/reroll",
    "/v1/quests/reroll/disposition",
    "/v1/account",
    "/v1/account/patreon/start",
    "/v1/account/patreon/complete",
    "/v1/account/patreon/status",
    "/v1/economy/commands",
    "/v1/economy/commands/disposition",
    "/v1/rooms/{room}/configuration",
    "/v1/rooms/{room}/configuration/disposition",
    "/v1/rooms/{room}/invitations",
    "/v1/rooms/{room}/invitations/disposition",
    "/v1/rooms/{room}/join",
    "/v1/rooms/{room}/join/disposition",
    "/v1/invitations/{invitation}/revoke",
    "/v1/invitations/{invitation}/revoke/disposition",
    "/v1/queue",
    "/v1/queue/disposition",
    "/v1/queue/{entry}/cancel",
    "/v1/queue/{entry}/cancel/disposition",
    "/v1/matches/{match}/chat/messages",
    "/v1/matches/{match}/chat/messages/disposition",
    "/v1/notifications/read",
    "/v1/notifications/read/disposition",
    "/v1/rooms/commands",
    "/v1/rooms/commands/disposition",
    "/v1/social/commands",
    "/v1/social/read",
    "/v1/social/read/disposition",
    "/v1/social/commands/disposition",
    "/v1/account/deletion",
    "/v1/account/deletion/observe",
    "/v1/decks/import",
    "/v1/decks/import/disposition",
    "/v1/decks",
    "/v1/decks/{deck}/revisions",
    "/v1/decks/{deck}/settings",
    "/v1/account/profile",
    "/v1/account/profile/disposition",
    "/v1/account/defaults",
    "/v1/account/defaults/disposition",
    "/v1/decks/commands/disposition",
    "/v1/predictions/printings/{printing}/source",
    "/v1/catalogue/resolve",
    "/v1/predictions/commands",
    "/v1/predictions/commands/disposition",
    "/v1/purchases",
];

#[tokio::test]
async fn duplicate_content_types_reject_every_json_post_before_upstream_work() {
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let (token, _) = identity();
    let id = "11111111-1111-1111-1111-111111111111";
    for route in JSON_POST_ROUTES {
        let mut path = (*route).to_owned();
        while let Some(start) = path.find('{') {
            let end = path[start..].find('}').unwrap() + start;
            path.replace_range(start..=end, id);
        }
        let observe = route.ends_with("/deletion/observe");
        let input = if observe {
            json!({"accountId":id,"requestId":id,"statusKey":URL_SAFE_NO_PAD.encode([0_u8;32])})
                .to_string()
        } else {
            "{}".to_owned()
        };
        for (first, second) in [
            ("application/json", "application/json"),
            ("application/json", "text/plain"),
            ("text/plain", "application/json"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(&path)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .header(header::CONTENT_TYPE, first)
                        .header(header::CONTENT_TYPE, second)
                        .body(Body::from(input.clone()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if observe {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::BAD_REQUEST
                },
                "{route}: {first}, {second}"
            );
            assert_eq!(
                body(response).await,
                if observe {
                    json!({"status":"fail","data":{"observer":"unavailable"}})
                } else {
                    json!({"status":"fail","data":{"body":"json_required"}})
                },
                "{route}: {first}, {second}"
            );
        }
    }
    assert!(backend.finish().is_empty());
}

#[test]
fn singleton_content_type_preserves_values_and_rejects_ambiguity() {
    let mut headers = HeaderMap::new();
    assert!(single_content_type(&headers).is_none());
    for value in [
        "application/json",
        "application/json; charset=utf-8",
        "text/plain",
        "application/json, text/plain",
    ] {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(value));
        assert_eq!(single_content_type(&headers).unwrap(), value);
    }
    headers.append(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    assert!(single_content_type(&headers).is_none());
}
