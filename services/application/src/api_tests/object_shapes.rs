//! Full positional arrays must fail before Auth, writes, or provider requests.
use super::*;

#[tokio::test]
async fn consumed_json_ambiguous_failure_fields_cannot_become_canonical_rejections() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let mut steps = Vec::new();
    for raw in [
        r#"{"status":"fail","data":{"listing":"listing unavailable"}}"#,
        r#"{"status":"fail","data":{"listing":"listing unavailable","listing":"listing unavailable"}}"#,
        r#"{"status":"fail","data":{"\u006cisting":"listing unavailable","listing":"listing unavailable"}}"#,
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        let mut step = Step::json("/rest/v1/rpc/execute_economy_command", Value::Null);
        step.body = raw.as_bytes().to_vec();
        steps.push(step);
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for expected in [
        StatusCode::CONFLICT,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let input = json!({"kind":"listing_cancel","command":{"requestId":Uuid::new_v4(),"listingId":Uuid::new_v4()}});
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/economy/commands")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    assert_eq!(backend.finish().len(), 6);
}

#[tokio::test]
async fn command_objects_reject_complete_positional_arrays_without_network_effects() {
    let (token, _) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let request = Uuid::new_v4();
    let target = Uuid::new_v4();
    let appearance = json!({"backId":target,"sleeveId":target,"diceId":target,"playmatId":null});
    let settings = json!({"name":"Deck","mode":"open","formatId":target,"appearance":appearance});
    let key = URL_SAFE_NO_PAD.encode([7; 32]);
    let cases = vec![
        ("/v1/account".into(), json!(["player_name"])),
        ("/v1/purchases".into(), json!([request, target])),
        ("/v1/decks".into(), json!([request, "Deck", "open", target])),
        (
            format!("/v1/decks/{target}/revisions"),
            json!([request, target, null, []]),
        ),
        (
            format!("/v1/decks/{target}/settings"),
            json!([request, target, settings, settings]),
        ),
        (
            "/v1/account/defaults".into(),
            json!([request, appearance, appearance]),
        ),
        ("/v1/catalogue/resolve".into(), json!([[target], []])),
        ("/v1/social/read".into(), json!([request, target, [target]])),
        (
            "/v1/account/deletion".into(),
            json!([request, "a".repeat(64)]),
        ),
        ("/v1/account/patreon/start".into(), json!([request, key])),
        ("/v1/account/patreon/status".into(), json!([request, key])),
        (
            "/v1/account/patreon/complete".into(),
            json!([request, key, "code"]),
        ),
        // Both the outer enum and its nested payload have object contracts.
        (
            "/v1/social/commands".into(),
            json!(["block",{"requestId":request,"otherAccountId":target,"blocked":true}]),
        ),
        (
            "/v1/social/commands".into(),
            json!({"kind":"block","command":[request,target,true]}),
        ),
        (
            "/v1/social/commands/disposition".into(),
            json!({"kind":"friendship","command":[request,target,"request"]}),
        ),
        (
            "/v1/economy/commands".into(),
            json!({"kind":"listing_buy","command":[request,target,"1","10"]}),
        ),
        (
            "/v1/economy/commands/disposition".into(),
            json!({"kind":"listing_cancel","command":[request,target]}),
        ),
        (
            "/v1/predictions/commands".into(),
            json!({"kind":"place_order","command":[request,target,"yes","50","1"]}),
        ),
        (
            "/v1/predictions/commands/disposition".into(),
            json!({"kind":"cancel_order","command":[request,target]}),
        ),
        (
            "/v1/decks/commands/disposition".into(),
            json!({"kind":"create","command":[request,"Deck","open",target]}),
        ),
        // Nested entries, settings, appearances and asset lines are also objects.
        (
            format!("/v1/decks/{target}/revisions"),
            json!({"requestId":request,"deckId":target,"expectedRevisionId":null,"entries":[[target,null,"main","1"]]}),
        ),
        (
            format!("/v1/decks/{target}/settings"),
            json!({"requestId":request,"deckId":target,"expected":["Deck","open",target,appearance],"settings":settings}),
        ),
        (
            "/v1/account/defaults".into(),
            json!({"requestId":request,"expected":appearance,"appearance":[target,target,target,null]}),
        ),
        (
            "/v1/economy/commands".into(),
            json!({"kind":"listing_create","command":{"requestId":request,"lot":[target,"1"],"totalPrice":"10"}}),
        ),
        (
            "/v1/economy/commands".into(),
            json!({"kind":"trade_create","command":{"requestId":request,"recipientId":target,"give":[[target,"1"]],"request":[]}}),
        ),
    ];
    for (path, value) in cases {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(&path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&value).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{path}: {value}"
        );
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/account/deletion/observe")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!([target, request, key]).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert!(backend.finish().is_empty());
}

#[test]
fn object_decoding_preserves_duplicates_required_nulls_and_genuine_lists() {
    use crate::{
        commands::{CommandAck, MarkDirectRead},
        decks::SaveDeck,
    };
    let id = Uuid::new_v4();
    let draft = json!({"requestId":id,"deckId":id,"expectedRevisionId":null,
        "entries":[{"definitionId":id,"printingId":null,"section":"main","quantity":"1"}]});
    assert!(parse_object::<SaveDeck>(draft.to_string().as_bytes()).is_ok());
    let mut missing = draft.clone();
    missing
        .as_object_mut()
        .unwrap()
        .remove("expectedRevisionId");
    assert!(parse_object::<SaveDeck>(missing.to_string().as_bytes()).is_err());
    let mut missing_entry = draft.clone();
    missing_entry["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("printingId");
    assert!(parse_object::<SaveDeck>(missing_entry.to_string().as_bytes()).is_err());
    let duplicate = format!(
        r#"{{"requestId":"{id}","deckId":"{id}","expectedRevisionId":null,"entries":[{{"definitionId":"{id}","definitionId":"{id}","printingId":null,"section":"main","quantity":"1"}}]}}"#
    );
    assert!(parse_object::<SaveDeck>(duplicate.as_bytes()).is_err());
    let duplicate = format!(
        r#"{{"requestId":"{id}","requestId":"{id}","deckId":"{id}","expectedRevisionId":null,"entries":[]}}"#
    );
    assert!(parse_object::<SaveDeck>(duplicate.as_bytes()).is_err());
    assert!(
        parse_object::<MarkDirectRead>(
            json!({"requestId":id,"conversationId":id,"messageIds":[id]})
                .to_string()
                .as_bytes()
        )
        .is_ok()
    );
    assert!(serde_json::from_value::<CommandAck>(json!([id])).is_err());
    assert!(serde_json::from_value::<CommandAck>(json!({"resourceId":id})).is_ok());
    assert!(
        serde_json::from_str::<CommandAck>(&format!(
            r#"{{"resourceId":"{id}","resourceId":"{id}"}}"#
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<crate::commands::CommandDisposition>(
            json!({"state":"accepted","response":{"status":"success","data":[id]}})
        )
        .is_err()
    );
}

#[tokio::test]
async fn positional_acknowledgment_cannot_confirm_a_command() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let listing = Uuid::new_v4();
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        Step::json(
            "/rest/v1/rpc/execute_economy_command",
            json!({"status":"success","data":[listing]}),
        ),
    ]);
    let response = router(config(&backend.base)).oneshot(Request::builder().method("POST")
        .uri("/v1/economy/commands")
        .header(header::AUTHORIZATION,format!("Bearer {token}"))
        .header(header::CONTENT_TYPE,"application/json")
        .body(Body::from(json!({"kind":"listing_buy","command":{"requestId":Uuid::new_v4(),"listingId":listing,"expectedRevision":"1","expectedTotal":"10"}}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(backend.finish().len(), 2);
}

#[tokio::test]
async fn raw_duplicate_acknowledgments_are_rejected_by_execute_and_recovery_routes() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let request = Uuid::new_v4();
    let appearance = json!({"backId":owner,"sleeveId":owner,"diceId":owner,"playmatId":null});
    let economy =
        json!({"kind":"listing_cancel","command":{"requestId":request,"listingId":owner}});
    let profile = json!({"requestId":request,"expected":appearance,"appearance":appearance});
    let routes = [
        (
            "/v1/economy/commands",
            "execute_economy_command",
            economy.clone(),
            false,
        ),
        (
            "/v1/economy/commands/disposition",
            "read_economy_command",
            economy,
            true,
        ),
        (
            "/v1/account/defaults",
            "change_profile_defaults_command",
            profile.clone(),
            false,
        ),
        (
            "/v1/account/defaults/disposition",
            "read_profile_defaults_command",
            profile,
            true,
        ),
        (
            "/v1/account/deletion",
            "request_account_deletion_command",
            json!({"requestId":request,"statusKeyHash":"a".repeat(64)}),
            false,
        ),
    ];
    let mut steps = Vec::new();
    for (_, rpc, _, lookup) in &routes {
        let ack = json!({"resourceId":owner});
        let data = if *lookup {
            json!({"state":"accepted","response":{"status":"success","data":ack}})
        } else {
            ack
        };
        let valid = json!({"status":"success","data":data}).to_string();
        // Equal duplicate values still violate the contract. Start from raw bytes;
        // constructing a Value here would remove the defect before the test.
        let duplicate = valid.replace(
            "\"resourceId\":",
            &format!("\"resourceId\":\"{owner}\",\"resourceId\":"),
        );
        for bytes in [valid.into_bytes(), duplicate.into_bytes()] {
            steps.push(caller_step(
                "/rest/v1/rpc/read_current_session",
                &token,
                session.clone(),
            ));
            let mut step = Step::json(&format!("/rest/v1/rpc/{rpc}"), Value::Null);
            step.body = bytes;
            steps.push(step);
        }
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for (path, _, input, lookup) in routes {
        for expected in [
            if lookup {
                StatusCode::OK
            } else {
                StatusCode::ACCEPTED
            },
            StatusCode::SERVICE_UNAVAILABLE,
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(input.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{path}");
        }
    }
    assert_eq!(backend.finish().len(), 20);
}

#[tokio::test]
async fn nested_enum_aliases_reject_before_auth_or_command_rpc() {
    let (token, _) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let id = Uuid::new_v4();
    for (path, input) in [
        (
            "/v1/decks",
            json!({"requestId":id,"name":"Enum fixture","formatId":id,"mode":{"open":null}}),
        ),
        (
            "/v1/social/commands",
            json!({"kind":"friendship","command":{"requestId":id,"otherAccountId":id,"action":{"request":null}}}),
        ),
        (
            "/v1/economy/commands",
            json!({"kind":"trade_response","command":{"requestId":id,"tradeId":id,"action":{"accept":null}}}),
        ),
        (
            "/v1/predictions/commands",
            json!({"kind":"place_order","command":{"requestId":id,"marketId":id,"side":{"yes":null},"limitCost":"60","quantity":"1"}}),
        ),
        (
            "/v1/rooms/commands",
            json!({"kind":"create","command":{"requestId":id,"formatId":id,"profileId":id,"mode":"open","competitive":false,"visibility":{"public":null}}}),
        ),
        (
            "/v1/rooms/commands",
            json!({"kind":{"create":null},"command":{"requestId":id,"formatId":id,"profileId":id,"mode":"open","competitive":false,"visibility":"public"}}),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let payload = body(response).await;
        assert_eq!(payload["status"], "fail");
    }
    assert!(backend.finish().is_empty());
}

#[tokio::test]
async fn legacy_deck_and_default_routes_reject_queries_before_authority_or_effects() {
    let (token, _) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let id = Uuid::new_v4();
    let appearance = json!({"backId":id,"sleeveId":id,"diceId":id,"playmatId":null});
    let settings =
        json!({"name":"Query fixture","mode":"open","formatId":id,"appearance":appearance});
    for (method, path, input) in [
        (
            "POST",
            "/v1/decks".to_owned(),
            json!({"requestId":id,"name":"Query fixture","mode":"open","formatId":id}),
        ),
        (
            "POST",
            format!("/v1/decks/{id}/revisions"),
            json!({"requestId":id,"deckId":id,"expectedRevisionId":null,"entries":[]}),
        ),
        (
            "POST",
            format!("/v1/decks/{id}/settings"),
            json!({"requestId":id,"deckId":id,"expected":settings,"settings":settings}),
        ),
        (
            "POST",
            "/v1/decks/commands/disposition".to_owned(),
            json!({"kind":"create","command":{"requestId":id,"name":"Query fixture","mode":"open","formatId":id}}),
        ),
        ("GET", format!("/v1/decks/{id}"), Value::Null),
        (
            "POST",
            "/v1/account/defaults".to_owned(),
            json!({"requestId":id,"expected":appearance,"appearance":appearance}),
        ),
        (
            "POST",
            "/v1/account/defaults/disposition".to_owned(),
            json!({"requestId":id,"expected":appearance,"appearance":appearance}),
        ),
    ] {
        for query in ["?unexpected=1", "?unexpected=%FF", "?"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(format!("{path}{query}"))
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(input.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}{query}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert_eq!(
                body(response).await,
                json!({"status":"fail","data":{"query":"unsupported"}})
            );
        }
    }
    assert!(backend.finish().is_empty());
}
