//! Private normalized recovery/replay contracts. No detector, native restore codec,
//! archive writer, runtime transport or new database authority is installed here.
use crate::{
    ApiResponse,
    amount::Amount,
    source_protocol::{
        SourceProof, SourceResponseError, accepted_response, checked, ids, one_failure,
        record_response,
    },
};
use chrono::{DateTime, Timelike, Utc};
use serde::Deserialize;
use uuid::Uuid;

/// Verified incident category; network disconnects are not server incidents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentKind {
    ServerProcess,
    Storage,
}
impl IncidentKind {
    /// Canonical private SQL parameter, never evidence that an incident occurred.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServerProcess => "server_process",
            Self::Storage => "storage",
        }
    }
}
fn owner(value: &SourceProof) -> bool {
    value.as_str().trim_matches(' ').chars().count() <= 256
}
fn incident_time(value: DateTime<Utc>) -> bool {
    // Freeze an already-normalized finite instant. PostgreSQL checks its own now;
    // no caller-side retry clock or leap-second/submicrosecond rounding is allowed.
    value.nanosecond() < 1_000_000_000 && value.nanosecond().is_multiple_of(1000)
}
pub(crate) fn incident_timestamp<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<DateTime<Utc>, D::Error> {
    let instant = DateTime::<Utc>::deserialize(input)?;
    if !incident_time(instant) {
        return Err(serde::de::Error::custom("invalid incident precision"));
    }
    Ok(instant)
}
fn advanced(value: &Amount, basis: &Amount) -> bool {
    basis
        .checked_add(&Amount::from(1))
        .is_ok_and(|next| &next == value)
}
/// Frozen trusted186 incident source. The kind/proof require an independent detector.
/// Original timestamps and request IDs must survive retry unchanged.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PauseGame {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    pub owner_ref: SourceProof,
    pub epoch: Amount,
    pub operational_revision: Amount,
    #[serde(deserialize_with = "incident_timestamp")]
    pub incident_at: DateTime<Utc>,
    pub kind: IncidentKind,
    pub proof_ref: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
}
/// Trusted186 exact-checkpoint restoration, not a browser reconnect action.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreGame {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    pub epoch: Amount,
    pub operational_revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub checkpoint_id: Uuid,
    pub new_owner_ref: SourceProof,
    pub restoration_ref: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
}
/// Durable checkpoint source and new fence; clockRevision is required nullable
/// because historical source graphs may have no clock rows.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameRecoveryAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub checkpoint_id: Uuid,
    pub epoch: Amount,
    pub operational_revision: Amount,
    #[serde(deserialize_with = "Option::deserialize")]
    pub clock_revision: Option<Amount>,
}
impl GameRecoveryAck {
    fn matches(&self, game: Uuid, epoch: &Amount, revision: &Amount) -> bool {
        self.resource_id == game
            && advanced(&self.epoch, epoch)
            && advanced(&self.operational_revision, revision)
            && self
                .clock_revision
                .as_ref()
                .is_none_or(|value| !value.is_zero())
    }
    /// Bind an independently known retained checkpoint and exact clock frontier.
    /// None means a known empty clock set, not an omitted context check.
    pub fn matches_source(&self, checkpoint: Uuid, clock_revision: Option<&Amount>) -> bool {
        !checkpoint.is_nil()
            && self.checkpoint_id == checkpoint
            && self.clock_revision.as_ref() == clock_revision
    }
}
impl PauseGame {
    /// Decode original-source success or one declared stored rejection.
    /// # Errors
    /// Rejects malformed data, invalid original envelope and mismatched source fences.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<GameRecoveryAck>, SourceResponseError> {
        let response: ApiResponse<GameRecoveryAck> = record_response(bytes)?;
        let valid = ids(&[self.game_id, self.request_id])
            && owner(&self.owner_ref)
            && incident_time(self.incident_at)
            && match &response {
                ApiResponse::Success { data } => {
                    data.matches(self.game_id, &self.epoch, &self.operational_revision)
                }
                ApiResponse::Fail { data } => one_failure(
                    data,
                    &[
                        ("basis", "stale or unavailable"),
                        ("incidentAt", "predates durable clock boundary"),
                    ],
                ),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
impl RestoreGame {
    /// Decode against the original checkpoint, owner and fence, without claiming a codec ran.
    /// # Errors
    /// Rejects malformed/context-mismatched responses and undeclared failures.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<GameRecoveryAck>, SourceResponseError> {
        let response: ApiResponse<GameRecoveryAck> = record_response(bytes)?;
        let valid = ids(&[self.game_id, self.checkpoint_id, self.request_id])
            && owner(&self.new_owner_ref)
            && match &response {
                ApiResponse::Success { data } => {
                    data.matches(self.game_id, &self.epoch, &self.operational_revision)
                        && data.checkpoint_id == self.checkpoint_id
                }
                ApiResponse::Fail { data } => {
                    one_failure(data, &[("basis", "stale or unavailable")])
                }
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
/// Trusted193 match-only incident; no live native game is invented before opening.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PauseMatch {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    pub basis: Amount,
    pub availability_basis: Amount,
    #[serde(deserialize_with = "incident_timestamp")]
    pub incident_at: DateTime<Utc>,
    pub kind: IncidentKind,
    pub proof_ref: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
}
/// Trusted193 exact incident restoration. Timers/bank rebasing remains SQL-owned.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeMatch {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    pub basis: Amount,
    pub availability_basis: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub incident_id: Uuid,
    pub proof_ref: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
}
/// Incident identity is the newly accepted command receipt, not match/request UUID.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatchIncidentAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub incident_id: Uuid,
    pub revision: Amount,
    pub availability_revision: Amount,
}
/// Restored match frontier without private timer/source contents.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatchResumeAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub revision: Amount,
    pub availability_revision: Amount,
}
fn match_frontier(
    resource: Uuid,
    revision: &Amount,
    availability: &Amount,
    original: Uuid,
    basis: &Amount,
    availability_basis: &Amount,
) -> bool {
    resource == original && advanced(revision, basis) && advanced(availability, availability_basis)
}
impl PauseMatch {
    /// Decode against the frozen match and both original generations.
    /// # Errors
    /// Rejects malformed/context-mismatched responses and undeclared failures.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<MatchIncidentAck>, SourceResponseError> {
        let response: ApiResponse<MatchIncidentAck> = record_response(bytes)?;
        let valid = ids(&[self.match_id, self.request_id])
            && incident_time(self.incident_at)
            && match &response {
                ApiResponse::Success { data } => match_frontier(
                    data.resource_id,
                    &data.revision,
                    &data.availability_revision,
                    self.match_id,
                    &self.basis,
                    &self.availability_basis,
                ),
                ApiResponse::Fail { data } => one_failure(
                    data,
                    &[
                        ("basis", "stale or unavailable match phase"),
                        ("clock", "outside inactive phase boundary"),
                    ],
                ),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
impl ResumeMatch {
    /// Decode the original accepted incident's resulting match generations.
    /// # Errors
    /// Rejects malformed/context-mismatched responses and undeclared failures.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<MatchResumeAck>, SourceResponseError> {
        let response: ApiResponse<MatchResumeAck> = record_response(bytes)?;
        let valid = ids(&[self.match_id, self.incident_id, self.request_id])
            && match &response {
                ApiResponse::Success { data } => match_frontier(
                    data.resource_id,
                    &data.revision,
                    &data.availability_revision,
                    self.match_id,
                    &self.basis,
                    &self.availability_basis,
                ),
                ApiResponse::Fail { data } => {
                    one_failure(data, &[("basis", "stale or unavailable match incident")])
                }
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
///185 failed-restore source. incidentId is also request identity, with no replacement UUID.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FinalizeUnrecoverableMatch {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    pub basis: Amount,
    pub availability_basis: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub incident_id: Uuid,
    pub failure_ref: SourceProof,
}
/// The only phase this source can return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum TerminalPhase {
    Terminal,
}
impl TryFrom<String> for TerminalPhase {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "terminal" => Ok(Self::Terminal),
            _ => Err("invalid string enum value"),
        }
    }
}

