//! HTTP protocol fixtures do not qualify actual Auth or database behavior.
use super::*;
use crate::test_http::{Mock, Step};
use axum::body::Body;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

#[path = "api_tests/query_rejection.rs"]
mod query_rejection;

#[path = "api_tests/content_type.rs"]
mod content_type;

#[path = "api_tests/economy.rs"]
mod economy;

#[path = "api_tests/native_commands.rs"]
mod native_commands;
#[path = "api_tests/native_receipts.rs"]
mod native_receipts;
#[path = "api_tests/native_reconnect.rs"]
mod native_reconnect;
#[path = "api_tests/native_stream.rs"]
mod native_stream;
#[path = "api_tests/native_views.rs"]
mod native_views;
#[path = "api_tests/quest_reroll.rs"]
mod quest_reroll;

#[path = "api_tests/exchange_reads.rs"]
mod exchange_reads;

#[path = "api_tests/object_shapes.rs"]
mod object_shapes;

#[path = "api_tests/ui_surface.rs"]
mod ui_surface;

#[tokio::test]
async fn private_objects_registration_ack_requires_a_record_without_duplicate_keys() {
    let (token, session) = identity();
    let account = Uuid::new_v4();
    let mut steps = Vec::new();
    for raw in [
        json!({"accountId":account}).to_string(),
        json!([account]).to_string(),
        format!(r#"{{"accountId":"{account}","accountId":"{account}"}}"#),
    ] {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        let mut step = Step::json("/rest/v1/rpc/register_account_command", Value::Null);
        step.body = format!(r#"{{"status":"success","data":{raw}}}"#).into_bytes();
        steps.push(step);
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
                    .method("POST")
                    .uri("/v1/account")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"new_player"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(backend.finish().len(), 6);
}

#[tokio::test]
async fn social_status_uses_original_caller_and_rejects_unscoped_or_inconsistent_status() {
    let (token, _) = identity();
    let owner = Uuid::new_v4();
    let peer = Uuid::new_v4();
    let person = json!({"id":peer,"username":"test_peer","isDeleted":false,"relationship":"none","friendshipId":null,"isBlocked":false,"canContact":false,"conversationId":null,"unreadMessages":"0","supporter":false});
    let global = json!({"status":"success","data":{"accountId":owner,"personId":null,"person":null,"unreadMessages":"9007199254740993"}});
    let selected = json!({"status":"success","data":{"accountId":owner,"personId":peer,"person":person,"unreadMessages":"9007199254740993"}});
    let mut foreign = selected.clone();
    foreign["data"]["person"]["id"] = json!(Uuid::new_v4());
    let mut missing = selected.clone();
    missing["data"].as_object_mut().unwrap().remove("person");
    let backend = Mock::new(
        vec![global, selected, foreign, missing]
            .into_iter()
            .map(|body| caller_step("/rest/v1/rpc/read_my_social_status", &token, body))
            .collect(),
    );
    let app = router(config(&backend.base));
    for (path, expected) in [
        ("/v1/social/status".to_owned(), StatusCode::OK),
        (format!("/v1/social/status?personId={peer}"), StatusCode::OK),
        (
            format!("/v1/social/status?personId={peer}"),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            format!("/v1/social/status?personId={peer}"),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }
    for path in [
        "/v1/social/status?actorId=private".to_owned(),
        format!("/v1/social/status?personId={}", Uuid::nil()),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let seen = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[0].2).unwrap(),
        json!({"p_peer":null})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[1].2).unwrap(),
        json!({"p_peer":peer})
    );
}

#[tokio::test]
async fn social_routes_bind_original_session_and_reject_untrusted_receipt_outputs() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let other = Uuid::new_v4();
    let request = Uuid::new_v4();
    let input = json!({"kind":"block","command":{"requestId":request,"otherAccountId":other,"blocked":true}});
    let outputs = vec![
        (
            false,
            StatusCode::ACCEPTED,
            json!({"status":"success","data":{"resourceId":other}}),
        ),
        (
            true,
            StatusCode::OK,
            json!({"status":"success","data":{"state":"unknown","response":null}}),
        ),
        (
            true,
            StatusCode::OK,
            json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":other}}}}),
        ),
        (
            true,
            StatusCode::OK,
            json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"contact":"contact unavailable"}}}}),
        ),
        (
            true,
            StatusCode::CONFLICT,
            json!({"status":"fail","data":{"request":"conflict"}}),
        ),
        (
            false,
            StatusCode::UNAUTHORIZED,
            json!({"status":"fail","data":{"session":"unauthorized"}}),
        ),
        (
            false,
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"status":"success","data":{"resourceId":Uuid::new_v4()}}),
        ),
        (
            true,
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"diagnostic":"private vendor text"}}}}),
        ),
    ];
    let mut steps = Vec::new();
    for (lookup, _, output) in &outputs {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            if *lookup {
                "/rest/v1/rpc/read_social_command"
            } else {
                "/rest/v1/rpc/execute_social_command"
            },
            output.clone(),
        ));
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for (lookup, status, _) in outputs {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(if lookup {
                        "/v1/social/commands/disposition"
                    } else {
                        "/v1/social/commands"
                    })
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(
            !body(response)
                .await
                .to_string()
                .contains("private vendor text")
        );
    }
    let seen = backend.finish();
    for index in (1..seen.len()).step_by(2) {
        let sent: Value = serde_json::from_slice(&seen[index].2).unwrap();
        assert_eq!(
            sent,
            json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_input":input})
        );
        assert!(!sent.to_string().contains(&token));
    }
}

