use super::{TestResult, account, connection, decks::library};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;

async fn session_for(
    client: &Client,
    subject: Uuid,
) -> Result<Uuid, Box<dyn std::error::Error + Send + Sync>> {
    let claims: Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    Ok(claims["session_id"].as_str().unwrap().parse()?)
}
async fn settings(client: &Client, deck: Uuid) -> Result<Value, tokio_postgres::Error> {
    Ok(client.query_one("SELECT jsonb_build_object('name',name,'mode',mode,'formatId',format_id,'appearance',jsonb_build_object('backId',back_id,'sleeveId',sleeve_id,'diceId',dice_id,'playmatId',playmat_id)) FROM app.deck WHERE id=$1", &[&deck]).await?.get(0))
}
async fn change(
    client: &Client,
    subject: Uuid,
    session: Uuid,
    request: Uuid,
    deck: Uuid,
    expected: &Value,
    next: &Value,
) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.change_deck_settings_command($1,$2,$3,$4,$5,$6)",
            &[&subject, &session, &request, &deck, expected, next],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn settings_are_atomic_recoverable_and_independent_of_contents() -> TestResult {
    let client = connection().await?;
    let (actor, subject, deck, _, _) = library(&client).await?;
    let session = session_for(&client, subject).await?;
    let original: Value = client
        .query_one(
            "SELECT app.save_deck_contents($1,$2,NULL,'[]',$3)",
            &[&actor, &deck, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    let original_revision: Uuid = original["data"]["resourceId"].as_str().unwrap().parse()?;
    let new_dice = Uuid::new_v4();
    client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,'dice','test-only:settings','active',true)", &[&new_dice]).await?;
    client.execute("INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,state) VALUES($1,$2,'free','active')", &[&actor,&new_dice]).await?;
    let initial = settings(&client, deck).await?;
    let mut next = initial.clone();
    next["name"] = json!("Renamed");
    next["mode"] = json!("open");
    next["appearance"]["diceId"] = json!(new_dice);
    let request = Uuid::new_v4();
    client.batch_execute("SET ROLE service_role").await?;
    let accepted = change(&client, subject, session, request, deck, &initial, &next).await?;
    assert_eq!(
        accepted,
        json!({"status":"success","data":{"resourceId":deck}})
    );
    assert_eq!(
        change(&client, subject, session, request, deck, &initial, &next).await?,
        accepted
    );
    let command = json!({"requestId":request,"deckId":deck,"expected":initial,"settings":next});
    let proof: Value = client
        .query_one(
            "SELECT app.read_deck_command($1,$2,'settings',$3)",
            &[&subject, &session, &command],
        )
        .await?
        .get(0);
    assert_eq!(
        proof,
        json!({"status":"success","data":{"state":"accepted","response":accepted}})
    );
    let mut altered = command.clone();
    altered["settings"]["name"] = json!("Changed intent");
    let conflict: Value = client
        .query_one(
            "SELECT app.read_deck_command($1,$2,'settings',$3)",
            &[&subject, &session, &altered],
        )
        .await?
        .get(0);
    assert_eq!(
        conflict,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    assert_eq!(
        change(&client, subject, session, request, deck, &initial, &initial).await?,
        conflict
    );
    let preserved: Uuid = client
        .query_one(
            "SELECT current_revision_id FROM app.deck WHERE id=$1",
            &[&deck],
        )
        .await?
        .get(0);
    assert_eq!(preserved, original_revision);
    // A later contents write has a different baseline and stays valid.
    let saved: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$3,$4,$5,'[]')",
            &[
                &subject,
                &session,
                &Uuid::new_v4(),
                &deck,
                &original_revision,
            ],
        )
        .await?
        .get(0);
    assert_eq!(saved["status"], "success");
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(settings(&client, deck).await?, next);
    let defaults: String=client.query_one("SELECT d.name FROM app.deck d JOIN app.profile p ON p.account_id=d.account_id WHERE d.id=$1 AND d.back_id=p.default_back_id", &[&deck]).await?.get(0);
    assert_eq!(defaults, "Renamed");
    let profile_dice: Uuid = client
        .query_one(
            "SELECT default_dice_id FROM app.profile WHERE account_id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    assert_ne!(profile_dice, new_dice);
    client
        .execute(
            "UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1",
            &[&actor],
        )
        .await?;
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        change(&client, subject, session, request, deck, &initial, &next).await?,
        accepted
    );
    assert_eq!(
        change(
            &client,
            subject,
            session,
            Uuid::new_v4(),
            deck,
            &next,
            &initial
        )
        .await?,
        json!({"status":"fail","data":{"account":"account unavailable"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn settings_rejections_preserve_all_fields_and_bind_the_original_terms() -> TestResult {
    let client = connection().await?;
    let (_, subject, deck, _, _) = library(&client).await?;
    let session = session_for(&client, subject).await?;
    let initial = settings(&client, deck).await?;
    let mut next = initial.clone();
    next["name"] = json!("Must not leak through");
    next["appearance"]["sleeveId"] = json!(Uuid::new_v4());
    client.batch_execute("SET ROLE service_role").await?;
    let request = Uuid::new_v4();
    let denied = change(&client, subject, session, request, deck, &initial, &next).await?;
    assert_eq!(
        denied,
        json!({"status":"fail","data":{"cosmetics":"cosmetic selection unavailable"}})
    );
    assert_eq!(
        change(&client, subject, session, request, deck, &initial, &next).await?,
        denied
    );
    next["appearance"] = initial["appearance"].clone();
    next["formatId"] = json!(Uuid::new_v4());
    assert_eq!(
        change(
            &client,
            subject,
            session,
            Uuid::new_v4(),
            deck,
            &initial,
            &next
        )
        .await?,
        json!({"status":"fail","data":{"deck":"invalid deck metadata"}})
    );
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(settings(&client, deck).await?, initial);
    let (_, other) = account(&client).await?;
    let other_session = session_for(&client, other).await?;
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        change(
            &client,
            other,
            other_session,
            Uuid::new_v4(),
            deck,
            &initial,
            &initial
        )
        .await?,
        json!({"status":"fail","data":{"deck":"deck unavailable"}})
    );
    assert_eq!(
        change(
            &client,
            subject,
            other_session,
            Uuid::new_v4(),
            deck,
            &initial,
            &initial
        )
        .await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn simultaneous_settings_changes_allow_only_one_observed_baseline() -> TestResult {
    let left = connection().await?;
    let right = connection().await?;
    let (_, subject, deck, _, _) = library(&left).await?;
    let session = session_for(&left, subject).await?;
    let initial = settings(&left, deck).await?;
    let mut a = initial.clone();
    a["name"] = json!("Window A");
    let mut b = initial.clone();
    b["name"] = json!("Window B");
    left.batch_execute("SET ROLE service_role").await?;
    right.batch_execute("SET ROLE service_role").await?;
    let (a, b) = tokio::join!(
        change(&left, subject, session, Uuid::new_v4(), deck, &initial, &a),
        change(&right, subject, session, Uuid::new_v4(), deck, &initial, &b)
    );
    let results = [a?, b?];
    assert_eq!(
        results.iter().filter(|r| r["status"] == "success").count(),
        1
    );
    assert_eq!(results.iter().filter(|r| **r==json!({"status":"fail","data":{"settings":"deck settings changed; reload before saving"}})).count(),1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn settings_shape_and_function_permissions_do_not_open_client_writes() -> TestResult {
    let client = connection().await?;
    let (_, subject, deck, _, _) = library(&client).await?;
    let session = session_for(&client, subject).await?;
    let initial = settings(&client, deck).await?;
    client.batch_execute("SET ROLE service_role").await?;
    for malformed in [
        Value::Null,
        json!([]),
        json!("invalid"),
        json!({}),
        {
            let mut value = initial.clone();
            value["appearance"]
                .as_object_mut()
                .unwrap()
                .remove("playmatId");
            value
        },
        {
            let mut value = initial.clone();
            value["appearance"]["playmatId"] = json!(Uuid::nil());
            value
        },
    ] {
        assert_eq!(
            change(
                &client,
                subject,
                session,
                Uuid::new_v4(),
                deck,
                &initial,
                &malformed
            )
            .await?,
            json!({"status":"fail","data":{"request":"invalid"}})
        );
    }
    client.batch_execute("RESET ROLE").await?;
    for role in ["anon", "authenticated"] {
        let allowed: bool=client.query_one("SELECT has_function_privilege($1,'app.change_deck_settings_command(uuid,uuid,uuid,uuid,jsonb,jsonb)','EXECUTE')", &[&role]).await?.get(0);
        assert!(!allowed);
    }
    let count: i64=client.query_one("SELECT count(*) FROM app.command_receipt WHERE target_ref=$1 AND scope='deck_settings'", &[&deck.to_string()]).await?.get(0);
    assert_eq!(count, 0);
    Ok(())
}
