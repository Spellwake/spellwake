//! Populated SQL native-boundary checks. References here satisfy namespace grammar;
//! no retained bytes, engine legality, full cards or production activation are qualified.
use super::{
    TestResult, ante_transaction_integrity as ante, connection, queue_intent_integrity as queue,
    room_start_integrity as rooms,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const OWNER: &str = "fixture:native-boundary";

/// Extract only an existing function body for a transaction-local upgrade control.
/// No historical migration DDL or ACL is replayed over a live/shared database.
fn historical_function<'a>(migration: &'a str, name: &str) -> &'a str {
    let signature = format!("FUNCTION app.{name}(");
    let function = migration.find(&signature).unwrap();
    let start = migration[..function].rfind("CREATE").unwrap();
    let end = function + migration[function..].find("END $$;").unwrap() + "END $$;".len();
    &migration[start..end]
}

#[path = "native_foundation/bank_history.rs"]
mod bank_history;
#[path = "native_foundation/clock_opening.rs"]
mod clock_opening;
#[path = "native_foundation/presentation.rs"]
mod presentation;
#[path = "native_foundation/races.rs"]
mod races;
#[path = "native_foundation/response_deadline.rs"]
mod response_deadline;
#[path = "native_foundation/transition_sources.rs"]
mod transition_sources;
struct Native {
    matched: Uuid,
    game: Uuid,
    context: Uuid,
    seat: Uuid,
    subject: Uuid,
    session: Uuid,
    original_basis: String,
    opening: Value,
}
fn artifact(f: &Native, role: &str, value: u64) -> String {
    format!(
        "native-artifact-v1/{}/{}/{role}/1/{value:064x}.json",
        f.game, f.context
    )
}
fn basis(f: &Native, revision: &str) -> Value {
    json!({"gameId":f.game,"contextId":f.context,"epoch":"0","revision":revision})
}
async fn native(c: &Client, structure: &str) -> FixtureResult<Native> {
    native_with_roster(c, structure, true).await
}
async fn native_with_roster(c: &Client, structure: &str, humans: bool) -> FixtureResult<Native> {
    let f = reserved_native(c, structure, humans).await?;
    let opened = accept_opening_fixture(c, &f, &f.opening).await?;
    assert_eq!(opened["status"], "success", "{opened}");
    Ok(f)
}
async fn reserved_native(c: &Client, structure: &str, humans: bool) -> FixtureResult<Native> {
    let (room, matched) = ante::reserve_match(c, humans, structure).await?;
    reserved_native_match(c, structure, &room, matched).await
}
/// Share native SQL opening metadata with an already configured canonical lobby.
async fn reserved_native_match(
    c: &Client,
    structure: &str,
    room: &rooms::Room,
    matched: Uuid,
) -> FixtureResult<Native> {
    rooms::confirm_adopted_ai(c, matched).await?;
    let row = c
        .query_one(
            "SELECT reserved_game_id,revision::text FROM app.match WHERE id=$1",
            &[&matched],
        )
        .await?;
    let auth = c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1", &[&room.source.actor]).await?;
    let seat = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&matched, &room.source.actor],
        )
        .await?
        .get(0);
    let mut f = Native {
        matched,
        game: row.get(0),
        context: Uuid::new_v4(),
        seat,
        subject: auth.get(0),
        session: auth.get(1),
        original_basis: row.get(1),
        opening: Value::Null,
    };
    f.opening = json!({"contextId":f.context,"snapshotRef":artifact(&f,"initial",1),"contextRevision":"fixture:native-boundary-v1","stateRef":artifact(&f,"state",2),"factsRef":artifact(&f,"facts",3),"eventRef":artifact(&f,"event",4),"qualificationRef":"fixture:SQL-metadata-only","eventSchemaRevision":"native-transition-v1"});
    if structure == "bo3" {
        f.opening["clockObservation"] = json!({"reason":"native_decision","seatId":f.seat,"acceptedRevision":"0","proofRef":f.opening["stateRef"]});
    }
    Ok(f)
}
async fn accept_opening_fixture(c: &Client, f: &Native, opening: &Value) -> FixtureResult<Value> {
    Ok(c.query_one(
        "SELECT app.accept_native_opening($1,$2::text::numeric,$3,$4,$5,'0',$6,$7)",
        &[
            &f.matched,
            &f.original_basis,
            &f.game,
            &OWNER,
            opening,
            &"1".repeat(64),
            &"2".repeat(64),
        ],
    )
    .await?
    .get(0))
}
async fn candidate(c: &Client, f: &Native) -> FixtureResult<Value> {
    let revision: String = c
        .query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game],
        )
        .await?
        .get(0);
    let rows=c.query("SELECT s.id,c.clock_revision::text,c.remaining_ms::text,c.policy_ref,c.paused FROM app.match_seat s JOIN app.match_clock c ON c.match_seat_id=s.id WHERE s.match_id=$1 ORDER BY s.id", &[&f.matched]).await?;
    let clocks:Vec<Value>=rows.iter().map(|r| {
        let mut clock=json!({"seatId":r.get::<_,Uuid>(0),"clockRevision":r.get::<_,String>(1),"remainingMs":r.get::<_,String>(2),"paused":true});
        if r.get::<_,String>(3)=="bo3-overall-bank-v1" && !r.get::<_,bool>(4) {clock["eligibility"]=json!({"reason":"native_wait","acceptedRevision":revision,"proofRef":"fixture:native-wait"});}
        clock
    }).collect();
    Ok(
        json!({"contextId":f.context,"stateRef":artifact(f,"state",10),"factsRef":artifact(f,"facts",11),"events":[artifact(f,"event",12),artifact(f,"event",13)],"facts":[],"clocks":clocks,"participation":[],"eventSchemaRevision":"native-transition-v1"}),
    )
}
async fn view(c: &Client, f: &Native, revision: &str, salt: u64) -> FixtureResult<Value> {
    view_for(c, f, revision, salt, f.seat).await
}
async fn view_for(
    c: &Client,
    f: &Native,
    revision: &str,
    salt: u64,
    viewer: Uuid,
) -> FixtureResult<Value> {
    let response: Value = c
        .query_one(
            "SELECT app.publish_native_view_source($1,$2,'0',$3,$4,'live',$5)",
            &[
                &f.game,
                &f.context,
                &revision,
                &viewer,
                &artifact(f, "view", salt),
            ],
        )
        .await?
        .get(0);
    assert_eq!(response["status"], "success", "{response}");
    Ok(response["data"].clone())
}
async fn host_advance(c: &Client, f: &Native) -> FixtureResult<Value> {
    let revision: String = c
        .query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game],
        )
        .await?
        .get(0);
    let original = json!({"version":"native-host-input-v1","basis":basis(f,&revision),"kind":"entropy","inputRef":artifact(f,"input",20),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
    let prepared = json!({"owner":OWNER,"fenceEpoch":"1","candidate":candidate(c,f).await?});
    let response: Value = c
        .query_one(
            "SELECT app.commit_native_host_input($1,$2,$3,$4)",
            &[&f.game, &Uuid::new_v4(), &original, &prepared],
        )
        .await?
        .get(0);
    assert_eq!(response["status"], "success", "{response}");
    Ok(response)
}
async fn submit(
    c: &Client,
    f: &Native,
    original: &Value,
    prepared: Option<&Value>,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.submit_native_human_command($1,$2,$3,$4)",
        &[&f.subject, &f.session, &original, &prepared],
    )
    .await
    .map(|r| r.get(0))
}
async fn finish_opening_game_as_draw(c: &Client, f: &Native) -> TestResult {
    let structure: String = c
        .query_one("SELECT structure FROM app.match WHERE id=$1", &[&f.matched])
        .await?
        .get(0);
    assert!(matches!(structure.as_str(), "bo1" | "bo3"));
    let mut body = candidate(c, f).await?;
    for clock in body["clocks"].as_array_mut().unwrap() {
        clock.as_object_mut().unwrap().remove("eligibility");
    }
    body["terminal"] = json!({"outcome":"draw","winnerSeatIds":[],"winnersRef":"fixture:SQL-draw","reason":"fixture"});
    let seats=c.query("SELECT id FROM app.match_seat WHERE match_id=$1 AND controller_kind='human' ORDER BY id", &[&f.matched]).await?;
    body["participation"] = json!(
        seats
            .iter()
            .map(|r| json!({"seatId":r.get::<_,Uuid>(0),"ownTurns":"3","reason":"fixture"}))
            .collect::<Vec<_>>()
    );
    let original = json!({"version":"native-host-input-v1","basis":basis(f,"0"),"kind":"native-work","inputRef":artifact(f,"input",80),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
    let accepted: Value = c
        .query_one(
            "SELECT app.commit_native_host_input($1,$2,$3,$4)",
            &[
                &f.game,
                &Uuid::new_v4(),
                &original,
                &json!({"owner":OWNER,"fenceEpoch":"1","candidate":body}),
            ],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    let result: Uuid = c
        .query_one(
            "SELECT id FROM app.game_result WHERE game_id=$1",
            &[&f.game],
        )
        .await?
        .get(0);
    let match_basis: String = c
        .query_one(
            "SELECT revision::text FROM app.match WHERE id=$1",
            &[&f.matched],
        )
        .await?
        .get(0);
    let consumed: Value = c
        .query_one(
            "SELECT app.consume_game_outcome($1,$2::text::numeric)",
            &[&result, &match_basis],
        )
        .await?
        .get(0);
    assert_eq!(
        consumed["data"]["phase"],
        if structure == "bo1" {
            "terminal"
        } else {
            "between_games"
        }
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn opening_provenance_survives_progress_and_never_uses_host_fence_as_epoch() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    c.batch_execute("SET ROLE service_role").await?;
    host_advance(&c, &f).await?;
    let source: Value = c
        .query_one("SELECT app.read_native_opening_source($1)", &[&f.game])
        .await?
        .get(0);
    assert_eq!(source["data"]["nativeEpoch"], "0");
    assert_eq!(source["data"]["opening"], f.opening);
    let duplicate: Value = c
        .query_one(
            "SELECT app.accept_native_opening($1,$2::text::numeric,$3,$4,$5,'0',$6,$7)",
            &[
                &f.matched,
                &f.original_basis,
                &f.game,
                &OWNER,
                &f.opening,
                &"1".repeat(64),
                &"2".repeat(64),
            ],
        )
        .await?
        .get(0);
    assert_eq!(duplicate["status"], "success");
    let access: Value = c
        .query_one(
            "SELECT app.read_native_human_access($1,$2,$3,$4)",
            &[&f.subject, &f.session, &f.game, &f.context],
        )
        .await?
        .get(0);
    assert_eq!(access["data"]["actor"]["seatId"], json!(f.seat));
    assert_eq!(access["data"]["source"]["nativeEpoch"], "0");
    assert_eq!(access["data"]["source"]["nativeRevision"], "1");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn first_view_wins_and_raw_split_frame_cursors_keep_their_issued_ceiling() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    c.batch_execute("SET ROLE service_role").await?;
    let first = view(&c, &f, "0", 30).await?;
    assert_eq!(view(&c, &f, "0", 31).await?, first);
    let first_id: Uuid = first["receiptId"].as_str().unwrap().parse()?;
    let opening: Value = c
        .query_one(
            "SELECT app.read_native_archive_source($1,$2,$3,NULL,1,'live',$4)",
            &[&f.game, &f.context, &f.seat, &first_id],
        )
        .await?
        .get(0);
    assert_eq!(opening["items"][0]["sequence"], "0");
    assert_eq!(opening["scannedThrough"], "0");
    host_advance(&c, &f).await?;
    let newer = view(&c, &f, "1", 32).await?;
    let newer_id: Uuid = newer["receiptId"].as_str().unwrap().parse()?;
    let scan: Value = c
        .query_one(
            "SELECT app.read_native_archive_source($1,$2,$3,NULL,2,'live',$4)",
            &[&f.game, &f.context, &f.seat, &newer_id],
        )
        .await?
        .get(0);
    assert_eq!(scan["nextAfter"], "1");
    assert_eq!(scan["scannedThrough"], "1");
    let cursor: Value = c
        .query_one(
            "SELECT app.issue_native_cursor_source($1,$2,'0','1',$3,1,'history',$4,$5)",
            &[&f.game, &f.context, &f.seat, &newer_id, &scan],
        )
        .await?
        .get(0);
    assert_eq!(cursor["data"]["basis"]["revision"], "1");
    assert_eq!(cursor["data"]["throughEventSequence"], "1");
    let token = cursor["data"]["token"].as_str().unwrap();
    host_advance(&c, &f).await?;
    let retained: Value = c
        .query_one(
            "SELECT app.read_native_cursor_source($1,$2,$3)",
            &[&f.game, &f.seat, &token],
        )
        .await?
        .get(0);
    assert_eq!(retained, cursor);
    let forged: Value = c
        .query_one(
            "SELECT app.read_native_cursor_source($1,$2,'n1.00000000-0000-4000-8000-000000000001')",
            &[&f.game, &f.seat],
        )
        .await?
        .get(0);
    assert_eq!(forged["data"]["cursor"], "unavailable");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn player_original_replays_before_new_preparation_and_changed_terms_conflict() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    c.batch_execute("SET ROLE service_role").await?;
    let issued = view(&c, &f, "0", 40).await?;
    let original =
        json!({"requestId":Uuid::new_v4(),"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let prepared = json!({"version":"native-player-accept-v1","basis":basis(&f,"0"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",41),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    let accepted = submit(&c, &f, &original, Some(&prepared)).await?;
    assert_eq!(accepted["status"], "success", "{accepted}");
    assert_eq!(accepted["data"]["revision"], "1");
    host_advance(&c, &f).await?;
    assert_eq!(submit(&c, &f, &original, None).await?, accepted);
    let mut changed = original.clone();
    changed["action"] = json!({"kind":"concede"});
    assert_eq!(
        submit(&c, &f, &changed, None).await?["data"]["request"],
        "conflict"
    );
    let query = json!({"gameId":f.game,"contextId":f.context,"outstanding":[original.clone()]});
    let recovered: Value = c
        .query_one(
            "SELECT app.read_native_human_receipts($1,$2,$3)",
            &[&f.subject, &f.session, &query],
        )
        .await?
        .get(0);
    assert_eq!(recovered["data"]["receipts"][0]["response"], accepted);
    assert_eq!(recovered["data"]["receipts"][0]["original"], original);
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "2"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stale_original_has_no_child_and_operational_errors_do_not_become_player_blame()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    c.batch_execute("SET ROLE service_role").await?;
    host_advance(&c, &f).await?;
    let original =
        json!({"requestId":Uuid::new_v4(),"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let response = submit(&c, &f, &original, None).await?;
    assert_eq!(response, json!({"status":"fail","data":{"basis":"stale"}}));
    let parent:Uuid=c.query_one("SELECT id FROM app.command_receipt WHERE scope='native_player_v1' AND principal_key=$1 AND request_ref=$2", &[&format!("native-seat:{}",f.seat),&original["requestId"].as_str().unwrap()]).await?.get(0);
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='host_candidate_v1' AND request_ref=$1", &[&parent.to_string()]).await?.get::<_,i64>(0),0);
    let issued = view(&c, &f, "1", 50).await?;
    let current =
        json!({"requestId":Uuid::new_v4(),"basis":basis(&f,"1"),"action":{"kind":"pass"}});
    let bad = json!({"version":"native-player-accept-v1","basis":basis(&f,"1"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",51),"owner":"fixture:lost-host","fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    assert_eq!(
        submit(&c, &f, &current, Some(&bad))
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND request_ref=$1", &[&current["requestId"].as_str().unwrap()]).await?.get::<_,i64>(0),0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn individual_ready_does_not_require_an_opponents_selection() -> TestResult {
    let c = connection().await?;
    let f = rooms::room(&c, true).await?;
    let b = rooms::basis(&c, f.id).await?;
    c.query_one(
        "SELECT app.select_room_deck($1,$2,$3::text::numeric,NULL,$4)",
        &[&f.other, &f.id, &b, &Uuid::new_v4()],
    )
    .await?;
    let b = rooms::basis(&c, f.id).await?;
    let auth=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1", &[&f.source.actor]).await?;
    let subject: Uuid = auth.get(0);
    let session: Uuid = auth.get(1);
    c.batch_execute("SET ROLE service_role").await?;
    let source: Value = c
        .query_one(
            "SELECT app.read_native_room_source($1,$2::text::numeric,$3,$4)",
            &[&f.id, &b, &f.source.release, &f.legality],
        )
        .await?
        .get(0);
    assert_eq!(source["status"], "success");
    assert!(
        source["data"]["terms"]["slots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["sourceStatus"] == "unselected" && x["entries"].is_null())
    );
    let original = json!({"requestId":Uuid::new_v4(),"expectedRevision":b});
    let prepared = json!({"version":"native-ready-prepared-v1","sourceHash":source["data"]["sourceHash"],"releaseId":f.source.release,"legalityRevisionId":f.legality,"selectedRevisionId":f.source.revision,"qualificationRef":"fixture:Ready-SQL-only"});
    let accepted: Value = c
        .query_one(
            "SELECT app.qualify_room_ready_command($1,$2,$3,$4,$5)",
            &[&subject, &session, &f.id, &original, &prepared],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success", "{accepted}");
    let duplicate: Value = c
        .query_one(
            "SELECT app.qualify_room_ready_command($1,$2,$3,$4,NULL)",
            &[&subject, &session, &f.id, &original],
        )
        .await?
        .get(0);
    assert_eq!(duplicate, accepted);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn expanded_queue_source_is_lossless_even_when_one_legal_choice_exceeds_a_megabyte()
-> TestResult {
    let c = connection().await?;
    let f = queue::fixture(&c).await?;
    let format: Uuid = c
        .query_one("SELECT format_id FROM app.room WHERE id=$1", &[&f.room.id])
        .await?
        .get(0);
    let padded = format!("{}a{}", " ".repeat(700_000), " ".repeat(700_000));
    let configuration = json!({"version":"matchmaking-profile-v1","influence":"none","planechase":false,"qualificationRef":format!("sha256:{}","1".repeat(64)),"configurations":[{"key":"large","formatId":format,"seatCount":"2","structure":"bo1","teams":[padded,null]}]});
    let profile:Uuid=c.query_one("INSERT INTO app.table_profile_revision(key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref,matchmaking_config) VALUES($1,'fixture','fixture',2,2,'fixture',$2) RETURNING id", &[&Uuid::new_v4().to_string(),&configuration]).await?.get(0);
    let original = queue::alternatives(profile, f.room.source.revision, "large", "0");
    let accepted = queue::enqueue(&c, f.room.source.actor, &original, Uuid::new_v4()).await?;
    assert_eq!(accepted["status"], "success");
    let intent: Uuid = accepted["data"]["intentId"].as_str().unwrap().parse()?;
    let expected: String = c
        .query_one(
            "SELECT normalized_source::text FROM app.command_receipt WHERE id=$1",
            &[&intent],
        )
        .await?
        .get(0);
    assert!(expected.len() > 1_048_576);
    assert!(serde_json::to_vec(&original)?.len() < 262_144);
    c.batch_execute("SET ROLE service_role").await?;
    let mut after: Option<String> = None;
    let mut bytes = Vec::new();
    let mut pages = 0;
    loop {
        let page: Value = c
            .query_one(
                "SELECT app.read_native_queue_intent_source($1,$2::text::numeric,8)",
                &[&intent, &after],
            )
            .await?
            .get(0);
        assert!(serde_json::to_vec(&page)?.len() <= 1_048_576);
        assert_eq!(
            page["sourceHash"],
            Sha256::digest(expected.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        for item in page["items"].as_array().unwrap() {
            assert_eq!(
                item["offset"].as_str().unwrap().parse::<usize>()?,
                bytes.len()
            );
            let chunk = STANDARD.decode(item["base64"].as_str().unwrap())?;
            assert_eq!(
                chunk.len(),
                item["byteLength"].as_str().unwrap().parse::<usize>()?
            );
            bytes.extend(chunk);
        }
        pages += 1;
        after = page["nextOffset"].as_str().map(str::to_owned);
        if after.is_none() {
            break;
        }
    }
    assert!(pages > 1);
    assert!(bytes == expected.as_bytes());
    assert!(serde_json::from_slice::<Value>(&bytes).is_ok());
    c.batch_execute("BEGIN; SET LOCAL ROLE authenticated")
        .await?;
    assert_eq!(
        c.query_one(
            "SELECT app.read_native_queue_intent_source($1,NULL,1)",
            &[&intent]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn bo3_wait_eligibility_is_required_and_invalid_candidate_rolls_back_original() -> TestResult
{
    let c = connection().await?;
    let f = native(&c, "bo3").await?;
    c.batch_execute("SET ROLE service_role").await?;
    let mut priority = candidate(&c, &f).await?;
    let first_clock = &mut priority["clocks"].as_array_mut().unwrap()[0];
    first_clock["paused"] = json!(false);
    first_clock["eligibility"] =
        json!({"reason":"native_priority","acceptedRevision":"0","proofRef":"fixture:priority"});
    let priority_original = json!({"version":"native-host-input-v1","basis":basis(&f,"0"),"kind":"clock","inputRef":artifact(&f,"input",61),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
    let priority_response: Value = c
        .query_one(
            "SELECT app.commit_native_host_input($1,$2,$3,$4)",
            &[
                &f.game,
                &Uuid::new_v4(),
                &priority_original,
                &json!({"owner":OWNER,"fenceEpoch":"1","candidate":priority}),
            ],
        )
        .await?
        .get(0);
    assert_eq!(priority_response["data"]["revision"], "1");
    let original = json!({"version":"native-host-input-v1","basis":basis(&f,"1"),"kind":"clock","inputRef":artifact(&f,"input",60),"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64)});
    let mut body = candidate(&c, &f).await?;
    for clock in body["clocks"].as_array_mut().unwrap() {
        clock["eligibility"] = Value::Null;
    }
    let prepared = json!({"owner":OWNER,"fenceEpoch":"1","candidate":body});
    let request = Uuid::new_v4();
    assert_eq!(
        c.query_one(
            "SELECT app.commit_native_host_input($1,$2,$3,$4)",
            &[&f.game, &request, &original, &prepared]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='native_host_input_v1' AND request_ref=$1", &[&request.to_string()]).await?.get::<_,i64>(0),0);
    assert_eq!(host_advance(&c, &f).await?["data"]["revision"], "2");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn native_rejection_and_unknown_recovery_never_advance_state_or_invent_input() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    c.batch_execute("SET ROLE service_role").await?;
    let issued = view(&c, &f, "0", 70).await?;
    let original =
        json!({"requestId":Uuid::new_v4(),"basis":basis(&f,"0"),"action":{"kind":"concede"}});
    let mut prepared = json!({"version":"native-player-reject-v1","basis":basis(&f,"0"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"errors":{"action":"unsupported"}});
    let rejected = submit(&c, &f, &original, Some(&prepared)).await?;
    assert_eq!(
        rejected,
        json!({"status":"fail","data":{"action":"unsupported"}})
    );
    prepared["errors"] = json!({"target":"invalid"});
    assert_eq!(submit(&c, &f, &original, Some(&prepared)).await?, rejected);
    let mut unknown = original.clone();
    unknown["requestId"] = json!(Uuid::new_v4());
    let query =
        json!({"gameId":f.game,"contextId":f.context,"outstanding":[unknown,original.clone()]});
    let response: Value = c
        .query_one(
            "SELECT app.read_native_human_receipts($1,$2,$3)",
            &[&f.subject, &f.session, &query],
        )
        .await?
        .get(0);
    assert_eq!(response["data"]["receipts"][0]["state"], "unknown");
    assert_eq!(response["data"]["receipts"][1]["response"], rejected);
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    let invalid_session: Value = c
        .query_one(
            "SELECT app.submit_native_human_command($1,$2,$3,NULL)",
            &[&f.subject, &Uuid::new_v4(), &original],
        )
        .await?
        .get(0);
    assert_eq!(invalid_session["data"]["session"], "unauthorized");
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='host_candidate_v1' AND target_ref=$1::uuid::text", &[&f.game]).await?.get::<_,i64>(0),0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unarmed_round_is_discoverable_and_own_qualified_reply_recovers_after_revision_advances()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo3").await?;
    c.batch_execute("SET ROLE service_role").await?;
    finish_opening_game_as_draw(&c, &f).await?;
    let own=c.query_one("SELECT d.id,m.revision::text,s.source_revision_id FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id JOIN app.match_seat s ON s.id=d.seat_id WHERE m.id=$1 AND s.id=$2 AND d.state='pending'", &[&f.matched,&f.seat]).await?;
    let decision: Uuid = own.get(0);
    let match_basis: String = own.get(1);
    let selected: Uuid = own.get(2);
    let mut after: Option<Uuid> = None;
    let mut found = false;
    loop {
        let page: Value = c
            .query_one(
                "SELECT app.read_native_unscheduled_decision_work($1,100)",
                &[&after],
            )
            .await?
            .get(0);
        found |= page["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == json!(decision));
        after = page["nextAfter"].as_str().map(str::parse).transpose()?;
        if after.is_none() {
            break;
        }
    }
    assert!(found);
    let source: Value = c
        .query_one(
            "SELECT app.read_native_between_source($1,$2::text::numeric,$3)",
            &[&decision, &match_basis, &selected],
        )
        .await?
        .get(0);
    assert_eq!(source["status"], "success");
    let intent = json!({"requestId":Uuid::new_v4(),"decisionId":decision,"expectedRevision":match_basis,"selectedRevisionId":selected});
    let prepared = json!({"version":"native-between-prepared-v1","sourceHash":source["data"]["sourceHash"],"replyRef":"fixture:reply","qualificationRef":"fixture:SQL-only"});
    let response: Value = c
        .query_one(
            "SELECT app.qualify_between_game_command($1,$2,$3,$4)",
            &[&f.subject, &f.session, &intent, &prepared],
        )
        .await?
        .get(0);
    assert_eq!(response["status"], "success", "{response}");
    assert!(response["data"].get("qualificationRef").is_none());
    let recovered: Value = c
        .query_one(
            "SELECT app.qualify_between_game_command($1,$2,$3,NULL)",
            &[&f.subject, &f.session, &intent],
        )
        .await?
        .get(0);
    assert_eq!(recovered, response);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn advancement_while_waiting_rolls_back_preclaimed_child_before_persisting_stale()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let issued = view(&c, &f, "0", 90).await?;
    let request = Uuid::new_v4();
    let original = json!({"requestId":request,"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let prepared = json!({"version":"native-player-accept-v1","basis":basis(&f,"0"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",91),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.match WHERE id=$1 FOR NO KEY UPDATE",
        &[&f.matched],
    )
    .await?;
    let waiting = connection().await?;
    waiting.batch_execute("SET ROLE service_role").await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let subject = f.subject;
    let session = f.session;
    let task = tokio::spawn(async move {
        waiting
            .query_one(
                "SELECT app.submit_native_human_command($1,$2,$3,$4)",
                &[&subject, &session, &original, &prepared],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    super::wait_for_database_lock(&c, pid).await?;
    host_advance(&c, &f).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??,
        json!({"status":"fail","data":{"basis":"stale"}})
    );
    let row=c.query_one("SELECT id,normalized_source FROM app.command_receipt WHERE scope='native_player_v1' AND request_ref=$1", &[&request.to_string()]).await?;
    let parent: Uuid = row.get(0);
    let source: Value = row.get(1);
    assert!(source["prepared"].is_null());
    assert!(source["childReceiptId"].is_null());
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='host_candidate_v1' AND request_ref=$1", &[&parent.to_string()]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "1"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn session_revoked_during_match_wait_rolls_back_original_child_and_frame() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let issued = view(&c, &f, "0", 100).await?;
    let request = Uuid::new_v4();
    let original = json!({"requestId":request,"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let prepared = json!({"version":"native-player-accept-v1","basis":basis(&f,"0"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",101),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.match WHERE id=$1 FOR NO KEY UPDATE",
        &[&f.matched],
    )
    .await?;
    let waiting = connection().await?;
    waiting.batch_execute("SET ROLE service_role").await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let subject = f.subject;
    let session = f.session;
    let task = tokio::spawn(async move {
        waiting
            .query_one(
                "SELECT app.submit_native_human_command($1,$2,$3,$4)",
                &[&subject, &session, &original, &prepared],
            )
            .await
            .map(|r| r.get::<_, Value>(0))
    });
    super::wait_for_database_lock(&c, pid).await?;
    assert_eq!(
        c.execute(
            "DELETE FROM auth.mfa_amr_claims WHERE session_id=$1",
            &[&f.session]
        )
        .await?,
        1
    );
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await??,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND request_ref=$1", &[&request.to_string()]).await?.get::<_,i64>(0),0);
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='host_candidate_v1' AND target_ref=$1::uuid::text", &[&f.game]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unissued_view_and_wrong_actor_preparations_have_no_durable_side_effects() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let issued = view(&c, &f, "0", 110).await?;
    let original =
        json!({"requestId":Uuid::new_v4(),"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let valid = json!({"version":"native-player-accept-v1","basis":basis(&f,"0"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",111),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    c.batch_execute("SET ROLE service_role").await?;
    let mut unissued = valid.clone();
    unissued["viewReceiptId"] = json!(Uuid::new_v4());
    assert_eq!(
        submit(&c, &f, &original, Some(&unissued))
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let mut wrong_actor = valid;
    wrong_actor["actorSeatId"] = json!(Uuid::new_v4());
    assert_eq!(
        submit(&c, &f, &original, Some(&wrong_actor))
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope IN('native_player_v1','host_candidate_v1') AND target_ref=$1::uuid::text", &[&f.game]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn late_protocol_constraint_failure_rolls_back_accepted_frame_and_both_originals()
-> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    let issued = view(&c, &f, "0", 120).await?;
    let request = Uuid::new_v4();
    let original = json!({"requestId":request,"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let prepared = json!({"version":"native-player-accept-v1","basis":basis(&f,"0"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",121),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    c.batch_execute("BEGIN; SET LOCAL ROLE service_role")
        .await?;
    assert_eq!(
        submit(&c, &f, &original, Some(&prepared)).await?["status"],
        "success"
    );
    c.batch_execute("RESET ROLE").await?;
    // Owner-only negative control after the real service-role path: force a late
    // new-protocol pending-source violation. This is never an exposed runtime DML.
    let poison = Uuid::new_v4();
    c.execute("INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref) VALUES($1,$2,'native_opening_v1',$2,$3,'pending','')", &[&format!("native-opening:{poison}"),&poison.to_string(),&"1".repeat(64)]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND request_ref=$1", &[&request.to_string()]).await?.get::<_,i64>(0),0);
    assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='host_candidate_v1' AND target_ref=$1::uuid::text", &[&f.game]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.game_checkpoint WHERE game_id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn replacement_controller_recovers_original_without_reusing_revoked_authority() -> TestResult
{
    let c = connection().await?;
    let f = native_with_roster(&c, "bo1", false).await?;
    let row=c.query_one("SELECT seat.id,b.id,g.id,g.binding_epoch::text,g.grant_secret_ref,s.deck_revision_id,r.configuration_id FROM app.match_seat seat JOIN app.ai_runner_binding b ON b.match_seat_id=seat.id JOIN app.controller_grant g ON g.binding_id=b.id AND g.binding_epoch=b.binding_epoch AND g.state='active' JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.room r ON r.id=s.room_id WHERE seat.match_id=$1 AND seat.controller_kind='ai'", &[&f.matched]).await?;
    let ai_seat: Uuid = row.get(0);
    let binding: Uuid = row.get(1);
    let grant: Uuid = row.get(2);
    let epoch: String = row.get(3);
    let secret: String = row.get(4);
    let revision: Uuid = row.get(5);
    let configuration: Uuid = row.get(6);
    c.batch_execute("SET ROLE service_role").await?;
    let access: Value = c
        .query_one(
            "SELECT app.read_native_controller_access($1,$2::text::numeric,$3,$4,$5)",
            &[&grant, &epoch, &secret, &f.game, &f.context],
        )
        .await?
        .get(0);
    assert_eq!(access["data"]["actor"]["seatId"], json!(ai_seat));
    assert_eq!(access["data"]["source"]["nativeEpoch"], "0");
    let wrong:Value=c.query_one("SELECT app.read_native_controller_access($1,$2::text::numeric,'fixture:wrong-secret',$3,$4)", &[&grant,&epoch,&f.game,&f.context]).await?.get(0);
    assert_eq!(wrong["data"]["actor"], "unavailable");
    let issued: Value = c
        .query_one(
            "SELECT app.publish_native_view_source($1,$2,'0','0',$3,'live',$4)",
            &[&f.game, &f.context, &ai_seat, &artifact(&f, "view", 130)],
        )
        .await?
        .get(0);
    let original =
        json!({"requestId":Uuid::new_v4(),"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let prepared = json!({"version":"native-player-accept-v1","basis":basis(&f,"0"),"actorSeatId":ai_seat,"viewReceiptId":issued["data"]["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",131),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    // The human sponsor derives its human seat, never the AI's prepared actor.
    assert_eq!(
        submit(&c, &f, &original, Some(&prepared))
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    let accepted: Value = c
        .query_one(
            "SELECT app.submit_native_controller_command($1,$2::text::numeric,$3,$4,$5)",
            &[&grant, &epoch, &secret, &original, &prepared],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success", "{accepted}");
    let rebound: Value = c
        .query_one(
            "SELECT app.rebind_ai_runner($1,$2::text::numeric,'fixture:rebind-proof',$3)",
            &[&binding, &epoch, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(rebound["status"], "success", "{rebound}");
    let next_epoch = rebound["data"]["epoch"].as_str().unwrap();
    let stop: Uuid = rebound["data"]["previousStopId"]
        .as_str()
        .unwrap()
        .parse()?;
    let mut after: Option<Uuid> = None;
    let mut discovered = false;
    loop {
        let page: Value = c
            .query_one("SELECT app.read_native_ai_binding_work($1,100)", &[&after])
            .await?
            .get(0);
        if let Some(found) = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == json!(binding))
        {
            assert_eq!(found["epoch"], next_epoch);
            assert_eq!(found["hasPendingStops"], true);
            assert!(found["startWork"].is_null());
            discovered = true;
        }
        after = page["nextAfter"].as_str().map(str::parse).transpose()?;
        if after.is_none() {
            break;
        }
    }
    assert!(
        discovered,
        "cold discovery must retain the historical stop dependency"
    );
    let stale: Value = c
        .query_one(
            "SELECT app.submit_native_controller_command($1,$2::text::numeric,$3,$4,NULL)",
            &[&grant, &epoch, &secret, &original],
        )
        .await?
        .get(0);
    assert_eq!(stale["data"]["actor"], "unavailable");
    let stops: Value = c
        .query_one(
            "SELECT app.read_ai_stop_work_page($1,NULL,100)",
            &[&binding],
        )
        .await?
        .get(0);
    assert!(
        stops["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["stopId"] == json!(stop))
    );
    let stopped: Value = c
        .query_one(
            "SELECT app.observe_ai_stop($1,$2::text::numeric,$3,'fixture:stopped',$4)",
            &[&binding, &epoch, &stop, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(stopped["status"], "success");
    let start: Value = c
        .query_one(
            "SELECT coalesce(app.read_ai_start_work($1,$2::text::numeric),'null'::jsonb)",
            &[&binding, &next_epoch],
        )
        .await?
        .get(0);
    let start_id: Uuid = start["startId"].as_str().unwrap().parse()?;
    let provider:Value=c.query_one("SELECT app.observe_ai_provider($1,$2::text::numeric,$3,'running',$4,'fixture:provider-proof',$5)", &[&binding,&next_epoch,&start_id,&format!("fixture:replacement:{}",Uuid::new_v4()),&Uuid::new_v4()]).await?.get(0);
    assert_eq!(provider["status"], "success");
    let new_secret = format!("fixture:replacement-secret:{}", Uuid::new_v4());
    let issued: Value = c
        .query_one(
            "SELECT app.issue_ai_grant($1,$2::text::numeric,$3,$4)",
            &[&binding, &next_epoch, &new_secret, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    let new_grant: Uuid = issued["data"]["resourceId"].as_str().unwrap().parse()?;
    let pending: Value = c
        .query_one(
            "SELECT app.read_native_controller_access($1,$2::text::numeric,$3,$4,$5)",
            &[&new_grant, &next_epoch, &new_secret, &f.game, &f.context],
        )
        .await?
        .get(0);
    assert_eq!(pending["data"]["actor"], "unavailable");
    let delivered: Value = c
        .query_one(
            "SELECT app.observe_ai_grant_delivery($1,0,'delivered','fixture:delivered',$2)",
            &[&new_grant, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(delivered["status"], "success");
    let ready:Value=c.query_one("SELECT app.commit_ai_ready_confirmation($1,$2::text::numeric,$3,'fixture:qualified','fixture:authenticated',$4,$5)", &[&new_grant,&next_epoch,&revision,&configuration,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(ready["status"], "success", "{ready}");
    let recovered: Value = c
        .query_one(
            "SELECT app.submit_native_controller_command($1,$2::text::numeric,$3,$4,NULL)",
            &[&new_grant, &next_epoch, &new_secret, &original],
        )
        .await?
        .get(0);
    assert_eq!(recovered, accepted);
    let query = json!({"gameId":f.game,"contextId":f.context,"outstanding":[original.clone()]});
    let receipts: Value = c
        .query_one(
            "SELECT app.read_native_controller_receipts($1,$2::text::numeric,$3,$4)",
            &[&new_grant, &next_epoch, &new_secret, &query],
        )
        .await?
        .get(0);
    assert_eq!(receipts["data"]["receipts"][0]["response"], accepted);
    let retained:Uuid=c.query_one("SELECT controller_grant_id FROM app.command_receipt WHERE scope='native_player_v1' AND principal_key=$1 AND request_ref=$2", &[&format!("native-seat:{ai_seat}"),&original["requestId"].as_str().unwrap()]).await?.get(0);
    assert_eq!(retained, grant);
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "1"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn future_and_foreign_native_bases_roll_back_without_stale_receipts() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    c.batch_execute("SET ROLE service_role").await?;
    for invalid_basis in [
        basis(&f, "1"),
        json!({"gameId":f.game,"contextId":Uuid::new_v4(),"epoch":"0","revision":"0"}),
        json!({"gameId":f.game,"contextId":f.context,"epoch":"1","revision":"0"}),
    ] {
        let original =
            json!({"requestId":Uuid::new_v4(),"basis":invalid_basis,"action":{"kind":"pass"}});
        assert_eq!(
            submit(&c, &f, &original, None).await?,
            json!({"status":"fail","data":{"game":"unavailable"}})
        );
        assert_eq!(c.query_one("SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND request_ref=$1", &[&original["requestId"].as_str().unwrap()]).await?.get::<_,i64>(0),0);
    }
    assert_eq!(
        c.query_one(
            "SELECT accepted_revision::text FROM app.game WHERE id=$1",
            &[&f.game]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deletion_pending_player_can_finish_previously_admitted_native_game() -> TestResult {
    let c = connection().await?;
    let f = native(&c, "bo1").await?;
    c.batch_execute("SET ROLE service_role").await?;
    let actor: Uuid = c
        .query_one(
            "SELECT account_id FROM app.match_seat WHERE id=$1",
            &[&f.seat],
        )
        .await?
        .get(0);
    let closing = super::deletion::request(&c, actor, Uuid::new_v4()).await?;
    assert_eq!(closing["status"], "success", "{closing}");
    let access: Value = c
        .query_one(
            "SELECT app.read_native_human_access($1,$2,$3,$4)",
            &[&f.subject, &f.session, &f.game, &f.context],
        )
        .await?
        .get(0);
    assert_eq!(access["status"], "success", "{access}");
    let issued = view(&c, &f, "0", 90).await?;
    let original =
        json!({"requestId":Uuid::new_v4(),"basis":basis(&f,"0"),"action":{"kind":"pass"}});
    let prepared = json!({"version":"native-player-accept-v1","basis":basis(&f,"0"),"actorSeatId":f.seat,"viewReceiptId":issued["receiptId"],"runtimeId":"1".repeat(64),"programSourceId":"2".repeat(64),"inputRef":artifact(&f,"input",91),"owner":OWNER,"fenceEpoch":"1","candidate":candidate(&c,&f).await?});
    let accepted = submit(&c, &f, &original, Some(&prepared)).await?;
    assert_eq!(accepted["status"], "success", "{accepted}");
    assert_eq!(accepted["data"]["revision"], "1");
    assert_eq!(submit(&c, &f, &original, None).await?, accepted);
    assert_eq!(
        c.query_one("SELECT state FROM app.account WHERE id=$1", &[&actor])
            .await?
            .get::<_, String>(0),
        "deletion_pending"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn current_controller_can_answer_its_own_round_and_recover_the_exact_reply() -> TestResult {
    let c = connection().await?;
    let f = native_with_roster(&c, "bo3", false).await?;
    let row=c.query_one("SELECT seat.id,g.id,g.binding_epoch::text,g.grant_secret_ref,seat.source_revision_id FROM app.match_seat seat JOIN app.ai_runner_binding b ON b.match_seat_id=seat.id JOIN app.controller_grant g ON g.binding_id=b.id AND g.binding_epoch=b.binding_epoch AND g.state='active' WHERE seat.match_id=$1 AND seat.controller_kind='ai'", &[&f.matched]).await?;
    let ai_seat: Uuid = row.get(0);
    let grant: Uuid = row.get(1);
    let epoch: String = row.get(2);
    let secret: String = row.get(3);
    let revision: Uuid = row.get(4);
    c.batch_execute("SET ROLE service_role").await?;
    finish_opening_game_as_draw(&c, &f).await?;
    let own=c.query_one("SELECT d.id,m.revision::text FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id WHERE d.match_id=$1 AND d.seat_id=$2 AND d.state='pending'", &[&f.matched,&ai_seat]).await?;
    let decision: Uuid = own.get(0);
    let match_basis: String = own.get(1);
    let source: Value = c
        .query_one(
            "SELECT app.read_native_between_source($1,$2::text::numeric,$3)",
            &[&decision, &match_basis, &revision],
        )
        .await?
        .get(0);
    assert_eq!(source["status"], "success");
    let original = json!({"requestId":Uuid::new_v4(),"decisionId":decision,"expectedRevision":match_basis,"selectedRevisionId":revision});
    let prepared = json!({"version":"native-between-prepared-v1","sourceHash":source["data"]["sourceHash"],"replyRef":"fixture:AI-sideboard","qualificationRef":"fixture:SQL-only"});
    let sponsor: Value = c
        .query_one(
            "SELECT app.qualify_between_game_command($1,$2,$3,$4)",
            &[&f.subject, &f.session, &original, &prepared],
        )
        .await?
        .get(0);
    assert_eq!(sponsor["data"]["actor"], "unavailable");
    let wrong:Value=c.query_one("SELECT app.qualify_controller_between_game_command($1,$2::text::numeric,'fixture:wrong-secret',$3,$4)", &[&grant,&epoch,&original,&prepared]).await?.get(0);
    assert_eq!(wrong["data"]["actor"], "unavailable");
    let accepted: Value = c
        .query_one(
            "SELECT app.qualify_controller_between_game_command($1,$2::text::numeric,$3,$4,$5)",
            &[&grant, &epoch, &secret, &original, &prepared],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success", "{accepted}");
    assert!(accepted["data"].get("qualificationRef").is_none());
    let duplicate: Value = c
        .query_one(
            "SELECT app.qualify_controller_between_game_command($1,$2::text::numeric,$3,$4,NULL)",
            &[&grant, &epoch, &secret, &original],
        )
        .await?
        .get(0);
    assert_eq!(duplicate, accepted);
    let read: Value = c
        .query_one(
            "SELECT app.read_qualified_controller_between_game_command($1,$2::text::numeric,$3,$4)",
            &[&grant, &epoch, &secret, &original],
        )
        .await?
        .get(0);
    assert_eq!(read["data"]["response"], accepted);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn native_scalar_and_artifact_grammars_reject_aliases_wrong_scope_and_size_overflow()
-> TestResult {
    let c = connection().await?;
    let game = Uuid::new_v4();
    let context = Uuid::new_v4();
    for value in [
        "",
        "+0",
        "-0",
        "00",
        "01",
        "0.0",
        "1e0",
        "NaN",
        "Infinity",
        " 0",
        "0 ",
        "0\n",
        "18446744073709551616",
    ] {
        assert!(
            !c.query_one("SELECT app.native_u64_is_canonical($1)", &[&value])
                .await?
                .get::<_, bool>(0),
            "accepted native alias {value:?}"
        );
    }
    let digest = "1".repeat(64);
    for (role, maximum) in [
        ("initial", 1_048_576_u64),
        ("input", 1_048_576),
        ("state", 32_768),
        ("facts", 33_554_432),
        ("event", 33_554_432),
        ("view", 33_554_432),
        ("replay", 33_554_432),
    ] {
        let good = format!("native-artifact-v1/{game}/{context}/{role}/{maximum}/{digest}.json");
        assert!(
            c.query_one(
                "SELECT app.native_artifact_ref_is_valid($1,$2,$3,$4)",
                &[&good, &game, &context, &role]
            )
            .await?
            .get::<_, bool>(0)
        );
        for bad in [
            good.replace(&format!("/{maximum}/"), &format!("/{}/", maximum + 1)),
            good.replace(&format!("/{maximum}/"), "/0/"),
            good.replace(&format!("/{maximum}/"), "/01/"),
            good.replace(&context.to_string(), &Uuid::new_v4().to_string()),
            format!("{good}\n"),
        ] {
            assert!(
                !c.query_one(
                    "SELECT app.native_artifact_ref_is_valid($1,$2,$3,$4)",
                    &[&bad, &game, &context, &role]
                )
                .await?
                .get::<_, bool>(0),
                "accepted malformed {role} artifact"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn constructed_room_writer_rechecks_format_and_every_selected_card_before_acceptance()
-> TestResult {
    let c = connection().await?;
    c.batch_execute("BEGIN").await?;
    let f = rooms::room(&c, true).await?;
    let format: Uuid = c
        .query_one("SELECT format_id FROM app.room WHERE id=$1", &[&f.id])
        .await?
        .get(0);
    c.execute(
        "UPDATE app.format SET key='pioneer' WHERE id=$1",
        &[&format],
    )
    .await?;
    let b = rooms::basis(&c, f.id).await?;
    let auth=c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1",&[&f.source.actor]).await?;
    let subject: Uuid = auth.get(0);
    let session: Uuid = auth.get(1);
    for (key, status, accepted) in [
        ("modern", Some("legal"), false),
        ("pioneer", None, false),
        ("pioneer", Some("banned"), false),
        ("pioneer", Some("restricted"), false),
        ("pioneer", Some("legal"), true),
    ] {
        let legality = Uuid::new_v4();
        c.execute("INSERT INTO app.legality_revision(id,source_ref,effective_at,imported_at) VALUES($1,$1::uuid::text,clock_timestamp(),clock_timestamp())",&[&legality]).await?;
        if let Some(status) = status {
            c.execute("INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status) VALUES($1,$2,$3,$4)",&[&f.source.definition,&format,&legality,&status]).await?;
        }
        let source: Value = c
            .query_one(
                "SELECT app.read_native_room_source($1,$2::text::numeric,$3,$4)",
                &[&f.id, &b, &f.source.release, &legality],
            )
            .await?
            .get(0);
        assert_eq!(source["status"], "success");
        let request = Uuid::new_v4();
        let original = json!({"requestId":request,"expectedRevision":b});
        let prepared = json!({"version":"native-ready-prepared-v1","sourceHash":source["data"]["sourceHash"],"releaseId":f.source.release,
            "legalityRevisionId":legality,"selectedRevisionId":f.source.revision,"qualificationRef":format!("native-constructed-ready-v1/{key}/sha256:{}","a".repeat(64))});
        if status.is_none() {
            c.batch_execute("SAVEPOINT pending_deletion").await?;
            c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.source.actor]).await?;
            let pending_reply: Value = c
                .query_one(
                    "SELECT app.qualify_room_ready_command($1,$2,$3,$4,$5)",
                    &[&subject, &session, &f.id, &original, &prepared],
                )
                .await?
                .get(0);
            assert_eq!(
                pending_reply,
                json!({"status":"fail","data":{"account":"unavailable"}})
            );
            assert_eq!(rooms::basis(&c, f.id).await?, b);
            c.batch_execute("ROLLBACK TO SAVEPOINT pending_deletion")
                .await?;
        }
        let reply: Value = c
            .query_one(
                "SELECT app.qualify_room_ready_command($1,$2,$3,$4,$5)",
                &[&subject, &session, &f.id, &original, &prepared],
            )
            .await?
            .get(0);
        let receipt=c.query_opt("SELECT normalized_source->'constructedPolicy' FROM app.command_receipt WHERE account_id=$1 AND request_ref=$2 AND scope='qualified_ready_intent_v1'",&[&f.source.actor,&request.to_string()]).await?;
        if accepted {
            assert_eq!(reply["status"], "success", "{reply}");
            let policy: Value = receipt.unwrap().get(0);
            assert_eq!(policy["formatKey"], key);
            assert_eq!(policy["cards"][0]["status"], "legal");
            assert_eq!(policy["legalityRevisionId"], json!(legality));
        } else {
            assert_eq!(
                reply,
                json!({"status":"fail","data":{"deck":"qualified selected revision required"}})
            );
            assert!(receipt.is_some());
            let disposition: Value = c
                .query_one(
                    "SELECT app.read_qualified_room_ready_command($1,$2,$3,$4)",
                    &[&subject, &session, &f.id, &original],
                )
                .await?
                .get(0);
            assert_eq!(
                disposition["data"],
                json!({"state":"rejected","response":reply})
            );
            let retry: Value = c
                .query_one(
                    "SELECT app.qualify_room_ready_command($1,$2,$3,$4,NULL)",
                    &[&subject, &session, &f.id, &original],
                )
                .await?
                .get(0);
            assert_eq!(retry, reply);
            assert_eq!(rooms::basis(&c, f.id).await?, b);
        }
    }
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn between_game_constructed_reply_requires_the_original_certified_room_roster() -> TestResult
{
    let c = connection().await?;
    let f = native(&c, "bo3").await?;
    c.batch_execute("SET ROLE service_role").await?;
    finish_opening_game_as_draw(&c, &f).await?;
    let row = c.query_one(
        "SELECT d.id,m.revision::text,s.source_revision_id,f.key FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id JOIN app.match_seat s ON s.id=d.seat_id JOIN app.format f ON f.id=m.format_id WHERE m.id=$1 AND s.id=$2 AND d.state='pending'",
        &[&f.matched, &f.seat],
    ).await?;
    let decision: Uuid = row.get(0);
    let basis: String = row.get(1);
    let selected: Uuid = row.get(2);
    let format: String = row.get(3);
    let source: Value = c
        .query_one(
            "SELECT app.read_native_between_source($1,$2::text::numeric,$3)",
            &[&decision, &basis, &selected],
        )
        .await?
        .get(0);
    let hash = source["data"]["sourceHash"]
        .as_str()
        .expect("fixture produced a pending source");
    let original = json!({"requestId":Uuid::new_v4(),"decisionId":decision,"expectedRevision":basis,"selectedRevisionId":selected});
    let prepared = json!({"version":"native-between-prepared-v1","replyRef":format!("native-between-reply-v1/{hash}"),
       "qualificationRef":format!("native-constructed-between-v1/{format}/{hash}"),"sourceHash":hash});
    let refusal = c
        .query_one(
            "SELECT app.qualify_between_game_command($1,$2,$3,$4)",
            &[&f.subject, &f.session, &original, &prepared],
        )
        .await
        .unwrap_err();
    assert_eq!(refusal.code(), Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        refusal.as_db_error().unwrap().message(),
        "between-game selection lacks accepted room policy"
    );
    let outcome: Value = c
        .query_one(
            "SELECT app.read_qualified_between_game_command($1,$2,$3)",
            &[&f.subject, &f.session, &original],
        )
        .await?
        .get(0);
    assert_eq!(outcome["data"], json!({"state":"unknown","response":null}));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn validated_countdown_requires_worker_authority_and_an_accepted_original() -> TestResult {
    let c = connection().await?;
    let room = Uuid::new_v4();
    let request = Uuid::new_v4();
    for role in [
        "anon",
        "authenticated",
        "supabase_auth_admin",
        "service_role",
    ] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        let error = c
            .query_one(
                "SELECT app.start_validated_countdown($1,0,$2)",
                &[&room, &request],
            )
            .await
            .unwrap_err();
        let expected = if role == "service_role" {
            SqlState::T_R_SERIALIZATION_FAILURE
        } else {
            SqlState::INSUFFICIENT_PRIVILEGE
        };
        assert_eq!(error.code(), Some(&expected), "{role}: {error}");
        c.batch_execute("RESET ROLE").await?;
    }
    let count: i64 = c
        .query_one(
            "SELECT count(*) FROM app.command_receipt WHERE request_ref=$1",
            &[&request.to_string()],
        )
        .await?
        .get(0);
    assert_eq!(count, 0);
    Ok(())
}
