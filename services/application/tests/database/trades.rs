use super::{
    TestResult, account, connection,
    pricing::buyer,
    social::{friend, participation_pair},
};
use serde_json::{Value, json};
use spellwake_application::{ApiResponse, commands::CommandAck};
use tokio_postgres::Client;
use uuid::Uuid;

fn resource(value: Value) -> Uuid {
    match serde_json::from_value::<ApiResponse<CommandAck>>(value).unwrap() {
        ApiResponse::Success { data } => data.resource_id,
        other => panic!("expected trade acknowledgment: {other:?}"),
    }
}

pub(super) async fn offer(
    client: &Client,
    sender: Uuid,
    recipient: Uuid,
    give: u64,
    want: u64,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    let coin: Uuid = client
        .query_one(
            "SELECT id FROM app.asset_definition WHERE currency_key='coin'",
            &[],
        )
        .await?
        .get(0);
    let bundle = |quantity: u64| {
        if quantity == 0 {
            json!([])
        } else {
            json!([{"assetId":coin,"quantity":quantity.to_string()}])
        }
    };
    client.batch_execute("SET ROLE service_role").await?;
    let result = super::economy_commands::execute_for_actor(client,sender,
        &json!({"kind":"trade_create","command":{"requestId":request,"recipientId":recipient,"give":bundle(give),"request":bundle(want)}}),false).await;
    client.batch_execute("RESET ROLE").await?;
    result
}

async fn respond(
    client: &Client,
    actor: Uuid,
    offer: Uuid,
    action: &str,
    request: Uuid,
) -> Result<Value, tokio_postgres::Error> {
    client.batch_execute("SET ROLE service_role").await?;
    let result = super::economy_commands::execute_for_actor(client,actor,
        &json!({"kind":"trade_response","command":{"requestId":request,"tradeId":offer,"action":action}}),false).await;
    client.batch_execute("RESET ROLE").await?;
    result
}

