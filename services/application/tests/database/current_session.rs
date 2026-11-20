//! Privileged fixtures exercise canonical request resolution; HTTP verifies signatures separately.
use super::{TestResult, account, connection, seed_auth_authority};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;

async fn read(client: &Client, claims: &str) -> Result<Value, tokio_postgres::Error> {
    client
        .batch_execute("RESET ROLE; SET ROLE authenticated")
        .await?;
    client
        .query_one(
            "SELECT set_config('request.jwt.claims',$1,false)",
            &[&claims],
        )
        .await?;
    Ok(client
        .query_one("SELECT app.read_current_session()", &[])
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn session_read_distinguishes_setup_active_pending_and_incompatible_accounts() -> TestResult {
    let client = connection().await?;
    let subject = Uuid::new_v4();
    client
        .execute("INSERT INTO auth.users(id) VALUES($1)", &[&subject])
        .await?;
    let session = seed_auth_authority(&client, subject).await?;
    let claims: Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    let expected = json!({"status":"success","data":{"state":"needs_registration","method":"email_code",
        "subjectId":subject,"sessionId":session,"accountId":null}});
    assert_eq!(read(&client, &claims.to_string()).await?, expected);
    client.batch_execute("RESET ROLE").await?;
    let actor = Uuid::new_v4();
    client.execute("INSERT INTO app.account(id,auth_subject,username,sign_in_method,created_at,state) VALUES($1,$2,$3,'email_code',statement_timestamp(),'active')",
        &[&actor,&subject,&format!("session_{}", &subject.simple().to_string()[..16])]).await?;
    let mut expected = expected;
    expected["data"]["state"] = json!("active");
    expected["data"]["accountId"] = json!(actor);
    assert_eq!(read(&client, &claims.to_string()).await?, expected);
    client.batch_execute("RESET ROLE").await?;
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=statement_timestamp() WHERE id=$1", &[&actor]).await?;
    expected["data"]["state"] = json!("deletion_pending");
    assert_eq!(read(&client, &claims.to_string()).await?, expected);
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE app.account SET sign_in_method='google' WHERE id=$1",
            &[&actor],
        )
        .await?;
    let denied = json!({"status":"fail","data":{"session":"unauthorized"}});
    assert_eq!(read(&client, &claims.to_string()).await?, denied);
    client.batch_execute("RESET ROLE").await?;
    let operation = Uuid::new_v4();
    client.execute("INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES($1,'retirement',$2::uuid::text,'account_retirement',$2,statement_timestamp(),'committed')", &[&operation,&actor]).await?;
    client.execute("UPDATE app.account SET sign_in_method='email_code',state='deleted',deletion_finalized_at=statement_timestamp(),retirement_operation_id=$2 WHERE id=$1", &[&actor,&operation]).await?;
    assert_eq!(read(&client, &claims.to_string()).await?, denied);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn session_read_rejects_invalid_root_claims_and_revoked_session() -> TestResult {
    let client = connection().await?;
    let (_, subject) = account(&client).await?;
    let claims: Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    let denied = json!({"status":"fail","data":{"session":"unauthorized"}});
    let mut variants = vec![String::new(), "{".into(), "null".into(), "[]".into()];
    for (key, value) in [
        ("sub", json!("bad")),
        ("session_id", json!(Uuid::new_v4())),
        ("role", json!("service_role")),
        ("aud", json!(["authenticated", "other"])),
        ("exp", json!(0)),
        ("exp", json!("tomorrow")),
        ("spellwake_auth", json!({})),
    ] {
        let mut invalid = claims.clone();
        invalid[key] = value;
        variants.push(invalid.to_string());
    }
    let mut metadata_only = claims.clone();
    let authority = metadata_only
        .as_object_mut()
        .unwrap()
        .remove("spellwake_auth")
        .unwrap();
    metadata_only["user_metadata"] = json!({"spellwake_auth":authority});
    variants.push(metadata_only.to_string());
    for invalid in variants {
        assert_eq!(read(&client, &invalid).await?, denied);
        assert_eq!(
            client
                .query_one("SELECT app.current_account_id()", &[])
                .await?
                .get::<_, Option<Uuid>>(0),
            None
        );
    }
    client.batch_execute("RESET ROLE").await?;
    let session = Uuid::parse_str(claims["session_id"].as_str().unwrap())?;
    client
        .execute(
            "UPDATE auth.sessions SET not_after=statement_timestamp() WHERE id=$1",
            &[&session],
        )
        .await?;
    assert_eq!(read(&client, &claims.to_string()).await?, denied);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn session_read_acl_exposes_only_no_argument_authenticated_entrypoint() -> TestResult {
    let client = connection().await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        let row = client.query_one("SELECT has_function_privilege($1,'app.read_current_session()','EXECUTE'),has_function_privilege($1,'app.read_current_auth_context()','EXECUTE')", &[&role]).await?;
        assert_eq!(row.get::<_, bool>(0), role == "authenticated");
        assert!(!row.get::<_, bool>(1));
    }
    let count: i64 = client.query_one("SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='app' AND p.proname='read_current_session' AND p.pronargs=0", &[]).await?.get(0);
    assert_eq!(count, 1);
    Ok(())
}
