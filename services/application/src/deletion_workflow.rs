//! Dedicated deletion queue. Startup polls only; installation is explicit.
use crate::temporal_runtime::RecordPayload;
use crate::{
    deletion_delivery::{DeletionBatch, DeletionDelivery, DeliveryError},
    temporal_runtime::TemporalConfig,
};
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use temporalio_client::{
    Client, WorkflowDescribeOptions, WorkflowExecutionStatus, WorkflowIdConflictPolicy,
    WorkflowIdReusePolicy, WorkflowStartOptions,
};
use temporalio_common::RetryPolicy;
use temporalio_macros::{activities, workflow, workflow_methods};
use temporalio_sdk::{
    ActivityOptions, ApplicationFailure, ContinueAsNewOptions, Runtime, TimerResult, Worker,
    WorkerOptions, WorkflowContext, WorkflowResult, WorkflowTermination,
    activities::{ActivityContext, ActivityError},
    runtime::worker_tuner::{FixedSizeSlotSupplier, TunerHolder},
};
use uuid::Uuid;

pub const DELETION_TASK_QUEUE: &str = "spellwake-deletion-v1";
pub const DELETION_WORKFLOW_ID: &str = "spellwake-deletion-cleanup-v1";
/// Safe deletion bootstrap failure with an optional private original cause.
pub struct DeletionRuntimeError {
    cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}
impl DeletionRuntimeError {
    fn caused(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self {
            cause: Some(error.into()),
        }
    }
    fn rejected() -> Self {
        Self { cause: None }
    }
}
impl fmt::Debug for DeletionRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeletionRuntimeError")
            .finish_non_exhaustive()
    }
}
impl fmt::Display for DeletionRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("deletion workflow runtime could not be established")
    }
}
impl std::error::Error for DeletionRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|cause| cause.as_ref() as _)
    }
}

/// Explicitly install or confirm the existing cleanup workflow.
/// # Errors
/// Retains a failed confirmation RPC locally, or rejects a wrong type/status.
pub async fn ensure_cleanup(client: &Client) -> Result<(), DeletionRuntimeError> {
    let started = client
        .start_workflow(
            DeletionCleanupWorkflow::run,
            None,
            WorkflowStartOptions::new(DELETION_TASK_QUEUE, DELETION_WORKFLOW_ID)
                .id_reuse_policy(WorkflowIdReusePolicy::AllowDuplicateFailedOnly)
                .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                .build(),
        )
        .await;
    if started.is_err() {
        eprintln!("deletion_workflow event=installation_ack_unconfirmed");
    }
    let description = client
        .get_workflow_handle::<DeletionCleanupWorkflow>(DELETION_WORKFLOW_ID)
        .describe(WorkflowDescribeOptions::default())
        .await
        .map_err(DeletionRuntimeError::caused)?;
    if description.workflow_type() != DELETION_WORKFLOW_ID
        || description.status() != WorkflowExecutionStatus::Running
    {
        return Err(DeletionRuntimeError::rejected());
    }
    Ok(())
}

