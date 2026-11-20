//! Canonical SQL admission/send sources; no network chat or native game is claimed.
use super::{TestResult, account, ante_transaction_integrity as ante, connection, deletion};
use serde_json::Value;
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
async fn send(
    c: &Client,
    a: Uuid,
    t: Uuid,
    b: &str,
    r: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    Ok(c.query_one(
        "SELECT app.send_game_message($1,$2,$3,$4)",
        &[&a, &t, &b, &r],
    )
    .await?
    .get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn admitted_humans_chat_without_contact_replay_exactly_and_reject_other_terms() -> TestResult
{
    let c = connection().await?;
    let (f, m) = ante::reserve(&c, true).await?;
    let t: Uuid = c
        .query_one("SELECT id FROM app.conversation WHERE match_id=$1", &[&m])
        .await?
        .get(0);
    let r = Uuid::new_v4();
    let first = send(&c, f.source.actor, t, "Hello 🎲", r).await?;
    assert_eq!(first["status"], "success");
    assert_eq!(send(&c, f.source.actor, t, "Hello 🎲", r).await?, first);
    assert_eq!(
        send(&c, f.source.actor, t, "changed", r)
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let outsider = account(&c).await?.0;
    for (a, b) in [
        (outsider, "hello"),
        (f.source.actor, " \n\t"),
        (f.source.actor, ""),
    ] {
        assert_eq!(send(&c, a, t, b, Uuid::new_v4()).await?["status"], "fail");
    }
    assert_eq!(
        send(&c, f.source.actor, t, &"x".repeat(4001), Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(
        send(&c, f.other, t, "reply", Uuid::new_v4()).await?["status"],
        "success"
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.message WHERE conversation_id=$1",
            &[&t]
        )
        .await?
        .get::<_, i64>(0),
        2
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.notification WHERE source_kind='dm_message' AND source_ref IN(SELECT id::text FROM app.message WHERE conversation_id=$1)",&[&t]).await?.get::<_,i64>(0),0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_recipient_can_receive_but_cannot_send_and_blocks_stop_group_delivery() -> TestResult
{
    let c = connection().await?;
    let (f, m) = ante::reserve(&c, true).await?;
    let t: Uuid = c
        .query_one("SELECT id FROM app.conversation WHERE match_id=$1", &[&m])
        .await?
        .get(0);
    let r = Uuid::new_v4();
    let original = send(&c, f.other, t, "before deletion", r).await?;
    assert_eq!(original["status"], "success");
    deletion::request(&c, f.other, Uuid::new_v4()).await?;
    assert_eq!(
        send(&c, f.source.actor, t, "still playing", Uuid::new_v4()).await?["status"],
        "success"
    );
    assert_eq!(
        send(&c, f.other, t, "new", Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(send(&c, f.other, t, "before deletion", r).await?, original);
    // Structural block fixture exercises sender gate; actual writer contention is separate.
    c.execute(
        "INSERT INTO app.block(blocker_id,blocked_id,created_at) VALUES($1,$2,clock_timestamp())",
        &[&f.other, &f.source.actor],
    )
    .await?;
    assert_eq!(
        send(&c, f.source.actor, t, "blocked", Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(c.execute("INSERT INTO app.message(conversation_id,sender_id,send_request_ref,body,sent_at) VALUES($1,$2,$3,'raw blocked',clock_timestamp())", &[&t,&f.source.actor,&Uuid::new_v4().to_string()]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    let id: Uuid = original["data"]["resourceId"].as_str().unwrap().parse()?;
    c.query_one("SELECT app.check_game_message_source($1)", &[&id])
        .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn raw_game_message_cannot_bypass_accepted_send_source_even_when_deferred() -> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve(&c, true).await?;
    let t: Uuid = c
        .query_one("SELECT id FROM app.conversation WHERE match_id=$1", &[&m])
        .await?
        .get(0);
    c.batch_execute("BEGIN; SET CONSTRAINTS ALL DEFERRED")
        .await?;
    c.execute("INSERT INTO app.message(conversation_id,sender_id,send_request_ref,body,sent_at) VALUES($1,$2,$3,'forged',clock_timestamp())",&[&t,&f.source.actor,&Uuid::new_v4().to_string()]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.message WHERE conversation_id=$1",
            &[&t]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn block_and_send_serialize_in_both_orders_without_rewriting_history() -> TestResult {
    for block_first in [false, true] {
        let c = connection().await?;
        let other = connection().await?;
        let (f, m) = ante::reserve(&c, true).await?;
        let t: Uuid = c
            .query_one("SELECT id FROM app.conversation WHERE match_id=$1", &[&m])
            .await?
            .get(0);
        let pid: i32 = other
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        c.batch_execute("BEGIN").await?;
        if block_first {
            let blocked: Value = c
                .query_one(
                    "SELECT app.change_block($1,$2,true,$3)",
                    &[&f.other, &f.source.actor, &Uuid::new_v4()],
                )
                .await?
                .get(0);
            assert_eq!(blocked["status"], "success");
            let actor = f.source.actor;
            let task =
                tokio::spawn(async move { send(&other, actor, t, "late", Uuid::new_v4()).await });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "fail");
        } else {
            let accepted = send(&c, f.source.actor, t, "first", Uuid::new_v4()).await?;
            assert_eq!(accepted["status"], "success");
            let actor = f.source.actor;
            let peer = f.other;
            let task = tokio::spawn(async move {
                other
                    .query_one(
                        "SELECT app.change_block($1,$2,true,$3)",
                        &[&peer, &actor, &Uuid::new_v4()],
                    )
                    .await
                    .map(|r| r.get::<_, Value>(0))
            });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "success");
            let id: Uuid = accepted["data"]["resourceId"].as_str().unwrap().parse()?;
            c.query_one("SELECT app.check_game_message_source($1)", &[&id])
                .await?;
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn terminal_match_and_send_serialize_in_both_orders_and_keep_accepted_history() -> TestResult
{
    for terminal_first in [false, true] {
        let c = connection().await?;
        let other = connection().await?;
        let (f, m) = ante::reserve(&c, true).await?;
        let row = c
            .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
            .await?;
        let g: Uuid = row.get(0);
        let opened = super::match_lifecycle_integrity::open(&c, m, g).await?;
        assert_eq!(opened["status"], "success");
        let seat: Uuid = c
            .query_one(
                "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
                &[&m, &f.source.actor],
            )
            .await?
            .get(0);
        let result = super::match_lifecycle_integrity::complete(&c, m, g, vec![seat]).await?;
        let t: Uuid = c
            .query_one("SELECT id FROM app.conversation WHERE match_id=$1", &[&m])
            .await?
            .get(0);
        let pid: i32 = other
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        c.batch_execute("BEGIN").await?;
        if terminal_first {
            assert_eq!(
                super::match_lifecycle_integrity::consume(&c, m, result).await?["status"],
                "success"
            );
            let actor = f.source.actor;
            let task =
                tokio::spawn(async move { send(&other, actor, t, "late", Uuid::new_v4()).await });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "fail");
        } else {
            let request = Uuid::new_v4();
            let accepted = send(&c, f.source.actor, t, "before terminal", request).await?;
            assert_eq!(accepted["status"], "success");
            let task = tokio::spawn(async move {
                super::match_lifecycle_integrity::consume(&other, m, result).await
            });
            super::wait_for_database_lock(&c, pid).await?;
            c.batch_execute("COMMIT").await?;
            assert_eq!(task.await??["status"], "success");
            assert_eq!(
                send(&c, f.source.actor, t, "before terminal", request).await?,
                accepted
            );
            let id: Uuid = accepted["data"]["resourceId"].as_str().unwrap().parse()?;
            c.query_one("SELECT app.check_game_message_source($1)", &[&id])
                .await?;
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn accepted_typed_receipt_cannot_point_to_a_missing_game_message() -> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve(&c, true).await?;
    let t: Uuid = c
        .query_one("SELECT id FROM app.conversation WHERE match_id=$1", &[&m])
        .await?
        .get(0);
    let request = Uuid::new_v4();
    let missing = Uuid::new_v4();
    c.batch_execute("BEGIN;SET CONSTRAINTS ALL DEFERRED")
        .await?;
    let receipt:Uuid=c.query_one("SELECT (app.begin_account_command($1,$2,'chat',$3::uuid::text,jsonb_build_object('action','send_game','body','orphan'))).id",&[&f.source.actor,&request,&t]).await?.get(0);
    c.execute("UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-message-v1','messageId',$2::uuid) WHERE id=$1",&[&receipt,&missing]).await?;
    c.query_one("SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid::text))",&[&receipt,&missing]).await?;
    assert_eq!(
        c.batch_execute("COMMIT").await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert!(
        !c.query_one(
            "SELECT EXISTS(SELECT 1 FROM app.command_receipt WHERE id=$1)",
            &[&receipt]
        )
        .await?
        .get::<_, bool>(0)
    );
    Ok(())
}
