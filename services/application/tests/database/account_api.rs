//! The HTTP facade delegates to the existing single transactional writer.
use super::{TestResult, account, auth_admission::slots, connection, seed_auth_authority};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn subject(
    client: &Client,
) -> Result<(Uuid, Uuid), Box<dyn std::error::Error + Send + Sync>> {
    let subject = Uuid::new_v4();
    client
        .execute("INSERT INTO auth.users(id) VALUES($1)", &[&subject])
        .await?;
    Ok((subject, seed_auth_authority(client, subject).await?))
}
async fn command(
    client: &Client,
    subject: Uuid,
    session: Uuid,
    username: &str,
    defaults: [Uuid; 3],
) -> Result<Value, tokio_postgres::Error> {
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    let [back, sleeve, dice] = defaults;
    Ok(client
        .query_one(
            "SELECT app.register_account_command($1,$2,$3,$4,$5,$6)",
            &[&subject, &session, &username, &back, &sleeve, &dice],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn account_and_deck_reads_use_live_caller_rls_and_exact_available_coins() -> TestResult {
    use spellwake_application::account::{AccountSummary, DeckLibraryPage};
    let client = connection().await?;
    let (owner, session) = subject(&client).await?;
    let defaults = slots(&client).await?;
    let result = command(&client, owner, session, "read_owner", defaults).await?;
    let actor = Uuid::parse_str(result["data"]["accountId"].as_str().unwrap())?;
    client.batch_execute("RESET ROLE").await?;
    let (other, other_session) = subject(&client).await?;
    command(&client, other, other_session, "read_other", defaults).await?;
    client.batch_execute("RESET ROLE").await?;
    let format = Uuid::new_v4();
    client
        .execute(
            "INSERT INTO app.format(id,key,name) VALUES($1,$1::uuid::text,'Owned format')",
            &[&format],
        )
        .await?;
    client
        .query_one(
            "SELECT app.create_deck($1,'Owner deck','collection',$2,$3)",
            &[&actor, &format, &Uuid::new_v4()],
        )
        .await?;
    let operation = super::operation(&client, actor).await?;
    let owner_id: Uuid = client
        .query_one(
            "SELECT id FROM app.asset_owner WHERE account_id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    let hold = Uuid::new_v4();
    client.execute("INSERT INTO app.asset_hold(id,owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,'ante',$1::uuid::text,'held',$3)",&[&hold,&owner_id,&operation]).await?;
    client.execute("INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) SELECT $1,i.id,1000 FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.owner_id=$2 AND d.currency_key='coin'",&[&hold,&owner_id]).await?;
    super::set_auth_claims(&client, owner).await?;
    let value: Value = client
        .query_one("SELECT app.read_my_account()", &[])
        .await?
        .get(0);
    let summary: AccountSummary = serde_json::from_value(value["data"].clone())?;
    summary.validate()?;
    assert_eq!(summary.account_id, actor);
    assert_eq!(summary.wallet.owned.to_string(), "14500");
    assert_eq!(summary.wallet.held.to_string(), "1000");
    assert_eq!(summary.wallet.available.to_string(), "13500");
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE app.cosmetic SET display_name='Free account back' WHERE id=$1",
            &[&defaults[0]],
        )
        .await?;
    let paid = Uuid::new_v4();
    client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,display_name) VALUES($1,'dice','test-only:catalogue','active','Catalogue dice')", &[&paid]).await?;
    super::set_auth_claims(&client, owner).await?;
    let catalogue: Value = client
        .query_one("SELECT app.read_my_cosmetic_catalogue(NULL,100)", &[])
        .await?
        .get(0);
    let page: spellwake_application::store::StorePage<
        spellwake_application::store::CosmeticSummary,
    > = serde_json::from_value(catalogue["data"].clone())?;
    page.validate_after(None, 100)?;
    assert!(
        page.items
            .iter()
            .any(|item| item.id == defaults[0] && item.is_owned && item.is_free)
    );
    assert!(
        page.items
            .iter()
            .any(|item| item.id == paid && !item.is_owned && !item.is_free)
    );
    assert!(!catalogue.to_string().contains("asset_ref"));
    let purchases: Value = client
        .query_one("SELECT app.read_my_purchases(NULL,100)", &[])
        .await?
        .get(0);
    assert_eq!(purchases["data"]["items"], json!([]));
    assert!(value["data"].get("authSubject").is_none());
    assert!(
        client
            .query("SELECT auth_subject FROM app.account", &[])
            .await
            .is_err()
    );
    assert!(
        client
            .query("SELECT next_competitive_sequence FROM app.format", &[])
            .await
            .is_err()
    );
    assert_eq!(
        client
            .query_one("SELECT count(*) FROM app.account", &[])
            .await?
            .get::<_, i64>(0),
        1
    );
    let page: Value = client
        .query_one("SELECT app.read_my_deck_library(NULL,1)", &[])
        .await?
        .get(0);
    let page: DeckLibraryPage = serde_json::from_value(page["data"].clone())?;
    page.validate_after(1, None)?;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].name, "Owner deck");
    let next: Value = client
        .query_one(
            "SELECT app.read_my_deck_library($1,1)",
            &[&page.next_cursor],
        )
        .await?
        .get(0);
    assert_eq!(next["data"]["items"], json!([]));
    super::set_auth_claims(&client, other).await?;
    let page: Value = client
        .query_one("SELECT app.read_my_deck_library(NULL,100)", &[])
        .await?
        .get(0);
    assert_eq!(page["data"]["items"], json!([]));
    let original_claims: String = client
        .query_one("SELECT current_setting('request.jwt.claims')", &[])
        .await?
        .get(0);
    client.batch_execute("RESET ROLE").await?;
    client
        .execute("DELETE FROM auth.sessions WHERE id=$1", &[&other_session])
        .await?;
    client.batch_execute("SET ROLE authenticated").await?;
    client
        .query_one(
            "SELECT set_config('request.jwt.claims',$1,false)",
            &[&original_claims],
        )
        .await?;
    let revoked: Value = client
        .query_one("SELECT app.read_my_account()", &[])
        .await?
        .get(0);
    assert_eq!(revoked["status"], "fail");
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn registration_command_preserves_once_only_signup_and_named_username_conflicts() -> TestResult
{
    let client = connection().await?;
    let (owner, session) = subject(&client).await?;
    let defaults = slots(&client).await?;
    let username = format!("api_{}", &owner.simple().to_string()[..16]);
    let created = command(&client, owner, session, &username, defaults).await?;
    assert_eq!(created["status"], "success");
    let actor = Uuid::parse_str(created["data"]["accountId"].as_str().unwrap())?;
    assert_eq!(
        command(&client, owner, session, "different_username", defaults).await?,
        created
    );
    client.batch_execute("RESET ROLE").await?;
    let row=client.query_one("SELECT (SELECT count(*) FROM app.ledger_operation WHERE source_scope='signup' AND actor_account_id=$1),(SELECT sum(i.owned_quantity)::text FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE o.account_id=$1 AND d.kind='coin'),(SELECT count(*) FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE o.account_id=$1 AND d.kind IN ('card','pack'))",&[&actor]).await?;
    assert_eq!(row.get::<_, i64>(0), 1);
    assert_eq!(row.get::<_, String>(1), "14500");
    assert_eq!(row.get::<_, i64>(2), 0);
    for name in [username.clone(), username.to_uppercase()] {
        let (other, other_session) = subject(&client).await?;
        assert_eq!(
            command(&client, other, other_session, &name, defaults).await?,
            json!({"status":"fail","data":{"username":"unavailable"}})
        );
        client.batch_execute("RESET ROLE").await?;
        assert_eq!(
            client
                .query_one(
                    "SELECT count(*) FROM app.account WHERE auth_subject=$1",
                    &[&other]
                )
                .await?
                .get::<_, i64>(0),
            0
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn registration_command_classifies_only_known_causes_and_keeps_config_failures_as_errors()
-> TestResult {
    let client = connection().await?;
    let (actor, owner) = account(&client).await?;
    let claims: Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&owner])
        .await?
        .get(0);
    let session = Uuid::parse_str(claims["session_id"].as_str().unwrap())?;
    let defaults = slots(&client).await?;
    assert_eq!(
        command(&client, owner, Uuid::new_v4(), "irrelevant", defaults).await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    client.batch_execute("RESET ROLE").await?;
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=statement_timestamp() WHERE id=$1",&[&actor]).await?;
    assert_eq!(
        command(&client, owner, session, "irrelevant", [Uuid::nil(); 3]).await?,
        json!({"status":"fail","data":{"account":"unavailable"}})
    );
    client.batch_execute("RESET ROLE").await?;
    let (fresh, fresh_session) = subject(&client).await?;
    assert_eq!(
        command(
            &client,
            fresh,
            fresh_session,
            "valid_config_name",
            [Uuid::nil(); 3]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    // A different23514 must not be mistaken for the known lifecycle check.
    assert_eq!(
        command(&client, fresh, fresh_session, "x", defaults)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.account WHERE auth_subject=$1",
                &[&fresh]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    for role in ["anon", "authenticated", "supabase_auth_admin"] {
        client.batch_execute(&format!("SET ROLE {role}")).await?;
        assert!(
            client
                .query_one(
                    "SELECT app.register_account_command($1,$2,'forbidden',$3,$3,$3)",
                    &[&fresh, &fresh_session, &Uuid::nil()]
                )
                .await
                .is_err()
        );
        client.batch_execute("RESET ROLE").await?;
    }
    Ok(())
}
