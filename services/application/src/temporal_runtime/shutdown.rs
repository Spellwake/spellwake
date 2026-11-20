//! Process signals shared by independently controlled worker entry points.
use std::io;

pub(crate) async fn shutdown_signal() -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result=tokio::signal::ctrl_c()=>result,
            _=terminate.recv()=>Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}