#[tokio::test]
async fn social_http_intake_denies_actor_injection_nil_ids_and_oversized_unicode() {
    let (token, _) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let valid = json!({"kind":"send_dm","command":{"requestId":Uuid::new_v4(),"conversationId":Uuid::new_v4(),"body":"Hello"}});
    let mut actor = valid.clone();
    actor["command"]["actorId"] = json!(Uuid::new_v4());
    let mut nil = valid.clone();
    nil["command"]["requestId"] = json!(Uuid::nil());
    let mut long = valid.clone();
    long["command"]["body"] = json!("🎲".repeat(4001));
    let mut blank = valid.clone();
    blank["command"]["body"] = json!(" \n\t");
    for input in [
        actor,
        nil,
        long,
        blank,
        json!({"kind":"merge","command":{}}),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/social/commands")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert!(backend.finish().is_empty());
}

#[tokio::test]
async fn social_reads_use_only_original_caller_and_validate_exact_history_scope_and_cursor() {
    let (token, _) = identity();
    let owner = Uuid::new_v4();
    let peer = Uuid::new_v4();
    let conversation = Uuid::new_v4();
    let message = Uuid::new_v4();
    let before_id = Uuid::new_v4();
    let person = json!({"id":peer,"username":"test_peer","isDeleted":false,"relationship":"none","friendshipId":null,"isBlocked":false,"canContact":false,"conversationId":conversation,"unreadMessages":"1","supporter":false});
    let item = json!({"id":message,"senderId":peer,"author":"test_peer","body":"History","sentAt":"2026-11-02T12:00:00.000002Z"});
    let page = json!({"accountId":owner,"conversationId":conversation,"person":person,"items":[item],"nextCursor":null});
    let mut unmasked = page.clone();
    unmasked["person"]["isDeleted"] = json!(true);
    let mut foreign = page.clone();
    foreign["items"][0]["senderId"] = json!(Uuid::new_v4());
    let outputs = vec![
        (StatusCode::OK, json!({"status":"success","data":page})),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"status":"success","data":unmasked}),
        ),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"status":"success","data":foreign}),
        ),
        (
            StatusCode::NOT_FOUND,
            json!({"status":"fail","data":{"conversation":"unavailable"}}),
        ),
        (
            StatusCode::UNAUTHORIZED,
            json!({"status":"fail","data":{"session":"unauthorized"}}),
        ),
    ];
    let backend = Mock::new(
        outputs
            .iter()
            .map(|(_, output)| {
                caller_step(
                    "/rest/v1/rpc/read_my_direct_messages",
                    &token,
                    output.clone(),
                )
            })
            .collect(),
    );
    let app = router(config(&backend.base));
    for (status, _) in outputs {
        let mut uri = reqwest::Url::parse(&format!(
            "https://app.example/v1/social/conversations/{conversation}/messages"
        ))
        .unwrap();
        uri.query_pairs_mut()
            .append_pair("beforeAt", "2026-11-02T12:00:00.000003+00:00")
            .append_pair("beforeId", &before_id.to_string());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("{}?{}", uri.path(), uri.query().unwrap()))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    for (_, _, bytes) in backend.finish() {
        let sent: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            sent,
            json!({"p_conversation":conversation,"p_before_at":"2026-11-02T12:00:00.000003Z","p_before_id":before_id,"p_limit":16})
        );
    }
}

#[tokio::test]
async fn social_directory_and_thread_routes_preserve_modes_and_reject_cursor_replays() {
    let (token, _) = identity();
    let owner = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    let peer = Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap();
    let after = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let person = json!({"id":peer,"username":"test_peer","isDeleted":false,"relationship":"none","friendshipId":null,"isBlocked":false,"canContact":false,"conversationId":null,"unreadMessages":"0","supporter":false});
    let valid =
        json!({"status":"success","data":{"accountId":owner,"items":[person],"nextCursor":null}});
    let mut stale = valid.clone();
    stale["data"]["items"][0]["id"] = json!(after);
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_my_social_people", &token, valid.clone()),
        caller_step("/rest/v1/rpc/read_my_social_people", &token, stale),
        caller_step("/rest/v1/rpc/read_my_social_people", &token, valid),
    ]);
    let app = router(config(&backend.base));
    for (path, status) in [
        (
            format!("/v1/social/people?query=%20TEST_%20&after={after}"),
            StatusCode::OK,
        ),
        (
            format!("/v1/social/threads?after={after}"),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        ("/v1/social/threads".into(), StatusCode::OK),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    let seen = backend.finish();
    let directory: Value = serde_json::from_slice(&seen[0].2).unwrap();
    assert_eq!(
        directory,
        json!({"p_kind":"directory","p_query":" TEST_ ","p_after":after,"p_limit":50})
    );
    let threads: Value = serde_json::from_slice(&seen[2].2).unwrap();
    assert_eq!(
        threads,
        json!({"p_kind":"threads","p_query":null,"p_after":null,"p_limit":50})
    );
}

#[tokio::test]
async fn social_read_intake_rejects_unpaired_cursors_and_unrecognized_or_oversized_queries() {
    let (token, _) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let conversation = Uuid::new_v4();
    for path in [
        "/v1/social/people".into(),
        "/v1/social/people?query=%20%20".into(),
        "/v1/social/threads?query=ignored".into(),
        "/v1/social/threads?actorId=private".into(),
        format!("/v1/social/conversations/{conversation}/messages?beforeAt=2026-11-02T12:00:00Z"),
        format!("/v1/social/conversations/{conversation}/messages?limit=17"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert!(backend.finish().is_empty());
}

#[tokio::test]
async fn read_ack_routes_bind_viewed_ids_and_exact_conversation_receipt() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let conversation = Uuid::new_v4();
    let request = Uuid::new_v4();
    let ids = vec![Uuid::new_v4(), Uuid::new_v4()];
    let input = json!({"requestId":request,"conversationId":conversation,"messageIds":ids});
    let outputs = vec![
        (
            false,
            StatusCode::ACCEPTED,
            json!({"status":"success","data":{"resourceId":conversation}}),
        ),
        (
            true,
            StatusCode::OK,
            json!({"status":"success","data":{"state":"unknown","response":null}}),
        ),
        (
            true,
            StatusCode::OK,
            json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"conversation":"conversation unavailable"}}}}),
        ),
        (
            false,
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"status":"success","data":{"resourceId":Uuid::new_v4()}}),
        ),
    ];
    let mut steps = Vec::new();
    for (lookup, _, output) in &outputs {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            if *lookup {
                "/rest/v1/rpc/read_direct_read_command"
            } else {
                "/rest/v1/rpc/mark_direct_messages_read"
            },
            output.clone(),
        ));
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for (lookup, status, _) in outputs {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(if lookup {
                        "/v1/social/read/disposition"
                    } else {
                        "/v1/social/read"
                    })
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let seen = backend.finish();
    let write: Value = serde_json::from_slice(&seen[1].2).unwrap();
    assert_eq!(
        write,
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":request,"p_conversation":conversation,"p_messages":ids})
    );
    let read: Value = serde_json::from_slice(&seen[3].2).unwrap();
    assert_eq!(
        read,
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_input":input})
    );
}

