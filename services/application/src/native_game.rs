//! Trusted in-process player-to-native proposal bridge, not an authenticated host.
//!
//! Callers must establish admission, immutable source truth and viewer authority.
//! No state/candidate is installed or published here. Safe projection and private
//! input records are provided; source admission, durable acceptance and HTTP
//! delivery remain separate dependencies. Input records are not saved-state bytes.
use crate::player_wire::{PlayerBasis, PlayerCommand, PlayerHandle};
use spellwake_engine::{
    effects::EffectError,
    game::{GameBasis, GameError, GameInput, GameState, NativeCandidate},
    priority::SeatId,
};
use std::{collections::BTreeSet, error::Error, fmt, sync::Arc};
use uuid::Uuid;
mod artifacts;
mod cast_activity;
pub(crate) use cast_activity::NativeCastFact;
mod cast_batch;
pub use cast_batch::{NativeCastBatchError, PreparedNativeCastBatch};
mod checkpoint;
mod clocks;
mod events;
mod evidence;
pub(crate) use clocks::NativeBankTimeout;
pub use clocks::{NativeBankExpiryIntent, NativeClockIntent};
pub use events::PreparedNativeEvent;
mod handles;
mod initial_source;
pub(crate) use initial_source::AdmissionManifest;
mod input_codec;
pub mod preparation_source;
mod projection;
mod results;
pub use results::{NativeTerminal, PreparedNativeResults};
mod source_reader;
pub(crate) use source_reader::{
    NativeHistoryRead, NativeReadOutcome, NativeReadRefusal, NativeReplayRead,
};
mod translate;
pub use artifacts::{
    MAX_NATIVE_ARTIFACT_BYTES, NativeArtifact, NativeArtifactError, NativeArtifactFailure,
    NativeArtifactRef, NativeArtifactRole, NativeArtifactStore,
};
pub use checkpoint::{
    NativeCheckpoint, NativeCheckpointError, NativeCheckpointExpected, NativeCheckpointInput,
    NativeRecoveryBudget, load_checkpoint,
};
pub use handles::NativeReference;
pub use initial_source::{
    NativeImmutableDisplay, NativeInitialCard, NativeInitialError, NativeInitialExpected,
    NativeInitialIds, NativeInitialParticipant, NativeInitialProvenance, NativeInitialSelection,
    NativeInitialSpec, NativeInitialZone, NativePhysicalCardSource, NativePrintedCard,
    NativeProgramPool, NativeProgramSource, PreparedNativeOpening, inspect_constructed_room,
    inspect_release_candidate, load_initial, native_runtime_identity, prepare_initial,
    prepare_selected_initial, restore_initial,
};
pub use input_codec::{
    NativeCodecError, PreparedNativeBankExpiry, PreparedNativeClockExpiry, PreparedNativeEntropy,
    restore_bank_expiry, restore_entropy, restore_input, restore_response_timeout,
};
pub use input_codec::{
    NativeViewError, NativeViewExpected, PreparedNativeView, RestoredNativeView, load_view,
    prepare_view, restore_view,
};
pub use projection::{
    NativeAbilityReference, NativeDisclosure, NativeDisplayScope, NativeDisplaySource,
    NativeObjectDisplay, NativeProjectedSnapshot, NativeProjectionError, NativeProjectionMetadata,
};
pub use source_reader::{
    IssuedNativeCursor, IssuedNativeHumanView, IssuedNativePlayerView, NativeActivityFactSource,
    NativeArchiveScan, NativeControllerAccess, NativeControllerAccessOutcome, NativeGameRuntime,
    NativeGameSource, NativeHistoricalObservation, NativeHistoryEvidence, NativeHostInputKind,
    NativeHostOriginalFields, NativeHostOwner, NativeHumanAccess, NativeHumanAccessOutcome,
    NativeHumanCommit, NativeHumanHistory, NativeHumanPresentation, NativeMatchWorker,
    NativeOpeningArtifacts, NativeOpeningOriginalFields, NativeOpeningResponseWait,
    NativeOpeningSource, NativePresentationOutcome, NativeResponseJobOutcome, NativeSourceError,
    NativeSourceRepository, NativeSystemAck, NativeSystemDisposition, NativeSystemOriginal,
    NativeTransitionSources, NativeViewOutcome, RecoveredNativeGame, run_owned_response_job,
};

