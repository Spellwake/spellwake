//! Server-only accepted metadata observation. No public player authority is created.
use super::*;
use crate::{
    ApiResponse,
    amount::Amount,
    backend::{BackendClient, BackendConfig, BackendError},
    match_history::{Availability, ControllerKind, GamePhase, MatchPhase},
    player_wire::NativeRevision,
    provider_json::ObjectResponse,
    source_protocol::{SourceProof, parse_utc_microsecond},
};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
mod access;
mod archive;
mod controller;
mod controller_commands;
pub use archive::NativeArchiveScan;
mod command;
mod cursor;
mod presentation;
mod rpc;
pub use cursor::IssuedNativeCursor;
pub use presentation::{NativeHumanPresentation, NativePresentationOutcome};
mod host_owner;
mod lifecycle;
pub use access::{
    NativeControllerAccess, NativeControllerAccessOutcome, NativeHumanAccess,
    NativeHumanAccessOutcome,
};
pub use command::NativeHumanCommit;
pub use host_owner::NativeHostOwner;
mod cuts;
mod historical;
mod history;
mod history_page;
mod history_replay;
mod replay_archive;
pub(super) use historical::{HistoricalCompletionAuthority, HistoricalCut};
pub use historical::{NativeHistoricalObservation, NativeHumanHistory};
pub use history_replay::NativeHistoryEvidence;
mod opening;
mod originals;
mod prepared_candidate;
mod response_wait;
mod rule_rejection;
pub use history::{NativeActivityFactSource, NativeTransitionSources};
pub use originals::{
    NativeHostInputKind, NativeHostOriginalFields, NativeOpeningOriginalFields, NativeSystemAck,
    NativeSystemDisposition, NativeSystemOriginal,
};
mod view;
mod view_runtime;
pub use lifecycle::RecoveredNativeGame;
pub use opening::{NativeOpeningArtifacts, NativeOpeningSource};
pub use view::{IssuedNativeHumanView, IssuedNativePlayerView};
pub use view_runtime::{
    CommanderWorker, NativeControllerSnapshot, NativeControllerViewOutcome, NativeGameRuntime,
    NativeViewOutcome, ReleaseOpeningWorker, open_commander_game_from_env,
    open_qualified_game_from_env,
};
pub(crate) use view_runtime::{
    NativeHistoryRead, NativeReadOutcome, NativeReadRefusal, NativeReplayRead,
};

