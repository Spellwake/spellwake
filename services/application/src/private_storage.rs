//! Shared private Storage mechanics; namespace and retention remain domain-owned.
use crate::provider_json::Object;
use crate::{
    backend::{BackendClient, BackendError},
    http_transport::bounded_body,
    price_source::{EvidenceHash, SourceError, hash_file},
};
use reqwest::{
    Method, StatusCode, Url,
    blocking::{Body, Response},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use uuid::Uuid;

const BUCKET_PRIVACY_TTL: Duration = Duration::from_secs(300);

fn verified_buckets() -> &'static Mutex<HashMap<(String, String), Instant>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), Instant>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Private accepted source ticket. Each domain validates identity, namespace and state.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EvidenceCapture {
    pub id: Uuid,
    pub evidence_state: String,
    pub payload_ref: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub processed: bool,
}

/// Shared expiry projection; callers still enforce their own provider namespace and identity.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EvidenceExpiry {
    pub payload_ref: String,
    pub body_hash: String,
}

// Storage may add fields; consume these authority-bearing keys without Value normalization.
#[derive(Deserialize)]
struct BucketMetadata {
    id: String,
    public: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AbsentObject {
    code: String,
    status_code: String,
}

#[derive(Debug)]
pub(crate) enum StorageError {
    Backend(BackendError),
    Request(reqwest::Error),
    Read(std::io::Error),
    Json(serde_json::Error),
    Source(SourceError),
    Mismatch,
    Contract,
    Status(StatusCode),
}
impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(status) => write!(
                f,
                "private evidence storage is unresolved (HTTP {})",
                status.as_u16()
            ),
            _ => f.write_str("private evidence storage is unresolved"),
        }
    }
}
impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Backend(e) => Some(e),
            Self::Request(e) => Some(e),
            Self::Read(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Source(e) => Some(e),
            Self::Contract | Self::Mismatch | Self::Status(_) => None,
        }
    }
}
impl From<BackendError> for StorageError {
    fn from(e: BackendError) -> Self {
        Self::Backend(e)
    }
}
impl From<reqwest::Error> for StorageError {
    fn from(e: reqwest::Error) -> Self {
        Self::Request(e.without_url())
    }
}

