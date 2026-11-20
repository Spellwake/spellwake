//! Pinned SDK payload and policy tests; actual Temporal runtime acceptance is separate.
use super::super::member_sweep_codec::codec;
use super::*;
use serde_json::{Value, json};
use temporalio_common::data_converters::{
    DataConverter, PayloadConversionError, SerializationContextData, WorkflowSerializationContext,
};
fn task() -> PatreonRefreshTask {
    PatreonRefreshTask::new(Uuid::from_u128(10), Uuid::from_u128(20)).unwrap()
}
fn applied() -> Value {
    json!({"associationId":task().association_id(),"sourceEventId":Uuid::from_u128(30),"outcome":"applied"})
}
#[tokio::test]
async fn actual_sdk_keeps_all_new_records_application_only_and_rejects_ambiguous_objects() {
    codec::<PatreonCurrentCursor>(json!({"after":null}), &["after"]).await;
    codec::<PatreonCurrentCursor>(json!({"after":Uuid::from_u128(5)}), &["after"]).await;
    codec::<PatreonCurrentBatch>(json!({"tasks":[]}), &["tasks"]).await;
    codec::<PatreonCurrentBatch>(json!({"tasks":[task()]}), &["tasks"]).await;
    codec::<PatreonRefreshTask>(
        serde_json::to_value(task()).unwrap(),
        &["associationId", "requestId"],
    )
    .await;
    codec::<PatreonRefreshOutcome>(applied(), &["associationId", "sourceEventId", "outcome"]).await;
    for outcome in ["unavailable", "superseded"] {
        codec::<PatreonRefreshOutcome>(
            json!({"associationId":task().association_id(),"sourceEventId":null,"outcome":outcome}),
            &["associationId", "sourceEventId", "outcome"],
        )
        .await;
    }
}
#[tokio::test]
async fn actual_sdk_rejects_nested_task_arrays_duplicates_and_invalid_dispositions() {
    let converter = DataConverter::default();
    let context = SerializationContextData::Workflow(WorkflowSerializationContext::new());
    for bad in [
        json!({"tasks":[[task().association_id(),task().request_id()]]}),
        json!({"tasks":[{"associationId":Uuid::nil(),"requestId":task().request_id()}]}),
        json!({"tasks":[{"associationId":task().association_id(),"requestId":task().request_id(),"campaignId":"private"}]}),
        json!({"tasks":null}),
    ] {
        let payload = converter.to_payload(&context, &bad).await.unwrap();
        assert!(
            converter
                .from_payload::<RecordPayload<PatreonCurrentBatch>>(&context, payload)
                .await
                .is_err()
        );
    }
    let mut payload = converter
        .to_payload(&context, &json!({"tasks":[task()]}))
        .await
        .unwrap();
    payload.data = format!(
        "{{\"tasks\":[{{\"associationId\":\"{}\",\"requestId\":\"{}\",\"requestId\":\"{}\"}}]}}",
        task().association_id(),
        task().request_id(),
        task().request_id()
    )
    .into_bytes();
    assert!(
        converter
            .from_payload::<RecordPayload<PatreonCurrentBatch>>(&context, payload)
            .await
            .is_err()
    );
    for bad in [
        json!({"associationId":task().association_id(),"sourceEventId":null,"outcome":"applied"}),
        json!({"associationId":task().association_id(),"sourceEventId":Uuid::from_u128(30),"outcome":"superseded"}),
        json!({"associationId":task().association_id(),"sourceEventId":null,"outcome":"pending"}),
        json!({"associationId":task().association_id(),"outcome":"unavailable"}),
    ] {
        let payload = converter.to_payload(&context, &bad).await.unwrap();
        assert!(
            converter
                .from_payload::<RecordPayload<PatreonRefreshOutcome>>(&context, payload)
                .await
                .is_err()
        );
    }
    for bad in [
        json!({"after":Uuid::nil()}),
        json!({"after":"00000000000000000000000000000005"}),
    ] {
        let payload = converter.to_payload(&context, &bad).await.unwrap();
        assert!(
            converter
                .from_payload::<RecordPayload<PatreonCurrentCursor>>(&context, payload)
                .await
                .is_err()
        );
    }
}
#[test]
fn batch_validation_fences_nonadvancing_work_and_correlated_result_only_advances_selected_task() {
    assert!(
        PatreonCurrentBatch { tasks: vec![] }
            .selected(None)
            .unwrap()
            .is_none()
    );
    let batch = PatreonCurrentBatch {
        tasks: vec![task()],
    };
    assert_eq!(
        batch
            .selected(Some(Uuid::from_u128(5)))
            .unwrap()
            .unwrap()
            .request_id(),
        task().request_id()
    );
    assert!(batch.selected(Some(task().association_id())).is_err());
    assert!(batch.selected(Some(Uuid::from_u128(11))).is_err());
    assert!(
        PatreonCurrentBatch {
            tasks: vec![task(), task()]
        }
        .selected(None)
        .is_err()
    );
    let result: PatreonRefreshOutcome = serde_json::from_value(applied()).unwrap();
    assert!(result_matches(&result, task()));
    assert!(!result_matches(
        &result,
        PatreonRefreshTask::new(Uuid::from_u128(11), task().request_id()).unwrap()
    ));
    for outcome in [
        PatreonRefreshOutcome::Unavailable {
            association_id: task().association_id(),
        },
        PatreonRefreshOutcome::Superseded {
            association_id: task().association_id(),
        },
    ] {
        assert!(result_matches(&outcome, task()));
    }
}
#[test]
fn activity_policy_is_bounded_and_cancellation_never_enters_handled_delivery_failure_path() {
    let options = activity_options();
    assert_eq!(
        options.close_timeouts.start_to_close(),
        Some(Duration::from_secs(360))
    );
    assert_eq!(options.heartbeat_timeout, Some(Duration::from_secs(60)));
    let retry = options.retry_policy.unwrap();
    assert_eq!(retry.initial_interval(), Duration::from_secs(30));
    assert_eq!(retry.maximum_interval(), Some(Duration::from_secs(300)));
    assert_eq!(retry.maximum_attempts(), 3);
    assert_eq!(SWEEP_ITERATIONS, 20);
    assert_eq!(SWEEP_WAIT, Duration::from_secs(60));
    assert!(matches!(
        handled_failure(
            ActivityExecutionError::Serialization(PayloadConversionError::WrongEncoding),
            true
        ),
        Err(WorkflowTermination::Cancelled { .. })
    ));
    assert!(
        handled_failure(
            ActivityExecutionError::Serialization(PayloadConversionError::WrongEncoding),
            false
        )
        .is_err()
    );
    let flag = Arc::new(AtomicBool::new(false));
    let guard = StopOnDrop(flag.clone());
    drop(guard);
    assert!(flag.load(Ordering::Acquire));
}
#[tokio::test]
async fn production_activity_configuration_preflight_performs_no_backend_or_provider_request() {
    let backend = crate::test_http::Mock::new(vec![]);
    let provider = crate::test_http::Mock::new(vec![]);
    let b = backend.base.clone();
    let p = provider.base.clone();
    tokio::task::spawn_blocking(move || {
        PatreonCurrentActivities::new(
            BackendConfig::new(&b, "sb_secret_test_only").unwrap(),
            PatreonConfig::fixture(&p),
        )
        .unwrap()
    })
    .await
    .unwrap();
    backend.finish();
    provider.finish();
}

#[path = "refresh_workflow_runtime_tests.rs"]
mod runtime;

#[path = "refresh_cancellation_tests.rs"]
mod cancellation;

#[path = "refresh_installer_tests.rs"]
mod installer;
