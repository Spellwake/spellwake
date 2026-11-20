//! Actual SQL identity/lock boundaries; no hosted AI execution is qualified.
use super::{TestResult, account, connection, lobbies, wait_for_database_lock};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
async fn identity(c: &Client, actor: Uuid) -> Result<(Uuid, Uuid), tokio_postgres::Error> {
    let row=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&actor]).await?;
    Ok((row.get(0), row.get(1)))
}
async fn qualified(c: &Client, room: Uuid, capacity: i64) -> Result<(), tokio_postgres::Error> {
    let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES($1,'fixture','fixture',2,$2,'fixture:structural-only') RETURNING id",&[&Uuid::new_v4().to_string(),&capacity]).await?.get(0);
    c.execute(
        "UPDATE app.room SET profile_id=$1 WHERE id=$2",
        &[&profile, &room],
    )
    .await?;
    Ok(())
}
async fn add(
    c: &Client,
    subject: Uuid,
    session: Uuid,
    request: Uuid,
    room: Uuid,
    basis: &str,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.add_room_ai_slot_command($1,$2,$3,$4,$5::text::numeric)",
        &[&subject, &session, &request, &room, &basis],
    )
    .await
    .map(|r| r.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn manual_ai_add_is_self_sponsored_reuses_empty_human_and_recovers_retired_history()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    qualified(&c, f.room, 4).await?;
    let (guest, _) = account(&c).await?;
    let (stranger, stranger_subject) = account(&c).await?;
    c.execute("INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'human',true)",&[&f.room,&guest]).await?;
    let empty:Uuid=c.query_one("INSERT INTO app.room_slot(room_id,seat_index,team_ref,controller_kind,ready) VALUES($1,2,'blue','human',false) RETURNING id",&[&f.room]).await?.get(0);
    let config: Uuid = c
        .query_one(
            "SELECT configuration_id FROM app.room WHERE id=$1",
            &[&f.room],
        )
        .await?
        .get(0);
    let (subject, session) = identity(&c, guest).await?;
    let (_, stranger_session) = identity(&c, stranger).await?;
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        add(
            &c,
            stranger_subject,
            stranger_session,
            Uuid::new_v4(),
            f.room,
            "0"
        )
        .await?["data"]["room"],
        "unavailable"
    );
    let accepted = add(&c, subject, session, request, f.room, "0").await?;
    assert_eq!(
        accepted,
        json!({"status":"success","data":{"resourceId":empty,"revision":"1"}})
    );
    assert_eq!(
        add(&c, subject, session, request, f.room, "0").await?,
        accepted
    );
    assert_eq!(
        add(&c, subject, session, request, f.room, "1").await?["data"]["request"],
        "conflict"
    );
    let lookup = "SELECT app.read_add_room_ai_slot_command($1,$2,$3,$4,$5::text::numeric)";
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &f.room, &"0"])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    assert_eq!(
        c.query_one(
            lookup,
            &[
                &stranger_subject,
                &stranger_session,
                &request,
                &f.room,
                &"0"
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["state"],
        "unknown"
    );
    c.batch_execute("RESET ROLE").await?;
    let row=c.query_one("SELECT sponsor_id,controller_kind,team_ref,seat_index,ready,deck_revision_id IS NULL FROM app.room_slot WHERE id=$1",&[&empty]).await?;
    assert_eq!(row.get::<_, Uuid>(0), guest);
    assert_eq!(row.get::<_, String>(1), "ai");
    assert_eq!(row.get::<_, String>(2), "blue");
    assert_eq!(row.get::<_, i64>(3), 2);
    assert!(!row.get::<_, bool>(4));
    assert!(row.get::<_, bool>(5));
    assert_ne!(
        c.query_one(
            "SELECT configuration_id FROM app.room WHERE id=$1",
            &[&f.room]
        )
        .await?
        .get::<_, Uuid>(0),
        config
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND controller_kind='human' AND ready",&[&f.room]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.ai_runner_binding WHERE room_slot_id=$1",
            &[&empty]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    c.query_one(
        "SELECT app.remove_ai_slot($1,$2,1,$3)",
        &[&guest, &empty, &Uuid::new_v4()],
    )
    .await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&guest]).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        add(&c, subject, session, request, f.room, "0").await?,
        accepted
    );
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &f.room, &"0"])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    assert_eq!(
        add(&c, subject, session, Uuid::new_v4(), f.room, "2").await?["data"]["account"],
        "unavailable"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn manual_ai_add_checks_qualified_capacity_competitive_state_and_private_capability()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (subject, session) = identity(&c, f.host).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        add(&c, subject, session, Uuid::new_v4(), f.room, "0").await?["data"]["room"],
        "unavailable"
    );
    assert_eq!(
        c.query_one(
            "SELECT app.add_room_ai_slot($1,$2,0,$3)",
            &[&f.host, &f.room, &Uuid::new_v4()]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    c.batch_execute("RESET ROLE").await?;
    qualified(&c, f.room, 2).await?;
    for role in ["anon", "authenticated"] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        assert_eq!(
            add(&c, subject, session, Uuid::new_v4(), f.room, "0")
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        c.batch_execute("RESET ROLE").await?;
    }
    c.execute(
        "UPDATE app.room SET competitive=true WHERE id=$1",
        &[&f.room],
    )
    .await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        add(&c, subject, session, Uuid::new_v4(), f.room, "0").await?["data"]["room"],
        "unavailable"
    );
    c.batch_execute("RESET ROLE").await?;
    c.execute(
        "UPDATE app.room SET competitive=false WHERE id=$1",
        &[&f.room],
    )
    .await?;
    c.batch_execute("SET ROLE service_role").await?;
    let first = add(&c, subject, session, Uuid::new_v4(), f.room, "0").await?;
    assert_eq!(first["status"], "success");
    assert_eq!(
        add(&c, subject, session, Uuid::new_v4(), f.room, "0").await?["data"]["revision"],
        "conflict"
    );
    assert_eq!(
        add(&c, subject, session, Uuid::new_v4(), f.room, "1").await?["data"]["capacity"],
        "full"
    );
    c.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    assert_eq!(
        add(&c, subject, session, Uuid::new_v4(), f.room, "1")
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    c.batch_execute("ROLLBACK; RESET ROLE").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND retired_at IS NULL",
            &[&f.room]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn manual_ai_add_serializes_same_basis_with_distinct_participant_sponsors() -> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    qualified(&c, f.room, 4).await?;
    let (guest, _) = account(&c).await?;
    c.execute("INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'human',false)",&[&f.room,&guest]).await?;
    let (s1, k1) = identity(&c, f.host).await?;
    let (s2, k2) = identity(&c, guest).await?;
    let left = connection().await?;
    let right = connection().await?;
    left.batch_execute("SET ROLE service_role").await?;
    right.batch_execute("SET ROLE service_role").await?;
    let (a, b) = tokio::join!(
        add(&left, s1, k1, Uuid::new_v4(), f.room, "0"),
        add(&right, s2, k2, Uuid::new_v4(), f.room, "0")
    );
    let a = a?;
    let b = b?;
    assert_ne!(a["status"], b["status"], "{a} {b}");
    let rejected = if a["status"] == "fail" { a } else { b };
    assert_eq!(rejected["data"]["revision"], "conflict");
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND retired_at IS NULL AND controller_kind='ai'",&[&f.room]).await?.get::<_,i64>(0),1);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn manual_ai_add_rechecks_session_after_host_account_wait_without_receipt_or_slot_effects()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    qualified(&c, f.room, 4).await?;
    let (guest, _) = account(&c).await?;
    c.execute("INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'human',false)",&[&f.room,&guest]).await?;
    let (subject, session) = identity(&c, guest).await?;
    let request = Uuid::new_v4();
    c.execute(
        "UPDATE auth.sessions SET not_after=clock_timestamp()+interval '2 seconds' WHERE id=$1",
        &[&session],
    )
    .await?;
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
        &[&f.host],
    )
    .await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    waiting.batch_execute("SET ROLE service_role").await?;
    let room = f.room;
    let task =
        tokio::spawn(async move { add(&waiting, subject, session, request, room, "0").await });
    wait_for_database_lock(&c, pid).await?;
    assert!(c.query_one("SELECT a.query_start<s.not_after FROM pg_stat_activity a CROSS JOIN auth.sessions s WHERE a.pid=$1 AND s.id=$2",&[&pid,&session]).await?.get::<_,bool>(0));
    c.query_one("SELECT pg_sleep(greatest(0,extract(epoch FROM(not_after-clock_timestamp())))::double precision+0.025) FROM auth.sessions WHERE id=$1",&[&session]).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key=$1 AND scope='ai_slot_add_v1' AND request_ref=$2",&[&format!("human:{guest}"),&request.to_string()]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND controller_kind='ai'",
            &[&f.room]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn manual_ai_add_invalidates_existing_authenticated_ai_ready_without_starting_a_runner()
-> TestResult {
    let c = connection().await?;
    let f = super::room_start_integrity::room_with_capacity(&c, false, false, 3).await?;
    let existing:Uuid=c.query_one("SELECT id FROM app.room_slot WHERE room_id=$1 AND controller_kind='ai' AND retired_at IS NULL",&[&f.id]).await?.get(0);
    assert!(
        c.query_one("SELECT app.ai_slot_ready($1)", &[&existing])
            .await?
            .get::<_, bool>(0)
    );
    let basis = super::room_start_integrity::basis(&c, f.id).await?;
    let (subject, session) = identity(&c, f.source.actor).await?;
    let before:i64=c.query_one("SELECT count(*)::bigint FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1",&[&f.id]).await?.get(0);
    c.batch_execute("SET ROLE service_role").await?;
    let accepted = add(&c, subject, session, Uuid::new_v4(), f.id, &basis).await?;
    assert_eq!(accepted["status"], "success", "{accepted}");
    c.batch_execute("RESET ROLE").await?;
    assert!(
        !c.query_one("SELECT app.ai_slot_ready($1)", &[&existing])
            .await?
            .get::<_, bool>(0)
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE s.room_id=$1",&[&f.id]).await?.get::<_,i64>(0),before);
    assert_eq!(
        c.query_one("SELECT revision::text FROM app.room WHERE id=$1", &[&f.id])
            .await?
            .get::<_, String>(0),
        accepted["data"]["revision"].as_str().unwrap()
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn manual_ai_add_cannot_mutate_claimed_queue_and_recovers_its_rejection() -> TestResult {
    use super::queue_intent_integrity as queue;
    let c = connection().await?;
    let f = queue::fixture(&c).await?;
    let a = queue::enqueue(
        &c,
        f.room.source.actor,
        &queue::alternatives(f.profile, f.room.source.revision, "duel", "0"),
        Uuid::new_v4(),
    )
    .await?;
    let b = queue::enqueue(
        &c,
        f.room.other,
        &queue::alternatives(f.profile, f.room.other_revision, "duel", "0"),
        Uuid::new_v4(),
    )
    .await?;
    let source = json!({"profileId":f.profile,"configurationKey":"duel","hostEntryId":a["data"]["resourceId"],"humanSeats":[{"queueEntryId":a["data"]["resourceId"],"seatIndex":"0"},{"queueEntryId":b["data"]["resourceId"],"seatIndex":"1"}],"aiSeats":[]});
    let claimed: Value = c
        .query_one(
            "SELECT app.claim_matchmaking($1,$2)",
            &[&source, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(claimed["status"], "success", "{claimed}");
    let room: Uuid = claimed["data"]["resourceId"].as_str().unwrap().parse()?;
    let basis = super::room_start_integrity::basis(&c, room).await?;
    let (subject, session) = identity(&c, f.room.source.actor).await?;
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    let rejected = add(&c, subject, session, request, room, &basis).await?;
    assert_eq!(
        rejected,
        json!({"status":"fail","data":{"queue":"cancel the claimed room before changing its roster"}})
    );
    assert_eq!(
        add(&c, subject, session, request, room, &basis).await?,
        rejected
    );
    let lookup = "SELECT app.read_add_room_ai_slot_command($1,$2,$3,$4,$5::text::numeric)";
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &room, &basis])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        rejected
    );
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &f.room.id, &basis])
            .await?
            .get::<_, Value>(0)["data"]["request"],
        "conflict"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND retired_at IS NULL AND controller_kind='ai'",&[&room]).await?.get::<_,i64>(0),0);
    Ok(())
}
