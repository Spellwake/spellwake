//! Durable daily source delivery. Postgres remains the source/observation authority;
//! workflow history contains no Supabase configuration or source file contents.

use crate::price_collector::{CollectorConfig, CollectorError, PriceCollector};
use crate::temporal_runtime::RecordPayload;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use temporalio_client::{
    Client, WorkflowDescribeOptions, WorkflowExecutionStatus, WorkflowIdConflictPolicy,
    WorkflowIdReusePolicy, WorkflowStartOptions,
};
use temporalio_common::{MemoValues, Priority, RetryPolicy};
use temporalio_macros::{activities, workflow, workflow_methods};
use temporalio_sdk::{
    ActivityOptions, ApplicationFailure, ContinueAsNewOptions, TimerResult, WorkflowContext,
    WorkflowResult, WorkflowTermination,
    activities::{ActivityContext, ActivityError},
};
use uuid::Uuid;

/// Dedicated price pipeline queue, separate from gameplay/provider workers.
pub const PRICE_TASK_QUEUE: &str = "spellwake-price-v1";
pub mod maintenance;
pub mod probe;
pub mod runtime;

/// Stable daily identity makes an unknown dispatch acknowledgement retryable.
pub fn day_workflow_id(slot: NaiveDate) -> String {
    format!("spellwake-price-day-v1:{slot}")
}

pub async fn ensure_day(
    client: &Client,
    slot: NaiveDate,
) -> Result<(), runtime::PriceRuntimeError> {
    if parse_slot(&slot.to_string()) != Some(slot) {
        return Err(runtime::PriceRuntimeError);
    }
    let mut memo = MemoValues::new();
    memo.insert("slot", slot);
    let started = client
        .start_workflow(
            PriceDayWorkflow::run,
            slot,
            WorkflowStartOptions::new(PRICE_TASK_QUEUE, day_workflow_id(slot))
                .id_reuse_policy(WorkflowIdReusePolicy::AllowDuplicateFailedOnly)
                .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                .memo(memo)
                .build(),
        )
        .await;
    if started.is_err() {
        eprintln!("price_workflow event=dispatch_ack_unconfirmed");
    }
    let description = client
        .get_workflow_handle::<PriceDayWorkflow>(day_workflow_id(slot))
        .describe(WorkflowDescribeOptions::default())
        .await
        .map_err(|_| runtime::PriceRuntimeError)?;
    if description.workflow_type() != "spellwake-price-day-v1"
        || description
            .memo()
            .get::<NaiveDate>("slot")
            .map_err(|_| runtime::PriceRuntimeError)?
            != Some(slot)
        || !matches!(
            description.status(),
            WorkflowExecutionStatus::Running | WorkflowExecutionStatus::Completed
        )
    {
        return Err(runtime::PriceRuntimeError);
    }
    Ok(())
}

/// Reconcile installation after an unknown acknowledgement without silently
/// changing an existing calendar's initial date. Memo survives continue-as-new.
pub async fn ensure_calendar(
    client: &Client,
    first_slot: NaiveDate,
) -> Result<(), runtime::PriceRuntimeError> {
    if parse_slot(&first_slot.to_string()) != Some(first_slot) {
        return Err(runtime::PriceRuntimeError);
    }
    let mut memo = MemoValues::new();
    memo.insert("firstSlot", first_slot);
    let started = client
        .start_workflow(
            PriceCalendarWorkflow::run,
            first_slot,
            WorkflowStartOptions::new(PRICE_TASK_QUEUE, "spellwake-price-calendar-v1")
                .id_reuse_policy(WorkflowIdReusePolicy::AllowDuplicateFailedOnly)
                .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                .memo(memo)
                .build(),
        )
        .await;
    if started.is_err() {
        eprintln!("price_workflow event=calendar_ack_unconfirmed");
    }
    let description = client
        .get_workflow_handle::<PriceCalendarWorkflow>("spellwake-price-calendar-v1")
        .describe(WorkflowDescribeOptions::default())
        .await
        .map_err(|_| runtime::PriceRuntimeError)?;
    if description.workflow_type() != "spellwake-price-calendar-v1"
        || description
            .memo()
            .get::<NaiveDate>("firstSlot")
            .map_err(|_| runtime::PriceRuntimeError)?
            != Some(first_slot)
        || description.status() != WorkflowExecutionStatus::Running
    {
        return Err(runtime::PriceRuntimeError);
    }
    Ok(())
}

