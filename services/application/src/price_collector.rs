//! Private worker orchestration. Source objects are retained and read back before
//! the single database slot decision; the winning receipt remains authoritative.

use crate::backend::{BackendClient, BackendConfig, BackendError};
use crate::price_source::{
    EvidenceHash, QualifiedArtifact, SourceError, extract_retained, is_all_cards_uri,
    qualify_current, verify_retained_index,
};
use chrono::{DateTime, Duration, NaiveDate, Timelike, Utc};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration as Timeout,
};
use uuid::Uuid;

const MAX_JSON: u64 = 1024 * 1024;

pub mod maintenance;

/// Private transport failure retaining diagnostics without exposing backend fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectorBackendFailure(BackendError);
impl fmt::Display for CollectorBackendFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
impl std::error::Error for CollectorBackendFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Opaque storage cause shared by clones for private local diagnostics.
/// Formatting never exposes paths, transport details or response content.
/// Equality compares the outer storage classification, not cause identity.
#[derive(Clone)]
pub struct CollectorStorageFailure(Arc<dyn std::error::Error + Send + Sync>);
impl fmt::Debug for CollectorStorageFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CollectorStorageFailure")
    }
}
impl PartialEq for CollectorStorageFailure {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
impl Eq for CollectorStorageFailure {}

/// Safe worker classification; diagnostic causes stay local and are never workflow payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectorError {
    Configuration,
    Backend,
    BackendCause(CollectorBackendFailure),
    Storage(CollectorStorageFailure),
    RetentionMismatch,
    InvalidManifest,
    InvalidCursor,
    NotDue,
    WindowExpired,
    PriceUnavailable,
    Source(SourceError),
}
impl fmt::Display for CollectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "price collector configuration is invalid",
            Self::Backend | Self::BackendCause(_) => {
                "price collector source disposition could not be established"
            }
            Self::Storage(_) => "private price evidence retention failed",
            Self::RetentionMismatch => "retained evidence does not match the verified source bytes",
            Self::InvalidManifest => "established price source manifest is invalid",
            Self::InvalidCursor => "prediction maintenance cursor is invalid",
            Self::NotDue => "daily price acquisition has not started",
            Self::WindowExpired => {
                "daily price acquisition window ended before retention completed"
            }
            Self::PriceUnavailable => "selected source has no requested printing",
            Self::Source(_) => "price source acquisition failed",
        })
    }
}
impl std::error::Error for CollectorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::BackendCause(error) => Some(error),
            Self::Storage(cause) => Some(cause.0.as_ref()),
            _ => None,
        }
    }
}
impl CollectorError {
    pub(crate) fn storage(cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Storage(CollectorStorageFailure(Arc::new(cause)))
    }
}
impl From<BackendError> for CollectorError {
    fn from(error: BackendError) -> Self {
        match error {
            BackendError::Configuration => Self::Configuration,
            error => Self::BackendCause(CollectorBackendFailure(error)),
        }
    }
}

// SQL returns bare UUID JSON strings for these receipts, not an object/envelope.
// Reuse canonical nonnil parsing; typed Uuid alone would normalize aliases.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct RpcIdentity(#[serde(deserialize_with = "crate::identifiers::uuid")] Uuid);

impl From<SourceError> for CollectorError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

