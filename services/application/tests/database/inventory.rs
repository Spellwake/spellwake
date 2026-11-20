use super::{TestResult, account, connection, operation, packs};
use serde_json::{Value, json};
use spellwake_application::inventory::{AssetIdentity, InventoryPage};
use tokio_postgres::Client;
use uuid::Uuid;

async fn read_for(
    client: &Client,
    actor: Uuid,
    after: Option<Uuid>,
    limit: i32,
) -> Result<Value, tokio_postgres::Error> {
    client.batch_execute("RESET ROLE").await?;
    let claims: Value = client
        .query_one(
            "SELECT test_fixture.claims_for_subject(auth_subject) FROM app.account WHERE id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    client.batch_execute("SET ROLE authenticated").await?;
    client
        .query_one(
            "SELECT set_config('request.jwt.claims',$1,false)",
            &[&claims.to_string()],
        )
        .await?;
    Ok(client
        .query_one(
            "SELECT app.read_my_inventory_page($1,$2)",
            &[&after, &limit],
        )
        .await?
        .get(0))
}
async fn page_for(
    client: &Client,
    actor: Uuid,
    after: Option<Uuid>,
    limit: i32,
) -> Result<InventoryPage, Box<dyn std::error::Error + Send + Sync>> {
    let value = read_for(client, actor, after, limit).await?;
    assert_eq!(value["status"], "success");
    let page: InventoryPage = serde_json::from_value(value["data"].clone())?;
    page.validate(usize::try_from(limit)?)?;
    assert!(
        page.items
            .iter()
            .all(|item| after.is_none_or(|cursor| item.id > cursor))
    );
    Ok(page)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn inventory_pages_project_exact_owner_counts_across_hold_open_and_release() -> TestResult {
    let client = connection().await?;
    let (product, actor, inventory) = packs::purchased(&client, 2).await?;
    let before_hold: String = client
        .query_one(
            "SELECT revision::text FROM app.inventory_asset WHERE id=$1",
            &[&inventory],
        )
        .await?
        .get(0);
    let (other, _) = account(&client).await?;
    let operation = operation(&client, actor).await?;
    let hold = Uuid::new_v4();
    client.execute("INSERT INTO app.asset_hold(id,owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,'ante',$1::uuid::text,'held',$3)",&[&hold,&actor,&operation]).await?;
    client
        .execute(
            "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,1)",
            &[&hold, &inventory],
        )
        .await?;
    let page = page_for(&client, actor, None, 100).await?;
    assert_eq!(page.items.len(), 2);
    let pack = page.items.iter().find(|item| item.id == inventory).unwrap();
    assert_eq!(
        (
            pack.owned.to_string(),
            pack.held.to_string(),
            pack.available.to_string()
        ),
        ("2".into(), "1".into(), "1".into())
    );
    assert!(matches!(pack.asset,AssetIdentity::Pack{recipe_id} if recipe_id==product.recipe));
    let revision = pack.revision.clone();
    assert!(revision > before_hold.parse()?);
    assert!(page_for(&client, other, None, 100).await?.items.is_empty());
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    let opened: Value = super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_open","command":{"requestId":Uuid::new_v4(),"inventoryAssetId":inventory}}), false)
        .await?
        .get(0);
    assert_eq!(opened["status"], "success");
    let page = page_for(&client, actor, None, 100).await?;
    assert_eq!(page.items.len(), 3);
    let pack = page.items.iter().find(|item| item.id == inventory).unwrap();
    assert_eq!(
        (
            pack.owned.to_string(),
            pack.held.to_string(),
            pack.available.to_string()
        ),
        ("1".into(), "1".into(), "0".into())
    );
    assert!(pack.revision > revision);
    let before_release = pack.revision.clone();
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    let denied: Value = super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_open","command":{"requestId":Uuid::new_v4(),"inventoryAssetId":inventory}}), false)
        .await?
        .get(0);
    assert_eq!(denied["status"], "fail");
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE app.asset_hold SET state='released' WHERE id=$1",
            &[&hold],
        )
        .await?;
    let page = page_for(&client, actor, None, 100).await?;
    let pack = page.items.iter().find(|item| item.id == inventory).unwrap();
    assert!(pack.revision > before_release);
    assert_eq!(pack.held.to_string(), "0");
    assert_eq!(pack.available.to_string(), "1");
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    let opened: Value = super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_open","command":{"requestId":Uuid::new_v4(),"inventoryAssetId":inventory}}), false)
        .await?
        .get(0);
    assert_eq!(opened["status"], "success");
    let mut after = None;
    let mut rows = Vec::new();
    loop {
        let page = page_for(&client, actor, after, 1).await?;
        after = page.next_cursor;
        rows.extend(page.items);
        if after.is_none() {
            break;
        }
    }
    assert!(rows.iter().all(|item| item.id != inventory));
    assert!(rows.windows(2).all(|pair| pair[0].id < pair[1].id));
    for limit in [0i32, 101] {
        assert_eq!(
            read_for(&client, actor, None, limit).await?,
            json!({"status":"fail","data":{"limit":"must_be_between_1_and_100"}})
        );
    }
    client.batch_execute("RESET ROLE").await?;
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&actor]).await?;
    assert_eq!(
        page_for(&client, actor, None, 100).await?.items.len(),
        rows.len()
    );
    client
        .query_one("SELECT set_config('request.jwt.claims','{}',false)", &[])
        .await?;
    assert_eq!(
        client
            .query_one("SELECT app.read_my_inventory_page(NULL,100)", &[])
            .await?
            .get::<_, Value>(0),
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    client.batch_execute("RESET ROLE").await?;
    for role in ["anon", "service_role", "supabase_auth_admin"] {
        client.batch_execute(&format!("SET ROLE {role}")).await?;
        assert!(
            client
                .query_one("SELECT app.read_my_inventory_page(NULL,100)", &[])
                .await
                .is_err()
        );
        client.batch_execute("RESET ROLE").await?;
    }
    let row=client.query_one("SELECT to_regprocedure('app.read_inventory_page(uuid,uuid,integer)') IS NULL,NOT prosecdef FROM pg_proc WHERE oid='app.read_my_inventory_page(uuid,integer)'::regprocedure",&[]).await?;
    assert!(row.get::<_, bool>(0));
    assert!(row.get::<_, bool>(1));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn inventory_read_distinguishes_unregistered_revoked_and_deleted_authority() -> TestResult {
    let client = connection().await?;
    let fresh = Uuid::new_v4();
    client
        .execute("INSERT INTO auth.users(id) VALUES($1)", &[&fresh])
        .await?;
    super::seed_auth_authority(&client, fresh).await?;
    super::set_auth_claims(&client, fresh).await?;
    let read = || async {
        Ok::<Value, tokio_postgres::Error>(
            client
                .query_one("SELECT app.read_my_inventory_page(NULL,100)", &[])
                .await?
                .get(0),
        )
    };
    assert_eq!(
        read().await?,
        json!({"status":"fail","data":{"account":"unavailable"}})
    );
    client.batch_execute("RESET ROLE").await?;
    let (actor, subject) = account(&client).await?;
    super::set_auth_claims(&client, subject).await?;
    assert_eq!(read().await?["status"], "success");
    client.batch_execute("RESET ROLE").await?;
    // Keep the original unexpired claims while deleting their native session.
    client
        .execute("DELETE FROM auth.sessions WHERE user_id=$1", &[&subject])
        .await?;
    client.batch_execute("SET ROLE authenticated").await?;
    assert_eq!(
        read().await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    client.batch_execute("RESET ROLE").await?;
    let (deleted, deleted_subject) = account(&client).await?;
    super::set_auth_claims(&client, deleted_subject).await?;
    client.batch_execute("RESET ROLE").await?;
    let operation = Uuid::new_v4();
    client.execute("INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES($1,'retirement',$2::uuid::text,'account_retirement',$2,statement_timestamp(),'committed')",&[&operation,&deleted]).await?;
    client.execute("UPDATE app.account SET state='deleted',deletion_requested_at=statement_timestamp(),deletion_finalized_at=statement_timestamp(),retirement_operation_id=$2 WHERE id=$1",&[&deleted,&operation]).await?;
    client.batch_execute("SET ROLE authenticated").await?;
    assert_eq!(
        read().await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert!(
        client
            .query_one("SELECT app.read_my_inventory_page($1,NULL,100)", &[&actor])
            .await
            .is_err()
    );
    Ok(())
}
