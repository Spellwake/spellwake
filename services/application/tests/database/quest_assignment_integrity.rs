//! Private assignment transactions with explicitly supplied policy choices, not an active distribution policy.
use super::{TestResult, account, connection, wait_for_database_lock};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn definition(c: &Client) -> Result<Uuid, tokio_postgres::Error> {
    c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) VALUES($1,'fixture','fixture',3,500,'fixture:casts','fixture:v1','fixture:qualified') RETURNING id", &[&Uuid::new_v4().to_string()]).await.map(|r|r.get(0))
}
async fn day(c: &Client) -> Result<String, tokio_postgres::Error> {
    c.query_one(
        "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD')",
        &[],
    )
    .await
    .map(|r| r.get(0))
}
async fn generate(
    c: &Client,
    owner: Uuid,
    def: Uuid,
    day: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.generate_daily_quest($1,$2,$3,$4)",
        &[&owner, &def, &day, &request],
    )
    .await
    .map(|r| {
        let response: Value = r.get(0);
        let original = spellwake_application::terminal_consumers::DailyQuestSelection {
            actor_id: owner,
            definition_id: def,
            day: day.into(),
            request_id: request,
        };
        if original.valid() {
            assert!(
                original
                    .decode_generate_response(response.to_string().as_bytes())
                    .is_ok()
            );
        }
        response
    })
}
async fn reroll(
    c: &Client,
    owner: Uuid,
    assignment: Uuid,
    def: Uuid,
    day: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    c.query_one(
        "SELECT app.reroll_daily_quest($1,$2,$3,$4,$5)",
        &[&owner, &assignment, &def, &day, &request],
    )
    .await
    .map(|r| {
        let response: Value = r.get(0);
        let original = spellwake_application::terminal_consumers::RerollQuest {
            assignment_id: assignment,
            selection: spellwake_application::terminal_consumers::DailyQuestSelection {
                actor_id: owner,
                definition_id: def,
                day: day.into(),
                request_id: request,
            },
        };
        if original.valid() {
            assert!(
                original
                    .decode_response(response.to_string().as_bytes())
                    .is_ok()
            );
        }
        response
    })
}
fn resource(v: &Value) -> Uuid {
    assert_eq!(v["status"], "success");
    v["data"]["resourceId"].as_str().unwrap().parse().unwrap()
}
async fn fixture(c: &Client) -> FixtureResult<(Uuid, Uuid, String)> {
    let (a, _) = account(c).await?;
    Ok((a, definition(c).await?, day(c).await?))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn daily_generation_is_once_per_utc_day_with_stable_accepted_and_rejected_replay()
-> TestResult {
    let c = connection().await?;
    let (a, d, today) = fixture(&c).await?;
    let request = Uuid::new_v4();
    let result = generate(&c, a, d, &today, request).await?;
    let id = resource(&result);
    assert_eq!(generate(&c, a, d, &today, request).await?, result);
    let other = definition(&c).await?;
    assert_eq!(
        resource(&generate(&c, a, other, &today, Uuid::new_v4()).await?),
        id
    );
    assert_eq!(
        generate(&c, a, other, &today, request)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let stale = Uuid::new_v4();
    let failure = generate(&c, a, d, "2000-01-01", stale).await?;
    assert_eq!(failure["status"], "fail");
    assert_eq!(generate(&c, a, d, "2000-01-01", stale).await?, failure);
    let counter=c.query_one("SELECT (SELECT count(*) FROM app.quest_assignment WHERE account_id=$1)::bigint,(SELECT count(*) FROM app.daily_quest_state WHERE account_id=$1)::bigint", &[&a]).await?;
    assert_eq!(counter.get::<_, i64>(0), 1);
    assert_eq!(counter.get::<_, i64>(1), 1);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn full_bank_can_reroll_once_without_a_fourth_assignment_or_consuming_daily_generation()
-> TestResult {
    let c = connection().await?;
    let (a, d, today) = fixture(&c).await?;
    let mut originals = Vec::new();
    for _ in 0..3 {
        originals.push(c.query_one("INSERT INTO app.quest_assignment(account_id,definition_id,source_day,assignment_ref,progress) VALUES($1,$2,'2026-01-01',$3,1) RETURNING id", &[&a,&d,&Uuid::new_v4().to_string()]).await?.get::<_,Uuid>(0));
    }
    let denied = generate(&c, a, d, &today, Uuid::new_v4()).await?;
    assert_eq!(denied["data"]["bank"], "full");
    let next = definition(&c).await?;
    let request = Uuid::new_v4();
    let changed = reroll(&c, a, originals[0], next, &today, request).await?;
    let replacement = resource(&changed);
    assert_eq!(
        reroll(&c, a, originals[0], next, &today, request).await?,
        changed
    );
    assert_eq!(
        reroll(&c, a, originals[1], next, &today, Uuid::new_v4()).await?["data"]["reroll"],
        "already used"
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.quest_assignment WHERE account_id=$1 AND completed_at IS NULL AND replaced_by_id IS NULL", &[&a]).await?.get::<_,i64>(0),3);
    assert_eq!(
        c.query_one(
            "SELECT progress::text FROM app.quest_assignment WHERE id=$1",
            &[&replacement]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    assert_eq!(c.query_one("SELECT generated_assignment_id FROM app.daily_quest_state WHERE account_id=$1 AND utc_day=$2", &[&a,&today]).await?.get::<_,Option<Uuid>>(0),None);
    assert_eq!(c.execute("UPDATE app.daily_quest_state SET rerolls_used=0,revision=revision+1 WHERE account_id=$1", &[&a]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(c.execute("INSERT INTO app.quest_assignment(account_id,definition_id,source_day,assignment_ref,progress) VALUES($1,$2,$3,$4,0)", &[&a,&d,&today,&Uuid::new_v4().to_string()]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn concurrent_daily_generators_wait_and_return_the_same_assignment() -> TestResult {
    let c = connection().await?;
    let (a, d, today) = fixture(&c).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    let first = resource(&generate(&c, a, d, &today, Uuid::new_v4()).await?);
    let task = tokio::spawn(async move { generate(&waiting, a, d, &today, Uuid::new_v4()).await });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(resource(&task.await??), first);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn generation_and_reroll_reject_pending_and_foreign_accounts_without_partial_daily_state()
-> TestResult {
    let c = connection().await?;
    let (a, d, today) = fixture(&c).await?;
    let owned = resource(&generate(&c, a, d, &today, Uuid::new_v4()).await?);
    let (other, _) = account(&c).await?;
    let next = definition(&c).await?;
    assert_eq!(
        reroll(&c, other, owned, next, &today, Uuid::new_v4()).await?["status"],
        "fail"
    );
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&other]).await?;
    assert_eq!(
        generate(&c, other, d, &today, Uuid::new_v4()).await?["data"]["account"],
        "unavailable"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.daily_quest_state WHERE account_id=$1",
            &[&other]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unqualified_quest_definitions_cannot_consume_generation_or_reroll_allowances() -> TestResult
{
    let c = connection().await?;
    let (owner, qualified, today) = fixture(&c).await?;
    let unavailable:Uuid=c.query_one("INSERT INTO app.quest_definition_revision(definition_key,revision,objective_ref,target,reward_coins) VALUES($1,'fixture','fixture:unqualified',3,500) RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    assert_eq!(
        generate(&c, owner, unavailable, &today, Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.daily_quest_state WHERE account_id=$1",
            &[&owner]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    let assignment = resource(&generate(&c, owner, qualified, &today, Uuid::new_v4()).await?);
    assert_eq!(
        reroll(&c, owner, assignment, unavailable, &today, Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert!(
        c.query_one(
            "SELECT rerolls_used=0 FROM app.daily_quest_state WHERE account_id=$1 AND utc_day=$2",
            &[&owner, &today]
        )
        .await?
        .get::<_, bool>(0)
    );
    assert_eq!(
        reroll(
            &c,
            owner,
            assignment,
            definition(&c).await?,
            &today,
            Uuid::new_v4()
        )
        .await?["status"],
        "success"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn reroll_source_rejections_preserve_original_day_and_inactive_account_contracts()
-> TestResult {
    let c = connection().await?;
    let (actor, def, today) = fixture(&c).await?;
    let assignment = resource(&generate(&c, actor, def, &today, Uuid::new_v4()).await?);
    let replacement = definition(&c).await?;
    assert_eq!(
        reroll(
            &c,
            actor,
            assignment,
            replacement,
            "2001-01-01",
            Uuid::new_v4()
        )
        .await?["data"],
        serde_json::json!({"day":"not current UTC day"})
    );
    c.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",&[&actor]).await?;
    assert_eq!(
        reroll(&c, actor, assignment, replacement, &today, Uuid::new_v4()).await?["data"],
        serde_json::json!({"account":"unavailable"})
    );
    Ok(())
}
