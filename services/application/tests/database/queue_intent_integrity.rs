//! Typed queue selection SQL fixtures do not qualify native decks or AI provider execution.
use super::{TestResult, connection, room_start_integrity, wait_for_database_lock};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) struct Queue {
    pub(super) room: room_start_integrity::Room,
    pub(super) profile: Uuid,
}
pub(super) async fn fixture(c: &Client) -> FixtureResult<Queue> {
    let room = room_start_integrity::room(c, true).await?;
    let format: Uuid = c
        .query_one("SELECT format_id FROM app.room WHERE id=$1", &[&room.id])
        .await?
        .get(0);
    let config = json!({"version":"matchmaking-profile-v1","influence":"none","planechase":false,"qualificationRef":format!("sha256:{}", "1".repeat(64)),"configurations":[{"key":"duel","formatId":format,"seatCount":"2","structure":"bo1","teams":[null,null]},{"key":"team-duel","formatId":format,"seatCount":"2","structure":"bo3","teams":["a","b"]}]});
    let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref,matchmaking_config) VALUES($1,'fixture','fixture',2,2,'fixture',$2) RETURNING id", &[&Uuid::new_v4().to_string(),&config]).await?.get(0);
    Ok(Queue { room, profile })
}
pub(super) fn alternatives(profile: Uuid, revision: Uuid, key: &str, ai: &str) -> Value {
    json!([{"deckRevisionId":revision,"choices":[{"profileId":profile,"configurationKey":key,"maximumAiSeats":ai,"maximumSponsoredAiSeats":ai}]}])
}
pub(super) async fn enqueue(
    c: &Client,
    actor: Uuid,
    body: &Value,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.enqueue_matchmaking_intent($1,'collection',false,$2,$3)",
        &[&actor, &body, &request],
    )
    .await
    .map(|r| r.get(0))
}
fn resource(v: &Value) -> Uuid {
    assert_eq!(v["status"], "success", "{v}");
    v["data"]["resourceId"].as_str().unwrap().parse().unwrap()
}
fn selection(profile: Uuid, first: Uuid, second: Option<Uuid>) -> Value {
    let humans = if let Some(second) = second {
        json!([{"queueEntryId":first,"seatIndex":"0"},{"queueEntryId":second,"seatIndex":"1"}])
    } else {
        json!([{"queueEntryId":first,"seatIndex":"0"}])
    };
    json!({"profileId":profile,"configurationKey":"duel","hostEntryId":first,"humanSeats":humans,"aiSeats":if second.is_some(){json!([])}else{json!([{"sponsorEntryId":first,"seatIndex":"1"}])}})
}
async fn claim(c: &Client, body: &Value, request: Uuid) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c
        .query_one("SELECT app.claim_matchmaking($1,$2)", &[&body, &request])
        .await
        .map(|r| r.get(0))?;
    match spellwake_application::admission_ports::QueueSelection::from_bytes(
        &serde_json::to_vec(body).unwrap(),
    ) {
        Ok(selection) => {
            spellwake_application::admission_ports::MatchmakingClaim {
                request_id: request,
                selection,
            }
            .decode_response(&serde_json::to_vec(&response).unwrap())
            .unwrap();
        }
        Err(_) => assert_ne!(response["status"], "success"),
    }
    Ok(response)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn typed_intake_rejects_foreign_decks_and_claims_only_the_concrete_intersection() -> TestResult
{
    let c = connection().await?;
    let f = fixture(&c).await?;
    let a = alternatives(f.profile, f.room.source.revision, "duel", "0");
    let request = Uuid::new_v4();
    let accepted = enqueue(&c, f.room.source.actor, &a, request).await?;
    assert_eq!(
        enqueue(&c, f.room.source.actor, &a, request).await?,
        accepted
    );
    let first = resource(&accepted);
    assert_eq!(
        enqueue(&c, f.room.other, &a, Uuid::new_v4()).await?["status"],
        "fail"
    );
    let other = enqueue(
        &c,
        f.room.other,
        &alternatives(f.profile, f.room.other_revision, "team-duel", "0"),
        Uuid::new_v4(),
    )
    .await?;
    let disjoint = selection(f.profile, first, Some(resource(&other)));
    assert_eq!(
        claim(&c, &disjoint, Uuid::new_v4()).await?["data"]["preferences"],
        "selection outside an accepted intent"
    );
    let second = resource(
        &enqueue(
            &c,
            f.room.other,
            &alternatives(f.profile, f.room.other_revision, "duel", "0"),
            Uuid::new_v4(),
        )
        .await?,
    );
    let chosen = selection(f.profile, first, Some(second));
    let request = Uuid::new_v4();
    let reserved = claim(&c, &chosen, request).await?;
    let room = resource(&reserved);
    assert_eq!(claim(&c, &chosen, request).await?, reserved);
    assert_eq!(claim(&c, &chosen, Uuid::new_v4()).await?["status"], "fail");
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.queue_entry WHERE claimed_room_id=$1 AND state='claimed'", &[&room]).await?.get::<_,i64>(0),2);
    assert_eq!(
        c.execute("UPDATE app.room SET structure='bo3' WHERE id=$1", &[&room])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let result: Value = c
        .query_one(
            "SELECT app.cancel_matchmaking($1,$2,$3)",
            &[&f.room.source.actor, &first, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(result["status"], "success");
    assert_eq!(
        c.query_one("SELECT state FROM app.room WHERE id=$1", &[&room])
            .await?
            .get::<_, String>(0),
        "closed"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn claimed_human_room_starts_with_accepted_decks_and_queue_reservation_in_one_commit()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let first = resource(
        &enqueue(
            &c,
            f.room.source.actor,
            &alternatives(f.profile, f.room.source.revision, "duel", "0"),
            Uuid::new_v4(),
        )
        .await?,
    );
    let second = resource(
        &enqueue(
            &c,
            f.room.other,
            &alternatives(f.profile, f.room.other_revision, "duel", "0"),
            Uuid::new_v4(),
        )
        .await?,
    );
    let room = resource(
        &claim(
            &c,
            &selection(f.profile, first, Some(second)),
            Uuid::new_v4(),
        )
        .await?,
    );
    let basis = room_start_integrity::basis(&c, room).await?;
    let forced: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:qualified',$4)",
            &[&f.room.source.actor, &room, &basis, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(forced["status"], "success", "{forced}");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&room],
    )
    .await?;
    let inputs:Vec<Value>=c.query("SELECT id,deck_revision_id FROM app.room_slot WHERE room_id=$1 ORDER BY seat_index", &[&room]).await?.iter().map(|r|json!({"slotId":r.get::<_,Uuid>(0),"revisionId":r.get::<_,Uuid>(1),"snapshotRef":"fixture:source"})).collect();
    let basis = room_start_integrity::basis(&c, room).await?;
    let started: Value = c
        .query_one(
            "SELECT app.start_room_match($1,$2::text::numeric,$3,$4,'fixture:qualified',$5,$6)",
            &[
                &room,
                &basis,
                &f.room.source.release,
                &f.room.legality,
                &json!(inputs),
                &Uuid::new_v4(),
            ],
        )
        .await?
        .get(0);
    let m = resource(&started);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.queue_entry WHERE claimed_room_id=$1 AND reserved_match_id=$2 AND state='reserved'", &[&room,&m]).await?.get::<_,i64>(0),2);
    let canceled: Value = c
        .query_one(
            "SELECT app.cancel_matchmaking($1,$2,$3)",
            &[&f.room.source.actor, &first, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(canceled["status"], "fail");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn casual_ai_claim_requires_explicit_capacity_and_sponsorship_and_competing_claim_waits()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let first = resource(
        &enqueue(
            &c,
            f.room.source.actor,
            &alternatives(f.profile, f.room.source.revision, "duel", "1"),
            Uuid::new_v4(),
        )
        .await?,
    );
    let chosen = selection(f.profile, first, None);
    let request = Uuid::new_v4();
    c.batch_execute("BEGIN").await?;
    let accepted = claim(&c, &chosen, request).await?;
    let room = resource(&accepted);
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let task = tokio::spawn(async move { claim(&waiting, &chosen, Uuid::new_v4()).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??["status"], "fail");
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.room_slot WHERE room_id=$1 AND controller_kind='ai' AND sponsor_id=$2 AND NOT ready AND deck_revision_id IS NULL", &[&room,&f.room.source.actor]).await?.get::<_,i64>(0),1);
    let state: String = c
        .query_one("SELECT state FROM app.room WHERE id=$1", &[&room])
        .await?
        .get(0);
    assert_eq!(state, "lobby");
    let no_ai = resource(
        &enqueue(
            &c,
            f.room.other,
            &alternatives(f.profile, f.room.other_revision, "duel", "0"),
            Uuid::new_v4(),
        )
        .await?,
    );
    assert_eq!(
        claim(&c, &selection(f.profile, no_ai, None), Uuid::new_v4()).await?["status"],
        "fail"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn whole_intent_cancel_finds_canceled_sibling_and_claim_rollback_releases_every_alternative()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let deck: Uuid = c
        .query_one(
            "SELECT deck_id FROM app.deck_revision WHERE id=$1",
            &[&f.room.source.revision],
        )
        .await?
        .get(0);
    let saved:Value=c.query_one("SELECT app.save_deck_contents($1,$2,$3,$4,$5)", &[&f.room.source.actor,&deck,&f.room.source.revision,&json!([{"definitionId":f.room.source.definition,"printingId":f.room.source.printing,"section":"main","quantity":"1"}]),&Uuid::new_v4()]).await?.get(0);
    let second_revision = resource(&saved);
    let mut body = alternatives(f.profile, f.room.source.revision, "duel", "1");
    body.as_array_mut()
        .unwrap()
        .push(alternatives(f.profile, second_revision, "duel", "1")[0].clone());
    let accepted = enqueue(&c, f.room.source.actor, &body, Uuid::new_v4()).await?;
    let first = resource(&accepted);
    let intent: Uuid = accepted["data"]["intentId"].as_str().unwrap().parse()?;
    let sibling: Uuid = c
        .query_one(
            "SELECT id FROM app.queue_entry WHERE intent_receipt_id=$1 AND id<>$2",
            &[&intent, &first],
        )
        .await?
        .get(0);
    let chosen = selection(f.profile, first, None);
    let request = Uuid::new_v4();
    c.batch_execute("BEGIN").await?;
    let abandoned = resource(&claim(&c, &chosen, request).await?);
    c.batch_execute("ROLLBACK").await?;
    assert!(
        c.query_opt("SELECT id FROM app.room WHERE id=$1", &[&abandoned])
            .await?
            .is_none()
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.queue_entry WHERE intent_receipt_id=$1 AND state='queued'", &[&intent]).await?.get::<_,i64>(0),2);
    let room = resource(&claim(&c, &chosen, request).await?);
    assert_eq!(
        c.query_one("SELECT state FROM app.queue_entry WHERE id=$1", &[&sibling])
            .await?
            .get::<_, String>(0),
        "canceled"
    );
    let canceled: Value = c
        .query_one(
            "SELECT app.cancel_matchmaking($1,$2,$3)",
            &[&f.room.other, &sibling, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(canceled["status"], "fail");
    assert_eq!(
        super::deletion::request(&c, f.room.source.actor, Uuid::new_v4()).await?["status"],
        "success"
    );
    let work: Value = c
        .query_one(
            "SELECT app.read_deletion_work($1,100)",
            &[&f.room.source.actor],
        )
        .await?
        .get(0);
    let typed: Vec<spellwake_application::deletion::CleanupWork> = serde_json::from_value(work)?;
    spellwake_application::deletion::validate_cleanup_work(&typed, 100)?;
    let original = typed
        .iter()
        .find(|item| item.kind == spellwake_application::deletion::CleanupKind::Queue)
        .unwrap();
    let cleaned: Value = c
        .query_one(
            "SELECT app.cancel_deletion_work($1,'queue',$2)",
            &[&f.room.source.actor, &original.id],
        )
        .await?
        .get(0);
    let parsed: spellwake_application::ApiResponse<spellwake_application::deletion::CleanupAck> =
        spellwake_application::decode_wire_object(&serde_json::to_vec(&cleaned)?)?;
    assert!(
        matches!(parsed,spellwake_application::ApiResponse::Success{data} if data.matches(original))
    );
    let canceled: Value = c
        .query_one(
            "SELECT app.cancel_matchmaking($1,$2,$3)",
            &[&f.room.source.actor, &sibling, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(canceled["status"], "success");
    assert_eq!(
        c.query_one("SELECT state FROM app.room WHERE id=$1", &[&room])
            .await?
            .get::<_, String>(0),
        "closed"
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.queue_entry WHERE intent_receipt_id=$1 AND state='canceled'", &[&intent]).await?.get::<_,i64>(0),2);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn either_human_can_leave_claimed_room_without_erasing_its_intent_history() -> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    for actor in [f.room.source.actor, f.room.other] {
        let first = resource(
            &enqueue(
                &c,
                f.room.source.actor,
                &alternatives(f.profile, f.room.source.revision, "duel", "0"),
                Uuid::new_v4(),
            )
            .await?,
        );
        let second = resource(
            &enqueue(
                &c,
                f.room.other,
                &alternatives(f.profile, f.room.other_revision, "duel", "0"),
                Uuid::new_v4(),
            )
            .await?,
        );
        let room = resource(
            &claim(
                &c,
                &selection(f.profile, first, Some(second)),
                Uuid::new_v4(),
            )
            .await?,
        );
        let stale: Value = c
            .query_one(
                "SELECT app.leave_room($1,$2,999,$3)",
                &[&actor, &room, &Uuid::new_v4()],
            )
            .await?
            .get(0);
        assert_eq!(stale["data"]["revision"], "conflict");
        let basis = room_start_integrity::basis(&c, room).await?;
        let request = Uuid::new_v4();
        let left: Value = c
            .query_one(
                "SELECT app.leave_room($1,$2,$3::text::numeric,$4)",
                &[&actor, &room, &basis, &request],
            )
            .await?
            .get(0);
        assert_eq!(left["status"], "success", "{left}");
        assert_eq!(
            c.query_one(
                "SELECT app.leave_room($1,$2,$3::text::numeric,$4)",
                &[&actor, &room, &basis, &request]
            )
            .await?
            .get::<_, Value>(0),
            left
        );
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.queue_entry WHERE claimed_room_id=$1 AND state='canceled'", &[&room]).await?.get::<_,i64>(0),2);
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn profile_rejects_missing_planar_pool_and_malformed_influence_before_queue_intake()
-> TestResult {
    let c = connection().await?;
    let f = fixture(&c).await?;
    let config: Value = c
        .query_one(
            "SELECT matchmaking_config FROM app.table_profile_revision WHERE id=$1",
            &[&f.profile],
        )
        .await?
        .get(0);
    let mut invalid = config;
    invalid["planechase"] = Value::Bool(true);
    assert_eq!(c.execute("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref,matchmaking_config) VALUES($1,'fixture','fixture',2,2,'fixture',$2)", &[&Uuid::new_v4().to_string(),&invalid]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    invalid["planechase"] = Value::Bool(false);
    invalid["influence"] = Value::String("distance:0".to_owned());
    assert_eq!(c.execute("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref,matchmaking_config) VALUES($1,'fixture','fixture',2,2,'fixture',$2)", &[&Uuid::new_v4().to_string(),&invalid]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));

    Ok(())
}