/// Runtime-only configuration for an operator-controlled backend process. It is
/// neither Debug nor Serialize, and never becomes a browser/native-app bundle.
pub struct CollectorConfig {
    backend: BackendConfig,
    bucket: String,
    stage: PathBuf,
}
impl CollectorConfig {
    pub fn from_env() -> Result<Self, CollectorError> {
        Self::new(
            &std::env::var("SPELLWAKE_SUPABASE_URL").map_err(|_| CollectorError::Configuration)?,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY")
                .map_err(|_| CollectorError::Configuration)?,
            &std::env::var("SPELLWAKE_PRICE_BUCKET").map_err(|_| CollectorError::Configuration)?,
            PathBuf::from(
                std::env::var_os("SPELLWAKE_PRICE_STAGE").ok_or(CollectorError::Configuration)?,
            ),
        )
    }
    pub fn new(
        base: &str,
        key: &str,
        bucket: &str,
        stage: PathBuf,
    ) -> Result<Self, CollectorError> {
        let backend = BackendConfig::new(base, key).map_err(|_| CollectorError::Configuration)?;
        if bucket.is_empty()
            || bucket.len() > 80
            || !bucket
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(CollectorError::Configuration);
        }
        Ok(Self {
            backend,
            bucket: bucket.into(),
            stage,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactManifest {
    version: String,
    slot: NaiveDate,
    source: String,
    hash: EvidenceHash,
    artifact_ref: String,
    metadata_hash: EvidenceHash,
    metadata_ref: String,
    source_updated_at: DateTime<Utc>,
    acquired_at: DateTime<Utc>,
    record_count: String,
    download_uri: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MissingManifest {
    version: String,
    slot: NaiveDate,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SlotManifest {
    Valid(ArtifactManifest),
    Missing(MissingManifest),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstablishedSlot {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub manifest: SlotManifest,
}
impl EstablishedSlot {
    fn validate(&self, expected: NaiveDate) -> Result<(), CollectorError> {
        let (version, slot) = match &self.manifest {
            SlotManifest::Missing(m) => (&m.version, m.slot),
            SlotManifest::Valid(m) => {
                let start = m
                    .slot
                    .and_hms_opt(12, 0, 0)
                    .ok_or(CollectorError::InvalidManifest)?
                    .and_utc();
                let count = m
                    .record_count
                    .parse::<u64>()
                    .map_err(|_| CollectorError::InvalidManifest)?;
                if m.source != "scryfall:all_cards"
                    || m.artifact_ref != format!("prices/scryfall/{}.jsonl.gz", m.hash.hex())
                    || m.metadata_ref
                        != format!("prices/scryfall/metadata/{}.json", m.metadata_hash.hex())
                    || m.acquired_at < start
                    || m.acquired_at >= start + Duration::hours(1)
                    || m.source_updated_at > m.acquired_at
                    || m.source_updated_at < m.acquired_at - Duration::hours(24)
                    || count == 0
                    || count > 5_000_000
                    || count.to_string() != m.record_count
                    || !is_all_cards_uri(&m.download_uri)
                {
                    return Err(CollectorError::InvalidManifest);
                }
                (&m.version, m.slot)
            }
        };
        if self.id.is_nil() || version != "scryfall-slot-v1" || slot != expected {
            return Err(CollectorError::InvalidManifest);
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionReport {
    pub manifest_id: Uuid,
    pub processed: u32,
    pub more: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExtractionTarget {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    market_id: Uuid,
    // External Scryfall printing UUID selected by SQL, distinct from app.printing.id.
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    printing_ref: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExtractionPage {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    items: Vec<ExtractionTarget>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    next_cursor: Option<Uuid>,
}

struct ExtractionStage(PathBuf);
impl ExtractionStage {
    fn new(root: &Path) -> Result<Self, CollectorError> {
        fs::create_dir_all(root).map_err(CollectorError::storage)?;
        let path = fs::canonicalize(root)
            .map_err(CollectorError::storage)?
            .join(format!("retained-{}", Uuid::new_v4()));
        fs::create_dir(&path).map_err(CollectorError::storage)?;
        Ok(Self(path))
    }
}
impl Drop for ExtractionStage {
    fn drop(&mut self) {
        if fs::remove_dir_all(&self.0).is_err() {
            eprintln!("price_collector event=extraction_cache_cleanup_failed");
        }
    }
}

pub struct PriceCollector {
    bucket: String,
    stage: PathBuf,
    backend: BackendClient,
}
impl PriceCollector {
    pub fn new(config: CollectorConfig) -> Result<Self, CollectorError> {
        let CollectorConfig {
            backend,
            bucket,
            stage,
        } = config;
        let backend = BackendClient::new(
            backend,
            "Spellwake-Private-Price-Collector/0.1",
            Timeout::from_secs(20 * 60),
        )
        .map_err(|_| CollectorError::Configuration)?;
        Ok(Self {
            bucket,
            stage,
            backend,
        })
    }
    fn rpc<T: DeserializeOwned>(&self, name: &str, payload: &Value) -> Result<T, CollectorError> {
        self.backend
            .rpc(name, payload)
            .map_err(CollectorError::from)
    }
    pub fn read_slot(&self, slot: NaiveDate) -> Result<Option<EstablishedSlot>, CollectorError> {
        let selected = self
            .rpc::<Option<crate::provider_json::Object<EstablishedSlot>>>(
                "read_established_price_slot",
                &json!({"p_slot":slot}),
            )?
            .map(|record| record.0);
        if let Some(record) = &selected {
            record.validate(slot)?;
        }
        Ok(selected)
    }
    fn private_bucket(&self) -> Result<(), CollectorError> {
        crate::private_storage::private_bucket(&self.backend, &self.bucket)
            .map_err(CollectorError::storage)
    }
    fn object_url(&self, key: &str, authenticated: bool) -> Result<Url, CollectorError> {
        crate::private_storage::object_url(&self.backend, &self.bucket, key, authenticated)
            .map_err(CollectorError::storage)
    }
    fn retain(
        &self,
        path: &Path,
        key: &str,
        hash: &EvidenceHash,
        size: u64,
        stage: &Path,
        kind: &str,
    ) -> Result<(), CollectorError> {
        crate::private_storage::retain_file(
            &self.backend,
            &self.bucket,
            stage,
            &crate::private_storage::RetainedFile {
                key,
                path,
                hash,
                size,
                content_type: kind,
            },
        )
        .map_err(|error| match error {
            crate::private_storage::StorageError::Mismatch => CollectorError::RetentionMismatch,
            crate::private_storage::StorageError::Source(source) => CollectorError::Source(source),
            error => CollectorError::storage(error),
        })
    }
    fn decide(
        &self,
        slot: NaiveDate,
        payload: Option<Value>,
    ) -> Result<EstablishedSlot, CollectorError> {
        let committed: Result<RpcIdentity, _> = self.rpc(
            "commit_price_slot",
            &json!({"p_slot":slot,"p_artifact":payload}),
        );
        // Whether its own call succeeded or acknowledgement was lost, read the
        // source's actual first-writer decision. Never assume this candidate won.
        if committed.is_err() {
            eprintln!("price_collector event=slot_ack_unconfirmed");
        }
        let established = self.read_slot(slot)?;
        if let Some(established) = established {
            if committed.as_ref().is_ok_and(|id| id.0 != established.id) {
                return Err(CollectorError::InvalidManifest);
            }
            return Ok(established);
        }
        match committed {
            Err(error) => Err(error),
            Ok(_) => Err(CollectorError::Backend),
        }
    }
    fn select_candidate(
        &self,
        slot: NaiveDate,
        candidate: QualifiedArtifact,
    ) -> Result<EstablishedSlot, CollectorError> {
        candidate.metadata.validate(candidate.acquired_at)?;
        if candidate.acquired_at.date_naive() != slot || candidate.acquired_at.hour() != 12 {
            return Err(CollectorError::WindowExpired);
        }
        let stage = candidate
            .artifact_path
            .parent()
            .ok_or(CollectorError::Configuration)?;
        self.private_bucket()?;
        let metadata_size = fs::metadata(&candidate.metadata_path)
            .map_err(CollectorError::storage)?
            .len();
        if metadata_size > MAX_JSON
            || candidate.compressed_bytes != candidate.metadata.compressed_size
        {
            return Err(CollectorError::RetentionMismatch);
        }
        self.retain(
            &candidate.metadata_path,
            &candidate.metadata_ref(),
            &candidate.metadata_hash,
            metadata_size,
            stage,
            "application/json",
        )?;
        self.retain(
            &candidate.artifact_path,
            &candidate.artifact_ref(),
            &candidate.hash,
            candidate.compressed_bytes,
            stage,
            "application/gzip",
        )?;
        self.decide(slot,Some(json!({"hash":candidate.hash,"artifactRef":candidate.artifact_ref(),
         "metadataHash":candidate.metadata_hash,"metadataRef":candidate.metadata_ref(),"downloadUri":candidate.metadata.jsonl_download_uri,
         "sourceUpdatedAt":candidate.metadata.updated_at,"acquiredAt":candidate.acquired_at,"recordCount":candidate.record_count.to_string()})))
    }
    fn download_retained(
        &self,
        key: &str,
        path: &Path,
        maximum: u64,
        exact: bool,
    ) -> Result<(), CollectorError> {
        let mut response = self
            .backend
            .request(Method::GET, self.object_url(key, true)?)
            .map_err(CollectorError::storage)?
            .header("Accept-Encoding", "identity")
            .send()
            .map_err(|error| CollectorError::storage(error.without_url()))?;
        if !response.status().is_success() {
            return Err(CollectorError::storage(
                crate::private_storage::StorageError::Status(response.status()),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > maximum || (exact && length != maximum))
        {
            return Err(CollectorError::RetentionMismatch);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(CollectorError::storage)?;
        let bytes = io::copy(&mut response.by_ref().take(maximum + 1), &mut file)
            .map_err(CollectorError::storage)?;
        file.sync_all().map_err(CollectorError::storage)?;
        if bytes > maximum || (exact && bytes != maximum) {
            return Err(CollectorError::RetentionMismatch);
        }
        Ok(())
    }
    fn selected_cards(
        &self,
        manifest: &ArtifactManifest,
        wanted: &std::collections::HashSet<Uuid>,
    ) -> Result<std::collections::HashMap<Uuid, Value>, CollectorError> {
        let stage = ExtractionStage::new(&self.stage)?;
        let index = stage.0.join("index.json");
        self.download_retained(&manifest.metadata_ref, &index, MAX_JSON, false)?;
        let metadata = verify_retained_index(&index, &manifest.metadata_hash)?;
        metadata.validate(manifest.acquired_at)?;
        if metadata.updated_at != manifest.source_updated_at
            || metadata.jsonl_download_uri != manifest.download_uri
        {
            return Err(CollectorError::InvalidManifest);
        }
        let artifact = stage.0.join("cards.jsonl.gz");
        self.download_retained(
            &manifest.artifact_ref,
            &artifact,
            metadata.compressed_size,
            true,
        )?;
        let records = manifest
            .record_count
            .parse()
            .map_err(|_| CollectorError::InvalidManifest)?;
        let scanned = extract_retained(&artifact, &manifest.hash, records, wanted)?;
        Ok(scanned.cards)
    }
    /// A maximum of1000 pending markets is processed per invocation. Completed
    /// rows disappear from work reads, so a partial retry resumes from evidence
    /// rather than replaying a source acquisition or releasing any stake.
    pub fn extract_slot(&self, slot: NaiveDate) -> Result<ExtractionReport, CollectorError> {
        let selected = self.read_slot(slot)?.ok_or(CollectorError::Backend)?;
        let mut targets = Vec::new();
        let mut wanted = std::collections::HashSet::new();
        let mut after = None;
        let mut more = false;
        for _ in 0..10 {
            let page: crate::provider_json::Object<ExtractionPage> = self.rpc(
                "read_prediction_extraction_page",
                &json!({"p_slot":slot,"p_after":after,"p_limit":100}),
            )?;
            let page = page.0;
            if page.items.len() > 100 {
                return Err(CollectorError::Backend);
            }
            let mut previous = after;
            for target in &page.items {
                if previous.is_some_and(|last| target.market_id <= last) {
                    return Err(CollectorError::Backend);
                }
                let printing = target.printing_ref;
                wanted.insert(printing);
                previous = Some(target.market_id);
            }
            if page.next_cursor.is_some() != (page.items.len() == 100)
                || (page.next_cursor.is_some() && page.next_cursor != previous)
            {
                return Err(CollectorError::Backend);
            }
            more = page.next_cursor.is_some();
            after = page.next_cursor;
            targets.extend(page.items);
            if !more {
                break;
            }
        }
        let cards = if targets.is_empty() {
            std::collections::HashMap::new()
        } else {
            match &selected.manifest {
                SlotManifest::Missing(_) => std::collections::HashMap::new(),
                SlotManifest::Valid(manifest) => self.selected_cards(manifest, &wanted)?,
            }
        };
        let mut processed = 0;
        for target in targets {
            let printing = target.printing_ref;
            let _:RpcIdentity=self.rpc("record_prediction_observation",&json!({"p_market":target.market_id,"p_manifest":selected.id,"p_card":cards.get(&printing)}))?;
            processed += 1;
        }
        Ok(ExtractionReport {
            manifest_id: selected.id,
            processed,
            more,
        })
    }
    /// A probe contains literal fields from one exact selected printing. The
    /// publication function still checks chosen finish, field and usable price.
    pub fn probe_printing(&self, slot: NaiveDate, printing: Uuid) -> Result<Uuid, CollectorError> {
        let selected = self.read_slot(slot)?.ok_or(CollectorError::Backend)?;
        let SlotManifest::Valid(manifest) = &selected.manifest else {
            return Err(CollectorError::PriceUnavailable);
        };
        let now = Utc::now();
        if manifest.acquired_at > now
            || now.signed_duration_since(manifest.acquired_at) > Duration::hours(24)
        {
            return Err(CollectorError::Source(SourceError::StaleArtifact));
        }
        let existing: Option<RpcIdentity> = self.rpc(
            "read_established_price_probe",
            &json!({"p_manifest":selected.id,"p_printing":printing}),
        )?;
        if let Some(id) = existing {
            return Ok(id.0);
        }
        let cards = self.selected_cards(manifest, &std::collections::HashSet::from([printing]))?;
        let card = cards
            .get(&printing)
            .ok_or(CollectorError::PriceUnavailable)?;
        self.rpc::<RpcIdentity>(
            "record_price_probe",
            &json!({"p_manifest":selected.id,"p_card":card}),
        )
        .map(|id| id.0)
    }

    /// One activity invocation, not a scheduler. Outside the acquisition window
    /// it never fetches a source or invents an acquisition time.
    pub fn collect_current(&self) -> Result<EstablishedSlot, CollectorError> {
        self.collect_at(Utc::now(), || qualify_current(&self.stage))
    }
    /// Bind a durable task to its intended date. Late delivery finalizes that
    /// missed slot; it must never acquire today's file under yesterday's task.
    pub fn collect_slot(&self, slot: NaiveDate) -> Result<EstablishedSlot, CollectorError> {
        self.collect_slot_at(slot, Utc::now(), || qualify_current(&self.stage))
    }
    fn collect_at(
        &self,
        now: DateTime<Utc>,
        acquire: impl FnOnce() -> Result<QualifiedArtifact, SourceError>,
    ) -> Result<EstablishedSlot, CollectorError> {
        self.collect_slot_at(now.date_naive(), now, acquire)
    }
    fn collect_slot_at(
        &self,
        slot: NaiveDate,
        now: DateTime<Utc>,
        acquire: impl FnOnce() -> Result<QualifiedArtifact, SourceError>,
    ) -> Result<EstablishedSlot, CollectorError> {
        if let Some(established) = self.read_slot(slot)? {
            return Ok(established);
        }
        let start = slot
            .and_hms_opt(12, 0, 0)
            .ok_or(CollectorError::InvalidManifest)?
            .and_utc();
        if now < start {
            return Err(CollectorError::NotDue);
        }
        if now >= start + Duration::hours(1) {
            return self.decide(slot, None);
        }
        self.select_candidate(slot, acquire()?)
    }
}
#[cfg(test)]
#[path = "price_collector/tests.rs"]
mod tests;
