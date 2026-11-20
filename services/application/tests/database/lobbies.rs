//! Canonical lobby transactions over owner-seeded configuration, not game admission.
use super::{TestResult, account, connection, decks, wait_for_database_lock};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

pub(super) struct Lobby {
    pub host: Uuid,
    pub room: Uuid,
    pub profile: Uuid,
    pub format: Uuid,
    pub revision: Uuid,
}

fn resource(value: &Value) -> Uuid {
    assert_eq!(value["status"], "success");
    value["data"]["resourceId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

pub(super) async fn lobby(c: &Client) -> Result<Lobby, Box<dyn std::error::Error + Send + Sync>> {
    let (host, _, deck, _, format) = decks::library(c).await?;
    let saved: Value = c
        .query_one(
            "SELECT app.save_deck_contents($1,$2,NULL,'[]'::jsonb,$3)",
            &[&host, &deck, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    let revision = resource(&saved);
    let profile = Uuid::new_v4();
    c.execute("INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref) VALUES($1,$1::uuid::text,'fixture','fixture')", &[&profile]).await?;
    let created: Value = c
        .query_one(
            "SELECT app.create_room($1,$2,$3,'collection',false,'private',$4)",
            &[&host, &format, &profile, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    Ok(Lobby {
        host,
        room: resource(&created),
        profile,
        format,
        revision,
    })
}

async fn choose(
    c: &Client,
    actor: Uuid,
    room: Uuid,
    expected: &str,
    revision: Option<Uuid>,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.select_room_deck($1,$2,$3::text::numeric,$4,$5)",
        &[&actor, &room, &expected, &revision, &request],
    )
    .await
    .map(|r| r.get(0))
}

async fn leave(
    c: &Client,
    actor: Uuid,
    room: Uuid,
    expected: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.leave_room($1,$2,$3::text::numeric,$4)",
        &[&actor, &room, &expected, &request],
    )
    .await
    .map(|r| r.get(0))
}

async fn guest(c: &Client, room: Uuid) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
    let (actor, _) = account(c).await?;
    c.execute("INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES($1,$2,1,'human',true)", &[&room,&actor]).await?;
    Ok(actor)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_creation_is_source_bound_atomic_and_rejects_invalid_configuration() -> TestResult {
    let c = connection().await?;
    let f = lobby(&c).await?;
    let request = Uuid::new_v4();
    let sql = "SELECT app.create_room($1,$2,$3,$4,$5,'public',$6)";
    let accepted: Value = c
        .query_one(
            sql,
            &[
                &f.host,
                &f.format,
                &f.profile,
                &"collection",
                &true,
                &request,
            ],
        )
        .await?
        .get(0);
    let room = resource(&accepted);
    assert_eq!(
        c.query_one(
            sql,
            &[
                &f.host,
                &f.format,
                &f.profile,
                &"collection",
                &true,
                &request
            ]
        )
        .await?
        .get::<_, Value>(0),
        accepted
    );
    let error = c
        .query_one(
            sql,
            &[&f.host, &f.format, &f.profile, &"open", &false, &request],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::CHECK_VIOLATION));
    let slot = c.query_one("SELECT account_id,ready,deck_revision_id,seat_index FROM app.room_slot WHERE room_id=$1", &[&room]).await?;
    assert_eq!(slot.get::<_, Uuid>(0), f.host);
    assert!(!slot.get::<_, bool>(1));
    assert_eq!(slot.get::<_, Option<Uuid>>(2), None);
    assert_eq!(slot.get::<_, i64>(3), 0);
    for (mode, competitive) in [("open", true), ("unknown", false)] {
        let result: Value = c
            .query_one(
                sql,
                &[
                    &f.host,
                    &f.format,
                    &f.profile,
                    &mode,
                    &competitive,
                    &Uuid::new_v4(),
                ],
            )
            .await?
            .get(0);
        assert_eq!(
            result,
            json!({"status":"fail","data":{"configuration":"invalid"}})
        );
    }
    let absent = Uuid::new_v4();
    let result: Value = c
        .query_one(
            sql,
            &[
                &f.host,
                &f.format,
                &absent,
                &"open",
                &false,
                &Uuid::new_v4(),
            ],
        )
        .await?
        .get(0);
    assert_eq!(result["data"]["profile"], "unavailable");
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room WHERE host_id=$1",
            &[&f.host]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deck_selection_checks_membership_revision_and_ownership_and_only_resets_its_player()
-> TestResult {
    let c = connection().await?;
    let f = lobby(&c).await?;
    let peer = guest(&c, f.room).await?;
    let foreign = lobby(&c).await?;
    assert_eq!(
        choose(&c, foreign.host, f.room, "0", None, Uuid::new_v4()).await?["data"]["room"],
        "unavailable"
    );
    assert_eq!(
        choose(
            &c,
            f.host,
            f.room,
            "0",
            Some(foreign.revision),
            Uuid::new_v4()
        )
        .await?["data"]["deck"],
        "unavailable"
    );
    for invalid in ["1", "-1", "0.5", "NaN", "Infinity"] {
        assert_eq!(
            choose(
                &c,
                f.host,
                f.room,
                invalid,
                Some(f.revision),
                Uuid::new_v4()
            )
            .await?["data"]["revision"],
            "conflict"
        );
    }
    c.execute("UPDATE app.room SET state='countdown',countdown_at=clock_timestamp()+interval '10 seconds' WHERE id=$1", &[&f.room]).await?;
    c.execute(
        "UPDATE app.room_slot SET ready=true WHERE room_id=$1",
        &[&f.room],
    )
    .await?;
    let request = Uuid::new_v4();
    let accepted = choose(&c, f.host, f.room, "0", Some(f.revision), request).await?;
    resource(&accepted);
    assert_eq!(
        choose(&c, f.host, f.room, "0", Some(f.revision), request).await?,
        accepted
    );
    let room = c
        .query_one(
            "SELECT state,revision::text,countdown_at IS NULL FROM app.room WHERE id=$1",
            &[&f.room],
        )
        .await?;
    assert_eq!(room.get::<_, String>(0), "lobby");
    assert_eq!(room.get::<_, String>(1), "1");
    assert!(room.get::<_, bool>(2));
    assert!(
        !c.query_one(
            "SELECT ready FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.room, &f.host]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert!(
        c.query_one(
            "SELECT ready FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.room, &peer]
        )
        .await?
        .get::<_, bool>(0)
    );
    resource(&choose(&c, f.host, f.room, "1", None, Uuid::new_v4()).await?);
    assert_eq!(
        c.query_one(
            "SELECT deck_revision_id FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.room, &f.host]
        )
        .await?
        .get::<_, Option<Uuid>>(0),
        None
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn guest_leaving_cancels_countdown_and_host_leaving_closes_everyone_with_replay() -> TestResult
{
    let c = connection().await?;
    let f = lobby(&c).await?;
    let peer = guest(&c, f.room).await?;
    c.execute("UPDATE app.room SET state='countdown',countdown_at=clock_timestamp()+interval '10 seconds' WHERE id=$1", &[&f.room]).await?;
    c.execute(
        "UPDATE app.room_slot SET ready=true WHERE room_id=$1",
        &[&f.room],
    )
    .await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&peer]).await?;
    resource(&leave(&c, peer, f.room, "0", Uuid::new_v4()).await?);
    assert_eq!(
        c.query_one(
            "SELECT account_id FROM app.room_slot WHERE room_id=$1 AND seat_index=1",
            &[&f.room]
        )
        .await?
        .get::<_, Option<Uuid>>(0),
        None
    );
    assert!(
        c.query_one(
            "SELECT ready FROM app.room_slot WHERE room_id=$1 AND account_id=$2",
            &[&f.room, &f.host]
        )
        .await?
        .get::<_, bool>(0)
    );
    let request = Uuid::new_v4();
    let accepted = leave(&c, f.host, f.room, "1", request).await?;
    resource(&accepted);
    assert_eq!(leave(&c, f.host, f.room, "1", request).await?, accepted);
    let row = c
        .query_one(
            "SELECT state,revision::text,countdown_at IS NULL FROM app.room WHERE id=$1",
            &[&f.room],
        )
        .await?;
    assert_eq!(row.get::<_, String>(0), "closed");
    assert_eq!(row.get::<_, String>(1), "2");
    assert!(row.get::<_, bool>(2));
    assert!(
        !c.query_one(
            "SELECT bool_or(ready) FROM app.room_slot WHERE room_id=$1",
            &[&f.room]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(
        choose(&c, f.host, f.room, "2", Some(f.revision), Uuid::new_v4()).await?["data"]["room"],
        "unavailable"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn waiting_guest_selection_rechecks_host_closure_after_actual_room_lock_wait() -> TestResult {
    let c = connection().await?;
    let f = lobby(&c).await?;
    let peer = guest(&c, f.room).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    resource(&leave(&c, f.host, f.room, "0", Uuid::new_v4()).await?);
    let room = f.room;
    let task =
        tokio::spawn(async move { choose(&waiting, peer, room, "0", None, Uuid::new_v4()).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??,
        json!({"status":"fail","data":{"room":"unavailable"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn private_lobby_writers_remain_closed_to_every_runtime_role() -> TestResult {
    let c = connection().await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for signature in [
            "app.create_room(uuid,uuid,uuid,text,boolean,text,uuid)",
            "app.select_room_deck(uuid,uuid,numeric,uuid,uuid)",
            "app.leave_room(uuid,uuid,numeric,uuid)",
        ] {
            assert!(
                !c.query_one(
                    "SELECT has_function_privilege($1::name,$2,'EXECUTE')",
                    &[&role, &signature]
                )
                .await?
                .get::<_, bool>(0)
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_accounts_cannot_create_or_select_but_can_release_existing_lobby_membership()
-> TestResult {
    let c = connection().await?;
    let f = lobby(&c).await?;
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.host]).await?;
    let denied: Value = c
        .query_one(
            "SELECT app.create_room($1,$2,$3,'collection',false,'private',$4)",
            &[&f.host, &f.format, &f.profile, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(
        denied,
        json!({"status":"fail","data":{"account":"unavailable"}})
    );
    assert_eq!(
        choose(&c, f.host, f.room, "0", Some(f.revision), Uuid::new_v4()).await?,
        denied
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.room WHERE host_id=$1",
            &[&f.host]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    resource(&leave(&c, f.host, f.room, "0", Uuid::new_v4()).await?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn rejected_room_terms_remain_rejected_after_revision_changes_and_cannot_be_rebound()
-> TestResult {
    let c = connection().await?;
    let f = lobby(&c).await?;
    let deck_request = Uuid::new_v4();
    let leave_request = Uuid::new_v4();
    let rejected = choose(&c, f.host, f.room, "1", Some(f.revision), deck_request).await?;
    assert_eq!(rejected["data"]["revision"], "conflict");
    assert_eq!(
        leave(&c, f.host, f.room, "1", leave_request).await?,
        rejected
    );
    c.execute("UPDATE app.room SET revision=1 WHERE id=$1", &[&f.room])
        .await?;
    assert_eq!(
        choose(&c, f.host, f.room, "1", Some(f.revision), deck_request).await?,
        rejected
    );
    assert_eq!(
        leave(&c, f.host, f.room, "1", leave_request).await?,
        rejected
    );
    assert_eq!(
        choose(&c, f.host, f.room, "0", Some(f.revision), deck_request)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        leave(&c, f.host, f.room, "0", leave_request)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    resource(&choose(&c, f.host, f.room, "1", Some(f.revision), Uuid::new_v4()).await?);
    c.execute(
        "UPDATE app.room_slot SET ready=true WHERE room_id=$1",
        &[&f.room],
    )
    .await?;
    resource(&choose(&c, f.host, f.room, "2", Some(f.revision), Uuid::new_v4()).await?);
    let row = c.query_one("SELECT r.revision::text,s.ready FROM app.room r JOIN app.room_slot s ON s.room_id=r.id WHERE r.id=$1", &[&f.room]).await?;
    assert_eq!(row.get::<_, String>(0), "2");
    assert!(row.get::<_, bool>(1));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn own_deck_selection_rejects_wrong_mode_format_and_unsealed_revisions() -> TestResult {
    let c = connection().await?;
    let f = lobby(&c).await?;
    let other = lobby(&c).await?;
    let deck: Uuid = c
        .query_one(
            "SELECT deck_id FROM app.deck_revision WHERE id=$1",
            &[&f.revision],
        )
        .await?
        .get(0);
    c.execute("UPDATE app.deck SET mode='open' WHERE id=$1", &[&deck])
        .await?;
    assert_eq!(
        choose(&c, f.host, f.room, "0", Some(f.revision), Uuid::new_v4()).await?["data"]["deck"],
        "unavailable"
    );
    c.execute(
        "UPDATE app.deck SET mode='collection',format_id=$2 WHERE id=$1",
        &[&deck, &other.format],
    )
    .await?;
    assert_eq!(
        choose(&c, f.host, f.room, "0", Some(f.revision), Uuid::new_v4()).await?["data"]["deck"],
        "unavailable"
    );
    c.execute(
        "UPDATE app.deck SET format_id=$2 WHERE id=$1",
        &[&deck, &f.format],
    )
    .await?;
    c.batch_execute("BEGIN").await?;
    let unsealed: Uuid = c.query_one("INSERT INTO app.deck_revision(deck_id,revision_number,created_at) VALUES($1,99,clock_timestamp()) RETURNING id", &[&deck]).await?.get(0);
    assert_eq!(
        choose(&c, f.host, f.room, "0", Some(unsealed), Uuid::new_v4()).await?["data"]["deck"],
        "unavailable"
    );
    c.batch_execute("ROLLBACK").await?;
    resource(&choose(&c, f.host, f.room, "0", Some(f.revision), Uuid::new_v4()).await?);
    Ok(())
}
