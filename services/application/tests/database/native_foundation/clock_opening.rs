//! SQL metadata/source checks only; these fixtures do not execute native bytes.
use super::*;

async fn snapshot(c: &Client, f: &Native) -> FixtureResult<Value> {
    // The canonical helper stays private. This is an owner-only test oracle;
    // restore the runtime role before the next acceptance call.
    let role: String = c.query_one("SELECT current_user::text", &[]).await?.get(0);
    if role == "service_role" {
        c.batch_execute("RESET ROLE").await?;
    }
    let result = c
        .query_one("SELECT app.match_clock_snapshot($1)", &[&f.matched])
        .await;
    if role == "service_role" {
        c.batch_execute("SET ROLE service_role").await?;
    }
    Ok(result?.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn opening_waiter_runs_only_its_selected_bank_and_exact_retry_does_not_restart_it()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo3").await?;
    let before = snapshot(&c, &f).await?;
    assert_eq!(before.as_array().unwrap().len(), 2);
    for row in before.as_array().unwrap() {
        assert_eq!(row["paused"], json!(row["seatId"] != json!(f.seat)));
        assert_eq!(row["remainingMs"], "1800000");
    }
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    let original = accept_opening_fixture(&c, &f, &f.opening).await?;
    assert_eq!(original["status"], "success");
    assert_eq!(snapshot(&c, &f).await?, before);
    let mut changed = f.opening.clone();
    changed["clockObservation"]["proofRef"] = json!(artifact(&f, "state", 99));
    assert!(accept_opening_fixture(&c, &f, &changed).await.is_err());
    assert_eq!(snapshot(&c, &f).await?, before);
    c.batch_execute("SET ROLE service_role").await?;
    host_advance(&c, &f).await?;
    let after = snapshot(&c, &f).await?;
    assert_ne!(after, before);
    assert_eq!(accept_opening_fixture(&c, &f, &f.opening).await?, original);
    assert_eq!(snapshot(&c, &f).await?, after);
    let stored: Value = c
        .query_one("SELECT app.read_native_opening_source($1)", &[&f.game])
        .await?
        .get(0);
    assert_eq!(stored["data"]["opening"], f.opening);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn malformed_or_missing_native_bo3_observation_leaves_reservation_and_original_unused()
-> TestResult {
    let c = connection().await?;
    let f = reserved_native(&c, "bo3", true).await?;
    let before = snapshot(&c, &f).await?;
    let mut bad = Vec::new();
    let mut missing = f.opening.clone();
    missing.as_object_mut().unwrap().remove("clockObservation");
    bad.push(missing);
    for value in [
        Value::Null,
        json!(false),
        json!({}),
        json!({"reason":"native_priority","seatId":f.seat,"acceptedRevision":"0","proofRef":f.opening["stateRef"]}),
        json!({"reason":"native_decision","seatId":Uuid::new_v4(),"acceptedRevision":"0","proofRef":f.opening["stateRef"]}),
    ] {
        let mut case = f.opening.clone();
        case["clockObservation"] = value;
        bad.push(case);
    }
    for (key, value) in [
        ("acceptedRevision", json!(0)),
        ("acceptedRevision", json!("1")),
        ("proofRef", json!("wrong")),
        ("extra", json!(true)),
    ] {
        let mut case = f.opening.clone();
        case["clockObservation"][key] = value;
        bad.push(case);
    }
    for case in bad {
        assert!(accept_opening_fixture(&c, &f, &case).await.is_err());
        assert_eq!(snapshot(&c, &f).await?, before);
        assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE target_ref=$1 AND scope IN('native_opening_v1','game_opening_v1')",&[&f.game.to_string()]).await?.get::<_,i64>(0),0);
        assert_eq!(
            c.query_one("SELECT state FROM app.game WHERE id=$1", &[&f.game])
                .await?
                .get::<_, String>(0),
            "reserved"
        );
    }
    assert_eq!(
        accept_opening_fixture(&c, &f, &f.opening).await?["status"],
        "success"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn bo1_observation_never_activates_an_overall_bank() -> TestResult {
    let c = connection().await?;
    let mut f = reserved_native(&c, "bo1", true).await?;
    f.opening["clockObservation"] = json!({"reason":"native_decision","seatId":f.seat,"acceptedRevision":"0","proofRef":f.opening["stateRef"]});
    accept_opening_fixture(&c, &f, &f.opening).await?;
    for row in snapshot(&c, &f).await?.as_array().unwrap() {
        assert_eq!(row["paused"], true);
        assert_eq!(row["remainingMs"], "0");
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn incident_before_first_move_restores_exact_opening_eligibility_without_native_advance()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo3").await?;
    let before = snapshot(&c, &f).await?;
    let incident:Value=c.query_one("SELECT app.pause_game_incident($1,$2,1,0,clock_timestamp(),'storage','fixture:initial-incident',$3)",&[&f.game,&OWNER,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(incident["status"], "success");
    for row in snapshot(&c, &f).await?.as_array().unwrap() {
        assert_eq!(row["paused"], true);
    }
    let checkpoint = Uuid::parse_str(incident["data"]["checkpointId"].as_str().unwrap())?;
    let request = Uuid::new_v4();
    let restored: Value = c
        .query_one(
            "SELECT app.restore_game_checkpoint($1,2,1,$2,$3,'fixture:restored-initial',$4)",
            &[&f.game, &checkpoint, &OWNER, &request],
        )
        .await?
        .get(0);
    assert_eq!(restored["status"], "success");
    let after = snapshot(&c, &f).await?;
    for row in after.as_array().unwrap() {
        assert_eq!(row["paused"], json!(row["seatId"] != json!(f.seat)));
        let old = before
            .as_array()
            .unwrap()
            .iter()
            .find(|old| old["seatId"] == row["seatId"])
            .unwrap();
        assert!(
            row["remainingMs"].as_str().unwrap().parse::<u64>()?
                <= old["remainingMs"].as_str().unwrap().parse::<u64>()?
        );
    }
    let retry: Value = c
        .query_one(
            "SELECT app.restore_game_checkpoint($1,2,1,$2,$3,'fixture:restored-initial',$4)",
            &[&f.game, &checkpoint, &OWNER, &request],
        )
        .await?
        .get(0);
    assert_eq!(retry, restored);
    assert_eq!(snapshot(&c, &f).await?, after);
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        accept_opening_fixture(&c, &f, &f.opening).await?["status"],
        "success"
    );
    assert_eq!(snapshot(&c, &f).await?, after);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn decision_clock_requires_exact_state_proof_and_can_transfer_to_priority_then_pause()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo3").await?;
    let other: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND id<>$2",
            &[&f.matched, &f.seat],
        )
        .await?
        .get(0);
    c.batch_execute("SET ROLE service_role").await?;
    for (revision, actor, reason) in [
        ("0", other, "native_decision"),
        ("1", f.seat, "native_priority"),
    ] {
        let mut body = candidate(&c, &f).await?;
        let proof = body["stateRef"].clone();
        for clock in body["clocks"].as_array_mut().unwrap() {
            if clock["seatId"] == json!(actor) {
                clock["paused"] = json!(false);
                clock["eligibility"] =
                    json!({"reason":reason,"acceptedRevision":revision,"proofRef":proof});
            }
        }
        let original = json!({"version":"native-host-input-v1","basis":basis(&f,revision),"kind":"native-work","inputRef":artifact(&f,"input",if revision=="0"{71}else{72}),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
        let request = Uuid::new_v4();
        if reason == "native_decision" {
            let before = snapshot(&c, &f).await?;
            let mut bad = body.clone();
            for row in bad["clocks"].as_array_mut().unwrap() {
                if row["seatId"] == json!(actor) {
                    row["eligibility"]["proofRef"] = json!("fixture:wrong-state");
                }
            }
            assert!(
                c.query_one(
                    "SELECT app.commit_native_host_input($1,$2,$3,$4)",
                    &[
                        &f.game,
                        &request,
                        &original,
                        &json!({"owner":OWNER,"fenceEpoch":"1","candidate":bad})
                    ]
                )
                .await
                .is_err()
            );
            assert_eq!(snapshot(&c, &f).await?, before);
        }
        let prepared = json!({"owner":OWNER,"fenceEpoch":"1","candidate":body});
        let accepted: Value = c
            .query_one(
                "SELECT app.commit_native_host_input($1,$2,$3,$4)",
                &[&f.game, &request, &original, &prepared],
            )
            .await?
            .get(0);
        assert_eq!(accepted["status"], "success");
        for row in snapshot(&c, &f).await?.as_array().unwrap() {
            assert_eq!(row["paused"], json!(row["seatId"] != json!(actor)));
        }
        let before_retry = snapshot(&c, &f).await?;
        let retry: Value = c
            .query_one(
                "SELECT app.commit_native_host_input($1,$2,$3,$4)",
                &[&f.game, &request, &original, &prepared],
            )
            .await?
            .get(0);
        assert_eq!(accepted, retry);
        assert_eq!(snapshot(&c, &f).await?, before_retry);
    }
    assert_eq!(host_advance(&c, &f).await?["status"], "success");
    for row in snapshot(&c, &f).await?.as_array().unwrap() {
        assert_eq!(row["paused"], true);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn forward_migration_preserves_legacy_native_bo3_originals_and_exact_recovery() -> TestResult
{
    let c = connection().await?;
    // All replaced bodies and seeded records are transaction-local and rolled
    // back. Other test connections cannot observe an old committed schema.
    c.batch_execute("BEGIN").await?;
    let opening =
        include_str!("../../../supabase/migrations/20261105021700_native_opening_context.sql");
    let native =
        include_str!("../../../supabase/migrations/20261106022400_native_original_opening.sql");
    let eligibility =
        include_str!("../../../supabase/migrations/20261103018900_native_creator_receipts.sql");
    let candidate = include_str!(
        "../../../supabase/migrations/20261106022300_native_canonical_claim_apply.sql"
    );
    for (source, name) in [
        (opening, "accept_reserved_game"),
        (opening, "check_native_creator_frame"),
        (native, "accept_native_opening"),
        (eligibility, "check_selected_clock_eligibility"),
        (candidate, "apply_accept_host_candidate"),
    ] {
        c.batch_execute(&historical_function(source, name).replacen(
            "CREATE FUNCTION",
            "CREATE OR REPLACE FUNCTION",
            1,
        ))
        .await?;
    }
    let mut f = reserved_native(&c, "bo3", true).await?;
    f.opening
        .as_object_mut()
        .unwrap()
        .remove("clockObservation");
    f.opening
        .as_object_mut()
        .unwrap()
        .remove("eventSchemaRevision");
    let ack = accept_opening_fixture(&c, &f, &f.opening).await?;
    c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
    let evidence_sql = "SELECT jsonb_agg(jsonb_build_object('id',id,'source',normalized_source::text,'response',response_payload::text,'hash',payload_hash,'state',state) ORDER BY id) FROM app.command_receipt WHERE target_ref=$1 AND scope IN('native_opening_v1','game_opening_v1')";
    let before: Value = c
        .query_one(evidence_sql, &[&f.game.to_string()])
        .await?
        .get(0);
    let clocks = snapshot(&c, &f).await?;
    let upgrade = include_str!(
        "../../../supabase/migrations/20261106023700_native_waiting_clock_sources.sql"
    );
    c.batch_execute(
        upgrade
            .strip_prefix("BEGIN;\n")
            .unwrap()
            .strip_suffix("COMMIT;\n")
            .unwrap(),
    )
    .await?;
    let observed: Value = c
        .query_one("SELECT app.read_native_opening_source($1)", &[&f.game])
        .await?
        .get(0);
    assert_eq!(observed["data"]["opening"], f.opening);
    assert_eq!(accept_opening_fixture(&c, &f, &f.opening).await?, ack);
    let after: Value = c
        .query_one(evidence_sql, &[&f.game.to_string()])
        .await?
        .get(0);
    assert_eq!(after, before);
    assert_eq!(snapshot(&c, &f).await?, clocks);
    assert!(
        clocks
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["paused"] == true)
    );
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn exhausted_nonchooser_blocks_new_opening_without_reset_or_partial_acceptance() -> TestResult
{
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = reserved_native(&c, "bo3", true).await?;
    // Defensive corrupt/pending-expiry fixture only. This does not claim a
    // native timeout can reach this state. Never commit bypassed source data.
    c.batch_execute("SET LOCAL session_replication_role=replica")
        .await?;
    c.execute("UPDATE app.match_clock SET remaining_ms=0 WHERE match_seat_id IN(SELECT id FROM app.match_seat WHERE match_id=$1 AND id<>$2)",&[&f.matched,&f.seat]).await?;
    c.batch_execute("SET LOCAL session_replication_role=origin;SAVEPOINT attempt")
        .await?;
    let error = accept_opening_fixture(&c, &f, &f.opening)
        .await
        .unwrap_err();
    let database = error
        .downcast_ref::<tokio_postgres::Error>()
        .unwrap()
        .as_db_error()
        .unwrap();
    assert_eq!(database.code(), &SqlState::CHECK_VIOLATION);
    assert_eq!(
        database.message(),
        "exhausted match bank must settle before another opening"
    );
    c.batch_execute("ROLLBACK TO SAVEPOINT attempt").await?;
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE target_ref=$1 AND scope IN('native_opening_v1','game_opening_v1')",&[&f.game.to_string()]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one("SELECT state FROM app.game WHERE id=$1", &[&f.game])
            .await?
            .get::<_, String>(0),
        "reserved"
    );
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn later_native_bo3_opening_preserves_depleted_banks_and_historical_original() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo3").await?;
    // Establish a measurable genuine database debit before the terminal fixture.
    c.query_one("SELECT pg_sleep(0.005)", &[]).await?;
    finish_opening_game_as_draw(&c, &f).await?;
    let historical: Value = c
        .query_one("SELECT app.read_native_opening_source($1)", &[&f.game])
        .await?
        .get(0);
    for row in c.query("SELECT id,seat_id FROM app.between_game_decision WHERE match_id=$1 AND after_game_id=$2 ORDER BY seat_id",&[&f.matched,&f.game]).await? {
        let decision: Uuid=row.get(0);let seat: Uuid=row.get(1);
        let reply: Value=c.query_one("SELECT app.reply_between_games($1,(SELECT revision FROM app.match WHERE id=$2),(SELECT source_revision_id FROM app.match_seat WHERE id=$3),'fixture:sideboard','fixture:validator',$4)",&[&decision,&f.matched,&seat,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(reply["status"],"success");
    }
    let next = c
        .query_one(
            "SELECT reserved_game_id,revision::text FROM app.match WHERE id=$1",
            &[&f.matched],
        )
        .await?;
    let mut later = Native {
        matched: f.matched,
        game: next.get(0),
        context: Uuid::new_v4(),
        seat: f.seat,
        subject: f.subject,
        session: f.session,
        original_basis: next.get(1),
        opening: Value::Null,
    };
    later.opening = json!({"contextId":later.context,"snapshotRef":artifact(&later,"initial",1),"contextRevision":"fixture:native-boundary-v1","stateRef":artifact(&later,"state",2),"factsRef":artifact(&later,"facts",3),"eventRef":artifact(&later,"event",4),"qualificationRef":"fixture:SQL-metadata-only","eventSchemaRevision":"native-transition-v1","clockObservation":{"reason":"native_decision","seatId":later.seat,"acceptedRevision":"0","proofRef":artifact(&later,"state",2)}});
    let before = snapshot(&c, &later).await?;
    let charged = before
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["seatId"] == json!(later.seat))
        .unwrap();
    assert!(charged["remainingMs"].as_str().unwrap().parse::<u64>()? < 1_800_000);
    assert_eq!(
        accept_opening_fixture(&c, &later, &later.opening).await?["status"],
        "success"
    );
    for row in snapshot(&c, &later).await?.as_array().unwrap() {
        let old = before
            .as_array()
            .unwrap()
            .iter()
            .find(|old| old["seatId"] == row["seatId"])
            .unwrap();
        assert_eq!(row["remainingMs"], old["remainingMs"]);
        assert_eq!(row["paused"], json!(row["seatId"] != json!(later.seat)));
    }
    let original: Value = c
        .query_one("SELECT app.read_native_opening_source($1)", &[&f.game])
        .await?
        .get(0);
    assert_eq!(original, historical);
    assert_eq!(
        accept_opening_fixture(&c, &f, &f.opening).await?["status"],
        "success"
    );
    Ok(())
}
