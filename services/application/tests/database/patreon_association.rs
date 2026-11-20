//! Synthetic normalized OAuth source proofs exercise SQL authority, never live provider OAuth.
use super::{TestResult, connection, pricing::buyer, set_auth_claims};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
#[derive(Clone, Copy)]
pub(super) struct Identity {
    pub(super) actor: Uuid,
    pub(super) subject: Uuid,
    pub(super) session: Uuid,
}
pub(super) async fn identity(
    c: &Client,
) -> Result<Identity, Box<dyn std::error::Error + Send + Sync>> {
    let (actor, subject) = buyer(c).await?;
    let session = c
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    Ok(Identity {
        actor,
        subject,
        session,
    })
}
pub(super) fn hash() -> String {
    Uuid::new_v4().simple().to_string().repeat(2)
}
pub(super) fn terms() -> Value {
    json!({"version":1,"campaignId":format!("campaign-{}",Uuid::new_v4()),"clientId":format!("client-{}",Uuid::new_v4()),"redirectUri":"https://spellwake.example.invalid/auth/patreon"})
}
pub(super) fn proof(t: &Value) -> Value {
    json!({"version":1,"source":"patreon-v2-identity-member-v1","campaignId":t["campaignId"],"userId":format!("user-{}",Uuid::new_v4()),"memberId":format!("member-{}",Uuid::new_v4()),"identityHash":hash(),"memberHash":hash(),"entitledTierIds":[]})
}
pub(super) async fn start(
    c: &Client,
    i: Identity,
    r: Uuid,
    s: &str,
    t: &Value,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.start_patreon_link($1,$2,$3,$4,$5)",
        &[&i.subject, &i.session, &r, &s, t],
    )
    .await
    .map(|v| v.get(0))
}
pub(super) async fn view(
    c: &Client,
    i: Identity,
    r: Uuid,
    s: &str,
    code: Option<&str>,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.read_patreon_link($1,$2,$3,$4,$5)",
        &[&i.subject, &i.session, &r, &s, &code],
    )
    .await
    .map(|v| v.get(0))
}
pub(super) async fn claim(
    c: &Client,
    i: Identity,
    r: Uuid,
    s: &str,
    code: &str,
    nonce: &str,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.claim_patreon_link($1,$2,$3,$4,$5,$6)",
        &[&i.subject, &i.session, &r, &s, &code, &nonce],
    )
    .await
    .map(|v| v.get(0))
}
pub(super) async fn finish(
    c: &Client,
    i: Identity,
    r: Uuid,
    s: &str,
    code: &str,
    nonce: &str,
    p: &Value,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.complete_patreon_link($1,$2,$3,$4,$5,$6,$7)",
        &[&i.subject, &i.session, &r, &s, &code, &nonce, p],
    )
    .await
    .map(|v| v.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_issue_claim_complete_replays_without_payment_grants_and_preserves_source_privacy()
-> TestResult {
    let c = connection().await?;
    let i = identity(&c).await?;
    let r = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    let t = terms();
    let p = proof(&t);
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        view(&c, i, r, &state, None).await?["data"]["phase"],
        "unknown"
    );
    let issued = start(&c, i, r, &state, &t).await?;
    assert_eq!(issued["status"], "success");
    assert_eq!(start(&c, i, r, &state, &t).await?, issued);
    assert_eq!(
        view(&c, i, r, &state, None).await?["data"]["phase"],
        "issued"
    );
    assert_eq!(
        start(&c, i, r, &hash(), &t).await?["data"]["request"],
        "conflict"
    );
    assert_eq!(
        claim(&c, i, r, &state, &code, &nonce).await?["data"]["mayExchange"],
        true
    );
    assert_eq!(
        claim(&c, i, r, &state, &code, &hash()).await?["data"]["mayExchange"],
        false
    );
    assert_eq!(
        claim(&c, i, r, &state, &hash(), &nonce).await?["data"]["request"],
        "conflict"
    );
    assert_eq!(
        view(&c, i, r, &state, Some(&code)).await?["data"]["phase"],
        "claimed"
    );
    let accepted = finish(&c, i, r, &state, &code, &nonce, &p).await?;
    assert_eq!(accepted["status"], "success");
    assert_eq!(finish(&c, i, r, &state, &code, &nonce, &p).await?, accepted);
    assert_eq!(
        view(&c, i, r, &state, Some(&hash())).await?["data"]["request"],
        "conflict"
    );
    assert_eq!(
        view(&c, i, r, &state, Some(&code)).await?["data"]["phase"],
        "linked"
    );
    c.batch_execute("RESET ROLE").await?;
    let association: Uuid = accepted["data"]["resourceId"].as_str().unwrap().parse()?;
    let row=c.query_one("SELECT patreon_user_ref,patreon_campaign_ref,active,active_tier_id FROM app.patreon_membership WHERE id=$1",&[&association]).await?;
    assert_eq!(row.get::<_, String>(0), p["userId"].as_str().unwrap());
    assert_eq!(row.get::<_, String>(1), t["campaignId"].as_str().unwrap());
    assert!(!row.get::<_, bool>(2));
    assert!(row.get::<_, Option<Uuid>>(3).is_none());
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.patreon_payment WHERE membership_id=$1",
            &[&association]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.cosmetic_grant WHERE account_id=$1 AND source_kind='patreon_payment'",&[&i.actor]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND state='pending'",
            &[&i.actor]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    set_auth_claims(&c, i.subject).await?;
    assert!(
        !c.query_one(
            "SELECT active FROM app.patreon_membership WHERE id=$1",
            &[&association]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert!(
        c.query_one(
            "SELECT patreon_user_ref FROM app.patreon_membership WHERE id=$1",
            &[&association]
        )
        .await
        .is_err()
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_latest_start_fences_old_sources_and_same_user_reauthorization_keeps_one_binding()
-> TestResult {
    let c = connection().await?;
    let i = identity(&c).await?;
    let t = terms();
    let mut p = proof(&t);
    let state = hash();
    let code = hash();
    let nonce = hash();
    let older = Uuid::new_v4();
    let newer = Uuid::new_v4();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let tier_a = format!("tier-{a}");
    let tier_b = format!("tier-{b}");
    for (id, external) in [(a, &tier_a), (b, &tier_b)] {
        c.execute("INSERT INTO app.supporter_tier(id,patreon_tier_ref,name,rank) VALUES($1,$2,'Synthetic tier',5)",&[&id,external]).await?;
    }
    p["entitledTierIds"] = json!([tier_b, tier_a]);
    c.batch_execute("SET ROLE service_role").await?;
    start(&c, i, older, &state, &t).await?;
    claim(&c, i, older, &state, &code, &nonce).await?;
    start(&c, i, newer, &state, &t).await?;
    assert_eq!(
        finish(&c, i, older, &state, &code, &nonce, &p).await?["data"]["request"],
        "superseded"
    );
    let code2 = hash();
    assert_eq!(
        claim(&c, i, newer, &state, &code2, &nonce).await?["data"]["mayExchange"],
        true
    );
    let result = finish(&c, i, newer, &state, &code2, &nonce, &p).await?;
    assert_eq!(result["status"], "success");
    assert_eq!(
        c.query_one(
            "SELECT active_tier_id FROM app.patreon_membership WHERE account_id=$1",
            &[&i.actor]
        )
        .await?
        .get::<_, Uuid>(0),
        a.min(b)
    );
    let third = Uuid::new_v4();
    let code3 = hash();
    start(&c, i, third, &state, &t).await?;
    claim(&c, i, third, &state, &code3, &nonce).await?;
    p["memberId"] = json!(format!("renewed-{}", Uuid::new_v4()));
    p["entitledTierIds"] = json!([]);
    assert_eq!(
        finish(&c, i, third, &state, &code3, &nonce, &p).await?,
        result
    );
    assert_eq!(
        view(&c, i, newer, &state, Some(&code2)).await?["data"]["phase"],
        "linked"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.patreon_membership WHERE account_id=$1",
            &[&i.actor]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    let row=c.query_one("SELECT patreon_member_ref,active,active_tier_id FROM app.patreon_membership WHERE account_id=$1",&[&i.actor]).await?;
    assert_eq!(row.get::<_, String>(0), p["memberId"].as_str().unwrap());
    assert!(!row.get::<_, bool>(1));
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.provider_event WHERE account_id=$1 AND provider='patreon'",
            &[&i.actor]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_shared_request_ids_stay_actor_scoped_and_spent_codes_cannot_cross_flows()
-> TestResult {
    let c = connection().await?;
    let a = identity(&c).await?;
    let b = identity(&c).await?;
    let t = terms();
    let r = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    let pa = proof(&t);
    let pb = proof(&t);
    c.batch_execute("SET ROLE service_role").await?;
    start(&c, a, r, &state, &t).await?;
    start(&c, b, r, &state, &t).await?;
    assert_eq!(
        claim(&c, a, r, &state, &code, &nonce).await?["data"]["mayExchange"],
        true
    );
    assert_eq!(
        claim(&c, b, r, &state, &code, &nonce).await?["data"]["request"],
        "spent"
    );
    assert_eq!(
        view(&c, b, r, &state, None).await?["data"]["phase"],
        "issued"
    );
    let code_b = hash();
    assert_eq!(
        claim(&c, b, r, &state, &code_b, &nonce).await?["data"]["mayExchange"],
        true
    );
    let linked_a = finish(&c, a, r, &state, &code, &nonce, &pa).await?;
    let linked_b = finish(&c, b, r, &state, &code_b, &nonce, &pb).await?;
    assert_eq!(linked_a["status"], "success");
    assert_eq!(linked_b["status"], "success");
    assert_ne!(
        linked_a["data"]["resourceId"],
        linked_b["data"]["resourceId"]
    );
    let later = Uuid::new_v4();
    let later_code = hash();
    start(&c, b, later, &state, &t).await?;
    claim(&c, b, later, &state, &later_code, &nonce).await?;
    assert_eq!(
        finish(&c, b, later, &state, &later_code, &nonce, &pa).await?["data"]["membership"],
        "unavailable"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(c.query_one("SELECT count(*) FROM app.provider_event WHERE provider='patreon' AND account_id IN ($1,$2)",&[&a.actor,&b.actor]).await?.get::<_,i64>(0),2);
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='patreon_complete'",&[&b.actor]).await?.get::<_,i64>(0),1);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_invalid_proofs_claim_secrets_and_pending_accounts_have_no_partial_effect()
-> TestResult {
    let c = connection().await?;
    let i = identity(&c).await?;
    let t = terms();
    let r = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    let p = proof(&t);
    c.batch_execute("SET ROLE service_role").await?;
    start(&c, i, r, &state, &t).await?;
    claim(&c, i, r, &state, &code, &nonce).await?;
    for malformed in [
        json!({}),
        {
            let mut v = p.clone();
            v["campaignId"] = json!("foreign");
            v
        },
        {
            let mut v = p.clone();
            v["entitledTierIds"] = json!(["duplicate", "duplicate"]);
            v
        },
        {
            let mut v = p.clone();
            v["access_token"] = json!("private-must-not-enter-proof");
            v
        },
        {
            let mut v = p.clone();
            v["userId"] = Value::Null;
            v
        },
    ] {
        assert_eq!(
            finish(&c, i, r, &state, &code, &nonce, &malformed).await?["data"]["proof"],
            "invalid"
        );
    }
    assert_eq!(
        finish(&c, i, r, &state, &code, &hash(), &p).await?["data"]["request"],
        "conflict"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.provider_event WHERE account_id=$1",
            &[&i.actor]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='patreon_complete'",&[&i.actor]).await?.get::<_,i64>(0),0);
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&i.actor]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        finish(&c, i, r, &state, &code, &nonce, &p).await?["data"]["account"],
        "unavailable"
    );
    assert_eq!(
        start(&c, i, Uuid::new_v4(), &state, &t).await?["data"]["account"],
        "unavailable"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.patreon_membership WHERE account_id=$1",
            &[&i.actor]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_runtime_capabilities_and_private_columns_are_explicit() -> TestResult {
    let c = connection().await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for (signature, service) in [
            ("app.start_patreon_link(uuid,uuid,uuid,text,jsonb)", true),
            ("app.read_patreon_link(uuid,uuid,uuid,text,text)", true),
            (
                "app.claim_patreon_link(uuid,uuid,uuid,text,text,text)",
                true,
            ),
            (
                "app.complete_patreon_link(uuid,uuid,uuid,text,text,text,jsonb)",
                true,
            ),
            ("app.patreon_terms(jsonb)", false),
            ("app.patreon_proof(jsonb)", false),
            ("app.patreon_start_record(uuid,uuid,uuid,text)", false),
            ("app.patreon_attempt_terms(uuid,uuid,text,text)", false),
            (
                "app.patreon_link_view(app.account,uuid,uuid,text,text)",
                false,
            ),
            ("app.patreon_entitled_tier(jsonb)", false),
            ("app.guard_membership_source()", false),
            ("app.check_patreon_association_commit()", false),
        ] {
            assert_eq!(
                c.query_one(
                    "SELECT has_function_privilege($1,$2,'EXECUTE')",
                    &[&role, &signature]
                )
                .await?
                .get::<_, bool>(0),
                role == "service_role" && service,
                "{role}:{signature}"
            );
        }
        assert!(
            !c.query_one(
                "SELECT has_sequence_privilege($1,'app.patreon_start_generation','USAGE')",
                &[&role]
            )
            .await?
            .get::<_, bool>(0)
        );
        for column in [
            "patreon_member_ref",
            "patreon_user_ref",
            "patreon_campaign_ref",
            "source_event_id",
        ] {
            assert_eq!(
                c.query_one(
                    "SELECT has_column_privilege($1,'app.patreon_membership',$2,'SELECT')",
                    &[&role, &column]
                )
                .await?
                .get::<_, bool>(0),
                role == "service_role"
            );
        }
        for table in [
            "app.patreon_membership",
            "app.provider_event",
            "app.command_receipt",
        ] {
            assert!(
                !c.query_one(
                    "SELECT has_table_privilege($1,$2,'INSERT')",
                    &[&role, &table]
                )
                .await?
                .get::<_, bool>(0)
            );
        }
    }
    let i = identity(&c).await?;
    let t = terms();
    c.batch_execute("SET ROLE service_role").await?;
    let mut bad = t.clone();
    bad["secret"] = json!("not-accepted");
    assert_eq!(
        start(&c, i, Uuid::nil(), &hash(), &t).await?["data"]["request"],
        "invalid"
    );
    assert_eq!(
        start(&c, i, Uuid::new_v4(), &hash(), &bad).await?["data"]["request"],
        "invalid"
    );
    c.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    assert_eq!(
        start(&c, i, Uuid::new_v4(), &hash(), &t)
            .await
            .unwrap_err()
            .code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}
async fn wait_blocked(c: &Client, pid: i32) -> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if c.query_one("SELECT cardinality(pg_blocking_pids($1))>0", &[&pid])
                .await
                .unwrap()
                .get::<_, bool>(0)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_cross_account_code_claim_wait_has_one_winner_and_no_pending_loser() -> TestResult {
    let c = connection().await?;
    let a = identity(&c).await?;
    let b = identity(&c).await?;
    let t = terms();
    let ra = Uuid::new_v4();
    let rb = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    c.batch_execute("SET ROLE service_role").await?;
    start(&c, a, ra, &state, &t).await?;
    start(&c, b, rb, &state, &t).await?;
    c.batch_execute("RESET ROLE").await?;
    let first = connection().await?;
    let second = connection().await?;
    let pid: i32 = second
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    first.batch_execute("SET ROLE service_role; BEGIN").await?;
    assert_eq!(
        claim(&first, a, ra, &state, &code, &nonce).await?["data"]["mayExchange"],
        true
    );
    second.batch_execute("SET ROLE service_role").await?;
    let s = state.clone();
    let k = code.clone();
    let n = nonce.clone();
    let task = tokio::spawn(async move { claim(&second, b, rb, &s, &k, &n).await });
    wait_blocked(&c, pid).await?;
    first.batch_execute("COMMIT").await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), task).await???["data"]["request"],
        "spent"
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='patreon_exchange_claim'",&[&b.actor]).await?.get::<_,i64>(0),0);
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id IN ($1,$2) AND state='pending'",&[&a.actor,&b.actor]).await?.get::<_,i64>(0),0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_auth_ban_after_tier_wait_rolls_back_association_event_and_completion() -> TestResult
{
    let locker = connection().await?;
    let i = identity(&locker).await?;
    let t = terms();
    let r = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    let mut p = proof(&t);
    let tier = Uuid::new_v4();
    let external = format!("tier-{tier}");
    locker.execute("INSERT INTO app.supporter_tier(id,patreon_tier_ref,name,rank) VALUES($1,$2,'Synthetic held tier',10)",&[&tier,&external]).await?;
    p["entitledTierIds"] = json!([external]);
    locker.batch_execute("SET ROLE service_role").await?;
    start(&locker, i, r, &state, &t).await?;
    claim(&locker, i, r, &state, &code, &nonce).await?;
    locker.batch_execute("RESET ROLE; BEGIN").await?;
    locker
        .query_one(
            "SELECT id FROM app.supporter_tier WHERE id=$1 FOR UPDATE",
            &[&tier],
        )
        .await?;
    let writer = connection().await?;
    let pid: i32 = writer
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    writer.batch_execute("SET ROLE service_role").await?;
    let task = tokio::spawn(async move { finish(&writer, i, r, &state, &code, &nonce, &p).await });
    wait_blocked(&locker, pid).await?;
    locker
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 day' WHERE id=$1",
            &[&i.subject],
        )
        .await?;
    locker.batch_execute("COMMIT").await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), task).await???,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    for sql in [
        "SELECT count(*) FROM app.patreon_membership WHERE account_id=$1",
        "SELECT count(*) FROM app.provider_event WHERE account_id=$1 AND provider='patreon'",
        "SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='patreon_complete'",
    ] {
        assert_eq!(
            locker.query_one(sql, &[&i.actor]).await?.get::<_, i64>(0),
            0
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_exchange_claim_rechecks_auth_after_unique_index_wait_and_rollback() -> TestResult {
    let c = connection().await?;
    let a = identity(&c).await?;
    let b = identity(&c).await?;
    let t = terms();
    let ra = Uuid::new_v4();
    let rb = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    c.batch_execute("SET ROLE service_role").await?;
    start(&c, a, ra, &state, &t).await?;
    start(&c, b, rb, &state, &t).await?;
    c.batch_execute("RESET ROLE").await?;
    let first = connection().await?;
    let second = connection().await?;
    let pid: i32 = second
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    first.batch_execute("SET ROLE service_role; BEGIN").await?;
    claim(&first, a, ra, &state, &code, &nonce).await?;
    second.batch_execute("SET ROLE service_role").await?;
    let task = tokio::spawn(async move { claim(&second, b, rb, &state, &code, &nonce).await });
    wait_blocked(&c, pid).await?;
    c.execute(
        "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 day' WHERE id=$1",
        &[&b.subject],
    )
    .await?;
    first.batch_execute("ROLLBACK").await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), task).await???,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id IN ($1,$2) AND scope='patreon_exchange_claim'",&[&a.actor,&b.actor]).await?.get::<_,i64>(0),0);
    Ok(())
}
pub(super) async fn timed_start(
    c: &Client,
    i: Identity,
    r: Uuid,
    s: &str,
    t: &Value,
    seconds: i32,
) -> TestResult {
    // Explicit owner-only normalized fixture fixes a short real expiry without changing any writer clock.
    c.execute("WITH timing AS (SELECT clock_timestamp()+$6::integer*interval '1 second' AS expiry),data AS (SELECT jsonb_build_object('version',1,'provider',$5::jsonb,'createdAt',expiry-interval '10 minutes','expiresAt',expiry,'generation',nextval('app.patreon_start_generation')::text) AS metadata FROM timing) INSERT INTO app.command_receipt(account_id,principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref,response_payload) SELECT $1,'human:'||$1::uuid::text,$3::uuid::text,'patreon_start',$1::uuid::text,app.auth_json_hash(jsonb_build_object('sessionId',$2::uuid,'stateHash',$4::text,'provider',$5::jsonb)),'accepted',metadata::text,jsonb_build_object('status','success','data',jsonb_build_object('resourceId',$3::uuid,'expiresAt',metadata->>'expiresAt')) FROM data",&[&i.actor,&i.session,&r,&s,t,&seconds]).await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres; includes a real six-second expiry wait"]
async fn patreon_expiry_blocks_new_exchange_but_accepted_completion_remains_recoverable()
-> TestResult {
    let c = connection().await?;
    let i = identity(&c).await?;
    let t = terms();
    let r = Uuid::new_v4();
    let expired = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    let p = proof(&t);
    timed_start(&c, i, expired, &state, &t, -60).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        view(&c, i, expired, &state, None).await?["data"]["phase"],
        "expired"
    );
    assert_eq!(
        claim(&c, i, expired, &state, &code, &nonce).await?["data"]["request"],
        "expired"
    );
    c.batch_execute("RESET ROLE").await?;
    timed_start(&c, i, r, &state, &t, 5).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        claim(&c, i, r, &state, &code, &nonce).await?["data"]["mayExchange"],
        true
    );
    let accepted = finish(&c, i, r, &state, &code, &nonce, &p).await?;
    assert_eq!(accepted["status"], "success");
    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
    assert_eq!(
        view(&c, i, r, &state, Some(&code)).await?["data"]["phase"],
        "linked"
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&i.actor]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(finish(&c, i, r, &state, &code, &nonce, &p).await?, accepted);
    assert_eq!(
        claim(&c, i, r, &state, &code, &nonce).await?["data"]["mayExchange"],
        false
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_same_flow_concurrent_claim_has_one_exchange_permission() -> TestResult {
    let c = connection().await?;
    let i = identity(&c).await?;
    let t = terms();
    let r = Uuid::new_v4();
    let state = hash();
    let code = hash();
    c.batch_execute("SET ROLE service_role").await?;
    start(&c, i, r, &state, &t).await?;
    let a = connection().await?;
    let b = connection().await?;
    a.batch_execute("SET ROLE service_role").await?;
    b.batch_execute("SET ROLE service_role").await?;
    let na = hash();
    let nb = hash();
    let (one, two) = tokio::join!(
        claim(&a, i, r, &state, &code, &na),
        claim(&b, i, r, &state, &code, &nb)
    );
    let one = one?;
    let two = two?;
    assert_ne!(one["data"]["mayExchange"], two["data"]["mayExchange"]);
    assert_eq!(one["status"], "success");
    assert_eq!(two["status"], "success");
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='patreon_exchange_claim'",&[&i.actor]).await?.get::<_,i64>(0),1);
    Ok(())
}
async fn second_session(
    c: &Client,
    i: Identity,
) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
    c.batch_execute("BEGIN").await?;
    let session = Uuid::new_v4();
    c.execute("INSERT INTO auth.sessions(id,user_id,refresh_token_hmac_key,refresh_token_counter) VALUES($1,$2,'fixture-only-key',0)",&[&session,&i.subject]).await?;
    c.execute(
        "INSERT INTO auth.mfa_amr_claims(session_id,authentication_method) VALUES($1,'otp')",
        &[&session],
    )
    .await?;
    let grant:Uuid=c.query_one("INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref) SELECT principal_key,'auth_session_v1',app.auth_uuid_hash($2),id::text,app.auth_json_hash(jsonb_build_object('version',1,'scope','auth_session_v1','subjectHash',app.auth_uuid_hash($1),'sessionHash',app.auth_uuid_hash($2),'bindingId',id::text)),'pending','' FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_binding_v1' AND state='accepted' RETURNING id",&[&i.subject,&session]).await?.get(0);
    c.query_one("SELECT app.accept_auth_receipt($1,jsonb_build_object('version',1,'method','email_code','initialClass','otp','bindingId',(SELECT target_ref FROM app.command_receipt WHERE id=$1),'proofHash',repeat('b',64)))",&[&grant]).await?;
    c.batch_execute("COMMIT").await?;
    Ok(session)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_nonce_context_is_bound_to_the_original_live_session_and_unqualified_rows_are_not_adopted()
-> TestResult {
    let c = connection().await?;
    let i = identity(&c).await?;
    let other = Identity {
        session: second_session(&c, i).await?,
        ..i
    };
    let t = terms();
    let r = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    let p = proof(&t);
    c.batch_execute("SET ROLE service_role").await?;
    start(&c, i, r, &state, &t).await?;
    assert_eq!(
        view(&c, other, r, &state, None).await?["data"]["request"],
        "conflict"
    );
    assert_eq!(
        claim(&c, other, r, &state, &code, &nonce).await?["data"]["request"],
        "conflict"
    );
    claim(&c, i, r, &state, &code, &nonce).await?;
    c.batch_execute("RESET ROLE").await?;
    let event = Uuid::new_v4();
    let member = Uuid::new_v4();
    c.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state) VALUES($1,'patreon',$1::uuid::text,$2,clock_timestamp(),'test-only:unqualified','test-only:unqualified','verified')",&[&event,&i.actor]).await?;
    c.execute("INSERT INTO app.patreon_membership(id,account_id,patreon_member_ref,active,source_event_id) VALUES($1,$2,$1::uuid::text,false,$3)",&[&member,&i.actor,&event]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        finish(&c, i, r, &state, &code, &nonce, &p).await?["data"]["membership"],
        "unavailable"
    );
    c.batch_execute("RESET ROLE").await?;
    assert!(
        c.query_one(
            "SELECT patreon_user_ref FROM app.patreon_membership WHERE id=$1",
            &[&member]
        )
        .await?
        .get::<_, Option<String>>(0)
        .is_none()
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='patreon_complete'",&[&i.actor]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_competing_account_completions_wait_on_one_stable_user_and_only_one_binds()
-> TestResult {
    let c = connection().await?;
    let a = identity(&c).await?;
    let b = identity(&c).await?;
    let t = terms();
    let pa = proof(&t);
    let ra = Uuid::new_v4();
    let rb = Uuid::new_v4();
    let state = hash();
    let ca = hash();
    let cb = hash();
    let nonce = hash();
    c.batch_execute("SET ROLE service_role").await?;
    start(&c, a, ra, &state, &t).await?;
    start(&c, b, rb, &state, &t).await?;
    claim(&c, a, ra, &state, &ca, &nonce).await?;
    claim(&c, b, rb, &state, &cb, &nonce).await?;
    c.batch_execute("RESET ROLE").await?;
    let first = connection().await?;
    let second = connection().await?;
    let pid: i32 = second
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    first.batch_execute("SET ROLE service_role; BEGIN").await?;
    assert_eq!(
        finish(&first, a, ra, &state, &ca, &nonce, &pa).await?["status"],
        "success"
    );
    second.batch_execute("SET ROLE service_role").await?;
    let s = state.clone();
    let p = pa.clone();
    let n = nonce.clone();
    let task = tokio::spawn(async move { finish(&second, b, rb, &s, &cb, &n, &p).await });
    wait_blocked(&c, pid).await?;
    first.batch_execute("COMMIT").await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), task).await???["data"]["membership"],
        "unavailable"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.patreon_membership WHERE patreon_user_ref=$1",
            &[&pa["userId"].as_str().unwrap()]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.provider_event WHERE provider='patreon' AND account_id=$1",
            &[&b.actor]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres; observes a real expiry during a tier FK wait"]
async fn patreon_completion_expiring_during_a_tier_wait_rolls_back_every_new_effect() -> TestResult
{
    let locker = connection().await?;
    let i = identity(&locker).await?;
    let t = terms();
    let r = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    let mut p = proof(&t);
    let tier = Uuid::new_v4();
    let external = format!("tier-{tier}");
    locker.execute("INSERT INTO app.supporter_tier(id,patreon_tier_ref,name,rank) VALUES($1,$2,'Expiry fixture',1)",&[&tier,&external]).await?;
    p["entitledTierIds"] = json!([external]);
    timed_start(&locker, i, r, &state, &t, 6).await?;
    locker.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        claim(&locker, i, r, &state, &code, &nonce).await?["data"]["mayExchange"],
        true
    );
    locker.batch_execute("RESET ROLE; BEGIN").await?;
    locker
        .query_one(
            "SELECT id FROM app.supporter_tier WHERE id=$1 FOR UPDATE",
            &[&tier],
        )
        .await?;
    let writer = connection().await?;
    let pid: i32 = writer
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    writer.batch_execute("SET ROLE service_role").await?;
    let task = tokio::spawn(async move { finish(&writer, i, r, &state, &code, &nonce, &p).await });
    wait_blocked(&locker, pid).await?;
    tokio::time::timeout(std::time::Duration::from_secs(8),async{
  loop{if locker.query_one("SELECT clock_timestamp()>=(result_ref::jsonb->>'expiresAt')::timestamptz FROM app.command_receipt WHERE account_id=$1 AND scope='patreon_start' AND request_ref=$2",&[&i.actor,&r.to_string()]).await.unwrap().get::<_,bool>(0){break;}tokio::time::sleep(std::time::Duration::from_millis(20)).await;}
 }).await?;
    locker.batch_execute("COMMIT").await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(3), task).await???["data"]["request"],
        "expired"
    );
    for sql in [
        "SELECT count(*) FROM app.patreon_membership WHERE account_id=$1",
        "SELECT count(*) FROM app.provider_event WHERE account_id=$1 AND provider='patreon'",
        "SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='patreon_complete'",
    ] {
        assert_eq!(
            locker.query_one(sql, &[&i.actor]).await?.get::<_, i64>(0),
            0
        );
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn patreon_qualified_identity_is_immutable_and_missing_completion_lineage_cannot_commit()
-> TestResult {
    let c = connection().await?;
    let i = identity(&c).await?;
    let other = identity(&c).await?;
    let t = terms();
    let r = Uuid::new_v4();
    let state = hash();
    let code = hash();
    let nonce = hash();
    let p = proof(&t);
    c.batch_execute("SET ROLE service_role").await?;
    start(&c, i, r, &state, &t).await?;
    claim(&c, i, r, &state, &code, &nonce).await?;
    assert_eq!(
        finish(&c, i, r, &state, &code, &nonce, &p).await?["status"],
        "success"
    );
    c.batch_execute("RESET ROLE").await?;
    for sql in [
        "UPDATE app.patreon_membership SET patreon_user_ref='other' WHERE account_id=$1",
        "UPDATE app.patreon_membership SET patreon_campaign_ref='other' WHERE account_id=$1",
        "UPDATE app.patreon_membership SET patreon_user_ref=NULL,patreon_campaign_ref=NULL WHERE account_id=$1",
    ] {
        assert!(c.execute(sql, &[&i.actor]).await.is_err());
    }
    let source = Uuid::new_v4();
    let association = Uuid::new_v4();
    let request = Uuid::new_v4();
    let forged = proof(&t);
    c.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state,effect,effect_key) VALUES($1,'patreon','oauth:'||$2::uuid::text||':'||$3::uuid::text,$2,clock_timestamp(),'oauth.association','sha256:'||app.auth_json_hash($4),'processed',$4,'none',jsonb_build_object('version',1,'kind','patreon_link_v1','startId',$3::uuid,'associationId',$5::uuid,'generation','999','proofHash',app.auth_json_hash($4)),'test-only:no-completion')",&[&source,&other.actor,&request,&forged,&association]).await?;
    let error=c.execute("INSERT INTO app.patreon_membership(id,account_id,patreon_member_ref,patreon_user_ref,patreon_campaign_ref,active,source_event_id) VALUES($1,$2,$3,$4,$5,false,$6)",&[&association,&other.actor,&forged["memberId"].as_str().unwrap(),&forged["userId"].as_str().unwrap(),&forged["campaignId"].as_str().unwrap(),&source]).await.unwrap_err();
    assert_eq!(
        error.code(),
        Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        error.as_db_error().unwrap().message(),
        "association requires completed source transaction"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.patreon_membership WHERE id=$1",
            &[&association]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}
