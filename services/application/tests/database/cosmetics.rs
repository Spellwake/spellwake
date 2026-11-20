use super::{TestResult, account, connection, decks::library};
use serde_json::Value;
use spellwake_application::{ApiResponse, commands::CommandAck};
use tokio_postgres::{Client, error::SqlState};
use uuid::Uuid;

fn success(value: Value) -> Uuid {
    match serde_json::from_value::<ApiResponse<CommandAck>>(value).unwrap() {
        ApiResponse::Success { data } => data.resource_id,
        other => panic!("expected appearance acknowledgment: {other:?}"),
    }
}

#[path = "../support/cosmetic_sources.rs"]
mod sources;
use sources::event;
pub(super) use sources::{cosmetic, purchase};

async fn select_defaults(
    client: &Client,
    actor: Uuid,
    back: Uuid,
    playmat: Option<Uuid>,
) -> Result<Value, tokio_postgres::Error> {
    Ok(client.query_one("SELECT app.change_profile_defaults_command(a.auth_subject,(test_fixture.claims_for_subject(a.auth_subject)->>'session_id')::uuid,$4,jsonb_build_object('backId',p.default_back_id,'sleeveId',p.default_sleeve_id,'diceId',p.default_dice_id,'playmatId',p.default_playmat_id),jsonb_build_object('backId',$2::uuid,'sleeveId',p.default_sleeve_id,'diceId',p.default_dice_id,'playmatId',$3::uuid)) FROM app.profile p JOIN app.account a ON a.id=p.account_id WHERE p.account_id=$1", &[&actor,&back,&playmat,&Uuid::new_v4()]).await?.get(0))
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn source_specific_reversal_preserves_other_grants_and_replaces_last_selection() -> TestResult
{
    let client = connection().await?;
    let (actor, subject, deck, _, _) = library(&client).await?;
    let (other, other_subject) = account(&client).await?;
    let free: Uuid = client
        .query_one(
            "SELECT default_back_id FROM app.profile WHERE account_id=$1",
            &[&actor],
        )
        .await?
        .get(0);
    let back = cosmetic(&client, "back").await?;
    let mat = cosmetic(&client, "playmat").await?;
    let first = purchase(&client, actor, back).await?;
    let second = purchase(&client, actor, back).await?;
    let mat_purchase = purchase(&client, actor, mat).await?;
    success(select_defaults(&client, actor, back, Some(mat)).await?);
    success(client.query_one("SELECT app.change_deck_appearance($1,$2,$3,p.default_sleeve_id,p.default_dice_id,$4,$5) FROM app.profile p WHERE p.account_id=$1", &[&actor,&deck,&back,&mat,&Uuid::new_v4()]).await?.get(0));
    // A source grant is bound to both the purchased item and the paid account.
    assert!(client.execute("INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,purchase_id,state) VALUES($1,$2,'stripe_purchase',$3,'active')", &[&other,&back,&first]).await.is_err());
    assert!(client.execute("INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,purchase_id,state) VALUES($1,$2,'stripe_purchase',$3,'active')", &[&actor,&mat,&first]).await.is_err());
    assert!(
        client
            .execute(
                "UPDATE app.cosmetic_grant SET state='revoked' WHERE purchase_id=$1",
                &[&first]
            )
            .await
            .is_err()
    );
    client
        .execute(
            "UPDATE app.purchase SET state='refunded' WHERE id=$1",
            &[&first],
        )
        .await?;
    assert_eq!(
        client
            .query_one(
                "SELECT default_back_id FROM app.profile WHERE account_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, Uuid>(0),
        back
    );
    assert_eq!(
        client
            .query_one("SELECT back_id FROM app.deck WHERE id=$1", &[&deck])
            .await?
            .get::<_, Uuid>(0),
        back
    );
    client
        .execute(
            "UPDATE app.purchase SET state='reversed' WHERE id=$1",
            &[&second],
        )
        .await?;
    assert_eq!(
        client
            .query_one(
                "SELECT default_back_id FROM app.profile WHERE account_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, Uuid>(0),
        free
    );
    assert_eq!(
        client
            .query_one("SELECT back_id FROM app.deck WHERE id=$1", &[&deck])
            .await?
            .get::<_, Uuid>(0),
        free
    );
    assert!(
        client
            .execute(
                "UPDATE app.purchase SET state='paid' WHERE id=$1",
                &[&second]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.cosmetic_grant SET state='active' WHERE purchase_id=$1",
                &[&second]
            )
            .await
            .is_err()
    );
    client
        .execute(
            "UPDATE app.purchase SET state='refunded' WHERE id=$1",
            &[&mat_purchase],
        )
        .await?;
    assert!(
        client
            .query_one(
                "SELECT default_playmat_id FROM app.profile WHERE account_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, Option<Uuid>>(0)
            .is_none()
    );
    assert!(
        client
            .query_one("SELECT playmat_id FROM app.deck WHERE id=$1", &[&deck])
            .await?
            .get::<_, Option<Uuid>>(0)
            .is_none()
    );
    let next: Value = client.query_one("SELECT app.resolve_account_appearance($1,$2,p.default_sleeve_id,p.default_dice_id,$3) FROM app.profile p WHERE p.account_id=$1", &[&actor,&back,&mat]).await?.get(0);
    assert_eq!(next["backId"], free.to_string());
    assert!(next["playmatId"].is_null());
    // Internal provider references are never readable to another account.
    client.batch_execute("SET ROLE authenticated").await?;
    super::set_auth_claims(&client, subject).await?;
    assert_eq!(
        client
            .query(
                "SELECT id FROM app.purchase WHERE id IN ($1,$2,$3)",
                &[&first, &second, &mat_purchase]
            )
            .await?
            .len(),
        3
    );
    super::set_auth_claims(&client, other_subject).await?;
    assert!(
        client
            .query("SELECT id FROM app.purchase WHERE id=$1", &[&first])
            .await?
            .is_empty()
    );
    assert!(
        client
            .query_one(
                "SELECT app.resolve_account_appearance($1,$2,$2,$2,NULL)",
                &[&actor, &back]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.purchase SET state='paid' WHERE id=$1",
                &[&first]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn paid_patreon_grants_survive_cancellation_but_not_all_payment_reversals() -> TestResult {
    let client = connection().await?;
    let (actor, _, _, _, _) = library(&client).await?;
    let back = cosmetic(&client, "back").await?;
    let dice = cosmetic(&client, "dice").await?;
    let tier = Uuid::new_v4();
    client.execute("INSERT INTO app.supporter_tier(id,patreon_tier_ref,name,rank) VALUES($1,$1::uuid::text,'Test tier',1)", &[&tier]).await?;
    for item in [back, dice] {
        client
            .execute(
                "INSERT INTO app.tier_cosmetic(tier_id,cosmetic_id) VALUES($1,$2)",
                &[&tier, &item],
            )
            .await?;
    }
    let proof = event(&client, actor, "patreon").await?;
    let membership = Uuid::new_v4();
    client.execute("INSERT INTO app.patreon_membership(id,account_id,patreon_member_ref,active_tier_id,active,source_event_id) VALUES($1,$2,$1::uuid::text,$3,true,$4)", &[&membership,&actor,&tier,&proof]).await?;
    assert_eq!(
        client
            .query_one("SELECT app.active_supporter_tier($1)", &[&actor])
            .await?
            .get::<_, Option<Uuid>>(0),
        Some(tier)
    );
    assert!(
        !client
            .query_one("SELECT app.has_cosmetic($1,$2,'back')", &[&actor, &back])
            .await?
            .get::<_, bool>(0)
    );
    let refs = [Uuid::new_v4().to_string(), Uuid::new_v4().to_string()];
    let mut ids = Vec::new();
    for payment in &refs {
        let financial = super::patreon_financial_integrity::proof(
            membership,
            tier,
            payment,
            "paid",
            &format!("paid:{payment}"),
        );
        let accepted = super::patreon_financial_integrity::commit(
            &client,
            &financial,
            &Uuid::new_v4().to_string(),
        )
        .await?;
        ids.push(accepted["paymentId"].as_str().unwrap().parse::<Uuid>()?);
    }
    assert_eq!(ids.len(), 2);
    let second = ids[1];
    client
        .execute(
            "UPDATE app.patreon_membership SET active=false,active_tier_id=NULL WHERE id=$1",
            &[&membership],
        )
        .await?;
    assert_eq!(
        client
            .query_one("SELECT app.active_supporter_tier($1)", &[&actor])
            .await?
            .get::<_, Option<Uuid>>(0),
        None
    );
    assert!(
        client
            .query_one("SELECT app.has_cosmetic($1,$2,'back')", &[&actor, &back])
            .await?
            .get::<_, bool>(0)
    );
    assert_eq!(client.query_one("SELECT count(*) FROM app.cosmetic_grant WHERE account_id=$1 AND source_kind='patreon_payment' AND state='active'", &[&actor]).await?.get::<_,i64>(0),4);
    let refund = super::patreon_financial_integrity::proof(
        membership,
        tier,
        &refs[0],
        "refunded",
        &format!("refund:{}", refs[0]),
    );
    super::patreon_financial_integrity::commit(&client, &refund, &Uuid::new_v4().to_string())
        .await?;
    assert!(
        client
            .query_one("SELECT app.has_cosmetic($1,$2,'back')", &[&actor, &back])
            .await?
            .get::<_, bool>(0)
    );
    let reverse = super::patreon_financial_integrity::proof(
        membership,
        tier,
        &refs[1],
        "reversed",
        &format!("reverse:{}", refs[1]),
    );
    super::patreon_financial_integrity::commit(&client, &reverse, &Uuid::new_v4().to_string())
        .await?;
    assert!(
        !client
            .query_one("SELECT app.has_cosmetic($1,$2,'back')", &[&actor, &back])
            .await?
            .get::<_, bool>(0)
    );
    assert!(
        client
            .execute(
                "UPDATE app.patreon_payment SET state='paid' WHERE id=$1",
                &[&second]
            )
            .await
            .is_err()
    );
    // A verified event cannot be moved to another account after granting.
    let (other, _) = account(&client).await?;
    assert!(
        client
            .execute(
                "UPDATE app.provider_event SET account_id=$1 WHERE id=$2",
                &[&other, &proof]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .execute(
                "UPDATE app.patreon_membership SET account_id=$1 WHERE id=$2",
                &[&other, &membership]
            )
            .await
            .is_err()
    );
    let other_proof = event(&client, other, "patreon").await?;
    assert!(client.execute("INSERT INTO app.patreon_membership(account_id,patreon_member_ref,active,source_event_id) VALUES($1,$2::uuid::text,false,$3)", &[&other,&membership,&other_proof]).await.is_err());
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn stale_repeatable_read_cannot_reequip_refunded_cosmetics() -> TestResult {
    let client = connection().await?;
    let (actor, _, deck, _, _) = library(&client).await?;
    let back = cosmetic(&client, "back").await?;
    let paid = purchase(&client, actor, back).await?;
    let mut stale = connection().await?;
    let transaction = stale
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    assert!(
        transaction
            .query_one("SELECT app.has_cosmetic($1,$2,'back')", &[&actor, &back])
            .await?
            .get::<_, bool>(0)
    );
    client
        .execute(
            "UPDATE app.purchase SET state='refunded' WHERE id=$1",
            &[&paid],
        )
        .await?;
    let error = transaction.query_one("SELECT app.change_deck_appearance($1,$2,$3,p.default_sleeve_id,p.default_dice_id,NULL,$4) FROM app.profile p WHERE p.account_id=$1", &[&actor,&deck,&back,&Uuid::new_v4()]).await.unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::T_R_SERIALIZATION_FAILURE));
    transaction.rollback().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires migrated isolated Postgres and SPELLWAKE_TEST_DATABASE_URL"]
async fn deletion_blocks_new_associations_but_preserves_inflight_reconciliation() -> TestResult {
    let client = connection().await?;
    let (actor, _, _, _, _) = library(&client).await?;
    let (pending, _) = account(&client).await?;
    let (other, _) = account(&client).await?;
    let proof = event(&client, actor, "patreon").await?;
    let member = Uuid::new_v4();
    client.execute("INSERT INTO app.patreon_membership(id,account_id,patreon_member_ref,active,source_event_id) VALUES($1,$2,$1::uuid::text,false,$3)", &[&member,&actor,&proof]).await?;
    assert!(
        client
            .execute("DELETE FROM app.patreon_membership WHERE id=$1", &[&member])
            .await
            .is_err()
    );
    let other_proof = event(&client, other, "patreon").await?;
    assert!(client.execute("INSERT INTO app.patreon_membership(account_id,patreon_member_ref,active,source_event_id) VALUES($1,$2::uuid::text,false,$3)", &[&other,&member,&other_proof]).await.is_err());
    let item = cosmetic(&client, "back").await?;
    let purchase = Uuid::new_v4();
    client.execute("INSERT INTO app.purchase(id,account_id,cosmetic_id,provider_request_ref,requested_at,state) VALUES($1,$2,$3,$1::uuid::text,clock_timestamp(),'creating')", &[&purchase,&actor,&item]).await?;
    for id in [actor, pending] {
        client.execute("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id=$1", &[&id]).await?;
    }
    let pending_proof = event(&client, pending, "patreon").await?;
    assert!(client.execute("INSERT INTO app.patreon_membership(account_id,patreon_member_ref,active,source_event_id) VALUES($1,$2::uuid::text,false,$3)", &[&pending,&Uuid::new_v4(),&pending_proof]).await.is_err());
    // Existing inactive membership proof remains reconcilable while pending.
    client
        .execute(
            "UPDATE app.patreon_membership SET active=false WHERE id=$1",
            &[&member],
        )
        .await?;
    assert!(client.execute("INSERT INTO app.purchase(account_id,cosmetic_id,provider_request_ref,requested_at,state) VALUES($1,$2,$3::uuid::text,clock_timestamp(),'creating')", &[&actor,&item,&Uuid::new_v4()]).await.is_err());
    client.execute("UPDATE app.purchase SET state='pending',stripe_checkout_ref=$1::uuid::text WHERE id=$1", &[&purchase]).await?;
    let paid_proof = event(&client, actor, "stripe").await?;
    client.execute("UPDATE app.purchase SET state='paid',stripe_payment_ref=$1::uuid::text,provider_event_id=$2 WHERE id=$1", &[&purchase,&paid_proof]).await?;
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.cosmetic_grant WHERE purchase_id=$1 AND state='active'",
                &[&purchase]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    client
        .execute(
            "UPDATE app.purchase SET state='refunded' WHERE id=$1",
            &[&purchase],
        )
        .await?;
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.cosmetic_grant WHERE purchase_id=$1 AND state='active'",
                &[&purchase]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    Ok(())
}
