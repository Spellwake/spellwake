//! Local PostgreSQL coverage of dispatch and canonical admission. The stored
//! qualification uses a fixture proof and does not qualify a gameplay release.
use super::{TestResult, connection, room_start_integrity as rooms};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;

async fn room_with_format(
    c: &Client,
    key: &str,
) -> Result<rooms::Room, Box<dyn std::error::Error + Send + Sync>> {
    let room = rooms::room(c, true).await?;
    c.execute(
        "UPDATE app.format SET key=$2 WHERE id=(SELECT format_id FROM app.room WHERE id=$1)",
        &[&room.id, &key],
    )
    .await?;
    Ok(room)
}

async fn qualify(
    c: &Client,
    room: &rooms::Room,
    key: &str,
    kind: &str,
    actor: Uuid,
    full: bool,
) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
    let auth = c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1", &[&actor]).await?;
    let subject: Uuid = auth.get(0);
    let session: Uuid = auth.get(1);
    let basis = rooms::basis(c, room.id).await?;
    let source: Value = c
        .query_one(
            "SELECT app.read_native_room_source($1,$2::text::numeric,$3,$4)",
            &[&room.id, &basis, &room.source.release, &room.legality],
        )
        .await?
        .get(0);
    assert_eq!(source["status"], "success");
    let request = Uuid::new_v4();
    let original = json!({"requestId":request,"expectedRevision":basis});
    let room_proof = format!(
        "native-constructed-ffa-candidate-v1/{key}/sha256:{}",
        "a".repeat(64)
    );
    let mut prepared = json!({"version":format!("native-{kind}-prepared-v1"),
        "sourceHash":source["data"]["sourceHash"],"releaseId":room.source.release,
        "legalityRevisionId":room.legality,"qualificationRef":room_proof});
    if kind == "ready" {
        prepared["qualificationRef"] = json!(format!(
            "native-constructed-ready-v1/{key}/sha256:{}",
            "b".repeat(64)
        ));
        prepared["selectedRevisionId"] = json!(if actor == room.source.actor {
            room.source.revision
        } else {
            room.other_revision
        });
        if full {
            prepared["countdownQualificationRef"] = json!(room_proof);
        }
    }
    let command = match kind {
        "ready" => "SELECT app.qualify_room_ready_command($1,$2,$3,$4,$5)",
        "countdown" => "SELECT app.qualify_room_countdown_command($1,$2,$3,$4,$5)",
        _ => panic!("unsupported fixture command"),
    };
    let reply: Value = c
        .query_one(
            command,
            &[&subject, &session, &room.id, &original, &prepared],
        )
        .await?
        .get(0);
    assert_eq!(reply["status"], "success", "{reply}");
    let row = c.query_one("SELECT id FROM app.command_receipt WHERE account_id=$1 AND request_ref=$2 AND scope=$3 AND state='accepted'", &[&actor,&request.to_string(),&format!("qualified_{kind}_intent_v1")]).await?;
    Ok(row.get(0))
}

async fn dispatch(c: &Client) -> Result<Value, tokio_postgres::Error> {
    c.query_one("SELECT app.dispatch_validated_countdowns(1)", &[])
        .await
        .map(|row| row.get(0))
}

