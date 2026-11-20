//! Real Postgres tests; use the isolated local database, never a production URL.
use spellwake_application::amount::Amount;
use std::error::Error;
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[path = "database/game_scope.rs"]
mod game_scope;

#[path = "database/season_calendar.rs"]
mod season_calendar;

#[path = "database/lobbies.rs"]
mod lobbies;

#[path = "database/lobby_access.rs"]
mod lobby_access;

#[path = "database/tier_integrity.rs"]
mod tier_integrity;

#[path = "database/progression_integrity.rs"]
mod progression_integrity;

#[path = "database/result_integrity.rs"]
mod result_integrity;

#[path = "database/lobby_cleanup_integrity.rs"]
mod lobby_cleanup_integrity;

#[path = "database/source_race_integrity.rs"]
mod source_race_integrity;

#[path = "database/patreon_financial_integrity.rs"]
mod patreon_financial_integrity;

#[path = "database/ai_runtime_integrity.rs"]
mod ai_runtime_integrity;

#[path = "database/ai_ready_integrity.rs"]
mod ai_ready_integrity;

#[path = "database/room_start_integrity.rs"]
mod room_start_integrity;

#[path = "database/queue_intent_integrity.rs"]
mod queue_intent_integrity;

#[path = "database/lobby_entry_integrity.rs"]
mod lobby_entry_integrity;

#[path = "database/admission_integrity.rs"]
mod admission_integrity;

#[path = "database/admission_port_contracts.rs"]
mod admission_port_contracts;

#[path = "database/win_reward_integrity.rs"]
mod win_reward_integrity;

#[path = "database/replay_publication_integrity.rs"]
mod replay_publication_integrity;

#[path = "database/operational_terminal_integrity.rs"]
mod operational_terminal_integrity;

#[path = "database/recovery_integrity.rs"]
mod recovery_integrity;

#[path = "database/host_integrity.rs"]
mod host_integrity;

#[path = "database/monthly_activity_integrity.rs"]
mod monthly_activity_integrity;

#[path = "database/quest_assignment_integrity.rs"]
mod quest_assignment_integrity;

#[path = "database/quest_reroll_boundaries.rs"]
mod quest_reroll_boundaries;

#[path = "database/participation_integrity.rs"]
mod participation_integrity;

#[path = "database/controller_integrity.rs"]
mod controller_integrity;

#[path = "database/game_chat_integrity.rs"]
mod game_chat_integrity;

#[path = "database/ante_scope_integrity.rs"]
mod ante_scope_integrity;

#[path = "database/ai_budget_integrity.rs"]
mod ai_budget_integrity;

#[path = "database/reference_history_integrity.rs"]
mod reference_history_integrity;

#[path = "database/import_integrity.rs"]
mod import_integrity;

#[path = "database/auth_receipts.rs"]
mod auth_receipts;

#[path = "database/auth_sessions.rs"]
mod auth_sessions;

#[path = "database/auth_provenance.rs"]
mod auth_provenance;

#[path = "database/asset_lifecycle.rs"]
mod asset_lifecycle;

#[path = "database/stripe.rs"]
mod stripe;

#[path = "database/auth_identity.rs"]
mod auth_identity;

#[path = "database/auth_admission.rs"]
mod auth_admission;

#[path = "database/account_api.rs"]
mod account_api;

#[path = "database/current_session.rs"]
mod current_session;

#[path = "database/deletion.rs"]
mod deletion;

#[path = "database/retirement.rs"]
mod retirement;

#[path = "database/inventory.rs"]
mod inventory;

#[path = "database/packs.rs"]
mod packs;

#[path = "database/prediction_resolution.rs"]
mod prediction_resolution;

#[path = "database/predictions.rs"]
mod predictions;

#[path = "database/market.rs"]
mod market;

#[path = "database/trades.rs"]
mod trades;

#[path = "database/pricing.rs"]
mod pricing;

