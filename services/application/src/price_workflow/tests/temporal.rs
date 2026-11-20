//! Real Temporal orchestration with fake source activities, not price evidence.
use super::super::*;
use crate::price_collector::maintenance::MaintenanceBatch;
use crate::price_workflow::maintenance::PredictionMaintenanceWorkflow;
use futures::TryStreamExt;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use temporalio_client::{
    ClientOptions, Connection, ConnectionOptions, WorkflowGetResultOptions, WorkflowStartOptions,
};
use temporalio_client::{WorkflowCancelOptions, WorkflowFetchHistoryOptions};
use temporalio_common::protos::temporal::api::history::v1::history_event::Attributes;
use temporalio_sdk::{Runtime, Worker, WorkerOptions};

async fn isolated_client() -> Client {
    let namespace = std::env::var("SPELLWAKE_TEST_TEMPORAL_NAMESPACE").unwrap();
    let suffix = namespace.strip_prefix("spellwake-price-test-").unwrap();
    assert!(suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let connection = Connection::connect(
        ConnectionOptions::new(reqwest::Url::parse("http://127.0.0.1:57233").unwrap())
            .identity("spellwake-isolated-workflow-test")
            .build(),
    )
    .await
    .unwrap();
    Client::new(connection, ClientOptions::new(namespace).build()).unwrap()
}

struct DrainActivities {
    selected: Uuid,
    collects: Arc<AtomicUsize>,
    extracts: Arc<AtomicUsize>,
}

#[activities]
impl DrainActivities {
    #[activity(name = "spellwake-price-collect-v1")]
    async fn collect(
        self: Arc<Self>,
        _ctx: ActivityContext,
        slot: NaiveDate,
    ) -> Result<Uuid, ActivityError> {
        if slot.to_string() != "1900-01-02" {
            return Err(ApplicationFailure::non_retryable("test task date changed").into());
        }
        self.collects.fetch_add(1, Ordering::SeqCst);
        Ok(self.selected)
    }
    #[activity(name = "spellwake-price-extract-v1")]
    async fn extract(
        self: Arc<Self>,
        _ctx: ActivityContext,
        slot: NaiveDate,
    ) -> Result<PriceDrainBatch, ActivityError> {
        if slot.to_string() != "1900-01-02" {
            return Err(ApplicationFailure::non_retryable("test drain date changed").into());
        }
        let batch = self.extracts.fetch_add(1, Ordering::SeqCst);
        Ok(PriceDrainBatch {
            manifest_id: self.selected,
            more: batch < 20,
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires tools/test_local_price_workflow.py owned Temporal service/namespace"]
async fn real_temporal_drain_continues_as_new_and_preserves_date_and_memo() {
    assert_eq!(
        DrainActivities::collect.name(),
        PriceActivities::collect.name()
    );
    assert_eq!(
        DrainActivities::extract.name(),
        PriceActivities::extract.name()
    );
    let client = isolated_client().await;
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let queue = format!("spellwake-drain-test-{}", Uuid::new_v4());
    let selected = Uuid::new_v4();
    let collects = Arc::new(AtomicUsize::new(0));
    let extracts = Arc::new(AtomicUsize::new(0));
    let options = WorkerOptions::new(&queue)
        .register_workflow::<PriceDayWorkflow>()
        .unwrap()
        .register_activities(DrainActivities {
            selected,
            collects: collects.clone(),
            extracts: extracts.clone(),
        })
        .build();
    let mut worker = Worker::new(&runtime, client.clone(), options).unwrap();
    let shutdown = worker.shutdown_handle();
    let mut memo = MemoValues::new();
    memo.insert("slot", NaiveDate::from_ymd_opt(1900, 1, 2).unwrap());
    let workflow_id = format!("drain-test-{}", Uuid::new_v4());
    let handle = client
        .start_workflow(
            PriceDayWorkflow::run,
            NaiveDate::from_ymd_opt(1900, 1, 2).unwrap(),
            WorkflowStartOptions::new(queue, &workflow_id)
                .memo(memo)
                .build(),
        )
        .await
        .unwrap();
    let exercise = async {
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            handle.get_result(WorkflowGetResultOptions::default()),
        )
        .await;
        shutdown();
        result
    };
    let (worker_result, result) = tokio::join!(worker.run(), exercise);
    worker_result.unwrap();
    assert_eq!(result.unwrap().unwrap(), selected);
    assert_eq!(collects.load(Ordering::SeqCst), 2);
    assert_eq!(extracts.load(Ordering::SeqCst), 21);
    let initial = handle
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap();
    let description = client
        .get_workflow_handle::<PriceDayWorkflow>(&workflow_id)
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap();
    assert_ne!(initial.run_id(), description.run_id());
    assert_eq!(
        description.memo().get::<NaiveDate>("slot").unwrap(),
        NaiveDate::from_ymd_opt(1900, 1, 2)
    );
}

struct MaintenanceActivities {
    calls: Arc<AtomicUsize>,
}

#[activities]
impl MaintenanceActivities {
    #[activity(name = "spellwake-prediction-maintain-v1")]
    async fn maintain(
        self: Arc<Self>,
        _ctx: ActivityContext,
        after: Option<Uuid>,
    ) -> Result<MaintenanceBatch, ActivityError> {
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        let expected = if index == 0 || index >= 21 {
            None
        } else {
            Some(Uuid::from_u128(index as u128))
        };
        if after != expected {
            return Err(ApplicationFailure::non_retryable("test sweep cursor changed").into());
        }
        let more = index < 20;
        Ok(MaintenanceBatch {
            processed: if more { 100 } else { 1 },
            settled: 0,
            more,
            next_cursor: more.then(|| Uuid::from_u128(index as u128 + 1)),
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires owned Temporal service; includes one real sixty-second sweep timer"]
async fn real_temporal_maintenance_continues_cursor_then_resets_after_its_timer() {
    assert_eq!(
        MaintenanceActivities::maintain.name(),
        PriceActivities::maintain.name()
    );
    let client = isolated_client().await;
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let queue = format!("spellwake-maintenance-test-{}", Uuid::new_v4());
    let workflow_id = format!("maintenance-test-{}", Uuid::new_v4());
    let calls = Arc::new(AtomicUsize::new(0));
    let options = WorkerOptions::new(&queue)
        .register_workflow::<PredictionMaintenanceWorkflow>()
        .unwrap()
        .register_activities(MaintenanceActivities {
            calls: calls.clone(),
        })
        .build();
    let mut worker = Worker::new(&runtime, client.clone(), options).unwrap();
    let shutdown = worker.shutdown_handle();
    let original = client
        .start_workflow(
            PredictionMaintenanceWorkflow::run,
            None,
            WorkflowStartOptions::new(queue, &workflow_id).build(),
        )
        .await
        .unwrap();
    let original_run = original
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap()
        .run_id()
        .to_owned();
    let latest = client.get_workflow_handle::<PredictionMaintenanceWorkflow>(&workflow_id);
    let exercise = async {
        let observed = tokio::time::timeout(Duration::from_secs(80), async {
            loop {
                if calls.load(Ordering::SeqCst) >= 22 {
                    let events: Vec<_> = latest
                        .fetch_history(WorkflowFetchHistoryOptions::default())
                        .try_collect()
                        .await?;
                    if events.iter().any(|event| {
                        matches!(
                            event.attributes,
                            Some(Attributes::TimerStartedEventAttributes(_))
                        )
                    }) {
                        return latest.describe(WorkflowDescribeOptions::default()).await;
                    }
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await;
        let canceled = latest.cancel(WorkflowCancelOptions::default()).await;
        let terminal = tokio::time::timeout(
            Duration::from_secs(5),
            latest.get_result(WorkflowGetResultOptions::default()),
        )
        .await;
        shutdown();
        (observed, canceled, terminal)
    };
    let (worker_result, (observed, canceled, terminal)) = tokio::join!(worker.run(), exercise);
    worker_result.unwrap();
    canceled.unwrap();
    assert!(terminal.unwrap().is_err());
    let observed = observed.unwrap().unwrap();
    assert_ne!(observed.run_id(), original_run);
    assert_eq!(calls.load(Ordering::SeqCst), 22);
}

struct DeletionSweepActivities {
    calls: Arc<AtomicUsize>,
}

#[activities]
impl DeletionSweepActivities {
    #[activity(name = "spellwake-deletion-deliver-v1")]
    async fn deliver(
        self: Arc<Self>,
        _ctx: ActivityContext,
        after: Option<Uuid>,
    ) -> Result<crate::deletion_delivery::DeletionBatch, ActivityError> {
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        let expected = if index == 0 || index >= 21 {
            None
        } else {
            Some(Uuid::from_u128(index as u128))
        };
        if after != expected {
            return Err(ApplicationFailure::non_retryable("test sweep cursor changed").into());
        }
        let more = index < 20;
        Ok(crate::deletion_delivery::DeletionBatch {
            retry_required: index == 0,
            attempted: if more { 25 } else { 0 },
            acknowledged: if more && index != 0 { 25 } else { 0 },
            more,
            next_cursor: more.then(|| Uuid::from_u128(index as u128 + 1)),
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires owned Temporal service; includes one real sixty-second sweep timer"]
async fn real_temporal_deletion_sweep_continues_cursor_then_resets_after_its_timer() {
    assert_eq!(
        DeletionSweepActivities::deliver.name(),
        crate::deletion_workflow::DeletionActivities::deliver.name()
    );
    let client = isolated_client().await;
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let queue = format!("spellwake-deletion-test-{}", Uuid::new_v4());
    let workflow_id = format!("deletion-test-{}", Uuid::new_v4());
    let calls = Arc::new(AtomicUsize::new(0));
    let options = WorkerOptions::new(&queue)
        .register_workflow::<crate::deletion_workflow::DeletionCleanupWorkflow>()
        .unwrap()
        .register_activities(DeletionSweepActivities {
            calls: calls.clone(),
        })
        .build();
    let mut worker = Worker::new(&runtime, client.clone(), options).unwrap();
    let shutdown = worker.shutdown_handle();
    let original = client
        .start_workflow(
            crate::deletion_workflow::DeletionCleanupWorkflow::run,
            None,
            WorkflowStartOptions::new(queue, &workflow_id).build(),
        )
        .await
        .unwrap();
    let original_run = original
        .describe(WorkflowDescribeOptions::default())
        .await
        .unwrap()
        .run_id()
        .to_owned();
    let latest = client
        .get_workflow_handle::<crate::deletion_workflow::DeletionCleanupWorkflow>(&workflow_id);
    let exercise = async {
        let observed = tokio::time::timeout(Duration::from_secs(80), async {
            loop {
                if calls.load(Ordering::SeqCst) >= 22 {
                    let events: Vec<_> = latest
                        .fetch_history(WorkflowFetchHistoryOptions::default())
                        .try_collect()
                        .await?;
                    if events.iter().any(|event| {
                        matches!(
                            event.attributes,
                            Some(Attributes::TimerStartedEventAttributes(_))
                        )
                    }) {
                        return latest.describe(WorkflowDescribeOptions::default()).await;
                    }
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await;
        let canceled = latest.cancel(WorkflowCancelOptions::default()).await;
        let terminal = tokio::time::timeout(
            Duration::from_secs(5),
            latest.get_result(WorkflowGetResultOptions::default()),
        )
        .await;
        shutdown();
        (observed, canceled, terminal)
    };
    let (worker_result, (observed, canceled, terminal)) = tokio::join!(worker.run(), exercise);
    worker_result.unwrap();
    canceled.unwrap();
    assert!(terminal.unwrap().is_err());
    let observed = observed.unwrap().unwrap();
    assert_ne!(observed.run_id(), original_run);
    assert_eq!(calls.load(Ordering::SeqCst), 22);
}

struct ProbeActivities {
    calls: Arc<AtomicUsize>,
}
#[activities]
impl ProbeActivities {
    #[activity(name = "spellwake-price-probe-v1")]
    async fn probe(
        self: Arc<Self>,
        _ctx: ActivityContext,
        input: crate::price_workflow::probe::ProbeInput,
    ) -> Result<Option<Uuid>, ActivityError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok((input.external_printing_id != Uuid::from_u128(1)).then(Uuid::new_v4))
    }
}
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires tools/test_local_price_workflow.py owned Temporal service/namespace"]
async fn real_temporal_probe_reconciles_shared_requests_and_checks_terminal_source_identity() {
    use crate::price_workflow::probe::{PriceProbeWorkflow, ProbeInput, ProbeStatus, ensure_probe};
    assert_eq!(ProbeActivities::probe.name(), PriceActivities::probe.name());
    let client = isolated_client().await;
    let input = ProbeInput {
        slot: NaiveDate::from_ymd_opt(1900, 1, 2).unwrap(),
        external_printing_id: Uuid::new_v4(),
    };
    let unavailable = ProbeInput {
        external_printing_id: Uuid::from_u128(1),
        ..input
    };
    let conflicting = ProbeInput {
        external_printing_id: Uuid::new_v4(),
        ..input
    };
    assert_eq!(
        ensure_probe(&client, input).await.unwrap(),
        ProbeStatus::Requested
    );
    let (one, two) = tokio::join!(ensure_probe(&client, input), ensure_probe(&client, input));
    assert_eq!(one.unwrap(), ProbeStatus::Requested);
    assert_eq!(two.unwrap(), ProbeStatus::Requested);
    assert_eq!(
        ensure_probe(&client, unavailable).await.unwrap(),
        ProbeStatus::Requested
    );
    let mut memo = MemoValues::new();
    memo.insert("source", input); // Actual input differs: identity must not be trusted on workflow ID alone.
    client
        .start_workflow(
            PriceProbeWorkflow::run,
            crate::temporal_runtime::RecordPayload(conflicting),
            WorkflowStartOptions::new(PRICE_TASK_QUEUE, conflicting.workflow_id())
                .memo(memo)
                .build(),
        )
        .await
        .unwrap();
    assert!(ensure_probe(&client, conflicting).await.is_err());
    let runtime = Runtime::from_current_tokio(Default::default()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let options = WorkerOptions::new(PRICE_TASK_QUEUE)
        .register_workflow::<PriceProbeWorkflow>()
        .unwrap()
        .register_activities(ProbeActivities {
            calls: calls.clone(),
        })
        .build();
    let mut worker = Worker::new(&runtime, client.clone(), options).unwrap();
    let shutdown = worker.shutdown_handle();
    let exercise = async {
        let result = tokio::time::timeout(Duration::from_secs(30), async {
            for source in [input, unavailable, conflicting] {
                client
                    .get_workflow_handle::<PriceProbeWorkflow>(source.workflow_id())
                    .get_result(WorkflowGetResultOptions::default())
                    .await
                    .unwrap();
            }
        })
        .await;
        shutdown();
        result
    };
    let (worker_result, result) = tokio::join!(worker.run(), exercise);
    worker_result.unwrap();
    result.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        ensure_probe(&client, input).await.unwrap(),
        ProbeStatus::Completed
    );
    assert_eq!(
        ensure_probe(&client, unavailable).await.unwrap(),
        ProbeStatus::Unavailable
    );
    assert!(ensure_probe(&client, conflicting).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
