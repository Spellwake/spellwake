//! Private deadline and sealed-pool selection contracts. No scheduler, native
//! validator, human action authority or runtime database adapter is installed.
use crate::source_protocol::utc_microsecond_text as due_text;
use crate::{
    ApiResponse,
    amount::Amount,
    handoffs::TimerSnapshot,
    source_protocol::{
        SourceProof, SourceResponseError, checked, flat_jsonb_octets, ids, one_failure,
        record_response,
    },
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
/// Source191 budget on original schedule/expiry terms, not enriched receipt data.
pub const MAX_DEADLINE_TERMS_BYTES: usize = 65_536;
fn advanced(value: &Amount, basis: &Amount) -> bool {
    basis
        .checked_add(&Amount::from(1))
        .is_ok_and(|next| &next == value)
}
/// Frozen trusted timing policy for a pending original between-game decision.
pub struct ScheduleDeadline {
    pub decision_id: Uuid,
    pub match_revision: Amount,
    pub deadline_revision: Amount,
    pub clock_revision: Amount,
    pub availability_revision: Amount,
    pub profile_id: Uuid,
    pub policy_ref: SourceProof,
    pub due_at: DateTime<Utc>,
    pub qualification_ref: SourceProof,
    pub request_id: Uuid,
}
/// Arm acknowledgment, not evidence that a scheduler delivered its handoff.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub deadline_revision: Amount,
    pub due_at: DateTime<Utc>,
}
impl ScheduleDeadline {
    /// Exact flat original JSONB byte size. This does not calculate a source hash.
    pub fn original_terms_octets(&self) -> Option<usize> {
        flat_jsonb_octets(
            &json!({"decisionId":self.decision_id,"matchRevision":self.match_revision,"deadlineRevision":self.deadline_revision,"clockRevision":self.clock_revision,"availabilityRevision":self.availability_revision,"profileId":self.profile_id,"policyRef":self.policy_ref.as_str(),"dueAt":due_text(self.due_at)?,"proofRef":self.qualification_ref.as_str()}),
        )
    }
    /// Decode against original generations and due instant, never a new retry clock.
    /// # Errors
    /// Rejects invalid input, budget overflow, malformed output or mismatched source.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<ScheduleAck>, SourceResponseError> {
        let response: ApiResponse<ScheduleAck> = record_response(bytes)?;
        let valid = ids(&[self.decision_id, self.profile_id, self.request_id])
            && self
                .original_terms_octets()
                .is_some_and(|n| n <= MAX_DEADLINE_TERMS_BYTES)
            && match &response {
                ApiResponse::Success { data } => {
                    self.deadline_revision.is_zero()
                        && data.resource_id == self.decision_id
                        && data.deadline_revision == Amount::from(1)
                        && data.due_at == self.due_at
                }
                ApiResponse::Fail { data } => one_failure(
                    data,
                    &[
                        ("decision", "unavailable"),
                        ("decision", "unavailable for a new qualified deadline"),
                    ],
                ),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
/// Read-only193 discovery; an empty result does not complete or cancel a timer.
pub struct DueDeadlines {
    pub after: Option<Uuid>,
    pub limit: u16,
}
/// Source193 has no nextCursor. Its observedAt precedes the statement reading
/// work, so this is not an as-of snapshot and does not bound accountedAt.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DueDeadlinePage {
    pub observed_at: DateTime<Utc>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<TimerSnapshot>,
}
impl DueDeadlines {
    /// Validate the maintained nonnil cursor subset and actual source page bound.
    pub fn valid(&self) -> bool {
        self.after.is_none_or(|id| !id.is_nil()) && (1..=100).contains(&self.limit)
    }
    /// Decode source work, preserving ordered decision identities and exact armed generations.
    /// # Errors
    /// Rejects malformed work, out-of-order/duplicate/over-limit items and not-yet-due work.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<DueDeadlinePage, SourceResponseError> {
        let page: DueDeadlinePage = crate::decode_wire_object(bytes)?;
        checked(page.valid_for(self), page)
    }
}
impl DueDeadlinePage {
    /// Check one source page against its exact discovery cursor and limit.
    pub fn valid_for(&self, original: &DueDeadlines) -> bool {
        let mut previous = original.after;
        original.valid()
            && self.items.len() <= usize::from(original.limit)
            && self.items.iter().all(|item| {
                let valid = item.valid_armed()
                    && item.due_at <= self.observed_at
                    && previous.is_none_or(|id| item.decision_id > id);
                previous = Some(item.decision_id);
                valid
            })
    }
    /// Conservative continuation only for a source page already validated by DueDeadlines.
    /// This is local metadata, not a source field or proof that more work exists.
    pub fn continuation(&self, original: &DueDeadlines) -> Option<Uuid> {
        (self.valid_for(original) && self.items.len() == usize::from(original.limit))
            .then(|| self.items.last().map(|item| item.decision_id))
            .flatten()
    }
}
/// Original selection from a trusted immutable-pool/native legality validator.
pub struct BetweenGameReply {
    pub decision_id: Uuid,
    pub basis: Amount,
    pub selected_revision_id: Uuid,
    pub reply_ref: SourceProof,
    pub qualification_ref: SourceProof,
    pub request_id: Uuid,
}
/// Source192 uses a required empty string when no next game is reserved yet.
/// NULL/missing is rejected; the stored protocol and fingerprint are unchanged.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BetweenReplyAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::empty_uuid")]
    pub reserved_game_id: Option<Uuid>,
    pub qualification_ref: SourceProof,
}
pub(crate) const SELECTION_FAILURES: &[(&str, &str)] = &[
    ("basis", "stale or unavailable between-game decision"),
    ("deck", "unavailable selected revision"),
    ("deck", "selection changes the immutable original card pool"),
];
impl BetweenGameReply {
    /// Decode exact original selection response, including the legacy empty UUID sentinel.
    /// # Errors
    /// Rejects invalid identity, malformed data, changed qualification or revision/failure drift.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<BetweenReplyAck>, SourceResponseError> {
        let response: ApiResponse<BetweenReplyAck> = record_response(bytes)?;
        let valid = ids(&[self.decision_id, self.selected_revision_id, self.request_id])
            && match &response {
                ApiResponse::Success { data } => {
                    data.resource_id == self.decision_id
                        && advanced(&data.revision, &self.basis)
                        && data.qualification_ref.as_str() == self.qualification_ref.as_str()
                }
                ApiResponse::Fail { data } => {
                    one_failure(data, SELECTION_FAILURES)
                        || one_failure(data, &[("deadline", "qualified cutoff has elapsed")])
                }
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
/// Trusted native default selection for one immutable armed deadline generation.
pub struct ExpireDeadline {
    pub decision_id: Uuid,
    pub deadline_revision: Amount,
    pub selected_revision_id: Uuid,
    pub reply_ref: SourceProof,
    pub qualification_ref: SourceProof,
    pub request_id: Uuid,
}
/// The match revision is sampled inside SQL, not supplied in the expiry hash.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpiryAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub reply_receipt_id: Uuid,
    pub deadline_revision: Amount,
    pub revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::empty_uuid")]
    pub reserved_game_id: Option<Uuid>,
}
impl ExpireDeadline {
    /// Exact original outer terms, not the inner reply or enriched snapshot envelope.
    pub fn original_terms_octets(&self) -> Option<usize> {
        flat_jsonb_octets(
            &json!({"decisionId":self.decision_id,"deadlineRevision":self.deadline_revision,"selectedRevisionId":self.selected_revision_id,"replyRef":self.reply_ref.as_str(),"proofRef":self.qualification_ref.as_str()}),
        )
    }
    /// Decode original expiry; early/stale delivery is an SQL retry exception, not stored fail.
    /// # Errors
    /// Rejects malformed/over-budget input, changed deadline identity and undeclared failures.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<ExpiryAck>, SourceResponseError> {
        let response: ApiResponse<ExpiryAck> = record_response(bytes)?;
        let valid = ids(&[self.decision_id, self.selected_revision_id, self.request_id])
            && !self.deadline_revision.is_zero()
            && self
                .original_terms_octets()
                .is_some_and(|n| n <= MAX_DEADLINE_TERMS_BYTES)
            && match &response {
                ApiResponse::Success { data } => {
                    data.resource_id == self.decision_id
                        && data.deadline_revision == self.deadline_revision
                        && !data.revision.is_zero()
                }
                ApiResponse::Fail { data } => one_failure(data, SELECTION_FAILURES),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
impl ExpiryAck {
    /// Optional binding to SQL's actual sampled basis and retained inner receipt.
    /// A stale discovery basis cannot supply this context.
    pub fn matches_source(&self, reply_receipt: Uuid, actual_sampled_basis: &Amount) -> bool {
        !reply_receipt.is_nil()
            && self.reply_receipt_id == reply_receipt
            && advanced(&self.revision, actual_sampled_basis)
    }
}

#[cfg(test)]
mod tests;
