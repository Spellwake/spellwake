//! Privileged relational fixtures, not accepted ante valuation or settlement evidence.
use super::{TestResult, account, connection, game_scope, operation, wait_for_database_lock};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn terms(c: &Client) -> FixtureResult<(game_scope::Seed, Uuid)> {
    let f = game_scope::seed(c).await?;
    c.execute(
        "UPDATE app.match SET mode='collection' WHERE id=$1",
        &[&f.match_id],
    )
    .await?;
    let id = c.query_one("INSERT INTO app.ante_commitment(match_id,terms_revision,state) VALUES($1,0,'offered') RETURNING id", &[&f.match_id]).await?.get(0);
    Ok((f, id))
}
async fn hold(c: &Client, owner: Uuid, commitment: Uuid, purpose: &str) -> FixtureResult<Uuid> {
    let op = operation(c, owner).await?;
    let owner: Uuid = c
        .query_one(
            "SELECT id FROM app.asset_owner WHERE account_id=$1",
            &[&owner],
        )
        .await?
        .get(0);
    Ok(c.query_one("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,$3::uuid::text,'held',$4) RETURNING id", &[&owner,&purpose,&commitment,&op]).await?.get(0))
}
async fn stake(
    c: &Client,
    commitment: Uuid,
    seat: Uuid,
    funding: Uuid,
    ai: bool,
    amount: &str,
) -> Result<Uuid, tokio_postgres::Error> {
    c.query_one("INSERT INTO app.ante_stake(commitment_id,match_seat_id,funding_kind,hold_id,ai_exposure_id,estimated_value_coins,valuation_policy_id) SELECT $1,$2,CASE WHEN $4 THEN 'ai_loss_allowance' ELSE 'owned_assets' END,CASE WHEN $4 THEN NULL::uuid ELSE $3 END,CASE WHEN $4 THEN $3 ELSE NULL::uuid END,$5::text::numeric,id FROM app.valuation_policy_revision WHERE state='active' RETURNING id", &[&commitment,&seat,&funding,&ai,&amount]).await.map(|r|r.get(0))
}
async fn ai_seat(c: &Client, match_id: Uuid, sponsor: Uuid, index: i64) -> FixtureResult<Uuid> {
    Ok(c.query_one("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,$3,'ai') RETURNING id", &[&match_id,&sponsor,&index]).await?.get(0))
}
async fn exposure(
    c: &Client,
    match_id: Uuid,
    commitment: Uuid,
    sponsor: Uuid,
) -> FixtureResult<Uuid> {
    let bucket: Uuid = c.query_one("INSERT INTO app.ai_loss_bucket(sponsor_id,utc_day,limit_coins,lost_coins,revision) VALUES($1,'2026-11-03',1000,0,0) RETURNING id", &[&sponsor]).await?.get(0);
    Ok(c.query_one("INSERT INTO app.ai_loss_exposure(bucket_id,match_id,commitment_id,reserved_coins,state) VALUES($1,$2,$3,200,'reserved') RETURNING id", &[&bucket,&match_id,&commitment]).await?.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn human_stakes_require_the_same_match_owner_ante_purpose_and_commitment() -> TestResult {
    let c = connection().await?;
    let (mut f, t) = terms(&c).await?;
    let separate_owner = Uuid::new_v4();
    let (separate_account, _) = super::account_fixture(&c, 0, Some(separate_owner)).await?;
    assert_ne!(separate_owner, separate_account);
    c.execute(
        "UPDATE app.match_seat SET account_id=$1 WHERE id=$2",
        &[&separate_account, &f.seat],
    )
    .await?;
    f.account = separate_account;
    let (other, other_t) = terms(&c).await?;
    let valid = hold(&c, f.account, t, "ante").await?;
    let wrong_owner = hold(&c, other.account, t, "ante").await?;
    let wrong_terms = hold(&c, f.account, other_t, "ante").await?;
    for (seat, funding) in [
        (other.seat, valid),
        (f.seat, wrong_owner),
        (f.seat, wrong_terms),
    ] {
        assert_eq!(
            stake(&c, t, seat, funding, false, "100")
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    assert_eq!(
        stake(&c, t, f.seat, valid, false, "0")
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    // Other-domain hold source checks are deferred; isolate the rejected attempt
    // so the intentionally incomplete trade fixture can never commit.
    c.batch_execute("BEGIN").await?;
    let wrong_purpose = hold(&c, f.account, t, "trade").await?;
    let denied = stake(&c, t, f.seat, wrong_purpose, false, "100")
        .await
        .unwrap_err();
    assert_eq!(
        denied.as_db_error().unwrap().message(),
        "ante stake must bind its match seat and funding source"
    );
    c.batch_execute("ROLLBACK").await?;
    stake(&c, t, f.seat, valid, false, "100").await?;
    assert_eq!(
        c.execute(
            "UPDATE app.match SET mode='open' WHERE id=$1",
            &[&f.match_id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.match_seat SET account_id=$1 WHERE id=$2",
            &[&other.account, &f.seat]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let empty_match = game_scope::seed(&c).await?;
    let denied = c
        .execute(
            "UPDATE app.ante_commitment SET match_id=$1 WHERE id=$2",
            &[&empty_match.match_id, &t],
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        denied.as_db_error().unwrap().message(),
        "ante stake must bind its match seat and funding source"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ai_stakes_share_one_exposure_without_cross_sponsor_or_cross_commitment_funding()
-> TestResult {
    let c = connection().await?;
    let (f, t) = terms(&c).await?;
    let (other, other_t) = terms(&c).await?;
    let a = ai_seat(&c, f.match_id, f.account, 1).await?;
    let b = ai_seat(&c, f.match_id, f.account, 2).await?;
    let (stranger, _) = account(&c).await?;
    let wrong_sponsor = ai_seat(&c, f.match_id, stranger, 3).await?;
    let x = exposure(&c, f.match_id, t, f.account).await?;
    let wrong = exposure(&c, other.match_id, other_t, other.account).await?;
    for (seat, funding) in [(wrong_sponsor, x), (a, wrong), (f.seat, x)] {
        assert_eq!(
            stake(&c, t, seat, funding, true, "100")
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    stake(&c, t, a, x, true, "100").await?;
    assert_eq!(
        stake(&c, t, b, x, true, "101").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    stake(&c, t, b, x, true, "100").await?;
    assert_eq!(
        c.execute(
            "UPDATE app.ai_loss_exposure SET reserved_coins=199 WHERE id=$1",
            &[&x]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.match_seat SET sponsor_id=$1 WHERE id=$2",
            &[&stranger, &a]
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
async fn ante_scope_supports_deferred_validation_but_rejects_an_invalid_commit_atomically()
-> TestResult {
    let c = connection().await?;
    let (f, t) = terms(&c).await?;
    let (other, _) = terms(&c).await?;
    let h = hold(&c, f.account, t, "ante").await?;
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    stake(&c, t, other.seat, h, false, "100").await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.ante_stake WHERE commitment_id=$1",
            &[&t]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stake_insertion_waits_for_parent_reassignment_and_rejects_the_new_wrong_match()
-> TestResult {
    let c = connection().await?;
    let (f, t) = terms(&c).await?;
    let (other, _) = terms(&c).await?;
    let h = hold(&c, f.account, t, "ante").await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.match_seat SET match_id=$1,seat_index=5 WHERE id=$2",
        &[&other.match_id, &f.seat],
    )
    .await?;
    let task = tokio::spawn(async move { stake(&waiting, t, f.seat, h, false, "100").await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