/// A task keeps its original canonical UTC day through retries and new runs.
pub fn parse_slot(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .filter(|slot| {
            slot.format("%Y-%m-%d").to_string() == raw
                && raw.len() == 10
                && (1..=9999).contains(&slot.year())
        })
}

fn activity_options(priority: u32) -> ActivityOptions {
    ActivityOptions::with_start_to_close_timeout(Duration::from_secs(1800))
        .heartbeat_timeout(Duration::from_secs(60))
        .priority(Priority::builder().priority_key(priority).build())
        .retry_policy(
            RetryPolicy::builder()
                .initial_interval(Duration::from_secs(30))
                .maximum_interval(Duration::from_secs(300))
                .build(),
        )
        .build()
}

fn activity_failure(error: CollectorError) -> ActivityError {
    eprintln!("price_workflow event=activity_failed code={error:?}");
    // Temporal recursively encodes Error::source. Keep private transport/codec
    // causes local; durable history receives only this safe owned message.
    if error == CollectorError::Configuration {
        ApplicationFailure::non_retryable(error.to_string()).into()
    } else {
        ApplicationFailure::new(error.to_string()).into()
    }
}

async fn blocking_activity<T: Send + 'static>(
    ctx: ActivityContext,
    action: impl FnOnce(PriceCollector) -> Result<T, CollectorError> + Send + 'static,
) -> Result<T, ActivityError> {
    // Construct/drop reqwest's blocking runtime off the async worker runtime.
    let mut task = tokio::task::spawn_blocking(move || {
        CollectorConfig::from_env()
            .and_then(PriceCollector::new)
            .and_then(action)
    });
    loop {
        tokio::select! {
            result = &mut task => {
                return result.map_err(|_| ApplicationFailure::new("price activity worker failed"))?
                    .map_err(activity_failure);
            }
            _ = tokio::time::sleep(Duration::from_secs(15)) => {
                ctx.record_heartbeat(()).await?;
                if ctx.is_cancelled() {
                    // Blocking HTTP may finish after cancellation. Its database
                    // writers are idempotent; no cancellation refund is invented.
                    return Err(ActivityError::cancelled());
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct PriceDrainBatch {
    manifest_id: Uuid,
    more: bool,
}

pub struct PriceActivities;

#[activities]
impl PriceActivities {
    #[activity(name = "spellwake-price-probe-v1")]
    pub async fn probe(
        ctx: ActivityContext,
        input: RecordPayload<probe::ProbeInput>,
    ) -> Result<Option<Uuid>, ActivityError> {
        let input = input.0;
        input
            .validate()
            .map_err(|_| ApplicationFailure::non_retryable("invalid price probe identity"))?;
        blocking_activity(ctx, move |collector| {
            match collector.probe_printing(input.slot, input.external_printing_id) {
                Ok(id) => Ok(Some(id)),
                Err(
                    CollectorError::PriceUnavailable
                    | CollectorError::Source(crate::price_source::SourceError::StaleArtifact),
                ) => Ok(None),
                Err(error) => Err(error),
            }
        })
        .await
    }

    #[activity(name = "spellwake-prediction-maintain-v1")]
    pub async fn maintain(
        ctx: ActivityContext,
        after: Option<Uuid>,
    ) -> Result<RecordPayload<crate::price_collector::maintenance::MaintenanceBatch>, ActivityError>
    {
        blocking_activity(ctx, move |collector| collector.maintain_predictions(after))
            .await
            .map(RecordPayload)
    }
    #[activity(name = "spellwake-price-dispatch-v1")]
    pub async fn dispatch(ctx: ActivityContext, slot: NaiveDate) -> Result<(), ActivityError> {
        ensure_day(&ctx.client(), slot).await.map_err(|_| {
            ApplicationFailure::new("daily price dispatch could not be established").into()
        })
    }
    #[activity(name = "spellwake-price-collect-v1")]
    pub async fn collect(ctx: ActivityContext, slot: NaiveDate) -> Result<Uuid, ActivityError> {
        blocking_activity(ctx, move |collector| {
            collector.collect_slot(slot).map(|slot| slot.id)
        })
        .await
    }

    #[activity(name = "spellwake-price-extract-v1")]
    pub async fn extract(
        ctx: ActivityContext,
        slot: NaiveDate,
    ) -> Result<RecordPayload<PriceDrainBatch>, ActivityError> {
        blocking_activity(ctx, move |collector| {
            collector.extract_slot(slot).map(|batch| PriceDrainBatch {
                manifest_id: batch.manifest_id,
                more: batch.more,
            })
        })
        .await
        .map(RecordPayload)
    }
}

#[workflow]
#[derive(Default)]
pub struct PriceDayWorkflow;

async fn wait_until<W>(ctx: &mut WorkflowContext<W>, target: DateTime<Utc>) -> WorkflowResult<()> {
    let now = DateTime::<Utc>::from(
        ctx.workflow_time()
            .ok_or_else(|| ApplicationFailure::non_retryable("price workflow time unavailable"))?,
    );
    if let Ok(remaining) = (target - now).to_std()
        && matches!(ctx.timer(remaining).await, TimerResult::Cancelled)
    {
        return Err(WorkflowTermination::cancelled());
    }
    Ok(())
}

/// One durable calendar launches independent daily jobs. Queue capacity still
/// affects timing, but old extraction is lower priority than new acquisition. Start explicitly once.
#[workflow]
#[derive(Default)]
pub struct PriceCalendarWorkflow;

#[workflow_methods]
impl PriceCalendarWorkflow {
    #[run(name = "spellwake-price-calendar-v1")]
    pub async fn run(ctx: &mut WorkflowContext<Self>, slot: NaiveDate) -> WorkflowResult<()> {
        if parse_slot(&slot.to_string()) != Some(slot) {
            return Err(ApplicationFailure::non_retryable("invalid calendar task date").into());
        }
        let start = slot
            .and_hms_opt(12, 0, 0)
            .ok_or_else(|| ApplicationFailure::non_retryable("invalid price calendar day"))?
            .and_utc();
        wait_until(ctx, start).await?;
        ctx.execute_activity(PriceActivities::dispatch, slot, activity_options(1))
            .await?;
        let next = slot
            .succ_opt()
            .ok_or_else(|| ApplicationFailure::non_retryable("price calendar date overflow"))?;
        ctx.continue_as_new(next, ContinueAsNewOptions::default())?;
        Ok(())
    }
}

#[workflow_methods]
impl PriceDayWorkflow {
    #[run(name = "spellwake-price-day-v1")]
    pub async fn run(ctx: &mut WorkflowContext<Self>, slot: NaiveDate) -> WorkflowResult<Uuid> {
        if parse_slot(&slot.to_string()) != Some(slot) {
            return Err(ApplicationFailure::non_retryable("invalid source task date").into());
        }
        let start = slot
            .and_hms_opt(12, 0, 0)
            .ok_or_else(|| ApplicationFailure::non_retryable("invalid source task day"))?
            .and_utc();
        wait_until(ctx, start).await?;
        let selected = ctx
            .execute_activity(PriceActivities::collect, slot, activity_options(1))
            .await?;
        wait_until(ctx, start + chrono::Duration::hours(1)).await?;
        // Bound each history while draining arbitrarily many pending markets.
        // New runs retain this date and resume the DB's established pending work.
        for _ in 0..20 {
            let batch = ctx
                .execute_activity(PriceActivities::extract, slot, activity_options(5))
                .await?
                .0;
            if batch.manifest_id != selected {
                return Err(
                    ApplicationFailure::non_retryable("selected source identity changed").into(),
                );
            }
            if !batch.more {
                return Ok(selected);
            }
        }
        ctx.continue_as_new(slot, ContinueAsNewOptions::default())?;
        Ok(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_dates_are_canonical_and_retry_wait_respects_source_rate_limit() {
        assert!(parse_slot("2026-11-01").is_some());
        for value in [
            "2026-1-01",
            "2026-11-1",
            "2026-02-30",
            "2026-11-01Z",
            "+10000-01-01",
            "0000-01-01",
        ] {
            assert!(parse_slot(value).is_none());
        }
        let options = activity_options(1);
        assert_eq!(options.heartbeat_timeout, Some(Duration::from_secs(60)));
        assert_eq!(options.priority.unwrap().priority_key, Some(1));
        assert_eq!(activity_options(5).priority.unwrap().priority_key, Some(5));
        assert_eq!(
            options.retry_policy.unwrap().initial_interval(),
            Duration::from_secs(30)
        );
        assert!(
            matches!(activity_failure(CollectorError::Configuration), ActivityError::Application(failure) if failure.is_non_retryable())
        );
        assert!(
            matches!(activity_failure(CollectorError::Backend), ActivityError::Application(failure) if !failure.is_non_retryable())
        );
    }

    #[path = "temporal.rs"]
    mod temporal;
}

#[cfg(test)]
mod diagnostic_privacy_tests {
    use super::*;
    use crate::backend::BackendError;
    use std::error::Error as _;
    use temporalio_common::{
        data_converters::{
            ActivitySerializationContext, DefaultFailureConverter, FailureConverter,
            PayloadConverter, SerializationContextData,
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
    fn temporal_failure_encoding_excludes_private_cause_tree_but_keeps_local_diagnostics() {
        let marker = "private-price-body-sentinel sb_secret_fixture_only https://private.invalid/?token=fixture";
        let error =
            CollectorError::from(BackendError::caused("body", std::io::Error::other(marker)));
        assert!(error.source().is_some());
        assert!(!format!("{error:?} {error}").contains(marker));
        // Positive control: the real SDK recursively persists a source if passed
        // the private error itself. This must detect the unsafe boundary.
        let unsafe_proto = encode(ApplicationFailure::new(error.clone()).into());
        assert!(format!("{unsafe_proto:?}").contains(marker));
        let safe_proto = encode(activity_failure(error));
        assert_eq!(
            safe_proto.message,
            "price collector source disposition could not be established"
        );
        assert!(safe_proto.cause.is_none());
        let encoded = format!("{safe_proto:?}");
        for sentinel in [
            "private-price-body-sentinel",
            "sb_secret_fixture_only",
            "private.invalid",
            "token=fixture",
        ] {
            assert!(!encoded.contains(sentinel));
        }
        let Some(FailureInfo::ApplicationFailureInfo(info)) = safe_proto.failure_info else {
            panic!()
        };
        assert!(!info.non_retryable);
        let configuration = encode(activity_failure(CollectorError::Configuration));
        let Some(FailureInfo::ApplicationFailureInfo(info)) = configuration.failure_info else {
            panic!()
        };
        assert!(info.non_retryable);
        assert!(configuration.cause.is_none());
    }
    #[test]
    fn temporal_encoding_keeps_new_staging_causes_local() {
        let marker = "private-source-io-sentinel sb_secret_fixture_only";
        let error = CollectorError::Source(crate::price_source::SourceError::from(
            std::io::Error::other(marker),
        ));
        assert!(
            error
                .source()
                .unwrap()
                .source()
                .unwrap()
                .is::<std::io::Error>()
        );
        assert!(!format!("{error:?} {error}").contains(marker));
        let unsafe_proto = encode(ApplicationFailure::new(error.clone()).into());
        assert!(format!("{unsafe_proto:?}").contains(marker));
        let safe_proto = encode(activity_failure(error));
        assert_eq!(safe_proto.message, "price source acquisition failed");
        assert!(safe_proto.cause.is_none());
        assert!(!format!("{safe_proto:?}").contains(marker));
    }
    #[test]
    fn temporal_encoding_keeps_collector_storage_causes_local() {
        let marker = "private-collector-storage-sentinel sb_secret_fixture_only";
        let error = CollectorError::storage(crate::private_storage::StorageError::Read(
            std::io::Error::other(marker),
        ));
        assert!(
            error
                .source()
                .unwrap()
                .source()
                .unwrap()
                .is::<std::io::Error>()
        );
        assert!(!format!("{error:?} {error}").contains(marker));
        let unsafe_proto = encode(ApplicationFailure::new(error.clone()).into());
        assert!(format!("{unsafe_proto:?}").contains(marker));
        let safe_proto = encode(activity_failure(error));
        assert_eq!(
            safe_proto.message,
            "private price evidence retention failed"
        );
        assert!(safe_proto.cause.is_none());
        assert!(!format!("{safe_proto:?}").contains(marker));
    }
    #[test]
    fn temporal_encoding_keeps_original_source_decoder_cause_local() {
        let marker = "private-source-decoder-sentinel sb_secret_fixture_only";
        let cause = serde_json::from_value::<bool>(serde_json::json!(marker)).unwrap_err();
        let error = CollectorError::Source(crate::price_source::SourceError::invalid_index(cause));
        assert!(
            error
                .source()
                .unwrap()
                .source()
                .unwrap()
                .is::<serde_json::Error>()
        );
        assert!(!format!("{error:?} {error}").contains(marker));
        let unsafe_proto = encode(ApplicationFailure::new(error.clone()).into());
        assert!(format!("{unsafe_proto:?}").contains(marker));
        let safe_proto = encode(activity_failure(error));
        assert_eq!(safe_proto.message, "price source acquisition failed");
        assert!(safe_proto.cause.is_none());
        assert!(!format!("{safe_proto:?}").contains(marker));
    }
}
