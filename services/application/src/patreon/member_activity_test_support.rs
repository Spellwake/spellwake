//! Shared passive cancellation observation and release ownership for member runtime tests.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use temporalio_sdk::activities::ActivityError;
use temporalio_sdk::interceptors::{
    ActivityInboundInterceptor, ExecuteActivityInput, ExecuteActivityOutput, Next,
};
pub(super) struct CancellationObservation {
    pub(super) activity_name: &'static str,
    pub(super) seen: Arc<AtomicBool>,
    pub(super) notify: Arc<tokio::sync::Notify>,
}
impl ActivityInboundInterceptor for CancellationObservation {
    fn execute_activity<'a>(
        &'a self,
        input: ExecuteActivityInput,
        next: Next<'a, ExecuteActivityInput, ExecuteActivityOutput<'a>>,
    ) -> ExecuteActivityOutput<'a> {
        let delivery = input.activity_info().activity_type == self.activity_name;
        Box::pin(async move {
            let result = next.run(input).await;
            if delivery && matches!(&result, Err(ActivityError::Cancelled { .. })) {
                self.seen.store(true, Ordering::Release);
                self.notify.notify_one();
            }
            result
        })
    }
}
pub(super) struct ReleaseGate(pub(super) Option<mpsc::Sender<()>>);
impl ReleaseGate {
    pub(super) fn release(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for ReleaseGate {
    fn drop(&mut self) {
        self.release();
    }
}
