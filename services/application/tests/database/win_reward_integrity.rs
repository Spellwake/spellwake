//! Reward consumption over normalized immutable competitive-credit fixtures, not native winner or ranked-season production.
use super::{TestResult, account, connection, game_scope, wait_for_database_lock};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn credit(c: &Client, actor: Uuid, when: &str, win: bool) -> FixtureResult<Uuid> {
    let f = game_scope::seed(c).await?;
    c.execute(
        "UPDATE app.match_seat SET account_id=$1 WHERE id=$2",
        &[&actor, &f.seat],
    )
    .await?;
    c.execute(
        "UPDATE app.match SET mode='collection',competitive=true,state='completed' WHERE id=$1",
        &[&f.match_id],
    )
    .await?;
    let result:Uuid=c.query_one("INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at,ranking_format_id,rank_sequence) SELECT id,$2,'win','fixture',$3::text::timestamptz,format_id,1 FROM app.match WHERE id=$1 RETURNING id", &[&f.match_id,&Uuid::new_v4().to_string(),&when]).await?.get(0);
    Ok(c.query_one("INSERT INTO app.competitive_result(account_id,match_result_id,source_ref,format_id,win,completed_at,policy_ref) SELECT $1,id,$3,ranking_format_id,$4,committed_at,'fixture' FROM app.match_result WHERE id=$2 RETURNING id", &[&actor,&result,&Uuid::new_v4().to_string(),&win]).await?.get(0))
}
async fn consume(c: &Client, id: Uuid) -> Result<Value, tokio_postgres::Error> {
    c.query_one("SELECT app.consume_competitive_win_v1($1)", &[&id])
        .await
        .map(|r| r.get(0))
}
async fn coins(c: &Client, actor: Uuid) -> Result<String, tokio_postgres::Error> {
    c.query_one("SELECT coalesce(sum(i.owned_quantity),0)::text FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE o.account_id=$1 AND d.currency_key='coin'", &[&actor]).await.map(|r|r.get(0))
}
async fn owner(c: &Client) -> FixtureResult<Uuid> {
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    let (a, _) = account(c).await?;
    Ok(a)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn daily_win_rewards_are_once_per_credit_shared_across_formats_and_capped_at_750()
-> TestResult {
    let c = connection().await?;
    let a = owner(&c).await?;
    let schedule = [250, 100, 100, 100, 0, 50, 0, 50, 0, 50, 0, 25, 0, 25, 0, 0];
    for (index, expected) in schedule.iter().enumerate() {
        let id = credit(&c, a, "2026-11-01T12:00:00Z", true).await?;
        let result = consume(&c, id).await?;
        assert_eq!(result["data"]["coins"], expected.to_string());
        assert_eq!(result["data"]["winIndex"], (index + 1).to_string());
        assert_eq!(consume(&c, id).await?, result);
    }
    assert_eq!(coins(&c, a).await?, "750");
    let row = c
        .query_one(
            "SELECT wins::text,issued_coins::text FROM app.daily_win_bucket WHERE account_id=$1",
            &[&a],
        )
        .await?;
    assert_eq!(row.get::<_, String>(0), "16");
    assert_eq!(row.get::<_, String>(1), "750");
    assert_eq!(c.execute("UPDATE app.daily_win_bucket SET wins=0,issued_coins=0,revision=revision+1 WHERE account_id=$1", &[&a]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn delayed_credits_keep_their_source_utc_days_and_losses_do_not_consume_a_win_slot()
-> TestResult {
    let c = connection().await?;
    let a = owner(&c).await?;
    c.batch_execute("SET TIME ZONE 'Pacific/Honolulu'").await?;
    let yesterday = credit(&c, a, "2026-10-31T23:59:59Z", true).await?;
    let next = credit(&c, a, "2026-11-01T00:00:00Z", true).await?;
    let lost = credit(&c, a, "2026-11-01T01:00:00Z", false).await?;
    assert_eq!(consume(&c, lost).await?["data"]["counted"], "false");
    assert_eq!(consume(&c, next).await?["data"]["day"], "2026-11-01");
    // An account pending deletion still receives proceeds of already accepted games.
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&a]).await?;
    assert_eq!(consume(&c, yesterday).await?["data"]["day"], "2026-10-31");
    assert_eq!(coins(&c, a).await?, "500");
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.daily_win_bucket WHERE account_id=$1 AND wins=1",
            &[&a]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competing_format_credits_wait_on_the_account_and_advance_one_shared_reward_index()
-> TestResult {
    let c = connection().await?;
    let a = owner(&c).await?;
    let first = credit(&c, a, "2026-11-01T00:00:00Z", true).await?;
    let second = credit(&c, a, "2026-11-01T01:00:00Z", true).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    assert_eq!(consume(&c, first).await?["data"]["coins"], "250");
    let task = tokio::spawn(async move { consume(&waiting, second).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??["data"]["coins"], "100");
    assert_eq!(coins(&c, a).await?, "350");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn aborted_reward_consumption_retries_without_partial_ledger_or_daily_history() -> TestResult
{
    let c = connection().await?;
    let a = owner(&c).await?;
    let id = credit(&c, a, "2026-11-01T00:00:00Z", true).await?;
    c.batch_execute("BEGIN").await?;
    assert_eq!(consume(&c, id).await?["data"]["coins"], "250");
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(coins(&c, a).await?, "0");
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.daily_win_bucket WHERE account_id=$1",
            &[&a]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(consume(&c, id).await?["data"]["coins"], "250");
    assert_eq!(coins(&c, a).await?, "250");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competitive_human_only_policy_is_checked_on_parent_changes_as_well_as_seat_insertions()
-> TestResult {
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    c.execute(
        "UPDATE app.match SET mode='collection',competitive=true WHERE id=$1",
        &[&f.match_id],
    )
    .await?;
    assert_eq!(c.execute("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,1,'ai')", &[&f.match_id,&f.account]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.execute(
        "UPDATE app.match SET competitive=false WHERE id=$1",
        &[&f.match_id],
    )
    .await?;
    c.execute("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,1,'ai')", &[&f.match_id,&f.account]).await?;
    assert_eq!(
        c.execute(
            "UPDATE app.match SET competitive=true WHERE id=$1",
            &[&f.match_id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn child_first_deferred_insertion_cannot_hide_an_ai_seat_from_a_new_competitive_parent()
-> TestResult {
    let c = connection().await?;
    let f = game_scope::seed(&c).await?;
    for deferred in ["app.match_seat_match_id_fkey", "ALL"] {
        let proposed = Uuid::new_v4();
        c.batch_execute(&format!("BEGIN; SET CONSTRAINTS {deferred} DEFERRED"))
            .await?;
        c.execute("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,1,'ai')", &[&proposed,&f.account]).await?;
        let inserted=c.execute("INSERT INTO app.match(id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) SELECT $1,format_id,profile_id,legality_revision_id,'collection',true,'bo1','active',0 FROM app.match WHERE id=$2", &[&proposed,&f.match_id]).await;
        let denied = if deferred == "ALL" {
            inserted?;
            c.batch_execute("COMMIT").await.unwrap_err()
        } else {
            inserted.unwrap_err()
        };
        assert_eq!(denied.code(), Some(&SqlState::CHECK_VIOLATION));
        c.batch_execute("ROLLBACK").await?;
        assert_eq!(
            c.query_one(
                "SELECT count(*)::bigint FROM app.match WHERE id=$1",
                &[&proposed]
            )
            .await?
            .get::<_, i64>(0),
            0
        );
    }
    Ok(())
}
