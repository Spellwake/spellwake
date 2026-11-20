//! Stripe delivery authentication only. A notification is not payment or grant proof.
//! Provider reconciliation must still match authoritative objects to stored purchase terms.

pub(crate) mod client;
mod envelope;
pub mod notification;
pub(crate) mod service;
#[cfg(test)]
pub(crate) mod test_fixture;

use crate::amount::Amount;
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{error::Error, fmt};
use uuid::Uuid;

/// Caller-owned purchase projection. Provider references and proof bodies stay private.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PurchaseView {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub purchase_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub cosmetic_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub state: PurchaseState,
    pub revision: Amount,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub price: PurchasePrice,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub checkout: CheckoutView,
}
/// Application payment state; a pending purchase may have an unknown create acknowledgment.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum PurchaseState {
    Creating,
    Pending,
    Paid,
    Canceled,
    Refunded,
    Reversed,
}
impl TryFrom<String> for PurchaseState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "creating" => Ok(Self::Creating),
            "pending" => Ok(Self::Pending),
            "paid" => Ok(Self::Paid),
            "canceled" => Ok(Self::Canceled),
            "refunded" => Ok(Self::Refunded),
            "reversed" => Ok(Self::Reversed),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Exact provider minor units, separate from collection coins.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PurchasePrice {
    pub currency: String,
    pub minor_units: Amount,
}
/// A stored purchase ID is not a promise of a ready checkout session.
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CheckoutView {
    Ready {
        url: String,
        expires_at: DateTime<Utc>,
    },
    Waiting,
    Unavailable,
}
impl PurchaseView {
    /// Validate the exact requested identity and approved navigation contract.
    ///
    /// # Errors
    /// Returns Contract for inconsistent identity, units, currency or checkout URL.
    pub fn validate(&self, expected: Uuid) -> Result<(), StripeError> {
        if self.purchase_id != expected
            || self.cosmetic_id.is_nil()
            || self.price.minor_units.is_zero()
            || self.price.minor_units.to_string().len() > 8
            || self.price.currency.len() != 3
            || !self.price.currency.bytes().all(|b| b.is_ascii_lowercase())
        {
            return Err(StripeError::Contract);
        }
        if let CheckoutView::Ready { url, .. } = &self.checkout {
            let parsed = reqwest::Url::parse(url).map_err(|_| StripeError::Contract)?;
            if url.len() > 4096
                || parsed.scheme() != "https"
                || parsed.origin().ascii_serialization() != "https://checkout.stripe.com"
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                return Err(StripeError::Contract);
            }
        }
        Ok(())
    }
}

/// Ingress limits must also be applied before a future HTTP handler buffers its body.
pub const MAX_WEBHOOK_BODY: usize = 512 * 1024;
/// Bound parsing before allocating any signature candidates.
pub const MAX_SIGNATURE_HEADER: usize = 8 * 1024;
const DELIVERY_TOLERANCE_SECONDS: u64 = 300;

/// Safe boundary errors retain only sanitized syntax/shape causes.
#[derive(Debug)]
pub enum StripeError {
    /// Server-selected secret/version configuration is malformed.
    Configuration,
    /// Input exceeds the selected ingress bounds.
    Intake,
    /// The supplied server clock is not a supported Unix timestamp.
    Clock,
    /// Header, freshness, or HMAC verification failed.
    Signature,
    /// A syntax/shape error with private scalar values excluded from its cause.
    Envelope(serde_json::Error),
    /// Signed metadata does not match the selected version/mode/object contract.
    Contract,
    /// Underlying MAC initialization failure, without credential contents.
    Key(hmac::digest::InvalidLength),
}
impl fmt::Display for StripeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Configuration | Self::Key(_) => "stripe verifier configuration is invalid",
            Self::Intake => "stripe notification exceeds its intake contract",
            Self::Clock => "stripe delivery clock is invalid",
            Self::Signature => "stripe delivery authentication failed",
            Self::Envelope(_) => "stripe notification envelope is invalid",
            Self::Contract => "stripe notification does not match the configured contract",
        })
    }
}
impl Error for StripeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Envelope(error) => Some(error),
            Self::Key(error) => Some(error),
            _ => None,
        }
    }
}

