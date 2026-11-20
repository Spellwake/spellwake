//! Shared private Temporal connection configuration; queues/workers stay domain-owned.
use reqwest::Url;
use std::{fmt, time::Duration};
use temporalio_client::{Client, ClientOptions, Connection, ConnectionOptions, TlsOptions};
mod record;
mod shutdown;
pub use record::RecordPayload;
pub(crate) use shutdown::shutdown_signal;
/// Safe bootstrap classification with an optional original, process-local cause.
pub struct TemporalRuntimeError {
    cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}
impl TemporalRuntimeError {
    /// Retain an original bootstrap failure without exposing it in formatting.
    /// This value must not be serialized into workflow history or API responses.
    pub fn caused(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self {
            cause: Some(error.into()),
        }
    }
    fn rejected() -> Self {
        Self { cause: None }
    }
}
impl fmt::Debug for TemporalRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TemporalRuntimeError")
            .finish_non_exhaustive()
    }
}
impl fmt::Display for TemporalRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("workflow connection could not be established")
    }
}
impl std::error::Error for TemporalRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_ref().map(|cause| cause.as_ref() as _)
    }
}
/// No Debug or Serialize implementation: the optional Temporal API key stays private.
#[derive(Clone)]
pub struct TemporalConfig {
    address: Url,
    namespace: String,
    api_key: Option<String>,
}
impl TemporalConfig {
    /// Read and validate private Temporal connection configuration.
    /// # Errors
    /// Returns a safe error for missing/non-Unicode variables or invalid policy.
    pub fn from_env() -> Result<Self, TemporalRuntimeError> {
        let address =
            std::env::var("SPELLWAKE_TEMPORAL_ADDRESS").map_err(TemporalRuntimeError::caused)?;
        let namespace =
            std::env::var("SPELLWAKE_TEMPORAL_NAMESPACE").map_err(TemporalRuntimeError::caused)?;
        let api_key = match std::env::var("SPELLWAKE_TEMPORAL_API_KEY") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => return Err(TemporalRuntimeError::caused(error)),
        };
        Self::new(&address, &namespace, api_key)
    }
    /// Validate an address, namespace and optional private API key.
    /// # Errors
    /// Rejects invalid URLs, nonlocal plaintext, URL credentials/path/query/fragment,
    /// invalid namespaces, or keys unsuitable for the selected connection.
    pub fn new(
        address: &str,
        namespace: &str,
        api_key: Option<String>,
    ) -> Result<Self, TemporalRuntimeError> {
        let address = Url::parse(address).map_err(TemporalRuntimeError::caused)?;
        let local = address.scheme() == "http"
            && matches!(address.host_str(), Some("127.0.0.1" | "[::1]" | "::1"));
        if !(local || address.scheme() == "https")
            || address.host_str().is_none()
            || !address.username().is_empty()
            || address.password().is_some()
            || address.path() != "/"
            || address.query().is_some()
            || address.fragment().is_some()
            || namespace.is_empty()
            || namespace.len() > 100
            || !namespace
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
            || api_key.as_ref().is_some_and(|key| {
                local
                    || key.is_empty()
                    || key.len() > 4096
                    || !key.is_ascii()
                    || key.bytes().any(|c| c.is_ascii_control())
            })
        {
            return Err(TemporalRuntimeError::rejected());
        }
        Ok(Self {
            address,
            namespace: namespace.into(),
            api_key,
        })
    }
    /// Establish the configured private connection and namespaced SDK client.
    /// # Errors
    /// Retains original SDK transport or client initialization failures locally.
    pub async fn connect(self, identity: &'static str) -> Result<Client, TemporalRuntimeError> {
        let tls = (self.address.scheme() == "https").then(TlsOptions::default);
        let options = ConnectionOptions::new(self.address)
            .identity(identity)
            .connect_timeout(Duration::from_secs(15))
            .maybe_tls_options(tls)
            .maybe_api_key(self.api_key)
            .build();
        let connection = Connection::connect(options)
            .await
            .map_err(TemporalRuntimeError::caused)?;
        Client::new(connection, ClientOptions::new(self.namespace).build())
            .map_err(TemporalRuntimeError::caused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_cannot_enter_plaintext_or_unqualified_connections() {
        assert!(TemporalConfig::new("http://127.0.0.1:57233", "spellwake-test", None).is_ok());
        assert!(
            TemporalConfig::new(
                "https://example.com:7233",
                "spellwake.production",
                Some("fake-key".into())
            )
            .is_ok()
        );
        for address in [
            "http://example.com",
            "http://localhost:7233",
            "https://user:secret@example.com",
            "https://example.com/path",
            "https://example.com?token=secret",
        ] {
            assert!(TemporalConfig::new(address, "test", None).is_err());
        }
        assert!(
            TemporalConfig::new("http://127.0.0.1:57233", "test", Some("fake-key".into())).is_err()
        );
        assert!(TemporalConfig::new("https://example.com", "invalid namespace", None).is_err());
    }
}

#[cfg(test)]
#[path = "temporal_runtime/object_tests.rs"]
mod object_tests;

#[cfg(test)]
#[path = "temporal_runtime/diagnostics.rs"]
mod diagnostic_tests;
