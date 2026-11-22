//! Accepted SQL source and role controls, not actual native Event codec proof.
use super::*;

type Controller = (Uuid, String, String);
async fn actor(c: &Client, f: &Native, ai: bool) -> FixtureResult<(Uuid, Option<Controller>)> {
    if !ai {
        return Ok((f.seat, None));
    }
    let row=c.query_one("SELECT seat.id,g.id,g.binding_epoch::text,g.grant_secret_ref FROM app.match_seat seat JOIN app.ai_runner_binding b ON b.match_seat_id=seat.id JOIN app.controller_grant g ON g.binding_id=b.id AND g.binding_epoch=b.binding_epoch AND g.state='active' WHERE seat.match_id=$1 AND seat.controller_kind='ai'", &[&f.matched]).await?;
    Ok((row.get(0), Some((row.get(1), row.get(2), row.get(3)))))
}
async fn player_case(
    c: &Client,
    f: &Native,
    seat: Uuid,
    revision: &str,
) -> FixtureResult<(Value, Value)> {
    let issued = view_for(c, f, revision, 501, seat).await?;
    let original =
        json!({"requestId":Uuid::new_v4(),"basis":basis(f,revision),"action":{"kind":"pass"}});
    let prepared = json!({"version":"native-player-accept-v1","basis":basis(f,revision),"actorSeatId":seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(f,"input",502),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(c,f).await?});
    Ok((original, prepared))
}
async fn submit_actor(
    c: &Client,
    f: &Native,
    original: &Value,
    prepared: Option<&Value>,
    controller: Option<&Controller>,
) -> Result<Value, tokio_postgres::Error> {
    if let Some((grant, epoch, secret)) = controller {
        c.query_one(
            "SELECT app.submit_native_controller_command($1,$2::text::numeric,$3,$4,$5)",
            &[grant, epoch, secret, original, &prepared],
        )
        .await
        .map(|r| r.get(0))
    } else {
        submit(c, f, original, prepared).await
    }
}

