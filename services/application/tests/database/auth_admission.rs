//! Privileged claim injection tests RLS admission, not JWT signature middleware.
use super::{TestResult, account, connection, seed_auth_authority};
use serde_json::{Value, json};
use tokio_postgres::{Client, GenericClient, error::SqlState};
use uuid::Uuid;

async fn claims(client: &Client, subject: Uuid) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0))
}

async fn read_as(client: &Client, claims: &Value) -> Result<Option<Uuid>, tokio_postgres::Error> {
    client
        .batch_execute("RESET ROLE; SET ROLE authenticated")
        .await?;
    client
        .query_one(
            "SELECT set_config('request.jwt.claims',$1,false)",
            &[&claims.to_string()],
        )
        .await?;
    Ok(client
        .query_one("SELECT app.current_account_id()", &[])
        .await?
        .get(0))
}

pub(super) async fn slots(client: &Client) -> Result<[Uuid; 3], tokio_postgres::Error> {
    let values = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    for (id, kind) in values.iter().zip(["back", "sleeve", "dice"]) {
        client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,$2,'test-only:auth-admission','active',true)", &[&id,&kind]).await?;
    }
    Ok(values)
}

async fn retirement<C: GenericClient + Sync>(
    client: &C,
    subject: Uuid,
    actor: Uuid,
) -> Result<(), tokio_postgres::Error> {
    let id: Uuid = client.query_one("INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref) VALUES('auth:'||app.auth_uuid_hash($1),'auth_retirement_v1','primary',$2::uuid::text,app.auth_json_hash(jsonb_build_object('version',1,'scope','auth_retirement_v1','subjectHash',app.auth_uuid_hash($1),'target',$2::uuid::text)),'pending','') RETURNING id", &[&subject,&actor]).await?.get(0);
    client
        .execute(
            "SELECT app.accept_auth_receipt($1,$2)",
            &[&id, &json!({"version":1,"proofHash":"b".repeat(64)})],
        )
        .await?;
    Ok(())
}

