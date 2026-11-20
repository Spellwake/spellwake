//! Caller-bound shop visibility and current pricing over actual guarded fixture products.
use super::{TestResult, connection, pricing, set_auth_claims};
use serde_json::{Value, json};
use spellwake_application::{
    amount::Amount,
    pack_shop::{OwnedPackQuote, PackProductPage},
    supply::PackQuantity,
};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pack_shop_requires_live_caller_and_executable_published_product() -> TestResult {
    let client = connection().await?;
    let (actor, subject) = pricing::buyer(&client).await?;
    let ready = pricing::product(&client, true).await?;
    let draft = pricing::product(&client, false).await?;
    let unpublished = pricing::product(&client, true).await?;
    let valued_only = pricing::product(&client, false).await?;
    client.query_one("SELECT app.qualify_booster_recipe($1,'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')", &[&valued_only.recipe]).await?;
    pricing::publish(
        &client,
        valued_only.recipe,
        (10, 10),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;

    let policy = pricing::publish(
        &client,
        ready.recipe,
        (10, 10),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    set_auth_claims(&client, subject).await?;
    let mut after: Option<Uuid> = None;
    let mut visible = Vec::new();
    loop {
        let value: Value = client
            .query_one("SELECT app.read_my_pack_products($1,32)", &[&after])
            .await?
            .get(0);
        let page: PackProductPage = serde_json::from_value(value["data"].clone())?;
        page.validate(after, 32)?;
        visible.extend(page.items.iter().map(|p| p.id));
        after = page.next_cursor;
        if after.is_none() {
            break;
        }
    }
    assert!(visible.contains(&ready.recipe));
    assert!(
        !visible.contains(&draft.recipe)
            && !visible.contains(&unpublished.recipe)
            && !visible.contains(&valued_only.recipe)
    );
    let value: Value = client
        .query_one("SELECT app.read_my_pack_quote($1,2)", &[&ready.recipe])
        .await?
        .get(0);
    let quote: OwnedPackQuote = serde_json::from_value(value["data"].clone())?;
    quote.validate(ready.recipe, PackQuantity::try_from(Amount::from(2))?)?;
    assert_eq!(value["data"]["quote"]["policyId"], policy.to_string());
    assert_eq!(value["data"]["quote"]["total"], "2100");
    let input = json!({"kind":"pack_buy","command":{"requestId":Uuid::new_v4(),"recipeId":ready.recipe,"quantity":"2","expectedPolicyId":policy,
        "expectedRevision":value["data"]["quote"]["revision"],"expectedTotal":"2100"}});
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    let purchase =
        super::economy_commands::execute_for_actor(&client, actor, &input, false).await?;
    assert_eq!(purchase["status"], "success");
    set_auth_claims(&client, subject).await?;
    let refreshed: Value = client
        .query_one("SELECT app.read_my_pack_quote($1,1)", &[&ready.recipe])
        .await?
        .get(0);
    assert_eq!(refreshed["data"]["quote"]["remaining"], "8");
    assert_eq!(refreshed["data"]["quote"]["unitPrice"], "1200");
    assert_eq!(refreshed["data"]["quote"]["total"], "1200");
    let unavailable: Value = client
        .query_one("SELECT app.read_my_pack_quote($1,1)", &[&draft.recipe])
        .await?
        .get(0);
    let missing: Value = client
        .query_one("SELECT app.read_my_pack_quote($1,1)", &[&Uuid::new_v4()])
        .await?
        .get(0);
    assert_eq!(unavailable, missing);
    let value: Value = client
        .query_one("SELECT app.read_my_pack_quote($1,11)", &[&ready.recipe])
        .await?
        .get(0);
    assert_eq!(value["data"]["quote"]["purpose"], "valuation_only");
    assert!(value["data"]["quote"]["total"].is_null());
    let quote: OwnedPackQuote = serde_json::from_value(value["data"].clone())?;
    quote.validate(ready.recipe, PackQuantity::try_from(Amount::from(11))?)?;
    for limit in [0i32, 33] {
        let value: Value = client
            .query_one("SELECT app.read_my_pack_products(NULL,$1)", &[&limit])
            .await?
            .get(0);
        assert_eq!(value["status"], "fail");
    }
    for quantity in [0i32, 1001] {
        let value: Value = client
            .query_one(
                "SELECT app.read_my_pack_quote($1,$2)",
                &[&ready.recipe, &quantity],
            )
            .await?
            .get(0);
        assert_eq!(value["status"], "fail");
    }
    client.batch_execute("RESET ROLE").await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for (signature, allowed) in [
            ("app.pack_product_json(app.booster_recipe)", false),
            (
                "app.read_my_pack_products(uuid,integer)",
                role == "authenticated",
            ),
            (
                "app.read_my_pack_quote(uuid,integer)",
                role == "authenticated",
            ),
        ] {
            let execute: bool = client
                .query_one(
                    "SELECT has_function_privilege($1,$2,'EXECUTE')",
                    &[&role, &signature],
                )
                .await?
                .get(0);
            assert_eq!(execute, allowed, "{role}:{signature}");
        }
    }
    client
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 hour' WHERE id=$1",
            &[&subject],
        )
        .await?;
    set_auth_claims(&client, subject).await?;
    let denied: Value = client
        .query_one("SELECT app.read_my_pack_products(NULL,32)", &[])
        .await?
        .get(0);
    assert_eq!(denied["status"], "fail");
    let denied: Value = client
        .query_one("SELECT app.read_my_pack_quote($1,1)", &[&ready.recipe])
        .await?
        .get(0);
    assert_eq!(denied["status"], "fail");
    Ok(())
}
