//! Real normalized lifecycle drainage before SQL retirement; native engine truth remains separate.
use super::{
    TestResult, connection, deletion, match_lifecycle_integrity as lifecycle, room_start_integrity,
};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn retire(c: &Client, actor: Uuid) -> FixtureResult<Value> {
    Ok(c.query_one("SELECT app.retire_account($1)", &[&actor])
        .await?
        .get(0))
}
async fn append_late_event(c: &Client, g: Uuid) -> Result<u64, tokio_postgres::Error> {
    c.execute("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) SELECT g.id,context.id,(SELECT max(sequence)+1 FROM app.game_event WHERE game_id=g.id),g.accepted_revision,'fixture:forbidden-late-tail' FROM app.game g JOIN app.game_context context ON context.game_id=g.id WHERE g.id=$1", &[&g]).await
}
async fn discard_unadmitted_fixture(c: &Client, m: Uuid) -> TestResult {
    // The admission helper seeds a separate, unused raw draft. It is not a played match.
    c.execute("DELETE FROM app.match_seat WHERE match_id=$1", &[&m])
        .await?;
    c.execute("DELETE FROM app.match WHERE id=$1", &[&m])
        .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn whole_match_and_quest_frontiers_drain_before_retirement_while_history_and_future_rank_remain()
-> TestResult {
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    c.execute("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) SELECT extract(year FROM now() AT TIME ZONE 'UTC')::integer,extract(quarter FROM now() AT TIME ZONE 'UTC')::integer,date_trunc('quarter',now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC',(date_trunc('quarter',now() AT TIME ZONE 'UTC')+interval '3 months') AT TIME ZONE 'UTC','fixture:rank-policy','active' ON CONFLICT(year,quarter) DO NOTHING", &[]).await?;
    for competitive in [false, true] {
        let f = room_start_integrity::room_with_competition(&c, true, competitive).await?;
        discard_unadmitted_fixture(&c, f.source.match_id).await?;
        let basis = room_start_integrity::basis(&c, f.id).await?;
        let countdown: Value = c
            .query_one(
                "SELECT app.start_room_countdown($1,$2,$3::text::numeric,'fixture:decks',$4)",
                &[&f.source.actor, &f.id, &basis, &Uuid::new_v4()],
            )
            .await?
            .get(0);
        assert_eq!(countdown["status"], "success");
        c.execute(
            "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
            &[&f.id],
        )
        .await?;
        let started = room_start_integrity::start(&c, &f, Uuid::new_v4()).await?;
        assert_eq!(started["status"], "success");
        let m: Uuid = started["data"]["resourceId"].as_str().unwrap().parse()?;
        let g: Uuid = started["data"]["gameId"].as_str().unwrap().parse()?;
        assert_eq!(
            deletion::request(&c, f.source.actor, Uuid::new_v4()).await?["status"],
            "success"
        );
        assert_eq!(
            retire(&c, f.source.actor).await?["data"]["state"],
            "waiting"
        );
        assert_eq!(lifecycle::open(&c, m, g).await?["status"], "success");
        let winner: Uuid = c
            .query_one(
                "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
                &[&m, &f.source.actor],
            )
            .await?
            .get(0);
        let result = lifecycle::complete(&c, m, g, vec![winner]).await?;
        assert!(
            c.query_opt("SELECT id FROM app.replay_manifest WHERE game_id=$1", &[&g])
                .await?
                .is_none()
        );
        assert_eq!(
            append_late_event(&c, g).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        assert_eq!(
            retire(&c, f.source.actor).await?["data"]["state"],
            "waiting"
        );
        assert_eq!(
            lifecycle::consume(&c, m, result).await?["status"],
            "success"
        );
        assert_eq!(
            retire(&c, f.source.actor).await?["data"]["state"],
            "waiting"
        );
        if !competitive {
            c.batch_execute("BEGIN").await?;
        }
        assert_eq!(
            c.query_one("SELECT app.consume_quest_game_activity($1,100)", &[&g])
                .await?
                .get::<_, Value>(0)["status"],
            "success"
        );
        if competitive {
            assert_eq!(
                retire(&c, f.source.actor).await?["data"]["state"],
                "waiting"
            );
            let match_result: Uuid = c
                .query_one("SELECT id FROM app.match_result WHERE match_id=$1", &[&m])
                .await?
                .get(0);
            assert_eq!(
                c.query_one(
                    "SELECT app.consume_ranked_match_result($1)",
                    &[&match_result]
                )
                .await?
                .get::<_, Value>(0)["status"],
                "success"
            );
        }
        let retired = retire(&c, f.source.actor).await?;
        assert_eq!(retired["data"]["state"], "retired", "{retired}");
        assert_eq!(retire(&c, f.source.actor).await?, retired);
        if !competitive {
            let late = connection().await?;
            let pid: i32 = late.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
            let pending = tokio::spawn(async move { append_late_event(&late, g).await });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(
                pending.await?.unwrap_err().code(),
                Some(&SqlState::CHECK_VIOLATION)
            );
        }
        assert_eq!(
            append_late_event(&c, g).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
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
                "SELECT count(*)::bigint FROM app.match_seat WHERE match_id=$1",
                &[&m]
            )
            .await?
            .get::<_, i64>(0),
            2
        );
        assert!(
            c.query_one(
                "SELECT state='active' FROM app.account WHERE id=$1",
                &[&f.other]
            )
            .await?
            .get::<_, bool>(0)
        );
        if competitive {
            assert!(
                c.query_one(
                    "SELECT EXISTS(SELECT 1 FROM app.rank WHERE account_id=$1 AND NOT finalized)",
                    &[&f.source.actor]
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
async fn terminal_ai_match_waits_for_proven_stop_but_retained_binding_history_does_not_block()
-> TestResult {
    let c = connection().await?;
    let (f, m) = super::ante_transaction_integrity::reserve(&c, false).await?;
    discard_unadmitted_fixture(&c, f.source.match_id).await?;
    assert_eq!(
        deletion::request(&c, f.source.actor, Uuid::new_v4()).await?["status"],
        "success"
    );
    let paused:Value=c.query_one("SELECT app.pause_match_between_games($1,(SELECT revision FROM app.match WHERE id=$1),(SELECT availability_revision FROM app.match WHERE id=$1),clock_timestamp(),'storage','fixture:incident',$2)", &[&m,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(paused["status"], "success");
    let incident: Uuid = paused["data"]["incidentId"].as_str().unwrap().parse()?;
    let terminal:Value=c.query_one("SELECT app.finalize_unrecoverable_match($1,(SELECT revision FROM app.match WHERE id=$1),(SELECT availability_revision FROM app.match WHERE id=$1),$2,'fixture:failed-restoration')", &[&m,&incident]).await?.get(0);
    assert_eq!(terminal["status"], "success");
    assert_eq!(
        retire(&c, f.source.actor).await?["data"]["state"],
        "waiting"
    );
    let b=c.query_one("SELECT b.id,b.binding_epoch::text,b.stop_receipt_id FROM app.ai_runner_binding b JOIN app.match_seat s ON s.id=b.match_seat_id WHERE s.match_id=$1", &[&m]).await?;
    let binding: Uuid = b.get(0);
    let epoch: String = b.get(1);
    let stop: Uuid = b.get(2);
    let observed:Value=c.query_one("SELECT app.observe_ai_stop($1,$2::text::numeric,$3,'fixture:qualified-generation-stop',$4)", &[&binding,&epoch,&stop,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(observed["status"], "success");
    let retired = retire(&c, f.source.actor).await?;
    assert_eq!(retired["data"]["state"], "retired", "{retired}");
    assert!(
        c.query_opt(
            "SELECT id FROM app.ai_runner_binding WHERE id=$1",
            &[&binding]
        )
        .await?
        .is_some()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deferred_terminal_result_cannot_seal_an_earlier_event_and_leave_an_unconsumed_tail()
-> TestResult {
    let c = connection().await?;
    let f = super::game_scope::seed(&c).await?;
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) SELECT game_id,context_id,1,accepted_revision,'fixture:later-than-proposed-terminal' FROM app.game_event WHERE id=$1", &[&f.event]).await?;
    c.execute(
        "UPDATE app.game SET state='completed' WHERE id=$1",
        &[&f.game],
    )
    .await?;
    c.execute("INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason,winner_seat_ids) VALUES($1,$2,'draw','fixture:none','fixture',ARRAY[]::uuid[])", &[&f.game,&f.event]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert!(
        c.query_opt(
            "SELECT id FROM app.game_result WHERE game_id=$1",
            &[&f.game]
        )
        .await?
        .is_none()
    );
    Ok(())
}