/// A match result always exists, including no-contest before the first game opens.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnrecoverableMatchAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub terminal_result_id: Uuid,
    pub revision: Amount,
    pub availability_revision: Amount,
    pub phase: TerminalPhase,
}
impl FinalizeUnrecoverableMatch {
    /// Decode success only. SQL stale/contradictory sources raise rather than returning fail.
    /// # Errors
    /// Rejects malformed data, invalid source identity, any non-success or mismatched frontier.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<UnrecoverableMatchAck, SourceResponseError> {
        let data: UnrecoverableMatchAck = accepted_response(bytes)?;
        checked(
            ids(&[self.match_id, self.incident_id])
                && match_frontier(
                    data.resource_id,
                    &data.revision,
                    &data.availability_revision,
                    self.match_id,
                    &self.basis,
                    &self.availability_basis,
                ),
            data,
        )
    }
}
///171 qualified publication source, not actual replay data or a public storage URL.
/// gameId is also its immutable request identity.
pub struct PublishReplay {
    pub game_id: Uuid,
    pub revision: Amount,
    pub initial_checkpoint_id: Uuid,
    pub through_event_sequence: Amount,
    pub artifact_ref: SourceProof,
    pub visibility_index_ref: SourceProof,
    pub schema_revision: SourceProof,
    pub qualification_ref: SourceProof,
}
/// Accepted manifest reference; native replay decoding/visibility remains unqualified.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayPublicationAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    pub through_sequence: Amount,
    pub revision: Amount,
}
impl PublishReplay {
    /// Decode success with exact original terminal revision and retained event frontier.
    /// # Errors
    /// Rejects malformed data, invalid original IDs, any non-success or mismatched frontier.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ReplayPublicationAck, SourceResponseError> {
        let data: ReplayPublicationAck = accepted_response(bytes)?;
        checked(
            ids(&[self.game_id, self.initial_checkpoint_id])
                && data.game_id == self.game_id
                && data.revision == self.revision
                && data.through_sequence == self.through_event_sequence,
            data,
        )
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_variants::<TerminalPhase>(&[("terminal", "Terminal")]);
    }
}
