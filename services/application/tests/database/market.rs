use super::{
    TestResult, connection,
    pricing::{buyer, product, publish},
};
use serde_json::{Value, json};
use spellwake_application::{ApiResponse, commands::CommandAck};
use tokio_postgres::{Client, types::ToSql};
use uuid::Uuid;

fn resource(value: Value) -> Uuid {
    match serde_json::from_value::<ApiResponse<CommandAck>>(value).unwrap() {
        ApiResponse::Success { data } => data.resource_id,
        other => panic!("expected marketplace acknowledgment: {other:?}"),
    }
}

// Canonical visibility tests retain fixture-owner access; caller ACLs are tested separately.
async fn canonical_read(
    client: &Client,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Value, tokio_postgres::Error> {
    Ok(client.query_one(sql, params).await?.get(0))
}

async fn service(
    client: &Client,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Value, tokio_postgres::Error> {
    client.batch_execute("SET ROLE service_role").await?;
    let result = client.query_one(sql, params).await;
    client.batch_execute("RESET ROLE").await?;
    Ok(result?.get(0))
}

pub(super) async fn stocked_seller(
    client: &Client,
) -> Result<(Uuid, Uuid, Uuid), Box<dyn std::error::Error + Send + Sync>> {
    let product = product(client, true).await?;
    publish(
        client,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let (seller, subject) = buyer(client).await?;
    let quote: Value = service(client, "SELECT app.quote_packs($1,2)", &[&product.recipe]).await?;
    let policy: Uuid = quote["policyId"].as_str().unwrap().parse()?;
    resource(
        economy(client,seller,&json!({"kind":"pack_buy","command":{"requestId":Uuid::new_v4(),"recipeId":product.recipe,"quantity":"2","expectedPolicyId":policy,"expectedRevision":quote["revision"],"expectedTotal":quote["total"]}}))
        .await?,
    );
    let asset: Uuid = client
        .query_one(
            "SELECT id FROM app.asset_definition WHERE recipe_id=$1",
            &[&product.recipe],
        )
        .await?
        .get(0);
    Ok((seller, subject, asset))
}

pub(super) async fn list(
    client: &Client,
    seller: Uuid,
    asset: Uuid,
    quantity: &str,
    price: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    economy(client,seller,&json!({"kind":"listing_create","command":{"requestId":request,"lot":{"assetId":asset,"quantity":quantity},"totalPrice":price}}))
    .await
}
pub(super) async fn buy(
    client: &Client,
    buyer: Uuid,
    listing: Uuid,
    price: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    economy(client,buyer,&json!({"kind":"listing_buy","command":{"requestId":request,"listingId":listing,"expectedRevision":"1","expectedTotal":price}}))
    .await
}
async fn cancel(
    client: &Client,
    seller: Uuid,
    listing: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    economy(client,seller,&json!({"kind":"listing_cancel","command":{"requestId":Uuid::new_v4(),"listingId":listing}}))
    .await
}
async fn coins(client: &Client, account: Uuid) -> Result<String, tokio_postgres::Error> {
    Ok(client.query_one("SELECT i.owned_quantity::text FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE owner_id=$1 AND d.currency_key='coin'", &[&account]).await?.get(0))
}

async fn economy(
    client: &Client,
    actor: Uuid,
    input: &Value,
) -> Result<Value, tokio_postgres::Error> {
    client.batch_execute("SET ROLE service_role").await?;
    let result = super::economy_commands::execute_for_actor(client, actor, input, false).await;
    client.batch_execute("RESET ROLE").await?;
    result
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn whole_lot_sale_reserves_once_conserves_assets_and_does_not_require_contact() -> TestResult
{
    let client = connection().await?;
    let (seller, seller_subject, asset) = stocked_seller(&client).await?;
    let (buyer, _) = buyer(&client).await?;
    let session: Uuid = client
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&seller_subject],
        )
        .await?
        .get(0);
    resource(
        service(
            &client,
            "SELECT app.execute_social_command($1,$2,$3)",
            &[&seller_subject, &session, &json!({"kind":"block","command":{"requestId":Uuid::new_v4(),"otherAccountId":buyer,"blocked":true}})],
        )
        .await?,
    );
    let request = Uuid::new_v4();
    let created = list(&client, seller, asset, "2", "3000", request).await?;
    let listing = resource(created.clone());
    assert_eq!(
        created,
        list(&client, seller, asset, "2", "3000", request).await?
    );
    assert_eq!(
        list(&client, seller, asset, "2", "3001", request).await?,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    assert_eq!(
        list(&client, seller, asset, "1", "1500", Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(client.query_one("SELECT a.available::text FROM app.asset_availability a JOIN app.inventory_asset i ON i.id=a.id WHERE i.owner_id=$1 AND i.asset_definition_id=$2", &[&seller,&asset]).await?.get::<_,String>(0),"0");
    assert!(client.execute("UPDATE app.asset_hold SET state='released' WHERE id=(SELECT hold_id FROM app.market_listing WHERE id=$1)", &[&listing]).await.is_err());
    assert!(
        client
            .execute(
                "UPDATE app.market_listing SET total_price_coins=1 WHERE id=$1",
                &[&listing]
            )
            .await
            .is_err()
    );
    let buy_request = Uuid::new_v4();
    let bought = buy(&client, buyer, listing, "3000", buy_request).await?;
    resource(bought.clone());
    assert_eq!(
        bought,
        buy(&client, buyer, listing, "3000", buy_request).await?
    );
    assert_eq!(coins(&client, seller).await?, "15250");
    assert_eq!(coins(&client, buyer).await?, "11500");
    for (owner, quantity) in [(seller, "0"), (buyer, "2")] {
        assert_eq!(client.query_one("SELECT owned_quantity::text FROM app.inventory_asset WHERE owner_id=$1 AND asset_definition_id=$2", &[&owner,&asset]).await?.get::<_,String>(0),quantity);
    }
    assert_eq!(client.query_one("SELECT count(*) FROM app.ledger_operation WHERE source_scope='market_purchase' AND source_ref=$1", &[&listing.to_string()]).await?.get::<_,i64>(0),1);
    assert_eq!(cancel(&client, seller, listing).await?["status"], "fail");
    client.batch_execute("SET ROLE service_role").await?;
    for sql in [
        "INSERT INTO app.ledger_operation(id) VALUES($1)",
        "INSERT INTO app.ledger_posting(id) VALUES($1)",
    ] {
        let error = client.execute(sql, &[&Uuid::new_v4()]).await.unwrap_err();
        assert_eq!(
            error.code(),
            Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
        );
    }
    client.batch_execute("RESET ROLE").await?;

    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn market_restrictions_apply_to_both_sides_and_cancellation_is_required_to_reprice()
-> TestResult {
    let client = connection().await?;
    let (seller, _, asset) = stocked_seller(&client).await?;
    let (buyer, _) = buyer(&client).await?;
    // Current pack value is1500; two whole packs are3000. Seller cannot give
    // more than130% of its incoming payment, and buyer cannot overpay either.
    for price in ["2000", "3901"] {
        let listing = resource(list(&client, seller, asset, "2", price, Uuid::new_v4()).await?);
        assert_eq!(
            buy(&client, buyer, listing, price, Uuid::new_v4()).await?["status"],
            "fail"
        );
        resource(cancel(&client, seller, listing).await?);
    }
    let listing = resource(list(&client, seller, asset, "2", "3900", Uuid::new_v4()).await?);
    assert_eq!(
        buy(&client, seller, listing, "3900", Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(
        buy(&client, buyer, listing, "3899", Uuid::new_v4()).await?["status"],
        "fail"
    );
    resource(buy(&client, buyer, listing, "3900", Uuid::new_v4()).await?);
    assert_eq!(coins(&client, buyer).await?, "10600");
    let coin: Uuid = client
        .query_one(
            "SELECT id FROM app.asset_definition WHERE currency_key='coin'",
            &[],
        )
        .await?
        .get(0);
    assert_eq!(
        list(&client, seller, coin, "1", "1", Uuid::new_v4()).await?["status"],
        "fail"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn two_buyers_and_cancel_purchase_races_have_one_terminal_winner() -> TestResult {
    let client = connection().await?;
    let (seller, _, asset) = stocked_seller(&client).await?;
    let (left, _) = buyer(&client).await?;
    let (right, _) = buyer(&client).await?;
    let listing = resource(list(&client, seller, asset, "1", "1500", Uuid::new_v4()).await?);
    let other = connection().await?;
    let (a, b) = tokio::join!(
        buy(&client, left, listing, "1500", Uuid::new_v4()),
        buy(&other, right, listing, "1500", Uuid::new_v4())
    );
    let statuses = [
        a?["status"].as_str().unwrap().to_owned(),
        b?["status"].as_str().unwrap().to_owned(),
    ];
    assert_eq!(statuses.iter().filter(|s| *s == "success").count(), 1);
    assert_eq!(statuses.iter().filter(|s| *s == "fail").count(), 1);
    let second = resource(list(&client, seller, asset, "1", "1500", Uuid::new_v4()).await?);
    let (a, b) = tokio::join!(
        buy(&client, left, second, "1500", Uuid::new_v4()),
        cancel(&other, seller, second)
    );
    let statuses = [
        a?["status"].as_str().unwrap().to_owned(),
        b?["status"].as_str().unwrap().to_owned(),
    ];
    assert_eq!(statuses.iter().filter(|s| *s == "success").count(), 1);
    assert_eq!(statuses.iter().filter(|s| *s == "fail").count(), 1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn canonical_market_reads_hide_commitment_and_terminal_party_details_and_page_stably()
-> TestResult {
    let client = connection().await?;
    let (seller, subject, asset) = stocked_seller(&client).await?;
    let (buyer, buyer_subject) = buyer(&client).await?;
    let (other, _) = super::account(&client).await?;
    let first = resource(list(&client, seller, asset, "1", "1500", Uuid::new_v4()).await?);
    let second = resource(list(&client, seller, asset, "1", "1500", Uuid::new_v4()).await?);
    let visible = canonical_read(
        &client,
        "SELECT app.read_market_listing($1,$2)",
        &[&buyer, &first],
    )
    .await?;
    assert!(visible.get("buyerId").is_none());
    assert!(visible.get("assessment").is_none());
    assert!(visible.get("holdId").is_none());
    let page = canonical_read(
        &client,
        "SELECT app.read_market_page($1,NULL,1,true)",
        &[&seller],
    )
    .await?;
    let after: Uuid = page["nextCursor"].as_str().unwrap().parse()?;
    let next = canonical_read(
        &client,
        "SELECT app.read_market_page($1,$2,1,true)",
        &[&seller, &after],
    )
    .await?;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(next["items"].as_array().unwrap().len(), 1);
    assert_ne!(page["items"][0]["id"], next["items"][0]["id"]);
    assert!(
        canonical_read(
            &client,
            "SELECT app.read_market_page($1,NULL,101,false)",
            &[&seller]
        )
        .await
        .is_err()
    );
    resource(buy(&client, buyer, first, "1500", Uuid::new_v4()).await?);
    assert!(
        canonical_read(
            &client,
            "SELECT app.read_market_listing($1,$2)",
            &[&other, &first]
        )
        .await
        .is_err()
    );
    let owned = canonical_read(
        &client,
        "SELECT app.read_market_listing($1,$2)",
        &[&buyer, &first],
    )
    .await?;
    assert_eq!(owned["buyerId"], buyer.to_string());
    assert_eq!(owned["paidCoins"], "1500");
    assert!(owned["assessment"].is_object());
    client.batch_execute("SET ROLE authenticated").await?;
    super::set_auth_claims(&client, buyer_subject).await?;
    assert!(
        client
            .query_one(
                "SELECT assessment FROM app.market_listing WHERE id=$1",
                &[&first]
            )
            .await
            .is_err()
    );
    assert_eq!(
        client
            .query(
                "SELECT id FROM app.market_listing WHERE id IN ($1,$2)",
                &[&first, &second]
            )
            .await?
            .len(),
        2
    );
    super::set_auth_claims(&client, subject).await?;
    assert_eq!(
        client
            .query("SELECT id FROM app.market_listing WHERE id=$1", &[&first])
            .await?
            .len(),
        1
    );
    client.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert!(
        client
            .query("SELECT id FROM app.market_listing", &[])
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn card_lots_and_held_buyer_funds_obey_the_same_conserved_settlement() -> TestResult {
    let client = connection().await?;
    let (seller, _, pack) = stocked_seller(&client).await?;
    let (buyer, _) = buyer(&client).await?;
    let card: Uuid = client.query_one("SELECT a.id FROM app.asset_definition p JOIN app.booster_slot s ON s.recipe_id=p.recipe_id JOIN app.booster_slot_card c ON c.slot_id=s.id JOIN app.asset_definition a ON a.printing_id=c.printing_id WHERE p.id=$1 AND c.weight=3", &[&pack]).await?.get(0);
    let inventory: Uuid = client
        .query_one("SELECT app.ensure_inventory($1,$2)", &[&seller, &card])
        .await?
        .get(0);
    let operation = super::operation(&client, seller).await?;
    client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'credit',3,'test',$1::uuid::text)", &[&operation,&inventory]).await?;
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&operation],
        )
        .await?;
    let listing = resource(list(&client, seller, card, "2", "2000", Uuid::new_v4()).await?);
    let coin_inventory: Uuid = client.query_one("SELECT i.id FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.owner_id=$1 AND d.currency_key='coin'", &[&buyer]).await?.get(0);
    let reserve = super::operation(&client, buyer).await?;
    let hold = Uuid::new_v4();
    client.execute("INSERT INTO app.asset_hold(id,owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,'ante',$1::uuid::text,'held',$3)", &[&hold,&buyer,&reserve]).await?;
    client
        .execute(
            "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,14000)",
            &[&hold, &coin_inventory],
        )
        .await?;
    let request = Uuid::new_v4();
    assert_eq!(
        buy(&client, buyer, listing, "2000", request).await?["status"],
        "fail"
    );
    client
        .execute(
            "UPDATE app.asset_hold SET state='released' WHERE id=$1",
            &[&hold],
        )
        .await?;
    assert_eq!(
        buy(&client, buyer, listing, "2000", request).await?["status"],
        "fail"
    );
    resource(buy(&client, buyer, listing, "2000", Uuid::new_v4()).await?);
    assert_eq!(client.query_one("SELECT owned_quantity::text FROM app.inventory_asset WHERE owner_id=$1 AND asset_definition_id=$2", &[&seller,&card]).await?.get::<_,String>(0),"1");
    assert_eq!(client.query_one("SELECT owned_quantity::text FROM app.inventory_asset WHERE owner_id=$1 AND asset_definition_id=$2", &[&buyer,&card]).await?.get::<_,String>(0),"2");
    assert_eq!(coins(&client, buyer).await?, "12500");
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn waiting_market_buyer_rechecks_state_after_cancellation_commits() -> TestResult {
    let mut client = connection().await?;
    let (seller, _, asset) = stocked_seller(&client).await?;
    let (buyer, _) = buyer(&client).await?;
    let listing = resource(list(&client, seller, asset, "2", "3000", Uuid::new_v4()).await?);
    let transaction = client.transaction().await?;
    transaction
        .query_one(
            "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
            &[&seller],
        )
        .await?;
    let writer = connection().await?;
    let observer = connection().await?;
    let name = format!("market-cancel-wait-{}", Uuid::new_v4());
    writer
        .query_one("SELECT set_config('application_name',$1,false)", &[&name])
        .await?;
    let task =
        tokio::spawn(async move { buy(&writer, buyer, listing, "3000", Uuid::new_v4()).await });
    let mut blocked = false;
    for _ in 0..100 {
        blocked = observer.query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock')", &[&name]).await?.get(0);
        if blocked {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        blocked,
        "buyer should wait on seller's cancellation transaction"
    );
    transaction
        .batch_execute("SET LOCAL ROLE service_role")
        .await?;
    resource(
        super::economy_commands::execute_for_actor(&transaction,seller,&json!({"kind":"listing_cancel","command":{"requestId":Uuid::new_v4(),"listingId":listing}}),false).await?,
    );
    transaction.commit().await?;
    assert_eq!(task.await??["status"], "fail");
    assert_eq!(coins(&client, buyer).await?, "14500");
    assert_eq!(
        client
            .query_one(
                "SELECT state FROM app.market_listing WHERE id=$1",
                &[&listing]
            )
            .await?
            .get::<_, String>(0),
        "canceled"
    );
    Ok(())
}
