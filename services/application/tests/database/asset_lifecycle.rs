//! Privileged lifecycle/source fixtures are not the full account finalizer.
use super::{
    TestResult, account, auth_admission::wait_on, auth_identity::mark_deleted, connection, fixture,
    operation,
};
use tokio_postgres::{Client, GenericClient, error::SqlState};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn all_application_relations_deny_raw_mutation_and_personal_recreation() -> TestResult {
    let client = connection().await?;
    let relations = client.query("SELECT c.oid::bigint,c.relname::text,c.relkind::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='app' AND c.relkind IN ('r','v') ORDER BY c.relname", &[]).await?;
    assert_eq!(
        relations
            .iter()
            .filter(|row| row.get::<_, String>(2) == "r")
            .count(),
        90
    );
    assert_eq!(
        relations
            .iter()
            .filter(|row| row.get::<_, String>(2) == "v")
            .count(),
        10
    );
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for relation in &relations {
            let table: String = relation.get(1);
            let target = format!("app.{table}");
            for privilege in [
                "INSERT",
                "UPDATE",
                "DELETE",
                "TRUNCATE",
                "REFERENCES",
                "TRIGGER",
                "MAINTAIN",
            ] {
                assert!(
                    !client
                        .query_one(
                            "SELECT has_table_privilege($1,$2,$3)",
                            &[&role, &target, &privilege]
                        )
                        .await?
                        .get::<_, bool>(0),
                    "{role} {table} {privilege}"
                );
            }
            let columns = client.query("SELECT a.attname::text FROM pg_attribute a WHERE a.attrelid=$1::text::regclass AND a.attnum>0 AND NOT a.attisdropped", &[&target]).await?;
            for column in columns {
                let column: String = column.get(0);
                for privilege in ["INSERT", "UPDATE", "REFERENCES"] {
                    assert!(
                        !client
                            .query_one(
                                "SELECT has_column_privilege($1,$2,$3,$4)",
                                &[&role, &target, &column, &privilege]
                            )
                            .await?
                            .get::<_, bool>(0),
                        "{role} {table}.{column} {privilege}"
                    );
                }
            }
        }
    }
    let (owner, _) = account(&client).await?;
    // The source fixture marks retirement; it does not qualify a finalizer.
    for retired in [false, true] {
        if retired {
            mark_deleted(&client, owner).await?;
        }
        client.batch_execute("SET ROLE service_role").await?;
        for sql in [
            "INSERT INTO app.profile(account_id) VALUES($1)",
            "UPDATE app.profile SET crop_x=0 WHERE account_id=$1",
            "INSERT INTO app.match_result(match_id,source_ref) VALUES($1,'forbidden')",
            "INSERT INTO app.game_participation(account_id) VALUES($1)",
        ] {
            assert_eq!(
                client.execute(sql, &[&owner]).await.unwrap_err().code(),
                Some(&SqlState::INSUFFICIENT_PRIVILEGE),
                "retired={retired}: {sql}"
            );
        }
        client.batch_execute("RESET ROLE").await?;
    }
    Ok(())
}

