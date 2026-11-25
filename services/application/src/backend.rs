//! Private Supabase transport shared by trusted application workers.
//! Configuration never implements Debug/Serialize or enters workflow payloads.
pub(crate) use crate::http_transport::{BackendError, read_json_body};
use crate::http_transport::{
    HttpTransport, Origin, bounded_body, read_json_body_bounded, sensitive_key,
};
use reqwest::{
    Method, StatusCode, Url,
    blocking::{RequestBuilder, Response},
    header::HeaderValue,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct BackendConfig {
    origin: Origin,
    key: HeaderValue,
}
impl BackendConfig {
    pub(crate) fn from_env() -> Result<Self, BackendError> {
        Self::new(
            &std::env::var("SPELLWAKE_SUPABASE_URL").map_err(|_| BackendError::Configuration)?,
            &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY")
                .map_err(|_| BackendError::Configuration)?,
        )
    }
    pub(crate) fn new(base: &str, key: &str) -> Result<Self, BackendError> {
        Ok(Self {
            origin: Origin::new(base)?,
            key: sensitive_key(key, "sb_secret_")?,
        })
    }
}

#[derive(Clone)]
pub(crate) struct BackendClient {
    http: HttpTransport,
}
impl BackendClient {
    pub(crate) fn new(
        config: BackendConfig,
        user_agent: &'static str,
        timeout: Duration,
    ) -> Result<Self, BackendError> {
        Ok(Self {
            http: HttpTransport::new(config.origin, config.key, user_agent, timeout)?,
        })
    }
    pub(crate) fn origin_str(&self) -> &str {
        self.http.origin_str()
    }
    pub(crate) fn url(&self, path: &str) -> Result<Url, BackendError> {
        self.http.url(path)
    }
    pub(crate) fn request(&self, method: Method, url: Url) -> Result<RequestBuilder, BackendError> {
        self.http.request(method, url)
    }
    pub(crate) fn rpc<T: DeserializeOwned>(
        &self,
        name: &str,
        payload: &Value,
    ) -> Result<T, BackendError> {
        read_json_body(self.rpc_response(name, payload)?)
    }

    /// Read a domain's explicitly bounded source response without widening the
    /// default1MiB RPC boundary. The caller owns the concrete source limit.
    pub(crate) fn rpc_bounded<T: DeserializeOwned>(
        &self,
        name: &str,
        payload: &Value,
        maximum_bytes: u64,
    ) -> Result<T, BackendError> {
        read_json_body_bounded(self.rpc_response(name, payload)?, maximum_bytes)
    }

    /// Preserve exact bounded bytes for aggregate domain work accounting,
    /// including document whitespace omitted by RawValue decoding.
    pub(crate) fn rpc_bytes_bounded(
        &self,
        name: &str,
        payload: &Value,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, BackendError> {
        bounded_body(self.rpc_response(name, payload)?, maximum_bytes)
            .map_err(|source| BackendError::caused("body", source))
    }

    /// Current PostgREST void functions acknowledge with204 and no JSON body.
    /// Keep this separate from record/nullable JSON decoding: empty responses
    /// cannot silently become an absent record or an accepted command receipt.
    pub(crate) fn rpc_void(&self, name: &str, payload: &Value) -> Result<(), BackendError> {
        let response = self.rpc_response(name, payload)?;
        if response.status() != StatusCode::NO_CONTENT {
            return Err(BackendError::Request);
        }
        bounded_body(response, 0)
            .map(|_| ())
            .map_err(|source| BackendError::caused("body", source))
    }

    fn rpc_response(&self, name: &str, payload: &Value) -> Result<Response, BackendError> {
        let response = self
            .http
            .rpc(name)?
            .timeout(Duration::from_secs(60))
            .json(payload)
            .send()
            .map_err(|source| BackendError::caused("send", source.without_url()))?;
        if !response.status().is_success() {
            return Err(BackendError::Request);
        }
        Ok(response)
    }

    /// Exact bounded bytes of one narrow PostgREST read of an `app` table that the
    /// service role may already SELECT. The table path passes the origin check;
    /// filters are added as typed query pairs, never spliced into the URL. This
    /// is not a general table-query framework: callers fix table and filters.
    pub(crate) fn select_bytes_bounded(
        &self,
        table: &str,
        filters: &[(&str, String)],
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, BackendError> {
        if table.is_empty() || !table.bytes().all(|c| c.is_ascii_lowercase() || c == b'_') {
            return Err(BackendError::Configuration);
        }
        let response = self
            .http
            .request(Method::GET, self.http.url(&format!("rest/v1/{table}"))?)?
            .query(filters)
            .header("Accept-Profile", "app")
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(60))
            .send()
            .map_err(|source| BackendError::caused("send", source.without_url()))?;
        if !response.status().is_success() {
            return Err(BackendError::Request);
        }
        bounded_body(response, maximum_bytes).map_err(|source| BackendError::caused("body", source))
    }

    /// Decode a record directly from the bounded body, retaining duplicate keys for typed rejection.
    pub(crate) fn rpc_object<T: DeserializeOwned>(
        &self,
        name: &str,
        payload: &Value,
    ) -> Result<T, BackendError> {
        self.rpc::<crate::provider_json::Object<T>>(name, payload)
            .map(|value| value.0)
    }

    /// A missing record is null; a present record must be an object.
    pub(crate) fn rpc_optional_object<T: DeserializeOwned>(
        &self,
        name: &str,
        payload: &Value,
    ) -> Result<Option<T>, BackendError> {
        self.rpc::<Option<crate::provider_json::Object<T>>>(name, payload)
            .map(|value| value.map(|object| object.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Mock, Step};
    use serde_json::json;

    #[test]
    fn void_acknowledgments_do_not_weaken_json_or_nullable_record_decoding() {
        let path = "/rest/v1/rpc/finish_shape_fixture";
        let mut denied = Step::no_content(path);
        denied.status = 403;
        let mock = Mock::new(vec![
            Step::no_content(path),
            Step::json(path, Value::Null),
            Step::json(path, json!({})),
            denied,
            Step::no_content(path),
        ]);
        let client = BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "void-test",
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(client.rpc_void("finish_shape_fixture", &json!({})).is_ok());
        for _ in 0..3 {
            assert!(client.rpc_void("finish_shape_fixture", &json!({})).is_err());
        }
        assert!(
            client
                .rpc::<Option<Value>>("finish_shape_fixture", &json!({}))
                .is_err()
        );
        assert_eq!(mock.finish().len(), 5);
    }

    #[test]
    #[ignore = "requires owned managed Supabase void source"]
    fn managed_void_rpc_uses_actual_postgrest_no_content() {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Fixture {
            scope: String,
            base: String,
            secret: String,
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            event_id: uuid::Uuid,
        }
        let path = std::env::var("SPELLWAKE_MANAGED_VOID_RPC_INPUT")
            .expect("owned managed fixture input required");
        let raw = std::fs::read(path).expect("owned fixture input readable");
        assert!(raw.len() <= 4096);
        let fixture: Fixture = crate::decode_wire_object(&raw).expect("valid owned fixture input");
        assert!(fixture.scope == "spellwake-owned-managed-void-v1");
        let origin = Url::parse(&fixture.base).expect("owned origin");
        assert!(origin.scheme() == "http" && origin.host_str() == Some("127.0.0.1"));
        let client = BackendClient::new(
            BackendConfig::new(&fixture.base, &fixture.secret).expect("owned backend"),
            "managed-void-contract",
            Duration::from_secs(10),
        )
        .expect("owned transport");
        let input = json!({"p_event":fixture.event_id});
        client
            .rpc_void("finish_stripe_evidence_expiry", &input)
            .expect("actual204ack");
        client
            .rpc_void("finish_stripe_evidence_expiry", &input)
            .expect("idempotent204ack");
        assert!(
            client
                .rpc::<Option<Value>>("finish_stripe_evidence_expiry", &input)
                .is_err()
        );
    }

    #[test]
    fn private_objects_preserve_list_scalar_and_nullable_rpc_shapes() {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Record {
            id: uuid::Uuid,
            enabled: bool,
        }
        let id = uuid::Uuid::new_v4();
        let path = "/rest/v1/rpc/read_shape_fixture";
        let record = json!({"id":id,"enabled":true});
        let mock = Mock::new(
            vec![
                json!(true),
                Value::Null,
                json!([id]),
                record.clone(),
                Value::Null,
                record,
                json!([id, true]),
                json!([id, true]),
            ]
            .into_iter()
            .map(|body| Step::json(path, body))
            .collect(),
        );
        let client = BackendClient::new(
            BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
            "test",
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(
            client
                .rpc::<bool>("read_shape_fixture", &json!({}))
                .unwrap()
        );
        client.rpc::<()>("read_shape_fixture", &json!({})).unwrap();
        assert_eq!(
            client
                .rpc::<Vec<uuid::Uuid>>("read_shape_fixture", &json!({}))
                .unwrap(),
            vec![id]
        );
        let record: Record = client.rpc_object("read_shape_fixture", &json!({})).unwrap();
        assert_eq!(record.id, id);
        assert!(record.enabled);
        assert!(
            client
                .rpc_optional_object::<Record>("read_shape_fixture", &json!({}))
                .unwrap()
                .is_none()
        );
        assert!(
            client
                .rpc_optional_object::<Record>("read_shape_fixture", &json!({}))
                .unwrap()
                .is_some()
        );
        assert!(
            client
                .rpc_object::<Record>("read_shape_fixture", &json!({}))
                .is_err()
        );
        assert!(
            client
                .rpc_optional_object::<Record>("read_shape_fixture", &json!({}))
                .is_err()
        );
        assert_eq!(mock.finish().len(), 8);
    }
    #[test]
    fn private_origin_and_rpc_names_cannot_redirect_credentials() {
        for base in [
            "http://example.com/",
            "https://u:p@example.com/",
            "https://example.com/path",
            "https://example.com/?key=x",
        ] {
            assert!(BackendConfig::new(base, "sb_secret_test_only").is_err());
        }
        let config = BackendConfig::new("http://127.0.0.1:1/", "sb_secret_test_only").unwrap();
        let client = BackendClient::new(config, "test", Duration::from_secs(1)).unwrap();
        assert!(client.url("https://example.com/steal").is_err());
        assert!(client.url("//example.com/steal").is_err());
        for name in ["../users", "https://example.com", "name?x=1", ""] {
            assert_eq!(
                client.rpc::<Value>(name, &json!({})).unwrap_err(),
                BackendError::Configuration
            );
        }
    }
    #[test]
    fn rpc_requires_success_bounded_json_and_refuses_redirects() {
        for (status, body) in [
            (302, b"{}".to_vec()),
            (500, b"secret internal error".to_vec()),
            (200, b"not json".to_vec()),
            (200, vec![b' '; 1024 * 1024 + 1]),
        ] {
            let mut step = Step::json("/rest/v1/rpc/test", json!({}));
            step.status = status;
            step.body = body;
            let mock = Mock::new(vec![step]);
            let client = BackendClient::new(
                BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
                "test",
                Duration::from_secs(1),
            )
            .unwrap();
            assert_eq!(
                client.rpc::<Value>("test", &json!({})).unwrap_err(),
                BackendError::Request
            );
            mock.finish();
        }
    }
}

#[cfg(test)]
mod cause_tests {
    use super::*;
    use crate::test_http::{Mock, Step};
    use serde_json::json;
    use std::error::Error;
    #[test]
    fn rpc_json_and_body_failures_keep_sources_but_root_formats_are_safe() {
        for oversized in [false, true] {
            let mut step = Step::json("/rest/v1/rpc/test", json!("private-response-sentinel"));
            if oversized {
                step.body = vec![b' '; 1024 * 1024 + 1];
            }
            let mock = Mock::new(vec![step]);
            let client = BackendClient::new(
                BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
                "test",
                Duration::from_secs(1),
            )
            .unwrap();
            let error = client.rpc::<u64>("test", &json!({})).unwrap_err();
            assert_eq!(error, BackendError::Request);
            assert_eq!(error.clone(), error);
            let source = error.source().unwrap();
            if oversized {
                assert!(source.downcast_ref::<std::io::Error>().is_some());
            } else {
                assert!(source.downcast_ref::<serde_json::Error>().is_some());
                assert!(source.to_string().contains("private-response-sentinel"));
            }
            for formatted in [
                format!("{error}"),
                format!("{error:?}"),
                format!("{error:#?}"),
            ] {
                assert!(!formatted.contains("private-response-sentinel"));
                assert!(!formatted.contains("sb_secret"));
                assert!(!formatted.contains(&mock.base));
            }
            mock.finish();
        }
    }
    #[test]
    fn rpc_send_keeps_actual_reqwest_cause_with_request_url_removed() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);
        let client = BackendClient::new(
            BackendConfig::new(&base, "sb_secret_test_only").unwrap(),
            "test",
            Duration::from_secs(1),
        )
        .unwrap();
        let error = client
            .rpc::<u64>("test", &json!({"sensitive":"never_log"}))
            .unwrap_err();
        let cause = error
            .source()
            .unwrap()
            .downcast_ref::<reqwest::Error>()
            .unwrap();
        assert!(cause.url().is_none());
        assert_eq!(error, BackendError::Request);
        assert!(!format!("{error:#?}").contains("never_log"));
        assert!(!format!("{error:#?}").contains(&base));
    }
}
