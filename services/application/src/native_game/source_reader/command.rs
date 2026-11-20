//! Typed original player acceptance and read-only historical receipt recovery.
use super::*;
use crate::{
    actor::RequestActor,
    admission_ports::HostCandidate,
    commands::CommandDisposition,
    player_wire::{PlayerCommand, PlayerCommandAck, PlayerReceipt, PlayerReceiptQuery},
};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Ack {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    actor_seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    epoch: NativeRevision,
    revision: NativeRevision,
}
impl Ack {
    fn public(
        self,
        original: &PlayerCommand,
        viewer: Uuid,
    ) -> Result<PlayerCommandAck, NativeSourceError> {
        let ack = PlayerCommandAck {
            request_id: self.request_id,
            actor_seat_id: self.actor_seat_id,
            basis: PlayerBasis {
                game_id: self.game_id,
                context_id: self.context_id,
                epoch: self.epoch,
                revision: self.revision,
            },
        };
        ack.validate(original, viewer).map_err(|_| invalid())?;
        Ok(ack)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    actor_seat_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    original: PlayerCommand,
    state: String,
    #[serde(deserialize_with = "Option::deserialize")]
    response: Option<ObjectResponse<Ack>>,
}
impl Receipt {
    fn public(self) -> Result<PlayerReceipt, NativeSourceError> {
        let disposition = match (self.state.as_str(), self.response) {
            ("unknown", None) => CommandDisposition::Unknown,
            ("accepted", Some(ObjectResponse(ApiResponse::Success { data }))) => {
                CommandDisposition::Accepted {
                    ack: data.public(&self.original, self.actor_seat_id)?,
                }
            }
            ("rejected", Some(ObjectResponse(ApiResponse::Fail { data }))) => {
                CommandDisposition::Rejected { errors: data }
            }
            _ => return Err(invalid()),
        };
        let receipt = PlayerReceipt {
            actor_seat_id: self.actor_seat_id,
            original: self.original,
            disposition,
        };
        receipt
            .validate(&receipt.original, receipt.actor_seat_id)
            .map_err(|_| invalid())?;
        Ok(receipt)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Batch {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    receipts: Vec<Receipt>,
}
#[derive(Clone, Copy)]
pub(super) enum PlayerAuthorityKind {
    Human,
    Controller,
}
fn gate_failure(
    errors: &BTreeMap<String, String>,
    submission: bool,
    kind: PlayerAuthorityKind,
) -> bool {
    errors.len() == 1
        && errors.iter().all(|(key, value)| {
            matches!(
                (key.as_str(), value.as_str()),
                ("game", "unavailable") | ("request", "invalid" | "conflict")
            ) || matches!(kind, PlayerAuthorityKind::Human)
                && key == "session"
                && value == "unauthorized"
                || (submission || matches!(kind, PlayerAuthorityKind::Controller))
                    && key == "actor"
                    && value == "unavailable"
        })
}
pub(super) fn receipt_query_raw(
    query: &PlayerReceiptQuery,
) -> Result<Box<serde_json::value::RawValue>, NativeSourceError> {
    query.validate().map_err(|_| invalid())?;
    let raw = serde_json::value::RawValue::from_string(
        serde_json::to_string(query).map_err(|e| backend_error("query", e))?,
    )
    .map_err(|e| backend_error("query", e))?;
    if !crate::source_protocol::original_source_octets(&raw).is_some_and(|size| size <= 1_048_576) {
        return Err(invalid());
    }
    Ok(raw)
}
fn backend_error(label: &'static str, error: serde_json::Error) -> NativeSourceError {
    NativeSourceError(Failure::Backend(BackendError::caused(label, error)))
}

/// Extract one already validated exact original, preserving authority-specific denials.
pub(super) fn decode_original_receipt(
    response: ApiResponse<Vec<PlayerReceipt>>,
) -> Result<Option<ApiResponse<PlayerCommandAck>>, NativeSourceError> {
    match response {
        ApiResponse::Success { mut data } => {
            if data.len() != 1 {
                return Err(invalid());
            }
            match data.pop().ok_or_else(invalid)?.disposition {
                CommandDisposition::Accepted { ack } => {
                    Ok(Some(ApiResponse::Success { data: ack }))
                }
                CommandDisposition::Rejected { errors } => {
                    Ok(Some(ApiResponse::Fail { data: errors }))
                }
                CommandDisposition::Unknown => Ok(None),
                CommandDisposition::Pending => Err(invalid()),
            }
        }
        ApiResponse::Fail { data } => Ok(Some(ApiResponse::Fail { data })),
        ApiResponse::Error { .. } => Err(invalid()),
    }
}

/// Distinct actor checks over one maintained native preparation/receipt owner.
#[derive(Clone, Copy)]
pub(super) enum NativePlayerAuthority<'a> {
    Human(&'a RequestActor, &'a NativeHumanAccess),
    Controller(
        &'a crate::controller_possession::ControllerSecretStore,
        &'a crate::controller_possession::PossessedController,
        &'a NativeControllerAccess,
    ),
}
impl NativePlayerAuthority<'_> {
    pub(super) fn viewer(&self) -> Uuid {
        match self {
            Self::Human(_, a) => a.viewer_seat_id(),
            Self::Controller(_, _, a) => a.viewer_seat_id(),
        }
    }
    pub(super) fn basis(&self) -> PlayerBasis {
        match self {
            Self::Human(_, a) => a.basis(),
            Self::Controller(_, _, a) => a.basis(),
        }
    }
    pub(super) fn active(&self) -> bool {
        match self {
            Self::Human(_, a) => a.active(),
            Self::Controller(_, _, a) => a.active(),
        }
    }
    pub(super) fn matches_source(&self, r: &RecoveredNativeGame) -> bool {
        match self {
            Self::Human(_, a) => a.matches_source(r),
            Self::Controller(_, _, a) => a.matches_source(r),
        }
    }
    pub(super) fn valid(&self) -> Result<bool, NativeSourceError> {
        match self {
            Self::Human(actor, a) => Ok(a.matches_actor(actor)),
            Self::Controller(store, proof, a) => Ok(store
                .current(proof)
                .map_err(|e| NativeSourceError(Failure::Possession(e)))?
                && a.matches_controller(proof)),
        }
    }
    pub(super) fn original(
        &self,
        repo: &NativeSourceRepository,
        original: &PlayerCommand,
    ) -> Result<Option<ApiResponse<PlayerCommandAck>>, NativeSourceError> {
        match self {
            Self::Human(actor, _) => repo.human_original(actor, original),
            Self::Controller(store, proof, _) => repo.controller_original(store, proof, original),
        }
    }
    fn submit(
        &self,
        repo: &NativeSourceRepository,
        original: &PlayerCommand,
        prepared: &serde_json::Value,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        match self {
            Self::Human(actor, _) => repo.submit_human(actor, self.viewer(), original, prepared),
            Self::Controller(store, proof, _) => {
                repo.submit_controller(store, proof, self.viewer(), original, prepared)
            }
        }
    }
    pub(super) fn stale(
        &self,
        repo: &NativeSourceRepository,
        original: &PlayerCommand,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        if let Some(response) = self.original(repo, original)? {
            return Ok(response);
        }
        let current = self.basis();
        if !self.valid()?
            || original.basis.game_id != current.game_id
            || original.basis.context_id != current.context_id
            || original.basis.epoch != current.epoch
            || original.basis.revision > current.revision
            || original.basis == current && self.active()
        {
            return Err(invalid());
        }
        self.stale_after_observation(repo, original)
    }
    pub(super) fn stale_after_observation(
        &self,
        repo: &NativeSourceRepository,
        original: &PlayerCommand,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        original.validate().map_err(|_| invalid())?;
        if !self.valid()?
            || original.basis.game_id != self.basis().game_id
            || original.basis.context_id != self.basis().context_id
        {
            return Err(invalid());
        }
        self.submit(repo, original, &serde_json::Value::Null)
    }
    pub(super) fn issue<'g>(
        &self,
        repo: &NativeSourceRepository,
        store: &NativeArtifactStore,
        recovered: &'g RecoveredNativeGame,
        metadata: &NativeProjectionMetadata,
        display: &dyn NativeDisplaySource,
    ) -> Result<IssuedNativePlayerView<'g>, NativeSourceError> {
        if !self.valid()? {
            return Err(invalid());
        }
        match self {
            Self::Human(_, a) => repo.issue_human_view(store, a, recovered, metadata, display),
            Self::Controller(_, proof, a) => {
                repo.issue_controller_view(store, a, proof, recovered, metadata, display)
            }
        }
    }
    fn validate_frame(
        &self,
        recovered: &RecoveredNativeGame,
        issued: &IssuedNativePlayerView<'_>,
        original: &PlayerCommand,
    ) -> Result<(), NativeSourceError> {
        match self {
            Self::Human(actor, a) => validate_current_human(actor, a, recovered, issued, original),
            Self::Controller(_, _, a) => {
                a.bind(recovered)?;
                if !self.valid()? {
                    return Err(invalid());
                }
                validate_player_view(self.viewer(), recovered, issued, original)
            }
        }
    }
}
pub(super) struct NativePlayerCommit<'a, 'g> {
    pub authority: NativePlayerAuthority<'a>,
    pub recovered: &'a RecoveredNativeGame,
    pub issued: &'a IssuedNativePlayerView<'g>,
    pub original: &'a PlayerCommand,
    pub next: &'a NativeCheckpoint,
    pub candidate: &'a HostCandidate,
}
/// Actual executed/retained player proposal and qualified host frame inputs.
/// Exact native Event/result/participation and clock evidence is checked before
/// fresh acceptance. Terminal receipts bypass today's native preparation.
/// This does not accept client-supplied proofs, clocks, native state or artifacts.
pub struct NativeHumanCommit<'a, 'g> {
    pub actor: &'a RequestActor,
    pub access: &'a NativeHumanAccess,
    pub recovered: &'a RecoveredNativeGame,
    pub issued: &'a IssuedNativeHumanView<'g>,
    pub original: &'a PlayerCommand,
    pub next: &'a NativeCheckpoint,
    pub candidate: &'a HostCandidate,
}
impl NativeSourceRepository {
    /// Recover original dispositions without executing a move, resolving handles,
    /// loading native bytes or requiring the current host lease/runtime to work.
    /// SQL owns the authenticated seat; every returned item must agree on that
    /// seat and exactly preserve the query order and original semantic intent.
    /// # Errors
    /// Rejects invalid/over-budget queries, malformed/duplicate/positional replies,
    /// missing/reordered/foreign originals, fabricated successors or unsafe errors.
    pub fn read_human_receipts(
        &self,
        actor: &RequestActor,
        query: &PlayerReceiptQuery,
    ) -> Result<ApiResponse<Vec<PlayerReceipt>>, NativeSourceError> {
        let raw = receipt_query_raw(query)?;
        let response: ObjectResponse<Batch> = self
            .backend
            .rpc_bounded(
                "read_native_human_receipts",
                &json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_query":raw}),
                2 * 1024 * 1024,
            )
            .map_err(|error| NativeSourceError(Failure::Backend(error)))?;
        decode_batch(response.0, query)
    }
    /// Look up exactly one original before current runtime/source/artifact work.
    /// None means Unknown only; stored success/rejection and outer gate refusals
    /// remain structured. This read never submits a gameplay input.
    /// # Errors
    /// Rejects invalid intent or malformed/uncorrelated/unsafe receipt output.
    pub fn human_original(
        &self,
        actor: &RequestActor,
        original: &PlayerCommand,
    ) -> Result<Option<ApiResponse<PlayerCommandAck>>, NativeSourceError> {
        original.validate().map_err(|_| invalid())?;
        decode_original_receipt(self.read_human_receipts(
            actor,
            &PlayerReceiptQuery {
                game_id: original.basis.game_id,
                context_id: original.basis.context_id,
                outstanding: vec![original.clone()],
            },
        )?)
    }