#[tokio::test]
async fn read_ack_http_rejects_duplicate_or_missing_message_ids_without_rpc() {
    let (token, _) = identity();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let id = Uuid::new_v4();
    for ids in [
        json!([]),
        json!([id, id]),
        json!([Uuid::nil()]),
        json!((0..17).map(|_| Uuid::new_v4()).collect::<Vec<_>>()),
    ] {
        let response=app.clone().oneshot(Request::builder().method("POST").uri("/v1/social/read").header(header::AUTHORIZATION,format!("Bearer {token}")).header(header::CONTENT_TYPE,"application/json").body(Body::from(serde_json::to_vec(&json!({"requestId":Uuid::new_v4(),"conversationId":id,"messageIds":ids})).unwrap())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert!(backend.finish().is_empty());
}

const ISSUER: &str = "https://auth.example.test/auth/v1";
const KEY: &str = "sb_publishable_test_only";

#[tokio::test]
async fn catalogue_routes_preserve_literal_queries_and_caller_scope_and_reject_unsolicited_labels()
{
    let (token, _) = identity();
    let card = Uuid::new_v4();
    let other = Uuid::new_v4();
    let printing = Uuid::new_v4();
    let card_label = json!({"id":card,"name":"Mask%_★","oracleRef":card});
    let mut print_label = json!({"id":printing,"definitionId":other,"externalPrintingRef":printing,"rarity":"rare","finish":"nonfoil","collectorNumber":"123a★","language":"en","set":{"id":Uuid::new_v4(),"name":"Test set","code":"TST"}});
    let wrong_print = print_label.clone();
    print_label["definitionId"] = json!(card);
    let backend = Mock::new(vec![
        caller_step(
            "/rest/v1/rpc/read_my_cards",
            &token,
            json!({"status":"success","data":{"items":[card_label],"nextCursor":null}}),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_printings",
            &token,
            json!({"status":"success","data":{"items":[wrong_print],"nextCursor":null}}),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_catalogue",
            &token,
            json!({"status":"success","data":{"cards":[{"id":other,"name":"Unsolicited","oracleRef":other}],"printings":[]}}),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_printings",
            &token,
            json!({"status":"success","data":{"items":[print_label],"nextCursor":null}}),
        ),
    ]);
    let app = router(config(&backend.base));
    let mut query = reqwest::Url::parse("https://app.example/v1/cards").unwrap();
    query
        .query_pairs_mut()
        .append_pair("query", "Mask%_★")
        .append_pair("exact", "true")
        .append_pair("limit", "100");
    let paths = vec![
        format!("{}?{}", query.path(), query.query().unwrap()),
        format!("/v1/cards/{card}/printings"),
        "/v1/catalogue/resolve".into(),
        "/v1/cards/printings?name=Mask%25_%E2%98%85&setCode=TST&collectorNumber=123a%E2%98%85"
            .into(),
    ];
    for (index, path) in paths.into_iter().enumerate() {
        let builder = Request::builder()
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        let request = if index == 2 {
            builder
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"definitionIds":[card],"printingIds":[]})).unwrap(),
                ))
                .unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        };
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            if matches!(index, 1 | 2) {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::OK
            }
        );
        if matches!(index, 1 | 2) {
            assert_eq!(body(response).await["code"], "unavailable");
        }
    }
    let seen = backend.finish();
    let first: Value = serde_json::from_slice(&seen[0].2).unwrap();
    assert_eq!(first["p_query"], "Mask%_★");
    assert_eq!(first["p_exact"], true);
    let last: Value = serde_json::from_slice(&seen[3].2).unwrap();
    assert_eq!(last["p_name"], "Mask%_★");
    assert_eq!(last["p_set_code"], "TST");
    assert_eq!(last["p_collector"], "123a★");
    assert!(last["p_definition"].is_null());
}

