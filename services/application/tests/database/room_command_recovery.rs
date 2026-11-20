//! Original verified-session room receipts are recoverable without current lobby membership.
use super::{TestResult, account, connection, lobbies};
use serde_json::{Value, json};
use uuid::Uuid;

async fn lookup(
    c: &tokio_postgres::Client,
    subject: Uuid,
    session: Uuid,
    input: &Value,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.read_room_command($1,$2,$3)",
        &[&subject, &session, input],
    )
    .await
    .map(|r| r.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_command_recovery_keeps_numeric_terms_and_checks_session_before_replay() -> TestResult
{
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let row=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&f.host]).await?;
    let subject: Uuid = row.get(0);
    let session: Uuid = row.get(1);
    let (_, foreign_subject) = account(&c).await?;
    let before: i64 = c
        .query_one(
            "SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key=$1",
            &[&format!("human:{}", f.host)],
        )
        .await?
        .get(0);
    c.batch_execute("SET ROLE service_role").await?;
    let create_id = Uuid::new_v4();
    let create = json!({"kind":"create","command":{"requestId":create_id,"formatId":f.format,"profileId":f.profile,"mode":"collection","competitive":false,"visibility":"private"}});
    assert_eq!(
        lookup(&c, subject, session, &create).await?["data"]["state"],
        "unknown"
    );
    let accepted: Value = c
        .query_one(
            "SELECT app.create_room_command($1,$2,$3,$4,$5,'collection',false,'private')",
            &[&subject, &session, &create_id, &f.format, &f.profile],
        )
        .await?
        .get(0);
    assert_eq!(
        lookup(&c, subject, session, &create).await?["data"]["response"],
        accepted
    );
    let mut changed = create.clone();
    changed["command"]["visibility"] = json!("public");
    assert_eq!(
        lookup(&c, subject, session, &changed).await?["data"]["request"],
        "conflict"
    );
    let select_id = Uuid::new_v4();
    let select = json!({"kind":"select_deck","command":{"requestId":select_id,"roomId":f.room,"expectedRevision":"0","deckRevisionId":f.revision}});
    let selected: Value = c
        .query_one(
            "SELECT app.select_room_deck_command($1,$2,$3,$4,0,$5)",
            &[&subject, &session, &select_id, &f.room, &f.revision],
        )
        .await?
        .get(0);
    assert_eq!(selected["status"], "success");
    assert_eq!(
        lookup(&c, subject, session, &select).await?["data"]["response"],
        selected
    );
    let leave_id = Uuid::new_v4();
    let leave = json!({"kind":"leave","command":{"requestId":leave_id,"roomId":f.room,"expectedRevision":"1"}});
    let left: Value = c
        .query_one(
            "SELECT app.leave_room_command($1,$2,$3,$4,1)",
            &[&subject, &session, &leave_id, &f.room],
        )
        .await?
        .get(0);
    assert_eq!(left["status"], "success");
    assert_eq!(
        lookup(&c, subject, session, &leave).await?["data"]["response"],
        left
    );
    assert_eq!(
        lookup(&c, subject, session, &select).await?["data"]["response"],
        selected
    );
    assert_eq!(
        lookup(&c, foreign_subject, session, &leave).await?["data"]["session"],
        "unauthorized"
    );
    assert_eq!(
        lookup(&c, subject, Uuid::new_v4(), &leave).await?["data"]["session"],
        "unauthorized"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key=$1",
            &[&format!("human:{}", f.host)]
        )
        .await?
        .get::<_, i64>(0),
        before + 3
    );
    c.batch_execute("SET ROLE authenticated").await?;
    assert_eq!(
        lookup(&c, subject, session, &leave)
            .await
            .unwrap_err()
            .code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_command_recovery_rejects_missing_nulls_unknown_fields_and_malformed_quantities_without_receipts()
-> TestResult {
    let c = connection().await?;
    let f = lobbies::lobby(&c).await?;
    let row=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&f.host]).await?;
    let subject: Uuid = row.get(0);
    let session: Uuid = row.get(1);
    let input = json!({"kind":"select_deck","command":{"requestId":Uuid::new_v4(),"roomId":f.room,"expectedRevision":"9007199254740993","deckRevisionId":null}});
    let before: i64 = c
        .query_one(
            "SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key=$1",
            &[&format!("human:{}", f.host)],
        )
        .await?
        .get(0);
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        lookup(&c, subject, session, &input).await?["data"]["state"],
        "unknown"
    );
    for mutation in 0..7 {
        let mut invalid = input.clone();
        match mutation {
            0 => {
                invalid["command"]
                    .as_object_mut()
                    .unwrap()
                    .remove("deckRevisionId");
            }
            1 => invalid["command"]["expectedRevision"] = json!(0),
            2 => invalid["command"]["expectedRevision"] = json!("01"),
            3 => invalid["command"]["actorId"] = json!(f.host),
            4 => invalid["command"]["roomId"] = json!(Uuid::nil()),
            5 => invalid["kind"] = json!("ready"),
            _ => invalid = json!([]),
        }
        assert_eq!(
            lookup(&c, subject, session, &invalid).await?["data"]["request"],
            "invalid"
        );
    }
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key=$1",
            &[&format!("human:{}", f.host)]
        )
        .await?
        .get::<_, i64>(0),
        before
    );
    Ok(())
}

