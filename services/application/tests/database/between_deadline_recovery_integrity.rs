//! Exact qualified between-game timer accounting across application availability.
use super::{
    TestResult, between_deadline_integrity as deadlines, connection, wait_for_database_lock,
};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn snapshot(c: &Client, d: Uuid) -> FixtureResult<Value> {
    Ok(
        c.query_one("SELECT app.between_deadline_snapshot($1)", &[&d])
            .await?
            .get(0),
    )
}
async fn pause(c: &Client, m: Uuid) -> FixtureResult<Value> {
    Ok(c.query_one("SELECT app.pause_match_between_games($1,(SELECT revision FROM app.match WHERE id=$1),(SELECT availability_revision FROM app.match WHERE id=$1),clock_timestamp(),'storage','fixture:timer-pause',$2)",&[&m,&Uuid::new_v4()]).await?.get(0))
}
async fn resume(c: &Client, m: Uuid, incident: Uuid) -> FixtureResult<Value> {
    Ok(c.query_one("SELECT app.resume_match_between_games($1,(SELECT revision FROM app.match WHERE id=$1),(SELECT availability_revision FROM app.match WHERE id=$1),$2,'fixture:timer-resume',$3)",&[&m,&incident,&Uuid::new_v4()]).await?.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn repeated_outages_preserve_exact_remaining_duration_and_historical_origin() -> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    deadlines::arm(
        &c,
        &f,
        &deadlines::due(&c, "1 hour 0.000123 seconds").await?,
        Uuid::new_v4(),
    )
    .await?;
    let original = snapshot(&c, f.decision).await?;
    for cycle in 0..2 {
        let paused = pause(&c, f.matched).await?;
        assert_eq!(paused["status"], "success", "{paused}");
        let frozen = snapshot(&c, f.decision).await?;
        assert!(frozen["dueAt"].is_null());
        assert!(frozen["remainingUs"].is_string());
        assert!(c.query_one("SELECT deadline_remaining_us=greatest(0,extract(epoch FROM (($2::jsonb->>'dueAt')::timestamptz-deadline_accounted_at))*1000000) FROM app.between_game_decision WHERE id=$1",&[&f.decision,&if cycle==0{original.clone()}else{c.query_one("SELECT normalized_source->'beforeDeadlineSnapshots'->0 FROM app.command_receipt WHERE id=$1",&[&paused["data"]["incidentId"].as_str().unwrap().parse::<Uuid>()?]).await?.get(0)}]).await?.get::<_,bool>(0));
        let id: Uuid = paused["data"]["incidentId"].as_str().unwrap().parse()?;
        assert_eq!(resume(&c, f.matched, id).await?["status"], "success");
        let restored = snapshot(&c, f.decision).await?;
        assert!(restored["remainingUs"].is_null());
        assert_eq!(restored["originReceiptId"], original["originReceiptId"]);
        assert!(c.query_one("SELECT deadline_at=deadline_accounted_at+(($2::jsonb->>'remainingUs')||' microseconds')::interval FROM app.between_game_decision WHERE id=$1",&[&f.decision,&frozen]).await?.get::<_,bool>(0));
        c.query_one("SELECT app.check_between_deadline($1)", &[&f.decision])
            .await?;
    }
    c.query_one(
        "SELECT app.check_between_deadline_snapshot($1)",
        &[&original],
    )
    .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn already_due_timer_stays_due_after_outage_and_discovery_excludes_frozen_rows() -> TestResult
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
    let page: Value = c
        .query_one("SELECT app.read_due_between_game_deadlines(NULL,100)", &[])
        .await?
        .get(0);
    let query = spellwake_application::deadline_ports::DueDeadlines {
        after: None,
        limit: 100,
    };
    assert!(query.decode_response(&serde_json::to_vec(&page)?).is_ok());
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["decisionId"] == f.decision.to_string())
    );
    let paused = pause(&c, f.matched).await?;
    assert_eq!(paused["status"], "success");
    let frozen = snapshot(&c, f.decision).await?;
    assert_eq!(frozen["remainingUs"], "0");
    let page: Value = c
        .query_one("SELECT app.read_due_between_game_deadlines(NULL,100)", &[])
        .await?
        .get(0);
    let query = spellwake_application::deadline_ports::DueDeadlines {
        after: None,
        limit: 100,
    };
    assert!(query.decode_response(&serde_json::to_vec(&page)?).is_ok());
    assert!(
        !page["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["decisionId"] == f.decision.to_string())
    );
    let incident = paused["data"]["incidentId"].as_str().unwrap().parse()?;
    assert_eq!(resume(&c, f.matched, incident).await?["status"], "success");
    let resumed_page: Value = c
        .query_one("SELECT app.read_due_between_game_deadlines(NULL,100)", &[])
        .await?
        .get(0);
    let resumed = query.decode_response(&serde_json::to_vec(&resumed_page)?)?;
    assert!(
        resumed
            .items
            .iter()
            .any(|timer| timer.decision_id == f.decision && timer.revision == 3.into())
    );
    let chosen:Uuid=c.query_one("SELECT s.source_revision_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.id=$1",&[&f.decision]).await?.get(0);
    assert_eq!(
        c.query_one(
            "SELECT app.expire_between_game_deadline($1,1,$2,'fixture:stale','fixture:native',$3)",
            &[&f.decision, &chosen, &Uuid::new_v4()]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    let request = Uuid::new_v4();
    let result: Value = c
        .query_one(
            "SELECT app.expire_between_game_deadline($1,3,$2,'fixture:expiry','fixture:native',$3)",
            &[&f.decision, &chosen, &request],
        )
        .await?
        .get(0);
    assert_eq!(result["status"], "success");
    let original = spellwake_application::deadline_ports::ExpireDeadline {
        decision_id: f.decision,
        deadline_revision: 3.into(),
        selected_revision_id: chosen,
        reply_ref: "fixture:expiry".parse()?,
        qualification_ref: "fixture:native".parse()?,
        request_id: request,
    };
    assert!(matches!(
        original.decode_response(&serde_json::to_vec(&result)?)?,
        spellwake_application::ApiResponse::Success { .. }
    ));
    assert!(
        c.query_one("SELECT app.read_due_between_game_deadlines(NULL,101)", &[])
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn frozen_timer_rejects_raw_refill_and_old_incident_boundary() -> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    deadlines::arm(&c, &f, &deadlines::due(&c, "1 hour").await?, Uuid::new_v4()).await?;
    // This timestamp precedes the acknowledged arm, even though clocks themselves
    // were already paused earlier. It cannot refund accepted timer time.
    assert_eq!(c.query_one("SELECT app.pause_match_between_games($1,(SELECT revision FROM app.match WHERE id=$1),0,(SELECT opened_at FROM app.between_game_decision WHERE id=$2),'storage','fixture:old-boundary',$3)",&[&f.matched,&f.decision,&Uuid::new_v4()]).await.unwrap_err().code(),Some(&SqlState::T_R_SERIALIZATION_FAILURE));
    let paused = pause(&c, f.matched).await?;
    assert_eq!(paused["status"], "success");
    c.batch_execute("BEGIN").await?;
    c.execute("UPDATE app.between_game_decision SET deadline_remaining_us=deadline_remaining_us+1,deadline_revision=deadline_revision+1 WHERE id=$1",&[&f.decision]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.query_one("SELECT app.check_between_deadline($1)", &[&f.decision])
        .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn arm_and_pause_share_the_match_fence_in_both_orders() -> TestResult {
    for arm_first in [true, false] {
        let c = connection().await?;
        let f = deadlines::fixture(&c).await?;
        let waiting = connection().await?;
        let pid: i32 = waiting
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let time = deadlines::due(&c, "1 hour").await?;
        c.batch_execute("BEGIN").await?;
        if arm_first {
            assert_eq!(
                deadlines::arm(&c, &f, &time, Uuid::new_v4()).await?["status"],
                "success"
            );
            let matched = f.matched;
            let task = tokio::spawn(async move { pause(&waiting, matched).await });
            wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "success");
            let s = snapshot(&c, f.decision).await?;
            assert_eq!(s["revision"], "2");
            assert!(s["dueAt"].is_null());
        } else {
            assert_eq!(pause(&c, f.matched).await?["status"], "success");
            let decision = f.decision;
            let basis = f.basis.clone();
            let clock = f.clock.clone();
            let availability = f.availability.clone();
            let profile = f.profile;
            let task = tokio::spawn(async move {
                waiting.query_one("SELECT app.schedule_between_game_deadline($1,$2::text::numeric,0,$3::text::numeric,$4::text::numeric,$5,'fixture:qualified-policy',$6::text::timestamptz,'fixture:timing-source',$7)",&[&decision,&basis,&clock,&availability,&profile,&time,&Uuid::new_v4()]).await
            });
            wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(
                task.await?.unwrap_err().code(),
                Some(&SqlState::T_R_SERIALIZATION_FAILURE)
            );
            assert_eq!(snapshot(&c, f.decision).await?["revision"], "0");
        }
        c.query_one("SELECT app.check_between_deadline($1)", &[&f.decision])
            .await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn a_recovery_chain_cannot_rebase_without_advancing_availability() -> TestResult {
    let c = connection().await?;
    let f = deadlines::fixture(&c).await?;
    deadlines::arm(&c, &f, &deadlines::due(&c, "1 hour").await?, Uuid::new_v4()).await?;
    let before = snapshot(&c, f.decision).await?;
    let source = Uuid::new_v4();
    let mut after = before.clone();
    after["revision"] = serde_json::json!("2");
    after["sourceReceiptId"] = serde_json::json!(source);
    after["dueAt"] = Value::Null;
    let remaining:String=c.query_one("SELECT (extract(epoch FROM (deadline_at-deadline_accounted_at))*1000000)::numeric(78,0)::text FROM app.between_game_decision WHERE id=$1",&[&f.decision]).await?.get(0);
    after["remainingUs"] = serde_json::json!(remaining);
    let terms = serde_json::json!({"version":"match-incident-v1","matchId":f.matched,"basis":f.basis,"availabilityBasis":"0","incidentAt":before["accountedAt"],"kind":"storage","proofRef":"fixture:forged-recovery"});
    let mut envelope = terms.clone();
    envelope["beforeDeadlineSnapshots"] = serde_json::json!([before]);
    envelope["deadlineSnapshots"] = serde_json::json!([after]);
    envelope["matchAvailabilityRevision"] = serde_json::json!("0");
    c.batch_execute("BEGIN").await?;
    c.execute("INSERT INTO app.command_receipt(id,principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source,response_payload) VALUES($1,'match-recovery:'||$2::uuid::text,'match_incident_v1',$2::uuid::text,$3::uuid::text,app.auth_json_hash($4),'accepted','',$5,jsonb_build_object('status','success','data',jsonb_build_object('resourceId',$2::uuid)))",&[&source,&f.matched,&Uuid::new_v4(),&terms,&envelope]).await?;
    let error = c
        .query_one("SELECT app.check_between_deadline_snapshot($1)", &[&after])
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        error.as_db_error().unwrap().message(),
        "deadline recovery must advance its exact availability source"
    );
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}
