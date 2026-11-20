//! Actual human mutation orchestration using shared frozen native resources.
use super::*;
use crate::{
    native_game::source_reader::{
        command::{NativePlayerAuthority, NativePlayerCommit},
        rule_rejection::RuleRejection,
    },
    player_wire::{PlayerCommand, PlayerCommandAck},
};
fn refusal(field: &str, reason: &str) -> ApiResponse<PlayerCommandAck> {
    ApiResponse::Fail {
        data: std::collections::BTreeMap::from([(field.to_owned(), reason.to_owned())]),
    }
}
impl NativeGameRuntime {
    /// Execute a current human intent with explicit trusted process ownership.
    /// The HTTP boundary reads originals before selecting optional runtime/config;
    /// this method also recovers any original committed meanwhile. SQL owns final
    /// actor/cut/host fencing and durable success or actual rule rejection.
    /// # Errors
    /// Source/runtime/registry/retention/clock/budget/internal native failures are
    /// safe service errors, never fabricated permanent player rejections.
    pub(crate) fn command_current(
        &self,
        repository: &NativeSourceRepository,
        actor: &RequestActor,
        owner: &NativeHostOwner,
        access: &NativeHumanAccess,
        original: &PlayerCommand,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        let authority = NativePlayerAuthority::Human(actor, access);
        if let Some(response) = authority.original(repository, original)? {
            return Ok(response);
        }
        self.command_after_original(repository, authority, owner, original)
    }
    pub(super) fn command_after_original(
        &self,
        repository: &NativeSourceRepository,
        authority: NativePlayerAuthority<'_>,
        owner: &NativeHostOwner,
        original: &PlayerCommand,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        match self.prepare_and_commit(repository, authority, owner, original) {
            Ok(response) => Ok(response),
            Err(error) => {
                // A competing/uncertain commit must remain recoverable even if
                // today's native source or Store is no longer usable.
                match authority.original(repository, original) {
                    Ok(Some(response)) => Ok(response),
                    _ => Err(error),
                }
            }
        }
    }
    fn prepare_and_commit(
        &self,
        repository: &NativeSourceRepository,
        authority: NativePlayerAuthority<'_>,
        owner: &NativeHostOwner,
        original: &PlayerCommand,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        original.validate().map_err(|_| invalid())?;
        if !authority.valid()? {
            return Err(invalid());
        }
        let current = authority.basis();
        if original.basis.game_id != current.game_id
            || original.basis.context_id != current.context_id
            || original.basis.epoch != current.epoch
            || original.basis.revision > current.revision
        {
            return Ok(refusal("basis", "invalid"));
        }
        if original.basis != current || !authority.active() {
            return authority.stale(repository, original);
        }
        #[cfg(test)]
        if let Some(hook) = &self.before_command_recovery {
            hook(repository);
        }
        let Some(recovered) = repository.recover_owned_history(
            original.basis.game_id,
            &self.store,
            &self.programs,
            self.recovery.clone(),
            owner,
        )?
        else {
            return Ok(refusal("game", "unavailable"));
        };
        if !authority.matches_source(&recovered) {
            // SQL observes the actual current lineage and owns the stale receipt.
            return authority.stale_after_observation(repository, original);
        }
        let provenance = recovered.checkpoint().provenance().ok_or_else(invalid)?;
        let display = NativeImmutableDisplay::new(&self.programs, provenance)
            .map_err(|error| NativeSourceError(Failure::Initial(error)))?;
        let metadata = NativeProjectionMetadata {
            through_event_sequence: recovered.source().last_event_sequence().clone(),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let issued = authority.issue(repository, &self.store, &recovered, &metadata, &display)?;
        let proposal = match issued.view().scope().propose(original) {
            Ok(proposal) => proposal,
            Err(error) => {
                if matches!(
                    error.native_error(),
                    Some(
                        GameError::Entropy(spellwake_engine::shuffle::EntropyError::Exhausted)
                            | GameError::Effect(EffectError::Window(
                                spellwake_engine::stack_window::StackWindowError::Objects(
                                    spellwake_engine::object_identity::ObjectIdentityError::Entropy(
                                        spellwake_engine::shuffle::EntropyError::Exhausted
                                    )
                                )
                            ))
                    )
                ) {
                    super::match_worker::refill_after_exhaustion(
                        repository, self, &recovered, owner,
                    )?;
                    return authority.stale_after_observation(repository, original);
                }
                let Some(rejection) = RuleRejection::from_bridge(&error) else {
                    return Err(NativeSourceError(Failure::Bridge(error)));
                };
                #[cfg(test)]
                self.observe_delivery(TestDeliveryStage::CommandReject, repository, &recovered);
                return repository
                    .reject_player(authority, &recovered, &issued, original, &rejection);
            }
        };
        let bytes = issued
            .view()
            .scope()
            .encode_input(&proposal)
            .map_err(|error| NativeSourceError(Failure::Input(error)))?;
        let input = self
            .store
            .retain(
                original.basis.game_id,
                original.basis.context_id,
                NativeArtifactRole::Input,
                &bytes,
            )
            .map_err(|error| NativeSourceError(Failure::Artifact(error)))?;
        let next = recovered
            .checkpoint()
            .append(
                &self.store,
                input,
                NativeCheckpointInput::Player,
                authority.viewer(),
            )
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        if next.candidate() != proposal.candidate() {
            return Err(invalid());
        }
        next.retain(&self.store)
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        next.prepare_activity_event()
            .and_then(|proof| proof.retain(&self.store))
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        next.prepare_activity_results()
            .and_then(|proof| proof.retain(&self.store))
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        let candidate = recovered.prepare_activity_candidate(&next)?;
        #[cfg(test)]
        self.observe_delivery(TestDeliveryStage::CommandCommit, repository, &recovered);
        repository.commit_player(
            &self.store,
            NativePlayerCommit {
                authority,
                recovered: &recovered,
                issued: &issued,
                original,
                next: &next,
                candidate: &candidate,
            },
        )
    }
}
