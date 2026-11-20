//! Relational progression checks over synthetic sources, not a qualified activity consumer.
use super::{TestResult, account, connection, game_scope, operation, wait_for_database_lock};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

async fn quest(c: &Client, owner: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    let definition = Uuid::new_v4();
    c.execute("INSERT INTO app.quest_definition_revision(id,definition_key,revision,objective_ref,target,reward_coins) VALUES($1,$1::uuid::text,'fixture','fixture',3,500)", &[&definition]).await?;
    c.query_one("INSERT INTO app.quest_assignment(account_id,definition_id,source_day,assignment_ref,progress) VALUES($1,$2,'2028-02-29',$2::uuid::text,0) RETURNING id", &[&owner,&definition]).await.map(|r| r.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn quests_bind_replacement_generation_and_reward_facts_to_the_same_account_and_day()
-> TestResult {
    let c = connection().await?;
    let source = game_scope::seed(&c).await?;
    let other = game_scope::seed(&c).await?;
    let a = quest(&c, source.account).await?;
    let replacement = quest(&c, source.account).await?;
    let foreign = quest(&c, other.account).await?;
    assert_eq!(
        c.execute(
            "UPDATE app.quest_assignment SET replaced_by_id=$2 WHERE id=$1",
            &[&a, &foreign]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::FOREIGN_KEY_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.quest_assignment SET replaced_by_id=id WHERE id=$1",
            &[&a]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    for (assignment, day) in [(foreign, "2028-02-29"), (a, "2028-03-01")] {
        assert_eq!(c.execute("INSERT INTO app.daily_quest_state(account_id,utc_day,generated_assignment_id,rerolls_used,revision) VALUES($1,$2,$3,0,0)", &[&source.account,&day,&assignment]).await.unwrap_err().code(), Some(&SqlState::FOREIGN_KEY_VIOLATION));
    }
    c.execute("INSERT INTO app.daily_quest_state(account_id,utc_day,generated_assignment_id,rerolls_used,revision) VALUES($1,'2028-02-29',$2,0,0)", &[&source.account,&a]).await?;
    let fact: Uuid = c.query_one("INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'fixture',3,'fixture') RETURNING id", &[&other.game,&other.event,&other.account]).await?.get(0);
    let reward = operation(&c, source.account).await?;
    assert_eq!(c.execute("UPDATE app.quest_assignment SET progress=3,completed_at=clock_timestamp(),reward_operation_id=$2,reward_source_fact_id=$3 WHERE id=$1", &[&a,&reward,&fact]).await.unwrap_err().code(), Some(&SqlState::FOREIGN_KEY_VIOLATION));
    c.execute(
        "UPDATE app.quest_assignment SET replaced_by_id=$2 WHERE id=$1",
        &[&a, &replacement],
    )
    .await?;
    assert_eq!(
        c.execute(
            "UPDATE app.quest_assignment SET progress=1 WHERE id=$1",
            &[&a]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competing_quest_replacements_cannot_commit_a_cycle_and_stale_snapshots_must_retry()
-> TestResult {
    let c = connection().await?;
    let (owner, _) = account(&c).await?;
    let a = quest(&c, owner).await?;
    let b = quest(&c, owner).await?;
    let waiting = connection().await?;
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.execute(
        "UPDATE app.quest_assignment SET replaced_by_id=$2 WHERE id=$1",
        &[&a, &b],
    )
    .await?;
    let task = tokio::spawn(async move {
        waiting
            .execute(
                "UPDATE app.quest_assignment SET replaced_by_id=$2 WHERE id=$1",
                &[&b, &a],
            )
            .await
    });
    wait_for_database_lock(&c, pid).await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        task.await?.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT replaced_by_id FROM app.quest_assignment WHERE id=$1",
            &[&b]
        )
        .await?
        .get::<_, Option<Uuid>>(0),
        None
    );
    let d = quest(&c, owner).await?;
    c.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    assert_eq!(
        c.execute(
            "UPDATE app.quest_assignment SET replaced_by_id=$2 WHERE id=$1",
            &[&b, &d]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn calendar_keys_are_real_canonical_days_and_independent_of_session_date_style() -> TestResult
{
    let c = connection().await?;
    c.batch_execute("SET DateStyle='SQL, DMY'").await?;
    for (day, valid) in [
        ("2028-02-29", true),
        ("2027-02-29", false),
        ("2028-2-29", false),
        ("2028-04-31", false),
        ("infinity", false),
        ("0000-01-01", false),
    ] {
        assert_eq!(
            c.query_one("SELECT app.valid_utc_day($1)", &[&day])
                .await?
                .get::<_, bool>(0),
            valid,
            "{day}"
        );
    }
    let (owner, _) = account(&c).await?;
    for day in ["2028-02-30", "2028-1-01"] {
        assert_eq!(c.execute("INSERT INTO app.daily_win_bucket(account_id,utc_day,wins,issued_coins,revision) VALUES($1,$2,0,0,0)", &[&owner,&day]).await.unwrap_err().code(), Some(&SqlState::CHECK_VIOLATION));
    }
    Ok(())
}
