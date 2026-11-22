//! Three-seat SQL admission and normalized timeout outputs; no native expiry/codec truth is asserted.
use super::{
    TestResult, connection, decks, match_lifecycle_integrity as lifecycle,
    room_start_integrity as lobby,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) struct Fixture {
    game: Uuid,
    pub(super) matched: Uuid,
    context: Uuid,
    seats: Vec<Uuid>,
}
pub(super) async fn fixture(c: &Client, ai_third: bool) -> FixtureResult<Fixture> {
    let room = lobby::room_with_capacity(c, true, false, 3).await?;
    if ai_third {
        let slot:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,deck_revision_id,seat_index,controller_kind,ready) VALUES($1,$2,$3,2,'ai',false) RETURNING id",&[&room.id,&room.source.actor,&room.source.revision]).await?.get(0);
        let grant = lobby::bootstrap_ai(c, slot, room.source.actor, room.source.revision).await?;
        let configuration: Uuid = c
            .query_one(
                "SELECT configuration_id FROM app.room WHERE id=$1",
                &[&room.id],
            )
            .await?
            .get(0);
        let ready:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,1,$2,'fixture:deck-qualified','fixture:authenticated-browser',$3,$4)",&[&grant,&room.source.revision,&configuration,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(ready["status"], "success");
    } else {
        let (peer, _, deck, _, _) = decks::library(c).await?;
        let format: Uuid = c
            .query_one(
                "SELECT format_id FROM app.match WHERE id=$1",
                &[&room.source.match_id],
            )
            .await?
            .get(0);
        c.execute(
            "UPDATE app.deck SET format_id=$1 WHERE id=$2",
            &[&format, &deck],
        )
        .await?;
        let saved:Value=c.query_one("SELECT app.save_deck_contents($1,$2,NULL,$3,$4)",&[&peer,&deck,&json!([{"definitionId":room.source.definition,"printingId":room.source.printing,"section":"main","quantity":"2"}]),&Uuid::new_v4()]).await?.get(0);
        assert_eq!(saved["status"], "success");
        let revision: Uuid = saved["data"]["resourceId"].as_str().unwrap().parse()?;
        let joined: Value = c
            .query_one(
                "SELECT app.join_room($1,$2,NULL,$3)",
                &[&peer, &room.id, &Uuid::new_v4()],
            )
            .await?
            .get(0);
        assert_eq!(joined["status"], "success");
        let basis = lobby::basis(c, room.id).await?;
        let selected: Value = c
            .query_one(
                "SELECT app.select_room_deck($1,$2,$3::text::numeric,$4,$5)",
                &[&peer, &room.id, &basis, &revision, &Uuid::new_v4()],
            )
            .await?
            .get(0);
        assert_eq!(selected["status"], "success");
        assert_eq!(
            lobby::ready(c, &room, peer, revision, true).await?["status"],
            "success"
        );
    }
    assert_eq!(
        lobby::ready(c, &room, room.source.actor, room.source.revision, true).await?["status"],
        "success"
    );
    assert_eq!(
        lobby::ready(c, &room, room.other, room.other_revision, true).await?["status"],
        "success"
    );
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&room.id],
    )
    .await?;
    let started = lobby::start(c, &room, Uuid::new_v4()).await?;
    assert_eq!(started["status"], "success", "{started}");
    let matched: Uuid = started["data"]["resourceId"].as_str().unwrap().parse()?;
    let game: Uuid = started["data"]["gameId"].as_str().unwrap().parse()?;
    assert_eq!(
        lifecycle::open(c, matched, game).await?["status"],
        "success"
    );
    let context: Uuid = c
        .query_one("SELECT id FROM app.game_context WHERE game_id=$1", &[&game])
        .await?
        .get(0);
    let seats = c
        .query(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index",
            &[&matched],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    Ok(Fixture {
        game,
        matched,
        context,
        seats,
    })
}
async fn effect(c: &Client, seat: Uuid, basis: &str) -> FixtureResult<Value> {
    let stamp:String=c.query_one("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",&[]).await?.get(0);
    Ok(
        json!({"seatId":seat,"sourceRevision":basis,"reason":"native_response_timeout","decisionRef":"fixture:native-pending","expiryRef":"fixture:qualified-expiry","expiredAt":stamp}),
    )
}
async fn candidate(
    c: &Client,
    f: &Fixture,
    effects: Option<Value>,
    terminal: bool,
) -> FixtureResult<Value> {
    let clocks:Vec<Value>=c.query("SELECT c.match_seat_id,c.clock_revision::text,c.remaining_ms::text FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1 ORDER BY s.id",&[&f.matched]).await?.iter().map(|r|json!({"seatId":r.get::<_,Uuid>(0),"clockRevision":r.get::<_,String>(1),"remainingMs":r.get::<_,String>(2),"paused":true})).collect();
    let mut result = json!({"contextId":f.context,"stateRef":"fixture:state","factsRef":"fixture:facts","events":["fixture:native-elimination-event"],"facts":[],"clocks":clocks,"participation":[],"terminal":null});
    if let Some(effects) = effects {
        result["nativeEliminations"] = effects;
    }
    if terminal {
        result["terminal"] = json!({"outcome":"win","winnerSeatIds":[f.seats[1]],"winnersRef":"fixture:actual-native-winner","reason":"fixture:native"});
        result["participation"]=json!(c.query("SELECT id FROM app.match_seat WHERE match_id=$1 AND controller_kind='human' ORDER BY id",&[&f.matched]).await?.iter().map(|r|json!({"seatId":r.get::<_,Uuid>(0),"ownTurns":"3","reason":"fixture:final-native-counts"})).collect::<Vec<_>>());
    }
    Ok(result)
}
async fn accept(
    c: &Client,
    f: &Fixture,
    basis: &str,
    request: Uuid,
    body: &Value,
) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c
        .query_one(
            "SELECT app.accept_host_candidate($1,'fixture:lifecycle',1,$2::text::numeric,$3,$4)",
            &[&f.game, &basis, &request, &body],
        )
        .await?
        .get(0);
    super::admission_port_contracts::candidate_response(
        f.game,
        "fixture:lifecycle",
        "1",
        basis,
        request,
        body,
        &response,
    );
    Ok(response)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn multiplayer_timeout_is_source_once_keeps_the_game_live_and_retains_all_final_humans()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c, false).await?;
    let body = candidate(
        &c,
        &f,
        Some(json!([effect(&c, f.seats[0], "0").await?])),
        false,
    )
    .await?;
    let request = Uuid::new_v4();
    let first = accept(&c, &f, "0", request, &body).await?;
    assert_eq!(first["status"], "success");
    assert_eq!(accept(&c, &f, "0", request, &body).await?, first);
    assert!(c.query_one("SELECT g.state='active' AND m.state='active' AND NOT EXISTS(SELECT 1 FROM app.game_result WHERE game_id=g.id) AND NOT EXISTS(SELECT 1 FROM app.match_result WHERE match_id=m.id) FROM app.game g JOIN app.match m ON m.id=g.match_id WHERE g.id=$1",&[&f.game]).await?.get::<_,bool>(0));
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE scope='native_seat_elimination_v1' AND request_ref=$1::uuid::text",&[&f.game]).await?.get::<_,i64>(0),1);
    let duplicate = candidate(
        &c,
        &f,
        Some(json!([effect(&c, f.seats[0], "1").await?])),
        false,
    )
    .await?;
    assert_eq!(
        accept(&c, &f, "1", Uuid::new_v4(), &duplicate)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let terminal = candidate(&c, &f, None, true).await?;
    assert_eq!(
        accept(&c, &f, "1", Uuid::new_v4(), &terminal).await?["status"],
        "success"
    );
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
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game_participation WHERE game_id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, i64>(0),
        3
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn native_elimination_rejects_foreign_duplicate_stale_future_and_malformed_output_atomically()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c, false).await?;
    let valid = effect(&c, f.seats[0], "0").await?;
    let mut foreign = valid.clone();
    foreign["seatId"] = json!(Uuid::new_v4());
    let mut stale = valid.clone();
    stale["sourceRevision"] = json!("9");
    let mut future = valid.clone();
    future["expiredAt"]=json!(c.query_one("SELECT to_char((clock_timestamp()+interval '1 hour') AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",&[]).await?.get::<_,String>(0));
    for effects in [
        Value::Null,
        json!({}),
        json!([foreign]),
        json!([stale]),
        json!([valid.clone(), valid.clone()]),
        json!([future]),
    ] {
        let body = candidate(&c, &f, Some(effects), false).await?;
        assert!(accept(&c, &f, "0", Uuid::new_v4(), &body).await.is_err());
        assert!(c.query_one("SELECT accepted_revision=0 AND NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE scope='native_seat_elimination_v1' AND request_ref=$1::uuid::text) FROM app.game WHERE id=$1",&[&f.game]).await?.get::<_,bool>(0));
    }
    assert_eq!(
        accept(
            &c,
            &f,
            "0",
            Uuid::new_v4(),
            &candidate(&c, &f, Some(json!([valid])), false).await?
        )
        .await?["status"],
        "success"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn accepted_empty_frame_cannot_acquire_a_late_elimination_child() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c, false).await?;
    let request = Uuid::new_v4();
    assert_eq!(
        accept(
            &c,
            &f,
            "0",
            request,
            &candidate(&c, &f, Some(json!([])), false).await?
        )
        .await?["status"],
        "success"
    );
    let source:Uuid=c.query_one("SELECT id FROM app.command_receipt WHERE principal_key='host:'||$1::uuid::text AND scope='host_candidate_v1' AND request_ref=$2::uuid::text",&[&f.game,&request]).await?.get(0);
    let opening:Uuid=c.query_one("SELECT source_receipt_id FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=0",&[&f.game]).await?.get(0);
    for parent in [source, opening, Uuid::new_v4()] {
        let child = Uuid::new_v4();
        c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
            .await?;
        c.execute("INSERT INTO app.command_receipt(id,principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source,response_payload) VALUES($1,'native-seat:'||$2::uuid::text||':'||$3::uuid::text,'native_seat_elimination_v1',$3::uuid::text,$2::uuid::text,app.auth_json_hash('{}'::jsonb),'accepted','',jsonb_build_object('version','native-seat-elimination-v1','hostReceiptId',$4::uuid),jsonb_build_object('status','success','data',jsonb_build_object('resourceId',$1::uuid)))",&[&child,&f.game,&f.seats[0],&parent]).await?;
        assert_eq!(
            c.batch_execute("COMMIT").await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        assert!(
            !c.query_one(
                "SELECT EXISTS(SELECT 1 FROM app.command_receipt WHERE id=$1)",
                &[&child]
            )
            .await?
            .get::<_, bool>(0)
        );
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ai_seat_timeout_has_no_human_participation_or_financial_effect() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c, true).await?;
    let body = candidate(
        &c,
        &f,
        Some(json!([effect(&c, f.seats[2], "0").await?])),
        false,
    )
    .await?;
    assert_eq!(
        accept(&c, &f, "0", Uuid::new_v4(), &body).await?["status"],
        "success"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game_participation WHERE game_id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert!(
        !c.query_one(
            "SELECT EXISTS(SELECT 1 FROM app.ledger_operation WHERE source_ref=$1::uuid::text)",
            &[&f.game]
        )
        .await?
        .get::<_, bool>(0)
    );
    let body = candidate(&c, &f, None, true).await?;
    assert_eq!(
        accept(&c, &f, "1", Uuid::new_v4(), &body).await?["status"],
        "success"
    );
    let result: Uuid = c
        .query_one(
            "SELECT id FROM app.game_result WHERE game_id=$1",
            &[&f.game],
        )
        .await?
        .get(0);
    lifecycle::consume(&c, f.matched, result).await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game_participation WHERE game_id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn multiple_eliminations_and_terminal_output_are_one_atomic_native_frame() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c, false).await?;
    let first = effect(&c, f.seats[0], "0").await?;
    let mut second = effect(&c, f.seats[2], "0").await?;
    let valid_second = second.clone();
    second["expiredAt"]=json!(c.query_one("SELECT to_char((clock_timestamp()+interval '1 hour') AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",&[]).await?.get::<_,String>(0));
    let failed = candidate(&c, &f, Some(json!([first.clone(), second])), true).await?;
    let request = Uuid::new_v4();
    assert_eq!(
        accept(&c, &f, "0", request, &failed)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert!(c.query_one("SELECT accepted_revision=0 AND state='active' AND NOT EXISTS(SELECT 1 FROM app.game_result WHERE game_id=$1) AND NOT EXISTS(SELECT 1 FROM app.game_participation WHERE game_id=$1) AND NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE scope='native_seat_elimination_v1' AND request_ref=$1::uuid::text) AND NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE scope='host_candidate_v1' AND request_ref=$2::uuid::text) FROM app.game WHERE id=$1",&[&f.game,&request]).await?.get::<_,bool>(0));
    let body = candidate(&c, &f, Some(json!([first, valid_second])), true).await?;
    assert_eq!(
        accept(&c, &f, "0", request, &body).await?["status"],
        "success"
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE scope='native_seat_elimination_v1' AND request_ref=$1::uuid::text",&[&f.game]).await?.get::<_,i64>(0),2);
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
async fn elimination_and_outage_share_the_current_host_fence_in_both_orders() -> TestResult {
    for outage_first in [false, true] {
        let c = connection().await?;
        let other = connection().await?;
        let f = fixture(&c, false).await?;
        let game = f.game;
        let body = candidate(
            &c,
            &f,
            Some(json!([effect(&c, f.seats[0], "0").await?])),
            false,
        )
        .await?;
        let pid: i32 = other
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        c.batch_execute("BEGIN").await?;
        if outage_first {
            let pause:Value=c.query_one("SELECT app.pause_game_incident($1,'fixture:lifecycle',1,0,clock_timestamp(),'storage','fixture:outage',$2)",&[&game,&Uuid::new_v4()]).await?.get(0);
            assert_eq!(pause["status"], "success");
            let task = tokio::spawn(async move {
                other
                    .query_one(
                        "SELECT app.accept_host_candidate($1,'fixture:lifecycle',1,0,$2,$3)",
                        &[&game, &Uuid::new_v4(), &body],
                    )
                    .await
                    .map(|r| r.get::<_, Value>(0))
            });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "fail");
            assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE scope='native_seat_elimination_v1' AND request_ref=$1::uuid::text",&[&game]).await?.get::<_,i64>(0),0);
        } else {
            let request = Uuid::new_v4();
            let accepted = accept(&c, &f, "0", request, &body).await?;
            assert_eq!(accepted["status"], "success");
            let task = tokio::spawn(async move {
                other.query_one("SELECT app.pause_game_incident($1,'fixture:lifecycle',1,0,clock_timestamp(),'storage','fixture:outage',$2)",&[&game,&Uuid::new_v4()]).await.map(|r|r.get::<_,Value>(0))
            });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "success");
            assert_eq!(accept(&c, &f, "0", request, &body).await?, accepted);
            let source:Uuid=c.query_one("SELECT id FROM app.command_receipt WHERE principal_key='host:'||$1::uuid::text AND scope='host_candidate_v1' AND request_ref=$2::uuid::text",&[&game,&request]).await?.get(0);
            c.query_one("SELECT app.check_native_creator_frame($1)", &[&source])
                .await?;
        }
    }
    Ok(())
}
