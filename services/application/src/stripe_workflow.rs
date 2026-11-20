//! Dedicated Stripe recovery pipeline. Activity histories contain cursors and counts only.
use crate::temporal_runtime::RecordPayload;
use crate::{stripe::service::StripeService, temporal_runtime::TemporalConfig};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
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

/// Separate from price, deletion and game workers.
pub const STRIPE_TASK_QUEUE: &str = "spellwake-stripe-v1";
/// One replayable sweep revisits durable purchase, notification and expiry work.
pub const STRIPE_WORKFLOW_ID: &str = "spellwake-stripe-reconciliation-v1";
pub struct StripeRuntimeError {
    source: Option<Box<dyn Error + Send + Sync>>,
}
impl StripeRuntimeError {
    fn caused<E: Error + Send + Sync + 'static>(e: E) -> Self {
        Self {
            source: Some(Box::new(e)),
        }
    }
    fn invalid() -> Self {
        Self { source: None }
    }
}
impl fmt::Display for StripeRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("stripe workflow runtime is unavailable")
    }
}
impl fmt::Debug for StripeRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for StripeRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn Error + 'static))
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum StripeWorkKind {
    #[default]
    Purchase,
    Event,
    Evidence,
}
impl TryFrom<String> for StripeWorkKind {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "purchase" => Ok(Self::Purchase),
            "event" => Ok(Self::Event),
            "evidence" => Ok(Self::Evidence),
            _ => Err("invalid string enum value"),
        }
    }
}

