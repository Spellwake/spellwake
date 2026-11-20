//! Compose actual native reconstruction with original receipt ownership.
use super::*;
use crate::{
    admission_ports::{AcceptReservedGame, OpeningAck},
    native_game::preparation_source::NativeReservedGameSource,
};

pub(super) type CheckpointObserver<'a> = dyn FnMut(
        &NativeGameSource,
        &NativeOpeningSource,
        &super::history::Group,
        &NativeCheckpoint,
        &mut NativeRecoveryBudget,
    ) -> Result<(), NativeSourceError>
    + 'a;

enum Recovery<'a> {
    StateOnly,
    History,
    OwnedHistory(&'a NativeHostOwner),
}

/// An observed accepted frontier and its actual reconstructed native checkpoint.
/// This is not a host lease or player authorization and must be revalidated on commit.
pub struct RecoveredNativeGame {
    source: NativeGameSource,
    opening: NativeOpeningSource,
    checkpoint: NativeCheckpoint,
    history_evidence: NativeHistoryEvidence,
}
impl RecoveredNativeGame {
    // A qualification driver can follow its own acknowledged setup commands. The final
    // independent cold recovery still checks every cut. Never promote this view to history.
    #[cfg(test)]
    pub(super) fn advance_observed_for_fixture(
        self,
        repository: &NativeSourceRepository,
        next: NativeCheckpoint,
    ) -> Result<Self, NativeSourceError> {
        let next_basis = next.basis();
        let prior = self.checkpoint();
        let (kind, parent, _) = next.retained_input().ok_or_else(invalid)?;
        if !matches!(kind, NativeCheckpointInput::Player)
            || parent != prior.reference()
            || prior.basis().revision.get().checked_add(1) != Some(next_basis.revision.get())
            || prior.basis().epoch != next_basis.epoch
            || next.initial_reference() != prior.initial_reference()
        {
            return Err(invalid());
        }
        let observed = repository.read(next_basis.game_id)?.ok_or_else(invalid)?;
        if observed.context_ids() != self.source.context_ids()
            || observed.operational_basis() != self.source.operational_basis()
            || observed.checkpoint_reference() != next.reference()
            || observed.accepted_revision() != &Amount::from(next_basis.revision.get())
            || observed.game_id() != self.source.game_id()
        {
            return Err(invalid());
        }
        Ok(Self {
            source: observed,
            opening: self.opening,
            checkpoint: next,
            history_evidence: NativeHistoryEvidence::StateOnly,
        })
    }
    /// Exact evidence scope established by this recovery, not feature/replay qualification.
    pub fn history_evidence(&self) -> NativeHistoryEvidence {
        self.history_evidence
    }
    /// SQL-selected frontier metadata, including independent operational fencing.
    pub fn source(&self) -> &NativeGameSource {
        &self.source
    }
    /// Original opening coordinates, not current host ownership.
    pub fn opening(&self) -> &NativeOpeningSource {
        &self.opening
    }
    /// Actual private native state reconstructed from verified original artifacts.
    pub fn checkpoint(&self) -> &NativeCheckpoint {
        &self.checkpoint
    }
}
/// Shared executed-proposal linkage for player and host originals.
pub(super) fn proposal_input<'a>(
    store: &NativeArtifactStore,
    recovered: &RecoveredNativeGame,
    next: &'a NativeCheckpoint,
    candidate: &crate::admission_ports::HostCandidate,
    expected: NativeCheckpointInput,
) -> Result<(&'a NativeArtifactRef, u64), NativeSourceError> {
    let prior = recovered.checkpoint();
    let source = recovered.source();
    let basis = prior.basis();
    if source.record.game_id != basis.game_id
        || source.record.context_id != basis.context_id
        || source.record.checkpoint.state_ref != *prior.reference()
        || source
            .record
            .seats
            .iter()
            .map(|seat| seat.seat_id)
            .ne(prior.roster.iter().map(|seat| seat.seat_id))
    {
        return Err(invalid());
    }
    let (kind, parent, input) = next.retained_input().ok_or_else(invalid)?;
    let next_revision = basis.revision.get().checked_add(1).ok_or_else(invalid)?;
    let (context, state, facts, mut events) = candidate.artifact_sources();
    let schema = candidate.event_schema_revision().ok_or_else(invalid)?;
    let event = next
        .prepare_event_schema(schema)
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
    let scoped = |text: &str, role| {
        NativeArtifactRef::try_from(text.to_owned()).is_ok_and(|reference| {
            reference.game_id() == basis.game_id
                && reference.context_id() == basis.context_id
                && reference.role() == role
        })
    };
    if !matches!(
        (kind, expected),
        (
            NativeCheckpointInput::ResponseTimeout,
            NativeCheckpointInput::ResponseTimeout
        ) | (NativeCheckpointInput::Player, NativeCheckpointInput::Player)
            | (
                NativeCheckpointInput::Entropy,
                NativeCheckpointInput::Entropy
            )
            | (
                NativeCheckpointInput::BankExpiry,
                NativeCheckpointInput::BankExpiry
            )
    ) || parent != prior.reference()
        || next.initial_reference() != prior.initial_reference()
        || next.basis().game_id != basis.game_id
        || next.basis().context_id != basis.context_id
        || next.basis().epoch != basis.epoch
        || next.basis().revision.get() != next_revision
        || candidate.turn_response_protocol() != next.turn_response_protocol()
        || candidate.opening_response_seat() != next.opening_response_seat()?
        || candidate.turn_response()
            != next
                .turn_response_after(prior.logical_wait().as_ref())?
                .as_ref()
        || context != basis.context_id
        || state != next.reference().as_str()
        || !scoped(facts, NativeArtifactRole::Facts)
        || events.len() != 1
        || events.next() != Some(event.reference().as_str())
        || !candidate.valid_for(&Amount::from(basis.revision.get()))
    {
        return Err(invalid());
    }
    let clocks_match = if matches!(expected, NativeCheckpointInput::BankExpiry) {
        let intent = recovered
            .bank_expiry_intent(next)
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e)))?;
        candidate.matches_native_bank_clocks(&intent)
    } else {
        let intent = recovered
            .clock_intent(next)
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e)))?;
        candidate.matches_native_clocks(&intent)
    };
    if !clocks_match {
        return Err(invalid());
    }
    let terminal = next
        .terminal()
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
    let participation = if terminal.is_some() {
        source
            .human_seats()
            .map(|seat| {
                next.own_turns(seat)
                    .map(|turns| (seat, turns))
                    .ok_or_else(invalid)
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![]
    };
    let matches = match schema {
        crate::admission_ports::NativeEventSchema::TransitionV1 => {
            candidate.matches_native_results(terminal.as_ref(), &participation)
        }
        crate::admission_ports::NativeEventSchema::ActivityV1
        | crate::admission_ports::NativeEventSchema::LandPlayV1 => {
            let facts = source
                .cast_accounts()
                .and_then(|accounts| accounts.facts_for_schema(next, schema))
                .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
            candidate.matches_native_activity_results(terminal.as_ref(), &participation, &facts)
        }
    };
    if !matches {
        return Err(invalid());
    }
    event
        .verify_retained(store, event.reference().as_str())
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
    next.prepare_results_schema(schema)
        .and_then(|proof| proof.verify_retained(store, facts))
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
    Ok((input, next_revision))
}
impl NativeSourceRepository {
    /// Commit one actually executed, retained native entropy proposal through231.
    /// Result-only evidence and terminal participation are checked against the actual
    /// native successor, including exact canonical retained Event/result bytes.
    /// Complete feature facts remain unsupported by result-only v1.
    /// # Errors
    /// Rejects foreign/gapped/player proposals, invalid artifact/basis linkage,
    /// stale fencing, changed original terms or malformed acknowledgment. SQL owns
    /// original/child/checkpoint reciprocity and exact original retry behavior.
    pub fn commit_entropy(
        &self,
        store: &NativeArtifactStore,
        source: &RecoveredNativeGame,
        request_id: Uuid,
        next: &NativeCheckpoint,
        candidate: &crate::admission_ports::HostCandidate,
        original: &NativeSystemOriginal,
    ) -> Result<crate::admission_ports::CandidateAck, NativeSourceError> {
        self.commit_host_input(
            store,
            source,
            request_id,
            next,
            candidate,
            original,
            NativeHostInputKind::Entropy,
        )
    }
    /// Commit a retained tentative bank expiry through the existing clock owner.
    /// SQL must authorize current due time, clock source and canonical BO3 result.
    /// # Errors
    /// Rejects unrelated originals/candidates, stale source, artifacts, policy or
    /// SQL acceptance. Exact accepted originals reconcile before fresh operands.
    pub fn commit_bank_expiry(
        &self,
        store: &NativeArtifactStore,
        source: &RecoveredNativeGame,
        request_id: Uuid,
        next: &NativeCheckpoint,
        candidate: &crate::admission_ports::HostCandidate,
        original: &NativeSystemOriginal,
    ) -> Result<crate::admission_ports::CandidateAck, NativeSourceError> {
        // Static original-to-native input correlation prevents an unrelated old
        // clock acknowledgment from being labeled a native bank expiry. This
        // needs no fresh clock state, retained-byte IO or current source basis.
        if !original.matches_bank_checkpoint(next) {
            return Err(invalid());
        }
        self.commit_host_input(
            store,
            source,
            request_id,
            next,
            candidate,
            original,
            NativeHostInputKind::Clock,
        )
    }
    #[allow(clippy::too_many_arguments)] // One shared private owner preserves both existing public entry-point shapes.
    fn commit_host_input(
        &self,
        store: &NativeArtifactStore,
        source: &RecoveredNativeGame,
        request_id: Uuid,
        next: &NativeCheckpoint,
        candidate: &crate::admission_ports::HostCandidate,
        original: &NativeSystemOriginal,
        host_kind: NativeHostInputKind,
    ) -> Result<crate::admission_ports::CandidateAck, NativeSourceError> {
        let prior = source.checkpoint();
        let basis = prior.basis();
        if request_id.is_nil()
            || original.request_id() != request_id
            || original.game_id() != basis.game_id
            || !original
                .host_fields()
                .is_some_and(|fields| fields.kind == host_kind)
        {
            return Err(invalid());
        }
        match self.read_system_disposition(original)? {
            NativeSystemDisposition::Accepted(NativeSystemAck::Host(ack)) => return Ok(ack),
            NativeSystemDisposition::Unknown => {}
            _ => return Err(invalid()),
        }
        let (prepared_original, input_kind) = match host_kind {
            NativeHostInputKind::Entropy => (
                NativeSystemOriginal::prepare_entropy(request_id, source, next)?,
                NativeCheckpointInput::Entropy,
            ),
            NativeHostInputKind::Clock => (
                NativeSystemOriginal::prepare_bank_expiry(request_id, source, next)?,
                NativeCheckpointInput::BankExpiry,
            ),
            NativeHostInputKind::NativeWork => return Err(invalid()),
        };
        if !original.matches_original(&prepared_original)? {
            return Err(invalid());
        }
        let (_, next_revision) = proposal_input(store, source, next, candidate, input_kind)?;
        self.send_host_candidate(source, request_id, original, candidate, next_revision)
    }
    /// Commit a retained response timeout at the original multiplayer opening.
    /// Reads an exact original disposition before checking a new due interval.
    /// # Errors
    /// Rejects different originals, unretained artifacts, stale/early intervals or failed SQL acceptance.
    pub fn commit_opening_response_timeout(
        &self,
        store: &NativeArtifactStore,
        source: &RecoveredNativeGame,
        request_id: Uuid,
        next: &NativeCheckpoint,
        original: &NativeSystemOriginal,
    ) -> Result<crate::admission_ports::CandidateAck, NativeSourceError> {
        let expected = NativeSystemOriginal::prepare_response_timeout(request_id, source, next)?;
        if original.request_id() != request_id || !original.matches_original(&expected)? {
            return Err(invalid());
        }
        match self.read_system_disposition(original)? {
            NativeSystemDisposition::Accepted(NativeSystemAck::Host(ack)) => return Ok(ack),
            NativeSystemDisposition::Unknown => {}
            _ => return Err(invalid()),
        }
        let wait = self
            .opening_response_wait(original.game_id())?
            .ok_or_else(invalid)?;
        let base = source.prepare_candidate(next)?;
        let frame = wait.frame(source, next, &base)?;
        let (_, revision) = proposal_input(
            store,
            source,
            next,
            &base,
            NativeCheckpointInput::ResponseTimeout,
        )?;
        self.send_host_candidate(source, request_id, original, &frame, revision)
    }
    fn send_host_candidate(
        &self,
        source: &RecoveredNativeGame,
        request_id: Uuid,
        original: &NativeSystemOriginal,
        candidate: &crate::admission_ports::HostCandidate,
        next_revision: u64,
    ) -> Result<crate::admission_ports::CandidateAck, NativeSourceError> {
        let basis = source.checkpoint().basis();
        let (owner, fence, _, _, _) = source.source().operational_basis();
        let raw: &serde_json::value::RawValue = serde_json::from_str(candidate.original_json())
            .map_err(|error| {
                NativeSourceError(Failure::Backend(BackendError::caused("candidate", error)))
            })?;
        let response: ObjectResponse<crate::admission_ports::CandidateAck> = self.backend.rpc("commit_native_host_input", &json!({
            "p_game":basis.game_id,"p_request":request_id,
            "p_original":serde_json::from_str::<&serde_json::value::RawValue>(original.original_json())
                .map_err(|error| NativeSourceError(Failure::Backend(BackendError::caused("original", error))))?,
            "p_prepared":{"owner":owner,"fenceEpoch":fence,"candidate":raw},
        })).map_err(|error| NativeSourceError(Failure::Backend(error)))?;
        match response.0 {
            ApiResponse::Success { data } if data.revision == Amount::from(next_revision) => {
                Ok(data)
            }
            _ => Err(invalid()),
        }
    }
    /// Accept a revision-zero native opening through its original224 owner.
    /// The caller must establish admission/card qualification independently. This
    /// checks observed reservation/native/receipt consistency, not deck legality.
    /// Exact original terms are retried unchanged after an uncertain acknowledgment.
    /// # Errors
    /// Rejects foreign original source, scope, roster, runtime/program identity,
    /// state/artifact mismatch or malformed/mismatched acknowledgment. SQL rejects
    /// stale reservation or changed original terms atomically. References do not
    /// establish retention; retain all artifacts and save the original before
    /// submitting. Terminal originals return before current native/byte checks.
    pub fn accept_opening(
        &self,
        store: &NativeArtifactStore,
        reservation: &NativeReservedGameSource,
        command: &AcceptReservedGame,
        checkpoint: &NativeCheckpoint,
        programs: &NativeProgramSource,
        original: &NativeSystemOriginal,
    ) -> Result<OpeningAck, NativeSourceError> {
        if !original.matches_opening_command(command)? {
            return Err(invalid());
        }
        match self.read_system_disposition(original)? {
            NativeSystemDisposition::Accepted(NativeSystemAck::Opening(ack)) => return Ok(ack),
            NativeSystemDisposition::Unknown => {}
            _ => return Err(invalid()),
        }
        if !original.matches_original(&NativeSystemOriginal::prepare_opening(
            command, checkpoint, programs,
        )?)? {
            return Err(invalid());
        }
        let r = reservation.metadata();
        let basis = checkpoint.basis();
        let context = &checkpoint.state().basis().context;
        let artifacts: crate::provider_json::Object<NativeOpeningArtifacts> =
            crate::decode_wire_object(command.opening.original_json().as_bytes()).map_err(
                |error| NativeSourceError(Failure::Backend(BackendError::caused("opening", error))),
            )?;
        let artifacts = artifacts.0;
        let first = r.seats.first().ok_or_else(invalid)?.seat_id;
        let binding = checkpoint
            .binding(first)
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        if command.match_id != r.match_id
            || command.game_id != r.game_id
            || command.expected_revision != r.match_revision
            || command
                .original_accepted_basis
                .as_ref()
                .is_some_and(|value| value != &command.expected_revision)
            || command.owner.as_str().trim_matches(' ').chars().count() > 256
            || basis.game_id != r.game_id
            || basis.revision.get() != 0
            || context.release() != r.release.id.to_string()
            || context.profile() != r.profile.id.to_string()
            || context.programs() != programs.identity().as_str()
            || !artifacts.valid(r.game_id, basis.context_id)
            || artifacts.initial() != checkpoint.initial_reference()
            || artifacts.state() != checkpoint.reference()
            || artifacts.event_schema_revision()
                != Some(crate::admission_ports::NativeEventSchema::TransitionV1)
            || artifacts.event()
                != checkpoint
                    .prepare_event()
                    .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?
                    .reference()
            || artifacts.facts()
                != checkpoint
                    .prepare_results()
                    .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?
                    .reference()
            || binding.seats.len() != r.seats.len()
            || checkpoint
                .provenance()
                .is_some_and(|provenance| !provenance.matches_reserved(reservation))
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
        if let Some(observation) = artifacts.clock_observation() {
            let chooser = crate::native_game::clocks::initial_waiter(checkpoint.state(), &binding)
                .map_err(|error| NativeSourceError(Failure::Checkpoint(error.into())))?;
            if !observation.matches(chooser, checkpoint.reference().as_str()) {
                return Err(invalid());
            }
        }
        checkpoint
            .verify_event(store, artifacts.event())
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        checkpoint
            .prepare_results()
            .and_then(|proof| proof.verify_retained(store, artifacts.facts().as_str()))
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        // Historical terminal originals returned before fresh native/byte checks.
        // SQL still owns atomic acceptance, current reservation and BO3 chooser requirements.
        let opening: &serde_json::value::RawValue =
            serde_json::from_str(command.opening.original_json()).map_err(|error| {
                NativeSourceError(Failure::Backend(BackendError::caused("opening", error)))
            })?;
        let response: ObjectResponse<OpeningAck> = self.backend.rpc("accept_native_opening", &json!({
            "p_match":command.match_id,"p_basis":command.expected_revision,"p_game":command.game_id,
            "p_owner":command.owner.as_str(),"p_opening":opening,"p_native_epoch":basis.epoch,
            "p_runtime_id":native_runtime_identity(),"p_program_source_id":programs.identity().hex(),
        })).map_err(|error| NativeSourceError(Failure::Backend(error)))?;
        match response.0 {
            ApiResponse::Success { data }
                if data.resource_id == r.game_id
                    && data.context_id == basis.context_id
                    && command
                        .expected_revision
                        .checked_add(&Amount::from(1))
                        .is_ok_and(|next| next == data.revision) =>
            {
                Ok(data)
            }
            _ => Err(invalid()),
        }
    }

    /// Reconstruct a SQL-selected accepted tip using its retained original opening.
    /// Current compiled runtime and program package must exactly match the original;
    /// no compatible historical runner or fencing-to-native-epoch inference is made.
    /// # Errors
    /// Rejects missing/foreign original coordinates, runtime/source mismatch,
    /// malformed artifacts, original epoch disagreement or recovery budget failure.
    /// Bank-expiry cuts require verified history and are refused by this method.
    /// Multiplayer successors also verify transition history because an elapsed
    /// response can precede later turns. None means the game source is unavailable.
    pub fn recover(
        &self,
        game: Uuid,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        mut budget: NativeRecoveryBudget,
    ) -> Result<Option<RecoveredNativeGame>, NativeSourceError> {
        self.recover_selected(
            game,
            store,
            programs,
            &mut budget,
            Recovery::StateOnly,
            None,
        )
    }

    /// Reconstruct one anchored accepted prefix and verify every versioned Event
    /// and result against its actual native cut using a single replay walk.
    /// Historical cuts retain an explicit unqualified evidence status.
    /// # Errors
    /// Rejects malformed/gapped accepted sources, native State/input disagreement,
    /// substituted Event/result bytes, unsupported feature facts or aggregate work
    /// exhaustion. No partial recovery is returned. None means unavailable source.
    pub fn recover_with_history(
        &self,
        game: Uuid,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        mut budget: NativeRecoveryBudget,
    ) -> Result<Option<RecoveredNativeGame>, NativeSourceError> {
        self.recover_selected(game, store, programs, &mut budget, Recovery::History, None)
    }

    /// Recover accepted history under an explicitly configured server owner.
    /// Selects NativeGameSource once, checks its owner/availability before artifact
    /// IO, and reuses that exact observation through replay and later preparation.
    /// This neither claims a lease nor replaces SQL's final acceptance fence.
    /// # Errors
    /// Rejects another/unavailable owner and all strict history recovery failures.
    pub fn recover_owned_history(
        &self,
        game: Uuid,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        mut budget: NativeRecoveryBudget,
        owner: &NativeHostOwner,
    ) -> Result<Option<RecoveredNativeGame>, NativeSourceError> {
        self.recover_selected(
            game,
            store,
            programs,
            &mut budget,
            Recovery::OwnedHistory(owner),
            None,
        )
    }

    pub(super) fn recover_observed(
        &self,
        game: Uuid,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        budget: &mut NativeRecoveryBudget,
        observer: &mut CheckpointObserver<'_>,
    ) -> Result<Option<RecoveredNativeGame>, NativeSourceError> {
        self.recover_selected(
            game,
            store,
            programs,
            budget,
            Recovery::History,
            Some(observer),
        )
    }

    fn recover_selected(
        &self,
        game: Uuid,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        budget: &mut NativeRecoveryBudget,
        mode: Recovery<'_>,
        mut observer: Option<&mut CheckpointObserver<'_>>,
    ) -> Result<Option<RecoveredNativeGame>, NativeSourceError> {
        let Some(source) = self.read(game)? else {
            return Ok(None);
        };
        // A multiplayer expiry can be an ancestor of a live turn. Authenticate
        // its elapsed interval even for an ordinary participant view.
        let check_history = !matches!(mode, Recovery::StateOnly)
            || (source.record.seats.len() >= 3 && !source.accepted_revision().is_zero());
        if let Recovery::OwnedHistory(owner) = mode {
            let (selected_owner, _, _, availability, _) = source.operational_basis();
            if selected_owner != owner.as_str()
                || availability != Availability::Available
                || source.match_availability() != Availability::Available
            {
                return Err(invalid());
            }
        }
        let opening = self.opening(game)?.ok_or_else(invalid)?;
        if opening.ids() != (source.match_id(), game, source.context_ids().0)
            || opening.runtime_id() != native_runtime_identity()
            || opening.program_source_id() != programs.identity().hex()
            || opening.artifacts().initial() != &source.record.initial_ref
            || opening.artifacts().context_revision() != source.context_revision()
        {
            return Err(invalid());
        }
        let history = if check_history {
            let Some(history) = self.read_transition_sources(&source, budget)? else {
                return Ok(None);
            };
            if history
                .groups
                .first()
                .map(|group| group.event_schema_revision)
                != Some(opening.artifacts().event_schema_revision())
            {
                return Err(invalid());
            }
            Some(history)
        } else {
            None
        };
        let initial_expected = source.initial_expected()?;
        if check_history {
            budget
                .charge_work(1, initial_expected.reference().size())
                .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        }
        let original = load_initial(store, &initial_expected, programs)
            .map_err(|error| NativeSourceError(Failure::Initial(error)))?;
        if original.candidate().state().basis().epoch != opening.native_epoch().get() {
            return Err(invalid());
        }
        if let Some(observation) = opening.artifacts().clock_observation() {
            let viewer = source.record.seats.first().ok_or_else(invalid)?.seat_id;
            let binding = original
                .binding(viewer)
                .map_err(|error| NativeSourceError(Failure::Initial(error)))?;
            let chooser =
                crate::native_game::clocks::initial_waiter(original.candidate().state(), &binding)
                    .map_err(|error| NativeSourceError(Failure::Checkpoint(error.into())))?;
            if !observation.matches(chooser, opening.artifacts().state().as_str()) {
                return Err(invalid());
            }
        }
        let expected = source
            .checkpoint_expected(&original)?
            .with_original_state(opening.artifacts().state().clone())
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        let (checkpoint, history_evidence) = if let Some(history) = history {
            let mut verifier = super::history_replay::HistoryReplay::new(&history);
            let mut observation_failure = None;
            let checkpoint = crate::native_game::checkpoint::load_checkpoint_observed(
                store,
                &expected,
                &initial_expected,
                original,
                budget,
                &mut |checkpoint, budget| {
                    verifier.observe(store, checkpoint, budget)?;
                    if let Some(observer) = observer.as_deref_mut()
                        && let Err(error) = observer(
                            &source,
                            &opening,
                            verifier.last_verified_group()?,
                            checkpoint,
                            budget,
                        )
                    {
                        observation_failure = Some(error);
                        return Err(NativeCheckpointError::Invalid);
                    }
                    Ok(())
                },
            );
            if let Some(error) = observation_failure {
                return Err(error);
            }
            let checkpoint =
                checkpoint.map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
            let evidence = verifier
                .finish()
                .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
            (checkpoint, evidence)
        } else {
            (
                load_checkpoint(
                    store,
                    &expected,
                    &initial_expected,
                    programs,
                    budget.clone(),
                )
                .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?,
                NativeHistoryEvidence::StateOnly,
            )
        };
        // A bank conclusion also needs the authenticated clock original and due
        // source. Basic live-view recovery intentionally does not read that evidence.
        // Native terminal input cannot be followed by another accepted native input,
        // so any successfully reconstructed bank input is necessarily the tip.
        if !check_history
            && checkpoint
                .retained_input()
                .is_some_and(|(kind, _, _)| matches!(kind, NativeCheckpointInput::BankExpiry))
        {
            return Err(invalid());
        }
        Ok(Some(RecoveredNativeGame {
            source,
            opening,
            checkpoint,
            history_evidence,
        }))
    }
}