#[tokio::test]
async fn deck_disposition_route_preserves_bound_body_and_distinguishes_unknown_rejected_and_conflict()
 {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let command = json!({"requestId":Uuid::new_v4(),"name":"Original terms","mode":"collection","formatId":Uuid::new_v4()});
    let outputs = vec![
        (
            StatusCode::OK,
            json!({"status":"success","data":{"state":"unknown","response":null}}),
        ),
        (
            StatusCode::OK,
            json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":{"resourceId":Uuid::new_v4()}}}}),
        ),
        (
            StatusCode::OK,
            json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"cosmetics":"default cosmetic unavailable"}}}}),
        ),
        (
            StatusCode::CONFLICT,
            json!({"status":"fail","data":{"request":"conflict"}}),
        ),
    ];
    let mut steps = Vec::new();
    for (_, output) in &outputs {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json("/rest/v1/rpc/read_deck_command", output.clone()));
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for (status, output) in outputs {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/decks/commands/disposition")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({"kind":"create","command":command})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(body(response).await, output);
    }
    let seen = backend.finish();
    for index in [1, 3, 5, 7] {
        let sent: Value = serde_json::from_slice(&seen[index].2).unwrap();
        assert_eq!(sent["p_subject"], session["data"]["subjectId"]);
        assert_eq!(sent["p_session"], session["data"]["sessionId"]);
        assert_eq!(sent["p_kind"], "create");
        assert_eq!(sent["p_command"], command);
        assert!(sent.get("p_actor").is_none());
    }
}

#[tokio::test]
async fn deck_writes_derive_session_and_preserve_revision_and_request_terms() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let subject = session["data"]["subjectId"].clone();
    let session_id = session["data"]["sessionId"].clone();
    let deck = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let format = Uuid::new_v4();
    let create = Uuid::new_v4();
    let save = Uuid::new_v4();
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/create_deck_command",
            json!({"status":"success","data":{"resourceId":deck}}),
        ),
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        Step::json(
            "/rest/v1/rpc/save_deck_command",
            json!({"status":"success","data":{"resourceId":revision}}),
        ),
    ]);
    let app = router(config(&backend.base));
    for (path, input, id) in [
        (
            "/v1/decks".into(),
            json!({"requestId":create,"name":"Owned","mode":"collection","formatId":format}),
            deck,
        ),
        (
            format!("/v1/decks/{deck}/revisions"),
            json!({"requestId":save,"deckId":deck,"expectedRevisionId":null,"entries":[]}),
            revision,
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
                    .body(Body::from(serde_json::to_vec(&input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            body(response).await,
            json!({"status":"success","data":{"resourceId":id}})
        );
    }
    let seen = backend.finish();
    for (index, request) in [(1, create), (3, save)] {
        let input: Value = serde_json::from_slice(&seen[index].2).unwrap();
        assert_eq!(input["p_subject"], subject);
        assert_eq!(input["p_session"], session_id);
        assert_eq!(input["p_request"], request.to_string());
        assert!(input.get("p_actor").is_none());
    }
    let input: Value = serde_json::from_slice(&seen[3].2).unwrap();
    assert_eq!(input["p_deck"], deck.to_string());
    assert!(input["p_expected_revision"].is_null());
    assert_eq!(input["p_entries"], json!([]));
}

#[tokio::test]
async fn deletion_uses_verified_session_and_observer_forwards_only_its_digest() {
    let (token, mut session) = identity();
    let account = Uuid::new_v4();
    let request = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(account);
    let key = URL_SAFE_NO_PAD.encode([0_u8; 32]);
    let digest = "66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925";
    let command = json!({"requestId":request,"statusKeyHash":digest});
    let observation = json!({"accountId":account,"requestId":request,"phase":"pending"});
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/request_account_deletion_command",
            json!({"status":"success","data":{"resourceId":account}}),
        ),
        Step::json(
            "/rest/v1/rpc/observe_account_deletion",
            json!({"status":"success","data":observation}),
        ),
    ]);
    let app = router(config(&backend.base));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/account/deletion")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&command).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/account/deletion/observe")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(
                        &json!({"accountId":account,"requestId":request,"statusKey":key}),
                    )
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let seen = backend.finish();
    let sent: Value = serde_json::from_slice(&seen[1].2).unwrap();
    assert_eq!(
        sent,
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":request,"p_status_key_hash":digest})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[2].2).unwrap(),
        json!({"p_account":account,"p_request":request,"p_status_key_hash":digest})
    );
    assert!(!String::from_utf8_lossy(&seen[2].2).contains(&key));
}