/// One accepted application seat mapped to its actual local native participant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NativeSeatBinding {
    pub seat_id: Uuid,
    pub native_seat: SeatId,
}

/// Source-bound metadata supplied by a trusted admission/checkpoint loader.
/// Construction validates consistency; it does not establish Auth/DB provenance.
#[derive(Clone)]
pub struct NativeBinding {
    basis: GameBasis,
    public_basis: PlayerBasis,
    seats: Vec<NativeSeatBinding>,
    viewer: Uuid,
    actor: SeatId,
}
impl NativeBinding {
    /// Bind a context UUID, full native basis, roster and verified viewer.
    /// Native GameId is the exact u128 representation of the application UUID.
    /// # Errors
    /// Rejects nil identities, duplicate/empty/oversized rosters and foreign viewers.
    pub fn new(
        context_id: Uuid,
        basis: GameBasis,
        seats: Vec<NativeSeatBinding>,
        viewer: Uuid,
    ) -> Result<Self, NativeBridgeError> {
        let game_id = Uuid::from_u128(basis.game.0);
        if game_id.is_nil()
            || context_id.is_nil()
            || viewer.is_nil()
            || !(1..=6).contains(&seats.len())
            || seats.iter().any(|seat| seat.seat_id.is_nil())
            || seats
                .iter()
                .map(|seat| seat.seat_id)
                .collect::<BTreeSet<_>>()
                .len()
                != seats.len()
            || seats
                .iter()
                .map(|seat| seat.native_seat)
                .collect::<BTreeSet<_>>()
                .len()
                != seats.len()
        {
            return Err(NativeBridgeError::InvalidBinding);
        }
        let actor = seats
            .iter()
            .find(|seat| seat.seat_id == viewer)
            .map(|seat| seat.native_seat)
            .ok_or(NativeBridgeError::InvalidBinding)?;
        let public_basis = PlayerBasis {
            game_id,
            context_id,
            epoch: basis.epoch.into(),
            revision: basis.revision.into(),
        };
        Ok(Self {
            basis,
            public_basis,
            seats,
            viewer,
            actor,
        })
    }
    fn validate_source(&self, source: &GameState) -> Result<(), NativeBridgeError> {
        if *source.basis() != self.basis
            || source
                .original_participants()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                != self
                    .seats
                    .iter()
                    .map(|seat| seat.native_seat)
                    .collect::<BTreeSet<_>>()
        {
            return Err(NativeBridgeError::InvalidBinding);
        }
        Ok(())
    }
    /// Exact player basis derived from the accepted native source, not a fence epoch.
    pub fn public_basis(&self) -> &PlayerBasis {
        &self.public_basis
    }
}

/// One immutable source/viewer scope. Native state is never serialized here.
/// Private input records retain only referenced public handles and typed locations.
/// The projector owns visible/interactable issuance; raw registration presence
/// checks alone are not a player-visibility or authentication proof.
pub struct NativeScope<'a> {
    source: &'a GameState,
    binding: NativeBinding,
    references: Vec<(PlayerHandle, NativeReference)>,
    // Enabled only while verifying a restored View's exact projection registry.
    used_handles: Option<BTreeSet<PlayerHandle>>,
    // Allocation identity binds proposals to this exact immutable borrowed scope.
    // It never crosses a serialization boundary or represents game authority.
    identity: Arc<()>,
}
impl<'a> NativeScope<'a> {
    /// Borrow an accepted source with matching complete context/basis/participant set.
    /// # Errors
    /// Rejects any source-basis substitution or roster inconsistent with the kernel.
    pub fn new(source: &'a GameState, binding: NativeBinding) -> Result<Self, NativeBridgeError> {
        binding.validate_source(source)?;
        Ok(Self {
            source,
            binding,
            references: Vec::new(),
            used_handles: None,
            identity: Arc::new(()),
        })
    }
    /// Current source-derived public basis; this does not certify a format or source.
    pub fn basis(&self) -> &PlayerBasis {
        &self.binding.public_basis
    }
    /// Verified application seat supplied by the caller, never selected by a command.
    pub fn viewer_seat_id(&self) -> Uuid {
        self.binding.viewer
    }
    /// Resolve and propose an actual native transition, leaving the source unchanged.
    /// The complete original intent and translated native input are retained.
    /// # Errors
    /// Rejects malformed/stale intent, unknown or wrong-kind handles, unsupported
    /// constructors, foreign decisions/targets, and typed native rejections.
    pub fn propose(
        &self,
        original: &PlayerCommand,
    ) -> Result<PreparedNativeInput, NativeBridgeError> {
        original
            .validate()
            .map_err(|_| NativeBridgeError::InvalidIntent)?;
        if original.basis != self.binding.public_basis {
            return Err(NativeBridgeError::StaleBasis);
        }
        let input = GameInput {
            basis: self.source.basis().clone(),
            actor: self.binding.actor,
            command: self.translate_action(&original.action)?,
        };
        // Native advance consumes input. Retain this exact allocation/order for
        // the future artifact/acceptance owner; journals can normalize payments.
        let candidate = self
            .source
            .advance(input.clone())
            .map_err(|cause| NativeBridgeError::Native(Box::new(cause)))?;
        Ok(PreparedNativeInput {
            identity: Arc::clone(&self.identity),
            original: original.clone(),
            viewer: self.binding.viewer,
            input,
            candidate,
        })
    }
    fn seat(&self, seat_id: Uuid) -> Result<SeatId, NativeBridgeError> {
        self.binding
            .seats
            .iter()
            .find(|seat| seat.seat_id == seat_id)
            .map(|seat| seat.native_seat)
            .ok_or(NativeBridgeError::InvalidReference)
    }
}

