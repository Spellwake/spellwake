//! Match-roster database fixtures, without native host or public chat activation.
use super::{TestResult, account, connection, game_scope, wait_for_database_lock};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn ensure(c: &Client, m: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    c.query_one("SELECT app.ensure_game_conversation($1)", &[&m])
        .await
        .map(|r| r.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn game_conversation_is_once_per_match_and_contains_exactly_its_human_roster() -> TestResult {
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    let (peer, _) = account(&c).await?;
    c.execute("INSERT INTO app.match_seat(match_id,account_id,seat_index,controller_kind) VALUES($1,$2,1,'human')", &[&f.match_id,&peer]).await?;
    c.execute("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,2,'ai')", &[&f.match_id,&f.account]).await?;
    let id = ensure(&c, f.match_id).await?;
    assert_eq!(ensure(&c, f.match_id).await?, id);
    let members=c.query("SELECT account_id FROM app.conversation_member WHERE conversation_id=$1 ORDER BY account_id", &[&id]).await?;
    let mut expected = [f.account, peer];
    expected.sort();
    assert_eq!(
        members
            .iter()
            .map(|r| r.get::<_, Uuid>(0))
            .collect::<Vec<_>>(),
        expected
    );
    let (outsider, _) = account(&c).await?;
    assert_eq!(
        c.execute(
            "INSERT INTO app.conversation_member(conversation_id,account_id) VALUES($1,$2)",
            &[&id, &outsider]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "DELETE FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&f.match_id, &peer]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.match_seat SET account_id=$1 WHERE match_id=$2 AND account_id=$3",
            &[&outsider, &f.match_id, &peer]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(c.execute("INSERT INTO app.conversation(kind,match_id,created_at) VALUES('game',$1,clock_timestamp())", &[&f.match_id]).await.unwrap_err().code(),Some(&SqlState::UNIQUE_VIOLATION));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn concurrent_game_chat_creation_reuses_the_committed_conversation_and_members() -> TestResult
{
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    let id = ensure(&c, f.match_id).await?;
    let task = tokio::spawn(async move { ensure(&waiting, f.match_id).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??, id);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn game_membership_waits_for_a_roster_change_and_rejects_the_departed_account() -> TestResult
{
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    let (peer, _) = account(&c).await?;
    c.execute("INSERT INTO app.match_seat(match_id,account_id,seat_index,controller_kind) VALUES($1,$2,1,'human')", &[&f.match_id,&peer]).await?;
    // Deliberately stage an empty game conversation to exercise the raw invariant.
    let id:Uuid=c.query_one("INSERT INTO app.conversation(kind,match_id,created_at) VALUES('game',$1,clock_timestamp()) RETURNING id", &[&f.match_id]).await?.get(0);
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.execute(
        "DELETE FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
        &[&f.match_id, &peer],
    )
    .await?;
    let task = tokio::spawn(async move {
        waiting
            .execute(
                "INSERT INTO app.conversation_member(conversation_id,account_id) VALUES($1,$2)",
                &[&id, &peer],
            )
            .await
    });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
