//! Synthetic source terms exercise actual SQL publication, not upstream provenance.
use super::{TestResult, account, connection, set_auth_claims};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;
use uuid::Uuid;

pub(super) fn metadata(oracle: Uuid, set: Uuid, printing: Uuid, name: &str) -> Value {
    json!({"oracleRef":oracle,"externalPrintingRef":printing,"name":name,
        "set":{"externalRef":set,"name":"Source set","code":format!("q{}",&set.simple().to_string()[..10])},
        "collectorNumber":"123a★/b","language":"en","rarity":"rare","finishes":["foil","nonfoil"],"layout":"normal","faceNames":[name]})
}
fn terms(records: i64, batches: i64) -> Value {
    let hash = Uuid::new_v4().simple().to_string().repeat(2);
    json!({"projection":"scryfall-catalogue-metadata-v1","artifactHash":format!("sha256:{hash}"),
        "metadataHash":format!("sha256:{}","1".repeat(64)),"stageHash":format!("sha256:{}","2".repeat(64)),
        "artifactRef":format!("prices/scryfall/{hash}.jsonl.gz"),"metadataRef":format!("prices/scryfall/metadata/{}.json","1".repeat(64)),
        "stageRef":format!("catalogue/scryfall/{}.jsonl","2".repeat(64)),"sourceUpdatedAt":"2026-11-01T10:00:00Z","acquiredAt":"2026-11-01T12:00:00Z",
        "compressedBytes":100,"decompressedBytes":2000,"recordCount":records,"stageBytes":2000,"supportedRecords":records,
        "unresolvedRecords":0,"printingCount":records*2,"definitionCount":1,"setCount":1,"batchCount":batches,"unresolvedReasons":{}})
}
pub(super) fn source_terms(batches: &[Value]) -> Value {
    let mut result = terms(
        batches
            .iter()
            .map(|batch| batch.as_array().unwrap().len() as i64)
            .sum(),
        batches.len() as i64,
    );
    result["batchHashes"] = json!(
        batches
            .iter()
            .map(|batch| format!(
                "sha256:{}",
                Sha256::digest(batch.to_string().as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ))
            .collect::<Vec<_>>()
    );
    result
}
pub(super) async fn begin(client: &Client, terms: &Value) -> Result<Uuid, tokio_postgres::Error> {
    Ok(client
        .query_one("SELECT app.begin_catalogue_publication($1)", &[terms])
        .await?
        .get(0))
}
pub(super) async fn append(
    client: &Client,
    publication: Uuid,
    sequence: i32,
    records: &Value,
) -> Result<Uuid, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.append_catalogue_batch($1,$2,$3)",
            &[&publication, &sequence, &records.to_string()],
        )
        .await?
        .get(0))
}
pub(super) async fn finish(
    client: &Client,
    publication: Uuid,
) -> Result<Uuid, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.finish_catalogue_publication($1)",
            &[&publication],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn catalogue_batches_remain_hidden_until_atomic_release_and_exact_receipts_replay()
