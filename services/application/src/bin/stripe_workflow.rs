//! Explicit workflow installation; it does not create a payment or activate a provider key.
use spellwake_application::{
    stripe_workflow::{STRIPE_WORKFLOW_ID, ensure_reconciliation},
    temporal_runtime::TemporalConfig,
};
use std::process::ExitCode;
#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["install"] {
        eprintln!("usage: stripe_workflow install");
        return ExitCode::FAILURE;
    }
    let result = async {
        let client = TemporalConfig::from_env()?
            .connect("spellwake-stripe-v1")
            .await?;
        ensure_reconciliation(&client)
            .await
            .map_err(spellwake_application::temporal_runtime::TemporalRuntimeError::caused)
    }
    .await;
    match result {
        Ok(()) => {
            println!(
                "{{\"status\":\"success\",\"data\":{{\"workflowId\":\"{STRIPE_WORKFLOW_ID}\"}}}}"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("stripe_workflow code=installation_failed message={error}");
            ExitCode::FAILURE
        }
    }
}
