//! Explicit singleton installation, separate from worker and provider activation.
use spellwake_application::{
    patreon::refresh_workflow::{
        PATREON_CURRENT_QUEUE, PATREON_CURRENT_WORKFLOW, ensure_current_refresh,
    },
    temporal_runtime::TemporalConfig,
};
use std::process::ExitCode;
#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["install"] {
        eprintln!("usage: patreon_current_workflow install");
        return ExitCode::FAILURE;
    }
    let result = async {
        let client = TemporalConfig::from_env()
            .map_err(|_| ())?
            .connect(PATREON_CURRENT_QUEUE)
            .await
            .map_err(|_| ())?;
        ensure_current_refresh(&client).await.map_err(|_| ())
    }
    .await;
    match result {
        Ok(()) => {
            println!(
                "{{\"status\":\"success\",\"data\":{{\"workflowId\":\"{PATREON_CURRENT_WORKFLOW}\"}}}}"
            );
            ExitCode::SUCCESS
        }
        Err(()) => {
            eprintln!("patreon_current_workflow code=installation_unresolved");
            ExitCode::FAILURE
        }
    }
}