pub(crate) fn bucket_name(bucket: &str) -> bool {
    !bucket.is_empty()
        && bucket.len() <= 100
        && bucket
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
pub(crate) fn private_bucket(backend: &BackendClient, bucket: &str) -> Result<(), StorageError> {
    if !bucket_name(bucket) {
        return Err(StorageError::Contract);
    }
    let cache_key = (backend.origin_str().to_owned(), bucket.to_owned());
    let now = Instant::now();
    if let Ok(guard) = verified_buckets().lock()
        && let Some(&verified_at) = guard.get(&cache_key)
        && now.saturating_duration_since(verified_at) < BUCKET_PRIVACY_TTL
    {
        return Ok(());
    }
    let response = backend
        .request(
            Method::GET,
            backend.url(&format!("storage/v1/bucket/{bucket}"))?,
        )?
        .timeout(std::time::Duration::from_secs(30))
        .send()?;
    if !response.status().is_success() {
        return Err(StorageError::Contract);
    }
    let body = bounded_body(response, 1024 * 1024).map_err(StorageError::Read)?;
    let Object(meta) =
        serde_json::from_slice::<Object<BucketMetadata>>(&body).map_err(StorageError::Json)?;
    if meta.id != bucket || meta.public {
        return Err(StorageError::Contract);
    }
    if let Ok(mut guard) = verified_buckets().lock() {
        if guard.len() >= 128 {
            guard.retain(|_, ts| now.saturating_duration_since(*ts) < BUCKET_PRIVACY_TTL);
        }
        guard.insert(cache_key, now);
    }
    Ok(())
}
pub(crate) fn object_url(
    backend: &BackendClient,
    bucket: &str,
    key: &str,
    authenticated: bool,
) -> Result<Url, StorageError> {
    if !bucket_name(bucket)
        || key.is_empty()
        || key.len() > 512
        || key
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
    {
        return Err(StorageError::Contract);
    }
    let prefix = if authenticated {
        "storage/v1/object/authenticated"
    } else {
        "storage/v1/object"
    };
    Ok(backend.url(&format!("{prefix}/{bucket}/{key}"))?)
}
fn existing(
    backend: &BackendClient,
    bucket: &str,
    key: &str,
    expected: &[u8],
) -> Result<bool, StorageError> {
    let hash: [u8; 32] = Sha256::digest(expected).into();
    read_bytes(
        backend,
        bucket,
        key,
        u64::try_from(expected.len()).map_err(|_| StorageError::Contract)?,
        &hash,
    )
    .map(|value| value.is_some())
}

/// Read exact authenticated original bytes. Domain callers establish source
/// authority, privacy and their own size limit before invoking this helper.
pub(crate) fn read_bytes(
    backend: &BackendClient,
    bucket: &str,
    key: &str,
    size: u64,
    hash: &[u8; 32],
) -> Result<Option<Vec<u8>>, StorageError> {
    let response = backend
        .request(Method::GET, object_url(backend, bucket, key, true)?)?
        .header("Accept-Encoding", "identity")
        .send()?;
    let Some(response) = present_object(response)? else {
        return Ok(None);
    };
    if response
        .headers()
        .get("content-encoding")
        .is_some_and(|h| h != "identity")
    {
        return Err(StorageError::Contract);
    }
    if response
        .content_length()
        .is_some_and(|length| length != size)
    {
        return Err(StorageError::Contract);
    }
    let bytes = bounded_body(response, size).map_err(StorageError::Read)?;
    let actual: [u8; 32] = Sha256::digest(&bytes).into();
    if u64::try_from(bytes.len()).ok() != Some(size) || &actual != hash {
        return Err(StorageError::Contract);
    }
    Ok(Some(bytes))
}

// Readback remains authoritative. Retain a sanitized failed upload cause only
// when later readback cannot establish retention; never log the key or body.
fn upload_failure(result: Result<Response, reqwest::Error>) -> Option<StorageError> {
    match result {
        Ok(response) if response.status().is_success() => None,
        Ok(response) => {
            let status = response.status();
            Some(
                response
                    .error_for_status()
                    .err()
                    .map(StorageError::from)
                    .unwrap_or(StorageError::Status(status)),
            )
        }
        Err(cause) => Some(StorageError::from(cause)),
    }
}

fn present_object(response: Response) -> Result<Option<Response>, StorageError> {
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if response.status() == StatusCode::BAD_REQUEST {
        let bytes = bounded_body(response, 1024 * 1024).map_err(StorageError::Read)?;
        let Object(error) =
            serde_json::from_slice::<Object<AbsentObject>>(&bytes).map_err(StorageError::Json)?;
        return if error.code == "NoSuchKey" && error.status_code == "404" {
            Ok(None)
        } else {
            Err(StorageError::Contract)
        };
    }
    if !response.status().is_success() {
        return Err(StorageError::Contract);
    }
    Ok(Some(response))
}

pub(crate) fn verify_file(
    backend: &BackendClient,
    bucket: &str,
    key: &str,
    hash: &EvidenceHash,
    size: u64,
    stage: &Path,
) -> Result<bool, StorageError> {
    let response = backend
        .request(Method::GET, object_url(backend, bucket, key, true)?)?
        .header("Accept-Encoding", "identity")
        .send()?;
    let Some(mut response) = present_object(response)? else {
        return Ok(false);
    };
    if response
        .headers()
        .get("content-encoding")
        .is_some_and(|value| value != "identity")
    {
        return Err(StorageError::Contract);
    }
    if response
        .content_length()
        .is_some_and(|length| length != size)
    {
        return Err(StorageError::Mismatch);
    }
    let path = stage.join(format!("readback-{}", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(StorageError::Read)?;
    let checked = (|| {
        let copied = io::copy(&mut response.by_ref().take(size + 1), &mut file)
            .map_err(StorageError::Read)?;
        file.sync_all().map_err(StorageError::Read)?;
        let (actual, bytes) = hash_file(&path).map_err(StorageError::Source)?;
        if copied != size || bytes != size || &actual != hash {
            return Err(StorageError::Mismatch);
        }
        Ok(())
    })();
    drop(file);
    let removed = fs::remove_file(&path).map_err(StorageError::Read);
    checked?;
    removed?;
    Ok(true)
}

/// Caller checks its private bucket once before retaining a group. Domain callers
/// own keys and retention policy; sized streaming and verified readback are shared.
pub(crate) struct RetainedFile<'a> {
    pub key: &'a str,
    pub path: &'a Path,
    pub hash: &'a EvidenceHash,
    pub size: u64,
    pub content_type: &'a str,
}
pub(crate) fn retain_file(
    backend: &BackendClient,
    bucket: &str,
    stage: &Path,
    evidence: &RetainedFile<'_>,
) -> Result<(), StorageError> {
    let RetainedFile {
        key,
        path,
        hash,
        size,
        content_type,
    } = *evidence;
    if size == 0 || size > 2147483648 {
        return Err(StorageError::Contract);
    }
    if verify_file(backend, bucket, key, hash, size, stage)? {
        return Ok(());
    }
    let file = File::open(path).map_err(StorageError::Read)?;
    let upload = backend
        .request(Method::POST, object_url(backend, bucket, key, false)?)?
        .header("Content-Type", content_type)
        .header("x-upsert", "false")
        .body(Body::sized(file, size))
        .send();
    let failure = upload_failure(upload);
    if failure.is_some() {
        tracing::warn!(
            event = "upload_ack_unconfirmed",
            "private storage upload ack unconfirmed"
        );
    }
    if !verify_file(backend, bucket, key, hash, size, stage)? {
        return Err(failure.unwrap_or(StorageError::Contract));
    }
    Ok(())
}
pub(crate) fn retain_bytes(
    backend: &BackendClient,
    bucket: &str,
    key: &str,
    bytes: &[u8],
) -> Result<(), StorageError> {
    private_bucket(backend, bucket)?;
    if existing(backend, bucket, key, bytes)? {
        return Ok(());
    }
    upload_and_verify_bytes(backend, bucket, key, bytes)
}

/// A create-only write followed by the same authenticated exact-byte read.
/// Use for newly generated content, where an existing object is unlikely.
pub(crate) fn retain_new_bytes(
    backend: &BackendClient,
    bucket: &str,
    key: &str,
    bytes: &[u8],
) -> Result<(), StorageError> {
    private_bucket(backend, bucket)?;
    upload_and_verify_bytes(backend, bucket, key, bytes)
}

fn upload_and_verify_bytes(
    backend: &BackendClient,
    bucket: &str,
    key: &str,
    bytes: &[u8],
) -> Result<(), StorageError> {
    let sent = backend
        .request(Method::POST, object_url(backend, bucket, key, false)?)?
        .header("Content-Type", "application/json")
        .header("x-upsert", "false")
        .body(bytes.to_vec())
        .send();
    let failure = upload_failure(sent);
    if failure.is_some() {
        tracing::warn!(
            event = "upload_ack_unconfirmed",
            "private storage upload ack unconfirmed"
        );
    }
    if !existing(backend, bucket, key, bytes)? {
        return Err(failure.unwrap_or(StorageError::Contract));
    }
    Ok(())
}

pub(crate) fn delete_payment_evidence(
    backend: &BackendClient,
    bucket: &str,
    key: &str,
) -> Result<(), StorageError> {
    if !key.starts_with("payments/stripe/") || !key.ends_with(".json") {
        return Err(StorageError::Contract);
    }
    delete_private_object(backend, bucket, key)
}

/// Delete an exact validated private object and positively verify absence.
/// Domain callers retain responsibility for source identity and namespace.
pub(crate) fn delete_private_object(
    backend: &BackendClient,
    bucket: &str,
    key: &str,
) -> Result<(), StorageError> {
    let object = object_url(backend, bucket, key, true)?;
    private_bucket(backend, bucket)?;
    let attempt = backend
        .request(
            Method::DELETE,
            backend.url(&format!("storage/v1/object/{bucket}"))?,
        )?
        .json(&serde_json::json!({"prefixes":[key]}))
        .send();
    if !attempt.as_ref().is_ok_and(|r| r.status().is_success()) {
        tracing::warn!(
            event = "delete_ack_unconfirmed",
            "private storage delete ack unconfirmed"
        );
    }
    let response = backend.request(Method::GET, object)?.send()?;
    if present_object(response)?.is_none() {
        return Ok(());
    }
    Err(StorageError::Contract)
}

#[cfg(test)]
#[path = "private_storage_tests.rs"]
mod tests;
