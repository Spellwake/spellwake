//! Real metadata/RLS qualification; fixture labels do not publish a production catalogue.
use super::{TestResult, account, connection, set_auth_claims};
use serde_json::{Value, json};
use spellwake_application::{
    ApiResponse,
    catalogue::{CardPage, CatalogueLabels, CatalogueLookup, PrintingPage},
};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn catalogue_lookup_preserves_exact_printing_labels_and_privacy_without_inventory()
-> TestResult {
    let client = connection().await?;
    let (_, subject) = account(&client).await?;
    let prefix = format!("Catalogue {} %_", Uuid::new_v4().simple());
    let name = format!("{prefix}★");
    let definitions = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    for (id, label) in [
        (definitions[0], name.clone()),
        (definitions[1], name.clone()),
        (definitions[2], format!("{}★", prefix.replace("%_", "ab"))),
    ] {
        client
            .execute(
                "INSERT INTO app.card_definition(id,oracle_ref,name) VALUES($1,$1::uuid::text,$2)",
                &[&id, &label],
            )
            .await?;
    }
    let set = Uuid::new_v4();
    let unknown_set = Uuid::new_v4();
    let code = format!("q{}", &set.simple().to_string()[..10]);
    client.execute("INSERT INTO app.card_set(id,external_set_ref,name,code) VALUES($1,$1::uuid::text,'Known set',$2)", &[&set,&code]).await?;
    client.execute("INSERT INTO app.card_set(id,external_set_ref,name) VALUES($1,$1::uuid::text,'Unknown labels')", &[&unknown_set]).await?;
    let printing_ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let external = Uuid::new_v4().to_string();
    for (id, finish) in [(printing_ids[0], "nonfoil"), (printing_ids[1], "foil")] {
        client.execute("INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref,collector_number,language) VALUES($1,$2,$3,$4,'rare',$5,'private:catalogue-art','123a★','en')", &[&id,&definitions[0],&set,&external,&finish]).await?;
    }
    client.execute("INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES($1,$2,$3,$1::uuid::text,'rare','etched','private:catalogue-art')", &[&printing_ids[2],&definitions[0],&unknown_set]).await?;
    let variant_printing = Uuid::new_v4();
    client.execute("INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref,collector_number,language) VALUES($1,$2,$3,$1::uuid::text,'rare','nonfoil','private:catalogue-art','124b','en')", &[&variant_printing,&definitions[1],&set]).await?;
    set_auth_claims(&client, subject).await?;
    let page: Value = client
        .query_one("SELECT app.read_my_cards($1,false,NULL,100)", &[&prefix])
        .await?
        .get(0);
    let page: ApiResponse<CardPage> = serde_json::from_value(page)?;
    match page {
        ApiResponse::Success { data } => {
            data.validate_after(None, 100)?;
            let ids: Vec<_> = data.items.iter().map(|card| card.id).collect();
            let mut expected = definitions[..2].to_vec();
            expected.sort();
            assert_eq!(ids, expected);
        }
        _ => panic!("card lookup failed"),
    }
    let exact: Value = client
        .query_one(
            "SELECT app.read_my_cards($1,true,NULL,1)",
            &[&name.to_lowercase()],
        )
        .await?
        .get(0);
    let mut named_ids = definitions[..2].to_vec();
    named_ids.sort();
    assert_eq!(exact["data"]["items"][0]["id"], named_ids[0].to_string());
    assert_eq!(exact["data"]["nextCursor"], named_ids[0].to_string());
    let next: Value = client
        .query_one(
            "SELECT app.read_my_cards($1,true,$2,1)",
            &[&name, &named_ids[0]],
        )
        .await?
        .get(0);
    assert_eq!(next["data"]["items"][0]["id"], named_ids[1].to_string());
    assert_eq!(next["data"]["nextCursor"], named_ids[1].to_string());
    let page: Value = client
        .query_one(
            "SELECT app.read_my_printings($1,NULL,NULL,NULL,NULL,100)",
            &[&definitions[0]],
        )
        .await?
        .get(0);
    let page: ApiResponse<PrintingPage> = serde_json::from_value(page)?;
    match page {
        ApiResponse::Success { data } => {
            data.validate_after(Some(definitions[0]), None, 100)?;
            assert_eq!(data.items.len(), 3);
            let known = data
                .items
                .iter()
                .find(|printing| printing.id == printing_ids[0])
                .unwrap();
            assert_eq!(known.collector_number.as_deref(), Some("123a★"));
            assert_eq!(known.language.as_deref(), Some("en"));
            let missing = data
                .items
                .iter()
                .find(|printing| printing.id == printing_ids[2])
                .unwrap();
            assert!(
                missing.collector_number.is_none()
                    && missing.language.is_none()
                    && missing.set.code.is_none()
            );
        }
        _ => panic!("printing lookup failed"),
    }
    let named: Value = client
        .query_one(
            "SELECT app.read_my_printings(NULL,$1,NULL,NULL,NULL,100)",
            &[&name],
        )
        .await?
        .get(0);
    assert_eq!(named["data"]["items"].as_array().unwrap().len(), 4);
    let filtered: Value = client
        .query_one(
            "SELECT app.read_my_printings(NULL,$1,$2,'123a★',NULL,100)",
            &[&name, &code.to_uppercase()],
        )
        .await?
        .get(0);
    let filtered: ApiResponse<PrintingPage> = serde_json::from_value(filtered)?;
    match filtered {
        ApiResponse::Success { data } => {
            data.validate_after(None, None, 100)?;
            assert_eq!(data.items.len(), 2);
            assert!(
                data.items
                    .iter()
                    .all(|printing| printing.definition_id == definitions[0])
            );
        }
        _ => panic!("precise printing lookup failed"),
    }
    let request = CatalogueLookup {
        definition_ids: vec![definitions[0], Uuid::new_v4()],
        printing_ids: vec![printing_ids[0], printing_ids[2], Uuid::new_v4()],
    };
    let resolved: Value = client
        .query_one(
            "SELECT app.read_my_catalogue($1,$2)",
            &[&request.definition_ids, &request.printing_ids],
        )
        .await?
        .get(0);
    assert!(!resolved.to_string().contains("private:catalogue-art"));
    let resolved: ApiResponse<CatalogueLabels> = serde_json::from_value(resolved)?;
    match resolved {
        ApiResponse::Success { data } => {
            data.validate(&request)?;
            assert_eq!(data.cards.len(), 1);
            assert_eq!(data.printings.len(), 2);
        }
        _ => panic!("metadata batch failed"),
    }
    let duplicate = vec![definitions[0], definitions[0]];
    let denied: Value = client
        .query_one("SELECT app.read_my_catalogue($1,'{}')", &[&duplicate])
        .await?
        .get(0);
    assert_eq!(denied, json!({"status":"fail","data":{"query":"invalid"}}));
    assert!(
        client
            .query_one(
                "SELECT image_ref FROM app.printing WHERE id=$1",
                &[&printing_ids[0]]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.card_definition SET name='Forbidden' WHERE id=$1",
                &[&definitions[0]]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 day' WHERE id=$1",
            &[&subject],
        )
        .await?;
    client.batch_execute("SET ROLE authenticated").await?;
    let revoked: Value = client
        .query_one("SELECT app.read_my_cards($1,false,NULL,100)", &[&prefix])
        .await?
        .get(0);
    assert_eq!(
        revoked,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    let visible: i64 = client
        .query_one(
            "SELECT count(*) FROM app.card_definition WHERE id=ANY($1)",
            &[&definitions.to_vec()],
        )
        .await?
        .get(0);
    assert_eq!(visible, 0);
    client.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert!(
        client
            .query_one("SELECT app.read_my_cards('',false,NULL,100)", &[])
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn set_discovery_uses_literal_name_or_code_search_and_caller_rls() -> TestResult {
    let client = connection().await?;
    let (_, subject) = account(&client).await?;
    let prefix = format!("Set {}", Uuid::new_v4().simple());
    let mut ids = [Uuid::new_v4(), Uuid::new_v4()];
    ids.sort();
    for (id, name, code) in [
        (ids[0], format!("{prefix} %_"), Some("FiX")),
        (ids[1], format!("{prefix} plain"), None),
    ] {
        client.execute("INSERT INTO app.card_set(id,external_set_ref,name,code) VALUES($1,$1::uuid::text,$2,$3)",&[&id,&name,&code]).await?;
    }
    set_auth_claims(&client, subject).await?;
    let value: Value = client
        .query_one("SELECT app.read_my_sets($1,NULL,1)", &[&prefix])
        .await?
        .get(0);
    let page: spellwake_application::catalogue::SetPage =
        serde_json::from_value(value["data"].clone())?;
    page.validate_after(None, 1)?;
    assert_eq!(page.items[0].id, ids[0]);
    assert_eq!(page.next_cursor, Some(ids[0]));
    let value: Value = client
        .query_one("SELECT app.read_my_sets($1,$2,1)", &[&prefix, &ids[0]])
        .await?
        .get(0);
    let page: spellwake_application::catalogue::SetPage =
        serde_json::from_value(value["data"].clone())?;
    page.validate_after(Some(ids[0]), 1)?;
    assert_eq!(page.items[0].id, ids[1]);
    assert!(page.items[0].code.is_none());
    let value: Value = client
        .query_one(
            "SELECT app.read_my_sets($1,NULL,100)",
            &[&format!("{prefix} %_")],
        )
        .await?
        .get(0);
    assert_eq!(value["data"]["items"].as_array().unwrap().len(), 1);
    let value: Value = client
        .query_one("SELECT app.read_my_sets('fix',NULL,100)", &[])
        .await?
        .get(0);
    assert!(
        value["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == ids[0].to_string())
    );
    client.batch_execute("RESET ROLE").await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        let allowed: bool = client
            .query_one(
                "SELECT has_function_privilege($1,'app.read_my_sets(text,uuid,integer)','EXECUTE')",
                &[&role],
            )
            .await?
            .get(0);
        assert_eq!(allowed, role == "authenticated");
    }
    let definer:bool=client.query_one("SELECT prosecdef FROM pg_proc WHERE oid='app.read_my_sets(text,uuid,integer)'::regprocedure",&[]).await?.get(0);
    assert!(!definer);
    client
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 hour' WHERE id=$1",
            &[&subject],
        )
        .await?;
    set_auth_claims(&client, subject).await?;
    let denied: Value = client
        .query_one("SELECT app.read_my_sets('',NULL,40)", &[])
        .await?
        .get(0);
    assert_eq!(denied["status"], "fail");
    Ok(())
}
