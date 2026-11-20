//! Exact observed-message acknowledgment over guarded synthetic social fixtures.
use super::{TestResult, account, connection, deletion, social};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;
async fn credentials(client: &Client, subject: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0))
}
fn resource(value: Value) -> Uuid {
    assert_eq!(value["status"], "success", "{value}");
    value["data"]["resourceId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}
async fn send(
    client: &Client,
    actor: Uuid,
    conversation: Uuid,
) -> Result<Uuid, tokio_postgres::Error> {
    Ok(resource(
        client
            .query_one(
                "SELECT app.send_direct_message($1,$2,'Viewed text',$3)",
                &[&actor, &conversation, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    ))
}
async fn mark(
    client: &Client,
    subject: Uuid,
    session: Uuid,
    request: Uuid,
    conversation: Uuid,
    ids: &[Uuid],
) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.mark_direct_messages_read($1,$2,$3,$4,$5)",
            &[&subject, &session, &request, &conversation, &ids],
        )
        .await?
        .get(0))
}
async fn unread(client: &Client, actor: Uuid) -> Result<i64, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT count(*) FROM app.notification WHERE recipient_id=$1 AND read_at IS NULL",
            &[&actor],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn read_ack_is_exact_private_source_bound_and_preserves_later_arrivals() -> TestResult {
    let client = connection().await?;
    let (viewer, subject) = account(&client).await?;
    let (peer, peer_subject) = account(&client).await?;
    let (third, _) = account(&client).await?;
    let session = credentials(&client, subject).await?;
    let peer_session = credentials(&client, peer_subject).await?;
    social::friend(&client, viewer, peer).await?;
    social::friend(&client, peer, third).await?;
    let conversation = resource(
        client
            .query_one(
                "SELECT app.open_direct_conversation($1,$2,$3)",
                &[&viewer, &peer, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    let other = resource(
        client
            .query_one(
                "SELECT app.open_direct_conversation($1,$2,$3)",
                &[&peer, &third, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    let first = send(&client, peer, conversation).await?;
    let second = send(&client, peer, conversation).await?;
    let foreign = send(&client, third, other).await?;
    send(&client, viewer, conversation).await?;
    let request = Uuid::new_v4();
    let input = json!({"requestId":request,"conversationId":conversation,"messageIds":[first]});
    client.batch_execute("SET ROLE service_role").await?;
    let unknown: Value = client
        .query_one(
            "SELECT app.read_direct_read_command($1,$2,$3)",
            &[&subject, &session, &input],
        )
        .await?
        .get(0);
    assert_eq!(unknown["data"]["state"], "unknown");
    let accepted = mark(&client, subject, session, request, conversation, &[first]).await?;
    assert_eq!(resource(accepted.clone()), conversation);
    assert_eq!(
        client
            .query_one(
                "SELECT app.read_direct_read_command($1,$2,$3)",
                &[&subject, &session, &input]
            )
            .await?
            .get::<_, Value>(0)["data"]["response"],
        accepted
    );
    assert_eq!(
        client
            .query_one(
                "SELECT app.read_direct_read_command($1,$2,$3)",
                &[&peer_subject, &peer_session, &input]
            )
            .await?
            .get::<_, Value>(0)["data"]["state"],
        "unknown"
    );
    client.batch_execute("RESET ROLE").await?;
    let timestamp: String = client
        .query_one(
            "SELECT read_at::text FROM app.notification WHERE recipient_id=$1 AND source_ref=$2",
            &[&viewer, &first.to_string()],
        )
        .await?
        .get(0);
    let later = send(&client, peer, conversation).await?;
    assert_eq!(
        mark(&client, subject, session, request, conversation, &[first]).await?,
        accepted
    );
    assert_eq!(unread(&client, viewer).await?, 2);
    assert_eq!(unread(&client, peer).await?, 2);
    assert_eq!(client.query_one("SELECT read_at::text FROM app.notification WHERE recipient_id=$1 AND source_ref=$2", &[&viewer,&first.to_string()]).await?.get::<_,String>(0),timestamp);
    assert_eq!(
        mark(&client, subject, session, request, conversation, &[second]).await?["data"]["request"],
        "conflict"
    );
    assert_eq!(
        mark(
            &client,
            subject,
            session,
            Uuid::new_v4(),
            conversation,
            &[second, foreign]
        )
        .await?["data"]["conversation"],
        "conversation unavailable"
    );
    assert_eq!(unread(&client, viewer).await?, 2);
    resource(
        client
            .query_one(
                "SELECT app.change_block($1,$2,true,$3)",
                &[&viewer, &peer, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    resource(
        mark(
            &client,
            subject,
            session,
            Uuid::new_v4(),
            conversation,
            &[second],
        )
        .await?,
    );
    deletion::request(&client, viewer, Uuid::new_v4()).await?;
    resource(
        mark(
            &client,
            subject,
            session,
            Uuid::new_v4(),
            conversation,
            &[later],
        )
        .await?,
    );
    assert_eq!(unread(&client, viewer).await?, 0);
    assert_eq!(unread(&client, peer).await?, 2);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn read_ack_validates_array_shape_scope_and_capabilities() -> TestResult {
    let client = connection().await?;
    let (_, subject) = account(&client).await?;
    let session = credentials(&client, subject).await?;
    for ids in [
        vec![],
        vec![Uuid::nil()],
        vec![Uuid::new_v4(); 2],
        (0..17).map(|_| Uuid::new_v4()).collect(),
    ] {
        assert_eq!(
            mark(
                &client,
                subject,
                session,
                Uuid::new_v4(),
                Uuid::new_v4(),
                &ids
            )
            .await?["data"]["request"],
            "invalid"
        );
    }
    for sql in [
        "SELECT app.valid_direct_read_ids(ARRAY[[gen_random_uuid(),gen_random_uuid()]])",
        "SELECT app.valid_direct_read_ids('[0:0]={11111111-1111-4111-8111-111111111111}'::uuid[])",
        "SELECT app.valid_direct_read_ids(ARRAY[NULL::uuid])",
    ] {
        assert!(!client.query_one(sql, &[]).await?.get::<_, bool>(0));
    }
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for function in [
            "app.mark_direct_messages_read(uuid,uuid,uuid,uuid,uuid[])",
            "app.read_direct_read_command(uuid,uuid,jsonb)",
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
        assert!(!client.query_one("SELECT has_function_privilege($1,'app.valid_direct_read_ids(uuid[])','EXECUTE')", &[&role]).await?.get::<_,bool>(0));
    }
    client
        .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    assert_eq!(
        mark(
            &client,
            subject,
            session,
            Uuid::new_v4(),
            Uuid::new_v4(),
            &[Uuid::new_v4()]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    client.batch_execute("ROLLBACK").await?;
    Ok(())
}
