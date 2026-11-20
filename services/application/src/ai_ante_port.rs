//! Trusted coordinator input for SQL136's AI-only ante branch. This installs no
//! runtime credential, sponsor-bearer endpoint, AI inventory or provider delivery.
use crate::{
    ApiResponse,
    amount::Amount,
    ante::{OFFER_FAILURES, OfferAck},
    commands::CommandDisposition,
    source_protocol::{
        SourceResponseError, checked, ids, one_failure, record_response, valid_digest,
    },
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Original AI nominal stake, not an account's human asset bundle. Zero withdraws.
/// The adapter supplies SQL's empty asset array; this type cannot carry human assets.
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiAnteStake {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    pub expected_terms_revision: Amount,
    pub coins: Amount,
}
/// Independently resolved original admitted AI-seat facts. No Deserialize: an
/// untrusted request must not supply its own sponsor, controller or allowance day.
/// The trusted adapter must resolve controller_kind='ai' and seat membership from
/// canonical state. Constructing this value is correlation, not authorization proof.
#[derive(Clone, PartialEq, Eq)]
pub struct AdmittedAiAnteContext {
    pub match_id: Uuid,
    pub seat_id: Uuid,
    pub sponsor_id: Uuid,
    pub match_started_at: DateTime<Utc>,
    pub commitment_id: Option<Uuid>,
    pub exposure_id: Option<Uuid>,
}
/// Own the original source and its resolved context across retries and receipt
/// lookups. Current allowance balances and today's UTC day are not replacement inputs.
pub struct PreparedAiAnte {
    source: AiAnteStake,
    context: AdmittedAiAnteContext,
}
impl AiAnteStake {
    /// Canonical identifiers and SQL136's inclusive 0..300 nominal stake bound.
    pub fn valid(&self) -> bool {
        ids(&[self.request_id, self.match_id, self.seat_id]) && self.coins <= 300.into()
    }
    /// Validate a historical acknowledgment without inventing current context.
    /// The canonical SQL original reader separately proves commitment/match identity.
    pub fn valid_ack(&self, ack: &OfferAck) -> bool {
        self.valid()
            && !ack.resource_id.is_nil()
            && self
                .expected_terms_revision
                .checked_add(&1.into())
                .is_ok_and(|n| n == ack.terms_revision)
            && valid_digest(&ack.terms_hash)
    }
    /// Preserve exact stored economic outcomes independently of mutable terms.
    pub fn valid_disposition(&self, disposition: &CommandDisposition<OfferAck>) -> bool {
        self.valid()
            && match disposition {
                CommandDisposition::Accepted { ack } => self.valid_ack(ack),
                CommandDisposition::Rejected { errors } => one_failure(errors, OFFER_FAILURES),
                CommandDisposition::Unknown | CommandDisposition::Pending => true,
            }
    }
}
impl PreparedAiAnte {
    /// Bind the source to original resolved AI membership and sponsor context.
    /// # Errors
    /// Rejects mismatched/nil identities, out-of-range coins or impossible exposure presence.
    /// A valid value does not authenticate the caller or establish database facts.
    pub fn new(
        source: AiAnteStake,
        context: AdmittedAiAnteContext,
    ) -> Result<Self, SourceResponseError> {
        let valid = source.valid()
            && ids(&[context.match_id, context.seat_id, context.sponsor_id])
            && context.match_id == source.match_id
            && context.seat_id == source.seat_id
            && context.commitment_id.is_none_or(|id| !id.is_nil())
            && context.exposure_id.is_none_or(|id| !id.is_nil())
            && (context.exposure_id.is_none() || context.commitment_id.is_some());
        checked(valid, Self { source, context })
    }
    /// Immutable original SQL source fields, never a mutable/rebased request.
    pub fn source(&self) -> &AiAnteStake {
        &self.source
    }
    /// Immutable original facts retained by the trusted coordinator.
    pub fn context(&self) -> &AdmittedAiAnteContext {
        &self.context
    }
    /// The source day follows match admission, not the time of the current retry.
    pub fn allowance_day(&self) -> chrono::NaiveDate {
        self.context.match_started_at.date_naive()
    }
    /// Require the exact original request and resolved context for recovery.
    pub fn matches_request(&self, source: &AiAnteStake, context: &AdmittedAiAnteContext) -> bool {
        self.source == *source && self.context == *context
    }
    /// Validate generated commitment identity, exact successor and canonical SQL hash.
    /// Initial requests have no preexisting commitment; SQL supplies its identity.
    pub fn valid_ack(&self, ack: &OfferAck) -> bool {
        self.source.valid_ack(ack)
            && self
                .context
                .commitment_id
                .is_none_or(|id| id == ack.resource_id)
    }
    /// Decode actual SQL output; SQLSTATE exceptions are separately classified by
    /// source_protocol::classify_sqlstate and never invented as stored failures.
    /// # Errors
    /// Rejects malformed/oversized output, foreign acknowledgments or unknown stored failures.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<OfferAck>, SourceResponseError> {
        if bytes.len() > crate::qualified_intents::MAX_QUALIFIED_INTENT_BYTES {
            return Err(SourceResponseError::context());
        }
        let response = record_response(bytes)?;
        let valid = match &response {
            ApiResponse::Success { data } => self.valid_ack(data),
            ApiResponse::Fail { data } => one_failure(data, OFFER_FAILURES),
            _ => false,
        };
        checked(valid, response)
    }
    /// Bind recovery to the retained source; a later terms revision cannot rebase it.
    /// # Errors
    /// Rejects changed source/context, invalid result identity or unsupported persisted failure.
    pub fn decode_disposition(
        &self,
        source: &AiAnteStake,
        context: &AdmittedAiAnteContext,
        bytes: &[u8],
    ) -> Result<CommandDisposition<OfferAck>, SourceResponseError> {
        if !self.matches_request(source, context) {
            return Err(SourceResponseError::context());
        }
        let result = crate::qualified_intents::decode_intent(bytes)?;
        let valid = match &result {
            CommandDisposition::Accepted { ack } => self.valid_ack(ack),
            CommandDisposition::Rejected { errors } => one_failure(errors, OFFER_FAILURES),
            _ => true,
        };
        checked(valid, result)
    }
}

#[cfg(test)]
mod tests;
