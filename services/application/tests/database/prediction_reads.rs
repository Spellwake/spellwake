//! Actual caller-owned predictions and anonymous depth; synthetic terms do not qualify a feed.
use super::{TestResult, connection, pricing::buyer, set_auth_claims};
use serde_json::{Value, json};
use spellwake_application::prediction_reads::{MarketPage, OrderPage, OwnedMarket, PositionPage};
use tokio_postgres::Client;
use uuid::Uuid;
async fn read(
    client: &Client,
    subject: Uuid,
    sql: &str,
    market: Uuid,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    set_auth_claims(client, subject).await?;
    let result = client.query_one(sql, &[&market]).await;
    client.batch_execute("RESET ROLE").await?;
    Ok(result?.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn prediction_reads_derive_owner_and_keep_depth_anonymous() -> TestResult {
    let client = connection().await?;
    let (a, sa) = buyer(&client).await?;
    let (b, sb) = buyer(&client).await?;
    let (_, other) = buyer(&client).await?;
    let market = super::predictions::market(&client, a, 3600).await?;
    super::predictions::place(&client, a, market, "yes", "60", "3", Uuid::new_v4()).await?;
    super::predictions::place(&client, b, market, "no", "50", "2", Uuid::new_v4()).await?;
    let value = read(
        &client,
        other,
        "SELECT app.read_my_prediction_market($1)",
        market,
    )
    .await?;
    let detail: OwnedMarket = serde_json::from_value(value["data"].clone())?;
    detail.validate(market)?;
    assert_eq!(
        value["data"]["market"]["book"],
        json!([{"side":"yes","limitCost":"60","quantity":"1"}])
    );
    for forbidden in ["accountId", "creatorId", "holdId", "orders", "positions"] {
        assert!(value["data"]["market"].get(forbidden).is_none());
        assert!(value["data"]["market"]["terms"].get(forbidden).is_none());
    }
    let after = Uuid::from_u128(market.as_u128() - 1);
    let discovery = read(
        &client,
        other,
        "SELECT app.read_my_prediction_markets($1,1)",
        after,
    )
    .await?;
    let parsed: MarketPage = serde_json::from_value(discovery["data"].clone())?;
    parsed.validate(Some(after), 1)?;
    assert_eq!(parsed.items[0].id, market);
    assert_eq!(parsed.next_cursor, Some(market));
    let value = read(
        &client,
        sa,
        "SELECT app.read_my_prediction_orders($1,NULL,100)",
        market,
    )
    .await?;
    let orders: OrderPage = serde_json::from_value(value["data"].clone())?;
    orders.validate(None, 100, Some(market))?;
    assert_eq!(orders.account_id, a);
    assert_eq!(value["data"]["items"][0]["actualFilledCost"], "120");
    assert_eq!(value["data"]["items"][0]["unfilledReservation"], "60");
    let value = read(
        &client,
        sb,
        "SELECT app.read_my_prediction_positions($1,NULL,100)",
        market,
    )
    .await?;
    let positions: PositionPage = serde_json::from_value(value["data"].clone())?;
    positions.validate(None, 100, Some(market))?;
    assert_eq!(positions.account_id, b);
    assert_eq!(value["data"]["items"][0]["actualCost"], "80");
    assert_eq!(value["data"]["items"][0]["fixedPayout"], "100");
    for sql in [
        "SELECT app.read_my_prediction_orders($1,NULL,100)",
        "SELECT app.read_my_prediction_positions($1,NULL,100)",
    ] {
        assert_eq!(
            read(&client, other, sql, market).await?["data"]["items"],
            json!([])
        );
    }
    let missing = read(
        &client,
        other,
        "SELECT app.read_my_prediction_market($1)",
        Uuid::new_v4(),
    )
    .await?;
    assert_eq!(missing["data"]["market"], "unavailable");
    client
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 hour' WHERE id=$1",
            &[&other],
        )
        .await?;
    assert_eq!(
        read(
            &client,
            other,
            "SELECT app.read_my_prediction_market($1)",
            market
        )
        .await?["status"],
        "fail"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn prediction_read_function_roles_and_bounds_are_explicit() -> TestResult {
    let client = connection().await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for signature in [
            "app.read_my_prediction_market(uuid)",
            "app.read_my_prediction_markets(uuid,integer)",
            "app.read_my_prediction_orders(uuid,uuid,integer)",
            "app.read_my_prediction_positions(uuid,uuid,integer)",
            "app.read_prediction_market(uuid,uuid)",
            "app.read_prediction_market_page(uuid,uuid,integer)",
            "app.read_prediction_orders(uuid,uuid,uuid,integer)",
            "app.read_prediction_positions(uuid,uuid,uuid,integer)",
        ] {
            let allowed: bool = client
                .query_one(
                    "SELECT has_function_privilege($1,$2,'EXECUTE')",
                    &[&role, &signature],
                )
                .await?
                .get(0);
            assert_eq!(
                allowed,
                role == "authenticated" && signature.starts_with("app.read_my_"),
                "{role}:{signature}"
            );
        }
    }
    let (_, subject) = buyer(&client).await?;
    set_auth_claims(&client, subject).await?;
    for limit in [0i32, 101] {
        let value: Value = client
            .query_one("SELECT app.read_my_prediction_markets(NULL,$1)", &[&limit])
            .await?
            .get(0);
        assert_eq!(value["status"], "fail");
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn aggregate_prediction_depth_can_exceed_individual_balance_precision() -> TestResult {
    let client = connection().await?;
    let (a, sa) = buyer(&client).await?;
    let (b, _) = buyer(&client).await?;
    let market = super::predictions::market(&client, a, 3600).await?;
    let quantity = format!("6{}", "0".repeat(77));
    for actor in [a, b] {
        let operation = super::operation(&client, actor).await?;
        client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) SELECT $1,i.id,'credit',$3::text::numeric,'test',$1::uuid::text FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.owner_id=$2 AND d.currency_key='coin'",&[&operation,&actor,&quantity]).await?;
        client
            .execute(
                "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
                &[&operation],
            )
            .await?;
        assert_eq!(
            super::predictions::place(
                &client,
                actor,
                market,
                "yes",
                "1",
                &quantity,
                Uuid::new_v4()
            )
            .await?["status"],
            "success"
        );
    }
    let value = read(
        &client,
        sa,
        "SELECT app.read_my_prediction_market($1)",
        market,
    )
    .await?;
    let quantity = value["data"]["market"]["book"][0]["quantity"]
        .as_str()
        .unwrap();
    assert_eq!(quantity.len(), 79);
    let parsed: OwnedMarket = serde_json::from_value(value["data"].clone())?;
    parsed.validate(market)?;
    Ok(())
}
