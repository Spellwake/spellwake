//! Original human stakes and redacted accepted terms; no AI funding or settlement source authority.
use crate::{
    amount::Amount, commands::CommandDisposition, exchange::AssetBundle, policy::Rational,
    source_protocol::valid_digest as valid_hash,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
pub(crate) const OFFER_FAILURES: &[(&str, &str)] = &[
    ("match", "unavailable initial ante window"),
    ("terms", "stale or no longer offered"),
];
/// Empty assets withdraw only the caller's offered stake; original line order is retained.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OfferAnte {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub expected_terms_revision: Amount,
    pub assets: AssetBundle,
}
/// Acceptance names the exact displayed canonical hash, not a client-generated valuation.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptAnte {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub expected_terms_revision: Amount,
    pub terms_hash: String,
}
/// Cancellation releases an offered agreement; committed terms cannot be canceled this way.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelAnte {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub expected_terms_revision: Amount,
}
impl OfferAnte {
    /// # Errors
    /// Rejects absent request/asset identities; bundle decoding enforces quantity/uniqueness bounds.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil()
            || self
                .assets
                .as_slice()
                .iter()
                .any(|line| line.asset_id.is_nil())
        {
            Err("invalid ante offer")
        } else {
            Ok(())
        }
    }
}
impl AcceptAnte {
    /// # Errors
    /// Rejects absent request identity or malformed canonical terms hash.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil() || !valid_hash(&self.terms_hash) {
            Err("invalid ante acceptance")
        } else {
            Ok(())
        }
    }
}
/// Specialized offer acknowledgment preserves the exact accepted revision and opaque hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OfferAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub terms_revision: Amount,
    pub terms_hash: String,
}
/// Early consent is offered; only whole-roster agreement commits or cancels zero stakes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum ConsentState {
    Offered,
    Committed,
    Canceled,
}
impl TryFrom<String> for ConsentState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "offered" => Ok(Self::Offered),
            "committed" => Ok(Self::Committed),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Actual accepted consent state and unchanged terms revision, not a generic resource-only ack.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub state: ConsentState,
    pub terms_revision: Amount,
}
/// A canceled-state acknowledgment has no invented revision field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum CanceledState {
    Canceled,
}
impl TryFrom<String> for CanceledState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Canonical cancellation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub state: CanceledState,
}
/// Only the operation's exact acknowledgment shape is accepted by its contextual validator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnteAck {
    Offer(OfferAck),
    Accept(AcceptAck),
    Cancel(CancelAck),
}
/// Original seat-principal recovery; unknown is distinct from rejected.
pub type AnteDisposition = CommandDisposition<AnteAck>;

