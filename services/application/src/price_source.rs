//! Complete retained Scryfall JSONL evidence, separate from daily sample acceptance.
//!
//! A successful file scan is not a published sample. Slot timing and the selected
//! immutable manifest still belong to the database and collector orchestration.

use chrono::{DateTime, Duration, Utc};
use flate2::bufread::MultiGzDecoder;
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration as StdDuration,
};
use uuid::Uuid;
mod consumed;

const BULK_INDEX: &str = "https://api.scryfall.com/bulk-data";
const ALL_CARDS_ID: &str = "922288cb-4bef-45e1-bb30-0c2bd3d3534f";
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const MAX_LINE_BYTES: usize = 262_144;
const MAX_COMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_RECORDS: usize = 5_000_000;
const MAX_TARGETS: usize = 100_000;

/// Opaque source cause retained for local diagnostics through [`std::error::Error::source`].
/// Clones share one cause. Formatting
/// never includes its potentially private filesystem path or serialization details.
#[derive(Clone)]
pub struct SourceFailure(Arc<dyn std::error::Error + Send + Sync>);
impl fmt::Debug for SourceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SourceFailure")
    }
}
/// Local status observation without response body, headers or URL.
#[derive(Debug)]
struct SourceStatusFailure(reqwest::StatusCode);
impl fmt::Display for SourceStatusFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "price source response returned HTTP {}",
            self.0.as_u16()
        )
    }
}
impl std::error::Error for SourceStatusFailure {}

/// Safe worker diagnostics never include response bodies, credentials or URLs.
/// Original transport, decoder and staging causes are available only through the local error chain.
/// Equality compares failure classification, not diagnostic cause identity.
#[derive(Debug, Clone)]
pub enum SourceError {
    Network,
    /// URL-free transport cause with the same classification as `Network`.
    NetworkCause(SourceFailure),
    RateLimited,
    InvalidIndex,
    /// Original metadata decoder cause with the same classification as `InvalidIndex`.
    InvalidIndexCause(SourceFailure),
    StaleArtifact,
    InvalidArtifact,
    /// Original artifact reader/decoder cause with the same classification as `InvalidArtifact`.
    InvalidArtifactCause(SourceFailure),
    BoundsExceeded,
    HashMismatch,
    Io(SourceFailure),
}
impl PartialEq for SourceError {
    fn eq(&self, other: &Self) -> bool {
        let classification = |error: &Self| match error {
            Self::NetworkCause(_) => std::mem::discriminant(&Self::Network),
            Self::InvalidIndexCause(_) => std::mem::discriminant(&Self::InvalidIndex),
            Self::InvalidArtifactCause(_) => std::mem::discriminant(&Self::InvalidArtifact),
            other => std::mem::discriminant(other),
        };
        classification(self) == classification(other)
    }
}
impl Eq for SourceError {}
impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Network | Self::NetworkCause(_) => "price source request failed",
            Self::RateLimited => {
                "price source rate limit requires at least 30 seconds before retry"
            }
            Self::InvalidIndex | Self::InvalidIndexCause(_) => {
                "price source metadata is invalid or unsupported"
            }
            Self::StaleArtifact => "price source artifact is stale or future dated",
            Self::InvalidArtifact | Self::InvalidArtifactCause(_) => {
                "price source artifact is incomplete or invalid"
            }
            Self::BoundsExceeded => "price source input exceeds worker bounds",
            Self::HashMismatch => "retained price source bytes do not match their evidence hash",
            Self::Io(_) => "price source staging failed",
        })
    }
}
impl std::error::Error for SourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(cause)
            | Self::NetworkCause(cause)
            | Self::InvalidIndexCause(cause)
            | Self::InvalidArtifactCause(cause) => Some(cause.0.as_ref()),
            _ => None,
        }
    }
}
impl SourceError {
    fn network(cause: reqwest::Error) -> Self {
        Self::NetworkCause(SourceFailure(Arc::new(cause.without_url())))
    }
    pub(crate) fn invalid_index(cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::InvalidIndexCause(SourceFailure(Arc::new(cause)))
    }
    pub(super) fn invalid_artifact(cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::InvalidArtifactCause(SourceFailure(Arc::new(cause)))
    }
    pub(crate) fn staging(cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Io(SourceFailure(Arc::new(cause)))
    }
}
impl From<io::Error> for SourceError {
    fn from(cause: io::Error) -> Self {
        Self::staging(cause)
    }
}

