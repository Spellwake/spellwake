//! Normalized lifecycle transactions, without real native codecs, controller clients or deadline activation.
use super::{TestResult, connection, room_start_integrity, wait_for_database_lock};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) async fn open(c: &Client, m: Uuid, g: Uuid) -> FixtureResult<Value> {
    open_with_context(c, m, g, g).await
}
pub(super) async fn open_with_context(
    c: &Client,
    m: Uuid,
    g: Uuid,
    context: Uuid,
) -> FixtureResult<Value> {
    room_start_integrity::confirm_adopted_ai(c, m).await?;
    let revision: String = c
        .query_one("SELECT revision::text FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    let proof = json!({"contextId":context,"snapshotRef":"fixture:context","contextRevision":"fixture:v1","stateRef":"fixture:state","factsRef":"fixture:facts","eventRef":"fixture:opening","qualificationRef":"fixture:normalized-only"});
    let response: Value = c
        .query_one(
            "SELECT app.accept_reserved_game($1,$2::text::numeric,$3,'fixture:lifecycle',$4)",
            &[&m, &revision, &g, &proof],
        )
        .await?
        .get(0);
    // Correlate actual SQL output with the original seven-field private port.
    spellwake_application::admission_ports::AcceptReservedGame {
        match_id: m,
        expected_revision: revision.parse()?,
        game_id: g,
        owner: "fixture:lifecycle".parse()?,
        opening: spellwake_application::admission_ports::OpeningArtifacts::from_bytes(
            &serde_json::to_vec(&proof)?,
        )?,
        original_accepted_basis: None,
    }
    .decode_response(&serde_json::to_vec(&response)?)?;
    assert_eq!(response["status"], "success");
    assert_eq!(response["data"]["resourceId"], json!(g));
    assert_eq!(response["data"]["contextId"], json!(context));
    assert_ne!(response["data"]["revision"], json!("0"));
    Ok(response)
}
pub(super) async fn complete(
    c: &Client,
    m: Uuid,
    g: Uuid,
    winners: Vec<Uuid>,
) -> FixtureResult<Uuid> {
    let context: Uuid = c
        .query_one("SELECT id FROM app.game_context WHERE game_id=$1", &[&g])
        .await?
        .get(0);
    let rows=c.query("SELECT s.id,c.clock_revision::text,c.remaining_ms::text,s.controller_kind FROM app.match_seat s JOIN app.match_clock c ON c.match_seat_id=s.id WHERE s.match_id=$1 ORDER BY s.id", &[&m]).await?;
    let clocks:Vec<Value>=rows.iter().map(|r|json!({"seatId":r.get::<_,Uuid>(0),"clockRevision":r.get::<_,String>(1),"remainingMs":r.get::<_,String>(2),"paused":true})).collect();
    let participation: Vec<Value> = rows
        .iter()
        .filter(|row| row.get::<_, String>(3) == "human")
        .map(|r| json!({"seatId":r.get::<_,Uuid>(0),"ownTurns":"3","reason":"fixture"}))
        .collect();
    let body = json!({"contextId":context,"stateRef":"fixture:terminal","factsRef":"fixture:facts","events":["fixture:terminal-event"],"facts":[],"clocks":clocks,"terminal":{"outcome":if winners.is_empty(){"draw"}else{"win"},"winnerSeatIds":winners,"winnersRef":"fixture:normalized-winners","reason":"fixture"},"participation":participation});
    let request = Uuid::new_v4();
    let accepted: Value = c
        .query_one(
            "SELECT app.accept_host_candidate($1,'fixture:lifecycle',1,0,$2,$3)",
            &[&g, &request, &body],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    super::admission_port_contracts::candidate_response(
        g,
        "fixture:lifecycle",
        "1",
        "0",
        request,
        &body,
        &accepted,
    );
    Ok(
        c.query_one("SELECT id FROM app.game_result WHERE game_id=$1", &[&g])
            .await?
            .get(0),
    )
}
pub(super) async fn consume(c: &Client, m: Uuid, result: Uuid) -> FixtureResult<Value> {
    let basis: String = c
        .query_one("SELECT revision::text FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    let response: Value = c
        .query_one(
            "SELECT app.consume_game_outcome($1,$2::text::numeric)",
            &[&result, &basis],
        )
        .await?
        .get(0);
    spellwake_application::admission_ports::ConsumeGameOutcome {
        result_id: result,
        expected_revision: basis.parse()?,
        match_id: m,
        original_accepted_basis: None,
    }
    .decode_response(&serde_json::to_vec(&response)?)?;
    Ok(response)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn drawn_bo3_game_adds_no_win_and_two_wins_finalize_once_after_exact_next_game_reservations()
-> TestResult {
    let c = connection().await?;
    let f = room_start_integrity::room(&c, true).await?;
    let teams: Vec<Value> = c
        .query("SELECT id FROM app.room_slot WHERE room_id=$1", &[&f.id])
        .await?
        .iter()
        .map(|r| json!({"slotId":r.get::<_,Uuid>(0),"teamRef":null}))
        .collect();
    let basis = room_start_integrity::basis(&c, f.id).await?;
    let configured: Value = c
        .query_one(
            "SELECT app.configure_room($1,$2,$3::text::numeric,$4,'bo3',$5,$6)",
            &[
                &f.source.actor,
                &f.id,
                &basis,
                &f.source.profile,
                &json!(teams),
                &Uuid::new_v4(),
            ],
        )
        .await?
        .get(0);
    assert_eq!(configured["status"], "success");
    let basis = room_start_integrity::basis(&c, f.id).await?;
    let countdown: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:deck',$4)",
            &[&f.source.actor, &f.id, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(countdown["status"], "success");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&f.id],
    )
    .await?;
    let started = room_start_integrity::start(&c, &f, Uuid::new_v4()).await?;
    assert_eq!(started["status"], "success");
    let m: Uuid = started["data"]["resourceId"].as_str().unwrap().parse()?;
    let mut g: Uuid = started["data"]["gameId"].as_str().unwrap().parse()?;
    let winner: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&m, &f.source.actor],
        )
        .await?
        .get(0);
    for round in 0..3 {
        let opened = if round == 0 {
            let waiting = connection().await?;
            let pid: i32 = waiting
                .query_one("SELECT pg_backend_pid()", &[])
                .await?
                .get(0);
            c.batch_execute("BEGIN").await?;
            c.query_one(
                "SELECT id FROM app.match_clock WHERE match_seat_id=$1 FOR NO KEY UPDATE",
                &[&winner],
            )
            .await?;
            let pending = tokio::spawn(async move { open(&waiting, m, g).await });
            wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            let accepted = pending.await??;
            assert!(c.query_one("SELECT bool_and(c.clock_revision=checkpoint.clock_revision) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id JOIN app.game_checkpoint checkpoint ON checkpoint.game_id=$2 WHERE s.match_id=$1", &[&m,&g]).await?.get::<_,bool>(0));
            accepted
        } else {
            open(&c, m, g).await?
        };
        assert_eq!(opened["status"], "success");
        assert_eq!(open(&c, m, g).await?, opened);
        let result = complete(&c, m, g, if round == 0 { vec![] } else { vec![winner] }).await?;
        let scored = consume(&c, m, result).await?;
        assert_eq!(scored["status"], "success");
        assert_eq!(consume(&c, m, result).await?, scored);
        // The opening receipt remains historical after outcome consumption and
        // the match frontier advance. open() samples the newer retry basis.
        assert_eq!(open(&c, m, g).await?, opened);
        if round == 0 {
            c.execute("UPDATE app.deck SET mode='open' WHERE id IN (SELECT revision.deck_id FROM app.deck_revision revision JOIN app.match_seat seat ON seat.source_revision_id=revision.id WHERE seat.match_id=$1)", &[&m]).await?;
        }
        if round < 2 {
            assert_eq!(scored["data"]["phase"], "between_games");
            let decisions=c.query("SELECT id,seat_id FROM app.between_game_decision WHERE match_id=$1 AND after_game_id=$2 ORDER BY seat_id", &[&m,&g]).await?;
            assert_eq!(decisions.len(), 2);
            for row in decisions {
                let decision: Uuid = row.get(0);
                let seat: Uuid = row.get(1);
                let revision: Uuid = c
                    .query_one(
                        "SELECT source_revision_id FROM app.match_seat WHERE id=$1",
                        &[&seat],
                    )
                    .await?
                    .get(0);
                let basis: String = c
                    .query_one("SELECT revision::text FROM app.match WHERE id=$1", &[&m])
                    .await?
                    .get(0);
                let response:Value=c.query_one("SELECT app.reply_between_games($1,$2::text::numeric,$3,'fixture:sideboard','fixture:validator',$4)", &[&decision,&basis,&revision,&Uuid::new_v4()]).await?.get(0);
                assert_eq!(response["status"], "success");
            }
            // Answered decisions do not rewrite their original result receipt.
            assert_eq!(consume(&c, m, result).await?, scored);
            g = c
                .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
                .await?
                .get(0);
        } else {
            assert_eq!(scored["data"]["phase"], "terminal");
        }
    }
    let final_row=c.query_one("SELECT winner_seat_ids,cardinality(final_game_ids),(SELECT count(*)::bigint FROM app.durable_handoff WHERE match_id=$1 AND kind='match_outcome') FROM app.match_result WHERE match_id=$1", &[&m]).await?;
    assert_eq!(final_row.get::<_, Vec<Uuid>>(0), vec![winner]);
    assert_eq!(final_row.get::<_, i32>(1), 3);
    assert_eq!(final_row.get::<_, i64>(2), 1);
    assert!(c.query_one("SELECT bool_and(remaining_ms=1800000) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=$1", &[&m]).await?.get::<_,bool>(0));
    Ok(())
}
