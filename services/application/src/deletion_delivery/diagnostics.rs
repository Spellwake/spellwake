use super::*;
use crate::test_http::{Mock, Step};
use std::error::Error as _;

fn client(mock: &Mock) -> DeletionDelivery {
    DeletionDelivery::new(BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap()).unwrap()
}

#[test]
fn backend_causes_keep_identity_across_clones_without_changing_classification() {
    let marker = "private-deletion-diagnostic sb_secret_fixture_only";
    let error = DeliveryError::from(BackendError::caused("body", std::io::Error::other(marker)));
    let cloned = error.clone();
    assert_eq!(error, DeliveryError::Backend);
    assert_eq!(cloned, error);
    assert_ne!(error, DeliveryError::Canceled);
    assert_ne!(error, DeliveryError::Configuration);
    let original = error.source().unwrap().source().unwrap();
    let copied = cloned.source().unwrap().source().unwrap();
    assert!(std::ptr::eq(original, copied));
    assert_eq!(
        original.downcast_ref::<std::io::Error>().unwrap().kind(),
        std::io::ErrorKind::Other
    );
    assert_eq!(original.to_string(), marker);
    assert!(!format!("{error:?} {error} {cloned:?}").contains(marker));
    for error in [
        DeliveryError::Backend,
        DeliveryError::Configuration,
        DeliveryError::Canceled,
    ] {
        assert!(error.source().is_none());
    }
}

#[test]
fn actual_page_decoder_failure_retains_serde_cause_before_any_cleanup() {
    let marker = "private-deletion-page-body sb_secret_fixture_only";
    let mock = Mock::new(vec![Step::json(
        "/rest/v1/rpc/read_deletion_account_page",
        json!({"items":marker,"nextCursor":null}),
    )]);
    let error = client(&mock).deliver(None).unwrap_err();
    assert_eq!(error, DeliveryError::Backend);
    let cause = error.source().unwrap().source().unwrap();
    assert!(cause.downcast_ref::<serde_json::Error>().is_some());
    assert!(cause.to_string().contains(marker));
    assert!(!format!("{error:?} {error}").contains(marker));
    assert_eq!(mock.finish().len(), 1);
}

#[test]
fn actual_mid_account_decoder_failure_preserves_retry_batch_and_cursor() {
    let actor = Uuid::new_v4();
    let mock = Mock::new(vec![
        Step::json(
            "/rest/v1/rpc/read_deletion_account_page",
            json!({"items":[actor],"nextCursor":actor}),
        ),
        Step::json(
            "/rest/v1/rpc/read_account_retirement_work",
            json!({"state":"private-unrecognized-retirement-state"}),
        ),
    ]);
    let batch = client(&mock).deliver(None).unwrap();
    assert!(batch.retry_required && batch.more);
    assert_eq!(batch.next_cursor, Some(actor));
    assert_eq!((batch.attempted, batch.acknowledged), (0, 0));
    assert_eq!(mock.finish().len(), 2);
}

#[test]
fn actual_page_transport_failure_keeps_original_url_free_reqwest_cause() {
    let mut step = Step::json("/rest/v1/rpc/read_deletion_account_page", json!({}));
    step.drop_ack = true;
    let mock = Mock::new(vec![step]);
    let error = client(&mock).deliver(None).unwrap_err();
    assert_eq!(error, DeliveryError::Backend);
    let cause = error.source().unwrap().source().unwrap();
    let request = cause.downcast_ref::<reqwest::Error>().unwrap();
    assert!(request.url().is_none());
    assert!(!format!("{error:?} {error}").contains(&mock.base));
    assert_eq!(mock.finish().len(), 1);
}
