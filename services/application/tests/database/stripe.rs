//! Privileged synthetic provider facts exercise the real service RPC transaction.
//! Signature/provider-HTTP qualification is separate; no live payment is claimed.
use super::{TestResult, connection, decks::library, set_auth_claims};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;
use uuid::Uuid;

fn key(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
async fn source(client: &Client, label: &str, target: &str) -> Result<Uuid, tokio_postgres::Error> {
    source_at_revision(client, label, target, None).await
}
async fn source_at_revision(
    client: &Client,
    label: &str,
    target: &str,
    revision: Option<&str>,
) -> Result<Uuid, tokio_postgres::Error> {
    let current: String = client.query_one("SELECT coalesce(max(revision),0)::text FROM app.purchase WHERE stripe_payment_ref='pi_stripePath'", &[]).await?.get(0);
    let capture = json!({"version":1,"kind":"api_read","bodyHash":key(label),"apiVersion":"2026-10-28.endive","live":false,"targetId":target,"purchaseRevision":revision.unwrap_or(&current)});
    let value: Value = client
        .query_one(
            "SELECT app.capture_stripe_event($1,'api:reconciliation',$2,NULL)",
            &[&format!("api-read:{}", key(label)), &capture],
        )
        .await?
        .get(0);
    Ok(value["id"].as_str().unwrap().parse().unwrap())
}
async fn purchase(client: &Client, id: Uuid) -> Result<Value, tokio_postgres::Error> {
    Ok(client
        .query_one("SELECT app.read_stripe_purchase($1)", &[&id])
        .await?
        .get(0))
}
fn effect(ticket: &Value, kind: &str, source_label: &str, target: &str) -> Value {
    let mut proof = json!({"version":1,"kind":kind,"effectKey":key(source_label),"termsHash":ticket["termsHash"],"sourceObjectKey":key(target),
        "checkoutKey":key("cs_test_stripePath"),"paymentKey":key("pi_stripePath"),"chargeKey":key("ch_stripePath"),"paid":true,"amountMinor":"500","currency":"usd"});
    if target.starts_with("du_") {
        proof["disputeKey"] = json!(key(target));
    }
    proof
}
async fn notification_contracts(
    c: &Client,
    purchase: Uuid,
    original: Uuid,
    retry: Uuid,
) -> TestResult {
    use spellwake_application::{
        decode_wire_object,
        stripe::notification::{
            AssociatedAck, CompletionAck, IgnoredAck, evidence_locator_matches,
        },
    };
    let hash = key(&Uuid::new_v4().to_string());
    let capture = json!({"version":1,"kind":"webhook","bodyHash":hash,"apiVersion":"2026-10-28.endive","live":false,"targetId":"cs_test_stripePath"});
    let row: Value = c
        .query_one(
            "SELECT app.capture_stripe_event($1,'checkout.session.completed',$2,clock_timestamp())",
            &[&format!("evt_{}", Uuid::new_v4().simple()), &capture],
        )
        .await?
        .get(0);
    let event: Uuid = row["id"].as_str().unwrap().parse()?;
    for _ in 0..2 {
        let row: Value = c
            .query_one(
                "SELECT app.associate_stripe_notification($1,$2)",
                &[&event, &purchase],
            )
            .await?
            .get(0);
        let ack: AssociatedAck = decode_wire_object(&serde_json::to_vec(&row)?)?;
        assert!(ack.confirmed());
    }
    let first: Value = c
        .query_one(
            "SELECT app.complete_stripe_notification($1,$2)",
            &[&event, &original],
        )
        .await?
        .get(0);
    let replay: Value = c
        .query_one(
            "SELECT app.complete_stripe_notification($1,$2)",
            &[&event, &retry],
        )
        .await?
        .get(0);
    assert_eq!(first, replay);
    let ack: CompletionAck = decode_wire_object(&serde_json::to_vec(&replay)?)?;
    assert!(ack.matches_observed(purchase, None));
    assert!(ack.matches_observed(purchase, Some(original)));
    assert!(!ack.matches_observed(purchase, Some(retry)));
    assert!(!ack.matches_observed(Uuid::new_v4(), None));
    let ignored:Value=c.query_one("SELECT app.capture_stripe_event($1,'fixture.unsupported',$2,clock_timestamp()-interval '31 days')",&[&format!("evt_{}",Uuid::new_v4().simple()),&capture]).await?.get(0);
    let unsupported: Uuid = ignored["id"].as_str().unwrap().parse()?;
    for _ in 0..2 {
        let row: Value = c
            .query_one("SELECT app.ignore_stripe_notification($1)", &[&unsupported])
            .await?
            .get(0);
        let ack: IgnoredAck = decode_wire_object(&serde_json::to_vec(&row)?)?;
        assert!(ack.confirmed());
    }
    let row: Value = c
        .query_one(
            "SELECT app.complete_stripe_notification($1,$2)",
            &[&unsupported, &retry],
        )
        .await?
        .get(0);
    let ack: CompletionAck = decode_wire_object(&serde_json::to_vec(&row)?)?;
    assert!(matches!(ack, CompletionAck::Unsupported {}));
    assert!(!ack.matches_observed(purchase, None));
    // Retention starts at authoritative receipt time, not the supplied older
    // delivery instant. This fresh captured source is intentionally not due.
    let expiry: Option<Value> = c
        .query_one(
            "SELECT app.begin_stripe_evidence_expiry($1)",
            &[&unsupported],
        )
        .await?
        .get(0);
    assert!(expiry.is_none());
    assert!(evidence_locator_matches(
        ignored["payloadRef"].as_str().unwrap(),
        &hash
    ));
    Ok(())
}
async fn apply(
    client: &Client,
    id: Uuid,
    event: Uuid,
    ticket: &Value,
    effect: &Value,
) -> Result<Value, tokio_postgres::Error> {
    let mut proof = effect.clone();
    let kind = proof["kind"].as_str().unwrap();
    let identity = match kind {
        "paid" => format!("paid:{}", proof["paymentKey"].as_str().unwrap()),
        "refunded" | "reversed" | "restored" => format!(
            "{kind}:{}:{}",
            proof["paymentKey"].as_str().unwrap(),
            proof["movementKey"].as_str().unwrap()
        ),
        _ => format!("{kind}:{}", proof["checkoutKey"].as_str().unwrap()),
    };
    proof["effectKey"] = json!(key(&identity));
    Ok(client.query_one("SELECT app.commit_stripe_effect($1,$2,$3::text::numeric,$4,'cs_test_stripePath','pi_stripePath')",
    &[&id,&event,&ticket["revision"].as_str().unwrap(),&proof]).await?.get(0))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stripe_reservation_paid_reversal_restoration_and_later_loss_are_source_once_only()
-> TestResult {
    let client = connection().await?;
    let (actor, subject, _, _, _) = library(&client).await?;
    let claims: Value = client
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    let session: Uuid = claims["session_id"].as_str().unwrap().parse()?;
    let cosmetic = Uuid::new_v4();
    client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state) VALUES($1,'back','test-only:stripe-path','active')",&[&cosmetic]).await?;
    let request = Uuid::new_v4();
    let terms = json!({"version":1,"priceId":"price_stripePath","currency":"usd","minorUnits":"500","apiVersion":"2026-10-28.endive","live":false,"successUrl":"https://app.example/return","cancelUrl":"https://app.example/cancel"});
    client.batch_execute("SET ROLE service_role").await?;
    let reserved: Value = client
        .query_one(
            "SELECT app.reserve_stripe_purchase($1,$2,$3,$4,$5)",
            &[&subject, &session, &request, &cosmetic, &terms],
        )
        .await?
        .get(0);
    assert_eq!(reserved["status"], "success");
    let id: Uuid = reserved["data"]["resourceId"].as_str().unwrap().parse()?;
    // Receipt-first replay does not require the old/current Price to remain available.
    let replay: Value = client
        .query_one(
            "SELECT app.reserve_stripe_purchase($1,$2,$3,$4,NULL)",
            &[&subject, &session, &request, &cosmetic],
        )
        .await?
        .get(0);
    assert_eq!(reserved, replay);
    let claimed: Value = client
        .query_one("SELECT app.claim_stripe_creation($1)", &[&id])
        .await?
        .get(0);
    assert!(claimed["deadline"].is_string());
    let ticket = purchase(&client, id).await?;
    client.query_one("SELECT app.ack_stripe_checkout($1,'cs_test_stripePath','pi_stripePath','https://checkout.stripe.com/c/pay/synthetic',statement_timestamp()+interval '1 hour',$2)",&[&id,&ticket["termsHash"].as_str().unwrap()]).await?;
    let a = source(&client, "paid-A", "cs_test_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    let mut paid = effect(
        &ticket,
        "paid",
        "payment-pi-stripePath",
        "cs_test_stripePath",
    );
    // The first observation can already include a won dispute. This is the
    // first grant, while retaining proof that its earlier debit was settled.
    paid["settledReversalKey"] = json!(key("txn_already_settled"));
    paid["reversalKey"] = json!(key("txn_already_settled"));
    paid["movementKey"] = json!(key("txn_already_returned"));
    paid["fundsReturned"] = json!(true);
    paid["refundMinorUnits"] = json!("0");
    let mut incomplete = paid.clone();
    incomplete.as_object_mut().unwrap().remove("chargeKey");
    assert!(apply(&client, id, a, &ticket, &incomplete).await.is_err());
    assert_eq!(purchase(&client, id).await?["state"], "pending");
    let result_a = apply(&client, id, a, &ticket, &paid).await?;
    assert_eq!(result_a["state"], "paid");
    let grant_a: Uuid = result_a["grantId"].as_str().unwrap().parse()?;
    // Separate paid notification is a no-op, not a second entitlement source.
    let a2 = source(&client, "paid-A2", "cs_test_stripePath").await?;
    let concurrent = source(&client, "concurrent-refund-capture", "cs_test_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    let again = apply(
        &client,
        id,
        a2,
        &ticket,
        &effect(
            &ticket,
            "paid",
            "different-paid-observation",
            "cs_test_stripePath",
        ),
    )
    .await?;
    assert_eq!(again["disposition"], "noop");
    notification_contracts(&client, id, a, a2).await?;
    assert_eq!(
        client
            .query_one(
                "SELECT processing_state FROM app.provider_event WHERE id=$1",
                &[&concurrent]
            )
            .await?
            .get::<_, String>(0),
        "received"
    );
    let old_debit = source(&client, "already-settled", "du_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    let mut old_loss = effect(&ticket, "reversed", "old-debit", "du_stripePath");
    old_loss["movementKey"] = json!(key("txn_already_settled"));
    assert_eq!(
        apply(&client, id, old_debit, &ticket, &old_loss).await?["disposition"],
        "noop"
    );
    let b = source(&client, "loss-B", "du_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    let mut loss = effect(&ticket, "reversed", "withdrawal-B", "du_stripePath");
    loss["movementKey"] = json!(key("txn_withdrawal_B"));
    let mut incomplete = loss.clone();
    incomplete.as_object_mut().unwrap().remove("disputeKey");
    assert!(apply(&client, id, b, &ticket, &incomplete).await.is_err());
    apply(&client, id, b, &ticket, &loss).await?;
    // Only a revision advance makes the concurrent older read uncommittable.
    // Its eventual RPC must ask for a fresh read, never return a paid success.
    let ticket = purchase(&client, id).await?;
    assert_eq!(
        apply(&client, id, concurrent, &ticket, &paid).await?["disposition"],
        "stale"
    );
    let c = source(&client, "restoration-C", "du_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    let mut restored = effect(&ticket, "restored", "reinstatement-C", "du_stripePath");
    restored["movementKey"] = json!(key("txn_reinstatement_C"));
    restored["reversalEventId"] = json!(b);
    restored["reversalKey"] = json!(key("txn_withdrawal_B"));
    restored["settledReversalKey"] = json!(key("txn_withdrawal_B"));
    restored["refundMinorUnits"] = json!("0");
    restored["fundsReturned"] = json!(true);
    let result_c = apply(&client, id, c, &ticket, &restored).await?;
    assert_eq!(result_c["state"], "paid");
    let grant_c: Uuid = result_c["grantId"].as_str().unwrap().parse()?;
    assert_ne!(grant_a, grant_c);
    let d = source(&client, "loss-D", "du_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    let mut later = effect(&ticket, "reversed", "withdrawal-D", "du_stripePath");
    later["movementKey"] = json!(key("txn_withdrawal_D"));
    apply(&client, id, d, &ticket, &later).await?;
    for (event, proof) in [(a, &paid), (b, &loss), (c, &restored)] {
        let ticket = purchase(&client, id).await?;
        let _ = apply(&client, id, event, &ticket, proof).await?;
    }
    assert_eq!(purchase(&client, id).await?["state"], "reversed");
    let rows = client
        .query(
            "SELECT id,state FROM app.cosmetic_grant WHERE purchase_id=$1 ORDER BY id",
            &[&id],
        )
        .await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.get::<_, String>(1) == "revoked"));
    let e = source(&client, "refund-E", "re_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    let mut refund = effect(&ticket, "refunded", "refund-E", "re_stripePath");
    refund["movementKey"] = json!(key("re_stripePath"));
    refund["refundMinorUnits"] = json!("500");
    for amount in [json!(null), json!("0"), json!("501"), json!(500)] {
        let mut invalid = refund.clone();
        invalid["refundMinorUnits"] = amount;
        assert!(apply(&client, id, e, &ticket, &invalid).await.is_err());
    }
    assert_eq!(
        apply(&client, id, e, &ticket, &refund).await?["state"],
        "refunded"
    );
    let stale = source(&client, "stale-paid-after-refund", "cs_test_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    assert_eq!(
        apply(&client, id, stale, &ticket, &paid).await?["state"],
        "refunded"
    );
    let f = source(&client, "refund-return-F", "re_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    let mut returned = effect(&ticket, "restored", "refund-return-F", "re_stripePath");
    returned["movementKey"] = json!(key("txn_refund_return_F"));
    returned["reversalEventId"] = json!(e);
    returned["reversalKey"] = json!(key("re_stripePath"));
    returned["settledReversalKey"] = json!(key("re_stripePath"));
    returned["refundMinorUnits"] = json!("0");
    returned["fundsReturned"] = json!(true);
    let result_f = apply(&client, id, f, &ticket, &returned).await?;
    assert_eq!(result_f["state"], "paid");
    assert_ne!(result_f["grantId"], result_a["grantId"]);
    assert_ne!(result_f["grantId"], result_c["grantId"]);
    let states: Vec<String> = client
        .query(
            "SELECT state FROM app.cosmetic_grant WHERE purchase_id=$1 ORDER BY state",
            &[&id],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(states, ["active", "revoked", "revoked"]);
    assert_eq!(
        client
            .query_one(
                "SELECT count(*) FROM app.purchase WHERE account_id=$1",
                &[&actor]
            )
            .await?
            .get::<_, i64>(0),
        1
    );
    client.batch_execute("RESET ROLE").await?;
    // Terminal purchases do not hide a source-established pending observation
    // from deletion diagnostics, even before account association is committed.
    let orphan = source_at_revision(
        &client,
        "optimistic-conflict",
        "cs_test_stripePath",
        Some("0"),
    )
    .await?;
    let requested: Value = super::deletion::request(&client, actor, Uuid::new_v4()).await?;
    assert_eq!(requested["status"], "success");
    client.batch_execute("SET ROLE service_role").await?;
    let diagnostics: Value = client
        .query_one("SELECT app.read_account_deletion_status($1)", &[&actor])
        .await?
        .get(0);
    assert_eq!(diagnostics["obligations"]["purchasesInFlight"], "0");
    assert_eq!(diagnostics["obligations"]["pendingStripeEvents"], "1");
    let settled = source(&client, "settled-after-conflict", "cs_test_stripePath").await?;
    let ticket = purchase(&client, id).await?;
    apply(&client, id, settled, &ticket, &paid).await?;
    let diagnostics: Value = client
        .query_one("SELECT app.read_account_deletion_status($1)", &[&actor])
        .await?
        .get(0);
    assert_eq!(diagnostics["obligations"]["pendingStripeEvents"], "0");
    assert_eq!(
        client
            .query_one(
                "SELECT effect->>'kind' FROM app.provider_event WHERE id=$1",
                &[&orphan]
            )
            .await?
            .get::<_, String>(0),
        "superseded"
    );
    let ticket = purchase(&client, id).await?;
    assert_eq!(
        apply(&client, id, orphan, &ticket, &paid).await?["disposition"],
        "stale"
    );
    client.batch_execute("RESET ROLE").await?;
    set_auth_claims(&client, subject).await?;
    let visible: Value = client
        .query_one("SELECT app.read_my_purchase($1)", &[&id])
        .await?
        .get(0);
    assert_eq!(visible["status"], "success");
    assert_eq!(visible["data"]["state"], "paid");
    let page: Value = client
        .query_one("SELECT app.read_my_purchases(NULL,100)", &[])
        .await?
        .get(0);
    let parsed: spellwake_application::store::StorePage<
        spellwake_application::stripe::PurchaseView,
    > = serde_json::from_value(page["data"].clone())?;
    parsed.validate_after(None, 100)?;
    assert!(
        parsed
            .items
            .iter()
            .any(|purchase| purchase.purchase_id == id)
    );
    assert!(!page.to_string().contains("price_stripePath"));
    assert!(
        client
            .query(
                "SELECT checkout_terms FROM app.purchase WHERE id=$1",
                &[&id]
            )
            .await
            .is_err()
    );
    assert!(
        client
            .query(
                "SELECT stripe_payment_ref FROM app.purchase WHERE id=$1",
                &[&id]
            )
            .await
            .is_err()
    );
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stripe_event_identity_ignores_fresh_delivery_time_but_rejects_changed_body() -> TestResult
{
    let client = connection().await?;
    let external = format!("evt_{}", Uuid::new_v4().simple());
    let capture = json!({"version":1,"kind":"webhook","bodyHash":key("same body"),"apiVersion":"2026-10-28.endive","live":false,"targetId":"cs_test_signal"});
    client.batch_execute("SET ROLE service_role").await?;
    let first:Value=client.query_one("SELECT app.capture_stripe_event($1,'checkout.session.completed',$2,statement_timestamp())",&[&external,&capture]).await?.get(0);
    let second:Value=client.query_one("SELECT app.capture_stripe_event($1,'checkout.session.completed',$2,statement_timestamp()+interval '2 minutes')",&[&external,&capture]).await?.get(0);
    assert_eq!(first, second);
    let mut bad = capture.clone();
    bad["bodyHash"] = json!(key("changed body"));
    assert!(client.query_one("SELECT app.capture_stripe_event($1,'checkout.session.completed',$2,statement_timestamp())",&[&external,&bad]).await.is_err());
    for table in ["purchase", "provider_event", "cosmetic_grant"] {
        assert!(
            !client
                .query_one(
                    "SELECT has_table_privilege('service_role',$1,'INSERT,UPDATE,DELETE,TRUNCATE')",
                    &[&format!("app.{table}")]
                )
                .await?
                .get::<_, bool>(0)
        );
    }
    client.batch_execute("RESET ROLE").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn stripe_webhook_effect_requires_a_finite_revision_and_processed_replay_keeps_its_purchase_target()
-> TestResult {
    use tokio_postgres::error::SqlState;
    let c = connection().await?;
    let (actor, subject, _, _, _) = library(&c).await?;
    let (other, other_subject, _, _, _) = library(&c).await?;
    let claims: Value = c
        .query_one("SELECT test_fixture.claims_for_subject($1)", &[&subject])
        .await?
        .get(0);
    let session: Uuid = claims["session_id"].as_str().unwrap().parse()?;
    let claims: Value = c
        .query_one(
            "SELECT test_fixture.claims_for_subject($1)",
            &[&other_subject],
        )
        .await?
        .get(0);
    let other_session: Uuid = claims["session_id"].as_str().unwrap().parse()?;
    let cosmetic = Uuid::new_v4();
    c.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state) VALUES($1,'back','fixture:stripe-identity','active')", &[&cosmetic]).await?;
    let terms = json!({"version":1,"priceId":"price_fixtureIdentity","currency":"usd","minorUnits":"500","apiVersion":"2026-10-28.endive","live":false,"successUrl":"https://app.example/return","cancelUrl":"https://app.example/cancel"});
    c.batch_execute("SET ROLE service_role").await?;
    let a: Value = c
        .query_one(
            "SELECT app.reserve_stripe_purchase($1,$2,$3,$4,$5)",
            &[&subject, &session, &Uuid::new_v4(), &cosmetic, &terms],
        )
        .await?
        .get(0);
    let id: Uuid = a["data"]["resourceId"].as_str().unwrap().parse()?;
    let b: Value = c
        .query_one(
            "SELECT app.reserve_stripe_purchase($1,$2,$3,$4,$5)",
            &[
                &other_subject,
                &other_session,
                &Uuid::new_v4(),
                &cosmetic,
                &terms,
            ],
        )
        .await?
        .get(0);
    let other_id: Uuid = b["data"]["resourceId"].as_str().unwrap().parse()?;
    c.query_one("SELECT app.claim_stripe_creation($1)", &[&id])
        .await?;
    let ticket = purchase(&c, id).await?;
    let checkout = format!("cs_fixture{}", id.simple());
    let payment = format!("pi_fixture{}", id.simple());
    c.query_one("SELECT app.ack_stripe_checkout($1,$2,$3,'https://checkout.stripe.com/c/pay/fixture',clock_timestamp()+interval '1 hour',$4)", &[&id,&checkout,&payment,&ticket["termsHash"].as_str().unwrap()]).await?;
    let ticket = purchase(&c, id).await?;
    let body_hash = key(&id.to_string());
    let capture = json!({"version":1,"kind":"webhook","bodyHash":body_hash,"apiVersion":"2026-10-28.endive","live":false,"targetId":checkout});
    let source: Value = c
        .query_one(
            "SELECT app.capture_stripe_event($1,'checkout.session.completed',$2,clock_timestamp())",
            &[&format!("evt_{}", id.simple()), &capture],
        )
        .await?
        .get(0);
    let event: Uuid = source["id"].as_str().unwrap().parse()?;
    c.query_one(
        "SELECT app.confirm_stripe_evidence($1,$2)",
        &[&event, &body_hash],
    )
    .await?;
    let proof = json!({"version":1,"kind":"paid","effectKey":key(&format!("paid:{}",key(&payment))),"termsHash":ticket["termsHash"],"sourceObjectKey":key(&checkout),"checkoutKey":key(&checkout),"paymentKey":key(&payment),"chargeKey":key(&format!("ch_{}",id.simple())),"paid":true,"amountMinor":"500","currency":"usd"});
    let sql = "SELECT app.commit_stripe_effect($1,$2,$3::text::numeric,$4,$5,$6)";
    for invalid in [None, Some("NaN"), Some("Infinity"), Some("-1"), Some("1.5")] {
        assert_eq!(
            c.query_one(sql, &[&id, &event, &invalid, &proof, &checkout, &payment])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INVALID_PARAMETER_VALUE)
        );
    }
    assert_eq!(purchase(&c, id).await?["state"], "pending");
    let revision = ticket["revision"].as_str().unwrap();
    let accepted: Value = c
        .query_one(sql, &[&id, &event, &revision, &proof, &checkout, &payment])
        .await?
        .get(0);
    let replay: Value = c
        .query_one(sql, &[&id, &event, &revision, &proof, &checkout, &payment])
        .await?
        .get(0);
    assert_eq!(accepted, replay);
    let denied = c
        .query_one(sql, &[&other_id, &event, &"0", &proof, &checkout, &payment])
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Some(&SqlState::CHECK_VIOLATION));
    assert_eq!(
        denied.as_db_error().unwrap().message(),
        "processed source belongs to another purchase"
    );
    c.batch_execute("RESET ROLE").await?;
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.cosmetic_grant WHERE account_id=$1 AND cosmetic_id=$2 AND state='active'", &[&actor,&cosmetic]).await?.get::<_,i64>(0),1);
    assert_eq!(c.query_one("SELECT count(*)::bigint FROM app.cosmetic_grant WHERE account_id=$1 AND cosmetic_id=$2", &[&other,&cosmetic]).await?.get::<_,i64>(0),0);
    Ok(())
}
