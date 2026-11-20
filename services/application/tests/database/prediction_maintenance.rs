use super::*;

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn maintenance_proven_positive_settles_before_future_samples_are_ready() -> TestResult {
    let client = connection().await?;
    let mut fixture = historical(&client, "reached_by").await?;
    let dates=client.query_one("SELECT ((clock_timestamp() AT TIME ZONE 'UTC')::date-1)::text,((clock_timestamp() AT TIME ZONE 'UTC')::date)::text,((clock_timestamp() AT TIME ZONE 'UTC')::date+1)::text", &[]).await?;
    fixture.slot = dates.get(0);
    let today: String = dates.get(1);
    let terminal: String = dates.get(2);
    client
        .batch_execute("SET session_replication_role=replica")
        .await?;
    client.execute("UPDATE app.prediction_market SET trading_closes_at=app.price_slot_start($2::text::date)-interval '1 hour',observation_starts_at=app.price_slot_start($2::text::date),deadline=app.price_slot_start($3::text::date)+interval '1 hour',terminal_slot_ref=$3 WHERE id=$1", &[&fixture.market,&fixture.slot,&terminal]).await?;
    client
        .batch_execute("SET session_replication_role=origin")
        .await?;
    // This owner-seeded normalized source is deliberately synthetic, as in the
    // publication fixtures. Establish it even before the window closes so a
    // crossing of13UTC cannot freeze global Missing beneath those other tests.
    // Actual acquisition/freshness enforcement is qualified separately.
    manifest(&client, &today, true).await?;
    observe(&client, &fixture, Some("12.34")).await?;
    client.batch_execute("SET ROLE service_role").await?;
    let result: Value = client
        .query_one(
            "SELECT app.maintain_prediction_market($1)",
            &[&fixture.market],
        )
        .await?
        .get(0);
    assert_eq!(result["outcome"], "yes");
    client.batch_execute("RESET ROLE").await?;
    assert!(
        client
            .query_one(
                "SELECT deadline>clock_timestamp() FROM app.prediction_market WHERE id=$1",
                &[&fixture.market]
            )
            .await?
            .get::<_, bool>(0)
    );
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.price_observation WHERE market_id=$1",
                &[&fixture.market]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    assert_eq!(balance(&client, fixture.yes).await?, "14580");
    assert_eq!(balance(&client, fixture.no).await?, "14420");
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn maintenance_closes_unfilled_then_waits_for_valid_selected_extraction_and_settles_once()
-> TestResult {
    let client = connection().await?;
    let fixture = historical(&client, "reached_by").await?;
    let source = manifest(&client, &fixture.slot, true).await?;
    client
        .batch_execute("SET session_replication_role=replica")
        .await?;
    client
        .execute(
            "UPDATE app.prediction_market SET state='open' WHERE id=$1",
            &[&fixture.market],
        )
        .await?;
    client.execute("UPDATE app.prediction_order SET quantity=3,state='open' WHERE market_id=$1 AND side='yes'", &[&fixture.market]).await?;
    client.execute("UPDATE app.hold_line SET quantity=180 WHERE hold_id=(SELECT hold_id FROM app.prediction_order WHERE market_id=$1 AND side='yes')", &[&fixture.market]).await?;
    client
        .batch_execute("SET session_replication_role=origin; SET ROLE service_role")
        .await?;
    let pending: Value = client
        .query_one(
            "SELECT app.maintain_prediction_market($1)",
            &[&fixture.market],
        )
        .await?
        .get(0);
    assert_eq!(pending, json!({"state":"pending"}));
    client.batch_execute("RESET ROLE").await?;
    let closed=client.query_one("SELECT m.state,o.state,l.quantity::text FROM app.prediction_market m JOIN app.prediction_order o ON o.market_id=m.id JOIN app.hold_line l ON l.hold_id=o.hold_id WHERE m.id=$1 AND o.side='yes'", &[&fixture.market]).await?;
    assert_eq!(closed.get::<_, String>(0), "closed");
    assert_eq!(closed.get::<_, String>(1), "closed");
    assert_eq!(closed.get::<_, String>(2), "120");
    assert_eq!(balance(&client, fixture.yes).await?, "14500");
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.price_observation WHERE market_id=$1",
                &[&fixture.market]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    observe(&client, &fixture, Some("11.23")).await?;
    client.batch_execute("SET ROLE service_role").await?;
    let result: Value = client
        .query_one(
            "SELECT app.maintain_prediction_market($1)",
            &[&fixture.market],
        )
        .await?
        .get(0);
    let again: Value = client
        .query_one(
            "SELECT app.maintain_prediction_market($1)",
            &[&fixture.market],
        )
        .await?
        .get(0);
    assert_eq!(result, again);
    assert_eq!(result["outcome"], "yes");
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(balance(&client, fixture.yes).await?, "14580");
    assert_eq!(balance(&client, fixture.no).await?, "14420");
    assert_eq!(
        client
            .query_one(
                "SELECT manifest_id FROM app.price_observation WHERE market_id=$1",
                &[&fixture.market]
            )
            .await?
            .get::<_, Uuid>(0),
        source
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn maintenance_missing_sample_voids_without_refund_issuance_and_competing_calls_converge()
-> TestResult {
    let client = connection().await?;
    let fixture = historical(&client, "at_deadline").await?;
    let second = connection().await?;
    client.batch_execute("SET ROLE service_role").await?;
    second.batch_execute("SET ROLE service_role").await?;
    let parameters: [&(dyn tokio_postgres::types::ToSql + Sync); 1] = [&fixture.market];
    let (first, again) = tokio::join!(
        client.query_one("SELECT app.maintain_prediction_market($1)", &parameters),
        second.query_one("SELECT app.maintain_prediction_market($1)", &parameters)
    );
    let mut results = Vec::new();
    for (connection, result) in [(&client, first), (&second, again)] {
        let row = match result {
            Ok(row) => row,
            Err(error) => {
                assert_eq!(error.code().map(|code| code.code()), Some("40001"));
                connection
                    .query_one("SELECT app.maintain_prediction_market($1)", &parameters)
                    .await?
            }
        };
        results.push(row.get::<_, Value>(0));
    }
    let first = &results[0];
    let again = &results[1];
    assert_eq!(first, again);
    assert_eq!(first["outcome"], "void");
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(balance(&client, fixture.yes).await?, "14500");
    assert_eq!(balance(&client, fixture.no).await?, "14500");
    assert_eq!(client.query_one("SELECT count(*) FROM app.ledger_posting WHERE operation_id=(SELECT settlement_operation_id FROM app.prediction_market WHERE id=$1)", &[&fixture.market]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn maintenance_pages_are_bounded_backend_only_and_exclude_future_or_resolved_markets()
-> TestResult {
    let client = connection().await?;
    let fixture = historical(&client, "reached_by").await?;
    let future = Uuid::new_v4();
    client.execute("INSERT INTO app.prediction_market(id,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,deadline,trading_closes_at,observation_starts_at,sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,next_order_sequence,state) SELECT $1,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,clock_timestamp()+interval '3 days',clock_timestamp()+interval '1 day',clock_timestamp()+interval '2 days',sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,1,'open' FROM app.prediction_market WHERE id=$2", &[&future,&fixture.market]).await?;
    for role in ["anon", "authenticated"] {
        client.batch_execute(&format!("SET ROLE {role}")).await?;
        assert!(
            client
                .query_one("SELECT app.read_prediction_maintenance_page(NULL,100)", &[])
                .await
                .is_err()
        );
        assert!(
            client
                .query_one(
                    "SELECT app.maintain_prediction_market($1)",
                    &[&fixture.market]
                )
                .await
                .is_err()
        );
        client.batch_execute("RESET ROLE").await?;
    }
    client.batch_execute("SET ROLE service_role").await?;
    assert!(
        client
            .query_one("SELECT app.read_prediction_maintenance_page(NULL,101)", &[])
            .await
            .is_err()
    );
    assert!(
        client
            .query_one(
                "SELECT app.read_prediction_maintenance_page($1,1)",
                &[&Uuid::nil()]
            )
            .await
            .is_err()
    );
    let not_due: Value = client
        .query_one("SELECT app.maintain_prediction_market($1)", &[&future])
        .await?
        .get(0);
    assert_eq!(not_due, json!({"state":"not_due"}));
    let mut cursor: Option<Uuid> = None;
    let mut found = false;
    loop {
        let page: Value = client
            .query_one(
                "SELECT app.read_prediction_maintenance_page($1,1)",
                &[&cursor],
            )
            .await?
            .get(0);
        let items = page["items"].as_array().unwrap();
        assert!(items.len() <= 1);
        if let Some(id) = items.first() {
            let id = Uuid::parse_str(id.as_str().unwrap())?;
            assert_ne!(id, future);
            assert!(cursor.is_none_or(|cursor| id > cursor));
            found |= id == fixture.market;
        }
        cursor = page["nextCursor"]
            .as_str()
            .map(Uuid::parse_str)
            .transpose()?;
        if cursor.is_none() {
            break;
        }
    }
    assert!(found);
    let resolved: Value = client
        .query_one(
            "SELECT app.maintain_prediction_market($1)",
            &[&fixture.market],
        )
        .await?
        .get(0);
    assert_eq!(resolved["state"], "final");
    let after = Uuid::from_u128(fixture.market.as_u128() - 1);
    let page: Value = client
        .query_one(
            "SELECT app.read_prediction_maintenance_page($1,1)",
            &[&after],
        )
        .await?
        .get(0);
    assert!(
        !page["items"]
            .as_array()
            .unwrap()
            .contains(&json!(fixture.market))
    );
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(
        client
            .query_one(
                "SELECT state FROM app.prediction_market WHERE id=$1",
                &[&future]
            )
            .await?
            .get::<_, String>(0),
        "open"
    );
    Ok(())
}
