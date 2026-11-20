//! Expiry is a qualified native selection through the ordinary sealed-pool transaction.
use super::{TestResult, between_deadline_integrity as deadlines, connection};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn selected(c: &Client, decision: Uuid) -> FixtureResult<Uuid> {
    Ok(c.query_one("SELECT s.source_revision_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.id=$1",&[&decision]).await?.get(0))
}
async fn expire(
    c: &Client,
    decision: Uuid,
    revision: &str,
    chosen: Uuid,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let value:Value=c.query_one("SELECT app.expire_between_game_deadline($1,$2::text::numeric,$3,'fixture:native-expired-selection','fixture:native-expiry-proof',$4)",&[&decision,&revision,&chosen,&request]).await?.get(0);
    let original = spellwake_application::deadline_ports::ExpireDeadline {
        decision_id: decision,
        deadline_revision: revision.parse().unwrap(),
        selected_revision_id: chosen,
        reply_ref: "fixture:native-expired-selection".parse().unwrap(),
        qualification_ref: "fixture:native-expiry-proof".parse().unwrap(),
        request_id: request,
    };
    let decoded = original
        .decode_response(&serde_json::to_vec(&value).unwrap())
        .unwrap();
    if let spellwake_application::ApiResponse::Success { data } = decoded {
        let retained=c.query_one("SELECT (normalized_source->>'replyReceiptId')::uuid,normalized_source->'replyInput'->>'basis' FROM app.command_receipt WHERE principal_key='between-expiry:'||$1::uuid::text AND scope='between_deadline_expiry_v1' AND request_ref=$2::uuid::text",&[&decision,&request]).await?;
        let basis: String = retained.get(1);
        assert!(data.matches_source(retained.get(0), &basis.parse().unwrap()));
    }
    Ok(value)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn due_selection_uses_normal_reply_then_reserves_one_successor_and_replays_history()
-> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    let time = deadlines::due(&c, "0 seconds").await?;
    assert_eq!(
        deadlines::arm(&c, &f, &time, Uuid::new_v4()).await?["status"],
        "success"
    );
    let chosen = selected(&c, f.decision).await?;
    let request = Uuid::new_v4();
    let result = expire(&c, f.decision, "1", chosen, request).await?;
    assert_eq!(result["status"], "success");
    assert_eq!(expire(&c, f.decision, "1", chosen, request).await?, result);
    let other=c.query_one("SELECT d.id,m.revision::text FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id WHERE d.match_id=$1 AND d.state='pending'",&[&f.matched]).await?;
    let id: Uuid = other.get(0);
    let basis: String = other.get(1);
    let revision = selected(&c, id).await?;
    let reply:Value=c.query_one("SELECT app.reply_between_games($1,$2::text::numeric,$3,'fixture:peer-selection','fixture:qualified',$4)",&[&id,&basis,&revision,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(reply["status"], "success");
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.game WHERE match_id=$1 AND game_index=1",
            &[&f.matched]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    assert_eq!(expire(&c, f.decision, "1", chosen, request).await?, result);
    assert_eq!(
        expire(&c, f.decision, "1", chosen, Uuid::new_v4())
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn early_and_obsolete_expiry_roll_back_both_command_identities() -> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    let time = deadlines::due(&c, "1 hour").await?;
    deadlines::arm(&c, &f, &time, Uuid::new_v4()).await?;
    let chosen = selected(&c, f.decision).await?;
    for generation in ["1", "2"] {
        let request = Uuid::new_v4();
        assert_eq!(
            expire(&c, f.decision, generation, chosen, request)
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::T_R_SERIALIZATION_FAILURE)
        );
        assert_eq!(
            c.query_one(
                "SELECT count(*) FROM app.command_receipt WHERE request_ref=$1",
                &[&request.to_string()]
            )
            .await?
            .get::<_, i64>(0),
            0
        );
    }
    assert!(
        c.query_one(
            "SELECT state='pending' FROM app.between_game_decision WHERE id=$1",
            &[&f.decision]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ordinary_late_reply_is_rejected_but_native_expiry_retains_the_original_pool() -> TestResult
{
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    deadlines::arm(
        &c,
        &f,
        &deadlines::due(&c, "0 seconds").await?,
        Uuid::new_v4(),
    )
    .await?;
    let chosen = selected(&c, f.decision).await?;
    let reply:Value=c.query_one("SELECT app.reply_between_games($1,$2::text::numeric,$3,'fixture:late-human','fixture:qualified',$4)",&[&f.decision,&f.basis,&chosen,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(reply["status"], "fail");
    assert!(reply["data"]["deadline"].is_string());
    // A real sealed revision belonging to a different participant is not authority
    // to replace this seat's immutable pool, including during qualified expiry.
    let other:Uuid=c.query_one("SELECT s.source_revision_id FROM app.match_seat s JOIN app.between_game_decision d ON d.match_id=s.match_id WHERE d.id=$1 AND s.id<>d.seat_id LIMIT 1",&[&f.decision]).await?.get(0);
    assert_eq!(
        expire(&c, f.decision, "1", other, Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(
        expire(&c, f.decision, "1", chosen, Uuid::new_v4()).await?["status"],
        "success"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn raw_late_reply_and_orphan_expiry_cannot_commit_valid_looking_receipts() -> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    deadlines::arm(
        &c,
        &f,
        &deadlines::due(&c, "0 seconds").await?,
        Uuid::new_v4(),
    )
    .await?;
    let chosen = selected(&c, f.decision).await?;
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    let receipt:Uuid=c.query_one("SELECT (app.claim_between_game_reply($1,$2::text::numeric,$3,'fixture:raw','fixture:qualified',$4)).id",&[&f.decision,&f.basis,&chosen,&Uuid::new_v4()]).await?.get(0);
    c.execute("UPDATE app.between_game_decision SET state='answered',selected_deck_revision_id=$2,reply_ref='fixture:raw' WHERE id=$1",&[&f.decision,&chosen]).await?;
    c.query_one("SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid,'revision',($3::text::numeric+1)::text,'qualificationRef','fixture:qualified'))",&[&receipt,&f.decision,&f.basis]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    let orphan:Uuid=c.query_one("SELECT (app.begin_between_deadline_command('between_deadline_expiry_v1',$1,$2,'{}'::jsonb)).id",&[&f.decision,&Uuid::new_v4()]).await?.get(0);
    c.query_one(
        "SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid))",
        &[&orphan, &f.decision],
    )
    .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        expire(&c, f.decision, "1", chosen, Uuid::new_v4()).await?["status"],
        "success"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn fabricated_future_observations_cannot_bypass_a_real_future_cutoff() -> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    deadlines::arm(&c, &f, &deadlines::due(&c, "1 hour").await?, Uuid::new_v4()).await?;
    let chosen = selected(&c, f.decision).await?;
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    let input: Value = serde_json::json!({"decisionId":f.decision,"deadlineRevision":"1","selectedRevisionId":chosen,"replyRef":"fixture:forged","proofRef":"fixture:native"});
    let outer: Uuid = c
        .query_one(
            "SELECT (app.begin_between_deadline_command('between_deadline_expiry_v1',$1,$2,$3)).id",
            &[&f.decision, &Uuid::new_v4(), &input],
        )
        .await?
        .get(0);
    let inner:Uuid=c.query_one("SELECT (app.claim_between_game_reply($1,$2::text::numeric,$3,'fixture:forged','fixture:native',$4)).id",&[&f.decision,&f.basis,&chosen,&outer]).await?.get(0);
    let terms = serde_json::json!({"decision":f.decision,"basis":f.basis,"revision":chosen,"reply":"fixture:forged","qualification":"fixture:native"});
    let snapshot: Value = c
        .query_one("SELECT app.between_deadline_snapshot($1)", &[&f.decision])
        .await?
        .get(0);
    let time:String=c.query_one("SELECT to_char((deadline_at+interval '1 microsecond') AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') FROM app.between_game_decision WHERE id=$1",&[&f.decision]).await?.get(0);
    let inner_source = serde_json::json!({"version":"between-game-reply-v1","input":terms,"acceptedAt":time,"deadlineSnapshot":snapshot,"expiryReceiptId":outer});
    let outer_source = serde_json::json!({"version":"between-deadline-expiry-v1","input":input,"snapshot":snapshot,"observedAt":time,"replyReceiptId":inner,"replyInput":terms});
    c.execute(
        "UPDATE app.command_receipt SET normalized_source=$2 WHERE id=$1",
        &[&inner, &inner_source],
    )
    .await?;
    c.execute(
        "UPDATE app.command_receipt SET normalized_source=$2 WHERE id=$1",
        &[&outer, &outer_source],
    )
    .await?;
    c.execute("UPDATE app.between_game_decision SET state='answered',selected_deck_revision_id=$2,reply_ref='fixture:forged' WHERE id=$1",&[&f.decision,&chosen]).await?;
    c.execute(
        "UPDATE app.match SET revision=revision+1 WHERE id=$1",
        &[&f.matched],
    )
    .await?;
    let revision: String = c
        .query_one(
            "SELECT revision::text FROM app.match WHERE id=$1",
            &[&f.matched],
        )
        .await?
        .get(0);
    c.query_one("SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid,'revision',$3::text,'reservedGameId','','qualificationRef','fixture:native'))",&[&inner,&f.decision,&revision]).await?;
    c.query_one("SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid,'replyReceiptId',$3::uuid,'deadlineRevision','1','revision',$4::text,'reservedGameId',''))",&[&outer,&f.decision,&inner,&revision]).await?;
    let error = c.batch_execute("COMMIT").await.unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    assert!(
        c.query_one(
            "SELECT state='pending' FROM app.between_game_decision WHERE id=$1",
            &[&f.decision]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn final_expiry_reserves_successor_and_retains_original_inner_source_on_replay() -> TestResult
{
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    // Answer the peer first, so qualified expiry is the operation reserving game2.
    let peer=c.query_one("SELECT d.id,s.source_revision_id,m.revision::text FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id JOIN app.match m ON m.id=d.match_id WHERE d.match_id=$1 AND d.id<>$2",&[&f.matched,&f.decision]).await?;
    let peer_id: Uuid = peer.get(0);
    let peer_revision: Uuid = peer.get(1);
    let basis: String = peer.get(2);
    let request = Uuid::new_v4();
    let reply:Value=c.query_one("SELECT app.reply_between_games($1,$2::text::numeric,$3,'fixture:peer-first','fixture:qualified',$4)",&[&peer_id,&basis,&peer_revision,&request]).await?.get(0);
    let original = spellwake_application::deadline_ports::BetweenGameReply {
        decision_id: peer_id,
        basis: basis.parse()?,
        selected_revision_id: peer_revision,
        reply_ref: "fixture:peer-first".parse()?,
        qualification_ref: "fixture:qualified".parse()?,
        request_id: request,
    };
    assert!(
        original
            .decode_response(&serde_json::to_vec(&reply)?)
            .is_ok()
    );
    assert_eq!(reply["data"]["reservedGameId"], "");
    let basis: String = c
        .query_one(
            "SELECT revision::text FROM app.match WHERE id=$1",
            &[&f.matched],
        )
        .await?
        .get(0);
    let updated = deadlines::Fixture { basis, ..f };
    deadlines::arm(
        &c,
        &updated,
        &deadlines::due(&c, "0 seconds").await?,
        Uuid::new_v4(),
    )
    .await?;
    let chosen = selected(&c, updated.decision).await?;
    let request = Uuid::new_v4();
    let result = expire(&c, updated.decision, "1", chosen, request).await?;
    let successor: Uuid = result["data"]["reservedGameId"].as_str().unwrap().parse()?;
    let expected: Uuid = c
        .query_one(
            "SELECT reserved_game_id FROM app.match WHERE id=$1",
            &[&updated.matched],
        )
        .await?
        .get(0);
    assert_eq!(successor, expected);
    assert_eq!(
        expire(&c, updated.decision, "1", chosen, request).await?,
        result
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.game WHERE match_id=$1 AND game_index=1",
            &[&updated.matched]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rejected_deadline_selection_sources_replay_after_answer_and_conflict_on_changed_terms()
-> TestResult {
    use spellwake_application::{
        ApiResponse,
        deadline_ports::{BetweenGameReply, ScheduleDeadline},
    };
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    let time = deadlines::due(&c, "0 seconds").await?;
    let missing = Uuid::new_v4();
    let schedule_request = Uuid::new_v4();
    let original = ScheduleDeadline {
        decision_id: missing,
        match_revision: f.basis.parse()?,
        deadline_revision: 0.into(),
        clock_revision: f.clock.parse()?,
        availability_revision: f.availability.parse()?,
        profile_id: f.profile,
        policy_ref: "fixture:policy".parse()?,
        due_at: time.parse()?,
        qualification_ref: "fixture:proof".parse()?,
        request_id: schedule_request,
    };
    let rejected:Value=c.query_one("SELECT app.schedule_between_game_deadline($1,$2::text::numeric,0,$3::text::numeric,$4::text::numeric,$5,'fixture:policy',$6::text::timestamptz,$7,$8)",&[&missing,&f.basis,&f.clock,&f.availability,&f.profile,&time,&"fixture:proof",&schedule_request]).await?.get(0);
    assert!(matches!(
        original.decode_response(&serde_json::to_vec(&rejected)?)?,
        ApiResponse::Fail { .. }
    ));
    deadlines::arm(&c, &f, &time, Uuid::new_v4()).await?;
    assert_eq!(c.query_one("SELECT app.schedule_between_game_deadline($1,$2::text::numeric,0,$3::text::numeric,$4::text::numeric,$5,'fixture:policy',$6::text::timestamptz,$7,$8)",&[&missing,&f.basis,&f.clock,&f.availability,&f.profile,&time,&"fixture:proof",&schedule_request]).await?.get::<_,Value>(0),rejected);
    assert_eq!(c.query_one("SELECT app.schedule_between_game_deadline($1,$2::text::numeric,0,$3::text::numeric,$4::text::numeric,$5,'fixture:policy',$6::text::timestamptz,$7,$8)",&[&missing,&f.basis,&f.clock,&f.availability,&f.profile,&time,&"fixture:changed",&schedule_request]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    let chosen = selected(&c, f.decision).await?;
    let reply_request = Uuid::new_v4();
    let reply: Value = c
        .query_one(
            "SELECT app.reply_between_games($1,$2::text::numeric,$3,$4,'fixture:qualified',$5)",
            &[
                &f.decision,
                &f.basis,
                &chosen,
                &"fixture:late",
                &reply_request,
            ],
        )
        .await?
        .get(0);
    let original = BetweenGameReply {
        decision_id: f.decision,
        basis: f.basis.parse()?,
        selected_revision_id: chosen,
        reply_ref: "fixture:late".parse()?,
        qualification_ref: "fixture:qualified".parse()?,
        request_id: reply_request,
    };
    assert!(matches!(
        original.decode_response(&serde_json::to_vec(&reply)?)?,
        ApiResponse::Fail { .. }
    ));
    // Foreign sealed pool gives a stored expiry rejection; valid pool then answers it.
    let foreign:Uuid=c.query_one("SELECT s.source_revision_id FROM app.match_seat s JOIN app.between_game_decision d ON d.match_id=s.match_id WHERE d.id=$1 AND s.id<>d.seat_id LIMIT 1",&[&f.decision]).await?.get(0);
    let expiry_request = Uuid::new_v4();
    let rejected_expiry = expire(&c, f.decision, "1", foreign, expiry_request).await?;
    assert_eq!(rejected_expiry["status"], "fail");
    expire(&c, f.decision, "1", chosen, Uuid::new_v4()).await?;
    assert_eq!(
        expire(&c, f.decision, "1", foreign, expiry_request).await?,
        rejected_expiry
    );
    assert_eq!(
        expire(&c, f.decision, "1", chosen, expiry_request)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT app.reply_between_games($1,$2::text::numeric,$3,$4,'fixture:qualified',$5)",
            &[
                &f.decision,
                &f.basis,
                &chosen,
                &"fixture:late",
                &reply_request
            ]
        )
        .await?
        .get::<_, Value>(0),
        reply
    );
    assert_eq!(
        c.query_one(
            "SELECT app.reply_between_games($1,$2::text::numeric,$3,$4,'fixture:qualified',$5)",
            &[
                &f.decision,
                &f.basis,
                &chosen,
                &"fixture:changed",
                &reply_request
            ]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
