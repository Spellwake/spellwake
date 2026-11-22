//! Private game-scoped byte retention. A ref must come from an independently
//! authorized accepted source; a self-selected hash/key confers no authority.
//! No native semantic codec, database commit, player transport or signed URL.
use crate::{
    backend::{BackendClient, BackendConfig},
    private_storage::{self, StorageError},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{error::Error, fmt, time::Duration};
use uuid::Uuid;

/// Per-object engineering bound, not native format or throughput qualification.
pub const MAX_NATIVE_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;
/// Closed private artifact responsibilities; this tag is not a payload decoder.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NativeArtifactRole {
    /// Initial source package manifest.
    Initial,
    /// Original player/host entropy input record.
    Input,
    /// Accepted private state reconstruction manifest.
    State,
    /// Native fact artifact.
    Facts,
    /// Ordered private event artifact.
    Event,
    /// Permitted seat-view artifact.
    View,
    /// Versioned participant replay manifest.
    Replay,
}
impl NativeArtifactRole {
    /// Canonical private namespace component, never an authorization statement.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Input => "input",
            Self::State => "state",
            Self::Facts => "facts",
            Self::Event => "event",
            Self::View => "view",
            Self::Replay => "replay",
        }
    }
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "initial" => Self::Initial,
            "input" => Self::Input,
            "state" => Self::State,
            "facts" => Self::Facts,
            "event" => Self::Event,
            "view" => Self::View,
            "replay" => Self::Replay,
            _ => return None,
        })
    }
}
/// Safe finite byte-store failure category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeArtifactFailure {
    /// Malformed/unsupported reference or bucket configuration.
    InvalidReference,
    /// Caller-established game, context or role does not match.
    WrongScope,
    /// Empty or over-budget artifact.
    Size,
    /// Required original artifact is absent.
    Missing,
    /// Private metadata, transport or exact-byte verification failed.
    Storage,
}
/// Safe public formatting with private typed Storage/transport cause retention.
pub struct NativeArtifactError {
    failure: NativeArtifactFailure,
    cause: Option<StorageError>,
}
impl NativeArtifactError {
    fn new(failure: NativeArtifactFailure) -> Self {
        Self {
            failure,
            cause: None,
        }
    }
    /// Finite failure class without a private key, URL or artifact body.
    pub fn failure(&self) -> NativeArtifactFailure {
        self.failure
    }
}
impl From<StorageError> for NativeArtifactError {
    fn from(cause: StorageError) -> Self {
        Self {
            failure: NativeArtifactFailure::Storage,
            cause: Some(cause),
        }
    }
}
impl fmt::Display for NativeArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.failure {
            NativeArtifactFailure::InvalidReference => "invalid native artifact reference",
            NativeArtifactFailure::WrongScope => "native artifact scope does not match",
            NativeArtifactFailure::Size => "native artifact exceeds its byte boundary",
            NativeArtifactFailure::Missing => "native artifact is unavailable",
            NativeArtifactFailure::Storage => "private native artifact storage is unresolved",
        })
    }
}
impl fmt::Debug for NativeArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for NativeArtifactError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_ref()
            .map(|value| value as &(dyn Error + 'static))
    }
}
fn invalid() -> NativeArtifactError {
    NativeArtifactError::new(NativeArtifactFailure::InvalidReference)
}
fn uuid(text: &str) -> Result<Uuid, NativeArtifactError> {
    let value = Uuid::parse_str(text).map_err(|_| invalid())?;
    if value.is_nil() || value.to_string() != text {
        return Err(invalid());
    }
    Ok(value)
}
/// Canonical content-bound private key. No Debug or public/signed URL conversion.
/// Syntax/hash consistency is not source authority or proof of durable acceptance.
#[derive(Clone, PartialEq, Eq)]
pub struct NativeArtifactRef {
    key: String,
    game: Uuid,
    context: Uuid,
    role: NativeArtifactRole,
    size: u64,
    hash: [u8; 32],
}
impl TryFrom<String> for NativeArtifactRef {
    type Error = NativeArtifactError;
    fn try_from(key: String) -> Result<Self, Self::Error> {
        if key.len() > 512 {
            return Err(invalid());
        }
        let parts: Vec<_> = key.split('/').collect();
        let ["native-artifact-v1", game, context, role, size, digest] = parts.as_slice() else {
            return Err(invalid());
        };
        let game = uuid(game)?;
        let context = uuid(context)?;
        let role = NativeArtifactRole::parse(role).ok_or_else(invalid)?;
        let parsed_size = size.parse::<u64>().map_err(|_| invalid())?;
        if parsed_size == 0
            || parsed_size > MAX_NATIVE_ARTIFACT_BYTES
            || parsed_size.to_string() != *size
        {
            return Err(invalid());
        }
        let hex = digest.strip_suffix(".json").ok_or_else(invalid)?;
        let hash = crate::source_protocol::digest_bytes(hex).ok_or_else(invalid)?;
        Ok(Self {
            key,
            game,
            context,
            role,
            size: parsed_size,
            hash,
        })
    }
}
impl Serialize for NativeArtifactRef {
    fn serialize<S: Serializer>(&self, output: S) -> Result<S::Ok, S::Error> {
        output.serialize_str(&self.key)
    }
}
impl<'de> Deserialize<'de> for NativeArtifactRef {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(input)?).map_err(serde::de::Error::custom)
    }
}
impl NativeArtifactRef {
    /// Compute a private content reference only; no IO/retention/acceptance proof.
    /// # Errors
    /// Rejects nil scope identities, empty bytes or over-bound payloads.
    pub fn for_bytes(
        game: Uuid,
        context: Uuid,
        role: NativeArtifactRole,
        bytes: &[u8],
    ) -> Result<Self, NativeArtifactError> {
        if game.is_nil() || context.is_nil() {
            return Err(invalid());
        }
        let size = u64::try_from(bytes.len())
            .map_err(|_| NativeArtifactError::new(NativeArtifactFailure::Size))?;
        if size == 0 || size > MAX_NATIVE_ARTIFACT_BYTES {
            return Err(NativeArtifactError::new(NativeArtifactFailure::Size));
        }
        let digest = Sha256::digest(bytes);
        let hash: [u8; 32] = digest.into();
        let hex = crate::price_source::hash_text(&hash);
        let key = format!(
            "native-artifact-v1/{game}/{context}/{}/{size}/{}.json",
            role.as_str(),
            hex.hex()
        );
        Ok(Self {
            key,
            game,
            context,
            role,
            size,
            hash,
        })
    }
    /// Original canonical private key for an authorized database/artifact owner.
    pub fn as_str(&self) -> &str {
        &self.key
    }
    /// Exact game namespace.
    pub fn game_id(&self) -> Uuid {
        self.game
    }
    /// Exact immutable context namespace.
    pub fn context_id(&self) -> Uuid {
        self.context
    }
    /// Expected private responsibility, independent of payload semantics.
    pub fn role(&self) -> NativeArtifactRole {
        self.role
    }
    /// Exact original byte count.
    pub fn size(&self) -> u64 {
        self.size
    }
    /// Exact SHA256 bytes committed by the key, not independent source authority.
    pub fn sha256(&self) -> &[u8; 32] {
        &self.hash
    }
}
/// Verified exact private bytes. No native semantic/source authority is implied.
pub struct NativeArtifact {
    reference: NativeArtifactRef,
    bytes: Vec<u8>,
}
impl NativeArtifact {
    /// Verified canonical reference, still requiring independent source authority.
    pub fn reference(&self) -> &NativeArtifactRef {
        &self.reference
    }
    /// Exact original bytes, never public diagnostics or a raw player response.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Transfer verified bytes to the required native decoder.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}
/// Server-only private Store. Distributed browser/Tauri clients must not own it.
/// The caller supplies independently authorized expected refs and scoped identity.
const DEFAULT_ARTIFACT_CACHE_BYTES: usize = 64 * 1024 * 1024;

struct ArtifactByteCache {
    entries: lru::LruCache<String, std::sync::Arc<[u8]>>,
    total_bytes: usize,
    max_bytes: usize,
}

impl ArtifactByteCache {
    fn new(max_bytes: usize) -> Self {
        Self {
            entries: lru::LruCache::unbounded(),
            total_bytes: 0,
            max_bytes,
        }
    }

