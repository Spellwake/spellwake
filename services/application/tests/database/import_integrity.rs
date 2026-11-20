//! Atomic database import; text resolution, API routing and editor integration are later layers.
use super::{TestResult, connection, decks};
use serde_json::{Value, json};
use tokio_postgres::error::SqlState;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn import_publishes_a_named_deck_and_its_sealed_contents_once_with_both_confirmed_ids()
-> TestResult {
    let c = connection().await?;
    let (actor, _, _, definition, format) = decks::library(&c).await?;
    let entries =
        json!([{"definitionId":definition,"printingId":null,"section":"main","quantity":"4"}]);
    let request = Uuid::new_v4();
    let sql = "SELECT app.import_deck($1,'Imported deck','collection',$2,$3,$4)";
    let accepted: Value = c
        .query_one(sql, &[&actor, &format, &entries, &request])
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    let deck: Uuid = accepted["data"]["resourceId"].as_str().unwrap().parse()?;
    let revision: Uuid = accepted["data"]["revisionId"].as_str().unwrap().parse()?;
    let row = c.query_one("SELECT d.name,d.current_revision_id,r.sealed_at IS NOT NULL,e.quantity::text FROM app.deck d JOIN app.deck_revision r ON r.deck_id=d.id JOIN app.deck_entry e ON e.revision_id=r.id WHERE d.id=$1", &[&deck]).await?;
    assert_eq!(row.get::<_, String>(0), "Imported deck");
    assert_eq!(row.get::<_, Uuid>(1), revision);
    assert!(row.get::<_, bool>(2));
    assert_eq!(row.get::<_, String>(3), "4");
    assert_eq!(
        c.query_one(sql, &[&actor, &format, &entries, &request])
            .await?
            .get::<_, Value>(0),
        accepted
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.deck_revision WHERE deck_id=$1",
            &[&deck]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    let changed =
        json!([{"definitionId":definition,"printingId":null,"section":"main","quantity":"5"}]);
    assert_eq!(
        c.query_one(sql, &[&actor, &format, &changed, &request])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn invalid_import_rolls_back_deck_creation_and_nested_receipts_but_retains_one_rejection()
-> TestResult {
    let c = connection().await?;
    let (actor, _, _, definition, format) = decks::library(&c).await?;
    let before: i64 = c
        .query_one(
            "SELECT count(*)::bigint FROM app.deck WHERE account_id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    let request = Uuid::new_v4();
    let entries =
        json!([{"definitionId":definition,"printingId":null,"section":"main","quantity":"-1"}]);
    let sql = "SELECT app.import_deck($1,'Rejected import','collection',$2,$3,$4)";
    let rejected: Value = c
        .query_one(sql, &[&actor, &format, &entries, &request])
        .await?
        .get(0);
    assert_eq!(rejected["status"], "fail");
    assert_eq!(
        c.query_one(sql, &[&actor, &format, &entries, &request])
            .await?
            .get::<_, Value>(0),
        rejected
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.deck WHERE account_id=$1",
            &[&actor]
        )
        .await?
        .get::<_, i64>(0),
        before
    );
    let row = c.query_one("SELECT id FROM app.command_receipt WHERE account_id=$1 AND scope='deck_import' AND request_ref=$2", &[&actor,&request.to_string()]).await?;
    let receipt: Uuid = row.get(0);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE account_id=$1 AND request_ref=$2", &[&actor,&receipt.to_string()]).await?.get::<_, i64>(0), 0);
    Ok(())
}