/// Deterministically reproduce the two prelocked actors a concurrent session facade can hold.
/// A deadlock victim must roll back both the outer room receipt and inner queue receipt.
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_command_facade_deadlock_rolls_back_and_original_intent_recovers_without_double_cancel()
-> TestResult {
    use super::{queue_intent_integrity as queue, room_start_integrity};
    use tokio_postgres::error::SqlState;
    let c = connection().await?;
    let f = queue::fixture(&c).await?;
    let mut entries = vec![];
    let actors = [f.room.source.actor, f.room.other];
    let revisions = [f.room.source.revision, f.room.other_revision];
    for (actor, revision) in actors.into_iter().zip(revisions) {
        let value = queue::enqueue(
            &c,
            actor,
            &queue::alternatives(f.profile, revision, "duel", "0"),
            Uuid::new_v4(),
        )
        .await?;
        assert_eq!(value["status"], "success");
        entries.push(
            value["data"]["resourceId"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()?,
        );
    }
    let selection = json!({"profileId":f.profile,"configurationKey":"duel","hostEntryId":entries[0],"humanSeats":[{"queueEntryId":entries[0],"seatIndex":"0"},{"queueEntryId":entries[1],"seatIndex":"1"}],"aiSeats":[]});
    let claimed: Value = c
        .query_one(
            "SELECT app.claim_matchmaking($1,$2)",
            &[&selection, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(claimed["status"], "success");
    let room: Uuid = claimed["data"]["resourceId"].as_str().unwrap().parse()?;
    let basis = room_start_integrity::basis(&c, room).await?;
    let mut identities = vec![];
    for actor in actors {
        let r=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&actor]).await?;
        identities.push((r.get::<_, Uuid>(0), r.get::<_, Uuid>(1), Uuid::new_v4()));
    }
    let left = connection().await?;
    let right = connection().await?;
    for (client, actor) in [(&left, actors[0]), (&right, actors[1])] {
        client
            .batch_execute("BEGIN; SET LOCAL ROLE service_role")
            .await?;
        // Test owner establishes precisely the actor lock taken by lock_session_account.
        client.batch_execute("RESET ROLE").await?;
        client
            .query_one(
                "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
                &[&actor],
            )
            .await?;
        client.batch_execute("SET LOCAL ROLE service_role").await?;
    }
    let run = |client: tokio_postgres::Client, identity: (Uuid, Uuid, Uuid), basis: String| async move {
        let result = client
            .query_one(
                "SELECT app.leave_room_command($1,$2,$3,$4,$5::text::numeric)",
                &[&identity.0, &identity.1, &identity.2, &room, &basis],
            )
            .await;
        client
            .batch_execute(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })
            .await
            .unwrap();
        result.map(|r| r.get::<_, Value>(0))
    };
    let (a, b) = tokio::join!(
        run(left, identities[0], basis.clone()),
        run(right, identities[1], basis.clone())
    );
    let results = [a, b];
    let mut victims = 0;
    for (i, result) in results.into_iter().enumerate() {
        let (subject, session, request) = identities[i];
        let intent = json!({"kind":"leave","command":{"requestId":request,"roomId":room,"expectedRevision":basis}});
        c.batch_execute("SET ROLE service_role").await?;
        match result {
            Ok(value) => {
                assert_eq!(value["status"], "success");
                assert_eq!(
                    lookup(&c, subject, session, &intent).await?["data"]["response"],
                    value
                );
            }
            Err(error) => {
                assert_eq!(error.code(), Some(&SqlState::T_R_DEADLOCK_DETECTED));
                victims += 1;
                assert_eq!(
                    lookup(&c, subject, session, &intent).await?["data"]["state"],
                    "unknown"
                );
                let retry: Value = c
                    .query_one(
                        "SELECT app.leave_room_command($1,$2,$3,$4,$5::text::numeric)",
                        &[&subject, &session, &request, &room, &basis],
                    )
                    .await?
                    .get(0);
                assert_eq!(retry["data"]["revision"], "conflict");
                assert_eq!(
                    lookup(&c, subject, session, &intent).await?["data"]["response"],
                    retry
                );
            }
        }
        c.batch_execute("RESET ROLE").await?;
    }
    assert_eq!(victims, 1);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.queue_entry WHERE claimed_room_id=$1 AND state='canceled'",&[&room]).await?.get::<_,i64>(0),2);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE principal_key IN($1,$2) AND state='pending'",&[&format!("human:{}",actors[0]),&format!("human:{}",actors[1])]).await?.get::<_,i64>(0),0);
    Ok(())
}