    fn get(&mut self, key: &str) -> Option<Vec<u8>> {
        self.entries.get(key).map(|bytes| bytes.as_ref().to_vec())
    }

    fn insert(&mut self, key: String, bytes: &[u8]) {
        if self.max_bytes == 0 || bytes.len() > self.max_bytes / 4 {
            return;
        }
        if let Some(previous) = self.entries.put(key, std::sync::Arc::from(bytes)) {
            self.total_bytes = self.total_bytes.saturating_sub(previous.len());
        }
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        while self.total_bytes > self.max_bytes {
            let Some((_, evicted)) = self.entries.pop_lru() else {
                self.total_bytes = 0;
                break;
            };
            self.total_bytes = self.total_bytes.saturating_sub(evicted.len());
        }
    }
}

pub struct NativeArtifactStore {
    backend: BackendClient,
    bucket: String,
    cache: std::sync::Arc<std::sync::Mutex<ArtifactByteCache>>,
}
impl NativeArtifactStore {
    pub(crate) fn new(backend: BackendClient, bucket: String) -> Result<Self, NativeArtifactError> {
        if !private_storage::bucket_name(&bucket) {
            return Err(invalid());
        }
        let max_bytes = std::env::var("SPELLWAKE_NATIVE_ARTIFACT_CACHE_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_ARTIFACT_CACHE_BYTES);
        Ok(Self {
            backend,
            bucket,
            cache: std::sync::Arc::new(std::sync::Mutex::new(ArtifactByteCache::new(max_bytes))),
        })
    }
    /// Read existing server configuration and the private native bucket setting.
    /// This does not create a bucket, activate a service or grant source authority.
    /// # Errors
    /// Rejects missing/invalid server config, bucket syntax or transport setup.
    pub fn from_env() -> Result<Self, NativeArtifactError> {
        let config = BackendConfig::from_env().map_err(StorageError::from)?;
        let backend = BackendClient::new(
            config,
            "spellwake-native-artifacts/1",
            Duration::from_secs(60),
        )
        .map_err(StorageError::from)?;
        let bucket = std::env::var("SPELLWAKE_NATIVE_ARTIFACT_BUCKET").map_err(|_| invalid())?;
        Self::new(backend, bucket)
    }
    /// Retain original private bytes and verify exact readback before returning.
    /// The reference/candidate must still be accepted by the database host owner.
    /// # Errors
    /// Rejects invalid scope/size and unproven bucket privacy or exact retention.
    pub fn retain(
        &self,
        game: Uuid,
        context: Uuid,
        role: NativeArtifactRole,
        bytes: &[u8],
    ) -> Result<NativeArtifactRef, NativeArtifactError> {
        let reference = NativeArtifactRef::for_bytes(game, context, role, bytes)?;
        private_storage::retain_bytes(&self.backend, &self.bucket, reference.as_str(), bytes)?;
        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(reference.as_str().to_owned(), bytes);
        }
        Ok(reference)
    }
    /// Keep proof of the authenticated, exact private readback for the newly
    /// generated View. SQL must still select this exact immutable reference
    /// before its bytes can be used to restore an issued View.
    pub(super) fn retain_new_view(
        &self,
        reference: NativeArtifactRef,
        bytes: Vec<u8>,
    ) -> Result<NativeArtifact, NativeArtifactError> {
        let expected = NativeArtifactRef::for_bytes(
            reference.game,
            reference.context,
            NativeArtifactRole::View,
            &bytes,
        )?;
        if expected != reference {
            return Err(invalid());
        }
        private_storage::retain_new_bytes(&self.backend, &self.bucket, reference.as_str(), &bytes)?;
        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(reference.as_str().to_owned(), &bytes);
        }
        Ok(NativeArtifact { reference, bytes })
    }
    /// Read a reference obtained from an independently authorized accepted source.
    /// Never use a player-provided path/hash as that authority.
    /// # Errors
    /// Rejects foreign game/context/role before IO, nonprivate bucket, missing,
    /// encoded, length-mismatched or hash-mismatched original bytes and IO failure.
    pub fn read(
        &self,
        reference: &NativeArtifactRef,
        game: Uuid,
        context: Uuid,
        role: NativeArtifactRole,
    ) -> Result<NativeArtifact, NativeArtifactError> {
        if reference.game != game || reference.context != context || reference.role != role {
            return Err(NativeArtifactError::new(NativeArtifactFailure::WrongScope));
        }
        // Content-addressed immutable artifacts verified by full SHA-256 digest can be
        // served directly from the bounded in-memory LRU cache without repeating HTTP I/O.
        if let Ok(mut guard) = self.cache.lock()
            && let Some(bytes) = guard.get(reference.as_str())
        {
            return Ok(NativeArtifact {
                reference: reference.clone(),
                bytes,
            });
        }
        private_storage::private_bucket(&self.backend, &self.bucket)?;
        let bytes = private_storage::read_bytes(
            &self.backend,
            &self.bucket,
            reference.as_str(),
            reference.size,
            &reference.hash,
        )?
        .ok_or_else(|| NativeArtifactError::new(NativeArtifactFailure::Missing))?;
        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(reference.as_str().to_owned(), &bytes);
        }
        Ok(NativeArtifact {
            reference: reference.clone(),
            bytes,
        })
    }
}
#[cfg(test)]
mod tests;
