//! Deck face cards: the card in a deck whose official art is shown on its box.
use super::{TestResult, connection, decks::library};
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
async fn face(client: &Client, deck: Uuid) -> Result<Option<Uuid>, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT face_definition_id FROM app.deck WHERE id=$1",
            &[&deck],
        )
        .await?
        .get(0))
}
async fn revision(client: &Client, deck: Uuid) -> Result<Option<Uuid>, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT current_revision_id FROM app.deck WHERE id=$1",
            &[&deck],
        )
        .await?
        .get(0))
}
fn entry(definition: Uuid, quantity: &str) -> Value {
    json!({"definitionId":definition,"printingId":null,"section":"main","quantity":quantity})
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn a_deck_keeps_a_face_card_from_its_contents_with_official_art() -> TestResult {
    let client = connection().await?;
    let (_, subject, deck, first, _) = library(&client).await?;
    let session = session_for(&client, subject).await?;
    let second = Uuid::new_v4();
    let outside = Uuid::new_v4();
    for (id, name) in [(second, "Second card"), (outside, "Outside card")] {
        client
            .execute(
                "INSERT INTO app.card_definition(id,oracle_ref,name) VALUES($1,$1::uuid::text,$2)",
                &[&id, &name],
            )
            .await?;
    }
    let set = Uuid::new_v4();
    client.execute("INSERT INTO app.card_set(id,external_set_ref,name) VALUES($1,$1::uuid::text,'Face set')", &[&set]).await?;
    // Published without an image, as the catalogue publisher does.
    let scryfall = Uuid::new_v4();
    client.execute("INSERT INTO app.printing(definition_id,set_id,external_printing_ref,rarity,finish,image_ref,language) VALUES($1,$2,$3,'rare','nonfoil',NULL,'en')", &[&second,&set,&scryfall.to_string()]).await?;
    client.execute("INSERT INTO app.printing(definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES($1,$2,$3,'common','nonfoil','private:catalogue-art')", &[&first,&set,&Uuid::new_v4().to_string()]).await?;
    let art: String = client
        .query_one(
            "SELECT image_ref FROM app.printing WHERE external_printing_ref=$1",
            &[&scryfall.to_string()],
        )
        .await?
        .get(0);
    let id = scryfall.to_string();
    assert_eq!(
        art,
        format!(
            "https://cards.scryfall.io/art_crop/front/{}/{}/{id}.jpg",
            &id[..1],
            &id[1..2]
        )
    );

    client.batch_execute("SET ROLE service_role").await?;
    // The first card of the first save becomes the face card.
    let saved: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$3,$4,NULL,$5)",
            &[
                &subject,
                &session,
                &Uuid::new_v4(),
                &deck,
                &json!([entry(second, "1"), entry(first, "4")]),
            ],
        )
        .await?
        .get(0);
    assert_eq!(saved["status"], "success");
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(face(&client, deck).await?, Some(second));

    let baseline = settings(&client, deck).await?;
    let mut chosen = baseline.clone();
    chosen["faceCardId"] = json!(first);
    let mut outside_change = baseline.clone();
    outside_change["faceCardId"] = json!(outside);
    let mut faced_baseline = baseline.clone();
    faced_baseline["faceCardId"] = json!(first);
    client.batch_execute("SET ROLE service_role").await?;
    let change = |expected: Value, next: Value| {
        let client = &client;
        async move {
            client
                .query_one(
                    "SELECT app.change_deck_settings_command($1,$2,$3,$4,$5,$6)",
                    &[&subject, &session, &Uuid::new_v4(), &deck, &expected, &next],
                )
                .await
                .map(|row| row.get::<_, Value>(0))
        }
    };
    assert_eq!(
        change(baseline.clone(), outside_change).await?,
        json!({"status":"fail","data":{"faceCard":"card not in deck"}})
    );
    assert_eq!(
        change(faced_baseline, baseline.clone()).await?,
        json!({"status":"fail","data":{"request":"invalid"}})
    );
    assert_eq!(change(baseline.clone(), chosen).await?["status"], "success");
    // A change without a face card keeps the chosen one.
    let mut renamed = baseline.clone();
    renamed["name"] = json!("Renamed");
    assert_eq!(change(baseline, renamed).await?["status"], "success");
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(face(&client, deck).await?, Some(first));

    // Removing the face card hands it to the first card of that save.
    let current = revision(&client, deck).await?;
    client.batch_execute("SET ROLE service_role").await?;
    let saved: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$3,$4,$5,$6)",
            &[
                &subject,
                &session,
                &Uuid::new_v4(),
                &deck,
                &current,
                &json!([entry(second, "2")]),
            ],
        )
        .await?
        .get(0);
    assert_eq!(saved["status"], "success");
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(face(&client, deck).await?, Some(second));

    // The owner's reads carry the face card and its official art.
    super::set_auth_claims(&client, subject).await?;
    let detail: Value = client
        .query_one("SELECT app.read_my_deck($1)", &[&deck])
        .await?
        .get(0);
    assert_eq!(
        detail["data"]["deck"]["faceCard"],
        json!({"definitionId":second,"artUrl":art})
    );
    let library: Value = client
        .query_one("SELECT app.read_my_deck_library(NULL,100)", &[])
        .await?
        .get(0);
    let listed = library["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == json!(deck))
        .unwrap();
    assert_eq!(
        listed["faceCard"],
        json!({"definitionId":second,"artUrl":art})
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn face_card_art_never_reveals_private_references_or_other_decks() -> TestResult {
    let client = connection().await?;
    let (actor, subject, deck, definition, _) = library(&client).await?;
    let set = Uuid::new_v4();
    client.execute("INSERT INTO app.card_set(id,external_set_ref,name) VALUES($1,$1::uuid::text,'Private set')", &[&set]).await?;
    client.execute("INSERT INTO app.printing(definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES($1,$2,$3,'common','nonfoil','private:catalogue-art')", &[&definition,&set,&Uuid::new_v4().to_string()]).await?;
    let saved: Value = client
        .query_one(
            "SELECT app.save_deck_contents($1,$2,NULL,$3,$4)",
            &[
                &actor,
                &deck,
                &json!([entry(definition, "1")]),
                &Uuid::new_v4(),
            ],
        )
        .await?
        .get(0);
    assert_eq!(saved["status"], "success");
    let current = revision(&client, deck).await?;
    let private: Option<String> = client
        .query_one("SELECT app.card_art_url($1,NULL)", &[&definition])
        .await?
        .get(0);
    assert_eq!(private, None);
    // The owner sees the card without art; another account sees nothing.
    let (_, stranger) = super::account(&client).await?;
    super::set_auth_claims(&client, subject).await?;
    let own: Value = client
        .query_one("SELECT app.deck_face_card_json($1,NULL)", &[&current])
        .await?
        .get(0);
    assert_eq!(own, json!({"definitionId":definition,"artUrl":null}));
    super::set_auth_claims(&client, stranger).await?;
    let other: Option<Value> = client
        .query_one("SELECT app.deck_face_card_json($1,NULL)", &[&current])
        .await?
        .get(0);
    assert_eq!(other, None);
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}
