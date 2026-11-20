use super::*;
use crate::stripe::test_fixture::{checkout, price};
use crate::test_http::{Mock, Step};
use serde_json::{Value, json};

const SECRET: &str = "sk_test_spellwake_synthetic_only";
fn get(path: &str, value: Value) -> Step {
    let mut step = Step::get(path.into(), 200, serde_json::to_vec(&value).unwrap());
    step.credentials = crate::test_http::Credentials::Stripe(SECRET.into());
    step
}
fn post(path: &str, value: Value) -> Step {
    let mut step = Step::json(path, value);
    step.credentials = crate::test_http::Credentials::Stripe(SECRET.into());
    step
}
fn merchant() -> Step {
    get(
        "/v1/account",
        json!({"id":"acct_synthetic","object":"account"}),
    )
}
fn config(base: &str, cosmetic: Uuid) -> StripeConfig {
    StripeConfig::new(
        SECRET.into(),
        "whsec_spellwake_synthetic_only".into(),
        "acct_synthetic".into(),
        BTreeMap::from([(cosmetic, "price_synthetic".into())]),
        "https://app.example/return".into(),
        "https://app.example/cancel".into(),
        base,
    )
    .unwrap()
}
fn ticket(id: Uuid, cosmetic: Uuid, terms: CheckoutTerms) -> Purchase {
    serde_json::from_value(json!({
 "id":id,"accountId":Uuid::new_v4(),"accountState":"active","cosmeticId":cosmetic,"state":"creating","revision":"1","terms":terms,"termsHash":"a".repeat(64),"requestKey":format!("spellwake-purchase-{id}"),"checkoutRef":null,"paymentRef":null,"deadline":Utc::now()+chrono::Duration::hours(23),"reversalEventId":null,"reversalKey":null
})).unwrap()
}
fn lines() -> Step {
    get(
        "/v1/checkout/sessions/cs_test_synthetic/line_items?limit=2",
        json!({"object":"list","has_more":false,"data":[{"id":"li_synthetic","quantity":1,"price":price()}]}),
    )
}

#[test]
fn checkout_uses_merchant_bound_private_credentials_and_exact_persisted_form() {
    let id = Uuid::new_v4();
    let cosmetic = Uuid::new_v4();
    let mock = Mock::new(vec![
        merchant(),
        get("/v1/prices/price_synthetic", price()),
        post("/v1/checkout/sessions", checkout(id)),
        lines(),
    ]);
    let client = StripeClient::new(config(&mock.base, cosmetic)).unwrap();
    let terms = client.terms(cosmetic).unwrap();
    assert_eq!(terms.minor_units, "500");
    let ticket = ticket(id, cosmetic, terms);
    let result = client.create(&ticket).unwrap();
    assert_eq!(result.id, "cs_test_synthetic");
    let seen = mock.finish();
    let (_, headers, body) = &seen[2];
    assert_eq!(headers.get("idempotency-key"), Some(&ticket.request_key));
    let parsed = reqwest::Url::parse(&format!(
        "https://fixture.invalid/?{}",
        String::from_utf8(body.clone()).unwrap()
    ))
    .unwrap();
    let form: BTreeMap<String, String> = parsed.query_pairs().into_owned().collect();
    assert_eq!(form.get("mode").map(String::as_str), Some("payment"));
    assert_eq!(
        form.get("line_items[0][price]").map(String::as_str),
        Some("price_synthetic")
    );
    assert_eq!(
        form.get("line_items[0][quantity]").map(String::as_str),
        Some("1")
    );
    assert_eq!(form.get("client_reference_id"), Some(&id.to_string()));
    assert_eq!(
        form.get("payment_intent_data[metadata][spellwake_purchase_id]"),
        Some(&id.to_string())
    );
    assert_eq!(form.len(), 8);
    assert!(!headers.contains_key("apikey"));
    assert!(!headers.contains_key("stripe-account"));
}

#[test]
fn unknown_create_ack_reuses_same_key_and_params_and_deadline_stops_new_dispatch() {
    let id = Uuid::new_v4();
    let cosmetic = Uuid::new_v4();
    let mut dropped = post("/v1/checkout/sessions", json!({}));
    dropped.drop_ack = true;
    let mock = Mock::new(vec![
        merchant(),
        get("/v1/prices/price_synthetic", price()),
        dropped,
        post("/v1/checkout/sessions", checkout(id)),
        lines(),
    ]);
    let client = StripeClient::new(config(&mock.base, cosmetic)).unwrap();
    let terms = client.terms(cosmetic).unwrap();
    let mut ticket = ticket(id, cosmetic, terms);
    assert!(matches!(
        client.create(&ticket),
        Err(ClientError::Network(_))
    ));
    assert!(client.create(&ticket).is_ok());
    ticket.deadline = Some(Utc::now() - chrono::Duration::seconds(1));
    assert!(matches!(client.create(&ticket), Err(ClientError::Deadline)));
    let seen = mock.finish();
    assert_eq!(seen.len(), 5);
    assert_eq!(seen[2].2, seen[3].2);
    assert_eq!(
        seen[2].1.get("idempotency-key"),
        seen[3].1.get("idempotency-key")
    );
}

#[test]
fn wrong_merchant_redirect_and_private_shape_values_cannot_become_provider_proof() {
    let cosmetic = Uuid::new_v4();
    let wrong = Mock::new(vec![get(
        "/v1/account",
        json!({"id":"acct_other","object":"account"}),
    )]);
    assert!(matches!(
        StripeClient::new(config(&wrong.base, cosmetic)),
        Err(ClientError::Contract)
    ));
    wrong.finish();
    let mut redirect = merchant();
    redirect.status = 302;
    redirect
        .response_headers
        .push(("Location".into(), "https://example.invalid/steal".into()));
    let redirect = Mock::new(vec![redirect]);
    assert!(matches!(
        StripeClient::new(config(&redirect.base, cosmetic)),
        Err(ClientError::Status(302))
    ));
    redirect.finish();
    let mut poisoned = price();
    poisoned["active"] = json!("PRIVATE_BILLING_CANARY");
    let mock = Mock::new(vec![
        merchant(),
        get("/v1/prices/price_synthetic", poisoned),
    ]);
    let client = StripeClient::new(config(&mock.base, cosmetic)).unwrap();
    let error = match client.terms(cosmetic) {
        Err(e) => e,
        Ok(_) => panic!("invalid source accepted"),
    };
    let mut cause: Option<&dyn Error> = Some(&error);
    while let Some(error) = cause {
        assert!(!format!("{error:?} {error}").contains("PRIVATE_BILLING_CANARY"));
        cause = error.source();
    }
    mock.finish();
    let id = Uuid::new_v4();
    let mut wrong_origin = checkout(id);
    wrong_origin["url"] = json!("https://checkout.stripe.com:444/c/pay/synthetic");
    let mock = Mock::new(vec![
        merchant(),
        get("/v1/prices/price_synthetic", price()),
        post("/v1/checkout/sessions", wrong_origin),
        lines(),
    ]);
    let client = StripeClient::new(config(&mock.base, cosmetic)).unwrap();
    let terms = client.terms(cosmetic).unwrap();
    assert!(matches!(
        client.create(&ticket(id, cosmetic, terms)),
        Err(ClientError::Contract)
    ));
    mock.finish();
}
