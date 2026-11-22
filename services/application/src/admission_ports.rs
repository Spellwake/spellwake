//! Closed trusted matcher, admission and native lifecycle contracts against the
//! canonical SQL sources. No matcher, validator, engine codec, host or runtime
//! grant is installed by these types. Proof text alone never qualifies a game.
use crate::{
    ApiResponse,
    amount::Amount,
    commands::CommandAck,
    source_protocol::{
        SourceProof, SourceResponseError, accepted_response, checked, ids, one_failure,
        record_response,
    },
};
use serde::{Deserialize, Deserializer, de};
use uuid::Uuid;
mod event_schema;
mod opening_clock;
mod payload;
pub use event_schema::NativeEventSchema;
pub(crate) use event_schema::present as present_event_schema;
pub use opening_clock::OpeningClockObservation;
pub(crate) use opening_clock::present as present_initial_clock;
pub use payload::{
    AdmissionInputs, HostCandidate, OpeningArtifacts, QueueSelection, RoomStartInputs,
};

#[cfg(test)]
mod tests;

fn advanced(revision: &Amount, basis: &Amount) -> bool {
    basis
        .checked_add(&Amount::from(1))
        .is_ok_and(|expected| expected == *revision)
}
fn owner_valid(owner: &SourceProof) -> bool {
    owner.as_str().trim_matches(' ').chars().count() <= 256
}
const MATCHER_FAILURES: &[(&str, &str)] = &[
    ("configuration", "format or competitive mismatch"),
    ("configuration", "unsupported"),
    ("deck", "source changed"),
    ("preferences", "selection outside an accepted intent"),
    ("queue", "intent unavailable"),
    ("selection", "duplicate, missing or foreign seat"),
    ("selection", "one typed intent per human required"),
];
pub(crate) const READY_FAILURES: &[(&str, &str)] = &[
    ("account", "unavailable"),
    ("deck", "qualified selected revision required"),
    ("ready", "unavailable"),
    ("revision", "conflict"),
    ("room", "unavailable"),
];
pub(crate) const COUNTDOWN_FAILURES: &[(&str, &str)] = &[
    ("account", "unavailable"),
    ("decks", "qualification required"),
    ("revision", "conflict"),
    ("room", "invalid slots or selected decks"),
    ("room", "unavailable"),
];
const ADMISSION_FAILURES: &[(&str, &str)] = &[
    ("account", "unavailable"),
    ("cards", "unsupported or not legal in accepted revision"),
    ("collection", "not enough available copies"),
    ("deck", "unavailable or incompatible"),
    ("match", "stale, accepted or started"),
    ("profile", "qualified roster and complete inputs required"),
    ("release", "unavailable"),
    ("seat", "duplicate input"),
    ("seat", "unavailable"),
];
const START_FAILURES: &[(&str, &str)] = &[
    ("room", "stale, not due or already reserved"),
    ("account", "unavailable"),
    ("ai", "authenticated current-epoch readiness required"),
    ("deck", "selected revision changed"),
    ("inputs", "complete occupied roster required"),
];
const CANDIDATE_FAILURES: &[(&str, &str)] = &[
    ("availability", "match recovering"),
    ("basis", "stale or terminal"),
    ("clocks", "BO1 overall bank is inactive"),
    ("clocks", "complete match clock state required"),
    ("clocks", "duplicate seat"),
    ("clocks", "overall bank requires its timeout transition"),
    (
        "clocks",
        "send the current bank; elapsed debit is authoritative",
    ),
    ("clocks", "stale or invalid"),
    ("context", "unavailable"),
    ("fence", "stale or unavailable"),
    ("game", "unavailable"),
];
///148 queue-matcher/queue_claim_v1: original selection and supplied request UUID.
pub struct MatchmakingClaim {
    pub request_id: Uuid,
    pub selection: QueueSelection,
}
impl MatchmakingClaim {
    /// Decode a generated room identity or the exact stored claim rejection.
    /// # Errors
    /// Rejects invalid request identity, malformed output or unknown disposition.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<CommandAck>, SourceResponseError> {
        let response: ApiResponse<CommandAck> = record_response(bytes)?;
        let valid = !self.request_id.is_nil()
            && match &response {
                ApiResponse::Success { data } => !data.resource_id.is_nil(),
                ApiResponse::Fail { data } => one_failure(data, MATCHER_FAILURES),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
///145 human:<actor>/room_ready: trusted qualification of the selected revision.
/// The existing session-derived unready route remains a separate human boundary.
pub struct QualifiedReady {
    pub actor_id: Uuid,
    pub room_id: Uuid,
    pub basis: Amount,
    pub ready: bool,
    pub selected_revision_id: Option<Uuid>,
    pub qualification_ref: Option<SourceProof>,
    pub request_id: Uuid,
}
/// Ready/countdown source revision can stay equal or advance one.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomRevisionAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub revision: Amount,
}
impl QualifiedReady {
    /// Validate the acknowledgment against original readiness terms, not current room state.
    /// # Errors
    /// Rejects invalid qualified identity, malformed output or room/revision/failure drift.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<RoomRevisionAck>, SourceResponseError> {
        let response: ApiResponse<RoomRevisionAck> = record_response(bytes)?;
        let valid = ids(&[self.actor_id, self.room_id, self.request_id])
            && self.selected_revision_id.is_none_or(|id| !id.is_nil())
            && (!self.ready
                || self.selected_revision_id.is_some() && self.qualification_ref.is_some())
            && match &response {
                ApiResponse::Success { data } => {
                    data.resource_id == self.room_id
                        && (data.revision == self.basis || advanced(&data.revision, &self.basis))
                }
                ApiResponse::Fail { data } => one_failure(data, READY_FAILURES),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
///115 human:<actor>/room_countdown: qualified host intent, not a client proof.
pub struct QualifiedCountdown {
    pub actor_id: Uuid,
    pub room_id: Uuid,
    pub basis: Amount,
    pub qualification_ref: SourceProof,
    pub request_id: Uuid,
}
impl QualifiedCountdown {
    /// Decode original equal/advanced room revision and exact countdown rejection.
    /// # Errors
    /// Rejects invalid identities, malformed records and mismatched room/revision/failures.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<RoomRevisionAck>, SourceResponseError> {
        let response: ApiResponse<RoomRevisionAck> = record_response(bytes)?;
        let valid = ids(&[self.actor_id, self.room_id, self.request_id])
            && match &response {
                ApiResponse::Success { data } => {
                    data.resource_id == self.room_id
                        && (data.revision == self.basis || advanced(&data.revision, &self.basis))
                }
                ApiResponse::Fail { data } => one_failure(data, COUNTDOWN_FAILURES),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
///113 admission:<match>/match_decks_v1. Fingerprint includes original basis,
/// release, qualification and seat inputs; accepted decks are immutable match snapshots.
pub struct AcceptMatchDecks {
    pub match_id: Uuid,
    pub basis: Amount,
    pub release_id: Uuid,
    pub qualification_ref: SourceProof,
    pub inputs: AdmissionInputs,
    pub request_id: Uuid,
}
/// Accepted immutable deck pool with exact release/qualification acknowledgment.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub release_id: Uuid,
    pub qualification_ref: SourceProof,
}
impl AcceptMatchDecks {
    /// Decode original admission receipt; subsequent transfers do not revalidate this pool.
    /// # Errors
    /// Rejects identity/release/qualification/revision drift and malformed stored failures.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<AdmissionAck>, SourceResponseError> {
        let response: ApiResponse<AdmissionAck> = record_response(bytes)?;
        let valid = ids(&[self.match_id, self.release_id, self.request_id])
            && match &response {
                ApiResponse::Success { data } => {
                    data.resource_id == self.match_id
                        && advanced(&data.revision, &self.basis)
                        && data.release_id == self.release_id
                        && data.qualification_ref.as_str() == self.qualification_ref.as_str()
                }
                ApiResponse::Fail { data } => one_failure(data, ADMISSION_FAILURES),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
///185 room-start:<room>/room_start_v1. SQL creates match seats from these original
/// slot inputs; the transformed child113 array is not the outer fingerprint.
pub struct StartRoomMatch {
    pub room_id: Uuid,
    pub basis: Amount,
    pub release_id: Uuid,
    pub legality_revision_id: Uuid,
    pub qualification_ref: SourceProof,
    pub inputs: RoomStartInputs,
    pub request_id: Uuid,
}
/// Newly admitted match and reserved game, not an operating native game.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoomStartAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
}
impl StartRoomMatch {
    /// Decode generated reservation identities or the handled admission rejection.
    /// # Errors
    /// Rejects malformed/noncanonical output, invalid request IDs or unknown failure shapes.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<RoomStartAck>, SourceResponseError> {
        let response: ApiResponse<RoomStartAck> = record_response(bytes)?;
        let valid = ids(&[
            self.room_id,
            self.release_id,
            self.legality_revision_id,
            self.request_id,
        ]) && match &response {
            ApiResponse::Success { .. } => true,
            ApiResponse::Fail { data } => {
                one_failure(data, START_FAILURES) || one_failure(data, ADMISSION_FAILURES)
            }
            ApiResponse::Error { .. } => false,
        };
        checked(valid, response)
    }
}
///189 match-opening:<match>/game_opening_v1; request/target are the game UUID.
/// The fingerprint deliberately excludes expected_revision.
pub struct AcceptReservedGame {
    pub match_id: Uuid,
    pub expected_revision: Amount,
    pub game_id: Uuid,
    pub owner: SourceProof,
    pub opening: OpeningArtifacts,
    /// Original accepted basis when independently retained; never a fresh retry basis.
    pub original_accepted_basis: Option<Amount>,
}
/// Game/context identity and historical match revision after opening.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpeningAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub context_id: Uuid,
    pub revision: Amount,
}
impl AcceptReservedGame {
    /// Decode successful opening or original replay; source exceptions are not stored fail data.
    /// # Errors
    /// Rejects malformed or mismatched acknowledgment and invalid original source identity.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<OpeningAck, SourceResponseError> {
        let ack: OpeningAck = accepted_response(bytes)?;
        let valid = ids(&[self.match_id, self.game_id])
            && owner_valid(&self.owner)
            && ack.resource_id == self.game_id
            && ack.context_id == self.opening.context_id()
            && !ack.revision.is_zero()
            && self
                .original_accepted_basis
                .as_ref()
                .is_none_or(|basis| advanced(&ack.revision, basis));
        checked(valid, ack)
    }
}
///187 match-lifecycle:<match>/game_outcome_v1, request/target immutable result UUID.
/// Only result identity enters the source hash; expected_revision is a retry basis.
pub struct ConsumeGameOutcome {
    pub result_id: Uuid,
    pub expected_revision: Amount,
    /// Independently known result/handoff parent, not an arbitrary decoded response ID.
    pub match_id: Uuid,
    pub original_accepted_basis: Option<Amount>,
}
/// Lifecycle phase emitted by187, separate from persisted match-state enums.
#[derive(Deserialize, PartialEq, Eq)]
#[serde(try_from = "String")]
pub enum OutcomePhase {
    Terminal,
    BetweenGames,
}
impl TryFrom<String> for OutcomePhase {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "terminal" => Ok(Self::Terminal),
            "between_games" => Ok(Self::BetweenGames),
            _ => Err("invalid outcome phase"),
        }
    }
}
fn encoded_ids<'de, D: Deserializer<'de>>(input: D) -> Result<Vec<Uuid>, D::Error> {
    #[derive(Deserialize)]
    struct Ids(#[serde(deserialize_with = "crate::identifiers::uuid_list")] Vec<Uuid>);
    let text = String::deserialize(input)?;
    serde_json::from_str::<Ids>(&text)
        .map(|x| x.0)
        .map_err(de::Error::custom)
}
/// Historical lifecycle result. Empty-string UUID and string-encoded list fields
/// preserve the existing source protocol rather than rewriting stored receipts.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameOutcomeAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub revision: Amount,
    pub phase: OutcomePhase,
    #[serde(deserialize_with = "crate::identifiers::empty_uuid")]
    pub terminal_result_id: Option<Uuid>,
    #[serde(deserialize_with = "encoded_ids")]
    pub pending_decision_ids: Vec<Uuid>,
}
impl ConsumeGameOutcome {
    /// Decode original decisions/result even after the current frontier has advanced.
    /// # Errors
    /// Rejects invalid trusted parent, malformed encoded IDs or phase/revision mismatch.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<GameOutcomeAck, SourceResponseError> {
        let ack: GameOutcomeAck = accepted_response(bytes)?;
        let valid = ids(&[self.match_id, self.result_id])
            && ack.resource_id == self.match_id
            && !ack.revision.is_zero()
            && self
                .original_accepted_basis
                .as_ref()
                .is_none_or(|basis| advanced(&ack.revision, basis))
            && ack
                .pending_decision_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && match ack.phase {
                OutcomePhase::Terminal => {
                    ack.terminal_result_id.is_some() && ack.pending_decision_ids.is_empty()
                }
                OutcomePhase::BetweenGames => {
                    ack.terminal_result_id.is_none() && ack.pending_decision_ids.len() == 2
                }
            };
        checked(valid, ack)
    }
}
///190 host:<game>/host_candidate_v1. Original owner/epoch/basis/frame determine
/// the fingerprint; the supplied request UUID never authorizes a native move.
pub struct AcceptHostCandidate {
    pub game_id: Uuid,
    pub owner: SourceProof,
    pub epoch: Amount,
    pub basis: Amount,
    pub request_id: Uuid,
    pub candidate: HostCandidate,
}
/// Private accepted checkpoint and game revision, never a seat-safe player view.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub revision: Amount,
}
impl AcceptHostCandidate {
    /// Decode a successful checkpoint or exact historical candidate rejection.
    /// # Errors
    /// Rejects malformed source/frame basis, revision or unknown failure vocabulary.
    pub fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<ApiResponse<CandidateAck>, SourceResponseError> {
        let response: ApiResponse<CandidateAck> = record_response(bytes)?;
        let valid = ids(&[self.game_id, self.request_id])
            && owner_valid(&self.owner)
            && match &response {
                ApiResponse::Success { data } => {
                    self.candidate.valid_for(&self.basis) && advanced(&data.revision, &self.basis)
                }
                ApiResponse::Fail { data } => one_failure(data, CANDIDATE_FAILURES),
                ApiResponse::Error { .. } => false,
            };
        checked(valid, response)
    }
}
