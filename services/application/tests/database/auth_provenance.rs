//! Privileged SQL source fixtures. SET SESSION AUTHORIZATION is not GoTrue login proof.
use super::{TestResult, connection};
use serde_json::{Value, json};
use tokio_postgres::{Client, GenericClient, Transaction, error::SqlState};
use uuid::Uuid;

async fn subject(client: &Client, providers: &[&str]) -> Result<Uuid, tokio_postgres::Error> {
    let subject = Uuid::new_v4();
    client.execute("INSERT INTO auth.users(id,email,email_confirmed_at) VALUES($1,$2,statement_timestamp())", &[&subject,&format!("{subject}@example.invalid")]).await?;
    for provider in providers {
        client.execute("INSERT INTO auth.identities(user_id,provider,provider_id,identity_data) VALUES($1,$2,$1::uuid::text,'{\"email_verified\":true}'::jsonb)", &[&subject,&provider]).await?;
    }
    Ok(subject)
}

pub(super) async fn seed_session<C: GenericClient + Sync>(
    client: &C,
    subject: Uuid,
    class: &str,
) -> Result<Uuid, tokio_postgres::Error> {
    let session = Uuid::new_v4();
    client.execute("INSERT INTO auth.sessions(id,user_id,refresh_token_hmac_key,refresh_token_counter) VALUES($1,$2,'fixture-only-key',0)", &[&session,&subject]).await?;
    client
        .execute(
            "INSERT INTO auth.mfa_amr_claims(session_id,authentication_method) VALUES($1,$2)",
            &[&session, &class],
        )
        .await?;
    Ok(session)
}

pub(super) fn payload(
    subject: Uuid,
    action: &str,
    provider: Option<&str>,
    provider_type: Option<&str>,
) -> Value {
    let mut value = json!({"actor_id":subject.to_string(),"action":action,"log_type":match action {"user_signedup"=>"team","token_refreshed"|"token_revoked"=>"token",_=>"account"}});
    if let Some(provider) = provider {
        value["traits"]["provider"] = json!(provider);
    }
    if let Some(provider) = provider_type {
        value["traits"]["provider_type"] = json!(provider);
    }
    value
}

pub(super) async fn audit(
    tx: &Transaction<'_>,
    value: &Value,
) -> Result<(), tokio_postgres::Error> {
    tx.execute(
        "INSERT INTO auth.audit_log_entries(payload) VALUES($1::jsonb::json)",
        &[&value],
    )
    .await?;
    Ok(())
}

pub(super) async fn hook(
    tx: &Transaction<'_>,
    subject: Uuid,
    session: Uuid,
    class: &str,
) -> Result<Value, tokio_postgres::Error> {
    let event = json!({"user_id":subject.to_string(),"authentication_method":class,"claims":{"sub":subject.to_string(),"session_id":session.to_string(),"role":"authenticated","aud":"authenticated","spellwake_auth":{"method":"untrusted-input"},"user_metadata":{"provider":"untrusted-input"}}});
    Ok(tx
        .query_one("SELECT app.custom_access_token_hook($1)", &[&event])
        .await?
        .get(0))
}

