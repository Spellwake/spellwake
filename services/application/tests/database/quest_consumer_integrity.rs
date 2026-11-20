//! Qualified-shape synthetic counter facts test SQL consumption, not native objective semantics.
use super::{TestResult, connection, game_scope, wait_for_database_lock};
use serde_json::Value;
use tokio_postgres::Client;
use uuid::Uuid;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(super) async fn fixture(c: &Client, mode: &str) -> FixtureResult<(game_scope::Seed, Uuid)> {
    let f = game_scope::seed(c).await?;
    c.execute(
        "UPDATE app.match SET mode=$2 WHERE id=$1",
        &[&f.match_id, &mode],
    )
    .await?;
    let definition:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) VALUES($1,'fixture:v1','fixture:objective',3,500,'fixture:casts','fixture:v1','fixture:normalized') RETURNING id", &[&Uuid::new_v4().to_string()]).await?.get(0);
    let day: String = c
        .query_one(
            "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD')",
            &[],
        )
        .await?
        .get(0);
    let generated: Value = c
        .query_one(
            "SELECT app.generate_daily_quest($1,$2,$3,$4)",
            &[&f.account, &definition, &day, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(generated["status"], "success");
    let assignment: Uuid = generated["data"]["resourceId"].as_str().unwrap().parse()?;
    // An older event is not made eligible by inserting its fact after assignment.
    c.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'fixture:casts',100,'fixture:v1')", &[&f.game,&f.event,&f.account]).await?;
    for (sequence, value) in [(1i64, 1i64), (2, 2)] {
        let event:Uuid=c.query_one("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES($1,$2,$3::bigint,0,'fixture') RETURNING id", &[&f.game,&f.context,&sequence]).await?.get(0);
        c.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'fixture:casts',$4::bigint,'fixture:v1')", &[&f.game,&event,&f.account,&value]).await?;
    }
    Ok((f, assignment))
}
pub(super) async fn consume(
    c: &Client,
    g: Uuid,
    limit: i32,
) -> Result<Value, tokio_postgres::Error> {
    let response: Value = c
        .query_one(
            "SELECT app.consume_quest_game_activity($1,$2)",
            &[&g, &limit],
        )
        .await?
        .get(0);
    let cursor:Uuid=c.query_one("SELECT id FROM app.activity_consumer_cursor WHERE consumer_key='quests-v1' AND game_id=$1",&[&g]).await?.get(0);
    assert!(
        spellwake_application::terminal_consumers::QuestActivityBatch {
            game_id: g,
            limit: limit.try_into().unwrap()
        }
        .decode_response(response.to_string().as_bytes(), Some(cursor))
        .is_ok()
    );
    Ok(response)
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn collection_counter_progress_reward_and_cursor_commit_together_after_retry_and_competing_consumers()
-> TestResult {
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
    let (f, assignment) = fixture(&c, "collection").await?;
    assert_eq!(consume(&c, f.game, 1).await?["data"]["rewards"], "0");
    assert_eq!(
        c.query_one(
            "SELECT progress::text FROM app.quest_assignment WHERE id=$1",
            &[&assignment]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    consume(&c, f.game, 1).await?;
    assert_eq!(
        c.query_one(
            "SELECT progress::text FROM app.quest_assignment WHERE id=$1",
            &[&assignment]
        )
        .await?
        .get::<_, String>(0),
        "1"
    );
    c.batch_execute("BEGIN").await?;
    assert_eq!(consume(&c, f.game, 1).await?["data"]["rewards"], "1");
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(
        c.query_one(
            "SELECT progress::text FROM app.quest_assignment WHERE id=$1",
            &[&assignment]
        )
        .await?
        .get::<_, String>(0),
        "1"
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.ledger_operation WHERE source_scope='quest_reward_v1' AND source_ref=$1", &[&assignment.to_string()]).await?.get::<_,i64>(0),0);
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&f.account]).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    assert_eq!(consume(&c, f.game, 1).await?["data"]["rewards"], "1");
    let game = f.game;
    let task = tokio::spawn(async move { consume(&waiting, game, 100).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(task.await??["data"]["rewards"], "0");
    assert_eq!(consume(&c, f.game, 100).await?["data"]["events"], "0");
    let row=c.query_one("SELECT q.progress::text,q.completed_at=e.accepted_at,o.state,(SELECT sum(i.owned_quantity)::text FROM app.inventory_asset i JOIN app.asset_owner owner ON owner.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE owner.account_id=q.account_id AND d.currency_key='coin') FROM app.quest_assignment q JOIN app.game_activity_fact f ON f.id=q.reward_source_fact_id JOIN app.game_event e ON e.id=f.source_event_id JOIN app.ledger_operation o ON o.id=q.reward_operation_id WHERE q.id=$1", &[&assignment]).await?;
    assert_eq!(row.get::<_, String>(0), "3");
    assert!(row.get::<_, bool>(1));
    assert_eq!(row.get::<_, String>(2), "committed");
    assert_eq!(row.get::<_, String>(3), "500");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn open_play_never_advances_the_same_counter_objective_but_its_cursor_still_advances()
-> TestResult {
    let c = connection().await?;
    let (f, assignment) = fixture(&c, "open").await?;
    let first = consume(&c, f.game, 100).await?;
    assert_eq!(first["data"]["events"], "3");
    assert_eq!(first["data"]["rewards"], "0");
    assert_eq!(consume(&c, f.game, 100).await?["data"]["events"], "0");
    assert_eq!(
        c.query_one(
            "SELECT progress::text FROM app.quest_assignment WHERE id=$1",
            &[&assignment]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    assert!(c.execute("UPDATE app.activity_consumer_cursor SET last_sequence=0,revision=revision+1 WHERE consumer_key='quests-v1' AND game_id=$1", &[&f.game]).await.is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn consumed_prefix_rejects_late_facts_after_an_actual_writer_wait_and_never_accepts_event_gaps()
-> TestResult {
    let c = connection().await?;
    let (f, _) = fixture(&c, "open").await?;
    consume(&c, f.game, 100).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    consume(&c, f.game, 100).await?;
    let game = f.game;
    let event = f.event;
    let actor = f.account;
    let task = tokio::spawn(async move {
        waiting.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'fixture:late',1,'fixture:v1')", &[&game,&event,&actor]).await
    });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
    );
    assert!(c.execute("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES($1,$2,5,0,'fixture:gap')", &[&f.game,&f.context]).await.is_err());
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.game_activity_fact WHERE game_id=$1 AND kind='fixture:late'", &[&f.game]).await?.get::<_,i64>(0),0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn reroll_and_completion_serialize_and_a_new_750_objective_never_reuses_older_facts()
-> TestResult {
    for reroll_wins in [false, true] {
        let c = connection().await?;
        c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING", &[]).await?;
        let (f, assignment) = fixture(&c, "collection").await?;
        let definition:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) VALUES($1,'fixture:v1','fixture:objective',3,750,'fixture:casts','fixture:v1','fixture:normalized') RETURNING id", &[&Uuid::new_v4().to_string()]).await?.get(0);
        let day: String = c
            .query_one(
                "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD')",
                &[],
            )
            .await?
            .get(0);
        let waiting = connection().await?;
        let pid: i32 = waiting
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        c.batch_execute("BEGIN").await?;
        if reroll_wins {
            let rerolled: Value = c
                .query_one(
                    "SELECT app.reroll_daily_quest($1,$2,$3,$4,$5)",
                    &[&f.account, &assignment, &definition, &day, &Uuid::new_v4()],
                )
                .await?
                .get(0);
            assert_eq!(rerolled["status"], "success");
            let game = f.game;
            let task = tokio::spawn(async move { consume(&waiting, game, 100).await });
            wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["data"]["rewards"], "0");
            let replacement: Uuid = rerolled["data"]["resourceId"].as_str().unwrap().parse()?;
            assert_eq!(
                c.query_one(
                    "SELECT progress::text FROM app.quest_assignment WHERE id=$1",
                    &[&replacement]
                )
                .await?
                .get::<_, String>(0),
                "0"
            );
            let event:Uuid=c.query_one("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES($1,$2,3,0,'fixture:new') RETURNING id", &[&f.game,&f.context]).await?.get(0);
            c.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'fixture:casts',3,'fixture:v1')", &[&f.game,&event,&f.account]).await?;
            assert_eq!(consume(&c, f.game, 100).await?["data"]["rewards"], "1");
            assert_eq!(c.query_one("SELECT sum(i.owned_quantity)::text FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE o.account_id=$1 AND d.currency_key='coin'", &[&f.account]).await?.get::<_,String>(0),"750");
        } else {
            assert_eq!(consume(&c, f.game, 100).await?["data"]["rewards"], "1");
            let actor = f.account;
            let task = tokio::spawn(async move {
                waiting
                    .query_one(
                        "SELECT app.reroll_daily_quest($1,$2,$3,$4,$5)",
                        &[&actor, &assignment, &definition, &day, &Uuid::new_v4()],
                    )
                    .await
                    .map(|r| r.get::<_, Value>(0))
            });
            wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "fail");
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn one_accepted_event_can_complete_two_participants_objectives() -> TestResult {
    let c = connection().await?;
    c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING",&[]).await?;
    let f = game_scope::seed(&c).await?;
    c.execute(
        "UPDATE app.match SET mode='collection' WHERE id=$1",
        &[&f.match_id],
    )
    .await?;
    let peer = super::account(&c).await?.0;
    c.execute("INSERT INTO app.match_seat(match_id,account_id,seat_index,controller_kind) VALUES($1,$2,1,'human')",&[&f.match_id,&peer]).await?;
    let definition:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) VALUES($1,'fixture:v1','fixture:objective',1,500,'fixture:casts','fixture:v1','fixture:normalized') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    let day: String = c
        .query_one(
            "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD')",
            &[],
        )
        .await?
        .get(0);
    for actor in [f.account, peer] {
        let response: Value = c
            .query_one(
                "SELECT app.generate_daily_quest($1,$2,$3,$4)",
                &[&actor, &definition, &day, &Uuid::new_v4()],
            )
            .await?
            .get(0);
        assert_eq!(response["status"], "success");
    }
    assert_eq!(consume(&c, f.game, 1).await?["data"]["rewards"], "0");
    let event:Uuid=c.query_one("INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES($1,$2,1,0,'fixture') RETURNING id",&[&f.game,&f.context]).await?.get(0);
    for actor in [f.account, peer] {
        c.execute("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'fixture:casts',1,'fixture:v1')",&[&f.game,&event,&actor]).await?;
    }
    let response = consume(&c, f.game, 1).await?;
    assert_eq!(response["data"]["events"], "1");
    assert_eq!(response["data"]["rewards"], "2");
    assert_eq!(consume(&c, f.game, 1).await?["data"]["rewards"], "0");
    Ok(())
}
