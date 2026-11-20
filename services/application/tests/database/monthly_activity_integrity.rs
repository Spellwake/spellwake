//! Consumption of persisted participation fixtures, not native event or supply-cutoff qualification.
use super::{TestResult, account, connection, game_scope, wait_for_database_lock};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn source(c: &Client, turns: i32, mode: &str) -> FixtureResult<(game_scope::Seed, Uuid)> {
    let f = game_scope::seed(c).await?;
    c.execute(
        "UPDATE app.match SET mode=$1 WHERE id=$2",
        &[&mode, &f.match_id],
    )
    .await?;
    c.execute(
        "UPDATE app.game SET state='completed' WHERE id=$1",
        &[&f.game],
    )
    .await?;
    let id=c.query_one("INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision) VALUES($1,$2,$3,$4::integer,$4>=3,'fixture','2024-03-01T00:15:00Z',0) RETURNING id", &[&f.game,&f.seat,&f.account,&turns]).await?.get(0);
    Ok((f, id))
}
async fn record(c: &Client, id: Uuid) -> Result<Option<Uuid>, tokio_postgres::Error> {
    c.query_one(
        "SELECT coalesce(to_jsonb(app.record_monthly_game_activity($1)),'null'::jsonb)",
        &[&id],
    )
    .await
    .map(|r| {
        let value: serde_json::Value = r.get(0);
        spellwake_application::terminal_consumers::MonthlyActivity {
            participation_id: id,
        }
        .decode_response(value.to_string().as_bytes())
        .unwrap()
        .0
    })
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn monthly_activity_counts_humans_once_in_the_utc_completion_month_in_both_modes()
-> TestResult {
    let c = connection().await?;
    c.batch_execute("SET TIME ZONE 'Pacific/Honolulu'").await?;
    for mode in ["open", "collection"] {
        let (f, p) = source(&c, 3, mode).await?;
        let row = record(&c, p).await?.unwrap();
        assert_eq!(record(&c, p).await?, Some(row));
        let activity=c.query_one("SELECT account_id,utc_month,source_ref FROM app.monthly_account_activity WHERE id=$1", &[&row]).await?;
        assert_eq!(activity.get::<_, Uuid>(0), f.account);
        assert_eq!(activity.get::<_, String>(1), "2024-03");
        assert_eq!(activity.get::<_, String>(2), p.to_string());
        assert_eq!(
            c.execute(
                "DELETE FROM app.monthly_account_activity WHERE id=$1",
                &[&row]
            )
            .await
            .unwrap_err()
            .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    let (_, short) = source(&c, 2, "collection").await?;
    assert_eq!(record(&c, short).await?, None);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn fabricated_monthly_activity_cannot_replace_a_real_account_month_source() -> TestResult {
    let c = connection().await?;
    let (f, p) = source(&c, 3, "open").await?;
    let (other, _) = account(&c).await?;
    for (owner, month, policy, source) in [
        (f.account, "2024-03", "unqualified:policy", p.to_string()),
        (
            f.account,
            "2024-03",
            "monthly-real-game-v1",
            "unqualified:source".into(),
        ),
        (other, "2024-03", "monthly-real-game-v1", p.to_string()),
        (f.account, "2024-02", "monthly-real-game-v1", p.to_string()),
    ] {
        assert_eq!(c.execute("INSERT INTO app.monthly_account_activity(account_id,utc_month,activity_policy_ref,source_ref) VALUES($1,$2,$3,$4)", &[&owner,&month,&policy,&source]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    }
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.monthly_account_activity WHERE account_id=$1",
            &[&f.account]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn concurrent_monthly_activity_replays_wait_for_one_immutable_source_record() -> TestResult {
    let c = connection().await?;
    let (_, p) = source(&c, 3, "collection").await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    let first = record(&c, p).await?;
    let task = tokio::spawn(async move { record(&waiting, p).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??, first);
    Ok(())
}
