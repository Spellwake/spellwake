//! Dedicated process; no game, deletion or price workers share its lifecycle.
use std::process::ExitCode;
#[tokio::main]
async fn main() -> ExitCode {
    match spellwake_application::stripe_workflow::run_worker().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("stripe_worker code=runtime_failed message={error}");
            ExitCode::FAILURE
        }
    }
}
