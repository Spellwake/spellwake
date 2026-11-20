//! Real SQL opening identity/provenance. Opaque fixture artifacts do not qualify native rules.
use super::{
    TestResult, ante_transaction_integrity as ante, connection,
    match_lifecycle_integrity as lifecycle, room_start_integrity, wait_for_database_lock,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn opening(context: Uuid) -> Value {
    json!({"contextId":context,"snapshotRef":"fixture:context","contextRevision":"fixture:v1","stateRef":"fixture:state","factsRef":"fixture:facts","eventRef":"fixture:opening","qualificationRef":"fixture:normalized-only"})
}
async fn reserved(c: &Client) -> FixtureResult<(Uuid, Uuid, String)> {
    let (_, matched) = ante::reserve(c, true).await?;
    room_start_integrity::confirm_adopted_ai(c, matched).await?;
    let r = c
        .query_one(
            "SELECT reserved_game_id,revision::text FROM app.match WHERE id=$1",
            &[&matched],
        )
        .await?;
    Ok((matched, r.get(0), r.get(1)))
}
async fn accept(
    c: &Client,
    matched: Uuid,
    game: Uuid,
    basis: &str,
    body: &Value,
) -> Result<Value, tokio_postgres::Error> {
    Ok(c.query_one(
        "SELECT app.accept_reserved_game($1,$2::text::numeric,$3,'fixture:lifecycle',$4)",
        &[&matched, &basis, &game, &body],
    )
    .await?
    .get(0))
}
async fn empty(c: &Client, game: Uuid) -> TestResult {
    assert_eq!(c.query_one("SELECT (SELECT count(*) FROM app.game_context WHERE game_id=$1)+(SELECT count(*) FROM app.game_checkpoint WHERE game_id=$1)+(SELECT count(*) FROM app.game_event WHERE game_id=$1)+(SELECT count(*) FROM app.operational_state WHERE game_id=$1)+(SELECT count(*) FROM app.command_receipt WHERE scope='game_opening_v1' AND target_ref=$1::text)",&[&game]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one("SELECT state FROM app.game WHERE id=$1", &[&game])
            .await?
            .get::<_, String>(0),
        "reserved"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn chosen_context_and_original_replay_survive_actual_native_progression() -> TestResult {
    let c = connection().await?;
    let (m, g, b) = reserved(&c).await?;
    let context = Uuid::new_v4();
    let body = opening(context);
    let first = accept(&c, m, g, &b, &body).await?;
    assert_eq!(first["data"]["contextId"], json!(context));
    let source:Uuid=c.query_one("SELECT source_receipt_id FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=0",&[&g]).await?.get(0);
    c.execute("SELECT app.check_native_creator_frame($1)", &[&source])
        .await?;
    let winner: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index LIMIT 1",
            &[&m],
        )
        .await?
        .get(0);
    lifecycle::complete(&c, m, g, vec![winner]).await?;
    assert_eq!(accept(&c, m, g, "999999", &body).await?, first);
    let mut changed = body.clone();
    changed["contextId"] = json!(Uuid::new_v4());
    assert_eq!(
        accept(&c, m, g, &b, &changed).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let mut changed = body.clone();
    changed["stateRef"] = json!("fixture:changed");
    assert_eq!(
        accept(&c, m, g, &b, &changed).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.game_context WHERE game_id=$1 AND id=$2",
            &[&g, &context]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn missing_null_alias_nil_and_extra_context_inputs_reject_without_partial_opening()
-> TestResult {
    let c = connection().await?;
    let (m, g, b) = reserved(&c).await?;
    let original = opening(Uuid::new_v4());
    let mut cases = vec![];
    let mut missing = original.clone();
    missing.as_object_mut().unwrap().remove("contextId");
    cases.push(missing);
    for value in [
        Value::Null,
        json!([]),
        json!(Uuid::nil()),
        json!("AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"),
        json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        json!("{aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa}"),
        json!(" aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
        json!("invalid"),
    ] {
        let mut body = original.clone();
        body["contextId"] = value;
        cases.push(body);
    }
    let mut extra = original.clone();
    extra["extra"] = json!(true);
    cases.push(extra);
    for body in cases {
        assert_eq!(
            accept(&c, m, g, &b, &body).await.unwrap_err().code(),
            Some(&SqlState::INVALID_PARAMETER_VALUE)
        );
        empty(&c, g).await?;
    }
    assert_eq!(accept(&c, m, g, &b, &original).await?["status"], "success");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn another_games_context_collision_rolls_back_receipt_and_reservation() -> TestResult {
    let c = connection().await?;
    let (m, g, b) = reserved(&c).await?;
    let context = Uuid::new_v4();
    accept(&c, m, g, &b, &opening(context)).await?;
    let (m2, g2, b2) = reserved(&c).await?;
    assert_eq!(
        accept(&c, m2, g2, &b2, &opening(context))
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::UNIQUE_VIOLATION)
    );
    empty(&c, g2).await?;
    assert_eq!(
        c.query_one(
            "SELECT game_id FROM app.game_context WHERE id=$1",
            &[&context]
        )
        .await?
        .get::<_, Uuid>(0),
        g
    );
    assert_eq!(
        accept(&c, m2, g2, &b2, &opening(Uuid::new_v4())).await?["status"],
        "success"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn concurrent_original_context_retries_serialize_and_changed_context_conflicts() -> TestResult
{
    for changed in [false, true] {
        let c = connection().await?;
        let (m, g, b) = reserved(&c).await?;
        let body = opening(Uuid::new_v4());
        let waiting = connection().await?;
        let pid: i32 = waiting
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        c.batch_execute("BEGIN").await?;
        let first = accept(&c, m, g, &b, &body).await?;
        let next = if changed {
            opening(Uuid::new_v4())
        } else {
            body.clone()
        };
        let original_basis = b.clone();
        let task =
            tokio::spawn(async move { accept(&waiting, m, g, &original_basis, &next).await });
        wait_for_database_lock(&c, pid).await?;
        c.batch_execute("COMMIT").await?;
        let result = task.await?;
        if changed {
            assert_eq!(result.unwrap_err().code(), Some(&SqlState::CHECK_VIOLATION));
        } else {
            assert_eq!(result?, first);
        }
        assert_eq!(
            c.query_one(
                "SELECT count(*) FROM app.game_context WHERE game_id=$1",
                &[&g]
            )
            .await?
            .get::<_, i64>(0),
            1
        );
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn lifecycle_fixture_reuses_original_context_after_progression() -> TestResult {
    let c = connection().await?;
    let (m, g, _) = reserved(&c).await?;
    let first = lifecycle::open(&c, m, g).await?;
    let winner: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index LIMIT 1",
            &[&m],
        )
        .await?
        .get(0);
    lifecycle::complete(&c, m, g, vec![winner]).await?;
    assert_eq!(lifecycle::open(&c, m, g).await?, first);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rewritten_original_context_and_hash_cannot_launder_existing_native_frame() -> TestResult {
    let c = connection().await?;
    let (m, g, b) = reserved(&c).await?;
    let context = Uuid::new_v4();
    accept(&c, m, g, &b, &opening(context)).await?;
    let changed = opening(Uuid::new_v4());
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    let rejected=c.execute("UPDATE app.command_receipt SET normalized_source=jsonb_set(normalized_source,'{contextId}',($2::jsonb)->'contextId'),payload_hash=app.auth_json_hash(jsonb_build_object('match',$3::uuid,'game',$1::uuid,'owner','fixture:lifecycle','opening',$2::jsonb)) WHERE scope='game_opening_v1' AND target_ref=($1::uuid)::text",&[&g,&changed,&m]).await.unwrap_err();
    // Terminal source immutability rejects this forgery before deferred proof.
    assert_eq!(rejected.code(), Some(&SqlState::CHECK_VIOLATION));
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(
        c.query_one("SELECT id FROM app.game_context WHERE game_id=$1", &[&g])
            .await?
            .get::<_, Uuid>(0),
        context
    );
    let source:Uuid=c.query_one("SELECT source_receipt_id FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=0",&[&g]).await?.get(0);
    c.execute("SELECT app.check_native_creator_frame($1)", &[&source])
        .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn late_finalization_failure_rolls_back_context_artifacts_receipt_and_clocks() -> TestResult {
    let c = connection().await?;
    let (m, g, b) = reserved(&c).await?;
    let body = opening(Uuid::new_v4());
    let before: Value = c
        .query_one("SELECT app.match_clock_snapshot($1)", &[&m])
        .await?
        .get(0);
    let trigger = format!("fixture_opening_{}", Uuid::new_v4().simple());
    c.batch_execute("CREATE FUNCTION pg_temp.fail_opening_finalization() RETURNS trigger LANGUAGE plpgsql AS $$BEGIN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='fixture late opening failure';END$$").await?;
    c.batch_execute(&format!("CREATE CONSTRAINT TRIGGER {trigger} AFTER UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (NEW.scope='game_opening_v1' AND NEW.state='accepted' AND NEW.target_ref='{g}') EXECUTE FUNCTION pg_temp.fail_opening_finalization()")).await?;
    c.batch_execute("BEGIN").await?;
    assert_eq!(accept(&c, m, g, &b, &body).await?["status"], "success");
    let result = c.batch_execute("COMMIT").await;
    c.batch_execute(&format!("DROP TRIGGER {trigger} ON app.command_receipt;DROP FUNCTION pg_temp.fail_opening_finalization()")).await?;
    assert_eq!(result.unwrap_err().code(), Some(&SqlState::CHECK_VIOLATION));
    empty(&c, g).await?;
    assert!(c.query_one("SELECT state='ready' AND reserved_game_id=$2 AND current_game_id IS NULL AND revision=$3::text::numeric FROM app.match WHERE id=$1",&[&m,&g,&b]).await?.get::<_,bool>(0));
    assert_eq!(
        c.query_one("SELECT app.match_clock_snapshot($1)", &[&m])
            .await?
            .get::<_, Value>(0),
        before
    );
    assert_eq!(accept(&c, m, g, &b, &body).await?["status"], "success");
    Ok(())
}
