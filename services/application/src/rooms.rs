//! Lobby intents carry no admission proof, controller grant or caller-selected principal.
pub mod configuration;
pub mod invitations;
pub mod reads;
use crate::{amount::Amount, commands::CommandAck, decks::PlayMode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Visibility is independent of gameplay qualification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum RoomVisibility {
    Public,
    Private,
}
impl TryFrom<String> for RoomVisibility {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "public" => Ok(Self::Public),
            "private" => Ok(Self::Private),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Creating a draft room does not admit a deck or start a match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRoom {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub profile_id: Uuid,
    pub mode: PlayMode,
    pub competitive: bool,
    pub visibility: RoomVisibility,
}
/// The required nullable revision explicitly clears a selection when null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectRoomDeck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub room_id: Uuid,
    pub expected_revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub deck_revision_id: Option<Uuid>,
}
/// Leaving releases participation, including during pending deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaveRoom {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub room_id: Uuid,
    pub expected_revision: Amount,
}
/// Each variant maps to one existing canonical transaction and receipt scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    content = "command",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RoomCommand {
    Create(CreateRoom),
    SelectDeck(SelectRoomDeck),
    Leave(LeaveRoom),
}
impl<'de> Deserialize<'de> for RoomCommand {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let wire: crate::provider_json::CommandObject = crate::provider_json::object(input)?;
        match wire.kind.as_str() {
            "create" => wire.payload().map(Self::Create),
            "select_deck" => wire.payload().map(Self::SelectDeck),
            "leave" => wire.payload().map(Self::Leave),
            _ => Err(serde::de::Error::custom("invalid command kind")),
        }
    }
}

impl RoomCommand {
    /// # Errors
    /// Rejects nil identities and competitive Open Play before persistence.
    pub fn validate(&self) -> Result<(), &'static str> {
        let valid = match self {
            Self::Create(c) => {
                !c.request_id.is_nil()
                    && !c.format_id.is_nil()
                    && !c.profile_id.is_nil()
                    && (!c.competitive || c.mode == PlayMode::Collection)
            }
            Self::SelectDeck(c) => {
                !c.request_id.is_nil()
                    && !c.room_id.is_nil()
                    && !c.deck_revision_id.is_some_and(|id| id.is_nil())
            }
            Self::Leave(c) => !c.request_id.is_nil() && !c.room_id.is_nil(),
        };
        if valid {
            Ok(())
        } else {
            Err("invalid room intent")
        }
    }
    pub(crate) fn valid_ack(&self, ack: &CommandAck) -> bool {
        !ack.resource_id.is_nil()
            && match self {
                Self::Create(_) => true,
                Self::SelectDeck(c) => ack.resource_id == c.room_id,
                Self::Leave(c) => ack.resource_id == c.room_id,
            }
    }
    pub(crate) fn failure(&self, errors: &BTreeMap<String, String>) -> bool {
        if errors.len() != 1 {
            return false;
        }
        let value = errors.iter().next().map(|(k, v)| (k.as_str(), v.as_str()));
        matches!(value, Some(("account", "unavailable")))
            || match self {
                Self::Create(_) => matches!(
                    value,
                    Some(
                        ("configuration", "invalid")
                            | ("format", "unavailable")
                            | ("profile", "unavailable")
                    )
                ),
                Self::SelectDeck(_) => matches!(
                    value,
                    Some(
                        ("room", "unavailable")
                            | ("revision", "conflict")
                            | ("deck", "unavailable")
                    )
                ),
                Self::Leave(_) => matches!(
                    value,
                    Some(
                        ("room", "unavailable")
                            | ("revision", "conflict")
                            | ("queue", "unavailable" | "already reserved")
                    )
                ),
            }
    }
    pub(crate) fn rpc(&self, subject: Uuid, session: Uuid, lookup: bool) -> (&'static str, Value) {
        if lookup {
            return (
                "read_room_command",
                json!({"p_subject":subject,"p_session":session,"p_input":self}),
            );
        }
        match self {
            Self::Create(c) => (
                "create_room_command",
                json!({"p_subject":subject,"p_session":session,"p_request":c.request_id,"p_format":c.format_id,"p_profile":c.profile_id,"p_mode":c.mode,"p_competitive":c.competitive,"p_visibility":c.visibility}),
            ),
            Self::SelectDeck(c) => (
                "select_room_deck_command",
                json!({"p_subject":subject,"p_session":session,"p_request":c.request_id,"p_room":c.room_id,"p_expected":c.expected_revision,"p_revision":c.deck_revision_id}),
            ),
            Self::Leave(c) => (
                "leave_room_command",
                json!({"p_subject":subject,"p_session":session,"p_request":c.request_id,"p_room":c.room_id,"p_expected":c.expected_revision}),
            ),
        }
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<RoomVisibility>(&["public", "private"]);
    }
}
