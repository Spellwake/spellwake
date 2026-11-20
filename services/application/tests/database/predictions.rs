use super::{
    TestResult, connection,
    pricing::{buyer, product},
};
use serde_json::{Value, json};
use spellwake_application::{ApiResponse, commands::CommandAck};
use tokio_postgres::{Client, types::ToSql};
use uuid::Uuid;

fn resource(value: Value) -> Uuid {
    match serde_json::from_value::<ApiResponse<CommandAck>>(value).unwrap() {
        ApiResponse::Success { data } => data.resource_id,
        other => panic!("expected prediction acknowledgment: {other:?}"),
    }
}
async fn canonical_read(
    client: &Client,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Value, tokio_postgres::Error> {
    // Fixture-owner canonical semantics only; caller/role projection proof is separate.
    assert_eq!(
        client
            .query_one("SELECT current_user::text", &[])
            .await?
            .get::<_, String>(0),
        "postgres"
    );
    Ok(client.query_one(sql, params).await?.get(0))
}
// Owner-only synthetic fixture. This does not qualify a price source or publish a real prediction.
pub(super) async fn market(
    client: &Client,
    creator: Uuid,
    cutoff: i32,
) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
    let product = product(client, false).await?;
    let printing: Uuid = client
        .query_one(
            "SELECT printing_id FROM app.asset_definition WHERE id=$1",
            &[&product.cards[0]],
        )
        .await?
        .get(0);
    let id = Uuid::new_v4();
    client.execute("INSERT INTO app.prediction_market(id,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,deadline,trading_closes_at,observation_starts_at,sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,next_order_sequence,state) VALUES($1,$2,$3,'test-only:usd','10.00','reached_by',clock_timestamp()+interval '2 days',clock_timestamp()+$4::integer*interval '1 second',clock_timestamp()+interval '1 day','test-only:sampling','test-only:terminal','test-only:v1',100,1,'open')", &[&id,&creator,&printing,&cutoff]).await?;
    Ok(id)
}
pub(super) async fn place(
    client: &Client,
    actor: Uuid,
    market: Uuid,
    side: &str,
    limit: &str,
    quantity: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    super::prediction_commands::execute_for_actor(client,actor,&json!({"kind":"place_order","command":{"requestId":request,"marketId":market,"side":side,"limitCost":limit,"quantity":quantity}}),false).await
}
async fn cancel(client: &Client, actor: Uuid, order: Uuid) -> Result<Value, tokio_postgres::Error> {
    super::prediction_commands::execute_for_actor(
        client,
        actor,
        &json!({"kind":"cancel_order","command":{"requestId":Uuid::new_v4(),"orderId":order}}),
        false,
    )
    .await
}

