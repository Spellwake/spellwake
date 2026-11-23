//! Standalone native Rust AI runtime worker binary.
//!
//! Runs the AI binding provisioner, opening-choice responder, and live
//! event-driven game driver against the configured Supabase backend and LLM
//! provider (`SPELLWAKE_AI_PROVIDER=openai|codex`).
use spellwake_application::ai_runtime::AiRuntimeService;
use tokio::sync::watch;
use tracing_subscriber::{EnvFilter, fmt};

fn main() -> Result<(), String> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("spellwake_application=info,ai_runtime_worker=info"));
    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .compact()
        .init();

    let service = AiRuntimeService::from_env()?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime failed: {e}"))?;
    rt.block_on(async move {
        let (tx, rx) = watch::channel(false);
        let handle = tokio::spawn(service.run_supervisor(rx));
        let _ = tokio::signal::ctrl_c().await;
        let _ = tx.send(true);
        let _ = handle.await;
    });
    Ok(())
}
