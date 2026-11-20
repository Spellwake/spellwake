//! Dedicated process for the deletion pipeline; no price/game workers share it.
use std::process::ExitCode;
#[tokio::main]
async fn main() -> ExitCode {
    match spellwake_application::deletion_workflow::run_worker().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("deletion_worker code=runtime_failed message={error}");
            ExitCode::FAILURE
        }
    }
}