fn opening_terms(f: &Native) -> Value {
    json!({"version":"native-opening-v1","matchId":f.matched,"gameId":f.game,"owner":OWNER,
        "originalBasis":f.original_basis,"opening":f.opening,"nativeEpoch":"0",
        "runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)})
}
async fn disposition(
    c: &Client,
    game: Uuid,
    request: Uuid,
    original: &Value,
) -> FixtureResult<Value> {
    Ok(c.query_one(
        "SELECT app.read_native_system_disposition($1,$2,$3)",
        &[&game, &request, original],
    )
    .await?
    .get(0))
}
async fn page(
    c: &Client,
    f: &Native,
    anchor: Uuid,
    after: Option<&str>,
    limit: i32,
) -> FixtureResult<Value> {
    Ok(c.query_one(
        "SELECT app.read_native_transition_sources($1,$2,$3,$4::text::numeric,$5)",
        &[&f.game, &f.context, &anchor, &after, &limit],
    )
    .await?
    .get(0))
}
async fn tip(c: &Client, f: &Native) -> FixtureResult<Uuid> {
    Ok(c.query_one("SELECT id FROM app.game_checkpoint WHERE game_id=$1 ORDER BY accepted_revision DESC LIMIT 1", &[&f.game]).await?.get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn new_opening_marker_is_required_without_poisoning_its_original() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = reserved_native(&c, "bo1", true).await?;
    for marker in [None, Some(Value::Null), Some(json!("unknown-v9"))] {
        let mut opening = f.opening.clone();
        opening
            .as_object_mut()
            .unwrap()
            .remove("eventSchemaRevision");
        if let Some(value) = marker {
            opening["eventSchemaRevision"] = value;
        }
        c.batch_execute("SAVEPOINT invalid_marker").await?;
        assert!(accept_opening_fixture(&c, &f, &opening).await.is_err());
        c.batch_execute("ROLLBACK TO SAVEPOINT invalid_marker")
            .await?;
        let count:i64=c.query_one("SELECT count(*) FROM app.command_receipt WHERE target_ref=$1 AND scope='native_opening_v1'", &[&f.game.to_string()]).await?.get(0);
        assert_eq!(count, 0);
    }
    let ack = accept_opening_fixture(&c, &f, &f.opening).await?;
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE; SET ROLE service_role")
        .await?;
    let found = disposition(&c, f.game, f.game, &opening_terms(&f)).await?;
    assert_eq!(found["data"]["state"], "accepted");
    assert_eq!(found["data"]["response"], ack);
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn original_disposition_is_read_only_exact_and_independent_of_later_native_basis()
-> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = native(&c, "bo1").await?;
    let unknown = reserved_native(&c, "bo1", true).await?;
    let accepted = host_advance(&c, &f).await?;
    let row=c.query_one("SELECT request_ref::uuid,normalized_source->'original' FROM app.command_receipt WHERE scope='native_host_input_v1' AND target_ref=$1", &[&f.game.to_string()]).await?;
    let request: Uuid = row.get(0);
    let original: Value = row.get(1);
    host_advance(&c, &f).await?;
    let count: i64 = c
        .query_one("SELECT count(*) FROM app.command_receipt", &[])
        .await?
        .get(0);
    c.batch_execute("SET ROLE service_role").await?;
    let found = disposition(&c, f.game, request, &original).await?;
    assert_eq!(found["data"]["state"], "accepted");
    assert_eq!(found["data"]["response"], accepted);
    assert_eq!(
        disposition(&c, unknown.game, unknown.game, &opening_terms(&unknown)).await?["data"],
        json!({"state":"unknown","response":null})
    );
    let mut changed = original.clone();
    changed["inputRef"] = json!(artifact(&f, "input", 999));
    assert_eq!(
        disposition(&c, f.game, request, &changed).await?["data"],
        json!({"request":"conflict"})
    );
    c.batch_execute("RESET ROLE").await?;
    let after: i64 = c
        .query_one("SELECT count(*) FROM app.command_receipt", &[])
        .await?
        .get(0);
    assert_eq!(after, count);
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn anchored_history_preserves_complete_groups_and_ignores_later_play() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = native(&c, "bo1").await?;
    let initial = tip(&c, &f).await?;
    host_advance(&c, &f).await?;
    let anchor = tip(&c, &f).await?;
    let first = page(&c, &f, anchor, None, 1).await?;
    assert_eq!(first["data"]["groups"].as_array().unwrap().len(), 1);
    assert_eq!(first["data"]["groups"][0]["checkpointId"], json!(initial));
    assert_eq!(
        first["data"]["groups"][0]["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(first["data"]["nextAfterRevision"], "0");
    host_advance(&c, &f).await?;
    c.batch_execute("SET ROLE service_role").await?;
    let second = page(&c, &f, anchor, Some("0"), 1).await?;
    assert_eq!(first["data"]["anchor"], second["data"]["anchor"]);
    assert_eq!(second["data"]["anchor"]["throughAcceptedRevision"], "1");
    assert_eq!(second["data"]["anchor"]["throughEventSequence"], "2");
    assert_eq!(second["data"]["groups"][0]["acceptedRevision"], "1");
    assert_eq!(
        second["data"]["groups"][0]["eventSchemaRevision"],
        "native-transition-v1"
    );
    let events = second["data"]["groups"][0]["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["sequence"], "1");
    assert_eq!(events[1]["sequence"], "2");
    assert!(second["data"]["nextAfterRevision"].is_null());
    assert!(
        page(&c, &f, anchor, Some("1"), 1).await?["data"]["groups"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        page(&c, &f, Uuid::new_v4(), None, 1).await?["data"],
        json!({"game":"unavailable"})
    );
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn native_source_readers_deny_distributed_roles_and_bad_cursors() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = native(&c, "bo1").await?;
    let anchor = tip(&c, &f).await?;
    for role in ["anon", "authenticated", "supabase_auth_admin"] {
        c.batch_execute(&format!("SAVEPOINT denied_role; SET ROLE {role}"))
            .await?;
        let error = c
            .query_one(
                "SELECT app.read_native_system_disposition($1,$1,$2)",
                &[&f.game, &opening_terms(&f)],
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
        c.batch_execute("ROLLBACK TO SAVEPOINT denied_role").await?;
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        let error = c
            .query_one(
                "SELECT app.read_native_transition_sources($1,$2,$3,NULL,1)",
                &[&f.game, &f.context, &anchor],
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::INSUFFICIENT_PRIVILEGE));
        c.batch_execute("ROLLBACK TO SAVEPOINT denied_role").await?;
    }
    c.batch_execute("SET ROLE service_role").await?;
    for (after, limit) in [
        (Some("-1"), 1),
        (Some("0.5"), 1),
        (Some("NaN"), 1),
        (Some("1"), 1),
        (None, 0),
        (None, 33),
    ] {
        c.batch_execute("SAVEPOINT bad_cursor").await?;
        let error = c
            .query_one(
                "SELECT app.read_native_transition_sources($1,$2,$3,$4::text::numeric,$5)",
                &[&f.game, &f.context, &anchor, &after, &limit],
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::INVALID_PARAMETER_VALUE));
        c.batch_execute("ROLLBACK TO SAVEPOINT bad_cursor").await?;
    }
    assert_eq!(page(&c, &f, anchor, None, 1).await?["status"], "success");
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn history_facts_keep_their_exact_creator_event_and_human_attribution() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = native(&c, "bo1").await?;
    let account: Uuid = c
        .query_one(
            "SELECT account_id FROM app.match_seat WHERE id=$1",
            &[&f.seat],
        )
        .await?
        .get(0);
    let mut body = candidate(&c, &f).await?;
    body["facts"] = json!([{"eventIndex":"1","accountId":account,"kind":"fixture:casts","revision":"fixture:v1","value":"3"}]);
    let original = json!({"version":"native-host-input-v1","basis":basis(&f,"0"),"kind":"entropy","inputRef":artifact(&f,"input",301),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
    c.query_one(
        "SELECT app.commit_native_host_input($1,$2,$3,$4)",
        &[
            &f.game,
            &Uuid::new_v4(),
            &original,
            &json!({"owner":OWNER,"fenceEpoch":"1","candidate":body}),
        ],
    )
    .await?;
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
    let anchor = tip(&c, &f).await?;
    c.batch_execute("SET ROLE service_role").await?;
    let result = page(&c, &f, anchor, Some("0"), 1).await?;
    let group = &result["data"]["groups"][0];
    assert_eq!(group["events"][0]["facts"], json!([]));
    assert_eq!(
        group["events"][1]["facts"],
        json!([{"accountId":account,"kind":"fixture:casts","factRevision":"fixture:v1","value":"3"}])
    );
    for event in group["events"].as_array().unwrap() {
        assert_eq!(event["creatorReceiptId"], group["creatorReceiptId"]);
        assert_eq!(event["acceptedRevision"], group["acceptedRevision"]);
    }
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn unknown_system_disposition_during_uncommitted_acceptance_can_be_reconciled() -> TestResult
{
    let reader = connection().await?;
    let f = native(&reader, "bo1").await?;
    let original = json!({"version":"native-host-input-v1","basis":basis(&f,"0"),"kind":"entropy","inputRef":artifact(&f,"input",311),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
    let prepared = json!({"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&reader,&f).await?});
    let request = Uuid::new_v4();
    let writer = connection().await?;
    writer.batch_execute("BEGIN").await?;
    let ack: Value = writer
        .query_one(
            "SELECT app.commit_native_host_input($1,$2,$3,$4)",
            &[&f.game, &request, &original, &prepared],
        )
        .await?
        .get(0);
    reader.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        disposition(&reader, f.game, request, &original).await?["data"],
        json!({"state":"unknown","response":null})
    );
    writer.batch_execute("COMMIT").await?;
    let found = disposition(&reader, f.game, request, &original).await?;
    assert_eq!(found["data"]["state"], "accepted");
    assert_eq!(found["data"]["response"], ack);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn transition_upgrade_preserves_unmarked_originals_and_classifies_a_mixed_prefix()
-> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let old = include_str!(
        "../../../supabase/migrations/20261106023700_native_waiting_clock_sources.sql"
    );
    for name in [
        "accept_reserved_game",
        "accept_native_opening",
        "check_native_creator_frame",
        "apply_accept_host_candidate",
    ] {
        c.batch_execute(historical_function(old, name)).await?;
    }
    let mut f = reserved_native(&c, "bo1", true).await?;
    f.opening
        .as_object_mut()
        .unwrap()
        .remove("eventSchemaRevision");
    let ack = accept_opening_fixture(&c, &f, &f.opening).await?;
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
    let before:Value=c.query_one("SELECT jsonb_agg(jsonb_build_object('source',normalized_source,'response',response_payload,'hash',payload_hash) ORDER BY id) FROM app.command_receipt WHERE target_ref=$1", &[&f.game.to_string()]).await?.get(0);
    let new =
        include_str!("../../../supabase/migrations/20261106023800_native_transition_sources.sql");
    // Existing read functions remain installed; replace only the six changed
    // bodies inside this rolled-back transaction, never replay their CREATE DDL.
    for name in [
        "accept_reserved_game",
        "accept_native_opening",
        "check_native_creator_frame",
        "apply_accept_host_candidate",
        "submit_native_player_command",
        "commit_native_host_input",
    ] {
        c.batch_execute(historical_function(new, name)).await?;
    }
    assert_eq!(accept_opening_fixture(&c, &f, &f.opening).await?, ack);
    assert_eq!(
        disposition(&c, f.game, f.game, &opening_terms(&f)).await?["data"]["response"],
        ack
    );
    let after:Value=c.query_one("SELECT jsonb_agg(jsonb_build_object('source',normalized_source,'response',response_payload,'hash',payload_hash) ORDER BY id) FROM app.command_receipt WHERE target_ref=$1", &[&f.game.to_string()]).await?.get(0);
    assert_eq!(after, before);
    c.batch_execute("SET CONSTRAINTS ALL DEFERRED").await?;
    host_advance(&c, &f).await?;
    let anchor = tip(&c, &f).await?;
    let groups = page(&c, &f, anchor, None, 32).await?["data"]["groups"].clone();
    assert!(groups[0]["eventSchemaRevision"].is_null());
    assert_eq!(groups[1]["eventSchemaRevision"], "native-transition-v1");
    assert_eq!(groups[0]["stateRef"], f.opening["stateRef"]);
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE; ROLLBACK")
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn pending_and_changed_system_originals_never_become_guessed_success() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = reserved_native(&c, "bo1", true).await?;
    let terms = opening_terms(&f);
    c.batch_execute("SAVEPOINT tentative_receipt").await?;
    c.query_one("SELECT app.begin_native_source_receipt('native-opening:'||$1::uuid::text,$1::uuid::text,'native_opening_v1',$1::uuid,$2::jsonb,NULL,NULL)", &[&f.game,&terms]).await?;
    let error = c
        .query_one(
            "SELECT app.read_native_system_disposition($1,$1,$2)",
            &[&f.game, &terms],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::T_R_SERIALIZATION_FAILURE));
    c.batch_execute("ROLLBACK TO SAVEPOINT tentative_receipt")
        .await?;
    assert_eq!(
        disposition(&c, f.game, f.game, &terms).await?["data"]["state"],
        "unknown"
    );
    for invalid in [
        json!({}),
        json!({"version":"unknown"}),
        json!({"version":"native-opening-v1","opening":null}),
    ] {
        c.batch_execute("SAVEPOINT malformed_original").await?;
        let error = c
            .query_one(
                "SELECT app.read_native_system_disposition($1,$1,$2)",
                &[&f.game, &invalid],
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::INVALID_PARAMETER_VALUE));
        c.batch_execute("ROLLBACK TO SAVEPOINT malformed_original")
            .await?;
    }
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn oversized_history_page_fails_without_truncating_a_checkpoint_group() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN; SET LOCAL statement_timeout='60s'")
        .await?;
    let f = native(&c, "bo1").await?;
    // Large SQL metadata fixture only. Avoid executing the same deferred
    // creator assertion once per bulk row; explicitly verify every real
    // original/creator below after restoring hooks. This is not trigger
    // throughput, native Event semantics or production activation evidence.
    c.batch_execute("SET LOCAL session_replication_role=replica")
        .await?;
    for revision in 0..24 {
        let mut body = candidate(&c, &f).await?;
        body["events"] = json!(vec![artifact(&f, "event", 401); 1024]);
        let original = json!({"version":"native-host-input-v1","basis":basis(&f,&revision.to_string()),"kind":"native-work","inputRef":artifact(&f,"input",402),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
        let response: Value = c
            .query_one(
                "SELECT app.commit_native_host_input($1,$2,$3,$4)",
                &[
                    &f.game,
                    &Uuid::new_v4(),
                    &original,
                    &json!({"owner":OWNER,"fenceEpoch":"1","candidate":body}),
                ],
            )
            .await?
            .get(0);
        assert_eq!(response["status"], "success");
    }
    c.batch_execute("SET LOCAL session_replication_role=origin")
        .await?;
    c.query("SELECT app.check_native_commit_receipt(id) FROM app.command_receipt WHERE target_ref=$1 AND scope='native_host_input_v1'", &[&f.game.to_string()]).await?;
    c.query("SELECT app.check_native_creator_frame(source_receipt_id) FROM app.game_checkpoint WHERE game_id=$1 ORDER BY accepted_revision", &[&f.game]).await?;
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
    let anchor = tip(&c, &f).await?;
    c.batch_execute("SET ROLE service_role; SAVEPOINT oversized_page")
        .await?;
    let error = c
        .query_one(
            "SELECT app.read_native_transition_sources($1,$2,$3,NULL,32)",
            &[&f.game, &f.context, &anchor],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code().map(SqlState::code), Some("54000"));
    c.batch_execute("ROLLBACK TO SAVEPOINT oversized_page")
        .await?;
    let bounded = page(&c, &f, anchor, Some("0"), 1).await?;
    assert_eq!(bounded["data"]["groups"].as_array().unwrap().len(), 1);
    assert_eq!(
        bounded["data"]["groups"][0]["events"]
            .as_array()
            .unwrap()
            .len(),
        1024
    );
    assert_eq!(bounded["data"]["nextAfterRevision"], "1");
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn fresh_human_controller_and_system_markers_reject_atomically_before_acceptance()
-> TestResult {
    for ai in [false, true] {
        let c = connection().await?;
        c.batch_execute("BEGIN").await?;
        let f = native_with_roster(&c, "bo1", !ai).await?;
        let (seat, controller) = actor(&c, &f, ai).await?;
        let (original, prepared) = player_case(&c, &f, seat, "0").await?;
        let system_original = json!({"version":"native-host-input-v1","basis":basis(&f,"0"),"kind":"entropy","inputRef":artifact(&f,"input",503),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
        let request = Uuid::new_v4();
        let before: Value = c
            .query_one("SELECT app.match_clock_snapshot($1)", &[&f.matched])
            .await?
            .get(0);
        for marker in [None, Some(Value::Null), Some(json!("unknown-v9"))] {
            let mut bad = prepared.clone();
            bad["candidate"]
                .as_object_mut()
                .unwrap()
                .remove("eventSchemaRevision");
            if let Some(value) = marker {
                bad["candidate"]["eventSchemaRevision"] = value;
            }
            c.batch_execute("SAVEPOINT fresh_marker").await?;
            let error = submit_actor(&c, &f, &original, Some(&bad), controller.as_ref())
                .await
                .unwrap_err();
            assert_eq!(error.code(), Some(&SqlState::INVALID_PARAMETER_VALUE));
            c.batch_execute("ROLLBACK TO SAVEPOINT fresh_marker")
                .await?;
            let error = c
                .query_one(
                    "SELECT app.commit_native_host_input($1,$2,$3,$4)",
                    &[
                        &f.game,
                        &request,
                        &system_original,
                        &json!({"owner":OWNER,"fenceEpoch":"1","candidate":bad["candidate"]}),
                    ],
                )
                .await
                .unwrap_err();
            assert_eq!(error.code(), Some(&SqlState::INVALID_PARAMETER_VALUE));
            c.batch_execute("ROLLBACK TO SAVEPOINT fresh_marker")
                .await?;
            let rows:i64=c.query_one("SELECT count(*) FROM app.command_receipt WHERE (scope='native_player_v1' AND request_ref=$1) OR (scope='native_host_input_v1' AND request_ref=$2)", &[&original["requestId"].as_str().unwrap(),&request.to_string()]).await?.get(0);
            assert_eq!(rows, 0);
            assert_eq!(
                c.query_one("SELECT app.match_clock_snapshot($1)", &[&f.matched])
                    .await?
                    .get::<_, Value>(0),
                before
            );
            assert_eq!(
                c.query_one(
                    "SELECT count(*) FROM app.game_checkpoint WHERE game_id=$1",
                    &[&f.game]
                )
                .await?
                .get::<_, i64>(0),
                1
            );
        }
        assert_eq!(
            submit_actor(&c, &f, &original, Some(&prepared), controller.as_ref()).await?["status"],
            "success"
        );
        c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE; ROLLBACK")
            .await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn upgrade_recovers_unmarked_human_controller_and_system_originals_after_host_fencing()
-> TestResult {
    for ai in [false, true] {
        let c = connection().await?;
        c.batch_execute("BEGIN").await?;
        let old_clock = include_str!(
            "../../../supabase/migrations/20261106023700_native_waiting_clock_sources.sql"
        );
        let old_commits =
            include_str!("../../../supabase/migrations/20261106023100_native_original_commits.sql");
        for (source, names) in [
            (
                old_clock,
                &[
                    "accept_reserved_game",
                    "accept_native_opening",
                    "check_native_creator_frame",
                    "apply_accept_host_candidate",
                ][..],
            ),
            (
                old_commits,
                &["submit_native_player_command", "commit_native_host_input"][..],
            ),
        ] {
            for name in names {
                c.batch_execute(&historical_function(source, name).replacen(
                    "CREATE FUNCTION",
                    "CREATE OR REPLACE FUNCTION",
                    1,
                ))
                .await?;
            }
        }
        let mut f = reserved_native(&c, "bo1", !ai).await?;
        f.opening
            .as_object_mut()
            .unwrap()
            .remove("eventSchemaRevision");
        let opening_ack = accept_opening_fixture(&c, &f, &f.opening).await?;
        let (seat, controller) = actor(&c, &f, ai).await?;
        let (original, mut prepared) = player_case(&c, &f, seat, "0").await?;
        prepared["candidate"]
            .as_object_mut()
            .unwrap()
            .remove("eventSchemaRevision");
        let player_ack =
            submit_actor(&c, &f, &original, Some(&prepared), controller.as_ref()).await?;
        assert_eq!(player_ack["status"], "success");
        let system_request = Uuid::new_v4();
        let system_original = json!({"version":"native-host-input-v1","basis":basis(&f,"1"),"kind":"entropy","inputRef":artifact(&f,"input",504),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
        let mut system_candidate = candidate(&c, &f).await?;
        system_candidate
            .as_object_mut()
            .unwrap()
            .remove("eventSchemaRevision");
        let system_ack: Value = c
            .query_one(
                "SELECT app.commit_native_host_input($1,$2,$3,$4)",
                &[
                    &f.game,
                    &system_request,
                    &system_original,
                    &json!({"owner":OWNER,"fenceEpoch":"1","candidate":system_candidate}),
                ],
            )
            .await?
            .get(0);
        assert_eq!(system_ack["status"], "success");
        c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE; SET CONSTRAINTS ALL DEFERRED")
            .await?;
        let evidence = "SELECT jsonb_agg(jsonb_build_object('id',id,'source',normalized_source::text,'response',response_payload::text,'hash',payload_hash) ORDER BY id) FROM app.command_receipt WHERE target_ref=$1 AND scope IN('native_opening_v1','game_opening_v1','native_player_v1','native_host_input_v1','host_candidate_v1')";
        let before: Value = c.query_one(evidence, &[&f.game.to_string()]).await?.get(0);
        let upgrade = include_str!(
            "../../../supabase/migrations/20261106023800_native_transition_sources.sql"
        );
        for name in [
            "accept_reserved_game",
            "accept_native_opening",
            "check_native_creator_frame",
            "apply_accept_host_candidate",
            "submit_native_player_command",
            "commit_native_host_input",
        ] {
            c.batch_execute(historical_function(upgrade, name)).await?;
        }
        let paused:Value=c.query_one("SELECT app.pause_game_incident($1,$2,1,0,clock_timestamp(),'storage','fixture:transition-upgrade',$3)", &[&f.game,&OWNER,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(paused["status"], "success");
        let checkpoint = Uuid::parse_str(paused["data"]["checkpointId"].as_str().unwrap())?;
        let restored:Value=c.query_one("SELECT app.restore_game_checkpoint($1,2,1,$2,'fixture:new-transition-host','fixture:transition-restoration',$3)", &[&f.game,&checkpoint,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(restored["status"], "success");
        assert_eq!(
            disposition(&c, f.game, f.game, &opening_terms(&f)).await?["data"]["response"],
            opening_ack
        );
        assert_eq!(
            disposition(&c, f.game, system_request, &system_original).await?["data"]["response"],
            system_ack
        );
        assert_eq!(
            submit_actor(&c, &f, &original, None, controller.as_ref()).await?,
            player_ack
        );
        let retry: Value = c
            .query_one(
                "SELECT app.commit_native_host_input($1,$2,$3,NULL)",
                &[&f.game, &system_request, &system_original],
            )
            .await?
            .get(0);
        assert_eq!(retry, system_ack);
        let after: Value = c.query_one(evidence, &[&f.game.to_string()]).await?.get(0);
        assert_eq!(after, before);
        let anchor = tip(&c, &f).await?;
        let history = page(&c, &f, anchor, None, 32).await?;
        assert!(
            history["data"]["groups"]
                .as_array()
                .unwrap()
                .iter()
                .all(|g| g["eventSchemaRevision"].is_null())
        );
        c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE; ROLLBACK")
            .await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn indexed_creator_lookup_preserves_malformed_output_id_rejection() -> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = native(&c, "bo1").await?;
    host_advance(&c, &f).await?;
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
    let creator:Uuid=c.query_one("SELECT source_receipt_id FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=1", &[&f.game]).await?.get(0);
    for invalid in [
        json!("not-a-uuid"),
        json!("00000000-0000-4000-8000-00000000000A"),
        json!(12),
        Value::Null,
    ] {
        // Deliberate owner-only source corruption, rolled back at every trial.
        // Verify source-check error classification, never accept the bad row.
        c.batch_execute("SAVEPOINT corrupt_source; SET LOCAL session_replication_role=replica")
            .await?;
        c.execute("UPDATE app.command_receipt SET normalized_source=jsonb_set(normalized_source,'{eventIds,0}',$2::jsonb) WHERE id=$1", &[&creator,&invalid]).await?;
        c.batch_execute("SET LOCAL session_replication_role=origin")
            .await?;
        let error = c
            .query_one("SELECT app.check_native_creator_frame($1)", &[&creator])
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
        c.batch_execute("ROLLBACK TO SAVEPOINT corrupt_source")
            .await?;
    }
    c.query_one("SELECT app.check_native_creator_frame($1)", &[&creator])
        .await?;
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[path = "transition_sources/activity.rs"]
mod activity;
