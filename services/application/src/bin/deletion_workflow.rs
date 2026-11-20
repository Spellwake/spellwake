//! Explicit operator installation, separate from worker startup.
use spellwake_application::{
    deletion_workflow::{DELETION_WORKFLOW_ID, ensure_cleanup},
    temporal_runtime::TemporalConfig,
};
use std::process::ExitCode;
#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["install"] {
        eprintln!("usage: deletion_workflow install");
        return ExitCode::FAILURE;
    }
    let result = async {
        let client = TemporalConfig::from_env()?
            .connect("spellwake-deletion-v1")
            .await?;
        ensure_cleanup(&client)
            .await
            .map_err(spellwake_application::temporal_runtime::TemporalRuntimeError::caused)
    }
    .await;
    match result {
        Ok(()) => {
            println!(
                "{{\"status\":\"success\",\"data\":{{\"workflowId\":\"{DELETION_WORKFLOW_ID}\"}}}}"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("deletion_workflow code=installation_failed message={error}");
            ExitCode::FAILURE
        }
    }
}
