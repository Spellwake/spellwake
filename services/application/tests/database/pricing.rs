use super::{TestResult, account_with_age, connection, operation};
use serde_json::{Value, json};
use spellwake_application::supply_source_ports::{
    BoosterOpening, QualifyBoosterOpening, QualifyBoosterRecipe,
};
use spellwake_application::{ApiResponse, commands::CommandAck};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

const EVIDENCE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub(super) struct Product {
    pub(super) product: Uuid,
    pub(super) recipe: Uuid,
    pub(super) slot: Uuid,
    pub(super) cards: [Uuid; 2],
}

fn resource(value: Value) -> Uuid {
    match serde_json::from_value::<ApiResponse<CommandAck>>(value).unwrap() {
        ApiResponse::Success { data } => data.resource_id,
        other => panic!("expected pack purchase acknowledgment: {other:?}"),
    }
}

pub(super) async fn product(
    client: &Client,
    qualify: bool,
) -> Result<Product, Box<dyn std::error::Error + Send + Sync>> {
    product_in_set(client, qualify, None).await
}
/// Recipes for multiple products can share a set without rewriting immutable product identity.
pub(super) async fn product_in_set(
    client: &Client,
    qualify: bool,
    existing_set: Option<Uuid>,
) -> Result<Product, Box<dyn std::error::Error + Send + Sync>> {
    let set = existing_set.unwrap_or_else(Uuid::new_v4);
    let product = Uuid::new_v4();
    let recipe = Uuid::new_v4();
    let slot = Uuid::new_v4();
    if existing_set.is_none() {
        client.execute("INSERT INTO app.card_set(id,external_set_ref,name) VALUES($1,$1::uuid::text,'Test set')", &[&set]).await?;
    }
    client.execute("INSERT INTO app.booster_product(id,set_id,name,external_product_ref) VALUES($1,$2,'Test-only pack',$1::uuid::text)", &[&product,&set]).await?;
    client.execute("INSERT INTO app.booster_recipe(id,product_id,revision_ref,evidence_ref) VALUES($1,$2,'fixture-v1','test-only:recipe-evidence')", &[&recipe,&product]).await?;
    client
        .execute(
            "INSERT INTO app.booster_slot(id,recipe_id,slot_key,draws) VALUES($1,$2,'test-slot',1)",
            &[&slot, &recipe],
        )
        .await?;
    let mut cards = Vec::new();
    for weight in [3i64, 1] {
        let definition = Uuid::new_v4();
        let printing = Uuid::new_v4();
        let asset = Uuid::new_v4();
        client.execute("INSERT INTO app.card_definition(id,oracle_ref,name) VALUES($1,$1::uuid::text,'Test card')", &[&definition]).await?;
        client.execute("INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES($1,$2,$3,$1::uuid::text,'rare','nonfoil','test-only:art')", &[&printing,&definition,&set]).await?;
        client.execute("INSERT INTO app.booster_slot_card(slot_id,printing_id,weight) VALUES($1,$2,$3::bigint)", &[&slot,&printing,&weight]).await?;
        client
            .execute(
                "INSERT INTO app.asset_definition(id,kind,printing_id) VALUES($1,'card',$2)",
                &[&asset, &printing],
            )
            .await?;
        cards.push(asset);
    }
    if qualify {
        let source = QualifyBoosterRecipe::new(recipe, EVIDENCE.to_owned().try_into()?)?;
        client
            .query_one(
                "SELECT app.qualify_booster_recipe($1,$2)",
                &[&source.recipe_id(), &source.manifest().as_str()],
            )
            .await?;
        qualify_independent_opening(client, recipe).await?;
    }
    Ok(Product {
        product,
        recipe,
        slot,
        cards: [cards[0], cards[1]],
    })
}

