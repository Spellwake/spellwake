//! Explicit synthetic reviewed manifests qualify SQL conversion, never physical evidence truth.
use super::{TestResult, connection, pricing, wait_for_database_lock};
use serde_json::{Value, json};
use spellwake_application::supply_source_ports::{BaselineManifest, PhysicalIntroduction};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
const HASH: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn physical(p: &pricing::Product, quantity: &str) -> Value {
    json!({"version":"physical-introduction-v1","recipeId":p.recipe,"productId":p.product,"runRef":"fixture:initial-run","language":"en","territory":"GLOBAL","unit":"physical_pack","qualification":"exact","physicalQuantity":quantity,"evidenceRef":HASH,"modifierN":"1","modifierD":"1","modifierRef":HASH,"normalQuantity":"100"})
}
async fn publish(c: &Client, manifest: &Value) -> Result<Value, tokio_postgres::Error> {
    let input = PhysicalIntroduction::from_bytes(&serde_json::to_vec(manifest).unwrap())
        .expect("fixture physical source grammar");
    c.query_one("SELECT app.publish_physical_introduction($1)", &[manifest])
        .await
        .map(|r| {
            let value: Value = r.get(0);
            input
                .decode_response(&serde_json::to_vec(&value).unwrap(), None)
                .expect("actual physical source receipt");
            value
        })
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn baseline_uses_exact_forecast_formula_and_freezes_the_complete_eligible_list() -> TestResult
{
    let c = connection().await?;
    let a = pricing::product(&c, true).await?;
    let b = pricing::product(&c, true).await?;
    let entry = |p: &pricing::Product| json!({"recipeId":p.recipe,"productId":p.product,"eligibleAt":"2026-10-01T00:00:00.000000Z","evidenceRef":HASH,"modifierN":"1","modifierD":"1","modifierRef":HASH});
    let manifest = json!({"version":"launch-baseline-v1","forecast":"3","cutoff":"2026-10-02T00:00:00.000000Z","configurationRef":HASH,"products":[entry(&a),entry(&b)]});
    // Exercise the actual publisher's minimum and ceil boundaries, rolling back each complete source.
    let mut boundary = manifest.clone();
    for _ in 0..5 {
        let product = pricing::product(&c, true).await?;
        boundary["products"]
            .as_array_mut()
            .unwrap()
            .push(entry(&product));
    }
    for (forecast, expected) in [("0", "20"), ("1", "20"), ("2", "35")] {
        boundary["forecast"] = json!(forecast);
        c.batch_execute("BEGIN").await?;
        let result: Value = c
            .query_one("SELECT app.publish_baseline_manifest($1)", &[&boundary])
            .await?
            .get(0);
        BaselineManifest::from_bytes(&serde_json::to_vec(&boundary)?)?
            .decode_response(&serde_json::to_vec(&result)?)?;
        assert_eq!(result["data"]["quantityPerProduct"], expected);
        let stocks:i64=c.query_one("SELECT count(*)::bigint FROM app.supply_stock WHERE recipe_id IN(SELECT (value->>'recipeId')::uuid FROM jsonb_array_elements($1::jsonb->'products'))", &[&boundary]).await?.get(0);
        assert_eq!(stocks, 7);
        c.batch_execute("ROLLBACK").await?;
    }
    // Failure late in a multi-product publication cannot leave the earlier product stocked.
    let mut invalid = manifest.clone();
    invalid["products"][if a.product < b.product { 1 } else { 0 }]["eligibleAt"] =
        json!("2026-10-03T00:00:00.000000Z");
    assert!(
        c.query_one("SELECT app.publish_baseline_manifest($1)", &[&invalid])
            .await
            .is_err()
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.supply_stock WHERE recipe_id=ANY($1)",
            &[&vec![a.recipe, b.recipe]]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    let first: Value = c
        .query_one("SELECT app.publish_baseline_manifest($1)", &[&manifest])
        .await?
        .get(0);
    let input = BaselineManifest::from_bytes(&serde_json::to_vec(&manifest)?)?;
    input.decode_response(&serde_json::to_vec(&first)?)?;
    assert_eq!(first["data"]["quantityPerProduct"], "180");
    assert_eq!(
        c.query_one("SELECT app.publish_baseline_manifest($1)", &[&manifest])
            .await?
            .get::<_, Value>(0),
        first
    );
    let mut changed = manifest;
    changed["forecast"] = json!("4");
    assert_eq!(
        c.query_one("SELECT app.publish_baseline_manifest($1)", &[&changed])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn exact_physical_quantity_is_floored_once_and_coverage_cannot_be_reissued() -> TestResult {
    let c = connection().await?;
    let p = pricing::product(&c, true).await?;
    let manifest = physical(&p, "12345");
    let first = publish(&c, &manifest).await?;
    assert_eq!(first["data"]["quantity"], "123");
    assert_eq!(first["data"]["disposition"], "published");
    let replay = publish(&c, &manifest).await?;
    PhysicalIntroduction::from_bytes(&serde_json::to_vec(&manifest)?)?.decode_response(
        &serde_json::to_vec(&replay)?,
        first["data"]["coverage"].as_str(),
    )?;
    assert_eq!(replay, first);
    let mut changed = manifest.clone();
    changed["physicalQuantity"] = json!("12346");
    assert_eq!(
        publish(&c, &changed).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT sum(starting_quantity)::text FROM app.supply_stock WHERE recipe_id=$1",
            &[&p.recipe]
        )
        .await?
        .get::<_, String>(0),
        "123"
    );
    let mut wrong = manifest;
    wrong["productId"] = json!(Uuid::new_v4());
    assert!(publish(&c, &wrong).await.is_err());
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unavailable_bounded_wrong_unit_and_zero_conversions_never_create_or_reset_stock()
-> TestResult {
    let c = connection().await?;
    for case in ["unavailable", "bounded", "wrong-unit", "zero"] {
        let p = pricing::product(&c, true).await?;
        let mut manifest = physical(&p, if case == "zero" { "99" } else { "10000" });
        if case == "wrong-unit" {
            manifest["unit"] = json!("physical_card");
        }
        if ["unavailable", "bounded"].contains(&case) {
            manifest["qualification"] = json!(case);
        }
        let result = publish(&c, &manifest).await?;
        assert_eq!(result["data"]["quantity"], "0");
        assert_eq!(result["data"]["windowId"], "");
        assert!(
            c.query_one(
                "SELECT current_pricing_policy_id IS NULL FROM app.booster_product WHERE id=$1",
                &[&p.product]
            )
            .await?
            .get::<_, bool>(0)
        );
        assert_eq!(publish(&c, &manifest).await?, result);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unavailable_observation_can_later_qualify_but_overlaps_and_reprints_cannot_issue_again()
-> TestResult {
    let c = connection().await?;
    let p = pricing::product(&c, true).await?;
    let exact = physical(&p, "10000");
    let mut missing = exact.clone();
    missing["qualification"] = json!("unavailable");
    missing["physicalQuantity"] = Value::Null;
    missing["evidenceRef"] = Value::Null;
    assert_eq!(
        publish(&c, &missing).await?["data"]["disposition"],
        "unavailable"
    );
    assert_eq!(
        publish(&c, &exact).await?["data"]["disposition"],
        "published"
    );
    let mut country = exact.clone();
    country["territory"] = json!("US");
    assert_eq!(
        publish(&c, &country).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let mut reprint = exact;
    reprint["runRef"] = json!("fixture:later-reprint");
    assert_eq!(
        publish(&c, &reprint).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.supply_stock WHERE recipe_id=$1",
            &[&p.recipe]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(c.query_one("SELECT app.publish_supply_window($1,1,1,1,1,$2,'bypass','introduction',NULL,$2,'bypass')", &[&p.recipe,&HASH]).await.unwrap_err().code(),Some(&SqlState::INSUFFICIENT_PRIVILEGE));
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn country_and_global_coverage_cannot_both_publish_when_their_sources_compete() -> TestResult
{
    let c = connection().await?;
    for (first_territory, second_territory) in [("US", "GLOBAL"), ("GLOBAL", "US")] {
        let p = pricing::product(&c, true).await?;
        let mut first = physical(&p, "10000");
        first["territory"] = json!(first_territory);
        let mut second = first.clone();
        second["territory"] = json!(second_territory);
        c.batch_execute("BEGIN").await?;
        assert_eq!(publish(&c, &first).await?["data"]["quantity"], "100");
        let waiting = connection().await?;
        let pid: i32 = waiting
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let task = tokio::spawn(async move { publish(&waiting, &second).await });
        wait_for_database_lock(&c, pid).await?;
        c.batch_execute("COMMIT").await?;
        assert_eq!(
            task.await?.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        assert_eq!(
            c.query_one(
                "SELECT sum(starting_quantity)::text FROM app.supply_stock WHERE recipe_id=$1",
                &[&p.recipe]
            )
            .await?
            .get::<_, String>(0),
            "100"
        );
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unavailable_or_zero_introduction_never_reprices_existing_stock() -> TestResult {
    let c = connection().await?;
    let p = pricing::product(&c, true).await?;
    let policy: Uuid = c
        .query_one(
            "SELECT app.publish_supply_window($1,75,100,1,1,$2,$3,'baseline',NULL,$2,NULL)",
            &[&p.recipe, &HASH, &Uuid::new_v4().to_string()],
        )
        .await?
        .get(0);
    let mut missing = physical(&p, "99");
    missing["qualification"] = json!("unavailable");
    missing["physicalQuantity"] = Value::Null;
    missing["evidenceRef"] = Value::Null;
    assert_eq!(
        publish(&c, &missing).await?["data"]["disposition"],
        "unavailable"
    );
    assert_eq!(
        publish(&c, &physical(&p, "99")).await?["data"]["disposition"],
        "zero"
    );
    assert_eq!(
        c.query_one(
            "SELECT current_pricing_policy_id FROM app.booster_product WHERE id=$1",
            &[&p.product]
        )
        .await?
        .get::<_, Uuid>(0),
        policy
    );
    assert_eq!(
        c.query_one(
            "SELECT sum(starting_quantity)::text FROM app.supply_stock WHERE recipe_id=$1",
            &[&p.recipe]
        )
        .await?
        .get::<_, String>(0),
        "75"
    );
    Ok(())
}
