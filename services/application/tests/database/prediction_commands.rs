//! Live-session facade over actual synthetic prediction matching and holds.
use super::{TestResult, connection, pricing::buyer};
use serde_json::{Value, json};
use tokio_postgres::Client;
use uuid::Uuid;

/// Test-only canonical subject/session lookup; execute using the service-only session facade.
pub(super) async fn execute_for_actor(
    client: &Client,
    actor: Uuid,
    input: &Value,
    lookup: bool,
) -> Result<Value, tokio_postgres::Error> {
    let role: String = client
        .query_one("SELECT current_user::text", &[])
        .await?
        .get(0);
    assert!(matches!(
        role.as_str(),
        "postgres" | "service_role" | "authenticated" | "anon"
    ));
    client.batch_execute("RESET ROLE").await?;
    let source=client.query_one("SELECT a.auth_subject,(test_fixture.claims_for_subject(a.auth_subject)->>'session_id')::uuid FROM app.account a WHERE id=$1",&[&actor]).await;
    client.batch_execute("SET ROLE service_role").await?;
    let source = source?;
    let subject: Uuid = source.get(0);
    let session: Uuid = source.get(1);
    let result = client
        .query_one(
            if lookup {
                "SELECT app.read_prediction_command($1,$2,$3)"
            } else {
                "SELECT app.execute_prediction_command($1,$2,$3)"
            },
            &[&subject, &session, input],
        )
        .await;
    client.batch_execute("RESET ROLE").await?;
    client.batch_execute(&format!("SET ROLE {role}")).await?;
    Ok(result?.get(0))
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn prediction_session_receipts_match_backing_and_cancel_only_unfilled() -> TestResult {
    let client = connection().await?;
    let (alice, _) = buyer(&client).await?;
    let (bob, _) = buyer(&client).await?;
    let market = super::predictions::market(&client, alice, 3600).await?;
    let input = json!({"kind":"place_order","command":{"requestId":Uuid::new_v4(),"marketId":market,"side":"yes","limitCost":"60","quantity":"3"}});
    assert_eq!(
        execute_for_actor(&client, alice, &input, true).await?["data"]["state"],
        "unknown"
    );
    let first = execute_for_actor(&client, alice, &input, false).await?;
    assert_eq!(first["status"], "success");
    let order: Uuid = first["data"]["resourceId"].as_str().unwrap().parse()?;
    assert_eq!(
        execute_for_actor(&client, alice, &input, false).await?,
        first
    );
    assert_eq!(
        execute_for_actor(&client, alice, &input, true).await?["data"]["state"],
        "accepted"
    );
    let opposite = json!({"kind":"place_order","command":{"requestId":Uuid::new_v4(),"marketId":market,"side":"no","limitCost":"40","quantity":"1"}});
    assert_eq!(
        execute_for_actor(&client, bob, &opposite, false).await?["status"],
        "success"
    );
    let cancel =
        json!({"kind":"cancel_order","command":{"requestId":Uuid::new_v4(),"orderId":order}});
    assert_eq!(
        execute_for_actor(&client, alice, &cancel, false).await?["status"],
        "success"
    );
    let row=client.query_one("SELECT o.state,o.filled_quantity::text,h.quantity::text FROM app.prediction_order o JOIN app.asset_hold a ON a.id=o.hold_id JOIN app.hold_line h ON h.hold_id=a.id WHERE o.id=$1",&[&order]).await?;
    assert_eq!(row.get::<_, String>(0), "canceled");
    assert_eq!(row.get::<_, String>(1), "1");
    assert_eq!(row.get::<_, String>(2), "60");
    let mut changed = input.clone();
    changed["command"]["quantity"] = json!("4");
    assert_eq!(
        execute_for_actor(&client, alice, &changed, true).await?["data"]["request"],
        "conflict"
    );
    let foreign =
        json!({"kind":"cancel_order","command":{"requestId":Uuid::new_v4(),"orderId":order}});
    assert_eq!(
        execute_for_actor(&client, bob, &foreign, false).await?["data"]["order"],
        "prediction order unavailable"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn prediction_session_parser_and_private_capabilities_are_explicit() -> TestResult {
    let client = connection().await?;
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for signature in [
            "app.execute_prediction_command(uuid,uuid,jsonb)",
            "app.read_prediction_command(uuid,uuid,jsonb)",
            "app.prediction_command_terms(jsonb)",
            "app.read_session_command(uuid,uuid,jsonb)",
            "app.place_prediction_order(uuid,uuid,text,text,text,uuid)",
            "app.cancel_prediction_order(uuid,uuid,uuid)",
        ] {
            let allowed: bool = client
                .query_one(
                    "SELECT has_function_privilege($1,$2,'EXECUTE')",
                    &[&role, &signature],
                )
                .await?
                .get(0);
            assert_eq!(
                allowed,
                role == "service_role"
                    && (signature.starts_with("app.execute_prediction_command")
                        || signature.starts_with("app.read_prediction_command")),
                "{role}:{signature}"
            );
        }
    }
    let base = json!({"kind":"place_order","command":{"requestId":Uuid::new_v4(),"marketId":Uuid::new_v4(),"side":"yes","limitCost":"60","quantity":"1"}});
    let parsed: Value = client
        .query_one("SELECT app.prediction_command_terms($1)", &[&base])
        .await?
        .get(0);
    assert_eq!(parsed["scope"], "prediction_order");
    for (key, value) in [
        ("actorId", json!(Uuid::new_v4())),
        ("quantity", json!(1)),
        ("limitCost", json!("100")),
        ("quantity", json!("9".repeat(78))),
    ] {
        let mut bad = base.clone();
        bad["command"][key] = value;
        let parsed: Option<Value> = client
            .query_one("SELECT app.prediction_command_terms($1)", &[&bad])
            .await?
            .get(0);
        assert!(parsed.is_none());
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn prediction_session_revocation_after_inventory_wait_rolls_back_order_and_receipt()
-> TestResult {
    let locker = connection().await?;
    let (actor, subject) = buyer(&locker).await?;
    let market = super::predictions::market(&locker, actor, 3600).await?;
    let request = Uuid::new_v4();
    let input = json!({"kind":"place_order","command":{"requestId":request,"marketId":market,"side":"yes","limitCost":"60","quantity":"1"}});
    let session: Uuid = locker
        .query_one(
            "SELECT (test_fixture.claims_for_subject($1)->>'session_id')::uuid",
            &[&subject],
        )
        .await?
        .get(0);
    let waiter = connection().await?;
    let pid: i32 = waiter
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    waiter.batch_execute("SET ROLE service_role").await?;
    locker.batch_execute("BEGIN").await?;
    locker
        .query_one(
            "SELECT id FROM app.inventory_asset WHERE owner_id=$1 FOR NO KEY UPDATE",
            &[&actor],
        )
        .await?;
    let task = tokio::spawn(async move {
        Ok::<Value, tokio_postgres::Error>(
            waiter
                .query_one(
                    "SELECT app.execute_prediction_command($1,$2,$3)",
                    &[&subject, &session, &input],
                )
                .await?
                .get(0),
        )
    });
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
    assert_eq!(
        locker
            .query_one(
                "SELECT count(*) FROM app.prediction_order WHERE account_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    assert_eq!(
        locker
            .query_one(
                "SELECT count(*) FROM app.command_receipt WHERE account_id=$1 AND request_ref=$2",
                &[&actor, &request.to_string()]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    assert_eq!(locker.query_one("SELECT available::text FROM app.asset_availability WHERE inventory_asset_id IN (SELECT id FROM app.inventory_asset WHERE owner_id=$1)",&[&actor]).await?.get::<_,String>(0),"14500");
    Ok(())
}