    /// Submit a known past/terminal original to SQL's verified stale branch.
    /// No native View/Input or candidate is fabricated; SQL rechecks actual lineage.
    /// # Errors
    /// Rejects actor/scope/future/current-active mismatch, missing lineage or transport.
    pub fn submit_human_stale(
        &self,
        actor: &RequestActor,
        access: &NativeHumanAccess,
        original: &PlayerCommand,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        NativePlayerAuthority::Human(actor, access).stale(self, original)
    }

    pub(super) fn reject_player(
        &self,
        authority: NativePlayerAuthority<'_>,
        recovered: &RecoveredNativeGame,
        issued: &IssuedNativePlayerView<'_>,
        original: &PlayerCommand,
        rejection: &super::rule_rejection::RuleRejection,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        if let Some(response) = authority.original(self, original)? {
            return Ok(response);
        }
        authority.validate_frame(recovered, issued, original)?;
        let prepared = json!({"version":"native-player-reject-v1","basis":original.basis,"actorSeatId":authority.viewer(),"viewReceiptId":issued.receipt_id(),"runtimeId":recovered.opening().runtime_id(),"programSourceId":recovered.opening().program_source_id(),"errors":rejection.errors()});
        authority.submit(self, original, &prepared)
    }
    /// Commit an actual proposal via the original231human owner, never a raw writer.
    /// Read original receipts first on retries; historical recovery must not depend
    /// on today's native artifacts, handles, runtime or operational fence.
    /// # Errors
    /// Rejects foreign actor/view/cut, non-player/gapped proposal, mismatched retained
    /// original/candidate, transport or acknowledgment drift. SQL freshly rechecks
    /// authority and serializes original/child/checkpoint reciprocity and retries.
    pub fn commit_human(
        &self,
        store: &NativeArtifactStore,
        request: NativeHumanCommit<'_, '_>,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        let NativeHumanCommit {
            actor,
            access,
            recovered,
            issued,
            original,
            next,
            candidate,
        } = request;
        self.commit_player(
            store,
            NativePlayerCommit {
                authority: NativePlayerAuthority::Human(actor, access),
                recovered,
                issued,
                original,
                next,
                candidate,
            },
        )
    }
    pub(super) fn commit_player(
        &self,
        store: &NativeArtifactStore,
        request: NativePlayerCommit<'_, '_>,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        let NativePlayerCommit {
            authority,
            recovered,
            issued,
            original,
            next,
            candidate,
        } = request;
        original.validate().map_err(|_| invalid())?;
        if let Some(response) = authority.original(self, original)? {
            return Ok(response);
        }
        authority.validate_frame(recovered, issued, original)?;
        let (input, _) = lifecycle::proposal_input(
            store,
            recovered,
            next,
            candidate,
            NativeCheckpointInput::Player,
        )?;
        let artifact = store
            .read(
                input,
                original.basis.game_id,
                original.basis.context_id,
                NativeArtifactRole::Input,
            )
            .map_err(|error| NativeSourceError(Failure::Artifact(error)))?;
        let restored = restore_input(
            recovered.checkpoint().state(),
            issued.view().scope().binding.clone(),
            artifact.bytes(),
        )
        .map_err(|error| NativeSourceError(Failure::Input(error)))?;
        if restored.original() != original
            || restored.viewer_seat_id() != authority.viewer()
            || restored.candidate() != next.candidate()
        {
            return Err(invalid());
        }
        let raw: &serde_json::value::RawValue = serde_json::from_str(candidate.original_json())
            .map_err(|error| backend_error("candidate", error))?;
        let (owner, fence, _, _, _) = recovered.source().operational_basis();
        let prepared = json!({"version":"native-player-accept-v1","basis":original.basis,"actorSeatId":authority.viewer(),"viewReceiptId":issued.receipt_id(),
            "runtimeId":recovered.opening().runtime_id(),"programSourceId":recovered.opening().program_source_id(),"owner":owner,"fenceEpoch":fence,"inputRef":input,"candidate":raw});
        authority.submit(self, original, &prepared)
    }
    fn submit_human(
        &self,
        actor: &RequestActor,
        viewer: Uuid,
        original: &PlayerCommand,
        prepared: &serde_json::Value,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        let response=self.backend.rpc::<ObjectResponse<Ack>>("submit_native_human_command",&json!({
            "p_subject":actor.subject(),"p_session":actor.session(),"p_original":original,"p_prepared":prepared,
        })).map_err(|error|NativeSourceError(Failure::Backend(error)))
            .and_then(|response|decode_submission(response.0,original,viewer));
        match response {
            Ok(response) => Ok(response),
            Err(error) => match self.human_original(actor, original) {
                Ok(Some(response)) => Ok(response),
                _ => Err(error),
            },
        }
    }
}
fn validate_current_human(
    actor: &RequestActor,
    access: &NativeHumanAccess,
    recovered: &RecoveredNativeGame,
    issued: &IssuedNativeHumanView<'_>,
    original: &PlayerCommand,
) -> Result<(), NativeSourceError> {
    access.bind(recovered)?;
    if !access.matches_actor(actor) {
        return Err(invalid());
    }
    validate_player_view(access.viewer_seat_id(), recovered, issued, original)
}
fn validate_player_view(
    viewer: Uuid,
    recovered: &RecoveredNativeGame,
    issued: &IssuedNativePlayerView<'_>,
    original: &PlayerCommand,
) -> Result<(), NativeSourceError> {
    if issued.disclosure != NativeDisclosure::LiveParticipant
        || issued.checkpoint_ref != *recovered.checkpoint().reference()
        || original.basis != *recovered.checkpoint().basis()
        || original.basis != *issued.view().scope().basis()
        || issued.view().snapshot().viewer_seat_id != viewer
    {
        return Err(invalid());
    }
    Ok(())
}

