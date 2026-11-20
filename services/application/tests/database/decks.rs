use super::{TestResult, account, connection, set_auth_claims};
use serde_json::{Value, json};
use spellwake_application::{
    ApiResponse,
    commands::{CommandAck, CommandDisposition},
};
use tokio_postgres::Client;
use uuid::Uuid;

async fn disposition(
    client: &Client,
    subject: Uuid,
    session: Uuid,
    kind: &str,
    command: &Value,
) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.read_deck_command($1,$2,$3,$4)",
            &[&subject, &session, &kind, command],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn deck_disposition_binds_owner_scope_target_and_full_terms_without_writing() -> TestResult {
    let client = connection().await?;
    let (actor, subject, deck, _, format) = library(&client).await?;
    let (_, other) = account(&client).await?;
    let claims: Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    let session: Uuid = claims["session_id"].as_str().unwrap().parse()?;
    let other_claims: Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&other])
        .await?
        .get(0);
    let other_session: Uuid = other_claims["session_id"].as_str().unwrap().parse()?;
    client.batch_execute("SET ROLE service_role").await?;
    let create_request = Uuid::new_v4();
    let command =
        json!({"requestId":create_request,"name":"Receipt terms","mode":"open","formatId":format});
    assert_eq!(
        disposition(&client, subject, session, "create", &command).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    let created: Value = client
        .query_one(
            "SELECT app.create_deck_command($1,$2,$3,'Receipt terms','open',$4)",
            &[&subject, &session, &create_request, &format],
        )
        .await?
        .get(0);
    let accepted = disposition(&client, subject, session, "create", &command).await?;
    assert_eq!(
        accepted,
        json!({"status":"success","data":{"state":"accepted","response":created}})
    );
    let typed: ApiResponse<CommandDisposition> = serde_json::from_value(accepted)?;
    assert!(matches!(
        typed,
        ApiResponse::Success {
            data: CommandDisposition::Accepted { .. }
        }
    ));
    let mut changed = command.clone();
    changed["name"] = json!("Different terms");
    assert_eq!(
        disposition(&client, subject, session, "create", &changed).await?,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    assert_eq!(
        disposition(&client, other, other_session, "create", &command).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    let save_request = Uuid::new_v4();
    let save =
        json!({"requestId":save_request,"deckId":deck,"expectedRevisionId":null,"entries":[]});
    let saved: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$3,$4,NULL,'[]')",
            &[&subject, &session, &save_request, &deck],
        )
        .await?
        .get(0);
    let first = resource(saved.clone());
    let next: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$3,$4,$5,'[]')",
            &[&subject, &session, &Uuid::new_v4(), &deck, &first],
        )
        .await?
        .get(0);
    assert_ne!(resource(next), first);
    assert_eq!(
        disposition(&client, subject, session, "save", &save).await?,
        json!({"status":"success","data":{"state":"accepted","response":saved}})
    );
    let mut wrong_target = save.clone();
    wrong_target["deckId"] = json!(resource(created));
    assert_eq!(
        disposition(&client, subject, session, "save", &wrong_target).await?,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    let rejected_request = Uuid::new_v4();
    let rejected = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$3,$4,NULL,'[]')",
            &[&subject, &session, &rejected_request, &deck],
        )
        .await?
        .get::<_, Value>(0);
    let rejected_body =
        json!({"requestId":rejected_request,"deckId":deck,"expectedRevisionId":null,"entries":[]});
    let proof = disposition(&client, subject, session, "save", &rejected_body).await?;
    assert_eq!(
        proof,
        json!({"status":"success","data":{"state":"rejected","response":rejected}})
    );
    let typed: ApiResponse<CommandDisposition> = serde_json::from_value(proof)?;
    assert!(matches!(
        typed,
        ApiResponse::Success {
            data: CommandDisposition::Rejected { .. }
        }
    ));
    assert_eq!(
        disposition(&client, subject, other_session, "save", &rejected_body).await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    let untouched = Uuid::new_v4();
    let unknown =
        json!({"requestId":untouched,"deckId":deck,"expectedRevisionId":null,"entries":[]});
    assert_eq!(
        disposition(&client, subject, session, "save", &unknown).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    client.batch_execute("RESET ROLE").await?;
    let count: i64 = client
        .query_one(
            "SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND request_ref=$2",
            &[&actor, &untouched.to_string()],
        )
        .await?
        .get(0);
    assert_eq!(count, 0);
    set_auth_claims(&client, subject).await?;
    assert!(
        disposition(&client, subject, session, "create", &command)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn authenticated_deck_commands_replay_terms_and_owner_detail_is_consistent() -> TestResult {
    let client = connection().await?;
    let (actor, subject, _, definition, format) = library(&client).await?;
    let (_, other_subject) = account(&client).await?;
    let claims: Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    let session: Uuid = claims["session_id"].as_str().unwrap().parse()?;
    let request = Uuid::new_v4();
    client.batch_execute("SET ROLE service_role").await?;
    let created: Value = client
        .query_one(
            "SELECT app.create_deck_command($1,$2,$3,'API deck','collection',$4)",
            &[&subject, &session, &request, &format],
        )
        .await?
        .get(0);
    let deck = resource(created.clone());
    assert_eq!(
        created,
        client
            .query_one(
                "SELECT app.create_deck_command($1,$2,$3,'API deck','collection',$4)",
                &[&subject, &session, &request, &format]
            )
            .await?
            .get::<_, Value>(0)
    );
    let conflict: Value = client
        .query_one(
            "SELECT app.create_deck_command($1,$2,$3,'Changed terms','collection',$4)",
            &[&subject, &session, &request, &format],
        )
        .await?
        .get(0);
    assert_eq!(
        conflict,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    let save_request = Uuid::new_v4();
    let entries = json!([{"definitionId":definition,"printingId":null,"section":"main","quantity":"9007199254740993"}]);
    let saved: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$3,$4,NULL,$5)",
            &[&subject, &session, &save_request, &deck, &entries],
        )
        .await?
        .get(0);
    let revision = resource(saved.clone());
    assert_eq!(
        saved,
        client
            .query_one(
                "SELECT app.save_deck_command($1,$2,$3,$4,NULL,$5)",
                &[&subject, &session, &save_request, &deck, &entries]
            )
            .await?
            .get::<_, Value>(0)
    );
    let stale: Value = client
        .query_one(
            "SELECT app.save_deck_command($1,$2,$3,$4,NULL,$5)",
            &[&subject, &session, &Uuid::new_v4(), &deck, &entries],
        )
        .await?
        .get(0);
    assert_eq!(
        stale,
        json!({"status":"fail","data":{"revision":"deck changed; reload before saving"}})
    );
    set_auth_claims(&client, subject).await?;
    let detail: Value = client
        .query_one("SELECT app.read_my_deck($1)", &[&deck])
        .await?
        .get(0);
    let detail: ApiResponse<spellwake_application::decks::reads::DeckDetail> =
        serde_json::from_value(detail)?;
    match detail {
        ApiResponse::Success { data } => {
            data.validate(deck)?;
            assert_eq!(data.revision.as_ref().map(|r| r.id), Some(revision));
            assert_eq!(
                data.revision.unwrap().entries[0].quantity.to_string(),
                "9007199254740993"
            );
        }
        _ => panic!("owner deck detail unavailable"),
    }
    let formats: Value = client
        .query_one("SELECT app.read_my_formats(NULL,100)", &[])
        .await?
        .get(0);
    let formats: ApiResponse<spellwake_application::decks::reads::FormatPage> =
        serde_json::from_value(formats)?;
    match formats {
        ApiResponse::Success { data } => data.validate(None, 100)?,
        _ => panic!("formats unavailable"),
    }
    assert!(
        client
            .query_one(
                "SELECT app.create_deck_command($1,$2,$3,'Denied','open',$4)",
                &[&subject, &session, &Uuid::new_v4(), &format]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .query_one(
                "SELECT app.lock_session_account($1,$2)",
                &[&subject, &session]
            )
            .await
            .is_err()
    );
    set_auth_claims(&client, other_subject).await?;
    let hidden: Value = client
        .query_one("SELECT app.read_my_deck($1)", &[&deck])
        .await?
        .get(0);
    assert_eq!(
        hidden,
        json!({"status":"fail","data":{"deck":"unavailable"}})
    );
    client.batch_execute("RESET ROLE").await?;
    super::deletion::request(&client, actor, Uuid::new_v4()).await?;
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        created,
        client
            .query_one(
                "SELECT app.create_deck_command($1,$2,$3,'API deck','collection',$4)",
                &[&subject, &session, &request, &format]
            )
            .await?
            .get::<_, Value>(0)
    );
    let pending: Value = client
        .query_one(
            "SELECT app.create_deck_command($1,$2,$3,'New commitment','collection',$4)",
            &[&subject, &session, &Uuid::new_v4(), &format],
        )
        .await?
        .get(0);
    assert_eq!(
        pending,
        json!({"status":"fail","data":{"account":"account unavailable"}})
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn deck_writer_stale_repeatable_read_cannot_commit_a_new_command() -> TestResult {
    let client = connection().await?;
    let (actor, subject, _, _, format) = library(&client).await?;
    let claims: Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    let session: Uuid = claims["session_id"].as_str().unwrap().parse()?;
    let writer = connection().await?;
    writer
        .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    writer
        .query_one("SELECT id FROM app.account WHERE id=$1", &[&actor])
        .await?;
    client
        .execute(
            "UPDATE app.account SET username=username WHERE id=$1",
            &[&actor],
        )
        .await?;
    writer.batch_execute("SET LOCAL ROLE service_role").await?;
    let request = Uuid::new_v4();
    let error = writer
        .query_one(
            "SELECT app.create_deck_command($1,$2,$3,'Stale snapshot','open',$4)",
            &[&subject, &session, &request, &format],
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    writer.batch_execute("ROLLBACK").await?;
    let count:i64=client.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='deck_create' AND request_ref=$2", &[&actor,&request.to_string()]).await?.get(0);
    assert_eq!(count, 0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn deck_writer_rechecks_live_authority_after_account_lock_wait() -> TestResult {
    let locker = connection().await?;
    let (actor, subject, _, _, format) = library(&locker).await?;
    let claims: Value = locker
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    let session: Uuid = claims["session_id"].as_str().unwrap().parse()?;
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
            &[&actor],
        )
        .await?;
    let request = Uuid::new_v4();
    let task = tokio::spawn(async move {
        waiter
            .query_one(
                "SELECT app.create_deck_command($1,$2,$3,'Must not create','open',$4)",
                &[&subject, &session, &request, &format],
            )
            .await
            .map(|row| row.get::<_, Value>(0))
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let blocked: bool = locker
                .query_one("SELECT cardinality(pg_blocking_pids($1))>0", &[&pid])
                .await
                .unwrap()
                .get(0);
            if blocked {
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
    let rejected = tokio::time::timeout(std::time::Duration::from_secs(3), task).await???;
    assert_eq!(
        rejected,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    let count:i64=locker.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='deck_create' AND request_ref=$2", &[&actor,&request.to_string()]).await?.get(0);
    assert_eq!(count, 0);
    Ok(())
}

fn resource(value: Value) -> Uuid {
    match serde_json::from_value::<ApiResponse<CommandAck>>(value).unwrap() {
        ApiResponse::Success { data } => data.resource_id,
        other => panic!("expected saved resource: {other:?}"),
    }
}

pub(super) async fn library(
    client: &Client,
) -> Result<(Uuid, Uuid, Uuid, Uuid, Uuid), Box<dyn std::error::Error + Send + Sync>> {
    let (actor, subject) = account(client).await?;
    let mut cosmetics = Vec::new();
    for kind in ["back", "sleeve", "dice"] {
        let id = Uuid::new_v4();
        client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,$2,'test-only:library','active',true)", &[&id,&kind]).await?;
        client.execute("INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,state) VALUES($1,$2,'free','active')", &[&actor,&id]).await?;
        cosmetics.push(id);
    }
    client.execute("INSERT INTO app.profile(account_id,default_back_id,default_sleeve_id,default_dice_id) VALUES($1,$2,$3,$4)", &[&actor,&cosmetics[0],&cosmetics[1],&cosmetics[2]]).await?;
    let format = Uuid::new_v4();
    client
        .execute(
            "INSERT INTO app.format(id,key,name) VALUES($1,$1::uuid::text,'Test format')",
            &[&format],
        )
        .await?;
    let definition = Uuid::new_v4();
    client.execute("INSERT INTO app.card_definition(id,oracle_ref,name) VALUES($1,$1::uuid::text,'Test definition')", &[&definition]).await?;
    let request = Uuid::new_v4();
    let created: Value = client
        .query_one(
            "SELECT app.create_deck($1,'Saved deck','collection',$2,$3)",
            &[&actor, &format, &request],
        )
        .await?
        .get(0);
    let deck = resource(created.clone());
    assert_eq!(
        created,
        client
            .query_one(
                "SELECT app.create_deck($1,'Saved deck','collection',$2,$3)",
                &[&actor, &format, &request]
            )
            .await?
            .get::<_, Value>(0)
    );
    Ok((actor, subject, deck, definition, format))
}

async fn save(
    client: &Client,
    actor: Uuid,
    deck: Uuid,
    expected: Option<Uuid>,
    entries: Value,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.save_deck_contents($1,$2,$3,$4,$5)",
            &[&actor, &deck, &expected, &entries, &request],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn deck_defaults_are_copied_and_history_is_private_and_immutable() -> TestResult {
    let client = connection().await?;
    let (actor, subject, deck, definition, format) = library(&client).await?;
    let (other, other_subject) = account(&client).await?;
    let entries =
        json!([{"definitionId":definition,"printingId":null,"section":"main","quantity":"4"}]);
    let request = Uuid::new_v4();
    let saved = save(&client, actor, deck, None, entries.clone(), request).await?;
    let revision = resource(saved.clone());
    assert_eq!(
        saved,
        save(&client, actor, deck, None, entries.clone(), request).await?
    );
    assert_eq!(
        save(
            &client,
            other,
            deck,
            Some(revision),
            entries.clone(),
            Uuid::new_v4()
        )
        .await?["status"],
        "fail"
    );
    let original: Uuid = client
        .query_one("SELECT back_id FROM app.deck WHERE id=$1", &[&deck])
        .await?
        .get(0);
    let replacement = Uuid::new_v4();
    client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,'back','test-only:new-default','active',true)", &[&replacement]).await?;
    // A catalogue item without a grant is not selectable.
    let defaults_sql = "SELECT app.change_profile_defaults_command(a.auth_subject,(test_fixture.claims_for_subject(a.auth_subject)->>'session_id')::uuid,$3,jsonb_build_object('backId',p.default_back_id,'sleeveId',p.default_sleeve_id,'diceId',p.default_dice_id,'playmatId',p.default_playmat_id),jsonb_build_object('backId',$2::uuid,'sleeveId',p.default_sleeve_id,'diceId',p.default_dice_id,'playmatId',NULL)) FROM app.profile p JOIN app.account a ON a.id=p.account_id WHERE p.account_id=$1";
    assert_eq!(
        client
            .query_one(defaults_sql, &[&actor, &replacement, &Uuid::new_v4()])
            .await?
            .get::<_, Value>(0)["status"],
        "fail"
    );
    client.execute("INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,state) VALUES($1,$2,'free','active')", &[&actor,&replacement]).await?;
    resource(
        client
            .query_one(defaults_sql, &[&actor, &replacement, &Uuid::new_v4()])
            .await?
            .get(0),
    );
    let next = resource(
        client
            .query_one(
                "SELECT app.create_deck($1,'New deck','open',$2,$3)",
                &[&actor, &format, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    assert_eq!(
        client
            .query_one("SELECT back_id FROM app.deck WHERE id=$1", &[&next])
            .await?
            .get::<_, Uuid>(0),
        replacement
    );
    assert_eq!(
        client
            .query_one("SELECT back_id FROM app.deck WHERE id=$1", &[&deck])
            .await?
            .get::<_, Uuid>(0),
        original
    );
    for statement in [
        "UPDATE app.deck_entry SET quantity=5 WHERE revision_id=$1",
        "DELETE FROM app.deck_entry WHERE revision_id=$1",
        "UPDATE app.deck_revision SET created_at=clock_timestamp() WHERE id=$1",
        "DELETE FROM app.deck_revision WHERE id=$1",
    ] {
        assert!(client.execute(statement, &[&revision]).await.is_err());
    }
    assert!(client.execute("INSERT INTO app.deck_entry(revision_id,definition_id,section,quantity) VALUES($1,$2,'sideboard',1)", &[&revision,&definition]).await.is_err());
    assert!(
        client
            .execute(
                "UPDATE app.deck SET current_revision_id=$1 WHERE id=$2",
                &[&revision, &next]
            )
            .await
            .is_err()
    );
    client.batch_execute("SET ROLE authenticated").await?;
    super::set_auth_claims(&client, subject).await?;
    assert_eq!(
        client
            .query(
                "SELECT id FROM app.deck_entry WHERE revision_id=$1",
                &[&revision]
            )
            .await?
            .len(),
        1
    );
    assert!(
        client
            .query_one(
                "SELECT app.create_deck($1,'Bypass','open',$2,$3)",
                &[&actor, &format, &Uuid::new_v4()]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.profile SET default_back_id=$1 WHERE account_id=$2",
                &[&replacement, &actor]
            )
            .await
            .is_err()
    );
    super::set_auth_claims(&client, other_subject).await?;
    assert!(
        client
            .query("SELECT id FROM app.deck WHERE id=$1", &[&deck])
            .await?
            .is_empty()
    );
    assert!(
        client
            .query("SELECT id FROM app.deck_revision WHERE id=$1", &[&revision])
            .await?
            .is_empty()
    );
    assert!(
        client
            .query(
                "SELECT id FROM app.deck_entry WHERE revision_id=$1",
                &[&revision]
            )
            .await?
            .is_empty()
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn invalid_deck_entries_do_not_publish_partial_revisions() -> TestResult {
    let mut client = connection().await?;
    let (actor, _, deck, definition, _) = library(&client).await?;
    let set = Uuid::new_v4();
    let other_definition = Uuid::new_v4();
    let printing = Uuid::new_v4();
    client.execute("INSERT INTO app.card_set(id,external_set_ref,name) VALUES($1,$1::uuid::text,'Test set')", &[&set]).await?;
    client.execute("INSERT INTO app.card_definition(id,oracle_ref,name) VALUES($1,$1::uuid::text,'Other definition')", &[&other_definition]).await?;
    client.execute("INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES($1,$2,$3,$1::uuid::text,'rare','nonfoil','test-only:image')", &[&printing,&other_definition,&set]).await?;
    let valid = json!({"definitionId":definition,"section":"main","quantity":"1"});
    let mut invalid = vec![
        json!([valid.clone(), valid.clone()]),
        json!({}),
        json!([{"definitionId":definition,"section":"main","quantity":1}]),
    ];
    invalid.push(
        json!([{"definitionId":definition,"printingId":printing,"section":"main","quantity":"1"}]),
    );
    for quantity in ["0", "01", "1.5", "1e3", "-1"] {
        invalid.push(json!([{"definitionId":definition,"section":"main","quantity":quantity}]));
    }
    invalid.push(json!([{"definitionId":Uuid::new_v4(),"section":"main","quantity":"1"}]));
    invalid.push(json!([{"definitionId":definition,"printingId":"not-a-uuid","section":"main","quantity":"1"}]));
    invalid
        .push(json!([{"definitionId":definition,"section":"main","quantity":"1","actorId":actor}]));
    for entries in invalid {
        assert_eq!(
            save(&client, actor, deck, None, entries, Uuid::new_v4()).await?["status"],
            "fail"
        );
    }
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.deck_revision WHERE deck_id=$1",
                &[&deck]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    let transaction = client.transaction().await?;
    transaction.execute("INSERT INTO app.deck_revision(deck_id,revision_number,created_at) VALUES($1,1,clock_timestamp())", &[&deck]).await?;
    assert!(transaction.commit().await.is_err());
    // Exact printing identity cannot be rewritten underneath sealed content.
    assert!(
        client
            .execute(
                "UPDATE app.printing SET definition_id=$1 WHERE id=$2",
                &[&definition, &printing]
            )
            .await
            .is_err()
    );
    // A full bounded draft, including maximum-size exact quantities/sections,
    // fits the shared command envelope rather than failing before validation.
    let large_entries: Vec<Value> = (0..1000)
        .map(|index| {
            json!({
                "definitionId":other_definition,"printingId":printing,
                "section":format!("main_{index:027}"),"quantity":"9".repeat(78)
            })
        })
        .collect();
    let revision = resource(
        save(
            &client,
            actor,
            deck,
            None,
            json!(large_entries),
            Uuid::new_v4(),
        )
        .await?,
    );
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.deck_entry WHERE revision_id=$1",
                &[&revision]
            )
            .await?
            .get::<_, i64>(0),
        1000
    );
    // An empty, sealed draft is legitimate storage; it is not admission approval.
    resource(
        save(
            &client,
            actor,
            deck,
            Some(revision),
            json!([]),
            Uuid::new_v4(),
        )
        .await?,
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn simultaneous_deck_saves_accept_only_one_expected_revision() -> TestResult {
    let client = connection().await?;
    let (actor, _, deck, definition, _) = library(&client).await?;
    let first = json!([{"definitionId":definition,"section":"main","quantity":"1"}]);
    let second = json!([{"definitionId":definition,"section":"main","quantity":"2"}]);
    let other = connection().await?;
    let (left, right) = tokio::join!(
        save(&client, actor, deck, None, first, Uuid::new_v4()),
        save(&other, actor, deck, None, second, Uuid::new_v4())
    );
    let statuses = [
        left?["status"].as_str().unwrap().to_owned(),
        right?["status"].as_str().unwrap().to_owned(),
    ];
    assert_eq!(statuses.iter().filter(|s| *s == "success").count(), 1);
    assert_eq!(statuses.iter().filter(|s| *s == "fail").count(), 1);
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.deck_revision WHERE deck_id=$1",
                &[&deck]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    Ok(())
}
