//! Trusted normalized financial proof fixtures, never a claim of a qualified Patreon financial feed.
use super::{TestResult, connection, decks, wait_for_database_lock};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) fn proof(
    member: Uuid,
    tier: Uuid,
    payment: &str,
    state: &str,
    occurrence: &str,
) -> Value {
    let hash: String = Sha256::digest(occurrence.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"family":"patreon-financial-v1","occurrenceRef":occurrence,"memberRef":member.to_string(),"paymentRef":payment,"tierRef":tier.to_string(),"initiatedAt":"2026-10-01T00:00:00.000000Z","paidAt":"2026-10-01T00:00:00.000000Z","occurredAt":if state=="paid"{"2026-10-01T00:00:00.000000Z"}else{"2026-10-02T00:00:00.000000Z"},"state":state,"amountMinor":"500","currency":"usd","bodyHash":hash,"evidenceRef":format!("fixture:financial:{occurrence}")})
}
pub(super) async fn commit(
    c: &Client,
    proof: &Value,
    delivery: &str,
) -> Result<Value, tokio_postgres::Error> {
    let value: Value = c
        .query_one(
            "SELECT app.commit_patreon_financial($1,$2)",
            &[&delivery, &proof],
        )
        .await?
        .get(0);
    let original = spellwake_application::patreon_source_ports::FinancialObservation::from_bytes(
        delivery,
        &serde_json::to_vec(proof).unwrap(),
    )
    .unwrap();
    assert_eq!(original.original_terms(), *proof);
    let decoded = original
        .decode_response(&serde_json::to_vec(&value).unwrap())
        .unwrap();
    let payment = c
        .query_opt(
            "SELECT id FROM app.patreon_payment WHERE payment_ref=$1",
            &[&proof["paymentRef"].as_str().unwrap()],
        )
        .await?
        .map(|row| row.get(0));
    assert!(decoded.matches_payment(payment));
    Ok(value)
}
struct Fixture {
    actor: Uuid,
    member: Uuid,
    tier: Uuid,
    cosmetic: Uuid,
    membership_source: Uuid,
}
async fn fixture(c: &Client) -> FixtureResult<Fixture> {
    let (actor, _, _, _, _) = decks::library(c).await?;
    let member = Uuid::new_v4();
    let tier = Uuid::new_v4();
    let cosmetic = Uuid::new_v4();
    let source = Uuid::new_v4();
    c.execute("INSERT INTO app.supporter_tier(id,patreon_tier_ref,name,rank) VALUES($1,$1::uuid::text,'Fixture',1)", &[&tier]).await?;
    c.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state) VALUES($1,'dice','fixture:financial','active')", &[&cosmetic]).await?;
    c.execute(
        "INSERT INTO app.tier_cosmetic(tier_id,cosmetic_id) VALUES($1,$2)",
        &[&tier, &cosmetic],
    )
    .await?;
    c.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state) VALUES($1,'patreon',$1::uuid::text,$2,clock_timestamp(),'fixture:legacy-association','fixture','verified')", &[&source,&actor]).await?;
    c.execute("INSERT INTO app.patreon_membership(id,account_id,patreon_member_ref,active_tier_id,active,source_event_id) VALUES($1,$2,$1::uuid::text,$3,true,$4)", &[&member,&actor,&tier,&source]).await?;
    Ok(Fixture {
        actor,
        member,
        tier,
        cosmetic,
        membership_source: source,
    })
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn financial_payment_and_delivery_retries_grant_once_while_reversal_is_terminal() -> TestResult
{
    let c = connection().await?;
    let f = fixture(&c).await?;
    let payment = Uuid::new_v4().to_string();
    let occurrence = format!("paid:{payment}");
    let body = proof(f.member, f.tier, &payment, "paid", &occurrence);
    let original_delivery = Uuid::new_v4().to_string();
    let first = commit(&c, &body, &original_delivery).await?;
    assert_eq!(first["disposition"], "applied");
    let id: Uuid = first["paymentId"].as_str().unwrap().parse()?;
    assert_eq!(commit(&c, &body, &Uuid::new_v4().to_string()).await?, first);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.cosmetic_grant WHERE patreon_payment_id=$1 AND state='active'", &[&id]).await?.get::<_,i64>(0),1);
    c.execute(
        "UPDATE app.patreon_membership SET active=false,active_tier_id=NULL WHERE id=$1",
        &[&f.member],
    )
    .await?;
    assert!(
        c.query_one(
            "SELECT app.has_cosmetic($1,$2,'dice')",
            &[&f.actor, &f.cosmetic]
        )
        .await?
        .get::<_, bool>(0)
    );
    let reversed = proof(
        f.member,
        f.tier,
        &payment,
        "reversed",
        &format!("reverse:{payment}"),
    );
    assert_eq!(
        commit(&c, &reversed, &Uuid::new_v4().to_string()).await?["state"],
        "reversed"
    );
    let delayed = proof(
        f.member,
        f.tier,
        &payment,
        "paid",
        &format!("late-paid:{payment}"),
    );
    assert_eq!(
        commit(&c, &delayed, &Uuid::new_v4().to_string()).await?["disposition"],
        "noop"
    );
    assert!(
        !c.query_one(
            "SELECT app.has_cosmetic($1,$2,'dice')",
            &[&f.actor, &f.cosmetic]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(commit(&c, &body, &original_delivery).await?, first);
    let mut another_evidence = body.clone();
    another_evidence["bodyHash"] = json!("b".repeat(64));
    another_evidence["evidenceRef"] = json!("fixture:another-verified-capture");
    assert_eq!(
        commit(&c, &another_evidence, &Uuid::new_v4().to_string()).await?,
        first
    );
    assert_eq!(
        commit(&c, &another_evidence, &original_delivery)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn membership_only_wrong_tier_and_retargeted_financial_proofs_cannot_grant_benefits()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let other = fixture(&c).await?;
    let payment = Uuid::new_v4().to_string();
    let occurrence = format!("paid:{payment}");
    let original = proof(f.member, f.tier, &payment, "paid", &occurrence);
    assert_eq!(c.execute("INSERT INTO app.patreon_payment(membership_id,tier_id,provider_event_id,payment_ref,paid_at,state) VALUES($1,$2,$3,$4,clock_timestamp(),'paid')", &[&f.member,&f.tier,&f.membership_source,&payment]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    let delivery = Uuid::new_v4().to_string();
    commit(&c, &original, &delivery).await?;
    let mut changed = original.clone();
    changed["tierRef"] = json!(other.tier.to_string());
    assert_eq!(
        commit(&c, &changed, &delivery).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    changed = proof(other.member, other.tier, &payment, "paid", &occurrence);
    assert_eq!(
        commit(&c, &changed, &Uuid::new_v4().to_string())
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let mut current_only = original;
    current_only["family"] = json!("patreon-current-v1");
    assert_eq!(
        commit(&c, &current_only, &Uuid::new_v4().to_string())
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competing_financial_deliveries_wait_and_share_one_payment_and_grant() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let payment = Uuid::new_v4().to_string();
    let body = proof(
        f.member,
        f.tier,
        &payment,
        "paid",
        &format!("paid:{payment}"),
    );
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    let first = commit(&c, &body, &Uuid::new_v4().to_string()).await?;
    let task =
        tokio::spawn(async move { commit(&waiting, &body, &Uuid::new_v4().to_string()).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??, first);
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.patreon_payment WHERE membership_id=$1",
            &[&f.member]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_deletion_reconciles_inflight_payment_but_never_starts_new_benefits() -> TestResult
{
    let c = connection().await?;
    let f = fixture(&c).await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.actor]).await?;
    let before = proof(
        f.member,
        f.tier,
        &Uuid::new_v4().to_string(),
        "paid",
        &Uuid::new_v4().to_string(),
    );
    assert_eq!(
        commit(&c, &before, &Uuid::new_v4().to_string()).await?["disposition"],
        "applied"
    );
    let stamp: String = c.query_one("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')", &[]).await?.get(0);
    let mut after = proof(
        f.member,
        f.tier,
        &Uuid::new_v4().to_string(),
        "paid",
        &Uuid::new_v4().to_string(),
    );
    for key in ["initiatedAt", "paidAt", "occurredAt"] {
        after[key] = json!(stamp);
    }
    let rejected = commit(&c, &after, &Uuid::new_v4().to_string()).await?;
    assert_eq!(rejected["disposition"], "ineligible_after_deletion");
    assert!(rejected["paymentId"].is_null());
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.patreon_payment WHERE membership_id=$1",
            &[&f.member]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn financial_identity_and_time_validation_leave_no_partial_sources_and_rollback_is_retryable()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let payment = Uuid::new_v4().to_string();
    let body = proof(
        f.member,
        f.tier,
        &payment,
        "paid",
        &Uuid::new_v4().to_string(),
    );
    let delivery = Uuid::new_v4().to_string();
    c.batch_execute("BEGIN").await?;
    let rolled_back = commit(&c, &body, &delivery).await?;
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.patreon_payment WHERE payment_ref=$1",
            &[&payment]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.provider_event WHERE external_event_ref=$1",
            &[&format!("financial:{delivery}")]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    let accepted = commit(&c, &body, &delivery).await?;
    assert_eq!(accepted["disposition"], rolled_back["disposition"]);
    for (key, value) in [
        ("amountMinor", "501"),
        ("currency", "eur"),
        ("paidAt", "2026-10-01T00:00:01.000000Z"),
        ("initiatedAt", "2026-09-30T00:00:00.000000Z"),
    ] {
        let mut changed = body.clone();
        changed[key] = json!(value);
        assert!(
            commit(&c, &changed, &Uuid::new_v4().to_string())
                .await
                .is_err()
        );
    }
    for invalid in [
        "infinity",
        "2026-10-01T00:00:00Z",
        "2026-02-30T00:00:00.000000Z",
        "2026-10-01T01:00:00.000000+01:00",
    ] {
        let mut changed = proof(
            f.member,
            f.tier,
            &Uuid::new_v4().to_string(),
            "paid",
            &Uuid::new_v4().to_string(),
        );
        changed["paidAt"] = json!(invalid);
        assert_eq!(
            commit(&c, &changed, &Uuid::new_v4().to_string())
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INVALID_PARAMETER_VALUE)
        );
    }
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.patreon_payment WHERE membership_id=$1",
            &[&f.member]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn reversal_waits_for_inflight_payment_and_revokes_its_grant_once() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let payment = Uuid::new_v4().to_string();
    let paid = proof(
        f.member,
        f.tier,
        &payment,
        "paid",
        &Uuid::new_v4().to_string(),
    );
    let reversed = proof(
        f.member,
        f.tier,
        &payment,
        "reversed",
        &Uuid::new_v4().to_string(),
    );
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    commit(&c, &paid, &Uuid::new_v4().to_string()).await?;
    let task =
        tokio::spawn(async move { commit(&waiting, &reversed, &Uuid::new_v4().to_string()).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??["state"], "reversed");
    assert!(
        !c.query_one(
            "SELECT app.has_cosmetic($1,$2,'dice')",
            &[&f.actor, &f.cosmetic]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.cosmetic_grant WHERE account_id=$1 AND state='revoked'", &[&f.actor]).await?.get::<_,i64>(0),1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stale_snapshot_cannot_relabel_a_tier_after_its_first_payment() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let stale = connection().await?;
    stale
        .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    stale
        .query_one("SELECT id FROM app.supporter_tier WHERE id=$1", &[&f.tier])
        .await?;
    let body = proof(
        f.member,
        f.tier,
        &Uuid::new_v4().to_string(),
        "paid",
        &Uuid::new_v4().to_string(),
    );
    commit(&c, &body, &Uuid::new_v4().to_string()).await?;
    assert_eq!(
        stale
            .execute(
                "UPDATE app.supporter_tier SET patreon_tier_ref=$2 WHERE id=$1",
                &[&f.tier, &Uuid::new_v4().to_string()]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    stale.batch_execute("ROLLBACK").await?;
    assert_eq!(
        c.execute(
            "UPDATE app.supporter_tier SET patreon_tier_ref=$2 WHERE id=$1",
            &[&f.tier, &Uuid::new_v4().to_string()]
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
async fn financial_original_proof_byte_budget_matches_postgres_and_exact_source_boundary()
-> TestResult {
    use spellwake_application::patreon_source_ports::{
        FinancialObservation, MAX_FINANCIAL_PROOF_BYTES,
    };
    let c = connection().await?;
    let f = fixture(&c).await?;
    let mut body = proof(
        f.member,
        f.tier,
        &Uuid::new_v4().to_string(),
        "paid",
        &Uuid::new_v4().to_string(),
    );
    body["evidenceRef"] = json!("fixture:🂡\n\"\\");
    let delivery = Uuid::new_v4().to_string();
    let original = FinancialObservation::from_bytes(&delivery, &serde_json::to_vec(&body)?)?;
    let expected: i64 = c
        .query_one("SELECT octet_length($1::jsonb::text)::bigint", &[&body])
        .await?
        .get(0);
    let initial = original.original_terms_octets().unwrap();
    assert_eq!(initial, usize::try_from(expected)?);
    body["evidenceRef"] = json!(format!(
        "{}{}",
        " ".repeat(MAX_FINANCIAL_PROOF_BYTES - initial),
        body["evidenceRef"].as_str().unwrap()
    ));
    let original = FinancialObservation::from_bytes(&delivery, &serde_json::to_vec(&body)?)?;
    assert_eq!(
        original.original_terms_octets(),
        Some(MAX_FINANCIAL_PROOF_BYTES)
    );
    assert_eq!(
        commit(&c, &body, &delivery).await?["disposition"],
        "applied"
    );
    body["evidenceRef"] = json!(format!(" {}", body["evidenceRef"].as_str().unwrap()));
    assert!(FinancialObservation::from_bytes(&delivery, &serde_json::to_vec(&body)?).is_err());
    assert_eq!(
        commit(&c, &body, &Uuid::new_v4().to_string())
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn financial_occurrence_cannot_be_accepted_from_a_future_source_clock() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let mut body = proof(
        f.member,
        f.tier,
        &Uuid::new_v4().to_string(),
        "paid",
        &Uuid::new_v4().to_string(),
    );
    let future:String=c.query_one("SELECT to_char((clock_timestamp()+interval '1 day') AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",&[]).await?.get(0);
    for key in ["initiatedAt", "paidAt", "occurredAt"] {
        body[key] = json!(future);
    }
    // The grammar is valid. Only the authoritative database clock can reject future truth.
    assert!(
        spellwake_application::patreon_source_ports::FinancialObservation::from_bytes(
            "future-source",
            &serde_json::to_vec(&body)?
        )
        .is_ok()
    );
    assert_eq!(
        commit(&c, &body, &Uuid::new_v4().to_string())
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn financial_original_enum_objects_cannot_be_normalized_into_valid_proofs() -> TestResult {
    let c = connection().await?;
    for (key, tag) in [("family", "patreon-financial-v1"), ("state", "paid")] {
        let mut body = proof(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "fixture:payment",
            "paid",
            "fixture:occurrence",
        );
        body[key] = json!({tag:null});
        let delivery = Uuid::new_v4().to_string();
        assert!(
            spellwake_application::patreon_source_ports::FinancialObservation::from_bytes(
                &delivery,
                &serde_json::to_vec(&body)?
            )
            .is_err()
        );
        assert_eq!(
            commit(&c, &body, &delivery).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        assert_eq!(c.query_one("SELECT count(*) FROM app.provider_event WHERE provider='patreon' AND external_event_ref='financial:'||$1::text",&[&delivery]).await?.get::<_,i64>(0),0);
    }
    Ok(())
}
