//! Real SQL facade authorization and receipts over normalized admission fixtures.
//! These tests do not qualify native gameplay or a running client.
use super::{TestResult, account, ante_transaction_integrity as ante, connection, set_auth_claims};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;

async fn identity(c: &Client, actor: Uuid) -> Result<(Uuid, Uuid), tokio_postgres::Error> {
    let row = c.query_one("SELECT auth_subject,(test_fixture.claims_for_subject(auth_subject)->>'session_id')::uuid FROM app.account WHERE id=$1", &[&actor]).await?;
    Ok((row.get(0), row.get(1)))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn game_chat_facades_bind_admitted_human_and_keep_blocked_history_recoverable() -> TestResult
{
    let c = connection().await?;
    let (f, m) = ante::reserve(&c, true).await?;
    let (subject, session) = identity(&c, f.source.actor).await?;
    let (outsider, other_subject) = account(&c).await?;
    let (_, other_session) = identity(&c, outsider).await?;
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    let lookup = "SELECT app.read_game_message_command($1,$2,$3,$4,'Hello 🎲')";
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &m])
            .await?
            .get::<_, Value>(0)["data"]["state"],
        "unknown"
    );
    let accepted: Value = c
        .query_one(
            "SELECT app.send_game_message_command($1,$2,$3,$4,'Hello 🎲')",
            &[&subject, &session, &request, &m],
        )
        .await?
        .get(0);
    assert_eq!(accepted["status"], "success");
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &m])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    assert_eq!(
        c.query_one(
            "SELECT app.send_game_message_command($1,$2,$3,$4,'changed')",
            &[&subject, &session, &request, &m]
        )
        .await?
        .get::<_, Value>(0)["data"]["request"],
        "conflict"
    );
    assert_eq!(
        c.query_one(
            "SELECT app.send_game_message_command($1,$2,$3,$4,'intruder')",
            &[&other_subject, &other_session, &Uuid::new_v4(), &m]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "fail"
    );
    c.batch_execute("RESET ROLE").await?;
    set_auth_claims(&c, subject).await?;
    let page: Value = c
        .query_one("SELECT app.read_my_game_chat($1,NULL,NULL,1)", &[&m])
        .await?
        .get(0);
    assert_eq!(page["data"]["canSend"], true);
    assert_eq!(
        page["data"]["items"][0]["senderId"],
        f.source.actor.to_string()
    );
    assert_eq!(page["data"]["items"][0]["body"], "Hello 🎲");
    assert_eq!(page["data"]["participants"].as_array().unwrap().len(), 2);
    assert_eq!(page["data"].as_object().unwrap().len(), 7);
    assert!(!page.to_string().contains("admissionReceiptId"));
    assert_eq!(
        page["data"]["nextCursor"]["id"],
        accepted["data"]["resourceId"]
    );
    assert_eq!(
        c.query_one(
            "SELECT app.read_my_game_chat($1,clock_timestamp(),NULL,16)",
            &[&m]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "fail"
    );
    set_auth_claims(&c, other_subject).await?;
    assert_eq!(
        c.query_one("SELECT app.read_my_game_chat($1,NULL,NULL,16)", &[&m])
            .await?
            .get::<_, Value>(0)["status"],
        "fail"
    );
    c.batch_execute("RESET ROLE").await?;
    c.query_one(
        "SELECT app.change_block($1,$2,true,$3)",
        &[&f.other, &f.source.actor, &Uuid::new_v4()],
    )
    .await?;
    set_auth_claims(&c, subject).await?;
    let blocked: Value = c
        .query_one("SELECT app.read_my_game_chat($1,NULL,NULL,16)", &[&m])
        .await?
        .get(0);
    assert_eq!(blocked["data"]["canSend"], false);
    assert_eq!(blocked["data"]["items"], page["data"]["items"]);
    c.batch_execute("RESET ROLE; SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &m])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    assert_eq!(
        c.query_one(
            "SELECT app.send_game_message_command($1,$2,$3,$4,'new blocked message')",
            &[&subject, &session, &Uuid::new_v4(), &m]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "fail"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn notification_reads_are_owned_and_acknowledgments_are_atomic_exact_and_first_read_only()
-> TestResult {
    let c = connection().await?;
    let (actor, subject) = account(&c).await?;
    let (other, _) = account(&c).await?;
    let (_, session) = identity(&c, actor).await?;
    // Structural producer fixtures isolate projection/ownership; producer integrity has separate tests.
    let mut ids = Vec::new();
    for (kind, body) in [
        ("dm_message", "New direct message"),
        ("trade_offer", "Trade offer received"),
        ("trade_accepted", "Trade accepted"),
        ("market_sold", "Marketplace listing sold"),
    ] {
        ids.push(c.query_one("INSERT INTO app.notification(recipient_id,source_kind,source_ref,body) VALUES($1,$2,$3,$4) RETURNING id", &[&actor,&kind,&Uuid::new_v4().to_string(),&body]).await?.get::<_,Uuid>(0));
    }
    let foreign: Uuid = c.query_one("INSERT INTO app.notification(recipient_id,source_kind,source_ref,body) VALUES($1,'trade_offer',$2,'Trade offer received') RETURNING id", &[&other,&Uuid::new_v4().to_string()]).await?.get(0);
    set_auth_claims(&c, subject).await?;
    let page: Value = c
        .query_one("SELECT app.read_my_notifications(NULL,1)", &[])
        .await?
        .get(0);
    assert_eq!(page["data"]["unreadCount"], "4");
    assert_eq!(page["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["data"]["items"][0].as_object().unwrap().len(), 5);
    assert!(page["data"]["items"][0]["readAt"].is_null());
    c.batch_execute("RESET ROLE; SET ROLE service_role").await?;
    let mixed = vec![ids[0], foreign];
    assert_eq!(
        c.query_one(
            "SELECT app.mark_notifications_read($1,$2,$3,$4)",
            &[&subject, &session, &Uuid::new_v4(), &mixed]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "fail"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.notification WHERE recipient_id=$1 AND read_at IS NOT NULL", &[&actor]).await?.get::<_,i64>(0), 0);
    let request = Uuid::new_v4();
    c.batch_execute("SET ROLE service_role").await?;
    let accepted: Value = c
        .query_one(
            "SELECT app.mark_notifications_read($1,$2,$3,$4)",
            &[&subject, &session, &request, &ids],
        )
        .await?
        .get(0);
    assert_eq!(
        accepted,
        json!({"status":"success","data":{"resourceId":actor}})
    );
    let lookup = "SELECT app.read_notifications_command($1,$2,$3,$4)";
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &ids])
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    let mut reversed = ids.clone();
    reversed.reverse();
    assert_eq!(
        c.query_one(lookup, &[&subject, &session, &request, &reversed])
            .await?
            .get::<_, Value>(0)["data"]["request"],
        "conflict"
    );
    c.batch_execute("RESET ROLE").await?;
    let before: Value = c
        .query_one(
            "SELECT jsonb_agg(read_at ORDER BY id) FROM app.notification WHERE recipient_id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    c.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            "SELECT app.mark_notifications_read($1,$2,$3,$4)",
            &[&subject, &session, &Uuid::new_v4(), &ids]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "success"
    );
    c.batch_execute("RESET ROLE").await?;
    let after: Value = c
        .query_one(
            "SELECT jsonb_agg(read_at ORDER BY id) FROM app.notification WHERE recipient_id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    assert_eq!(before, after);
    set_auth_claims(&c, subject).await?;
    assert_eq!(
        c.query_one("SELECT app.read_my_notifications(NULL,100)", &[])
            .await?
            .get::<_, Value>(0)["data"]["unreadCount"],
        "0"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn chat_notification_facades_reject_repeatable_read_before_any_receipt() -> TestResult {
    let c = connection().await?;
    let (actor, subject) = account(&c).await?;
    let (_, session) = identity(&c, actor).await?;
    let ids = vec![Uuid::new_v4()];
    for sql in [
        "SELECT app.mark_notifications_read($1,$2,$3,$4)",
        "SELECT app.read_notifications_command($1,$2,$3,$4)",
    ] {
        c.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ; SET LOCAL ROLE service_role")
            .await?;
        let error = c
            .query_one(sql, &[&subject, &session, &Uuid::new_v4(), &ids])
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
        );
        c.batch_execute("ROLLBACK").await?;
    }
    c.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ; SET LOCAL ROLE service_role")
        .await?;
    assert_eq!(
        c.query_one(
            "SELECT app.read_game_message_command($1,$2,$3,$4,'body')",
            &[&subject, &session, &Uuid::new_v4(), &Uuid::new_v4()]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(
        c.query_one(
            "SELECT count(*)::bigint FROM app.command_receipt WHERE account_id=$1",
            &[&actor]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn generic_and_direct_read_acknowledgments_preserve_first_read_in_both_orders() -> TestResult
{
    for direct_first in [true, false] {
        let c = connection().await?;
        let (actor, subject) = account(&c).await?;
        let (peer, _) = account(&c).await?;
        let (_, session) = identity(&c, actor).await?;
        super::social::friend(&c, actor, peer).await?;
        let opened: Value = c
            .query_one(
                "SELECT app.open_direct_conversation($1,$2,$3)",
                &[&actor, &peer, &Uuid::new_v4()],
            )
            .await?
            .get(0);
        let conversation: Uuid = opened["data"]["resourceId"].as_str().unwrap().parse()?;
        let sent: Value = c
            .query_one(
                "SELECT app.send_direct_message($1,$2,'hello',$3)",
                &[&peer, &conversation, &Uuid::new_v4()],
            )
            .await?
            .get(0);
        let message: Uuid = sent["data"]["resourceId"].as_str().unwrap().parse()?;
        let notification: Uuid = c.query_one("SELECT id FROM app.notification WHERE recipient_id=$1 AND source_kind='dm_message' AND source_ref=$2",&[&actor,&message.to_string()]).await?.get(0);
        let mut first: Option<Value> = None;
        for direct in [direct_first, !direct_first] {
            c.batch_execute("SET ROLE service_role").await?;
            let result: Value = if direct {
                c.query_one(
                    "SELECT app.mark_direct_messages_read($1,$2,$3,$4,$5)",
                    &[
                        &subject,
                        &session,
                        &Uuid::new_v4(),
                        &conversation,
                        &vec![message],
                    ],
                )
                .await?
                .get(0)
            } else {
                c.query_one(
                    "SELECT app.mark_notifications_read($1,$2,$3,$4)",
                    &[&subject, &session, &Uuid::new_v4(), &vec![notification]],
                )
                .await?
                .get(0)
            };
            assert_eq!(result["status"], "success");
            c.batch_execute("RESET ROLE").await?;
            let timestamp: Value = c
                .query_one(
                    "SELECT to_jsonb(read_at) FROM app.notification WHERE id=$1",
                    &[&notification],
                )
                .await?
                .get(0);
            assert!(!timestamp.is_null());
            if let Some(previous) = first.as_ref() {
                assert_eq!(*previous, timestamp);
            } else {
                first = Some(timestamp);
            }
        }
        // A later arrival is never swept into an earlier exact-ID acknowledgment.
        c.query_one(
            "SELECT app.send_direct_message($1,$2,'later',$3)",
            &[&peer, &conversation, &Uuid::new_v4()],
        )
        .await?;
        super::deletion::request(&c, actor, Uuid::new_v4()).await?;
        set_auth_claims(&c, subject).await?;
        assert_eq!(
            c.query_one("SELECT app.read_my_notifications(NULL,100)", &[])
                .await?
                .get::<_, Value>(0)["data"]["unreadCount"],
            "1"
        );
        c.batch_execute("RESET ROLE; SET ROLE service_role").await?;
        assert_eq!(
            c.query_one(
                "SELECT app.mark_notifications_read($1,$2,$3,$4)",
                &[&subject, &session, &Uuid::new_v4(), &vec![notification]]
            )
            .await?
            .get::<_, Value>(0)["status"],
            "success"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn game_chat_send_rechecks_live_session_after_roster_account_wait() -> TestResult {
    let c = connection().await?;
    let waiting = connection().await?;
    let (f, m) = ante::reserve(&c, true).await?;
    let (subject, session) = identity(&c, f.source.actor).await?;
    let request = Uuid::new_v4();
    let pid: i32 = waiting
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    c.batch_execute("BEGIN").await?;
    c.query_one(
        "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
        &[&f.other],
    )
    .await?;
    let send = tokio::spawn(async move {
        waiting.batch_execute("SET ROLE service_role").await?;
        waiting
            .query_one(
                "SELECT app.send_game_message_command($1,$2,$3,$4,'after wait')",
                &[&subject, &session, &request, &m],
            )
            .await
            .map(|row| row.get::<_, Value>(0))
    });
    super::wait_for_database_lock(&c, pid).await?;
    c.execute(
        "UPDATE auth.sessions SET not_after=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&session],
    )
    .await?;
    c.batch_execute("COMMIT").await?;
    assert_eq!(
        send.await??,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.command_receipt WHERE account_id=$1 AND scope='chat' AND request_ref=$2",&[&f.source.actor,&request.to_string()]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn game_chat_history_survives_pending_and_completion_with_exact_cursor_and_anonymous_author()
-> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve(&c, true).await?;
    let (subject, session) = identity(&c, f.source.actor).await?;
    let (peer_subject, _) = identity(&c, f.other).await?;
    let request = Uuid::new_v4();
    let sent: Value = c
        .query_one(
            "SELECT app.send_game_message_command($1,$2,$3,$4,'historic text')",
            &[&subject, &session, &request, &m],
        )
        .await?
        .get(0);
    assert_eq!(sent["status"], "success");
    let second: Value = c
        .query_one(
            "SELECT app.send_game_message_command($1,$2,$3,$4,'second text')",
            &[&subject, &session, &Uuid::new_v4(), &m],
        )
        .await?
        .get(0);
    assert_eq!(second["status"], "success");
    set_auth_claims(&c, peer_subject).await?;
    let page: Value = c
        .query_one("SELECT app.read_my_game_chat($1,NULL,NULL,1)", &[&m])
        .await?
        .get(0);
    let before = page["data"]["nextCursor"].clone();
    let older: Value = c
        .query_one(
            "SELECT app.read_my_game_chat($1,$2::text::timestamptz,$3,1)",
            &[
                &m,
                &before["sentAt"].as_str().unwrap(),
                &before["id"].as_str().unwrap().parse::<Uuid>()?,
            ],
        )
        .await?
        .get(0);
    assert_eq!(older["data"]["items"][0]["id"], sent["data"]["resourceId"]);
    c.batch_execute("RESET ROLE").await?;
    let game: Uuid = c
        .query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
        .await?
        .get(0);
    super::match_lifecycle_integrity::open(&c, m, game).await?;
    let seat: Uuid = c
        .query_one(
            "SELECT id FROM app.match_seat WHERE match_id=$1 AND account_id=$2",
            &[&m, &f.source.actor],
        )
        .await?
        .get(0);
    let result = super::match_lifecycle_integrity::complete(&c, m, game, vec![seat]).await?;
    super::match_lifecycle_integrity::consume(&c, m, result).await?;
    super::deletion::request(&c, f.source.actor, Uuid::new_v4()).await?;
    set_auth_claims(&c, subject).await?;
    let pending: Value = c
        .query_one("SELECT app.read_my_game_chat($1,NULL,NULL,16)", &[&m])
        .await?
        .get(0);
    assert_eq!(pending["status"], "success");
    assert_eq!(pending["data"]["canSend"], false);
    c.batch_execute("RESET ROLE; SET ROLE service_role").await?;
    assert_eq!(
        c.query_one(
            "SELECT app.read_game_message_command($1,$2,$3,$4,'historic text')",
            &[&subject, &session, &request, &m]
        )
        .await?
        .get::<_, Value>(0)["data"]["response"],
        sent
    );
    c.batch_execute("RESET ROLE").await?;
    // Structural retired-identity fixture tests masking only, not deletion-worker qualification.
    let op = super::operation(&c, f.source.actor).await?;
    c.execute(
        "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
        &[&op],
    )
    .await?;
    c.execute("UPDATE app.account SET state='deleted',auth_subject=NULL,deletion_requested_at=clock_timestamp(),deletion_finalized_at=clock_timestamp(),retirement_operation_id=$2 WHERE id=$1",&[&f.source.actor,&op]).await?;
    set_auth_claims(&c, peer_subject).await?;
    let masked: Value = c
        .query_one("SELECT app.read_my_game_chat($1,NULL,NULL,16)", &[&m])
        .await?
        .get(0);
    assert!(
        masked["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["author"] == "Deleted player")
    );
    assert!(
        masked["data"]["participants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == f.source.actor.to_string()
                && p["username"] == "Deleted player"
                && p["isDeleted"] == true)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn game_chat_read_never_grants_unadmitted_or_ai_only_sponsors_participation() -> TestResult {
    let c = connection().await?;
    let fixture = super::game_scope::seed(&c).await?;
    let (subject, _) = identity(&c, fixture.account).await?;
    c.query_one(
        "SELECT app.ensure_game_conversation($1)",
        &[&fixture.match_id],
    )
    .await?;
    set_auth_claims(&c, subject).await?;
    assert_eq!(
        c.query_one(
            "SELECT app.read_my_game_chat($1,NULL,NULL,16)",
            &[&fixture.match_id]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "fail"
    );
    c.batch_execute("RESET ROLE").await?;
    let (sponsor, sponsor_subject) = account(&c).await?;
    c.execute("INSERT INTO app.match_seat(match_id,sponsor_id,seat_index,controller_kind) VALUES($1,$2,1,'ai')",&[&fixture.match_id,&sponsor]).await?;
    set_auth_claims(&c, sponsor_subject).await?;
    assert_eq!(
        c.query_one(
            "SELECT app.read_my_game_chat($1,NULL,NULL,16)",
            &[&fixture.match_id]
        )
        .await?
        .get::<_, Value>(0)["status"],
        "fail"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.conversation_member cm JOIN app.conversation thread ON thread.id=cm.conversation_id WHERE thread.match_id=$1 AND cm.account_id=$2",&[&fixture.match_id,&sponsor]).await?.get::<_,i64>(0),0);
    Ok(())
}
