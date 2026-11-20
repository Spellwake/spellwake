use super::*;
use crate::backend::BackendError;
use std::error::Error as _;
use temporalio_common::{
    data_converters::{
        ActivitySerializationContext, DefaultFailureConverter, FailureConverter, PayloadConverter,
        SerializationContextData,
    },
    error::{OutgoingActivityError, OutgoingError},
    protos::temporal::api::failure::v1::{Failure, failure::FailureInfo},
};

fn encode(error: ActivityError) -> Failure {
    let ActivityError::Application(failure) = error else {
        panic!("expected application failure")
    };
    DefaultFailureConverter::default().to_failure(
        OutgoingError::Activity(OutgoingActivityError::Application(failure)),
        &PayloadConverter::default(),
        &SerializationContextData::Activity(ActivitySerializationContext::new()),
    )
}

#[test]
fn actual_sdk_encoding_keeps_deletion_diagnostics_local_and_retryability_unchanged() {
    let marker =
        "private-deletion-body sb_secret_fixture_only https://private.invalid/?subject=fixture";
    let error = DeliveryError::from(BackendError::caused("json", std::io::Error::other(marker)));
    assert!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .to_string()
            .contains(marker)
    );
    assert!(!format!("{error:?} {error}").contains(marker));
    // Positive control: passing the private Error itself lets the real SDK
    // persist its source tree. The production conversion must not do this.
    let unsafe_proto = encode(ApplicationFailure::new(error.clone()).into());
    assert!(format!("{unsafe_proto:?}").contains(marker));
    let safe_proto = encode(delivery_failure(error));
    assert_eq!(
        safe_proto.message,
        "deletion cleanup disposition could not be established"
    );
    assert!(safe_proto.cause.is_none());
    let encoded = format!("{safe_proto:?}");
    for sentinel in [
        "private-deletion-body",
        "sb_secret_fixture_only",
        "private.invalid",
        "subject=fixture",
    ] {
        assert!(!encoded.contains(sentinel));
    }
    let Some(FailureInfo::ApplicationFailureInfo(info)) = safe_proto.failure_info else {
        panic!()
    };
    assert!(!info.non_retryable);
    let configuration = encode(delivery_failure(DeliveryError::Configuration));
    let Some(FailureInfo::ApplicationFailureInfo(info)) = configuration.failure_info else {
        panic!()
    };
    assert!(info.non_retryable);
    assert!(configuration.cause.is_none());
    assert!(matches!(
        delivery_failure(DeliveryError::Canceled),
        ActivityError::Cancelled { .. }
    ));
}

#[test]
fn delivery_bootstrap_and_operator_wrapper_keep_the_original_cause_tree_private() {
    let marker = "private-bootstrap sb_secret_fixture_only";
    let delivery = DeliveryError::from(BackendError::caused(
        "preflight",
        std::io::Error::other(marker),
    ));
    let runtime = DeletionRuntimeError::caused(delivery);
    let operator = crate::temporal_runtime::TemporalRuntimeError::caused(runtime);
    let runtime = operator
        .source()
        .unwrap()
        .downcast_ref::<DeletionRuntimeError>()
        .unwrap();
    let delivery = runtime
        .source()
        .unwrap()
        .downcast_ref::<DeliveryError>()
        .unwrap();
    assert_eq!(*delivery, DeliveryError::Backend);
    let original = delivery
        .source()
        .unwrap()
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert_eq!(original.to_string(), marker);
    assert!(!format!("{operator:?} {operator} {runtime:?} {runtime}").contains(marker));
    assert!(DeletionRuntimeError::rejected().source().is_none());
}

#[tokio::test]
async fn actual_preflight_task_join_error_remains_available_locally() {
    let task = tokio::spawn(std::future::pending::<()>());
    task.abort();
    let joined = task.await.unwrap_err();
    let error = DeletionRuntimeError::caused(joined);
    let original = error
        .source()
        .unwrap()
        .downcast_ref::<tokio::task::JoinError>()
        .unwrap();
    assert!(original.is_cancelled());
    assert_eq!(
        error.to_string(),
        "deletion workflow runtime could not be established"
    );
}
