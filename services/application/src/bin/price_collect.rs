//! Operator-controlled source activities; never included in the shipped game client.
use chrono::NaiveDate;
use spellwake_application::{
    ApiResponse,
    price_collector::{CollectorConfig, PriceCollector},
};
use std::process::ExitCode;
use uuid::Uuid;

enum Task {
    Collect,
    CollectSlot(NaiveDate),
    Extract(NaiveDate),
    Probe(NaiveDate, Uuid),
    Maintain(Option<Uuid>),
}
fn date(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .filter(|date| date.format("%Y-%m-%d").to_string() == raw)
}
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = match args.as_slice() {
        [] => Some(Task::Collect),
        [action] if action == "maintain" => Some(Task::Maintain(None)),
        [action, cursor] if action == "maintain" => Uuid::parse_str(cursor)
            .ok()
            .filter(|id| !id.is_nil())
            .map(|id| Task::Maintain(Some(id))),
        [action, slot] if action == "collect" => date(slot).map(Task::CollectSlot),
        [action, slot] if action == "extract" => date(slot).map(Task::Extract),
        [action, slot, printing] if action == "probe" => date(slot)
            .zip(Uuid::parse_str(printing).ok())
            .map(|(slot, printing)| Task::Probe(slot, printing)),
        _ => None,
    };
    let Some(task) = task else {
        eprintln!(
            "usage: price_collect [collect YYYY-MM-DD | extract YYYY-MM-DD | probe YYYY-MM-DD SCRYFALL-UUID | maintain [CURSOR]]"
        );
        return ExitCode::FAILURE;
    };
    let result = CollectorConfig::from_env()
        .and_then(PriceCollector::new)
        .and_then(|collector| match task {
            Task::Maintain(after) => collector
                .maintain_predictions(after)
                .map(|batch| serde_json::json!(batch)),
            Task::Collect => collector
                .collect_current()
                .map(|value| serde_json::json!(value)),
            Task::CollectSlot(slot) => collector
                .collect_slot(slot)
                .map(|value| serde_json::json!(value)),
            Task::Extract(slot) => collector
                .extract_slot(slot)
                .map(|value| serde_json::json!(value)),
            Task::Probe(slot, printing) => collector
                .probe_printing(slot, printing)
                .map(|id| serde_json::json!({"resourceId":id})),
        });
    match result {
        Ok(data) => {
            if serde_json::to_writer(std::io::stdout().lock(), &ApiResponse::Success { data })
                .is_err()
            {
                eprintln!("price_collector code=output_failed");
                return ExitCode::FAILURE;
            }
            println!();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("price_collector code={error:?}");
            let response: ApiResponse<()> = ApiResponse::Error {
                message: error.to_string(),
                code: "price_activity_failed".into(),
            };
            if serde_json::to_writer(std::io::stdout().lock(), &response).is_err() {
                eprintln!("price_collector code=output_failed");
            }
            println!();
            ExitCode::FAILURE
        }
    }
}
