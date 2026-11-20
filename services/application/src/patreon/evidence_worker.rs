//! Dedicated restartable retention sweeps; durable work lives in provider_event, not memory.
use super::intake::{EvidenceService, IntakeError};
use crate::{backend::BackendConfig, temporal_runtime::shutdown_signal};
use std::{
    error::Error,
    fmt,
    future::Future,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

/// Safe process error retains the underlying cause without rendering private configuration.
pub struct PatreonEvidenceWorkerError {
    source: Box<dyn Error + Send + Sync>,
}
impl PatreonEvidenceWorkerError {
    fn caused(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(error),
        }
    }
}
impl fmt::Display for PatreonEvidenceWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("patreon evidence worker is unavailable")
    }
}
impl fmt::Debug for PatreonEvidenceWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for PatreonEvidenceWorkerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}
struct Stop {
    flag: Arc<AtomicBool>,
    wake: mpsc::Sender<()>,
}
impl Drop for Stop {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::Release);
        let _ = self.wake.send(());
    }
}
/// Run the dedicated process with bounded one-source steps and full-sweep restart.
/// No webhook secret, OAuth credential or creator token is needed.
/// # Errors
/// Returns a safe configuration/process error; individual source failures remain durable for retry.
pub async fn run_evidence_worker() -> Result<(), PatreonEvidenceWorkerError> {
    let backend = BackendConfig::from_env().map_err(PatreonEvidenceWorkerError::caused)?;
    let bucket = std::env::var("SPELLWAKE_PATREON_EVIDENCE_BUCKET")
        .map_err(PatreonEvidenceWorkerError::caused)?;
    run(backend, bucket, shutdown_signal()).await
}

async fn run(
    backend: BackendConfig,
    bucket: String,
    shutdown: impl Future<Output = io::Result<()>>,
) -> Result<(), PatreonEvidenceWorkerError> {
    let flag = Arc::new(AtomicBool::new(false));
    let (wake, stop) = mpsc::channel();
    let guard = Stop {
        flag: flag.clone(),
        wake,
    };
    let mut task = tokio::task::spawn_blocking(move || -> Result<(), IntakeError> {
        let service = EvidenceService::new(backend, bucket, flag.clone())?;
        let mut after = None;
        while !flag.load(Ordering::Acquire) {
            match service.work(after) {
                Ok(Some(id)) => {
                    after = Some(id);
                    continue;
                }
                Ok(None) => {}
                Err(_) if flag.load(Ordering::Acquire) => break,
                Err(_) => eprintln!("patreon_evidence event=sweep_unresolved"),
            }
            after = None;
            // Restarting from NULL revisits failures and insertions behind a UUID cursor.
            match stop.recv_timeout(Duration::from_secs(60)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        Ok(())
    });
    tokio::pin!(shutdown);
    tokio::select! {
     result=&mut task=>{drop(guard);result.map_err(PatreonEvidenceWorkerError::caused)?.map_err(PatreonEvidenceWorkerError::caused)},
     signal=&mut shutdown=>{drop(guard);let result=task.await.map_err(PatreonEvidenceWorkerError::caused)?;signal.map_err(PatreonEvidenceWorkerError::caused)?;result.map_err(PatreonEvidenceWorkerError::caused)},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Mock, Step};
    use serde_json::json;
    #[tokio::test]
    async fn dedicated_worker_starts_with_a_fresh_cursor_and_stop_wakes_the_idle_wait() {
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let ready = std::sync::Mutex::new(Some(ready_tx));
        let backend = Mock::new_with_observer(
            vec![Step::json("/rest/v1/rpc/read_patreon_work", json!([]))],
            move |_| {
                if let Some(tx) = ready.lock().unwrap().take() {
                    let _ = tx.send(());
                }
            },
        );
        let config = BackendConfig::new(&backend.base, "sb_secret_test_only").unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            run(config, "patreon_private".into(), async {
                ready_rx.await.unwrap();
                Ok(())
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let calls = backend.finish();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&calls[0].2).unwrap(),
            json!({"p_kind":"evidence","p_after":null,"p_limit":1,"p_bucket":"patreon_private"})
        );
    }
    #[tokio::test]
    async fn dedicated_worker_rejects_invalid_configuration_without_any_network_request() {
        let backend = Mock::new(vec![]);
        let config = BackendConfig::new(&backend.base, "sb_secret_test_only").unwrap();
        assert!(
            run(config, "public/bucket".into(), std::future::pending())
                .await
                .is_err()
        );
        assert!(backend.finish().is_empty());
    }
}
