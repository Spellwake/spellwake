//! Private finite member sweep. Domain activity names and payloads remain independent.
use super::refresh_workflow::PatreonCurrentActivities;
use crate::temporal_runtime::RecordPayload;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use temporalio_common::{RetryPolicy, error::ActivityExecutionError};
use temporalio_sdk::{
    ActivityOptions, ApplicationFailure, TimerResult, WorkflowContext, WorkflowTermination,
    activities::{ActivityContext, ActivityError},
};
use uuid::Uuid;

pub(super) const SWEEP_ITERATIONS: usize = 20;
pub(super) const SWEEP_WAIT: Duration = Duration::from_secs(60);

/// Durable membership sweep position. Null restarts selection, not a source watermark.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemberSweepCursor {
    #[serde(deserialize_with = "super::member_context::optional_id")]
    pub(super) after: Option<Uuid>,
}
#[derive(Debug)]
pub(super) struct InvalidBatch;
pub(super) fn select_one<T: Copy>(
    tasks: &[T],
    after: Option<Uuid>,
    id: impl FnOnce(T) -> Uuid,
) -> Result<Option<T>, InvalidBatch> {
    if tasks.len() > 1
        || tasks
            .first()
            .is_some_and(|t| after.is_some_and(|a| id(*t) <= a))
    {
        return Err(InvalidBatch);
    }
    Ok(tasks.first().copied())
}
pub(super) fn activity_options() -> ActivityOptions {
    ActivityOptions::with_start_to_close_timeout(Duration::from_secs(360))
        .heartbeat_timeout(Duration::from_secs(60))
        .retry_policy(
            RetryPolicy::builder()
                .initial_interval(Duration::from_secs(30))
                .maximum_interval(Duration::from_secs(300))
                .maximum_attempts(3)
                .build(),
        )
        .build()
}
pub(super) struct StopOnDrop(pub(super) Arc<AtomicBool>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
/// Cooperative cancellation fences later network groups; an in-flight blocking group may finish.
pub(super) async fn execute<T: Send + 'static, E: Send + 'static>(
    ctx: ActivityContext,
    work: impl FnOnce(Arc<AtomicBool>) -> Result<T, E> + Send + 'static,
) -> Result<T, ActivityError> {
    if ctx.is_cancelled() {
        return Err(ActivityError::cancelled());
    }
    let stopped = Arc::new(AtomicBool::new(false));
    let _stop = StopOnDrop(stopped.clone());
    let mut task = tokio::task::spawn_blocking(move || work(stopped));
    loop {
        tokio::select! {
            biased;
            _=ctx.cancelled()=>return Err(ActivityError::cancelled()),
            result=&mut task=>return result.map_err(|_|ApplicationFailure::new("patreon activity worker failed"))?.map_err(|_|{eprintln!("patreon_current event=activity_unresolved");ApplicationFailure::new("patreon current refresh remains unresolved").into()}),
            _=tokio::time::sleep(Duration::from_secs(15))=>{ctx.record_heartbeat(()).await?;}
        }
    }
}
pub(super) fn handled_failure(
    error: ActivityExecutionError,
    cancelled: bool,
) -> Result<(), WorkflowTermination> {
    if cancelled || matches!(error, ActivityExecutionError::Cancelled(_)) {
        return Err(WorkflowTermination::cancelled());
    }
    match error {
        ActivityExecutionError::Failed(_) => Ok(()),
        other => Err(other.into()),
    }
}
/// Preserve discovery/delivery/timer command ordering for current membership refresh.
pub(super) async fn run_sweep<W>(
    ctx: &mut WorkflowContext<W>,
    mut cursor: MemberSweepCursor,
) -> Result<MemberSweepCursor, WorkflowTermination> {
    if cursor.after.is_some_and(|id| id.is_nil()) {
        return Err(ApplicationFailure::non_retryable("invalid patreon cursor").into());
    }
    for _ in 0..SWEEP_ITERATIONS {
        if ctx.cancellation_token().is_cancelled() {
            return Err(WorkflowTermination::cancelled());
        }
        let discovered = ctx
            .execute_activity(
                PatreonCurrentActivities::discover,
                RecordPayload(cursor.clone()),
                activity_options(),
            )
            .await
            .map(|b| b.0);
        let batch = match discovered {
            Ok(batch) => batch,
            Err(error) => {
                handled_failure(error, ctx.cancellation_token().is_cancelled())?;
                eprintln!("patreon_current event=discovery_unresolved");
                if matches!(ctx.timer(SWEEP_WAIT).await, TimerResult::Cancelled) {
                    return Err(WorkflowTermination::cancelled());
                }
                continue;
            }
        };
        if ctx.cancellation_token().is_cancelled() {
            return Err(WorkflowTermination::cancelled());
        }
        let Some(task) = batch
            .selected(cursor.after)
            .map_err(|_| ApplicationFailure::non_retryable("invalid patreon discovery batch"))?
        else {
            if matches!(ctx.timer(SWEEP_WAIT).await, TimerResult::Cancelled) {
                return Err(WorkflowTermination::cancelled());
            }
            cursor.after = None;
            continue;
        };
        let delivered = ctx
            .execute_activity(
                PatreonCurrentActivities::deliver,
                RecordPayload(task),
                activity_options(),
            )
            .await
            .map(|r| super::refresh_workflow::result_matches(&r.0, task));
        match delivered {
            Ok(true) => {}
            Ok(false) => {
                return Err(ApplicationFailure::non_retryable(
                    "invalid patreon delivery correlation",
                )
                .into());
            }
            Err(error) => {
                handled_failure(error, ctx.cancellation_token().is_cancelled())?;
                eprintln!("patreon_current event=delivery_unresolved");
            }
        }
        if ctx.cancellation_token().is_cancelled() {
            return Err(WorkflowTermination::cancelled());
        }
        // Exhaustion cannot starve later candidates; the full sweep revisits them.
        cursor.after = Some(task.association_id());
    }
    Ok(cursor)
}
