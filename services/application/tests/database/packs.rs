use super::{
    TestResult, connection, operation,
    pricing::{self, Product},
};
use serde_json::{Value, json};
use spellwake_application::packs::PackOutcome;
use tokio_postgres::Client;
use uuid::Uuid;

/// Fixture-only original-claim read; restore the caller's role before returning.
async fn read_outcome(
    client: &Client,
    actor: Uuid,
    opened: Uuid,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let role: String = client
        .query_one("SELECT current_user::text", &[])
        .await?
        .get(0);
    assert!(matches!(
        role.as_str(),
        "postgres" | "service_role" | "authenticated" | "anon"
    ));
    client.batch_execute("RESET ROLE").await?;
    let subject: Uuid = client
        .query_one(
            "SELECT auth_subject FROM app.account WHERE id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    super::set_auth_claims(client, subject).await?;
    let result = client
        .query_one("SELECT app.read_my_pack_open($1)", &[&opened])
        .await;
    client.batch_execute("RESET ROLE").await?;
    client.batch_execute(&format!("SET ROLE {role}")).await?;
    let value: Value = result?.get(0);
    if value["status"] != "success" {
        return Err("owned pack result unavailable".into());
    }
    let parsed: spellwake_application::packs::OwnedPackOutcome =
        serde_json::from_value(value["data"].clone())?;
    parsed.validate(opened)?;
    assert_eq!(parsed.account_id, actor);
    Ok(value["data"]["outcome"].clone())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn caller_pack_outcomes_hide_foreign_missing_and_private_seed_data() -> TestResult {
    let client = connection().await?;
    let (_, actor, inventory) = purchased(&client, 1).await?;
    let (other, _) = pricing::buyer(&client).await?;
    client.batch_execute("SET ROLE service_role").await?;
    let opened=super::economy_commands::execute_for_actor(&client,actor,&json!({"kind":"pack_open","command":{"requestId":Uuid::new_v4(),"inventoryAssetId":inventory}}),false).await?;
    let opened: Uuid = opened["data"]["resourceId"].as_str().unwrap().parse()?;
    let outcome = read_outcome(&client, actor, opened).await?;
    assert!(outcome.get("seed").is_none());
    assert!(read_outcome(&client, other, opened).await.is_err());
    assert!(read_outcome(&client, other, Uuid::new_v4()).await.is_err());
    super::deletion::request(&client, actor, Uuid::new_v4()).await?;
    assert_eq!(read_outcome(&client, actor, opened).await?, outcome);
    client.batch_execute("RESET ROLE").await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        assert_eq!(
            client
                .query_one(
                    "SELECT has_function_privilege($1,'app.read_my_pack_open(uuid)','EXECUTE')",
                    &[&role]
                )
                .await?
                .get::<_, bool>(0),
            role == "authenticated"
        );
        assert!(
            !client
                .query_one(
                    "SELECT has_function_privilege($1,'app.read_pack_open(uuid,uuid)','EXECUTE')",
                    &[&role]
                )
                .await?
                .get::<_, bool>(0)
        );
    }
    Ok(())
}

async fn qualify_opening(
    client: &Client,
    recipe: Uuid,
    plan: &Value,
) -> Result<(), tokio_postgres::Error> {
    let parsed = spellwake_application::supply_source_ports::BoosterOpening::from_bytes(
        &serde_json::to_vec(plan).unwrap(),
    )
    .expect("fixture joint/multiset grammar");
    assert_eq!(
        serde_json::from_str::<Value>(parsed.original_json()).unwrap(),
        *plan
    );
    client.query_one("SELECT app.qualify_booster_opening($1,$2,'sha256:'||encode(sha256(convert_to($2::jsonb::text,'UTF8')),'hex'))", &[&recipe,plan]).await?;
    Ok(())
}

pub(super) async fn purchased(
    client: &Client,
    quantity: i32,
) -> Result<(Product, Uuid, Uuid), Box<dyn std::error::Error + Send + Sync>> {
    let product = pricing::product(client, true).await?;
    qualify_opening(
        client,
        product.recipe,
        &json!({"version":"pack-opening-v1","groups":[]}),
    )
    .await?;
    let (actor, _) = pricing::buyer(client).await?;
    pricing::publish(
        client,
        product.recipe,
        (10, 10),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let quote: Value = client
        .query_one(
            "SELECT app.quote_packs($1,$2)",
            &[&product.recipe, &quantity],
        )
        .await?
        .get(0);
    let policy: Uuid = quote["policyId"].as_str().unwrap().parse()?;
    let result: Value = client
        .query_one(
            "SELECT app.buy_packs($1,$2,$3,$4,$5,$6,$7)",
            &[
                &actor,
                &product.recipe,
                &quantity,
                &policy,
                &quote["revision"].as_str().unwrap(),
                &quote["total"].as_str().unwrap(),
                &Uuid::new_v4(),
            ],
        )
        .await?
        .get(0);
    assert_eq!(result["status"], "success");
    let inventory=client.query_one("SELECT i.id FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.owner_id=$1 AND d.recipe_id=$2", &[&actor,&product.recipe]).await?.get(0);
    Ok((product, actor, inventory))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn purchase_requires_executable_recipe_and_rejected_terms_remain_final() -> TestResult {
    let client = connection().await?;
    let product = pricing::product(&client, false).await?;
    client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[
                &product.recipe,
                &"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ],
        )
        .await?;
    let (actor, _) = pricing::buyer(&client).await?;
    pricing::publish(
        &client,
        product.recipe,
        (10, 10),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let quote: Value = client
        .query_one("SELECT app.quote_packs($1,1)", &[&product.recipe])
        .await?
        .get(0);
    let policy: Uuid = quote["policyId"].as_str().unwrap().parse()?;
    let request = Uuid::new_v4();
    client.batch_execute("SET ROLE service_role").await?;
    let rejected: Value = super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_buy","command":{"requestId":request,"recipeId":product.recipe,"quantity":"1","expectedPolicyId":policy,"expectedRevision":quote["revision"],"expectedTotal":quote["total"]}}),false)
        .await?
        .get(0);
    assert_eq!(rejected["status"], "fail");
    assert!(
        client
            .query_one(
                "SELECT app.buy_packs_priced($1,$2,1,$3,$4,$5,$6)",
                &[
                    &actor,
                    &product.recipe,
                    &policy,
                    &quote["revision"].as_str().unwrap(),
                    &quote["total"].as_str().unwrap(),
                    &Uuid::new_v4()
                ]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(client.query_one("SELECT count(*) FROM app.ledger_operation WHERE actor_account_id=$1 AND kind='pack_purchase'",&[&actor]).await?.get::<_,i64>(0),0);
    pricing::qualify_independent_opening(&client, product.recipe).await?;
    client.batch_execute("SET ROLE service_role").await?;
    let repeated: Value = super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_buy","command":{"requestId":request,"recipeId":product.recipe,"quantity":"1","expectedPolicyId":policy,"expectedRevision":quote["revision"],"expectedTotal":quote["total"]}}),false)
        .await?
        .get(0);
    assert_eq!(repeated, rejected);
    let accepted: Value = super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_buy","command":{"requestId":Uuid::new_v4(),"recipeId":product.recipe,"quantity":"1","expectedPolicyId":policy,"expectedRevision":quote["revision"],"expectedTotal":quote["total"]}}),false)
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn opening_rollback_and_deferred_result_proof_preserve_inventory() -> TestResult {
    let client = connection().await?;
    let (_, actor, inventory) = purchased(&client, 1).await?;
    let request = Uuid::new_v4();
    // Exercise a response lost before transaction commit, then a fresh committed retry.
    client.batch_execute("BEGIN").await?;
    let pending: Value = super::economy_commands::execute_row_for_actor(
        &client,
        actor,
        &json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":inventory}}),
        false,
    )
    .await?
    .get(0);
    assert_eq!(pending["status"], "success");
    client.batch_execute("ROLLBACK").await?;
    for corruption in [
        "UPDATE app.ledger_posting SET quantity=quantity+1 WHERE operation_id=$1 AND direction='credit'",
        "UPDATE app.pack_open_receipt SET result_ref=jsonb_set(result_ref::jsonb,'{contents,0,quantity}','\"2\"'::jsonb)::text WHERE operation_id=$1",
        "DELETE FROM app.ledger_posting WHERE operation_id=$1 AND direction='credit'",
    ] {
        client.batch_execute("BEGIN").await?;
        let result: Value = super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":inventory}}), false)
            .await?
            .get(0);
        let opened: Uuid = result["data"]["resourceId"].as_str().unwrap().parse()?;
        let operation: Uuid = client
            .query_one(
                "SELECT operation_id FROM app.pack_open_receipt WHERE id=$1",
                &[&opened],
            )
            .await?
            .get(0);
        // Test-owner fault injection bypasses only immediate guards. Deferred events
        // were queued by the real writer and must reject the inconsistent transaction.
        client
            .batch_execute("SET LOCAL session_replication_role=replica")
            .await?;
        assert_eq!(client.execute(corruption, &[&operation]).await?, 1);
        client
            .batch_execute("SET LOCAL session_replication_role=origin")
            .await?;
        assert!(
            client
                .batch_execute("SET CONSTRAINTS ALL IMMEDIATE")
                .await
                .is_err()
        );
        client.batch_execute("ROLLBACK").await?;
        assert_eq!(
            client
                .query_one(
                    "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                    &[&inventory]
                )
                .await?
                .get::<_, String>(0),
            "1"
        );
        assert_eq!(
            client
                .query_one(
                    "SELECT count(*) FROM app.pack_open_receipt WHERE inventory_asset_id=$1",
                    &[&inventory]
                )
                .await?
                .get::<_, i64>(0),
            0
        );
    }
    let accepted: Value = super::economy_commands::execute_row_for_actor(
        &client,
        actor,
        &json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":inventory}}),
        false,
    )
    .await?
    .get(0);
    assert_eq!(accepted["status"], "success");
    let retry: Value = super::economy_commands::execute_row_for_actor(
        &client,
        actor,
        &json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":inventory}}),
        false,
    )
    .await?
    .get(0);
    assert_eq!(retry, accepted);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pack_sampler_matches_independent_large_integer_reference() -> TestResult {
    use num_bigint::BigUint;
    use sha2::{Digest, Sha256};
    let client = connection().await?;
    let seed: Vec<u8> = (0..32).collect();
    for maximum in [
        "1".to_owned(),
        "3".to_owned(),
        (BigUint::from(10u8).pow(78) + BigUint::from(7u8)).to_string(),
        (BigUint::from(10u8).pow(82) - BigUint::from(1u8)).to_string(),
    ] {
        let max: BigUint = maximum.parse()?;
        let space = BigUint::from(1u8) << 512;
        let threshold = &space - &space % &max;
        for counter in [0i64, 1, 999_999_999] {
            let mut next = counter;
            let expected = loop {
                let mut bytes = Vec::new();
                for block in 0..2 {
                    let mut hash = Sha256::new();
                    hash.update(&seed);
                    hash.update(format!("spellwake-pack-v1:{next}:{block}"));
                    bytes.extend_from_slice(&hash.finalize());
                }
                next += 1;
                let value = BigUint::from_bytes_be(&bytes);
                if value < threshold {
                    break (value % &max).to_string();
                }
                assert!(next - counter < 16);
            };
            let actual=client.query_one("SELECT sample::text,next_counter FROM app.pack_sample_v1($1,$2,$3::text::numeric)",&[&seed,&counter,&maximum]).await?;
            assert_eq!(actual.get::<_, String>(0), expected);
            assert_eq!(actual.get::<_, i64>(1), next);
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn opening_consumes_one_available_pack_and_commits_same_private_outcome_on_retry()
-> TestResult {
    let client = connection().await?;
    let (product, actor, inventory) = purchased(&client, 2).await?;
    let request = Uuid::new_v4();
    client.batch_execute("SET ROLE service_role").await?;
    let result: Value = super::economy_commands::execute_row_for_actor(
        &client,
        actor,
        &json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":inventory}}),
        false,
    )
    .await?
    .get(0);
    assert_eq!(result["status"], "success");
    let opened: Uuid = result["data"]["resourceId"].as_str().unwrap().parse()?;
    let repeated: Value = super::economy_commands::execute_row_for_actor(
        &client,
        actor,
        &json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":inventory}}),
        false,
    )
    .await?
    .get(0);
    assert_eq!(result, repeated);
    let public = read_outcome(&client, actor, opened).await?;
    let outcome: PackOutcome = serde_json::from_value(public.clone())?;
    outcome.validate()?;
    assert_eq!(outcome.recipe_id, product.recipe);
    assert_eq!(outcome.contents.len(), 1);
    assert_eq!(outcome.contents[0].quantity.to_string(), "1");
    assert!(public.get("seed").is_none());
    assert!(
        client
            .query_one(
                "SELECT app.read_pack_open($1,$2)",
                &[&Uuid::new_v4(), &opened]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.pack_open_receipt SET result_ref='{}' WHERE id=$1",
                &[&opened]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .query_one("SELECT app.new_pack_seed_v1()", &[])
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(
        client
            .query_one(
                "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, String>(0),
        "1"
    );
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.pack_open_receipt WHERE inventory_asset_id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    assert!(
        client
            .execute(
                "UPDATE app.pack_open_receipt SET result_ref='{}' WHERE id=$1",
                &[&opened]
            )
            .await
            .is_err()
    );
    let private: Value = client
        .query_one(
            "SELECT result_ref::jsonb FROM app.pack_open_receipt WHERE id=$1",
            &[&opened],
        )
        .await?
        .get(0);
    let replay: Value = client
        .query_one(
            "SELECT app.draw_booster_recipe_v1($1,decode($2,'hex'))",
            &[&product.recipe, &private["seed"].as_str().unwrap()],
        )
        .await?
        .get(0);
    assert_eq!(replay, private["contents"]);
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&actor]).await?;
    client.batch_execute("SET ROLE service_role").await?;
    let history = read_outcome(&client, actor, opened).await?;
    assert_eq!(history, public);
    let stopped: Value = super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_open","command":{"requestId":Uuid::new_v4(),"inventoryAssetId":inventory}}), false)
        .await?
        .get(0);
    assert_eq!(stopped["status"], "fail");
    let accepted_retry: Value = super::economy_commands::execute_row_for_actor(
        &client,
        actor,
        &json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":inventory}}),
        false,
    )
    .await?
    .get(0);
    assert_eq!(accepted_retry, result);
    client.batch_execute("RESET ROLE").await?;
    for role in ["anon", "authenticated"] {
        client.batch_execute(&format!("SET ROLE {role}")).await?;
        assert!(
            super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_open","command":{"requestId":Uuid::new_v4(),"inventoryAssetId":inventory}}), false)
                .await
                .is_err()
        );
        client.batch_execute("RESET ROLE").await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn held_packs_do_not_open_and_competing_requests_cannot_consume_one_pack_twice() -> TestResult
{
    let client = connection().await?;
    let (_, actor, inventory) = purchased(&client, 1).await?;
    let op = operation(&client, actor).await?;
    let hold = Uuid::new_v4();
    client.execute("INSERT INTO app.asset_hold(id,owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,'ante',$1::uuid::text,'held',$3)",&[&hold,&actor,&op]).await?;
    client
        .execute(
            "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,1)",
            &[&hold, &inventory],
        )
        .await?;
    let request = Uuid::new_v4();
    client.batch_execute("SET ROLE service_role").await?;
    let rejected: Value = super::economy_commands::execute_row_for_actor(
        &client,
        actor,
        &json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":inventory}}),
        false,
    )
    .await?
    .get(0);
    assert_eq!(rejected["status"], "fail");
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE app.asset_hold SET state='released' WHERE id=$1",
            &[&hold],
        )
        .await?;
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        super::economy_commands::execute_row_for_actor(&client, actor, &json!({"kind":"pack_open","command":{"requestId":request,"inventoryAssetId":inventory}}), false)
            .await?
            .get::<_, Value>(0),
        rejected
    );
    let other = connection().await?;
    other.batch_execute("SET ROLE service_role").await?;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let left = async {
        super::economy_commands::execute_row_for_actor(
            &client,
            actor,
            &json!({"kind":"pack_open","command":{"requestId":a,"inventoryAssetId":inventory}}),
            false,
        )
        .await
    };
    let right = async {
        super::economy_commands::execute_row_for_actor(
            &other,
            actor,
            &json!({"kind":"pack_open","command":{"requestId":b,"inventoryAssetId":inventory}}),
            false,
        )
        .await
    };
    let (left, right) = tokio::join!(left, right);
    let statuses = [left?.get::<_, Value>(0), right?.get::<_, Value>(0)];
    assert_eq!(
        statuses.iter().filter(|r| r["status"] == "success").count(),
        1
    );
    assert_eq!(statuses.iter().filter(|r| r["status"] == "fail").count(), 1);
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(
        client
            .query_one(
                "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, String>(0),
        "0"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn joint_outcomes_preserve_cross_slot_dependencies_and_plan_qualification_is_immutable()
-> TestResult {
    let client = connection().await?;
    let product = pricing::product(&client, false).await?;
    client
        .execute(
            "UPDATE app.booster_slot SET collation_ref='joint-test' WHERE id=$1",
            &[&product.slot],
        )
        .await?;
    client.execute("UPDATE app.booster_slot_card SET expected_numerator=1,expected_denominator=2 WHERE slot_id=$1", &[&product.slot]).await?;
    let second = Uuid::new_v4();
    client.execute("INSERT INTO app.booster_slot(id,recipe_id,slot_key,draws,collation_ref) VALUES($1,$2,'second',1,'joint-test')", &[&second,&product.recipe]).await?;
    client.execute("INSERT INTO app.booster_slot_card(slot_id,printing_id,weight,expected_numerator,expected_denominator) SELECT $1,printing_id,1,1,2 FROM app.booster_slot_card WHERE slot_id=$2", &[&second,&product.slot]).await?;
    client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,'sha256:'||repeat('a',64))",
            &[&product.recipe],
        )
        .await?;
    let printings: Vec<Uuid> = client
        .query(
            "SELECT printing_id FROM app.booster_slot_card WHERE slot_id=$1 ORDER BY printing_id",
            &[&product.slot],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    let card = |slot, printing| json!({"slotId":slot,"printingId":printing,"quantity":"1"});
    let plan = json!({"version":"pack-opening-v1","groups":[{"collationRef":"joint-test","kind":"joint","outcomes":[{"weight":"1","cards":[card(product.slot,printings[0]),card(second,printings[1])]},{"weight":"1","cards":[card(product.slot,printings[1]),card(second,printings[0])]}]}]});
    assert!(
        qualify_opening(
            &client,
            product.recipe,
            &json!({"version":"pack-opening-v1","groups":[]})
        )
        .await
        .is_err()
    );
    let mut wrong = plan.clone();
    wrong["groups"][0]["outcomes"][0]["weight"] = json!("2");
    assert!(
        qualify_opening(&client, product.recipe, &wrong)
            .await
            .is_err()
    );
    qualify_opening(&client, product.recipe, &plan).await?;
    qualify_opening(&client, product.recipe, &plan).await?;
    assert!(
        qualify_opening(&client, product.recipe, &wrong)
            .await
            .is_err()
    );
    for n in 0..16i32 {
        let contents:Value=client.query_one("SELECT app.draw_booster_recipe_v1($1,sha256(convert_to($2::integer::text,'UTF8')))", &[&product.recipe,&n]).await?.get(0);
        assert_eq!(contents.as_array().unwrap().len(), 2);
        assert!(
            contents
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["quantity"] == "1")
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn weighted_multiset_without_replacement_depletes_authored_units_exactly() -> TestResult {
    let client = connection().await?;
    let product = pricing::product(&client, false).await?;
    client
        .execute(
            "UPDATE app.booster_slot SET draws=4,collation_ref='sheet-test' WHERE id=$1",
            &[&product.slot],
        )
        .await?;
    client.execute("UPDATE app.booster_slot_card SET expected_numerator=weight,expected_denominator=1 WHERE slot_id=$1", &[&product.slot]).await?;
    client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,'sha256:'||repeat('a',64))",
            &[&product.recipe],
        )
        .await?;
    let plan = json!({"version":"pack-opening-v1","groups":[{"collationRef":"sheet-test","kind":"multiset-without-replacement"}]});
    qualify_opening(&client, product.recipe, &plan).await?;
    for n in 0..16i32 {
        let contents:Value=client.query_one("SELECT app.draw_booster_recipe_v1($1,sha256(convert_to($2::integer::text,'UTF8')))", &[&product.recipe,&n]).await?.get(0);
        let quantities: Vec<_> = contents
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["quantity"].as_str().unwrap())
            .collect();
        assert_eq!(quantities.len(), 2);
        assert!(quantities.contains(&"3") && quantities.contains(&"1"));
    }
    Ok(())
}
