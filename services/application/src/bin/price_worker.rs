//! Dedicated Temporal price-pipeline worker; no gameplay workers share this process.
use spellwake_application::price_workflow::runtime::run_worker;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match run_worker().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("price_worker code=runtime_failed message={error}");
            ExitCode::FAILURE
        }
    }
}
