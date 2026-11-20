use super::{TestResult, account, connection, operation};
use serde_json::Value;
use spellwake_application::{
    ApiResponse,
    commands::{CommandAck, CommandDisposition},
};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

fn resource(value: Value) -> Uuid {
    match serde_json::from_value::<ApiResponse<CommandAck>>(value).unwrap() {
        ApiResponse::Success { data } => data.resource_id,
        other => panic!("expected command acknowledgment: {other:?}"),
    }
}

pub(super) async fn friend(client: &Client, left: Uuid, right: Uuid) -> TestResult {
    for (actor, other, action) in [(left, right, "request"), (right, left, "accept")] {
        let request = Uuid::new_v4();
        let row = client
            .query_one(
                "SELECT app.change_friendship($1,$2,$3,$4)",
                &[&actor, &other, &action, &request],
            )
            .await?;
        resource(row.get(0));
    }
    Ok(())
}

async fn open_dm(
    client: &Client,
    actor: Uuid,
    other: Uuid,
    request: Uuid,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    Ok(client
        .query_one(
            "SELECT app.open_direct_conversation($1,$2,$3)",
            &[&actor, &other, &request],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn dm_receipts_bind_terms_and_expired_history_remains_readable() -> TestResult {
    let client = connection().await?;
    let (left, left_subject) = account(&client).await?;
    let (right, _) = account(&client).await?;
    let (outsider, outsider_subject) = account(&client).await?;
    let denied_request = Uuid::new_v4();
    assert_eq!(
        open_dm(&client, left, right, denied_request).await?["status"],
        "fail"
    );
    friend(&client, left, right).await?;
    // A rejected command keeps its disposition; newly eligible work uses a new request.
    assert_eq!(
        open_dm(&client, left, right, denied_request).await?["status"],
        "fail"
    );
    let opening = Uuid::new_v4();
    let conversation = resource(open_dm(&client, left, right, opening).await?);
    assert_eq!(
        open_dm(&client, left, outsider, opening)
            .await
            .unwrap_err()
            .downcast_ref::<tokio_postgres::Error>()
            .unwrap()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        resource(open_dm(&client, left, right, opening).await?),
        conversation
    );
    assert_eq!(
        resource(open_dm(&client, right, left, Uuid::new_v4()).await?),
        conversation
    );
    let send = Uuid::new_v4();
    let sql = "SELECT app.send_direct_message($1,$2,$3,$4)";
    let first: Value = client
        .query_one(sql, &[&left, &conversation, &"hello", &send])
        .await?
        .get(0);
    let id = resource(first.clone());
    let disposition: CommandDisposition = serde_json::from_value(
        client
            .query_one(
                "SELECT app.read_account_command($1,'chat',$2)",
                &[&left, &send],
            )
            .await?
            .get::<_, Value>(0),
    )?;
    assert_eq!(
        disposition,
        CommandDisposition::Accepted {
            ack: CommandAck { resource_id: id }
        }
    );
    let other_disposition: CommandDisposition = serde_json::from_value(
        client
            .query_one(
                "SELECT app.read_account_command($1,'chat',$2)",
                &[&right, &send],
            )
            .await?
            .get::<_, Value>(0),
    )?;
    assert_eq!(other_disposition, CommandDisposition::Unknown);
    assert_eq!(
        first,
        client
            .query_one(sql, &[&left, &conversation, &"hello", &send])
            .await?
            .get::<_, Value>(0)
    );
    assert_eq!(
        client
            .query_one("SELECT count(*) FROM app.message WHERE id=$1", &[&id])
            .await?
            .get::<_, i64>(0),
        1
    );
    let changed = client
        .query_one(sql, &[&left, &conversation, &"different", &send])
        .await
        .unwrap_err();
    assert_eq!(changed.code(), Some(&SqlState::CHECK_VIOLATION));
    client
        .query_one(
            "SELECT app.change_friendship($1,$2,'remove',$3)",
            &[&left, &right, &Uuid::new_v4()],
        )
        .await?;
    assert_eq!(
        client
            .query_one(sql, &[&left, &conversation, &"new", &Uuid::new_v4()])
            .await?
            .get::<_, Value>(0)["status"],
        "fail"
    );
    client.batch_execute("SET ROLE authenticated").await?;
    super::set_auth_claims(&client, left_subject).await?;
    assert_eq!(
        client
            .query_one("SELECT body FROM app.message WHERE id=$1", &[&id])
            .await?
            .get::<_, String>(0),
        "hello"
    );
    super::set_auth_claims(&client, outsider_subject).await?;
    assert!(
        client
            .query("SELECT id FROM app.message WHERE id=$1", &[&id])
            .await?
            .is_empty()
    );
    assert!(
        client
            .query_one(
                "SELECT app.change_block($1,$2,true,$3)",
                &[&left, &right, &Uuid::new_v4()]
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn concurrent_dm_open_has_one_pair_and_blocking_preserves_history() -> TestResult {
    let client = connection().await?;
    let (left, _) = account(&client).await?;
    let (right, _) = account(&client).await?;
    friend(&client, left, right).await?;
    let first = connection().await?;
    let second = connection().await?;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let (one, two) = tokio::join!(
        open_dm(&first, left, right, a),
        open_dm(&second, right, left, b)
    );
    assert_eq!(resource(one?), resource(two?));
    let row = client
        .query_one(
            "SELECT app.change_block($1,$2,true,$3)",
            &[&right, &left, &Uuid::new_v4()],
        )
        .await?;
    resource(row.get(0));
    assert!(
        !client
            .query_one("SELECT app.contact_allowed($1,$2)", &[&left, &right])
            .await?
            .get::<_, bool>(0)
    );
    let removed: Value = client
        .query_one(
            "SELECT app.change_friendship($1,$2,'remove',$3)",
            &[&left, &right, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    resource(removed);
    client
        .query_one(
            "SELECT app.change_block($1,$2,false,$3)",
            &[&right, &left, &Uuid::new_v4()],
        )
        .await?;
    assert!(
        !client
            .query_one("SELECT app.contact_allowed($1,$2)", &[&left, &right])
            .await?
            .get::<_, bool>(0)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn stale_contact_snapshot_cannot_send_after_a_block() -> TestResult {
    let mut first = connection().await?;
    let (left, _) = account(&first).await?;
    let (right, _) = account(&first).await?;
    friend(&first, left, right).await?;
    let conversation = resource(open_dm(&first, left, right, Uuid::new_v4()).await?);
    let stale = first
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    assert!(
        stale
            .query_one("SELECT app.contact_allowed($1,$2)", &[&left, &right])
            .await?
            .get::<_, bool>(0)
    );
    let second = connection().await?;
    second
        .query_one(
            "SELECT app.change_block($1,$2,true,$3)",
            &[&right, &left, &Uuid::new_v4()],
        )
        .await?;
    let conflict = stale
        .query_one(
            "SELECT app.send_direct_message($1,$2,'blocked',$3)",
            &[&left, &conversation, &Uuid::new_v4()],
        )
        .await
        .unwrap_err();
    assert_eq!(conflict.code(), Some(&SqlState::T_R_SERIALIZATION_FAILURE));
    stale.rollback().await?;
    assert_eq!(
        second
            .query_one(
                "SELECT count(*) FROM app.message WHERE conversation_id=$1",
                &[&conversation]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn pages_are_bounded_authorized_and_deleted_authors_are_anonymous() -> TestResult {
    let client = connection().await?;
    let (left, _) = account(&client).await?;
    let (right, _) = account(&client).await?;
    let (outsider, _) = account(&client).await?;
    friend(&client, left, right).await?;
    let conversation = resource(open_dm(&client, left, right, Uuid::new_v4()).await?);
    for body in ["one", "two", "three"] {
        resource(
            client
                .query_one(
                    "SELECT app.send_direct_message($1,$2,$3,$4)",
                    &[&left, &conversation, &body, &Uuid::new_v4()],
                )
                .await?
                .get(0),
        );
    }
    let rows=client.query("SELECT message_id,sent_at::text,body FROM app.read_conversation_messages($1,$2,NULL,NULL,2)",&[&right,&conversation]).await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<_, String>(2), "three");
    let before_id: Uuid = rows[1].get(0);
    let before_at: String = rows[1].get(1);
    let next = client
        .query(
            "SELECT body FROM app.read_conversation_messages($1,$2,$3::text::timestamptz,$4,2)",
            &[&right, &conversation, &before_at, &before_id],
        )
        .await?;
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].get::<_, String>(0), "one");
    assert_eq!(
        client
            .query(
                "SELECT * FROM app.read_conversation_messages($1,$2,NULL,NULL,101)",
                &[&right, &conversation]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INVALID_PARAMETER_VALUE)
    );
    assert_eq!(
        client
            .query(
                "SELECT * FROM app.read_conversation_messages($1,$2,NULL,NULL,2)",
                &[&outsider, &conversation]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    let source = operation(&client, left).await?;
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&source],
        )
        .await?;
    client.execute("UPDATE app.account SET state='deleted',auth_subject=NULL,deletion_requested_at=clock_timestamp(),deletion_finalized_at=clock_timestamp(),retirement_operation_id=$2 WHERE id=$1",&[&left,&source]).await?;
    assert_eq!(
        client
            .query_one(
                "SELECT author FROM app.read_conversation_messages($1,$2,NULL,NULL,1)",
                &[&right, &conversation]
            )
            .await?
            .get::<_, String>(0),
        "Deleted player"
    );
    let edit = client
        .execute(
            "UPDATE app.message SET body='rewritten' WHERE id=$1",
            &[&before_id],
        )
        .await
        .unwrap_err();
    assert_eq!(edit.code(), Some(&SqlState::CHECK_VIOLATION));
    Ok(())
}

/// Metadata fixture for contact SQL only, not a native game/format qualification.
pub(super) async fn participation_pair(
    client: &Client,
    left: Uuid,
    right: Uuid,
    mode: &str,
    days: i32,
    right_turns: i32,
) -> TestResult {
    let format = Uuid::new_v4();
    let profile = Uuid::new_v4();
    let legality = Uuid::new_v4();
    let match_id = Uuid::new_v4();
    let game = Uuid::new_v4();
    client
        .execute(
            "INSERT INTO app.format(id,key,name) VALUES($1,$1::uuid::text,'test-only')",
            &[&format],
        )
        .await?;
    client.execute("INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref) VALUES($1,$1::uuid::text,'test-only','test-only:rules')",&[&profile]).await?;
    client.execute("INSERT INTO app.legality_revision(id,source_ref,effective_at,imported_at) VALUES($1,'test-only:legality',clock_timestamp(),clock_timestamp())",&[&legality]).await?;
    client.execute("INSERT INTO app.match(id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) VALUES($1,$2,$3,$4,$5,false,'bo1','completed',1)",&[&match_id,&format,&profile,&legality,&mode]).await?;
    client.execute("INSERT INTO app.game(id,match_id,game_index,state,accepted_revision) VALUES($1,$2,1,'completed',1)",&[&game,&match_id]).await?;
    for (index, actor, turns) in [(0_i64, left, 3), (1_i64, right, right_turns)] {
        let seat = Uuid::new_v4();
        client.execute("INSERT INTO app.match_seat(id,match_id,account_id,seat_index,controller_kind) VALUES($1,$2,$3,$4,'human')",&[&seat,&match_id,&actor,&index]).await?;
        client.execute("INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision) VALUES($1,$2,$3,$4::integer::numeric,$4>=3,'test-only',clock_timestamp()-$5::integer*interval '24 hours',1)",&[&game,&seat,&actor,&turns,&days]).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn shared_contact_requires_both_humans_and_recent_evidence_in_either_mode() -> TestResult {
    let client = connection().await?;
    let (left, _) = account(&client).await?;
    for (mode, days, turns, expected) in [
        ("open", 13, 3, true),
        ("collection", 13, 3, true),
        ("open", 1, 2, false),
        ("open", 15, 3, false),
    ] {
        let (right, _) = account(&client).await?;
        participation_pair(&client, left, right, mode, days, turns).await?;
        assert_eq!(
            client
                .query_one("SELECT app.contact_allowed($1,$2)", &[&left, &right])
                .await?
                .get::<_, bool>(0),
            expected
        );
        if turns >= 3 {
            let boundary=client.query_one("SELECT app.contact_allowed_at($1,$2,greatest(p.completed_at,b.completed_at)+interval '336 hours'),app.contact_allowed_at($1,$2,greatest(p.completed_at,b.completed_at)+interval '336 hours 0.000001 seconds') FROM app.game_participation p JOIN app.game_participation b ON b.game_id=p.game_id AND b.account_id=$2 WHERE p.account_id=$1",&[&left,&right]).await?;
            assert!(boundary.get::<_, bool>(0));
            assert!(!boundary.get::<_, bool>(1));
        }
        if expected {
            resource(
                client
                    .query_one(
                        "SELECT app.change_block($1,$2,true,$3)",
                        &[&right, &left, &Uuid::new_v4()],
                    )
                    .await?
                    .get(0),
            );
            assert!(
                !client
                    .query_one("SELECT app.contact_allowed($1,$2)", &[&left, &right])
                    .await?
                    .get::<_, bool>(0)
            );
        }
    }
    assert!(!client.query_one("SELECT has_function_privilege('service_role','app.contact_allowed_at(uuid,uuid,timestamptz)','EXECUTE')",&[]).await?.get::<_,bool>(0));
    Ok(())
}