#[tokio::test]
async fn observer_invalid_keys_never_lookup_and_cross_scope_reply_is_unavailable() {
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    for key in ["A".repeat(42), format!("{}B", "A".repeat(42))] {
        let response=app.clone().oneshot(Request::builder().method("POST").uri("/v1/account/deletion/observe")
            .header(header::CONTENT_TYPE,"application/json")
            .body(Body::from(serde_json::to_vec(&json!({"accountId":Uuid::new_v4(),"requestId":Uuid::new_v4(),"statusKey":key})).unwrap())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    assert!(backend.finish().is_empty());
    let backend = Mock::new(vec![Step::json(
        "/rest/v1/rpc/observe_account_deletion",
        json!({"status":"success","data":{"accountId":Uuid::new_v4(),"requestId":Uuid::new_v4(),"phase":"pending"}}),
    )]);
    let response=router(config(&backend.base)).oneshot(Request::builder().method("POST").uri("/v1/account/deletion/observe")
        .header(header::CONTENT_TYPE,"application/json")
        .body(Body::from(serde_json::to_vec(&json!({"accountId":Uuid::new_v4(),"requestId":Uuid::new_v4(),"statusKey":"A".repeat(43)})).unwrap())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    backend.finish();
}

#[tokio::test]
async fn profile_defaults_bind_both_snapshots_to_the_verified_account_and_receipt() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let appearance = json!({"backId":Uuid::new_v4(),"sleeveId":Uuid::new_v4(),"diceId":Uuid::new_v4(),"playmatId":null});
    let mut expected = appearance.clone();
    expected["diceId"] = json!(Uuid::new_v4());
    let request = Uuid::new_v4();
    let command = json!({"requestId":request,"expected":expected,"appearance":appearance});
    let rejection = json!({"status":"fail","data":{"defaults":"account defaults changed; reload before saving"}});
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/change_profile_defaults_command",
            rejection.clone(),
        ),
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/read_profile_defaults_command",
            json!({"status":"success","data":{"state":"rejected","response":rejection}}),
        ),
    ]);
    let app = router(config(&backend.base));
    for (path, status) in [
        ("/v1/account/defaults", StatusCode::CONFLICT),
        ("/v1/account/defaults/disposition", StatusCode::OK),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&command).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    let seen = backend.finish();
    let sent: Value = serde_json::from_slice(&seen[1].2).unwrap();
    assert_eq!(sent["p_subject"], session["data"]["subjectId"]);
    assert_eq!(sent["p_session"], session["data"]["sessionId"]);
    assert_eq!(sent["p_request"], request.to_string());
    assert_eq!(sent["p_expected"], expected);
    assert_eq!(sent["p_appearance"], appearance);
    assert!(sent.get("p_actor").is_none());
    let lookup: Value = serde_json::from_slice(&seen[3].2).unwrap();
    assert_eq!(lookup["p_command"], command);
}

