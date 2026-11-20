//! Whole started-room SQL topology; native snapshot contents remain independently qualified.
use super::{
    TestResult, ante_transaction_integrity as ante, connection,
    match_lifecycle_integrity as lifecycle,
};
use serde_json::Value;
use tokio_postgres::error::SqlState;
use uuid::Uuid;
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ready_and_active_matches_cannot_gain_unpointed_or_unopened_native_frontiers() -> TestResult
{
    let c = connection().await?;
    let (_, m) = ante::reserve(&c, true).await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES($1,1,'reserved',0)",&[&m]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.game SET state='active' WHERE id=$1", &[&g])
        .await?;
    c.execute("UPDATE app.match SET state='active',current_game_id=$2,reserved_game_id=NULL,revision=revision+1 WHERE id=$1",&[&m,&g]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.game SET accepted_revision=1 WHERE id=$1", &[&g])
        .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(lifecycle::open(&c, m, g).await?["status"], "success");
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision) SELECT game_id,id,1,'fixture:forged','fixture:forged',0 FROM app.game_context WHERE game_id=$1",&[&g]).await?;
    c.execute("UPDATE app.game SET accepted_revision=1 WHERE id=$1", &[&g])
        .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );

    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.game SET state='completed' WHERE id=$1", &[&g])
        .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES($1,1,'reserved',0)",&[&m]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn ordinary_terminal_match_is_frozen_before_any_replay_is_published() -> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve(&c, true).await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    lifecycle::open(&c, m, g).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index LIMIT 1",
            &[&m],
        )
        .await?
        .get(0);
    let result = lifecycle::complete(&c, m, g, vec![seat]).await?;
    // A real native result awaiting lifecycle is a valid active-match frontier.
    c.query_one("SELECT app.check_started_match_frontier($1)", &[&m])
        .await?;
    assert_eq!(
        lifecycle::consume(&c, m, result).await?["data"]["phase"],
        "terminal"
    );
    for state in ["ready", "active", "between_games"] {
        c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
            .await?;
        c.execute(
            "UPDATE app.match SET state=$2,revision=revision+1 WHERE id=$1",
            &[&m, &state],
        )
        .await?;
        assert_eq!(
            c.batch_execute("COMMIT").await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.between_game_decision(match_id,seat_id,decision_kind,basis_revision,clock_revision,state) VALUES($1,$2,'ready',0,0,'pending')",&[&m,&seat]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert!(
        !c.query_one(
            "SELECT EXISTS(SELECT 1 FROM app.replay_manifest WHERE game_id=$1)",
            &[&g]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn between_game_phase_requires_current_replies_and_cannot_fake_a_successor() -> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve_match(&c, true, "bo3").await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    lifecycle::open(&c, m, g).await?;
    let winner: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index LIMIT 1",
            &[&m],
        )
        .await?
        .get(0);
    let result = lifecycle::complete(&c, m, g, vec![winner]).await?;
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at,winner_seat_ids,final_game_ids,source_game_result_id) VALUES($1,$2,'win','fixture:premature',clock_timestamp(),$3,$4,$5)",&[&m,&Uuid::new_v4().to_string(),&vec![winner],&vec![g],&result]).await?;
    c.execute("UPDATE app.match SET state='completed',current_game_id=NULL,revision=revision+1 WHERE id=$1",&[&m]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );

    let consumed = lifecycle::consume(&c, m, result).await?;
    assert_eq!(consumed["data"]["phase"], "between_games");
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.between_game_decision d SET state='answered',reply_ref='fixture:raw-reply',selected_deck_revision_id=s.source_revision_id FROM app.match_seat s WHERE d.match_id=$1 AND s.id=d.seat_id",&[&m]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.between_game_decision d SET state='answered',reply_ref='fixture:raw-reply',selected_deck_revision_id=s.source_revision_id FROM app.match_seat s WHERE d.match_id=$1 AND s.id=d.seat_id",&[&m]).await?;
    let successor:Uuid=c.query_one("INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES($1,1,'reserved',0) RETURNING id",&[&m]).await?.get(0);
    c.execute(
        "UPDATE app.match SET state='ready',reserved_game_id=$2,revision=revision+1 WHERE id=$1",
        &[&m, &successor],
    )
    .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let peers=c.query("SELECT d.id,s.source_revision_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.match_id=$1 ORDER BY d.id",&[&m]).await?;
    let first: Uuid = peers[0].get(0);
    let first_revision: Uuid = peers[0].get(1);
    let second: Uuid = peers[1].get(0);
    let second_revision: Uuid = peers[1].get(1);
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("UPDATE app.between_game_decision SET state='answered',reply_ref='fixture:raw',selected_deck_revision_id=$2 WHERE id=$1",&[&first,&first_revision]).await?;
    assert_eq!(c.query_one("SELECT app.reply_between_games($1,$2::text::numeric,$3,'fixture:last','fixture:qualified',$4)",&[&second,&consumed["data"]["revision"].as_str().unwrap(),&second_revision,&Uuid::new_v4()]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.batch_execute("ROLLBACK").await?;
    // One actual reply leaves the other pending at its original, lower basis.
    let d=c.query_one("SELECT d.id,s.source_revision_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.match_id=$1 ORDER BY d.id LIMIT 1",&[&m]).await?;
    let id: Uuid = d.get(0);
    let revision: Uuid = d.get(1);
    let basis = consumed["data"]["revision"].as_str().unwrap();
    let reply:Value=c.query_one("SELECT app.reply_between_games($1,$2::text::numeric,$3,'fixture:selected','fixture:qualified',$4)",&[&id,&basis,&revision,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(reply["status"], "success");
    c.query_one("SELECT app.check_started_match_frontier($1)", &[&m])
        .await?;
    Ok(())
}