async fn available(client: &Client, actor: Uuid) -> Result<String, tokio_postgres::Error> {
    Ok(client.query_one("SELECT a.available::text FROM app.asset_availability a JOIN app.inventory_asset i ON i.id=a.id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.owner_id=$1 AND d.currency_key='coin'", &[&actor]).await?.get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn partial_fills_keep_actual_backing_and_cancel_only_unfilled_units() -> TestResult {
    let client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let (bob, _) = buyer(&client).await?;
    let market = market(&client, alice, 3600).await?;
    let before_a: i64 = available(&client, alice).await?.parse()?;
    let before_b: i64 = available(&client, bob).await?.parse()?;
    let yes = resource(place(&client, alice, market, "yes", "60", "3", Uuid::new_v4()).await?);
    let no = resource(place(&client, bob, market, "no", "50", "2", Uuid::new_v4()).await?);
    assert_eq!(
        available(&client, alice).await?,
        (before_a - 180).to_string()
    );
    assert_eq!(available(&client, bob).await?, (before_b - 80).to_string());
    let pair=client.query_one("SELECT quantity::text,yes_actual_cost_coins::text,no_actual_cost_coins::text FROM app.prediction_pair WHERE market_id=$1", &[&market]).await?;
    assert_eq!(
        (
            pair.get::<_, String>(0),
            pair.get::<_, String>(1),
            pair.get::<_, String>(2)
        ),
        ("2".into(), "120".into(), "80".into())
    );
    resource(cancel(&client, alice, yes).await?);
    assert_eq!(
        available(&client, alice).await?,
        (before_a - 120).to_string()
    );
    assert_eq!(cancel(&client, bob, no).await?["status"], "fail");
    assert!(client.execute("UPDATE app.asset_hold SET state='released' WHERE id=(SELECT hold_id FROM app.prediction_order WHERE id=$1)", &[&no]).await.is_err());
    assert!(client.execute("UPDATE app.hold_line SET quantity=1 WHERE hold_id=(SELECT hold_id FROM app.prediction_order WHERE id=$1)", &[&yes]).await.is_err());
    assert_eq!(client.query_one("SELECT count(*) FROM app.ledger_posting WHERE operation_id IN (SELECT match_operation_id FROM app.prediction_pair WHERE market_id=$1)", &[&market]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn matching_uses_best_price_then_oldest_and_skips_own_orders() -> TestResult {
    let client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let (bob, _) = buyer(&client).await?;
    let (carol, _) = buyer(&client).await?;
    let market = market(&client, alice, 3600).await?;
    let lower = resource(place(&client, bob, market, "no", "50", "1", Uuid::new_v4()).await?);
    let best = resource(place(&client, carol, market, "no", "60", "1", Uuid::new_v4()).await?);
    let later = resource(place(&client, bob, market, "no", "60", "1", Uuid::new_v4()).await?);
    let own = resource(place(&client, alice, market, "no", "99", "1", Uuid::new_v4()).await?);
    let yes = resource(place(&client, alice, market, "yes", "50", "2", Uuid::new_v4()).await?);
    let rows=client.query("SELECT no_order_id,yes_actual_cost_coins::text FROM app.prediction_pair WHERE yes_order_id=$1 ORDER BY matched_at,id", &[&yes]).await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<_, Uuid>(0), best);
    assert_eq!(rows[1].get::<_, Uuid>(0), later);
    assert!(rows.iter().all(|row| row.get::<_, String>(1) == "40"));
    for id in [lower, own] {
        assert_eq!(
            client
                .query_one(
                    "SELECT filled_quantity::text FROM app.prediction_order WHERE id=$1",
                    &[&id]
                )
                .await?
                .get::<_, String>(0),
            "0"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn order_receipts_bind_terms_and_reject_cutoff_precision_and_unavailable_funds() -> TestResult
{
    let client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let market = market(&client, alice, 3600).await?;
    let request = Uuid::new_v4();
    let result = place(&client, alice, market, "yes", "60", "1", request).await?;
    resource(result.clone());
    assert_eq!(
        result,
        place(&client, alice, market, "yes", "60", "1", request).await?
    );
    assert_eq!(
        place(&client, alice, market, "no", "60", "1", request).await?["data"]["request"],
        "conflict"
    );
    for (limit, quantity) in [
        ("0", "1"),
        ("100", "1"),
        ("1", "0"),
        ("1", "01"),
        ("1.0", "1"),
        (
            "99",
            "999999999999999999999999999999999999999999999999999999999999999999999999999999",
        ),
    ] {
        assert_eq!(
            place(
                &client,
                alice,
                market,
                "yes",
                limit,
                quantity,
                Uuid::new_v4()
            )
            .await?["status"],
            "fail"
        );
    }
    let closed = self::market(&client, alice, -1).await?;
    assert_eq!(
        place(&client, alice, closed, "yes", "50", "1", Uuid::new_v4()).await?["status"],
        "fail"
    );
    let response = client
        .query_one("SELECT app.close_prediction_trading($1)", &[&closed])
        .await?;
    assert!(response.get::<_, bool>(0));
    assert!(
        !client
            .query_one("SELECT app.close_prediction_trading($1)", &[&closed])
            .await?
            .get::<_, bool>(0)
    );
    client.batch_execute("SET ROLE service_role").await?;
    assert!(
        client
            .execute(
                "UPDATE app.prediction_market SET state='closed' WHERE id=$1",
                &[&market]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "DELETE FROM app.prediction_order WHERE market_id=$1",
                &[&market]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn cutoff_closes_unfilled_reservations_once_but_preserves_positions() -> TestResult {
    let client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let (bob, _) = buyer(&client).await?;
    let market = market(&client, alice, 2).await?;
    let yes = resource(place(&client, alice, market, "yes", "60", "3", Uuid::new_v4()).await?);
    resource(place(&client, bob, market, "no", "50", "1", Uuid::new_v4()).await?);
    let wait: f64 = client.query_one("SELECT greatest(0,extract(epoch FROM trading_closes_at-clock_timestamp()))::double precision FROM app.prediction_market WHERE id=$1", &[&market]).await?.get(0);
    tokio::time::sleep(std::time::Duration::from_secs_f64(wait + 0.02)).await;
    assert!(
        client
            .query_one("SELECT app.close_prediction_trading($1)", &[&market])
            .await?
            .get::<_, bool>(0)
    );
    assert!(
        !client
            .query_one("SELECT app.close_prediction_trading($1)", &[&market])
            .await?
            .get::<_, bool>(0)
    );
    assert_eq!(available(&client, alice).await?, "14440");
    assert_eq!(available(&client, bob).await?, "14460");
    assert_eq!(
        client
            .query_one(
                "SELECT state FROM app.prediction_order WHERE id=$1",
                &[&yes]
            )
            .await?
            .get::<_, String>(0),
        "closed"
    );
    assert_eq!(cancel(&client, alice, yes).await?["status"], "fail");
    assert_eq!(
        place(&client, alice, market, "yes", "50", "1", Uuid::new_v4()).await?["status"],
        "fail"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn stale_participant_snapshot_requires_retry_instead_of_matching_unlocked_accounts()
-> TestResult {
    let client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let (bob, _) = buyer(&client).await?;
    let market = market(&client, alice, 3600).await?;
    let mut stale = connection().await?;
    let transaction = stale
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    transaction
        .query_one(
            "SELECT orders_revision::text FROM app.prediction_market WHERE id=$1",
            &[&market],
        )
        .await?;
    resource(place(&client, bob, market, "no", "50", "1", Uuid::new_v4()).await?);
    // Fixture-owner canonical transition deliberately preserves this source-snapshot test.
    // Public session facades separately reject non-READ-COMMITTED callers.

    let error = transaction
        .query_one(
            "SELECT app.place_prediction_order($1,$2,'yes','50','1',$3)",
            &[&alice, &market, &Uuid::new_v4()],
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    transaction.rollback().await?;
    resource(place(&client, alice, market, "yes", "50", "1", Uuid::new_v4()).await?);
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.prediction_pair WHERE market_id=$1",
                &[&market]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn waiting_order_cannot_fill_units_after_their_cancellation_commits() -> TestResult {
    let mut client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let (bob, _) = buyer(&client).await?;
    let market = market(&client, alice, 3600).await?;
    let yes = resource(place(&client, alice, market, "yes", "60", "1", Uuid::new_v4()).await?);
    let transaction = client.transaction().await?;
    transaction
        .query_one(
            "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
            &[&alice],
        )
        .await?;
    let writer = connection().await?;
    let observer = connection().await?;
    let name = format!("prediction-cancel-wait-{}", Uuid::new_v4());
    writer
        .query_one("SELECT set_config('application_name',$1,false)", &[&name])
        .await?;
    let task =
        tokio::spawn(
            async move { place(&writer, bob, market, "no", "50", "1", Uuid::new_v4()).await },
        );
    let mut blocked = false;
    for _ in 0..100 {
        blocked=observer.query_one("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock')", &[&name]).await?.get(0);
        if blocked {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(blocked, "order writer must visibly wait for cancellation");
    // Fixture-owner canonical transition deliberately preserves this source-snapshot test.
    // Public session facades separately reject non-READ-COMMITTED callers.

    resource(
        transaction
            .query_one(
                "SELECT app.cancel_prediction_order($1,$2,$3)",
                &[&alice, &yes, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    transaction.commit().await?;
    let error = task.await?.unwrap_err();
    assert_eq!(
        error.code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    resource(place(&client, bob, market, "no", "50", "1", Uuid::new_v4()).await?);
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.prediction_pair WHERE market_id=$1",
                &[&market]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    assert_eq!(available(&client, alice).await?, "14500");
    assert_eq!(available(&client, bob).await?, "14450");
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn canonical_depth_and_owner_commitment_projections_preserve_matching_semantics() -> TestResult
{
    let client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let (bob, _) = buyer(&client).await?;
    let (other, _) = buyer(&client).await?;
    let market = market(&client, alice, 3600).await?;
    let yes = resource(place(&client, alice, market, "yes", "60", "3", Uuid::new_v4()).await?);
    resource(place(&client, bob, market, "no", "50", "1", Uuid::new_v4()).await?);
    let public = canonical_read(
        &client,
        "SELECT app.read_prediction_market($1,$2)",
        &[&other, &market],
    )
    .await?;
    assert_eq!(
        public["book"],
        serde_json::json!([{"side":"yes","limitCost":"60","quantity":"2"}])
    );
    for forbidden in ["creatorId", "holdId", "accountId", "orders", "positions"] {
        assert!(public.get(forbidden).is_none());
    }
    let orders = canonical_read(
        &client,
        "SELECT app.read_prediction_orders($1,$2,NULL,1)",
        &[&alice, &market],
    )
    .await?;
    assert_eq!(orders["items"][0]["id"], yes.to_string());
    assert_eq!(orders["items"][0]["actualFilledCost"], "60");
    assert_eq!(orders["items"][0]["unfilledReservation"], "120");
    let cursor: Uuid = orders["nextCursor"].as_str().unwrap().parse()?;
    assert_eq!(
        canonical_read(
            &client,
            "SELECT app.read_prediction_orders($1,$2,$3,1)",
            &[&alice, &market, &cursor]
        )
        .await?["items"],
        serde_json::json!([])
    );
    let positions = canonical_read(
        &client,
        "SELECT app.read_prediction_positions($1,$2,NULL,100)",
        &[&bob, &market],
    )
    .await?;
    assert_eq!(positions["items"][0]["side"], "no");
    assert_eq!(positions["items"][0]["actualCost"], "40");
    assert!(positions["items"][0].get("accountId").is_none());
    for sql in [
        "SELECT app.read_prediction_orders($1,$2,NULL,100)",
        "SELECT app.read_prediction_positions($1,$2,NULL,100)",
    ] {
        assert_eq!(
            canonical_read(&client, sql, &[&other, &market]).await?["items"],
            serde_json::json!([])
        );
    }
    assert!(
        canonical_read(
            &client,
            "SELECT app.read_prediction_positions($1,$2,NULL,101)",
            &[&alice, &market]
        )
        .await
        .is_err()
    );
    resource(cancel(&client, alice, yes).await?);
    assert_eq!(
        canonical_read(
            &client,
            "SELECT app.read_prediction_market($1,$2)",
            &[&other, &market]
        )
        .await?["book"],
        serde_json::json!([])
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn different_markets_cannot_reserve_the_same_coins_twice() -> TestResult {
    let client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let left = market(&client, alice, 3600).await?;
    let right = market(&client, alice, 3600).await?;
    let other = connection().await?;
    let (a, b) = tokio::join!(
        place(&client, alice, left, "yes", "99", "100", Uuid::new_v4()),
        place(&other, alice, right, "no", "99", "100", Uuid::new_v4())
    );
    let statuses = [
        a?["status"].as_str().unwrap().to_owned(),
        b?["status"].as_str().unwrap().to_owned(),
    ];
    assert_eq!(statuses.iter().filter(|s| *s == "success").count(), 1);
    assert_eq!(statuses.iter().filter(|s| *s == "fail").count(), 1);
    assert_eq!(available(&client, alice).await?, "4600");
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn deleting_counterparty_is_not_matched_and_can_cancel_its_unfilled_reserve() -> TestResult {
    let client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let (bob, _) = buyer(&client).await?;
    let market = market(&client, alice, 3600).await?;
    let yes = resource(place(&client, alice, market, "yes", "60", "3", Uuid::new_v4()).await?);
    resource(place(&client, bob, market, "no", "40", "1", Uuid::new_v4()).await?);
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&alice]).await?;
    assert_eq!(
        place(&client, alice, market, "yes", "60", "1", Uuid::new_v4()).await?["status"],
        "fail"
    );
    resource(place(&client, bob, market, "no", "40", "1", Uuid::new_v4()).await?);
    assert_eq!(
        client
            .query_one(
                "SELECT filled_quantity::text,state FROM app.prediction_order WHERE id=$1",
                &[&yes]
            )
            .await?
            .get::<_, String>(0),
        "1"
    );
    assert_eq!(available(&client, alice).await?, "14440");
    assert_eq!(cancel(&client, alice, yes).await?["status"], "fail");
    // A separate still-open order permits explicit cleanup while deletion waits.
    let (carol, _) = buyer(&client).await?;
    let own = resource(place(&client, carol, market, "yes", "1", "1", Uuid::new_v4()).await?);
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&carol]).await?;
    resource(cancel(&client, carol, own).await?);
    assert_eq!(available(&client, carol).await?, "14500");
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn accepting_before_cutoff_cannot_commit_new_orders_after_cutoff() -> TestResult {
    let mut client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let market = market(&client, alice, 2).await?;
    let transaction = client.transaction().await?;
    // Fixture-owner canonical transition deliberately preserves this source-snapshot test.
    // Public session facades separately reject non-READ-COMMITTED callers.

    resource(
        transaction
            .query_one(
                "SELECT app.place_prediction_order($1,$2,'yes','50','1',$3)",
                &[&alice, &market, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    let wait: f64=transaction.query_one("SELECT greatest(0,extract(epoch FROM trading_closes_at-clock_timestamp()))::double precision FROM app.prediction_market WHERE id=$1", &[&market]).await?.get(0);
    tokio::time::sleep(std::time::Duration::from_secs_f64(wait + 0.02)).await;
    let error = transaction.commit().await.unwrap_err();
    assert_eq!(
        error.code(),
        Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.prediction_order WHERE market_id=$1",
                &[&market]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    assert_eq!(available(&client, alice).await?, "14500");
    Ok(())
}