pub(super) async fn qualify_independent_opening(
    client: &Client,
    recipe: Uuid,
) -> Result<(), tokio_postgres::Error> {
    let plan = json!({"version":"pack-opening-v1","groups":[]});
    let hash: String = client
        .query_one(
            "SELECT 'sha256:'||encode(sha256(convert_to($1::jsonb::text,'UTF8')),'hex')",
            &[&plan],
        )
        .await?
        .get(0);
    let source = QualifyBoosterOpening::new(
        recipe,
        BoosterOpening::from_bytes(&serde_json::to_vec(&plan).unwrap())
            .expect("fixture opening grammar"),
        hash.try_into().expect("database hash grammar"),
    )
    .expect("fixture opening target");
    let retained: Value =
        serde_json::from_str(source.plan().original_json()).expect("retained fixture plan");
    client
        .query_one(
            "SELECT app.qualify_booster_opening($1,$2,$3)",
            &[&source.recipe_id(), &retained, &source.hash().as_str()],
        )
        .await?;
    Ok(())
}

pub(super) async fn publish(
    client: &Client,
    recipe: Uuid,
    quantities: (i64, i64),
    modifier: (i64, i64),
    origin: &str,
    revision: &str,
    coverage: Option<&str>,
) -> Result<Uuid, tokio_postgres::Error> {
    let (quantity, normal) = quantities;
    let (numerator, denominator) = modifier;
    Ok(client.query_one("SELECT app.publish_supply_window($1,$2::bigint,$3::bigint,$4::bigint,$5::bigint,$6,$7,$8,NULL,$6,$9)", &[&recipe,&quantity,&normal,&numerator,&denominator,&EVIDENCE,&revision,&origin,&coverage]).await?.get(0))
}

pub(super) async fn buyer(
    client: &Client,
) -> Result<(Uuid, Uuid), Box<dyn std::error::Error + Send + Sync>> {
    buyer_with_age(client, 0).await
}

pub(super) async fn buyer_with_age(
    client: &Client,
    age_seconds: i64,
) -> Result<(Uuid, Uuid), Box<dyn std::error::Error + Send + Sync>> {
    let (actor, subject) = account_with_age(client, age_seconds).await?;
    client.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    let coin: Uuid = client
        .query_one(
            "SELECT id FROM app.asset_definition WHERE currency_key='coin'",
            &[],
        )
        .await?
        .get(0);
    let inventory: Uuid = client
        .query_one("SELECT app.ensure_inventory($1,$2)", &[&actor, &coin])
        .await?
        .get(0);
    let operation = operation(client, actor).await?;
    client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'credit',14500,'test',$1::uuid::text)", &[&operation,&inventory]).await?;
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&operation],
        )
        .await?;
    Ok((actor, subject))
}

async fn quote(
    client: &Client,
    recipe: Uuid,
    quantity: i32,
) -> Result<Value, tokio_postgres::Error> {
    let value: Value = client
        .query_one("SELECT app.quote_packs($1,$2)", &[&recipe, &quantity])
        .await?
        .get(0);
    serde_json::from_value::<spellwake_application::supply::PackQuote>(value.clone()).unwrap();
    Ok(value)
}

