//! Normalized checkpoint/incident proofs test SQL persistence, without a native encode/restore codec.
use super::{TestResult, connection, host_integrity};
use serde_json::Value;
use spellwake_application::{
    ApiResponse,
    recovery_ports::{IncidentKind, PauseGame, PauseMatch, RestoreGame, ResumeMatch},
};
use tokio_postgres::error::SqlState;
use uuid::Uuid;
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn supervised_exit_freezes_at_durable_clock_and_replays_after_host_replacement() -> TestResult
{
    let c = connection().await?;
    let f = host_integrity::fixture(&c).await?;
    let clock_cut: String = c.query_one(
        "SELECT to_char((clock_timestamp() - interval '1 second') AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')", &[]
    ).await?.get(0);
    let accepted = host_integrity::accept(
        &c,
        f.game,
        "1",
        "0",
        Uuid::new_v4(),
        &host_integrity::candidate(&f),
    )
    .await?;
    assert_eq!(accepted["status"], "success");
    let instance = Uuid::new_v4();
    let next = Uuid::new_v4();
    let registration = "SELECT app.register_native_host_watch($1,'fixture-host')";
    let exit = "SELECT app.pause_native_host_exit($1,$2::text::timestamptz,9123,$3)";
    for (id, expected) in [(instance, true), (next, false)] {
        assert_eq!(
            c.query_one(registration, &[&id]).await?.get::<_, Value>(0),
            serde_json::json!({"registered":expected})
        );
    }
    let ack: Value = c
        .query_one(exit, &[&instance, &clock_cut, &"signal:15"])
        .await?
        .get(0);
    assert_eq!(
        ack["data"],
        serde_json::json!({"instanceId":instance,"paused":1,"stale":0})
    );
    let paused=c.query_one("SELECT o.availability,o.incident_at=c.last_accounted_at,o.incident_at>$3::text::timestamptz,c.paused,c.remaining_ms::text FROM app.operational_state o CROSS JOIN app.match_clock c WHERE o.game_id=$1 AND c.match_seat_id=$2", &[&f.game,&f.seat,&clock_cut]).await?;
    assert_eq!(paused.get::<_, String>(0), "recovering");
    assert!(paused.get::<_, bool>(1) && paused.get::<_, bool>(2) && paused.get::<_, bool>(3));
    assert_eq!(paused.get::<_, String>(4), "59000");
    assert_eq!(
        c.query_one(registration, &[&next])
            .await?
            .get::<_, Value>(0)["registered"],
        true
    );
    assert_eq!(
        c.query_one(exit, &[&instance, &clock_cut, &"signal:15"])
            .await?
            .get::<_, Value>(0),
        ack
    );
    assert_eq!(
        c.query_one(exit, &[&instance, &clock_cut, &"exit:0"])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert!(
        !c.query_one(
            "SELECT finished FROM app.native_host_watch WHERE instance_id=$1",
            &[&next]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn incident_fences_old_host_and_restores_exact_checkpoint_without_refilling_or_charging_downtime()
-> TestResult {
    let c = connection().await?;
    let f = host_integrity::fixture(&c).await?;
    let first = host_integrity::accept(
        &c,
        f.game,
        "1",
        "0",
        Uuid::new_v4(),
        &host_integrity::candidate(&f),
    )
    .await?;
    assert_eq!(first["status"], "success");
    let stamp=c.query_one("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')", &[]).await?.get::<_,String>(0);
    let expected:String=c.query_one("SELECT greatest(0,remaining_ms-floor(extract(epoch FROM ($2::text::timestamptz-last_accounted_at))*1000))::text FROM app.match_clock WHERE match_seat_id=$1", &[&f.seat,&stamp]).await?.get(0);
    let request = Uuid::new_v4();
    c.batch_execute("BEGIN").await?;
    let paused:Value=c.query_one("SELECT app.pause_game_incident($1,'fixture-host',1,0,$2::text::timestamptz,'storage','fixture:qualified-incident',$3)", &[&f.game,&stamp,&request]).await?.get(0);
    assert_eq!(paused["status"], "success", "{paused}");
    let original_pause = PauseGame {
        game_id: f.game,
        owner_ref: "fixture-host".parse()?,
        epoch: 1.into(),
        operational_revision: 0.into(),
        incident_at: stamp.parse()?,
        kind: IncidentKind::Storage,
        proof_ref: "fixture:qualified-incident".parse()?,
        request_id: request,
    };
    let decoded = original_pause.decode_response(&serde_json::to_vec(&paused)?)?;
    let ApiResponse::Success { data } = decoded else {
        panic!("expected pause source");
    };
    let source=c.query_one("SELECT cp.id,(SELECT max(clock_revision)::text FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=g.match_id) FROM app.game g JOIN app.game_checkpoint cp ON cp.game_id=g.id AND cp.accepted_revision=g.accepted_revision WHERE g.id=$1",&[&f.game]).await?;
    let source_clock: Option<String> = source.get(1);
    let source_clock = source_clock.map(|value| value.parse()).transpose()?;
    assert!(data.matches_source(source.get(0), source_clock.as_ref()));
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let game = f.game;
    let body = host_integrity::candidate(&f);
    let task = tokio::spawn(async move {
        host_integrity::accept(&waiting, game, "1", "1", Uuid::new_v4(), &body).await
    });
    super::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??["status"], "fail");

    c.batch_execute("SET TIME ZONE 'America/New_York'").await?;
    assert_eq!(c.query_one("SELECT app.pause_game_incident($1,'fixture-host',1,0,$2::text::timestamptz,'storage','fixture:qualified-incident',$3)", &[&f.game,&stamp,&request]).await?.get::<_,Value>(0),paused);
    let row = c
        .query_one(
            "SELECT remaining_ms::text,paused FROM app.match_clock WHERE match_seat_id=$1",
            &[&f.seat],
        )
        .await?;
    assert_eq!(row.get::<_, String>(0), expected);
    assert!(row.get::<_, bool>(1));
    assert_eq!(
        host_integrity::accept(
            &c,
            f.game,
            "1",
            "1",
            Uuid::new_v4(),
            &host_integrity::candidate(&f)
        )
        .await?["status"],
        "fail"
    );
    let checkpoint: Uuid = paused["data"]["checkpointId"].as_str().unwrap().parse()?;
    let request = Uuid::new_v4();
    c.batch_execute("BEGIN").await?;
    c.execute("UPDATE app.match_clock SET remaining_ms=remaining_ms-1,clock_revision=clock_revision+1 WHERE match_seat_id=$1", &[&f.seat]).await?;
    assert_eq!(c.query_one("SELECT app.restore_game_checkpoint($1,2,1,$2,'fixture:replacement-host','fixture:exact-codec-proof',$3)", &[&f.game,&checkpoint,&request]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.batch_execute("ROLLBACK").await?;
    let restored:Value=c.query_one("SELECT app.restore_game_checkpoint($1,2,1,$2,'fixture:replacement-host','fixture:exact-codec-proof',$3)", &[&f.game,&checkpoint,&request]).await?.get(0);
    assert_eq!(restored["status"], "success", "{restored}");
    let original_restore = RestoreGame {
        game_id: f.game,
        epoch: 2.into(),
        operational_revision: 1.into(),
        checkpoint_id: checkpoint,
        new_owner_ref: "fixture:replacement-host".parse()?,
        restoration_ref: "fixture:exact-codec-proof".parse()?,
        request_id: request,
    };
    assert!(matches!(
        original_restore.decode_response(&serde_json::to_vec(&restored)?)?,
        ApiResponse::Success { .. }
    ));
    assert_eq!(c.query_one("SELECT app.restore_game_checkpoint($1,2,1,$2,'fixture:replacement-host','fixture:exact-codec-proof',$3)", &[&f.game,&checkpoint,&request]).await?.get::<_,Value>(0),restored);
    assert_eq!(
        c.query_one(
            "SELECT remaining_ms::text FROM app.match_clock WHERE match_seat_id=$1",
            &[&f.seat]
        )
        .await?
        .get::<_, String>(0),
        expected
    );
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "1"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game_checkpoint WHERE game_id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    assert_eq!(c.execute("UPDATE app.operational_state SET fence_epoch=fence_epoch+1,revision=revision+1 WHERE game_id=$1", &[&f.game]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(c.execute("UPDATE app.operational_state SET incident_receipt_id=NULL,recovery_checkpoint_id=NULL,incident_at=NULL,incident_ref=NULL,restoration_receipt_id=NULL,fence_epoch=fence_epoch+1,revision=revision+1 WHERE game_id=$1", &[&f.game]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn between_game_incident_freezes_decisions_without_a_fake_live_game_and_preserves_the_bank()
-> TestResult {
    let c = connection().await?;
    let (_, m) = super::ante_transaction_integrity::reserve_match(&c, true, "bo3").await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    super::match_lifecycle_integrity::open(&c, m, g).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index LIMIT 1",
            &[&m],
        )
        .await?
        .get(0);
    let result = super::match_lifecycle_integrity::complete(&c, m, g, vec![seat]).await?;
    assert_eq!(
        super::match_lifecycle_integrity::consume(&c, m, result).await?["data"]["phase"],
        "between_games"
    );
    let basis: String = c
        .query_one("SELECT revision::text FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    let clocks:Value=c.query_one("SELECT jsonb_agg(jsonb_build_object('seat',c.match_seat_id,'remaining',c.remaining_ms::text,'revision',c.clock_revision::text) ORDER BY c.match_seat_id) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1", &[&m]).await?.get(0);
    let time:String=c.query_one("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",&[]).await?.get(0);
    let pause_request = Uuid::new_v4();
    let paused:Value=c.query_one("SELECT app.pause_match_between_games($1,$2::text::numeric,0,$3::text::timestamptz,'storage','fixture:match-only-incident',$4)", &[&m,&basis,&time,&pause_request]).await?.get(0);
    let original_pause = PauseMatch {
        match_id: m,
        basis: basis.parse()?,
        availability_basis: 0.into(),
        incident_at: time.parse()?,
        kind: IncidentKind::Storage,
        proof_ref: "fixture:match-only-incident".parse()?,
        request_id: pause_request,
    };
    assert!(matches!(
        original_pause.decode_response(&serde_json::to_vec(&paused)?)?,
        ApiResponse::Success { .. }
    ));
    assert_eq!(paused["status"], "success", "{paused}");
    let incident: Uuid = paused["data"]["incidentId"].as_str().unwrap().parse()?;
    let decision=c.query_one("SELECT d.id,s.source_revision_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.match_id=$1 ORDER BY d.id LIMIT 1", &[&m]).await?;
    let decision_id: Uuid = decision.get(0);
    let revision: Uuid = decision.get(1);
    let paused_basis = paused["data"]["revision"].as_str().unwrap().to_owned();
    let denied:Value=c.query_one("SELECT app.reply_between_games($1,$2::text::numeric,$3,'fixture:selection','fixture:legality',$4)", &[&decision_id,&paused_basis,&revision,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(denied["status"], "fail");
    assert_eq!(c.execute("UPDATE app.between_game_decision SET deadline_at=clock_timestamp()+interval '1 minute' WHERE id=$1", &[&decision_id]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.match_clock SET clock_revision=clock_revision+1 WHERE match_seat_id=$1",
        &[&seat],
    )
    .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.match_clock SET clock_revision=clock_revision+1 WHERE match_seat_id=$1",
        &[&seat],
    )
    .await?;
    assert_eq!(c.query_one("SELECT app.resume_match_between_games($1,$2::text::numeric,1,$3,'fixture:match-only-restored',$4)", &[&m,&paused_basis,&incident,&Uuid::new_v4()]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.batch_execute("ROLLBACK").await?;
    c.batch_execute("BEGIN").await?;
    let resume_request = Uuid::new_v4();
    let restored:Value=c.query_one("SELECT app.resume_match_between_games($1,$2::text::numeric,1,$3,'fixture:match-only-restored',$4)", &[&m,&paused_basis,&incident,&resume_request]).await?.get(0);
    let original_resume = ResumeMatch {
        match_id: m,
        basis: paused_basis.parse()?,
        availability_basis: 1.into(),
        incident_id: incident,
        proof_ref: "fixture:match-only-restored".parse()?,
        request_id: resume_request,
    };
    assert!(matches!(
        original_resume.decode_response(&serde_json::to_vec(&restored)?)?,
        ApiResponse::Success { .. }
    ));
    assert_eq!(restored["status"], "success", "{restored}");
    assert_eq!(c.query_one("SELECT jsonb_agg(jsonb_build_object('seat',c.match_seat_id,'remaining',c.remaining_ms::text,'revision',c.clock_revision::text) ORDER BY c.match_seat_id) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1", &[&m]).await?.get::<_,Value>(0),clocks);
    let restored_basis = restored["data"]["revision"].as_str().unwrap().to_owned();
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let task = tokio::spawn(async move {
        waiting.query_one("SELECT app.reply_between_games($1,$2::text::numeric,$3,'fixture:selection','fixture:legality',$4)", &[&decision_id,&restored_basis,&revision,&Uuid::new_v4()]).await.map(|r|r.get::<_,Value>(0))
    });
    super::wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    let accepted = task.await??;
    assert_eq!(accepted["status"], "success", "{accepted}");
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game WHERE match_id=$1",
            &[&m]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn preopening_incident_does_not_create_native_state_and_admission_waits_for_match_resume()
-> TestResult {
    let c = connection().await?;
    let (_, m) = super::ante_transaction_integrity::reserve(&c, true).await?;
    let before = c
        .query_one(
            "SELECT revision::text,reserved_game_id FROM app.match WHERE id=$1",
            &[&m],
        )
        .await?;
    let basis: String = before.get(0);
    let g: Uuid = before.get(1);
    let request = Uuid::new_v4();
    let time:String=c.query_one("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')", &[]).await?.get(0);
    let paused:Value=c.query_one("SELECT app.pause_match_between_games($1,$2::text::numeric,0,$3::text::timestamptz,'server_process','fixture:opening-not-started',$4)", &[&m,&basis,&time,&request]).await?.get(0);
    assert_eq!(paused["status"], "success", "{paused}");
    let original_pause = PauseMatch {
        match_id: m,
        basis: basis.parse()?,
        availability_basis: 0.into(),
        incident_at: time.parse()?,
        kind: IncidentKind::ServerProcess,
        proof_ref: "fixture:opening-not-started".parse()?,
        request_id: request,
    };
    assert!(matches!(
        original_pause.decode_response(&serde_json::to_vec(&paused)?)?,
        ApiResponse::Success { .. }
    ));
    assert_eq!(c.query_one("SELECT app.pause_match_between_games($1,$2::text::numeric,0,$3::text::timestamptz,'server_process','fixture:opening-not-started',$4)", &[&m,&basis,&time,&request]).await?.get::<_,Value>(0),paused);
    assert_eq!(
        super::match_lifecycle_integrity::open(&c, m, g)
            .await
            .unwrap_err()
            .downcast_ref::<tokio_postgres::Error>()
            .unwrap()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game_checkpoint WHERE game_id=$1",
            &[&g]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    let incident: Uuid = paused["data"]["incidentId"].as_str().unwrap().parse()?;
    let paused_basis = paused["data"]["revision"].as_str().unwrap();
    let resume_request = Uuid::new_v4();
    let restored:Value=c.query_one("SELECT app.resume_match_between_games($1,$2::text::numeric,1,$3,'fixture:opening-available',$4)", &[&m,&paused_basis,&incident,&resume_request]).await?.get(0);
    let original_resume = ResumeMatch {
        match_id: m,
        basis: paused_basis.parse()?,
        availability_basis: 1.into(),
        incident_id: incident,
        proof_ref: "fixture:opening-available".parse()?,
        request_id: resume_request,
    };
    assert!(matches!(
        original_resume.decode_response(&serde_json::to_vec(&restored)?)?,
        ApiResponse::Success { .. }
    ));
    assert_eq!(restored["status"], "success", "{restored}");
    assert_eq!(
        super::match_lifecycle_integrity::open(&c, m, g).await?["status"],
        "success"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game WHERE match_id=$1",
            &[&m]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rejected_recovery_sources_replay_original_terms_after_frontier_changes() -> TestResult {
    let c = connection().await?;
    let f = host_integrity::fixture(&c).await?;
    assert_eq!(
        host_integrity::accept(
            &c,
            f.game,
            "1",
            "0",
            Uuid::new_v4(),
            &host_integrity::candidate(&f)
        )
        .await?["status"],
        "success"
    );
    let time:String=c.query_one("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",&[]).await?.get(0);
    let pause_id = Uuid::new_v4();
    let rejected_pause:Value=c.query_one("SELECT app.pause_game_incident($1,'wrong-owner',1,0,$2::text::timestamptz,'storage',$3,$4)",&[&f.game,&time,&"fixture:rejected",&pause_id]).await?.get(0);
    let pause = PauseGame {
        game_id: f.game,
        owner_ref: "wrong-owner".parse()?,
        epoch: 1.into(),
        operational_revision: 0.into(),
        incident_at: time.parse()?,
        kind: IncidentKind::Storage,
        proof_ref: "fixture:rejected".parse()?,
        request_id: pause_id,
    };
    assert!(matches!(
        pause.decode_response(&serde_json::to_vec(&rejected_pause)?)?,
        ApiResponse::Fail { .. }
    ));
    let paused:Value=c.query_one("SELECT app.pause_game_incident($1,'fixture-host',1,0,$2::text::timestamptz,'storage','fixture:accepted',$3)",&[&f.game,&time,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(paused["status"], "success", "{paused}");
    assert_eq!(c.query_one("SELECT app.pause_game_incident($1,'wrong-owner',1,0,$2::text::timestamptz,'storage',$3,$4)",&[&f.game,&time,&"fixture:rejected",&pause_id]).await?.get::<_,Value>(0),rejected_pause);
    assert_eq!(c.query_one("SELECT app.pause_game_incident($1,'wrong-owner',1,0,$2::text::timestamptz,'storage',$3,$4)",&[&f.game,&time,&"fixture:changed",&pause_id]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    let checkpoint: Uuid = paused["data"]["checkpointId"].as_str().unwrap().parse()?;
    let wrong_checkpoint = Uuid::new_v4();
    let restore_id = Uuid::new_v4();
    let rejected_restore: Value = c
        .query_one(
            "SELECT app.restore_game_checkpoint($1,2,1,$2,'next-owner',$3,$4)",
            &[&f.game, &wrong_checkpoint, &"fixture:rejected", &restore_id],
        )
        .await?
        .get(0);
    let restore = RestoreGame {
        game_id: f.game,
        epoch: 2.into(),
        operational_revision: 1.into(),
        checkpoint_id: wrong_checkpoint,
        new_owner_ref: "next-owner".parse()?,
        restoration_ref: "fixture:rejected".parse()?,
        request_id: restore_id,
    };
    assert!(matches!(
        restore.decode_response(&serde_json::to_vec(&rejected_restore)?)?,
        ApiResponse::Fail { .. }
    ));
    let restored: Value = c
        .query_one(
            "SELECT app.restore_game_checkpoint($1,2,1,$2,'next-owner','fixture:accepted',$3)",
            &[&f.game, &checkpoint, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(restored["status"], "success", "{restored}");
    assert_eq!(
        c.query_one(
            "SELECT app.restore_game_checkpoint($1,2,1,$2,'next-owner',$3,$4)",
            &[&f.game, &wrong_checkpoint, &"fixture:rejected", &restore_id]
        )
        .await?
        .get::<_, Value>(0),
        rejected_restore
    );
    assert_eq!(
        c.query_one(
            "SELECT app.restore_game_checkpoint($1,2,1,$2,'next-owner',$3,$4)",
            &[&f.game, &wrong_checkpoint, &"fixture:changed", &restore_id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );

    let (_, m) = super::ante_transaction_integrity::reserve(&c, true).await?;
    let basis: String = c
        .query_one("SELECT revision::text FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    let time:String=c.query_one("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",&[]).await?.get(0);
    let pause_id = Uuid::new_v4();
    // Wrong availability, not an invalid input envelope.
    let rejected_pause:Value=c.query_one("SELECT app.pause_match_between_games($1,$2::text::numeric,99,$3::text::timestamptz,'storage',$4,$5)",&[&m,&basis,&time,&"fixture:rejected",&pause_id]).await?.get(0);
    let pause = PauseMatch {
        match_id: m,
        basis: basis.parse()?,
        availability_basis: 99.into(),
        incident_at: time.parse()?,
        kind: IncidentKind::Storage,
        proof_ref: "fixture:rejected".parse()?,
        request_id: pause_id,
    };
    assert!(matches!(
        pause.decode_response(&serde_json::to_vec(&rejected_pause)?)?,
        ApiResponse::Fail { .. }
    ));
    let paused:Value=c.query_one("SELECT app.pause_match_between_games($1,$2::text::numeric,0,$3::text::timestamptz,'storage','fixture:accepted',$4)",&[&m,&basis,&time,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(paused["status"], "success", "{paused}");
    assert_eq!(c.query_one("SELECT app.pause_match_between_games($1,$2::text::numeric,99,$3::text::timestamptz,'storage',$4,$5)",&[&m,&basis,&time,&"fixture:rejected",&pause_id]).await?.get::<_,Value>(0),rejected_pause);
    assert_eq!(c.query_one("SELECT app.pause_match_between_games($1,$2::text::numeric,99,$3::text::timestamptz,'storage',$4,$5)",&[&m,&basis,&time,&"fixture:changed",&pause_id]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    let paused_basis = paused["data"]["revision"].as_str().unwrap();
    let incident: Uuid = paused["data"]["incidentId"].as_str().unwrap().parse()?;
    let wrong_incident = Uuid::new_v4();
    let resume_id = Uuid::new_v4();
    let rejected_resume: Value = c
        .query_one(
            "SELECT app.resume_match_between_games($1,$2::text::numeric,1,$3,$4,$5)",
            &[
                &m,
                &paused_basis,
                &wrong_incident,
                &"fixture:rejected",
                &resume_id,
            ],
        )
        .await?
        .get(0);
    let resume = ResumeMatch {
        match_id: m,
        basis: paused_basis.parse()?,
        availability_basis: 1.into(),
        incident_id: wrong_incident,
        proof_ref: "fixture:rejected".parse()?,
        request_id: resume_id,
    };
    assert!(matches!(
        resume.decode_response(&serde_json::to_vec(&rejected_resume)?)?,
        ApiResponse::Fail { .. }
    ));
    let restored:Value=c.query_one("SELECT app.resume_match_between_games($1,$2::text::numeric,1,$3,'fixture:accepted',$4)",&[&m,&paused_basis,&incident,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(restored["status"], "success", "{restored}");
    assert_eq!(
        c.query_one(
            "SELECT app.resume_match_between_games($1,$2::text::numeric,1,$3,$4,$5)",
            &[
                &m,
                &paused_basis,
                &wrong_incident,
                &"fixture:rejected",
                &resume_id
            ]
        )
        .await?
        .get::<_, Value>(0),
        rejected_resume
    );
    assert_eq!(
        c.query_one(
            "SELECT app.resume_match_between_games($1,$2::text::numeric,1,$3,$4,$5)",
            &[
                &m,
                &paused_basis,
                &wrong_incident,
                &"fixture:changed",
                &resume_id
            ]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
