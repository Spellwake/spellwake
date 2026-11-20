//! Synthetic authored source contracts and actual PostgreSQL budget/hash oracles.
use super::{TestResult, connection, pricing};
use serde_json::{Value, json};
use spellwake_application::supply_source_ports::*;
use uuid::Uuid;
const HASH: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn physical_original_jsonb_budget_matches_postgres_through_the_exact_boundary() -> TestResult
{
    let c = connection().await?;
    let mut source = json!({"version":"physical-introduction-v1","recipeId":Uuid::new_v4(),"productId":Uuid::new_v4(),"runRef":"🂡\n\\\"é","language":"en","territory":"GLOBAL","unit":"unknown","qualification":"unavailable","physicalQuantity":null,"evidenceRef":"","modifierN":"1","modifierD":"1","modifierRef":HASH,"normalQuantity":"100"});
    let base: i32 = c
        .query_one("SELECT octet_length($1::jsonb::text)", &[&source])
        .await?
        .get(0);
    let base = usize::try_from(base)?;
    let parsed = PhysicalIntroduction::from_bytes(&serde_json::to_vec(&source)?)?;
    assert_eq!(parsed.original_terms_octets(), Some(base));
    for size in [65536usize, 65537] {
        source["evidenceRef"] = json!("x".repeat(size - base));
        let actual: i32 = c
            .query_one("SELECT octet_length($1::jsonb::text)", &[&source])
            .await?
            .get(0);
        assert_eq!(usize::try_from(actual)?, size);
        let parsed = PhysicalIntroduction::from_bytes(&serde_json::to_vec(&source)?);
        if size == 65536 {
            assert_eq!(parsed?.original_terms_octets(), Some(size));
        } else {
            assert!(parsed.is_err());
        }
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn recipe_and_opening_qualification_use_actual_void_idempotency_and_postgres_hash()
-> TestResult {
    let mut c = connection().await?;
    let p = pricing::product(&c, false).await?;
    let recipe = QualifyBoosterRecipe::new(p.recipe, HASH.to_owned().try_into()?)?;
    c.query_one(
        "SELECT app.qualify_booster_recipe($1,$2)",
        &[&recipe.recipe_id(), &recipe.manifest().as_str()],
    )
    .await?;
    // Other fixtures legitimately advance this shared epoch. Keep this retry's
    // before/after observation under its canonical row lock, rather than
    // attributing an unrelated concurrent publication to this recipe retry.
    let observation = c.transaction().await?;
    let before: String = observation
        .query_one("SELECT catalogue_revision::text FROM app.valuation_policy_revision WHERE state='active' FOR NO KEY UPDATE", &[])
        .await?
        .get(0);
    observation
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&recipe.recipe_id(), &recipe.manifest().as_str()],
        )
        .await?;
    assert_eq!(
        observation.query_one("SELECT catalogue_revision::text FROM app.valuation_policy_revision WHERE state='active'", &[])
            .await?
            .get::<_, String>(0),
        before
    );
    observation.commit().await?;
    let changed = format!("sha256:{}", "b".repeat(64));
    assert!(
        c.query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&recipe.recipe_id(), &changed]
        )
        .await
        .is_err()
    );
    let raw = r#"{ "version": "pack-opening-v1", "groups": [] }"#;
    let plan = BoosterOpening::from_bytes(raw.as_bytes())?;
    let value: Value = serde_json::from_str(plan.original_json())?;
    let hash: String = c
        .query_one(
            "SELECT 'sha256:'||encode(sha256(convert_to($1::jsonb::text,'UTF8')),'hex')",
            &[&value],
        )
        .await?
        .get(0);
    let source = QualifyBoosterOpening::new(p.recipe, plan, hash.try_into()?)?;
    for _ in 0..2 {
        c.query_one(
            "SELECT app.qualify_booster_opening($1,$2,$3)",
            &[&source.recipe_id(), &value, &source.hash().as_str()],
        )
        .await?;
    }
    assert!(
        c.query_one(
            "SELECT app.qualify_booster_opening($1,$2,$3)",
            &[&source.recipe_id(), &value, &changed]
        )
        .await
        .is_err()
    );
    let actual: i32 = c
        .query_one("SELECT octet_length($1::jsonb::text)", &[&value])
        .await?
        .get(0);
    assert_eq!(
        source.plan().original_terms_octets(),
        Some(usize::try_from(actual)?)
    );
    Ok(())
}
