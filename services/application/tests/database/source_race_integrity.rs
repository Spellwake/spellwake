//! Actual parent/source concurrency and terminal history controls for database-tier integrity.
use super::{
    TestResult, account, connection, deletion, game_scope, lobbies, wait_for_database_lock,
};
use tokio_postgres::error::SqlState;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn guest_retirement_never_reopens_a_lobby_its_host_already_closed() -> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (guest, _) = account(&c).await?;
    c.execute("INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'human',true)", &[&f.room,&guest]).await?;
    let closed: serde_json::Value = c
        .query_one(
            "SELECT app.leave_room($1,$2,0,$3)",
            &[&f.host, &f.room, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(closed["status"], "success");
    deletion::request(&c, guest, Uuid::new_v4()).await?;
    let retired: serde_json::Value = c
        .query_one("SELECT app.retire_account($1)", &[&guest])
        .await?
        .get(0);
    assert_eq!(retired["data"]["state"], "retired");
    let row = c
        .query_one(
            "SELECT host_id,state,countdown_at IS NULL FROM app.room WHERE id=$1",
            &[&f.room],
        )
        .await?;
    assert_eq!(row.get::<_, Uuid>(0), f.host);
    assert_eq!(row.get::<_, String>(1), "closed");
    assert!(row.get::<_, bool>(2));
    assert!(
        !c.query_one(
            "SELECT ready FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.room, &f.host]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn activity_resolves_the_committed_game_parent_after_a_move_even_with_its_scalar_fk_deferred()
-> TestResult {
    let c = connection().await?;
    let local = game_scope::seed(&c).await?;
    let (other, _) = account(&c).await?;
    let compatible:Uuid=c.query_one("INSERT INTO app.match(format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) SELECT format_id,profile_id,legality_revision_id,mode,competitive,structure,state,0 FROM app.match WHERE id=$1 RETURNING id", &[&local.match_id]).await?.get(0);
    c.execute("INSERT INTO app.match_seat(match_id,account_id,seat_index,controller_kind) VALUES($1,$2,0,'human')", &[&compatible,&other]).await?;
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.game SET match_id=$2,game_index=1 WHERE id=$1",
        &[&local.game, &compatible],
    )
    .await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let task = tokio::spawn(async move {
        waiting
            .batch_execute("BEGIN; SET CONSTRAINTS app.game_activity_fact_game_id_fkey DEFERRED")
            .await?;
        let result = waiting.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'race',1,'fixture')", &[&local.game,&local.event,&local.account]).await;
        waiting.batch_execute("ROLLBACK").await?;
        result
    });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game_activity_fact WHERE game_id=$1",
            &[&local.game]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(c.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'race',1,'fixture')", &[&local.game,&local.event,&local.account]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'race',1,'fixture')", &[&local.game,&local.event,&other]).await?;

    Ok(())
}
