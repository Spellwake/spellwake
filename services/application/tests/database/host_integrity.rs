//! Normalized private storage-port fixtures. Artifact strings are not a native codec,
//! durable object-store receipt, authorized player command or playable-match qualification.
use super::{TestResult, account, connection, game_scope, wait_for_database_lock};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn staged(c: &Client) -> FixtureResult<game_scope::Seed> {
    let f = game_scope::seed(c).await?;
    c.execute("INSERT INTO app.match_clock(match_seat_id,remaining_ms,clock_revision,policy_ref,paused,last_accounted_at) VALUES($1,60000,0,'fixture',false,clock_timestamp())", &[&f.seat]).await?;
    Ok(f)
}
pub(super) async fn fixture(c: &Client) -> FixtureResult<game_scope::Seed> {
    let f = staged(c).await?;
    c.execute("INSERT INTO app.operational_state(game_id,owner_instance_ref,fence_epoch,availability,revision) VALUES($1,'fixture-host',1,'available',0)", &[&f.game]).await?;
    Ok(f)
}
pub(super) fn candidate(f: &game_scope::Seed) -> Value {
    json!({"contextId":f.context,"stateRef":"fixture:state","factsRef":"fixture:facts","events":["fixture:event"],"facts":[{"eventIndex":"0","accountId":f.account,"kind":"fixture","revision":"fixture","value":"1"}],"clocks":[{"seatId":f.seat,"clockRevision":"0","remainingMs":"59000","paused":false}],"participation":[],"terminal":null})
}
pub(super) async fn accept(
    c: &Client,
    game: Uuid,
    epoch: &str,
    basis: &str,
    request: Uuid,
    payload: &Value,
) -> Result<Value, tokio_postgres::Error> {
    let response:Value=c.query_one("SELECT app.accept_host_candidate($1,'fixture-host',$2::text::numeric,$3::text::numeric,$4,$5)", &[&game,&epoch,&basis,&request,&payload]).await.map(|r|r.get(0))?;
    super::admission_port_contracts::candidate_response(
        game,
        "fixture-host",
        epoch,
        basis,
        request,
        payload,
        &response,
    );
    Ok(response)
}
async fn counts(c: &Client, game: Uuid) -> Result<(String, i64, i64, i64), tokio_postgres::Error> {
    let row=c.query_one("SELECT accepted_revision::text,(SELECT count(*) FROM app.game_checkpoint WHERE game_id=g.id)::bigint,(SELECT count(*) FROM app.game_event WHERE game_id=g.id)::bigint,(SELECT count(*) FROM app.game_activity_fact WHERE game_id=g.id)::bigint FROM app.game g WHERE id=$1", &[&game]).await?;
    Ok((row.get(0), row.get(1), row.get(2), row.get(3)))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn host_candidate_accepts_state_event_fact_clock_and_receipt_once_under_one_fence()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let body = candidate(&f);
    let request = Uuid::new_v4();
    let accepted = accept(&c, f.game, "1", "0", request, &body).await?;
    assert_eq!(accepted["status"], "success");
    assert_eq!(accepted["data"]["revision"], "1");
    assert_eq!(
        accept(&c, f.game, "1", "0", request, &body).await?,
        accepted
    );
    assert_eq!(counts(&c, f.game).await?, ("1".into(), 1, 2, 1));
    let clock=c.query_one("SELECT remaining_ms::text,clock_revision::text FROM app.match_clock WHERE match_seat_id=$1", &[&f.seat]).await?;
    assert_eq!(clock.get::<_, String>(0), "59000");
    assert_eq!(clock.get::<_, String>(1), "1");
    let mut changed = body.clone();
    changed["stateRef"] = json!("fixture:changed");
    assert_eq!(
        accept(&c, f.game, "1", "0", request, &changed)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        accept(&c, f.game, "1", "0", Uuid::new_v4(), &body).await?["data"]["basis"],
        "stale or terminal"
    );
    c.execute("UPDATE app.operational_state SET owner_instance_ref='new-owner',fence_epoch=2,revision=revision+1 WHERE game_id=$1", &[&f.game]).await?;
    // Accepted disposition survives ownership transfer; it does not reapply effects.
    assert_eq!(
        accept(&c, f.game, "1", "0", request, &body).await?,
        accepted
    );
    assert_eq!(
        accept(&c, f.game, "1", "1", Uuid::new_v4(), &body).await?["data"]["fence"],
        "stale or unavailable"
    );
    assert_eq!(counts(&c, f.game).await?, ("1".into(), 1, 2, 1));
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn terminal_candidate_records_all_human_participation_and_durable_outcome_atomically()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let mut body = candidate(&f);
    body["terminal"] = json!({"outcome":"draw","winnerSeatIds":[],"winnersRef":"fixture:no-winners","reason":"fixture"});
    body["participation"] = json!([{"seatId":f.seat,"ownTurns":"3","reason":"fixture"}]);
    let request = Uuid::new_v4();
    let accepted = accept(&c, f.game, "1", "0", request, &body).await?;
    assert_eq!(accepted["status"], "success");
    assert_eq!(
        accept(&c, f.game, "1", "0", request, &body).await?,
        accepted
    );
    let row=c.query_one("SELECT g.state,p.real_game,p.accepted_revision::text,p.completed_at=c.last_accounted_at,c.paused,(SELECT count(*) FROM app.game_result WHERE game_id=g.id)::bigint,(SELECT count(*) FROM app.durable_handoff WHERE game_id=g.id AND kind='game_outcome')::bigint FROM app.game g JOIN app.game_participation p ON p.game_id=g.id JOIN app.match_clock c ON c.match_seat_id=p.match_seat_id WHERE g.id=$1", &[&f.game]).await?;
    assert_eq!(row.get::<_, String>(0), "completed");
    assert!(row.get::<_, bool>(1));
    assert_eq!(row.get::<_, String>(2), "1");
    assert!(row.get::<_, bool>(3));
    assert!(row.get::<_, bool>(4));
    assert_eq!(row.get::<_, i64>(5), 1);
    assert_eq!(row.get::<_, i64>(6), 1);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn a_late_invalid_fact_or_participation_rolls_back_every_candidate_effect_and_receipt()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let (outsider, _) = account(&c).await?;
    let mut body = candidate(&f);
    body["facts"][0]["accountId"] = json!(outsider);
    let request = Uuid::new_v4();
    assert_eq!(
        accept(&c, f.game, "1", "0", request, &body)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(counts(&c, f.game).await?, ("0".into(), 0, 1, 0));
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE scope='host_candidate_v1' AND request_ref=$1", &[&request.to_string()]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one(
            "SELECT remaining_ms::text FROM app.match_clock WHERE match_seat_id=$1",
            &[&f.seat]
        )
        .await?
        .get::<_, String>(0),
        "60000"
    );
    body = candidate(&f);
    body["terminal"] = json!({"outcome":"win","winnerSeatIds":[f.seat],"winnersRef":"fixture:winners","reason":"fixture"});
    body["participation"] = json!([{"seatId":Uuid::new_v4(),"ownTurns":"3","reason":"fixture"}]);
    assert_eq!(
        accept(&c, f.game, "1", "0", Uuid::new_v4(), &body)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(counts(&c, f.game).await?, ("0".into(), 0, 1, 0));
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competing_candidates_serialize_on_the_game_and_only_one_basis_can_advance() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let body = candidate(&f);
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    assert_eq!(
        accept(&c, f.game, "1", "0", Uuid::new_v4(), &body).await?["status"],
        "success"
    );
    let task =
        tokio::spawn(
            async move { accept(&waiting, f.game, "1", "0", Uuid::new_v4(), &body).await },
        );
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??["data"]["basis"], "stale or terminal");
    assert_eq!(counts(&c, f.game).await?, ("1".into(), 1, 2, 1));
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn host_acceptance_rejects_bad_context_clock_topups_and_malformed_payload_without_effects()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let mut body = candidate(&f);
    body["contextId"] = json!(Uuid::new_v4());
    assert_eq!(
        accept(&c, f.game, "1", "0", Uuid::new_v4(), &body).await?["data"]["context"],
        "unavailable"
    );
    body = candidate(&f);
    body["clocks"][0]["remainingMs"] = json!("60001");
    assert_eq!(
        accept(&c, f.game, "1", "0", Uuid::new_v4(), &body).await?["data"]["clocks"],
        "stale or invalid"
    );
    body = candidate(&f);
    body["events"] = json!([null]);
    assert_eq!(
        accept(&c, f.game, "1", "0", Uuid::new_v4(), &body)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    body = candidate(&f);
    body["clocks"][0]["remainingMs"] = json!(59_000);
    assert_eq!(
        accept(&c, f.game, "1", "0", Uuid::new_v4(), &body)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    assert_eq!(counts(&c, f.game).await?, ("0".into(), 0, 1, 0));
    assert_eq!(c.execute("UPDATE app.operational_state SET owner_instance_ref='changed',revision=revision+1 WHERE game_id=$1", &[&f.game]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        c.execute(
            "UPDATE app.match_seat SET team_ref='changed' WHERE id=$1",
            &[&f.seat]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.match SET structure='bo3' WHERE id=$1",
            &[&f.match_id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.game_event SET private_payload_ref='changed' WHERE id=$1",
            &[&f.event]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

async fn establish(c: &Client, game: Uuid) -> Result<u64, tokio_postgres::Error> {
    c.execute("INSERT INTO app.operational_state(game_id,owner_instance_ref,fence_epoch,availability,revision) VALUES($1,'fixture-host',1,'available',0)", &[&game]).await
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn host_establishment_fences_roster_changes_in_both_actual_lock_orders() -> TestResult {
    let c = connection().await?;
    let first = staged(&c).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    establish(&c, first.game).await?;
    let task = tokio::spawn(async move {
        waiting
            .execute(
                "UPDATE app.match_seat SET team_ref='changed' WHERE id=$1",
                &[&first.seat],
            )
            .await
    });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let second = staged(&c).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.match_seat SET team_ref='final-before-host' WHERE id=$1",
        &[&second.seat],
    )
    .await?;
    let task = tokio::spawn(async move { establish(&waiting, second.game).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    task.await??;
    assert_eq!(
        c.query_one(
            "SELECT team_ref FROM app.match_seat WHERE id=$1",
            &[&second.seat]
        )
        .await?
        .get::<_, String>(0),
        "final-before-host"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn a_hosted_game_keeps_its_match_parent_and_reassignment_waits_for_host_establishment()
-> TestResult {
    let c = connection().await?;
    let f = staged(&c).await?;
    let other:Uuid=c.query_one("INSERT INTO app.match(format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) SELECT format_id,profile_id,legality_revision_id,mode,competitive,structure,state,0 FROM app.match WHERE id=$1 RETURNING id", &[&f.match_id]).await?.get(0);
    // Pre-host draft reassignment is still permitted.
    c.execute(
        "UPDATE app.game SET match_id=$1,game_index=7 WHERE id=$2",
        &[&other, &f.game],
    )
    .await?;
    c.execute(
        "UPDATE app.game SET match_id=$1,game_index=0 WHERE id=$2",
        &[&f.match_id, &f.game],
    )
    .await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    establish(&c, f.game).await?;
    let task = tokio::spawn(async move {
        waiting
            .execute(
                "UPDATE app.game SET match_id=$1,game_index=7 WHERE id=$2",
                &[&other, &f.game],
            )
            .await
    });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one("SELECT match_id FROM app.game WHERE id=$1", &[&f.game])
            .await?
            .get::<_, Uuid>(0),
        f.match_id
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn candidate_waits_for_ownership_rotation_and_cannot_commit_under_the_old_epoch() -> TestResult
{
    let c = connection().await?;
    let f = fixture(&c).await?;
    let body = candidate(&f);
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.operational_state SET fence_epoch=2,revision=revision+1 WHERE game_id=$1",
        &[&f.game],
    )
    .await?;
    let task =
        tokio::spawn(
            async move { accept(&waiting, f.game, "1", "0", Uuid::new_v4(), &body).await },
        );
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??["data"]["fence"], "stale or unavailable");
    assert_eq!(counts(&c, f.game).await?, ("0".into(), 0, 1, 0));
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deferred_invalid_candidate_commit_rolls_back_its_response_receipt_and_all_state()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let (other, _) = account(&c).await?;
    let mut body = candidate(&f);
    body["facts"][0]["accountId"] = json!(other);
    let request = Uuid::new_v4();
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    // A caller must commit before publishing an accepted response or view.
    assert_eq!(
        accept(&c, f.game, "1", "0", request, &body).await?["status"],
        "success"
    );
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(counts(&c, f.game).await?, ("0".into(), 0, 1, 0));
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE scope='host_candidate_v1' AND request_ref=$1", &[&request.to_string()]).await?.get::<_,i64>(0),0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn no_contest_pauses_every_human_clock_and_persists_each_own_turn_total() -> TestResult {
    let c = connection().await?;
    let f = staged(&c).await?;
    let (peer, _) = account(&c).await?;
    let seat:Uuid=c.query_one("INSERT INTO app.match_seat(match_id,account_id,seat_index,controller_kind) VALUES($1,$2,1,'human') RETURNING id", &[&f.match_id,&peer]).await?.get(0);
    c.execute("INSERT INTO app.match_clock(match_seat_id,remaining_ms,clock_revision,policy_ref,paused,last_accounted_at) VALUES($1,50000,7,'fixture',false,clock_timestamp())", &[&seat]).await?;
    establish(&c, f.game).await?;
    let mut body = candidate(&f);
    body["clocks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"seatId":seat,"clockRevision":"7","remainingMs":"49000","paused":false}));
    body["terminal"] = json!({"outcome":"no_contest","winnerSeatIds":[],"winnersRef":"fixture:none","reason":"fixture"});
    body["participation"] = json!([{"seatId":f.seat,"ownTurns":"2","reason":"fixture"},{"seatId":seat,"ownTurns":"3","reason":"fixture"}]);
    assert_eq!(
        accept(&c, f.game, "1", "0", Uuid::new_v4(), &body).await?["status"],
        "success"
    );
    let rows=c.query("SELECT p.real_game,c.clock_revision::text,c.paused FROM app.game_participation p JOIN app.match_clock c ON c.match_seat_id=p.match_seat_id WHERE p.game_id=$1 ORDER BY p.own_turns", &[&f.game]).await?;
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].get::<_, bool>(0));
    assert!(rows[1].get::<_, bool>(0));
    for row in rows {
        assert_eq!(row.get::<_, String>(1), "8");
        assert!(row.get::<_, bool>(2));
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stale_repeatable_read_cannot_hide_a_new_host_when_changing_game_identity() -> TestResult {
    let c = connection().await?;
    let f = staged(&c).await?;
    let stale = connection().await?;
    stale
        .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    stale
        .query_one(
            "SELECT game_index::text FROM app.game WHERE id=$1",
            &[&f.game],
        )
        .await?;
    establish(&c, f.game).await?;
    assert_eq!(
        stale
            .execute("UPDATE app.game SET game_index=1 WHERE id=$1", &[&f.game])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    stale.batch_execute("ROLLBACK").await?;
    assert_eq!(
        c.query_one(
            "SELECT game_index::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    Ok(())
}
