//! Immutable participation parent references, not proof of native own-turn observations.
use super::{TestResult, account, connection, game_scope, wait_for_database_lock};
use tokio_postgres::{Client, error::SqlState};

async fn participation(c: &Client, f: &game_scope::Seed) -> Result<u64, tokio_postgres::Error> {
    c.execute("INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision) VALUES($1,$2,$3,3,true,'fixture',clock_timestamp(),0)", &[&f.game,&f.seat,&f.account]).await
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn accepted_participation_keeps_its_terminal_game_revision_and_human_seat_parent()
-> TestResult {
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    c.execute(
        "UPDATE app.game SET state='completed' WHERE id=$1",
        &[&f.game],
    )
    .await?;
    participation(&c, &f).await?;
    for sql in [
        "UPDATE app.game SET accepted_revision=1 WHERE id=$1",
        "UPDATE app.game SET state='active' WHERE id=$1",
    ] {
        assert_eq!(
            c.execute(sql, &[&f.game]).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    let (other, _) = account(&c).await?;
    assert_eq!(
        c.execute(
            "UPDATE app.match_seat SET account_id=$1 WHERE id=$2",
            &[&other, &f.seat]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.game SET state='active' WHERE id=$1", &[&f.game])
        .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one("SELECT state FROM app.game WHERE id=$1", &[&f.game])
            .await?
            .get::<_, String>(0),
        "completed"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn participation_waits_for_seat_reassignment_then_rejects_the_departed_identity() -> TestResult
{
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    c.execute(
        "UPDATE app.game SET state='completed' WHERE id=$1",
        &[&f.game],
    )
    .await?;
    let (other, _) = account(&c).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.match_seat SET account_id=$1 WHERE id=$2",
        &[&other, &f.seat],
    )
    .await?;
    let task = tokio::spawn(async move { participation(&waiting, &f).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