/// Closed human operations, never a caller-selected controller seat or private SQL scope.
pub enum AnteIntent {
    Offer(OfferAnte),
    Accept(AcceptAnte),
    Cancel(CancelAnte),
}
impl AnteIntent {
    pub(crate) fn request_id(&self) -> Uuid {
        match self {
            Self::Offer(i) => i.request_id,
            Self::Accept(i) => i.request_id,
            Self::Cancel(i) => i.request_id,
        }
    }
    pub(crate) fn basis(&self) -> &Amount {
        match self {
            Self::Offer(i) => &i.expected_terms_revision,
            Self::Accept(i) => &i.expected_terms_revision,
            Self::Cancel(i) => &i.expected_terms_revision,
        }
    }
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Offer(_) => "offer",
            Self::Accept(_) => "accept",
            Self::Cancel(_) => "cancel",
        }
    }
    pub(crate) fn valid(&self) -> bool {
        !self.request_id().is_nil()
            && match self {
                Self::Offer(i) => i.validate().is_ok(),
                Self::Accept(i) => i.validate().is_ok(),
                Self::Cancel(_) => true,
            }
    }
    pub(crate) fn parameters(
        &self,
        subject: Uuid,
        session: Uuid,
        target: Uuid,
    ) -> serde_json::Value {
        serde_json::json!({"p_subject":subject,"p_session":session,"p_request":self.request_id(),"p_kind":self.kind(),"p_target":target,"p_expected":self.basis(),"p_lines":match self{Self::Offer(i)=>Some(&i.assets),_=>None},"p_hash":match self{Self::Accept(i)=>Some(&i.terms_hash),_=>None}})
    }
    /// Bind the precise operation acknowledgment to its original target/terms revision.
    pub fn valid_ack(&self, target: Uuid, ack: &AnteAck) -> bool {
        match (self, ack) {
            (Self::Offer(i), AnteAck::Offer(a)) => {
                !a.resource_id.is_nil()
                    && valid_hash(&a.terms_hash)
                    && i.expected_terms_revision
                        .checked_add(&1.into())
                        .is_ok_and(|n| a.terms_revision == n)
            }
            (Self::Accept(i), AnteAck::Accept(a)) => {
                a.resource_id == target && a.terms_revision == i.expected_terms_revision
            }
            (Self::Cancel(_), AnteAck::Cancel(a)) => a.resource_id == target,
            _ => false,
        }
    }
    /// Verify exact recovered acknowledgment/rejection without reevaluating current business state.
    pub fn valid_disposition(&self, target: Uuid, disposition: &AnteDisposition) -> bool {
        match disposition {
            CommandDisposition::Accepted { ack } => self.valid_ack(target, ack),
            CommandDisposition::Rejected { errors } => self.terminal_failure(errors),
            _ => true,
        }
    }
    pub(crate) fn terminal_failure(&self, errors: &BTreeMap<String, String>) -> bool {
        errors.len() == 1 && errors.iter().all(|(k, v)| match self {
            Self::Offer(_) => OFFER_FAILURES.contains(&(k.as_str(), v.as_str())),
            Self::Accept(_) => matches!(
                (k.as_str(), v.as_str()),
                (
                    "terms",
                    "unavailable or stale ante agreement" | "ante asset sources changed"
                ) | (
                    "stakes",
                    "a positive stake cannot stand alone"
                        | "restricted outgoing stake exceeds130percent of the largest other stake"
                ) | ("assets", "ante reservation unavailable")
                    | (
                        "allowance",
                        "AI exposure differs from accepted start-day stakes"
                    )
            ),
            Self::Cancel(_) => matches!(
                (k.as_str(), v.as_str()),
                ("terms", "ante cannot be canceled at this basis")
            ),
        })
    }
}
/// Existing commitment lifecycle, including terminal settlement/refund.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum AnteState {
    Offered,
    Committed,
    Settled,
    Refunded,
    Canceled,
}
impl TryFrom<String> for AnteState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "offered" => Ok(Self::Offered),
            "committed" => Ok(Self::Committed),
            "settled" => Ok(Self::Settled),
            "refunded" => Ok(Self::Refunded),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Human owned holds and contingent AI allowance are distinct funding classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum FundingKind {
    OwnedAssets,
    AiLossAllowance,
}
impl TryFrom<String> for FundingKind {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "owned_assets" => Ok(Self::OwnedAssets),
            "ai_loss_allowance" => Ok(Self::AiLossAllowance),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Redacted canonical stake, not a hold/exposure identifier or raw valuation assessment.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayStake {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    pub funding_kind: FundingKind,
    #[serde(deserialize_with = "Option::deserialize")]
    pub ai_coins: Option<Amount>,
    pub entries: AssetBundle,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub value: Option<Rational>,
}
/// Accepted allocation classification; an AI reward never becomes persistent AI inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum AllocationKind {
    Return,
    Transfer,
    Retire,
    AiUnissued,
    AiIssue,
}
impl TryFrom<String> for AllocationKind {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "return" => Ok(Self::Return),
            "transfer" => Ok(Self::Transfer),
            "retire" => Ok(Self::Retire),
            "ai_unissued" => Ok(Self::AiUnissued),
            "ai_issue" => Ok(Self::AiIssue),
            _ => Err("invalid string enum value"),
        }
    }
}

