//! Selected bank SQL policy and normalized priority sources; no native priority qualification.
use super::{TestResult, ante_transaction_integrity as ante, connection};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn accept(
    c: &Client,
    g: Uuid,
    _epoch: &str,
    basis: &str,
    request: Uuid,
    payload: &Value,
) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c
        .query_one(
            "SELECT app.accept_host_candidate($1,'fixture:lifecycle',1,$2::text::numeric,$3,$4)",
            &[&g, &basis, &request, &payload],
        )
        .await?
        .get(0);
    super::admission_port_contracts::candidate_response(
        g,
        "fixture:lifecycle",
        "1",
        basis,
        request,
        payload,
        &response,
    );
    Ok(response)
}
async fn candidate(
    c: &Client,
    m: Uuid,
    g: Uuid,
    basis: &str,
    eligible: Option<Uuid>,
) -> FixtureResult<Value> {
    let context: Uuid = c
        .query_one("SELECT id FROM app.game_context WHERE game_id=$1", &[&g])
        .await?
        .get(0);
    let clocks:Vec<Value>=c.query("SELECT s.id,c.clock_revision::text,c.remaining_ms::text,c.paused FROM app.match_seat s JOIN app.match_clock c ON c.match_seat_id=s.id WHERE s.match_id=$1 ORDER BY s.id",&[&m]).await?.iter().map(|r|{
  let seat:Uuid=r.get(0);let mut v=json!({"seatId":seat,"clockRevision":r.get::<_,String>(1),"remainingMs":r.get::<_,String>(2),"paused":Some(seat)!=eligible});
  if Some(seat)!=eligible && !r.get::<_,bool>(3){v["eligibility"]=json!({"reason":"native_wait","acceptedRevision":basis,"proofRef":"fixture:normalized-native-wait"});}
  if Some(seat)==eligible{v["eligibility"]=json!({"reason":"native_priority","acceptedRevision":basis,"proofRef":"fixture:normalized-native-priority"});}v
 }).collect();
    Ok(
        json!({"contextId":context,"stateRef":"fixture:state","factsRef":"fixture:facts","events":["fixture:clock-event"],"facts":[],"clocks":clocks,"participation":[],"terminal":null}),
    )
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn absolute_millisecond_rounding_is_partition_invariant_and_finite() -> TestResult {
    let c = connection().await?;
    assert!(c.query_one("SELECT app.clock_elapsed_ms('2026-01-01T00:00:00.0001Z','2026-01-01T00:00:00.0008Z')+app.clock_elapsed_ms('2026-01-01T00:00:00.0008Z','2026-01-01T00:00:00.0014Z')=app.clock_elapsed_ms('2026-01-01T00:00:00.0001Z','2026-01-01T00:00:00.0014Z') AND app.clock_elapsed_ms('2026-01-01T00:00:00.0001Z','2026-01-01T00:00:00.0014Z')=1",&[]).await?.get::<_,bool>(0));
    for sql in [
        "SELECT app.clock_elapsed_ms('infinity',now())",
        "SELECT app.clock_elapsed_ms(now(),'2020-01-01')",
        "SELECT app.clock_elapsed_ms(NULL,now())",
    ] {
        assert!(c.query_one(sql, &[]).await.is_err());
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn admitted_bo1_is_explicitly_inactive_and_cannot_start_an_overall_bank() -> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve(&c, true).await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    super::match_lifecycle_integrity::open(&c, m, g).await?;
    assert!(c.query_one("SELECT bool_and(c.remaining_ms=0 AND c.paused AND c.policy_ref='bo1-bank-inactive-v1') FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1",&[&m]).await?.get::<_,bool>(0));
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&m, &f.source.actor],
        )
        .await?
        .get(0);
    let body = candidate(&c, m, g, "0", Some(seat)).await?;
    let result = accept(&c, g, "1", "0", Uuid::new_v4(), &body).await?;
    assert_eq!(result["status"], "fail");
    assert!(c.execute("UPDATE app.match_clock SET paused=false,clock_revision=clock_revision+1 WHERE match_seat_id=$1",&[&seat]).await.is_err());
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn bo3_requires_priority_source_and_debits_the_database_acceptance_boundary() -> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve_match(&c, true, "bo3").await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    super::match_lifecycle_integrity::open(&c, m, g).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&m, &f.source.actor],
        )
        .await?
        .get(0);
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.match_clock SET paused=false,clock_revision=clock_revision+1 WHERE match_seat_id=$1",&[&seat]).await?;
    assert!(c.batch_execute("COMMIT").await.is_err());
    let mut body = candidate(&c, m, g, "0", Some(seat)).await?;
    let index = body["clocks"]
        .as_array()
        .unwrap()
        .iter()
        .position(|v| v["seatId"] == seat.to_string())
        .unwrap();
    let valid = body.clone();
    body["clocks"][index]
        .as_object_mut()
        .unwrap()
        .remove("eligibility");
    assert!(
        accept(&c, g, "1", "0", Uuid::new_v4(), &body)
            .await
            .is_err()
    );
    assert_eq!(
        accept(&c, g, "1", "0", Uuid::new_v4(), &valid).await?["status"],
        "success"
    );
    // Zero-elapsed pause still needs an accepted eligibility-ending source.
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.match_clock SET paused=true,clock_revision=clock_revision+1 WHERE match_seat_id=$1",&[&seat]).await?;
    assert!(c.batch_execute("COMMIT").await.is_err());
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute(
        "UPDATE app.game SET accepted_revision=accepted_revision+1 WHERE id=$1",
        &[&g],
    )
    .await?;
    assert!(c.batch_execute("COMMIT").await.is_err());
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute(
        "UPDATE app.match SET current_game_id=NULL,revision=revision+1 WHERE id=$1",
        &[&m],
    )
    .await?;
    assert!(c.batch_execute("COMMIT").await.is_err());
    let before=c.query_one("SELECT remaining_ms::text,last_accounted_at FROM app.match_clock WHERE match_seat_id=$1",&[&seat]).await?;
    assert_eq!(before.get::<_, String>(0), "1800000");
    c.query_one("SELECT pg_sleep(0.005)", &[]).await?;
    let body = candidate(&c, m, g, "1", None).await?;
    let request = Uuid::new_v4();
    assert_eq!(
        accept(&c, g, "1", "1", request, &body).await?["status"],
        "success"
    );
    assert!(c.query_one("SELECT c.remaining_ms=(source.normalized_source->'before'->(SELECT (ordinality-1)::int FROM jsonb_array_elements(source.normalized_source->'before') WITH ORDINALITY x(value,ordinality) WHERE value->>'seatId'=$1::uuid::text)->>'remainingMs')::numeric-app.clock_elapsed_ms((source.normalized_source->'before'->(SELECT (ordinality-1)::int FROM jsonb_array_elements(source.normalized_source->'before') WITH ORDINALITY x(value,ordinality) WHERE value->>'seatId'=$1::uuid::text)->>'lastAccountedAt')::timestamptz,c.last_accounted_at) AND c.remaining_ms<1800000 AND c.paused FROM app.match_clock c JOIN app.command_receipt source ON source.request_ref=$2::uuid::text AND source.scope='host_candidate_v1' WHERE c.match_seat_id=$1",&[&seat,&request]).await?.get::<_,bool>(0));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn exact_checkpoint_restore_resumes_only_preincident_priority_without_charging_downtime()
-> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve_match(&c, true, "bo3").await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    super::match_lifecycle_integrity::open(&c, m, g).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&m, &f.source.actor],
        )
        .await?
        .get(0);
    assert_eq!(
        accept(
            &c,
            g,
            "1",
            "0",
            Uuid::new_v4(),
            &candidate(&c, m, g, "0", Some(seat)).await?
        )
        .await?["status"],
        "success"
    );
    let mut remaining = String::new();
    for cycle in 0..2 {
        let owner = if cycle == 0 {
            "fixture:lifecycle"
        } else {
            "fixture:restored-owner"
        };
        let epoch = (1 + cycle * 2).to_string();
        let operational = (cycle * 2).to_string();
        let paused:Value=c.query_one("SELECT app.pause_game_incident($1,$2,$3::text::numeric,$4::text::numeric,clock_timestamp(),'storage','fixture:incident-source',$5)",&[&g,&owner,&epoch,&operational,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(paused["status"], "success");
        let checkpoint: Uuid = paused["data"]["checkpointId"].as_str().unwrap().parse()?;
        remaining = c
            .query_one(
                "SELECT remaining_ms::text FROM app.match_clock WHERE match_seat_id=$1",
                &[&seat],
            )
            .await?
            .get(0);
        c.query_one("SELECT pg_sleep(0.005)", &[]).await?;
        let restore_epoch = (2 + cycle * 2).to_string();
        let restore_revision = (1 + cycle * 2).to_string();
        let restored:Value=c.query_one("SELECT app.restore_game_checkpoint($1,$2::text::numeric,$3::text::numeric,$4,'fixture:restored-owner','fixture:exact-native-restoration',$5)",&[&g,&restore_epoch,&restore_revision,&checkpoint,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(restored["status"], "success", "{restored}");
        assert!(c.query_one("SELECT remaining_ms::text=$2 AND NOT paused AND eligibility_receipt_id IS NOT NULL FROM app.match_clock WHERE match_seat_id=$1",&[&seat,&remaining]).await?.get::<_,bool>(0));
        assert!(c.query_one("SELECT bool_and(paused) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1 AND s.id<>$2",&[&m,&seat]).await?.get::<_,bool>(0));
    }
    let body = candidate(&c, m, g, "1", None).await?;
    let accepted: Value = c
        .query_one(
            "SELECT app.accept_host_candidate($1,'fixture:restored-owner',5,1,$2,$3)",
            &[&g, &Uuid::new_v4(), &body],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    assert!(c.query_one("SELECT paused AND remaining_ms<=$2::text::numeric FROM app.match_clock WHERE match_seat_id=$1",&[&seat,&remaining]).await?.get::<_,bool>(0));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stale_repeatable_read_cannot_remove_a_newly_eligible_game_frontier() -> TestResult {
    let c = connection().await?;
    let stale = connection().await?;
    let (f, m) = ante::reserve_match(&c, true, "bo3").await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    super::match_lifecycle_integrity::open(&c, m, g).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&m, &f.source.actor],
        )
        .await?
        .get(0);
    stale
        .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    assert!(stale.query_one("SELECT bool_and(c.paused) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1",&[&m]).await?.get::<_,bool>(0));
    assert_eq!(
        accept(
            &c,
            g,
            "1",
            "0",
            Uuid::new_v4(),
            &candidate(&c, m, g, "0", Some(seat)).await?
        )
        .await?["status"],
        "success"
    );
    stale
        .execute(
            "UPDATE app.match SET current_game_id=NULL,revision=revision+1 WHERE id=$1",
            &[&m],
        )
        .await?;
    assert_eq!(
        stale.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert!(
        c.query_one(
            "SELECT current_game_id=$2 FROM app.match WHERE id=$1",
            &[&m, &g]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}
