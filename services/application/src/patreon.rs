//! Patreon source authentication and selected observations, separate from account/grant authority.
//! A signed member notification has no signed delivery time or globally unique event ID.
pub mod association;
pub mod client;
mod evidence_worker;
pub(crate) mod intake;
pub use evidence_worker::{PatreonEvidenceWorkerError, run_evidence_worker};
#[cfg(test)]
mod member_activity_test_support;
mod member_context;
#[cfg(test)]
mod member_installer_tests;
mod member_sweep;
#[cfg(test)]
mod member_sweep_codec;
pub(crate) mod oauth;
mod refresh;
pub mod refresh_workflow;
pub(crate) mod service;
mod source;
use crate::provider_json::{Object, string};
use hmac::{Hmac, KeyInit, Mac};
use md5::Md5;
pub use refresh::{
    PatreonRefreshAttempt, PatreonRefreshError, PatreonRefreshOutcome, PatreonRefreshService,
    PatreonRefreshTask,
};
use serde::Deserialize;
use std::{error::Error, fmt};

/// Buffer limits must also be enforced by the eventual HTTP ingress before buffering.
pub const MAX_WEBHOOK_BODY: usize = 512 * 1024;
/// Safe errors preserve underlying transport/syntax causes without private value diagnostics.
pub enum PatreonError {
    Configuration,
    Intake,
    Signature,
    Contract,
    Json(serde_json::Error),
    Key(hmac::digest::InvalidLength),
    Network(reqwest::Error),
    Body(std::io::Error),
    Status(u16),
}
impl fmt::Display for PatreonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration | Self::Key(_) => "patreon source configuration is invalid",
            Self::Intake => "patreon source exceeds its intake bounds",
            Self::Signature => "patreon notification authentication failed",
            Self::Contract | Self::Json(_) => "patreon source does not match its contract",
            Self::Network(_) | Self::Body(_) | Self::Status(_) => {
                "patreon source could not be established"
            }
        })
    }
}
impl fmt::Debug for PatreonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for PatreonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            Self::Key(e) => Some(e),
            Self::Network(e) => Some(e),
            Self::Body(e) => Some(e),
            _ => None,
        }
    }
}
impl From<reqwest::Error> for PatreonError {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error.without_url())
    }
}
pub(super) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
pub(super) fn credential(value: &str) -> bool {
    (16..=4096).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_graphic())
}
#[derive(Deserialize)]
struct MemberEnvelope {
    data: Object<MemberRoute>,
}
#[derive(Deserialize)]
struct MemberRoute {
    #[serde(rename = "type", deserialize_with = "string")]
    kind: String,
    #[serde(deserialize_with = "string")]
    id: String,
    relationships: Object<MemberRelationships>,
}
#[derive(Deserialize)]
pub(super) struct MemberRelationships {
    pub campaign: Object<One>,
    pub user: Object<One>,
}
#[derive(Deserialize)]
pub(super) struct One {
    pub data: Object<Reference>,
}
#[derive(Deserialize)]
pub(super) struct Reference {
    #[serde(rename = "type", deserialize_with = "string")]
    pub kind: String,
    #[serde(deserialize_with = "string")]
    pub id: String,
}
impl Reference {
    fn matches(&self, kind: &str) -> bool {
        self.kind == kind && identifier(&self.id)
    }
}
/// An authenticated routing signal only. Never a login, membership state or payment proof.
/// No Debug, Serialize or public constructor; the raw provider payload stays private.
pub struct VerifiedPatreonNotification {
    member_id: String,
    user_id: String,
    campaign_id: String,
    body_sha256: String,
}
impl VerifiedPatreonNotification {
    pub fn member_id(&self) -> &str {
        &self.member_id
    }
    pub fn user_id(&self) -> &str {
        &self.user_id
    }
    pub fn campaign_id(&self) -> &str {
        &self.campaign_id
    }
    /// Transport fingerprint, not a provider-assigned event or payment identifier.
    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }
}
/// Endpoint-specific secret and campaign. Never serialize or log either credential.
pub struct PatreonWebhookVerifier {
    secret: String,
    campaign_id: String,
}
impl PatreonWebhookVerifier {
    /// Configure one campaign endpoint. No provider is contacted.
    /// # Errors
    /// Rejects malformed bounded campaign IDs or secrets.
    pub fn new(secret: String, campaign_id: String) -> Result<Self, PatreonError> {
        if !credential(&secret) || secret.len() > 512 || !identifier(&campaign_id) {
            return Err(PatreonError::Configuration);
        }
        Ok(Self {
            secret,
            campaign_id,
        })
    }
    /// Verify unchanged bytes before parsing. The unsigned event header is intentionally unused.
    /// # Errors
    /// Rejects oversized bodies, malformed/incorrect MACs, missing references and other campaigns.
    /// Replays authenticate: Patreon supplies no signed freshness timestamp in this protocol.
    pub fn verify(
        &self,
        body: &[u8],
        signature: &str,
    ) -> Result<VerifiedPatreonNotification, PatreonError> {
        if body.is_empty() || body.len() > MAX_WEBHOOK_BODY {
            return Err(PatreonError::Intake);
        }
        if signature.len() != 32 || !signature.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(PatreonError::Signature);
        }
        let mut expected = [0u8; 16];
        for (byte, pair) in expected
            .iter_mut()
            .zip(signature.as_bytes().as_chunks::<2>().0)
        {
            let nibble = |b: u8| {
                if b.is_ascii_digit() {
                    b - b'0'
                } else {
                    b.to_ascii_lowercase() - b'a' + 10
                }
            };
            *byte = nibble(pair[0]) * 16 + nibble(pair[1]);
        }
        let mut mac =
            Hmac::<Md5>::new_from_slice(self.secret.as_bytes()).map_err(PatreonError::Key)?;
        mac.update(body);
        mac.verify_slice(&expected)
            .map_err(|_| PatreonError::Signature)?;
        let envelope = serde_json::from_slice::<Object<MemberEnvelope>>(body)
            .map_err(PatreonError::Json)?
            .0;
        let member = envelope.data.0;
        let refs = member.relationships.0;
        if member.kind != "member"
            || !identifier(&member.id)
            || !refs.campaign.0.data.0.matches("campaign")
            || refs.campaign.0.data.0.id != self.campaign_id
            || !refs.user.0.data.0.matches("user")
        {
            return Err(PatreonError::Contract);
        }
        Ok(VerifiedPatreonNotification {
            member_id: member.id,
            user_id: refs.user.0.data.0.id,
            campaign_id: self.campaign_id.clone(),
            body_sha256: crate::provider_json::fingerprint(body),
        })
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod test_fixture;