async fn definition(client: &Client) -> Result<Uuid, tokio_postgres::Error> {
    let id = Uuid::new_v4();
    client.execute("INSERT INTO app.asset_definition(id,kind,currency_key) VALUES($1,'coin',$1::uuid::text)",&[&id]).await?;
    Ok(id)
}
async fn burn_fixture_balance(client: &Client, owner: Uuid, inventory: Uuid) -> TestResult {
    let operation = operation(client, owner).await?;
    client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'debit',100,'test',$1::uuid::text)",&[&operation,&inventory]).await?;
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&operation],
        )
        .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn effective_asset_privileges_close_all_nonreads_and_unused_helpers() -> TestResult {
    let client = connection().await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for table in [
            "asset_owner",
            "inventory_asset",
            "ledger_operation",
            "ledger_posting",
            "asset_hold",
            "hold_line",
        ] {
            for privilege in [
                "INSERT",
                "UPDATE",
                "DELETE",
                "TRUNCATE",
                "REFERENCES",
                "TRIGGER",
                "MAINTAIN",
            ] {
                assert!(
                    !client
                        .query_one(
                            "SELECT has_table_privilege($1,$2,$3)",
                            &[&role, &format!("app.{table}"), &privilege]
                        )
                        .await?
                        .get::<_, bool>(0),
                    "{role} {table} {privilege}"
                );
            }
        }
        for function in [
            "app.lock_asset_account_for_write(uuid)",
            "app.ensure_inventory(uuid,uuid)",
            "app.prepare_exchange_inventory(uuid,uuid,uuid[])",
            "app.apply_exchange(uuid,uuid,jsonb,jsonb,uuid,text)",
            "app.guard_asset_write_lifecycle()",
            "app.guard_asset_owner_lifecycle()",
            "app.lock_prediction_market(uuid,uuid)",
            "app.refresh_prediction_hold(uuid)",
            "app.cancel_prediction_unfilled(uuid,text)",
        ] {
            assert!(
                !client
                    .query_one(
                        "SELECT has_function_privilege($1,$2,'EXECUTE')",
                        &[&role, &function]
                    )
                    .await?
                    .get::<_, bool>(0),
                "{role} {function}"
            );
        }
    }
    let (owner, _) = account(&client).await?;
    let asset = definition(&client).await?;
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        client
            .query_one("SELECT app.ensure_inventory($1,$2)", &[&owner, &asset])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    assert_eq!(
        client
            .execute(
                "UPDATE app.asset_owner SET state='retired' WHERE id=$1",
                &[&owner]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    );
    client.batch_execute("RESET ROLE").await?;
    // The actual AFTER application and early trigger order must survive060.
    let applied:bool=client.query_one("SELECT (tgtype&2)=0 FROM pg_trigger WHERE tgrelid='app.ledger_posting'::regclass AND tgname='ledger_posting_apply'",&[]).await?.get(0);
    assert!(applied);
    for table in ["inventory_asset", "asset_hold", "hold_line"] {
        let first:String=client.query_one("SELECT tgname::text FROM pg_trigger WHERE tgrelid=$1::text::regclass AND NOT tgisinternal ORDER BY tgname LIMIT 1",&[&format!("app.{table}")]).await?.get(0);
        assert!(first.starts_with("asset_lifecycle_"));
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn owner_retirement_requires_deleted_source_zero_inventory_and_no_empty_hold() -> TestResult {
    let client = connection().await?;
    let (owner, _, inventory) = fixture(&client).await?;
    assert_eq!(
        client
            .execute(
                "UPDATE app.asset_owner SET state='retired' WHERE id=$1",
                &[&owner]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    // A committed source alone cannot erase positive quantities.
    mark_deleted(&client, owner).await?;
    assert_eq!(
        client
            .execute(
                "UPDATE app.asset_owner SET state='retired' WHERE id=$1",
                &[&owner]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
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
    let (empty, _, balance) = fixture(&client).await?;
    let reserve = operation(&client, empty).await?;
    client.execute("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,'ante',$2::uuid::text,'held',$2)",&[&empty,&reserve]).await?;
    burn_fixture_balance(&client, empty, balance).await?;
    mark_deleted(&client, empty).await?;
    assert_eq!(
        client
            .execute(
                "UPDATE app.asset_owner SET state='retired' WHERE id=$1",
                &[&empty]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    // Zero assets and the exact source permit a terminal owner; this does not
    // assert that application/provider/match obligations were finalized.
    let (zero, _) = account(&client).await?;
    mark_deleted(&client, zero).await?;
    client
        .execute(
            "UPDATE app.asset_owner SET state='retired' WHERE id=$1",
            &[&zero],
        )
        .await?;
    for query in [
        "UPDATE app.asset_owner SET state='active' WHERE id=$1",
        "UPDATE app.asset_owner SET state='retired' WHERE id=$1",
        "DELETE FROM app.asset_owner WHERE id=$1",
    ] {
        assert_eq!(
            client.execute(query, &[&zero]).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn deleted_or_retired_owners_cannot_create_post_or_reserve_assets() -> TestResult {
    let client = connection().await?;
    let (owner, _, inventory) = fixture(&client).await?;
    burn_fixture_balance(&client, owner, inventory).await?;
    let old = operation(&client, owner).await?;
    let hold:Uuid=client.query_one("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,'ante',$2::uuid::text,'held',$2) RETURNING id",&[&owner,&old]).await?.get(0);
    client
        .execute(
            "UPDATE app.asset_hold SET state='released' WHERE id=$1",
            &[&hold],
        )
        .await?;
    mark_deleted(&client, owner).await?;
    let asset = definition(&client).await?;
    assert_eq!(
        client
            .query_one("SELECT app.ensure_inventory($1,$2)", &[&owner, &asset])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    client
        .execute(
            "UPDATE app.asset_owner SET state='retired' WHERE id=$1",
            &[&owner],
        )
        .await?;
    let source = operation(&client, owner).await?;
    for direction in ["credit", "debit"] {
        assert_eq!(client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,$3,1,'test',$1::uuid::text)",&[&source,&inventory,&direction]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    }
    assert_eq!(client.execute("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,'ante',$2::uuid::text,'held',$2)",&[&owner,&source]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        client
            .execute(
                "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,1)",
                &[&hold, &inventory]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        client
            .execute(
                "UPDATE app.asset_hold SET state='released' WHERE id=$1",
                &[&hold]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        client
            .execute(
                "UPDATE app.inventory_asset SET revision=revision+1 WHERE id=$1",
                &[&inventory]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        client
            .query_one(
                "SELECT owned_quantity::text FROM app.inventory_asset WHERE id=$1",
                &[&inventory]
            )
            .await?
            .get::<_, String>(0),
        "0"
    );
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.ledger_posting WHERE operation_id=$1",
                &[&source]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn asset_target_identity_and_cross_owner_hold_lines_reject_before_lower_locks() -> TestResult
{
    let client = connection().await?;
    let (owner, _, inventory) = fixture(&client).await?;
    let (peer, _, peer_inventory) = fixture(&client).await?;
    let source = operation(&client, owner).await?;
    let hold:Uuid=client.query_one("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,'ante',$2::uuid::text,'held',$2) RETURNING id",&[&owner,&source]).await?.get(0);
    assert_eq!(
        client
            .execute(
                "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,1)",
                &[&hold, &peer_inventory]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    client
        .execute(
            "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,1)",
            &[&hold, &inventory],
        )
        .await?;
    for (sql, id, target) in [
        (
            "UPDATE app.inventory_asset SET id=$2 WHERE id=$1",
            inventory,
            Uuid::new_v4(),
        ),
        (
            "UPDATE app.inventory_asset SET owner_id=$2 WHERE id=$1",
            inventory,
            peer,
        ),
        (
            "UPDATE app.asset_hold SET owner_id=$2 WHERE id=$1",
            hold,
            peer,
        ),
        (
            "UPDATE app.asset_hold SET operation_id=$2 WHERE id=$1",
            hold,
            source,
        ),
    ] {
        // operation_id must actually change to exercise its immutable target.
        let target = if sql.contains("operation_id") {
            Uuid::new_v4()
        } else {
            target
        };
        assert_eq!(
            client
                .execute(sql, &[&id, &target])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    assert_eq!(
        client
            .execute(
                "UPDATE app.hold_line SET inventory_asset_id=$2 WHERE hold_id=$1",
                &[&hold, &peer_inventory]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        client
            .query_one(
                "SELECT app.ensure_inventory($1,$2)",
                &[&Uuid::new_v4(), &Uuid::new_v4()]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn late_inventory_creation_waits_for_account_deletion_then_rejects_without_effect()
-> TestResult {
    let observer = connection().await?;
    let (owner, _) = account(&observer).await?;
    let asset = definition(&observer).await?;
    let mut finalizing = connection().await?;
    let tx = finalizing.transaction().await?;
    let blocker: i32 = tx.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
    mark_deleted(&tx, owner).await?;
    let name = format!("late-asset-lifecycle-{}", Uuid::new_v4());
    let child_name = name.clone();
    let (ready, started) = tokio::sync::oneshot::channel();
    let child = tokio::spawn(async move {
        let client = connection().await?;
        client
            .query_one(
                "SELECT set_config('application_name',$1,false)",
                &[&child_name],
            )
            .await?;
        ready
            .send(())
            .map_err(|_| "lifecycle observer disappeared")?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
            client
                .query_one("SELECT app.ensure_inventory($1,$2)", &[&owner, &asset])
                .await
                .unwrap_err()
                .code()
                .cloned(),
        )
    });
    started.await?;
    wait_on(&observer, &name, blocker, true).await?;
    tx.commit().await?;
    assert_eq!(child.await??, Some(SqlState::CHECK_VIOLATION));
    assert_eq!(observer.query_one("SELECT count(*) FROM app.inventory_asset WHERE owner_id=$1 AND asset_definition_id=$2",&[&owner,&asset]).await?.get::<_,i64>(0),0);
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stale_repeatable_read_cannot_create_inventory_after_lifecycle_change() -> TestResult {
    let observer = connection().await?;
    let (owner, _) = account(&observer).await?;
    let asset = definition(&observer).await?;
    let mut old = connection().await?;
    let tx = old.transaction().await?;
    tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .await?;
    tx.query_one("SELECT state FROM app.account WHERE id=$1", &[&owner])
        .await?;
    mark_deleted(&observer, owner).await?;
    assert_eq!(
        tx.query_one("SELECT app.ensure_inventory($1,$2)", &[&owner, &asset])
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::T_R_SERIALIZATION_FAILURE)
    );
    tx.rollback().await?;
    assert_eq!(observer.query_one("SELECT count(*) FROM app.inventory_asset WHERE owner_id=$1 AND asset_definition_id=$2",&[&owner,&asset]).await?.get::<_,i64>(0),0);
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum AssetWrite {
    Credit,
    Debit,
    NewHold,
    HoldLine,
    ReleaseHold,
    InventoryRevision,
}

const ASSET_WRITES: [AssetWrite; 6] = [
    AssetWrite::Credit,
    AssetWrite::Debit,
    AssetWrite::NewHold,
    AssetWrite::HoldLine,
    AssetWrite::ReleaseHold,
    AssetWrite::InventoryRevision,
];

struct WriteFixture {
    owner: Uuid,
    inventory: Uuid,
    operation: Uuid,
    hold: Uuid,
}

impl WriteFixture {
    async fn create(client: &Client) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (owner, _, inventory) = fixture(client).await?;
        let source = operation(client, owner).await?;
        let hold_source = operation(client, owner).await?;
        let hold = client.query_one("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,'ante',$2::uuid::text,'held',$2) RETURNING id", &[&owner,&hold_source]).await?.get(0);
        Ok(Self {
            owner,
            inventory,
            operation: source,
            hold,
        })
    }

    async fn mutate<C: GenericClient + Sync>(
        &self,
        client: &C,
        kind: AssetWrite,
    ) -> Result<u64, tokio_postgres::Error> {
        match kind {
            AssetWrite::Credit | AssetWrite::Debit => {
                let direction = if matches!(kind, AssetWrite::Credit) { "credit" } else { "debit" };
                client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,$3,1,'test',$1::uuid::text)", &[&self.operation,&self.inventory,&direction]).await
            }
            AssetWrite::NewHold => client.execute("INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,'ante',$2::uuid::text,'held',$2)", &[&self.owner,&self.operation]).await,
            AssetWrite::HoldLine => client.execute("INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,1)", &[&self.hold,&self.inventory]).await,
            AssetWrite::ReleaseHold => client.execute("UPDATE app.asset_hold SET state='released' WHERE id=$1", &[&self.hold]).await,
            AssetWrite::InventoryRevision => client.execute("UPDATE app.inventory_asset SET revision=revision+1 WHERE id=$1", &[&self.inventory]).await,
        }
    }

    async fn snapshot(&self, client: &Client) -> Result<serde_json::Value, tokio_postgres::Error> {
        Ok(client.query_one("SELECT jsonb_build_object('inventory',(SELECT to_jsonb(i) FROM app.inventory_asset i WHERE id=$1),'holds',(SELECT jsonb_agg(to_jsonb(h) ORDER BY h.id) FROM app.asset_hold h WHERE owner_id=$2),'lines',(SELECT jsonb_agg(to_jsonb(l) ORDER BY l.id) FROM app.hold_line l JOIN app.asset_hold h ON h.id=l.hold_id WHERE h.owner_id=$2),'postings',(SELECT jsonb_agg(to_jsonb(p) ORDER BY p.id) FROM app.ledger_posting p WHERE inventory_asset_id=$1))", &[&self.inventory,&self.owner]).await?.get(0))
    }
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn pending_accounts_allow_committed_asset_work_through_every_guard() -> TestResult {
    let client = connection().await?;
    for kind in ASSET_WRITES {
        let setup = WriteFixture::create(&client).await?;
        client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=statement_timestamp() WHERE id=$1", &[&setup.owner]).await?;
        assert_eq!(setup.mutate(&client, kind).await?, 1, "{kind:?}");
        let fresh = definition(&client).await?;
        client
            .query_one(
                "SELECT app.ensure_inventory($1,$2)",
                &[&setup.owner, &fresh],
            )
            .await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn late_posting_and_hold_writes_wait_for_deletion_and_leave_no_effect() -> TestResult {
    let observer = connection().await?;
    for kind in ASSET_WRITES {
        let setup = WriteFixture::create(&observer).await?;
        let before = setup.snapshot(&observer).await?;
        let mut finalizing = connection().await?;
        let tx = finalizing.transaction().await?;
        let blocker: i32 = tx.query_one("SELECT pg_backend_pid()", &[]).await?.get(0);
        mark_deleted(&tx, setup.owner).await?;
        let name = format!("late-asset-write-{}", Uuid::new_v4());
        let child_name = name.clone();
        let (ready, started) = tokio::sync::oneshot::channel();
        let child = tokio::spawn(async move {
            let client = connection().await?;
            client
                .query_one(
                    "SELECT set_config('application_name',$1,false)",
                    &[&child_name],
                )
                .await?;
            ready
                .send(())
                .map_err(|_| "lifecycle observer disappeared")?;
            let error = setup.mutate(&client, kind).await.unwrap_err();
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((error.code().cloned(), setup))
        });
        started.await?;
        wait_on(&observer, &name, blocker, true).await?;
        tx.commit().await?;
        let (code, setup) = child.await??;
        assert_eq!(code, Some(SqlState::CHECK_VIOLATION), "{kind:?}");
        assert_eq!(setup.snapshot(&observer).await?, before, "{kind:?}");
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stale_repeatable_read_posting_and_hold_writes_serialize_without_effect() -> TestResult {
    let observer = connection().await?;
    for kind in ASSET_WRITES {
        let setup = WriteFixture::create(&observer).await?;
        let before = setup.snapshot(&observer).await?;
        let mut old = connection().await?;
        let tx = old.transaction().await?;
        tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .await?;
        tx.query_one("SELECT state FROM app.account WHERE id=$1", &[&setup.owner])
            .await?;
        mark_deleted(&observer, setup.owner).await?;
        assert_eq!(
            setup.mutate(&tx, kind).await.unwrap_err().code(),
            Some(&SqlState::T_R_SERIALIZATION_FAILURE),
            "{kind:?}"
        );
        tx.rollback().await?;
        assert_eq!(setup.snapshot(&observer).await?, before, "{kind:?}");
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn owner_retirement_directly_rejects_missing_and_wrong_source_proofs() -> TestResult {
    let client = connection().await?;
    for defect in ["absent", "pending", "kind", "scope", "reference", "actor"] {
        let (owner, _) = account(&client).await?;
        let (peer, _) = account(&client).await?;
        let source = if defect == "absent" {
            None
        } else {
            let source = Uuid::new_v4();
            let kind = if defect == "kind" {
                "test"
            } else {
                "retirement"
            };
            let scope = if defect == "scope" {
                "test"
            } else {
                "account_retirement"
            };
            let reference = if defect == "reference" { peer } else { owner }.to_string();
            let actor = if defect == "actor" { peer } else { owner };
            let state = if defect == "pending" {
                "pending"
            } else {
                "committed"
            };
            client.execute("INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES($1,$2,$3,$4,$5,statement_timestamp(),$6)", &[&source,&kind,&reference,&scope,&actor,&state]).await?;
            Some(source)
        };
        let deleted = client.execute("UPDATE app.account SET state='deleted',deletion_requested_at=statement_timestamp(),deletion_finalized_at=statement_timestamp(),retirement_operation_id=$2 WHERE id=$1", &[&owner,&source]).await;
        // A missing operation is already impossible under the account CHECK.
        // Keep that earlier guard intact instead of weakening fixture schema.
        if defect == "absent" {
            assert_eq!(
                deleted.unwrap_err().code(),
                Some(&SqlState::CHECK_VIOLATION)
            );
            continue;
        }
        deleted?;
        assert_eq!(
            client
                .execute(
                    "UPDATE app.asset_owner SET state='retired' WHERE id=$1",
                    &[&owner]
                )
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION),
            "{defect}"
        );
        assert_eq!(
            client
                .query_one("SELECT state FROM app.asset_owner WHERE id=$1", &[&owner])
                .await?
                .get::<_, String>(0),
            "active"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn owner_identity_and_initial_lifecycle_cannot_be_rewritten() -> TestResult {
    let client = connection().await?;
    let (owner, _) = account(&client).await?;
    let (peer, _) = account(&client).await?;
    for query in [
        "UPDATE app.asset_owner SET id=$2 WHERE id=$1",
        "UPDATE app.asset_owner SET account_id=$2 WHERE id=$1",
    ] {
        assert_eq!(
            client
                .execute(query, &[&owner, &peer])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    for state in ["active", "unknown", "retired"] {
        assert_eq!(
            client
                .execute(
                    "UPDATE app.asset_owner SET state=$2 WHERE id=$1",
                    &[&owner, &state]
                )
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    for state in ["unknown", "retired"] {
        assert_eq!(
            client
                .execute(
                    "INSERT INTO app.asset_owner(account_id,state) VALUES($1,$2)",
                    &[&owner, &state]
                )
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    assert_eq!(
        client
            .execute(
                "INSERT INTO app.asset_owner(account_id,state) VALUES($1,'active')",
                &[&Uuid::new_v4()]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        client
            .query_one(
                "SELECT account_id,state FROM app.asset_owner WHERE id=$1",
                &[&owner]
            )
            .await?
            .get::<_, Uuid>(0),
        owner
    );
    Ok(())
}
