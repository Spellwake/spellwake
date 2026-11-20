use super::{TestResult, account, connection, decks::library};
use serde_json::{Value, json};
use tokio_postgres::Client;
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
async fn defaults(client: &Client, actor: Uuid) -> Result<Value, tokio_postgres::Error> {
    Ok(client.query_one("SELECT jsonb_build_object('backId',default_back_id,'sleeveId',default_sleeve_id,'diceId',default_dice_id,'playmatId',default_playmat_id) FROM app.profile WHERE account_id=$1", &[&actor]).await?.get(0))
}
async fn change(
    client: &Client,
    subject: Uuid,
    session: Uuid,
    request: Uuid,
    expected: &Value,
    appearance: &Value,
) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.change_profile_defaults_command($1,$2,$3,$4,$5)",
            &[&subject, &session, &request, expected, appearance],
        )
        .await?
        .get(0))
}
async fn lookup(
    client: &Client,
    subject: Uuid,
    session: Uuid,
    command: &Value,
) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.read_profile_defaults_command($1,$2,$3)",
            &[&subject, &session, command],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn defaults_recover_exact_terms_and_only_new_decks_copy_the_selection() -> TestResult {
    let client = connection().await?;
    let (actor, subject, old_deck, _, format) = library(&client).await?;
    let session = session(&client, subject).await?;
    let original = defaults(&client, actor).await?;
    let dice = Uuid::new_v4();
    client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,'dice','test-only:defaults','active',true)", &[&dice]).await?;
    client.execute("INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,state) VALUES($1,$2,'free','active')", &[&actor,&dice]).await?;
    let mut next = original.clone();
    next["diceId"] = json!(dice);
    let request = Uuid::new_v4();
    let command = json!({"requestId":request,"expected":original,"appearance":next});
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        lookup(&client, subject, session, &command).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    let accepted = change(&client, subject, session, request, &original, &next).await?;
    assert_eq!(accepted["status"], "success");
    assert_eq!(
        change(&client, subject, session, request, &original, &next).await?,
        accepted
    );
    assert_eq!(
        lookup(&client, subject, session, &command).await?,
        json!({"status":"success","data":{"state":"accepted","response":accepted}})
    );
    let mut altered = command.clone();
    altered["expected"] = next.clone();
    assert_eq!(
        lookup(&client, subject, session, &altered).await?,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    assert_eq!(
        change(&client, subject, session, request, &next, &next).await?,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    let created: Value = client
        .query_one(
            "SELECT app.create_deck_command($1,$2,$3,'After defaults','collection',$4)",
            &[&subject, &session, &Uuid::new_v4(), &format],
        )
        .await?
        .get(0);
    let new_deck: Uuid = created["data"]["resourceId"].as_str().unwrap().parse()?;
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(defaults(&client, actor).await?, next);
    let old_dice: Uuid = client
        .query_one("SELECT dice_id FROM app.deck WHERE id=$1", &[&old_deck])
        .await?
        .get(0);
    let new_dice: Uuid = client
        .query_one("SELECT dice_id FROM app.deck WHERE id=$1", &[&new_deck])
        .await?
        .get(0);
    assert_eq!(old_dice.to_string(), original["diceId"].as_str().unwrap());
    assert_eq!(new_dice, dice);
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&actor]).await?;
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        change(&client, subject, session, request, &original, &next).await?,
        accepted
    );
    assert_eq!(
        change(&client, subject, session, Uuid::new_v4(), &next, &original).await?,
        json!({"status":"fail","data":{"account":"account unavailable"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn defaults_reject_unowned_slots_and_serialize_stale_concurrent_edits() -> TestResult {
    let left = connection().await?;
    let right = connection().await?;
    let (actor, subject, _, _, _) = library(&left).await?;
    let session = session(&left, subject).await?;
    let original = defaults(&left, actor).await?;
    let mut invalid = original.clone();
    invalid["sleeveId"] = json!(Uuid::new_v4());
    left.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        change(&left, subject, session, Uuid::new_v4(), &original, &invalid).await?,
        json!({"status":"fail","data":{"cosmetics":"cosmetic selection unavailable"}})
    );
    left.batch_execute("RESET ROLE").await?;
    assert_eq!(defaults(&left, actor).await?, original);
    let mut options = Vec::new();
    for _ in 0..2 {
        let dice = Uuid::new_v4();
        left.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,'dice','test-only:concurrent-defaults','active',true)", &[&dice]).await?;
        left.execute("INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,state) VALUES($1,$2,'free','active')", &[&actor,&dice]).await?;
        let mut next = original.clone();
        next["diceId"] = json!(dice);
        options.push(next);
    }
    left.batch_execute("SET ROLE service_role").await?;
    right.batch_execute("SET ROLE service_role").await?;
    let (a, b) = tokio::join!(
        change(
            &left,
            subject,
            session,
            Uuid::new_v4(),
            &original,
            &options[0]
        ),
        change(
            &right,
            subject,
            session,
            Uuid::new_v4(),
            &original,
            &options[1]
        )
    );
    let results = [a?, b?];
    assert_eq!(
        results.iter().filter(|r| r["status"] == "success").count(),
        1
    );
    assert_eq!(results.iter().filter(|r| **r==json!({"status":"fail","data":{"defaults":"account defaults changed; reload before saving"}})).count(),1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn defaults_recovery_cannot_read_another_owner_and_clients_cannot_write() -> TestResult {
    let client = connection().await?;
    let (actor, subject, _, _, _) = library(&client).await?;
    let session = session(&client, subject).await?;
    let (_, other) = account(&client).await?;
    let other_session = client
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&other],
        )
        .await?
        .get::<_, Uuid>(0);
    let original = defaults(&client, actor).await?;
    let request = Uuid::new_v4();
    let command = json!({"requestId":request,"expected":original,"appearance":original});
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        change(&client, subject, session, request, &original, &original).await?["status"],
        "success"
    );
    assert_eq!(
        lookup(&client, other, other_session, &command).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    assert_eq!(
        change(
            &client,
            subject,
            other_session,
            Uuid::new_v4(),
            &original,
            &original
        )
        .await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    for malformed in [json!({}), Value::Null, json!([]), {
        let mut value = command.clone();
        value["appearance"]
            .as_object_mut()
            .unwrap()
            .remove("playmatId");
        value
    }] {
        assert_eq!(
            lookup(&client, subject, session, &malformed).await?,
            json!({"status":"fail","data":{"request":"invalid"}})
        );
    }
    client.batch_execute("RESET ROLE").await?;
    for role in ["anon", "authenticated"] {
        for function in [
            "app.change_profile_defaults_command(uuid,uuid,uuid,jsonb,jsonb)",
            "app.read_profile_defaults_command(uuid,uuid,jsonb)",
        ] {
            assert!(
                !client
                    .query_one(
                        "SELECT has_function_privilege($1,$2,'EXECUTE')",
                        &[&role, &function]
                    )
                    .await?
                    .get::<_, bool>(0)
            );
        }
    }
    Ok(())
}
