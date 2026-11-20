//! Actual caller projections over guarded synthetic economy fixtures.
use super::{
    TestResult, connection, economy_commands::execute_for_actor, pricing::buyer, set_auth_claims,
    social::friend,
};
use serde_json::{Value, json};
use tokio_postgres::{Client, types::ToSql};
use uuid::Uuid;

async fn read(
    client: &Client,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<Value, tokio_postgres::Error> {
    Ok(client.query_one(sql, params).await?.get(0))
}
fn resource(value: Value) -> Uuid {
    assert_eq!(value["status"], "success", "{value}");
    value["data"]["resourceId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}
fn data(value: Value) -> Value {
    assert_eq!(value["status"], "success", "{value}");
    value["data"].clone()
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn trade_reads_are_participant_only_paged_and_anonymous_after_retirement() -> TestResult {
    let client = connection().await?;
    let (left, left_subject) = buyer(&client).await?;
    let (right, right_subject) = buyer(&client).await?;
    let (_, outsider) = buyer(&client).await?;
    friend(&client, left, right).await?;
    let a = resource(super::trades::offer(&client, left, right, 100, 110, Uuid::new_v4()).await?);
    let b = resource(super::trades::offer(&client, left, right, 100, 110, Uuid::new_v4()).await?);
    let old_name: String = client
        .query_one("SELECT username FROM app.account WHERE id=$1", &[&left])
        .await?
        .get(0);
    set_auth_claims(&client, right_subject).await?;
    let value = data(read(&client, "SELECT app.read_my_trade($1)", &[&a]).await?);
    serde_json::from_value::<spellwake_application::exchange_reads::OwnedTrade>(value.clone())?
        .validate(a)?;
    assert_eq!(value["accountId"], right.to_string());
    assert_eq!(value["trade"]["sender"]["username"], old_name);
    assert_eq!(value["trade"]["give"][0]["quantity"], "100");
    assert_eq!(value["trade"]["give"][0]["asset"]["kind"], "coin");
    assert!(value["trade"]["settlement"].is_null());
    assert!(value["trade"].get("senderHoldId").is_none());
    for field in ["sender_hold_id", "settlement_operation_id", "assessment"] {
        assert!(
            client
                .query_one(
                    &format!("SELECT {field} FROM app.trade_offer WHERE id=$1"),
                    &[&a]
                )
                .await
                .is_err()
        );
    }
    assert_eq!(
        client
            .query_one("SELECT id FROM app.trade_offer WHERE id=$1", &[&a])
            .await?
            .get::<_, Uuid>(0),
        a
    );
    let page = data(read(&client, "SELECT app.read_my_trades(NULL,1,'offered')", &[]).await?);
    serde_json::from_value::<spellwake_application::exchange_reads::TradePage>(page.clone())?
        .validate(
            None,
            1,
            Some(spellwake_application::exchange_reads::TradeState::Offered),
        )?;
    let after: Uuid = page["nextCursor"].as_str().unwrap().parse()?;
    let next = data(
        read(
            &client,
            "SELECT app.read_my_trades($1,1,'offered')",
            &[&after],
        )
        .await?,
    );
    assert_ne!(page["items"][0]["id"], next["items"][0]["id"]);
    set_auth_claims(&client, outsider).await?;
    let missing = read(&client, "SELECT app.read_my_trade($1)", &[&Uuid::new_v4()]).await?;
    assert_eq!(
        read(&client, "SELECT app.read_my_trade($1)", &[&a]).await?,
        missing
    );
    assert!(
        data(read(&client, "SELECT app.read_my_trades(NULL,8,NULL)", &[]).await?)["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    client.batch_execute("RESET ROLE").await?;
    resource(execute_for_actor(&client,right,&json!({"kind":"trade_response","command":{"requestId":Uuid::new_v4(),"tradeId":a,"action":"accept"}}),false).await?);
    resource(execute_for_actor(&client,left,&json!({"kind":"trade_response","command":{"requestId":Uuid::new_v4(),"tradeId":b,"action":"cancel"}}),false).await?);
    set_auth_claims(&client, right_subject).await?;
    let accepted = data(read(&client, "SELECT app.read_my_trade($1)", &[&a]).await?);
    serde_json::from_value::<spellwake_application::exchange_reads::OwnedTrade>(accepted.clone())?
        .validate(a)?;
    assert_eq!(
        accepted["trade"]["settlement"]["outgoing"]["total"],
        json!({"numerator":"100","denominator":"1"})
    );
    client.batch_execute("RESET ROLE").await?;
    super::deletion::request(&client, left, Uuid::new_v4()).await?;
    let retired = read(&client, "SELECT app.retire_account($1)", &[&left]).await?;
    assert_eq!(retired["data"]["state"], "retired");
    set_auth_claims(&client, right_subject).await?;
    let anonymous = data(read(&client, "SELECT app.read_my_trade($1)", &[&a]).await?);
    serde_json::from_value::<spellwake_application::exchange_reads::OwnedTrade>(anonymous.clone())?
        .validate(a)?;
    assert_eq!(anonymous["trade"]["sender"]["username"], "Deleted player");
    assert_eq!(anonymous["trade"]["sender"]["isDeleted"], true);
    assert!(!anonymous.to_string().contains(&old_name));
    assert_eq!(
        anonymous["trade"]["settlement"],
        accepted["trade"]["settlement"]
    );
    // The retired subject is not needed to read another participant's retained history.
    assert_ne!(left_subject, right_subject);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn market_reads_preserve_public_active_and_private_terminal_visibility() -> TestResult {
    let client = connection().await?;
    let (seller, seller_subject, asset) = super::market::stocked_seller(&client).await?;
    let (buyer, buyer_subject) = buyer(&client).await?;
    let (_, outsider) = super::account(&client).await?;
    let listing =
        resource(super::market::list(&client, seller, asset, "1", "1500", Uuid::new_v4()).await?);
    set_auth_claims(&client, outsider).await?;
    let public = data(
        read(
            &client,
            "SELECT app.read_my_market_listing($1)",
            &[&listing],
        )
        .await?,
    );
    serde_json::from_value::<spellwake_application::exchange_reads::OwnedListing>(public.clone())?
        .validate(listing)?;
    assert_eq!(public["listing"]["state"], "listed");
    assert!(public["listing"]["participant"].is_null());
    assert_eq!(public["listing"]["asset"]["kind"], "pack");
    assert!(public["listing"].get("holdId").is_none());
    assert!(public["listing"].get("assessment").is_none());
    set_auth_claims(&client, seller_subject).await?;
    let own = data(read(&client, "SELECT app.read_my_market(NULL,8,true)", &[]).await?);
    serde_json::from_value::<spellwake_application::exchange_reads::ListingPage>(own.clone())?
        .validate(None, 8, true)?;
    assert_eq!(own["items"].as_array().unwrap().len(), 1);
    assert!(own["items"][0]["participant"].is_object());
    client.batch_execute("RESET ROLE").await?;
    resource(super::market::buy(&client, buyer, listing, "1500", Uuid::new_v4()).await?);
    set_auth_claims(&client, outsider).await?;
    assert_eq!(
        read(
            &client,
            "SELECT app.read_my_market_listing($1)",
            &[&listing]
        )
        .await?,
        read(
            &client,
            "SELECT app.read_my_market_listing($1)",
            &[&Uuid::new_v4()]
        )
        .await?
    );
    set_auth_claims(&client, buyer_subject).await?;
    let sold = data(
        read(
            &client,
            "SELECT app.read_my_market_listing($1)",
            &[&listing],
        )
        .await?,
    );
    serde_json::from_value::<spellwake_application::exchange_reads::OwnedListing>(sold.clone())?
        .validate(listing)?;
    assert_eq!(
        sold["listing"]["participant"]["buyer"]["id"],
        buyer.to_string()
    );
    assert_eq!(sold["listing"]["participant"]["paidCoins"], "1500");
    assert!(sold["listing"]["participant"]["settlement"]["outgoing"]["policyId"].is_string());
    assert_eq!(
        sold["listing"]["participant"]["settlement"]["outgoing"]["catalogueRevision"],
        sold["listing"]["participant"]["settlement"]["incoming"]["catalogueRevision"]
    );
    client.batch_execute("RESET ROLE").await?;
    let previous_name: String = client
        .query_one("SELECT username FROM app.account WHERE id=$1", &[&seller])
        .await?
        .get(0);
    super::deletion::request(&client, seller, Uuid::new_v4()).await?;
    let retired = read(&client, "SELECT app.retire_account($1)", &[&seller]).await?;
    assert_eq!(retired["data"]["state"], "retired");
    set_auth_claims(&client, buyer_subject).await?;
    let anonymous = data(
        read(
            &client,
            "SELECT app.read_my_market_listing($1)",
            &[&listing],
        )
        .await?,
    );
    serde_json::from_value::<spellwake_application::exchange_reads::OwnedListing>(
        anonymous.clone(),
    )?
    .validate(listing)?;
    assert_eq!(anonymous["listing"]["seller"]["username"], "Deleted player");
    assert_eq!(
        anonymous["listing"]["participant"],
        sold["listing"]["participant"]
    );
    assert!(!anonymous.to_string().contains(&previous_name));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn exchange_read_roles_bounds_and_live_authority_are_explicit() -> TestResult {
    let client = connection().await?;
    let (_, subject) = buyer(&client).await?;
    let public = [
        "app.read_my_trade(uuid)",
        "app.read_my_trades(uuid,integer,text)",
        "app.read_my_market_listing(uuid)",
        "app.read_my_market(uuid,integer,boolean)",
    ];
    let private = [
        "app.account_identity_json(app.account)",
        "app.asset_identity_json(app.asset_definition)",
        "app.settlement_assessment_json(jsonb)",
        "app.trade_offer_json(app.trade_offer)",
        "app.market_listing_view(app.market_listing,uuid)",
        "app.read_market_listing(uuid,uuid)",
        "app.read_market_page(uuid,uuid,integer,boolean)",
    ];
    for role in [
        "anon",
        "authenticated",
        "service_role",
        "supabase_auth_admin",
    ] {
        for function in public {
            assert_eq!(
                client
                    .query_one(
                        "SELECT has_function_privilege($1,$2,'EXECUTE')",
                        &[&role, &function]
                    )
                    .await?
                    .get::<_, bool>(0),
                role == "authenticated"
            );
        }
        for function in private {
            assert!(
                !client
                    .query_one(
                        "SELECT has_function_privilege($1,$2,'EXECUTE')",
                        &[&role, &function]
                    )
                    .await?
                    .get::<_, bool>(0)
            );
        }
    }
    set_auth_claims(&client, subject).await?;
    for sql in [
        "SELECT app.read_my_trades(NULL,9,NULL)",
        "SELECT app.read_my_trades(NULL,8,'unknown')",
        "SELECT app.read_my_market(NULL,9,false)",
        "SELECT app.read_my_market(NULL,8,NULL)",
    ] {
        assert_eq!(
            read(&client, sql, &[]).await?,
            json!({"status":"fail","data":{"page":"invalid"}})
        );
    }
    let claims: String = client
        .query_one("SELECT current_setting('request.jwt.claims')", &[])
        .await?
        .get(0);
    client.batch_execute("RESET ROLE").await?;
    client
        .execute(
            "UPDATE auth.users SET banned_until=clock_timestamp()+interval '1 day' WHERE id=$1",
            &[&subject],
        )
        .await?;
    client.batch_execute("SET ROLE authenticated").await?;
    client
        .query_one(
            "SELECT set_config('request.jwt.claims',$1,false)",
            &[&claims],
        )
        .await?;
    assert_eq!(
        read(&client, "SELECT app.read_my_market(NULL,8,false)", &[]).await?,
        json!({"status":"fail","data":{"session":"unauthorized"}})
    );
    Ok(())
}