/// The display maps private source stake identity to its original public seat.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplayAllocation {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub source_seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub asset_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub recipient_seat_id: Uuid,
    pub quantity: Amount,
    pub classification: AllocationKind,
}
/// Bounded current terms and accepted allocation display beside the unchanged canonical hash.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnteAgreement {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub state: AnteState,
    pub terms_revision: Amount,
    #[serde(deserialize_with = "Option::deserialize")]
    pub terms_hash: Option<String>,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub accepted_seat_ids: Vec<Uuid>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub accepted_at: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub start_day: Option<String>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub stakes: Vec<DisplayStake>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub settlement_result_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::provider_json::optional_objects")]
    pub allocations: Option<Vec<DisplayAllocation>>,
}
/// No commitment is explicit null, not an invented empty offered agreement.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnteDetail {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub own_seat_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub agreement: Option<AnteAgreement>,
}
impl AnteDetail {
    /// # Errors
    /// Rejects foreign context, malformed bounded terms or leaked/contradictory funding/allocation shape.
    pub fn validate(&self, owner: Uuid, matched: Uuid) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || self.match_id != matched
            || self.own_seat_id.is_nil()
        {
            return Err("invalid ante context");
        }
        let Some(a) = &self.agreement else {
            return Ok(());
        };
        if a.id.is_nil()
            || a.terms_hash.as_deref().is_some_and(|h| !valid_hash(h))
            || a.accepted_seat_ids.len() > 6
            || a.accepted_seat_ids.iter().any(Uuid::is_nil)
            || a.accepted_seat_ids.iter().collect::<BTreeSet<_>>().len()
                != a.accepted_seat_ids.len()
            || a.stakes.len() > 6
            || a.stakes.windows(2).any(|p| p[0].seat_id >= p[1].seat_id)
            || a.start_day
                .as_ref()
                .is_some_and(|s| !crate::progression::valid_day(s))
        {
            return Err("invalid ante agreement");
        }
        for stake in &a.stakes {
            if stake.seat_id.is_nil()
                || stake
                    .entries
                    .as_slice()
                    .windows(2)
                    .any(|p| p[0].asset_id >= p[1].asset_id)
                || stake
                    .value
                    .as_ref()
                    .is_some_and(|v| *v == Rational::whole(0.into()))
            {
                return Err("invalid ante stake");
            }
            match stake.funding_kind {
                FundingKind::OwnedAssets
                    if stake.ai_coins.is_none() && !stake.entries.is_empty() => {}
                FundingKind::AiLossAllowance
                    if stake.entries.is_empty()
                        && stake
                            .ai_coins
                            .as_ref()
                            .is_some_and(|n| !n.is_zero() && n <= &300.into()) => {}
                _ => return Err("invalid stake funding"),
            }
        }
        if a.allocations.is_some() != a.settlement_result_id.is_some()
            || matches!(a.state, AnteState::Settled | AnteState::Refunded)
                != a.allocations.is_some()
        {
            return Err("invalid ante settlement presence");
        }
        if let Some(rows) = &a.allocations {
            if rows.len() > 1200 {
                return Err("oversized accepted allocations");
            }
            for row in rows {
                if row.quantity.is_zero()
                    || !a.stakes.iter().any(|s| {
                        s.seat_id == row.source_seat_id
                            && (s.funding_kind == FundingKind::AiLossAllowance
                                || s.entries
                                    .as_slice()
                                    .iter()
                                    .any(|l| l.asset_id == row.asset_id))
                    })
                {
                    return Err("invalid accepted allocation source");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<ConsentState>(&[
            "offered",
            "committed",
            "canceled",
        ]);
        crate::test_http::string_enum_roundtrip::<CanceledState>(&["canceled"]);
        crate::test_http::string_enum_roundtrip::<AnteState>(&[
            "offered",
            "committed",
            "settled",
            "refunded",
            "canceled",
        ]);
        crate::test_http::string_enum_roundtrip::<FundingKind>(&[
            "owned_assets",
            "ai_loss_allowance",
        ]);
        crate::test_http::string_enum_roundtrip::<AllocationKind>(&[
            "return",
            "transfer",
            "retire",
            "ai_unissued",
            "ai_issue",
        ]);
    }
}
