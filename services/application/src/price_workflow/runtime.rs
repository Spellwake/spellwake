//! Explicit runtime configuration; never discovers another project's profiles.

use super::maintenance::PredictionMaintenanceWorkflow;
use super::probe::PriceProbeWorkflow;
use super::{PRICE_TASK_QUEUE, PriceActivities, PriceCalendarWorkflow, PriceDayWorkflow};
use crate::price_collector::CollectorConfig;
use crate::temporal_runtime::TemporalConfig;
use std::{fmt, time::Duration};
use temporalio_sdk::{
    Runtime, Worker, WorkerOptions,
    runtime::worker_tuner::{FixedSizeSlotSupplier, TunerHolder},
};

#[derive(Debug)]
pub struct PriceRuntimeError;
impl fmt::Display for PriceRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("price workflow runtime failed; check private worker configuration and service availability")
    }
}
impl std::error::Error for PriceRuntimeError {}

pub async fn run_worker() -> Result<(), PriceRuntimeError> {
    // Fail configuration before polling; no source acquisition occurs here.
    tokio::task::spawn_blocking(CollectorConfig::from_env)
        .await
        .map_err(|_| PriceRuntimeError)?
        .map_err(|_| PriceRuntimeError)?;
    let client = TemporalConfig::from_env()
        .map_err(|_| PriceRuntimeError)?
        .connect("spellwake-price-v1")
        .await
        .map_err(|_| PriceRuntimeError)?;
    let runtime = Runtime::from_current_tokio(Default::default()).map_err(|_| PriceRuntimeError)?;
    let tuner = TunerHolder::builder()
        .workflow_task_slot_supplier(FixedSizeSlotSupplier::new(4))
        .activity_task_slot_supplier(FixedSizeSlotSupplier::new(2))
        .local_activity_task_slot_supplier(FixedSizeSlotSupplier::new(1))
        .nexus_task_slot_supplier(FixedSizeSlotSupplier::new(1))
        .build();
    let options = WorkerOptions::new(PRICE_TASK_QUEUE)
        .tuner(tuner)
        .client_identity_override("spellwake-price-v1")
        .max_cached_workflows(16)
        .graceful_shutdown_period(Duration::from_secs(30))
        .register_workflow::<PriceProbeWorkflow>()
        .map_err(|_| PriceRuntimeError)?
        .register_workflow::<PriceDayWorkflow>()
        .map_err(|_| PriceRuntimeError)?
        .register_workflow::<PriceCalendarWorkflow>()
        .map_err(|_| PriceRuntimeError)?
        .register_workflow::<PredictionMaintenanceWorkflow>()
        .map_err(|_| PriceRuntimeError)?
        .register_activities(PriceActivities)
        .build();
    let mut worker = Worker::new(&runtime, client, options).map_err(|_| PriceRuntimeError)?;
    let shutdown = worker.shutdown_handle();
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown();
        }
    });
    let result = worker.run().await.map_err(|_| PriceRuntimeError);
    signal.abort();
    result
}
