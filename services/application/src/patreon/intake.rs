//! Authenticated routing/evidence intake only. No membership, payment or grant acknowledgment.
use super::{PatreonError, PatreonWebhookVerifier};
use crate::{
    backend::{BackendClient, BackendConfig, BackendError},
    private_storage::{self, EvidenceCapture, StorageError},
};
use chrono::{Datelike, Utc};
use serde_json::json;
use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

/// Endpoint-specific private configuration. Construction performs no I/O.
pub(crate) struct IntakeConfig {
    verifier: PatreonWebhookVerifier,
    bucket: String,
}
impl IntakeConfig {
    pub fn new(campaign: String, secret: String, bucket: String) -> Result<Self, PatreonError> {
        if !private_storage::bucket_name(&bucket) {
            return Err(PatreonError::Configuration);
        }
        Ok(Self {
            verifier: PatreonWebhookVerifier::new(secret, campaign)?,
            bucket,
        })
    }
    pub fn from_env() -> Result<Self, PatreonError> {
        let read = |name| std::env::var(name).map_err(|_| PatreonError::Configuration);
        Self::new(
            read("SPELLWAKE_PATREON_CAMPAIGN_ID")?,
            read("SPELLWAKE_PATREON_WEBHOOK_SECRET")?,
            read("SPELLWAKE_PATREON_EVIDENCE_BUCKET")?,
        )
    }
}
pub(crate) enum IntakeError {
    Authentication(PatreonError),
    Backend(BackendError),
    Storage(StorageError),
    Contract,
}
impl fmt::Display for IntakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("patreon notification intake is unresolved")
    }
}
impl fmt::Debug for IntakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for IntakeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authentication(e) => Some(e),
            Self::Backend(e) => Some(e),
            Self::Storage(e) => Some(e),
            Self::Contract => None,
        }
    }
}
impl From<BackendError> for IntakeError {
    fn from(e: BackendError) -> Self {
        Self::Backend(e)
    }
}
impl From<StorageError> for IntakeError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

/// Reuses accepted private source rows and shared Storage mechanics; no provider request is made.
pub(crate) struct IntakeService {
    evidence: EvidenceService,
    config: Arc<IntakeConfig>,
}
impl IntakeService {
    pub fn new(backend: BackendConfig, config: Arc<IntakeConfig>) -> Result<Self, IntakeError> {
        Ok(Self {
            evidence: EvidenceService::new(
                backend,
                config.bucket.clone(),
                Arc::new(AtomicBool::new(false)),
            )?,
            config,
        })
    }
    /// One received attempt retains its UUID for explicit same-attempt recovery.
    /// Invalid signatures are rejected before any capture or Storage request.
    pub fn ingest(&self, body: &[u8], signature: &str, delivery: Uuid) -> Result<(), IntakeError> {
        let notice = self
            .config
            .verifier
            .verify(body, signature)
            .map_err(IntakeError::Authentication)?;
        if delivery.is_nil() {
            return Err(IntakeError::Contract);
        }
        let ticket:EvidenceCapture=self.evidence.backend.rpc_object("capture_patreon_notification",&json!({"p_delivery":delivery,"p_capture":{
            "version":1,"kind":"webhook","source":"patreon-v2-member-notification-v1","campaignId":notice.campaign_id(),"userId":notice.user_id(),"memberId":notice.member_id(),"bodyHash":notice.body_sha256()
        }}))?;
        if ticket.id != delivery
            || ticket.payload_ref != path(delivery, notice.body_sha256())?
            || !matches!(
                ticket.evidence_state.as_str(),
                "pending" | "retained" | "expired"
            )
            || !(1..=9999).contains(&ticket.expires_at.year())
            || !ticket
                .expires_at
                .timestamp_subsec_nanos()
                .is_multiple_of(1000)
        {
            return Err(IntakeError::Contract);
        }
        // A durable routing signal is independent of raw availability or processing.
        // Due/retained/expired replay never fabricates another readback confirmation.
        if ticket.evidence_state != "pending" || ticket.expires_at <= Utc::now() {
            return Ok(());
        }
        private_storage::retain_bytes(
            &self.evidence.backend,
            &self.config.bucket,
            &ticket.payload_ref,
            body,
        )?;
        let confirmed: bool = self.evidence.backend.rpc(
            "confirm_patreon_evidence",
            &json!({"p_event":delivery,"p_hash":notice.body_sha256()}),
        )?;
        if !confirmed {
            return Err(IntakeError::Contract);
        }
        Ok(())
    }
}

/// Independent retention consumer; it needs no webhook secret or creator token.
pub(crate) struct EvidenceService {
    backend: BackendClient,
    bucket: String,
    stopped: Arc<AtomicBool>,
}
impl EvidenceService {
    pub fn new(
        backend: BackendConfig,
        bucket: String,
        stopped: Arc<AtomicBool>,
    ) -> Result<Self, IntakeError> {
        if !private_storage::bucket_name(&bucket) {
            return Err(IntakeError::Contract);
        }
        Ok(Self {
            backend: BackendClient::new(
                backend,
                "spellwake-patreon-evidence",
                Duration::from_secs(60),
            )?,
            bucket,
            stopped,
        })
    }
    fn running(&self) -> Result<(), IntakeError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(IntakeError::Contract);
        }
        Ok(())
    }
    /// Visit one source; failures remain in durable work for the next full sweep.
    pub fn work(&self, after: Option<Uuid>) -> Result<Option<Uuid>, IntakeError> {
        self.running()?;
        if after.is_some_and(|id| id.is_nil()) {
            return Err(IntakeError::Contract);
        }
        let ids: Vec<Uuid> = self.backend.rpc(
            "read_patreon_work",
            &json!({"p_kind":"evidence","p_after":after,"p_limit":1,"p_bucket":self.bucket}),
        )?;
        if ids.len() > 1
            || ids
                .iter()
                .any(|id| id.is_nil() || after.is_some_and(|cursor| *id <= cursor))
        {
            return Err(IntakeError::Contract);
        }
        let Some(id) = ids.first().copied() else {
            return Ok(None);
        };
        self.running()?;
        if self.expire(id).is_err() && !self.stopped.load(Ordering::Acquire) {
            eprintln!("patreon_evidence event=source_unresolved");
        }
        self.running()?;
        Ok(Some(id))
    }
    /// Execute a due source ticket, positively verify exact-object absence, then acknowledge it.
    /// Missing/not-yet-due tickets produce no deletion or completion request.
    pub fn expire(&self, event: Uuid) -> Result<(), IntakeError> {
        self.running()?;
        if event.is_nil() {
            return Err(IntakeError::Contract);
        }
        let Some(ticket) = self
            .backend
            .rpc_optional_object::<private_storage::EvidenceExpiry>(
                "begin_patreon_evidence_expiry",
                &json!({"p_event":event}),
            )?
        else {
            return Ok(());
        };
        if ticket.payload_ref != path(event, &ticket.body_hash)? {
            return Err(IntakeError::Contract);
        }
        self.running()?;
        private_storage::delete_private_object(&self.backend, &self.bucket, &ticket.payload_ref)?;
        self.running()?;
        self.backend
            .rpc_void("finish_patreon_evidence_expiry", &json!({"p_event":event}))?;
        Ok(())
    }
}
fn path(event: Uuid, hash: &str) -> Result<String, IntakeError> {
    if event.is_nil()
        || hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err(IntakeError::Contract);
    }
    Ok(format!("payments/patreon/{event}/{hash}.json"))
}

#[cfg(test)]
#[path = "intake_tests.rs"]
mod tests;
