//! Controlled provider profiles exercise the actual reconciler; database tests
//! separately verify that its normalized proofs create immutable grants.
use super::*;
use crate::{
    stripe::test_fixture::{checkout, price},
    test_http::{Mock, Step},
};
use std::collections::BTreeMap;

const SECRET: &str = "sk_test_spellwake_synthetic_only";

#[test]
fn expiry_rejects_unbound_hash_and_locator_before_storage_or_finish() {
    let hash = "a".repeat(64);
    for row in [
        json!({"payloadRef":format!("payments/stripe/{}.json","b".repeat(64)),"bodyHash":hash}),
        json!({"payloadRef":format!("payments/stripe/{hash}.json"),"bodyHash":"A".repeat(64)}),
        json!({"payloadRef":format!("payments/stripe/../{hash}.json"),"bodyHash":hash}),
        json!({"payloadRef":"payments/stripe/other.json","bodyHash":"invalid"}),
    ] {
        let backend = Mock::new(vec![Step::json(
            "/rest/v1/rpc/begin_stripe_evidence_expiry",
            row,
        )]);
        let provider = Mock::new(vec![]);
        assert!(
            private_fixture(&backend, &provider)
                .expire_evidence(Uuid::new_v4())
                .is_err()
        );
        assert_eq!(backend.finish().len(), 1);
        assert!(provider.finish().is_empty());
    }
}

#[test]
fn notification_acknowledgments_are_typed_on_the_actual_backend_path() {
    let event = Uuid::new_v4();
    let purchase = Uuid::new_v4();
    for (name, key) in [
        ("associate_stripe_notification", "associated"),
        ("ignore_stripe_notification", "ignored"),
    ] {
        for (row, expected) in [
            (json!({key:true}), true),
            (json!({key:false}), false),
            (json!({key:"true"}), false),
            (json!([true]), false),
            (json!({key:true,"extra":true}), false),
        ] {
            let backend = Mock::new(vec![Step::json(&format!("/rest/v1/rpc/{name}"), row)]);
            let provider = Mock::new(vec![]);
            let service = private_fixture(&backend, &provider);
            let result = if key == "associated" {
                service.associate_notification(event, purchase)
            } else {
                service.ignore_notification(event)
            };
            assert_eq!(result.is_ok(), expected);
            assert_eq!(backend.finish().len(), 1);
            assert!(provider.finish().is_empty());
        }
    }
    let original = Uuid::new_v4();
    let fresh_retry = Uuid::new_v4();
    for (row, expected) in [
        (
            json!({"kind":"observed","observationId":original,"purchaseId":purchase}),
            true,
        ),
        (
            json!({"kind":"observed","observationId":original,"purchaseId":Uuid::new_v4()}),
            false,
        ),
        (
            json!({"kind":"observed","observationId":null,"purchaseId":purchase}),
            false,
        ),
        (json!({"kind":"unsupported"}), false),
        (
            json!({"kind":{"observed":null},"observationId":original,"purchaseId":purchase}),
            false,
        ),
    ] {
        let backend = Mock::new(vec![Step::json(
            "/rest/v1/rpc/complete_stripe_notification",
            row,
        )]);
        let provider = Mock::new(vec![]);
        assert_eq!(
            private_fixture(&backend, &provider)
                .complete_notification(event, fresh_retry, purchase)
                .is_ok(),
            expected
        );
        let seen = backend.finish();
        assert_eq!(
            serde_json::from_slice::<Value>(&seen[0].2).unwrap(),
            json!({"p_event":event,"p_observation":fresh_retry})
        );
        assert!(provider.finish().is_empty());
    }
}

