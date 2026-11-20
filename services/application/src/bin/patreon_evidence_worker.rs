//! Separate retention process; starting it does not register a provider or install a workflow.
use std::process::ExitCode;
#[tokio::main]
async fn main() -> ExitCode {
    match spellwake_application::patreon::run_evidence_worker().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("patreon_evidence code=runtime_failed message={error}");
            ExitCode::FAILURE
        }
    }
}
