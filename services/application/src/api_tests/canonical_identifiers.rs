//! Exercise the actual router's lexical gates without any authenticated backend activity.
use super::*;
const CANONICAL: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
#[tokio::test]
async fn every_legacy_identifier_path_rejects_aliases_and_utf8_errors_as_private_json() {
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let cases = [
        ("GET", "/v1/purchases/ID", "purchase"),
        ("GET", "/v1/decks/ID", "deck"),
        ("POST", "/v1/decks/ID/revisions", "deck"),
        ("POST", "/v1/decks/ID/settings", "deck"),
        ("GET", "/v1/cards/ID/printings", "definition"),
        ("GET", "/v1/trades/ID", "trade"),
        ("GET", "/v1/market/listings/ID", "listing"),
        ("GET", "/v1/packs/openings/ID", "pack"),
        ("GET", "/v1/packs/products/ID/quote", "product"),
        ("GET", "/v1/predictions/markets/ID", "market"),
        ("GET", "/v1/predictions/printings/ID/source", "printing"),
        ("POST", "/v1/predictions/printings/ID/source", "printing"),
        (
            "GET",
            "/v1/social/conversations/ID/messages",
            "conversation",
        ),
    ];
    for bad in [
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
        "aaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa",
        "urn:uuid:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "%7Baaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa%7D",
        "00000000-0000-0000-0000-000000000000",
        "%FF",
    ] {
        for (method, template, field) in cases {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(template.replace("ID", bad))
                        .header(header::AUTHORIZATION, "Bearer placeholder")
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{template} {bad}"
            );
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let value = body(response).await;
            assert_eq!(value["status"], "fail");
            assert_eq!(value["data"][field], "invalid", "{template} {bad}");
        }
    }
    backend.finish();
}
#[tokio::test]
async fn all_public_uuid_query_fields_keep_omission_and_reject_aliases_before_authority() {
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let id = CANONICAL;
    let bases = vec![
        ("/v1/inventory".to_owned(), "after"),
        ("/v1/cards?query=Island".to_owned(), "after"),
        ("/v1/cards/printings?name=Island".to_owned(), "after"),
        ("/v1/sets".to_owned(), "after"),
        ("/v1/trades".to_owned(), "after"),
        ("/v1/market/listings".to_owned(), "after"),
        ("/v1/packs/products".to_owned(), "after"),
        ("/v1/predictions/orders".to_owned(), "after"),
        ("/v1/predictions/orders".to_owned(), "marketId"),
        ("/v1/social/people?query=Island".to_owned(), "after"),
        ("/v1/social/status".to_owned(), "personId"),
        (
            format!("/v1/social/conversations/{id}/messages?beforeAt=2026-11-04T00%3A00%3A00Z"),
            "beforeId",
        ),
        ("/v1/quests".to_owned(), "after"),
        ("/v1/ranks".to_owned(), "after"),
        ("/v1/ranks".to_owned(), "formatId"),
        ("/v1/ranks".to_owned(), "seasonId"),
        ("/v1/rooms".to_owned(), "after"),
        (format!("/v1/rooms/{id}"), "afterSlot"),
        ("/v1/invitations".to_owned(), "roomId"),
        ("/v1/invitations".to_owned(), "after"),
        ("/v1/table-profiles".to_owned(), "after"),
        ("/v1/queue".to_owned(), "intentId"),
        ("/v1/queue".to_owned(), "after"),
        (
            format!("/v1/matches/{id}/chat?beforeAt=2026-11-04T00%3A00%3A00Z"),
            "beforeId",
        ),
        ("/v1/notifications".to_owned(), "after"),
    ];
    assert_eq!(bases.len(), 25);
    for (base, key) in bases {
        for bad in [
            "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
            "aaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa",
            "urn:uuid:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "",
        ] {
            let sep = if base.contains('?') { '&' } else { '?' };
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("{base}{sep}{key}={bad}"))
                        .header(header::AUTHORIZATION, "Bearer placeholder")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{base} {key} {bad}"
            );
            assert_eq!(body(response).await["data"]["query"], "invalid");
        }
        // Paired cursors require both fields; omit both for the default-page positive.
        let omitted = if key == "beforeId" {
            base.split('?').next().unwrap_or(&base)
        } else {
            &base
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(omitted)
                    .header(header::AUTHORIZATION, "Bearer placeholder")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "omission: {base} {key}"
        );
    }
    backend.finish();
}
#[tokio::test]
async fn canonical_non_rfc_paths_and_queries_pass_lexical_gates_and_text_is_opaque() {
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    for uri in [
        "/v1/purchases/11111111-1111-1111-1111-111111111111",
        "/v1/inventory?after=aaaaaaaa-aaaa-faaa-aaaa-aaaaaaaaaaaa",
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
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let input = json!({"requestId":CANONICAL,"formatId":CANONICAL,"name":CANONICAL.to_uppercase(),"mode":"open","entries":[]});
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/decks/import")
                .header(header::AUTHORIZATION, "Bearer placeholder")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    backend.finish();
}
#[tokio::test]
async fn upstream_aliases_are_rejected_without_rewriting_opaque_notification_references() {
    let (token, mut session) = identity();
    let owner = Uuid::parse_str("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb").unwrap();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let page = json!({"accountId":owner,"unreadCount":"1","items":[{"id":CANONICAL,"sourceKind":"future-kind","sourceRef":CANONICAL.to_uppercase(),"body":"Reference text","readAt":null}],"nextCursor":CANONICAL});
    let mut alias_item = page.clone();
    alias_item["items"][0]["id"] = json!(CANONICAL.to_uppercase());
    let mut alias_cursor = page.clone();
    alias_cursor["nextCursor"] = json!(CANONICAL.to_uppercase());
    let mut alias_owner = page.clone();
    alias_owner["accountId"] = json!(owner.to_string().to_uppercase());
    let mut steps = vec![];
    for data in [alias_item, alias_cursor, alias_owner, page] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(caller_step(
            "/rest/v1/rpc/read_my_notifications",
            &token,
            json!({"status":"success","data":data}),
        ));
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for expected in [
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::OK,
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/notifications?limit=1")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        if expected == StatusCode::OK {
            assert_eq!(
                body(response).await["data"]["items"][0]["sourceRef"],
                CANONICAL.to_uppercase()
            );
        }
    }
    backend.finish();
}
