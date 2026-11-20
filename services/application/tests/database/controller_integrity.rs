//! Controller lineage only; no hosted runner, secret delivery or game-view capability qualification.
use super::{TestResult, account, connection, lobbies, wait_for_database_lock};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct Binding {
    room: Uuid,
    slot: Uuid,
    seat: Uuid,
    binding: Uuid,
    match_id: Uuid,
}
async fn binding(c: &Client) -> FixtureResult<Binding> {
    let f = lobbies::lobby(c).await?;
    let slot:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'ai',false) RETURNING id", &[&f.room,&f.host]).await?.get(0);
    let legality:Uuid=c.query_one("INSERT INTO app.legality_revision(source_ref,effective_at,imported_at) VALUES($1,clock_timestamp(),clock_timestamp()) RETURNING id", &[&Uuid::new_v4().to_string()]).await?.get(0);
    let match_id:Uuid=c.query_one("INSERT INTO app.match(room_id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) VALUES($1,$2,$3,$4,'collection',false,'bo1','active',0) RETURNING id", &[&f.room,&f.format,&f.profile,&legality]).await?.get(0);
    let seat:Uuid=c.query_one("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,1,'ai') RETURNING id", &[&match_id,&f.host]).await?.get(0);
    let binding:Uuid=c.query_one("INSERT INTO app.ai_runner_binding(room_slot_id,binding_epoch,desired_state,observed_state) VALUES($1,1,'running','pending') RETURNING id", &[&slot]).await?.get(0);
    Ok(Binding {
        room: f.room,
        slot,
        seat,
        binding,
        match_id,
    })
}
async fn grant(
    c: &Client,
    b: Uuid,
    epoch: &str,
    seat: Option<Uuid>,
) -> Result<Uuid, tokio_postgres::Error> {
    c.query_one("INSERT INTO app.controller_grant(binding_id,binding_epoch,match_seat_id,grant_secret_ref,state) VALUES($1,$2::text::numeric,$3,$4,'active') RETURNING id", &[&b,&epoch,&seat,&Uuid::new_v4().to_string()]).await.map(|r|r.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn controller_grants_follow_epoch_and_nullable_admission_seat_without_rewriting_history()
-> TestResult {
    let c = connection().await?;
    let f = binding(&c).await?;
    let old = grant(&c, f.binding, "1", None).await?;
    assert_eq!(
        grant(&c, f.binding, "0", None).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        grant(&c, f.binding, "1", Some(f.seat))
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::UNIQUE_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.ai_runner_binding SET match_seat_id=$1 WHERE id=$2",
            &[&f.seat, &f.binding]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.ai_runner_binding SET match_seat_id=$1,binding_epoch=2 WHERE id=$2",
            &[&f.seat, &f.binding]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.execute(
        "UPDATE app.controller_grant SET state='revoked' WHERE id=$1",
        &[&old],
    )
    .await?;
    // With duplicate authority removed, the nullable seat/epoch lineage check still rejects.
    assert_eq!(
        grant(&c, f.binding, "1", Some(f.seat))
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.execute(
        "UPDATE app.ai_runner_binding SET match_seat_id=$1,binding_epoch=2 WHERE id=$2",
        &[&f.seat, &f.binding],
    )
    .await?;
    grant(&c, f.binding, "2", Some(f.seat)).await?;
    assert_eq!(
        c.execute(
            "UPDATE app.controller_grant SET state='active' WHERE id=$1",
            &[&old]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.ai_runner_binding SET binding_epoch=1 WHERE id=$1",
            &[&f.binding]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute("DELETE FROM app.controller_grant WHERE id=$1", &[&old])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT binding_epoch::text,match_seat_id FROM app.controller_grant WHERE id=$1",
            &[&old]
        )
        .await?
        .get::<_, String>(0),
        "1"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn controller_binding_preserves_ai_room_seat_index_and_sponsor_through_parent_changes()
-> TestResult {
    let c = connection().await?;
    let f = binding(&c).await?;
    let human_slot: Uuid = c
        .query_one(
            "SELECT id FROM app.room_slot WHERE room_id=$1 AND controller_kind='human'",
            &[&f.room],
        )
        .await?
        .get(0);
    assert_eq!(c.execute("INSERT INTO app.ai_runner_binding(room_slot_id,binding_epoch,desired_state,observed_state) VALUES($1,1,'running','pending')", &[&human_slot]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.execute(
        "UPDATE app.ai_runner_binding SET match_seat_id=$1,binding_epoch=2 WHERE id=$2",
        &[&f.seat, &f.binding],
    )
    .await?;
    let (other, _) = account(&c).await?;
    for sql in [
        "UPDATE app.room_slot SET sponsor_id=$1 WHERE id=$2",
        "UPDATE app.match_seat SET sponsor_id=$1 WHERE id=$2",
    ] {
        let id = if sql.contains("room_slot") {
            f.slot
        } else {
            f.seat
        };
        assert_eq!(
            c.execute(sql, &[&other, &id]).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    assert_eq!(
        c.execute(
            "UPDATE app.room_slot SET seat_index=7 WHERE id=$1",
            &[&f.slot]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.match_seat SET seat_index=7 WHERE id=$1",
            &[&f.seat]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.match SET room_id=NULL WHERE id=$1",
            &[&f.match_id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.room_slot SET controller_kind='human',account_id=$1 WHERE id=$2",
            &[&other, &f.slot]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stale_grant_waits_for_binding_rotation_and_is_rejected_after_the_epoch_commits()
-> TestResult {
    let c = connection().await?;
    let f = binding(&c).await?;
    let old = grant(&c, f.binding, "1", None).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.controller_grant SET state='revoked' WHERE id=$1",
        &[&old],
    )
    .await?;
    c.execute(
        "UPDATE app.ai_runner_binding SET binding_epoch=2 WHERE id=$1",
        &[&f.binding],
    )
    .await?;
    let task = tokio::spawn(async move { grant(&waiting, f.binding, "1", None).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deferred_controller_rotation_commits_only_after_revoke_rotate_and_regrant_agree()
-> TestResult {
    let c = connection().await?;
    let f = binding(&c).await?;
    let original = grant(&c, f.binding, "1", None).await?;
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute(
        "UPDATE app.ai_runner_binding SET binding_epoch=2 WHERE id=$1",
        &[&f.binding],
    )
    .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT binding_epoch::text FROM app.ai_runner_binding WHERE id=$1",
            &[&f.binding]
        )
        .await?
        .get::<_, String>(0),
        "1"
    );
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute(
        "UPDATE app.ai_runner_binding SET binding_epoch=2 WHERE id=$1",
        &[&f.binding],
    )
    .await?;
    c.execute(
        "UPDATE app.controller_grant SET state='revoked' WHERE id=$1",
        &[&original],
    )
    .await?;
    let renewed = grant(&c, f.binding, "2", None).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        c.query_one(
            "SELECT state FROM app.controller_grant WHERE id=$1",
            &[&renewed]
        )
        .await?
        .get::<_, String>(0),
        "active"
    );
    Ok(())
}
