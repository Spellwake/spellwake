//! Human pregame intents/status, never provider credential or readiness proof authority.
use crate::{
    amount::Amount,
    commands::{CommandAck, CommandDisposition, RevisionAck},
    progression::validate_page,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;
/// Shared original revision fence used by add/remove/unready; the target comes from the path.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub expected_revision: Amount,
}
/// Explicit null asks for a draft AI selection; absence is malformed.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootstrapRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub expected_revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub deck_revision_id: Option<Uuid>,
}
/// Explicit null clears the AI slot's assigned deck; absence is malformed.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetAiDeckRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub expected_revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub deck_revision_id: Option<Uuid>,
}
/// A desired-running binding at its original epoch; private start identity is excluded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootstrapAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub epoch: Amount,
}
/// Exact wire shapes are selected and context-validated against the original operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ControlAck {
    Revision(RevisionAck),
    Bootstrap(BootstrapAck),
    Removed(CommandAck),
}
/// Existing command states and required response/null semantics remain unchanged.
pub type ControlDisposition = CommandDisposition<ControlAck>;

/// The original operation, never a caller-selected private function/scope.
#[derive(Debug, Clone, Copy)]
pub enum ControlKind {
    Add,
    Bootstrap,
    Remove,
    Unready,
    SetTier,
    SetDeck,
}
impl ControlKind {
    /// Validate the operation-specific acknowledgment against its original target and basis.
    pub fn valid_ack(self, target: Uuid, basis: &Amount, ack: &ControlAck) -> bool {
        match (self, ack) {
            (Self::Add, ControlAck::Revision(a)) => !a.resource_id.is_nil() && a.revision > *basis,
            (Self::Unready | Self::SetTier, ControlAck::Revision(a)) => {
                a.resource_id == target
                    && (a.revision == *basis
                        || basis
                            .checked_add(&1.into())
                            .is_ok_and(|next| a.revision == next))
            }
            (Self::SetDeck, ControlAck::Revision(a)) => {
                a.resource_id == target && a.revision >= *basis
            }
            (Self::Bootstrap, ControlAck::Bootstrap(a)) => {
                !a.resource_id.is_nil() && a.epoch == 1.into()
            }
            (Self::Remove, ControlAck::Removed(a)) => a.resource_id == target,
            _ => false,
        }
    }
    /// Bind exact recovered acknowledgment/rejection to the original operation and context.
    pub fn valid_disposition(
        self,
        target: Uuid,
        basis: &Amount,
        disposition: &ControlDisposition,
    ) -> bool {
        match disposition {
            CommandDisposition::Accepted { ack } => self.valid_ack(target, basis, ack),
            CommandDisposition::Rejected { errors } => self.failure(errors),
            _ => true,
        }
    }
    pub(crate) fn failure(self, errors: &BTreeMap<String, String>) -> bool {
        errors.len() == 1
            && errors.iter().all(|(k, v)| match self {
                Self::Add => matches!(
                    (k.as_str(), v.as_str()),
                    ("account" | "room", "unavailable")
                        | ("revision", "conflict")
                        | (
                            "queue",
                            "cancel the claimed room before changing its roster"
                        )
                        | ("capacity", "full")
                ),
                Self::Bootstrap => matches!(
                    (k.as_str(), v.as_str()),
                    ("slot", "stale or unavailable") | ("deck", "unavailable")
                ),
                Self::Remove => matches!(
                    (k.as_str(), v.as_str()),
                    ("slot", "stale or unavailable")
                        | (
                            "queue",
                            "cancel the claimed room before changing its roster"
                        )
                ),
                Self::Unready => matches!(
                    (k.as_str(), v.as_str()),
                    ("account" | "room" | "ready", "unavailable") | ("revision", "conflict")
                ),
                Self::SetTier => matches!(
                    (k.as_str(), v.as_str()),
                    ("account", "unavailable")
                        | ("slot", "stale or unavailable")
                        | ("revision", "conflict")
                        | (
                            "queue",
                            "cancel the claimed room before changing its roster"
                        )
                ),
                Self::SetDeck => matches!(
                    (k.as_str(), v.as_str()),
                    ("account", "unavailable")
                        | ("slot", "stale or unavailable")
                        | ("revision", "conflict")
                        | ("deck", "unavailable")
                        | (
                            "queue",
                            "cancel the claimed room before changing its roster"
                        )
                ),
            })
    }
}
/// Lifecycle intent and observation stay separate even for the current binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum DesiredState {
    Running,
    Stopped,
}
impl TryFrom<String> for DesiredState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "running" => Ok(Self::Running),
            "stopped" => Ok(Self::Stopped),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Actual recorded observation; pending/unavailable/unknown do not imply provider success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum ObservedState {
    Pending,
    Running,
    Ready,
    Unavailable,
    Unknown,
    Stopped,
}
impl TryFrom<String> for ObservedState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "ready" => Ok(Self::Ready),
            "unavailable" => Ok(Self::Unavailable),
            "unknown" => Ok(Self::Unknown),
            "stopped" => Ok(Self::Stopped),
            _ => Err("invalid string enum value"),
        }
    }
}

/// The unique desired-running binding, without model/provider/secret references.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingStatus {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub epoch: Amount,
    pub desired_state: DesiredState,
    pub observed_state: ObservedState,
}
/// A retained AI slot's own/host-safe state and pending historical stop work.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiSlotStatus {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub slot_id: Uuid,
    pub seat_index: Amount,
    #[serde(default)]
    pub ai_tier: crate::ai_runtime::AiTier,
    pub is_sponsor: bool,
    pub is_retired: bool,
    pub stop_pending: bool,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub deck_revision_id: Option<Uuid>,
    pub is_ready: bool,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub binding: Option<BindingStatus>,
}
/// Sponsor/host page includes retained retired slots so stop requests can be observed honestly.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiStatusPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub room_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<AiSlotStatus>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl AiStatusPage {
    /// # Errors
    /// Rejects foreign context, exposed foreign deck choices or contradictory runner/Ready state.
    pub fn validate(
        &self,
        owner: Uuid,
        room: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil() || self.account_id != owner || self.room_id != room {
            return Err("invalid AI status context");
        }
        validate_page(
            &self.items.iter().map(|s| s.slot_id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        for slot in &self.items {
            if slot.seat_index > Amount::from(i64::MAX.unsigned_abs())
                || (!slot.is_sponsor && slot.deck_revision_id.is_some())
                || (slot.is_retired && (slot.is_ready || slot.binding.is_some()))
            {
                return Err("invalid AI slot status");
            }
            if let Some(binding) = &slot.binding
                && (binding.desired_state != DesiredState::Running)
            {
                return Err("invalid current AI binding");
            }
            if slot.is_ready
                && !slot
                    .binding
                    .as_ref()
                    .is_some_and(|b| b.observed_state == ObservedState::Ready)
            {
                return Err("invalid authenticated AI readiness");
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
        crate::test_http::string_enum_roundtrip::<DesiredState>(&["running", "stopped"]);
        crate::test_http::string_enum_roundtrip::<ObservedState>(&[
            "pending",
            "running",
            "ready",
            "unavailable",
            "unknown",
            "stopped",
        ]);
    }
}
