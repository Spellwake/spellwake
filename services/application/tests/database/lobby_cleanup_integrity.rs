//! Database cleanup provenance and FK races; no invitation, controller or native producer proof.
use super::{TestResult, account, connection, deletion, lobbies, wait_for_database_lock};
use tokio_postgres::Client;
use uuid::Uuid;

async fn clean(c: &Client, owner: Uuid) -> Result<i32, tokio_postgres::Error> {
    c.query_one("SELECT app.cleanup_pre_game_lobbies($1,100)", &[&owner])
        .await
        .map(|r| r.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deleting_host_cleans_only_source_qualified_pre_game_rooms_and_retires_without_room_anchors()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (guest, _) = account(&c).await?;
    c.execute("INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'human',true)", &[&f.room,&guest]).await?;
    assert_eq!(clean(&c, f.host).await?, 0);
    deletion::request(&c, f.host, Uuid::new_v4()).await?;
    assert_eq!(clean(&c, f.host).await?, 1);
    assert_eq!(clean(&c, f.host).await?, 0);
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room WHERE id=$1",
            &[&f.room]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1",
            &[&f.room]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(
        c.query_one("SELECT state FROM app.account WHERE id=$1", &[&guest])
            .await?
            .get::<_, String>(0),
        "active"
    );
    let result: serde_json::Value = c
        .query_one("SELECT app.retire_account($1)", &[&f.host])
        .await?
        .get(0);
    assert_eq!(result["data"]["state"], "retired");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deleting_guest_vacates_its_slot_without_erasing_another_hosts_lobby_or_ready_state()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let (guest, _) = account(&c).await?;
    c.execute("INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'human',true)", &[&f.room,&guest]).await?;
    c.execute(
        "UPDATE app.room_slot SET ready=true WHERE room_id=$1",
        &[&f.room],
    )
    .await?;
    c.execute("UPDATE app.room SET state='countdown',countdown_at=clock_timestamp()+interval '10 seconds' WHERE id=$1", &[&f.room]).await?;
    deletion::request(&c, guest, Uuid::new_v4()).await?;
    let result: serde_json::Value = c
        .query_one("SELECT app.retire_account($1)", &[&guest])
        .await?
        .get(0);
    assert_eq!(result["data"]["state"], "retired");
    let room = c
        .query_one(
            "SELECT host_id,state,countdown_at IS NULL,revision::text FROM app.room WHERE id=$1",
            &[&f.room],
        )
        .await?;
    assert_eq!(room.get::<_, Uuid>(0), f.host);
    assert_eq!(room.get::<_, String>(1), "lobby");
    assert!(room.get::<_, bool>(2));
    assert_eq!(room.get::<_, String>(3), "1");
    assert!(
        c.query_one(
            "SELECT ready FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.room, &f.host]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(
        c.query_one(
            "SELECT account_id FROM app.room_slot WHERE room_id=$1 AND seat_index=1",
            &[&f.room]
        )
        .await?
        .get::<_, Option<Uuid>>(0),
        None
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn cleanup_preserves_every_matched_invited_or_controller_owned_room() -> TestResult {
    let c = connection().await?;
    for kind in ["match", "invitation", "controller"] {
        let f = lobbies::lobby(&c).await?;
        match kind {
            "match" => {
                let legality = Uuid::new_v4();
                c.execute("INSERT INTO app.legality_revision(id,source_ref,effective_at,imported_at) VALUES($1,$1::uuid::text,clock_timestamp(),clock_timestamp())", &[&legality]).await?;
                c.execute("INSERT INTO app.match(room_id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) VALUES($1,$2,$3,$4,'collection',false,'bo1','completed',0)", &[&f.room,&f.format,&f.profile,&legality]).await?;
            }
            "invitation" => {
                let token = Uuid::new_v4().simple().to_string().repeat(2);
                let issued: serde_json::Value = c
                    .query_one(
                        "SELECT app.issue_room_invitation($1,$2,0,$3,$4)",
                        &[&f.host, &f.room, &token, &Uuid::new_v4()],
                    )
                    .await?
                    .get(0);
                let invite: Uuid = issued["data"]["resourceId"].as_str().unwrap().parse()?;
                c.query_one(
                    "SELECT app.revoke_room_invitation($1,$2,$3)",
                    &[&f.host, &invite, &Uuid::new_v4()],
                )
                .await?;
            }
            _ => {
                let slot: Uuid = c.query_one("INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'ai',false) RETURNING id", &[&f.room,&f.host]).await?.get(0);
                c.execute("INSERT INTO app.ai_runner_binding(room_slot_id,binding_epoch,desired_state,observed_state) VALUES($1,1,'running','pending')", &[&slot]).await?;
            }
        }
        deletion::request(&c, f.host, Uuid::new_v4()).await?;
        assert_eq!(clean(&c, f.host).await?, 0, "{kind}");
        let result: serde_json::Value = c
            .query_one("SELECT app.retire_account($1)", &[&f.host])
            .await?
            .get(0);
        assert_eq!(result["data"]["state"], "waiting", "{kind}");
        assert_eq!(
            c.query_one(
                "SELECT count(*)::bigint FROM app.room WHERE id=$1",
                &[&f.room]
            )
            .await?
            .get::<_, i64>(0),
            1
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn canonical_deletion_intent_and_room_creation_are_both_required_not_just_pending_or_closed_labels()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.host]).await?;
    assert_eq!(clean(&c, f.host).await?, 0);
    let g = lobbies::lobby(&c).await?;
    c.execute(
        "UPDATE app.room SET visibility='public',state='closed' WHERE id=$1",
        &[&g.room],
    )
    .await?;
    deletion::request(&c, g.host, Uuid::new_v4()).await?;
    assert_eq!(clean(&c, g.host).await?, 0);
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room WHERE id IN ($1,$2)",
            &[&f.room, &g.room]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deletion_waits_for_inflight_invitation_source_then_preserves_its_committed_anchor()
-> TestResult {
    let observer = connection().await?;
    let f = lobbies::lobby(&observer).await?;
    let reference = connection().await?;
    reference.batch_execute("BEGIN").await?;
    let token = Uuid::new_v4().simple().to_string().repeat(2);
    let issued: serde_json::Value = reference
        .query_one(
            "SELECT app.issue_room_invitation($1,$2,0,$3,$4)",
            &[&f.host, &f.room, &token, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(issued["status"], "success");
    let worker = connection().await?;
    let pid: i32 = worker
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let task =
        tokio::spawn(async move { deletion::request(&worker, f.host, Uuid::new_v4()).await });
    wait_for_database_lock(&observer, pid).await?;
    reference.batch_execute("COMMIT").await?;
    task.await??;
    assert_eq!(clean(&observer, f.host).await?, 0);
    assert_eq!(
        observer
            .query_one(
                "SELECT count(*)::bigint FROM app.room WHERE id=$1",
                &[&f.room]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    Ok(())
}
