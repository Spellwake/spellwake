//! Actual SQL136 producer compatibility; synthetic admitted facts are not a running AI.
use super::{TestResult, ante_transaction_integrity::reserve, connection};
use serde_json::{Value, json};
use spellwake_application::ai_ante_port::*;
use uuid::Uuid;

async fn execute(
    c: &tokio_postgres::Client,
    p: &PreparedAiAnte,
) -> Result<Value, tokio_postgres::Error> {
    let s = p.source();
    c.query_one(
        "SELECT app.set_ante_stake($1,$2,$3::text::numeric,'[]'::jsonb,$4::text::numeric,$5)",
        &[
            &s.match_id,
            &s.seat_id,
            &s.expected_terms_revision.to_string(),
            &s.coins.to_string(),
            &s.request_id,
        ],
    )
    .await
    .map(|r| r.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn original_ai_stakes_generate_one_start_day_exposure_and_replay_after_later_terms()
-> TestResult {
    let c = connection().await?;
    let (f, m) = reserve(&c, false).await?;
    // AI allowance needs the coin definition, without crediting a sponsor wallet.
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    let row=c.query_one("SELECT s.id,s.sponsor_id,to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') FROM app.match_seat s JOIN app.match m ON m.id=s.match_id WHERE s.match_id=$1 AND s.controller_kind='ai'", &[&m]).await?;
    let context = AdmittedAiAnteContext {
        match_id: m,
        seat_id: row.get(0),
        sponsor_id: row.get(1),
        match_started_at: row.get::<_, String>(2).parse()?,
        commitment_id: None,
        exposure_id: None,
    };
    assert_eq!(context.sponsor_id, f.source.actor);
    let source = AiAnteStake {
        request_id: Uuid::new_v4(),
        match_id: m,
        seat_id: context.seat_id,
        expected_terms_revision: 0.into(),
        coins: 100.into(),
    };
    let first = PreparedAiAnte::new(source, context)?;
    let original = execute(&c, &first).await?;
    assert_eq!(original["status"], "success");
    first.decode_response(&serde_json::to_vec(&original)?)?;
    let commitment: Uuid = original["data"]["resourceId"].as_str().unwrap().parse()?;
    let row=c.query_one("SELECT x.id,b.sponsor_id,b.utc_day FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id WHERE x.commitment_id=$1", &[&commitment]).await?;
    let exposure: Uuid = row.get(0);
    assert_eq!(row.get::<_, Uuid>(1), first.context().sponsor_id);
    assert_eq!(row.get::<_, String>(2), first.allowance_day().to_string());
    for (basis, coins) in [(1, 50), (2, 0), (3, 300)] {
        let context = AdmittedAiAnteContext {
            commitment_id: Some(commitment),
            exposure_id: Some(exposure),
            ..first.context().clone()
        };
        let source = AiAnteStake {
            expected_terms_revision: basis.into(),
            coins: coins.into(),
            request_id: Uuid::new_v4(),
            ..first.source().clone()
        };
        let prepared = PreparedAiAnte::new(source, context)?;
        let result = execute(&c, &prepared).await?;
        prepared.decode_response(&serde_json::to_vec(&result)?)?;
        let d = json!({"state":"accepted","response":result});
        prepared.decode_disposition(
            prepared.source(),
            prepared.context(),
            &serde_json::to_vec(&d)?,
        )?;
        assert_eq!(execute(&c, &first).await?, original);
        first.decode_disposition(
            first.source(),
            first.context(),
            &serde_json::to_vec(&json!({"state":"accepted","response":original}))?,
        )?;
    }
    let row=c.query_one("SELECT count(*)::bigint,min(reserved_coins)::text,min(b.lost_coins)::text FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id WHERE x.commitment_id=$1", &[&commitment]).await?;
    assert_eq!(row.get::<_, i64>(0), 1);
    assert_eq!(row.get::<_, String>(1), "300");
    assert_eq!(row.get::<_, String>(2), "0");
    let changed = AiAnteStake {
        coins: 200.into(),
        ..first.source().clone()
    };
    let changed = PreparedAiAnte::new(changed, first.context().clone())?;
    let error = execute(&c, &changed).await.unwrap_err();
    assert_eq!(error.code().map(|c| c.code()), Some("23514"));
    assert_eq!(execute(&c, &first).await?, original);
    Ok(())
}
