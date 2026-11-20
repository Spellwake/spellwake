//! Application API entry point.
use spellwake_application::api::{ApiConfig, router};
use std::net::SocketAddr;

fn main() -> Result<(), &'static str> {
    let config = ApiConfig::from_env().map_err(|_| "application API configuration is invalid")?;
    let bind: SocketAddr = std::env::var("SPELLWAKE_API_BIND")
        .unwrap_or_else(|_| "127.0.0.1:58080".into())
        .parse()
        .map_err(|_| "application API bind is invalid")?;
    // Native admission owns blocking HTTP clients. Construct and retain their
    // router outside Tokio so client startup and final drop run synchronously.
    let app = router(config);
    let runtime = tokio::runtime::Runtime::new().map_err(|_| "application API runtime failed")?;
    runtime.block_on(async {
        #[cfg(unix)]
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(|_| "application API shutdown signal registration failed")?;
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|_| "application API bind failed")?;
        eprintln!("application_api code=ready");
        axum::serve(listener, app.clone())
            .with_graceful_shutdown(async move {
                #[cfg(unix)]
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = terminate.recv() => {}
                }
                #[cfg(not(unix))]
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|_| "application API server failed")
    })
}