#[tokio::test]
#[ignore = "requires its own migrated local Postgres database"]
async fn due_sweep_reserves_both_qualified_paths_and_isolates_policy_drift() -> TestResult {
    let c = connection().await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        let access = c.query_one("SELECT has_function_privilege($1,'app.dispatch_validated_countdowns(integer)','EXECUTE'), has_table_privilege($1,'app.countdown_dispatch_cursor','SELECT,INSERT,UPDATE,DELETE')", &[&role]).await?;
        assert!(!access.get::<_, bool>(0));
        assert!(!access.get::<_, bool>(1));
    }
    let changed = room_with_format(&c, "legacy").await?;
    qualify(
        &c,
        &changed,
        "legacy",
        "countdown",
        changed.source.actor,
        false,
    )
    .await?;
    let forced = room_with_format(&c, "modern").await?;
    let forced_receipt = qualify(
        &c,
        &forced,
        "modern",
        "countdown",
        forced.source.actor,
        false,
    )
    .await?;
    let ready = room_with_format(&c, "pioneer").await?;
    qualify(&c, &ready, "pioneer", "ready", ready.other, false).await?;
    let ready_receipt = qualify(&c, &ready, "pioneer", "ready", ready.source.actor, true).await?;
    let ready_basis = rooms::basis(&c, ready.id).await?;
    let noop_receipt = qualify(
        &c,
        &ready,
        "pioneer",
        "countdown",
        ready.source.actor,
        false,
    )
    .await?;
    assert_eq!(ready_basis, rooms::basis(&c, ready.id).await?);
    let unproved = room_with_format(&c, "standard").await?;
    let unproved_basis = rooms::basis(&c, unproved.id).await?;
    let older: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:legacy-room',$4)",
            &[
                &unproved.source.actor,
                &unproved.id,
                &unproved_basis,
                &Uuid::new_v4(),
            ],
        )
        .await?
        .get(0);
    assert_eq!(older["status"], "success", "{older}");
    let rooms = [changed.id, forced.id, ready.id, unproved.id];
    c.execute("UPDATE app.room SET countdown_at=clock_timestamp() + interval '1 hour' WHERE id=ANY($1) AND state='countdown'", &[&&rooms[..]]).await?;
    assert_eq!(dispatch(&c).await?["examined"], 0);

    c.execute("UPDATE app.format SET key='changed-dispatch-policy' WHERE id=(SELECT format_id FROM app.room WHERE id=$1)", &[&changed.id]).await?;
    c.execute("UPDATE app.room r SET countdown_at=clock_timestamp() - candidate.age * interval '1 second' FROM unnest($1::uuid[]) WITH ORDINALITY AS candidate(id,age) WHERE r.id=candidate.id", &[&&[ready.id, forced.id, changed.id, unproved.id][..]]).await?;
    let concurrent = connection().await?;
    concurrent
        .batch_execute("BEGIN; SELECT * FROM app.countdown_dispatch_cursor FOR UPDATE")
        .await?;
    assert_eq!(dispatch(&c).await?, json!({"state":"busy"}));
    concurrent.batch_execute("ROLLBACK").await?;
    let skipped = dispatch(&c).await?;
    assert_eq!(skipped["examined"], 1);
    assert_eq!(skipped["attempted"], 0);
    let failed = dispatch(&c).await?;
    assert_eq!(failed["failed"], 1, "{failed}");
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.match WHERE room_id=$1",
            &[&changed.id]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    for (room, original) in [(&forced, forced_receipt), (&ready, ready_receipt)] {
        let result = dispatch(&c).await?;
        assert_eq!(result["reserved"], 1, "{result}");
        let row = concurrent.query_one("SELECT m.id,g.id,h.state,(SELECT request_ref FROM app.command_receipt WHERE target_ref=$1::uuid::text AND scope='room_start_v1' AND state='accepted') FROM app.match m JOIN app.game g ON g.match_id=m.id AND g.state='reserved' JOIN app.durable_handoff h ON h.game_id=g.id AND h.kind='ready_match' WHERE m.room_id=$1", &[&room.id]).await?;
        let match_id: Uuid = row.get(0);
        let game_id: Uuid = row.get(1);
        assert_eq!(row.get::<_, String>(2), "pending");
        assert_eq!(row.get::<_, String>(3), original.to_string());
        assert_ne!(original, noop_receipt);
        let basis: String = c
            .query_one(
                "SELECT response_payload#>>'{data,revision}' FROM app.command_receipt WHERE id=$1",
                &[&original],
            )
            .await?
            .get(0);
        let retry: Value = c
            .query_one(
                "SELECT app.start_validated_countdown($1,$2::text::numeric,$3)",
                &[&room.id, &basis, &original],
            )
            .await?
            .get(0);
        assert_eq!(
            retry,
            json!({"status":"success", "data":{"resourceId":match_id,"gameId":game_id}})
        );
    }
    assert_eq!(dispatch(&c).await?["examined"], 0);
    assert_eq!(dispatch(&c).await?["attempted"], 0);
    assert_eq!(dispatch(&c).await?["failed"], 1);
    Ok(())
}
