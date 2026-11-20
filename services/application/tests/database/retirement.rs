//! Actual private retirement transactions over synthetic owned domain fixtures.
//! These tests do not qualify missing host/provider producers or managed Admin HTTP.
use super::{TestResult, account, connection, decks, deletion, operation, pricing, social};
use serde_json::{Value, json};
use tokio_postgres::{Client, GenericClient, error::SqlState};
use uuid::Uuid;

async fn retire<C: GenericClient + Sync>(
    client: &C,
    actor: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one("SELECT app.retire_account($1)", &[&actor])
        .await?
        .get(0))
}

/// Measures the SQL transaction only, with synthetic holdings and no host/provider obligations.
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn large_retirement_preserves_exact_quantities_and_other_owners() -> TestResult {
    let mut client = connection().await?;
    let (actor, _) = account(&client).await?;
    let (peer, _) = account(&client).await?;
    let product = pricing::product(&client, true).await?;
    let source = operation(&client, actor).await?;
    // Seed 2,000 holdings in one statement, without changing the timeout for retirement itself.
    let seed = client.transaction().await?;
    seed.batch_execute("SET LOCAL statement_timeout = '60s'")
        .await?;
    let count = seed
        .execute(
            "WITH printings AS (
                INSERT INTO app.printing(definition_id,set_id,external_printing_ref,rarity,finish,image_ref)
                SELECT p.definition_id,p.set_id,gen_random_uuid()::text,'rare','nonfoil',NULL
                FROM app.asset_definition a JOIN app.printing p ON p.id=a.printing_id
                CROSS JOIN generate_series(1,2000) n WHERE a.id=$1 RETURNING id
             ), assets AS (
                INSERT INTO app.asset_definition(kind,printing_id)
                SELECT 'card',id FROM printings RETURNING id
             ) INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
             SELECT $2,app.ensure_inventory($3,id),'credit',3,'test',$2::uuid::text FROM assets",
            &[&product.cards[0], &source, &actor],
        )
        .await?;
    seed.commit().await?;
    assert_eq!(count, 2000);
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&source],
        )
        .await?;
    let coin: Uuid = client
        .query_one(
            "INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin',$1::uuid::text) RETURNING id",
            &[&Uuid::new_v4()],
        )
        .await?
        .get(0);
    let maximum = "9".repeat(78);
    let source = operation(&client, actor).await?;
    client.execute(
        "INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
         VALUES($1,app.ensure_inventory($2,$3),'credit',$4::text::numeric,'test',$1::uuid::text)",
        &[&source, &actor, &coin, &maximum],
    ).await?;
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&source],
        )
        .await?;
    let pack: Uuid = client
        .query_one(
            "INSERT INTO app.asset_definition(kind,recipe_id) VALUES('pack',$1) RETURNING id",
            &[&product.recipe],
        )
        .await?
        .get(0);
    credit(&client, actor, pack, 2).await?;
    credit(&client, peer, coin, 7).await?;
    // A zero holding must not generate a zero debit or hide a remaining positive holding.
    client
        .query_one(
            "SELECT app.ensure_inventory($1,$2)",
            &[&actor, &product.cards[1]],
        )
        .await?;
    assert_eq!(
        deletion::request(&client, actor, Uuid::new_v4()).await?["status"],
        "success"
    );

    let started = std::time::Instant::now();
    let accepted = retire(&client, actor).await?;
    let elapsed = started.elapsed();
    assert_eq!(accepted["data"]["state"], "retired");
    let operation: Uuid = accepted["data"]["retirementOperationId"]
        .as_str()
        .unwrap()
        .parse()?;
    let row = client.query_one(
        "SELECT (SELECT count(*) FROM app.inventory_asset WHERE owner_id=$1 AND owned_quantity<>0),
                (SELECT count(*) FROM app.ledger_posting WHERE operation_id=$2 AND direction='debit'),
                (SELECT count(*) FROM app.ledger_posting WHERE operation_id=$2 AND (direction<>'debit' OR quantity<=0)),
                (SELECT quantity::text FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id WHERE p.operation_id=$2 AND i.asset_definition_id=$3),
                (SELECT owned_quantity::text FROM app.inventory_asset WHERE owner_id=$4 AND asset_definition_id=$3),
                (SELECT count(*) FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id WHERE p.operation_id=$2 AND i.owner_id<>$1)",
        &[&actor, &operation, &coin, &peer],
    ).await?;
    assert_eq!(row.get::<_, i64>(0), 0);
    assert_eq!(row.get::<_, i64>(1), 2002);
    assert_eq!(row.get::<_, i64>(2), 0);
    assert_eq!(row.get::<_, String>(3), maximum);
    assert_eq!(row.get::<_, String>(4), "7");
    assert_eq!(row.get::<_, i64>(5), 0);
    assert_eq!(retire(&client, actor).await?, accepted);
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.ledger_posting WHERE operation_id=$1",
                &[&operation]
            )
            .await?
            .get::<_, i64>(0),
        2002
    );
    eprintln!(
        "retirement SQL capacity: 2002 positive holdings, 2000 distinct card printings, exact 78-digit coins, elapsed_ms={}",
        elapsed.as_millis()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn retirement_serializes_both_receipt_foreign_key_lock_orders() -> TestResult {
    let mut client = connection().await?;
    let (actor, _, _, _, format) = decks::library(&client).await?;
    deletion::request(&client, actor, Uuid::new_v4()).await?;
    let tx = client.transaction().await?;
    assert_eq!(retire(&tx, actor).await?["data"]["state"], "retired");
    let blocker: i32 = tx.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    let observer = connection().await?;
    let name = format!("retirement-before-receipt-{}", Uuid::new_v4());
    let task_name = name.clone();
    let child = tokio::spawn(async move {
        let writer = connection().await?;
        writer
            .query_one(
                "SELECT set_config('application_name',$1,false)",
                &[&task_name],
            )
            .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
            writer
                .query_one(
                    "SELECT app.create_deck($1,'Late private name','collection',$2,$3)",
                    &[&actor, &format, &Uuid::new_v4()],
                )
                .await?
                .get::<_, Value>(0),
        )
    });
    super::auth_admission::wait_on(&observer, &name, blocker, false).await?;
    tx.commit().await?;
    assert_eq!(child.await??["status"], "fail");
    assert_eq!(
        observer
            .query_one(
                "SELECT count(*) FROM app.deck WHERE account_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, i64>(0),
        1
    );

    let (actor, _) = account(&client).await?;
    deletion::request(&client, actor, Uuid::new_v4()).await?;
    let tx = client.transaction().await?;
    let pending:Uuid=tx.query_one("SELECT (app.begin_account_command($1,$2,'retirement_drain_fixture',$1::uuid::text,'{}')).id", &[&actor,&Uuid::new_v4()]).await?.get(0);
    let blocker: i32 = tx.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    let name = format!("receipt-before-retirement-{}", Uuid::new_v4());
    let task_name = name.clone();
    let child = tokio::spawn(async move {
        let finalizing = connection().await?;
        finalizing
            .query_one(
                "SELECT set_config('application_name',$1,false)",
                &[&task_name],
            )
            .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(retire(&finalizing, actor).await?)
    });
    super::auth_admission::wait_on(&observer, &name, blocker, false).await?;
    // The earlier FK holder must be able to finish its normal account work;
    // retirement has not acquired a weaker row lock and then tried to upgrade.
    tx.query_one(
        "SELECT id FROM app.account WHERE id=$1 FOR NO KEY UPDATE",
        &[&actor],
    )
    .await?;
    tx.query_one(
        "SELECT app.finish_account_command($1,true,jsonb_build_object('resourceId',$2::uuid))",
        &[&pending, &actor],
    )
    .await?;
    tx.commit().await?;
    assert_eq!(child.await??["data"]["state"], "retired");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unqualified_completed_game_and_provider_rows_block_private_retirement() -> TestResult {
    let client = connection().await?;
    let (actor, _, inventory) = super::fixture(&client).await?;
    let (peer, _) = account(&client).await?;
    social::participation_pair(&client, actor, peer, "open", 0, 3).await?;
    deletion::request(&client, actor, Uuid::new_v4()).await?;
    client.execute("INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at) SELECT match_id,match_id::text,'draw','unqualified:fixture',clock_timestamp() FROM app.match_seat WHERE account_id=$1", &[&actor]).await?;
    assert_eq!(
        client
            .query_one("SELECT app.read_account_deletion_status($1)", &[&actor])
            .await?
            .get::<_, Value>(0)["obligations"]["matchesWithoutResult"],
        "0"
    );
    assert_eq!(retire(&client, actor).await?["data"]["state"], "waiting");
    assert_eq!(
        client
            .query_one(
                "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, String>(0),
        "100"
    );
    let (actor, _, inventory) = super::fixture(&client).await?;
    deletion::request(&client, actor, Uuid::new_v4()).await?;
    client.execute("INSERT INTO app.provider_event(provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state) VALUES('patreon',$1::uuid::text,$1,clock_timestamp(),'unqualified:fixture','unqualified:fixture','processed')", &[&actor]).await?;
    assert_eq!(retire(&client, actor).await?["data"]["state"], "waiting");
    assert_eq!(
        client
            .query_one(
                "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, String>(0),
        "100"
    );
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for function in [
            "app.retire_account(uuid)",
            "app.read_account_retirement_work(uuid)",
            "app.account_auth_cleanup_receipt(uuid)",
            "app.account_retirement_blocked(uuid,uuid)",
            "app.read_account_retirement_proof(uuid,uuid)",
        ] {
            assert!(
                client
                    .query_one(
                        "SELECT has_function_privilege($1,$2,'EXECUTE')",
                        &[&role, &function]
                    )
                    .await?
                    .get::<_, bool>(0)
                    == (role == "service_role"
                        && matches!(
                            function,
                            "app.retire_account(uuid)" | "app.read_account_retirement_work(uuid)"
                        ))
            );
        }
    }
    Ok(())
}

async fn credit(client: &Client, actor: Uuid, definition: Uuid, quantity: i64) -> TestResult {
    let source = operation(client, actor).await?;
    client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,app.ensure_inventory($2,$3),'credit',$4::bigint,'test',$1::uuid::text)", &[&source,&actor,&definition,&quantity]).await?;
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&source],
        )
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn unqualified_rank_remains_blocked_while_unused_daily_state_can_retire() -> TestResult {
    let client = connection().await?;
    let format = Uuid::new_v4();
    let season = Uuid::new_v4();
    let set = Uuid::new_v4();
    client.execute("INSERT INTO app.format(id,key,name) VALUES($1,$1::uuid::text,'Unqualified rank fixture')", &[&format]).await?;
    client.execute("INSERT INTO app.season(id,year,quarter,start_at,end_at,policy_ref,state) VALUES($1,4242,1,'4242-01-01T00:00:00Z','4242-04-01T00:00:00Z','unqualified:fixture','closed')", &[&season]).await?;
    client.execute("INSERT INTO app.card_set(id,external_set_ref,name) VALUES($1,$1::uuid::text,'Unqualified ballot fixture')", &[&set]).await?;
    for kind in ["rank", "daily_win", "daily_quest", "ballot"] {
        let (actor, _, inventory) = super::fixture(&client).await?;
        deletion::request(&client, actor, Uuid::new_v4()).await?;
        match kind {
            "rank" => {
                client.execute("INSERT INTO app.rank(account_id,format_id,season_id,tier,rating_ref,revision,finalized) VALUES($1,$2,$3,'bronze','unqualified:rating',0,false)", &[&actor,&format,&season]).await?;
            }
            "daily_win" => {
                client.execute("INSERT INTO app.daily_win_bucket(account_id,utc_day,wins,issued_coins,revision) VALUES($1,'2026-11-02',0,0,0)", &[&actor]).await?;
            }
            "daily_quest" => {
                client.execute("INSERT INTO app.daily_quest_state(account_id,utc_day,rerolls_used,revision) VALUES($1,'2026-11-02',0,0)", &[&actor]).await?;
            }
            "ballot" => {
                assert!(client.execute("INSERT INTO app.set_ballot(account_id,season_id,set_id,revision) VALUES($1,$2,$3,0)", &[&actor,&season,&set]).await.is_err());
                // An unavailable, unqualified ballot cannot become a fabricated
                // retirement anchor. Its source guard leaves no row behind.
                assert_eq!(retire(&client, actor).await?["data"]["state"], "retired");
                continue;
            }
            _ => unreachable!(),
        }
        let status: Value = client
            .query_one("SELECT app.read_account_deletion_status($1)", &[&actor])
            .await?
            .get(0);
        assert!(
            status["obligations"]
                .as_object()
                .unwrap()
                .values()
                .all(|value| value == "0")
        );
        assert_eq!(
            retire(&client, actor).await?["data"]["state"],
            if kind == "rank" { "waiting" } else { "retired" },
            "{kind}"
        );
        assert_eq!(
            client
                .query_one(
                    "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                    &[&inventory]
                )
                .await?
                .get::<_, String>(0),
            if kind == "rank" { "100" } else { "0" }
        );
    }
    Ok(())
}
async fn observe(
    client: &Client,
    actor: Uuid,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT app.observe_account_deletion($1,$2,$3)",
            &[&actor, &request, &"a".repeat(64)],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn canonical_retirement_burns_all_assets_once_minimizes_and_waits_for_auth_cleanup()
-> TestResult {
    let mut client = connection().await?;
    let (actor, subject, deck, _, _) = decks::library(&client).await?;
    let product = pricing::product(&client, true).await?;
    let coin:Uuid=client.query_one("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin',$1::uuid::text) RETURNING id", &[&Uuid::new_v4()]).await?.get(0);
    let pack: Uuid = client
        .query_one(
            "INSERT INTO app.asset_definition(kind,recipe_id) VALUES('pack',$1) RETURNING id",
            &[&product.recipe],
        )
        .await?
        .get(0);
    for (definition, quantity) in [(coin, 1234), (product.cards[0], 3), (pack, 2)] {
        credit(&client, actor, definition, quantity).await?;
    }
    let username: String = client
        .query_one("SELECT username FROM app.account WHERE id=$1", &[&actor])
        .await?
        .get(0);
    let free: Value = client
        .query_one(
            "SELECT jsonb_build_array(back_id,sleeve_id,dice_id) FROM app.deck WHERE id=$1",
            &[&deck],
        )
        .await?
        .get(0);
    client
        .execute(
            "UPDATE app.deck SET name='Private draft label' WHERE id=$1",
            &[&deck],
        )
        .await?;
    let (peer, _) = account(&client).await?;
    social::friend(&client, actor, peer).await?;
    let opened: Value = client
        .query_one(
            "SELECT app.open_direct_conversation($1,$2,$3)",
            &[&actor, &peer, &Uuid::new_v4()],
        )
        .await?
        .get(0);
    let conversation: Uuid = opened["data"]["resourceId"].as_str().unwrap().parse()?;
    for (sender, body) in [
        (actor, "Preserve approved history"),
        (peer, "Private inbox notification"),
    ] {
        let sent: Value = client
            .query_one(
                "SELECT app.send_direct_message($1,$2,$3,$4)",
                &[&sender, &conversation, &body, &Uuid::new_v4()],
            )
            .await?
            .get(0);
        assert_eq!(sent["status"], "success");
    }
    let request = Uuid::new_v4();
    assert_eq!(
        deletion::request(&client, actor, request).await?["status"],
        "success"
    );
    let accepted = retire(&client, actor).await?;
    assert_eq!(accepted["data"]["state"], "retired");
    let operation: Uuid = accepted["data"]["retirementOperationId"]
        .as_str()
        .unwrap()
        .parse()?;
    assert_eq!(retire(&client, actor).await?, accepted);
    let row=client.query_one("SELECT (SELECT count(*) FROM app.inventory_asset WHERE owner_id=$1 AND owned_quantity<>0),(SELECT count(*) FROM app.ledger_posting WHERE operation_id=$2 AND direction='debit'),(SELECT count(*) FROM app.ledger_posting WHERE operation_id=$2 AND direction='credit'),(SELECT count(*) FROM app.profile WHERE account_id=$1),(SELECT count(*) FROM app.cosmetic_grant WHERE account_id=$1 AND state='active'),(SELECT count(*) FROM app.notification WHERE recipient_id=$1),(SELECT state FROM app.asset_owner WHERE account_id=$1)", &[&actor,&operation]).await?;
    assert_eq!(
        (
            row.get::<_, i64>(0),
            row.get::<_, i64>(1),
            row.get::<_, i64>(2)
        ),
        (0, 3, 0)
    );
    assert_eq!(
        (
            row.get::<_, i64>(3),
            row.get::<_, i64>(4),
            row.get::<_, i64>(5)
        ),
        (0, 0, 0)
    );
    assert_eq!(row.get::<_, String>(6), "retired");
    let saved:Value=client.query_one("SELECT jsonb_build_object('name',name,'appearance',jsonb_build_array(back_id,sleeve_id,dice_id),'playmat',playmat_id) FROM app.deck WHERE id=$1", &[&deck]).await?.get(0);
    assert_eq!(
        saved,
        json!({"name":"Deleted deck","appearance":free,"playmat":null})
    );
    let row = client
        .query_one(
            "SELECT username,auth_subject,auth_cleanup_subject FROM app.account WHERE id=$1",
            &[&actor],
        )
        .await?;
    assert_eq!(row.get::<_, String>(0), username);
    assert_eq!(row.get::<_, Option<Uuid>>(1), None);
    assert_eq!(row.get::<_, Option<Uuid>>(2), Some(subject));
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.message WHERE conversation_id=$1",
                &[&conversation]
            )
            .await?
            .get::<_, i64>(0),
        2
    );
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.conversation_member WHERE conversation_id=$1",
                &[&conversation]
            )
            .await?
            .get::<_, i64>(0),
        2
    );
    assert_eq!(
        observe(&client, actor, request).await?["data"]["phase"],
        "finalizing"
    );
    let tx = client.transaction().await?;
    super::auth_identity::canonical_delete(&tx, subject).await?;
    tx.rollback().await?;
    assert_eq!(
        observe(&client, actor, request).await?["data"]["phase"],
        "finalizing"
    );
    let tx = client.transaction().await?;
    super::auth_identity::canonical_delete(&tx, subject).await?;
    tx.commit().await?;
    let final_state = observe(&client, actor, request).await?;
    assert_eq!(final_state["data"]["phase"], "deleted");
    assert_eq!(
        final_state["data"]["retirementOperationId"],
        operation.to_string()
    );
    assert!(
        final_state["data"]["authCleanupReceiptId"]
            .as_str()
            .is_some()
    );
    assert_eq!(retire(&client, actor).await?, accepted);
    assert_eq!(client.query_one("SELECT count(*) FROM app.ledger_operation WHERE kind='retirement' AND actor_account_id=$1", &[&actor]).await?.get::<_,i64>(0),1);
    let history=client.query_one("SELECT author,body FROM app.read_conversation_messages($1,$2,NULL,NULL,100) WHERE sender_id=$3", &[&peer,&conversation,&actor]).await?;
    assert_eq!(history.get::<_, String>(0), "Deleted player");
    assert_eq!(history.get::<_, String>(1), "Preserve approved history");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn retirement_requires_real_request_and_blocks_even_an_empty_hold() -> TestResult {
    let client = connection().await?;
    let (actor, _, inventory) = super::fixture(&client).await?;
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&actor]).await?;
    assert_eq!(retire(&client, actor).await?["data"]["state"], "waiting");
    assert_eq!(
        deletion::request(&client, actor, Uuid::new_v4()).await?["status"],
        "success"
    );
    let source = operation(&client, actor).await?;
    let hold:Uuid=client.query_one("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,'ante',$2::uuid::text,'held',$2) RETURNING id", &[&actor,&source]).await?.get(0);
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&source],
        )
        .await?;
    assert_eq!(
        client
            .query_one("SELECT app.read_account_deletion_status($1)", &[&actor])
            .await?
            .get::<_, Value>(0)["obligations"]["heldCommitments"],
        "0"
    );
    assert_eq!(retire(&client, actor).await?["data"]["state"], "waiting");
    assert_eq!(
        client
            .query_one(
                "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, String>(0),
        "100"
    );
    client
        .execute(
            "UPDATE app.asset_hold SET state='released' WHERE id=$1",
            &[&hold],
        )
        .await?;
    assert_eq!(retire(&client, actor).await?["data"]["state"], "retired");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn retirement_rejects_stale_isolation_and_rolls_back_all_effects_on_late_failure()
-> TestResult {
    let mut client = connection().await?;
    let (actor, subject, deck, _, _) = decks::library(&client).await?;
    let coin:Uuid=client.query_one("INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin',$1::uuid::text) RETURNING id", &[&Uuid::new_v4()]).await?.get(0);
    credit(&client, actor, coin, 100).await?;
    deletion::request(&client, actor, Uuid::new_v4()).await?;
    for isolation in ["REPEATABLE READ", "SERIALIZABLE"] {
        let tx = client.transaction().await?;
        tx.batch_execute(&format!("SET TRANSACTION ISOLATION LEVEL {isolation}"))
            .await?;
        assert_eq!(
            retire(&tx, actor).await.unwrap_err().code(),
            Some(&SqlState::T_R_SERIALIZATION_FAILURE)
        );
        tx.rollback().await?;
    }
    let suffix = actor.simple().to_string();
    let function = format!("test_fixture.fail_retirement_{suffix}");
    let trigger = format!("test_retirement_{suffix}");
    client.batch_execute(&format!("CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.id='{actor}'::uuid AND NEW.auth_subject IS NULL AND OLD.auth_subject IS NOT NULL THEN RAISE EXCEPTION 'owned retirement failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER {trigger} AFTER UPDATE ON app.account FOR EACH ROW EXECUTE FUNCTION {function}()" )).await?;
    let tx = client.transaction().await?;
    assert_eq!(
        retire(&tx, actor).await.unwrap_err().code(),
        Some(&SqlState::RAISE_EXCEPTION)
    );
    tx.rollback().await?;
    client
        .batch_execute(&format!(
            "DROP TRIGGER {trigger} ON app.account; DROP FUNCTION {function}()"
        ))
        .await?;
    let row=client.query_one("SELECT state,auth_subject,retirement_operation_id,(SELECT sum(owned_quantity)::text FROM app.inventory_asset WHERE owner_id=$1),(SELECT count(*) FROM app.cosmetic_grant WHERE account_id=$1 AND state='active'),(SELECT name FROM app.deck WHERE id=$2),(SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='account_retirement_v1') FROM app.account WHERE id=$1", &[&actor,&deck]).await?;
    assert_eq!(row.get::<_, String>(0), "deletion_pending");
    assert_eq!(row.get::<_, Uuid>(1), subject);
    assert_eq!(row.get::<_, Option<Uuid>>(2), None);
    assert_eq!(row.get::<_, String>(3), "100");
    assert_eq!(row.get::<_, i64>(4), 3);
    assert_eq!(row.get::<_, String>(5), "Saved deck");
    assert_eq!(row.get::<_, i64>(6), 0);
    assert_eq!(retire(&client, actor).await?["data"]["state"], "retired");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn retirement_worker_rediscovers_committed_cleanup_and_never_completes_from_missing_target()
-> TestResult {
    let mut client = connection().await?;
    let (actor, subject) = account(&client).await?;
    let active_error = client
        .query_one("SELECT app.read_account_retirement_work($1)", &[&actor])
        .await
        .unwrap_err();
    assert_eq!(
        active_error.code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
    );
    deletion::request(&client, actor, Uuid::new_v4()).await?;
    let pending: Value = client
        .query_one("SELECT app.read_account_retirement_work($1)", &[&actor])
        .await?
        .get(0);
    assert_eq!(pending, json!({"state":"pending","accountId":actor}));
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(retire(&client, actor).await?["data"]["state"], "retired");
    let cleanup: Value = client
        .query_one("SELECT app.read_account_retirement_work($1)", &[&actor])
        .await?
        .get(0);
    assert_eq!(cleanup["state"], "cleanup");
    assert_eq!(cleanup["subject"], subject.to_string());
    let operation = cleanup["retirementOperationId"].clone();
    let page: Value = client
        .query_one(
            "SELECT app.read_deletion_account_page($1,1)",
            &[&Uuid::from_u128(actor.as_u128() - 1)],
        )
        .await?
        .get(0);
    assert_eq!(page["items"], json!([actor]));
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE app.account SET auth_cleanup_subject=NULL WHERE id=$1",
            &[&actor],
        )
        .await?;
    let unproven: Value = client
        .query_one("SELECT app.read_account_retirement_work($1)", &[&actor])
        .await?
        .get(0);
    assert_eq!(
        unproven,
        json!({"state":"unproven","accountId":actor,"retirementOperationId":operation})
    );
    let page: Value = client
        .query_one(
            "SELECT app.read_deletion_account_page($1,1)",
            &[&Uuid::from_u128(actor.as_u128() - 1)],
        )
        .await?
        .get(0);
    assert_eq!(page["items"], json!([actor]));
    client
        .execute(
            "UPDATE app.account SET auth_cleanup_subject=$2 WHERE id=$1",
            &[&actor, &subject],
        )
        .await?;
    let tx = client.transaction().await?;
    super::auth_identity::canonical_delete(&tx, subject).await?;
    tx.rollback().await?;
    let retry: Value = client
        .query_one("SELECT app.read_account_retirement_work($1)", &[&actor])
        .await?
        .get(0);
    assert_eq!(retry, cleanup);
    let tx = client.transaction().await?;
    super::auth_identity::canonical_delete(&tx, subject).await?;
    tx.commit().await?;
    client.batch_execute("SET ROLE service_role").await?;
    let complete: Value = client
        .query_one("SELECT app.read_account_retirement_work($1)", &[&actor])
        .await?
        .get(0);
    assert_eq!(complete["state"], "complete");
    assert_eq!(complete["retirementOperationId"], operation);
    assert!(complete.get("subject").is_none());
    assert!(
        !complete["authCleanupReceiptId"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    let page: Value = client
        .query_one(
            "SELECT app.read_deletion_account_page($1,1)",
            &[&Uuid::from_u128(actor.as_u128() - 1)],
        )
        .await?
        .get(0);
    assert_ne!(page["items"], json!([actor]));
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn invalid_legacy_retirement_source_stays_discoverable_without_stalling_later_accounts()
-> TestResult {
    let client = connection().await?;
    let (first, _) = account(&client).await?;
    let (second, _) = account(&client).await?;
    let (broken, healthy) = if first < second {
        (first, second)
    } else {
        (second, first)
    };
    let wrong_source = operation(&client, broken).await?;
    // Explicit owner-created legacy corruption, not a canonical producer proof.
    client.execute("UPDATE app.account SET state='deleted',auth_subject=NULL,retirement_operation_id=$2,deletion_requested_at=clock_timestamp(),deletion_finalized_at=clock_timestamp() WHERE id=$1", &[&broken,&wrong_source]).await?;
    deletion::request(&client, healthy, Uuid::new_v4()).await?;
    let page: Value = client
        .query_one(
            "SELECT app.read_deletion_account_page($1,1)",
            &[&Uuid::from_u128(broken.as_u128() - 1)],
        )
        .await?
        .get(0);
    assert_eq!(page["items"], json!([broken]));
    assert_eq!(
        client
            .query_one("SELECT app.read_account_retirement_work($1)", &[&broken])
            .await
            .unwrap_err()
            .code(),
        Some(&tokio_postgres::error::SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    let page: Value = client
        .query_one(
            "SELECT app.read_deletion_account_page($1,1)",
            &[&Uuid::from_u128(healthy.as_u128() - 1)],
        )
        .await?
        .get(0);
    assert_eq!(page["items"], json!([healthy]));
    assert_eq!(retire(&client, healthy).await?["data"]["state"], "retired");
    assert_eq!(
        client
            .query_one("SELECT app.account_auth_cleanup_receipt($1)", &[&broken])
            .await?
            .get::<_, Option<Uuid>>(0),
        None
    );
    Ok(())
}
