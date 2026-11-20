//! Current-tier-only durable refresh. Private provider evidence never crosses an activity boundary.
use super::{
    PatreonRefreshOutcome, PatreonRefreshService, PatreonRefreshTask, client::PatreonConfig,
};
use crate::{
    backend::BackendConfig,
    temporal_runtime::{RecordPayload, TemporalConfig, shutdown_signal},
};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, sync::Arc, time::Duration};
use temporalio_client::{
    Client, WorkflowDescribeOptions, WorkflowExecutionStatus, WorkflowIdConflictPolicy,
    WorkflowIdReusePolicy, WorkflowStartOptions,
};
use temporalio_macros::{activities, workflow, workflow_methods};
use temporalio_sdk::{
    ContinueAsNewOptions, Runtime, Worker, WorkerOptions, WorkflowContext, WorkflowResult,
    activities::{ActivityContext, ActivityError},
    runtime::worker_tuner::{FixedSizeSlotSupplier, TunerHolder},
};
use uuid::Uuid;

/// Separate from retention, financial reconciliation, game, deletion and price work.
pub const PATREON_CURRENT_QUEUE: &str = "spellwake-patreon-current-v1";
/// Explicit singleton installation under one fixed server deployment scope.
pub const PATREON_CURRENT_WORKFLOW: &str = "spellwake-patreon-current-refresh-v1";
use super::member_sweep::{self, InvalidBatch};
#[cfg(test)]
use super::member_sweep::{
    SWEEP_ITERATIONS, SWEEP_WAIT, StopOnDrop, activity_options, handled_failure,
};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use temporalio_common::error::ActivityExecutionError;
#[cfg(test)]
use temporalio_sdk::WorkflowTermination;

/// Safe runtime failure. Underlying causes are retained for trusted diagnostics only.
pub struct PatreonCurrentRuntimeError {
    source: Option<Box<dyn Error + Send + Sync>>,
}
impl PatreonCurrentRuntimeError {
    fn caused(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            source: Some(Box::new(error)),
        }
    }
    fn invalid() -> Self {
        Self { source: None }
    }
}
impl fmt::Display for PatreonCurrentRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("patreon current refresh runtime is unavailable")
    }
}
impl fmt::Debug for PatreonCurrentRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for PatreonCurrentRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn Error + 'static))
    }
}

