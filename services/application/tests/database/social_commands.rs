//! Actual session-bound social commands; synthetic Auth authority, no real messages.
use super::{TestResult, account, connection, deletion};
use serde_json::{Value, json};
use tokio_postgres::{Client, GenericClient, error::SqlState};
use uuid::Uuid;

async fn session(client: &Client, subject: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0))
}
async fn command<C: GenericClient + Sync>(
    client: &C,
    subject: Uuid,
    session: Uuid,
    input: &Value,
    lookup: bool,
) -> Result<Value, tokio_postgres::Error> {
    let sql = if lookup {
        "SELECT app.read_social_command($1,$2,$3)"
    } else {
        "SELECT app.execute_social_command($1,$2,$3)"
    };
    Ok(client
        .query_one(sql, &[&subject, &session, input])
        .await?
        .get(0))
}
fn friend(other: Uuid, action: &str) -> Value {
    json!({"kind":"friendship","command":{"requestId":Uuid::new_v4(),"otherAccountId":other,"action":action}})
}
fn resource(value: &Value) -> Uuid {
    assert_eq!(value["status"], "success", "{value}");
    value["data"]["resourceId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn session_social_commands_replay_exact_terms_and_keep_blocked_history() -> TestResult {
    let client = connection().await?;
    let (left, left_subject) = account(&client).await?;
    let (right, right_subject) = account(&client).await?;
    let left_session = session(&client, left_subject).await?;
    let right_session = session(&client, right_subject).await?;
    let request = friend(right, "request");
    let unknown = command(&client, left_subject, left_session, &request, true).await?;
    assert_eq!(
        unknown,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    let before: i64 = client
        .query_one(
            "SELECT count(*) FROM app.command_receipt WHERE account_id=$1",
            &[&left],
        )
        .await?
        .get(0);
    assert_eq!(before, 0);
    client.batch_execute("SET ROLE service_role").await?;
    let sent = command(&client, left_subject, left_session, &request, false).await?;
    resource(&sent);
    assert_eq!(
        command(&client, left_subject, left_session, &request, false).await?,
        sent
    );
    assert_eq!(
        command(&client, left_subject, left_session, &request, true).await?["data"]["response"],
        sent
    );
    let mut changed = request.clone();
    changed["command"]["action"] = json!("remove");
    for lookup in [false, true] {
        assert_eq!(
            command(&client, left_subject, left_session, &changed, lookup).await?,
            json!({"status":"fail","data":{"request":"conflict"}})
        );
    }
    assert_eq!(
        command(&client, right_subject, right_session, &request, true).await?["data"]["state"],
        "unknown"
    );
    assert_eq!(
        command(&client, left_subject, right_session, &request, true).await?["data"]["session"],
        "unauthorized"
    );
    resource(
        &command(
            &client,
            right_subject,
            right_session,
            &friend(left, "accept"),
            false,
        )
        .await?,
    );
    let open =
        json!({"kind":"open_dm","command":{"requestId":Uuid::new_v4(),"otherAccountId":right}});
    let conversation = resource(&command(&client, left_subject, left_session, &open, false).await?);
    let message = json!({"kind":"send_dm","command":{"requestId":Uuid::new_v4(),"conversationId":conversation,"body":"Exact 🎲 message"}});
    let first = command(&client, left_subject, left_session, &message, false).await?;
    resource(&first);
    assert_eq!(
        command(&client, left_subject, left_session, &message, false).await?,
        first
    );
    let mut changed = message.clone();
    changed["command"]["body"] = json!("Changed");
    assert_eq!(
        command(&client, left_subject, left_session, &changed, true).await?["data"]["request"],
        "conflict"
    );
    let block = json!({"kind":"block","command":{"requestId":Uuid::new_v4(),"otherAccountId":left,"blocked":true}});
    assert_eq!(
        resource(&command(&client, right_subject, right_session, &block, false).await?),
        left
    );
    let mut blocked = message.clone();
    blocked["command"]["requestId"] = json!(Uuid::new_v4());
    assert_eq!(
        command(&client, left_subject, left_session, &blocked, false).await?["data"]["contact"],
        "contact unavailable"
    );
    assert_eq!(
        command(&client, left_subject, left_session, &message, false).await?,
        first
    );
    assert_eq!(
        command(&client, left_subject, left_session, &blocked, true).await?["data"]["state"],
        "rejected"
    );
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.message WHERE conversation_id=$1",
                &[&conversation]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    let history = client
        .query_one(
            "SELECT body FROM app.read_conversation_messages($1,$2,NULL,NULL,100)",
            &[&right, &conversation],
        )
        .await?;
    assert_eq!(history.get::<_, String>(0), "Exact 🎲 message");
    deletion::request(&client, left, Uuid::new_v4()).await?;
    assert_eq!(
        command(&client, left_subject, left_session, &message, false).await?,
        first
    );
    let denied = friend(right, "remove");
    assert_eq!(
        command(&client, left_subject, left_session, &denied, false).await?["status"],
        "fail"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn social_facade_rejects_injected_shape_nil_ids_and_direct_prototype_execution() -> TestResult
{
    let client = connection().await?;
    let (_, subject) = account(&client).await?;
    let session = session(&client, subject).await?;
    let valid = friend(Uuid::new_v4(), "request");
    let mut injected = valid.clone();
    injected["command"]["actorId"] = json!(Uuid::new_v4());
    let mut nil = valid.clone();
    nil["command"]["requestId"] = json!(Uuid::nil());
    let mut wrong = valid.clone();
    wrong["command"]["otherAccountId"] = json!({"id":Uuid::new_v4()});
    for invalid in [Value::Null, json!([]), json!({}), injected, nil, wrong] {
        for lookup in [false, true] {
            assert_eq!(
                command(&client, subject, session, &invalid, lookup).await?,
                json!({"status":"fail","data":{"request":"invalid"}})
            );
        }
    }
    for function in [
        "app.change_friendship(uuid,uuid,text,uuid)",
        "app.change_block(uuid,uuid,boolean,uuid)",
        "app.open_direct_conversation(uuid,uuid,uuid)",
        "app.send_direct_message(uuid,uuid,text,uuid)",
        "app.social_command_terms(jsonb)",
    ] {
        assert!(
            !client
                .query_one(
                    "SELECT has_function_privilege('service_role',$1,'EXECUTE')",
                    &[&function]
                )
                .await?
                .get::<_, bool>(0)
        );
    }
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for function in [
            "app.execute_social_command(uuid,uuid,jsonb)",
            "app.read_social_command(uuid,uuid,jsonb)",
        ] {
            assert_eq!(
                client
                    .query_one(
                        "SELECT has_function_privilege($1,$2,'EXECUTE')",
                        &[&role, &function]
                    )
                    .await?
                    .get::<_, bool>(0),
                role == "service_role"
            );
        }
    }
    for lookup in [false, true] {
        client
            .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await?;
        assert_eq!(
            command(&client, subject, session, &valid, lookup)
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::T_R_SERIALIZATION_FAILURE)
        );
        client.batch_execute("ROLLBACK").await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn social_session_rechecks_authority_after_waiting_for_the_peer() -> TestResult {
    let locker = connection().await?;
    let (actor, subject) = account(&locker).await?;
    let (peer, _) = account(&locker).await?;
    let session = session(&locker, subject).await?;
    let waiter = connection().await?;
    let pid: i32 = waiter
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    waiter.batch_execute("SET ROLE service_role").await?;
    locker.batch_execute("BEGIN").await?;
    locker
        .query_one(
            "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
            &[&peer],
        )
        .await?;
    let input = friend(peer, "request");
    let request = input["command"]["requestId"].as_str().unwrap().to_owned();
    let task = tokio::spawn(async move { command(&waiter, subject, session, &input, false).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if locker
                .query_one("SELECT cardinality(pg_blocking_pids($1))>0", &[&pid])
                .await
                .unwrap()
                .get::<_, bool>(0)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    locker
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 day' WHERE id=$1",
            &[&subject],
        )
        .await?;
    locker.batch_execute("COMMIT").await?;
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(3), task).await???,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(
        locker
            .query_one(
                "SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND request_ref=$2",
                &[&actor, &request]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn opposing_social_requests_share_sorted_pair_locks_without_deadlock() -> TestResult {
    let locker = connection().await?;
    let (left, left_subject) = account(&locker).await?;
    let (right, right_subject) = account(&locker).await?;
    let left_session = session(&locker, left_subject).await?;
    let right_session = session(&locker, right_subject).await?;
    let a = connection().await?;
    let b = connection().await?;
    let pid_a: i32 = a.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    let pid_b: i32 = b.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    a.batch_execute("SET ROLE service_role").await?;
    b.batch_execute("SET ROLE service_role").await?;
    locker.batch_execute("BEGIN").await?;
    locker
        .query_one(
            "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
            &[&left.max(right)],
        )
        .await?;
    let ta = tokio::spawn(async move {
        command(
            &a,
            left_subject,
            left_session,
            &friend(right, "request"),
            false,
        )
        .await
    });
    let tb = tokio::spawn(async move {
        command(
            &b,
            right_subject,
            right_session,
            &friend(left, "request"),
            false,
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        loop {
            let blocked:bool=locker.query_one("SELECT cardinality(pg_blocking_pids($1))>0 AND cardinality(pg_blocking_pids($2))>0", &[&pid_a,&pid_b]).await.unwrap().get(0);
            if blocked {break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await?;
    locker.batch_execute("COMMIT").await?;
    let (ra, rb) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::join!(ta, tb)
    })
    .await?;
    assert_eq!(resource(&ra??), resource(&rb??));
    assert_eq!(locker.query_one("SELECT count(*) FROM app.friendship WHERE account_low_id=least($1::uuid,$2::uuid) AND account_high_id=greatest($1::uuid,$2::uuid)", &[&left,&right]).await?.get::<_,i64>(0),1);
    Ok(())
}
