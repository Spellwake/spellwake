//! Original-claim social reads over synthetic guarded domain fixtures.
use super::{TestResult, account, connection, deletion, set_auth_claims, social};
use serde_json::{Value, json};
use spellwake_application::social::{
    DirectMessagePage, MessageCursor, SocialPeoplePage, SocialStatus,
};
use tokio_postgres::Client;
use uuid::Uuid;

async fn people(
    client: &Client,
    kind: &str,
    query: Option<&str>,
    after: Option<Uuid>,
    limit: i32,
) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.read_my_social_people($1,$2,$3,$4)",
            &[&kind, &query, &after, &limit],
        )
        .await?
        .get(0))
}
async fn messages(
    client: &Client,
    conversation: Uuid,
    before: Option<&MessageCursor>,
    limit: i32,
) -> Result<Value, tokio_postgres::Error> {
    let at = before.map(|cursor| cursor.sent_at.to_rfc3339());
    let id = before.map(|cursor| cursor.id);
    Ok(client
        .query_one(
            "SELECT app.read_my_direct_messages($1,$2::text::timestamptz,$3,$4)",
            &[&conversation, &at, &id, &limit],
        )
        .await?
        .get(0))
}
fn data(value: Value) -> Value {
    assert_eq!(value["status"], "success", "{value}");
    value["data"].clone()
}
fn resource(value: Value) -> Uuid {
    data(value)["resourceId"].as_str().unwrap().parse().unwrap()
}