pub use super::member_sweep::MemberSweepCursor as PatreonCurrentCursor;
/// A recorded discovery result contains at most one application-only logical task.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PatreonCurrentBatch {
    pub(super) tasks: Vec<PatreonRefreshTask>,
}
impl PatreonCurrentBatch {
    pub(super) fn selected(
        &self,
        after: Option<Uuid>,
    ) -> Result<Option<PatreonRefreshTask>, InvalidBatch> {
        member_sweep::select_one(&self.tasks, after, |t| t.association_id())
    }
}
pub(super) fn result_matches(result: &PatreonRefreshOutcome, task: PatreonRefreshTask) -> bool {
    let association = match result {
        PatreonRefreshOutcome::Applied { association_id, .. }
        | PatreonRefreshOutcome::Superseded { association_id }
        | PatreonRefreshOutcome::Unavailable { association_id } => *association_id,
    };
    association == task.association_id()
}
/// Private server activity configuration. No Debug/Serialize implementation or public credentials.
pub struct PatreonCurrentActivities {
    backend: BackendConfig,
    patreon: PatreonConfig,
}
impl PatreonCurrentActivities {
    // Construct only from a blocking context; preflight creates/drops a blocking client without IO.
    fn new(
        backend: BackendConfig,
        patreon: PatreonConfig,
    ) -> Result<Self, super::PatreonRefreshError> {
        PatreonRefreshService::new(backend.clone(), patreon.clone())?;
        Ok(Self { backend, patreon })
    }
    async fn execute<T: Send + 'static>(
        self: Arc<Self>,
        ctx: ActivityContext,
        work: impl FnOnce(&PatreonRefreshService) -> Result<T, super::PatreonRefreshError>
        + Send
        + 'static,
    ) -> Result<T, ActivityError> {
        member_sweep::execute(ctx, move |stopped| {
            let service = PatreonRefreshService::new(self.backend.clone(), self.patreon.clone())?
                .with_stop(stopped);
            work(&service)
        })
        .await
    }
}
#[activities]
impl PatreonCurrentActivities {
    #[activity(name = "spellwake-patreon-current-discover-v1")]
    pub async fn discover(
        self: Arc<Self>,
        ctx: ActivityContext,
        cursor: RecordPayload<PatreonCurrentCursor>,
    ) -> Result<RecordPayload<PatreonCurrentBatch>, ActivityError> {
        self.execute(ctx, move |service| {
            service.discover(cursor.0.after).map(|task| {
                RecordPayload(PatreonCurrentBatch {
                    tasks: task.into_iter().collect(),
                })
            })
        })
        .await
    }
    #[activity(name = "spellwake-patreon-current-deliver-v1")]
    pub async fn deliver(
        self: Arc<Self>,
        ctx: ActivityContext,
        task: RecordPayload<PatreonRefreshTask>,
    ) -> Result<RecordPayload<PatreonRefreshOutcome>, ActivityError> {
        self.execute(ctx, move |service| {
            service.deliver(&task.0).map(RecordPayload)
        })
        .await
    }
}
#[workflow]
#[derive(Default)]
pub struct PatreonCurrentWorkflow;
#[workflow_methods]
impl PatreonCurrentWorkflow {
    #[run(name = "spellwake-patreon-current-refresh-v1")]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        cursor: RecordPayload<PatreonCurrentCursor>,
    ) -> WorkflowResult<()> {
        let cursor = member_sweep::run_sweep(ctx, cursor.0).await?;
        ctx.continue_as_new(RecordPayload(cursor), ContinueAsNewOptions::default())?;
        Ok(())
    }
}
/// Install explicitly, verifying the actual type and running state even after an uncertain start.
/// # Errors
/// Returns a safe runtime error if installation cannot be confirmed. Does not start a worker.
pub async fn ensure_current_refresh(client: &Client) -> Result<(), PatreonCurrentRuntimeError> {
    let started = client
        .start_workflow(
            PatreonCurrentWorkflow::run,
            RecordPayload(PatreonCurrentCursor::default()),
            WorkflowStartOptions::new(PATREON_CURRENT_QUEUE, PATREON_CURRENT_WORKFLOW)
                .id_reuse_policy(WorkflowIdReusePolicy::AllowDuplicateFailedOnly)
                .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                .build(),
        )
        .await;
    if started.is_err() {
        eprintln!("patreon_current event=installation_ack_unconfirmed");
    }
    let description = client
        .get_workflow_handle::<PatreonCurrentWorkflow>(PATREON_CURRENT_WORKFLOW)
        .describe(WorkflowDescribeOptions::default())
        .await
        .map_err(PatreonCurrentRuntimeError::caused)?;
    if description.workflow_type() != PATREON_CURRENT_WORKFLOW
        || description.status() != WorkflowExecutionStatus::Running
    {
        return Err(PatreonCurrentRuntimeError::invalid());
    }
    Ok(())
}
fn worker_options(
    queue: &str,
    activities: PatreonCurrentActivities,
) -> Result<WorkerOptions, PatreonCurrentRuntimeError> {
    let tuner = TunerHolder::builder()
        .workflow_task_slot_supplier(FixedSizeSlotSupplier::new(2))
        .activity_task_slot_supplier(FixedSizeSlotSupplier::new(1))
        .local_activity_task_slot_supplier(FixedSizeSlotSupplier::new(1))
        .nexus_task_slot_supplier(FixedSizeSlotSupplier::new(1))
        .build();
    let options = WorkerOptions::new(queue)
        .tuner(tuner)
        .client_identity_override(PATREON_CURRENT_QUEUE)
        .max_cached_workflows(8)
        .graceful_shutdown_period(Duration::from_secs(360))
        .register_workflow::<PatreonCurrentWorkflow>()
        .map_err(PatreonCurrentRuntimeError::caused)?
        .register_activities(activities)
        .build();
    Ok(options)
}
/// Run only the current-tier worker. Startup validates local configuration without installing work.
/// # Errors
/// Returns safe configuration/connection/process errors. No provider data is printed.
pub async fn run_worker() -> Result<(), PatreonCurrentRuntimeError> {
    let activities = tokio::task::spawn_blocking(|| {
        PatreonCurrentActivities::new(BackendConfig::from_env()?, PatreonConfig::from_env()?)
    })
    .await
    .map_err(PatreonCurrentRuntimeError::caused)?
    .map_err(PatreonCurrentRuntimeError::caused)?;
    let client = TemporalConfig::from_env()
        .map_err(PatreonCurrentRuntimeError::caused)?
        .connect(PATREON_CURRENT_QUEUE)
        .await
        .map_err(PatreonCurrentRuntimeError::caused)?;
    let runtime = Runtime::from_current_tokio(Default::default())
        .map_err(PatreonCurrentRuntimeError::caused)?;
    let options = worker_options(PATREON_CURRENT_QUEUE, activities)?;
    let mut worker =
        Worker::new(&runtime, client, options).map_err(PatreonCurrentRuntimeError::caused)?;
    let shutdown = worker.shutdown_handle();
    let run = worker.run();
    tokio::pin!(run);
    let signal = shutdown_signal();
    tokio::pin!(signal);
    tokio::select! {
        result=&mut run=>result.map_err(PatreonCurrentRuntimeError::caused),
        signal_result=&mut signal=>{
            shutdown();
            let worker_result=run.await.map_err(PatreonCurrentRuntimeError::caused);
            signal_result.map_err(PatreonCurrentRuntimeError::caused)?;
            worker_result
        }
    }
}
#[cfg(test)]
#[path = "refresh_workflow_tests.rs"]
mod tests;
