//! Actual blocked-backend races through the composed native boundary owners.
//! Synthetic SQL metadata does not qualify native bytes or provider execution.
use super::*;
#[path = "controller_ante.rs"]
mod controller_ante;

async fn worker() -> FixtureResult<(Client, i32)> {
    let c = connection().await?;
    c.batch_execute("SET ROLE service_role").await?;
    let pid = c.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    Ok((c, pid))
}

async fn own_between(c: &Client, f: &Native, seat: Uuid) -> FixtureResult<(Uuid, Value, Value)> {
    let row=c.query_one("SELECT d.id,m.revision::text,s.source_revision_id FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id JOIN app.match_seat s ON s.id=d.seat_id WHERE d.match_id=$1 AND d.seat_id=$2 AND d.state='pending'", &[&f.matched,&seat]).await?;
    let decision: Uuid = row.get(0);
    let revision: String = row.get(1);
    let selected: Uuid = row.get(2);
    let source: Value = c
        .query_one(
            "SELECT app.read_native_between_source($1,$2::text::numeric,$3)",
            &[&decision, &revision, &selected],
        )
        .await?
        .get(0);
    assert_eq!(source["status"], "success", "{source}");
    let original = json!({"requestId":Uuid::new_v4(),"decisionId":decision,"expectedRevision":revision,"selectedRevisionId":selected});
    let prepared = json!({"version":"native-between-prepared-v1","sourceHash":source["data"]["sourceHash"],"replyRef":"fixture:race-reply","qualificationRef":"fixture:SQL-only"});
    Ok((decision, original, prepared))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_originals_renew_auth_after_room_slot_and_deck_waits() -> TestResult {
    // Countdown's maintained apply path has only the room write lock; unlike
    // Ready it takes no downstream slot/deck lock. Do not invent such a wait.
    for (countdown, wait) in [
        (false, "room"),
        (true, "room"),
        (false, "slot"),
        (false, "deck"),
    ] {
        let c = connection().await?;
        let f = rooms::room(&c, true).await?;
        let revision = rooms::basis(&c, f.id).await?;
        let auth=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1", &[&f.source.actor]).await?;
        let subject: Uuid = auth.get(0);
        let session: Uuid = auth.get(1);
        let source: Value = c
            .query_one(
                "SELECT app.read_native_room_source($1,$2::text::numeric,$3,$4)",
                &[&f.id, &revision, &f.source.release, &f.legality],
            )
            .await?
            .get(0);
        assert_eq!(source["status"], "success");
        let original = json!({"requestId":Uuid::new_v4(),"expectedRevision":revision});
        let mut prepared = json!({"version":if countdown{"native-countdown-prepared-v1"}else{"native-ready-prepared-v1"},"sourceHash":source["data"]["sourceHash"],"releaseId":f.source.release,"legalityRevisionId":f.legality,"qualificationRef":"fixture:race-source"});
        if !countdown {
            prepared["selectedRevisionId"] = json!(f.source.revision);
        }
        c.batch_execute("BEGIN").await?;
        match wait {
            "room" => {
                c.query_one(
                    "SELECT id FROM app.room WHERE id=$1 FOR NO KEY UPDATE",
                    &[&f.id],
                )
                .await?;
            }
            "slot" => {
                c.query_one("SELECT id FROM app.room_slot WHERE room_id=$1 AND account_id=$2 AND retired_at IS NULL FOR NO KEY UPDATE", &[&f.id,&f.source.actor]).await?;
            }
            _ => {
                c.query_one(
                    "SELECT id FROM app.deck_revision WHERE id=$1 FOR UPDATE",
                    &[&f.source.revision],
                )
                .await?;
            }
        }
        let (waiting, pid) = worker().await?;
        let room = f.id;
        let task = tokio::spawn(async move {
            let sql = if countdown {
                "SELECT app.qualify_room_countdown_command($1,$2,$3,$4,$5)"
            } else {
                "SELECT app.qualify_room_ready_command($1,$2,$3,$4,$5)"
            };
            waiting
                .query_one(sql, &[&subject, &session, &room, &original, &prepared])
                .await
                .map(|r| r.get::<_, Value>(0))
        });
        crate::wait_for_database_lock(&c, pid).await?;
        assert_eq!(
            c.execute(
                "DELETE FROM auth.mfa_amr_claims WHERE session_id=$1",
                &[&session]
            )
            .await?,
            1
        );
        c.batch_execute("COMMIT").await?;
        assert_eq!(
            task.await??,
            json!({"status":"fail","data":{"session":"unauthorized"}}),
            "{countdown}/{wait}"
        );
        assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE target_ref=$1 AND scope IN('qualified_ready_intent_v1','qualified_countdown_intent_v1','room_ready','room_countdown')", &[&room.to_string()]).await?.get::<_,i64>(0),0);
        assert_eq!(rooms::basis(&c, room).await?, revision);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn human_round_original_renews_auth_after_match_decision_and_clock_waits() -> TestResult {
    for wait in ["match", "decision", "clock"] {
        let c = connection().await?;
        let f = native(&c, "bo3").await?;
        finish_opening_game_as_draw(&c, &f).await?;
        let (decision, original, prepared) = own_between(&c, &f, f.seat).await?;
        c.batch_execute("BEGIN").await?;
        match wait {
            "match" => {
                c.query_one(
                    "SELECT id FROM app.match WHERE id=$1 FOR NO KEY UPDATE",
                    &[&f.matched],
                )
                .await?;
            }
            "decision" => {
                c.query_one(
                    "SELECT id FROM app.between_game_decision WHERE id=$1 FOR NO KEY UPDATE",
                    &[&decision],
                )
                .await?;
            }
            _ => {
                c.query_one(
                    "SELECT match_seat_id FROM app.match_clock WHERE match_seat_id=$1 FOR UPDATE",
                    &[&f.seat],
                )
                .await?;
            }
        }
        let (waiting, pid) = worker().await?;
        let subject = f.subject;
        let session = f.session;
        let task = tokio::spawn(async move {
            waiting
                .query_one(
                    "SELECT app.qualify_between_game_command($1,$2,$3,$4)",
                    &[&subject, &session, &original, &prepared],
                )
                .await
                .map(|r| r.get::<_, Value>(0))
        });
        crate::wait_for_database_lock(&c, pid).await?;
        c.execute(
            "DELETE FROM auth.mfa_amr_claims WHERE session_id=$1",
            &[&session],
        )
        .await?;
        c.batch_execute("COMMIT").await?;
        assert_eq!(
            task.await??,
            json!({"status":"fail","data":{"session":"unauthorized"}}),
            "{wait}"
        );
        assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE target_ref=$1 AND scope IN('qualified_between_intent_v1','between_game_reply_v1')", &[&decision.to_string()]).await?.get::<_,i64>(0),0);
        assert_eq!(
            c.query_one(
                "SELECT state FROM app.between_game_decision WHERE id=$1",
                &[&decision]
            )
            .await?
            .get::<_, String>(0),
            "pending"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn concurrent_same_original_installs_one_native_frame_and_both_callers_recover_it()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let issued = view(&c, &f, "0", 201).await?;
    let original =
        json!({"requestId":Uuid::new_v4(),"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let prepared = json!({"version":"native-player-accept-v1","basis":basis(&f,"0"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",202),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.match WHERE id=$1 FOR NO KEY UPDATE",
        &[&f.matched],
    )
    .await?;
    let (first, pid) = worker().await?;
    let subject = f.subject;
    let session = f.session;
    let a = original.clone();
    let one = tokio::spawn(async move {
        first
            .query_one(
                "SELECT app.submit_native_human_command($1,$2,$3,$4)",
                &[&subject, &session, &a, &prepared],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    crate::wait_for_database_lock(&c, pid).await?;
    let (second, pid) = worker().await?;
    let b = original.clone();
    let two = tokio::spawn(async move {
        second
            .query_one(
                "SELECT app.submit_native_human_command($1,$2,$3,NULL)",
                &[&subject, &session, &b],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    crate::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    let accepted = one.await??;
    assert_eq!(accepted["status"], "success", "{accepted}");
    assert_eq!(two.await??, accepted);
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "1"
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND request_ref=$1", &[&original["requestId"].as_str().unwrap()]).await?.get::<_,i64>(0),1);
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='host_candidate_v1' AND target_ref=$1", &[&f.game.to_string()]).await?.get::<_,i64>(0),1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn concurrent_view_and_cursor_issuers_retain_the_first_semantic_winner() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    c.batch_execute("BEGIN").await?;
    let first = view(&c, &f, "0", 210).await?;
    let (waiting, pid) = worker().await?;
    let game = f.game;
    let context = f.context;
    let seat = f.seat;
    let other = artifact(&f, "view", 211);
    let task = tokio::spawn(async move {
        waiting
            .query_one(
                "SELECT app.publish_native_view_source($1,$2,'0','0',$3,'live',$4)",
                &[&game, &context, &seat, &other],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    crate::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??, json!({"status":"success","data":first}));
    let receipt: Uuid = first["receiptId"].as_str().unwrap().parse()?;
    c.batch_execute("BEGIN").await?;
    let cursor: Value = c
        .query_one(
            "SELECT app.issue_native_cursor_source($1,$2,'0','0',$3,0,'snapshot',$4,NULL)",
            &[&game, &context, &seat, &receipt],
        )
        .await?
        .get(0);
    let (waiting, pid) = worker().await?;
    let task = tokio::spawn(async move {
        waiting
            .query_one(
                "SELECT app.issue_native_cursor_source($1,$2,'0','0',$3,0,'snapshot',$4,NULL)",
                &[&game, &context, &seat, &receipt],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    crate::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??, cursor);
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE target_ref=$1 AND scope IN('native_view_v1','native_cursor_v1')", &[&game.to_string()]).await?.get::<_,i64>(0),2);
    Ok(())
}

struct Controller {
    binding: Uuid,
    grant: Uuid,
    epoch: String,
    secret: String,
    seat: Uuid,
}
async fn controller(c: &Client, f: &Native) -> FixtureResult<Controller> {
    let row=c.query_one("SELECT b.id,g.id,b.binding_epoch::text,g.grant_secret_ref,s.id FROM app.ai_runner_binding b JOIN app.match_seat s ON s.id=b.match_seat_id JOIN app.controller_grant g ON g.binding_id=b.id AND g.binding_epoch=b.binding_epoch AND g.state='active' WHERE s.match_id=$1 AND s.controller_kind='ai'", &[&f.matched]).await?;
    Ok(Controller {
        binding: row.get(0),
        grant: row.get(1),
        epoch: row.get(2),
        secret: row.get(3),
        seat: row.get(4),
    })
}
async fn controller_proposal(
    c: &Client,
    f: &Native,
    actor: &Controller,
    between: bool,
) -> FixtureResult<(Value, Value)> {
    if between {
        let (_, original, prepared) = own_between(c, f, actor.seat).await?;
        return Ok((original, prepared));
    }
    let issued: Value = c
        .query_one(
            "SELECT app.publish_native_view_source($1,$2,'0','0',$3,'live',$4)",
            &[&f.game, &f.context, &actor.seat, &artifact(f, "view", 220)],
        )
        .await?
        .get(0);
    assert_eq!(issued["status"], "success");
    Ok((
        json!({"requestId":Uuid::new_v4(),"basis":basis(f,"0"),"action":{"kind":"pass"}}),
        json!({"version":"native-player-accept-v1","basis":basis(f,"0"),"actorSeatId":actor.seat,"viewReceiptId":issued["data"]["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(f,"input",221),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(c,f).await?}),
    ))
}
fn controller_sql(between: bool) -> &'static str {
    if between {
        "SELECT app.qualify_controller_between_game_command($1,$2::text::numeric,$3,$4,$5)"
    } else {
        "SELECT app.submit_native_controller_command($1,$2::text::numeric,$3,$4,$5)"
    }
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn controller_move_and_round_serialize_canonical_rebind_after_binding_wait() -> TestResult {
    for between in [false, true] {
        let c = connection().await?;
        let f = native_with_roster(&c, "bo3", false).await?;
        if between {
            finish_opening_game_as_draw(&c, &f).await?;
        }
        let actor = controller(&c, &f).await?;
        let (original, prepared) = controller_proposal(&c, &f, &actor, between).await?;
        let request = original["requestId"].as_str().unwrap().to_owned();
        let replay = original.clone();
        let binding = actor.binding;
        let epoch = actor.epoch.clone();
        let secret = actor.secret.clone();
        c.batch_execute("BEGIN").await?;
        c.query_one(
            "SELECT id FROM app.ai_runner_binding WHERE id=$1 FOR NO KEY UPDATE",
            &[&actor.binding],
        )
        .await?;
        let (waiting, pid) = worker().await?;
        let grant = actor.grant;
        let task = tokio::spawn(async move {
            waiting
                .query_one(
                    controller_sql(between),
                    &[
                        &actor.grant,
                        &actor.epoch,
                        &actor.secret,
                        &original,
                        &prepared,
                    ],
                )
                .await
                .map(|r| r.get::<_, Value>(0))
        });
        crate::wait_for_database_lock(&c, pid).await?;
        // Canonical rebind acquires sponsor/room before binding. Raw owner
        // binding-first revocation instead fires a reverse room-capacity lock
        // and is not an exposed runtime order. Exercise the real owner here.
        let (rebinder, pid) = worker().await?;
        let old_epoch = epoch.clone();
        let rebind = tokio::spawn(async move {
            rebinder.query_one("SELECT app.rebind_ai_runner($1,$2::text::numeric,'fixture:concurrent-rebind',$3)",&[&binding,&old_epoch,&Uuid::new_v4()]).await.map(|r|r.get::<_,Value>(0))
        });
        crate::wait_for_database_lock(&c, pid).await?;
        c.batch_execute("COMMIT").await?;
        let accepted = task.await??;
        assert_eq!(accepted["status"], "success", "{accepted}");
        let rebound = rebind.await??;
        assert_eq!(rebound["status"], "success", "{rebound}");
        c.batch_execute("SET ROLE service_role").await?;
        let denied: Value = c
            .query_one(
                controller_sql(between),
                &[&grant, &epoch, &secret, &replay, &Option::<Value>::None],
            )
            .await?
            .get(0);
        assert_eq!(
            denied,
            json!({"status":"fail","data":{"actor":"unavailable"}})
        );
        c.batch_execute("RESET ROLE").await?;
        assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE request_ref=$1 AND scope IN('native_player_v1','qualified_between_intent_v1') AND state='accepted'", &[&request]).await?.get::<_,i64>(0),1);
        assert_eq!(
            c.query_one(
                "SELECT accepted_revision::text FROM app.game WHERE id=$1",
                &[&f.game]
            )
            .await?
            .get::<_, String>(0),
            "1"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn controller_authority_is_pinned_through_match_wait_before_revocation_can_commit()
-> TestResult {
    for between in [false, true] {
        let c = connection().await?;
        let f = native_with_roster(&c, "bo3", false).await?;
        if between {
            finish_opening_game_as_draw(&c, &f).await?;
        }
        let actor = controller(&c, &f).await?;
        let (original, prepared) = controller_proposal(&c, &f, &actor, between).await?;
        let replay = original.clone();
        let grant = actor.grant;
        let epoch = actor.epoch.clone();
        let secret = actor.secret.clone();
        c.batch_execute("BEGIN").await?;
        c.query_one(
            "SELECT id FROM app.match WHERE id=$1 FOR NO KEY UPDATE",
            &[&f.matched],
        )
        .await?;
        let (waiting, pid) = worker().await?;
        let task = tokio::spawn(async move {
            waiting
                .query_one(
                    controller_sql(between),
                    &[
                        &actor.grant,
                        &actor.epoch,
                        &actor.secret,
                        &original,
                        &prepared,
                    ],
                )
                .await
                .map(|r| r.get::<_, Value>(0))
        });
        crate::wait_for_database_lock(&c, pid).await?;
        // The composed authority helper already holds this grant FOR SHARE.
        // A concurrent revoke must wait, rather than changing authority under it.
        let revoker = connection().await?;
        let pid: i32 = revoker
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let revoke = tokio::spawn(async move {
            revoker
                .execute(
                    "UPDATE app.controller_grant SET state='revoked' WHERE id=$1",
                    &[&grant],
                )
                .await
        });
        crate::wait_for_database_lock(&c, pid).await?;
        c.batch_execute("COMMIT").await?;
        let accepted = task.await??;
        assert_eq!(accepted["status"], "success", "{accepted}");
        assert_eq!(revoke.await??, 1);
        c.batch_execute("SET ROLE service_role").await?;
        let denied: Value = c
            .query_one(
                controller_sql(between),
                &[&grant, &epoch, &secret, &replay, &Option::<Value>::None],
            )
            .await?
            .get(0);
        assert_eq!(
            denied,
            json!({"status":"fail","data":{"actor":"unavailable"}})
        );
        c.batch_execute("RESET ROLE").await?;
        assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE request_ref=$1 AND scope IN('native_player_v1','qualified_between_intent_v1') AND state='accepted'", &[&replay["requestId"].as_str().unwrap()]).await?.get::<_,i64>(0),1);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn incident_during_native_match_wait_rolls_back_then_same_original_can_retry_new_fence()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let issued = view(&c, &f, "0", 230).await?;
    let original =
        json!({"requestId":Uuid::new_v4(),"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let mut prepared = json!({"version":"native-player-accept-v1","basis":basis(&f,"0"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",231),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.match WHERE id=$1 FOR NO KEY UPDATE",
        &[&f.matched],
    )
    .await?;
    let (waiting, pid) = worker().await?;
    let subject = f.subject;
    let session = f.session;
    let intent = original.clone();
    let proposal = prepared.clone();
    let task = tokio::spawn(async move {
        waiting
            .query_one(
                "SELECT app.submit_native_human_command($1,$2,$3,$4)",
                &[&subject, &session, &intent, &proposal],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    crate::wait_for_database_lock(&c, pid).await?;
    let paused:Value=c.query_one("SELECT app.pause_game_incident($1,$2,1,0,clock_timestamp(),'storage','fixture:concurrent-incident',$3)", &[&f.game,&OWNER,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(paused["status"], "success", "{paused}");
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE request_ref=$1 AND scope='native_player_v1'", &[&original["requestId"].as_str().unwrap()]).await?.get::<_,i64>(0),0);
    let checkpoint: Uuid = paused["data"]["checkpointId"].as_str().unwrap().parse()?;
    let restored:Value=c.query_one("SELECT app.restore_game_checkpoint($1,2,1,$2,'fixture:restored-host','fixture:SQL-codec-proof',$3)", &[&f.game,&checkpoint,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(restored["status"], "success", "{restored}");
    prepared["owner"] = json!("fixture:restored-host");
    prepared["fenceEpoch"] = restored["data"]["epoch"].clone();
    prepared["candidate"] = candidate(&c, &f).await?;
    c.batch_execute("SET ROLE service_role").await?;
    let accepted = submit(&c, &f, &original, Some(&prepared)).await?;
    assert_eq!(accepted["status"], "success", "{accepted}");
    assert_eq!(accepted["data"]["epoch"], "0");
    assert_eq!(accepted["data"]["revision"], "1");
    assert_eq!(submit(&c, &f, &original, None).await?, accepted);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn qualified_round_losing_match_wait_to_deadline_expiry_leaves_no_outer_or_extra_reply()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo3").await?;
    finish_opening_game_as_draw(&c, &f).await?;
    let (decision, _, _) = own_between(&c, &f, f.seat).await?;
    let row=c.query_one("SELECT m.revision::text,d.clock_revision::text,m.availability_revision::text,m.profile_id FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id WHERE d.id=$1", &[&decision]).await?;
    let revision: String = row.get(0);
    let clock: String = row.get(1);
    let availability: String = row.get(2);
    let profile: Uuid = row.get(3);
    let armed:Value=c.query_one("SELECT app.schedule_between_game_deadline($1,$2::text::numeric,0,$3::text::numeric,$4::text::numeric,$5,'fixture:timer',clock_timestamp(),'fixture:timing',$6)", &[&decision,&revision,&clock,&availability,&profile,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(armed["status"], "success");
    let (_, original, prepared) = own_between(&c, &f, f.seat).await?;
    let request = original["requestId"].as_str().unwrap().to_owned();
    let selected: Uuid = original["selectedRevisionId"].as_str().unwrap().parse()?;
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.match WHERE id=$1 FOR NO KEY UPDATE",
        &[&f.matched],
    )
    .await?;
    let (waiting, pid) = worker().await?;
    let subject = f.subject;
    let session = f.session;
    let task = tokio::spawn(async move {
        waiting
            .query_one(
                "SELECT app.qualify_between_game_command($1,$2,$3,$4)",
                &[&subject, &session, &original, &prepared],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    crate::wait_for_database_lock(&c, pid).await?;
    let expired:Value=c.query_one("SELECT app.expire_between_game_deadline($1,1,$2,'fixture:default-reply','fixture:qualified-default',$3)", &[&decision,&selected,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(expired["status"], "success", "{expired}");
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE request_ref=$1 AND scope='qualified_between_intent_v1'", &[&request]).await?.get::<_,i64>(0),0);
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE target_ref=$1 AND scope='between_game_reply_v1'", &[&decision.to_string()]).await?.get::<_,i64>(0),1);
    assert_eq!(
        c.query_one(
            "SELECT state FROM app.between_game_decision WHERE id=$1",
            &[&decision]
        )
        .await?
        .get::<_, String>(0),
        "answered"
    );
    Ok(())
}