/// Canonical content identity cannot carry a malformed prefix or panic when
/// converted to a Storage object key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct EvidenceHash(String);
impl EvidenceHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn hex(&self) -> &str {
        self.0.trim_start_matches("sha256:")
    }
}
impl TryFrom<String> for EvidenceHash {
    type Error = SourceError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 71
            || !value.starts_with("sha256:")
            || !value.as_bytes()[7..]
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        {
            return Err(SourceError::HashMismatch);
        }
        Ok(Self(value))
    }
}
impl<'de> Deserialize<'de> for EvidenceHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Current source metadata is parsed without accepting a different bulk product
/// or silently falling back to an older JSON-array transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkArtifact {
    object: String,
    id: Uuid,
    #[serde(rename = "type")]
    kind: String,
    pub updated_at: DateTime<Utc>,
    pub jsonl_download_uri: String,
    pub compressed_size: u64,
}

pub(crate) fn is_all_cards_uri(value: &str) -> bool {
    value
        .strip_prefix("https://data.scryfall.io/all-cards/all-cards-")
        .and_then(|suffix| suffix.strip_suffix(".jsonl.gz"))
        .is_some_and(|stamp| stamp.len() == 14 && stamp.bytes().all(|byte| byte.is_ascii_digit()))
}

impl BulkArtifact {
    pub fn validate(&self, acquired_at: DateTime<Utc>) -> Result<(), SourceError> {
        if self.object != "bulk_data"
            || self.id.to_string() != ALL_CARDS_ID
            || self.kind != "all_cards"
            || !is_all_cards_uri(&self.jsonl_download_uri)
            || self.compressed_size == 0
            || self.compressed_size > MAX_COMPRESSED_BYTES
        {
            return Err(SourceError::InvalidIndex);
        }
        if self.updated_at > acquired_at
            || acquired_at.signed_duration_since(self.updated_at) > Duration::hours(24)
        {
            return Err(SourceError::StaleArtifact);
        }
        Ok(())
    }
}

/// Both raw index and compressed artifact survive local qualification. Production
/// orchestration must retain these exact bytes before committing a slot manifest.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualifiedArtifact {
    pub metadata: BulkArtifact,
    pub acquired_at: DateTime<Utc>,
    pub artifact_path: PathBuf,
    pub metadata_path: PathBuf,
    pub hash: EvidenceHash,
    pub metadata_hash: EvidenceHash,
    pub qualification_path: PathBuf,
    pub compressed_bytes: u64,
    pub decompressed_bytes: u64,
    pub record_count: u64,
}

impl QualifiedArtifact {
    pub fn artifact_ref(&self) -> String {
        format!("prices/scryfall/{}.jsonl.gz", self.hash.hex())
    }
    pub fn metadata_ref(&self) -> String {
        format!("prices/scryfall/metadata/{}.json", self.metadata_hash.hex())
    }
}

pub(crate) fn hash_text(bytes: &[u8]) -> EvidenceHash {
    let body: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    EvidenceHash(format!("sha256:{body}"))
}

/// Hashes compressed bytes rather than a decompressor's transformed response.
pub fn hash_file(path: &Path) -> Result<(EvidenceHash, u64), SourceError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_COMPRESSED_BYTES {
        return Err(SourceError::BoundsExceeded);
    }
    let mut reader = HashingReader::new(file);
    io::copy(&mut reader, &mut io::sink())?;
    Ok((hash_text(&reader.digest.finalize()), reader.bytes))
}

struct HashingReader<R> {
    inner: R,
    digest: Sha256,
    bytes: u64,
}
impl<R> HashingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
            bytes: 0,
        }
    }
}
impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.bytes = self
            .bytes
            .checked_add(read as u64)
            .filter(|bytes| *bytes <= MAX_COMPRESSED_BYTES)
            .ok_or_else(|| io::Error::other("compressed evidence bound exceeded"))?;
        self.digest.update(&buffer[..read]);
        Ok(read)
    }
}

/// Price data stays literal: missing, null and malformed fields are preserved so
/// the observation boundary can mark them unusable, never invent zero prices.
fn card_identity(value: &Value) -> Result<Uuid, SourceError> {
    if value.get("object").and_then(Value::as_str) != Some("card") {
        return Err(SourceError::InvalidArtifact);
    }
    let raw_id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or(SourceError::InvalidArtifact)?;
    let id = Uuid::parse_str(raw_id).map_err(SourceError::invalid_artifact)?;
    if id.to_string() != raw_id || id.is_nil() {
        return Err(SourceError::InvalidArtifact);
    }
    Ok(id)
}

