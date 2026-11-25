//! Application API entry point.
use spellwake_application::api::{ApiConfig, router};
use std::net::SocketAddr;
use tokio::sync::watch;
use tracing_subscriber::{EnvFilter, fmt};

fn main() -> Result<(), &'static str> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("spellwake_application=info,application_api=info"));
    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .compact()
        .init();

    let config = ApiConfig::from_env().map_err(|_| "application API configuration is invalid")?;
    let ai_runtime = config
        .build_ai_runtime()
        .map_err(|_| "application AI runtime configuration is invalid")?;
    let bind: SocketAddr = std::env::var("SPELLWAKE_API_BIND")
        .unwrap_or_else(|_| "127.0.0.1:58080".into())
        .parse()
        .map_err(|_| "application API bind is invalid")?;
    // Native admission owns pooled HTTP clients. Construct and retain their
    // router outside Tokio so client startup and final drop run synchronously.
    let app = router(config);
    // Configure a 16 MiB stack so native custom-card compilation and oracle parsing
    // satisfy `card_programs::custom_card`'s stack depth invariant on every worker/blocking thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .map_err(|_| "application API runtime failed")?;
    runtime.block_on(async {
        #[cfg(unix)]
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(|_| "application API shutdown signal registration failed")?;
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|_| "application API bind failed")?;
        let (ai_shutdown_tx, ai_shutdown_rx) = watch::channel(false);
        let ai_handle = ai_runtime.map(|svc| tokio::spawn(svc.run_supervisor(ai_shutdown_rx)));
        tracing::info!(bind = %bind, code = "ready", "application_api listening");
        let serve_result = axum::serve(
            listener,
            app.clone()
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            #[cfg(unix)]
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            #[cfg(not(unix))]
            let _ = tokio::signal::ctrl_c().await;
            let _ = ai_shutdown_tx.send(true);
        })
        .await;
        if let Some(handle) = ai_handle {
            let _ = handle.await;
        }
        serve_result.map_err(|_| "application API server failed")
    })
}
