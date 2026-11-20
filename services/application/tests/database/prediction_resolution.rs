use super::{
    TestResult, connection,
    pricing::{buyer, product},
};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;

#[path = "prediction_maintenance.rs"]
mod maintenance;

// Fixture-owner canonical publication only. Runtime authorization is tested through the session facade.
async fn canonical_publication(
    client: &Client,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<tokio_postgres::Row, tokio_postgres::Error> {
    let role: String = client
        .query_one("SELECT current_user::text", &[])
        .await?
        .get(0);
    assert!(matches!(role.as_str(), "postgres" | "service_role"));
    client.batch_execute("RESET ROLE").await?;
    let result = client.query_one(sql, params).await;
    client.batch_execute(&format!("SET ROLE {role}")).await?;
    result
}

// Deliberately synthetic source records, inserted by the test DB owner. These
// fixtures do not establish that a Scryfall artifact was downloaded or retained.
pub(super) async fn manifest(
    client: &Client,
    slot: &str,
    valid: bool,
) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
    let data: Value=client.query_one("SELECT jsonb_build_object('version','scryfall-slot-v1','slot',$1::text,'state',CASE WHEN $2 THEN 'valid' ELSE 'missing' END)||CASE WHEN $2 THEN jsonb_build_object('source','scryfall:all_cards','hash','sha256:'||repeat('a',64),'artifactRef','prices/scryfall/'||repeat('a',64)||'.jsonl.gz','recordCount','1','downloadUri','https://data.scryfall.io/all-cards/all-cards-20261031212149.jsonl.gz','acquiredAt',least(clock_timestamp(),app.price_slot_start($1::text::date)+interval '10 minutes'),'sourceUpdatedAt',app.price_slot_start($1::text::date)-interval '1 hour') ELSE '{}'::jsonb END", &[&slot,&valid]).await?.get(0);
    let reference = data.to_string();
    client
        .batch_execute("SET session_replication_role=replica")
        .await?;
    client.execute("INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref) VALUES('collector:scryfall-v1',$1,'price_slot',$1,encode(sha256(convert_to($2::text::jsonb::text,'UTF8')),'hex'),'accepted',$2::text::jsonb::text) ON CONFLICT DO NOTHING", &[&slot,&reference]).await?;
    client
        .batch_execute("SET session_replication_role=origin")
        .await?;
    Ok(client.query_one("SELECT id FROM app.command_receipt WHERE principal_key='collector:scryfall-v1' AND scope='price_slot' AND request_ref=$1", &[&slot]).await?.get(0))
}
async fn balance(client: &Client, actor: Uuid) -> Result<String, tokio_postgres::Error> {
    Ok(client.query_one("SELECT i.owned_quantity::text FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.owner_id=$1 AND d.currency_key='coin'", &[&actor]).await?.get(0))
}
struct Historical {
    market: Uuid,
    yes: Uuid,
    no: Uuid,
    printing_ref: String,
    slot: String,
}
async fn historical(
    client: &Client,
    condition: &str,
) -> Result<Historical, Box<dyn std::error::Error + Send + Sync>> {
    historical_span(client, condition, 0).await
}
async fn historical_span(
    client: &Client,
    condition: &str,
    days: i32,
) -> Result<Historical, Box<dyn std::error::Error + Send + Sync>> {
    let (yes, _) = buyer(client).await?;
    let (no, _) = buyer(client).await?;
    let product = product(client, false).await?;
    let row=client.query_one("SELECT p.id,p.external_printing_ref FROM app.printing p JOIN app.asset_definition d ON d.printing_id=p.id WHERE d.id=$1", &[&product.cards[0]]).await?;
    let printing: Uuid = row.get(0);
    let printing_ref: String = row.get(1);
    // A disjoint ancient date per fixture avoids colliding with live daily slots.
    let offset = (Uuid::new_v4().as_u128() % 1000000) as i32;
    let slot: String = client
        .query_one("SELECT (date '1000-01-01'-$1::integer)::text", &[&offset])
        .await?
        .get(0);
    let market = Uuid::new_v4();
    client.execute("INSERT INTO app.prediction_market(id,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,deadline,trading_closes_at,observation_starts_at,sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,next_order_sequence,state) VALUES($1,$2,$3,'scryfall:usd','10.00',$4,app.price_slot_start($5::text::date)+interval '1 hour',app.price_slot_start($5::text::date)-$6::integer*interval '1 day'-interval '1 hour',app.price_slot_start($5::text::date)-$6::integer*interval '1 day','scryfall-daily-12-13-utc-v1',$5,'scryfall-sampled-gte-v1',100,3,'closed')", &[&market,&yes,&printing,&condition,&slot,&days]).await?;
    // Only the fixture session bypasses historical insertion cutoff triggers.
    // Construct fully funded past fills; matching itself is tested separately.
    client
        .batch_execute("SET session_replication_role=replica")
        .await?;
    let mut orders = Vec::new();
    for (sequence, actor, side, cost) in [(1i64, yes, "yes", 120i64), (2, no, "no", 80)] {
        let order = Uuid::new_v4();
        let hold = Uuid::new_v4();
        let operation = Uuid::new_v4();
        let inventory: Uuid=client.query_one("SELECT i.id FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.owner_id=$1 AND d.currency_key='coin'", &[&actor]).await?.get(0);
        client.execute("INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES($1,'prediction_order',$2::uuid::text,'prediction_order',$3,clock_timestamp(),'committed')", &[&operation,&order,&actor]).await?;
        client.execute("INSERT INTO app.asset_hold(id,owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,'prediction',$3::uuid::text,'held',$4)", &[&hold,&actor,&order,&operation]).await?;
        client.execute("INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,$3::bigint)", &[&hold,&inventory,&cost]).await?;
        client.execute("INSERT INTO app.prediction_order(id,market_id,account_id,hold_id,side,limit_cost_coins,quantity,filled_quantity,accepted_sequence,state,accepted_at) VALUES($1,$2,$3,$4,$5,$6::bigint/2,2,2,$7::bigint,'filled',app.price_slot_start($8::text::date)-$9::integer*interval '1 day'-interval '2 hours')", &[&order,&market,&actor,&hold,&side,&cost,&sequence,&slot,&days]).await?;
        orders.push(order);
    }
    let pair = Uuid::new_v4();
    let operation = Uuid::new_v4();
    client.execute("INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES($1,'prediction_match',$2::uuid::text,'prediction_match',$3,clock_timestamp(),'committed')", &[&operation,&pair,&no]).await?;
    client.execute("INSERT INTO app.prediction_pair(id,market_id,yes_order_id,no_order_id,quantity,fixed_payout_coins,yes_actual_cost_coins,no_actual_cost_coins,match_operation_id,matched_at) VALUES($1,$2,$3,$4,2,100,120,80,$5,app.price_slot_start($6::text::date)-$7::integer*interval '1 day'-interval '90 minutes')", &[&pair,&market,&orders[0],&orders[1],&operation,&slot,&days]).await?;
    client
        .batch_execute("SET session_replication_role=origin")
        .await?;
    Ok(Historical {
        market,
        yes,
        no,
        printing_ref,
        slot,
    })
}
async fn observe(
    client: &Client,
    fixture: &Historical,
    price: Option<&str>,
) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
    let manifest = manifest(client, &fixture.slot, true).await?;
    let card = json!({"object":"card","id":fixture.printing_ref,"finishes":["nonfoil"],"prices":{"usd":price}});
    Ok(client
        .query_one(
            "SELECT app.record_prediction_observation($1,$2,$3)",
            &[&fixture.market, &manifest, &card],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn outcomes_settle_actual_costs_and_fixed_winnings_once_without_issuance() -> TestResult {
    let client = connection().await?;
    for (price, outcome, yes_balance, no_balance) in [
        ("10.00", "yes", "14580", "14420"),
        ("9.99999999999999999999", "no", "14380", "14620"),
    ] {
        let fixture = historical(&client, "at_deadline").await?;
        let evidence = observe(&client, &fixture, Some(price)).await?;
        client.batch_execute("SET ROLE service_role").await?;
        let result: Value = client
            .query_one("SELECT app.resolve_prediction($1)", &[&fixture.market])
            .await?
            .get(0);
        let again: Value = client
            .query_one("SELECT app.resolve_prediction($1)", &[&fixture.market])
            .await?
            .get(0);
        client.batch_execute("RESET ROLE").await?;
        assert_eq!(result["outcome"], outcome);
        assert_eq!(result, again);
        assert_eq!(balance(&client, fixture.yes).await?, yes_balance);
        assert_eq!(balance(&client, fixture.no).await?, no_balance);
        assert_eq!(client.query_one("SELECT sum(CASE WHEN direction='credit' THEN quantity ELSE -quantity END)::text FROM app.ledger_posting WHERE operation_id=(SELECT settlement_operation_id FROM app.prediction_market WHERE id=$1)", &[&fixture.market]).await?.get::<_,String>(0),"0");
        assert!(
            client
                .execute(
                    "UPDATE app.price_observation SET price_usd_decimal='1' WHERE id=$1",
                    &[&evidence]
                )
                .await
                .is_err()
        );
        assert!(
            client
                .execute(
                    "UPDATE app.prediction_market SET outcome='void' WHERE id=$1",
                    &[&fixture.market]
                )
                .await
                .is_err()
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn missing_prices_void_and_release_original_backing_without_minting_refunds() -> TestResult {
    let client = connection().await?;
    let fixture = historical(&client, "reached_by").await?;
    observe(&client, &fixture, None).await?;
    let result: Value = client
        .query_one("SELECT app.resolve_prediction($1)", &[&fixture.market])
        .await?
        .get(0);
    assert_eq!(result["outcome"], "void");
    assert_eq!(balance(&client, fixture.yes).await?, "14500");
    assert_eq!(balance(&client, fixture.no).await?, "14500");
    assert_eq!(client.query_one("SELECT count(*) FROM app.ledger_posting WHERE operation_id=(SELECT settlement_operation_id FROM app.prediction_market WHERE id=$1)", &[&fixture.market]).await?.get::<_,i64>(0),0);
    assert_eq!(client.query_one("SELECT count(*) FROM app.asset_hold h JOIN app.prediction_order o ON o.hold_id=h.id WHERE o.market_id=$1 AND h.state='released'", &[&fixture.market]).await?.get::<_,i64>(0),2);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn valid_shared_artifact_waits_for_extraction_but_missing_slot_can_finalize() -> TestResult {
    let client = connection().await?;
    let waiting = historical(&client, "at_deadline").await?;
    manifest(&client, &waiting.slot, true).await?;
    let result: Value = client
        .query_one("SELECT app.resolve_prediction($1)", &[&waiting.market])
        .await?
        .get(0);
    assert_eq!(result["state"], "pending");
    observe(&client, &waiting, Some("11.00")).await?;
    assert_eq!(
        client
            .query_one("SELECT app.resolve_prediction($1)", &[&waiting.market])
            .await?
            .get::<_, Value>(0)["outcome"],
        "yes"
    );
    let missing = historical(&client, "at_deadline").await?;
    assert_eq!(
        client
            .query_one("SELECT app.resolve_prediction($1)", &[&missing.market])
            .await?
            .get::<_, Value>(0)["outcome"],
        "void"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn publication_requires_current_exact_finish_evidence_and_freezes_a_future_calendar()
-> TestResult {
    let client = connection().await?;
    let (actor, _) = buyer(&client).await?;
    let product = product(&client, false).await?;
    let row=client.query_one("SELECT p.id,p.external_printing_ref FROM app.printing p JOIN app.asset_definition d ON d.printing_id=p.id WHERE d.id=$1", &[&product.cards[0]]).await?;
    let printing: Uuid = row.get(0);
    let reference: String = row.get(1);
    let slot: String = client
        .query_one(
            "SELECT ((clock_timestamp() AT TIME ZONE 'UTC'-interval '12 hours')::date)::text",
            &[],
        )
        .await?
        .get(0);
    let source = manifest(&client, &slot, true).await?;
    let card = json!({"object":"card","id":reference,"finishes":["nonfoil","foil"],"prices":{"usd":"2.37","usd_foil":null,"usd_etched":null}});
    client.batch_execute("SET ROLE service_role").await?;
    let probe: Uuid = client
        .query_one("SELECT app.record_price_probe($1,$2)", &[&source, &card])
        .await?
        .get(0);
    let external: Uuid = reference.parse()?;
    assert_eq!(
        client
            .query_one(
                "SELECT app.read_established_price_probe($1,$2)",
                &[&source, &external]
            )
            .await?
            .get::<_, Option<Uuid>>(0),
        Some(probe)
    );
    assert!(
        client
            .query_one(
                "SELECT app.read_established_price_probe($1,$2)",
                &[&source, &Uuid::new_v4()]
            )
            .await?
            .get::<_, Option<Uuid>>(0)
            .is_none()
    );

    assert_eq!(
        client
            .query_one("SELECT app.record_price_probe($1,$2)", &[&source, &card])
            .await?
            .get::<_, Uuid>(0),
        probe
    );
    let request = Uuid::new_v4();
    let publish = "SELECT app.publish_prediction($1,$2,$3,$4,clock_timestamp()+interval '1 hour',((date_trunc('day',clock_timestamp() AT TIME ZONE 'UTC')+interval '2 days 12 hours') AT TIME ZONE 'UTC'),((date_trunc('day',clock_timestamp() AT TIME ZONE 'UTC')+interval '2 days 12 hours') AT TIME ZONE 'UTC')+interval '1 hour',$5,$6)";
    let result: Value = canonical_publication(
        &client,
        publish,
        &[&actor, &printing, &"10.00", &"reached_by", &probe, &request],
    )
    .await?
    .get(0);
    assert_eq!(result["status"], "success");
    let market: Uuid = result["data"]["resourceId"].as_str().unwrap().parse()?;
    let replay: Value=canonical_publication(&client, "SELECT app.publish_prediction($1,m.printing_id,m.target_usd_decimal,m.condition,m.trading_closes_at,m.observation_starts_at,m.deadline,m.publication_probe_id,$2) FROM app.prediction_market m WHERE m.id=$3", &[&actor,&request,&market]).await?.get(0);
    assert_eq!(result, replay);
    assert!(canonical_publication(&client, "SELECT app.publish_prediction($1,m.printing_id,'11.00',m.condition,m.trading_closes_at,m.observation_starts_at,m.deadline,m.publication_probe_id,$2) FROM app.prediction_market m WHERE m.id=$3", &[&actor,&request,&market]).await.is_err());
    // New commands below use fresh request IDs and explicit future timestamps.
    for target in ["0", "NaN", "1e2", "-1", "abc"] {
        let invalid: Value = canonical_publication(
            &client,
            publish,
            &[
                &actor,
                &printing,
                &target,
                &"reached_by",
                &probe,
                &Uuid::new_v4(),
            ],
        )
        .await?
        .get(0);
        assert_eq!(invalid["status"], "fail");
    }
    assert!(
        client
            .execute(
                "UPDATE app.command_receipt SET result_ref='{}' WHERE id=$1",
                &[&source]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.price_observation SET state='missing' WHERE market_id=$1",
                &[&market]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    assert!(
        client
            .execute(
                "UPDATE app.prediction_market SET publication_probe_id=NULL WHERE id=$1",
                &[&market]
            )
            .await
            .is_err()
    );
    let terms=client.query_one("SELECT price_metric_ref,terminal_slot_ref,(SELECT count(*) FROM app.prediction_slots(m)) FROM app.prediction_market m WHERE id=$1", &[&market]).await?;
    assert_eq!(terms.get::<_, String>(0), "scryfall:usd");
    assert_eq!(terms.get::<_, i64>(2), 1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn daily_slot_identity_is_stable_across_session_date_and_timezone_settings() -> TestResult {
    let client = connection().await?;
    let offset = (Uuid::new_v4().as_u128() % 1000000) as i32;
    let slot: String = client
        .query_one("SELECT (date '1500-01-01'-$1::integer)::text", &[&offset])
        .await?
        .get(0);
    client
        .batch_execute(
            "SET ROLE service_role; SET DateStyle='ISO, MDY'; SET TimeZone='America/New_York'",
        )
        .await?;
    let first: Uuid = client
        .query_one(
            "SELECT app.commit_price_slot($1::text::date,NULL)",
            &[&slot],
        )
        .await?
        .get(0);
    client
        .batch_execute("SET DateStyle='German, DMY'; SET TimeZone='Pacific/Auckland'")
        .await?;
    let second: Uuid = client
        .query_one(
            "SELECT app.commit_price_slot($1::text::date,NULL)",
            &[&slot],
        )
        .await?
        .get(0);
    assert_eq!(first, second);
    let established: Value = client
        .query_one(
            "SELECT app.read_established_price_slot($1::text::date)",
            &[&slot],
        )
        .await?
        .get(0);
    assert_eq!(established["id"], first.to_string());
    assert_eq!(established["manifest"]["slot"], slot);
    client
        .batch_execute("RESET ROLE; RESET DateStyle; RESET TimeZone")
        .await?;
    assert_eq!(client.query_one("SELECT count(*) FROM app.command_receipt WHERE principal_key='collector:scryfall-v1' AND scope='price_slot' AND target_ref=$1", &[&slot]).await?.get::<_,i64>(0),1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn reached_by_needs_all_negative_samples_but_one_positive_overrides_a_missing_slot()
-> TestResult {
    let client = connection().await?;
    for (first, last, expected) in [
        (Some("9.99"), Some("9.99"), "no"),
        (None, Some("9.99"), "void"),
        (Some("10.00"), None, "yes"),
    ] {
        let fixture = historical_span(&client, "reached_by", 1).await?;
        let previous: String = client
            .query_one("SELECT ($1::text::date-1)::text", &[&fixture.slot])
            .await?
            .get(0);
        let first_slot = Historical {
            market: fixture.market,
            yes: fixture.yes,
            no: fixture.no,
            printing_ref: fixture.printing_ref.clone(),
            slot: previous,
        };
        observe(&client, &first_slot, first).await?;
        observe(&client, &fixture, last).await?;
        let result: Value = client
            .query_one("SELECT app.resolve_prediction($1)", &[&fixture.market])
            .await?
            .get(0);
        assert_eq!(result["outcome"], expected);
    }
    // A past positive from a different scheduled day cannot stand in for the
    // fixed terminal sample when the market uses at-deadline semantics.
    let fixture = historical_span(&client, "at_deadline", 1).await?;
    let previous: String = client
        .query_one("SELECT ($1::text::date-1)::text", &[&fixture.slot])
        .await?
        .get(0);
    let earlier = Historical {
        market: fixture.market,
        yes: fixture.yes,
        no: fixture.no,
        printing_ref: fixture.printing_ref.clone(),
        slot: previous,
    };
    observe(&client, &earlier, Some("20.00")).await?;
    observe(&client, &fixture, Some("9.00")).await?;
    assert_eq!(
        client
            .query_one("SELECT app.resolve_prediction($1)", &[&fixture.market])
            .await?
            .get::<_, Value>(0)["outcome"],
        "no"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn partial_canceled_positions_settle_during_deletion_and_competing_workers_pay_once()
-> TestResult {
    let client = connection().await?;
    let fixture = historical(&client, "at_deadline").await?;
    // Historical fixture for an original three-contract order with two fills
    // whose remaining reservation was already canceled. Normal cancellation and
    // fill production are exercised by the prediction command tests.
    client
        .batch_execute("SET session_replication_role=replica")
        .await?;
    client.execute("UPDATE app.prediction_order SET quantity=3,state='canceled' WHERE market_id=$1 AND account_id=$2", &[&fixture.market,&fixture.yes]).await?;
    client
        .batch_execute("SET session_replication_role=origin")
        .await?;
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&fixture.yes]).await?;
    observe(&client, &fixture, Some("10.00")).await?;
    let other = connection().await?;
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 1] = [&fixture.market];
    let (a, b) = tokio::join!(
        client.query_one("SELECT app.resolve_prediction($1)", &params),
        other.query_one("SELECT app.resolve_prediction($1)", &params)
    );
    let mut accepted = Vec::new();
    for response in [a, b] {
        match response {
            Ok(row) => accepted.push(row.get::<_, Value>(0)),
            Err(error) => {
                assert_eq!(
                    error.code(),
                    Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
                );
                accepted.push(
                    client
                        .query_one("SELECT app.resolve_prediction($1)", &[&fixture.market])
                        .await?
                        .get(0),
                );
            }
        }
    }
    assert_eq!(accepted[0], accepted[1]);
    assert_eq!(balance(&client, fixture.yes).await?, "14580");
    assert_eq!(balance(&client, fixture.no).await?, "14420");
    assert_eq!(client.query_one("SELECT count(*) FROM app.ledger_operation WHERE kind='prediction_settlement' AND source_ref=$1", &[&fixture.market.to_string()]).await?.get::<_,i64>(0),1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn publication_rejects_wrong_printing_finish_nonstring_and_stale_price_proofs() -> TestResult
{
    let client = connection().await?;
    let (actor, _) = buyer(&client).await?;
    let slot: String = client
        .query_one(
            "SELECT ((clock_timestamp() AT TIME ZONE 'UTC'-interval '12 hours')::date)::text",
            &[],
        )
        .await?
        .get(0);
    let source = manifest(&client, &slot, true).await?;
    for (finishes, price) in [
        (json!(["foil"]), json!("2.00")),
        (json!(["nonfoil"]), json!(2)),
        (json!(["nonfoil"]), Value::Null),
    ] {
        let product = product(&client, false).await?;
        let row=client.query_one("SELECT p.id,p.external_printing_ref FROM app.printing p JOIN app.asset_definition d ON d.printing_id=p.id WHERE d.id=$1", &[&product.cards[0]]).await?;
        let printing: Uuid = row.get(0);
        let reference: String = row.get(1);
        let card =
            json!({"object":"card","id":reference,"finishes":finishes,"prices":{"usd":price}});
        client.batch_execute("SET ROLE service_role").await?;
        let probe: Uuid = client
            .query_one("SELECT app.record_price_probe($1,$2)", &[&source, &card])
            .await?
            .get(0);
        let result: Value=canonical_publication(&client, "SELECT app.publish_prediction($1,$2,'10.00','reached_by',clock_timestamp()+interval '1 hour',clock_timestamp()+interval '2 days',clock_timestamp()+interval '4 days',$3,$4)", &[&actor,&printing,&probe,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(result["status"], "fail");
        client.batch_execute("RESET ROLE").await?;
    }
    let fixture = historical(&client, "at_deadline").await?;
    let old_source = manifest(&client, &fixture.slot, true).await?;
    let card = json!({"object":"card","id":fixture.printing_ref,"finishes":["nonfoil"],"prices":{"usd":"2.00"}});
    client.batch_execute("SET ROLE service_role").await?;
    assert!(
        client
            .query_one(
                "SELECT app.record_price_probe($1,$2)",
                &[&old_source, &card]
            )
            .await
            .is_err()
    );
    let probe: Uuid = client
        .query_one("SELECT app.record_price_probe($1,$2)", &[&source, &card])
        .await?
        .get(0);
    let other_printing: Uuid = client
        .query_one(
            "SELECT printing_id FROM app.prediction_market WHERE id=$1",
            &[&fixture.market],
        )
        .await?
        .get(0);
    // Valid evidence for a different UUID cannot publish a market for this printing.
    let wrong: Uuid = client
        .query_one(
            "SELECT id FROM app.printing WHERE id<>$1 ORDER BY id LIMIT 1",
            &[&other_printing],
        )
        .await?
        .get(0);
    let result: Value=canonical_publication(&client, "SELECT app.publish_prediction($1,$2,'10.00','reached_by',clock_timestamp()+interval '1 hour',clock_timestamp()+interval '2 days',clock_timestamp()+interval '4 days',$3,$4)", &[&actor,&wrong,&probe,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(result["status"], "fail");
    let mut changed = card;
    changed["prices"]["usd"] = json!("3.00");
    assert!(
        client
            .query_one("SELECT app.record_price_probe($1,$2)", &[&source, &changed])
            .await
            .is_err()
    );
    assert_ne!(other_printing, wrong);
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn normalized_artifact_validation_requires_exact_source_freshness_window_and_retention_identity()
-> TestResult {
    let client = connection().await?;
    let valid = json!({"metadataHash":format!("sha256:{}","b".repeat(64)),"metadataRef":format!("prices/scryfall/metadata/{}.json","b".repeat(64)),"hash":format!("sha256:{}","a".repeat(64)),"artifactRef":format!("prices/scryfall/{}.jsonl.gz","a".repeat(64)),"downloadUri":"https://data.scryfall.io/all-cards/all-cards-20261101120000.jsonl.gz","acquiredAt":"2026-11-01T12:10:00Z","sourceUpdatedAt":"2026-10-31T12:10:00Z","recordCount":"123"});
    let sql = "SELECT app.validate_price_artifact(date '2026-11-01',$1,timestamptz '2026-11-01T12:30:00Z')";
    let manifest: Value = client.query_one(sql, &[&valid]).await?.get(0);
    assert_eq!(manifest["state"], "valid");
    assert_eq!(manifest["slot"], "2026-11-01");
    for (key, bad) in [
        ("acquiredAt", json!("2026-11-01T11:59:59Z")),
        ("acquiredAt", json!("2026-11-01T12:30:01Z")),
        ("sourceUpdatedAt", json!("2026-10-31T12:09:59Z")),
        ("sourceUpdatedAt", json!("2026-11-01T12:10:01Z")),
        ("recordCount", json!("0")),
        ("recordCount", json!(123)),
        ("recordCount", json!("5000001")),
        ("hash", json!("bad")),
        ("artifactRef", json!("unretained")),
        (
            "downloadUri",
            json!("https://example.com/all-cards.jsonl.gz"),
        ),
    ] {
        let mut changed = valid.clone();
        changed[key] = bad;
        assert!(client.query_one(sql, &[&changed]).await.is_err());
    }
    assert!(client.query_one("SELECT app.validate_price_artifact(date '2026-11-01',$1,timestamptz '2026-11-01T13:00:00Z')", &[&valid]).await.is_err());
    client.batch_execute("SET ROLE service_role").await?;
    assert!(
        client.query_one(sql, &[&valid]).await.is_err(),
        "workers cannot select their own validation clock"
    );
    assert!(
        client
            .query_one(
                "SELECT app.commit_price_slot((clock_timestamp() AT TIME ZONE 'UTC')::date+1,$1)",
                &[&json!({})]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    // The success above qualifies normalized metadata only. It does not download
    // a Scryfall artifact or prove the referenced storage object exists.
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn extraction_work_is_bounded_paged_and_excludes_established_observations() -> TestResult {
    let client = connection().await?;
    let fixture = historical(&client, "at_deadline").await?;
    let other = Uuid::new_v4();
    client.execute("INSERT INTO app.prediction_market(id,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,deadline,trading_closes_at,observation_starts_at,sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,next_order_sequence,state) SELECT $1,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,deadline,trading_closes_at,observation_starts_at,sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,1,'closed' FROM app.prediction_market WHERE id=$2", &[&other,&fixture.market]).await?;
    client.batch_execute("SET ROLE service_role").await?;
    let first: Value = client
        .query_one(
            "SELECT app.read_prediction_extraction_page($1::text::date,NULL,1)",
            &[&fixture.slot],
        )
        .await?
        .get(0);
    let cursor: Uuid = first["nextCursor"].as_str().unwrap().parse()?;
    let second: Value = client
        .query_one(
            "SELECT app.read_prediction_extraction_page($1::text::date,$2,1)",
            &[&fixture.slot, &cursor],
        )
        .await?
        .get(0);
    let ids = [
        first["items"][0]["marketId"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()?,
        second["items"][0]["marketId"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()?,
    ];
    assert!(ids[0] < ids[1]);
    assert!(ids.contains(&fixture.market) && ids.contains(&other));
    for limit in [0i32, 101] {
        assert!(
            client
                .query_one(
                    "SELECT app.read_prediction_extraction_page($1::text::date,NULL,$2)",
                    &[&fixture.slot, &limit]
                )
                .await
                .is_err()
        );
    }
    assert!(
        client
            .query_one(
                "SELECT app.read_prediction_extraction_page(current_date+1,NULL,100)",
                &[]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    observe(&client, &fixture, Some("10.00")).await?;
    let page: Value = client
        .query_one(
            "SELECT app.read_prediction_extraction_page($1::text::date,NULL,100)",
            &[&fixture.slot],
        )
        .await?
        .get(0);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["marketId"], other.to_string());
    let terminal_only = historical_span(&client, "at_deadline", 1).await?;
    let earlier: String = client
        .query_one("SELECT ($1::text::date-1)::text", &[&terminal_only.slot])
        .await?
        .get(0);
    let early: Value = client
        .query_one(
            "SELECT app.read_prediction_extraction_page($1::text::date,NULL,100)",
            &[&earlier],
        )
        .await?
        .get(0);
    assert!(
        !early["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["marketId"] == terminal_only.market.to_string())
    );
    let final_page: Value = client
        .query_one(
            "SELECT app.read_prediction_extraction_page($1::text::date,NULL,100)",
            &[&terminal_only.slot],
        )
        .await?
        .get(0);
    assert!(
        final_page["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["marketId"] == terminal_only.market.to_string())
    );
    client.batch_execute("SET ROLE authenticated").await?;
    assert!(
        client
            .query_one(
                "SELECT app.read_prediction_extraction_page($1::text::date,NULL,100)",
                &[&fixture.slot]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}
