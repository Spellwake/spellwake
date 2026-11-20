//! Privileged SQL guard fixtures, not actual GoTrue/provider qualification.
use super::{TestResult, account, connection, seed_auth_authority};
use serde_json::{Value, json};
use tokio_postgres::{Client, GenericClient, error::SqlState};
use uuid::Uuid;

pub(super) async fn canonical_delete<C: GenericClient + Sync>(
    client: &C,
    subject: Uuid,
) -> Result<(), tokio_postgres::Error> {
    client
        .batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
        .await?;
    let payload = json!({"actor_id":Uuid::new_v4(),"action":"user_deleted","log_type":"team","traits":{"user_id":subject}});
    client
        .execute(
            "INSERT INTO auth.audit_log_entries(payload) VALUES($1::jsonb::json)",
            &[&payload],
        )
        .await?;
    client
        .execute("DELETE FROM auth.users WHERE id=$1", &[&subject])
        .await?;
    Ok(())
}
async fn subject(client: &Client) -> Result<Uuid, tokio_postgres::Error> {
    let id = Uuid::new_v4();
    client
        .execute("INSERT INTO auth.users(id) VALUES($1)", &[&id])
        .await?;
    Ok(id)
}
async fn identity(
    client: &Client,
    subject: Uuid,
    provider: &str,
) -> Result<Uuid, tokio_postgres::Error> {
    Ok(client.query_one("INSERT INTO auth.identities(user_id,provider,provider_id,identity_data) VALUES($1,$2,$3,'{\"email_verified\":true}'::jsonb) RETURNING id",&[&subject,&provider,&Uuid::new_v4().to_string()]).await?.get(0))
}
pub(super) async fn mark_deleted<C: GenericClient + Sync>(
    client: &C,
    actor: Uuid,
) -> Result<Uuid, tokio_postgres::Error> {
    let op = Uuid::new_v4();
    client.execute("INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES($1,'retirement',$2::uuid::text,'account_retirement',$2,statement_timestamp(),'committed')",&[&op,&actor]).await?;
    client.execute("UPDATE app.account SET state='deleted',deletion_requested_at=statement_timestamp(),deletion_finalized_at=statement_timestamp(),retirement_operation_id=$2 WHERE id=$1",&[&actor,&op]).await?;
    Ok(op)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn identities_preserve_bound_family_keys_ownership_and_allow_metadata_refresh() -> TestResult
{
    let client = connection().await?;
    let first = subject(&client).await?;
    let other = subject(&client).await?;
    let selected = identity(&client, first, "email").await?;
    let alternate = identity(&client, first, "discord").await?;
    // All identity mutation kinds remain permitted before first binding.
    client
        .execute(
            "UPDATE auth.identities SET provider='google',provider_id='prebinding' WHERE id=$1",
            &[&alternate],
        )
        .await?;
    client
        .execute("DELETE FROM auth.identities WHERE id=$1", &[&alternate])
        .await?;
    let alternate = identity(&client, first, "discord").await?;
    seed_auth_authority(&client, first).await?;
    client.execute("UPDATE auth.identities SET identity_data=identity_data||'{\"email\":\"changed@example.invalid\",\"email_verified\":true}'::jsonb WHERE id=$1",&[&selected]).await?;
    identity(&client, first, "email").await?; // Family cardinality is not row cardinality.
    assert_eq!(
        identity(&client, first, "google").await.unwrap_err().code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    for sql in [
        "UPDATE auth.identities SET provider='google' WHERE id=$1",
        "UPDATE auth.identities SET provider_id='other' WHERE id=$1",
        "UPDATE auth.identities SET id=gen_random_uuid() WHERE id=$1",
        "DELETE FROM auth.identities WHERE id=$1",
    ] {
        assert_eq!(
            client.execute(sql, &[&selected]).await.unwrap_err().code(),
            Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
        );
    }
    assert!(
        client
            .execute(
                "UPDATE auth.identities SET user_id=$2 WHERE id=$1",
                &[&selected, &other]
            )
            .await
            .is_err()
    );
    assert!(client.execute("UPDATE auth.identities SET identity_data=identity_data||'{\"email_verified\":true}'::jsonb WHERE id=$1",&[&alternate]).await.is_err());
    let incoming = identity(&client, other, "email").await?;
    assert!(
        client
            .execute(
                "UPDATE auth.identities SET user_id=$2 WHERE id=$1",
                &[&incoming, &first]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE auth.users SET id=$2 WHERE id=$1",
                &[&first, &Uuid::new_v4()]
            )
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn canonical_unregistered_cleanup_is_target_bound_retires_once_and_cascades() -> TestResult {
    let mut client = connection().await?;
    let target = subject(&client).await?;
    identity(&client, target, "email").await?;
    seed_auth_authority(&client, target).await?;
    assert_eq!(
        client
            .execute("DELETE FROM auth.users WHERE id=$1", &[&target])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    let tx = client.transaction().await?;
    canonical_delete(&tx, target).await?;
    tx.commit().await?;
    let row=client.query_one("SELECT (SELECT count(*) FROM auth.users WHERE id=$1),(SELECT count(*) FROM auth.identities WHERE user_id=$1),(SELECT count(*) FROM auth.sessions WHERE user_id=$1),(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_retirement_v1' AND target_ref='unregistered' AND state='accepted')",&[&target]).await?;
    assert_eq!(
        (
            row.get::<_, i64>(0),
            row.get::<_, i64>(1),
            row.get::<_, i64>(2),
            row.get::<_, i64>(3)
        ),
        (0, 0, 0, 1)
    );
    let cleanup: Uuid = client.query_one("SELECT c.id FROM app.command_receipt c JOIN app.command_receipt r ON r.id=(c.result_ref::jsonb->>'retirementId')::uuid WHERE c.principal_key='auth:'||app.auth_uuid_hash($1) AND c.scope='auth_cleanup_v1' AND c.state='accepted' AND r.scope='auth_retirement_v1' AND r.target_ref='unregistered' AND c.target_ref=r.target_ref", &[&target]).await?.get(0);
    assert!(
        client
            .query_one("SELECT app.read_bound_auth_method($1)", &[&target])
            .await?
            .get::<_, Option<String>>(0)
            .is_some()
    );
    assert!(
        client
            .execute("INSERT INTO auth.users(id) VALUES($1)", &[&target])
            .await
            .is_ok()
    );
    assert!(identity(&client, target, "email").await.is_err());
    // This privileged UUID reuse fixture is not a new usable Auth identity.
    let tx = client.transaction().await?;
    canonical_delete(&tx, target).await?;
    tx.commit().await?;
    assert_eq!(client.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_retirement_v1'",&[&target]).await?.get::<_,i64>(0),1);
    assert_eq!(client.query_one("SELECT id FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_cleanup_v1'", &[&target]).await?.get::<_,Uuid>(0), cleanup);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn registered_retirement_is_source_bound_replays_after_clear_and_allows_later_cleanup()
-> TestResult {
    let mut client = connection().await?;
    let (actor, subject) = account(&client).await?;
    assert_eq!(
        client
            .execute(
                "UPDATE app.account SET auth_cleanup_subject=auth_subject WHERE id=$1",
                &[&actor]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert!(
        client
            .query_one(
                "SELECT app.record_registered_auth_retirement($1,$2,$3)",
                &[&subject, &actor, &Uuid::new_v4()]
            )
            .await
            .is_err()
    );
    let tx = client.transaction().await?;
    assert_eq!(
        canonical_delete(&tx, subject).await.unwrap_err().code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    tx.rollback().await?;
    let op = mark_deleted(&client, actor).await?;
    let id: Uuid = client
        .query_one(
            "SELECT app.record_registered_auth_retirement($1,$2,$3)",
            &[&subject, &actor, &op],
        )
        .await?
        .get(0);
    assert!(
        client
            .query_one(
                "SELECT app.record_registered_auth_retirement($1,$2,$3)",
                &[&Uuid::new_v4(), &actor, &op]
            )
            .await
            .is_err()
    );
    // Even a deleted row must be detached by the app finalizer before Auth cleanup.
    let tx = client.transaction().await?;
    assert!(canonical_delete(&tx, subject).await.is_err());
    tx.rollback().await?;
    client
        .execute(
            "UPDATE app.account SET auth_cleanup_subject=auth_subject,auth_subject=NULL WHERE id=$1",
            &[&actor],
        )
        .await?;
    let replay: Uuid = client
        .query_one(
            "SELECT app.record_registered_auth_retirement($1,$2,$3)",
            &[&subject, &actor, &op],
        )
        .await?
        .get(0);
    assert_eq!(replay, id);
    assert!(
        client
            .query_one(
                "SELECT app.record_registered_auth_retirement($1,$2,$3)",
                &[&Uuid::new_v4(), &actor, &op]
            )
            .await
            .is_err()
    );
    let tx = client.transaction().await?;
    canonical_delete(&tx, subject).await?;
    tx.rollback().await?;
    let retained: Option<Uuid> = client
        .query_one(
            "SELECT auth_cleanup_subject FROM app.account WHERE id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    assert_eq!(retained, Some(subject));
    assert_eq!(client.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_cleanup_v1'", &[&subject]).await?.get::<_,i64>(0), 0);
    let tx = client.transaction().await?;
    canonical_delete(&tx, subject).await?;
    tx.commit().await?;
    assert_eq!(
        client
            .query_one("SELECT count(*) FROM auth.users WHERE id=$1", &[&subject])
            .await?
            .get::<_, i64>(0),
        0
    );
    let cleared: Option<Uuid> = client
        .query_one(
            "SELECT auth_cleanup_subject FROM app.account WHERE id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    assert_eq!(cleared, None);
    let cleanup: Value = client.query_one("SELECT result_ref::jsonb FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_cleanup_v1' AND target_ref=$2::uuid::text AND state='accepted'", &[&subject,&actor]).await?.get(0);
    assert_eq!(cleanup["retirementId"], id.to_string());
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn cleanup_denies_soft_deletion_unsupported_isolation_and_helper_execution() -> TestResult {
    let mut client = connection().await?;
    let target = subject(&client).await?;
    seed_auth_authority(&client, target).await?;
    assert!(
        client
            .execute(
                "UPDATE auth.users SET deleted_at=statement_timestamp() WHERE id=$1",
                &[&target]
            )
            .await
            .is_err()
    );
    for isolation in ["REPEATABLE READ", "SERIALIZABLE"] {
        let tx = client.transaction().await?;
        tx.batch_execute(&format!("SET TRANSACTION ISOLATION LEVEL {isolation}"))
            .await?;
        assert_eq!(
            canonical_delete(&tx, target).await.unwrap_err().code(),
            Some(&SqlState::T_R_SERIALIZATION_FAILURE)
        );
        tx.rollback().await?;
    }
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        assert_eq!(
            client
                .query_one(
                    "SELECT has_column_privilege($1,'app.account','auth_cleanup_subject','SELECT')",
                    &[&role]
                )
                .await?
                .get::<_, bool>(0),
            role == "service_role",
            "private cleanup target: {role}"
        );
        client.batch_execute(&format!("SET ROLE {role}")).await?;
        for sql in [
            "SELECT app.read_bound_auth_method($1)",
            "SELECT app.read_auth_retirement($1)",
            "SELECT app.read_auth_cleanup_proof($1)",
            "SELECT app.record_registered_auth_retirement($1,$1,$1)",
            "SELECT app.record_auth_cleanup($1,$1)",
            "SELECT app.check_registered_auth_retirement($1,$1)",
        ] {
            assert_eq!(
                client.query_one(sql, &[&target]).await.unwrap_err().code(),
                Some(&SqlState::INSUFFICIENT_PRIVILEGE)
            );
        }
        client.batch_execute("RESET ROLE").await?;
    }
    let row=client.query_one("SELECT (SELECT count(*) FROM auth.users WHERE id=$1),(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_retirement_v1')",&[&target]).await?;
    assert_eq!((row.get::<_, i64>(0), row.get::<_, i64>(1)), (1, 0));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn cleanup_context_cannot_be_spoofed_stale_retargeted_or_used_under_another_login()
-> TestResult {
    let mut client = connection().await?;
    let target = subject(&client).await?;
    seed_auth_authority(&client, target).await?;
    for variant in [
        "missing",
        "invalid_json",
        "wrong_kind",
        "wrong_target",
        "wrong_actor",
        "wrong_hash",
        "stale_xact",
        "absent_source",
        "wrong_login",
        "modified_source",
    ] {
        let tx = client.transaction().await?;
        tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
            .await?;
        if variant != "missing" && variant != "invalid_json" {
            let payload = if variant == "wrong_kind" {
                json!({"actor_id":target,"action":"login","log_type":"account"})
            } else {
                json!({"actor_id":Uuid::new_v4(),"action":"user_deleted","log_type":"team","traits":{"user_id":if variant=="wrong_target" { Uuid::new_v4() } else { target }}})
            };
            tx.execute(
                "INSERT INTO auth.audit_log_entries(payload) VALUES($1::jsonb::json)",
                &[&payload],
            )
            .await?;
            let mut context: Value = serde_json::from_str(
                &tx.query_one("SELECT current_setting('spellwake.auth_provenance')", &[])
                    .await?
                    .get::<_, String>(0),
            )?;
            match variant {
                "wrong_actor" => context["actorHash"] = json!("0".repeat(64)),
                "wrong_hash" => context["sourceHash"] = json!("0".repeat(64)),
                "stale_xact" => context["xact"] = json!("0"),
                "absent_source" => context["sourceId"] = json!(Uuid::new_v4()),
                _ => {}
            }
            tx.query_one(
                "SELECT set_config('spellwake.auth_provenance',$1,true)",
                &[&context.to_string()],
            )
            .await?;
        } else {
            tx.query_one(
                "SELECT set_config('spellwake.auth_provenance',$1,true)",
                &[&if variant == "missing" { "" } else { "{" }],
            )
            .await?;
        }
        if variant == "modified_source" {
            tx.batch_execute("RESET SESSION AUTHORIZATION").await?;
            tx.execute("UPDATE auth.audit_log_entries SET payload=jsonb_set(payload::jsonb,'{actor_id}',to_jsonb($1::uuid::text))::json WHERE id=(current_setting('spellwake.auth_provenance')::jsonb->>'sourceId')::uuid",&[&Uuid::new_v4()]).await?;
            tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
                .await?;
        }
        if variant == "wrong_login" {
            tx.batch_execute("RESET SESSION AUTHORIZATION").await?;
        }
        assert_eq!(
            tx.execute("DELETE FROM auth.users WHERE id=$1", &[&target])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION),
            "{variant}"
        );
        tx.rollback().await?;
    }
    assert_eq!(
        client
            .query_one("SELECT count(*) FROM auth.users WHERE id=$1", &[&target])
            .await?
            .get::<_, i64>(0),
        1
    );
    assert_eq!(client.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_retirement_v1'",&[&target]).await?.get::<_,i64>(0),0);
    let row=client.query_one("SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='app' AND p.proname IN ('read_bound_auth_method','read_auth_retirement','read_auth_cleanup_proof','check_registered_auth_retirement','record_registered_auth_retirement','guard_auth_identity','guard_auth_user_cleanup') AND p.prosecdef AND p.proconfig=ARRAY['search_path=\"\"']",&[]).await?;
    assert_eq!(row.get::<_, i64>(0), 7);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn first_registration_commits_before_waiting_cleanup_which_must_then_deny() -> TestResult {
    let observer = connection().await?;
    let subject = subject(&observer).await?;
    let session = seed_auth_authority(&observer, subject).await?;
    let [back, sleeve, dice] = super::auth_admission::slots(&observer).await?;
    let mut registration = connection().await?;
    let tx = registration.transaction().await?;
    let blocker: i32 = tx.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    tx.batch_execute("SET LOCAL ROLE service_role").await?;
    let username = format!("race_{}", &subject.simple().to_string()[..16]);
    let created: Value = tx
        .query_one(
            "SELECT app.register_account_command($1,$2,$3,$4,$5,$6)",
            &[&subject, &session, &username, &back, &sleeve, &dice],
        )
        .await?
        .get(0);
    assert_eq!(created["status"], "success");
    let name = format!("cleanup-after-registration-{}", Uuid::new_v4());
    let child_name = name.clone();
    let (ready, started) = tokio::sync::oneshot::channel();
    let child = tokio::spawn(async move {
        let mut cleanup = connection().await?;
        cleanup
            .query_one(
                "SELECT set_config('application_name',$1,false)",
                &[&child_name],
            )
            .await?;
        let tx = cleanup.transaction().await?;
        ready.send(()).map_err(|_| "cleanup observer disappeared")?;
        let failure = canonical_delete(&tx, subject)
            .await
            .unwrap_err()
            .code()
            .cloned();
        tx.rollback().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(failure)
    });
    started.await?;
    super::auth_admission::wait_on(&observer, &name, blocker, true).await?;
    tx.commit().await?;
    assert_eq!(
        child.await??,
        Some(SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    assert_eq!(
        observer
            .query_one(
                "SELECT count(*) FROM app.account WHERE auth_subject=$1",
                &[&subject]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    assert_eq!(observer.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_retirement_v1'",&[&subject]).await?.get::<_,i64>(0),0);
    super::set_auth_claims(&observer, subject).await?;
    assert_eq!(
        observer
            .query_one("SELECT app.read_current_session()", &[])
            .await?
            .get::<_, Value>(0)["data"]["state"],
        "active"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn initial_issuance_blocks_cleanup_then_committed_retirement_denies_the_new_session()
-> TestResult {
    let observer = connection().await?;
    let subject = subject(&observer).await?;
    observer.execute("UPDATE auth.users SET email='initial-issuance@example.invalid',email_confirmed_at=statement_timestamp() WHERE id=$1",&[&subject]).await?;
    identity(&observer, subject, "email").await?;
    let mut producer = connection().await?;
    let tx = producer.transaction().await?;
    let blocker: i32 = tx.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    let session = super::auth_provenance::seed_session(&tx, subject, "otp").await?;
    tx.batch_execute("SET LOCAL SESSION AUTHORIZATION supabase_auth_admin")
        .await?;
    super::auth_provenance::audit(
        &tx,
        &super::auth_provenance::payload(subject, "user_signedup", Some("email"), None),
    )
    .await?;
    let issued = super::auth_provenance::hook(&tx, subject, session, "otp").await?;
    assert_eq!(issued["claims"]["spellwake_auth"]["method"], "email_code");
    let name = format!("cleanup-after-issuance-{}", Uuid::new_v4());
    let child_name = name.clone();
    let (ready, started) = tokio::sync::oneshot::channel();
    let child = tokio::spawn(async move {
        let mut cleanup = connection().await?;
        cleanup
            .query_one(
                "SELECT set_config('application_name',$1,false)",
                &[&child_name],
            )
            .await?;
        let tx = cleanup.transaction().await?;
        ready.send(()).map_err(|_| "cleanup observer disappeared")?;
        canonical_delete(&tx, subject).await?;
        tx.commit().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    started.await?;
    super::auth_admission::wait_on(&observer, &name, blocker, true).await?;
    tx.commit().await?;
    child.await??;
    let row=observer.query_one("SELECT (SELECT count(*) FROM auth.users WHERE id=$1),(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_binding_v1'),(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_session_v1'),(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_retirement_v1'),app.read_auth_session_authority($1,$2) IS NULL",&[&subject,&session]).await?;
    assert_eq!(
        (
            row.get::<_, i64>(0),
            row.get::<_, i64>(1),
            row.get::<_, i64>(2),
            row.get::<_, i64>(3),
            row.get::<_, bool>(4)
        ),
        (0, 1, 1, 1, true)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rolled_back_canonical_cleanup_restores_identity_and_original_family_liveness() -> TestResult
{
    let mut client = connection().await?;
    let subject = subject(&client).await?;
    client.execute("UPDATE auth.users SET email='rollback-cleanup@example.invalid',email_confirmed_at=statement_timestamp() WHERE id=$1",&[&subject]).await?;
    identity(&client, subject, "email").await?;
    let (session, initial) =
        super::auth_provenance::issue(&mut client, subject, "email", false).await?;
    let tx = client.transaction().await?;
    canonical_delete(&tx, subject).await?;
    tx.rollback().await?;
    assert!(
        client
            .query_one(
                "SELECT app.auth_session_is_live($1,$2,'otp')",
                &[&subject, &session]
            )
            .await?
            .get::<_, bool>(0)
    );
    assert_eq!(client.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope IN ('auth_retirement_v1','auth_cleanup_v1')",&[&subject]).await?.get::<_,i64>(0),0);
    let (_, again) = super::auth_provenance::issue(&mut client, subject, "email", true).await?;
    assert_eq!(
        initial["claims"]["spellwake_auth"]["bindingId"],
        again["claims"]["spellwake_auth"]["bindingId"]
    );
    assert_eq!(again["claims"]["spellwake_auth"]["method"], "email_code");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn registered_retirement_rejects_wrong_ledger_source_kind_actor_state_and_first_cleared_subject()
-> TestResult {
    let client = connection().await?;
    let (other, _) = account(&client).await?;
    for variant in ["kind", "scope", "reference", "actor", "state", "cleared"] {
        let (actor, subject) = account(&client).await?;
        let operation = Uuid::new_v4();
        let kind = if variant == "kind" {
            "transfer"
        } else {
            "retirement"
        };
        let scope = if variant == "scope" {
            "wrong_retirement"
        } else {
            "account_retirement"
        };
        let reference = if variant == "reference" {
            Uuid::new_v4()
        } else {
            actor
        }
        .to_string();
        let owner = if variant == "actor" { other } else { actor };
        let state = if variant == "state" {
            "pending"
        } else {
            "committed"
        };
        client.execute("INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES($1,$2,$3,$4,$5,statement_timestamp(),$6)",&[&operation,&kind,&reference,&scope,&owner,&state]).await?;
        client.execute("UPDATE app.account SET state='deleted',deletion_requested_at=statement_timestamp(),deletion_finalized_at=statement_timestamp(),retirement_operation_id=$2 WHERE id=$1",&[&actor,&operation]).await?;
        if variant == "cleared" {
            client
                .execute(
                    "UPDATE app.account SET auth_subject=NULL WHERE id=$1",
                    &[&actor],
                )
                .await?;
        }
        assert_eq!(
            client
                .query_one(
                    "SELECT app.record_registered_auth_retirement($1,$2,$3)",
                    &[&subject, &actor, &operation]
                )
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION),
            "{variant}"
        );
        assert_eq!(client.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash($1) AND scope='auth_retirement_v1'",&[&subject]).await?.get::<_,i64>(0),0);
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn identity_mutation_cannot_consume_pending_binding_or_retirement_receipts() -> TestResult {
    let mut client = connection().await?;
    for scope in ["auth_binding_v1", "auth_retirement_v1"] {
        let target = subject(&client).await?;
        let tx = client.transaction().await?;
        let payload = if scope == "auth_binding_v1" {
            json!({"version":1,"scope":scope,"subjectHash":tx.query_one("SELECT app.auth_uuid_hash($1)",&[&target]).await?.get::<_,String>(0)})
        } else {
            json!({"version":1,"scope":scope,"subjectHash":tx.query_one("SELECT app.auth_uuid_hash($1)",&[&target]).await?.get::<_,String>(0),"target":"unregistered"})
        };
        let reference = if scope == "auth_binding_v1" {
            "primary"
        } else {
            "unregistered"
        };
        tx.execute("INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref) VALUES('auth:'||app.auth_uuid_hash($1),$2,'primary',$3,app.auth_json_hash($4),'pending','')",&[&target,&scope,&reference,&payload]).await?;
        assert_eq!(tx.execute("INSERT INTO auth.identities(user_id,provider,provider_id,identity_data) VALUES($1,'email',$1::uuid::text,'{}')",&[&target]).await.unwrap_err().code(),Some(&SqlState::T_R_SERIALIZATION_FAILURE));
        tx.rollback().await?;
    }
    Ok(())
}