pub(super) async fn issue(
    client: &mut Client,
    subject: Uuid,
    provider: &str,
    pkce: bool,
) -> Result<(Uuid, Value), Box<dyn std::error::Error + Send + Sync>> {
    let class = if provider == "email" { "otp" } else { "oauth" };
    let tx = client.transaction().await?;
    let session = seed_session(&tx, subject, class).await?;
    tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
        .await?;
    audit(
        &tx,
        &payload(
            subject,
            if pkce { "login" } else { "user_signedup" },
            if pkce { None } else { Some(provider) },
            if pkce { Some(provider) } else { None },
        ),
    )
    .await?;
    let value = hook(&tx, subject, session, class).await?;
    tx.commit().await?;
    Ok((session, value))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn hook_establishes_exact_family_and_refresh_reuses_immutable_session_grant() -> TestResult {
    let mut owner = connection().await?;
    let subject = subject(&owner, &["google", "discord", "email"]).await?;
    let (session, first) = issue(&mut owner, subject, "google", false).await?;
    let first = &first["claims"]["spellwake_auth"];
    assert_eq!(first["method"], "google");
    let tx = owner.transaction().await?;
    tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
        .await?;
    audit(&tx, &payload(subject, "token_refreshed", None, None)).await?;
    audit(&tx, &payload(subject, "token_revoked", None, None)).await?;
    let refresh = hook(&tx, subject, session, "token_refresh").await?;
    assert_eq!(&refresh["claims"]["spellwake_auth"], first);
    tx.batch_execute("SAVEPOINT consumed").await?;
    assert_eq!(
        hook(&tx, subject, session, "token_refresh")
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    tx.batch_execute("ROLLBACK TO SAVEPOINT consumed").await?;
    tx.commit().await?;
    let (_, second) = issue(&mut owner, subject, "google", true).await?;
    assert_eq!(
        second["claims"]["spellwake_auth"]["bindingId"],
        first["bindingId"]
    );
    assert_ne!(
        second["claims"]["spellwake_auth"]["sessionGrantId"],
        first["sessionGrantId"]
    );
    let tx = owner.transaction().await?;
    let denied_session = seed_session(&tx, subject, "oauth").await?;
    tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
        .await?;
    audit(&tx, &payload(subject, "login", Some("discord"), None)).await?;
    assert_eq!(
        hook(&tx, subject, denied_session, "oauth")
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    tx.rollback().await?;
    assert_eq!(
        owner
            .query_one(
                "SELECT count(*) FROM auth.sessions WHERE id=$1",
                &[&denied_session]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    assert_eq!(owner.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1)", &[&subject]).await?.get::<_,i64>(0),3);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn closed_method_matrix_requires_verified_identity_and_ignores_metadata() -> TestResult {
    let mut owner = connection().await?;
    for provider in ["google", "apple", "discord", "email"] {
        let subject = subject(&owner, &[provider]).await?;
        owner.execute("UPDATE auth.identities SET identity_data='{\"email_verified\":\"true\"}'::jsonb WHERE user_id=$1", &[&subject]).await?;
        let class = if provider == "email" { "otp" } else { "oauth" };
        let tx = owner.transaction().await?;
        let session = seed_session(&tx, subject, class).await?;
        tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
            .await?;
        audit(
            &tx,
            &payload(subject, "user_signedup", Some(provider), None),
        )
        .await?;
        assert_eq!(
            hook(&tx, subject, session, class).await.unwrap_err().code(),
            Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
        );
        tx.rollback().await?;
        owner.execute("UPDATE auth.identities SET identity_data='{\"email_verified\":true}'::jsonb WHERE user_id=$1", &[&subject]).await?;
        let (_, accepted) = issue(&mut owner, subject, provider, true).await?;
        assert_eq!(
            accepted["claims"]["spellwake_auth"]["method"],
            if provider == "email" {
                "email_code"
            } else {
                provider
            }
        );
    }
    let subject = subject(&owner, &["email"]).await?;
    owner
        .execute(
            "UPDATE auth.users SET phone='fixture-phone' WHERE id=$1",
            &[&subject],
        )
        .await?;
    let tx = owner.transaction().await?;
    let session = seed_session(&tx, subject, "otp").await?;
    tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
        .await?;
    audit(&tx, &payload(subject, "user_signedup", Some("email"), None)).await?;
    assert_eq!(
        hook(&tx, subject, session, "otp").await.unwrap_err().code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn conflicting_and_unsupported_sources_cannot_be_replaced_by_a_later_valid_event()
-> TestResult {
    let mut owner = connection().await?;
    let subject = subject(&owner, &["google", "discord", "email"]).await?;
    for sources in [
        vec![
            payload(subject, "login", Some("google"), None),
            payload(subject, "login", Some("discord"), None),
            payload(subject, "login", Some("google"), None),
        ],
        vec![
            payload(subject, "login", Some("google"), Some("google")),
            payload(subject, "login", Some("google"), None),
        ],
        vec![
            payload(subject, "login", Some("google"), None),
            payload(Uuid::new_v4(), "login", Some("google"), None),
        ],
        vec![
            payload(subject, "token_refreshed", None, None),
            payload(subject, "login", Some("google"), None),
        ],
    ] {
        let tx = owner.transaction().await?;
        let session = seed_session(&tx, subject, "oauth").await?;
        tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
            .await?;
        for source in sources {
            audit(&tx, &source).await?;
        }
        assert_eq!(
            hook(&tx, subject, session, "oauth")
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
        );
        tx.rollback().await?;
    }
    assert_eq!(owner.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1)", &[&subject]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn missing_or_stale_source_and_untrusted_role_cannot_mint_authority() -> TestResult {
    let mut owner = connection().await?;
    let subject = subject(&owner, &["email"]).await?;
    let session = seed_session(&owner, subject, "otp").await?;
    let tx = owner.transaction().await?;
    tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
        .await?;
    audit(&tx, &payload(subject, "user_signedup", Some("email"), None)).await?;
    let stale: String = tx
        .query_one("SELECT current_setting('spellwake.auth_provenance')", &[])
        .await?
        .get(0);
    tx.commit().await?;
    for previous in ["", &stale] {
        let tx = owner.transaction().await?;
        tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
            .await?;
        tx.query_one(
            "SELECT set_config('spellwake.auth_provenance',$1,true)",
            &[&previous],
        )
        .await?;
        assert_eq!(
            hook(&tx, subject, session, "otp").await.unwrap_err().code(),
            Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
        );
        tx.rollback().await?;
    }
    for role in ["anon", "authenticated", "service_role"] {
        let caller = connection().await?;
        caller.batch_execute(&format!("SET ROLE {role}")).await?;
        assert_eq!(
            caller
                .query_one("SELECT app.custom_access_token_hook('{}'::jsonb)", &[])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
    }
    owner.batch_execute("SET ROLE supabase_auth_admin").await?;
    assert_eq!(
        owner
            .query_one("SELECT app.custom_access_token_hook('{}'::jsonb)", &[])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    owner.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn refresh_never_backfills_a_missing_session_grant_and_retirement_denies() -> TestResult {
    let mut owner = connection().await?;
    let subject = subject(&owner, &["google"]).await?;
    let (first_session, _) = issue(&mut owner, subject, "google", false).await?;
    let ungranted = seed_session(&owner, subject, "oauth").await?;
    let tx = owner.transaction().await?;
    tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
        .await?;
    audit(&tx, &payload(subject, "token_refreshed", None, None)).await?;
    assert_eq!(
        hook(&tx, subject, ungranted, "token_refresh")
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    tx.rollback().await?;
    let tx = owner.transaction().await?;
    let retired: Uuid = tx.query_one("INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref) VALUES('auth:'||app.auth_uuid_hash($1),'auth_retirement_v1','primary','unregistered',app.auth_json_hash(jsonb_build_object('version',1,'scope','auth_retirement_v1','subjectHash',app.auth_uuid_hash($1),'target','unregistered')),'pending','') RETURNING id", &[&subject]).await?.get(0);
    tx.execute(
        "SELECT app.accept_auth_receipt($1,$2)",
        &[&retired, &json!({"version":1,"proofHash":"a".repeat(64)})],
    )
    .await?;
    tx.commit().await?;
    let tx = owner.transaction().await?;
    tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
        .await?;
    audit(&tx, &payload(subject, "token_refreshed", None, None)).await?;
    assert_eq!(
        hook(&tx, subject, first_session, "token_refresh")
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competing_families_serialize_and_stale_repeatable_read_must_retry() -> TestResult {
    for isolation in [
        tokio_postgres::IsolationLevel::ReadCommitted,
        tokio_postgres::IsolationLevel::RepeatableRead,
    ] {
        for (winner, loser) in [("google", "discord"), ("discord", "google")] {
            let observer = connection().await?;
            let subject = subject(&observer, &[winner, loser]).await?;
            let mut first = connection().await?;
            let tx = first.transaction().await?;
            let first_pid: i32 = tx.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
            let first_session = seed_session(&tx, subject, "oauth").await?;
            tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
                .await?;
            audit(&tx, &payload(subject, "login", None, Some(winner))).await?;
            assert_eq!(
                hook(&tx, subject, first_session, "oauth").await?["claims"]["spellwake_auth"]["method"],
                winner
            );
            let name = format!("auth-race-{}", Uuid::new_v4());
            let second_name = name.clone();
            let (ready, started) = tokio::sync::oneshot::channel();
            let second = tokio::spawn(async move {
                let mut client = connection().await?;
                client
                    .query_one(
                        "SELECT set_config('application_name',$1,false)",
                        &[&second_name],
                    )
                    .await?;
                let tx = client
                    .build_transaction()
                    .isolation_level(isolation)
                    .start()
                    .await?;
                let session = seed_session(&tx, subject, "oauth").await?;
                tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
                    .await?;
                audit(&tx, &payload(subject, "login", None, Some(loser))).await?;
                ready
                    .send(())
                    .map_err(|_| "Auth race observer disappeared")?;
                let error = hook(&tx, subject, session, "oauth").await.unwrap_err();
                let code = error.code().cloned();
                tx.rollback().await?;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>((code, session))
            });
            started.await?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let waiting: bool = observer.query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a JOIN pg_locks l ON l.pid=a.pid WHERE a.application_name=$1 AND l.locktype='advisory' AND NOT l.granted AND $2=ANY(pg_blocking_pids(a.pid)))", &[&name,&first_pid]).await?.get(0);
                if waiting {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "second Auth exchange never waited on the authority fence"
                );
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            tx.commit().await?;
            let (code, session) = second.await??;
            assert_eq!(
                code,
                Some(
                    if matches!(isolation, tokio_postgres::IsolationLevel::ReadCommitted) {
                        SqlState::INVALID_AUTHORIZATION_SPECIFICATION
                    } else {
                        SqlState::T_R_SERIALIZATION_FAILURE
                    }
                )
            );
            assert_eq!(observer.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1)",&[&subject]).await?.get::<_,i64>(0),2);
            assert_eq!(
                observer
                    .query_one(
                        "SELECT count(*) FROM auth.sessions WHERE id=$1",
                        &[&session]
                    )
                    .await?
                    .get::<_, i64>(0),
                0
            );
        }
    }
    Ok(())
}