pub(super) async fn wait_on(
    observer: &Client,
    name: &str,
    blocker: i32,
    without_advisory: bool,
) -> TestResult {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let row = observer.query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE a.application_name=$1 AND a.wait_event_type='Lock' AND $2=ANY(pg_blocking_pids(a.pid))),EXISTS(SELECT 1 FROM pg_stat_activity a JOIN pg_locks l ON l.pid=a.pid WHERE a.application_name=$1 AND l.locktype='advisory' AND l.granted)", &[&name,&blocker]).await?;
        if row.get::<_, bool>(0) {
            if without_advisory {
                assert!(
                    !row.get::<_, bool>(1),
                    "signup fence acquired before Auth parent"
                );
            }
            return Ok(());
        }
        assert!(
            std::time::Instant::now() < deadline,
            "registration never reached the expected lock"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn account_reads_require_exact_root_authority_and_reject_subject_only_tokens() -> TestResult {
    let client = connection().await?;
    let (actor, subject) = account(&client).await?;
    let valid = claims(&client, subject).await?;
    assert_eq!(read_as(&client, &valid).await?, Some(actor));
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.asset_owner WHERE account_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    let mut variants = vec![
        json!({}),
        json!(["invalid"]),
        json!({"sub":subject.to_string()}),
        valid.clone(),
    ];
    variants
        .last_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("spellwake_auth");
    for (key, value) in [
        ("sub", json!("invalid")),
        ("session_id", json!(Uuid::new_v4().to_string())),
        ("role", json!("service_role")),
        ("aud", json!("other")),
        ("exp", json!(0)),
        ("exp", json!("future")),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        variants.push(invalid);
    }
    for (key, value) in [
        ("method", json!("google")),
        ("bindingId", json!(Uuid::new_v4().to_string())),
        ("sessionGrantId", json!(Uuid::new_v4().to_string())),
        ("version", json!(2)),
        ("extra", json!("forged")),
    ] {
        let mut invalid = valid.clone();
        invalid["spellwake_auth"][key] = value;
        variants.push(invalid);
    }
    for invalid in variants {
        assert_eq!(read_as(&client, &invalid).await?, None);
        assert_eq!(
            client
                .query_one(
                    "SELECT count(*) FROM app.asset_owner WHERE account_id=$1",
                    &[&actor]
                )
                .await?
                .get::<_, i64>(0),
            0
        );
    }
    client.query_one("SELECT set_config('request.jwt.claims','',false),set_config('request.jwt.claim.sub',$1,false)", &[&subject.to_string()]).await?;
    assert_eq!(
        client
            .query_one("SELECT app.current_account_id()", &[])
            .await?
            .get::<_, Option<Uuid>>(0),
        None
    );
    let mut array_audience = valid.clone();
    array_audience["aud"] = json!(["authenticated"]);
    assert_eq!(read_as(&client, &array_audience).await?, Some(actor));
    assert_eq!(
        client
            .query_one("SELECT current_user", &[])
            .await?
            .get::<_, String>(0),
        "authenticated"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn account_admission_rechecks_session_lifecycle_and_retirement() -> TestResult {
    let mut client = connection().await?;
    let (actor, subject) = account(&client).await?;
    let valid = claims(&client, subject).await?;
    let session = Uuid::parse_str(valid["session_id"].as_str().unwrap())?;
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=statement_timestamp() WHERE id=$1", &[&actor]).await?;
    assert_eq!(read_as(&client, &valid).await?, Some(actor));
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    assert_eq!(
        client
            .query_one(
                "SELECT app.register_account($1,$2,'irrelevant',$3,$3,$3)",
                &[&subject, &session, &Uuid::new_v4()]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE app.account SET state='active',deletion_requested_at=NULL,sign_in_method='google' WHERE id=$1",
            &[&actor],
        )
        .await?;
    assert_eq!(read_as(&client, &valid).await?, None);
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    assert_eq!(
        client
            .query_one(
                "SELECT app.register_account($1,$2,'irrelevant',$3,$3,$3)",
                &[&subject, &session, &Uuid::new_v4()]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE app.account SET sign_in_method='email_code' WHERE id=$1",
            &[&actor],
        )
        .await?;
    client
        .execute(
            "UPDATE auth.sessions SET not_after=statement_timestamp() WHERE id=$1",
            &[&session],
        )
        .await?;
    assert_eq!(read_as(&client, &valid).await?, None);
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE auth.sessions SET not_after=NULL WHERE id=$1",
            &[&session],
        )
        .await?;
    assert_eq!(read_as(&client, &valid).await?, Some(actor));
    client.batch_execute("RESET ROLE").await?;
    let transaction = client.transaction().await?;
    retirement(&transaction, subject, actor).await?;
    transaction.commit().await?;
    assert_eq!(read_as(&client, &valid).await?, None);
    assert_eq!(
        client
            .query_one("SELECT app.read_current_session()", &[])
            .await?
            .get::<_, Value>(0),
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    assert_eq!(
        client
            .query_one(
                "SELECT app.register_account($1,$2,'irrelevant',$3,$3,$3)",
                &[&subject, &session, &Uuid::new_v4()]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn registration_rechecks_retirement_after_waiting_on_a_cleared_subject() -> TestResult {
    registration_rechecks_retirement_after_waiting_on_a_cleared_subject_case(false).await
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn registration_rechecks_retirement_after_waiting_on_a_cleared_subject_through_facade()
-> TestResult {
    registration_rechecks_retirement_after_waiting_on_a_cleared_subject_case(true).await
}
async fn registration_rechecks_retirement_after_waiting_on_a_cleared_subject_case(
    facade: bool,
) -> TestResult {
    let function = if facade {
        "register_account_command"
    } else {
        "register_account"
    };
    let observer = connection().await?;
    let (actor, subject) = account(&observer).await?;
    let valid = claims(&observer, subject).await?;
    let session = Uuid::parse_str(valid["session_id"].as_str().unwrap())?;
    let [back, sleeve, dice] = slots(&observer).await?;
    let mut finalizer = connection().await?;
    let tx = finalizer.transaction().await?;
    let blocker: i32 = tx.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    tx.query_one(
        "SELECT id FROM app.account WHERE id=$1 FOR UPDATE",
        &[&actor],
    )
    .await?;
    let operation = Uuid::new_v4();
    tx.execute("INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES($1,'retirement',$2::uuid::text,'account_retirement',$2,statement_timestamp(),'committed')", &[&operation,&actor]).await?;
    retirement(&tx, subject, actor).await?;
    tx.execute("UPDATE app.account SET auth_subject=NULL,state='deleted',deletion_requested_at=statement_timestamp(),deletion_finalized_at=statement_timestamp(),retirement_operation_id=$2 WHERE id=$1", &[&actor,&operation]).await?;
    let name = format!("retire-register-{}", Uuid::new_v4());
    let second_name = name.clone();
    let username = format!("new_{}", &subject.simple().to_string()[..16]);
    let expected_name = username.clone();
    let (ready, started) = tokio::sync::oneshot::channel();
    let waiting = tokio::spawn(async move {
        let client = connection().await?;
        client
            .query_one(
                "SELECT set_config('application_name',$1,false)",
                &[&second_name],
            )
            .await?;
        client.batch_execute("SET ROLE service_role").await?;
        ready
            .send(())
            .map_err(|_| "registration observer disappeared")?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
            client
                .query_one(
                    &format!("SELECT app.{function}($1,$2,$3,$4,$5,$6)"),
                    &[&subject, &session, &username, &back, &sleeve, &dice],
                )
                .await
                .map(|row| row.get::<_, serde_json::Value>(0))
                .map_err(|error| error.code().cloned()),
        )
    });
    started.await?;
    wait_on(&observer, &name, blocker, false).await?;
    tx.commit().await?;
    assert_eq!(
        waiting.await??,
        if facade {
            Ok(serde_json::json!({"status":"fail","data":{"session":"unauthorized"}}))
        } else {
            Err(Some(SqlState::INVALID_AUTHORIZATION_SPECIFICATION))
        }
    );
    assert_eq!(
        observer
            .query_one(
                "SELECT count(*) FROM app.account WHERE username=$1 OR auth_subject=$2",
                &[&expected_name, &subject]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn registration_locks_auth_parent_before_signup_and_cannot_outlive_its_deletion() -> TestResult
{
    registration_locks_auth_parent_before_signup_and_cannot_outlive_its_deletion_case(false).await
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn registration_locks_auth_parent_before_signup_and_cannot_outlive_its_deletion_through_facade()
-> TestResult {
    registration_locks_auth_parent_before_signup_and_cannot_outlive_its_deletion_case(true).await
}
async fn registration_locks_auth_parent_before_signup_and_cannot_outlive_its_deletion_case(
    facade: bool,
) -> TestResult {
    let function = if facade {
        "register_account_command"
    } else {
        "register_account"
    };
    let observer = connection().await?;
    let subject = Uuid::new_v4();
    observer
        .execute("INSERT INTO auth.users(id) VALUES($1)", &[&subject])
        .await?;
    let session = seed_auth_authority(&observer, subject).await?;
    let [back, sleeve, dice] = slots(&observer).await?;
    let mut deleting = connection().await?;
    let tx = deleting.transaction().await?;
    let blocker: i32 = tx.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    super::auth_identity::canonical_delete(&tx, subject).await?;
    let name = format!("delete-register-{}", Uuid::new_v4());
    let second_name = name.clone();
    let (ready, started) = tokio::sync::oneshot::channel();
    let waiting = tokio::spawn(async move {
        let client = connection().await?;
        client
            .query_one(
                "SELECT set_config('application_name',$1,false)",
                &[&second_name],
            )
            .await?;
        client.batch_execute("SET ROLE service_role").await?;
        ready.send(()).map_err(|_| "parent observer disappeared")?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
            client
                .query_one(
                    &format!("SELECT app.{function}($1,$2,'deleted_parent',$3,$4,$5)"),
                    &[&subject, &session, &back, &sleeve, &dice],
                )
                .await
                .map(|row| row.get::<_, serde_json::Value>(0))
                .map_err(|error| error.code().cloned()),
        )
    });
    started.await?;
    wait_on(&observer, &name, blocker, true).await?;
    tx.commit().await?;
    assert_eq!(
        waiting.await??,
        if facade {
            Ok(serde_json::json!({"status":"fail","data":{"session":"unauthorized"}}))
        } else {
            Err(Some(SqlState::INVALID_AUTHORIZATION_SPECIFICATION))
        }
    );
    assert_eq!(
        observer
            .query_one(
                "SELECT count(*) FROM app.account WHERE auth_subject=$1",
                &[&subject]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    Ok(())
}
