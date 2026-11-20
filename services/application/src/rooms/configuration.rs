//! Host configuration carries exact original terms, never native qualification.
use crate::{amount::Amount, commands::CommandDisposition};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
/// Match structure remains subject to the canonical qualified profile capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum MatchStructure {
    Bo1,
    Bo3,
}
impl TryFrom<String> for MatchStructure {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "bo1" => Ok(Self::Bo1),
            "bo3" => Ok(Self::Bo3),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Preserve submitted label text. PostgreSQL btrim removes ASCII spaces only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamAssignment {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub slot_id: Uuid,
    #[serde(deserialize_with = "Option::deserialize")]
    pub team_ref: Option<String>,
}
impl TeamAssignment {
    fn valid(&self) -> bool {
        !self.slot_id.is_nil()
            && self.team_ref.as_ref().is_none_or(|text| {
                !text.contains('\0') && (1..=128).contains(&text.trim_matches(' ').chars().count())
            })
    }
}
/// One complete current-slot assignment, preserving original order and labels for replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigureRoom {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub expected_revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub profile_id: Uuid,
    pub structure: MatchStructure,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub teams: Vec<TeamAssignment>,
}
impl ConfigureRoom {
    /// # Errors
    /// Rejects nil identities, duplicate slots and invalid or oversized qualified-profile teams.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil()
            || self.profile_id.is_nil()
            || !valid_teams(&self.teams, false)
            || !serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 32768)
        {
            return Err("invalid room configuration");
        }
        Ok(())
    }
    /// Bind a parsed canonical acknowledgment to the original room and basis.
    pub fn valid_ack(&self, room: Uuid, ack: &ConfigurationAck) -> bool {
        !room.is_nil()
            && ack.resource_id == room
            && (ack.revision == self.expected_revision
                || self
                    .expected_revision
                    .checked_add(&Amount::from(1))
                    .is_ok_and(|next| ack.revision == next))
    }
}
fn valid_teams(teams: &[TeamAssignment], ordered: bool) -> bool {
    teams.len() <= 6
        && teams.iter().all(TeamAssignment::valid)
        && teams
            .iter()
            .map(|t| t.slot_id)
            .collect::<BTreeSet<_>>()
            .len()
            == teams.len()
        && (!ordered
            || teams
                .windows(2)
                .all(|pair| pair[0].slot_id < pair[1].slot_id))
}
pub use crate::commands::RevisionAck as ConfigurationAck;
pub type ConfigurationDisposition = CommandDisposition<ConfigurationAck>;
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ConfigurationResponse {
    Ack(ConfigurationAck),
    Disposition(ConfigurationDisposition),
}
/// Only the original host can read these current team labels.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostConfiguration {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub room_id: Uuid,
    pub revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub profile_id: Uuid,
    pub structure: MatchStructure,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub teams: Vec<TeamAssignment>,
}
impl HostConfiguration {
    /// # Errors
    /// Rejects a foreign owner/room, invalid profile or malformed current team list.
    pub fn validate(&self, owner: Uuid, room: Uuid) -> Result<(), &'static str> {
        if owner.is_nil()
            || room.is_nil()
            || self.account_id != owner
            || self.room_id != room
            || self.profile_id.is_nil()
            || !valid_teams(&self.teams, true)
        {
            return Err("invalid host room configuration");
        }
        Ok(())
    }
}
pub(crate) fn failure(errors: &BTreeMap<String, String>) -> bool {
    errors.len() == 1
        && matches!(
            errors.iter().next().map(|(k, v)| (k.as_str(), v.as_str())),
            Some(
                ("account", "unavailable")
                    | ("room", "unavailable")
                    | ("revision", "conflict")
                    | ("configuration", "unavailable")
                    | (
                        "teams",
                        "complete slot assignment required" | "foreign slot" | "duplicate slot"
                    )
            )
        )
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<MatchStructure>(&["bo1", "bo3"]);
    }
}
