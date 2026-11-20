//! Normalized archive qualification is not native replay decoding or hidden-information filtering.
use super::{
    TestResult, ante_transaction_integrity, connection, match_lifecycle_integrity,
    wait_for_database_lock,
};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
async fn publish(c: &Client, g: Uuid, checkpoint: Uuid) -> Result<Value, tokio_postgres::Error> {
    let value:Value=c.query_one("SELECT app.publish_game_replay($1,1,$2,1,'fixture:archive','fixture:event-time-visibility','fixture:codec-v1','fixture:verified-archive')", &[&g,&checkpoint]).await.map(|r|r.get(0))?;
    let original = spellwake_application::recovery_ports::PublishReplay {
        game_id: g,
        revision: 1.into(),
        initial_checkpoint_id: checkpoint,
        through_event_sequence: 1.into(),
        artifact_ref: "fixture:archive".parse().unwrap(),
        visibility_index_ref: "fixture:event-time-visibility".parse().unwrap(),
        schema_revision: "fixture:codec-v1".parse().unwrap(),
        qualification_ref: "fixture:verified-archive".parse().unwrap(),
    };
    assert!(
        original
            .decode_response(&serde_json::to_vec(&value).unwrap())
            .is_ok()
    );
    Ok(value)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn final_replay_requires_original_checkpoint_and_terminal_frontier_then_publishes_once_under_contention()
-> TestResult {
    let c = connection().await?;
    let (_, m) = ante_transaction_integrity::reserve(&c, true).await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    assert_eq!(
        match_lifecycle_integrity::open(&c, m, g).await?["status"],
        "success"
    );
    assert_eq!(c.execute("INSERT INTO app.replay_manifest(game_id,context_id,artifact_ref,visibility_index_ref,schema_revision) SELECT game_id,id,'fixture:unqualified','fixture:unqualified','fixture:unqualified' FROM app.game_context WHERE game_id=$1", &[&g]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    let checkpoint: Uuid = c
        .query_one(
            "SELECT id FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=0",
            &[&g],
        )
        .await?
        .get(0);
    assert_eq!(
        publish(&c, g, checkpoint).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index LIMIT 1",
            &[&m],
        )
        .await?
        .get(0);
    let result = match_lifecycle_integrity::complete(&c, m, g, vec![seat]).await?;
    match_lifecycle_integrity::consume(&c, m, result).await?;
    let wrong: Uuid = c
        .query_one(
            "SELECT id FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=1",
            &[&g],
        )
        .await?
        .get(0);
    assert_eq!(
        publish(&c, g, wrong).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN").await?;
    let rolled = publish(&c, g, checkpoint).await?;
    assert_eq!(rolled["status"], "success");
    c.batch_execute("ROLLBACK").await?;
    assert!(
        c.query_opt("SELECT id FROM app.replay_manifest WHERE game_id=$1", &[&g])
            .await?
            .is_none()
    );
    let stale = connection().await?;
    stale
        .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    stale
        .query_one(
            "SELECT count(*)::bigint FROM app.replay_manifest WHERE game_id=$1",
            &[&g],
        )
        .await?;
    c.batch_execute("BEGIN").await?;
    let accepted = publish(&c, g, checkpoint).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let task = tokio::spawn(async move { publish(&waiting, g, checkpoint).await });
    wait_for_database_lock(&c, pid).await?;
    let append = connection().await?;
    let append_pid: i32 = append
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let late = tokio::spawn(async move {
        append.execute("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) SELECT game_id,context_id,2,1,'fixture:contending-append' FROM app.game_checkpoint WHERE id=$1", &[&checkpoint]).await
    });
    wait_for_database_lock(&c, append_pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        late.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(task.await??, accepted);
    assert_eq!(
        stale
            .execute(
                "UPDATE app.game SET accepted_revision=accepted_revision+1 WHERE id=$1",
                &[&g]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    stale.batch_execute("ROLLBACK").await?;

    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.replay_manifest WHERE game_id=$1",
            &[&g]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    assert_eq!(c.query_one("SELECT app.publish_game_replay($1,1,$2,1,'different-archive','fixture:event-time-visibility','fixture:codec-v1','fixture:verified-archive')", &[&g,&checkpoint]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(c.execute("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) SELECT game_id,context_id,2,1,'fixture:late-event' FROM app.game_checkpoint WHERE id=$1", &[&checkpoint]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        c.execute(
            "UPDATE app.game SET accepted_revision=accepted_revision+1 WHERE id=$1",
            &[&g]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