fn project_card(id: Uuid, value: Value) -> Result<Value, SourceError> {
    let finishes = value
        .get("finishes")
        .and_then(Value::as_array)
        .filter(|items| items.iter().all(Value::is_string))
        .ok_or(SourceError::InvalidArtifact)?;
    let mut prices = serde_json::Map::new();
    for field in ["usd", "usd_foil", "usd_etched"] {
        if let Some(value) = value.get("prices").and_then(|prices| prices.get(field)) {
            prices.insert(field.to_owned(), value.clone());
        }
    }
    Ok(serde_json::json!({"object":"card","id":id,"finishes":finishes,"prices":prices}))
}

#[derive(Debug)]
pub struct ArtifactScan {
    pub hash: EvidenceHash,
    pub compressed_bytes: u64,
    pub record_count: u64,
    pub decompressed_bytes: u64,
    /// Only requested exact printing UUIDs are retained, never a full card catalogue.
    pub cards: HashMap<Uuid, Value>,
}

/// Reads all gzip members through their CRC/end markers. It never stops after
/// finding a wanted card; trailing corruption or duplicate UUIDs reject the file.
pub fn scan_artifact(path: &Path, wanted: &HashSet<Uuid>) -> Result<ArtifactScan, SourceError> {
    if wanted.len() > MAX_TARGETS {
        return Err(SourceError::BoundsExceeded);
    }
    let mut cards = HashMap::new();
    let evidence = visit_artifact(path, |id, value| {
        // Validate the price projection for every record, including unwanted ones.
        let projection = project_card(id, value)?;
        if wanted.contains(&id) {
            cards.insert(id, projection);
        }
        Ok(())
    })?;
    Ok(ArtifactScan {
        hash: evidence.hash,
        compressed_bytes: evidence.compressed_bytes,
        record_count: evidence.record_count,
        decompressed_bytes: evidence.decompressed_bytes,
        cards,
    })
}

pub(crate) struct ArtifactEvidence {
    pub hash: EvidenceHash,
    pub compressed_bytes: u64,
    pub record_count: u64,
    pub decompressed_bytes: u64,
}

/// Consumers may stage bounded local output, but must not publish from this callback.
/// Evidence is returned only after all members, CRC markers and identities are checked.
pub(crate) fn visit_artifact(
    path: &Path,
    mut visit: impl FnMut(Uuid, Value) -> Result<(), SourceError>,
) -> Result<ArtifactEvidence, SourceError> {
    if fs::metadata(path)?.len() > MAX_COMPRESSED_BYTES {
        return Err(SourceError::BoundsExceeded);
    }
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_COMPRESSED_BYTES {
        return Err(SourceError::BoundsExceeded);
    }
    let decoder = MultiGzDecoder::new(BufReader::new(HashingReader::new(file)));
    let mut decoded = BufReader::new(decoder);
    let mut identities = HashSet::new();
    let mut bytes = 0u64;
    let mut line = Vec::new();
    loop {
        line.clear();
        // take() bounds even a malicious line without a newline. The outer
        // reader retains any bytes after this line for the next iteration.
        let read = decoded
            .by_ref()
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(SourceError::invalid_artifact)?;
        if read == 0 {
            break;
        }
        if line.len() > MAX_LINE_BYTES {
            return Err(SourceError::BoundsExceeded);
        }
        bytes = bytes
            .checked_add(read as u64)
            .filter(|bytes| *bytes <= MAX_DECOMPRESSED_BYTES)
            .ok_or(SourceError::BoundsExceeded)?;
        consumed::validate(&line)?;
        let value: Value = serde_json::from_slice(&line).map_err(SourceError::invalid_artifact)?;
        let id = card_identity(&value)?;
        if identities.len() >= MAX_RECORDS {
            return Err(SourceError::BoundsExceeded);
        }
        if !identities.insert(id) {
            return Err(SourceError::InvalidArtifact);
        }
        visit(id, value)?;
    }
    if identities.is_empty() {
        return Err(SourceError::InvalidArtifact);
    }
    let source = decoded.into_inner().into_inner().into_inner();
    Ok(ArtifactEvidence {
        hash: hash_text(&source.digest.finalize()),
        compressed_bytes: source.bytes,
        record_count: identities.len() as u64,
        decompressed_bytes: bytes,
    })
}

