//! Explicit operator dispatch. Starting a worker alone never enables a calendar.
use spellwake_application::{
    ApiResponse,
    price_workflow::{
        PriceDayWorkflow, day_workflow_id, ensure_calendar, ensure_day,
        maintenance::{MAINTENANCE_WORKFLOW_ID, ensure_maintenance},
        parse_slot,
        runtime::PriceRuntimeError,
    },
    temporal_runtime::TemporalConfig,
};
use std::process::ExitCode;
use temporalio_client::WorkflowGetResultOptions;

async fn execute(
    action: &str,
    slot: Option<chrono::NaiveDate>,
) -> Result<serde_json::Value, PriceRuntimeError> {
    let client = TemporalConfig::from_env()
        .map_err(|_| PriceRuntimeError)?
        .connect("spellwake-price-v1")
        .await
        .map_err(|_| PriceRuntimeError)?;
    if action == "maintenance" {
        ensure_maintenance(&client).await?;
        return Ok(serde_json::json!({"workflowId":MAINTENANCE_WORKFLOW_ID}));
    }
    let slot = slot.ok_or(PriceRuntimeError)?;
    if action == "schedule" {
        ensure_calendar(&client, slot).await?;
        return Ok(
            serde_json::json!({"workflowId":"spellwake-price-calendar-v1","firstSlot":slot}),
        );
    }
    ensure_day(&client, slot)
        .await
        .map_err(|_| PriceRuntimeError)?;
    if action == "start-day" {
        return Ok(serde_json::json!({"workflowId":day_workflow_id(slot)}));
    }
    let selected = client
        .get_workflow_handle::<PriceDayWorkflow>(day_workflow_id(slot))
        .get_result(WorkflowGetResultOptions::default())
        .await
        .map_err(|_| PriceRuntimeError)?;
    Ok(serde_json::json!({"manifestId":selected}))
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let task = match args.as_slice() {
        [action] if action == "maintenance" => Some((action.as_str(), None)),
        [action, slot] if matches!(action.as_str(), "run-day" | "start-day" | "schedule") => {
            parse_slot(slot).map(|slot| (action.as_str(), Some(slot)))
        }
        _ => None,
    };
    let Some((action, slot)) = task else {
        eprintln!(
            "usage: price_workflow maintenance | [run-day | start-day | schedule] YYYY-MM-DD"
        );
        return ExitCode::FAILURE;
    };
    let result = execute(action, slot).await;
    let success = result.is_ok();
    let response = match result {
        Ok(data) => ApiResponse::Success { data },
        Err(error) => {
            eprintln!("price_workflow code=runtime_failed");
            ApiResponse::Error {
                message: error.to_string(),
                code: "price_workflow_failed".into(),
            }
        }
    };
    if serde_json::to_writer(std::io::stdout().lock(), &response).is_err() {
        return ExitCode::FAILURE;
    }
    println!();
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