async fn buy(
    client: &Client,
    actor: Uuid,
    quote: &Value,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let recipe: Uuid = quote["recipeId"].as_str().unwrap().parse().unwrap();
    let policy: Uuid = quote["policyId"].as_str().unwrap().parse().unwrap();
    let quantity: i32 = quote["quantity"].as_str().unwrap().parse().unwrap();
    Ok(client
        .query_one(
            "SELECT app.buy_packs($1,$2,$3,$4,$5,$6,$7)",
            &[
                &actor,
                &recipe,
                &quantity,
                &policy,
                &quote["revision"].as_str().unwrap(),
                &quote["total"].as_str().unwrap_or("1"),
                &request,
            ],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn sequential_pack_purchase_is_atomic_source_unique_and_stale_quotes_fail() -> TestResult {
    let client = connection().await?;
    let product = product(&client, true).await?;
    let (actor, _) = buyer(&client).await?;
    let (other, _) = buyer(&client).await?;
    let policy = publish(
        &client,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let quoted = quote(&client, product.recipe, 2).await?;
    assert_eq!(quoted["unitPrice"], "1000");
    assert_eq!(quoted["total"], "2250");
    let request = Uuid::new_v4();
    let result = buy(&client, actor, &quoted, request).await?;
    let operation = resource(result.clone());
    assert_eq!(result, buy(&client, actor, &quoted, request).await?);
    assert_eq!(
        buy(&client, other, &quoted, Uuid::new_v4()).await?["status"],
        "fail"
    );
    let rows = client.query("SELECT d.kind,i.owned_quantity::text FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.owner_id=$1 ORDER BY d.kind", &[&actor]).await?;
    assert_eq!(
        rows.iter()
            .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
            .collect::<Vec<_>>(),
        vec![
            ("coin".to_owned(), "12250".to_owned()),
            ("pack".to_owned(), "2".to_owned())
        ]
    );
    assert_eq!(
        client
            .query_one(
                "SELECT sum(paid_coins)::text FROM app.supply_purchase WHERE operation_id=$1",
                &[&operation]
            )
            .await?
            .get::<_, String>(0),
        "2250"
    );
    assert_eq!(client.query_one("SELECT sum(remaining_quantity)::text FROM app.supply_stock WHERE pricing_policy_id=$1", &[&policy]).await?.get::<_,String>(0),"2");
    assert!(
        client
            .execute(
                "UPDATE app.supply_purchase SET paid_coins=1 WHERE operation_id=$1",
                &[&operation]
            )
            .await
            .is_err()
    );
    // A request cannot be retargeted or silently repriced.
    let mut changed = quoted.clone();
    changed["total"] = json!("1");
    assert!(buy(&client, actor, &changed, request).await.is_err());
    let terminal = quote(&client, product.recipe, 2).await?;
    assert_eq!(terminal["total"], "3250");
    resource(buy(&client, actor, &terminal, Uuid::new_v4()).await?);
    let sold_out = quote(&client, product.recipe, 1).await?;
    assert_eq!(sold_out["unitPrice"], "2000");
    assert_eq!(sold_out["purpose"], "valuation_only");
    assert!(sold_out["total"].is_null());
    assert_eq!(
        buy(&client, actor, &sold_out, Uuid::new_v4()).await?["status"],
        "fail"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn carryover_republishes_one_window_and_retains_original_purchase_provenance() -> TestResult {
    let client = connection().await?;
    let product = product(&client, true).await?;
    let (actor, _) = buyer(&client).await?;
    let old_revision = Uuid::new_v4().to_string();
    let old_policy = publish(
        &client,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &old_revision,
        None,
    )
    .await?;
    let old = resource(
        buy(
            &client,
            actor,
            &quote(&client, product.recipe, 2).await?,
            Uuid::new_v4(),
        )
        .await?,
    );
    let revision = Uuid::new_v4().to_string();
    let current = publish(
        &client,
        product.recipe,
        (2, 4),
        (1, 1),
        "introduction",
        &revision,
        Some("fixture-coverage-a"),
    )
    .await?;
    assert_eq!(
        current,
        publish(
            &client,
            product.recipe,
            (2, 4),
            (1, 1),
            "introduction",
            &revision,
            Some("fixture-coverage-a")
        )
        .await?
    );
    assert!(
        publish(
            &client,
            product.recipe,
            (3, 4),
            (1, 1),
            "introduction",
            &revision,
            Some("fixture-coverage-a")
        )
        .await
        .is_err()
    );
    assert_eq!(
        old_policy,
        publish(
            &client,
            product.recipe,
            (4, 4),
            (1, 1),
            "baseline",
            &old_revision,
            None
        )
        .await?
    );
    assert_eq!(
        quote(&client, product.recipe, 1).await?["policyId"],
        current.to_string()
    );
    assert!(
        publish(
            &client,
            product.recipe,
            (0, 4),
            (1, 1),
            "introduction",
            &Uuid::new_v4().to_string(),
            Some("fixture-zero")
        )
        .await
        .is_err()
    );
    let quoted = quote(&client, product.recipe, 3).await?;
    assert_eq!(quoted["total"], "3750");
    let purchase = resource(buy(&client, actor, &quoted, Uuid::new_v4()).await?);
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.supply_purchase WHERE operation_id=$1",
                &[&purchase]
            )
            .await?
            .get::<_, i64>(0),
        2
    );
    assert_eq!(
        client
            .query_one(
                "SELECT pricing_policy_id FROM app.supply_purchase WHERE operation_id=$1",
                &[&old]
            )
            .await?
            .get::<_, Uuid>(0),
        old_policy
    );
    assert_eq!(
        client
            .query_one(
                "SELECT current_pricing_policy_id FROM app.booster_product WHERE id=$1",
                &[&product.product]
            )
            .await?
            .get::<_, Uuid>(0),
        current
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn exact_recipe_values_refuse_unqualified_sources_and_preserve_fractional_bundles()
-> TestResult {
    let client = connection().await?;
    let product = product(&client, false).await?;
    let lines = json!([{"assetId":product.cards[0],"quantity":"3"}]);
    assert!(
        client
            .query_one("SELECT app.assess_bundle($1)", &[&lines])
            .await
            .is_err()
    );
    assert!(quote(&client, product.recipe, 1).await.is_err());
    client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&product.recipe, &EVIDENCE],
        )
        .await?;
    publish(
        &client,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let common: Value = client
        .query_one("SELECT app.assess_bundle($1)", &[&lines])
        .await?
        .get(0);
    let _: spellwake_application::policy::Rational =
        serde_json::from_value(common["total"].clone())?;
    assert!(common["policyId"].as_str().is_some());
    assert!(common["catalogueRevision"].as_str().is_some());
    assert_eq!(
        common["total"],
        json!({"numerator":"2000","denominator":"1"})
    );
    assert_eq!(
        common["lines"][0]["unitAssessment"]["value"],
        json!({"numerator":"2000","denominator":"3"})
    );
    let rare: Value = client
        .query_one(
            "SELECT app.assess_bundle($1)",
            &[&json!([{"assetId":product.cards[1],"quantity":"1"}])],
        )
        .await?
        .get(0);
    assert_eq!(rare["total"], json!({"numerator":"2000","denominator":"1"}));
    assert!(
        client
            .execute(
                "UPDATE app.booster_slot_card SET weight=2 WHERE slot_id=$1",
                &[&product.slot]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "INSERT INTO app.booster_slot(recipe_id,slot_key,draws) VALUES($1,'late-slot',1)",
                &[&product.recipe]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.booster_recipe SET evidence_ref='changed' WHERE id=$1",
                &[&product.recipe]
            )
            .await
            .is_err()
    );
    client.batch_execute("SET ROLE authenticated").await?;
    assert!(
        client
            .query_one("SELECT app.assess_bundle($1)", &[&lines])
            .await
            .is_err()
    );
    assert!(
        client
            .query_one("SELECT app.quote_packs($1,1)", &[&product.recipe])
            .await
            .is_err()
    );
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    let _: Value = client
        .query_one("SELECT app.assess_bundle($1)", &[&lines])
        .await?
        .get(0);
    assert!(
        client
            .execute(
                "UPDATE app.supply_stock SET remaining_quantity=0 WHERE recipe_id=$1",
                &[&product.recipe]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn scarce_modified_prices_match_rust_and_competing_buyers_cannot_double_spend_stock()
-> TestResult {
    let client = connection().await?;
    let product = product(&client, true).await?;
    publish(
        &client,
        product.recipe,
        (2, 4),
        (3, 2),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let quoted = quote(&client, product.recipe, 2).await?;
    let rust = spellwake_application::policy::PricingWindow::new(
        2.into(),
        2.into(),
        4.into(),
        spellwake_application::policy::Rational::new(3.into(), 2.into())?,
    )?;
    assert_eq!(quoted["unitPrice"], rust.valuation_quote()?.to_string());
    assert_eq!(quoted["total"], rust.purchase_total(2)?.to_string());
    let (left, _) = buyer(&client).await?;
    let (right, _) = buyer(&client).await?;
    let other = connection().await?;
    let (a, b) = tokio::join!(
        buy(&client, left, &quoted, Uuid::new_v4()),
        buy(&other, right, &quoted, Uuid::new_v4())
    );
    let status = [
        a?["status"].as_str().unwrap().to_owned(),
        b?["status"].as_str().unwrap().to_owned(),
    ];
    assert_eq!(status.iter().filter(|s| *s == "success").count(), 1);
    assert_eq!(status.iter().filter(|s| *s == "fail").count(), 1);
    assert_eq!(quote(&client, product.recipe, 1).await?["remaining"], "0");
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn repeatable_read_cannot_qualify_a_stale_recipe_or_accept_a_stale_price() -> TestResult {
    let client = connection().await?;
    let product = product(&client, false).await?;
    let mut stale = connection().await?;
    let transaction = stale
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    transaction
        .query_one(
            "SELECT content_revision::text FROM app.booster_recipe WHERE id=$1",
            &[&product.recipe],
        )
        .await?;
    client
        .execute(
            "UPDATE app.booster_slot_card SET weight=weight+1 WHERE slot_id=$1",
            &[&product.slot],
        )
        .await?;
    let error = transaction
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&product.recipe, &EVIDENCE],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::T_R_SERIALIZATION_FAILURE));
    transaction.rollback().await?;
    client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&product.recipe, &EVIDENCE],
        )
        .await?;
    qualify_independent_opening(&client, product.recipe).await?;
    publish(
        &client,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let (actor, _) = buyer(&client).await?;
    let old_quote = quote(&client, product.recipe, 1).await?;
    let transaction = stale
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    transaction
        .query_one(
            "SELECT pricing_revision::text FROM app.booster_product WHERE id=$1",
            &[&product.product],
        )
        .await?;
    resource(buy(&client, actor, &old_quote, Uuid::new_v4()).await?);
    let error = transaction
        .query_one("SELECT app.quote_packs($1,1)", &[&product.recipe])
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::T_R_SERIALIZATION_FAILURE));
    transaction.rollback().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn correlated_marginals_and_identical_asset_minimum_do_not_assume_uniform_rarity()
-> TestResult {
    let client = connection().await?;
    let first = product(&client, false).await?;
    client.execute("UPDATE app.booster_slot SET draws=2,collation_ref='test-only:without-replacement-artifact' WHERE id=$1", &[&first.slot]).await?;
    assert!(
        client
            .query_one(
                "SELECT app.qualify_booster_recipe($1,$2)",
                &[&first.recipe, &EVIDENCE]
            )
            .await
            .is_err()
    );
    client.execute("UPDATE app.booster_slot_card SET expected_numerator=1,expected_denominator=1 WHERE slot_id=$1", &[&first.slot]).await?;
    client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&first.recipe, &EVIDENCE],
        )
        .await?;
    publish(
        &client,
        first.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let bundle = json!([{"assetId":first.cards[0],"quantity":"1"}]);
    let assessed: Value = client
        .query_one("SELECT app.assess_bundle($1)", &[&bundle])
        .await?
        .get(0);
    // Both candidates occur exactly once, despite 3:1 sampling weights. The
    // reviewed marginal artifact, rather than independent draws, controls value.
    assert_eq!(
        assessed["total"],
        json!({"numerator":"500","denominator":"1"})
    );
    let second = product(&client, false).await?;
    let printing: Uuid = client
        .query_one(
            "SELECT printing_id FROM app.asset_definition WHERE id=$1",
            &[&first.cards[0]],
        )
        .await?
        .get(0);
    client
        .execute(
            "INSERT INTO app.booster_slot_card(slot_id,printing_id,weight) VALUES($1,$2,4)",
            &[&second.slot, &printing],
        )
        .await?;
    // An unpublished/unqualified alternative is not an invented price basis.
    assert_eq!(
        client
            .query_one("SELECT app.assess_bundle($1)", &[&bundle])
            .await?
            .get::<_, Value>(0)["total"],
        assessed["total"]
    );
    client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&second.recipe, &EVIDENCE],
        )
        .await?;
    publish(
        &client,
        second.recipe,
        (4, 4),
        (1, 2),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let minimum: Value = client
        .query_one("SELECT app.assess_bundle($1)", &[&bundle])
        .await?
        .get(0);
    assert_eq!(
        minimum["total"],
        json!({"numerator":"1000","denominator":"3"})
    );
    assert_eq!(
        minimum["lines"][0]["unitAssessment"]["basis"]["quote"]["recipeId"],
        second.recipe.to_string()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn exchange_limit_is_exact_and_held_coins_or_deleting_accounts_cannot_buy() -> TestResult {
    let client = connection().await?;
    let product = product(&client, true).await?;
    publish(
        &client,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let (actor, _) = buyer(&client).await?;
    let coin: Uuid = client
        .query_one(
            "SELECT id FROM app.asset_definition WHERE currency_key='coin'",
            &[],
        )
        .await?
        .get(0);
    let outgoing = json!([{"assetId":product.cards[0],"quantity":"3"}]);
    for (amount, allowed) in [("1538", false), ("1539", true)] {
        let incoming = json!([{"assetId":coin,"quantity":amount}]);
        let assessment: Value = client
            .query_one("SELECT app.assess_exchange($1,$2)", &[&outgoing, &incoming])
            .await?
            .get(0);
        assert_eq!(assessment["outgoingAllowed"], allowed);
    }
    let gift: Value = client
        .query_one("SELECT app.assess_exchange('[]'::jsonb,$1)", &[&outgoing])
        .await?
        .get(0);
    assert_eq!(gift["outgoingAllowed"], true);
    let inventory: Uuid = client
        .query_one(
            "SELECT id FROM app.inventory_asset WHERE owner_id=$1 AND asset_definition_id=$2",
            &[&actor, &coin],
        )
        .await?
        .get(0);
    let op = operation(&client, actor).await?;
    let hold = Uuid::new_v4();
    client.execute("INSERT INTO app.asset_hold(id,owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,'ante',$1::uuid::text,'held',$3)", &[&hold,&actor,&op]).await?;
    client
        .execute(
            "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,14000)",
            &[&hold, &inventory],
        )
        .await?;
    let quoted = quote(&client, product.recipe, 1).await?;
    assert_eq!(
        buy(&client, actor, &quoted, Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(quote(&client, product.recipe, 1).await?["remaining"], "4");
    client
        .execute(
            "UPDATE app.asset_hold SET state='released' WHERE id=$1",
            &[&hold],
        )
        .await?;
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&actor]).await?;
    assert_eq!(
        buy(&client, actor, &quoted, Uuid::new_v4()).await?["status"],
        "fail"
    );
    client.batch_execute("SET ROLE service_role").await?;
    assert!(
        client
            .execute(
                "UPDATE app.booster_product SET current_pricing_policy_id=NULL WHERE id=$1",
                &[&product.product]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.booster_product SET pricing_revision=0 WHERE id=$1",
                &[&product.product]
            )
            .await
            .is_err()
    );
    assert_eq!(
        client
            .execute(
                "UPDATE app.booster_product SET name='Renamed test pack' WHERE id=$1",
                &[&product.product],
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    client.batch_execute("RESET ROLE").await?;
    // Catalogue fixture setup is owner-only; service callers need an explicit writer.
    client
        .execute(
            "UPDATE app.booster_product SET name='Renamed test pack' WHERE id=$1",
            &[&product.product],
        )
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn catalogue_changes_wait_for_assessments_and_invalidate_old_repeatable_read_snapshots()
-> TestResult {
    let mut client = connection().await?;
    let first = product(&client, true).await?;
    publish(
        &client,
        first.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let second = product(&client, false).await?;
    let printing: Uuid = client
        .query_one(
            "SELECT printing_id FROM app.asset_definition WHERE id=$1",
            &[&first.cards[0]],
        )
        .await?
        .get(0);
    client
        .execute(
            "INSERT INTO app.booster_slot_card(slot_id,printing_id,weight) VALUES($1,$2,4)",
            &[&second.slot, &printing],
        )
        .await?;
    let bundle = json!([{"assetId":first.cards[0],"quantity":"1"}]);
    let reader = client.transaction().await?;
    reader
        .query_one("SELECT app.assess_bundle($1)", &[&bundle])
        .await?;
    let observer = connection().await?;
    let writer = connection().await?;
    let name = format!("catalogue-fence-{}", Uuid::new_v4());
    writer
        .query_one("SELECT set_config('application_name',$1,false)", &[&name])
        .await?;
    let recipe = second.recipe;
    let task = tokio::spawn(async move {
        writer
            .query_one(
                "SELECT app.qualify_booster_recipe($1,$2)",
                &[&recipe, &EVIDENCE],
            )
            .await
    });
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
        "qualification must wait for the transaction's entire assessment"
    );
    reader.commit().await?;
    task.await??;
    let mut stale = connection().await?;
    let old = stale
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    old.query_one(
        "SELECT catalogue_revision::text FROM app.valuation_policy_revision WHERE state='active'",
        &[],
    )
    .await?;
    publish(
        &observer,
        second.recipe,
        (4, 4),
        (1, 2),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let error = old
        .query_one("SELECT app.assess_bundle($1)", &[&bundle])
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::T_R_SERIALIZATION_FAILURE));
    old.rollback().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn multiple_slots_sum_expected_copies_and_finishes_count_as_distinct_outputs() -> TestResult {
    let client = connection().await?;
    let product = product(&client, false).await?;
    let row = client.query_one("SELECT p.id,p.definition_id,p.set_id,p.external_printing_ref FROM app.asset_definition a JOIN app.printing p ON p.id=a.printing_id WHERE a.id=$1", &[&product.cards[0]]).await?;
    let nonfoil: Uuid = row.get(0);
    let definition: Uuid = row.get(1);
    let set: Uuid = row.get(2);
    let reference: String = row.get(3);
    let foil = Uuid::new_v4();
    let foil_asset = Uuid::new_v4();
    client.execute("INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES($1,$2,$3,$4,'rare','foil','test-only:foil-art')", &[&foil,&definition,&set,&reference]).await?;
    client
        .execute(
            "INSERT INTO app.asset_definition(id,kind,printing_id) VALUES($1,'card',$2)",
            &[&foil_asset, &foil],
        )
        .await?;
    let slot = Uuid::new_v4();
    client.execute("INSERT INTO app.booster_slot(id,recipe_id,slot_key,draws) VALUES($1,$2,'bonus-finish',1)", &[&slot,&product.recipe]).await?;
    for printing in [nonfoil, foil] {
        client
            .execute(
                "INSERT INTO app.booster_slot_card(slot_id,printing_id,weight) VALUES($1,$2,1)",
                &[&slot, &printing],
            )
            .await?;
    }
    client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&product.recipe, &EVIDENCE],
        )
        .await?;
    publish(
        &client,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    for (asset, numerator, denominator) in [
        (product.cards[0], "800", "3"),
        (product.cards[1], "4000", "3"),
        (foil_asset, "2000", "3"),
    ] {
        let assessed: Value = client
            .query_one(
                "SELECT app.assess_bundle($1)",
                &[&json!([{"assetId":asset,"quantity":"1"}])],
            )
            .await?
            .get(0);
        assert_eq!(
            assessed["total"],
            json!({"numerator":numerator,"denominator":denominator})
        );
        assert_eq!(
            assessed["lines"][0]["unitAssessment"]["basis"]["candidateCount"],
            "3"
        );
        let _: spellwake_application::policy::Rational =
            serde_json::from_value(assessed["total"].clone())?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn catalogue_retries_preserve_epoch_and_product_revision() -> TestResult {
    let mut client = connection().await?;
    let product = product(&client, true).await?;
    let revision = Uuid::new_v4().to_string();
    let policy = publish(
        &client,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &revision,
        None,
    )
    .await?;
    let transaction = client.transaction().await?;
    transaction
        .query_one("SELECT app.lock_valuation_catalogue(true)", &[])
        .await?;
    let state_sql = "SELECT v.catalogue_revision::text,p.pricing_revision::text FROM app.valuation_policy_revision v CROSS JOIN app.booster_product p WHERE v.state='active' AND p.id=$1";
    let before = transaction
        .query_one(state_sql, &[&product.product])
        .await?;
    transaction
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&product.recipe, &EVIDENCE],
        )
        .await?;
    assert_eq!(
        transaction
            .query_one(
                "SELECT app.publish_supply_window($1,4,4,1,1,$2,$3,'baseline',NULL,$2,NULL)",
                &[&product.recipe, &EVIDENCE, &revision]
            )
            .await?
            .get::<_, Uuid>(0),
        policy
    );
    let after = transaction
        .query_one(state_sql, &[&product.product])
        .await?;
    assert_eq!(before.get::<_, String>(0), after.get::<_, String>(0));
    assert_eq!(before.get::<_, String>(1), after.get::<_, String>(1));
    transaction.commit().await?;
    Ok(())
}
