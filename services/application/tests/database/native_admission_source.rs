//! Real SQL source observation; fixture refs do not qualify native card execution.
use super::{
    TestResult, ante_transaction_integrity as ante, connection,
    match_lifecycle_integrity as lifecycle,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;
async fn read(c: &Client, game: Uuid) -> Result<Value, tokio_postgres::Error> {
    Ok(
        c.query_one("SELECT app.read_native_admission_source($1)", &[&game])
            .await?
            .get(0),
    )
}
async fn game(c: &Client, m: Uuid) -> Result<Uuid, tokio_postgres::Error> {
    Ok(
        c.query_one("SELECT reserved_game_id FROM app.match WHERE id=$1", &[&m])
            .await?
            .get(0),
    )
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn original_reserved_source_preserves_exact_deck_release_and_printing_read_only() -> TestResult
{
    let c = connection().await?;
    let (f, m) = ante::reserve_match(&c, true, "bo1").await?;
    let g = game(&c, m).await?;
    c.batch_execute("BEGIN READ ONLY").await?;
    let source = read(&c, g).await?;
    assert_eq!(source["status"], "success");
    let data = &source["data"];
    assert_eq!(data["version"], "native-admission-source-v1");
    assert_eq!(data["gameId"], json!(g));
    assert_eq!(data["matchId"], json!(m));
    assert_eq!(data["roomId"], json!(f.id));
    assert_eq!(data["gameIndex"], "0");
    assert_eq!(data["release"]["id"], json!(f.source.release));
    for seat in data["seats"].as_array().unwrap() {
        assert_eq!(seat["selectedRevisionId"], seat["originalRevisionId"]);
        assert!(seat["selectionDecisionId"].is_null());
        assert!(seat["selectionReplyRef"].is_null());
        let id: Uuid = seat["selectedRevisionId"].as_str().unwrap().parse()?;
        let expected:Value=c.query_one("SELECT jsonb_agg(jsonb_build_object('entryId',e.id,'definitionId',e.definition_id,'oracleRef',d.oracle_ref,'definitionRevisionId',b.id,'definitionRevisionRef',b.revision_ref,'rulesTextRef',b.rules_text_ref,'programRef',p.program_ref,'unlimitedBasicLand',p.unlimited_basic_land,'printing',CASE WHEN x.id IS NULL THEN NULL ELSE jsonb_build_object('id',x.id,'externalPrintingRef',x.external_printing_ref,'finish',x.finish,'setId',x.set_id) END,'section',e.section,'quantity',e.quantity::text) ORDER BY e.id) FROM app.deck_entry e JOIN app.card_definition d ON d.id=e.definition_id JOIN app.card_definition_revision b ON b.definition_id=d.id JOIN app.release_card_program p ON p.definition_revision_id=b.id AND p.release_id=$2 LEFT JOIN app.printing x ON x.id=e.printing_id WHERE e.revision_id=$1",&[&id,&f.source.release]).await?.get(0);
        assert_eq!(seat["entries"], expected);
    }
    assert_eq!(read(&c, g).await?, source);
    assert_eq!(
        c.query_one(
            "SELECT count(*) FROM app.game_context WHERE game_id=$1",
            &[&g]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    c.batch_execute("COMMIT").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn only_reserved_accepted_sources_are_available_and_service_role_alone_can_read() -> TestResult
{
    let c = connection().await?;
    let (_, m) = ante::reserve_match(&c, true, "bo1").await?;
    let g = game(&c, m).await?;
    let unavailable = json!({"status":"fail","data":{"game":"unavailable"}});
    for id in [Uuid::nil(), Uuid::new_v4()] {
        assert_eq!(read(&c, id).await?, unavailable);
    }
    assert_eq!(
        c.query_one("SELECT app.read_native_admission_source(NULL)", &[])
            .await?
            .get::<_, Value>(0),
        unavailable
    );
    for role in ["anon", "authenticated", "supabase_auth_admin"] {
        c.batch_execute(&format!("BEGIN; SET LOCAL ROLE {role}"))
            .await?;
        assert_eq!(
            read(&c, g).await.unwrap_err().code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        c.batch_execute("ROLLBACK").await?;
    }
    c.batch_execute("BEGIN READ ONLY; SET LOCAL ROLE service_role")
        .await?;
    assert_eq!(read(&c, g).await?["status"], "success");
    c.batch_execute("COMMIT").await?;
    lifecycle::open(&c, m, g).await?;
    assert_eq!(read(&c, g).await?, unavailable);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn sideboard_selection_is_not_original_revision_and_fourth_game_after_draws_is_valid()
-> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve_match(&c, true, "bo3").await?;
    let mut g = game(&c, m).await?;
    let deck: Uuid = c
        .query_one(
            "SELECT deck_id FROM app.deck_revision WHERE id=$1",
            &[&f.source.revision],
        )
        .await?
        .get(0);
    let selected:Value=c.query_one("SELECT app.save_deck_contents($1,$2,$3,$4,$5)",&[&f.source.actor,&deck,&f.source.revision,&json!([
        {"definitionId":f.source.definition,"printingId":f.source.printing,"section":"main","quantity":"1"},
        {"definitionId":f.source.definition,"printingId":f.source.printing,"section":"sideboard","quantity":"1"}
    ]),&Uuid::new_v4()]).await?.get(0);
    assert_eq!(selected["status"], "success");
    let selected: Uuid = selected["data"]["resourceId"].as_str().unwrap().parse()?;
    for index in 0..4 {
        let source = read(&c, g).await?;
        assert_eq!(source["status"], "success");
        assert_eq!(source["data"]["gameIndex"], index.to_string());
        let seat = source["data"]["seats"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["accountId"] == json!(f.source.actor))
            .unwrap();
        assert_eq!(seat["originalRevisionId"], json!(f.source.revision));
        assert_eq!(
            seat["selectedRevisionId"],
            json!(if index == 0 {
                f.source.revision
            } else {
                selected
            })
        );
        if index > 0 {
            assert_eq!(seat["entries"].as_array().unwrap().len(), 2);
            assert!(
                seat["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["section"] == "sideboard")
            );
            assert!(!seat["selectionDecisionId"].is_null());
        }
        if index == 1 {
            let decision: Uuid = seat["selectionDecisionId"].as_str().unwrap().parse()?;
            for sql in [
                "UPDATE app.between_game_decision SET after_game_id=gen_random_uuid() WHERE id=$1",
                "UPDATE app.between_game_decision SET seat_id=gen_random_uuid() WHERE id=$1",
                "DELETE FROM app.command_receipt WHERE scope='between_game_reply_v1' AND target_ref=$1::uuid::text",
                "UPDATE app.deck_entry SET quantity=quantity+1 WHERE revision_id=(SELECT selected_deck_revision_id FROM app.between_game_decision WHERE id=$1)",
                "UPDATE app.game SET state='active' WHERE id=(SELECT after_game_id FROM app.between_game_decision WHERE id=$1)",
            ] {
                c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
                    .await?;
                c.execute(sql, &[&decision]).await?;
                c.batch_execute("SET LOCAL session_replication_role=origin")
                    .await?;
                assert_eq!(
                    read(&c, g).await.unwrap_err().code(),
                    Some(&SqlState::CHECK_VIOLATION)
                );
                c.batch_execute("ROLLBACK").await?;
                assert_eq!(read(&c, g).await?, source);
            }
        }
        if index == 2 {
            // Keep game zero/start provenance intact, so this reaches the
            // qualified-history consistency error rather than unavailable.
            let decision: Uuid = seat["selectionDecisionId"].as_str().unwrap().parse()?;
            c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
                .await?;
            c.execute("UPDATE app.game SET game_index=game_index+10 WHERE id=(SELECT after_game_id FROM app.between_game_decision WHERE id=$1)",&[&decision]).await?;
            c.batch_execute("SET LOCAL session_replication_role=origin")
                .await?;
            assert_eq!(
                read(&c, g).await.unwrap_err().code(),
                Some(&SqlState::CHECK_VIOLATION)
            );
            c.batch_execute("ROLLBACK").await?;
            assert_eq!(read(&c, g).await?, source);
        }
        if index == 3 {
            break;
        }
        lifecycle::open(&c, m, g).await?;
        let result = lifecycle::complete(&c, m, g, vec![]).await?;
        assert_eq!(
            lifecycle::consume(&c, m, result).await?["status"],
            "success"
        );
        for row in c.query("SELECT d.id,s.account_id,s.source_revision_id FROM app.between_game_decision d JOIN app.match_seat s ON s.id=d.seat_id WHERE d.match_id=$1 AND d.after_game_id=$2 ORDER BY d.seat_id",&[&m,&g]).await? {
            let decision:Uuid=row.get(0);let owner:Option<Uuid>=row.get(1);let original:Uuid=row.get(2);
            let revision=if owner==Some(f.source.actor){selected}else{original};
            let reply:Value=c.query_one("SELECT app.reply_between_games($1,(SELECT revision FROM app.match WHERE id=$2),$3,'fixture:sideboard','fixture:validator',$4)",&[&decision,&m,&revision,&Uuid::new_v4()]).await?.get(0);
            assert_eq!(reply["status"],"success");
        }
        g = game(&c, m).await?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn immutable_identity_guards_cover_legacy_revisions_without_freezing_display_names()
-> TestResult {
    let c = connection().await?;
    let (f, _) = ante::reserve_match(&c, true, "bo1").await?;
    c.execute(
        "UPDATE app.card_definition SET name='Updated display label' WHERE id=$1",
        &[&f.source.definition],
    )
    .await?;
    for sql in [
        "UPDATE app.card_definition SET oracle_ref='changed' WHERE id=$1",
        "UPDATE app.card_definition SET id=gen_random_uuid() WHERE id=$1",
    ] {
        assert_eq!(
            c.execute(sql, &[&f.source.definition])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    let body: Uuid = c
        .query_one(
            "SELECT definition_revision_id FROM app.release_card_program WHERE release_id=$1",
            &[&f.source.release],
        )
        .await?
        .get(0);
    for sql in [
        "UPDATE app.card_definition_revision SET revision_ref='changed' WHERE id=$1",
        "UPDATE app.card_definition_revision SET rules_text_ref='changed' WHERE id=$1",
        "UPDATE app.card_definition_revision SET id=gen_random_uuid() WHERE id=$1",
        "UPDATE app.card_definition_revision SET definition_id=gen_random_uuid() WHERE id=$1",
    ] {
        assert_eq!(
            c.execute(sql, &[&body]).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn repeatable_read_observation_does_not_lock_or_reserve_the_opening() -> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve_match(&c, true, "bo1").await?;
    let g = game(&c, m).await?;
    c.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await?;
    let before = read(&c, g).await?;
    assert_eq!(before["status"], "success");
    let writer = connection().await?;
    lifecycle::open(&writer, m, g).await?;
    assert_eq!(read(&c, g).await?, before);
    c.batch_execute("COMMIT").await?;
    assert_eq!(read(&c, g).await?["status"], "fail");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn nullable_printing_and_exact_quantity_bound_are_preserved_without_inventory_revalidation()
-> TestResult {
    use super::room_start_integrity as room;
    let c = connection().await?;
    for quantity in [2, 1000, 1001] {
        let f = room::room(&c, true).await?;
        let deck: Uuid = c
            .query_one(
                "SELECT deck_id FROM app.deck_revision WHERE id=$1",
                &[&f.source.revision],
            )
            .await?
            .get(0);
        let saved:Value=c.query_one("SELECT app.save_deck_contents($1,$2,$3,$4,$5)",&[&f.source.actor,&deck,&f.source.revision,&json!([{"definitionId":f.source.definition,"printingId":null,"section":"main","quantity":quantity.to_string()}]),&Uuid::new_v4()]).await?.get(0);
        assert_eq!(saved["status"], "success");
        let revision: Uuid = saved["data"]["resourceId"].as_str().unwrap().parse()?;
        let selected:Value=c.query_one("SELECT app.select_room_deck($1,$2,(SELECT revision FROM app.room WHERE id=$2),$3,$4)",&[&f.source.actor,&f.id,&revision,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(selected["status"], "success");
        let countdown:Value=c.query_one("SELECT app.start_room_countdown($1,$2,(SELECT revision FROM app.room WHERE id=$2),'fixture:deck',$3)",&[&f.source.actor,&f.id,&Uuid::new_v4()]).await?.get(0);
        assert_eq!(countdown["status"], "success");
        c.execute(
            "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
            &[&f.id],
        )
        .await?;
        let started = room::start(&c, &f, Uuid::new_v4()).await?;
        assert_eq!(started["status"], "success");
        let g: Uuid = started["data"]["gameId"].as_str().unwrap().parse()?;
        let source = read(&c, g).await?;
        if quantity > 1000 {
            assert_eq!(
                source,
                json!({"status":"fail","data":{"game":"unavailable"}})
            );
            continue;
        }
        assert_eq!(source["status"], "success");
        let seat = source["data"]["seats"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["accountId"] == json!(f.source.actor))
            .unwrap();
        assert!(seat["entries"][0]["printing"].is_null());
        assert_eq!(seat["entries"][0]["quantity"], quantity.to_string());
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn qualified_frontier_and_ambiguous_dictionary_corruptions_raise_without_partial_results()
-> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve_match(&c, true, "bo1").await?;
    let g = game(&c, m).await?;
    let before = read(&c, g).await?;
    assert_eq!(before["status"], "success");
    // Owner-only corruption controls deliberately bypass mutation triggers, then
    // restore normal checking before the read. They are never admission inputs.
    // The immediate table CHECK rejects a dual frontier even with replication
    // triggers disabled; verify that existing lower-layer rejection directly.
    assert_eq!(
        c.execute(
            "UPDATE app.match SET current_game_id=reserved_game_id WHERE id=$1",
            &[&m]
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
        .await?;
    c.execute(
        "UPDATE app.game SET accepted_revision=1 WHERE match_id=$1",
        &[&m],
    )
    .await?;
    c.batch_execute("SET LOCAL session_replication_role=origin")
        .await?;
    assert_eq!(
        read(&c, g).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(read(&c, g).await?, before);
    c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
        .await?;
    c.execute(
        "DELETE FROM app.release_card_program WHERE release_id=$1",
        &[&f.source.release],
    )
    .await?;
    c.batch_execute("SET LOCAL session_replication_role=origin")
        .await?;
    assert_eq!(
        read(&c, g).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(read(&c, g).await?, before);
    c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
        .await?;
    let body:Uuid=c.query_one("INSERT INTO app.card_definition_revision(definition_id,revision_ref,rules_text_ref) VALUES($1,'fixture:ambiguous','fixture') RETURNING id",&[&f.source.definition]).await?.get(0);
    c.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref) VALUES($1,$2,'fixture')",&[&f.source.release,&body]).await?;
    c.batch_execute("SET LOCAL session_replication_role=origin")
        .await?;
    assert_eq!(
        read(&c, g).await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    c.batch_execute("ROLLBACK").await?;
    assert_eq!(read(&c, g).await?, before);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn missing_provenance_and_each_native_frame_are_rejected_without_promoting_partial_sources()
-> TestResult {
    let c = connection().await?;
    let (_, m) = ante::reserve_match(&c, true, "bo1").await?;
    let g = game(&c, m).await?;
    let before = read(&c, g).await?;
    for scope in ["match_decks_v1", "room_start_v1"] {
        c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
            .await?;
        c.execute("DELETE FROM app.command_receipt WHERE scope=$1 AND (target_ref=$2::uuid::text OR response_payload->'data'->>'resourceId'=$2::uuid::text)",&[&scope,&m]).await?;
        c.batch_execute("SET LOCAL session_replication_role=origin")
            .await?;
        assert_eq!(
            read(&c, g).await?,
            json!({"status":"fail","data":{"game":"unavailable"}})
        );
        c.batch_execute("ROLLBACK").await?;
        assert_eq!(read(&c, g).await?, before);
    }
    for sql in [
        "INSERT INTO app.game_context(game_id,release_id,profile_id,snapshot_ref,context_revision) SELECT $1,m.accepted_release_id,m.profile_id,'fixture','fixture' FROM app.game g JOIN app.match m ON m.id=g.match_id WHERE g.id=$1",
        "INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision) VALUES($1,gen_random_uuid(),0,'fixture','fixture',0)",
        "INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES($1,gen_random_uuid(),0,0,'fixture')",
        "INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason) VALUES($1,gen_random_uuid(),'draw','fixture','fixture')",
        "INSERT INTO app.operational_state(game_id,owner_instance_ref,fence_epoch,availability,revision) VALUES($1,'fixture',1,'available',0)",
        "INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at) SELECT match_id,gen_random_uuid()::text,'draw','fixture',clock_timestamp() FROM app.game WHERE id=$1",
    ] {
        c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
            .await?;
        c.execute(sql, &[&g]).await?;
        c.batch_execute("SET LOCAL session_replication_role=origin")
            .await?;
        assert_eq!(
            read(&c, g).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        c.batch_execute("ROLLBACK").await?;
        assert_eq!(read(&c, g).await?, before);
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn empty_and_distinct_entry_bounds_and_unknown_section_preservation_are_explicit()
-> TestResult {
    let c = connection().await?;
    let (f, m) = ante::reserve_match(&c, true, "bo1").await?;
    let g = game(&c, m).await?;
    let before = read(&c, g).await?;
    for many in [false, true] {
        c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
            .await?;
        c.execute(
            "DELETE FROM app.deck_entry WHERE revision_id=$1",
            &[&f.source.revision],
        )
        .await?;
        if many {
            c.execute("INSERT INTO app.deck_entry(revision_id,definition_id,printing_id,section,quantity) SELECT $1,$2,$3,'custom_'||n,1 FROM generate_series(1,1001) n",&[&f.source.revision,&f.source.definition,&f.source.printing]).await?;
        }
        c.batch_execute("SET LOCAL session_replication_role=origin")
            .await?;
        assert_eq!(
            read(&c, g).await?,
            json!({"status":"fail","data":{"game":"unavailable"}})
        );
        c.batch_execute("ROLLBACK").await?;
        assert_eq!(read(&c, g).await?, before);
    }
    c.batch_execute("BEGIN; SET LOCAL session_replication_role=replica")
        .await?;
    c.execute(
        "UPDATE app.deck_entry SET section='custom_role' WHERE revision_id=$1",
        &[&f.source.revision],
    )
    .await?;
    c.batch_execute("SET LOCAL session_replication_role=origin")
        .await?;
    let changed = read(&c, g).await?;
    assert_eq!(changed["status"], "success");
    let seat = changed["data"]["seats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["accountId"] == json!(f.source.actor))
        .unwrap();
    assert_eq!(seat["entries"][0]["section"], "custom_role");
    c.batch_execute("ROLLBACK").await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn accepted_nonbasic_deck_source_survives_actual_inventory_transfer_after_match_acceptance()
-> TestResult {
    use super::room_start_integrity as room;
    let c = connection().await?;
    let mut f = room::room(&c, true).await?;
    let body: Uuid = c
        .query_one(
            "SELECT definition_revision_id FROM app.release_card_program WHERE release_id=$1",
            &[&f.source.release],
        )
        .await?
        .get(0);
    let release:Uuid=c.query_one("INSERT INTO app.qualified_release(release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref) VALUES($1,'fixture','fixture','fixture','fixture') RETURNING id",&[&Uuid::new_v4().to_string()]).await?.get(0);
    c.execute("INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref) VALUES($1,$2,'fixture')",&[&release,&body]).await?;
    f.source.release = release;
    let asset: Uuid = c
        .query_one(
            "SELECT id FROM app.asset_definition WHERE printing_id=$1",
            &[&f.source.printing],
        )
        .await?
        .get(0);
    let mut inventories = vec![];
    for actor in [f.source.actor, f.other] {
        let inventory: Uuid = c
            .query_one("SELECT app.ensure_inventory($1,$2)", &[&actor, &asset])
            .await?
            .get(0);
        inventories.push(inventory);
        let op = super::operation(&c, actor).await?;
        c.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'credit',2,'fixture',$1::uuid::text)",&[&op,&inventory]).await?;
        c.execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&op],
        )
        .await?;
    }
    let countdown:Value=c.query_one("SELECT app.start_room_countdown($1,$2,(SELECT revision FROM app.room WHERE id=$2),'fixture:deck',$3)",&[&f.source.actor,&f.id,&Uuid::new_v4()]).await?.get(0);
    assert_eq!(countdown["status"], "success");
    c.execute(
        "UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        &[&f.id],
    )
    .await?;
    let started = room::start(&c, &f, Uuid::new_v4()).await?;
    assert_eq!(started["status"], "success");
    let g: Uuid = started["data"]["gameId"].as_str().unwrap().parse()?;
    let before = read(&c, g).await?;
    assert_eq!(before["status"], "success");
    let transfer = super::operation(&c, f.source.actor).await?;
    c.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'debit',2,'fixture',$1::uuid::text),($1,$3,'credit',2,'fixture',$1::uuid::text)",&[&transfer,&inventories[0],&inventories[1]]).await?;
    c.execute(
        "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
        &[&transfer],
    )
    .await?;
    assert_eq!(
        c.query_one(
            "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
            &[&inventories[0]]
        )
        .await?
        .get::<_, String>(0),
        "0"
    );
    assert_eq!(read(&c, g).await?, before);
    Ok(())
}
