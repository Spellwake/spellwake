//! Strict shared flow contracts; raw state/code/authorization URL are never diagnostic values.
use super::{PatreonError, identifier};
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub state_key: String,
}
impl LinkRequest {
    /// # Errors
    /// Rejects nil request identity and noncanonical32-byte state keys.
    pub fn state_hash(&self) -> Result<String, &'static str> {
        if self.request_id.is_nil() {
            return Err("invalid link request");
        }
        crate::capability::key_hash(&self.state_key)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompleteLinkRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub state_key: String,
    pub code: String,
}
impl CompleteLinkRequest {
    /// # Errors
    /// Rejects nil identity, noncanonical state and malformed bounded one-use code.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil() || !super::credential(&self.code) {
            return Err("invalid link completion");
        }
        crate::capability::key_hash(&self.state_key).map(|_| ())
    }
    pub(crate) fn flow(&self) -> LinkRequest {
        LinkRequest {
            request_id: self.request_id,
            state_key: self.state_key.clone(),
        }
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerTerms {
    pub version: u8,
    pub campaign_id: String,
    pub client_id: String,
    pub redirect_uri: String,
}
impl ServerTerms {
    /// Validate public server-selected OAuth terms without contacting the provider.
    /// # Errors
    /// Rejects unsupported version, identity or unsafe callback URL.
    pub fn validate(&self) -> Result<(), PatreonError> {
        let url =
            reqwest::Url::parse(&self.redirect_uri).map_err(|_| PatreonError::Configuration)?;
        if self.version != 1
            || !identifier(&self.campaign_id)
            || !identifier(&self.client_id)
            || self.redirect_uri.len() > 2048
            || url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.redirect_uri.bytes().any(|b| b.is_ascii_whitespace())
        {
            return Err(PatreonError::Configuration);
        }
        Ok(())
    }
    pub(crate) fn authorization_url(&self, state: &str) -> Result<String, PatreonError> {
        self.validate()?;
        crate::capability::key_hash(state).map_err(|_| PatreonError::Contract)?;
        let mut url = reqwest::Url::parse("https://www.patreon.com/oauth2/authorize")
            .map_err(|_| PatreonError::Configuration)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("scope", "identity")
            .append_pair("state", state);
        Ok(url.into())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum LinkPhase {
    Unknown,
    Issued,
    Claimed,
    Linked,
    Expired,
    Superseded,
}
impl TryFrom<String> for LinkPhase {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "unknown" => Ok(Self::Unknown),
            "issued" => Ok(Self::Issued),
            "claimed" => Ok(Self::Claimed),
            "linked" => Ok(Self::Linked),
            "expired" => Ok(Self::Expired),
            "superseded" => Ok(Self::Superseded),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Owner-bound flow observation. Linked is a historical association ID, not current paid entitlement.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkStatus {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub phase: LinkPhase,
    #[serde(deserialize_with = "Option::deserialize")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub association_id: Option<Uuid>,
}
impl LinkStatus {
    /// # Errors
    /// Rejects wrong scope, missing/extra phase values and unsupported timestamp precision.
    pub fn validate(&self, account: Uuid, request: Uuid) -> Result<(), &'static str> {
        if account.is_nil()
            || request.is_nil()
            || self.account_id != account
            || self.request_id != request
            || (self.phase == LinkPhase::Unknown) != self.expires_at.is_none()
            || (self.phase == LinkPhase::Linked) != self.association_id.is_some()
            || self.association_id.is_some_and(|id| id.is_nil())
            || self.expires_at.is_some_and(|time| {
                !(1..=9999).contains(&time.year()) || time.timestamp_subsec_nanos() % 1000 != 0
            })
        {
            return Err("invalid link status");
        }
        Ok(())
    }
}
/// Private URL-bearing result. Do not log, cache or persist outside approved scoped OAuth handling.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkStart {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub authorization_url: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InternalStatus {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub phase: LinkPhase,
    #[serde(deserialize_with = "Option::deserialize")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub generation: Option<String>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub server_terms: Option<ServerTerms>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub association_id: Option<Uuid>,
}
impl InternalStatus {
    pub fn validate(&self, account: Uuid, request: Uuid) -> Result<(), PatreonError> {
        self.public()
            .validate(account, request)
            .map_err(|_| PatreonError::Contract)?;
        if (self.phase == LinkPhase::Unknown)
            != (self.generation.is_none() && self.server_terms.is_none())
            || self.generation.as_ref().is_some_and(|v| {
                v.starts_with('0')
                    || v.parse::<i64>().map_or(true, |n| n <= 0)
                    || v.bytes().any(|b| !b.is_ascii_digit())
            })
        {
            return Err(PatreonError::Contract);
        }
        if self.phase != LinkPhase::Unknown
            && (self.generation.is_none() || self.server_terms.is_none())
        {
            return Err(PatreonError::Contract);
        }
        if let Some(terms) = &self.server_terms {
            terms.validate()?
        }
        Ok(())
    }
    pub fn public(&self) -> LinkStatus {
        LinkStatus {
            account_id: self.account_id,
            request_id: self.request_id,
            phase: self.phase,
            expires_at: self.expires_at,
            association_id: self.association_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn shared_flow_fixture_rejects_alias_keys_wrong_scope_phase_and_missing_nulls() {
        let f: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/patreon-association.json"
        ))
        .unwrap();
        let flow: LinkRequest = serde_json::from_value(f["flow"].clone()).unwrap();
        assert_eq!(
            flow.state_hash().unwrap(),
            "66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925"
        );
        let complete: CompleteLinkRequest = serde_json::from_value(f["complete"].clone()).unwrap();
        complete.validate().unwrap();
        let status: LinkStatus = serde_json::from_value(f["status"].clone()).unwrap();
        status.validate(status.account_id, flow.request_id).unwrap();
        let internal: InternalStatus = serde_json::from_value(f["internal"].clone()).unwrap();
        internal
            .validate(status.account_id, flow.request_id)
            .unwrap();
        let terms = internal.server_terms.unwrap();
        let actual =
            reqwest::Url::parse(&terms.authorization_url(&flow.state_key).unwrap()).unwrap();
        let expected =
            reqwest::Url::parse(f["start"]["authorizationUrl"].as_str().unwrap()).unwrap();
        assert_eq!(
            actual
                .query_pairs()
                .collect::<std::collections::BTreeMap<_, _>>(),
            expected.query_pairs().collect()
        );
        for key in [
            "A".repeat(42),
            "A".repeat(44),
            format!("{}B", "A".repeat(42)),
        ] {
            assert!(
                LinkRequest {
                    request_id: flow.request_id,
                    state_key: key
                }
                .state_hash()
                .is_err()
            );
        }
        let mut invalid = f["status"].clone();
        invalid.as_object_mut().unwrap().remove("associationId");
        assert!(serde_json::from_value::<LinkStatus>(invalid).is_err());
        let mut wrong = f["status"].clone();
        wrong["phase"] = json!("claimed");
        assert!(
            serde_json::from_value::<LinkStatus>(wrong)
                .unwrap()
                .validate(status.account_id, flow.request_id)
                .is_err()
        );
        let mut unknown = f["internal"].clone();
        unknown["phase"] = json!("unknown");
        unknown["expiresAt"] = Value::Null;
        assert!(
            serde_json::from_value::<InternalStatus>(unknown)
                .unwrap()
                .validate(status.account_id, flow.request_id)
                .is_err()
        );
        for uri in [
            "http://127.0.0.1/callback",
            "https://user:secret@example.invalid/callback",
            "https://example.invalid/callback?next=other",
            "https://example.invalid/#state",
        ] {
            let mut invalid = terms.clone();
            invalid.redirect_uri = uri.into();
            assert!(invalid.validate().is_err());
        }
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<LinkPhase>(&[
            "unknown",
            "issued",
            "claimed",
            "linked",
            "expired",
            "superseded",
        ]);
    }
}
