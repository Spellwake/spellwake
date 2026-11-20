//! Tiny retained bank fixtures qualify SQL timeout composition, not room admission or native timeout truth.
use super::{TestResult, connection};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct Fixture {
    game: Uuid,
    matched: Uuid,
    loser: Uuid,
    winner: Uuid,
    context: Uuid,
    initial_checkpoint: Uuid,
    commitment: Option<Uuid>,
}
async fn candidate(c: &Client, f: &Fixture, terminal: bool) -> FixtureResult<Value> {
    let rows=c.query("SELECT s.id,s.account_id,c.clock_revision::text,c.remaining_ms::text FROM app.match_seat s JOIN app.match_clock c ON c.match_seat_id=s.id WHERE s.match_id=$1 ORDER BY s.id",&[&f.matched]).await?;
    let clocks:Vec<Value>=rows.iter().map(|r|{let seat:Uuid=r.get(0);let mut v=json!({"seatId":seat,"clockRevision":r.get::<_,String>(2),"remainingMs":r.get::<_,String>(3),"paused":terminal || seat!=f.loser});if !terminal && seat==f.loser{v["eligibility"]=json!({"reason":"native_priority","acceptedRevision":"0","proofRef":"fixture:priority"});}v}).collect();
    let mut result = json!({"contextId":f.context,"stateRef":"fixture:state","factsRef":"fixture:facts","events":["fixture:bank-event"],"facts":[],"clocks":clocks,"terminal":null,"participation":[]});
    if terminal {
        result["terminal"] = json!({"outcome":"win","winnerSeatIds":[f.winner],"winnersRef":"fixture:native-timeout-winner","reason":"fixture:bank-forfeit"});
        result["participation"]=json!(rows.iter().map(|r|json!({"seatId":r.get::<_,Uuid>(0),"ownTurns":"3","reason":"fixture:normalized-participation"})).collect::<Vec<_>>());
        let revision = rows
            .iter()
            .find(|r| r.get::<_, Uuid>(0) == f.loser)
            .unwrap()
            .get::<_, String>(2);
        result["bankTimeout"] = json!({"kind":"overall_bank","seatId":f.loser,"clockRevision":revision,"proofRef":"fixture:qualified-native-timeout"});
    }
    Ok(result)
}
async fn accept(
    c: &Client,
    f: &Fixture,
    basis: &str,
    request: Uuid,
    payload: &Value,
) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c
        .query_one(
            "SELECT app.accept_host_candidate($1,'fixture-host',1,$2::text::numeric,$3,$4)",
            &[&f.game, &basis, &request, &payload],
        )
        .await?
        .get(0);
    super::admission_port_contracts::candidate_response(
        f.game,
        "fixture-host",
        "1",
        basis,
        request,
        payload,
        &response,
    );
    Ok(response)
}
async fn fixture(c: &Client, milliseconds: &str) -> FixtureResult<Fixture> {
    fixture_options(c, milliseconds, false, false).await
}
async fn fixture_options(
    c: &Client,
    milliseconds: &str,
    competitive: bool,
    ante: bool,
) -> FixtureResult<Fixture> {
    c.batch_execute("BEGIN").await?;
    // Reuse qualified dictionaries and both real saved revisions. The independently
    // retained short bank below remains separate from the ordinary room-start policy.
    let room = super::room_start_integrity::room(c, true).await?;
    let seed = &room.source;
    c.execute("UPDATE app.match_seat SET controller_kind='human',account_id=$2,sponsor_id=NULL WHERE id=$1",&[&seed.ai,&room.other]).await?;
    c.execute(
        "UPDATE app.match SET structure='bo3',competitive=$2 WHERE id=$1",
        &[&seed.match_id, &competitive],
    )
    .await?;
    let inputs = json!([{"seatId":seed.human,"revisionId":seed.revision,"snapshotRef":"fixture:copied-loser"},{"seatId":seed.ai,"revisionId":room.other_revision,"snapshotRef":"fixture:copied-winner"}]);
    let admitted: Value = c
        .query_one(
            "SELECT app.accept_match_decks($1,0,$2,'fixture:qualified-deck-copies',$3,$4)",
            &[&seed.match_id, &seed.release, &inputs, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(admitted["status"], "success");
    let game:Uuid=c.query_one("INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES($1,0,'reserved',0) RETURNING id",&[&seed.match_id]).await?.get(0);
    c.execute("UPDATE app.match SET state='ready',started_at=clock_timestamp(),reserved_game_id=$2 WHERE id=$1",&[&seed.match_id,&game]).await?;
    let commitment = if ante {
        let coin = super::ante_transaction_integrity::fund(c, seed.actor).await?;
        super::ante_transaction_integrity::fund(c, room.other).await?;
        let first = super::ante_transaction_integrity::offer(
            c,
            seed.match_id,
            seed.human,
            "0",
            json!([{"assetId":coin,"quantity":"10"}]),
            None,
        )
        .await?;
        assert_eq!(first["status"], "success");
        let second = super::ante_transaction_integrity::offer(
            c,
            seed.match_id,
            seed.ai,
            "1",
            json!([{"assetId":coin,"quantity":"10"}]),
            None,
        )
        .await?;
        assert_eq!(second["status"], "success");
        let id: Uuid = second["data"]["resourceId"].as_str().unwrap().parse()?;
        let hash = second["data"]["termsHash"].as_str().unwrap();
        super::ante_transaction_integrity::accept(c, id, seed.human, "2", hash).await?;
        assert_eq!(
            super::ante_transaction_integrity::accept(c, id, seed.ai, "2", hash).await?["data"]["state"],
            "committed"
        );
        Some(id)
    } else {
        None
    };
    c.execute(
        "UPDATE app.match SET state='active',current_game_id=$2,reserved_game_id=NULL WHERE id=$1",
        &[&seed.match_id, &game],
    )
    .await?;
    c.execute("UPDATE app.game SET state='active' WHERE id=$1", &[&game])
        .await?;
    let context:Uuid=c.query_one("INSERT INTO app.game_context(game_id,release_id,profile_id,snapshot_ref,context_revision) VALUES($1,$2,$3,'fixture:initial','fixture:v1') RETURNING id",&[&game,&seed.release,&seed.profile]).await?.get(0);
    c.execute("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES($1,$2,0,0,'fixture:initial-event')",&[&game,&context]).await?;
    // Explicit retained opening source supplies a short historical test bank.
    // This is deliberately not evidence that room admission selected this duration.
    let opening:Uuid=c.query_one("INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref) VALUES('fixture-retained-opening','game_opening_v1',$1::uuid::text,$1::uuid::text,app.auth_json_hash('{}'::jsonb),'pending','') RETURNING id",&[&game]).await?.get(0);
    c.execute("INSERT INTO app.match_clock(match_seat_id,remaining_ms,clock_revision,policy_ref,paused,last_accounted_at,eligibility_receipt_id) VALUES($1,$3::text::numeric,0,'bo3-overall-bank-v1',true,clock_timestamp(),$4),($2,1800000,0,'bo3-overall-bank-v1',true,clock_timestamp(),$4)",&[&seed.human,&seed.ai,&milliseconds,&opening]).await?;
    c.execute("UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-opening-v1','gameId',$1::uuid,'matchId',$2::uuid,'clockValues',app.match_clock_snapshot($2)) WHERE id=$3",&[&game,&seed.match_id,&opening]).await?;
    c.query_one(
        "SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid))",
        &[&opening, &game],
    )
    .await?;
    let initial_checkpoint:Uuid=c.query_one("INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision) VALUES($1,$2,0,'fixture:initial','fixture:initial-facts',0) RETURNING id",&[&game,&context]).await?.get(0);
    c.execute("INSERT INTO app.operational_state(game_id,owner_instance_ref,fence_epoch,revision,availability) VALUES($1,'fixture-host',1,0,'available')",&[&game]).await?;
    let f = Fixture {
        game,
        matched: seed.match_id,
        loser: seed.human,
        winner: seed.ai,
        context,
        initial_checkpoint,
        commitment,
    };
    assert_eq!(
        accept(c, &f, "0", Uuid::new_v4(), &candidate(c, &f, false).await?).await?["status"],
        "success"
    );
    c.batch_execute("COMMIT").await?;
    Ok(f)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn first_game_bank_loss_finalizes_bo3_once_without_a_second_win_or_sideboard() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "1").await?;
    c.query_one("SELECT pg_sleep(0.005)", &[]).await?;
    let body = candidate(&c, &f, true).await?;
    let request = Uuid::new_v4();
    let accepted = accept(&c, &f, "1", request, &body).await?;
    assert_eq!(accepted["status"], "success", "{accepted}");
    assert_eq!(accept(&c, &f, "1", request, &body).await?, accepted);
    let result: Uuid = c
        .query_one(
            "SELECT id FROM app.game_result WHERE game_id=$1",
            &[&f.game],
        )
        .await?
        .get(0);
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES($1,1,'reserved',0)",&[&f.matched]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let completed = super::match_lifecycle_integrity::consume(&c, f.matched, result).await?;
    assert_eq!(completed["data"]["phase"], "terminal");
    assert_eq!(
        super::match_lifecycle_integrity::consume(&c, f.matched, result).await?,
        completed
    );
    let row=c.query_one("SELECT r.terminal_kind,r.winner_seat_ids,cardinality(r.final_game_ids),(SELECT count(*)::bigint FROM app.between_game_decision WHERE match_id=r.match_id),(SELECT count(*)::bigint FROM app.game WHERE match_id=r.match_id) FROM app.match_result r WHERE r.match_id=$1",&[&f.matched]).await?;
    assert_eq!(row.get::<_, String>(0), "bank_forfeit");
    assert_eq!(row.get::<_, Vec<Uuid>>(1), vec![f.winner]);
    assert_eq!(row.get::<_, i32>(2), 1);
    assert_eq!(row.get::<_, i64>(3), 0);
    assert_eq!(row.get::<_, i64>(4), 1);
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.between_game_decision(match_id,seat_id,decision_kind,basis_revision,clock_revision,state) VALUES($1,$2,'ready',0,0,'pending')",&[&f.matched,&f.loser]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );

    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute(
        "UPDATE app.match SET state='ready',revision=revision+1 WHERE id=$1",
        &[&f.matched],
    )
    .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES($1,1,'reserved',0)",&[&f.matched]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );

    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn premature_timeout_keeps_the_same_request_retryable_until_the_bank_expires() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "1000").await?;
    let body = candidate(&c, &f, true).await?;
    let request = Uuid::new_v4();
    assert_eq!(
        accept(&c, &f, "1", request, &body)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert!(!c.query_one("SELECT EXISTS(SELECT 1 FROM app.command_receipt WHERE request_ref=$1::uuid::text AND scope='host_candidate_v1')",&[&request]).await?.get::<_,bool>(0));
    c.query_one("SELECT pg_sleep(1.1)", &[]).await?;
    assert_eq!(
        accept(&c, &f, "1", request, &body).await?["status"],
        "success"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn incident_recorded_zero_can_finish_after_restore_without_resuming_the_exhausted_bank()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "1").await?;
    c.query_one("SELECT pg_sleep(0.005)", &[]).await?;
    let paused:Value=c.query_one("SELECT app.pause_game_incident($1,'fixture-host',1,0,clock_timestamp(),'storage','fixture:incident',$2)",&[&f.game,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(paused["status"], "success");
    let checkpoint: Uuid = paused["data"]["checkpointId"].as_str().unwrap().parse()?;
    let restored:Value=c.query_one("SELECT app.restore_game_checkpoint($1,2,1,$2,'fixture-restored','fixture:exact-restore',$3)",&[&f.game,&checkpoint,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(restored["status"], "success");
    assert!(c.query_one("SELECT paused AND remaining_ms=0 AND app.bank_exhaustion_source(eligibility_receipt_id,match_seat_id) FROM app.match_clock WHERE match_seat_id=$1",&[&f.loser]).await?.get::<_,bool>(0));
    let again:Value=c.query_one("SELECT app.pause_game_incident($1,'fixture-restored',3,2,clock_timestamp(),'storage','fixture:second-incident',$2)",&[&f.game,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(again["status"], "success");
    let restored_again:Value=c.query_one("SELECT app.restore_game_checkpoint($1,4,3,$2,'fixture-restored','fixture:second-exact-restore',$3)",&[&f.game,&checkpoint,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(restored_again["status"], "success");
    assert!(c.query_one("SELECT app.bank_exhaustion_source(eligibility_receipt_id,match_seat_id) FROM app.match_clock WHERE match_seat_id=$1",&[&f.loser]).await?.get::<_,bool>(0));
    let body = candidate(&c, &f, true).await?;
    let accepted: Value = c
        .query_one(
            "SELECT app.accept_host_candidate($1,'fixture-restored',5,1,$2,$3)",
            &[&f.game, &Uuid::new_v4(), &body],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success", "{accepted}");
    let result: Uuid = c
        .query_one(
            "SELECT id FROM app.game_result WHERE game_id=$1",
            &[&f.game],
        )
        .await?
        .get(0);
    assert_eq!(
        super::match_lifecycle_integrity::consume(&c, f.matched, result).await?["data"]["phase"],
        "terminal"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ordinary_acceptance_cannot_launder_an_unsupported_pause_in_the_same_transaction()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "1800000").await?;
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.match_clock SET paused=true,clock_revision=clock_revision+1 WHERE match_seat_id=$1",&[&f.loser]).await?;
    let mut body = candidate(&c, &f, false).await?;
    for v in body["clocks"].as_array_mut().unwrap() {
        if v["seatId"] == f.loser.to_string() {
            v["eligibility"]["acceptedRevision"] = json!("1");
        }
    }
    assert_eq!(
        accept(&c, &f, "1", Uuid::new_v4(), &body)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("ROLLBACK").await?;
    assert!(
        c.query_one(
            "SELECT NOT paused FROM app.match_clock WHERE match_seat_id=$1",
            &[&f.loser]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn bank_win_reuses_coin_ante_rank_reward_quest_and_replay_consumers_once() -> TestResult {
    let c = connection().await?;
    c.execute("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) SELECT extract(year FROM now() AT TIME ZONE 'UTC')::int,extract(quarter FROM now() AT TIME ZONE 'UTC')::int,date_trunc('quarter',now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC',(date_trunc('quarter',now() AT TIME ZONE 'UTC')+interval '3 months') AT TIME ZONE 'UTC','fixture:rank-policy','active' ON CONFLICT(year,quarter) DO NOTHING",&[]).await?;
    let f = fixture_options(&c, "1", true, true).await?;
    c.query_one("SELECT pg_sleep(0.005)", &[]).await?;
    let body = candidate(&c, &f, true).await?;
    assert_eq!(
        accept(&c, &f, "1", Uuid::new_v4(), &body).await?["status"],
        "success"
    );
    let game_result: Uuid = c
        .query_one(
            "SELECT id FROM app.game_result WHERE game_id=$1",
            &[&f.game],
        )
        .await?
        .get(0);
    let finished = super::match_lifecycle_integrity::consume(&c, f.matched, game_result).await?;
    let result: Uuid = finished["data"]["terminalResultId"]
        .as_str()
        .unwrap()
        .parse()?;
    let commitment = f.commitment.unwrap();
    let settlement: Value = c
        .query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result])
        .await?
        .get(0);
    assert_eq!(settlement["status"], "success", "{settlement}");
    assert_eq!(
        c.query_one(
            "SELECT state FROM app.ante_commitment WHERE id=$1",
            &[&commitment]
        )
        .await?
        .get::<_, String>(0),
        "settled"
    );
    assert!(c.query_one("SELECT (SELECT owned_quantity=1010 FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.match_seat s ON s.account_id=o.account_id JOIN app.asset_definition a ON a.id=i.asset_definition_id WHERE s.id=$1 AND a.kind='coin') AND (SELECT owned_quantity=990 FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.match_seat s ON s.account_id=o.account_id JOIN app.asset_definition a ON a.id=i.asset_definition_id WHERE s.id=$2 AND a.kind='coin')",&[&f.winner,&f.loser]).await?.get::<_,bool>(0));
    let ranked: Value = c
        .query_one("SELECT app.consume_ranked_match_result($1)", &[&result])
        .await?
        .get(0);
    assert_eq!(ranked["status"], "success", "{ranked}");
    assert_eq!(
        c.query_one("SELECT app.consume_ranked_match_result($1)", &[&result])
            .await?
            .get::<_, Value>(0),
        ranked
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.competitive_result WHERE match_result_id=$1 AND win",
            &[&result]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    assert_eq!(
        c.query_one("SELECT app.consume_quest_game_activity($1,100)", &[&f.game])
            .await?
            .get::<_, Value>(0)["status"],
        "success"
    );
    let replay:Value=c.query_one("SELECT app.publish_game_replay($1,2,$2,2,'fixture:bank-replay','fixture:visibility','fixture:v1','fixture:archive-proof')",&[&f.game,&f.initial_checkpoint]).await?.get(0);
    assert_eq!(replay["status"], "success", "{replay}");
    assert_eq!(
        c.query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result])
            .await?
            .get::<_, Value>(0),
        settlement
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn bank_timeout_and_outage_serialize_in_both_orders() -> TestResult {
    for outage_first in [false, true] {
        let c = connection().await?;
        let other = connection().await?;
        let f = fixture(&c, "1").await?;
        c.query_one("SELECT pg_sleep(0.005)", &[]).await?;
        let payload = candidate(&c, &f, true).await?;
        let game = f.game;
        let pid: i32 = other
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        c.batch_execute("BEGIN").await?;
        if outage_first {
            let paused:Value=c.query_one("SELECT app.pause_game_incident($1,'fixture-host',1,0,clock_timestamp(),'storage','fixture:outage',$2)",&[&game,&Uuid::new_v4()]).await?.get(0);
            assert_eq!(paused["status"], "success");
            let task = tokio::spawn(async move {
                other
                    .query_one(
                        "SELECT app.accept_host_candidate($1,'fixture-host',1,1,$2,$3)",
                        &[&game, &Uuid::new_v4(), &payload],
                    )
                    .await
                    .map(|r| r.get::<_, Value>(0))
            });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "fail");
            assert!(
                !c.query_one(
                    "SELECT EXISTS(SELECT 1 FROM app.game_result WHERE game_id=$1)",
                    &[&game]
                )
                .await?
                .get::<_, bool>(0)
            );
        } else {
            assert_eq!(
                accept(&c, &f, "1", Uuid::new_v4(), &payload).await?["status"],
                "success"
            );
            let task = tokio::spawn(async move {
                other.query_one("SELECT app.pause_game_incident($1,'fixture-host',1,0,clock_timestamp(),'storage','fixture:outage',$2)",&[&game,&Uuid::new_v4()]).await.map(|r|r.get::<_,Value>(0))
            });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "fail");
            assert!(c.query_one("SELECT EXISTS(SELECT 1 FROM app.game_result WHERE game_id=$1 AND bank_forfeit_receipt_id IS NOT NULL)",&[&game]).await?.get::<_,bool>(0));
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn late_native_fact_failure_rolls_back_bank_zero_event_checkpoint_result_and_receipt()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c, "1").await?;
    c.query_one("SELECT pg_sleep(0.005)", &[]).await?;
    let mut body = candidate(&c, &f, true).await?;
    let valid = body.clone();
    let request = Uuid::new_v4();
    body["facts"] = json!([{"eventIndex":"99","accountId":Uuid::new_v4(),"kind":"fixture","revision":"fixture","value":"1"}]);
    assert_eq!(
        accept(&c, &f, "1", request, &body)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    assert!(c.query_one("SELECT g.accepted_revision=1 AND g.state='active' AND NOT EXISTS(SELECT 1 FROM app.game_result WHERE game_id=g.id) AND NOT EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id AND accepted_revision>1) AND (SELECT remaining_ms=1 AND NOT paused FROM app.match_clock WHERE match_seat_id=$2) AND NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE request_ref=$3::uuid::text AND scope='host_candidate_v1') FROM app.game g WHERE id=$1",&[&f.game,&f.loser,&request]).await?.get::<_,bool>(0));
    assert_eq!(
        accept(&c, &f, "1", request, &valid).await?["status"],
        "success"
    );
    Ok(())
}
