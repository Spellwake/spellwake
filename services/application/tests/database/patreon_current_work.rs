//! Read-only logical recovery over canonical synthetic admission, not live Patreon evidence.
use super::*;
use tokio_postgres::error::SqlState;

async fn discover(
    c: &Client,
    campaign: &str,
    after: Option<Uuid>,
    limit: i32,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.read_patreon_refresh_work($1,$2,$3)",
        &[&campaign, &after, &limit],
    )
    .await
    .map(|r| r.get(0))
}
async fn recover(
    c: &Client,
    association: Uuid,
    request: Uuid,
) -> Result<Option<Value>, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.read_patreon_refresh_result($1,$2)",
        &[&association, &request],
    )
    .await
    .map(|r| r.get(0))
}
async fn accept(c: &Client, l: &Linked, request: Uuid) -> Result<Value, tokio_postgres::Error> {
    let ticket = prepare(c, l.association).await?.unwrap();
    commit(c, request, &ticket, &proof(l, json!([]))).await
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn discovery_orders_bounds_and_filters_without_granting_admission() -> TestResult {
    let c = connection().await?;
    let first = linked(&c).await?;
    let second = linked(&c).await?;
    let foreign = linked(&c).await?;
    let campaign = first.proof["campaignId"].as_str().unwrap();
    let (active_tier, _) = tier(&c, 17).await?;
    // Owner-only fixture moves a second admitted row into the same campaign.
    // It is deliberately not valid admission under that campaign's original source.
    c.batch_execute("SET session_replication_role=replica")
        .await?;
    c.execute(
        "UPDATE app.patreon_membership SET patreon_campaign_ref=$2,active=true,active_tier_id=$3 WHERE id=$1",
        &[&second.association, &campaign, &active_tier],
    )
    .await?;
    c.batch_execute("SET session_replication_role=origin; SET ROLE service_role")
        .await?;
    let mut ids = vec![first.association, second.association];
    ids.sort();
    assert_eq!(discover(&c, campaign, None, 100).await?, json!(ids));
    assert_eq!(discover(&c, campaign, None, 1).await?, json!([ids[0]]));
    assert_eq!(
        discover(&c, campaign, Some(ids[0]), 1).await?,
        json!([ids[1]])
    );
    assert_eq!(discover(&c, campaign, Some(ids[1]), 100).await?, json!([]));
    assert!(prepare(&c, second.association).await.is_err());
    assert_eq!(
        discover(&c, foreign.proof["campaignId"].as_str().unwrap(), None, 100).await?,
        json!([foreign.association])
    );
    let request = Uuid::new_v4();
    oauth::start(&c, first.identity, request, &oauth::hash(), &first.terms).await?;
    assert_eq!(discover(&c, campaign, None, 100).await?, json!(ids));
    assert!(prepare(&c, first.association).await?.is_none());
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&first.identity.actor]).await?;
    assert_eq!(
        discover(&c, campaign, None, 100).await?,
        json!([second.association])
    );
    c.batch_execute("SET session_replication_role=replica")
        .await?;
    c.execute("UPDATE app.patreon_membership SET patreon_user_ref=NULL,patreon_campaign_ref=NULL WHERE id=$1",&[&second.association]).await?;
    c.batch_execute("SET session_replication_role=origin")
        .await?;
    assert_eq!(discover(&c, campaign, None, 100).await?, json!([]));
    assert!(
        recover(&c, second.association, Uuid::new_v4())
            .await?
            .is_none()
    );
    // Owner-only retained-membership fixture, not real retirement qualification.
    c.batch_execute("SET session_replication_role=replica")
        .await?;
    c.execute("UPDATE app.account SET state='deleted',deletion_requested_at=clock_timestamp(),deletion_finalized_at=clock_timestamp(),retirement_operation_id=$2 WHERE id=$1", &[&foreign.identity.actor,&Uuid::new_v4()]).await?;
    c.batch_execute("SET session_replication_role=origin")
        .await?;
    assert_eq!(
        discover(&c, foreign.proof["campaignId"].as_str().unwrap(), None, 100).await?,
        json!([])
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn recovery_survives_newer_refresh_member_reauthorization_and_pending_deletion() -> TestResult
{
    let c = connection().await?;
    let l = linked(&c).await?;
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    assert!(recover(&c, l.association, request).await?.is_none());
    let accepted = accept(&c, &l, request).await?;
    assert_eq!(
        recover(&c, l.association, request).await?,
        Some(accepted.clone())
    );
    accept(&c, &l, Uuid::new_v4()).await?;
    assert_eq!(
        recover(&c, l.association, request).await?,
        Some(accepted.clone())
    );
    assert!(recover(&c, l.association, Uuid::new_v4()).await?.is_none());
    assert!(recover(&c, Uuid::new_v4(), request).await?.is_none());
    let mut changed = l.proof.clone();
    changed["memberId"] = json!(format!("member-{}", Uuid::new_v4()));
    let r = Uuid::new_v4();
    let state = oauth::hash();
    let code = oauth::hash();
    let nonce = oauth::hash();
    oauth::start(&c, l.identity, r, &state, &l.terms).await?;
    oauth::claim(&c, l.identity, r, &state, &code, &nonce).await?;
    oauth::finish(&c, l.identity, r, &state, &code, &nonce, &changed).await?;
    assert_eq!(
        recover(&c, l.association, request).await?,
        Some(accepted.clone())
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&l.identity.actor]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(recover(&c, l.association, request).await?, Some(accepted));
    assert!(prepare(&c, l.association).await?.is_none());
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn read_arguments_capabilities_volatility_and_no_write_contract() -> TestResult {
    let c = connection().await?;
    let l = linked(&c).await?;
    let request = Uuid::new_v4();
    let accepted = accept(&c, &l, request).await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        for sql in [
            "SELECT app.read_patreon_refresh_work('campaign',NULL,1)",
            "SELECT app.read_patreon_refresh_result('11111111-1111-1111-1111-111111111111','22222222-2222-2222-2222-222222222222')",
        ] {
            let result = c.query_one(sql, &[]).await;
            if role == "service_role" {
                result?;
            } else {
                assert_eq!(
                    result.unwrap_err().code(),
                    Some(&SqlState::INSUFFICIENT_PRIVILEGE)
                );
            }
        }
        assert_eq!(
            c.execute(
                "UPDATE app.provider_event SET capture=capture WHERE false",
                &[]
            )
            .await
            .unwrap_err()
            .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        assert_eq!(
            c.execute(
                "UPDATE app.command_receipt SET result_ref=result_ref WHERE false",
                &[]
            )
            .await
            .unwrap_err()
            .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        c.batch_execute("RESET ROLE").await?;
    }
    for sql in [
        "SELECT app.read_patreon_refresh_work(NULL,NULL,1)",
        "SELECT app.read_patreon_refresh_work('',NULL,1)",
        "SELECT app.read_patreon_refresh_work('é',NULL,1)",
        "SELECT app.read_patreon_refresh_work(repeat('a',129),NULL,1)",
        "SELECT app.read_patreon_refresh_work('a b',NULL,1)",
        "SELECT app.read_patreon_refresh_work('a',NULL,NULL)",
        "SELECT app.read_patreon_refresh_work('a',NULL,0)",
        "SELECT app.read_patreon_refresh_work('a',NULL,101)",
        "SELECT app.read_patreon_refresh_work('a','00000000-0000-0000-0000-000000000000',1)",
        "SELECT app.read_patreon_refresh_result(NULL,NULL)",
        "SELECT app.read_patreon_refresh_result('11111111-1111-1111-1111-111111111111',NULL)",
        "SELECT app.read_patreon_refresh_result('00000000-0000-0000-0000-000000000000','11111111-1111-1111-1111-111111111111')",
        "SELECT app.read_patreon_refresh_result('11111111-1111-1111-1111-111111111111','00000000-0000-0000-0000-000000000000')",
    ] {
        assert_eq!(
            c.query_one(sql, &[]).await.unwrap_err().code(),
            Some(&SqlState::INVALID_PARAMETER_VALUE),
            "{sql}"
        );
    }
    for (signature, volatility) in [
        ("app.read_patreon_refresh_work(text,uuid,integer)", "s"),
        ("app.read_patreon_refresh_result(uuid,uuid)", "s"),
        (
            "app.patreon_oauth_commit_source(app.patreon_membership)",
            "v",
        ),
    ] {
        assert_eq!(
            c.query_one(
                "SELECT provolatile::text FROM pg_proc WHERE oid=$1::text::regprocedure",
                &[&signature]
            )
            .await?
            .get::<_, String>(0),
            volatility
        );
    }
    c.batch_execute("BEGIN; SET ROLE service_role").await?;
    let before:i64=c.query_one("SELECT coalesce(sum(n_tup_ins+n_tup_upd+n_tup_del),0)::bigint FROM pg_stat_xact_user_tables",&[]).await?.get(0);
    assert_eq!(recover(&c, l.association, request).await?, Some(accepted));
    assert_eq!(
        discover(&c, l.proof["campaignId"].as_str().unwrap(), None, 100).await?,
        json!([l.association])
    );
    assert_eq!(c.query_one("SELECT coalesce(sum(n_tup_ins+n_tup_upd+n_tup_del),0)::bigint FROM pg_stat_xact_user_tables",&[]).await?.get::<_,i64>(0),before);
    c.batch_execute("ROLLBACK; RESET ROLE; BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    assert!(recover(&c, l.association, request).await?.is_some());
    discover(&c, "a", None, 1).await?;
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn recovery_rejects_corrupt_source_root_receipt_and_parent_without_fabricating_success()
-> TestResult {
    let c = connection().await?;
    let l = linked(&c).await?;
    accept(&c, &l, Uuid::new_v4()).await?;
    let request = Uuid::new_v4();
    let accepted = accept(&c, &l, request).await?;
    let source: Uuid = serde_json::from_value(accepted["sourceEventId"].clone())?;
    // Owner-only rollback fixtures bypass production immutability guards, one premise at a time.
    for (sql, code) in [
        (
            "UPDATE app.provider_event SET payload_ref='sha256:bad' WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET processing_state='verified' WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET event_type='api.other' WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET account_id=NULL WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET effect=jsonb_set(effect,'{proofHash}','\"bad\"') WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET effect=jsonb_set(effect,'{ticketHash}','\"bad\"') WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET effect=effect||'{\"extra\":true}'::jsonb WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET capture=capture||'{\"extra\":true}'::jsonb WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET effect=jsonb_set(effect,'{associationId}','\"11111111-1111-1111-1111-111111111111\"') WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET effect=jsonb_set(effect,'{requestId}','\"11111111-1111-1111-1111-111111111111\"') WHERE id=$1",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.provider_event SET effect=jsonb_set(effect,'{requestId}','\"not-uuid\"') WHERE id=$1",
            SqlState::INVALID_TEXT_REPRESENTATION,
        ),
        (
            "UPDATE app.provider_event SET payload_ref='bad' WHERE id=(SELECT (effect->>'parentEventId')::uuid FROM app.provider_event WHERE id=$1)",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "DELETE FROM app.provider_event WHERE id=(SELECT (effect->>'anchorEventId')::uuid FROM app.provider_event WHERE id=$1)",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "DELETE FROM app.provider_event WHERE id=(SELECT (effect->>'parentEventId')::uuid FROM app.provider_event WHERE id=$1)",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "DELETE FROM app.command_receipt WHERE scope='patreon_exchange_claim' AND account_id=(SELECT account_id FROM app.provider_event WHERE id=$1)",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "DELETE FROM app.command_receipt WHERE scope='patreon_start' AND account_id=(SELECT account_id FROM app.provider_event WHERE id=$1)",
            SqlState::CHECK_VIOLATION,
        ),
        (
            "UPDATE app.command_receipt SET result_ref='not-json' WHERE scope='patreon_complete' AND account_id=(SELECT account_id FROM app.provider_event WHERE id=$1)",
            SqlState::INVALID_TEXT_REPRESENTATION,
        ),
        (
            "DELETE FROM app.command_receipt WHERE scope='patreon_complete' AND account_id=(SELECT account_id FROM app.provider_event WHERE id=$1)",
            SqlState::CHECK_VIOLATION,
        ),
    ] {
        c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
            .await?;
        assert_eq!(c.execute(sql, &[&source]).await?, 1, "{sql}");
        c.batch_execute("SET LOCAL session_replication_role=origin; SET ROLE service_role")
            .await?;
        assert_eq!(
            recover(&c, l.association, request)
                .await
                .unwrap_err()
                .code(),
            Some(&code),
            "{sql}"
        );
        c.batch_execute("ROLLBACK; RESET ROLE").await?;
        assert_eq!(
            recover(&c, l.association, request).await?,
            Some(accepted.clone())
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn recovery_facade_keeps_calling_snapshot_and_next_statement_sees_commit() -> TestResult {
    let owner = connection().await?;
    let reader = connection().await?;
    let l = linked(&owner).await?;
    let request = Uuid::new_v4();
    let gate: i64 = owner
        .query_one(
            "SELECT hashtextextended($1,0)",
            &[&Uuid::new_v4().to_string()],
        )
        .await?
        .get(0);
    owner
        .query_one("SELECT pg_advisory_lock($1)", &[&gate])
        .await?;
    let pid: i32 = reader
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    reader.batch_execute("SET ROLE service_role").await?;
    let association = l.association;
    let task = tokio::spawn(async move {
        let result:Option<Value>=reader.query_one("WITH gate AS MATERIALIZED (SELECT pg_advisory_xact_lock($1)) SELECT app.read_patreon_refresh_result($2,$3) FROM gate",&[&gate,&association,&request]).await?.get(0);
        Ok::<_, tokio_postgres::Error>((reader, result))
    });
    blocked(&owner, pid).await?;
    let accepted = accept(&owner, &l, request).await?;
    owner
        .query_one("SELECT pg_advisory_unlock($1)", &[&gate])
        .await?;
    let (reader, first) = task.await??;
    assert!(first.is_none());
    assert_eq!(
        recover(&reader, association, request).await?,
        Some(accepted)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn discovered_candidate_cannot_admit_after_concurrent_account_deletion() -> TestResult {
    let owner = connection().await?;
    let reader = connection().await?;
    let l = linked(&owner).await?;
    let campaign = l.proof["campaignId"].as_str().unwrap().to_owned();
    let gate: i64 = owner
        .query_one(
            "SELECT hashtextextended($1,0)",
            &[&Uuid::new_v4().to_string()],
        )
        .await?
        .get(0);
    owner
        .query_one("SELECT pg_advisory_lock($1)", &[&gate])
        .await?;
    let pid: i32 = reader
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    reader.batch_execute("SET ROLE service_role").await?;
    let task = tokio::spawn(async move {
        let page: Value = reader.query_one("WITH gate AS MATERIALIZED (SELECT pg_advisory_xact_lock($1)) SELECT app.read_patreon_refresh_work($2,NULL,100) FROM gate", &[&gate,&campaign]).await?.get(0);
        Ok::<_, tokio_postgres::Error>((reader, page, campaign))
    });
    blocked(&owner, pid).await?;
    owner.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&l.identity.actor]).await?;
    owner
        .query_one("SELECT pg_advisory_unlock($1)", &[&gate])
        .await?;
    let (reader, page, campaign) = task.await??;
    assert_eq!(page, json!([l.association]));
    assert!(prepare(&reader, l.association).await?.is_none());
    assert_eq!(discover(&reader, &campaign, None, 100).await?, json!([]));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn accepted_history_survives_concurrent_same_user_new_member_authorization() -> TestResult {
    let owner = connection().await?;
    let reader = connection().await?;
    let l = linked(&owner).await?;
    let request = Uuid::new_v4();
    let accepted = accept(&owner, &l, request).await?;
    let gate: i64 = owner
        .query_one(
            "SELECT hashtextextended($1,0)",
            &[&Uuid::new_v4().to_string()],
        )
        .await?
        .get(0);
    owner
        .query_one("SELECT pg_advisory_lock($1)", &[&gate])
        .await?;
    let pid: i32 = reader
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    reader.batch_execute("SET ROLE service_role").await?;
    let association = l.association;
    let task = tokio::spawn(async move {
        let result: Option<Value> = reader.query_one("WITH gate AS MATERIALIZED (SELECT pg_advisory_xact_lock($1)) SELECT app.read_patreon_refresh_result($2,$3) FROM gate", &[&gate,&association,&request]).await?.get(0);
        Ok::<_, tokio_postgres::Error>((reader, result))
    });
    blocked(&owner, pid).await?;
    let mut changed = l.proof.clone();
    changed["memberId"] = json!(format!("member-{}", Uuid::new_v4()));
    let r = Uuid::new_v4();
    let state = oauth::hash();
    let code = oauth::hash();
    let nonce = oauth::hash();
    owner.batch_execute("SET ROLE service_role").await?;
    oauth::start(&owner, l.identity, r, &state, &l.terms).await?;
    oauth::claim(&owner, l.identity, r, &state, &code, &nonce).await?;
    assert_eq!(
        oauth::finish(&owner, l.identity, r, &state, &code, &nonce, &changed).await?["status"],
        "success"
    );
    owner
        .query_one("SELECT pg_advisory_unlock($1)", &[&gate])
        .await?;
    let (reader, result) = task.await??;
    assert_eq!(result, Some(accepted.clone()));
    assert_eq!(
        recover(&reader, association, request).await?,
        Some(accepted)
    );
    assert_eq!(
        prepare(&reader, association).await?.unwrap()["memberId"],
        changed["memberId"]
    );
    Ok(())
}
