//! Native frame SQL provenance over normalized artifacts; no codec/rules truth is asserted.
use super::{
    TestResult, ante_transaction_integrity as ante, connection,
    match_lifecycle_integrity as lifecycle,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct Frame {
    game: Uuid,
    matched: Uuid,
    actor: Uuid,
    seat: Uuid,
    context: Uuid,
    event: Uuid,
    source: Uuid,
    opening: Uuid,
}
async fn frame(c: &Client) -> FixtureResult<Frame> {
    let (room, matched) = ante::reserve(c, true).await?;
    let game: Uuid = c
        .query_one(
            "SELECT reserved_game_id FROM app.match WHERE id=$1",
            &[&matched],
        )
        .await?
        .get(0);
    assert_eq!(
        lifecycle::open(c, matched, game).await?["status"],
        "success"
    );
    let context: Uuid = c
        .query_one("SELECT id FROM app.game_context WHERE game_id=$1", &[&game])
        .await?
        .get(0);
    let opening:Uuid=c.query_one("SELECT source_receipt_id FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=0",&[&game]).await?.get(0);
    let clocks:Vec<Value>=c.query("SELECT match_seat_id,clock_revision::text,remaining_ms::text FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1 ORDER BY s.id",&[&matched]).await?.iter().map(|r|json!({"seatId":r.get::<_,Uuid>(0),"clockRevision":r.get::<_,String>(1),"remainingMs":r.get::<_,String>(2),"paused":true})).collect();
    let body = json!({"contextId":context,"stateRef":"fixture:active-state","factsRef":"fixture:active-facts","events":["fixture:active-event"],"facts":[],"clocks":clocks,"participation":[],"terminal":null});
    let accepted: Value = c
        .query_one(
            "SELECT app.accept_host_candidate($1,'fixture:lifecycle',1,0,$2,$3)",
            &[&game, &Uuid::new_v4(), &body],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    let row = c
        .query_one(
            "SELECT id,source_receipt_id FROM app.game_event WHERE game_id=$1 AND sequence=1",
            &[&game],
        )
        .await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&matched, &room.source.actor],
        )
        .await?
        .get(0);
    Ok(Frame {
        game,
        matched,
        actor: room.source.actor,
        seat,
        context,
        event: row.get(0),
        source: row.get(1),
        opening,
    })
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn future_checkpoint_and_event_cannot_poison_an_unreferenced_native_key() -> TestResult {
    let c = connection().await?;
    let f = frame(&c).await?;
    for creator in [None, Some(f.opening), Some(f.source)] {
        c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
            .await?;
        c.execute("INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision,source_receipt_id) VALUES($1,$2,2,'fixture:poison','fixture:poison',2,$3)",&[&f.game,&f.context,&creator]).await?;
        assert_eq!(
            c.batch_execute("COMMIT").await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    for creator in [None, Some(f.source)] {
        c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
            .await?;
        c.execute("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref,source_receipt_id) VALUES($1,$2,2,1,'fixture:poison',$3)",&[&f.game,&f.context,&creator]).await?;
        assert_eq!(
            c.batch_execute("COMMIT").await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game_checkpoint WHERE game_id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    c.query_one("SELECT app.check_native_game_storage($1)", &[&f.game])
        .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn late_reward_fact_and_terminal_reinterpretation_require_the_exact_original_frame()
-> TestResult {
    let c = connection().await?;
    let f = frame(&c).await?;
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'fixture',1,'fixture:v1')",&[&f.game,&f.event,&f.actor]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute(
        "UPDATE app.game SET state='completed' WHERE id=$1",
        &[&f.game],
    )
    .await?;
    c.execute("INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason,winner_seat_ids) VALUES($1,$2,'win','fixture:forged','fixture:forged',$3)",&[&f.game,&f.event,&vec![f.seat]]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert!(c.query_one("SELECT state='active' AND accepted_revision=1 AND NOT EXISTS(SELECT 1 FROM app.game_activity_fact WHERE game_id=$1) AND NOT EXISTS(SELECT 1 FROM app.game_result WHERE game_id=$1) FROM app.game WHERE id=$1",&[&f.game]).await?.get::<_,bool>(0));
    // A genuine later terminal frame and lifecycle consumption still succeed.
    let row=c.query_one("SELECT owner_instance_ref,fence_epoch::text FROM app.operational_state WHERE game_id=$1",&[&f.game]).await?;
    assert_eq!(row.get::<_, String>(0), "fixture:lifecycle");
    let clocks:Vec<Value>=c.query("SELECT c.match_seat_id,c.clock_revision::text,c.remaining_ms::text FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1 ORDER BY s.id",&[&f.matched]).await?.iter().map(|r|json!({"seatId":r.get::<_,Uuid>(0),"clockRevision":r.get::<_,String>(1),"remainingMs":r.get::<_,String>(2),"paused":true})).collect();
    let participants:Vec<Value>=c.query("SELECT id FROM app.match_seat WHERE match_id=$1 AND controller_kind='human' ORDER BY id",&[&f.matched]).await?.iter().map(|r|json!({"seatId":r.get::<_,Uuid>(0),"ownTurns":"3","reason":"fixture:native"})).collect();
    let body = json!({"contextId":f.context,"stateRef":"fixture:terminal-state","factsRef":"fixture:terminal-facts","events":["fixture:terminal-event"],"facts":[],"clocks":clocks,"participation":participants,"terminal":{"outcome":"win","winnerSeatIds":[f.seat],"winnersRef":"fixture:terminal-winners","reason":"fixture:native"}});
    let accepted: Value = c
        .query_one(
            "SELECT app.accept_host_candidate($1,'fixture:lifecycle',1,1,$2,$3)",
            &[&f.game, &Uuid::new_v4(), &body],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    let result: Uuid = c
        .query_one(
            "SELECT id FROM app.game_result WHERE game_id=$1",
            &[&f.game],
        )
        .await?
        .get(0);
    assert_eq!(
        lifecycle::consume(&c, f.matched, result).await?["data"]["phase"],
        "terminal"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn accepted_canonical_creator_cannot_commit_without_its_native_frame() -> TestResult {
    let c = connection().await?;
    let f = frame(&c).await?;
    let request = Uuid::new_v4();
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source,response_payload) VALUES('host:'||$1::uuid::text,'host_candidate_v1',$1::uuid::text,$2::uuid::text,app.auth_json_hash('{}'::jsonb),'accepted','',jsonb_build_object('version','host-clock-transition-v1','gameId',$1::uuid,'matchId',$3::uuid,'acceptedRevision','2'),jsonb_build_object('status','success','data',jsonb_build_object('resourceId',$4::uuid::text,'revision','2')))",&[&f.game,&request,&f.matched,&Uuid::new_v4()]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert!(!c.query_one("SELECT EXISTS(SELECT 1 FROM app.command_receipt WHERE scope='host_candidate_v1' AND request_ref=$1::uuid::text)",&[&request]).await?.get::<_,bool>(0));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn creator_metadata_cannot_substitute_a_different_checkpoint_clock_basis() -> TestResult {
    let c = connection().await?;
    let f = frame(&c).await?;
    let base: Value = c
        .query_one(
            "SELECT normalized_source FROM app.command_receipt WHERE id=$1",
            &[&f.source],
        )
        .await?
        .get(0);
    for mismatch_timeout in [false, true] {
        let mut normalized = base.clone();
        let checkpoint = Uuid::new_v4();
        let event = Uuid::new_v4();
        let source = Uuid::new_v4();
        let request = Uuid::new_v4();
        let stamp:String=c.query_one("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",&[]).await?.get(0);
        normalized["basisRevision"] = json!("1");
        normalized["acceptedRevision"] = json!("2");
        normalized["acceptedAt"] = json!(stamp);
        normalized["before"] = normalized["after"].clone();
        for clock in normalized["after"].as_array_mut().unwrap() {
            clock["clockRevision"] = json!("3");
            clock["lastAccountedAt"] = json!(stamp);
        }
        for clock in normalized["candidate"]["clocks"].as_array_mut().unwrap() {
            clock["clockRevision"] = json!("2");
        }
        normalized["eventIds"] = json!([event]);
        if mismatch_timeout {
            normalized["candidate"]["bankTimeout"] = json!({"kind":"overall_bank","seatId":f.seat,"clockRevision":"2","proofRef":"fixture:inconsistent-original"});
        }
        let checkpoint_clock = if mismatch_timeout { 3_i64 } else { 999_i64 };

        c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
            .await?;
        c.execute("INSERT INTO app.command_receipt(id,principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source,response_payload) VALUES($1,'host:'||$2::uuid::text,'host_candidate_v1',$2::uuid::text,$3::uuid::text,app.auth_json_hash(jsonb_build_object('owner',$4::jsonb->>'owner','epoch',$4::jsonb->>'epoch','basis',$4::jsonb->>'basisRevision','candidate',$4::jsonb->'candidate')),'accepted','',$4,jsonb_build_object('status','success','data',jsonb_build_object('resourceId',$5::uuid::text,'revision','2')))",&[&source,&f.game,&request,&normalized,&checkpoint]).await?;
        c.execute("INSERT INTO app.game_checkpoint(id,game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision,source_receipt_id) VALUES($1,$2,$3,2,'fixture:active-state','fixture:active-facts',$5::bigint,$4)",&[&checkpoint,&f.game,&f.context,&source,&checkpoint_clock]).await?;
        c.execute("INSERT INTO app.game_event(id,game_id,context_id,sequence,accepted_revision,private_payload_ref,source_receipt_id) VALUES($1,$2,$3,2,2,'fixture:active-event',$4)",&[&event,&f.game,&f.context,&source]).await?;
        c.execute(
            "UPDATE app.game SET accepted_revision=2 WHERE id=$1",
            &[&f.game],
        )
        .await?;
        if mismatch_timeout {
            let error = c
                .query_one("SELECT app.check_native_creator_frame($1)", &[&source])
                .await
                .unwrap_err();
            assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
            assert!(
                error
                    .as_db_error()
                    .unwrap()
                    .message()
                    .contains("derived bank timeout")
            );
            c.batch_execute("ROLLBACK").await?;
        } else {
            assert_eq!(
                c.batch_execute("COMMIT").await.unwrap_err().code(),
                Some(&SqlState::CHECK_VIOLATION)
            );
        }
        assert!(
            !c.query_one(
                "SELECT EXISTS(SELECT 1 FROM app.command_receipt WHERE id=$1)",
                &[&source]
            )
            .await?
            .get::<_, bool>(0)
        );
    }
    Ok(())
}