async fn balance(client: &Client, actor: Uuid) -> Result<String, tokio_postgres::Error> {
    Ok(client.query_one("SELECT i.owned_quantity::text FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE o.account_id=$1 AND d.currency_key='coin'", &[&actor]).await?.get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn trade_reserves_sealed_terms_and_completes_after_friendship_expires_once() -> TestResult {
    let client = connection().await?;
    let (left, subject) = buyer(&client).await?;
    let (right, right_subject) = buyer(&client).await?;
    let (outsider, other_subject) = account(&client).await?;
    participation_pair(&client, left, right, "open", 15, 3).await?;
    friend(&client, left, right).await?;
    let request = Uuid::new_v4();
    let created = offer(&client, left, right, 100, 110, request).await?;
    let trade = resource(created.clone());
    // A buggy privileged caller cannot release funds without closing its offer.
    assert!(
        client
            .execute(
                "UPDATE app.asset_hold SET state='released' WHERE commitment_ref=$1",
                &[&trade.to_string()]
            )
            .await
            .is_err()
    );
    client.batch_execute("SET ROLE service_role").await?;
    assert!(
        client
            .execute(
                "UPDATE app.asset_hold SET state='released' WHERE commitment_ref=$1",
                &[&trade.to_string()]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;

    assert_eq!(
        created,
        offer(&client, left, right, 100, 110, request).await?
    );
    assert_eq!(
        offer(&client, left, right, 100, 111, request).await?,
        json!({"status":"fail","data":{"request":"conflict"}})
    );
    assert_eq!(client.query_one("SELECT sum(l.quantity)::text FROM app.hold_line l JOIN app.asset_hold h ON h.id=l.hold_id WHERE h.commitment_ref=$1 AND h.state='held'", &[&trade.to_string()]).await?.get::<_,String>(0),"100");
    assert_eq!(
        offer(&client, left, right, 14500, 100, Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert!(
        client
            .execute(
                "UPDATE app.trade_line SET quantity=1 WHERE trade_id=$1",
                &[&trade]
            )
            .await
            .is_err()
    );
    assert!(client.execute("UPDATE app.hold_line SET quantity=1 WHERE hold_id=(SELECT sender_hold_id FROM app.trade_offer WHERE id=$1)", &[&trade]).await.is_err());
    assert_eq!(
        respond(&client, outsider, trade, "accept", Uuid::new_v4()).await?["status"],
        "fail"
    );
    resource(
        client
            .query_one(
                "SELECT app.change_friendship($1,$2,'remove',$3)",
                &[&left, &right, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    assert_eq!(
        offer(&client, left, right, 1, 1, Uuid::new_v4()).await?["status"],
        "fail"
    );
    let accept_request = Uuid::new_v4();
    let accepted = respond(&client, right, trade, "accept", accept_request).await?;
    resource(accepted.clone());
    assert_eq!(
        accepted,
        respond(&client, right, trade, "accept", accept_request).await?
    );
    assert_eq!(balance(&client, left).await?, "14510");
    assert_eq!(balance(&client, right).await?, "14490");
    assert_eq!(client.query_one("SELECT count(*) FROM app.ledger_operation WHERE source_scope='trade_settlement' AND source_ref=$1", &[&trade.to_string()]).await?.get::<_,i64>(0),1);
    assert_eq!(
        client
            .query_one(
                "SELECT state FROM app.asset_hold WHERE commitment_ref=$1",
                &[&trade.to_string()]
            )
            .await?
            .get::<_, String>(0),
        "consumed"
    );
    client.batch_execute("SET ROLE authenticated").await?;
    super::set_auth_claims(&client, subject).await?;
    assert_eq!(
        client
            .query("SELECT id FROM app.trade_line WHERE trade_id=$1", &[&trade])
            .await?
            .len(),
        2
    );
    super::set_auth_claims(&client, right_subject).await?;
    assert_eq!(
        client
            .query("SELECT id FROM app.trade_line WHERE trade_id=$1", &[&trade])
            .await?
            .len(),
        2
    );
    super::set_auth_claims(&client, other_subject).await?;
    assert!(
        client
            .query("SELECT id FROM app.trade_offer WHERE id=$1", &[&trade])
            .await?
            .is_empty()
    );
    assert!(
        client
            .query("SELECT id FROM app.trade_line WHERE trade_id=$1", &[&trade])
            .await?
            .is_empty()
    );
    client
        .batch_execute("RESET ROLE; SET ROLE service_role")
        .await?;
    assert!(
        client
            .execute(
                "UPDATE app.trade_offer SET state='offered' WHERE id=$1",
                &[&trade]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE; SET ROLE anon").await?;
    assert!(
        client
            .query("SELECT id FROM app.trade_offer WHERE id=$1", &[&trade])
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn restricted_gifts_fail_but_mature_sender_can_give_to_a_new_recipient() -> TestResult {
    let client = connection().await?;
    let (left, _) = super::pricing::buyer_with_age(&client, 31 * 86400).await?;
    let (right, _) = buyer(&client).await?;
    friend(&client, left, right).await?;
    let gift = resource(offer(&client, left, right, 100, 0, Uuid::new_v4()).await?);
    assert_eq!(
        respond(&client, right, gift, "accept", Uuid::new_v4()).await?["status"],
        "fail"
    );
    assert_eq!(balance(&client, left).await?, "14500");
    for index in 0..20 {
        participation_pair(
            &client,
            left,
            right,
            if index % 2 == 0 { "open" } else { "collection" },
            1,
            3,
        )
        .await?;
    }
    resource(respond(&client, right, gift, "accept", Uuid::new_v4()).await?);
    assert_eq!(balance(&client, left).await?, "14400");
    assert_eq!(balance(&client, right).await?, "14600");
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn blocks_prevent_acceptance_but_allow_cancellation_and_competing_responses_serialize()
-> TestResult {
    let client = connection().await?;
    let (left, _) = buyer(&client).await?;
    let (right, _) = buyer(&client).await?;
    friend(&client, left, right).await?;
    let rejected = resource(offer(&client, left, right, 10, 10, Uuid::new_v4()).await?);
    resource(respond(&client, right, rejected, "reject", Uuid::new_v4()).await?);
    assert_eq!(
        client
            .query_one(
                "SELECT state FROM app.asset_hold WHERE commitment_ref=$1",
                &[&rejected.to_string()]
            )
            .await?
            .get::<_, String>(0),
        "released"
    );

    let first = resource(offer(&client, left, right, 100, 110, Uuid::new_v4()).await?);
    resource(
        client
            .query_one(
                "SELECT app.change_block($1,$2,true,$3)",
                &[&left, &right, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    assert_eq!(
        respond(&client, right, first, "accept", Uuid::new_v4()).await?["status"],
        "fail"
    );
    resource(respond(&client, left, first, "cancel", Uuid::new_v4()).await?);
    resource(
        client
            .query_one(
                "SELECT app.change_block($1,$2,false,$3)",
                &[&left, &right, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    let second = resource(offer(&client, left, right, 100, 110, Uuid::new_v4()).await?);
    let other = connection().await?;
    let (a, b) = tokio::join!(
        respond(&client, left, second, "cancel", Uuid::new_v4()),
        respond(&other, right, second, "accept", Uuid::new_v4())
    );
    let status = [
        a?["status"].as_str().unwrap().to_owned(),
        b?["status"].as_str().unwrap().to_owned(),
    ];
    assert_eq!(status.iter().filter(|s| *s == "success").count(), 1);
    assert_eq!(status.iter().filter(|s| *s == "fail").count(), 1);
    let state: String = client
        .query_one("SELECT state FROM app.trade_offer WHERE id=$1", &[&second])
        .await?
        .get(0);
    assert_eq!(
        balance(&client, left).await?,
        if state == "accepted" {
            "14510"
        } else {
            "14500"
        }
    );
    assert_eq!(
        balance(&client, right).await?,
        if state == "accepted" {
            "14490"
        } else {
            "14500"
        }
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn both_restricted_sides_and_held_recipient_funds_are_checked_at_acceptance() -> TestResult {
    let client = connection().await?;
    let (left, _) = buyer(&client).await?;
    let (right, _) = buyer(&client).await?;
    friend(&client, left, right).await?;
    for (give, want, expected) in [
        (131, 100, "fail"),
        (100, 131, "fail"),
        (130, 100, "success"),
        (100, 130, "success"),
    ] {
        let trade = resource(offer(&client, left, right, give, want, Uuid::new_v4()).await?);
        assert_eq!(
            respond(&client, right, trade, "accept", Uuid::new_v4()).await?["status"],
            expected
        );
        if expected == "fail" {
            resource(respond(&client, left, trade, "cancel", Uuid::new_v4()).await?);
        }
    }
    let trade = resource(offer(&client, left, right, 100, 110, Uuid::new_v4()).await?);
    let coin_inventory: Uuid = client.query_one("SELECT i.id FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.owner_id=$1 AND d.currency_key='coin'", &[&right]).await?.get(0);
    let operation = super::operation(&client, right).await?;
    let hold = Uuid::new_v4();
    client.execute("INSERT INTO app.asset_hold(id,owner_id,purpose,commitment_ref,state,operation_id) VALUES($1,$2,'ante',$1::uuid::text,'held',$3)", &[&hold,&right,&operation]).await?;
    client
        .execute(
            "INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) VALUES($1,$2,14450)",
            &[&hold, &coin_inventory],
        )
        .await?;
    let rejected = Uuid::new_v4();
    assert_eq!(
        respond(&client, right, trade, "accept", rejected).await?["status"],
        "fail"
    );
    client
        .execute(
            "UPDATE app.asset_hold SET state='released' WHERE id=$1",
            &[&hold],
        )
        .await?;
    assert_eq!(
        respond(&client, right, trade, "accept", rejected).await?["status"],
        "fail"
    );
    resource(respond(&client, right, trade, "accept", Uuid::new_v4()).await?);
    let pending = resource(offer(&client, left, right, 10, 10, Uuid::new_v4()).await?);
    client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&right]).await?;
    assert_eq!(
        respond(&client, right, pending, "accept", Uuid::new_v4()).await?["status"],
        "fail"
    );
    resource(respond(&client, left, pending, "cancel", Uuid::new_v4()).await?);
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn mixed_card_pack_coin_trade_conserves_each_asset_without_minting_shop_stock() -> TestResult
{
    let client = connection().await?;
    let product = super::pricing::product(&client, true).await?;
    super::pricing::publish(
        &client,
        product.recipe,
        (4, 4),
        (1, 1),
        "baseline",
        &Uuid::new_v4().to_string(),
        None,
    )
    .await?;
    let (left, _) = buyer(&client).await?;
    let (right, _) = buyer(&client).await?;
    friend(&client, left, right).await?;
    let quote: Value = client
        .query_one("SELECT app.quote_packs($1,2)", &[&product.recipe])
        .await?
        .get(0);
    let policy: Uuid = quote["policyId"].as_str().unwrap().parse()?;
    resource(
        client
            .query_one(
                "SELECT app.buy_packs($1,$2,2,$3,$4,$5,$6)",
                &[
                    &left,
                    &product.recipe,
                    &policy,
                    &quote["revision"].as_str().unwrap(),
                    &quote["total"].as_str().unwrap(),
                    &Uuid::new_v4(),
                ],
            )
            .await?
            .get(0),
    );
    let pack: Uuid = client
        .query_one(
            "SELECT id FROM app.asset_definition WHERE recipe_id=$1",
            &[&product.recipe],
        )
        .await?
        .get(0);
    let coin: Uuid = client
        .query_one(
            "SELECT id FROM app.asset_definition WHERE currency_key='coin'",
            &[],
        )
        .await?
        .get(0);
    let card = product.cards[0];
    let inventory: Uuid = client
        .query_one("SELECT app.ensure_inventory($1,$2)", &[&left, &card])
        .await?
        .get(0);
    let op = super::operation(&client, left).await?;
    client.execute("INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES($1,$2,'credit',2,'test',$1::uuid::text)", &[&op,&inventory]).await?;
    client
        .execute(
            "UPDATE app.ledger_operation SET state='committed' WHERE id=$1",
            &[&op],
        )
        .await?;
    let give = json!([{"assetId":card,"quantity":"1"},{"assetId":pack,"quantity":"1"}]);
    let want = json!([{"assetId":coin,"quantity":"2200"}]);
    let trade = resource(
        client
            .query_one(
                "SELECT app.create_trade($1,$2,$3,$4,$5)",
                &[&left, &right, &give, &want, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    resource(respond(&client, right, trade, "accept", Uuid::new_v4()).await?);
    assert_eq!(balance(&client, left).await?, "14450");
    assert_eq!(balance(&client, right).await?, "12300");
    for asset in [card, pack] {
        let rows = client.query("SELECT owned_quantity::text FROM app.inventory_asset WHERE owner_id IN ($1,$2) AND asset_definition_id=$3", &[&left,&right,&asset]).await?;
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.get::<_, String>(0) == "1"));
    }
    assert_eq!(
        client
            .query_one(
                "SELECT sum(remaining_quantity)::text FROM app.supply_stock WHERE recipe_id=$1",
                &[&product.recipe]
            )
            .await?
            .get::<_, String>(0),
        "2"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn stale_trade_acceptance_after_a_block_cannot_transfer_assets() -> TestResult {
    let client = connection().await?;
    let (left, _) = buyer(&client).await?;
    let (right, _) = buyer(&client).await?;
    friend(&client, left, right).await?;
    let trade = resource(offer(&client, left, right, 100, 110, Uuid::new_v4()).await?);
    let mut stale = connection().await?;
    let transaction = stale
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    transaction.batch_execute("RESET ROLE").await?;
    transaction
        .query_one("SELECT state FROM app.trade_offer WHERE id=$1", &[&trade])
        .await?;
    resource(
        client
            .query_one(
                "SELECT app.change_block($1,$2,true,$3)",
                &[&left, &right, &Uuid::new_v4()],
            )
            .await?
            .get(0),
    );
    let error = transaction
        .query_one(
            "SELECT app.respond_trade($1,$2,'accept',$3)",
            &[&right, &trade, &Uuid::new_v4()],
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.code(),
        Some(&tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE)
    );
    transaction.rollback().await?;
    assert_eq!(balance(&client, left).await?, "14500");
    assert_eq!(balance(&client, right).await?, "14500");
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn invalid_live_trade_replacement_preserves_the_confirmed_terms() -> TestResult {
    let client = connection().await?;
    let (actor, subject) = account(&client).await?;
    let (peer, _) = account(&client).await?;
    let session: Uuid = client.query_one("INSERT INTO app.trade_session(initiator_id,counterparty_id,initiator_ready,counterparty_ready,counterparty_confirmed) VALUES($1,$2,true,true,true) RETURNING id", &[&actor,&peer]).await?.get(0);
    let original = Uuid::new_v4();
    let replacement = Uuid::new_v4();
    for asset in [original, replacement] {
        client.execute("INSERT INTO app.asset_definition(id,kind,currency_key) VALUES($1,'coin',$1::uuid::text)", &[&asset]).await?;
    }
    client.execute("INSERT INTO app.trade_session_item(session_id,owner_id,asset_definition_id,quantity) VALUES($1,$2,$3,1)", &[&session,&actor,&original]).await?;
    super::set_auth_claims(&client, subject).await?;
    let read = "SELECT app.read_my_trade_session($1)";
    let before: Value = client.query_one(read, &[&session]).await?.get(0);
    assert_eq!(before["status"], "success");
    let items = json!([
        {"assetId":replacement,"quantity":"2"},
        {"assetId":Uuid::new_v4(),"quantity":"1"}
    ]);
    let rejected: Value = client
        .query_one(
            "SELECT app.update_trade_session_offer($1,1,0,$2)",
            &[&session, &items],
        )
        .await?
        .get(0);
    assert_eq!(
        rejected,
        json!({"status":"fail","data":{"assets":"unavailable"}})
    );
    let after: Value = client.query_one(read, &[&session]).await?.get(0);
    assert_eq!(after, before);
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}