#[path = "database/cosmetics.rs"]
mod cosmetics;

#[path = "database/catalogue.rs"]
mod catalogue;
#[path = "database/catalogue_publication.rs"]
mod catalogue_publication;
#[path = "database/deck_settings.rs"]
mod deck_settings;
#[path = "database/decks.rs"]
mod decks;
#[path = "database/profile_defaults.rs"]
mod profile_defaults;

#[path = "database/social.rs"]
mod social;

#[path = "database/social_commands.rs"]
mod social_commands;

#[path = "database/social_reads.rs"]
mod social_reads;

#[path = "database/social_read_ack.rs"]
mod social_read_ack;

#[path = "database/economy_commands.rs"]
mod economy_commands;

#[path = "database/exchange_reads.rs"]
mod exchange_reads;

async fn connection() -> Result<Client, Box<dyn Error + Send + Sync>> {
    let address = std::env::var("SPELLWAKE_TEST_DATABASE_URL")?;
    // This harness intentionally refuses remote or credential-bearing connections.
    let base = "host=127.0.0.1 port=55439 user=postgres dbname=spellwake_application";
    let isolated_name =
        |suffix: &str| suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit());
    let fresh_test_target = address
        .strip_prefix(&(base.to_owned() + "_test_"))
        .is_some_and(isolated_name);
    // An owned stand-alone cluster listens only on its private Unix socket. Both
    // the socket directory and the disposable database carry generated identities.
    let private_socket = address
        .strip_prefix("host=/tmp/spellwake-pg-")
        .and_then(|rest| {
            rest.split_once(" port=55440 user=postgres dbname=spellwake_application_test_")
        })
        .is_some_and(|(directory, name)| isolated_name(directory) && isolated_name(name));
    if address != base && !fresh_test_target && !private_socket {
        return Err("database test target must be the isolated local Postgres instance".into());
    }
    let (client, connection) = tokio_postgres::connect(&address, NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("application test database connection failed: {error}");
        }
    });
    if private_socket {
        let owned: bool = client.query_one(
            "SELECT current_setting('cluster_name') = 'spellwake-api-qualification' AND inet_server_addr() IS NULL", &[]
        ).await?.get(0);
        if !owned {
            return Err("private test socket does not identify the qualification cluster".into());
        }
    }
    client.batch_execute("SET statement_timeout='10s'").await?;
    Ok(client)
}

