//! Qualified-capacity lobby/queue sources, not native match initialization.
use super::{TestResult, account, connection, lobbies, wait_for_database_lock};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct Room {
    host: Uuid,
    id: Uuid,
    profile: Uuid,
    revision: Uuid,
}
async fn room(c: &Client, public: bool) -> FixtureResult<Room> {
    let f = lobbies::lobby(c).await?;
    let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES($1,'fixture','fixture',2,2,'fixture') RETURNING id", &[&Uuid::new_v4().to_string()]).await?.get(0);
    // Draft fixture configuration; canonical host configuration is the next port.
    c.execute(
        "UPDATE app.room SET profile_id=$1,visibility=$2 WHERE id=$3",
        &[
            &profile,
            &if public { "public" } else { "private" },
            &f.room,
        ],
    )
    .await?;
    Ok(Room {
        host: f.host,
        id: f.room,
        profile,
        revision: f.revision,
    })
}
async fn join(
    c: &Client,
    a: Uuid,
    room: Uuid,
    token: Option<&str>,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.join_room($1,$2,$3,$4)",
        &[&a, &room, &token, &request],
    )
    .await
    .map(|r| r.get(0))
}
fn resource(v: &Value) -> Uuid {
    assert_eq!(v["status"], "success");
    v["data"]["resourceId"].as_str().unwrap().parse().unwrap()
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn public_join_uses_available_human_capacity_and_replays_without_duplicate_membership()
-> TestResult {
    let c = connection().await?;
    let f = room(&c, true).await?;
    let (a, _) = account(&c).await?;
    let (b, _) = account(&c).await?;
    let request = Uuid::new_v4();
    let result = join(&c, a, f.id, None, request).await?;
    assert_eq!(resource(&result), f.id);
    assert_eq!(join(&c, a, f.id, None, request).await?, result);
    assert_eq!(
        join(&c, b, f.id, None, Uuid::new_v4()).await?["data"]["capacity"],
        "full"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.id, &a]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    assert_eq!(c.execute("INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES($1,$2,2,'human',false)", &[&f.id,&b]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    let row = c
        .query_one(
            "SELECT seat_capacity FROM app.table_profile_revision WHERE id=$1",
            &[&f.profile],
        )
        .await?;
    assert_eq!(row.get::<_, i64>(0), 2);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn private_game_invitation_admits_into_capacity_and_revocation_blocks_new_joins() -> TestResult
{
    let c = connection().await?;
    let f = room(&c, false).await?;
    let (a, _) = account(&c).await?;
    let token = "a".repeat(64);
    assert_eq!(
        join(&c, a, f.id, None, Uuid::new_v4()).await?["status"],
        "fail"
    );
    let invitation: Value = c
        .query_one(
            "SELECT app.issue_room_invitation($1,$2,0,$3,$4)",
            &[&f.host, &f.id, &token, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    let invite = resource(&invitation);
    let result = join(&c, a, f.id, Some(&token), Uuid::new_v4()).await?;
    assert_eq!(result["status"], "success");
    let revoked: Value = c
        .query_one(
            "SELECT app.revoke_room_invitation($1,$2,$3)",
            &[&f.host, &invite, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(revoked["status"], "success");
    c.query_one(
        "SELECT app.leave_room($1,$2,1,$3)",
        &[&a, &f.id, &Uuid::new_v4()],
    )
    .await?;
    let (b, _) = account(&c).await?;
    assert_eq!(
        join(&c, b, f.id, Some(&token), Uuid::new_v4()).await?["data"]["invitation"],
        "unavailable"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competing_joins_wait_and_cannot_occupy_the_last_slot_twice() -> TestResult {
    let c = connection().await?;
    let f = room(&c, true).await?;
    let (a, _) = account(&c).await?;
    let (b, _) = account(&c).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    assert_eq!(
        join(&c, a, f.id, None, Uuid::new_v4()).await?["status"],
        "success"
    );
    let task = tokio::spawn(async move { join(&waiting, b, f.id, None, Uuid::new_v4()).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??["data"]["capacity"], "full");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn raw_queue_and_invitation_sources_cannot_begin_in_terminal_or_foreign_states() -> TestResult
{
    let c = connection().await?;
    let f = room(&c, true).await?;
    let (other, _) = account(&c).await?;
    for state in ["reserved", "canceled", "nonsense"] {
        assert_eq!(c.execute("INSERT INTO app.queue_entry(account_id,deck_revision_id,mode,competitive,preferences_ref,state) VALUES($1,$2,'collection',false,'fixture',$3)", &[&f.host,&f.revision,&state]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    }
    let token = Uuid::new_v4().simple().to_string().repeat(2);
    assert_eq!(c.execute("INSERT INTO app.invitation(room_id,issuer_id,token_hash,state) VALUES($1,$2,$3,'canceled')", &[&f.id,&f.host,&token]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(c.execute("INSERT INTO app.invitation(room_id,issuer_id,token_hash,state) VALUES($1,$2,$3,'active')", &[&f.id,&other,&token]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute(
        "INSERT INTO app.invitation(room_id,issuer_id,token_hash,state) VALUES($1,$2,$3,'active')",
        &[&f.id, &other, &token],
    )
    .await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.room SET host_id=$1 WHERE id=$2",
            &[&other, &f.id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