/// A routing signal names a provider object; it never establishes a payment state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripeEventKind {
    /// Checkout completed; this alone does not establish payment success.
    CheckoutCompleted,
    /// Stripe signals delayed payment success for a checkout.
    CheckoutAsyncSucceeded,
    /// Stripe signals delayed payment failure for a checkout.
    CheckoutAsyncFailed,
    /// Stripe signals checkout expiration.
    CheckoutExpired,
    /// A charge-success signal requiring purchase/term reconciliation.
    ChargeSucceeded,
    /// A charge-refund signal, potentially a partial refund.
    ChargeRefunded,
    /// A charge changed; no terminal disposition is implied.
    ChargeUpdated,
    /// A refund was created; it may still be pending.
    RefundCreated,
    /// A refund changed; inspect its authoritative disposition.
    RefundUpdated,
    /// A refund failed; this is not a successful refund.
    RefundFailed,
    /// A dispute began; no final outcome is implied.
    DisputeCreated,
    /// A dispute changed.
    DisputeUpdated,
    /// A dispute closed; the signal alone does not select its outcome.
    DisputeClosed,
    /// Dispute-related funds were withdrawn.
    DisputeFundsWithdrawn,
    /// Dispute-related funds were reinstated.
    DisputeFundsReinstated,
}
impl StripeEventKind {
    fn classify(kind: &str) -> Option<(Self, &'static str, &'static str)> {
        Some(match kind {
            "checkout.session.completed" => (Self::CheckoutCompleted, "checkout.session", "cs_"),
            "checkout.session.async_payment_succeeded" => {
                (Self::CheckoutAsyncSucceeded, "checkout.session", "cs_")
            }
            "checkout.session.async_payment_failed" => {
                (Self::CheckoutAsyncFailed, "checkout.session", "cs_")
            }
            "checkout.session.expired" => (Self::CheckoutExpired, "checkout.session", "cs_"),
            "charge.succeeded" => (Self::ChargeSucceeded, "charge", "ch_"),
            "charge.refunded" => (Self::ChargeRefunded, "charge", "ch_"),
            "charge.updated" => (Self::ChargeUpdated, "charge", "ch_"),
            "refund.created" => (Self::RefundCreated, "refund", "re_"),
            "refund.updated" => (Self::RefundUpdated, "refund", "re_"),
            "refund.failed" => (Self::RefundFailed, "refund", "re_"),
            "charge.dispute.created" => (Self::DisputeCreated, "dispute", "du_"),
            "charge.dispute.updated" => (Self::DisputeUpdated, "dispute", "du_"),
            "charge.dispute.closed" => (Self::DisputeClosed, "dispute", "du_"),
            "charge.dispute.funds_withdrawn" => (Self::DisputeFundsWithdrawn, "dispute", "du_"),
            "charge.dispute.funds_reinstated" => (Self::DisputeFundsReinstated, "dispute", "du_"),
            _ => return None,
        })
    }
}

/// Opaque authenticated metadata. No constructor, deserializer, or raw body escape.
/// Passing its supported signal alone to a grant writer would discard this boundary.
pub struct VerifiedStripeNotification {
    id: String,
    event_type: String,
    created: DateTime<Utc>,
    api_version: String,
    live: bool,
    body_sha256: String,
    signal: Option<(StripeEventKind, String)>,
}
impl VerifiedStripeNotification {
    /// Provider event identity for a future durable deduplication boundary.
    pub fn event_id(&self) -> &str {
        &self.id
    }
    /// Authenticated provider event type, including unsupported types.
    pub fn event_type(&self) -> &str {
        &self.event_type
    }
    /// Provider event creation time, distinct from signed delivery freshness.
    pub fn created(&self) -> DateTime<Utc> {
        self.created
    }
    /// Exact server-selected API version matched by the authenticated envelope.
    pub fn api_version(&self) -> &str {
        &self.api_version
    }
    /// Live/test mode already matched to the endpoint configuration.
    pub fn is_live(&self) -> bool {
        self.live
    }
    /// Lowercase SHA256 hex of the unchanged authenticated body bytes.
    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }
    /// Optional supported routing kind/object ID, never payment or grant proof.
    pub fn signal(&self) -> Option<(StripeEventKind, &str)> {
        self.signal.as_ref().map(|(kind, id)| (*kind, id.as_str()))
    }
}

/// Endpoint-specific secret and server-selected event contract. Never log or serialize.
pub struct StripeWebhookVerifier {
    secret: String,
    api_version: String,
    live: bool,
}
impl StripeWebhookVerifier {
    /// Configure one endpoint secret and its exact event API version and mode.
    ///
    /// # Errors
    /// Returns Configuration for invalid bounded ASCII credentials/version tokens.
    pub fn new(secret: String, api_version: String, live: bool) -> Result<Self, StripeError> {
        if !secret.starts_with("whsec_")
            || !(16..=512).contains(&secret.len())
            || !secret
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || api_version.is_empty()
            || api_version.len() > 64
            || !api_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        {
            return Err(StripeError::Configuration);
        }
        Ok(Self {
            secret,
            api_version,
            live,
        })
    }

