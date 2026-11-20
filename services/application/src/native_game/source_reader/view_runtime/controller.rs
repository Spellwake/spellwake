//! Possessed-controller live View composition, with final local and SQL refresh.
use super::*;
use crate::controller_possession::{ControllerSecretStore, PossessedController};
impl NativeGameRuntime {
    /// Compose a seat-filtered live player View using actual local possession.
    /// Current SQL authority/source are refreshed after all recovery, issuance
    /// and projection work, including error paths. No full replay is authorized.
    /// # Errors
    /// Returns safe source/runtime/artifact errors only while authority and source
    /// still agree. Changed authority/source never delivers the composed snapshot.
    pub fn controller_view(
        &self,
        repository: &NativeSourceRepository,
        secrets: &ControllerSecretStore,
        proof: &PossessedController,
        request: &PlayerViewRequest,
    ) -> Result<NativeViewOutcome, NativeSourceError> {
        request.validate().map_err(|_| invalid())?;
        let access = match repository.controller_access(
            secrets,
            proof,
            request.game_id,
            request.context_id,
        )? {
            NativeControllerAccessOutcome::Granted(access) => access,
            NativeControllerAccessOutcome::Unavailable => {
                return Ok(NativeViewOutcome::Unavailable);
            }
        };
        let composed = (|| {
            let Some(recovered) = repository.recover(
                request.game_id,
                &self.store,
                &self.programs,
                self.recovery.clone(),
            )?
            else {
                return Ok(NativeViewOutcome::Unavailable);
            };
            if !access.matches_source(&recovered) {
                return Ok(NativeViewOutcome::Stale);
            }
            let provenance = recovered.checkpoint().provenance().ok_or_else(invalid)?;
            let display = NativeImmutableDisplay::new(&self.programs, provenance)
                .map_err(|e| NativeSourceError(Failure::Initial(e)))?;
            let metadata = NativeProjectionMetadata {
                through_event_sequence: recovered.source().last_event_sequence().clone(),
                deadline_at: None,
                disclosure: NativeDisclosure::LiveParticipant,
            };
            #[cfg(test)]
            self.observe_delivery(TestDeliveryStage::Issue, repository, &recovered);
            let issued = repository.issue_controller_view(
                &self.store,
                &access,
                proof,
                &recovered,
                &metadata,
                &display,
            )?;
            let snapshot = issued
                .with_live_deadline(repository, recovered.checkpoint(), access.viewer_seat_id())?
                .into_snapshot();
            #[cfg(test)]
            self.observe_delivery(TestDeliveryStage::Refresh, repository, &recovered);
            Ok(NativeViewOutcome::Snapshot(Box::new(snapshot)))
        })();
        match repository.controller_access(secrets, proof, request.game_id, request.context_id)? {
            NativeControllerAccessOutcome::Unavailable => Ok(NativeViewOutcome::Unavailable),
            NativeControllerAccessOutcome::Granted(current)
                if !access.same_delivery_source(&current)
                    && !access.same_except_publication(&current) =>
            {
                Ok(NativeViewOutcome::Stale)
            }
            NativeControllerAccessOutcome::Granted(_) => composed,
        }
    }
}

impl NativeGameRuntime {
    /// Execute one original controller intent using the shared actual player pipeline.
    /// Original recovery precedes access/native work. Late local revocation can
    /// suppress delivery after a remote commit; valid replacement authority can
    /// recover the same seat-original without repeating the move.
    /// # Errors
    /// Returns safe source/runtime failures; no arbitrary prepared input or
    /// fabricated permanent native rejection is accepted.
    pub fn controller_command(
        &self,
        repository: &NativeSourceRepository,
        secrets: &ControllerSecretStore,
        proof: &PossessedController,
        owner: &NativeHostOwner,
        original: &crate::player_wire::PlayerCommand,
    ) -> Result<ApiResponse<crate::player_wire::PlayerCommandAck>, NativeSourceError> {
        if let Some(response) = repository.controller_original(secrets, proof, original)? {
            return Ok(response);
        }
        let access = match repository.controller_access(
            secrets,
            proof,
            original.basis.game_id,
            original.basis.context_id,
        )? {
            NativeControllerAccessOutcome::Granted(access) => access,
            NativeControllerAccessOutcome::Unavailable => {
                return Ok(ApiResponse::Fail {
                    data: std::collections::BTreeMap::from([(
                        "actor".to_owned(),
                        "unavailable".to_owned(),
                    )]),
                });
            }
        };
        self.command_after_original(
            repository,
            super::super::command::NativePlayerAuthority::Controller(secrets, proof, &access),
            owner,
            original,
        )
    }
}
