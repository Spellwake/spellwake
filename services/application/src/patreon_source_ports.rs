//! Closed normalized Patreon financial/notification contracts. Existing OAuth,
//! current-member refresh, intake and Temporal protocols are preserved. No feed,
//! notification-completion worker, database grant or Discord integration is added.
use crate::{
    amount::Amount,
    source_protocol::{
        SourceProof, SourceResponseError, checked, flat_jsonb_octets, parse_utc_microsecond,
        valid_digest,
    },
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, de};
use serde_json::{Value, json};
use uuid::Uuid;
///119 bounds the proof itself, not a reconstructed or enriched event source.
pub const MAX_FINANCIAL_PROOF_BYTES: usize = 65_536;
#[derive(Deserialize)]
#[serde(try_from = "String")]
enum FinancialFamily {
    #[serde(rename = "patreon-financial-v1")]
    V1,
}
/// Immutable financial observation state. A refund/reversal is terminal for Patreon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", try_from = "String")]
pub enum FinancialState {
    Paid,
    Refunded,
    Reversed,
}
impl FinancialState {
    /// Canonical source text, never current membership state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paid => "paid",
            Self::Refunded => "refunded",
            Self::Reversed => "reversed",
        }
    }
}
struct FinancialInstant {
    wire: String,
    instant: DateTime<Utc>,
}
impl<'de> Deserialize<'de> for FinancialInstant {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let wire = String::deserialize(input)?;
        let instant = parse_utc_microsecond(&wire)
            .ok_or_else(|| de::Error::custom("invalid canonical source instant"))?;
        Ok(Self { wire, instant })
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FinancialProof {
    family: FinancialFamily,
    occurrence_ref: SourceProof,
    member_ref: SourceProof,
    payment_ref: SourceProof,
    tier_ref: SourceProof,
    initiated_at: FinancialInstant,
    paid_at: FinancialInstant,
    occurred_at: FinancialInstant,
    state: FinancialState,
    amount_minor: Amount,
    currency: String,
    body_hash: String,
    evidence_ref: SourceProof,
}
fn bound(value: &SourceProof, max: usize) -> bool {
    value.as_str().trim_matches(' ').chars().count() <= max
}
impl FinancialProof {
    fn terms(&self) -> Value {
        let FinancialFamily::V1 = self.family;
        json!({"family":"patreon-financial-v1","occurrenceRef":self.occurrence_ref.as_str(),"memberRef":self.member_ref.as_str(),"paymentRef":self.payment_ref.as_str(),"tierRef":self.tier_ref.as_str(),"initiatedAt":self.initiated_at.wire,"paidAt":self.paid_at.wire,"occurredAt":self.occurred_at.wire,"state":self.state.as_str(),"amountMinor":self.amount_minor,"currency":self.currency,"bodyHash":self.body_hash,"evidenceRef":self.evidence_ref.as_str()})
    }
    fn valid(&self) -> bool {
        [
            &self.occurrence_ref,
            &self.member_ref,
            &self.payment_ref,
            &self.tier_ref,
        ]
        .iter()
        .all(|r| bound(r, 256))
            && !self.amount_minor.is_zero()
            && self.amount_minor.to_string().len() <= 30
            && self.currency.len() == 3
            && self.currency.bytes().all(|b| b.is_ascii_lowercase())
            && valid_digest(&self.body_hash)
            && self.initiated_at.instant <= self.paid_at.instant
            && self.paid_at.instant <= self.occurred_at.instant
            && (self.state != FinancialState::Paid
                || self.paid_at.instant == self.occurred_at.instant)
            && flat_jsonb_octets(&self.terms()).is_some_and(|n| n <= MAX_FINANCIAL_PROOF_BYTES)
    }
}
/// Original delivery and validated normalized financial proof, retained in memory.
/// This envelope does not qualify a financial feed or provider evidence.
pub struct FinancialObservation {
    delivery_ref: SourceProof,
    proof: FinancialProof,
}
/// Canonical result disposition. Duplicate deliveries return the canonical result;
/// SQL does not return a separate `duplicate` disposition here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", try_from = "String")]
pub enum FinancialDisposition {
    Applied,
    Noop,
    IneligibleAfterDeletion,
}
/// Bare119 result, not a public API envelope or a current payment query.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinancialAck {
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub payment_id: Option<Uuid>,
    pub disposition: FinancialDisposition,
    pub state: FinancialState,
}
impl FinancialObservation {
    /// Parse original source bytes, preserving exact strings and rejecting duplicate fields.
    /// PostgreSQL separately verifies occurredAt against its clock and derives association.
    /// # Errors
    /// Rejects malformed, noncanonical, incorrectly ordered or over-budget source envelopes.
    pub fn from_bytes(delivery_ref: &str, proof: &[u8]) -> Result<Self, SourceResponseError> {
        let delivery_ref: SourceProof = delivery_ref
            .parse()
            .map_err(|_| SourceResponseError::context())?;
        let proof: FinancialProof = crate::decode_wire_object(proof)?;
        checked(
            bound(&delivery_ref, 200) && proof.valid(),
            Self {
                delivery_ref,
                proof,
            },
        )
    }
    /// Original opaque provider delivery identity, never an application UUID.
    pub fn delivery_ref(&self) -> &str {
        self.delivery_ref.as_str()
    }
    /// Original private proof fields for a future trusted adapter; never public diagnostics.
    pub fn original_terms(&self) -> Value {
        self.proof.terms()
    }
    /// Exact proof byte size after PostgreSQL JSONB rendering; not a canonical source hash.
    pub fn original_terms_octets(&self) -> Option<usize> {
        flat_jsonb_octets(&self.proof.terms())
    }
    /// Decode a bare historical source result, without re-reading today's membership/payment.
    /// # Errors
    /// Rejects malformed output and impossible disposition/null/state combinations.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<FinancialAck, SourceResponseError> {
        let data: FinancialAck = crate::decode_wire_object(bytes)?;
        let valid = match data.disposition {
            FinancialDisposition::Applied => {
                data.payment_id.is_some() && data.state == self.proof.state
            }
            FinancialDisposition::IneligibleAfterDeletion => {
                data.payment_id.is_none() && data.state == self.proof.state
            }
            FinancialDisposition::Noop => {
                data.payment_id.is_some()
                    && (data.state != FinancialState::Paid
                        || self.proof.state == FinancialState::Paid)
            }
        };
        checked(valid, data)
    }
}
impl FinancialAck {
    /// Compare a supplied known historical payment expectation exactly, including NULL.
    /// Not calling this method makes no additional identity-context claim.
    pub fn matches_payment(&self, expected: Option<Uuid>) -> bool {
        expected.is_none_or(|id| !id.is_nil()) && self.payment_id == expected
    }
}
/// Original183 notification source identity, not financial grant authority.
pub struct ProcessNotification {
    pub event_id: Uuid,
}
/// Processed notification result, including accepted ignored branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", try_from = "String")]
pub enum NotificationDisposition {
    Refreshed,
    IgnoredUnlinked,
    IgnoredInactive,
    IgnoredStaleMember,
}
/// Bare183 response; both nullable fields must be present.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub event_id: Uuid,
    pub disposition: NotificationDisposition,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub association_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub refresh_source_id: Option<Uuid>,
}
/// Independently retained historical identity expectations. NULL is known absence,
/// not a request to skip comparison or to infer today's association.
pub struct NotificationContext {
    pub association_id: Option<Uuid>,
    pub refresh_source_id: Option<Uuid>,
}
impl ProcessNotification {
    /// Decode one stored source result, never a generic error/fail response.
    /// # Errors
    /// Rejects malformed data, changed event identity and invalid conditional-null shapes.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<NotificationAck, SourceResponseError> {
        let data: NotificationAck = crate::decode_wire_object(bytes)?;
        let valid = !self.event_id.is_nil()
            && data.event_id == self.event_id
            && match data.disposition {
                NotificationDisposition::Refreshed => {
                    data.association_id.is_some() && data.refresh_source_id.is_some()
                }
                NotificationDisposition::IgnoredUnlinked => {
                    data.association_id.is_none() && data.refresh_source_id.is_none()
                }
                NotificationDisposition::IgnoredInactive
                | NotificationDisposition::IgnoredStaleMember => {
                    data.association_id.is_some() && data.refresh_source_id.is_none()
                }
            };
        checked(valid, data)
    }
}
impl NotificationAck {
    /// Compare both known-nullable original source IDs; no current-member inference.
    pub fn matches_source(&self, context: &NotificationContext) -> bool {
        [context.association_id, context.refresh_source_id]
            .into_iter()
            .flatten()
            .all(|id| !id.is_nil())
            && self.association_id == context.association_id
            && self.refresh_source_id == context.refresh_source_id
    }
}
#[cfg(test)]
mod tests;

impl TryFrom<String> for FinancialFamily {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "patreon-financial-v1" => Ok(Self::V1),
            _ => Err("invalid financial source family"),
        }
    }
}
impl TryFrom<String> for FinancialState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "paid" => Ok(Self::Paid),
            "refunded" => Ok(Self::Refunded),
            "reversed" => Ok(Self::Reversed),
            _ => Err("invalid financial source state"),
        }
    }
}
impl TryFrom<String> for FinancialDisposition {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "applied" => Ok(Self::Applied),
            "noop" => Ok(Self::Noop),
            "ineligible_after_deletion" => Ok(Self::IneligibleAfterDeletion),
            _ => Err("invalid financial disposition"),
        }
    }
}
impl TryFrom<String> for NotificationDisposition {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "refreshed" => Ok(Self::Refreshed),
            "ignored_unlinked" => Ok(Self::IgnoredUnlinked),
            "ignored_inactive" => Ok(Self::IgnoredInactive),
            "ignored_stale_member" => Ok(Self::IgnoredStaleMember),
            _ => Err("invalid notification disposition"),
        }
    }
}