/// Private proposal only. The future host must persist/fence/accept it before
/// consuming the candidate state or exposing any player view or receipt.
pub struct PreparedNativeInput {
    identity: Arc<()>,
    original: PlayerCommand,
    viewer: Uuid,
    input: GameInput,
    candidate: NativeCandidate,
}
impl PreparedNativeInput {
    /// Exact original public intent, including its action and all ordered fields.
    pub fn original(&self) -> &PlayerCommand {
        &self.original
    }
    /// Original verified application viewer seat.
    pub fn viewer_seat_id(&self) -> Uuid {
        self.viewer
    }
    /// Exact translated private native input; never a player response payload.
    pub fn native_input(&self) -> &GameInput {
        &self.input
    }
    /// Actual private candidate; reading it is not durable acceptance or disclosure.
    pub fn candidate(&self) -> &NativeCandidate {
        &self.candidate
    }
    /// Transfer the proposal to its trusted persistence/acceptance owner.
    /// This performs no install, SQL transaction, publication or acknowledgment.
    pub fn into_parts(self) -> (PlayerCommand, Uuid, GameInput, NativeCandidate) {
        (self.original, self.viewer, self.input, self.candidate)
    }
}

/// Safe bridge failure. Native diagnostics are retained as typed causes, not messages.
#[derive(Clone, PartialEq, Eq)]
pub enum NativeBridgeError {
    InvalidBinding,
    InvalidIntent,
    StaleBasis,
    InvalidReference,
    WrongReferenceKind,
    WrongDecision,
    Unsupported,
    Native(Box<GameError>),
    Observation(Box<EffectError>),
}
impl NativeBridgeError {
    /// Inspect the native rejection internally without exposing its Debug output.
    pub fn native_error(&self) -> Option<&GameError> {
        match self {
            Self::Native(cause) => Some(cause),
            _ => None,
        }
    }
    /// Inspect a native characteristic-read failure internally.
    pub fn observation_error(&self) -> Option<&EffectError> {
        match self {
            Self::Observation(cause) => Some(cause),
            _ => None,
        }
    }
}
impl fmt::Display for NativeBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidBinding => "native source binding is unavailable",
            Self::InvalidIntent => "native input is invalid",
            Self::StaleBasis => "native input basis is stale",
            Self::InvalidReference => "issued reference is unavailable",
            Self::WrongReferenceKind => "issued reference kind does not match",
            Self::WrongDecision => "native decision is unavailable",
            Self::Unsupported => "native input shape is unsupported",
            Self::Native(_) => "native input was rejected",
            Self::Observation(_) => "native observation is unavailable",
        })
    }
}
impl fmt::Debug for NativeBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
// Native GameError/EffectError do not implement std::error::Error. Preserve their
// typed values through the accessors above rather than inventing a string cause.
impl Error for NativeBridgeError {}

#[cfg(test)]
mod tests;