fn decode_submission(
    response: ApiResponse<Ack>,
    original: &PlayerCommand,
    viewer: Uuid,
) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
    decode_player_submission(response, original, viewer, PlayerAuthorityKind::Human)
}
pub(super) fn decode_player_submission(
    response: ApiResponse<Ack>,
    original: &PlayerCommand,
    viewer: Uuid,
    kind: PlayerAuthorityKind,
) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
    match response {
        ApiResponse::Success { data } => Ok(ApiResponse::Success {
            data: data.public(original, viewer)?,
        }),
        ApiResponse::Fail { data }
            if gate_failure(&data, true, kind)
                || PlayerReceipt {
                    actor_seat_id: viewer,
                    original: original.clone(),
                    disposition: CommandDisposition::Rejected {
                        errors: data.clone(),
                    },
                }
                .validate(original, viewer)
                .is_ok() =>
        {
            Ok(ApiResponse::Fail { data })
        }
        _ => Err(invalid()),
    }
}

fn decode_batch(
    response: ApiResponse<Batch>,
    query: &PlayerReceiptQuery,
) -> Result<ApiResponse<Vec<PlayerReceipt>>, NativeSourceError> {
    decode_player_batch(response, query, PlayerAuthorityKind::Human)
}
pub(super) fn decode_player_batch(
    response: ApiResponse<Batch>,
    query: &PlayerReceiptQuery,
    kind: PlayerAuthorityKind,
) -> Result<ApiResponse<Vec<PlayerReceipt>>, NativeSourceError> {
    match response {
        ApiResponse::Success { data } => {
            let receipts = data
                .receipts
                .into_iter()
                .map(Receipt::public)
                .collect::<Result<Vec<_>, _>>()?;
            if receipts.len() != query.outstanding.len() {
                return Err(invalid());
            }
            if let Some(first) = receipts.first() {
                query
                    .validate_receipts(&receipts, first.actor_seat_id)
                    .map_err(|_| invalid())?;
            }
            Ok(ApiResponse::Success { data: receipts })
        }
        ApiResponse::Fail { data } if gate_failure(&data, false, kind) => {
            Ok(ApiResponse::Fail { data })
        }
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests;
