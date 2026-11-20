//! Actual SQL metadata observation; opaque fixture refs do not qualify native bytes.
use super::{
    TestResult, ante_transaction_integrity as ante, connection, host_integrity,
    match_lifecycle_integrity as lifecycle, wait_for_database_lock,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn without_policy(clocks: &Value) -> Value {
    Value::Array(
        clocks
            .as_array()
            .unwrap()
            .iter()
            .map(|clock| {
                let mut row = clock.as_object().unwrap().clone();
                assert!(row.remove("policyRef").unwrap().is_string());
                Value::Object(row)
            })
            .collect(),
    )
}
async fn assert_policy_snapshot(
    c: &Client,
    matched: Uuid,
    source: &Value,
    policy: &str,
) -> TestResult {
    let clocks = &source["data"]["matchClocks"];
    assert!(!clocks.as_array().unwrap().is_empty());
    assert!(
        clocks
            .as_array()
            .unwrap()
            .iter()
            .all(|clock| clock["policyRef"] == policy)
    );
    let canonical: Value = c
        .query_one("SELECT app.match_clock_snapshot($1)", &[&matched])
        .await?
        .get(0);
    assert_eq!(without_policy(clocks), canonical);
    Ok(())
}
async fn read(c: &Client, game: Uuid) -> Result<Value, tokio_postgres::Error> {
    Ok(
        c.query_one("SELECT app.read_native_game_source($1)", &[&game])
            .await?
            .get(0),
    )
}
async fn reserve(c: &Client, structure: &str) -> FixtureResult<(Uuid, Uuid)> {
    let (_, m) = ante::reserve_match(c, true, structure).await?;
    let g = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    Ok((m, g))
}
async fn opened(c: &Client, structure: &str) -> FixtureResult<(Uuid, Uuid)> {
    let (m, g) = reserve(c, structure).await?;
    lifecycle::open(c, m, g).await?;
    Ok((m, g))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn missing_nil_reserved_and_unqualified_legacy_are_unavailable_without_writes() -> TestResult
{
    let c = connection().await?;
    let (m, g) = reserve(&c, "bo1").await?;
    let legacy = host_integrity::fixture(&c).await?;
    c.batch_execute("BEGIN READ ONLY").await?;
    let unavailable = json!({"status":"fail","data":{"game":"unavailable"}});
    for game in [Uuid::nil(), Uuid::new_v4(), g, legacy.game] {
        assert_eq!(read(&c, game).await?, unavailable);
    }
    assert_eq!(
        c.query_one("SELECT app.read_native_game_source(NULL)", &[])
            .await?
            .get::<_, Value>(0),
        unavailable
    );
    c.batch_execute("COMMIT").await?;
    assert_eq!(c.query_one("SELECT (SELECT count(*) FROM app.command_receipt WHERE scope='game_opening_v1' AND target_ref=($1::uuid)::text)+(SELECT count(*) FROM app.game_context WHERE game_id=$1)",&[&g]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one("SELECT state FROM app.match WHERE id=$1", &[&m])
            .await?
            .get::<_, String>(0),
        "ready"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn exact_opening_source_roster_clocks_and_creator_are_observed_read_only() -> TestResult {
    let c = connection().await?;
    let (m, g) = opened(&c, "bo1").await?;
    let before: Value = c
        .query_one("SELECT app.match_retained_frontiers($1)", &[&m])
        .await?
        .get(0);
    let source = read(&c, g).await?;
    assert_eq!(source["status"], "success");
    let data = &source["data"];
    assert_eq!(data["version"], "native-game-source-v1");
    assert_eq!(data["gameId"], json!(g));
    assert_eq!(data["matchId"], json!(m));
    assert_eq!(data["contextId"], json!(g));
    assert_eq!(data["gameState"], "active");
    assert_eq!(data["matchState"], "active");
    assert_eq!(data["acceptedRevision"], "0");
    assert_eq!(data["lastEventSequence"], "0");
    assert_eq!(data["initialRef"], "fixture:context");
    assert_eq!(data["contextRevision"], "fixture:v1");
    let expected: Value=c.query_one("SELECT jsonb_build_object('id',cp.id,'acceptedRevision',cp.accepted_revision::text,'stateRef',cp.private_state_ref,'clockRevision',cp.clock_revision::text,'creatorReceiptId',cp.source_receipt_id) FROM app.game_checkpoint cp WHERE game_id=$1 AND accepted_revision=0",&[&g]).await?.get(0);
    assert_eq!(data["checkpoint"], expected);
    let expected: Value=c.query_one("SELECT jsonb_agg(jsonb_build_object('seatId',id,'seatIndex',seat_index::text,'accountId',account_id,'controllerKind',controller_kind,'sourceRevisionId',source_revision_id,'deckSnapshotRef',deck_snapshot_ref) ORDER BY seat_index) FROM app.match_seat WHERE match_id=$1",&[&m]).await?.get(0);
    assert_eq!(data["seats"], expected);
    assert_eq!(data["seats"].as_array().unwrap().len(), 2);
    let expected: Value = c
        .query_one("SELECT app.match_clock_snapshot($1)", &[&m])
        .await?
        .get(0);
    assert_eq!(without_policy(&data["matchClocks"]), expected);
    assert!(
        data["matchClocks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|clock| clock["policyRef"] == "bo1-bank-inactive-v1")
    );
    assert_eq!(
        data["operational"],
        json!({"ownerRef":"fixture:lifecycle","fenceEpoch":"1","revision":"0","availability":"available","recoveryCheckpointId":null})
    );
    assert_eq!(read(&c, g).await?, source);
    assert_eq!(
        c.query_one("SELECT app.match_retained_frontiers($1)", &[&m])
            .await?
            .get::<_, Value>(0),
        before
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn completed_earlier_bo3_game_remains_readable_when_next_game_is_ready() -> TestResult {
    let c = connection().await?;
    let (m, g) = opened(&c, "bo3").await?;
    let original = read(&c, g).await?;
    assert_policy_snapshot(&c, m, &original, "bo3-overall-bank-v1").await?;
    let result = lifecycle::complete(&c, m, g, vec![]).await?;
    let completed = read(&c, g).await?;
    assert_eq!(completed["data"]["acceptedRevision"], "1");
    assert_policy_snapshot(&c, m, &completed, "bo3-overall-bank-v1").await?;
    assert_eq!(
        lifecycle::consume(&c, m, result).await?["status"],
        "success"
    );
    for row in c.query("SELECT id,seat_id FROM app.between_game_decision WHERE match_id=$1 AND after_game_id=$2 ORDER BY seat_id",&[&m,&g]).await? {
        let decision: Uuid=row.get(0); let seat: Uuid=row.get(1);
        let source: Value=c.query_one("SELECT app.reply_between_games($1,(SELECT revision FROM app.match WHERE id=$2),(SELECT source_revision_id FROM app.match_seat WHERE id=$3),'fixture:sideboard','fixture:validator',$4)",&[&decision,&m,&seat,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(source["status"],"success");
    }
    let historical = read(&c, g).await?;
    assert_policy_snapshot(&c, m, &historical, "bo3-overall-bank-v1").await?;
    assert_eq!(historical["status"], "success");
    assert_eq!(historical["data"]["gameState"], "completed");
    assert_eq!(historical["data"]["matchState"], "ready");
    assert_eq!(
        historical["data"]["initialRef"],
        original["data"]["initialRef"]
    );
    assert_eq!(historical["data"]["seats"], original["data"]["seats"]);
    assert_eq!(
        historical["data"]["checkpoint"]["stateRef"],
        "fixture:terminal"
    );
    assert_eq!(historical["data"]["lastEventSequence"], "1");
    let next: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    assert_ne!(next, g);
    assert_eq!(read(&c, next).await?["status"], "fail");
    lifecycle::open(&c, m, next).await?;
    let later = read(&c, g).await?;
    assert_policy_snapshot(&c, m, &later, "bo3-overall-bank-v1").await?;
    assert_eq!(later["status"], "success");
    assert_eq!(
        later["data"]["checkpoint"],
        historical["data"]["checkpoint"]
    );
    assert_ne!(
        later["data"]["matchClocks"][0]["clockRevision"],
        later["data"]["checkpoint"]["clockRevision"]
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn incident_and_restoration_keep_checkpoint_and_operational_clock_bases_distinct()
-> TestResult {
    let c = connection().await?;
    let (m, g) = opened(&c, "bo1").await?;
    let original = read(&c, g).await?;
    assert_policy_snapshot(&c, m, &original, "bo1-bank-inactive-v1").await?;
    let paused: Value=c.query_one("SELECT app.pause_game_incident($1,'fixture:lifecycle',1,0,clock_timestamp(),'storage','fixture:source-reader',$2)",&[&g,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(paused["status"], "success");
    let source = read(&c, g).await?;
    assert_policy_snapshot(&c, m, &source, "bo1-bank-inactive-v1").await?;
    assert_eq!(source["status"], "success");
    assert_eq!(source["data"]["checkpoint"], original["data"]["checkpoint"]);
    assert_eq!(source["data"]["operational"]["availability"], "recovering");
    assert_eq!(source["data"]["operational"]["fenceEpoch"], "2");
    let cp: Uuid = paused["data"]["checkpointId"].as_str().unwrap().parse()?;
    assert_eq!(
        source["data"]["operational"]["recoveryCheckpointId"],
        json!(cp)
    );
    let restored: Value=c.query_one("SELECT app.restore_game_checkpoint($1,2,1,$2,'fixture:replacement','fixture:exact-codec',$3)",&[&g,&cp,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(restored["status"], "success");
    let source = read(&c, g).await?;
    assert_policy_snapshot(&c, m, &source, "bo1-bank-inactive-v1").await?;
    assert_eq!(source["data"]["checkpoint"], original["data"]["checkpoint"]);
    assert_eq!(
        source["data"]["operational"]["ownerRef"],
        "fixture:replacement"
    );
    assert_eq!(source["data"]["operational"]["availability"], "available");
    assert_eq!(
        without_policy(&source["data"]["matchClocks"]),
        c.query_one("SELECT app.match_clock_snapshot($1)", &[&m])
            .await?
            .get::<_, Value>(0)
    );
    assert_ne!(
        source["data"]["matchClocks"][0]["clockRevision"],
        source["data"]["checkpoint"]["clockRevision"]
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn calling_query_snapshot_survives_actual_concurrent_native_commit_without_mixed_frontiers()
-> TestResult {
    let c = connection().await?;
    let (m, g) = opened(&c, "bo1").await?;
    let original = read(&c, g).await?;
    let key = i64::from_be_bytes(Uuid::new_v4().as_bytes()[..8].try_into()?);
    c.execute("SELECT pg_advisory_lock($1)", &[&key]).await?;
    let observer = connection().await?;
    let pid: i32 = observer
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let task = tokio::spawn(async move {
        observer.query_one("WITH prior AS MATERIALIZED (SELECT app.read_native_game_source($1) value), gate AS MATERIALIZED (SELECT pg_advisory_lock($2) FROM prior) SELECT prior.value,app.read_native_game_source($1) FROM prior CROSS JOIN gate",&[&g,&key]).await
    });
    wait_for_database_lock(&c, pid).await?;
    lifecycle::complete(&c, m, g, vec![]).await?;
    c.execute("SELECT pg_advisory_unlock($1)", &[&key]).await?;
    let observed = task.await??;
    assert_eq!(observed.get::<_, Value>(0), original);
    assert_eq!(observed.get::<_, Value>(1), original);
    let fresh = read(&c, g).await?;
    assert_eq!(fresh["data"]["acceptedRevision"], "1");
    assert_eq!(fresh["data"]["lastEventSequence"], "1");
    assert_eq!(fresh["data"]["checkpoint"]["stateRef"], "fixture:terminal");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn execution_is_service_only_and_privilege_does_not_grant_raw_mutation() -> TestResult {
    let c = connection().await?;
    let (_, g) = opened(&c, "bo1").await?;
    let original = read(&c, g).await?;
    for role in ["anon", "authenticated", "supabase_auth_admin"] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        assert_eq!(
            read(&c, g).await.unwrap_err().code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        c.batch_execute("RESET ROLE").await?;
    }
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(read(&c, g).await?, original);
    assert_eq!(
        c.execute(
            "UPDATE app.game SET accepted_revision=accepted_revision+1 WHERE id=$1",
            &[&g]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    c.batch_execute("RESET ROLE").await?;
    let row=c.query_one("SELECT provolatile,prosecdef,proconfig FROM pg_proc WHERE oid='app.read_native_game_source(uuid)'::regprocedure",&[]).await?;
    assert_eq!(row.get::<_, i8>(0), b's' as i8);
    assert!(row.get::<_, bool>(1));
    assert!(
        row.get::<_, Vec<String>>(2)
            .iter()
            .any(|x| x == "search_path=\"\"" || x == "search_path=")
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn inconsistent_qualified_frontier_raises_instead_of_returning_defaults() -> TestResult {
    let c = connection().await?;
    let (_, g) = opened(&c, "bo1").await?;
    let original = read(&c, g).await?;
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision,source_receipt_id) SELECT game_id,context_id,1,'fixture:poison','fixture:poison',clock_revision,source_receipt_id FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=0",&[&g]).await?;
    assert_eq!(
        read(&c, g).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(read(&c, g).await?, original);
    // A counterfeit same-game activity row also fails the actual original
    // creator checker, without invoking any new hand-written source validator.
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) SELECT e.game_id,e.id,s.account_id,'fixture',1,'fixture' FROM app.game_event e JOIN app.game g ON g.id=e.game_id JOIN app.match_seat s ON s.match_id=g.match_id WHERE e.game_id=$1 AND e.sequence=0 AND s.account_id IS NOT NULL LIMIT 1",&[&g]).await?;
    assert_eq!(
        read(&c, g).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(read(&c, g).await?, original);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres; policy metadata, not native clock eligibility"]
async fn selected_clock_policy_is_local_to_source_and_preserves_canonical_history() -> TestResult {
    let c = connection().await?;
    let mut games = vec![];
    for (structure, policy, amount) in [
        ("bo1", "bo1-bank-inactive-v1", "0"),
        ("bo3", "bo3-overall-bank-v1", "1800000"),
    ] {
        let (m, g) = opened(&c, structure).await?;
        let before:Value=c.query_one("SELECT jsonb_agg(jsonb_build_object('id',id,'normalized',normalized_source::text,'response',response_payload::text) ORDER BY id) FROM app.command_receipt WHERE target_ref=ANY(ARRAY[$1::uuid::text,$2::uuid::text,(SELECT room_id::text FROM app.match WHERE id=$1)])",&[&m,&g]).await?.get(0);
        let helper: Value = c
            .query_one("SELECT app.match_clock_snapshot($1)", &[&m])
            .await?
            .get(0);
        assert!(
            helper
                .as_array()
                .unwrap()
                .iter()
                .all(|clock| clock.get("policyRef").is_none())
        );
        c.batch_execute("BEGIN READ ONLY").await?;
        let observed = read(&c, g).await?;
        c.batch_execute("COMMIT").await?;
        let clocks = &observed["data"]["matchClocks"];
        assert_eq!(without_policy(clocks), helper);
        let rows = clocks.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(
            rows.windows(2)
                .all(|pair| pair[0]["seatId"].as_str().unwrap()
                    < pair[1]["seatId"].as_str().unwrap())
        );
        for clock in rows {
            assert_eq!(clock["policyRef"], policy);
            assert_eq!(clock["remainingMs"], amount);
            assert_eq!(clock["paused"], true);
            let seat: Uuid = clock["seatId"].as_str().unwrap().parse()?;
            let actual=c.query_one("SELECT c.policy_ref,s.match_id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE c.match_seat_id=$1",&[&seat]).await?;
            assert_eq!(actual.get::<_, String>(0), policy);
            assert_eq!(actual.get::<_, Uuid>(1), m);
        }
        let after:Value=c.query_one("SELECT jsonb_agg(jsonb_build_object('id',id,'normalized',normalized_source::text,'response',response_payload::text) ORDER BY id) FROM app.command_receipt WHERE target_ref=ANY(ARRAY[$1::uuid::text,$2::uuid::text,(SELECT room_id::text FROM app.match WHERE id=$1)])",&[&m,&g]).await?.get(0);
        assert_eq!(before, after);
        games.push((g, policy));
    }
    // The second match cannot supply policy for the first match's clocks.
    for (game, policy) in games {
        assert!(
            read(&c, game).await?["data"]["matchClocks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|clock| clock["policyRef"] == policy)
        );
    }
    Ok(())
}

async fn clock_dml_denied(c: &Client, seat: Uuid) -> TestResult {
    for sql in [
        "INSERT INTO app.match_clock(match_seat_id,remaining_ms,clock_revision,policy_ref,paused,last_accounted_at) VALUES($1,0,0,'bo1-bank-inactive-v1',true,clock_timestamp())",
        "UPDATE app.match_clock SET policy_ref=policy_ref WHERE match_seat_id=$1",
        "DELETE FROM app.match_clock WHERE match_seat_id=$1",
    ] {
        assert_eq!(
            c.execute(sql, &[&seat]).await.unwrap_err().code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres; private RPC and raw clock permission matrix"]
async fn clock_policy_enrichment_does_not_expand_rpc_or_raw_clock_permissions() -> TestResult {
    let c = connection().await?;
    let (m, g) = opened(&c, "bo1").await?;
    let seats: Vec<Uuid> = c
        .query(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY id",
            &[&m],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    let subject:Uuid=c.query_one("SELECT a.auth_subject FROM app.account a JOIN app.match_seat s ON s.account_id=a.id WHERE s.match_id=$1 ORDER BY s.id LIMIT 1",&[&m]).await?.get(0);
    let (_, stranger) = super::account(&c).await?;
    for role in ["anon", "supabase_auth_admin"] {
        c.batch_execute(&format!("SET ROLE {role}")).await?;
        assert_eq!(
            read(&c, g).await.unwrap_err().code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        assert_eq!(
            c.query(
                "SELECT policy_ref FROM app.match_clock WHERE match_seat_id=ANY($1)",
                &[&seats]
            )
            .await
            .unwrap_err()
            .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        clock_dml_denied(&c, seats[0]).await?;
        c.batch_execute("RESET ROLE").await?;
    }
    super::set_auth_claims(&c, subject).await?;
    assert_eq!(
        read(&c, g).await.unwrap_err().code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    assert_eq!(
        c.query(
            "SELECT policy_ref FROM app.match_clock WHERE match_seat_id=ANY($1)",
            &[&seats]
        )
        .await?
        .len(),
        2
    );
    assert_eq!(
        c.query(
            "SELECT eligibility_receipt_id FROM app.match_clock WHERE match_seat_id=ANY($1)",
            &[&seats]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    clock_dml_denied(&c, seats[0]).await?;
    super::set_auth_claims(&c, stranger).await?;
    assert!(
        c.query(
            "SELECT policy_ref FROM app.match_clock WHERE match_seat_id=ANY($1)",
            &[&seats]
        )
        .await?
        .is_empty()
    );
    c.batch_execute("RESET ROLE; SET ROLE service_role").await?;
    assert_eq!(
        read(&c, g).await?["data"]["matchClocks"][0]["policyRef"],
        "bo1-bank-inactive-v1"
    );
    assert_eq!(c.query("SELECT policy_ref,eligibility_receipt_id FROM app.match_clock WHERE match_seat_id=ANY($1)",&[&seats]).await?.len(),2);
    clock_dml_denied(&c, seats[0]).await?;
    c.batch_execute("RESET ROLE").await?;
    Ok(())
}
