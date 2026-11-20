//! Raw private worker response controls; no live provider, SQL or settlement claim.
use super::*;
use std::error::Error as _;
const SLOT: &str = "/rest/v1/rpc/read_established_price_slot";
const EXTRACT: &str = "/rest/v1/rpc/read_prediction_extraction_page";
const MAINTENANCE: &str = "/rest/v1/rpc/read_prediction_maintenance_page";
const OBSERVE: &str = "/rest/v1/rpc/record_prediction_observation";
const PROBE_READ: &str = "/rest/v1/rpc/read_established_price_probe";
const PROBE: &str = "/rest/v1/rpc/record_price_probe";
fn has_cause<E: std::error::Error + 'static>(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(cause) = current {
        if cause.is::<E>() {
            return true;
        }
        current = cause.source();
    }
    false
}
fn id() -> Uuid {
    Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap()
}
fn invalid_ids() -> Vec<Value> {
    vec![
        json!(Uuid::nil()),
        json!(id().to_string().to_uppercase()),
        json!(id().simple().to_string()),
        json!([id()]),
        Value::Null,
    ]
}
fn selected_missing() -> Value {
    serde_json::to_value(missing()).unwrap()
}
fn slot() -> NaiveDate {
    time("2026-11-01T13:00:00Z").date_naive()
}
#[test]
fn private_pages_require_cursor_key_before_any_mutation() {
    let stage = Stage::new();
    for items in [json!([]), json!([id()])] {
        let mock = Mock::new(vec![Step::json(MAINTENANCE, json!({"items":items}))]);
        assert!(mock.client(&stage).maintain_predictions(None).is_err());
        assert_eq!(mock.finish().len(), 1);
    }
    for items in [
        json!([]),
        json!([{"marketId":id(),"printingRef":Uuid::new_v4()}]),
    ] {
        let mock = Mock::new(vec![
            Step::json(SLOT, selected_missing()),
            Step::json(EXTRACT, json!({"items":items})),
        ]);
        assert!(mock.client(&stage).extract_slot(slot()).is_err());
        assert_eq!(mock.finish().len(), 2);
    }
    // Explicit-null end of page remains valid for both actual consumer paths.
    let mock = Mock::new(vec![Step::json(
        MAINTENANCE,
        json!({"items":[],"nextCursor":null}),
    )]);
    assert_eq!(
        mock.client(&stage)
            .maintain_predictions(None)
            .unwrap()
            .processed,
        0
    );
    mock.finish();
    let mock = Mock::new(vec![
        Step::json(SLOT, selected_missing()),
        Step::json(EXTRACT, json!({"items":[],"nextCursor":null})),
    ]);
    assert_eq!(
        mock.client(&stage).extract_slot(slot()).unwrap().processed,
        0
    );
    mock.finish();
}
#[test]
fn extraction_rejects_noncanonical_or_nil_target_identities_before_observation() {
    let stage = Stage::new();
    for field in ["marketId", "printingRef"] {
        for bad in invalid_ids() {
            let mut target = json!({"marketId":id(),"printingRef":Uuid::new_v4()});
            target[field] = bad;
            let mock = Mock::new(vec![
                Step::json(SLOT, selected_missing()),
                Step::json(EXTRACT, json!({"items":[target],"nextCursor":null})),
            ]);
            assert!(mock.client(&stage).extract_slot(slot()).is_err());
            assert_eq!(mock.finish().len(), 2);
        }
    }
}
#[test]
fn maintenance_rejects_aliased_items_and_cursor_spelling_on_full_page() {
    let stage = Stage::new();
    for bad in invalid_ids() {
        let mock = Mock::new(vec![Step::json(
            MAINTENANCE,
            json!({"items":[bad],"nextCursor":null}),
        )]);
        assert!(mock.client(&stage).maintain_predictions(None).is_err());
        assert_eq!(mock.finish().len(), 1);
    }
    let start = id().as_u128();
    let ids: Vec<_> = (0..100).map(|n| Uuid::from_u128(start + n)).collect();
    let last = *ids.last().unwrap();
    for cursor in [
        json!(last.to_string().to_uppercase()),
        json!(last.simple().to_string()),
        json!(Uuid::nil()),
    ] {
        let mock = Mock::new(vec![Step::json(
            MAINTENANCE,
            json!({"items":ids,"nextCursor":cursor}),
        )]);
        assert!(mock.client(&stage).maintain_predictions(None).is_err());
        assert_eq!(mock.finish().len(), 1);
    }
}
#[test]
fn scalar_observation_ack_cannot_report_success_for_invalid_identity() {
    let stage = Stage::new();
    for ack in invalid_ids() {
        let mock = Mock::new(vec![
            Step::json(SLOT, selected_missing()),
            Step::json(
                EXTRACT,
                json!({"items":[{"marketId":id(),"printingRef":Uuid::new_v4()}],"nextCursor":null}),
            ),
            Step::json(OBSERVE, ack),
        ]);
        assert!(mock.client(&stage).extract_slot(slot()).is_err());
        let calls = mock.finish();
        assert_eq!(calls.iter().filter(|(p, _, _)| p == OBSERVE).count(), 1);
        // This is a post-submit refusal, not a promise that a remote write rolled back.
    }
}
#[test]
fn scalar_probe_ack_and_recovery_require_canonical_identity() {
    let stage = Stage::new();
    let (candidate, selected) = fresh_probe_source(&stage);
    let SlotManifest::Valid(manifest) = &selected.manifest else {
        panic!()
    };
    let day = manifest.slot;
    let printing = Uuid::parse_str(card_in(&candidate)["id"].as_str().unwrap()).unwrap();
    for ack in invalid_ids() {
        let mut steps = vec![
            Step::json(SLOT, json!(selected)),
            Step::json(PROBE_READ, Value::Null),
        ];
        steps.extend(selected_downloads(&candidate));
        steps.push(Step::json(PROBE, ack));
        let mock = Mock::new(steps);
        assert!(mock.client(&stage).probe_printing(day, printing).is_err());
        let calls = mock.finish();
        assert_eq!(calls.iter().filter(|(p, _, _)| p == PROBE).count(), 1);
    }
    for ack in invalid_ids().into_iter().filter(|v| !v.is_null()) {
        let mock = Mock::new(vec![
            Step::json(SLOT, json!(selected)),
            Step::json(PROBE_READ, ack),
        ]);
        assert!(mock.client(&stage).probe_printing(day, printing).is_err());
        assert_eq!(mock.finish().len(), 2);
    }
    let mock = Mock::new(vec![
        Step::json(SLOT, json!(selected)),
        Step::json(PROBE_READ, json!(id())),
    ]);
    assert_eq!(
        mock.client(&stage).probe_printing(day, printing).unwrap(),
        id()
    );
    assert_eq!(mock.finish().len(), 2);
}
#[test]
fn established_slot_and_final_operation_reject_aliases_or_nil() {
    let stage = Stage::new();
    for bad in invalid_ids() {
        let mut selected = selected_missing();
        selected["id"] = bad.clone();
        let mock = Mock::new(vec![Step::json(SLOT, selected)]);
        assert!(mock.client(&stage).read_slot(slot()).is_err());
        mock.finish();
        let mock = Mock::new(vec![
            Step::json(MAINTENANCE, json!({"items":[id()],"nextCursor":null})),
            Step::json(
                "/rest/v1/rpc/maintain_prediction_market",
                json!({"state":"final","outcome":"yes","operationId":bad}),
            ),
        ]);
        assert!(mock.client(&stage).maintain_predictions(None).is_err());
        mock.finish();
    }
}
#[test]
fn malformed_slot_ack_recovers_authoritative_winner_without_resubmission() {
    let stage = Stage::new();
    let winner = missing();
    for ack in invalid_ids() {
        let mock = Mock::new(vec![
            Step::json("/rest/v1/rpc/commit_price_slot", ack),
            Step::json(SLOT, json!(winner)),
        ]);
        assert_eq!(
            mock.client(&stage).decide(slot(), None).unwrap().id,
            winner.id
        );
        assert_eq!(mock.finish().len(), 2);
    }
}
#[test]
fn unconfirmed_slot_without_original_preserves_initial_rpc_cause() {
    let stage = Stage::new();
    let mut lost = Step::json("/rest/v1/rpc/commit_price_slot", Value::Null);
    lost.drop_ack = true;
    let mock = Mock::new(vec![lost, Step::json(SLOT, Value::Null)]);
    let error = mock.client(&stage).decide(slot(), None).unwrap_err();
    assert!(error.source().is_some());
    assert!(error.source().unwrap().source().is_some());
    assert!(
        has_cause::<reqwest::Error>(&error),
        "actual send error must survive every wrapper"
    );
    assert_eq!(
        error.to_string(),
        "price collector source disposition could not be established"
    );
    mock.finish();
    let mut broken = Step::json(MAINTENANCE, Value::Null);
    broken.body = b"not-json-private-body-marker".to_vec();
    let mock = Mock::new(vec![broken]);
    let error = mock.client(&stage).maintain_predictions(None).unwrap_err();
    assert!(error.source().is_some());
    assert!(
        has_cause::<serde_json::Error>(&error),
        "actual codec error must survive every wrapper"
    );
    assert!(!format!("{error:?} {error}").contains("private-body-marker"));
    mock.finish();
}
#[test]
fn workflow_batch_cursor_is_required_nullable_not_omitted() {
    let valid = json!({"processed":0,"settled":0,"nextCursor":null,"more":false});
    assert!(serde_json::from_value::<maintenance::MaintenanceBatch>(valid.clone()).is_ok());
    let mut omitted = valid.clone();
    omitted.as_object_mut().unwrap().remove("nextCursor");
    assert!(serde_json::from_value::<maintenance::MaintenanceBatch>(omitted).is_err());
    for bad in invalid_ids().into_iter().filter(|v| !v.is_null()) {
        let mut value = valid.clone();
        value["nextCursor"] = bad;
        assert!(serde_json::from_value::<maintenance::MaintenanceBatch>(value).is_err());
    }
}
