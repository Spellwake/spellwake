//! Router/source protocol tests supplement the real511-case database qualification.
use super::*;
async fn post(app: Router, token: &str, path: &str, body: String) -> Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn human_writes_mount_every_operation_and_recover_exact_special_ack_and_source_parameters() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    let target = Uuid::new_v4();
    let resource = Uuid::new_v4();
    let request = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let hash = "a".repeat(64);
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let base = json!({"p_subject":session["data"]["subjectId"],"p_session":session["data"]["sessionId"],"p_request":request});
    let mut cases = vec![];
    for (suffix, write, read, ack, extra, body) in [
        (
            format!("/v1/rooms/{target}/ai"),
            "add_room_ai_slot_command",
            "read_add_room_ai_slot_command",
            json!({"resourceId":resource,"revision":"4"}),
            json!({"p_room":target,"p_expected":"2"}),
            json!({"requestId":request,"expectedRevision":"2"}),
        ),
        (
            format!("/v1/ai-slots/{target}/bootstrap"),
            "manage_ai_slot_command",
            "read_ai_slot_command",
            json!({"resourceId":resource,"epoch":"1"}),
            json!({"p_kind":"bootstrap","p_slot":target,"p_expected":"2","p_revision":revision}),
            json!({"requestId":request,"expectedRevision":"2","deckRevisionId":revision}),
        ),
        (
            format!("/v1/ai-slots/{target}/remove"),
            "manage_ai_slot_command",
            "read_ai_slot_command",
            json!({"resourceId":target}),
            json!({"p_kind":"remove","p_slot":target,"p_expected":"2","p_revision":null}),
            json!({"requestId":request,"expectedRevision":"2"}),
        ),
        (
            format!("/v1/rooms/{target}/unready"),
            "unready_room_command",
            "read_unready_room_command",
            json!({"resourceId":target,"revision":"2"}),
            json!({"p_room":target,"p_expected":"2"}),
            json!({"requestId":request,"expectedRevision":"2"}),
        ),
        (
            format!("/v1/matches/{target}/ante"),
            "human_ante_command",
            "read_human_ante_command",
            json!({"resourceId":resource,"termsRevision":"3","termsHash":hash}),
            json!({"p_kind":"offer","p_target":target,"p_expected":"2","p_lines":[],"p_hash":null}),
            json!({"requestId":request,"expectedTermsRevision":"2","assets":[]}),
        ),
        (
            format!("/v1/ante/{target}/accept"),
            "human_ante_command",
            "read_human_ante_command",
            json!({"resourceId":target,"state":"offered","termsRevision":"2"}),
            json!({"p_kind":"accept","p_target":target,"p_expected":"2","p_lines":null,"p_hash":hash}),
            json!({"requestId":request,"expectedTermsRevision":"2","termsHash":hash}),
        ),
        (
            format!("/v1/ante/{target}/cancel"),
            "human_ante_command",
            "read_human_ante_command",
            json!({"resourceId":target,"state":"canceled"}),
            json!({"p_kind":"cancel","p_target":target,"p_expected":"2","p_lines":null,"p_hash":null}),
            json!({"requestId":request,"expectedTermsRevision":"2"}),
        ),
        (
            format!("/v1/seasons/{target}/ballot"),
            "replace_supply_ballot_command",
            "read_supply_ballot_command",
            json!({"resourceId":target,"approvals":"2"}),
            json!({"p_season":target,"p_sets":[revision,resource]}),
            json!({"requestId":request,"setIds":[revision,resource]}),
        ),
    ] {
        let mut params = base.clone();
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        cases.push((suffix, write, read, ack, params, body));
    }
    let mut steps = vec![];
    for (_, write, read, ack, _, _) in &cases {
        for (function, data) in [
            (*write, json!({"status":"success","data":ack})),
            (
                *read,
                json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":ack}}}),
            ),
        ] {
            steps.push(caller_step(
                "/rest/v1/rpc/read_current_session",
                &token,
                session.clone(),
            ));
            steps.push(Step::json(&format!("/rest/v1/rpc/{function}"), data));
        }
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for (path, _, _, ack, _, body) in &cases {
        for lookup in [false, true] {
            let response = post(
                app.clone(),
                &token,
                &format!("{path}{}", if lookup { "/disposition" } else { "" }),
                body.to_string(),
            )
            .await;
            assert_eq!(
                response.status(),
                if lookup {
                    StatusCode::OK
                } else {
                    StatusCode::ACCEPTED
                },
                "{path}"
            );
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let value: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap())
                    .unwrap();
            assert_eq!(
                if lookup {
                    &value["data"]["response"]["data"]
                } else {
                    &value["data"]
                },
                ack
            );
        }
    }
    let seen = backend.finish();
    assert_eq!(seen.len(), cases.len() * 4);
    for (index, (_, _, _, _, expected, _)) in cases.iter().enumerate() {
        for offset in [1, 3] {
            assert_eq!(
                serde_json::from_slice::<Value>(&seen[index * 4 + offset].2).unwrap(),
                *expected
            );
        }
    }
}
#[tokio::test]
async fn human_command_recovery_distinguishes_outer_failures_private_leaks_and_exact_context() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    let target = Uuid::new_v4();
    let request = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let cases = [
        (
            format!("/v1/matches/{target}/ante"),
            json!({"requestId":request,"expectedTermsRevision":"0","assets":[]}),
            json!({"status":"fail","data":{"assets":"ante bundle unavailable"}}),
            StatusCode::CONFLICT,
            "human_ante_command",
        ),
        (
            format!("/v1/matches/{target}/ante/disposition"),
            json!({"requestId":request,"expectedTermsRevision":"0","assets":[]}),
            json!({"status":"success","data":{"state":"unknown","response":null}}),
            StatusCode::OK,
            "read_human_ante_command",
        ),
        (
            format!("/v1/matches/{target}/ante/disposition"),
            json!({"requestId":request,"expectedTermsRevision":"0","assets":[]}),
            json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"assets":"ante bundle unavailable"}}}}),
            StatusCode::SERVICE_UNAVAILABLE,
            "read_human_ante_command",
        ),
        (
            format!("/v1/ai-slots/{target}/bootstrap"),
            json!({"requestId":request,"expectedRevision":"0","deckRevisionId":null}),
            json!({"status":"success","data":{"resourceId":Uuid::new_v4(),"epoch":"1","startKey":"private-sentinel"}}),
            StatusCode::SERVICE_UNAVAILABLE,
            "manage_ai_slot_command",
        ),
        (
            format!("/v1/ai-slots/{target}/remove"),
            json!({"requestId":request,"expectedRevision":"0"}),
            json!({"status":"success","data":{"resourceId":Uuid::new_v4()}}),
            StatusCode::SERVICE_UNAVAILABLE,
            "manage_ai_slot_command",
        ),
        (
            format!("/v1/seasons/{target}/ballot/disposition"),
            json!({"requestId":request,"setIds":[]}),
            json!({"status":"success","data":{"state":"rejected","response":{"status":"fail","data":{"request":"conflict"}}}}),
            StatusCode::SERVICE_UNAVAILABLE,
            "read_supply_ballot_command",
        ),
    ];
    let mut steps = vec![];
    for (_, _, reply, _, function) in &cases {
        steps.push(caller_step(
            "/rest/v1/rpc/read_current_session",
            &token,
            session.clone(),
        ));
        steps.push(Step::json(
            &format!("/rest/v1/rpc/{function}"),
            reply.clone(),
        ));
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for (path, body, _, status, _) in cases {
        let response = post(app.clone(), &token, &path, body.to_string()).await;
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("private-sentinel"));
    }
    assert_eq!(backend.finish().len(), 12);
}
#[tokio::test]
async fn human_commands_reject_forged_authority_aliases_missing_nulls_and_duplicate_keys_without_io()
 {
    let id = Uuid::new_v4();
    let backend = Mock::new(vec![]);
    let app = router(config(&backend.base));
    let mut cases = vec![];
    for path in [
        format!("/v1/rooms/{id}/ai"),
        format!("/v1/rooms/{id}/unready"),
        format!("/v1/ai-slots/{id}/remove"),
    ] {
        for field in ["actorId", "sponsorId", "qualificationRef", "seatId"] {
            cases.push((
                path.clone(),
                json!({"requestId":id,"expectedRevision":"0",field:id}).to_string(),
            ));
        }
    }
    cases.extend([
  (format!("/v1/ai-slots/{id}/bootstrap"),json!({"requestId":id,"expectedRevision":"0"}).to_string()),
  (format!("/v1/matches/{id}/ante"),json!({"requestId":id,"expectedTermsRevision":"0","assets":[],"aiCoins":"100"}).to_string()),
  (format!("/v1/matches/{id}/ante"),format!("{{\"requestId\":\"{id}\",\"expectedTermsRevision\":\"0\",\"assets\":[{{\"assetId\":\"{id}\",\"quantity\":\"1\",\"quantity\":\"1\"}}]}}")),
  (format!("/v1/seasons/{id}/ballot"),json!({"requestId":id,"setIds":[id,id]}).to_string()),
  (format!("/v1/rooms/%7B{id}%7D/ai"),json!({"requestId":id,"expectedRevision":"0"}).to_string()),
  (format!("/v1/rooms/{id}/ai?worker=true"),json!({"requestId":id,"expectedRevision":"0"}).to_string()),
 ]);
    for (path, body) in cases {
        let response = post(app.clone(), "untouched", &path, body).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    assert!(backend.finish().is_empty());
}
#[tokio::test]
async fn human_history_mounts_all_reads_with_original_bearer_owner_target_and_cursor_context() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    let target = Uuid::new_v4();
    let seat = Uuid::new_v4();
    let format = Uuid::new_v4();
    let profile = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let page = json!({"accountId":owner,"items":[],"nextCursor":null});
    let scoped = json!({"accountId":owner,"matchId":target,"items":[],"nextCursor":null});
    let summary = json!({"id":target,"formatId":format,"profileId":profile,"mode":"collection","competitive":false,"structure":"bo1","state":"completed","revision":"2","startedAt":"2026-01-01T00:00:00Z","currentGameId":null,"reservedGameId":null,"availability":"available","availabilityRevision":"0"});
    let detail = json!({"accountId":owner,"match":summary,"ownSeatId":seat,"result":{"id":Uuid::new_v4(),"outcome":"win","committedAt":"2026-01-01T00:01:00Z","rankingFormatId":null,"rankSequence":null,"winnerSeatIds":[seat],"terminalKind":"bank_forfeit"}});
    let cases = vec![
        (
            "/v1/matches".to_owned(),
            "read_my_matches",
            page.clone(),
            json!({"p_after":null,"p_limit":50}),
        ),
        (
            format!("/v1/matches/{target}"),
            "read_match_metadata",
            detail,
            json!({"p_match":target}),
        ),
        (
            format!("/v1/matches/{target}/games"),
            "read_match_games",
            json!({"accountId":owner,"matchId":target,"items":[{"id":Uuid::new_v4(),"contextId":null,"gameIndex":"0","state":"canceled","acceptedRevision":"0","result":null}],"nextCursor":null}),
            json!({"p_match":target,"p_after":null,"p_limit":50}),
        ),
        (
            format!("/v1/matches/{target}/seats"),
            "read_match_seats",
            scoped.clone(),
            json!({"p_match":target,"p_after":null,"p_limit":50}),
        ),
        (
            format!("/v1/matches/{target}/decisions"),
            "read_my_match_decisions",
            scoped,
            json!({"p_match":target,"p_after":null,"p_limit":50}),
        ),
        (
            format!("/v1/matches/{target}/ante"),
            "read_match_ante",
            json!({"accountId":owner,"matchId":target,"ownSeatId":seat,"agreement":null}),
            json!({"p_match":target}),
        ),
        (
            "/v1/competitive-results".to_owned(),
            "read_my_competitive_results",
            page.clone(),
            json!({"p_after":null,"p_limit":50,"p_format":null,"p_season":null}),
        ),
        (
            "/v1/daily-wins".to_owned(),
            "read_my_daily_wins",
            page,
            json!({"p_after":null,"p_limit":50}),
        ),
        (
            format!("/v1/seasons/{target}/ballot"),
            "read_supply_ballot",
            json!({"accountId":owner,"seasonId":target,"cutoff":"2026-12-25T00:00:00Z","openedAt":"2026-10-01T00:00:00Z","isOpen":true,"canVote":false,"qualifies":false,"approvedSetIds":[],"publication":null,"items":[],"nextCursor":null}),
            json!({"p_season":target,"p_after":null,"p_limit":50}),
        ),
        (
            format!("/v1/rooms/{target}/ai"),
            "read_room_ai_status",
            json!({"accountId":owner,"roomId":target,"items":[{"slotId":Uuid::new_v4(),"seatIndex":"99","isSponsor":true,"isRetired":false,"stopPending":false,"deckRevisionId":null,"isReady":false,"binding":{"id":Uuid::new_v4(),"epoch":"0","desiredState":"running","observedState":"pending"}}],"nextCursor":null}),
            json!({"p_room":target,"p_after":null,"p_limit":50}),
        ),
    ];
    let mut steps = vec![];
    for (_, function, data, _) in &cases {
        for foreign in [false, true] {
            let mut data = data.clone();
            if foreign {
                data["accountId"] = json!(Uuid::new_v4());
            }
            steps.push(caller_step(
                "/rest/v1/rpc/read_current_session",
                &token,
                session.clone(),
            ));
            steps.push(caller_step(
                &format!("/rest/v1/rpc/{function}"),
                &token,
                json!({"status":"success","data":data}),
            ));
        }
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for (path, _, _, _) in &cases {
        for status in [StatusCode::OK, StatusCode::SERVICE_UNAVAILABLE] {
            let r = app
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
            assert_eq!(r.status(), status, "{path}");
            assert_eq!(r.headers()[header::CACHE_CONTROL], "no-store");
        }
    }
    let seen = backend.finish();
    assert_eq!(seen.len(), 40);
    for (index, (_, _, _, params)) in cases.iter().enumerate() {
        for offset in [1, 3] {
            assert_eq!(
                serde_json::from_slice::<Value>(&seen[index * 4 + offset].2).unwrap(),
                *params
            );
        }
    }
}
#[tokio::test]
async fn human_special_acknowledgments_are_objects_in_direct_and_recovered_success() {
    let (token, mut session) = identity();
    let owner = Uuid::new_v4();
    let target = Uuid::new_v4();
    let request = Uuid::new_v4();
    session["data"]["state"] = json!("active");
    session["data"]["accountId"] = json!(owner);
    let cases = [
        (
            format!("/v1/ai-slots/{target}/bootstrap"),
            "manage_ai_slot_command",
            "read_ai_slot_command",
            json!({"requestId":request,"expectedRevision":"0","deckRevisionId":null}),
            json!([target, "1"]),
        ),
        (
            format!("/v1/seasons/{target}/ballot"),
            "replace_supply_ballot_command",
            "read_supply_ballot_command",
            json!({"requestId":request,"setIds":[]}),
            json!([target, "0"]),
        ),
        (
            format!("/v1/matches/{target}/ante"),
            "human_ante_command",
            "read_human_ante_command",
            json!({"requestId":request,"expectedTermsRevision":"0","assets":[]}),
            json!([target, "1", "a".repeat(64)]),
        ),
        (
            format!("/v1/ante/{target}/accept"),
            "human_ante_command",
            "read_human_ante_command",
            json!({"requestId":request,"expectedTermsRevision":"0","termsHash":"a".repeat(64)}),
            json!([target, "offered", "0"]),
        ),
        (
            format!("/v1/ante/{target}/cancel"),
            "human_ante_command",
            "read_human_ante_command",
            json!({"requestId":request,"expectedTermsRevision":"0"}),
            json!([target, "canceled"]),
        ),
    ];
    let mut steps = vec![];
    for (_, write, read, _, ack) in &cases {
        for (name, data) in [
            (*write, json!({"status":"success","data":ack})),
            (
                *read,
                json!({"status":"success","data":{"state":"accepted","response":{"status":"success","data":ack}}}),
            ),
        ] {
            steps.push(caller_step(
                "/rest/v1/rpc/read_current_session",
                &token,
                session.clone(),
            ));
            steps.push(Step::json(&format!("/rest/v1/rpc/{name}"), data));
        }
    }
    let backend = Mock::new(steps);
    let app = router(config(&backend.base));
    for (path, _, _, body, _) in &cases {
        for lookup in [false, true] {
            let response = post(
                app.clone(),
                &token,
                &format!("{path}{}", if lookup { "/disposition" } else { "" }),
                body.to_string(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{path}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
    }
    assert_eq!(backend.finish().len(), 20);
}
