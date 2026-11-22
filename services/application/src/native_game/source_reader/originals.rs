//! Exact retained system originals and read-only acknowledgment reconciliation.
use super::*;
use crate::{
    admission_ports::{CandidateAck, OpeningAck},
    source_protocol::valid_digest,
};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

/// Existing private SQL original kind; this does not implement a native operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum NativeHostInputKind {
    /// Actual retained entropy input; the maintained producer uses this kind.
    Entropy,
    /// Host clock input. The input alone never grants deadline authority.
    Clock,
    /// Historical SQL work original, not an admitted native executor by itself.
    NativeWork,
}
impl TryFrom<String> for NativeHostInputKind {
    type Error = crate::source_protocol::SourceResponseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "entropy" => Ok(Self::Entropy),
            "clock" => Ok(Self::Clock),
            "native-work" => Ok(Self::NativeWork),
            _ => Err(crate::source_protocol::SourceResponseError::context()),
        }
    }
}
// Opaque RawValue fields must be decoded directly from JSON rather than
// through Serde's internally tagged enum buffer. The discriminator is only a
// first pass; the selected complete owner rejects all unknown/duplicate keys
// before original_source_terms can collect JSONB accounting values.
#[derive(Deserialize)]
struct Version {
    version: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpeningShape {
    #[serde(rename = "version")]
    _version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    owner: SourceProof,
    original_basis: Amount,
    opening: Box<RawValue>,
    native_epoch: NativeRevision,
    runtime_id: String,
    program_source_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostShape {
    #[serde(rename = "version")]
    _version: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    basis: PlayerBasis,
    kind: NativeHostInputKind,
    input_ref: NativeArtifactRef,
    runtime_id: String,
    program_source_id: String,
}
enum RawOriginal {
    Opening(OpeningShape),
    Host(HostShape),
}
fn parse_shape(bytes: &[u8]) -> Result<(RawOriginal, Box<RawValue>), NativeSourceError> {
    if bytes.len() > 1_048_576 {
        return Err(invalid());
    }
    let version: Version = crate::decode_wire_object(bytes).map_err(decode_error)?;
    let error = |error| {
        NativeSourceError(Failure::Backend(BackendError::caused(
            "native original",
            error,
        )))
    };
    match version.version.as_str() {
        "native-opening-v1" => crate::source_protocol::original_source_terms::<
            crate::provider_json::Object<OpeningShape>,
        >(bytes, 1_048_576)
        .map(|(crate::provider_json::Object(value), raw)| (RawOriginal::Opening(value), raw))
        .map_err(error),
        "native-host-input-v1" => crate::source_protocol::original_source_terms::<
            crate::provider_json::Object<HostShape>,
        >(bytes, 1_048_576)
        .map(|(crate::provider_json::Object(value), raw)| (RawOriginal::Host(value), raw))
        .map_err(error),
        _ => Err(invalid()),
    }
}

enum Terms {
    Opening {
        matched: Uuid,
        owner: SourceProof,
        basis: Amount,
        epoch: NativeRevision,
        artifacts: Box<NativeOpeningArtifacts>,
    },
    Host {
        basis: PlayerBasis,
        kind: NativeHostInputKind,
        input: NativeArtifactRef,
    },
}
/// Borrowed closed opening coordinates. Parsing alone grants no admission.
pub struct NativeOpeningOriginalFields<'a> {
    pub match_id: Uuid,
    pub owner: &'a str,
    pub original_basis: &'a Amount,
    pub native_epoch: NativeRevision,
    pub artifacts: &'a NativeOpeningArtifacts,
}
/// Borrowed original host input coordinates, not a current lease or execution proof.
pub struct NativeHostOriginalFields<'a> {
    pub basis: &'a PlayerBasis,
    pub kind: NativeHostInputKind,
    pub input_ref: &'a NativeArtifactRef,
}
/// Exact private original retained before its first committing RPC.
/// A first attempt derives truthful artifact refs before constructing these terms;
/// a retry observes these saved terms before loading/executing native code.
/// No Debug or public player serialization exposes its private coordinates.
pub struct NativeSystemOriginal {
    request: Uuid,
    game: Uuid,
    runtime: String,
    programs: String,
    terms: Terms,
    raw: Box<RawValue>,
}
pub(super) fn decode_error(error: serde_json::Error) -> NativeSourceError {
    NativeSourceError(Failure::Backend(BackendError::caused(
        "native original",
        error,
    )))
}
impl NativeSystemOriginal {
    pub(super) fn matches_original(&self, other: &Self) -> Result<bool, NativeSourceError> {
        let left: serde_json::Value = serde_json::from_str(self.raw.get()).map_err(decode_error)?;
        let right: serde_json::Value =
            serde_json::from_str(other.raw.get()).map_err(decode_error)?;
        Ok(self.request == other.request && self.game == other.game && left == right)
    }
    pub(super) fn matches_opening_command(
        &self,
        command: &crate::admission_ports::AcceptReservedGame,
    ) -> Result<bool, NativeSourceError> {
        let Some(fields) = self.opening_fields() else {
            return Ok(false);
        };
        let original: serde_json::Value =
            serde_json::from_str(self.raw.get()).map_err(decode_error)?;
        let opening: serde_json::Value =
            serde_json::from_str(command.opening.original_json()).map_err(decode_error)?;
        Ok(self.game == command.game_id
            && fields.match_id == command.match_id
            && fields.owner == command.owner.as_str()
            && fields.original_basis == &command.expected_revision
            && command
                .original_accepted_basis
                .as_ref()
                .is_none_or(|basis| basis == fields.original_basis)
            && original.get("opening") == Some(&opening))
    }
    /// Parse an independently retained complete original without loading native data.
    /// # Errors
    /// Rejects invalid/duplicate/positional/unknown terms, private ref scope,
    /// bounds, nil request or opening request unequal to its game. No acceptance
    /// or artifact retention is inferred from successful parsing.
    pub fn from_bytes(request: Uuid, bytes: &[u8]) -> Result<Self, NativeSourceError> {
        if request.is_nil() {
            return Err(invalid());
        }
        let (value, raw) = parse_shape(bytes)?;
        let (game, runtime, programs, terms) = match value {
            RawOriginal::Opening(OpeningShape {
                match_id,
                game_id,
                owner,
                original_basis,
                opening,
                native_epoch,
                runtime_id,
                program_source_id,
                ..
            }) => {
                // Generic original byte/JSONB limits and native scoped roles have
                // different owners; both established contracts must hold.
                crate::admission_ports::OpeningArtifacts::from_bytes(opening.get().as_bytes())
                    .map_err(|error| {
                        NativeSourceError(Failure::Backend(BackendError::caused(
                            "native opening original",
                            error,
                        )))
                    })?;
                let artifacts: NativeOpeningArtifacts =
                    crate::decode_wire_object(opening.get().as_bytes()).map_err(decode_error)?;
                let context = artifacts.state().context_id();
                if game_id != request
                    || owner.as_str().trim_matches(' ').chars().count() > 256
                    || !artifacts.valid(game_id, context)
                {
                    return Err(invalid());
                }
                (
                    game_id,
                    runtime_id,
                    program_source_id,
                    Terms::Opening {
                        matched: match_id,
                        owner,
                        basis: original_basis,
                        epoch: native_epoch,
                        artifacts: Box::new(artifacts),
                    },
                )
            }
            RawOriginal::Host(HostShape {
                basis,
                kind,
                input_ref,
                runtime_id,
                program_source_id,
                ..
            }) => {
                if basis.game_id.is_nil()
                    || basis.context_id.is_nil()
                    || input_ref.game_id() != basis.game_id
                    || input_ref.context_id() != basis.context_id
                    || input_ref.role() != NativeArtifactRole::Input
                {
                    return Err(invalid());
                }
                (
                    basis.game_id,
                    runtime_id,
                    program_source_id,
                    Terms::Host {
                        basis,
                        kind,
                        input: input_ref,
                    },
                )
            }
        };
        if !valid_digest(&runtime) || !valid_digest(&programs) {
            return Err(invalid());
        }
        Ok(Self {
            request,
            game,
            runtime,
            programs,
            terms,
            raw,
        })
    }
    /// Original request identity, never a replacement retry ID.
    pub fn request_id(&self) -> Uuid {
        self.request
    }
    /// Exact original game namespace.
    pub fn game_id(&self) -> Uuid {
        self.game
    }
    /// Original runtime hash; it is not compared with today's runtime on lookup.
    pub fn runtime_id(&self) -> &str {
        &self.runtime
    }
    /// Original program package hash; lookup does not load or replace the package.
    pub fn program_source_id(&self) -> &str {
        &self.programs
    }
    /// Exact original JSON for private command-owner persistence only.
    pub fn original_json(&self) -> &str {
        self.raw.get()
    }
    /// Opening coordinates when this is the retained opening original.
    pub fn opening_fields(&self) -> Option<NativeOpeningOriginalFields<'_>> {
        match &self.terms {
            Terms::Opening {
                matched,
                owner,
                basis,
                epoch,
                artifacts,
            } => Some(NativeOpeningOriginalFields {
                match_id: *matched,
                owner: owner.as_str(),
                original_basis: basis,
                native_epoch: *epoch,
                artifacts,
            }),
            Terms::Host { .. } => None,
        }
    }
    /// Host input coordinates when this is a retained host original.
    pub fn host_fields(&self) -> Option<NativeHostOriginalFields<'_>> {
        match &self.terms {
            Terms::Host { basis, kind, input } => Some(NativeHostOriginalFields {
                basis,
                kind: *kind,
                input_ref: input,
            }),
            Terms::Opening { .. } => None,
        }
    }
    /// Freeze first-attempt opening terms after real preparation, before committing.
    /// # Errors
    /// Rejects inconsistent command/checkpoint scope, noninitial checkpoint or
    /// invalid original grammar. Actual Event/Results/clock/admission comparison
    /// and private retention are still required before fresh acceptance.
    pub fn prepare_opening(
        command: &crate::admission_ports::AcceptReservedGame,
        checkpoint: &NativeCheckpoint,
        programs: &NativeProgramSource,
    ) -> Result<Self, NativeSourceError> {
        if checkpoint.retained_input().is_some()
            || checkpoint.basis().revision.get() != 0
            || command.game_id != checkpoint.basis().game_id
            || command.opening.context_id() != checkpoint.basis().context_id
        {
            return Err(invalid());
        }
        let opening: &RawValue =
            serde_json::from_str(command.opening.original_json()).map_err(decode_error)?;
        let value = json!({"version":"native-opening-v1","matchId":command.match_id,"gameId":command.game_id,"owner":command.owner.as_str(),"originalBasis":command.expected_revision,"opening":opening,"nativeEpoch":checkpoint.basis().epoch,"runtimeId":native_runtime_identity(),"programSourceId":programs.identity().hex()});
        Self::from_bytes(
            command.game_id,
            &serde_json::to_vec(&value).map_err(decode_error)?,
        )
    }
    /// Freeze the maintained actual entropy proposal's unchanged original identity.
    /// # Errors
    /// Rejects nil request, foreign/gapped/nonentropy proposal or invalid original.
    /// This performs no source observation, retention or acceptance itself.
    pub fn prepare_entropy(
        request: Uuid,
        source: &RecoveredNativeGame,
        next: &NativeCheckpoint,
    ) -> Result<Self, NativeSourceError> {
        Self::prepare_host(request, source, next, NativeCheckpointInput::Entropy)
    }

    pub(super) fn prepare_response_timeout(
        request: Uuid,
        source: &RecoveredNativeGame,
        next: &NativeCheckpoint,
    ) -> Result<Self, NativeSourceError> {
        if source.checkpoint().opening_response_seat()?.is_none() {
            return Err(invalid());
        }
        Self::prepare_host(
            request,
            source,
            next,
            NativeCheckpointInput::ResponseTimeout,
        )
    }

    fn prepare_host(
        request: Uuid,
        source: &RecoveredNativeGame,
        next: &NativeCheckpoint,
        expected: NativeCheckpointInput,
    ) -> Result<Self, NativeSourceError> {
        let prior = source.checkpoint();
        let (kind, parent, input) = next.retained_input().ok_or_else(invalid)?;
        let wire = match (kind, expected) {
            (NativeCheckpointInput::Entropy, NativeCheckpointInput::Entropy) => "entropy",
            (NativeCheckpointInput::ResponseTimeout, NativeCheckpointInput::ResponseTimeout) => {
                "clock"
            }
            _ => return Err(invalid()),
        };
        if parent != prior.reference()
            || next.initial_reference() != prior.initial_reference()
            || next.basis().game_id != prior.basis().game_id
            || next.basis().context_id != prior.basis().context_id
            || next.basis().epoch != prior.basis().epoch
            || prior.basis().revision.get().checked_add(1) != Some(next.basis().revision.get())
        {
            return Err(invalid());
        }
        let value = json!({"version":"native-host-input-v1","basis":prior.basis(),"kind":wire,"inputRef":input,"runtimeId":source.opening().runtime_id(),"programSourceId":source.opening().program_source_id()});
        Self::from_bytes(request, &serde_json::to_vec(&value).map_err(decode_error)?)
    }
}

