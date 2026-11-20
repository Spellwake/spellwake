//! Normalized invitation/hash fixtures qualify SQL boundaries, not native admission or token generation.
use super::{TestResult, account, connection, lobbies, set_auth_claims, wait_for_database_lock};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
async fn identity(c: &Client, actor: Uuid) -> Result<(Uuid, Uuid), tokio_postgres::Error> {
    let r=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&actor]).await?;
    Ok((r.get(0), r.get(1)))
}
async fn basis(c: &Client, room: Uuid) -> Result<String, tokio_postgres::Error> {
    c.query_one("SELECT revision::text FROM app.room WHERE id=$1", &[&room])
        .await
        .map(|r| r.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn invitation_facades_recover_hash_only_receipts_and_revoke_without_evicting_current_members()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (guest, _) = account(&c).await?;
    let (host_subject, host_session) = identity(&c, f.host).await?;
    let (guest_subject, guest_session) = identity(&c, guest).await?;
    let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES($1,'fixture','fixture',2,2,'fixture:sql-only') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    c.execute(
        "UPDATE app.room SET profile_id=$1 WHERE id=$2",
        &[&profile, &f.room],
    )
    .await?;
    let expected = basis(&c, f.room).await?;
    let hash = "9".repeat(64);
    let issue = Uuid::new_v4();
    let join = Uuid::new_v4();
    let revoke = Uuid::new_v4();
    let lookup = "SELECT app.read_room_invitation_command($1,$2,$3,$4,$5,$6::text::numeric,$7)";
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            lookup,
            &[
                &host_subject,
                &host_session,
                &"issue",
                &issue,
                &f.room,
                &Some(expected.clone()),
                &Some(hash.clone())
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["state"],
        "unknown"
    );
    let issued: Value = c
        .query_one(
            "SELECT app.issue_room_invitation_command($1,$2,$3,$4,$5::text::numeric,$6)",
            &[
                &host_subject,
                &host_session,
                &issue,
                &f.room,
                &expected,
                &hash,
            ],
        )
        .await?
        .get(0);
    assert_eq!(issued["status"], "success");
    let invitation: Uuid = issued["data"]["resourceId"].as_str().unwrap().parse()?;
    assert_eq!(
        c.query_one(
            lookup,
            &[
                &host_subject,
                &host_session,
                &"issue",
                &issue,
                &f.room,
                &Some(expected.clone()),
                &Some(hash.clone())
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["response"],
        issued
    );
    assert_eq!(
        c.query_one(
            lookup,
            &[
                &host_subject,
                &host_session,
                &"issue",
                &issue,
                &f.room,
                &Some(expected.clone()),
                &Some("8".repeat(64))
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["request"],
        "conflict"
    );
    let denied: Value = c
        .query_one(
            "SELECT app.join_room_command($1,$2,$3,$4,NULL)",
            &[&guest_subject, &guest_session, &Uuid::new_v4(), &f.room],
        )
        .await?
        .get(0);
    assert_eq!(denied["data"]["invitation"], "unavailable");
    let joined: Value = c
        .query_one(
            "SELECT app.join_room_command($1,$2,$3,$4,$5)",
            &[&guest_subject, &guest_session, &join, &f.room, &hash],
        )
        .await?
        .get(0);
    assert_eq!(joined["status"], "success");
    assert_eq!(joined["data"]["resourceId"], f.room.to_string());
    assert!(joined["data"]["slotId"].is_string());
    let revoked: Value = c
        .query_one(
            "SELECT app.revoke_room_invitation_command($1,$2,$3,$4)",
            &[&host_subject, &host_session, &revoke, &invitation],
        )
        .await?
        .get(0);
    assert_eq!(revoked["status"], "success");
    let member_noop: Value = c
        .query_one(
            "SELECT app.join_room_command($1,$2,$3,$4,NULL)",
            &[&guest_subject, &guest_session, &Uuid::new_v4(), &f.room],
        )
        .await?
        .get(0);
    assert_eq!(member_noop, joined);
    c.batch_execute("RESET ROLE").await?;
    let current = basis(&c, f.room).await?;
    c.batch_execute("SET ROLE service_role").await?;
    let left: Value = c
        .query_one(
            "SELECT app.leave_room_command($1,$2,$3,$4,$5::text::numeric)",
            &[
                &guest_subject,
                &guest_session,
                &Uuid::new_v4(),
                &f.room,
                &current,
            ],
        )
        .await?
        .get(0);
    assert_eq!(left["status"], "success");
    assert_eq!(
        c.query_one(
            "SELECT app.join_room_command($1,$2,$3,$4,$5)",
            &[
                &guest_subject,
                &guest_session,
                &Uuid::new_v4(),
                &f.room,
                &hash
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["invitation"],
        "unavailable"
    );
    assert_eq!(
        c.query_one(
            lookup,
            &[
                &guest_subject,
                &guest_session,
                &"join",
                &join,
                &f.room,
                &None::<String>,
                &Some(hash.clone())
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["response"],
        joined
    );
    c.batch_execute("RESET ROLE").await?;
    set_auth_claims(&c, guest_subject).await?;
    assert_eq!(
        c.query_one("SELECT app.read_my_room_invitations(NULL,NULL,100)", &[])
            .await?
            .get::<_, Value>(0)["data"]["items"],
        json!([])
    );
    set_auth_claims(&c, host_subject).await?;
    let own: Value = c
        .query_one(
            "SELECT app.read_my_room_invitations($1,NULL,100)",
            &[&f.room],
        )
        .await?
        .get(0);
    let items = own["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].as_object().unwrap().len(), 3);
    assert_eq!(items[0]["state"], "canceled");
    c.batch_execute("RESET ROLE").await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&f.host]).await?;
    let current = basis(&c, f.room).await?;
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            "SELECT app.revoke_room_invitation_command($1,$2,$3,$4)",
            &[&host_subject, &host_session, &Uuid::new_v4(), &invitation]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "success"
    );
    assert_eq!(
        c.query_one(
            "SELECT app.issue_room_invitation_command($1,$2,$3,$4,$5::text::numeric,$6)",
            &[
                &host_subject,
                &host_session,
                &Uuid::new_v4(),
                &f.room,
                &current,
                &hash
            ]
        )
        .await?
        .get::<_, Value>(0)["data"]["account"],
        "unavailable"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn join_facade_rechecks_session_after_sorted_account_lock_wait() -> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (guest, _) = account(&c).await?;
    let (subject, session) = identity(&c, guest).await?;
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
    let task = tokio::spawn(async move {
        waiting
            .query_one(
                "SELECT app.join_room_command($1,$2,$3,$4,NULL)",
                &[&subject, &session, &Uuid::new_v4(), &room],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    wait_for_database_lock(&c, pid).await?;
    c.execute(
        "UPDATE auth.sessions SET not_after=statement_timestamp() WHERE id=$1",
        &[&session],
    )
    .await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.room, &guest]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}
