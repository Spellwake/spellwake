//! Shared exact-printing extraction from one immutable retained daily source.
use super::{
    PRICE_TASK_QUEUE, PriceActivities, activity_options, parse_slot, runtime::PriceRuntimeError,
};
use crate::temporal_runtime::RecordPayload;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use temporalio_client::{
    Client, WorkflowDescribeOptions, WorkflowExecutionStatus, WorkflowGetResultOptions,
    WorkflowIdConflictPolicy, WorkflowIdReusePolicy, WorkflowStartOptions,
};
use temporalio_common::MemoValues;
use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{ApplicationFailure, WorkflowContext, WorkflowResult};
use uuid::Uuid;

/// Public source identity only; never account IDs, credentials, prices or artifact paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeInput {
    pub slot: NaiveDate,
    pub external_printing_id: Uuid,
}
impl ProbeInput {
    /// Validate canonical fixed-day identity before dispatch or worker execution.
    /// # Errors
    /// Rejects noncanonical dates and nil external printing identity.
    pub fn validate(&self) -> Result<(), PriceRuntimeError> {
        if parse_slot(&self.slot.to_string()) != Some(self.slot)
            || self.external_printing_id.is_nil()
        {
            return Err(PriceRuntimeError);
        }
        Ok(())
    }
    /// One shared workflow for all accounts and finishes of the exact source printing/day.
    pub fn workflow_id(&self) -> String {
        format!(
            "spellwake-price-probe-v1:{}:{}",
            self.slot, self.external_printing_id
        )
    }
}
/// Dispatch status never promises that a particular finish has a usable USD field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum ProbeStatus {
    Requested,
    Completed,
    Unavailable,
}
impl TryFrom<String> for ProbeStatus {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "requested" => Ok(Self::Requested),
            "completed" => Ok(Self::Completed),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Reconcile unknown dispatch acknowledgments using the exact shared workflow type and memo.
/// # Errors
/// Rejects unavailable runtime, conflicting identity, invalid result or unsupported terminal state.
pub async fn ensure_probe(
    client: &Client,
    input: ProbeInput,
) -> Result<ProbeStatus, PriceRuntimeError> {
    input.validate()?;
    let mut memo = MemoValues::new();
    memo.insert("source", RecordPayload(input));
    let started = client
        .start_workflow(
            PriceProbeWorkflow::run,
            RecordPayload(input),
            WorkflowStartOptions::new(PRICE_TASK_QUEUE, input.workflow_id())
                .id_reuse_policy(WorkflowIdReusePolicy::AllowDuplicateFailedOnly)
                .id_conflict_policy(WorkflowIdConflictPolicy::UseExisting)
                .memo(memo)
                .build(),
        )
        .await;
    if started.is_err() {
        eprintln!("price_workflow event=probe_dispatch_ack_unconfirmed");
    }
    let handle = client.get_workflow_handle::<PriceProbeWorkflow>(input.workflow_id());
    let description = handle
        .describe(WorkflowDescribeOptions::default())
        .await
        .map_err(|_| PriceRuntimeError)?;
    if description.workflow_type() != "spellwake-price-probe-v1"
        || description
            .memo()
            .get::<RecordPayload<ProbeInput>>("source")
            .map_err(|_| PriceRuntimeError)?
            .map(|value| value.0)
            != Some(input)
    {
        return Err(PriceRuntimeError);
    }
    match description.status() {
        WorkflowExecutionStatus::Running => Ok(ProbeStatus::Requested),
        WorkflowExecutionStatus::Completed => match handle
            .get_result(WorkflowGetResultOptions::default())
            .await
            .map_err(|_| PriceRuntimeError)?
        {
            Some(id) if !id.is_nil() => Ok(ProbeStatus::Completed),
            None => Ok(ProbeStatus::Unavailable),
            _ => Err(PriceRuntimeError),
        },
        _ => Err(PriceRuntimeError),
    }
}
#[workflow]
#[derive(Default)]
pub struct PriceProbeWorkflow;
#[workflow_methods]
impl PriceProbeWorkflow {
    #[run(name = "spellwake-price-probe-v1")]
    pub async fn run(
        ctx: &mut WorkflowContext<Self>,
        input: RecordPayload<ProbeInput>,
    ) -> WorkflowResult<Option<Uuid>> {
        let input = input.0;
        input
            .validate()
            .map_err(|_| ApplicationFailure::non_retryable("invalid price probe identity"))?;
        Ok(ctx
            .execute_activity(
                PriceActivities::probe,
                RecordPayload(input),
                activity_options(7),
            )
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_identity_is_finish_independent_and_rejects_noncanonical_or_private_input() {
        let id = Uuid::new_v4();
        let input = ProbeInput {
            slot: NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(),
            external_printing_id: id,
        };
        input.validate().unwrap();
        assert_eq!(
            input.workflow_id(),
            format!("spellwake-price-probe-v1:2026-11-02:{id}")
        );
        assert!(
            ProbeInput {
                external_printing_id: Uuid::nil(),
                ..input
            }
            .validate()
            .is_err()
        );
        assert!(
            ProbeInput {
                slot: NaiveDate::from_ymd_opt(10000, 1, 1).unwrap(),
                ..input
            }
            .validate()
            .is_err()
        );
        let mut value = serde_json::to_value(input).unwrap();
        value["accountId"] = serde_json::json!(Uuid::new_v4());
        assert!(serde_json::from_value::<ProbeInput>(value).is_err());
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<ProbeStatus>(&[
            "requested",
            "completed",
            "unavailable",
        ]);
    }
}
