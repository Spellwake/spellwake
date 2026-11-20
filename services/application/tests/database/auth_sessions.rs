//! Managed-shape fixtures exercise the private predicate, not user admission.
use super::{TestResult, connection};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn seed(
    client: &Client,
    class: &str,
    counter: bool,
) -> Result<(Uuid, Uuid), tokio_postgres::Error> {
    let subject = Uuid::new_v4();
    let session = Uuid::new_v4();
    client
        .execute("INSERT INTO auth.users(id) VALUES($1)", &[&subject])
        .await?;
    client.execute("INSERT INTO auth.sessions(id,user_id,refresh_token_hmac_key,refresh_token_counter) VALUES($1,$2,CASE WHEN $3 THEN 'fixture-only-key' ELSE NULL END,CASE WHEN $3 THEN 0 ELSE NULL END)", &[&session,&subject,&counter]).await?;
    client
        .execute(
            "INSERT INTO auth.mfa_amr_claims(session_id,authentication_method) VALUES($1,$2)",
            &[&session, &class],
        )
        .await?;
    if !counter {
        client
            .execute(
                "INSERT INTO auth.refresh_tokens(user_id,session_id,revoked) VALUES($1,$2,false)",
                &[&subject.to_string(), &session],
            )
            .await?;
    }
    Ok((subject, session))
}

async fn live(
    client: &Client,
    subject: Uuid,
    session: Uuid,
    class: &str,
) -> Result<bool, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.auth_session_is_live($1,$2,$3)",
            &[&subject, &session, &class],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn legacy_session_rotation_preserves_live_child_but_family_revocation_denies() -> TestResult {
    let client = connection().await?;
    let (subject, session) = seed(&client, "otp", false).await?;
    assert!(live(&client, subject, session, "otp").await?);
    client
        .execute(
            "UPDATE auth.refresh_tokens SET revoked=true WHERE session_id=$1",
            &[&session],
        )
        .await?;
    assert!(!live(&client, subject, session, "otp").await?);
    client
        .execute(
            "INSERT INTO auth.refresh_tokens(user_id,session_id,revoked) VALUES($1,$2,false)",
            &[&Uuid::new_v4().to_string(), &session],
        )
        .await?;
    assert!(!live(&client, subject, session, "otp").await?);
    client
        .execute(
            "INSERT INTO auth.refresh_tokens(user_id,session_id,revoked) VALUES($1,$2,false)",
            &[&subject.to_string(), &session],
        )
        .await?;
    assert!(live(&client, subject, session, "otp").await?);
    client
        .execute(
            "UPDATE auth.refresh_tokens SET revoked=true WHERE session_id=$1",
            &[&session],
        )
        .await?;
    assert!(!live(&client, subject, session, "otp").await?);
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM auth.sessions WHERE id=$1",
                &[&session]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn counter_session_requires_well_formed_metadata_and_matching_original_amr() -> TestResult {
    let client = connection().await?;
    let (subject, session) = seed(&client, "oauth", true).await?;
    assert!(live(&client, subject, session, "oauth").await?);
    for class in ["otp", "password", "token_refresh", "", "mfa/totp"] {
        assert!(!live(&client, subject, session, class).await?);
    }
    assert!(!live(&client, Uuid::new_v4(), session, "oauth").await?);
    assert!(!live(&client, subject, Uuid::new_v4(), "oauth").await?);
    for (key, counter) in [
        (None, Some(0i64)),
        (Some("fixture-only-key"), None),
        (Some(""), Some(0)),
        (Some("fixture-only-key"), Some(-1)),
    ] {
        client.execute("UPDATE auth.sessions SET refresh_token_hmac_key=$2,refresh_token_counter=$3 WHERE id=$1", &[&session,&key,&counter]).await?;
        assert!(!live(&client, subject, session, "oauth").await?);
    }
    client.execute("UPDATE auth.sessions SET refresh_token_hmac_key='fixture-only-key',refresh_token_counter=8 WHERE id=$1", &[&session]).await?;
    assert!(live(&client, subject, session, "oauth").await?);
    client.execute("INSERT INTO auth.mfa_amr_claims(session_id,authentication_method) VALUES($1,'password')", &[&session]).await?;
    assert!(!live(&client, subject, session, "oauth").await?);
    client.execute("DELETE FROM auth.mfa_amr_claims WHERE session_id=$1 AND authentication_method='password'", &[&session]).await?;
    assert!(live(&client, subject, session, "oauth").await?);
    client
        .execute(
            "DELETE FROM auth.mfa_amr_claims WHERE session_id=$1",
            &[&session],
        )
        .await?;
    assert!(!live(&client, subject, session, "oauth").await?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn live_session_checks_user_expiry_deletion_and_private_permissions() -> TestResult {
    let client = connection().await?;
    let (subject, session) = seed(&client, "otp", false).await?;
    // A preexisting soft-deleted managed row remains denied by the liveness
    // predicate; the new cleanup guard forbids producing one via UPDATE.
    let deleted_subject = Uuid::new_v4();
    client
        .execute(
            "INSERT INTO auth.users(id,deleted_at) VALUES($1,statement_timestamp())",
            &[&deleted_subject],
        )
        .await?;
    let deleted_session = super::seed_auth_authority(&client, deleted_subject).await?;
    assert!(!live(&client, deleted_subject, deleted_session, "otp").await?);
    for column in ["banned_until"] {
        client.execute(&format!("UPDATE auth.users SET {column}=statement_timestamp()+interval '1 day' WHERE id=$1"), &[&subject]).await?;
        assert!(!live(&client, subject, session, "otp").await?);
        client
            .execute(
                &format!("UPDATE auth.users SET {column}=NULL WHERE id=$1"),
                &[&subject],
            )
            .await?;
        assert!(live(&client, subject, session, "otp").await?);
    }
    client.execute("UPDATE auth.users SET banned_until=statement_timestamp()-interval '1 second' WHERE id=$1", &[&subject]).await?;
    assert!(live(&client, subject, session, "otp").await?);
    client
        .execute(
            "UPDATE auth.sessions SET not_after=statement_timestamp() WHERE id=$1",
            &[&session],
        )
        .await?;
    assert!(!live(&client, subject, session, "otp").await?);
    client
        .execute(
            "UPDATE auth.sessions SET not_after=statement_timestamp()+interval '1 day' WHERE id=$1",
            &[&session],
        )
        .await?;
    assert!(live(&client, subject, session, "otp").await?);
    for role in ["anon", "authenticated", "service_role"] {
        let caller = connection().await?;
        caller.batch_execute(&format!("SET ROLE {role}")).await?;
        assert_eq!(
            live(&caller, subject, session, "otp")
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
    }
    client
        .execute("DELETE FROM auth.sessions WHERE id=$1", &[&session])
        .await?;
    assert!(!live(&client, subject, session, "otp").await?);
    assert!(
        !client
            .query_one("SELECT app.auth_session_is_live(NULL,NULL,NULL)", &[])
            .await?
            .get::<_, bool>(0)
    );
    Ok(())
}