fn source_client() -> Result<Client, SourceError> {
    Client::builder()
        .user_agent("Spellwake-Price-Collector/0.1 (+https://spellwake.com)")
        .https_only(true)
        .redirect(Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(StdDuration::from_secs(20))
        .timeout(StdDuration::from_secs(20 * 60))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
        .map_err(SourceError::network)
}

fn checked_response(
    response: reqwest::blocking::Response,
) -> Result<reqwest::blocking::Response, SourceError> {
    if response.status().as_u16() == 429 {
        return Err(SourceError::RateLimited);
    }
    if !response.status().is_success() {
        return Err(SourceError::NetworkCause(SourceFailure(Arc::new(
            SourceStatusFailure(response.status()),
        ))));
    }
    Ok(response)
}

fn fetch_index(client: &Client) -> Result<(BulkArtifact, Vec<u8>), SourceError> {
    let mut response = client
        .get(BULK_INDEX)
        .timeout(StdDuration::from_secs(30))
        .header("Accept", "application/json")
        .header("Accept-Encoding", "identity")
        .send()
        .map_err(SourceError::network)
        .and_then(checked_response)?;
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_INDEX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err(SourceError::BoundsExceeded);
    }
    let selected = parse_index(&bytes)?;
    Ok((selected, bytes))
}

pub(crate) fn parse_index(bytes: &[u8]) -> Result<BulkArtifact, SourceError> {
    #[derive(Deserialize)]
    struct Index {
        object: String,
        has_more: bool,
        data: Vec<Box<serde_json::value::RawValue>>,
    }
    #[derive(Deserialize)]
    struct Discriminator {
        #[serde(rename = "type")]
        kind: Option<Value>,
    }
    let raw = serde_json::from_slice::<crate::provider_json::Object<Index>>(bytes)
        .map_err(SourceError::invalid_index)?
        .0;
    if raw.object != "list" || raw.has_more {
        return Err(SourceError::InvalidIndex);
    }
    let mut selected = None;
    for candidate in raw.data {
        // Unrelated products keep their own schemas. Preserve the selected raw
        // object until typed decoding so known duplicate fields remain visible.
        if !candidate.get().trim_start().starts_with('{') {
            continue;
        }
        let kind =
            serde_json::from_str::<crate::provider_json::Object<Discriminator>>(candidate.get())
                .map_err(SourceError::invalid_index)?
                .0;
        if kind.kind.as_ref().and_then(Value::as_str) != Some("all_cards") {
            continue;
        }
        if selected.is_some() {
            return Err(SourceError::InvalidIndex);
        }
        let value =
            serde_json::from_str::<crate::provider_json::Object<BulkArtifact>>(candidate.get())
                .map_err(SourceError::invalid_index)?
                .0;
        selected = Some(value);
    }
    selected.ok_or(SourceError::InvalidIndex)
}

#[cfg(test)]
#[path = "price_source_object_tests.rs"]
mod object_tests;

