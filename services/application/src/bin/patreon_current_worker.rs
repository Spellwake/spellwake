//! Independent current-tier worker. No automatic workflow installation.
use std::process::ExitCode;
#[tokio::main]
async fn main() -> ExitCode {
    match spellwake_application::patreon::refresh_workflow::run_worker().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("patreon_current_worker code=runtime_failed message={error}");
            ExitCode::FAILURE
        }
    }
}
