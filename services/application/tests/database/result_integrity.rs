//! Accepted-source relational checks, separate from actual native result/credit production.
use super::{TestResult, account, connection, game_scope};
use tokio_postgres::error::SqlState;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn activity_requires_a_human_participant_and_parent_changes_cannot_orphan_accepted_facts()
-> TestResult {
    let c = connection().await?;
    let local = game_scope::seed(&c).await?;
    let other = game_scope::seed(&c).await?;
    let sql = "INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision) VALUES($1,$2,$3,'fixture',1,'fixture')";
    assert_eq!(
        c.execute(sql, &[&local.game, &local.event, &other.account])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    // In-play facts are valid; completed-game participation is not a prerequisite.
    c.execute(sql, &[&local.game, &local.event, &local.account])
        .await?;
    assert_eq!(
        c.execute(
            "UPDATE app.match_seat SET account_id=$2 WHERE id=$1",
            &[&local.seat, &other.account]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.game SET match_id=$2,game_index=1 WHERE id=$1",
            &[&local.game, &other.match_id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT account_id FROM app.match_seat WHERE id=$1",
            &[&local.seat]
        )
        .await?
        .get::<_, Uuid>(0),
        local.account
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn competitive_credit_binds_participant_format_time_and_one_record_per_completed_match()
-> TestResult {
    let c = connection().await?;
    let local = game_scope::seed(&c).await?;
    let (other, _) = account(&c).await?;
    let format: Uuid = c
        .query_one(
            "SELECT format_id FROM app.match WHERE id=$1",
            &[&local.match_id],
        )
        .await?
        .get(0);
    let result = Uuid::new_v4();
    let insert = "INSERT INTO app.match_result(id,match_id,source_ref,outcome,winners_ref,committed_at,ranking_format_id,rank_sequence) VALUES($1,$2,$1::uuid::text,'win','fixture',clock_timestamp(),$3,1)";
    assert_eq!(
        c.execute(insert, &[&result, &local.match_id, &format])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.execute(
        "UPDATE app.match SET competitive=true,mode='collection' WHERE id=$1",
        &[&local.match_id],
    )
    .await?;
    c.execute(insert, &[&result, &local.match_id, &format])
        .await?;
    let credit = "INSERT INTO app.competitive_result(account_id,match_result_id,source_ref,format_id,win,completed_at,policy_ref) SELECT $1,id,$3::uuid::text,ranking_format_id,false,committed_at,'fixture' FROM app.match_result WHERE id=$2";
    assert_eq!(
        c.execute(credit, &[&other, &result, &Uuid::new_v4()])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(c.execute("INSERT INTO app.competitive_result(account_id,match_result_id,source_ref,format_id,win,completed_at,policy_ref) SELECT $1,id,$3::uuid::text,ranking_format_id,false,committed_at+interval '1 second','fixture' FROM app.match_result WHERE id=$2", &[&local.account,&result,&Uuid::new_v4()]).await.unwrap_err().code(), Some(&SqlState::FOREIGN_KEY_VIOLATION));
    c.execute(credit, &[&local.account, &result, &Uuid::new_v4()])
        .await?;
    assert_eq!(
        c.execute(credit, &[&local.account, &result, &Uuid::new_v4()])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::UNIQUE_VIOLATION)
    );
    assert_eq!(
        c.execute(
            "UPDATE app.match SET competitive=false WHERE id=$1",
            &[&local.match_id]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
