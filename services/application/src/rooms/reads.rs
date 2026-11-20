//! One-statement room snapshots; UUID slot cursors do not freeze later pages.
use super::RoomVisibility;
use crate::{amount::Amount, decks::PlayMode, progression::validate_page};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Safe room metadata excludes queue receipts, invitations and native artifacts.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub host_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub profile_id: Uuid,
    pub mode: PlayMode,
    pub competitive: bool,
    pub visibility: RoomVisibility,
    pub state: String,
    pub revision: Amount,
    pub structure: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub configuration_id: Uuid,
    #[serde(deserialize_with = "Option::deserialize")]
    pub target_seat_count: Option<Amount>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub countdown_at: Option<DateTime<Utc>>,
}
impl RoomSummary {
    fn validate(&self) -> Result<(), &'static str> {
        if [
            self.id,
            self.host_id,
            self.format_id,
            self.profile_id,
            self.configuration_id,
        ]
        .iter()
        .any(Uuid::is_nil)
            || (self.competitive && self.mode != PlayMode::Collection)
            || !matches!(self.structure.as_str(), "bo1" | "bo3")
            || self
                .target_seat_count
                .as_ref()
                .is_some_and(|n| n < &Amount::from(2) || n > &Amount::from(6))
        {
            return Err("invalid room summary");
        }
        Ok(())
    }
}
/// Caller may inspect only their own selected human deck, including in a public lobby.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomSlot {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub account_id: Option<Uuid>,
    pub seat_index: Amount,
    pub controller_kind: String,
    pub ready: bool,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub deck_revision_id: Option<Uuid>,
}
/// A bounded page contains current slot identities, not retired history.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomSlotPage {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<RoomSlot>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl RoomSlotPage {
    fn validate(&self, owner: Uuid, after: Option<Uuid>, limit: usize) -> Result<(), &'static str> {
        validate_page(
            &self.items.iter().map(|s| s.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        let mut indices = std::collections::BTreeSet::new();
        let mut accounts = std::collections::BTreeSet::new();
        for slot in &self.items {
            if !indices.insert(slot.seat_index.to_string())
                || slot.account_id.is_some_and(|id| !accounts.insert(id))
                || slot.seat_index > Amount::from(i64::MAX.unsigned_abs())
                || slot.account_id.is_some_and(|id| id.is_nil())
                || slot.deck_revision_id.is_some_and(|id| id.is_nil())
                || !matches!(slot.controller_kind.as_str(), "human" | "ai")
                || (slot.controller_kind == "ai" && slot.account_id.is_some())
                || (slot.deck_revision_id.is_some()
                    && (slot.controller_kind != "human" || slot.account_id != Some(owner)))
            {
                return Err("invalid or private room slot");
            }
        }
        Ok(())
    }
}
/// Original bearer determines both visibility and this request owner.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<RoomSummary>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl RoomPage {
    /// # Errors
    /// Rejects a foreign owner, invalid metadata or a response outside the requested page.
    pub fn validate(
        &self,
        owner: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil() || self.account_id != owner {
            return Err("room page owner mismatch");
        }
        validate_page(
            &self.items.iter().map(|r| r.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        for room in &self.items {
            room.validate()?;
        }
        Ok(())
    }
}
/// Room metadata and one current-slot page share one SQL statement snapshot.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomDetail {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub room: RoomSummary,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub slots: RoomSlotPage,
}
impl RoomDetail {
    /// # Errors
    /// Rejects foreign caller/target, malformed metadata, private selection or invalid pagination.
    pub fn validate(
        &self,
        owner: Uuid,
        room: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil() || self.account_id != owner || self.room.id != room {
            return Err("room detail target mismatch");
        }
        self.room.validate()?;
        self.slots.validate(owner, after, limit)
    }
}
