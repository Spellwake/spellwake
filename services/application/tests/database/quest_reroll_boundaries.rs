//! Actual session/assignment transactions over synthetic qualified objectives.
//! Fixture qualification references do not establish native objective semantics.
use super::{TestResult, account, connection, wait_for_database_lock};
use serde_json::{Value, json};
use tokio_postgres::{Client, GenericClient, error::SqlState};
use uuid::Uuid;

/// Serialize only these tests' operational pool edits; spawned transaction clients
/// deliberately do not take this fixture-only lock.
async fn pool_connection() -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
    let c = connection().await?;
    c.query_one(
        "SELECT pg_advisory_lock(hashtextextended('fixture:reroll-enabled-pool',0))",
        &[],
    )
    .await?;
    Ok(c)
}
async fn definition<C: GenericClient + Sync>(
    c: &C,
    key: &str,
    revision: &str,
) -> Result<Uuid, tokio_postgres::Error> {
    Ok(c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) VALUES($1,$2,'fixture:objective',3,500,'fixture:casts','fixture:v1','fixture:qualified') RETURNING id", &[&key,&revision]).await?.get(0))
}
async fn assignment<C: GenericClient + Sync>(
    c: &C,
    actor: Uuid,
    definition: Uuid,
) -> Result<Uuid, tokio_postgres::Error> {
    Ok(c.query_one("INSERT INTO app.quest_assignment(account_id,definition_id,source_day,assignment_ref,progress) VALUES($1,$2,'2026-01-01',$3,1) RETURNING id", &[&actor,&definition,&Uuid::new_v4().to_string()]).await?.get(0))
}
async fn session(c: &Client, subject: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    Ok(c.query_one(
        "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
        &[&subject],
    )
    .await?
    .get(0))
}
async fn call<C: GenericClient + Sync>(
    c: &C,
    subject: Uuid,
    session: Uuid,
    request: Uuid,
    assignment: Uuid,
    lookup: bool,
) -> Result<Value, tokio_postgres::Error> {
    let sql = if lookup {
        "SELECT app.read_quest_reroll_command($1,$2,$3,$4)"
    } else {
        "SELECT app.reroll_quest_command($1,$2,$3,$4)"
    };
    Ok(
        c.query_one(sql, &[&subject, &session, &request, &assignment])
            .await?
            .get(0),
    )
}
async fn setup(
    c: &Client,
) -> Result<(Uuid, Uuid, Uuid, Uuid), Box<dyn std::error::Error + Send + Sync>> {
    let (actor, subject) = account(c).await?;
    let original = definition(c, &Uuid::new_v4().to_string(), "old").await?;
    let target = assignment(c, actor, original).await?;
    Ok((actor, subject, session(c, subject).await?, target))
}
fn accepted(value: &Value, target: Uuid) -> Uuid {
    assert_eq!(value["status"], "success");
    let ack: spellwake_application::qualified_intents::QuestRerollAck =
        serde_json::from_value(value["data"].clone()).unwrap();
    assert_eq!(ack.replaced_id, target);
    assert_ne!(ack.resource_id, target);
    assert_eq!(value["data"].as_object().unwrap().len(), 2);
    ack.resource_id
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn enabled_pool_is_explicit_qualified_unique_and_content_stays_immutable() -> TestResult {
    let c = pool_connection().await?;
    let key = Uuid::new_v4().to_string();
    let first = definition(&c, &key, "one").await?;
    let second = definition(&c, &key, "two").await?;
    assert!(
        !c.query_one(
            "SELECT reroll_enabled FROM app.quest_definition_revision WHERE id=$1",
            &[&first]
        )
        .await?
        .get::<_, bool>(0)
    );
    c.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id=$1",
        &[&first],
    )
    .await?;
    assert_eq!(
        c.execute(
            "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id=$1",
            &[&second]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::UNIQUE_VIOLATION)
    );
    for sql in [
        "UPDATE app.quest_definition_revision SET reward_coins=750 WHERE id=$1",
        "UPDATE app.quest_definition_revision SET objective_qualification_ref='other',reroll_enabled=false WHERE id=$1",
        "DELETE FROM app.quest_definition_revision WHERE id=$1",
    ] {
        assert_eq!(
            c.execute(sql, &[&first]).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    let draft:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins) VALUES($1,'draft','fixture:draft',3,500) RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    assert_eq!(
        c.execute(
            "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id=$1",
            &[&draft]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=false WHERE id=$1",
        &[&first],
    )
    .await?;
    c.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id=$1",
        &[&second],
    )
    .await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        let row=c.query_one("SELECT has_function_privilege($1::name,'app.apply_quest_reroll(uuid,uuid,uuid,uuid,text)','EXECUTE'),has_column_privilege($1::name,'app.quest_definition_revision','reroll_enabled','UPDATE'),has_function_privilege($1::name,'app.reroll_quest_command(uuid,uuid,uuid,uuid)','EXECUTE'),has_function_privilege($1::name,'app.read_quest_reroll_command(uuid,uuid,uuid,uuid)','EXECUTE')",&[&role]).await?;
        assert!(!row.get::<_, bool>(0));
        assert!(!row.get::<_, bool>(1));
        assert_eq!(row.get::<_, bool>(2), role == "service_role");
        assert_eq!(row.get::<_, bool>(3), role == "service_role");
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn original_intent_replays_after_pool_changes_and_conflicts_never_reselect() -> TestResult {
    let mut c = pool_connection().await?;
    let (actor, subject, session, target) = setup(&c).await?;
    let tx = c.transaction().await?;
    tx.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=false WHERE reroll_enabled",
        &[],
    )
    .await?;
    let old_key:String=tx.query_one("SELECT d.definition_key FROM app.quest_assignment q JOIN app.quest_definition_revision d ON d.id=q.definition_id WHERE q.id=$1",&[&target]).await?.get(0);
    let same = definition(&tx, &old_key, "new").await?;
    let selected = definition(&tx, &Uuid::new_v4().to_string(), "selected").await?;
    tx.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id IN($1,$2)",
        &[&same, &selected],
    )
    .await?;
    let request = Uuid::new_v4();
    assert_eq!(
        call(&tx, subject, session, request, target, true).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    let result = call(&tx, subject, session, request, target, false).await?;
    let replacement = accepted(&result, target);
    let row = tx
        .query_one(
            "SELECT definition_id,progress::text FROM app.quest_assignment WHERE id=$1",
            &[&replacement],
        )
        .await?;
    assert_eq!(row.get::<_, Uuid>(0), selected);
    assert_eq!(row.get::<_, String>(1), "0");
    let source:Value=tx.query_one("SELECT normalized_source FROM app.command_receipt WHERE principal_key='human:'||$1::uuid::text AND scope='quest_reroll_intent_v1' AND request_ref=$2::uuid::text",&[&actor,&request]).await?.get(0);
    assert_eq!(source["definitionId"], selected.to_string());
    assert_eq!(source["version"], "quest-reroll-equal-v1");
    tx.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=false WHERE reroll_enabled",
        &[],
    )
    .await?;
    assert_eq!(
        call(&tx, subject, session, request, target, false).await?,
        result
    );
    assert_eq!(
        call(&tx, subject, session, request, target, true).await?,
        json!({"status":"success","data":{"state":"accepted","response":result}})
    );
    for lookup in [false, true] {
        assert_eq!(
            call(&tx, subject, session, request, Uuid::new_v4(), lookup).await?,
            json!({"status":"fail","data":{"request":"conflict"}})
        );
    }
    let rejection = Uuid::new_v4();
    let rejected = call(&tx, subject, session, rejection, target, false).await?;
    assert_eq!(
        rejected,
        json!({"status":"fail","data":{"assignment":"unavailable"}})
    );
    assert_eq!(
        call(&tx, subject, session, rejection, target, false).await?,
        rejected
    );
    assert_eq!(
        call(&tx, subject, session, rejection, target, true).await?,
        json!({"status":"success","data":{"state":"rejected","response":rejected}})
    );
    assert_eq!(
        tx.query_one(
            "SELECT count(*)::bigint FROM app.quest_assignment WHERE account_id=$1",
            &[&actor]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn no_enabled_replacement_rolls_back_receipt_and_daily_allowance() -> TestResult {
    let mut c = pool_connection().await?;
    let (actor, subject, session, target) = setup(&c).await?;
    let tx = c.transaction().await?;
    tx.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=false WHERE reroll_enabled",
        &[],
    )
    .await?;
    let request = Uuid::new_v4();
    tx.batch_execute("SAVEPOINT no_pool").await?;
    assert_eq!(
        call(&tx, subject, session, request, target, false)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE)
    );
    tx.batch_execute("ROLLBACK TO SAVEPOINT no_pool").await?;
    assert_eq!(
        call(&tx, subject, session, request, target, true).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    let unchanged=tx.query_one("SELECT (SELECT count(*) FROM app.daily_quest_state WHERE account_id=$1)::bigint,(SELECT count(*) FROM app.quest_assignment WHERE account_id=$1)::bigint",&[&actor]).await?;
    assert_eq!(unchanged.get::<_, i64>(0), 0);
    assert_eq!(unchanged.get::<_, i64>(1), 1);
    let selected = definition(&tx, &Uuid::new_v4().to_string(), "enabled").await?;
    tx.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id=$1",
        &[&selected],
    )
    .await?;
    accepted(
        &call(&tx, subject, session, request, target, false).await?,
        target,
    );
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn concurrent_same_intent_reuses_one_replacement_and_allowance() -> TestResult {
    let c = pool_connection().await?;
    let (actor, subject, session, target) = setup(&c).await?;
    let selected = definition(&c, &Uuid::new_v4().to_string(), "enabled").await?;
    c.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id=$1",
        &[&selected],
    )
    .await?;
    let first = connection().await?;
    let second = connection().await?;
    let request = Uuid::new_v4();
    let blocker = connection().await?;
    blocker.batch_execute("BEGIN").await?;
    blocker
        .query_one(
            "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
            &[&actor],
        )
        .await?;
    let first_pid: i32 = first
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let second_pid: i32 = second
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    let a =
        tokio::spawn(async move { call(&first, subject, session, request, target, false).await });
    let b =
        tokio::spawn(async move { call(&second, subject, session, request, target, false).await });
    wait_for_database_lock(&c, first_pid).await?;
    wait_for_database_lock(&c, second_pid).await?;
    blocker.batch_execute("COMMIT").await?;
    let a = a.await??;
    assert_eq!(a, b.await??);
    accepted(&a, target);
    let row=c.query_one("SELECT (SELECT count(*) FROM app.quest_assignment WHERE account_id=$1)::bigint,(SELECT rerolls_used::text FROM app.daily_quest_state WHERE account_id=$1), (SELECT generated_assignment_id FROM app.daily_quest_state WHERE account_id=$1)",&[&actor]).await?;
    assert_eq!(row.get::<_, i64>(0), 2);
    assert_eq!(row.get::<_, String>(1), "1");
    assert_eq!(row.get::<_, Option<Uuid>>(2), None);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn authority_is_refreshed_after_account_wait_before_any_receipt_disclosure() -> TestResult {
    for lookup in [false, true] {
        let mut c = pool_connection().await?;
        let (actor, subject, session, target) = setup(&c).await?;
        let tx = c.transaction().await?;
        tx.query_one(
            "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
            &[&actor],
        )
        .await?;
        let waiter = connection().await?;
        let pid: i32 = waiter
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let task = tokio::spawn(async move {
            call(&waiter, subject, session, Uuid::new_v4(), target, lookup).await
        });
        let monitor = connection().await?;
        wait_for_database_lock(&monitor, pid).await?;
        tx.execute(
            "UPDATE auth.sessions SET not_after=clock_timestamp()-interval '1 second' WHERE id=$1",
            &[&session],
        )
        .await?;
        tx.commit().await?;
        assert_eq!(
            task.await??,
            json!({"status":"fail","data":{"session":"unauthorized"}})
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn foreign_pending_and_invalid_intents_cannot_hide_or_change_original_receipts() -> TestResult
{
    let c = pool_connection().await?;
    let (actor, subject, session, target) = setup(&c).await?;
    let (_, other, other_session, _) = setup(&c).await?;
    let selected = definition(&c, &Uuid::new_v4().to_string(), "enabled").await?;
    c.execute(
        "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id=$1",
        &[&selected],
    )
    .await?;
    let original: Uuid = c
        .query_one(
            "SELECT definition_id FROM app.quest_assignment WHERE id=$1",
            &[&target],
        )
        .await?
        .get(0);
    assignment(&c, actor, original).await?;
    assignment(&c, actor, original).await?;
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    let result = call(&c, subject, session, request, target, false).await?;
    c.batch_execute("RESET ROLE").await?;
    accepted(&result, target);
    let bank=c.query_one("SELECT count(*)::bigint FROM app.quest_assignment WHERE account_id=$1 AND completed_at IS NULL AND replaced_by_id IS NULL",&[&actor]).await?;
    assert_eq!(bank.get::<_, i64>(0), 3);
    assert_eq!(
        call(&c, other, other_session, request, target, true).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    assert_eq!(
        call(&c, other, other_session, request, target, false).await?,
        json!({"status":"fail","data":{"assignment":"unavailable"}})
    );
    for lookup in [false, true] {
        assert_eq!(
            call(&c, subject, other_session, request, target, lookup).await?,
            json!({"status":"fail","data":{"session":"unauthorized"}})
        );
        for (request, target) in [(Uuid::nil(), target), (request, Uuid::nil())] {
            assert_eq!(
                call(&c, subject, session, request, target, lookup).await?,
                json!({"status":"fail","data":{"request":"invalid"}})
            );
        }
    }
    let rejected_request = Uuid::new_v4();
    let missing = Uuid::new_v4();
    let rejected = call(&c, subject, session, rejected_request, missing, false).await?;
    assert_eq!(
        rejected,
        json!({"status":"fail","data":{"assignment":"unavailable"}})
    );
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&actor]).await?;
    assert_eq!(
        call(&c, subject, session, request, target, false).await?,
        result
    );
    assert_eq!(
        call(&c, subject, session, request, target, true).await?,
        json!({"status":"success","data":{"state":"accepted","response":result}})
    );
    assert_eq!(
        call(&c, subject, session, rejected_request, missing, false).await?,
        rejected
    );
    assert_eq!(
        call(&c, subject, session, rejected_request, missing, true).await?,
        json!({"status":"success","data":{"state":"rejected","response":rejected}})
    );
    assert_eq!(
        call(&c, subject, session, Uuid::new_v4(), missing, false).await?,
        json!({"status":"fail","data":{"account":"unavailable"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn distinct_facade_and_private_intents_compete_for_one_allowance_under_actual_lock_waits()
-> TestResult {
    for legacy in [false, true] {
        let c = pool_connection().await?;
        let (actor, subject, session, target) = setup(&c).await?;
        let original: Uuid = c
            .query_one(
                "SELECT definition_id FROM app.quest_assignment WHERE id=$1",
                &[&target],
            )
            .await?
            .get(0);
        let other = assignment(&c, actor, original).await?;
        let selected = definition(&c, &Uuid::new_v4().to_string(), "enabled").await?;
        c.execute(
            "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id=$1",
            &[&selected],
        )
        .await?;
        let blocker = connection().await?;
        blocker.batch_execute("BEGIN").await?;
        blocker
            .query_one(
                "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
                &[&actor],
            )
            .await?;
        let first = connection().await?;
        let second = connection().await?;
        let first_pid: i32 = first
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let second_pid: i32 = second
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let a = tokio::spawn(async move {
            call(&first, subject, session, Uuid::new_v4(), target, false).await
        });
        let b = tokio::spawn(async move {
            if legacy {
                second.query_one("SELECT app.reroll_daily_quest($1,$2,$3,to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD'),$4)",&[&actor,&other,&selected,&Uuid::new_v4()]).await.map(|r|r.get::<_,Value>(0))
            } else {
                call(&second, subject, session, Uuid::new_v4(), other, false).await
            }
        });
        wait_for_database_lock(&c, first_pid).await?;
        wait_for_database_lock(&c, second_pid).await?;
        blocker.batch_execute("COMMIT").await?;
        let results = [a.await??, b.await??];
        assert_eq!(
            results.iter().filter(|v| v["status"] == "success").count(),
            1
        );
        assert!(results.contains(&json!({"status":"fail","data":{"reroll":"already used"}})));
        assert_eq!(
            c.query_one(
                "SELECT count(*)::bigint FROM app.quest_assignment WHERE account_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, i64>(0),
            3
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn facade_and_actual_objective_consumer_preserve_both_completion_orders() -> TestResult {
    for facade_first in [false, true] {
        let c = pool_connection().await?;
        c.execute("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING",&[]).await?;
        let (f, target) = super::quest_consumer_integrity::fixture(&c, "collection").await?;
        let subject: Uuid = c
            .query_one(
                "SELECT auth_subject FROM app.account WHERE id=$1",
                &[&f.account],
            )
            .await?
            .get(0);
        let session = session(&c, subject).await?;
        let selected = definition(&c, &Uuid::new_v4().to_string(), "enabled").await?;
        c.execute(
            "UPDATE app.quest_definition_revision SET reroll_enabled=true WHERE id=$1",
            &[&selected],
        )
        .await?;
        c.batch_execute("BEGIN").await?;
        let waiting = connection().await?;
        let pid: i32 = waiting
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        if facade_first {
            accepted(
                &call(&c, subject, session, Uuid::new_v4(), target, false).await?,
                target,
            );
            let game = f.game;
            let task = tokio::spawn(async move {
                super::quest_consumer_integrity::consume(&waiting, game, 100).await
            });
            let monitor = connection().await?;
            wait_for_database_lock(&monitor, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["data"]["rewards"], "0");
        } else {
            assert_eq!(
                super::quest_consumer_integrity::consume(&c, f.game, 100).await?["data"]["rewards"],
                "1"
            );
            let task = tokio::spawn(async move {
                call(&waiting, subject, session, Uuid::new_v4(), target, false).await
            });
            let monitor = connection().await?;
            wait_for_database_lock(&monitor, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(
                task.await??,
                json!({"status":"fail","data":{"assignment":"unavailable"}})
            );
        }
    }
    Ok(())
}