/// Safe typed acknowledgment from the existing original's exact stored response.
pub enum NativeSystemAck {
    /// Opening game/context and historical match revision.
    Opening(OpeningAck),
    /// Accepted host checkpoint and native successor revision.
    Host(CandidateAck),
}
/// Read-only original reconciliation result, never a newly committed operation.
pub enum NativeSystemDisposition {
    /// No committed original was observed; a concurrent commit may still finish.
    Unknown,
    /// Exact terminal success from the original creator.
    Accepted(NativeSystemAck),
    /// Exact stored native rejection with validated safe native failure vocabulary.
    Rejected(BTreeMap<String, String>),
    /// Request identity already belongs to different original terms.
    Conflict,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDispositionEnvelope {
    status: String,
    data: Box<RawValue>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDisposition {
    state: String,
    #[serde(deserialize_with = "Option::deserialize")]
    response: Option<Box<RawValue>>,
}
fn native_rejection(errors: &BTreeMap<String, String>) -> bool {
    (1..=11).contains(&errors.len())
        && errors.iter().all(|(key, value)| {
            matches!(
                key.as_str(),
                "request"
                    | "game"
                    | "basis"
                    | "actor"
                    | "source"
                    | "ability"
                    | "target"
                    | "decision"
                    | "response"
                    | "payment"
                    | "action"
            ) && matches!(
                value.as_str(),
                "invalid" | "stale" | "unavailable" | "not permitted" | "unsupported"
            )
        })
}
impl NativeSourceRepository {
    /// Observe an exact saved system original before native work on retries.
    /// # Errors
    /// Rejects backend failure, malformed/extra/duplicate state/response, fabricated
    /// ack scope/revision or unsafe failure fields. No native source/runtime,
    /// current lease, admission or Storage call is made. Pending remains retryable.
    pub fn read_system_disposition(
        &self,
        original: &NativeSystemOriginal,
    ) -> Result<NativeSystemDisposition, NativeSourceError> {
        let raw:Box<RawValue>=self.backend.rpc("read_native_system_disposition",&json!({"p_game":original.game,"p_request":original.request,"p_original":&original.raw})).map_err(|error|NativeSourceError(Failure::Backend(error)))?;
        let envelope: RawDispositionEnvelope =
            crate::decode_wire_object(raw.get().as_bytes()).map_err(decode_error)?;
        if envelope.status == "fail" {
            let response: ObjectResponse<CandidateAck> =
                crate::decode_wire_object(raw.get().as_bytes()).map_err(decode_error)?;
            return match response.0 {
                ApiResponse::Fail { data }
                    if data.len() == 1 && data.get("request").is_some_and(|v| v == "conflict") =>
                {
                    Ok(NativeSystemDisposition::Conflict)
                }
                _ => Err(invalid()),
            };
        }
        if envelope.status != "success" {
            return Err(invalid());
        }
        let data: RawDisposition =
            crate::decode_wire_object(envelope.data.get().as_bytes()).map_err(decode_error)?;
        match (data.state.as_str(), data.response) {
            ("unknown", None) => Ok(NativeSystemDisposition::Unknown),
            ("accepted", Some(raw)) => {
                let ack = match &original.terms {
                    Terms::Opening {
                        basis, artifacts, ..
                    } => {
                        let response: ObjectResponse<OpeningAck> =
                            crate::decode_wire_object(raw.get().as_bytes())
                                .map_err(decode_error)?;
                        let ApiResponse::Success { data } = response.0 else {
                            return Err(invalid());
                        };
                        if data.resource_id != original.game
                            || data.context_id != artifacts.state().context_id()
                            || basis.checked_add(&Amount::from(1)).ok().as_ref()
                                != Some(&data.revision)
                        {
                            return Err(invalid());
                        }
                        NativeSystemAck::Opening(data)
                    }
                    Terms::Host { basis, .. } => {
                        let response: ObjectResponse<CandidateAck> =
                            crate::decode_wire_object(raw.get().as_bytes())
                                .map_err(decode_error)?;
                        let ApiResponse::Success { data } = response.0 else {
                            return Err(invalid());
                        };
                        if basis
                            .revision
                            .get()
                            .checked_add(1)
                            .map(Amount::from)
                            .as_ref()
                            != Some(&data.revision)
                        {
                            return Err(invalid());
                        }
                        NativeSystemAck::Host(data)
                    }
                };
                Ok(NativeSystemDisposition::Accepted(ack))
            }
            ("rejected", Some(raw)) => {
                let response: ObjectResponse<CandidateAck> =
                    crate::decode_wire_object(raw.get().as_bytes()).map_err(decode_error)?;
                match response.0 {
                    ApiResponse::Fail { data } if native_rejection(&data) => {
                        Ok(NativeSystemDisposition::Rejected(data))
                    }
                    _ => Err(invalid()),
                }
            }
            _ => Err(invalid()),
        }
    }
}

#[cfg(test)]
mod tests;