#[tokio::test]
async fn deck_settings_keep_expected_and_new_values_bound_to_verified_session() {
    let (token, mut session) = identity();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let deck = Uuid::new_v4();
    let expected = json!({"name":"Initial","mode":"collection","formatId":Uuid::new_v4(),"appearance":{"backId":Uuid::new_v4(),"sleeveId":Uuid::new_v4(),"diceId":Uuid::new_v4(),"playmatId":null}});
    let mut next = expected.clone();
    next["name"] = json!("Renamed");
    let request = Uuid::new_v4();
    let command = json!({"requestId":request,"deckId":deck,"expected":expected,"settings":next});
    let rejection =
        json!({"status":"fail","data":{"settings":"deck settings changed; reload before saving"}});
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/change_deck_settings_command",
            rejection.clone(),
        ),
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/read_deck_command",
            json!({"status":"success","data":{"state":"rejected","response":rejection}}),
        ),
    ]);
    let app = router(config(&backend.base));
    for (path, input, status) in [
        (
            format!("/v1/decks/{deck}/settings"),
            command.clone(),
            StatusCode::CONFLICT,
        ),
        (
            "/v1/decks/commands/disposition".into(),
            json!({"kind":"settings","command":command}),
            StatusCode::OK,
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
                    .body(Body::from(serde_json::to_vec(&input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    let seen = backend.finish();
    let sent: Value = serde_json::from_slice(&seen[1].2).unwrap();
    assert_eq!(sent["p_subject"], session["data"]["subjectId"]);
    assert_eq!(sent["p_session"], session["data"]["sessionId"]);
    assert_eq!(sent["p_request"], request.to_string());
    assert_eq!(sent["p_deck"], deck.to_string());
    assert_eq!(sent["p_expected"], expected);
    assert_eq!(sent["p_settings"], next);
    assert!(sent.get("p_actor").is_none());
    let recovered: Value = serde_json::from_slice(&seen[3].2).unwrap();
    assert_eq!(recovered["p_kind"], "settings");
    assert_eq!(recovered["p_command"], command);
}

#[tokio::test]
async fn deck_detail_rejects_cross_revision_projection_and_retains_original_bearer() {
    let (token, _) = identity();
    let deck = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let detail = json!({"deck":{"id":deck,"name":"Owned","mode":"collection","currentRevisionId":revision,
        "format":{"id":Uuid::new_v4(),"key":"standard","name":"Standard"},"appearance":{"backId":Uuid::new_v4(),"sleeveId":Uuid::new_v4(),"diceId":Uuid::new_v4(),"playmatId":null}},"revision":null});
    let backend = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_my_deck",
        &token,
        json!({"status":"success","data":detail}),
    )]);
    let response = router(config(&backend.base))
        .oneshot(
            Request::builder()
                .uri(format!("/v1/decks/{deck}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body(response).await["code"], "unavailable");
    backend.finish();
}

#[tokio::test]
async fn account_summary_and_library_preserve_caller_scope_and_reject_bad_balance() {
    let (token, _) = identity();
    let account = Uuid::new_v4();
    let defaults = json!({"backId":Uuid::new_v4(),"sleeveId":Uuid::new_v4(),"diceId":Uuid::new_v4(),"playmatId":null});
    let summary = json!({"accountId":account,"username":"api_owner","state":"active","appearanceRevision":"2","defaults":defaults,"wallet":{"owned":"9007199254740993","held":"1","available":"9007199254740992"}});
    let deck = Uuid::new_v4();
    let page = json!({"items":[{"id":deck,"name":"Owned deck","mode":"collection","currentRevisionId":null,"format":{"id":Uuid::new_v4(),"key":"standard","name":"Standard"},"appearance":defaults}],"nextCursor":deck});
    let backend = Mock::new(vec![
        caller_step(
            "/rest/v1/rpc/read_my_account",
            &token,
            json!({"status":"success","data":summary}),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_deck_library",
            &token,
            json!({"status":"success","data":page}),
        ),
        caller_step(
            "/rest/v1/rpc/read_my_account",
            &token,
            json!({"status":"success","data":{"wallet":{"owned":"1","held":"2","available":"0"},"accountId":account,"username":"api_owner","state":"active","appearanceRevision":"2","defaults":defaults}}),
        ),
    ]);
    let app = router(config(&backend.base));
    for (path, status) in [
        ("/v1/account", 200),
        ("/v1/decks?limit=1", 200),
        ("/v1/account", 503),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        let value = body(response).await;
        assert_eq!(
            value["status"],
            if status == 200 { "success" } else { "error" }
        );
        assert!(value.get("authSubject").is_none());
    }
    backend.finish();
}
fn config(base: &str) -> ApiConfig {
    ApiConfig::new(
        base.into(),
        KEY.into(),
        "sb_secret_test_only".into(),
        ISSUER.into(),
        [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()],
        vec!["http://localhost:5188".into()],
    )
    .unwrap()
}
fn identity() -> (String, Value) {
    let claims = json!({"sub":Uuid::new_v4(),"session_id":Uuid::new_v4(),"iss":ISSUER,
        "aud":"authenticated","role":"authenticated","exp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()+3600,
        "spellwake_auth":{"version":1,"method":"email_code","bindingId":Uuid::new_v4(),"sessionGrantId":Uuid::new_v4()}});
    let bearer = format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap()),
        URL_SAFE_NO_PAD.encode(b"synthetic")
    );
    let session = json!({"status":"success","data":{"subjectId":claims["sub"],"sessionId":claims["session_id"],"method":"email_code","state":"needs_registration","accountId":null}});
    (bearer, session)
}
fn caller_step(path: &str, token: &str, data: Value) -> Step {
    let mut step = Step::json(path, data);
    step.credentials = crate::test_http::Credentials::Caller(KEY.into(), token.into());
    step
}
async fn body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn checkout_route_connects_verified_actor_reservation_provider_create_and_ack() {
    use crate::stripe::test_fixture::{checkout, price};
    const STRIPE_KEY: &str = "sk_test_spellwake_synthetic_only";
    let (token, mut session) = identity();
    let actor = Uuid::new_v4();
    let id = Uuid::new_v4();
    let cosmetic = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(actor);
    let subject = session["data"]["subjectId"].clone();
    let session_id = session["data"]["sessionId"].clone();
    let terms = json!({"version":1,"priceId":"price_synthetic","currency":"usd","minorUnits":"500","live":false,"apiVersion":"2026-10-28.endive","successUrl":"https://app.example/return","cancelUrl":"https://app.example/cancel"});
    let ticket = json!({"id":id,"accountId":actor,"accountState":"active","cosmeticId":cosmetic,"state":"creating","revision":"1","terms":terms,"termsHash":"a".repeat(64),"requestKey":format!("spellwake-purchase-{id}"),"checkoutRef":null,"paymentRef":null,"deadline":chrono::Utc::now()+chrono::Duration::hours(23),"reversalEventId":null,"reversalKey":null});
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        Step::json("/rest/v1/rpc/read_stripe_reservation", Value::Null),
        Step::json(
            "/rest/v1/rpc/reserve_stripe_purchase",
            json!({"status":"success","data":{"resourceId":id}}),
        ),
        Step::json("/rest/v1/rpc/claim_stripe_creation", ticket),
        Step::json(
            "/rest/v1/rpc/ack_stripe_checkout",
            json!({"state":"pending"}),
        ),
    ]);
    let stripe_get = |path: &str, data: Value| {
        let mut step = Step::get(path.into(), 200, serde_json::to_vec(&data).unwrap());
        step.credentials = crate::test_http::Credentials::Stripe(STRIPE_KEY.into());
        step
    };
    let mut create = Step::json("/v1/checkout/sessions", checkout(id));
    create.credentials = crate::test_http::Credentials::Stripe(STRIPE_KEY.into());
    let vendor = Mock::new(vec![
        stripe_get(
            "/v1/account",
            json!({"id":"acct_synthetic","object":"account"}),
        ),
        stripe_get("/v1/prices/price_synthetic", price()),
        create,
        stripe_get(
            "/v1/checkout/sessions/cs_test_synthetic/line_items?limit=2",
            json!({"object":"list","has_more":false,"data":[{"id":"li_synthetic","quantity":1,"price":price()}]}),
        ),
    ]);
    let mut configured = config(&backend.base);
    configured.stripe = Some(
        StripeConfig::new(
            STRIPE_KEY.into(),
            "whsec_spellwake_synthetic_only".into(),
            "acct_synthetic".into(),
            BTreeMap::from([(cosmetic, "price_synthetic".into())]),
            "https://app.example/return".into(),
            "https://app.example/cancel".into(),
            &vendor.base,
        )
        .unwrap(),
    );
    let request = Uuid::new_v4();
    let response = router(configured)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/purchases")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"requestId":request,"cosmeticId":cosmetic}))
                        .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        body(response).await,
        json!({"status":"success","data":{"resourceId":id}})
    );
    let seen = backend.finish();
    let reserve: Value = serde_json::from_slice(&seen[2].2).unwrap();
    assert_eq!(reserve["p_subject"], subject);
    assert_eq!(reserve["p_session"], session_id);
    assert_eq!(reserve["p_terms"]["minorUnits"], "500");
    assert_eq!(reserve["p_request"], request.to_string());
    let acknowledgment: Value = serde_json::from_slice(&seen[4].2).unwrap();
    assert_eq!(acknowledgment["p_purchase"], id.to_string());
    assert_eq!(acknowledgment["p_checkout"], "cs_test_synthetic");
    vendor.finish();
}