impl StripeWorkKind {
    fn name(self) -> &'static str {
        match self {
            Self::Purchase => "purchase",
            Self::Event => "event",
            Self::Evidence => "evidence",
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Purchase => 0,
            Self::Event => 1,
            Self::Evidence => 2,
        }
    }
    fn next(self) -> Self {
        match self {
            Self::Purchase => Self::Event,
            Self::Event => Self::Evidence,
            Self::Evidence => Self::Purchase,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StripeCursor {
    pub kind: StripeWorkKind,
    pub after: [Option<Uuid>; 3],
    pub tails: [bool; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StripeWork {
    pub kind: StripeWorkKind,
    pub after: Option<Uuid>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StripeBatch {
    pub cursor: Option<Uuid>,
    pub completed: u8,
    pub tail: bool,
}

struct StopOnDrop(Arc<AtomicBool>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub struct StripeActivities;
#[activities]
impl StripeActivities {
    #[activity(name = "spellwake-stripe-deliver-v1")]
    pub async fn deliver(
        ctx: ActivityContext,
        work: RecordPayload<StripeWork>,
    ) -> Result<RecordPayload<StripeBatch>, ActivityError> {
        let work = work.0;
        if work.after.is_some_and(|id| id.is_nil()) {
            return Err(ApplicationFailure::non_retryable("invalid stripe work cursor").into());
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let _stop = StopOnDrop(stopped.clone());
        let mut task = tokio::task::spawn_blocking(move || {
            StripeService::from_env(stopped)?.work(work.kind.name(), work.after)
        });
        loop {
            tokio::select! {
                result=&mut task=>return result.map_err(|_|ApplicationFailure::new("stripe activity worker failed"))?.map(RecordPayload).map_err(|_|ApplicationFailure::new("stripe activity remains unresolved").into()),
                _=tokio::time::sleep(Duration::from_secs(15))=>{ctx.record_heartbeat(()).await?;if ctx.is_cancelled(){return Err(ActivityError::cancelled());}}
            }
        }
    }
}
#[workflow]
#[derive(Default)]
pub struct StripeWorkflow;
#[workflow_methods]
impl StripeWorkflow {
    #[run(name = "spellwake-stripe-reconciliation-v1")]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        cursor: RecordPayload<StripeCursor>,
    ) -> WorkflowResult<()> {
        let mut cursor = cursor.0;
        if cursor.after.iter().flatten().any(Uuid::is_nil) {
            return Err(ApplicationFailure::non_retryable("invalid stripe sweep cursor").into());
        }
        for _ in 0..20 {
            let index = cursor.kind.index();
            let after = cursor.after[index];
            let options = ActivityOptions::with_start_to_close_timeout(Duration::from_secs(600))
                .heartbeat_timeout(Duration::from_secs(60))
                .retry_policy(
                    RetryPolicy::builder()
                        .initial_interval(Duration::from_secs(30))
                        .maximum_interval(Duration::from_secs(300))
                        .build(),
                )
                .build();
            let batch = ctx
                .execute_activity(
                    StripeActivities::deliver,
                    RecordPayload(StripeWork {
                        kind: cursor.kind,
                        after,
                    }),
                    options,
                )
                .await?
                .0;
            if batch.completed > 1
                || (batch.tail && (batch.cursor.is_some() || batch.completed != 0))
                || (!batch.tail
                    && batch
                        .cursor
                        .is_none_or(|id| id.is_nil() || after.is_some_and(|a| id <= a)))
            {
                return Err(
                    ApplicationFailure::non_retryable("invalid stripe batch disposition").into(),
                );
            }
            cursor.after[index] = batch.cursor;
            cursor.tails[index] = batch.tail;
            cursor.kind = cursor.kind.next();
            if cursor.tails.iter().all(|tail| *tail) {
                if matches!(
                    ctx.timer(Duration::from_secs(60)).await,
                    TimerResult::Cancelled
                ) {
                    return Err(WorkflowTermination::cancelled());
                }
                ctx.continue_as_new(
                    RecordPayload(StripeCursor::default()),
                    ContinueAsNewOptions::default(),
                )?;
            }
        }
        ctx.continue_as_new(RecordPayload(cursor), ContinueAsNewOptions::default())?;
        Ok(())
    }
}

/// Install explicitly and verify the actual running workflow after an unknown start acknowledgment.
///
/// # Errors
/// Returns a safe runtime error if the requested workflow cannot be verified running.
pub async fn ensure_reconciliation(client: &Client) -> Result<(), StripeRuntimeError> {
    let started = client
        .start_workflow(
            StripeWorkflow::run,
            RecordPayload(StripeCursor::default()),
            WorkflowStartOptions::new(STRIPE_TASK_QUEUE, STRIPE_WORKFLOW_ID)
                .id_reuse_policy(WorkflowIdReusePolicy::AllowDuplicateFailedOnly)
                .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                .build(),
        )
        .await;
    if started.is_err() {
        eprintln!("stripe_workflow event=installation_ack_unconfirmed");
    }
    let description = client
        .get_workflow_handle::<StripeWorkflow>(STRIPE_WORKFLOW_ID)
        .describe(WorkflowDescribeOptions::default())
        .await
        .map_err(StripeRuntimeError::caused)?;
    if description.workflow_type() != STRIPE_WORKFLOW_ID
        || description.status() != WorkflowExecutionStatus::Running
    {
        return Err(StripeRuntimeError::invalid());
    }
    Ok(())
}

/// Run the dedicated worker. Startup validates configuration but never installs a workflow.
///
/// # Errors
/// Returns a runtime error on configuration, connection or worker failure; no private config is printed.
pub async fn run_worker() -> Result<(), StripeRuntimeError> {
    tokio::task::spawn_blocking(|| {
        StripeService::from_env(Arc::new(AtomicBool::new(false))).map(|_| ())
    })
    .await
    .map_err(StripeRuntimeError::caused)?
    .map_err(StripeRuntimeError::caused)?;
    let client = TemporalConfig::from_env()
        .map_err(StripeRuntimeError::caused)?
        .connect(STRIPE_TASK_QUEUE)
        .await
        .map_err(StripeRuntimeError::caused)?;
    let runtime =
        Runtime::from_current_tokio(Default::default()).map_err(StripeRuntimeError::caused)?;
    let tuner = TunerHolder::builder()
        .workflow_task_slot_supplier(FixedSizeSlotSupplier::new(2))
        .activity_task_slot_supplier(FixedSizeSlotSupplier::new(2))
        .local_activity_task_slot_supplier(FixedSizeSlotSupplier::new(1))
        .nexus_task_slot_supplier(FixedSizeSlotSupplier::new(1))
        .build();
    let options = WorkerOptions::new(STRIPE_TASK_QUEUE)
        .tuner(tuner)
        .client_identity_override(STRIPE_TASK_QUEUE)
        .max_cached_workflows(8)
        .graceful_shutdown_period(Duration::from_secs(360))
        .register_workflow::<StripeWorkflow>()
        .map_err(StripeRuntimeError::caused)?
        .register_activities(StripeActivities)
        .build();
    let mut worker = Worker::new(&runtime, client, options).map_err(StripeRuntimeError::caused)?;
    let shutdown = worker.shutdown_handle();
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown();
        }
    });
    let result = worker.run().await.map_err(StripeRuntimeError::caused);
    signal.abort();
    result
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<StripeWorkKind>(&[
            "purchase", "event", "evidence",
        ]);
    }
}