/// Safe source-reader failure; original transport/codec causes remain private.
pub struct NativeSourceError(Failure);
enum Failure {
    Invalid,
    Backend(BackendError),
    Initial(NativeInitialError),
    Checkpoint(NativeCheckpointError),
    View(NativeViewError),
    Input(NativeCodecError),
    Artifact(NativeArtifactError),
    Possession(crate::controller_possession::ControllerPossessionError),
    Bridge(NativeBridgeError),
}
impl fmt::Display for NativeSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("accepted native source is unavailable or inconsistent")
    }
}
impl fmt::Debug for NativeSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for NativeSourceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.0 {
            Failure::Invalid => None,
            Failure::Backend(e) => Some(e),
            Failure::Initial(e) => Some(e),
            Failure::Checkpoint(e) => Some(e),
            Failure::View(e) => Some(e),
            Failure::Input(e) => Some(e),
            Failure::Artifact(e) => Some(e),
            Failure::Possession(e) => Some(e),
            Failure::Bridge(e) => Some(e),
        }
    }
}
fn invalid() -> NativeSourceError {
    NativeSourceError(Failure::Invalid)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Checkpoint {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    id: Uuid,
    accepted_revision: Amount,
    state_ref: NativeArtifactRef,
    clock_revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    creator_receipt_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Operational {
    owner_ref: SourceProof,
    fence_epoch: Amount,
    revision: Amount,
    availability: Availability,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    recovery_checkpoint_id: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Seat {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    seat_index: Amount,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    account_id: Option<Uuid>,
    controller_kind: ControllerKind,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    source_revision_id: Uuid,
    deck_snapshot_ref: SourceProof,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Clock {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    policy_ref: SourceProof,
    remaining_ms: Amount,
    clock_revision: Amount,
    paused: bool,
    last_accounted_at: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    release_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    profile_id: Uuid,
    game_state: GamePhase,
    match_state: MatchPhase,
    match_availability: Availability,
    accepted_revision: Amount,
    context_revision: SourceProof,
    initial_ref: NativeArtifactRef,
    #[serde(deserialize_with = "crate::provider_json::object")]
    checkpoint: Checkpoint,
    #[serde(deserialize_with = "crate::provider_json::object")]
    operational: Operational,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    seats: Vec<Seat>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    match_clocks: Vec<Clock>,
    last_event_sequence: Amount,
}
impl Record {
    fn valid(&self, game: Uuid) -> bool {
        let scoped = |r: &NativeArtifactRef, role| {
            r.game_id() == self.game_id && r.context_id() == self.context_id && r.role() == role
        };
        self.version == "native-game-source-v1"
            && self.game_id == game
            && matches!(
                self.game_state,
                GamePhase::Active | GamePhase::Completed | GamePhase::NoContest
            )
            && self.match_state != MatchPhase::Admitting
            && self.accepted_revision == self.checkpoint.accepted_revision
            && self.last_event_sequence >= self.accepted_revision
            && scoped(&self.initial_ref, NativeArtifactRole::Initial)
            && self.initial_ref.size() <= 1_048_576
            && scoped(&self.checkpoint.state_ref, NativeArtifactRole::State)
            && self.checkpoint.state_ref.size() <= 32_768
            && !self.operational.fence_epoch.is_zero()
            && (1..=256).contains(
                &self
                    .operational
                    .owner_ref
                    .as_str()
                    .trim_matches(' ')
                    .chars()
                    .count(),
            )
            && (2..=6).contains(&self.seats.len())
            && self
                .seats
                .iter()
                .all(|seat| seat.seat_index <= Amount::from(9_223_372_036_854_775_807u64))
            && self
                .seats
                .windows(2)
                .all(|pair| pair[0].seat_index < pair[1].seat_index)
            && self
                .seats
                .iter()
                .map(|s| s.seat_id)
                .collect::<BTreeSet<_>>()
                .len()
                == self.seats.len()
            && self.seats.iter().all(|s| {
                matches!(
                    (s.controller_kind, s.account_id),
                    (ControllerKind::Human, Some(_)) | (ControllerKind::Ai, None)
                )
            })
            && self.match_clocks.len() == self.seats.len()
            && self
                .match_clocks
                .windows(2)
                .all(|pair| pair[0].seat_id < pair[1].seat_id)
            && self
                .match_clocks
                .iter()
                .map(|c| c.seat_id)
                .collect::<BTreeSet<_>>()
                == self
                    .seats
                    .iter()
                    .map(|s| s.seat_id)
                    .collect::<BTreeSet<_>>()
            && self.match_clocks.iter().all(|c| {
                c.clock_revision >= self.checkpoint.clock_revision
                    && parse_utc_microsecond(&c.last_accounted_at).is_some()
            })
    }
}
/// Sealed accepted SQL observation, not a retained mutation lease or player capability.
/// Only the trusted repository can construct it. No Debug or serialization exposes refs.
pub struct NativeGameSource {
    record: Record,
}
impl NativeGameSource {
    /// Exact game selected by the server call.
    pub fn game_id(&self) -> Uuid {
        self.record.game_id
    }
    /// Parent match of this retained game, possibly already preparing a later game.
    pub fn match_id(&self) -> Uuid {
        self.record.match_id
    }
    /// Original context, accepted release and profile identities.
    pub fn context_ids(&self) -> (Uuid, Uuid, Uuid) {
        (
            self.record.context_id,
            self.record.release_id,
            self.record.profile_id,
        )
    }
    /// Observed game and parent match phases, not permission to issue commands.
    pub fn phases(&self) -> (GamePhase, MatchPhase) {
        (self.record.game_state, self.record.match_state)
    }
    /// Exact event sequence frontier of this retained game/context.
    pub fn last_event_sequence(&self) -> &Amount {
        &self.record.last_event_sequence
    }
    /// Clock basis stored with the checkpoint, distinct from current match clocks.
    pub fn checkpoint_clock_revision(&self) -> &Amount {
        &self.record.checkpoint.clock_revision
    }
    /// Original seat/controller mapping; AI sponsorship is never human authority.
    pub fn controllers(
        &self,
    ) -> impl ExactSizeIterator<Item = (Uuid, Option<Uuid>, ControllerKind)> {
        self.record
            .seats
            .iter()
            .map(|s| (s.seat_id, s.account_id, s.controller_kind))
    }
    /// Native input revision copied from SQL, still an application bounded amount.
    pub fn accepted_revision(&self) -> &Amount {
        &self.record.accepted_revision
    }
    /// Original accepted engineering context revision; not native epoch.
    pub fn context_revision(&self) -> &str {
        self.record.context_revision.as_str()
    }
    /// Exact current checkpoint and creator identity.
    pub fn checkpoint_ids(&self) -> (Uuid, Uuid) {
        (
            self.record.checkpoint.id,
            self.record.checkpoint.creator_receipt_id,
        )
    }
    /// Current parent match availability, independent of the game's historical phase.
    pub fn match_availability(&self) -> Availability {
        self.record.match_availability
    }
    /// Server fence and revision for a later mutation's independent revalidation.
    pub fn operational_basis(&self) -> (&str, &Amount, &Amount, Availability, Option<Uuid>) {
        let o = &self.record.operational;
        (
            o.owner_ref.as_str(),
            &o.fence_epoch,
            &o.revision,
            o.availability,
            o.recovery_checkpoint_id,
        )
    }
    /// Original copied deck sources in accepted seat order, not current collection.
    pub fn deck_sources(&self) -> impl ExactSizeIterator<Item = (Uuid, Uuid, &str)> {
        self.record.seats.iter().map(|s| {
            (
                s.seat_id,
                s.source_revision_id,
                s.deck_snapshot_ref.as_str(),
            )
        })
    }
    pub(super) fn cast_accounts(
        &self,
    ) -> Result<super::cast_activity::NativeCastAccounts, NativeCheckpointError> {
        let rows = self
            .record
            .seats
            .iter()
            .map(|seat| match (seat.controller_kind, seat.account_id) {
                (ControllerKind::Human, Some(account)) => Ok((seat.seat_id, Some(account))),
                (ControllerKind::Ai, None) => Ok((seat.seat_id, None)),
                (ControllerKind::Human, None) | (ControllerKind::Ai, Some(_)) => {
                    Err(NativeCheckpointError::Invalid)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        super::cast_activity::NativeCastAccounts::new(self.game_id(), self.context_ids().0, rows)
    }
    /// SQL-selected human match seats only; AI sponsors are not participants.
    pub(crate) fn human_seats(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.record
            .seats
            .iter()
            .filter(|seat| seat.controller_kind == ControllerKind::Human)
            .map(|seat| seat.seat_id)
    }
    /// Observe the exact selected clock-policy text for one retained match seat.
    /// Unknown policy text is metadata, not executable clock qualification; this
    /// never infers a policy from remaining balance or paused state.
    pub fn clock_policy(&self, seat: Uuid) -> Option<&str> {
        self.record
            .match_clocks
            .iter()
            .find(|clock| clock.seat_id == seat)
            .map(|clock| clock.policy_ref.as_str())
    }
    /// Current shared match clocks, joined by seat UUID rather than roster position.
    pub fn clocks(&self) -> impl ExactSizeIterator<Item = (Uuid, &Amount, &Amount, bool, &str)> {
        self.record.match_clocks.iter().map(|c| {
            (
                c.seat_id,
                &c.remaining_ms,
                &c.clock_revision,
                c.paused,
                c.last_accounted_at.as_str(),
            )
        })
    }
    /// Bind the database-selected original root and exact ordered accepted roster.
    /// # Errors
    /// Rejects inconsistent original reference/scope or engineering bounds.
    pub fn initial_expected(&self) -> Result<NativeInitialExpected, NativeSourceError> {
        NativeInitialExpected::new(
            self.record.initial_ref.clone(),
            self.record.release_id,
            self.record.profile_id,
            self.record.seats.iter().map(|s| s.seat_id).collect(),
        )
        .map_err(|e| NativeSourceError(Failure::Initial(e)))
    }
    /// Bind the current checkpoint using the actual original opening's native epoch.
    /// This does not load bytes or share an implicit budget with load_initial.
    /// # Errors
    /// Rejects a foreign original root, full context or ordered native roster, and
    /// SQL revisions outside native u64. Operational fence is never substituted.
    pub fn checkpoint_expected(
        &self,
        original: &PreparedNativeOpening,
    ) -> Result<NativeCheckpointExpected, NativeSourceError> {
        let r = &self.record;
        if original.reference() != &r.initial_ref {
            return Err(invalid());
        }
        let first = r.seats.first().ok_or_else(invalid)?.seat_id;
        let binding = original
            .binding(first)
            .map_err(|e| NativeSourceError(Failure::Initial(e)))?;
        let b = &binding.basis;
        if b.game.0 != r.game_id.as_u128()
            || b.revision != 0
            || b.context.release() != r.release_id.to_string()
            || b.context.profile() != r.profile_id.to_string()
            || binding.seats.len() != r.seats.len()
            || binding
                .seats
                .iter()
                .zip(&r.seats)
                .enumerate()
                .any(|(index, (native, seat))| {
                    native.seat_id != seat.seat_id
                        || usize::try_from(native.native_seat.0).ok() != Some(index)
                })
        {
            return Err(invalid());
        }
        let revision =
            NativeRevision::try_from(r.accepted_revision.to_string()).map_err(|_| invalid())?;
        NativeCheckpointExpected::new(
            r.checkpoint.state_ref.clone(),
            PlayerBasis {
                game_id: r.game_id,
                context_id: r.context_id,
                epoch: b.epoch.into(),
                revision,
            },
        )
        .map_err(|e| NativeSourceError(Failure::Checkpoint(e)))
    }
}
/// Trusted private RPC repository. Never put this credential in browser/Tauri clients.
pub struct NativeSourceRepository {
    backend: BackendClient,
}
impl NativeSourceRepository {
    pub(crate) fn new(backend: BackendClient) -> Self {
        Self { backend }
    }
    /// Read existing trusted server configuration.
    /// # Errors
    /// Rejects missing/invalid backend origin/key or transport configuration.
    pub fn from_env() -> Result<Self, NativeSourceError> {
        let config =
            BackendConfig::from_env().map_err(|e| NativeSourceError(Failure::Backend(e)))?;
        let backend =
            BackendClient::new(config, "spellwake-native-source/1", Duration::from_secs(60))
                .map_err(|e| NativeSourceError(Failure::Backend(e)))?;
        Ok(Self::new(backend))
    }
    /// Observe one accepted SQL frontier. None means the exact declared unavailable
    /// result; invalid responses and backend faults remain errors. No bytes are loaded.
    /// # Errors
    /// Rejects nil request IDs before IO, transport/shape/duplicate/correlation
    /// failures, foreign artifact roles/scope, invalid roster/clocks and unknown fail.
    pub fn read(&self, game: Uuid) -> Result<Option<NativeGameSource>, NativeSourceError> {
        self.read_bounded(game, None)
    }
    /// The same accepted source observation with request-budget accounting.
    pub(super) fn read_bounded(
        &self,
        game: Uuid,
        budget: Option<&mut NativeRecoveryBudget>,
    ) -> Result<Option<NativeGameSource>, NativeSourceError> {
        if game.is_nil() {
            return Err(invalid());
        }
        let response: ObjectResponse<Record> =
            self.native_record("read_native_game_source", &json!({"p_game":game}), budget)?;
        match response.0 {
            ApiResponse::Success { data } if data.valid(game) => {
                Ok(Some(NativeGameSource { record: data }))
            }
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("game").is_some_and(|v| v == "unavailable") =>
            {
                Ok(None)
            }
            _ => Err(invalid()),
        }
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod connected;
