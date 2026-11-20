//! Cross-domain database invariants; owner fixtures do not certify provider or native facts.
use super::{TestResult, account, connection};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn pending_purchase(c: &Client) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
    let (owner, _) = account(c).await?;
    let cosmetic = Uuid::new_v4();
    c.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,'dice','fixture:deadline','active',false)", &[&cosmetic]).await?;
    let id = Uuid::new_v4();
    c.execute("INSERT INTO app.purchase(id,account_id,cosmetic_id,provider_request_ref,requested_at,state) VALUES($1,$2,$3,$1::uuid::text,clock_timestamp(),'creating')", &[&id,&owner,&cosmetic]).await?;
    Ok(id)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn purchase_deadline_rejects_every_partial_tuple_and_nonfinite_or_wrong_interval()
-> TestResult {
    let c = connection().await?;
    let purchase = pending_purchase(&c).await?;
    let change = "UPDATE app.purchase SET first_create_attempt_at=$2::text::timestamptz,create_deadline_at=$3::text::timestamptz WHERE id=$1";
    for (first, end) in [
        (Some("2026-11-03T00:00:00Z"), None),
        (None, Some("2026-11-03T23:00:00Z")),
        (Some("infinity"), Some("infinity")),
        (Some("-infinity"), Some("-infinity")),
        (Some("2026-11-03T00:00:00Z"), Some("2026-11-04T00:00:00Z")),
    ] {
        let error = c
            .execute(change, &[&purchase, &first, &end])
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
        let constraint = error.as_db_error().and_then(|e| e.constraint());
        if first.is_some_and(|value| value.contains("infinity")) {
            assert!(matches!(
                constraint,
                Some(
                    "finite_create_deadline_at"
                        | "finite_first_create_attempt_at"
                        | "purchase_creation_deadline_tuple"
                )
            ));
        } else {
            assert_eq!(constraint, Some("purchase_creation_deadline_tuple"));
        }
        assert!(c.query_one("SELECT first_create_attempt_at IS NULL AND create_deadline_at IS NULL FROM app.purchase WHERE id=$1", &[&purchase]).await?.get::<_, bool>(0));
    }
    c.execute(
        change,
        &[
            &purchase,
            &Some("2026-11-03T00:00:00Z"),
            &Some("2026-11-03T23:00:00Z"),
        ],
    )
    .await?;
    assert!(c.query_one("SELECT create_deadline_at=first_create_attempt_at+interval '23 hours' FROM app.purchase WHERE id=$1", &[&purchase]).await?.get::<_, bool>(0));
    // Established creation identity still uses the existing immutable purchase proof guard.
    assert_eq!(
        c.execute(change, &[&purchase, &None::<&str>, &None::<&str>])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn account_creation_time_cannot_be_rewritten_to_bypass_age_restrictions_or_start_at_infinity()
-> TestResult {
    let c = connection().await?;
    let (actor, _) = account(&c).await?;
    let error = c
        .execute(
            "UPDATE app.account SET created_at=clock_timestamp()-interval '31 days' WHERE id=$1",
            &[&actor],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    let subject = Uuid::new_v4();
    c.execute("INSERT INTO auth.users(id) VALUES($1)", &[&subject])
        .await?;
    super::seed_auth_authority(&c, subject).await?;
    let username = format!("clock_{}", &subject.simple().to_string()[..12]);
    let error = c.execute("INSERT INTO app.account(auth_subject,username,sign_in_method,created_at,state) VALUES($1,$2,'email_code','infinity','active')", &[&subject,&username]).await.unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        error.as_db_error().and_then(|e| e.constraint()),
        Some("finite_created_at")
    );
    Ok(())
}