fn private_fixture(backend: &Mock, provider: &Mock) -> StripeService {
    StripeService::new(
        BackendConfig::new(&backend.base, "sb_secret_test_only").unwrap(),
        StripeConfig::new(
            SECRET.into(),
            "whsec_spellwake_synthetic_only".into(),
            "acct_synthetic".into(),
            BTreeMap::from([(Uuid::new_v4(), "price_synthetic".into())]),
            "https://app.example/return".into(),
            "https://app.example/cancel".into(),
            &provider.base,
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn private_objects_stripe_purchase_and_terms_arrays_cannot_reach_checkout_reads() {
    let id = Uuid::new_v4();
    let terms = json!({"version":1,"priceId":"price_synthetic","currency":"usd","minorUnits":"500","live":false,"apiVersion":API_VERSION,"successUrl":"https://app.example/return","cancelUrl":"https://app.example/cancel"});
    let ticket = json!({"id":id,"accountId":Uuid::new_v4(),"accountState":"active","cosmeticId":Uuid::new_v4(),"state":"pending","revision":"1","terms":terms,"termsHash":"a".repeat(64),"requestKey":format!("spellwake-purchase-{id}"),"checkoutRef":"cs_test_synthetic","paymentRef":null,"deadline":null,"reversalEventId":null,"reversalKey":null});
    let mut nested = ticket.clone();
    nested["terms"] = crate::test_http::positional(
        &terms,
        &[
            "version",
            "priceId",
            "currency",
            "minorUnits",
            "live",
            "apiVersion",
            "successUrl",
            "cancelUrl",
        ],
    );
    let duplicate = ticket.to_string().replace(
        "\"priceId\":",
        "\"priceId\":\"price_synthetic\",\"priceId\":",
    );
    for ticket in [
        crate::test_http::positional(
            &ticket,
            &[
                "id",
                "accountId",
                "accountState",
                "cosmeticId",
                "state",
                "revision",
                "terms",
                "termsHash",
                "requestKey",
                "checkoutRef",
                "paymentRef",
                "deadline",
                "reversalEventId",
                "reversalKey",
            ],
        ),
        nested,
    ]
    .into_iter()
    .map(|value| value.to_string())
    .chain(std::iter::once(duplicate))
    {
        let mut step = Step::json("/rest/v1/rpc/read_stripe_purchase", Value::Null);
        step.body = ticket.into_bytes();
        let backend = Mock::new(vec![step]);
        // The existing Stripe constructor verifies the configured merchant first.
        // This witness forbids subsequent purchase-specific checkout/payment requests.
        let provider = Mock::new(vec![get(
            "/v1/account",
            json!({"id":"acct_synthetic","object":"account"}),
        )]);
        assert!(private_fixture(&backend, &provider).reconcile(id).is_err());
        assert_eq!(backend.finish().len(), 1);
        assert_eq!(provider.finish().len(), 1);
    }
}

#[test]
fn private_objects_stripe_notification_and_expiry_records_precede_effects() {
    let id = Uuid::new_v4();
    for row in [
        json!(["checkout.session.completed",{"targetId":"cs_test_synthetic"}]),
        json!({"eventType":"unhandled","capture":[null]}),
    ] {
        let backend = Mock::new(vec![Step::json(
            "/rest/v1/rpc/read_stripe_notification",
            row,
        )]);
        let provider = Mock::new(vec![]);
        assert!(
            private_fixture(&backend, &provider)
                .process_notification(id)
                .is_err()
        );
        assert_eq!(backend.finish().len(), 1);
        assert!(provider.finish().is_empty());
    }
    let backend = Mock::new(vec![Step::json(
        "/rest/v1/rpc/begin_stripe_evidence_expiry",
        json!([
            format!("payments/stripe/{}.json", "a".repeat(64)),
            "a".repeat(64)
        ]),
    )]);
    let provider = Mock::new(vec![]);
    assert!(
        private_fixture(&backend, &provider)
            .expire_evidence(id)
            .is_err()
    );
    assert_eq!(backend.finish().len(), 1);
    assert!(provider.finish().is_empty());
}

#[test]
fn private_objects_effect_results_bind_exact_sql_revision_transitions() {
    let id = Uuid::new_v4();
    for (disposition, revision, expected) in [
        ("applied", "2", true),
        ("applied", "1", false),
        ("duplicate", "1", true),
        ("duplicate", "2", false),
        ("noop", "1", true),
        ("noop", "2", false),
        ("stale", "1", false),
        ("stale", "2", false),
    ] {
        let backend = Mock::new(vec![Step::json(
            "/rest/v1/rpc/commit_stripe_effect",
            json!({"disposition":disposition,"purchaseId":id,"revision":revision,"state":"paid","grantId":null}),
        )]);
        let provider = Mock::new(vec![]);
        let value: EffectResult = private_fixture(&backend, &provider)
            .backend
            .rpc_object("commit_stripe_effect", &json!({}))
            .unwrap();
        assert_eq!(value.matches(id, "1"), expected);
        assert_eq!(backend.finish().len(), 1);
        assert!(provider.finish().is_empty());
    }
    let valid = json!({"disposition":"applied","purchaseId":id,"revision":"2","state":"paid","grantId":null});
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("grantId");
    let mut noncanonical = valid.clone();
    noncanonical["revision"] = json!("02");
    for body in [
        crate::test_http::positional(
            &valid,
            &["disposition", "purchaseId", "revision", "state", "grantId"],
        ),
        missing,
        noncanonical,
    ] {
        let backend = Mock::new(vec![Step::json("/rest/v1/rpc/commit_stripe_effect", body)]);
        let provider = Mock::new(vec![]);
        assert!(
            private_fixture(&backend, &provider)
                .backend
                .rpc_object::<EffectResult>("commit_stripe_effect", &json!({}))
                .is_err()
        );
        assert_eq!(backend.finish().len(), 1);
        assert!(provider.finish().is_empty());
    }
    let mut duplicate = Step::json("/rest/v1/rpc/commit_stripe_effect", Value::Null);
    duplicate.body = valid
        .to_string()
        .replace(
            "\"purchaseId\":",
            &format!("\"purchaseId\":\"{id}\",\"purchaseId\":"),
        )
        .into_bytes();
    let backend = Mock::new(vec![duplicate]);
    let provider = Mock::new(vec![]);
    assert!(
        private_fixture(&backend, &provider)
            .backend
            .rpc_object::<EffectResult>("commit_stripe_effect", &json!({}))
            .is_err()
    );
    assert_eq!(backend.finish().len(), 1);
    assert!(provider.finish().is_empty());
    let huge = "9007199254740993";
    let value: EffectResult = serde_json::from_value(json!({"disposition":"applied","purchaseId":id,"revision":"9007199254740994","state":"paid","grantId":null})).unwrap();
    assert!(value.matches(id, huge));
    assert!(!value.matches(Uuid::new_v4(), huge));
}

#[test]
fn canonical_rejections_bind_request_and_product_but_authority_failures_do_not() {
    let request = Uuid::new_v4();
    let cosmetic = Uuid::new_v4();
    for field in ["cosmetic", "account"] {
        let response = settled_reservation_response(
            ApiResponse::Fail {
                data: BTreeMap::from([(field.into(), "unavailable".into())]),
            },
            request,
            cosmetic,
        );
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({"status":"fail","data":{"request":"terminal_rejection","requestId":request,"cosmeticId":cosmetic,"reason":format!("{field}_unavailable")}})
        );
    }
    for (field, reason) in [
        ("session", "unauthorized"),
        ("request", "conflict"),
        ("provider", "unavailable"),
    ] {
        let response = ApiResponse::Fail {
            data: BTreeMap::from([(field.into(), reason.into())]),
        };
        assert_eq!(
            settled_reservation_response(response.clone(), request, cosmetic),
            response
        );
    }
    let response = ApiResponse::Success {
        data: CommandAck {
            resource_id: Uuid::new_v4(),
        },
    };
    assert_eq!(
        settled_reservation_response(response.clone(), request, cosmetic),
        response
    );
}
fn get(path: &str, value: Value) -> Step {
    let mut step = Step::get(path.into(), 200, serde_json::to_vec(&value).unwrap());
    step.credentials = crate::test_http::Credentials::Stripe(SECRET.into());
    step
}
fn run(state: &str, refund: bool, won: bool, wrong_return: bool) -> Option<Value> {
    run_with_disputes(state, refund, won, wrong_return, None, false)
}
fn run_with_disputes(
    state: &str,
    refund: bool,
    won: bool,
    wrong_return: bool,
    second_loss_first: Option<bool>,
    stale_commit: bool,
) -> Option<Value> {
    let id = Uuid::new_v4();
    let cosmetic = Uuid::new_v4();
    let reversal = Uuid::new_v4();
    let mut session = checkout(id);
    session["status"] = json!("complete");
    session["payment_status"] = json!("paid");
    session["payment_intent"] = json!("pi_synthetic");
    let mut provider = vec![
        get(
            "/v1/account",
            json!({"id":"acct_synthetic","object":"account"}),
        ),
        get("/v1/checkout/sessions/cs_test_synthetic", session),
        get(
            "/v1/checkout/sessions/cs_test_synthetic/line_items?limit=2",
            json!({"object":"list","has_more":false,"data":[{"id":"li_synthetic","quantity":1,"price":price()}]}),
        ),
        get(
            "/v1/payment_intents/pi_synthetic",
            json!({"id":"pi_synthetic","object":"payment_intent","livemode":false,"status":"succeeded","currency":"usd","amount":500,"amount_received":500,"latest_charge":"ch_synthetic","metadata":{"spellwake_purchase_id":id}}),
        ),
        get(
            "/v1/charges/ch_synthetic",
            json!({"id":"ch_synthetic","object":"charge","livemode":false,"currency":"usd","amount":500,"amount_captured":500,"amount_refunded":0,"paid":true,"captured":true,"payment_intent":"pi_synthetic"}),
        ),
        get(
            "/v1/refunds?payment_intent=pi_synthetic&limit=100",
            json!({"object":"list","has_more":false,"data":if refund {vec![json!({"id":"re_synthetic","object":"refund","payment_intent":"pi_synthetic","charge":"ch_synthetic","status":"failed","amount":500,"currency":"usd","failure_balance_transaction":"txn_return"})]} else {vec![]}}),
        ),
    ];
    if refund {
        provider.push(get("/v1/balance_transactions/txn_return", json!({"id":"txn_return","object":"balance_transaction","source":if wrong_return {"re_unrelated"} else {"re_synthetic"},"type":"refund_failure","amount":500,"currency":"usd"})));
    }
    if !wrong_return {
        let mut data = if refund {
            vec![]
        } else {
            vec![
                json!({"id":"du_synthetic","object":"dispute","livemode":false,"payment_intent":"pi_synthetic","charge":"ch_synthetic","status":if won {"won"} else {"lost"},"balance_transactions":if won {vec![json!({"id":"txn_loss","currency":"usd","amount":-500,"created":1}),json!({"id":"txn_credit","currency":"usd","amount":500,"created":2})]} else {vec![json!({"id":"txn_loss","currency":"usd","amount":-500,"created":1})]}}),
            ]
        };
        if let Some(first) = second_loss_first {
            let loss = json!({"id":"du_other","object":"dispute","livemode":false,"payment_intent":"pi_synthetic","charge":"ch_synthetic","status":"lost","balance_transactions":[{"id":"txn_other_loss","amount":-500,"currency":"usd","created":3}]});
            if first {
                data.insert(0, loss);
            } else {
                data.push(loss);
            }
        }
        provider.push(get(
            "/v1/disputes?payment_intent=pi_synthetic&limit=100",
            json!({"object":"list","has_more":false,"data":data}),
        ));
    }
    let provider = Mock::new(provider);
    let terms = json!({"version":1,"priceId":"price_synthetic","currency":"usd","minorUnits":"500","apiVersion":API_VERSION,"live":false,"successUrl":"https://app.example/return","cancelUrl":"https://app.example/cancel"});
    let ticket = json!({"id":id,"accountId":Uuid::new_v4(),"accountState":"active","cosmeticId":cosmetic,"state":state,"revision":"1","terms":terms,"termsHash":"a".repeat(64),"requestKey":format!("spellwake-purchase-{id}"),"checkoutRef":"cs_test_synthetic","paymentRef":"pi_synthetic","deadline":null,"reversalEventId":reversal,"reversalKey":hash(if refund {"re_synthetic"} else {"txn_loss"})});
    let mut steps = vec![Step::json("/rest/v1/rpc/read_stripe_purchase", ticket)];
    if !wrong_return {
        steps.push(Step::json("/rest/v1/rpc/capture_stripe_event", json!({"id":Uuid::new_v4(),"evidenceState":"none","payloadRef":"test-only:api","expiresAt":Utc::now(),"processed":false})));
        steps.push(Step::json(
            "/rest/v1/rpc/commit_stripe_effect",
            json!({"disposition":if stale_commit {"stale"} else {"applied"},"purchaseId":id,"revision":"2","state":state,"grantId":null}),
        ));
    }
    let backend = Mock::new(steps);
    let config = StripeConfig::new(
        SECRET.into(),
        "whsec_spellwake_synthetic_only".into(),
        "acct_synthetic".into(),
        BTreeMap::from([(cosmetic, "price_synthetic".into())]),
        "https://app.example/return".into(),
        "https://app.example/cancel".into(),
        &provider.base,
    )
    .unwrap();
    let service = StripeService::new(
        BackendConfig::new(&backend.base, "sb_secret_test_only").unwrap(),
        config,
    )
    .unwrap();
    let result = service.reconcile(id);
    provider.finish();
    let calls = backend.finish();
    if wrong_return || stale_commit {
        assert!(result.is_err());
        None
    } else {
        assert!(result.is_ok());
        Some(serde_json::from_slice::<Value>(&calls.last().unwrap().2).unwrap()["p_effect"].clone())
    }
}
#[test]
fn first_won_observation_grants_once_and_remembers_the_settled_debit() {
    let proof = run("pending", false, true, false).unwrap();
    assert_eq!(proof["kind"], "paid");
    assert_eq!(proof["settledReversalKey"], hash("txn_loss"));
    assert_eq!(proof["fundsReturned"], true);
}
#[test]
fn verified_dispute_and_failed_refund_returns_produce_new_grant_proofs() {
    for (state, refund) in [("reversed", false), ("refunded", true)] {
        let proof = run(state, refund, true, false).unwrap();
        assert_eq!(proof["kind"], "restored");
        assert_eq!(proof["fundsReturned"], true);
        assert_eq!(proof["refundMinorUnits"], "0");
        assert!(proof["reversalEventId"].is_string());
    }
    assert!(run("refunded", true, true, true).is_none());
}
#[test]
fn actual_dispute_loss_produces_revocation_not_a_payment_grant() {
    let proof = run("paid", false, false, false).unwrap();
    assert_eq!(proof["kind"], "reversed");
    assert_eq!(proof["movementKey"], hash("txn_loss"));
}

#[test]
fn unresolved_loss_overrides_a_won_dispute_in_either_provider_order() {
    for first in [false, true] {
        let proof = run_with_disputes("paid", false, true, false, Some(first), false).unwrap();
        assert_eq!(proof["kind"], "reversed");
        assert_eq!(proof["movementKey"], hash("txn_other_loss"));
        assert_eq!(proof["disputeKey"], hash("du_other"));
        for field in [
            "settledReversalKey",
            "reversalKey",
            "reversalEventId",
            "refundMinorUnits",
            "fundsReturned",
        ] {
            assert!(
                proof.get(field).is_none(),
                "stale restoration field {field}"
            );
        }
    }
}

#[test]
fn superseded_observation_requires_a_fresh_read_before_completing_notification() {
    assert!(run_with_disputes("paid", false, false, false, None, true).is_none());
}

#[test]
fn private_effect_rpc_rejects_nested_object_and_array_dispositions() {
    for bad in [json!({"applied":null}), json!(["applied"])] {
        let body = json!({"disposition":bad,"purchaseId":Uuid::new_v4(),"revision":"2","state":"paid","grantId":null});
        let backend = Mock::new(vec![Step::json("/rest/v1/rpc/commit_stripe_effect", body)]);
        let provider = Mock::new(vec![]);
        assert!(
            private_fixture(&backend, &provider)
                .backend
                .rpc_object::<EffectResult>("commit_stripe_effect", &json!({}))
                .is_err()
        );
        // This validates an unresolved response, not absence of an upstream SQL effect.
        assert_eq!(backend.finish().len(), 1);
        assert!(provider.finish().is_empty());
    }
}
