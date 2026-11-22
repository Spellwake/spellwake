//! Scale-to-zero background reconciler triggered by Supabase pg_cron only when
//! durable work rows exist, eliminating the need for Temporal Cloud ($100/mo).
use super::*;
use crate::{
    deletion_delivery::DeletionDelivery,
    patreon::{PatreonRefreshService, intake::EvidenceService},
    stripe::service::StripeService,
};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MAX_RECONCILE_BODY: usize = 1024;
const MAX_BATCHES_PER_DOMAIN: usize = 20;
const RECONCILE_BUDGET: Duration = Duration::from_secs(20);

fn constant_time_secret_eq(expected: &str, provided: &str) -> bool {
    let a = Sha256::digest(expected.as_bytes());
    let b = Sha256::digest(provided.as_bytes());
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0 && expected.len() == provided.len()
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconcileRequest {
    #[serde(default)]
    domains: Option<Vec<String>>,
}

pub(super) async fn reconcile(State(state): State<ApiState>, request: Request) -> Response {
    if request.uri().query().is_some() {
        return fail(StatusCode::BAD_REQUEST, "query", "unsupported");
    }
    let Some(expected_secret) = state.config.reconcile_secret.as_deref() else {
        return unavailable();
    };
    let mut headers = request.headers().get_all("x-spellwake-reconcile").iter();
    let provided = headers.next().and_then(|v| v.to_str().ok());
    if provided.is_none() || headers.next().is_some() {
        return fail(StatusCode::UNAUTHORIZED, "reconcile", "unauthorized");
    }
    if !constant_time_secret_eq(expected_secret, provided.unwrap_or("")) {
        return fail(StatusCode::UNAUTHORIZED, "reconcile", "unauthorized");
    }
    if !is_json_content_type(request.headers()) {
        return fail(StatusCode::BAD_REQUEST, "body", "json_required");
    }
    let bytes = match to_bytes(request.into_body(), MAX_RECONCILE_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid_or_too_large"),
    };
    let parsed: ReconcileRequest = match parse_object(&bytes) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "body", "invalid"),
    };
    let domains: Vec<String> = parsed
        .domains
        .unwrap_or_else(|| vec!["stripe".into(), "patreon".into(), "deletion".into()]);
    if domains.len() > 8
        || domains
            .iter()
            .any(|d| !matches!(d.as_str(), "stripe" | "patreon" | "deletion"))
    {
        return fail(StatusCode::BAD_REQUEST, "domains", "invalid");
    }
    let run_stripe = domains.iter().any(|d| d == "stripe");
    let run_patreon = domains.iter().any(|d| d == "patreon");
    let run_deletion = domains.iter().any(|d| d == "deletion");

    state
        .blocking(move |config| {
            let started = Instant::now();
            let stopped = Arc::new(AtomicBool::new(false));
            let backend_cfg = match config.backend_config() {
                Ok(cfg) => cfg,
                Err(_) => return unavailable(),
            };

            let mut stripe_completed: u64 = 0;
            let mut stripe_errors: u64 = 0;
            if run_stripe
                && let Some(stripe_cfg) = config.stripe.clone()
                && started.elapsed() < RECONCILE_BUDGET
            {
                if let Ok(service) = StripeService::new(backend_cfg.clone(), stripe_cfg) {
                    for kind in ["purchase", "event", "evidence"] {
                        let mut cursor = None;
                        for _ in 0..MAX_BATCHES_PER_DOMAIN {
                            if started.elapsed() >= RECONCILE_BUDGET {
                                stopped.store(true, Ordering::Release);
                                break;
                            }
                            match service.work(kind, cursor) {
                                Ok(batch) => {
                                    stripe_completed += u64::from(batch.completed);
                                    if batch.tail || batch.cursor.is_none() {
                                        break;
                                    }
                                    cursor = batch.cursor;
                                }
                                Err(_) => {
                                    stripe_errors += 1;
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    stripe_errors += 1;
                }
            }

            let mut patreon_evidence_completed: u64 = 0;
            let mut patreon_refreshes_completed: u64 = 0;
            let mut patreon_errors: u64 = 0;
            if run_patreon && started.elapsed() < RECONCILE_BUDGET {
                if let Some(intake) = &config.patreon_intake {
                    if let Ok(evidence) = EvidenceService::new(
                        backend_cfg.clone(),
                        intake.bucket().to_owned(),
                        stopped.clone(),
                    ) {
                        let mut after = None;
                        for _ in 0..MAX_BATCHES_PER_DOMAIN {
                            if started.elapsed() >= RECONCILE_BUDGET {
                                stopped.store(true, Ordering::Release);
                                break;
                            }
                            match evidence.work(after) {
                                Ok(Some(next)) => {
                                    patreon_evidence_completed += 1;
                                    after = Some(next);
                                }
                                Ok(None) => break,
                                Err(_) => {
                                    patreon_errors += 1;
                                    break;
                                }
                            }
                        }
                    } else {
                        patreon_errors += 1;
                    }
                }
                if let Some(oauth) = &config.patreon
                    && started.elapsed() < RECONCILE_BUDGET
                {
                    if let Ok(refresh) =
                        PatreonRefreshService::new(backend_cfg.clone(), oauth.source().clone())
                    {
                        let refresh = refresh.with_stop(stopped.clone());
                        let mut after = None;
                        for _ in 0..MAX_BATCHES_PER_DOMAIN {
                            if started.elapsed() >= RECONCILE_BUDGET {
                                stopped.store(true, Ordering::Release);
                                break;
                            }
                            match refresh.discover(after) {
                                Ok(Some(task)) => {
                                    let assoc = task.association_id();
                                    match refresh.observe(assoc) {
                                        Ok(Some(attempt)) => {
                                            if refresh.commit(&attempt).is_ok() {
                                                patreon_refreshes_completed += 1;
                                            } else {
                                                patreon_errors += 1;
                                            }
                                        }
                                        Ok(None) => {}
                                        Err(_) => patreon_errors += 1,
                                    }
                                    after = Some(assoc);
                                }
                                Ok(None) => break,
                                Err(_) => {
                                    patreon_errors += 1;
                                    break;
                                }
                            }
                        }
                    } else {
                        patreon_errors += 1;
                    }
                }
            }

            let mut deletion_acknowledged: u64 = 0;
            let mut deletion_errors: u64 = 0;
            if run_deletion && started.elapsed() < RECONCILE_BUDGET {
                if let Ok(delivery) = DeletionDelivery::new(backend_cfg) {
                    let mut after = None;
                    for _ in 0..MAX_BATCHES_PER_DOMAIN {
                        if started.elapsed() >= RECONCILE_BUDGET {
                            stopped.store(true, Ordering::Release);
                            break;
                        }
                        match delivery.deliver_cancellable(after, &stopped) {
                            Ok(batch) => {
                                deletion_acknowledged += u64::from(batch.acknowledged);
                                if !batch.more || batch.next_cursor.is_none() {
                                    break;
                                }
                                after = batch.next_cursor;
                            }
                            Err(_) => {
                                deletion_errors += 1;
                                break;
                            }
                        }
                    }
                } else {
                    deletion_errors += 1;
                }
            }

            tracing::info!(
                stripe_completed,
                stripe_errors,
                patreon_evidence_completed,
                patreon_refreshes_completed,
                patreon_errors,
                deletion_acknowledged,
                deletion_errors,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "background reconcile sweep finished"
            );

            success(json!({
                "stripe": {
                    "configured": config.stripe.is_some(),
                    "completed": stripe_completed,
                    "errors": stripe_errors,
                },
                "patreon": {
                    "configured": config.patreon.is_some() || config.patreon_intake.is_some(),
                    "evidenceCompleted": patreon_evidence_completed,
                    "refreshesCompleted": patreon_refreshes_completed,
                    "errors": patreon_errors,
                },
                "deletion": {
                    "acknowledged": deletion_acknowledged,
                    "errors": deletion_errors,
                }
            }))
        })
        .await
}
