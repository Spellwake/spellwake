//! Aggregate allowance integrity only: no ante allocation, loss-source or coin issuance qualification.
use super::{TestResult, account, connection, game_scope, wait_for_database_lock};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn bucket(c: &Client, owner: Uuid, day: &str) -> Result<Uuid, tokio_postgres::Error> {
    c.query_one("INSERT INTO app.ai_loss_bucket(sponsor_id,utc_day,limit_coins,lost_coins,revision) VALUES($1,$2,1000,0,0) RETURNING id", &[&owner,&day]).await.map(|r| r.get(0))
}

async fn commitment(c: &Client) -> Result<(Uuid, Uuid), Box<dyn std::error::Error + Send + Sync>> {
    let f = game_scope::seed(c).await?;
    c.execute(
        "UPDATE app.match SET mode='collection' WHERE id=$1",
        &[&f.match_id],
    )
    .await?;
    let id = c.query_one("INSERT INTO app.ante_commitment(match_id,terms_revision,state) VALUES($1,0,'offered') RETURNING id", &[&f.match_id]).await?.get(0);
    Ok((f.match_id, id))
}

async fn reserve(
    c: &Client,
    bucket: Uuid,
    term: (Uuid, Uuid),
    amount: &str,
) -> Result<Uuid, tokio_postgres::Error> {
    c.query_one("INSERT INTO app.ai_loss_exposure(bucket_id,match_id,commitment_id,reserved_coins,state) VALUES($1,$2,$3,$4::text::numeric,'reserved') RETURNING id", &[&bucket,&term.0,&term.1,&amount]).await.map(|r| r.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn lost_plus_all_outstanding_ai_reservations_never_exceed_the_shared_daily_allowance()
-> TestResult {
    let c = connection().await?;
    let (sponsor, _) = account(&c).await?;
    let pool = bucket(&c, sponsor, "2026-11-03").await?;
    let mut exposures = Vec::new();
    for _ in 0..3 {
        exposures.push(reserve(&c, pool, commitment(&c).await?, "300").await?);
    }
    let fourth = commitment(&c).await?;
    assert_eq!(
        reserve(&c, pool, fourth, "101").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.execute(
        "UPDATE app.ai_loss_bucket SET lost_coins=100,revision=revision+1 WHERE id=$1",
        &[&pool],
    )
    .await?;
    assert_eq!(
        c.execute(
            "UPDATE app.ai_loss_bucket SET lost_coins=101,revision=revision+1 WHERE id=$1",
            &[&pool]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.ai_loss_bucket SET lost_coins=99,revision=revision+1 WHERE id=$1",
            &[&pool]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.execute(
        "UPDATE app.ai_loss_exposure SET state='released' WHERE id=$1",
        &[&exposures[0]],
    )
    .await?;
    reserve(&c, pool, fourth, "300").await?;
    let total = c.query_one("SELECT b.lost_coins::text,(SELECT sum(reserved_coins)::text FROM app.ai_loss_exposure WHERE bucket_id=b.id AND state='reserved') FROM app.ai_loss_bucket b WHERE b.id=$1", &[&pool]).await?;
    assert_eq!(total.get::<_, String>(0), "100");
    assert_eq!(total.get::<_, String>(1), "900");
    assert_eq!(
        c.execute(
            "UPDATE app.ai_loss_exposure SET state='reserved' WHERE id=$1",
            &[&exposures[0]]
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
async fn sponsor_match_limit_cannot_be_bypassed_with_another_day_or_an_unrelated_commitment()
-> TestResult {
    let c = connection().await?;
    let (sponsor, _) = account(&c).await?;
    let first = bucket(&c, sponsor, "2026-11-03").await?;
    let next = bucket(&c, sponsor, "2026-11-04").await?;
    let term = commitment(&c).await?;
    reserve(&c, first, term, "200").await?;
    assert_eq!(
        reserve(&c, next, term, "101").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    reserve(&c, next, term, "100").await?;
    let foreign = commitment(&c).await?;
    assert_eq!(
        reserve(&c, first, (foreign.0, term.1), "1")
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.execute(
        "UPDATE app.ante_commitment SET state='committed' WHERE id=$1",
        &[&term.1],
    )
    .await?;
    assert_eq!(
        c.execute(
            "UPDATE app.ai_loss_exposure SET reserved_coins=201 WHERE bucket_id=$1",
            &[&first]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let (other, _) = account(&c).await?;
    let unrelated = bucket(&c, other, "2026-11-03").await?;
    assert_eq!(
        reserve(&c, unrelated, term, "1").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn concurrent_ai_reservations_wait_on_the_sponsor_fence_and_reject_the_second_overspend()
-> TestResult {
    let c = connection().await?;
    let (sponsor, _) = account(&c).await?;
    let pool = bucket(&c, sponsor, "2026-11-03").await?;
    for _ in 0..3 {
        reserve(&c, pool, commitment(&c).await?, "300").await?;
    }
    let first = commitment(&c).await?;
    let second = commitment(&c).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    reserve(&c, pool, first, "100").await?;
    let task = tokio::spawn(async move { reserve(&waiting, pool, second, "100").await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(c.query_one("SELECT sum(reserved_coins)::text FROM app.ai_loss_exposure WHERE bucket_id=$1 AND state='reserved'", &[&pool]).await?.get::<_, String>(0), "1000");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_sponsors_can_release_existing_exposure_but_cannot_reserve_again_or_replenish_losses()
-> TestResult {
    let c = connection().await?;
    let (sponsor, _) = account(&c).await?;
    let pool = bucket(&c, sponsor, "2026-11-03").await?;
    let held = reserve(&c, pool, commitment(&c).await?, "100").await?;
    let future = commitment(&c).await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&sponsor]).await?;
    assert_eq!(
        reserve(&c, pool, future, "100").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.execute(
        "UPDATE app.ai_loss_exposure SET state='released' WHERE id=$1",
        &[&held],
    )
    .await?;
    c.execute(
        "UPDATE app.ai_loss_bucket SET lost_coins=50,revision=revision+1 WHERE id=$1",
        &[&pool],
    )
    .await?;
    assert_eq!(
        c.execute(
            "UPDATE app.ai_loss_bucket SET lost_coins=0,revision=revision+1 WHERE id=$1",
            &[&pool]
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
async fn reservation_waits_for_committing_ante_terms_and_cannot_arrive_after_the_cutoff()
-> TestResult {
    let c = connection().await?;
    let (sponsor, _) = account(&c).await?;
    let pool = bucket(&c, sponsor, "2026-11-03").await?;
    let term = commitment(&c).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.ante_commitment SET state='committed' WHERE id=$1",
        &[&term.1],
    )
    .await?;
    let task = tokio::spawn(async move { reserve(&waiting, pool, term, "100").await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.ai_loss_exposure WHERE bucket_id=$1",
            &[&pool]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}
