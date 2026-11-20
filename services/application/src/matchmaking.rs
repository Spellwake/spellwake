//! User queue consent is separate from trusted matcher roster selection and native admission.
use crate::{
    amount::Amount, commands::CommandDisposition, decks::PlayMode, progression::validate_page,
    rooms::configuration::MatchStructure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
/// Preserve the exact published key and requested AI ceilings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueChoice {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub profile_id: Uuid,
    pub configuration_key: String,
    pub maximum_ai_seats: Amount,
    pub maximum_sponsored_ai_seats: Amount,
}
impl QueueChoice {
    fn valid(&self) -> bool {
        !self.profile_id.is_nil()
            && valid_key(&self.configuration_key)
            && self.maximum_ai_seats <= Amount::from(5)
            && self.maximum_sponsored_ai_seats <= self.maximum_ai_seats
    }
}
fn valid_key(key: &str) -> bool {
    !key.contains('\0') && (1..=128).contains(&key.trim_matches(' ').chars().count())
}
/// One owned sealed deck can opt into several distinct compatible profile configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueAlternative {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub deck_revision_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub choices: Vec<QueueChoice>,
}
fn valid_content(mode: PlayMode, competitive: bool, alternatives: &[QueueAlternative]) -> bool {
    (!competitive || mode == PlayMode::Collection)
        && (1..=32).contains(&alternatives.len())
        && alternatives
            .iter()
            .map(|a| a.deck_revision_id)
            .collect::<BTreeSet<_>>()
            .len()
            == alternatives.len()
        && alternatives.iter().all(|a| {
            !a.deck_revision_id.is_nil()
                && (1..=128).contains(&a.choices.len())
                && a.choices
                    .iter()
                    .map(|c| (c.profile_id, c.configuration_key.as_str()))
                    .collect::<BTreeSet<_>>()
                    .len()
                    == a.choices.len()
                && a.choices
                    .iter()
                    .all(|c| c.valid() && (!competitive || c.maximum_ai_seats.is_zero()))
        })
}
/// Original whole intent, with no matcher-selected roster, source proof or caller principal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnqueueMatchmaking {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub mode: PlayMode,
    pub competitive: bool,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub alternatives: Vec<QueueAlternative>,
}
impl EnqueueMatchmaking {
    /// # Errors
    /// Rejects malformed consent, duplicate alternatives/choices or transport overflow.
    /// PostgreSQL additionally measures its canonical256KiB command envelope.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil()
            || !valid_content(self.mode, self.competitive, &self.alternatives)
            || !serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 264192)
        {
            return Err("invalid queue intent");
        }
        Ok(())
    }
}
/// Cancel the complete original intent through any of its owned entry identities.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelMatchmaking {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
}
/// Both entry and intent are needed for status grouping and recovery.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueAck {
    pub resource_id: Uuid,
    pub intent_id: Uuid,
}
impl<'de> Deserialize<'de> for EnqueueAck {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            intent_id: Uuid,
        }
        let value: Wire = crate::provider_json::object(input)?;
        Ok(Self {
            resource_id: value.resource_id,
            intent_id: value.intent_id,
        })
    }
}
impl EnqueueAck {
    /// Check the two canonical source identities without guessing a current queue state.
    pub fn valid(&self) -> bool {
        !self.resource_id.is_nil() && !self.intent_id.is_nil()
    }
}
pub type EnqueueDisposition = CommandDisposition<EnqueueAck>;
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum EnqueueResponse {
    Ack(EnqueueAck),
    Disposition(EnqueueDisposition),
}
/// Safe owner status excludes claim receipts and normalized qualification metadata.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueEntry {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub intent_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub deck_revision_id: Uuid,
    pub mode: PlayMode,
    pub competitive: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub alternative_index: Option<Amount>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub format_id: Option<Uuid>,
    pub state: String,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub claimed_room_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub reserved_match_id: Option<Uuid>,
}
impl QueueEntry {
    fn valid(&self, intent: Option<Uuid>) -> bool {
        let typed = match (self.intent_id, &self.alternative_index, self.format_id) {
            (None, None, None) => true,
            (Some(id), Some(index), Some(format)) => {
                !id.is_nil() && !format.is_nil() && index <= &Amount::from(31)
            }
            _ => false,
        };
        let lifecycle = match self.state.as_str() {
            "queued" => self.claimed_room_id.is_none() && self.reserved_match_id.is_none(),
            "claimed" => self.claimed_room_id.is_some() && self.reserved_match_id.is_none(),
            "reserved" => self.claimed_room_id.is_some() && self.reserved_match_id.is_some(),
            "canceled" => self.reserved_match_id.is_none(),
            _ => false,
        };
        typed
            && lifecycle
            && !self.deck_revision_id.is_nil()
            && (!self.competitive || self.mode == PlayMode::Collection)
            && intent.is_none_or(|id| self.intent_id == Some(id))
            && !self.claimed_room_id.is_some_and(|id| id.is_nil())
            && !self.reserved_match_id.is_some_and(|id| id.is_nil())
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueuePage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<QueueEntry>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl QueuePage {
    /// # Errors
    /// Rejects a foreign caller/filter, malformed lifecycle tuple or invalid requested page.
    pub fn validate(
        &self,
        owner: Uuid,
        intent: Option<Uuid>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || intent.is_some_and(|id| id.is_nil())
            || self.items.iter().any(|i| !i.valid(intent))
        {
            return Err("invalid queue page");
        }
        validate_page(
            &self.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )
    }
}
/// An accepted original intent, reconstructed with an explicit nested public-field allowlist.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueIntent {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub intent_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub mode: PlayMode,
    pub competitive: bool,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub alternatives: Vec<QueueAlternative>,
}
impl QueueIntent {
    /// # Errors
    /// Rejects foreign owner/target or any malformed original consent; current status is separate.
    pub fn validate(&self, owner: Uuid, intent: Uuid) -> Result<(), &'static str> {
        if owner.is_nil()
            || intent.is_nil()
            || self.account_id != owner
            || self.intent_id != intent
            || self.request_id.is_nil()
            || !valid_content(self.mode, self.competitive, &self.alternatives)
        {
            return Err("invalid original queue intent");
        }
        Ok(())
    }
}
/// Published SQL profile metadata is not a claim of native engine coverage.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableProfile {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub key: String,
    pub minimum_seats: Amount,
    pub seat_capacity: Amount,
    pub has_matchmaking: bool,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableProfilePage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<TableProfile>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl TableProfilePage {
    /// # Errors
    /// Rejects owner/page mismatches or capacities outside the published profile source domain.
    pub fn validate(
        &self,
        owner: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || self.items.iter().any(|p| {
                p.minimum_seats < Amount::from(2)
                    || p.minimum_seats > p.seat_capacity
                    || p.seat_capacity > Amount::from(6)
            })
        {
            return Err("invalid table profile page");
        }
        validate_page(
            &self.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )
    }
}
/// Public configuration descriptors omit the private team layout and source qualification reference.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileConfiguration {
    pub index: Amount,
    pub key: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    pub seat_count: Amount,
    pub structure: MatchStructure,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileConfigurationPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub profile_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<ProfileConfiguration>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next_index: Option<Amount>,
}
impl ProfileConfigurationPage {
    /// # Errors
    /// Rejects foreign owner/profile, invalid ordinals/keys/layout bounds and cursor mismatches.
    pub fn validate(
        &self,
        owner: Uuid,
        profile: Uuid,
        after: Option<u16>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || profile.is_nil()
            || self.account_id != owner
            || self.profile_id != profile
            || after.is_some_and(|i| i > 511)
            || !(1..=100).contains(&limit)
            || self.items.len() > limit
            || self.items.iter().any(|c| {
                c.index > Amount::from(511)
                    || after.is_some_and(|a| c.index <= Amount::from(u64::from(a)))
                    || !valid_key(&c.key)
                    || c.format_id.is_nil()
                    || c.seat_count < Amount::from(2)
                    || c.seat_count > Amount::from(6)
                    || (c.structure == MatchStructure::Bo3 && c.seat_count != Amount::from(2))
            })
            || (after.is_none() && self.items.is_empty())
            || (after.map_or(0, |i| u64::from(i) + 1)..)
                .zip(&self.items)
                .any(|(expected, item)| item.index != Amount::from(expected))
            || self.next_index
                != if self.items.len() == limit {
                    self.items.last().map(|i| i.index.clone())
                } else {
                    None
                }
        {
            return Err("invalid profile configuration page");
        }
        Ok(())
    }
}
pub(crate) fn enqueue_failure(errors: &BTreeMap<String, String>) -> bool {
    errors.len() == 1
        && matches!(
            errors.iter().next().map(|(k, v)| (k.as_str(), v.as_str())),
            Some(
                ("account", "unavailable")
                    | ("intent", "invalid envelope")
                    | ("alternatives", "distinct bounded decks required")
                    | ("alternative", "invalid shape")
                    | ("choices", "distinct bounded configurations required")
                    | (
                        "choice",
                        "invalid shape" | "unsupported format, profile or AI allowance"
                    )
                    | ("deck", "owned sealed revision required")
            )
        )
}
pub(crate) fn cancel_failure(errors: &BTreeMap<String, String>) -> bool {
    errors.len() == 1
        && matches!(
            errors.iter().next().map(|(k, v)| (k.as_str(), v.as_str())),
            Some(("account", "unavailable") | ("queue", "unavailable" | "already reserved"))
        )
}