/// Observe an actual competing SQL lock before releasing a test transaction.
async fn wait_for_database_lock(c: &Client, pid: i32) -> TestResult {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if c.query_one("SELECT cardinality(pg_blocking_pids($1))>0", &[&pid])
            .await?
            .get::<_, bool>(0)
        {
            return Ok(());
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "expected actual SQL lock wait"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

async fn account(client: &Client) -> Result<(Uuid, Uuid), Box<dyn Error + Send + Sync>> {
    account_with_age(client, 0).await
}

// Historical fixture creation, not a production import or an age-changing command.
async fn account_with_age(
    client: &Client,
    age_seconds: i64,
) -> Result<(Uuid, Uuid), Box<dyn Error + Send + Sync>> {
    account_fixture(client, age_seconds, None).await
}

/// Optional owner identity exercises joins without assuming owner and account IDs coincide.
async fn account_fixture(
    client: &Client,
    age_seconds: i64,
    owner_id: Option<Uuid>,
) -> Result<(Uuid, Uuid), Box<dyn Error + Send + Sync>> {
    let account = Uuid::new_v4();
    let subject = Uuid::new_v4();
    let username = format!("test_{}", &account.simple().to_string()[..16]);
    client
        .execute("INSERT INTO auth.users(id) VALUES($1)", &[&subject])
        .await?;
    seed_auth_authority(client, subject).await?;
    client.execute("INSERT INTO app.account(id,auth_subject,username,sign_in_method,created_at,state) VALUES($1,$2,$3,'email_code',clock_timestamp()-($4::bigint*interval '1 second'),'active')", &[&account,&subject,&username,&age_seconds]).await?;
    client
        .execute(
            "INSERT INTO app.asset_owner(id,account_id,state) VALUES($1,$2,'active')",
            &[&owner_id.unwrap_or(account), &account],
        )
        .await?;
    Ok((account, subject))
}

/// Owner-only synthetic authority for unrelated domain/RLS fixtures.
async fn seed_auth_authority(
    client: &Client,
    subject: Uuid,
) -> Result<Uuid, Box<dyn Error + Send + Sync>> {
    let session = Uuid::new_v4();
    client
        .execute(
            "SELECT test_fixture.seed_auth_authority($1,$2)",
            &[&subject, &session],
        )
        .await?;
    Ok(session)
}

/// Temporarily use the original owner for fixture lookup, then restore the read role.
async fn set_auth_claims(client: &Client, subject: Uuid) -> TestResult {
    client.batch_execute("RESET ROLE").await?;
    let claims: serde_json::Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    assert!(
        claims.is_object(),
        "fixture subject has no seeded authority"
    );
    client.batch_execute("SET ROLE authenticated").await?;
    client
        .query_one(
            "SELECT set_config('request.jwt.claims',$1,false)",
            &[&claims.to_string()],
        )
        .await?;
    Ok(())
}

async fn operation(client: &Client, account: Uuid) -> Result<Uuid, Box<dyn Error + Send + Sync>> {
    let id = Uuid::new_v4();
    client.execute("INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES($1,'test',$1::uuid::text,'test',$2,clock_timestamp(),'pending')", &[&id,&account]).await?;
    Ok(id)
}

async fn fixture(client: &Client) -> Result<(Uuid, Uuid, Uuid), Box<dyn Error + Send + Sync>> {
    let (account, subject) = account(client).await?;
    let definition = Uuid::new_v4();
    client
        .execute(
            "INSERT INTO app.asset_definition(id,kind,currency_key) VALUES($1,'coin',$1::uuid::text)",
            &[&definition],
        )
        .await?;
    let row = client
        .query_one(
            "SELECT app.ensure_inventory($1,$2)",
            &[&account, &definition],
        )
        .await?;
    let asset: Uuid = row.get(0);
    let source = operation(client, account).await?;
    client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'credit',100,'test',$1::uuid::text)", &[&source,&asset]).await?;
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&source],
        )
        .await?;
    Ok((account, subject, asset))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn schema_permissions_and_exact_owned_quantities() -> TestResult {
    let client = connection().await?;
    let counts = client.query_one("SELECT (SELECT count(*) FROM pg_tables WHERE schemaname='app')::bigint,(SELECT count(*) FROM pg_views WHERE schemaname='app')::bigint", &[]).await?;
    assert_eq!(counts.get::<_, i64>(0), 95);
    assert_eq!(counts.get::<_, i64>(1), 10);
    let (owner, subject, asset) = fixture(&client).await?;
    let (_, other_subject) = account(&client).await?;
    client.batch_execute("SET ROLE authenticated").await?;
    set_auth_claims(&client, subject).await?;
    let row = client
        .query_one(
            "SELECT available::text FROM app.asset_availability WHERE inventory_asset_id=$1",
            &[&asset],
        )
        .await?;
    assert_eq!(row.get::<_, String>(0).parse::<Amount>()?, 100.into());
    assert!(
        client
            .execute(
                "UPDATE app.inventory_asset SET owned_quantity=1000 WHERE id=$1",
                &[&asset]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "INSERT INTO app.asset_owner(account_id,state) VALUES($1,'active')",
                &[&owner]
            )
            .await
            .is_err()
    );
    set_auth_claims(&client, other_subject).await?;
    assert!(
        client
            .query("SELECT id FROM app.inventory_asset WHERE id=$1", &[&asset])
            .await?
            .is_empty()
    );
    client.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert!(
        client
            .query("SELECT * FROM app.coin_balance", &[])
            .await
            .is_err()
    );
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    assert!(
        client
            .execute(
                "UPDATE app.inventory_asset SET owned_quantity=1000 WHERE id=$1",
                &[&asset]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn held_assets_cannot_be_spent_or_reserved_twice() -> TestResult {
    let mut client = connection().await?;
    let (owner, _, asset) = fixture(&client).await?;
    let reserve_operation = operation(&client, owner).await?;
    let first_hold = Uuid::new_v4();
    let second_hold = Uuid::new_v4();
    for hold in [first_hold, second_hold] {
        client.execute("INSERT INTO app.asset_hold(id,owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,'ante',$1::uuid::text,'held',$3)", &[&hold,&owner,&reserve_operation]).await?;
    }
    let first_transaction = client.transaction().await?;
    first_transaction
        .execute(
            "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,80)",
            &[&first_hold, &asset],
        )
        .await?;
    let application_name = format!("hold-race-{}", Uuid::new_v4());
    let second_name = application_name.clone();
    let (ready, started) = tokio::sync::oneshot::channel();
    let waiting = tokio::spawn(async move {
        let second = connection().await?;
        second
            .query_one(
                "SELECT set_config('application_name',$1,false)",
                &[&second_name],
            )
            .await?;
        ready.send(()).map_err(|_| "race observer disappeared")?;
        let result = second
            .execute(
                "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,80)",
                &[&second_hold, &asset],
            )
            .await;
        Ok::<_, Box<dyn Error + Send + Sync>>(result)
    });
    started.await?;
    // Prove the second backend is actually waiting on our lock, rather than merely running later.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        first_transaction
            .batch_execute("SELECT pg_stat_clear_snapshot()")
            .await?;
        let state = first_transaction.query_one(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock')",
            &[&application_name],
        ).await?;
        if state.get::<_, bool>(0) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "second reservation never reached the lock"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    first_transaction.commit().await?;
    let rejected = waiting.await??;
    assert_eq!(
        rejected.unwrap_err().code(),
        Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
    );
    let spending = operation(&client, owner).await?;
    let debit = client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'debit',21,'test',$1::uuid::text)", &[&spending,&asset]).await;
    assert_eq!(
        debit.unwrap_err().code(),
        Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
    );
    let row = client.query_one("SELECT owned::text,held::text,available::text FROM app.asset_availability WHERE inventory_asset_id=$1", &[&asset]).await?;
    assert_eq!(
        (
            row.get::<_, String>(0),
            row.get::<_, String>(1),
            row.get::<_, String>(2)
        ),
        ("100".into(), "80".into(), "20".into())
    );
    client
        .execute(
            "UPDATE app.asset_hold SET state='released' WHERE id=$1",
            &[&first_hold],
        )
        .await?;
    assert!(
        client
            .execute(
                "UPDATE app.asset_hold SET state='held' WHERE id=$1",
                &[&first_hold]
            )
            .await
            .is_err()
    );
    client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'debit',100,'test',$1::uuid::text)", &[&spending,&asset]).await?;
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&spending],
        )
        .await?;
    assert!(
        client
            .execute(
                "DELETE FROM app.ledger_posting WHERE operation_id=$1",
                &[&spending]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.ledger_operation SET state='pending' WHERE id=$1",
                &[&spending]
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn registration_is_source_unique_and_grants_no_starter_assets() -> TestResult {
    let client = connection().await?;
    let subject = Uuid::new_v4();
    client
        .execute("INSERT INTO auth.users(id) VALUES($1)", &[&subject])
        .await?;
    let session = seed_auth_authority(&client, subject).await?;
    let back = Uuid::new_v4();
    let sleeve = Uuid::new_v4();
    let dice = Uuid::new_v4();
    for (id, kind) in [(back, "back"), (sleeve, "sleeve"), (dice, "dice")] {
        client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,$2,'test-only:free-cosmetic','active',true)", &[&id,&kind]).await?;
    }
    let extra_back = Uuid::new_v4();
    let extra_sleeve = Uuid::new_v4();
    for (id, kind) in [(extra_back, "back"), (extra_sleeve, "sleeve")] {
        client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES($1,$2,'test-only:unselected-free','active',true)", &[&id,&kind]).await?;
    }
    let username = format!("signup_{}", &subject.simple().to_string()[..16]);
    let first = connection().await?;
    let second = connection().await?;
    first.batch_execute("SET ROLE service_role").await?;
    second.batch_execute("SET ROLE service_role").await?;
    let sql = "SELECT app.register_account($1,$2,$3,$4,$5,$6)";
    let parameters: &[&(dyn tokio_postgres::types::ToSql + Sync)] =
        &[&subject, &session, &username, &back, &sleeve, &dice];
    let (left, right) = tokio::join!(
        first.query_one(sql, parameters),
        second.query_one(sql, parameters)
    );
    let id: Uuid = left?.get(0);
    assert_eq!(id, right?.get::<_, Uuid>(0));
    let holdings = client.query("SELECT d.kind,i.owned_quantity::text FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE o.account_id=$1", &[&id]).await?;
    assert_eq!(holdings.len(), 1);
    assert_eq!(holdings[0].get::<_, String>(0), "coin");
    assert_eq!(holdings[0].get::<_, String>(1), "14500");
    assert_eq!(client.query_one("SELECT count(*) FROM app.ledger_operation WHERE source_scope='signup' AND source_ref=$1", &[&id.to_string()]).await?.get::<_,i64>(0),1);
    // Assert this pre-existing catalogue snapshot, not fixtures other parallel tests add later.
    let known_free = vec![back, sleeve, dice, extra_back, extra_sleeve];
    assert_eq!(client.query_one("SELECT count(*) FROM app.cosmetic_grant WHERE account_id=$1 AND cosmetic_id=ANY($2) AND source_kind='free' AND state='active'", &[&id,&known_free]).await?.get::<_,i64>(0),5);
    let missing_session = Uuid::new_v4();
    assert_eq!(
        first
            .query_one(
                sql,
                &[&subject, &missing_session, &username, &back, &sleeve, &dice]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&tokio_postgres::error::SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    let removed_signature = first
        .query_one(
            "SELECT app.register_account($1::uuid,$2::text,'google'::text,$3::uuid,$4::uuid,$5::uuid)",
            &[&subject, &username, &back, &sleeve, &dice],
        )
        .await
        .unwrap_err();
    assert_eq!(
        removed_signature.code(),
        Some(&tokio_postgres::error::SqlState::UNDEFINED_FUNCTION)
    );
    client.batch_execute("SET ROLE authenticated").await?;
    let denied = client.query_one(sql, parameters).await.unwrap_err();
    assert_eq!(
        denied.code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn skipped_duplicate_posting_has_no_balance_effect() -> TestResult {
    let client = connection().await?;
    let (owner, _, asset) = fixture(&client).await?;
    let operation = operation(&client, owner).await?;
    let posting = Uuid::new_v4();
    let sql = "INSERT INTO app.ledger_posting(id,operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,$3,'credit',100,'test',$2::uuid::text) ON CONFLICT(id) DO NOTHING";
    assert_eq!(
        client.execute(sql, &[&posting, &operation, &asset]).await?,
        1
    );
    assert_eq!(
        client.execute(sql, &[&posting, &operation, &asset]).await?,
        0
    );
    let row = client
        .query_one(
            "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
            &[&asset],
        )
        .await?;
    assert_eq!(row.get::<_, String>(0), "200");
    assert_eq!(
        client
            .query_one(
                "SELECT sum(quantity)::text FROM app.ledger_posting WHERE inventory_asset_id=$1",
                &[&asset]
            )
            .await?
            .get::<_, String>(0),
        "200"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn stale_repeatable_read_reservation_must_retry() -> TestResult {
    let mut first = connection().await?;
    let (owner, _, asset) = fixture(&first).await?;
    let operation = operation(&first, owner).await?;
    let first_hold = Uuid::new_v4();
    let second_hold = Uuid::new_v4();
    for hold in [first_hold, second_hold] {
        first.execute("INSERT INTO app.asset_hold(id,owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,'ante',$1::uuid::text,'held',$3)",&[&hold,&owner,&operation]).await?;
    }
    let stale = first
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    assert_eq!(
        stale
            .query_one(
                "SELECT count(*) FROM app.hold_line WHERE inventory_asset_id=$1",
                &[&asset]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    let second = connection().await?;
    second
        .execute(
            "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,80)",
            &[&second_hold, &asset],
        )
        .await?;
    let conflict = stale
        .execute(
            "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,80)",
            &[&first_hold, &asset],
        )
        .await
        .unwrap_err();
    assert_eq!(
        conflict.code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    stale.rollback().await?;
    assert_eq!(
        second
            .query_one(
                "SELECT available::text FROM app.asset_availability WHERE inventory_asset_id=$1",
                &[&asset]
            )
            .await?
            .get::<_, String>(0),
        "20"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn sql_rejects_fractional_quantities_and_future_helpers_are_private() -> TestResult {
    let client = connection().await?;
    let (owner, _, asset) = fixture(&client).await?;
    let operation = operation(&client, owner).await?;
    let fractional=client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'credit',1.5,'test',$1::uuid::text)",&[&operation,&asset]).await.unwrap_err();
    assert_eq!(
        fractional.code(),
        Some(&tokio_postgres::error::SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        client
            .query_one(
                "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                &[&asset]
            )
            .await?
            .get::<_, String>(0),
        "100"
    );
    let name = format!("app.acl_probe_{}", Uuid::new_v4().simple());
    // Identifier consists only of a fixed prefix and generated UUID hex, never request text.
    client.batch_execute(&format!("CREATE FUNCTION {name}() RETURNS integer LANGUAGE sql SECURITY DEFINER SET search_path='' AS 'SELECT 1'" )).await?;
    let signature = format!("{name}()");
    for role in ["anon", "authenticated", "service_role"] {
        assert!(
            !client
                .query_one(
                    "SELECT has_function_privilege($1,$2,'EXECUTE')",
                    &[&role, &signature]
                )
                .await?
                .get::<_, bool>(0)
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn sql_maturity_uses_elapsed_time_across_dst_and_both_conditions() -> TestResult {
    let client = connection().await?;
    client
        .batch_execute("SET TIME ZONE 'America/New_York'")
        .await?;
    let row=client.query_one("SELECT app.trade_restricted('2026-10-16 12:00-04','2026-11-15 11:00-05',20),app.trade_restricted('2026-10-16 12:00-04','2026-11-15 10:59:59-05',20),app.trade_restricted('2026-10-16 12:00-04','2026-11-15 11:00-05',19)",&[]).await?;
    assert!(!row.get::<_, bool>(0));
    assert!(row.get::<_, bool>(1));
    assert!(row.get::<_, bool>(2));
    let dependency=client.query_one("SELECT EXISTS(SELECT 1 FROM pg_rewrite r JOIN pg_depend d ON d.classid='pg_rewrite'::regclass AND d.objid=r.oid WHERE r.ev_class='app.account_qualifying_count'::regclass AND d.refobjid='app.trade_restricted(timestamptz,timestamptz,numeric)'::regprocedure::oid)",&[]).await?;
    assert!(
        dependency.get::<_, bool>(0),
        "view must use the tested elapsed-time policy"
    );
    Ok(())
}

#[path = "database/pack_shop.rs"]
mod pack_shop;

#[path = "database/prediction_commands.rs"]
mod prediction_commands;

#[path = "database/prediction_reads.rs"]
mod prediction_reads;

#[path = "database/prediction_publication.rs"]
mod prediction_publication;

#[path = "database/patreon_association.rs"]
mod patreon_association;

#[path = "database/patreon_intake.rs"]
mod patreon_intake;

#[path = "database/patreon_refresh.rs"]
mod patreon_refresh;

#[path = "database/supply_source_integrity.rs"]
mod supply_source_integrity;

#[path = "database/match_lifecycle_integrity.rs"]
mod match_lifecycle_integrity;

#[path = "database/rank_source_integrity.rs"]
mod rank_source_integrity;

#[path = "database/supply_ballot_integrity.rs"]
mod supply_ballot_integrity;

#[path = "database/quest_consumer_integrity.rs"]
mod quest_consumer_integrity;

#[path = "database/ai_ante_port_contracts.rs"]
mod ai_ante_port_contracts;
#[path = "database/ante_transaction_integrity.rs"]
mod ante_transaction_integrity;

#[path = "database/controller_rebind_integrity.rs"]
mod controller_rebind_integrity;

#[path = "database/retirement_cleanup_integrity.rs"]
mod retirement_cleanup_integrity;

#[path = "database/retirement_source_obligations_integrity.rs"]
mod retirement_source_obligations_integrity;

#[path = "database/retirement_minimization_integrity.rs"]
mod retirement_minimization_integrity;

#[path = "database/patreon_notification_processing_integrity.rs"]
mod patreon_notification_processing_integrity;

#[path = "database/game_message_integrity.rs"]
mod game_message_integrity;

#[path = "database/clock_bank_integrity.rs"]
mod clock_bank_integrity;

#[path = "database/bank_forfeit_integrity.rs"]
mod bank_forfeit_integrity;

#[path = "database/match_frontier_integrity.rs"]
mod match_frontier_integrity;

#[path = "database/native_creator_integrity.rs"]
mod native_creator_integrity;

#[path = "database/native_elimination_integrity.rs"]
mod native_elimination_integrity;

#[path = "database/between_deadline_integrity.rs"]
mod between_deadline_integrity;

#[path = "database/between_expiry_integrity.rs"]
mod between_expiry_integrity;

#[path = "database/between_deadline_recovery_integrity.rs"]
mod between_deadline_recovery_integrity;

#[path = "database/handoff_integrity.rs"]
mod handoff_integrity;

#[path = "database/whole_access.rs"]
mod whole_access;

#[path = "database/import_commands.rs"]
mod import_commands;

#[path = "database/progression_reads.rs"]
mod progression_reads;

#[path = "database/room_command_recovery.rs"]
mod room_command_recovery;

#[path = "database/room_reads.rs"]
mod room_reads;

#[path = "database/room_configuration.rs"]
mod room_configuration;

#[path = "database/room_invitations.rs"]
mod room_invitations;

#[path = "database/auth_after_wait.rs"]
mod auth_after_wait;

#[path = "database/matchmaking_api.rs"]
mod matchmaking_api;

#[path = "database/chat_notifications_api.rs"]
mod chat_notifications_api;

#[path = "database/profile_art_api.rs"]
mod profile_art_api;

#[path = "database/manual_ai_slot.rs"]
mod manual_ai_slot;

#[path = "database/human_foundation.rs"]
mod human_foundation;

#[path = "database/supply_port_contracts.rs"]
mod supply_port_contracts;

#[path = "database/controller_stop_paging.rs"]
mod controller_stop_paging;

#[path = "database/native_opening_context.rs"]
mod native_opening_context;

#[path = "database/native_source_reader.rs"]
mod native_source_reader;

#[path = "database/native_admission_source.rs"]
mod native_admission_source;

#[path = "database/native_foundation.rs"]
mod native_foundation;
