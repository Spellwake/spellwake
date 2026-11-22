//! Credential-agnostic HTTP mechanics. Typed facades decide which key may enter.
use lru::LruCache;
use reqwest::{
    Method, Url,
    blocking::{Client, RequestBuilder, Response},
    header::{HeaderMap, HeaderValue},
    redirect::Policy,
};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::Read,
    num::NonZeroUsize,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

#[derive(Clone, PartialEq, Eq, Hash)]
struct PoolKey {
    origin: String,
    key_digest: [u8; 32],
    user_agent: &'static str,
    timeout_ms: u128,
}

fn client_pool() -> &'static Mutex<LruCache<PoolKey, Client>> {
    static POOL: OnceLock<Mutex<LruCache<PoolKey, Client>>> = OnceLock::new();
    POOL.get_or_init(|| {
        // Safe constant capacity > 0; 64 distinct (origin, credential, agent, timeout) pools.
        Mutex::new(LruCache::new(
            NonZeroUsize::new(64).expect("non-zero pool capacity"),
        ))
    })
}

fn pooled_client(
    origin: &Origin,
    key: HeaderValue,
    user_agent: &'static str,
    timeout: Duration,
) -> Result<Client, BackendError> {
    let key_digest: [u8; 32] = Sha256::digest(key.as_bytes()).into();
    let pool_key = PoolKey {
        origin: origin.0.as_str().to_owned(),
        key_digest,
        user_agent,
        timeout_ms: timeout.as_millis(),
    };
    if let Ok(mut guard) = client_pool().lock()
        && let Some(existing) = guard.get(&pool_key)
    {
        return Ok(existing.clone());
    }
    let mut headers = HeaderMap::new();
    headers.insert("apikey", key);
    // Construct on a dedicated OS thread so blocking client init never panics
    // if invoked from a Tokio async context (including async tests).
    let created = std::thread::spawn(move || standard_client(headers, user_agent, timeout))
        .join()
        .map_err(|_| BackendError::Configuration)?
        .map_err(|_| BackendError::Configuration)?;
    if let Ok(mut guard) = client_pool().lock()
        && let Some((_, evicted)) = guard.push(pool_key, created.clone())
    {
        // Drop any evicted blocking client off the caller thread so Tokio never panics.
        let _ = std::thread::Builder::new()
            .name("http-pool-drop".into())
            .spawn(move || drop(evicted));
    }
    Ok(created)
}

#[derive(Clone)]
pub(crate) struct Origin(Url);
impl Origin {
    pub(crate) fn new(base: &str) -> Result<Self, BackendError> {
        let base = Url::parse(base).map_err(|_| BackendError::Configuration)?;
        let loopback = matches!(
            base.host_str(),
            Some("127.0.0.1" | "[::1]" | "::1" | "localhost")
        );
        if !(base.scheme() == "https" || (base.scheme() == "http" && loopback))
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(BackendError::Configuration);
        }
        Ok(Self(base))
    }
    fn check(&self, url: &Url) -> Result<(), BackendError> {
        if url.origin() != self.0.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(BackendError::Configuration);
        }
        Ok(())
    }
}

pub(crate) fn sensitive_key(value: &str, prefix: &str) -> Result<HeaderValue, BackendError> {
    if !value.starts_with(prefix) || !(16..=512).contains(&value.len()) {
        return Err(BackendError::Configuration);
    }
    let mut key = HeaderValue::from_str(value).map_err(|_| BackendError::Configuration)?;
    key.set_sensitive(true);
    Ok(key)
}

#[derive(Clone)]
pub(crate) struct HttpTransport {
    origin: Origin,
    http: Client,
}
impl HttpTransport {
    pub(crate) fn new(
        origin: Origin,
        key: HeaderValue,
        user_agent: &'static str,
        timeout: Duration,
    ) -> Result<Self, BackendError> {
        let http = pooled_client(&origin, key, user_agent, timeout)?;
        Ok(Self { origin, http })
    }
    pub(crate) fn origin_str(&self) -> &str {
        self.origin.0.as_str()
    }
    pub(crate) fn from_client(origin: Origin, http: Client) -> Self {
        Self { origin, http }
    }
    pub(crate) fn url(&self, path: &str) -> Result<Url, BackendError> {
        let url = self
            .origin
            .0
            .join(path)
            .map_err(|_| BackendError::Configuration)?;
        self.origin.check(&url)?;
        Ok(url)
    }
    pub(crate) fn request(&self, method: Method, url: Url) -> Result<RequestBuilder, BackendError> {
        self.origin.check(&url)?;
        Ok(self.http.request(method, url))
    }
    pub(crate) fn rpc(&self, name: &str) -> Result<RequestBuilder, BackendError> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err(BackendError::Configuration);
        }
        Ok(self
            .request(Method::POST, self.url(&format!("rest/v1/rpc/{name}"))?)?
            .header("Content-Profile", "app")
            .header("Accept-Profile", "app")
            .header("Accept", "application/json"))
    }
}

/// Mechanical policy only; typed callers own each vendor's credential headers.
pub(crate) fn standard_client(
    headers: HeaderMap,
    user_agent: &'static str,
    timeout: Duration,
) -> Result<Client, reqwest::Error> {
    Client::builder()
        .default_headers(headers)
        .user_agent(user_agent)
        .redirect(Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .pool_max_idle_per_host(64)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10).min(timeout))
        .timeout(timeout)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
}

pub(crate) fn bounded_body(mut response: Response, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        #[cfg(test)]
        println!("HTTP_RESPONSE_BODY_LIMIT_EXCEEDED {limit}");
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "response exceeds boundary",
        ));
    }
    Ok(bytes)
}

/// Root formatting is safe; underlying source values must never be printed recursively.
#[derive(Clone)]
pub(crate) enum BackendError {
    Configuration,
    Request,
    RequestCause {
        stage: &'static str,
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
}
impl BackendError {
    pub(crate) fn caused(
        stage: &'static str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::RequestCause {
            stage,
            source: Arc::new(source),
        }
    }
}
// Equality preserves failure classification; the diagnostic cause is not domain identity.
impl PartialEq for BackendError {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (Self::Configuration, Self::Configuration))
            || (!matches!(self, Self::Configuration) && !matches!(other, Self::Configuration))
    }
}
impl Eq for BackendError {}
impl fmt::Debug for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration => f.write_str("BackendError::Configuration"),
            Self::Request => f.write_str("BackendError::Request"),
            Self::RequestCause { stage, .. } => f
                .debug_struct("BackendError::Request")
                .field("stage", stage)
                .finish_non_exhaustive(),
        }
    }
}
impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "private backend configuration is invalid",
            Self::Request | Self::RequestCause { .. } => {
                "private backend response could not be established"
            }
        })
    }
}
impl std::error::Error for BackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RequestCause { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

pub(crate) fn read_json_body<T: DeserializeOwned>(response: Response) -> Result<T, BackendError> {
    const MAX_JSON: u64 = 1024 * 1024;
    read_json_body_bounded(response, MAX_JSON)
}

/// A domain owner may select its explicit source-envelope bound. Ordinary RPCs
/// retain their existing limit; this does not provide an unbounded JSON reader.
pub(crate) fn read_json_body_bounded<T: DeserializeOwned>(
    response: Response,
    maximum_bytes: u64,
) -> Result<T, BackendError> {
    let bytes = bounded_body(response, maximum_bytes)
        .map_err(|source| BackendError::caused("body", source))?;
    serde_json::from_slice(&bytes).map_err(|source| BackendError::caused("json", source))
}