#[tokio::test]
async fn accepted_checkout_replay_does_not_require_current_provider_availability() {
    let (token, mut session) = identity();
    let id = Uuid::new_v4();
    let cosmetic = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(Uuid::new_v4());
    let ack = json!({"status":"success","data":{"resourceId":id}});
    let backend = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session),
        Step::json("/rest/v1/rpc/read_stripe_reservation", ack.clone()),
    ]);
    let mut configured = config(&backend.base);
    configured.stripe = Some(
        StripeConfig::new(
            "sk_test_spellwake_synthetic_only".into(),
            "whsec_spellwake_synthetic_only".into(),
            "acct_synthetic".into(),
            BTreeMap::from([(cosmetic, "price_synthetic".into())]),
            "https://app.example/return".into(),
            "https://app.example/cancel".into(),
            "http://127.0.0.1:1/",
        )
        .unwrap(),
    );
    let response = router(configured)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/purchases")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"requestId":Uuid::new_v4(),"cosmeticId":cosmetic}))
                        .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(body(response).await, ack);
    assert_eq!(backend.finish().len(), 2);
}

#[tokio::test]
async fn purchase_read_retains_original_bearer_and_returns_only_safe_owner_projection() {
    let (token, _) = identity();
    let id = Uuid::new_v4();
    let data = json!({"purchaseId":id,"cosmeticId":Uuid::new_v4(),"createdAt":"2026-11-01T00:00:00Z","state":"pending","revision":"1",
        "price":{"currency":"usd","minorUnits":"500"},"checkout":{"status":"waiting"}});
    let backend = Mock::new(vec![caller_step(
        "/rest/v1/rpc/read_my_purchase",
        &token,
        json!({"status":"success","data":data}),
    )]);
    let response = router(config(&backend.base))
        .oneshot(
            Request::builder()
                .uri(format!("/v1/purchases/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result = body(response).await;
    assert_eq!(result["data"]["price"]["minorUnits"], "500");
    let output = result.to_string();
    for private in [
        "paymentRef",
        "checkoutRef",
        "termsHash",
        "requestKey",
        "provider_event",
    ] {
        assert!(!output.contains(private));
    }
    let seen = backend.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&seen[0].2).unwrap(),
        json!({"p_purchase":id})
    );
}
#[tokio::test]
async fn intake_and_fallbacks_always_use_safe_envelopes() {
    let app = router(config("http://127.0.0.1:1/"));
    for (method, path, input, expected) in [
        ("GET", "/missing", "", 404),
        ("DELETE", "/v1/session", "", 405),
        ("GET", "/v1/session", "", 401),
        ("GET", "/v1/session?subject=x", "", 400),
        (
            "POST",
            "/v1/account",
            r#"{"username":"abc","subject":"injected"}"#,
            400,
        ),
        ("POST", "/v1/account", r#"{"username":" abc"}"#, 400),
        (
            "POST",
            "/v1/account",
            r#"{"username":"abc","username":"def"}"#,
            400,
        ),
        ("GET", "/v1/inventory?account=x", "", 400),
        ("GET", "/v1/inventory?limit=1&limit=2", "", 400),
        ("GET", "/v1/inventory?limit=101", "", 400),
        ("GET", "/v1/inventory?after=invalid", "", 400),
        ("GET", "/v1/purchases/not-a-uuid", "", 400),
        (
            "POST",
            "/v1/purchases",
            r#"{"requestId":"00000000-0000-0000-0000-000000000000","cosmeticId":"00000000-0000-0000-0000-000000000000"}"#,
            400,
        ),
        (
            "POST",
            "/v1/purchases",
            r#"{"requestId":"00000000-0000-0000-0000-000000000001","cosmeticId":"00000000-0000-0000-0000-000000000002","paid":true}"#,
            400,
        ),
        ("POST", "/v1/webhooks/stripe", "{}", 400),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if expected != 401 {
            request = request.header(header::AUTHORIZATION, "Bearer synthetic");
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(input)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), expected, "{path}");
        assert_eq!(body(response).await["status"], "fail");
    }
    let mut request = Request::builder()
        .uri("/v1/session")
        .body(Body::empty())
        .unwrap();
    request.headers_mut().append(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer one"),
    );
    request.headers_mut().append(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer two"),
    );
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn cors_is_exact_and_never_permits_credentials() {
    let mut settings = config("http://127.0.0.1:1/");
    settings
        .origins
        .push(HeaderValue::from_static("tauri://localhost"));
    let app = router(settings);
    for (origin, allowed) in [
        ("http://localhost:5188", true),
        ("http://localhost:5189", false),
        ("https://evil.test", false),
        ("tauri://localhost", true),
        ("tauri://evil.test", false),
        ("tauri://localhost/", false),
        ("null", false),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/v1/account")
                    .header(header::ORIGIN, origin)
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "authorization,content-type",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_some(),
            allowed
        );
        assert!(
            !response
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
        );
    }
}
#[tokio::test]
async fn registration_derives_actor_and_uses_only_configured_grants() {
    let (token, session) = identity();
    let account = Uuid::new_v4();
    let mock = Mock::new(vec![
        caller_step("/rest/v1/rpc/read_current_session", &token, session.clone()),
        Step::json(
            "/rest/v1/rpc/register_account_command",
            json!({"status":"success","data":{"accountId":account}}),
        ),
    ]);
    let cfg = config(&mock.base);
    let defaults = cfg.defaults;
    let app = router(cfg);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/account")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"username":"new_player"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await["data"]["accountId"], json!(account));
    let seen = mock.finish();
    let payload: Value = serde_json::from_slice(&seen[1].2).unwrap();
    assert_eq!(
        payload,
        json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],
        "p_username":"new_player","p_back":defaults[0],"p_sleeve":defaults[1],"p_dice":defaults[2]})
    );
}
#[tokio::test]
async fn operational_and_business_failures_are_distinct() {
    for (data, status) in [
        (
            json!({"status":"fail","data":{"username":"unavailable"}}),
            409,
        ),
        (
            json!({"status":"fail","data":{"session":"unauthorized"}}),
            401,
        ),
        (
            json!({"status":"fail","data":{"unknown":"private_vendor_text"}}),
            503,
        ),
    ] {
        let (token, session) = identity();
        let mock = Mock::new(vec![
            caller_step("/rest/v1/rpc/read_current_session", &token, session),
            Step::json("/rest/v1/rpc/register_account_command", data),
        ]);
        let response = router(config(&mock.base))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/account")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"player"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert!(
            !body(response)
                .await
                .to_string()
                .contains("private_vendor_text")
        );
        mock.finish();
    }
}
#[tokio::test]
async fn inventory_rejects_replayed_page_and_requires_cursor_field() {
    let (token, _) = identity();
    let id = Uuid::new_v4();
    for page in [
        json!({"items":[],"nextCursor":null}),
        json!({"items":[]}),
        json!({"items":[{"id":id,"assetId":Uuid::new_v4(),"revision":"1","owned":"10","held":"0","available":"10","asset":{"kind":"coin","currencyKey":"gold"}}],"nextCursor":null}),
    ] {
        let missing = page.get("nextCursor").is_none();
        let nonempty = !page["items"].as_array().unwrap().is_empty();
        let mock = Mock::new(vec![caller_step(
            "/rest/v1/rpc/read_my_inventory_page",
            &token,
            json!({"status":"success","data":page}),
        )]);
        let response = router(config(&mock.base))
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/inventory?after={id}"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status().as_u16(),
            if missing || nonempty { 503 } else { 200 }
        );
        mock.finish();
    }
}
#[tokio::test]
async fn cancellation_retains_permit_until_blocking_work_really_finishes() {
    let state = ApiState {
        config: Arc::new(config("http://127.0.0.1:1/")),
        permits: Arc::new(Semaphore::new(1)),
        stream_permits: Arc::new(Semaphore::new(1)),
    };
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        task_state
            .blocking(move |_| {
                let _ = started_tx.send(());
                finish_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                success(json!({}))
            })
            .await
    });
    started_rx.await.unwrap();
    task.abort();
    let _ = task.await;
    assert_eq!(state.permits.available_permits(), 0);
    assert_eq!(
        state
            .blocking(|_| panic!("saturated work must not run"))
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    finish_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if state.permits.available_permits() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn startup_rejects_invalid_defaults_credentials_and_nonexact_origins() {
    let make = |public: &str, secret: &str, defaults, origin: &str| {
        ApiConfig::new(
            "http://127.0.0.1:1/".into(),
            public.into(),
            secret.into(),
            ISSUER.into(),
            defaults,
            vec![origin.into()],
        )
    };
    let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    assert!(make(KEY, "sb_secret_test_only", ids, "tauri://localhost").is_ok());
    assert!(
        make(
            KEY,
            "sb_secret_test_only",
            [Uuid::nil(); 3],
            "http://localhost:5188"
        )
        .is_err()
    );
    assert!(
        make(
            "sb_secret_test_only",
            "sb_secret_test_only",
            ids,
            "http://localhost:5188"
        )
        .is_err()
    );
    assert!(make(KEY, KEY, ids, "http://localhost:5188").is_err());
    for origin in [
        "*",
        "null",
        "http://localhost:5188/path",
        "http://localhost:5188/",
        "http://u:p@localhost:5188",
        "http://localhost:5188?key=x",
    ] {
        assert!(make(KEY, "sb_secret_test_only", ids, origin).is_err());
    }
}
#[tokio::test]
async fn oversized_body_and_empty_origin_configuration_are_closed() {
    let mut cfg = config("http://127.0.0.1:1/");
    cfg.origins.clear();
    let app = router(cfg);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/account")
                .header(header::AUTHORIZATION, "Bearer synthetic")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://localhost:5188")
                .body(Body::from("x".repeat(4097)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    assert_eq!(
        body(response).await,
        json!({"status":"fail","data":{"body":"invalid_or_too_large"}})
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .header(header::ORIGIN, "http://localhost:5188")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
}

#[path = "api_tests/pack_shop.rs"]
mod pack_shop;

#[path = "api_tests/set_catalogue.rs"]
mod set_catalogue;

#[path = "api_tests/prediction_commands.rs"]
mod prediction_commands;

#[path = "api_tests/prediction_reads.rs"]
mod prediction_reads;

#[path = "api_tests/prediction_publication.rs"]
mod prediction_publication;

#[path = "api_tests/patreon.rs"]
mod patreon;

#[path = "api_tests/patreon_ingress.rs"]
mod patreon_ingress;

#[path = "api_tests/import.rs"]
mod import;

#[path = "api_tests/progression.rs"]
mod progression;

#[path = "api_tests/rooms.rs"]
mod rooms;

#[path = "api_tests/room_configuration.rs"]
mod room_configuration;

#[path = "api_tests/invitations.rs"]
mod invitations;

#[path = "api_tests/matchmaking.rs"]
mod matchmaking;

#[path = "api_tests/chat_notifications.rs"]
mod chat_notifications;

#[path = "api_tests/profile_art.rs"]
mod profile_art;

#[path = "api_tests/canonical_identifiers.rs"]
mod canonical_identifiers;

#[path = "api_tests/human_foundation.rs"]
mod human_foundation;