    /// Authenticate unchanged bytes before parsing any vendor fields.
    ///
    /// The caller supplies one header and a trusted Unix-seconds clock. A future
    /// HTTP adapter must reject duplicate signature headers and enforce body limits
    /// while reading. Event creation may be old; the signed delivery must be fresh.
    ///
    /// # Errors
    /// Returns Intake for oversized inputs, Clock for an invalid server clock,
    /// Signature for malformed/stale/unauthenticated delivery, Envelope for safely
    /// parsed syntax/shape errors, and Contract for wrong version/mode/object IDs.
    /// Key retains an underlying MAC-initialization error if initialization fails.
    pub fn verify(
        &self,
        body: &[u8],
        header: &str,
        now: i64,
    ) -> Result<VerifiedStripeNotification, StripeError> {
        if body.is_empty()
            || body.len() > MAX_WEBHOOK_BODY
            || header.is_empty()
            || header.len() > MAX_SIGNATURE_HEADER
        {
            return Err(StripeError::Intake);
        }
        if now < 0 || DateTime::<Utc>::from_timestamp(now, 0).is_none() {
            return Err(StripeError::Clock);
        }
        let (timestamp, candidates) = signature_header(header)?;
        if now.abs_diff(timestamp) > DELIVERY_TOLERANCE_SECONDS {
            return Err(StripeError::Signature);
        }
        let mut mac =
            Hmac::<Sha256>::new_from_slice(self.secret.as_bytes()).map_err(StripeError::Key)?;
        mac.update(timestamp.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        // Each candidate uses the library's constant-time verification primitive.
        // Do not replace this with an equality comparison of computed hex strings.
        let mut valid = false;
        for candidate in candidates {
            valid |= mac.clone().verify_slice(&candidate).is_ok();
        }
        if !valid {
            return Err(StripeError::Signature);
        }
        let event = envelope::parse(body).map_err(StripeError::Envelope)?;
        if event.object != "event"
            || !provider_id(&event.id, "evt_")
            || event.api_version != self.api_version
            || event.live != self.live
            || event.kind.is_empty()
            || event.kind.len() > 128
            || !event
                .kind
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.'))
        {
            return Err(StripeError::Contract);
        }
        let created = DateTime::<Utc>::from_timestamp(event.created, 0)
            .filter(|_| event.created >= 0)
            .ok_or(StripeError::Contract)?;
        let signal = if let Some((kind, object, prefix)) = StripeEventKind::classify(&event.kind) {
            let target = event.data.0.object.0;
            let id = target.id.ok_or(StripeError::Contract)?;
            if target.object.as_deref() != Some(object) || !provider_id(&id, prefix) {
                return Err(StripeError::Contract);
            }
            Some((kind, id))
        } else {
            None
        };
        Ok(VerifiedStripeNotification {
            id: event.id,
            event_type: event.kind,
            created,
            api_version: event.api_version,
            live: event.live,
            body_sha256: crate::provider_json::fingerprint(body),
            signal,
        })
    }
}

fn provider_id(value: &str, prefix: &str) -> bool {
    value.len() <= 255
        && value.strip_prefix(prefix).is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}

fn signature_header(header: &str) -> Result<(i64, Vec<[u8; 32]>), StripeError> {
    if !header.is_ascii() || header.bytes().any(|b| b == 127 || (b < 32 && b != b'\t')) {
        return Err(StripeError::Signature);
    }
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for (index, item) in header.split(',').enumerate() {
        if index >= 32 {
            return Err(StripeError::Signature);
        }
        let (key, value) = item
            .trim_matches([' ', '\t'])
            .split_once('=')
            .ok_or(StripeError::Signature)?;
        if key.is_empty()
            || key.len() > 32
            || value.is_empty()
            || value.len() > 256
            || !key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
            || !value.bytes().all(|b| b.is_ascii_graphic() && b != b'=')
        {
            return Err(StripeError::Signature);
        }
        match key {
            "t" => {
                if timestamp.is_some()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                    || (value.len() > 1 && value.starts_with('0'))
                {
                    return Err(StripeError::Signature);
                }
                timestamp = Some(value.parse::<i64>().map_err(|_| StripeError::Signature)?);
            }
            "v1" => {
                if signatures.len() >= 8 {
                    return Err(StripeError::Signature);
                }
                signatures.push(decode_signature(value)?);
            }
            _ => {}
        }
    }
    if signatures.is_empty() {
        return Err(StripeError::Signature);
    }
    Ok((timestamp.ok_or(StripeError::Signature)?, signatures))
}

fn decode_signature(value: &str) -> Result<[u8; 32], StripeError> {
    if value.len() != 64 {
        return Err(StripeError::Signature);
    }
    let digit = |b| match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(StripeError::Signature),
    };
    let mut decoded = [0; 32];
    for (target, pair) in decoded.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        *target = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Ok(decoded)
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<PurchaseState>(&[
            "creating", "pending", "paid", "canceled", "refunded", "reversed",
        ]);
    }
}
