//! Session-bound economy bridge over synthetic authority and actual guarded writers.
use super::{TestResult, connection, pricing::buyer, social::friend};
use serde_json::{Value, json};
use tokio_postgres::{Client, GenericClient};
use uuid::Uuid;

/// Fixture-owner credential lookup only; the actual command executes under the original role.
pub(super) async fn execute_for_actor<C: GenericClient + Sync>(
    client: &C,
    actor: Uuid,
    input: &Value,
    lookup: bool,
) -> Result<Value, tokio_postgres::Error> {
    Ok(execute_row_for_actor(client, actor, input, lookup)
        .await?
        .get(0))
}

pub(super) async fn execute_row_for_actor<C: GenericClient + Sync>(
    client: &C,
    actor: Uuid,
    input: &Value,
    lookup: bool,
) -> Result<tokio_postgres::Row, tokio_postgres::Error> {
    let role: String = client
        .query_one("SELECT current_user::text", &[])
        .await?
        .get(0);
    assert!(matches!(
        role.as_str(),
        "postgres" | "service_role" | "anon" | "authenticated"
    ));
    client.batch_execute("RESET ROLE").await?;
    let source=client.query_one("SELECT a.auth_subject,(test_fixture.claims_for_subject(a.auth_subject)->>'session_id')::uuid FROM app.account a WHERE id=$1", &[&actor]).await;
    client.batch_execute(&format!("SET ROLE {role}")).await?;
    let source = source?;
    call_row(client, source.get(0), source.get(1), input, lookup).await
}
async fn call<C: GenericClient + Sync>(
    client: &C,
    subject: Uuid,
    session: Uuid,
    input: &Value,
    lookup: bool,
) -> Result<Value, tokio_postgres::Error> {
    Ok(call_row(client, subject, session, input, lookup)
        .await?
        .get(0))
}
async fn call_row<C: GenericClient + Sync>(
    client: &C,
    subject: Uuid,
    session: Uuid,
    input: &Value,
    lookup: bool,
) -> Result<tokio_postgres::Row, tokio_postgres::Error> {
    client
        .query_one(
            if lookup {
                "SELECT app.read_economy_command($1,$2,$3)"
            } else {
                "SELECT app.execute_economy_command($1,$2,$3)"
            },
            &[&subject, &session, input],
        )
        .await
}
async fn coin(client: &Client) -> Result<Uuid, tokio_postgres::Error> {
    Ok(client
        .query_one(
            "SELECT id FROM app.asset_definition WHERE currency_key='coin'",
            &[],
        )
        .await?
        .get(0))
}
fn trade(peer: Uuid, coin: Uuid) -> Value {
    json!({"kind":"trade_create","command":{"requestId":Uuid::new_v4(),"recipientId":peer,"give":[{"assetId":coin,"quantity":"100"}],"request":[{"assetId":coin,"quantity":"100"}]}})
}
fn resource(value: &Value) -> Uuid {
    assert_eq!(value["status"], "success", "{value}");
    value["data"]["resourceId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn economy_session_trade_exact_recovery_and_pending_cleanup() -> TestResult {
    let client = connection().await?;
    let (left, _) = buyer(&client).await?;
    let (right, _) = buyer(&client).await?;
    friend(&client, left, right).await?;
    let input = trade(right, coin(&client).await?);
    client.batch_execute("SET ROLE service_role").await?;
    assert_eq!(
        execute_for_actor(&client, left, &input, true).await?,
        json!({"status":"success","data":{"state":"unknown","response":null}})
    );
    let ack = execute_for_actor(&client, left, &input, false).await?;
    let offer = resource(&ack);
    assert_eq!(execute_for_actor(&client, left, &input, false).await?, ack);
    assert_eq!(
        execute_for_actor(&client, left, &input, true).await?["data"]["response"],
        ack
    );
    let mut changed = input.clone();
    changed["command"]["give"][0]["quantity"] = json!("101");
    for lookup in [false, true] {
        assert_eq!(
            execute_for_actor(&client, left, &changed, lookup).await?,
            json!({"status":"fail","data":{"request":"conflict"}})
        );
    }
    super::deletion::request(&client, left, Uuid::new_v4()).await?;
    let cancel = json!({"kind":"trade_response","command":{"requestId":Uuid::new_v4(),"tradeId":offer,"action":"cancel"}});
    resource(&execute_for_actor(&client, left, &cancel, false).await?);
    assert_eq!(
        execute_for_actor(&client, left, &cancel, true).await?["data"]["state"],
        "accepted"
    );
    client.batch_execute("RESET ROLE").await?;
    assert_eq!(client.query_one("SELECT h.state::text FROM app.trade_offer t JOIN app.asset_hold h ON h.id=t.sender_hold_id WHERE t.id=$1", &[&offer]).await?.get::<_,String>(0),"released");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn economy_parser_preserves_canonical_receipt_terms_and_rejects_injection() -> TestResult {
    let client = connection().await?;
    let target = Uuid::new_v4();
    let request = Uuid::new_v4();
    let policy = Uuid::new_v4();
    let cases = [
        (
            "trade_create",
            json!({"requestId":request,"recipientId":target,"give":[],"request":[{"assetId":target,"quantity":"9"}]}),
            "trade_create",
            json!({"give":[],"request":[{"assetId":target,"quantity":"9"}]}),
        ),
        (
            "trade_response",
            json!({"requestId":request,"tradeId":target,"action":"accept"}),
            "trade_response",
            json!({"action":"accept"}),
        ),
        (
            "listing_create",
            json!({"requestId":request,"lot":{"assetId":target,"quantity":"2"},"totalPrice":"9007199254740993"}),
            "market_create",
            json!({"quantity":"2","price":"9007199254740993"}),
        ),
        (
            "listing_cancel",
            json!({"requestId":request,"listingId":target}),
            "market_cancel",
            json!({}),
        ),
        (
            "listing_buy",
            json!({"requestId":request,"listingId":target,"expectedRevision":"1","expectedTotal":"9"}),
            "market_buy",
            json!({"revision":"1","price":"9"}),
        ),
        (
            "pack_buy",
            json!({"requestId":request,"recipeId":target,"quantity":"2","expectedPolicyId":policy,"expectedRevision":"0","expectedTotal":"9"}),
            "pack_purchase",
            json!({"quantity":2,"policy":policy,"revision":"0","total":"9"}),
        ),
        (
            "pack_open",
            json!({"requestId":request,"inventoryAssetId":target}),
            "pack_open",
            json!({"inventoryAssetId":target}),
        ),
    ];
    for (kind, command, scope, terms) in cases {
        let input = json!({"kind":kind,"command":command});
        let parsed: Value = client
            .query_one("SELECT app.economy_command_terms($1)", &[&input])
            .await?
            .get(0);
        assert_eq!(
            parsed,
            json!({"requestId":request,"targetId":target,"scope":scope,"terms":terms})
        );
        for field in ["actorId", "estimatedValue", "seed"] {
            let mut bad = input.clone();
            bad["command"][field] = json!(target);
            assert!(
                client
                    .query_one("SELECT app.economy_command_terms($1) IS NULL", &[&bad])
                    .await?
                    .get::<_, bool>(0)
            );
        }
        let mut nil = input;
        nil["command"]["requestId"] = json!(Uuid::nil());
        assert!(
            client
                .query_one("SELECT app.economy_command_terms($1) IS NULL", &[&nil])
                .await?
                .get::<_, bool>(0)
        );
    }
    for bad in [
        json!(null),
        json!([]),
        json!({"kind":"trade_create","command":{}}),
    ] {
        assert!(
            client
                .query_one("SELECT app.economy_command_terms($1) IS NULL", &[&bad])
                .await?
                .get::<_, bool>(0)
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn economy_private_writer_acl_and_isolation_are_explicit() -> TestResult {
    let mut client = connection().await?;
    let (actor, _) = buyer(&client).await?;
    let private = [
        "app.create_trade(uuid,uuid,jsonb,jsonb,uuid)",
        "app.respond_trade(uuid,uuid,text,uuid)",
        "app.create_listing(uuid,uuid,text,text,uuid)",
        "app.cancel_listing(uuid,uuid,uuid)",
        "app.buy_listing(uuid,uuid,text,text,uuid)",
        "app.buy_packs(uuid,uuid,integer,uuid,text,text,uuid)",
        "app.open_pack(uuid,uuid,uuid)",
        "app.economy_command_terms(jsonb)",
    ];
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for function in private {
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
        for function in [
            "app.execute_economy_command(uuid,uuid,jsonb)",
            "app.read_economy_command(uuid,uuid,jsonb)",
        ] {
            assert_eq!(
                client
                    .query_one(
                        "SELECT has_function_privilege($1,$2,'EXECUTE')",
                        &[&role, &function]
                    )
                    .await?
                    .get::<_, bool>(0),
                role == "service_role"
            );
        }
    }
    let input = trade(Uuid::new_v4(), coin(&client).await?);
    for lookup in [false, true] {
        let transaction = client
            .build_transaction()
            .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
            .start()
            .await?;
        let error = execute_for_actor(&transaction, actor, &input, lookup)
            .await
            .expect_err("stale authority snapshots must retry");
        assert_eq!(
            error.code(),
            Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
        );
        transaction.rollback().await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn economy_rechecks_after_inventory_wait_and_rolls_back_receipt_and_hold() -> TestResult {
    let locker = connection().await?;
    let (actor, subject) = buyer(&locker).await?;
    let (peer, _) = buyer(&locker).await?;
    friend(&locker, actor, peer).await?;
    let asset = coin(&locker).await?;
    let input = trade(peer, asset);
    let session: Uuid = locker
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    let request = input["command"]["requestId"].as_str().unwrap().to_owned();
    let waiter = connection().await?;
    let pid: i32 = waiter
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    waiter.batch_execute("SET ROLE service_role").await?;
    locker.batch_execute("BEGIN").await?;
    locker.query_one("SELECT id FROM app.inventory_asset WHERE owner_id=$1 AND asset_definition_id=$2 FOR NO KEY UPDATE", &[&actor,&asset]).await?;
    let task = tokio::spawn(async move { call(&waiter, subject, session, &input, false).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if locker
                .query_one("SELECT cardinality(pg_blocking_pids($1))>0", &[&pid])
                .await
                .unwrap()
                .get::<_, bool>(0)
            {
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
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(3), task).await???,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    assert_eq!(locker.query_one("SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND scope='trade_create' AND request_ref=$2", &[&actor,&request]).await?.get::<_,i64>(0),0);
    assert_eq!(
        locker
            .query_one(
                "SELECT count(*) FROM app.trade_offer WHERE sender_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    Ok(())
}
