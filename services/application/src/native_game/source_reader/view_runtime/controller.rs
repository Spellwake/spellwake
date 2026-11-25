//! Possessed-controller live View composition, with final local and SQL refresh.
use super::*;
use crate::{
    ai_runtime::{PriorityAffordances, SeatKnowledgeView, probe_priority_affordances},
    controller_possession::{ControllerSecretStore, PossessedController},
    native_game::NativeCastBatchError,
    player_wire::{PlayerAction, PlayerCommand, PlayerCommandAck, PlayerSnapshot},
};

/// Live controller View together with verified priority affordances (legal land plays,
/// mana sources, castable spells, and activatable abilities) and durable seat-scoped
/// revealed-card knowledge computed on the exact same scope.
pub struct NativeControllerSnapshot {
    pub snapshot: Box<PlayerSnapshot>,
    pub basic_actions: Vec<PlayerAction>,
    pub affordances: PriorityAffordances,
    pub knowledge: Box<SeatKnowledgeView>,
}

/// Outcome of a controller view read that also returns verified basic affordances.
pub enum NativeControllerViewOutcome {
    Snapshot(NativeControllerSnapshot),
    Unavailable,
    Stale,
}

impl NativeGameRuntime {
    /// Compose a seat-filtered live player View using actual local possession and the
    /// shared verified checkpoint cache.
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
        match self.controller_view_inner(repository, secrets, proof, request, false)? {
            NativeControllerViewOutcome::Snapshot(view) => {
                Ok(NativeViewOutcome::Snapshot(view.snapshot))
            }
            NativeControllerViewOutcome::Unavailable => Ok(NativeViewOutcome::Unavailable),
            NativeControllerViewOutcome::Stale => Ok(NativeViewOutcome::Stale),
        }
    }

    /// Compose a seat-filtered live player View along with verified own basic land and
    /// mana affordances for AI decision-making.
    /// # Errors
    /// Returns safe source/runtime/artifact errors only while authority and source
    /// still agree.
    pub fn controller_view_with_actions(
        &self,
        repository: &NativeSourceRepository,
        secrets: &ControllerSecretStore,
        proof: &PossessedController,
        request: &PlayerViewRequest,
    ) -> Result<NativeControllerViewOutcome, NativeSourceError> {
        self.controller_view_inner(repository, secrets, proof, request, true)
    }

    fn controller_view_inner(
        &self,
        repository: &NativeSourceRepository,
        secrets: &ControllerSecretStore,
        proof: &PossessedController,
        request: &PlayerViewRequest,
        include_basic_actions: bool,
    ) -> Result<NativeControllerViewOutcome, NativeSourceError> {
        request.validate().map_err(|_| invalid())?;
        let access = match repository.controller_access(
            secrets,
            proof,
            request.game_id,
            request.context_id,
        )? {
            NativeControllerAccessOutcome::Granted(access) => access,
            NativeControllerAccessOutcome::Unavailable => {
                return Ok(NativeControllerViewOutcome::Unavailable);
            }
        };
        let composed = (|| {
            let Some(recovered) = repository.recover_cached(
                request.game_id,
                &self.store,
                &self.programs,
                self.recovery.clone(),
                &self.checkpoints,
            )?
            else {
                return Ok(NativeControllerViewOutcome::Unavailable);
            };
            if !access.matches_source(&recovered) {
                return Ok(NativeControllerViewOutcome::Stale);
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
            let affordances = if include_basic_actions {
                probe_priority_affordances(issued.view().scope(), issued.view().snapshot())
            } else {
                PriorityAffordances::default()
            };
            let knowledge = recovered.checkpoint().seat_knowledge(
                issued.view().snapshot().viewer_seat_id,
                &display,
                Some(issued.view().scope()),
                issued.view().snapshot(),
            );
            let basic_actions = affordances.basic_actions.clone();
            let snapshot = issued.into_snapshot();
            #[cfg(test)]
            self.observe_delivery(TestDeliveryStage::Refresh, repository, &recovered);
            Ok(NativeControllerViewOutcome::Snapshot(
                NativeControllerSnapshot {
                    snapshot: Box::new(snapshot),
                    basic_actions,
                    affordances,
                    knowledge: Box::new(knowledge),
                },
            ))
        })();
        match repository.controller_access(secrets, proof, request.game_id, request.context_id)? {
            NativeControllerAccessOutcome::Unavailable => {
                Ok(NativeControllerViewOutcome::Unavailable)
            }
            NativeControllerAccessOutcome::Granted(current)
                if !access.same_delivery_source(&current)
                    && !access.same_except_publication(&current) =>
            {
                Ok(NativeControllerViewOutcome::Stale)
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
        original: &PlayerCommand,
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
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

    /// Atomically pre-validate a mana-and-cast batch (0..=8 mana activations followed by
    /// one cast) against the current owned checkpoint before committing each validated step.
    /// If any step in the batch is illegal, zero mana activations are committed.
    /// # Errors
    /// Returns safe source/runtime failures. Rule/batch rejections return `ApiResponse::Fail`.
    pub fn controller_cast_batch(
        &self,
        repository: &NativeSourceRepository,
        secrets: &ControllerSecretStore,
        proof: &PossessedController,
        owner: &NativeHostOwner,
        originals: &[PlayerCommand],
    ) -> Result<ApiResponse<PlayerCommandAck>, NativeSourceError> {
        let Some(first) = originals.first() else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([(
                    "batch".to_owned(),
                    "invalid mana-and-cast batch".to_owned(),
                )]),
            });
        };
        let access = match repository.controller_access(
            secrets,
            proof,
            first.basis.game_id,
            first.basis.context_id,
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
        let Some(recovered) = repository.recover_owned_history_cached(
            first.basis.game_id,
            &self.store,
            &self.programs,
            self.recovery.clone(),
            owner,
            &self.checkpoints,
        )?
        else {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([(
                    "game".to_owned(),
                    "unavailable".to_owned(),
                )]),
            });
        };
        if !access.matches_source(&recovered) {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("basis".to_owned(), "stale".to_owned())]),
            });
        }
        let provenance = recovered.checkpoint().provenance().ok_or_else(invalid)?;
        let display = NativeImmutableDisplay::new(&self.programs, provenance)
            .map_err(|error| NativeSourceError(Failure::Initial(error)))?;
        let metadata = NativeProjectionMetadata {
            through_event_sequence: recovered.source().last_event_sequence().clone(),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let issued = repository.issue_controller_view(
            &self.store,
            &access,
            proof,
            &recovered,
            &metadata,
            &display,
        )?;
        // Atomic pre-validation: if any mana tap or the final cast fails, commit nothing.
        if let Err(error) = issued.view().scope().prepare_cast_batch(originals) {
            let detail = match &error {
                NativeCastBatchError::InvalidShape => "invalid mana-and-cast batch".to_owned(),
                NativeCastBatchError::Boundary { step } => {
                    format!("batch step {step} crosses a required player boundary")
                }
                NativeCastBatchError::Bridge { step, .. } => {
                    format!("batch step {step} was rejected by game rules")
                }
                NativeCastBatchError::Codec { step, .. } => {
                    format!("batch step {step} could not be recorded")
                }
                NativeCastBatchError::ReplayMismatch { step } => {
                    format!("batch step {step} failed reconstruction")
                }
            };
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("batch".to_owned(), detail)]),
            });
        }

        // All steps validated cleanly on the exact current state; commit them sequentially
        // with freshly updated bases while holding the verified checkpoint cache warm.
        let mut basis = first.basis.clone();
        let mut last_ack: Option<ApiResponse<PlayerCommandAck>> = None;
        for step_cmd in originals {
            let mut command = step_cmd.clone();
            command.basis = basis.clone();
            let resp = self.controller_command(repository, secrets, proof, owner, &command)?;
            match &resp {
                ApiResponse::Success { data } => {
                    basis = data.basis.clone();
                    last_ack = Some(resp);
                }
                ApiResponse::Fail { .. } | ApiResponse::Error { .. } => return Ok(resp),
            }
        }
        Ok(last_ack.unwrap_or_else(|| ApiResponse::Fail {
            data: std::collections::BTreeMap::from([(
                "batch".to_owned(),
                "empty cast batch".to_owned(),
            )]),
        }))
    }
}
