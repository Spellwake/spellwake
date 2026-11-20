//! Trusted failed-restoration fixtures exercise SQL no-contest, not a native codec failure.
use super::{
    TestResult, ante_transaction_integrity as ante, connection,
    match_lifecycle_integrity as lifecycle, wait_for_database_lock,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn basis(c: &Client, m: Uuid) -> FixtureResult<(String, String)> {
    let row = c
        .query_one(
            "SELECT revision::text,availability_revision::text FROM app.match WHERE id=$1",
            &[&m],
        )
        .await?;
    Ok((row.get(0), row.get(1)))
}
async fn pause_between(c: &Client, m: Uuid) -> FixtureResult<(Uuid, String, String)> {
    let (revision, availability) = basis(c, m).await?;
    let paused:Value=c.query_one("SELECT app.pause_match_between_games($1,$2::text::numeric,$3::text::numeric,clock_timestamp(),'storage','fixture:incident',$4)", &[&m,&revision,&availability,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(paused["status"], "success", "{paused}");
    Ok((
        paused["data"]["incidentId"].as_str().unwrap().parse()?,
        paused["data"]["revision"].as_str().unwrap().to_owned(),
        paused["data"]["availabilityRevision"]
            .as_str()
            .unwrap()
            .to_owned(),
    ))
}
async fn terminal(
    c: &Client,
    m: Uuid,
    revision: &str,
    availability: &str,
    incident: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let value:Value=c.query_one("SELECT app.finalize_unrecoverable_match($1,$2::text::numeric,$3::text::numeric,$4,'fixture:verified-failed-restoration')", &[&m,&revision,&availability,&incident]).await.map(|r|r.get(0))?;
    let original = spellwake_application::recovery_ports::FinalizeUnrecoverableMatch {
        match_id: m,
        basis: revision.parse().unwrap(),
        availability_basis: availability.parse().unwrap(),
        incident_id: incident,
        failure_ref: "fixture:verified-failed-restoration".parse().unwrap(),
    };
    assert!(
        original
            .decode_response(&serde_json::to_vec(&value).unwrap())
            .is_ok()
    );
    Ok(value)
}
fn resource(v: &Value) -> Uuid {
    assert_eq!(v["status"], "success", "{v}");
    v["data"]["resourceId"].as_str().unwrap().parse().unwrap()
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn failed_preopening_match_cancels_offered_ante_without_fabricating_played_game_or_result()
-> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve(&c, true).await?;
    let coin = ante::fund(&c, f.source.actor).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&m, &f.source.actor],
        )
        .await?
        .get(0);
    let offered = ante::offer(
        &c,
        m,
        seat,
        "0",
        json!([{"assetId":coin,"quantity":"10"}]),
        None,
    )
    .await?;
    let commitment = resource(&offered);
    let (incident, revision, availability) = pause_between(&c, m).await?;
    let result = terminal(&c, m, &revision, &availability, incident).await?;
    assert_eq!(result["status"], "success", "{result}");
    assert_eq!(
        terminal(&c, m, &revision, &availability, incident).await?,
        result
    );
    let row=c.query_one("SELECT terminal_kind,cardinality(final_game_ids),source_game_result_id IS NULL FROM app.match_result WHERE match_id=$1", &[&m]).await?;
    assert_eq!(row.get::<_, String>(0), "unrecoverable_incident");
    assert_eq!(row.get::<_, i32>(1), 0);
    assert!(row.get::<_, bool>(2));
    assert_eq!(
        c.query_one(
            "SELECT state FROM app.ante_commitment WHERE id=$1",
            &[&commitment]
        )
        .await?
        .get::<_, String>(0),
        "canceled"
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.asset_hold WHERE ante_commitment_id=$1 AND state='held'", &[&commitment]).await?.get::<_,i64>(0),0);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.game_result r JOIN app.game g ON g.id=r.game_id WHERE g.match_id=$1", &[&m]).await?.get::<_,i64>(0),0);
    assert_eq!(
        c.query_one("SELECT state FROM app.game WHERE match_id=$1", &[&m])
            .await?
            .get::<_, String>(0),
        "canceled"
    );
    assert_eq!(
        c.execute("UPDATE app.match SET state='ready' WHERE id=$1", &[&m])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(c.query_one("SELECT app.finalize_unrecoverable_match($1,$2::text::numeric,$3::text::numeric,$4,'different-proof')", &[&m,&revision,&availability,&incident]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn failed_active_match_preserves_native_prefix_and_refunds_committed_human_ai_funding()
-> TestResult {
    let c = connection().await?;
    for humans in [true, false] {
        let (f, m) = ante::reserve(&c, humans).await?;
        let coin = ante::fund(&c, f.source.actor).await?;
        if humans {
            ante::fund(&c, f.other).await?;
        }
        let rows = c
            .query(
                "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index",
                &[&m],
            )
            .await?;
        let a: Uuid = rows[0].get(0);
        let b: Uuid = rows[1].get(0);
        let first = ante::offer(
            &c,
            m,
            a,
            "0",
            json!([{"assetId":coin,"quantity":"10"}]),
            None,
        )
        .await?;
        let commitment = resource(&first);
        let second = ante::offer(
            &c,
            m,
            b,
            "1",
            if humans {
                json!([{"assetId":coin,"quantity":"10"}])
            } else {
                json!([])
            },
            if humans { None } else { Some("10") },
        )
        .await?;
        let hash = second["data"]["termsHash"].as_str().unwrap();
        ante::accept(&c, commitment, a, "2", hash).await?;
        ante::accept(&c, commitment, b, "2", hash).await?;
        let g: Uuid = c
            .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
            .await?
            .get(0);
        lifecycle::open(&c, m, g).await?;
        let paused:Value=c.query_one("SELECT app.pause_game_incident($1,'fixture:lifecycle',1,0,clock_timestamp(),'server_process','fixture:incident',$2)", &[&g,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(paused["status"], "success", "{paused}");
        let incident: Uuid = c
            .query_one(
                "SELECT incident_receipt_id FROM app.match WHERE id=$1",
                &[&m],
            )
            .await?
            .get(0);
        let (revision, availability) = basis(&c, m).await?;
        if !humans {
            c.batch_execute("BEGIN").await?;
        }
        let result = terminal(&c, m, &revision, &availability, incident).await?;
        if !humans {
            let slot:Uuid=c.query_one("SELECT id FROM app.room_slot WHERE room_id=$1 AND controller_kind='ai' AND retired_at IS NULL", &[&f.id]).await?.get(0);
            let waiting = connection().await?;
            let pid: i32 = waiting
                .query_one("SELECT pg_backend_pid()", &[])
                .await?
                .get(0);
            let task = tokio::spawn(async move {
                waiting.execute("INSERT INTO app.ai_runner_binding(room_slot_id,binding_epoch,desired_state,observed_state) VALUES($1,1,'running','pending')", &[&slot]).await
            });
            wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(
                task.await?.unwrap_err().code(),
                Some(&SqlState::CHECK_VIOLATION)
            );
        }

        assert_eq!(result["status"], "success", "{result}");
        let result_id: Uuid = result["data"]["terminalResultId"]
            .as_str()
            .unwrap()
            .parse()?;
        assert_eq!(
            c.query_one(
                "SELECT state FROM app.ante_commitment WHERE id=$1",
                &[&commitment]
            )
            .await?
            .get::<_, String>(0),
            "committed"
        );
        let refund: Value = c
            .query_one("SELECT app.settle_ante($1,$2)", &[&commitment, &result_id])
            .await?
            .get(0);
        assert_eq!(refund["status"], "success", "{refund}");
        assert!(
            spellwake_application::terminal_consumers::AnteSettlement {
                commitment_id: commitment,
                result_id
            }
            .decode_response(
                refund.to_string().as_bytes(),
                spellwake_application::ante::AnteState::Refunded
            )
            .is_ok()
        );
        assert_eq!(
            c.query_one(
                "SELECT state FROM app.ante_commitment WHERE id=$1",
                &[&commitment]
            )
            .await?
            .get::<_, String>(0),
            "refunded"
        );
        assert_eq!(
            c.query_one(
                "SELECT count(*)::bigint FROM app.game_event WHERE game_id=$1",
                &[&g]
            )
            .await?
            .get::<_, i64>(0),
            1
        );
        assert_eq!(
            c.query_one(
                "SELECT count(*)::bigint FROM app.game_result WHERE game_id=$1",
                &[&g]
            )
            .await?
            .get::<_, i64>(0),
            0
        );
        assert_eq!(
            c.query_one(
                "SELECT count(*)::bigint FROM app.game_participation WHERE game_id=$1",
                &[&g]
            )
            .await?
            .get::<_, i64>(0),
            0
        );
        let context: Uuid = c
            .query_one("SELECT id FROM app.game_context WHERE game_id=$1", &[&g])
            .await?
            .get(0);
        assert_eq!(c.execute("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES($1,$2,1,0,'fixture:late')", &[&g,&context]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
        assert_eq!(c.execute("INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision) VALUES($1,$2,1,'fixture:late','fixture:late',3)", &[&g,&context]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
        assert_eq!(c.execute("INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision) VALUES($1,$2,$3,3,true,'fabricated',clock_timestamp(),0)", &[&g,&a,&f.source.actor]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
        let initial: Uuid = c
            .query_one(
                "SELECT id FROM app.game_checkpoint WHERE game_id=$1 AND accepted_revision=0",
                &[&g],
            )
            .await?
            .get(0);
        let replay:Value=c.query_one("SELECT app.publish_game_replay($1,0,$2,0,'fixture:retained-prefix-archive','fixture:historical-visibility','fixture:codec-v1','fixture:qualified-retained-prefix')", &[&g,&initial]).await?.get(0);
        assert_eq!(replay["status"], "success", "{replay}");
        assert_eq!(c.query_one("SELECT app.publish_game_replay($1,0,$2,0,'fixture:retained-prefix-archive','fixture:historical-visibility','fixture:codec-v1','fixture:qualified-retained-prefix')", &[&g,&initial]).await?.get::<_,Value>(0),replay);
        if !humans {
            let binding:Uuid=c.query_one("SELECT runner.id FROM app.ai_runner_binding runner JOIN app.match_seat seat ON seat.id=runner.match_seat_id WHERE seat.match_id=$1", &[&m]).await?.get(0);
            assert_eq!(
                c.execute(
                    "UPDATE app.ai_runner_binding SET desired_state='running' WHERE id=$1",
                    &[&binding]
                )
                .await
                .unwrap_err()
                .code(),
                Some(&SqlState::CHECK_VIOLATION)
            );
            let grant: Value = c
                .query_one(
                    "SELECT app.issue_ai_grant($1,2,'fixture:forbidden-after-terminal',$2)",
                    &[&binding, &Uuid::new_v4()],
                )
                .await?
                .get(0);
            assert_eq!(grant["status"], "fail");
            let stop: Uuid = c
                .query_one(
                    "SELECT stop_receipt_id FROM app.ai_runner_binding WHERE id=$1",
                    &[&binding],
                )
                .await?
                .get(0);
            let observed: Value = c
                .query_one(
                    "SELECT app.observe_ai_stop($1,2,$2,'fixture:verified-stop',$3)",
                    &[&binding, &stop, &Uuid::new_v4()],
                )
                .await?
                .get(0);
            assert_eq!(observed["status"], "success");
            assert_eq!(
                c.query_one(
                    "SELECT lost_coins::text FROM app.ai_loss_bucket WHERE sponsor_id=$1",
                    &[&f.source.actor]
                )
                .await?
                .get::<_, String>(0),
                "0"
            );
            assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ai_runner_binding runner JOIN app.match_seat seat ON seat.id=runner.match_seat_id WHERE seat.match_id=$1 AND runner.desired_state='running'", &[&m]).await?.get::<_,i64>(0),0);
        }
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn between_game_failure_keeps_actual_win_history_and_cancels_decisions_once_under_retry_and_contention()
-> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve_match(&c, true, "bo3").await?;
    let g: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    lifecycle::open(&c, m, g).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 ORDER BY seat_index LIMIT 1",
            &[&m],
        )
        .await?
        .get(0);
    let game_result = lifecycle::complete(&c, m, g, vec![seat]).await?;
    lifecycle::consume(&c, m, game_result).await?;
    let (incident, revision, availability) = pause_between(&c, m).await?;
    c.batch_execute("BEGIN").await?;
    let rolled = terminal(&c, m, &revision, &availability, incident).await?;
    assert_eq!(rolled["status"], "success");
    c.batch_execute("ROLLBACK").await?;
    assert!(
        c.query_opt("SELECT id FROM app.match_result WHERE match_id=$1", &[&m])
            .await?
            .is_none()
    );
    c.batch_execute("BEGIN").await?;
    let accepted = terminal(&c, m, &revision, &availability, incident).await?;
    assert_eq!(accepted["status"], "success", "{accepted}");
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let basis = revision.clone();
    let av = availability.clone();
    let task = tokio::spawn(async move { terminal(&waiting, m, &basis, &av, incident).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??, accepted);
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game WHERE match_id=$1",
            &[&m]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.game_participation WHERE game_id=$1",
            &[&g]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    assert_eq!(
        c.query_one(
            "SELECT cardinality(winner_seat_ids) FROM app.game_result WHERE id=$1",
            &[&game_result]
        )
        .await?
        .get::<_, i32>(0),
        1
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.between_game_decision WHERE match_id=$1 AND state='canceled'", &[&m]).await?.get::<_,i64>(0),2);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.between_game_decision WHERE match_id=$1 AND state='expired'", &[&m]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn room_only_controller_parent_fence_serializes_both_orders_without_a_live_runner_index_conflict()
-> TestResult {
    let c = connection().await?;
    for binding_first in [true, false] {
        let (f, m) = ante::reserve(&c, false).await?;
        let g: Uuid = c
            .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
            .await?
            .get(0);
        lifecycle::open(&c, m, g).await?;
        let paused:Value=c.query_one("SELECT app.pause_game_incident($1,'fixture:lifecycle',1,0,clock_timestamp(),'storage','fixture:incident',$2)", &[&g,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(paused["status"], "success");
        let incident: Uuid = c
            .query_one(
                "SELECT incident_receipt_id FROM app.match WHERE id=$1",
                &[&m],
            )
            .await?
            .get(0);
        let (revision, availability) = basis(&c, m).await?;
        let row=c.query_one("SELECT b.id,b.room_slot_id FROM app.ai_runner_binding b JOIN app.match_seat seat ON seat.id=b.match_seat_id WHERE seat.match_id=$1", &[&m]).await?;
        let binding: Uuid = row.get(0);
        let slot: Uuid = row.get(1);
        c.query_one("SELECT app.request_ai_stop($1,$2)", &[&binding, &incident])
            .await?;
        if binding_first {
            c.batch_execute("BEGIN").await?;
            c.execute("INSERT INTO app.ai_runner_binding(room_slot_id,binding_epoch,desired_state,observed_state) VALUES($1,1,'running','pending')", &[&slot]).await?;
            c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await?;
            let waiting = connection().await?;
            let pid: i32 = waiting
                .query_one("SELECT pg_backend_pid()", &[])
                .await?
                .get(0);
            let rev = revision.clone();
            let av = availability.clone();
            let task =
                tokio::spawn(async move { terminal(&waiting, m, &rev, &av, incident).await });
            wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(
                task.await?.unwrap_err().code(),
                Some(&SqlState::T_R_SERIALIZATION_FAILURE)
            );
            assert_eq!(
                terminal(&c, m, &revision, &availability, incident).await?["status"],
                "success"
            );
        } else {
            c.batch_execute("BEGIN").await?;
            assert_eq!(
                terminal(&c, m, &revision, &availability, incident).await?["status"],
                "success"
            );
            let waiting = connection().await?;
            let pid: i32 = waiting
                .query_one("SELECT pg_backend_pid()", &[])
                .await?
                .get(0);
            let task = tokio::spawn(async move {
                waiting.batch_execute("BEGIN").await?;
                waiting.execute("INSERT INTO app.ai_runner_binding(room_slot_id,binding_epoch,desired_state,observed_state) VALUES($1,1,'running','pending')", &[&slot]).await?;
                waiting.batch_execute("SET CONSTRAINTS ALL IMMEDIATE").await
            });
            wait_for_database_lock(&c, pid).await?;
            c.batch_execute("SET CONSTRAINTS ALL IMMEDIATE; COMMIT")
                .await?;
            assert_eq!(
                task.await?.unwrap_err().code(),
                Some(&SqlState::CHECK_VIOLATION)
            );
        }
        assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE slot.room_id=$1 AND b.desired_state='running'", &[&f.id]).await?.get::<_,i64>(0),0);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn operational_no_contest_drains_rank_sequence_without_win_credit_or_rating_change()
-> TestResult {
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    c.execute("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) SELECT extract(year FROM now() AT TIME ZONE 'UTC')::integer,extract(quarter FROM now() AT TIME ZONE 'UTC')::integer,date_trunc('quarter',now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC',(date_trunc('quarter',now() AT TIME ZONE 'UTC')+interval '3 months') AT TIME ZONE 'UTC','fixture:rank-policy','active' ON CONFLICT(year,quarter) DO NOTHING", &[]).await?;
    let f = super::room_start_integrity::room_with_competition(&c, true, true).await?;
    let revision = super::room_start_integrity::basis(&c, f.id).await?;
    let countdown: Value = c
        .query_one(
            "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:deck-qualified',$4)",
            &[&f.source.actor, &f.id, &revision, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(countdown["status"], "success");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&f.id],
    )
    .await?;
    let started = super::room_start_integrity::start(&c, &f, Uuid::new_v4()).await?;
    let m = resource(&started);
    let (incident, revision, availability) = pause_between(&c, m).await?;
    assert_eq!(
        terminal(&c, m, &revision, &availability, incident).await?["status"],
        "success"
    );
    let result: Uuid = c
        .query_one("SELECT id FROM app.match_result WHERE match_id=$1", &[&m])
        .await?
        .get(0);
    let consumed: Value = c
        .query_one("SELECT app.consume_ranked_match_result($1)", &[&result])
        .await?
        .get(0);
    assert_eq!(consumed["status"], "success", "{consumed}");
    assert_eq!(
        c.query_one("SELECT app.consume_ranked_match_result($1)", &[&result])
            .await?
            .get::<_, Value>(0),
        consumed
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.competitive_result WHERE match_result_id=$1 AND NOT win AND rating_delta=0 AND rating_before=rating_after", &[&result]).await?.get::<_,i64>(0),2);
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.daily_win_bucket WHERE account_id=ANY($1)",
            &[&vec![f.source.actor, f.other]]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert!(c.query_one("SELECT f.last_ranked_sequence=r.rank_sequence FROM app.match_result r JOIN app.format f ON f.id=r.ranking_format_id WHERE r.id=$1", &[&result]).await?.get::<_,bool>(0));
    Ok(())
}