-> TestResult {
    let client = connection().await?;
    let (_, subject, deck, _, _) = super::decks::library(&client).await?;
    let session: Uuid = client
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    let oracle = Uuid::new_v4();
    let set = Uuid::new_v4();
    let name = format!("Published {}", oracle.simple());
    let first = json!([metadata(oracle, set, Uuid::new_v4(), &name)]);
    let mut second_record = metadata(oracle, set, Uuid::new_v4(), &name);
    second_record["language"] = json!("ja");
    let second = json!([second_record]);
    let source = source_terms(&[first.clone(), second.clone()]);
    client.batch_execute("SET ROLE service_role").await?;
    let publication = begin(&client, &source).await?;
    let batch = append(&client, publication, 1, &first).await?;
    assert_eq!(append(&client, publication, 1, &first).await?, batch);
    assert!(finish(&client, publication).await.is_err());
    let definition: Uuid = client
        .query_one(
            "SELECT id FROM app.card_definition WHERE oracle_ref=$1",
            &[&oracle.to_string()],
        )
        .await?
        .get(0);
    let entries =
        json!([{"definitionId":definition,"printingId":null,"quantity":"1","section":"main"}]);
    let rejected_request = Uuid::new_v4();
    let rejected: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$5,$3,NULL,$4)",
            &[&subject, &session, &deck, &entries, &rejected_request],
        )
        .await?
        .get(0);
    assert_eq!(rejected["status"], "fail");
    assert_eq!(rejected["data"]["entries"], "invalid deck entries");
    client.batch_execute("RESET ROLE").await?;
    set_auth_claims(&client, subject).await?;
    let set_code = format!("q{}", &set.simple().to_string()[..10]);
    let hidden_sets: Value = client
        .query_one("SELECT app.read_my_sets($1,NULL,100)", &[&set_code])
        .await?
        .get(0);
    assert_eq!(hidden_sets["data"]["items"], json!([]));
    let hidden: Value = client
        .query_one("SELECT app.read_my_cards($1,true,NULL,100)", &[&name])
        .await?
        .get(0);
    assert_eq!(hidden["data"]["items"], json!([]));
    let labels: Value = client
        .query_one(
            "SELECT app.read_my_catalogue($1,'{}'::uuid[])",
            &[&vec![definition]],
        )
        .await?
        .get(0);
    assert_eq!(labels["data"]["cards"], json!([]));
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    append(&client, publication, 2, &second).await?;
    let observer = connection().await?;
    set_auth_claims(&observer, subject).await?;
    client.batch_execute("BEGIN").await?;
    assert_eq!(finish(&client, publication).await?, publication);
    let uncommitted: Value = observer
        .query_one(
            "SELECT app.read_my_printings($1,NULL,NULL,NULL,NULL,100)",
            &[&definition],
        )
        .await?
        .get(0);
    assert_eq!(uncommitted["data"]["items"], json!([]));
    client.batch_execute("COMMIT").await?;
    let committed: Value = observer
        .query_one(
            "SELECT app.read_my_printings($1,NULL,NULL,NULL,NULL,100)",
            &[&definition],
        )
        .await?
        .get(0);
    assert_eq!(committed["data"]["items"].as_array().unwrap().len(), 4);
    assert_eq!(finish(&client, publication).await?, publication);
    assert_eq!(begin(&client, &source).await?, publication);
    assert_eq!(append(&client, publication, 1, &first).await?, batch);
    let replay: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$5,$3,NULL,$4)",
            &[&subject, &session, &deck, &entries, &rejected_request],
        )
        .await?
        .get(0);
    assert_eq!(replay, rejected);
    let saved: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$5,$3,NULL,$4)",
            &[&subject, &session, &deck, &entries, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    assert_eq!(saved["status"], "success");
    let mut changed = first.clone();
    changed[0]["collectorNumber"] = json!("other");
    assert!(append(&client, publication, 1, &changed).await.is_err());
    let mut changed_terms = source.clone();
    changed_terms["stageBytes"] = json!(2001);
    assert!(begin(&client, &changed_terms).await.is_err());
    client.batch_execute("RESET ROLE").await?;
    set_auth_claims(&client, subject).await?;
    let visible: Value = client
        .query_one(
            "SELECT app.read_my_printings($1,NULL,NULL,NULL,NULL,100)",
            &[&definition],
        )
        .await?
        .get(0);
    let rows = visible["data"]["items"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| row["collectorNumber"] == "123a★/b"));
    assert!(rows.iter().any(|row| row["language"] == "ja"));
    assert!(!visible.to_string().contains("imageRef"));
    client.batch_execute("RESET ROLE").await?;
    set_auth_claims(&client, subject).await?;
    let visible_sets: Value = client
        .query_one("SELECT app.read_my_sets($1,NULL,100)", &[&set_code])
        .await?
        .get(0);
    assert_eq!(visible_sets["data"]["items"].as_array().unwrap().len(), 1);
    client.batch_execute("RESET ROLE").await?;
    let (faces, images, programs): (i64, i64, i64) = {
        let row=client.query_one("SELECT (SELECT count(*) FROM app.card_face f JOIN app.card_definition_revision r ON r.id=f.definition_revision_id WHERE r.catalogue_receipt_id=$1), (SELECT count(*) FROM app.printing WHERE catalogue_receipt_id=$1 AND image_ref IS NOT NULL), (SELECT count(*) FROM app.release_card_program p JOIN app.card_definition_revision r ON r.id=p.definition_revision_id WHERE r.catalogue_receipt_id=$1)",&[&publication]).await?;
        (row.get(0), row.get(1), row.get(2))
    };
    assert_eq!((faces, images, programs), (1, 0, 0));
    assert!(client.execute("UPDATE app.card_definition_revision SET source_metadata='{}' WHERE catalogue_receipt_id=$1",&[&publication]).await.is_err());
    assert!(client.execute("UPDATE app.card_face SET name='Changed' WHERE definition_revision_id IN (SELECT id FROM app.card_definition_revision WHERE catalogue_receipt_id=$1)",&[&publication]).await.is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn catalogue_publication_rejects_invalid_shapes_duplicate_batches_and_client_or_raw_service_writes()
-> TestResult {
    let client = connection().await?;
    let (_, subject) = account(&client).await?;
    let oracle = Uuid::new_v4();
    let set = Uuid::new_v4();
    let record = metadata(oracle, set, Uuid::new_v4(), "Canonical source");
    let source = source_terms(&[json!([record]), json!([record])]);
    client.batch_execute("SET ROLE service_role").await?;
    let publication = begin(&client, &source).await?;
    let mut invalid = record.clone();
    invalid["finishes"] = json!(["foil", "foil"]);
    let invalid_publication = begin(&client, &source_terms(&[json!([invalid])])).await?;
    assert!(
        append(&client, invalid_publication, 1, &json!([invalid]))
            .await
            .is_err()
    );
    let mut invalid = record.clone();
    invalid["actorId"] = json!(Uuid::new_v4());
    let invalid_publication = begin(&client, &source_terms(&[json!([invalid])])).await?;
    assert!(
        append(&client, invalid_publication, 1, &json!([invalid]))
            .await
            .is_err()
    );
    assert!(
        append(&client, publication, 0, &json!([record]))
            .await
            .is_err()
    );
    append(&client, publication, 1, &json!([record])).await?;
    assert!(
        append(&client, publication, 2, &json!([record]))
            .await
            .is_err()
    );
    assert!(finish(&client, publication).await.is_err());
    assert!(client.execute("INSERT INTO app.card_definition(oracle_ref,name) VALUES('raw-service-denied','Denied')",&[]).await.is_err());
    client.batch_execute("RESET ROLE").await?;
    set_auth_claims(&client, subject).await?;
    assert!(begin(&client, &source).await.is_err());
    assert!(
        append(&client, publication, 1, &json!([record]))
            .await
            .is_err()
    );
    assert!(finish(&client, publication).await.is_err());
    client.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert!(begin(&client, &source).await.is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn catalogue_new_sources_preserve_historical_revisions_and_cannot_reassign_a_saved_printing()
-> TestResult {
    let client = connection().await?;
    let oracle = Uuid::new_v4();
    let set = Uuid::new_v4();
    let printing = Uuid::new_v4();
    let original = metadata(oracle, set, printing, "Original name");
    client.batch_execute("SET ROLE service_role").await?;
    let first = begin(&client, &source_terms(&[json!([original])])).await?;
    append(&client, first, 1, &json!([original])).await?;
    finish(&client, first).await?;
    let mut updated = original.clone();
    updated["name"] = json!("Updated source name");
    updated["faceNames"] = json!(["Updated source name"]);
    let mut next_terms = source_terms(&[json!([updated])]);
    next_terms["sourceUpdatedAt"] = json!("2026-11-01T11:00:00Z");
    let next = begin(&client, &next_terms).await?;
    append(&client, next, 1, &json!([updated])).await?;
    let before: String = client
        .query_one(
            "SELECT name FROM app.card_definition WHERE oracle_ref=$1",
            &[&oracle.to_string()],
        )
        .await?
        .get(0);
    assert_eq!(before, "Original name");
    finish(&client, next).await?;
    let historical:Value=client.query_one("SELECT source_metadata->'common'->'name' FROM app.card_definition_revision WHERE catalogue_receipt_id=$1",&[&first]).await?.get(0);
    assert_eq!(historical, "Original name");
    let stale = begin(&client, &source_terms(&[json!([original])])).await?;
    append(&client, stale, 1, &json!([original])).await?;
    assert!(finish(&client, stale).await.is_err());
    let current_name: String = client
        .query_one(
            "SELECT name FROM app.card_definition WHERE oracle_ref=$1",
            &[&oracle.to_string()],
        )
        .await?
        .get(0);
    assert_eq!(current_name, "Updated source name");
    let mut reassigned = updated.clone();
    let replacement = Uuid::new_v4();
    reassigned["set"] = metadata(oracle, replacement, printing, "Ignored")["set"].clone();
    reassigned["finishes"] = json!(["etched"]);
    let mut bad_terms = source_terms(&[json!([reassigned])]);
    bad_terms["sourceUpdatedAt"] = json!("2026-11-01T11:30:00Z");
    let bad = begin(&client, &bad_terms).await?;
    assert!(append(&client, bad, 1, &json!([reassigned])).await.is_err());
    assert!(finish(&client, bad).await.is_err());
    let current: Uuid = client
        .query_one(
            "SELECT catalogue_receipt_id FROM app.printing WHERE external_printing_ref=$1 LIMIT 1",
            &[&printing.to_string()],
        )
        .await?
        .get(0);
    // Unchanged printing fields retain their original source; the definition's
    // changed name points to the newer immutable revision instead.
    assert_eq!(current, first);
    let definition_source: Uuid = client
        .query_one(
            "SELECT catalogue_receipt_id FROM app.card_definition WHERE oracle_ref=$1",
            &[&oracle.to_string()],
        )
        .await?
        .get(0);
    assert_eq!(definition_source, next);
    let state: String = client
        .query_one("SELECT state FROM app.command_receipt WHERE id=$1", &[&bad])
        .await?
        .get(0);
    assert_eq!(state, "pending");
    let leaked: i64 = client
        .query_one(
            "SELECT count(*) FROM app.card_set WHERE external_set_ref=$1",
            &[&replacement.to_string()],
        )
        .await?
        .get(0);
    assert_eq!(leaked, 0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres"]
async fn pending_catalogue_cannot_feed_assets_avatars_programs_or_qualified_pack_candidates()
-> TestResult {
    let client = connection().await?;
    let (actor, _, _, _, _) = super::decks::library(&client).await?;
    let record = metadata(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Pending consumer source",
    );
    let publication = begin(&client, &source_terms(&[json!([record])])).await?;
    append(&client, publication, 1, &json!([record])).await?;
    let printing: Uuid = client
        .query_one(
            "SELECT id FROM app.printing WHERE catalogue_receipt_id=$1 LIMIT 1",
            &[&publication],
        )
        .await?
        .get(0);
    assert!(
        client
            .execute(
                "INSERT INTO app.asset_definition(kind,printing_id) VALUES('card',$1)",
                &[&printing]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.profile SET printing_id=$1,crop_x=0.5,crop_y=0.5,crop_scale=1 WHERE account_id=$2",
                &[&printing, &actor]
            )
            .await
            .is_err()
    );
    let release = Uuid::new_v4();
    client.execute("INSERT INTO app.qualified_release(id,release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref) VALUES($1,$1::uuid::text,'test-only:kernel','test-only:rules','test-only:programs','test-only:source-fence')",&[&release]).await?;
    assert!(client.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref) SELECT $1,id,'test-only:program' FROM app.card_definition_revision WHERE catalogue_receipt_id=$2",&[&release,&publication]).await.is_err());
    let product = super::pricing::product(&client, false).await?;
    client
        .execute(
            "DELETE FROM app.booster_slot_card WHERE slot_id=$1 AND weight=3",
            &[&product.slot],
        )
        .await?;
    client
        .execute(
            "INSERT INTO app.booster_slot_card(slot_id,printing_id,weight) VALUES($1,$2,3)",
            &[&product.slot, &printing],
        )
        .await?;
    let evidence = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let error = client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&product.recipe, &evidence],
        )
        .await
        .unwrap_err();
    assert!(
        error
            .as_db_error()
            .unwrap()
            .message()
            .contains("published catalogue")
    );
    finish(&client, publication).await?;
    client
        .execute(
            "INSERT INTO app.asset_definition(kind,printing_id) VALUES('card',$1)",
            &[&printing],
        )
        .await?;
    client
        .execute(
            "UPDATE app.profile SET printing_id=$1,crop_x=0.5,crop_y=0.5,crop_scale=1 WHERE account_id=$2",
            &[&printing, &actor],
        )
        .await?;
    client
        .query_one(
            "SELECT app.qualify_booster_recipe($1,$2)",
            &[&product.recipe, &evidence],
        )
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn native_deck_publication_rejects_an_unlinked_sideboard_in_either_selection() -> TestResult {
    let c = connection().await?;
    let (actor, _, deck, main, _) = super::decks::library(&c).await?;
    let side = Uuid::new_v4();
    c.execute(
        "INSERT INTO app.card_definition(id,oracle_ref,name) VALUES($1,$1::uuid::text,'Sideboard')",
        &[&side],
    )
    .await?;
    let release = Uuid::new_v4();
    c.execute("INSERT INTO app.qualified_release(id,release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref) VALUES($1,$1::uuid::text,'fixture','fixture','fixture','fixture')",&[&release]).await?;
    for (definition, reference) in [
        (main, "fixture:main".to_owned()),
        (side, format!("catalogue:{}", Uuid::new_v4())),
    ] {
        let revision:Uuid=c.query_one("INSERT INTO app.card_definition_revision(definition_id,revision_ref,rules_text_ref) VALUES($1,$2,'fixture:rules') RETURNING id",&[&definition,&reference]).await?.get(0);
        c.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref,unlimited_basic_land) VALUES($1,$2,'fixture:program',true)",&[&release,&revision]).await?;
    }
    let mut prior: Option<Uuid> = None;
    for (section, valid) in [("main", true), ("sideboard", false), ("main", false)] {
        let mut entries =
            vec![json!({"definitionId":main,"printingId":null,"section":"main","quantity":"60"})];
        if !valid {
            entries.push(
                json!({"definitionId":side,"printingId":null,"section":section,"quantity":"1"}),
            );
        }
        let saved: Value = c
            .query_one(
                "SELECT app.save_deck_contents($1,$2,$3,$4,$5)",
                &[&actor, &deck, &prior, &json!(entries), &Uuid::new_v4()],
            )
            .await?
            .get(0);
        assert_eq!(saved["status"], "success");
        let selected: Uuid = c
            .query_one(
                "SELECT current_revision_id FROM app.deck WHERE id=$1",
                &[&deck],
            )
            .await?
            .get(0);
        let projected = c
            .query_one(
                "SELECT app.native_deck_source($1,$2)",
                &[&release, &selected],
            )
            .await;
        if valid {
            let rows: Value = projected?.get(0);
            assert_eq!(rows.as_array().unwrap().len(), 1);
            assert_eq!(rows[0]["quantity"], "60");
        } else {
            let err = projected.unwrap_err();
            assert_eq!(
                err.code(),
                Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
            );
            assert_eq!(
                err.as_db_error().unwrap().message(),
                "native deck publication source is incomplete"
            );
        }
        prior = Some(selected);
    }
    Ok(())
}
