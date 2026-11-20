//! Continuous, bounded lifecycle sweeps over durable unresolved market rows.
use super::*;

pub const MAINTENANCE_WORKFLOW_ID: &str = "spellwake-prediction-maintenance-v1";

pub async fn ensure_maintenance(client: &Client) -> Result<(), runtime::PriceRuntimeError> {
    let started = client
        .start_workflow(
            PredictionMaintenanceWorkflow::run,
            None,
            WorkflowStartOptions::new(PRICE_TASK_QUEUE, MAINTENANCE_WORKFLOW_ID)
                .id_reuse_policy(WorkflowIdReusePolicy::AllowDuplicateFailedOnly)
                .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                .build(),
        )
        .await;
    if started.is_err() {
        eprintln!("price_workflow event=maintenance_ack_unconfirmed");
    }
    let description = client
        .get_workflow_handle::<PredictionMaintenanceWorkflow>(MAINTENANCE_WORKFLOW_ID)
        .describe(WorkflowDescribeOptions::default())
        .await
        .map_err(|_| runtime::PriceRuntimeError)?;
    if description.workflow_type() != MAINTENANCE_WORKFLOW_ID
        || description.status() != WorkflowExecutionStatus::Running
    {
        return Err(runtime::PriceRuntimeError);
    }
    Ok(())
}

#[workflow]
#[derive(Default)]
pub struct PredictionMaintenanceWorkflow;

#[workflow_methods]
impl PredictionMaintenanceWorkflow {
    #[run(name = "spellwake-prediction-maintenance-v1")]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        mut after: Option<Uuid>,
    ) -> WorkflowResult<()> {
        if after.is_some_and(|id| id.is_nil()) {
            return Err(ApplicationFailure::non_retryable("invalid maintenance cursor").into());
        }
        for _ in 0..20 {
            let batch = ctx
                .execute_activity(PriceActivities::maintain, after, activity_options(1))
                .await?
                .0;
            if batch.processed > 100
                || batch.settled > batch.processed
                || (batch.more
                    && (batch.processed != 100
                        || batch.next_cursor.is_none_or(|next| {
                            next.is_nil() || after.is_some_and(|after| next <= after)
                        })))
                || (!batch.more && (batch.processed >= 100 || batch.next_cursor.is_some()))
            {
                return Err(ApplicationFailure::non_retryable(
                    "invalid maintenance batch disposition",
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
                // Revisit pending markets and discover rows inserted behind a
                // prior cursor. Publication never depends on an HTTP dispatch ACK.
                ctx.continue_as_new(None::<Uuid>, ContinueAsNewOptions::default())?;
            }
            after = batch.next_cursor;
        }
        ctx.continue_as_new(after, ContinueAsNewOptions::default())?;
        Ok(())
    }
}