/// Acquires and verifies current source bytes in a new UUID-named staging
/// directory. Failed stages are retained for explicit operator cleanup, not used
/// as an alternate historical source. No database or Storage write occurs here.
pub fn qualify_current(directory: &Path) -> Result<QualifiedArtifact, SourceError> {
    let client = source_client()?;
    let (metadata, index_bytes) = fetch_index(&client)?;
    metadata.validate(Utc::now())?;
    // Parse the already strict URL before making any data-origin request.
    let uri = Url::parse(&metadata.jsonl_download_uri).map_err(SourceError::invalid_index)?;
    fs::create_dir_all(directory)?;
    let stage = fs::canonicalize(directory)?.join(format!("source-{}", Uuid::new_v4()));
    fs::create_dir(&stage)?;
    let metadata_path = stage.join("bulk-index.json");
    let mut index_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&metadata_path)?;
    index_file.write_all(&index_bytes)?;
    index_file.sync_all()?;
    let metadata_hash = hash_text(&Sha256::digest(&index_bytes));
    let artifact_path = stage.join("all-cards.jsonl.gz");
    let mut response = client
        .get(uri)
        .header("Accept", "application/gzip")
        .header("Accept-Encoding", "identity")
        .send()
        .map_err(SourceError::network)
        .and_then(checked_response)?;
    if response
        .content_length()
        .is_some_and(|size| size != metadata.compressed_size)
    {
        return Err(SourceError::InvalidArtifact);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&artifact_path)?;
    let copied = io::copy(
        &mut response.by_ref().take(MAX_COMPRESSED_BYTES + 1),
        &mut file,
    )?;
    file.sync_all()?;
    if copied != metadata.compressed_size || copied > MAX_COMPRESSED_BYTES {
        return Err(SourceError::InvalidArtifact);
    }
    // Acquisition includes completing the source bytes, not a backdated start.
    let acquired_at = Utc::now();
    let (hash, compressed_bytes) = hash_file(&artifact_path)?;
    persist_json(
        &stage.join("acquisition.json"),
        &serde_json::json!({"state":"downloaded_unverified","metadata":metadata,
      "acquiredAt":acquired_at,"artifactPath":artifact_path,"metadataPath":metadata_path,"hash":hash,"metadataHash":metadata_hash,
      "compressedBytes":compressed_bytes}),
    )?;
    metadata.validate(acquired_at)?;
    let scan = scan_artifact(&artifact_path, &HashSet::new())?;
    if scan.hash != hash || scan.compressed_bytes != compressed_bytes {
        return Err(SourceError::HashMismatch);
    }
    let qualification_path = stage.join("qualification.json");
    let result = QualifiedArtifact {
        metadata,
        acquired_at,
        artifact_path,
        metadata_path,
        hash,
        metadata_hash,
        qualification_path,
        compressed_bytes,
        decompressed_bytes: scan.decompressed_bytes,
        record_count: scan.record_count,
    };
    persist_json(&result.qualification_path, &result)?;
    Ok(result)
}

fn persist_json(path: &Path, value: &impl Serialize) -> Result<(), SourceError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer(&mut file, value).map_err(SourceError::staging)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

/// Read-only verification is deliberately distinct from acquisition. It does not
/// invent when the bytes were originally acquired or make an old file eligible
/// for a new daily slot.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedInspection {
    pub verified_at: DateTime<Utc>,
    pub metadata: BulkArtifact,
    pub hash: EvidenceHash,
    pub metadata_hash: EvidenceHash,
    pub compressed_bytes: u64,
    pub decompressed_bytes: u64,
    pub record_count: u64,
}
pub fn inspect_retained(artifact: &Path, index: &Path) -> Result<RetainedInspection, SourceError> {
    let mut bytes = Vec::new();
    File::open(index)?
        .take(MAX_INDEX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err(SourceError::BoundsExceeded);
    }
    let metadata = parse_index(&bytes)?;
    // This checks the source product/transport shape only. Inspection grants no
    // source freshness or acquisition assertion, so no current time substitutes.
    metadata.validate(metadata.updated_at)?;
    let scan = scan_artifact(artifact, &HashSet::new())?;
    if scan.compressed_bytes != metadata.compressed_size {
        return Err(SourceError::InvalidArtifact);
    }
    Ok(RetainedInspection {
        verified_at: Utc::now(),
        metadata,
        hash: scan.hash,
        metadata_hash: hash_text(&Sha256::digest(&bytes)),
        compressed_bytes: scan.compressed_bytes,
        decompressed_bytes: scan.decompressed_bytes,
        record_count: scan.record_count,
    })
}

/// Extract only from bytes matching the already established manifest, using the
/// same compressed stream that is fully decoded and identity checked.
pub fn extract_retained(
    path: &Path,
    expected_hash: &EvidenceHash,
    expected_records: u64,
    wanted: &HashSet<Uuid>,
) -> Result<ArtifactScan, SourceError> {
    let scan = scan_artifact(path, wanted)?;
    if &scan.hash != expected_hash || scan.record_count != expected_records {
        return Err(SourceError::HashMismatch);
    }
    Ok(scan)
}

/// Verify the retained original index bytes before parsing its selected product.
/// Freshness is evaluated against the established acquisition by the consumer.
pub fn verify_retained_index(
    path: &Path,
    expected: &EvidenceHash,
) -> Result<BulkArtifact, SourceError> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_INDEX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err(SourceError::BoundsExceeded);
    }
    if &hash_text(&Sha256::digest(&bytes)) != expected {
        return Err(SourceError::HashMismatch);
    }
    parse_index(&bytes)
}

#[cfg(test)]
mod staging_tests;