pub struct DeletionActivities;
struct StopDelivery(Arc<AtomicBool>);
impl Drop for StopDelivery {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn delivery_failure(error: DeliveryError) -> ActivityError {
    if error == DeliveryError::Canceled {
        return ActivityError::cancelled();
    }
    // The SDK recursively encodes Error::source(). Only the safe owned message
    // crosses into durable history; backend diagnostics stay in this process.
    let message = error.to_string();
    if error == DeliveryError::Configuration {
        ApplicationFailure::non_retryable(message).into()
    } else {
        ApplicationFailure::new(message).into()
    }
}
#[activities]
impl DeletionActivities {
    #[activity(name = "spellwake-deletion-deliver-v1")]
    pub async fn deliver(
        ctx: ActivityContext,
        after: Option<Uuid>,
    ) -> Result<RecordPayload<DeletionBatch>, ActivityError> {
        let stopped = Arc::new(AtomicBool::new(false));
        let _stop = StopDelivery(stopped.clone());
        let mut task = tokio::task::spawn_blocking(move || {
            DeletionDelivery::from_env()?.deliver_cancellable(after, &stopped)
        });
        loop {
            tokio::select! {
                result=&mut task => return result.map_err(|_|ApplicationFailure::new("deletion activity worker failed"))?.map(RecordPayload).map_err(|error| {
                    eprintln!("deletion_workflow event=activity_failed code={error:?}");
                delivery_failure(error)
                }),
                _=tokio::time::sleep(Duration::from_secs(15))=> {
                    ctx.record_heartbeat(()).await?;
                    if ctx.is_cancelled() {return Err(ActivityError::cancelled());}
                }
            }
        }
    }
}
#[workflow]
#[derive(Default)]
pub struct DeletionCleanupWorkflow;
#[workflow_methods]
impl DeletionCleanupWorkflow {
    #[run(name = "spellwake-deletion-cleanup-v1")]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        mut after: Option<Uuid>,
    ) -> WorkflowResult<()> {
        if after.is_some_and(|id| id.is_nil()) {
            return Err(ApplicationFailure::non_retryable("invalid deletion cursor").into());
        }
        for _ in 0..20 {
            let options = ActivityOptions::with_start_to_close_timeout(Duration::from_secs(3600))
                .heartbeat_timeout(Duration::from_secs(60))
                .retry_policy(
                    RetryPolicy::builder()
                        .initial_interval(Duration::from_secs(30))
                        .maximum_interval(Duration::from_secs(300))
                        .build(),
                )
                .build();
            let batch = ctx
                .execute_activity(DeletionActivities::deliver, after, options)
                .await?
                .0;
            if batch.attempted > 25
                || batch.acknowledged > batch.attempted
                || (batch.more
                    && batch.next_cursor.is_none_or(|next| {
                        next.is_nil() || after.is_some_and(|previous| next <= previous)
                    }))
                || (!batch.more
                    && (batch.attempted != 0
                        || batch.acknowledged != 0
                        || batch.next_cursor.is_some()
                        || batch.retry_required))
            {
                return Err(ApplicationFailure::non_retryable(
                    "invalid deletion batch disposition",
                )
                .into());
            }
            if !batch.more {
                if matches!(
                    ctx.timer(Duration::from_secs(60)).await,
                    TimerResult::Cancelled
                ) {
                    return Err(WorkflowTermination::cancelled());
                }
                ctx.continue_as_new(None::<Uuid>, ContinueAsNewOptions::default())?;
            }
            after = batch.next_cursor;
        }
        ctx.continue_as_new(after, ContinueAsNewOptions::default())?;
        Ok(())
    }
}

/// Poll the existing deletion queue without installing a workflow.
/// # Errors
/// Retains preflight, connection, SDK registration and worker failures locally.
pub async fn run_worker() -> Result<(), DeletionRuntimeError> {
    tokio::task::spawn_blocking(|| DeletionDelivery::from_env().map(|_| ()))
        .await
        .map_err(DeletionRuntimeError::caused)?
        .map_err(DeletionRuntimeError::caused)?;
    let client = TemporalConfig::from_env()
        .map_err(DeletionRuntimeError::caused)?
        .connect(DELETION_TASK_QUEUE)
        .await
        .map_err(DeletionRuntimeError::caused)?;
    let runtime =
        Runtime::from_current_tokio(Default::default()).map_err(DeletionRuntimeError::caused)?;
    let tuner = TunerHolder::builder()
        .workflow_task_slot_supplier(FixedSizeSlotSupplier::new(2))
        .activity_task_slot_supplier(FixedSizeSlotSupplier::new(2))
        .local_activity_task_slot_supplier(FixedSizeSlotSupplier::new(1))
        .nexus_task_slot_supplier(FixedSizeSlotSupplier::new(1))
        .build();
    let options = WorkerOptions::new(DELETION_TASK_QUEUE)
        .tuner(tuner)
        .client_identity_override(DELETION_TASK_QUEUE)
        .max_cached_workflows(8)
        .graceful_shutdown_period(Duration::from_secs(30))
        .register_workflow::<DeletionCleanupWorkflow>()
        .map_err(DeletionRuntimeError::caused)?
        .register_activities(DeletionActivities)
        .build();
    let mut worker =
        Worker::new(&runtime, client, options).map_err(DeletionRuntimeError::caused)?;
    let shutdown = worker.shutdown_handle();
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown();
        }
    });
    let result = worker.run().await.map_err(DeletionRuntimeError::caused);
    signal.abort();
    result
}

#[cfg(test)]
#[path = "deletion_workflow/diagnostics.rs"]
mod diagnostic_privacy_tests;