async fn status(
    client: &Client,
    peer: Option<Uuid>,
) -> Result<SocialStatus, Box<dyn std::error::Error + Send + Sync>> {
    let value: Value = client
        .query_one("SELECT app.read_my_social_status($1)", &[&peer])
        .await?
        .get(0);
    let result: SocialStatus = serde_json::from_value(data(value))?;
    result.validate(peer)?;
    Ok(result)
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn global_unread_is_independent_of_thread_pages_and_selected_peer() -> TestResult {
    let client = connection().await?;
    let (actor, subject) = account(&client).await?;
    let (a, _) = account(&client).await?;
    let (b, _) = account(&client).await?;
    let (_, other_subject) = account(&client).await?;
    let mut conversations = Vec::new();
    for peer in [a, b] {
        social::friend(&client, actor, peer).await?;
        let cid = resource(
            client
                .query_one(
                    "SELECT app.open_direct_conversation($1,$2,$3)",
                    &[&actor, &peer, &Uuid::new_v4()],
                )
                .await?
                .get(0),
        );
        for body in ["first", "second"] {
            resource(
                client
                    .query_one(
                        "SELECT app.send_direct_message($1,$2,$3,$4)",
                        &[&peer, &cid, &body, &Uuid::new_v4()],
                    )
                    .await?
                    .get(0),
            );
        }
        // Outgoing messages and the other recipient's notifications are not this actor's total.
        resource(
            client
                .query_one(
                    "SELECT app.send_direct_message($1,$2,'outgoing',$3)",
                    &[&actor, &cid, &Uuid::new_v4()],
                )
                .await?
                .get(0),
        );
        conversations.push(cid);
    }
    set_auth_claims(&client, subject).await?;
    let first = data(people(&client, "threads", None, None, 1).await?);
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["items"][0]["unreadMessages"], "2");
    assert_eq!(
        status(&client, None).await?.unread_messages.to_string(),
        "4"
    );
    let selected = status(&client, Some(b)).await?;
    assert_eq!(selected.unread_messages.to_string(), "4");
    assert_eq!(selected.person.unwrap().unread_messages.to_string(), "2");
    set_auth_claims(&client, other_subject).await?;
    assert_eq!(
        status(&client, None).await?.unread_messages.to_string(),
        "0"
    );
    client.batch_execute("RESET ROLE").await?;
    client.execute("UPDATE app.notification SET read_at=statement_timestamp() WHERE recipient_id=$1 AND source_ref IN (SELECT id::text FROM app.message WHERE conversation_id=$2)", &[&actor,&conversations[0]]).await?;
    set_auth_claims(&client, subject).await?;
    assert_eq!(
        status(&client, Some(a)).await?.unread_messages.to_string(),
        "2"
    );
    let result: Value = client
        .query_one("SELECT app.read_my_social_status($1)", &[&Uuid::nil()])
        .await?
        .get(0);
    assert_eq!(result["data"]["page"], "invalid");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn selected_status_reuses_thread_visibility_and_masks_retired_identity() -> TestResult {
    let client = connection().await?;
    let (actor, subject) = account(&client).await?;
    let (friend, _) = account(&client).await?;
    let (blocked, _) = account(&client).await?;
    let (recent, _) = account(&client).await?;
    let (stranger, _) = account(&client).await?;
    social::friend(&client, actor, friend).await?;
    resource(
        client
            .query_one(
                "SELECT app.change_block($1,$2,true,$3)",
                &[&actor, &blocked, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    social::participation_pair(&client, actor, recent, "open", 1, 3).await?;
    for peer in [friend, blocked, recent, stranger] {
        deletion::request(&client, peer, Uuid::new_v4()).await?;
    }
    set_auth_claims(&client, subject).await?;
    for peer in [friend, blocked, recent] {
        assert!(status(&client, Some(peer)).await?.person.is_some());
    }
    assert!(status(&client, Some(stranger)).await?.person.is_none());
    assert!(
        status(&client, Some(Uuid::new_v4()))
            .await?
            .person
            .is_none()
    );
    assert!(status(&client, Some(actor)).await?.person.is_none());
    client.batch_execute("RESET ROLE").await?;
    let retired: Value = client
        .query_one("SELECT app.retire_account($1)", &[&friend])
        .await?
        .get(0);
    assert_eq!(retired["data"]["state"], "retired");
    set_auth_claims(&client, subject).await?;
    // Retirement removes the relationship and there is no retained DM to reveal a tombstone.
    assert!(status(&client, Some(friend)).await?.person.is_none());
    client.batch_execute("RESET ROLE").await?;
    for role in ["anon", "service_role", "supabase_auth_admin"] {
        let public:bool=client.query_one("SELECT has_function_privilege($1::name,'app.read_my_social_status(uuid)','EXECUTE')",&[&role]).await?.get(0);
        assert!(!public);
    }
    let public:bool=client.query_one("SELECT has_function_privilege('authenticated','app.read_my_social_status(uuid)','EXECUTE')",&[]).await?.get(0);
    assert!(public);
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        let allowed:bool=client.query_one("SELECT has_function_privilege($1::name,'app.social_thread_candidates(uuid,timestamptz,uuid)','EXECUTE')",&[&role]).await?.get(0);
        assert!(!allowed);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn directory_and_thread_pages_keep_names_scoped_and_literal() -> TestResult {
    let client = connection().await?;
    let (actor, subject) = account(&client).await?;
    let (a, _) = account(&client).await?;
    let (b, _) = account(&client).await?;
    let (pending, _) = account(&client).await?;
    let prefix = format!("rs_{}", &Uuid::new_v4().simple().to_string()[..10]);
    for (id, suffix) in [(actor, "owner"), (a, "i"), (b, "b"), (pending, "pending")] {
        let name = format!("{prefix}_{suffix}");
        let name = if id == a { name.to_uppercase() } else { name };
        client
            .execute(
                "UPDATE app.account SET username=$2 WHERE id=$1",
                &[&id, &name],
            )
            .await?;
    }
    for (from, to) in [(actor, a), (b, actor), (actor, pending)] {
        resource(
            client
                .query_one(
                    "SELECT app.change_friendship($1,$2,'request',$3)",
                    &[&from, &to, &Uuid::new_v4()],
                )
                .await?
                .get(0),
        );
    }
    deletion::request(&client, pending, Uuid::new_v4()).await?;
    set_auth_claims(&client, subject).await?;
    let mut after = None;
    let mut ids = Vec::new();
    for _ in 0..3 {
        let page: SocialPeoplePage = serde_json::from_value(data(
            people(&client, "directory", Some(&prefix.to_uppercase()), after, 1).await?,
        ))?;
        page.validate(Some(&prefix), after, 1)?;
        assert_eq!(page.account_id, actor);
        ids.extend(page.items.iter().map(|person| person.id));
        after = page.next_cursor;
    }
    ids.sort();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(after, None);
    assert!(
        data(people(&client, "directory", Some("%_"), None, 100).await?)["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        data(people(&client, "directory", Some("İ"), None, 100).await?)["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let threads: SocialPeoplePage =
        serde_json::from_value(data(people(&client, "threads", None, None, 100).await?))?;
    threads.validate(None, None, 100)?;
    assert_eq!(threads.items.len(), 3);
    assert_eq!(
        threads
            .items
            .iter()
            .find(|person| person.id == a)
            .unwrap()
            .relationship,
        spellwake_application::social::Relationship::OutgoingRequest
    );
    assert_eq!(
        threads
            .items
            .iter()
            .find(|person| person.id == b)
            .unwrap()
            .relationship,
        spellwake_application::social::Relationship::IncomingRequest
    );
    assert!(
        !threads
            .items
            .iter()
            .find(|person| person.id == pending)
            .unwrap()
            .can_contact
    );
    let serialized = serde_json::to_string(&threads)?;
    assert!(!serialized.contains(&subject.to_string()));
    assert!(!serialized.contains("authSubject"));
    assert_eq!(
        people(&client, "threads", Some("ignored"), None, 100).await?["data"]["page"],
        "invalid"
    );
    assert_eq!(
        people(&client, "directory", Some(" "), None, 100).await?["data"]["page"],
        "invalid"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn direct_history_remains_private_readable_after_block_and_anonymous_after_retirement()
-> TestResult {
    let client = connection().await?;
    let (left, left_subject) = account(&client).await?;
    let (right, right_subject) = account(&client).await?;
    let (_, outsider_subject) = account(&client).await?;
    social::friend(&client, left, right).await?;
    let conversation = resource(
        client
            .query_one(
                "SELECT app.open_direct_conversation($1,$2,$3)",
                &[&left, &right, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
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
    let original_name: String = client
        .query_one("SELECT username FROM app.account WHERE id=$1", &[&left])
        .await?
        .get(0);
    set_auth_claims(&client, right_subject).await?;
    let first: DirectMessagePage =
        serde_json::from_value(data(messages(&client, conversation, None, 2).await?))?;
    first.validate(conversation, None, 2)?;
    assert_eq!(first.account_id, right);
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.person.unread_messages.to_string(), "3");
    assert!(first.person.can_contact);
    let second: DirectMessagePage = serde_json::from_value(data(
        messages(&client, conversation, first.next_cursor.as_ref(), 2).await?,
    ))?;
    second.validate(conversation, first.next_cursor.as_ref(), 2)?;
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].body, "one");
    assert!(
        first
            .items
            .iter()
            .all(|one| second.items.iter().all(|two| one.id != two.id))
    );
    set_auth_claims(&client, outsider_subject).await?;
    assert_eq!(
        messages(&client, conversation, None, 2).await?,
        json!({"status":"fail","data":{"conversation":"unavailable"}})
    );
    assert!(
        data(people(&client, "threads", None, None, 100).await?)["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    client.batch_execute("RESET ROLE").await?;
    resource(
        client
            .query_one(
                "SELECT app.change_block($1,$2,true,$3)",
                &[&right, &left, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    set_auth_claims(&client, right_subject).await?;
    let blocked: DirectMessagePage =
        serde_json::from_value(data(messages(&client, conversation, None, 16).await?))?;
    blocked.validate(conversation, None, 16)?;
    assert!(!blocked.person.can_contact);
    assert!(blocked.person.is_blocked);
    assert_eq!(blocked.items.len(), 3);
    client.batch_execute("RESET ROLE").await?;
    deletion::request(&client, left, Uuid::new_v4()).await?;
    let retired: Value = client
        .query_one("SELECT app.retire_account($1)", &[&left])
        .await?
        .get(0);
    assert_eq!(retired["data"]["state"], "retired");
    set_auth_claims(&client, right_subject).await?;
    let anonymous: DirectMessagePage =
        serde_json::from_value(data(messages(&client, conversation, None, 16).await?))?;
    anonymous.validate(conversation, None, 16)?;
    assert!(anonymous.person.is_deleted);
    assert!(
        anonymous
            .items
            .iter()
            .all(|message| message.author == "Deleted player")
    );
    assert!(!serde_json::to_string(&anonymous)?.contains(&original_name));
    assert!(
        data(people(&client, "directory", Some(&original_name), None, 100).await?)["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let threads = data(people(&client, "threads", None, None, 100).await?);
    assert_eq!(threads["items"][0]["username"], "Deleted player");
    assert!(!serde_json::to_string(&threads)?.contains(&left_subject.to_string()));
    let current = status(&client, Some(left)).await?;
    assert!(current.person.unwrap().is_deleted);
    assert_eq!(current.unread_messages.to_string(), "3");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn recent_thread_candidates_require_both_real_participants_in_either_mode() -> TestResult {
    let client = connection().await?;
    let (actor, subject) = account(&client).await?;
    let mut expected = Vec::new();
    for (mode, days, turns, included) in [
        ("open", 13, 3, true),
        ("collection", 2, 3, true),
        ("open", 15, 3, false),
        ("collection", 1, 2, false),
    ] {
        let (peer, _) = account(&client).await?;
        social::participation_pair(&client, actor, peer, mode, days, turns).await?;
        if included {
            expected.push(peer);
        }
    }
    set_auth_claims(&client, subject).await?;
    let page: SocialPeoplePage =
        serde_json::from_value(data(people(&client, "threads", None, None, 100).await?))?;
    page.validate(None, None, 100)?;
    let actual = page
        .items
        .iter()
        .map(|person| person.id)
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(actual, expected);
    assert!(page.items.iter().all(|person| person.can_contact
        && person.conversation_id.is_none()
        && person.unread_messages.is_zero()));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn social_read_capabilities_and_original_session_authority_are_explicit() -> TestResult {
    let client = connection().await?;
    let (_, subject) = account(&client).await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for function in [
            "app.read_my_social_people(text,text,uuid,integer)",
            "app.read_my_direct_messages(uuid,timestamptz,uuid,integer)",
        ] {
            assert_eq!(
                client
                    .query_one(
                        "SELECT has_function_privilege($1,$2,'EXECUTE')",
                        &[&role, &function]
                    )
                    .await?
                    .get::<_, bool>(0),
                role == "authenticated"
            );
        }
        assert!(!client.query_one("SELECT has_function_privilege($1,'app.social_person_snapshot(uuid,uuid,timestamptz)','EXECUTE')", &[&role]).await?.get::<_,bool>(0));
    }
    assert!(!client.query_one("SELECT has_function_privilege('service_role','app.read_conversation_messages(uuid,uuid,timestamptz,uuid,integer)','EXECUTE')", &[]).await?.get::<_,bool>(0));
    set_auth_claims(&client, subject).await?;
    assert_eq!(
        people(&client, "threads", None, None, 101).await?["data"]["page"],
        "invalid"
    );
    let malformed: Value = client
        .query_one(
            "SELECT app.read_my_direct_messages($1,clock_timestamp(),NULL,16)",
            &[&Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(malformed["data"]["page"], "invalid");
    let other = connection().await?;
    other
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 day' WHERE id=$1",
            &[&subject],
        )
        .await?;
    assert_eq!(
        people(&client, "threads", None, None, 100).await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn maximum_unicode_messages_page_under_the_existing_response_envelope() -> TestResult {
    let client = connection().await?;
    let (left, _) = account(&client).await?;
    let (right, subject) = account(&client).await?;
    social::friend(&client, left, right).await?;
    let conversation = resource(
        client
            .query_one(
                "SELECT app.open_direct_conversation($1,$2,$3)",
                &[&left, &right, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    let body = "🎲".repeat(4000);
    for _ in 0..17 {
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
    set_auth_claims(&client, subject).await?;
    assert_eq!(
        messages(&client, conversation, None, 17).await?["data"]["page"],
        "invalid"
    );
    let first: DirectMessagePage =
        serde_json::from_value(data(messages(&client, conversation, None, 16).await?))?;
    first.validate(conversation, None, 16)?;
    assert_eq!(first.items.len(), 16);
    assert!(first.items.iter().all(|message| message.body == body));
    assert!(serde_json::to_vec(&first)?.len() < 1024 * 1024);
    let next: DirectMessagePage = serde_json::from_value(data(
        messages(&client, conversation, first.next_cursor.as_ref(), 16).await?,
    ))?;
    next.validate(conversation, first.next_cursor.as_ref(), 16)?;
    assert_eq!(next.items.len(), 1);
    assert_eq!(next.items[0].body, body);
    assert!(
        !first
            .items
            .iter()
            .any(|message| message.id == next.items[0].id)
    );
    Ok(())
}
