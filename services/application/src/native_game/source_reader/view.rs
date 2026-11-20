//! Issue/load the semantic SQL first-winner View, never regenerate its handles.
use super::*;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    basis: PlayerBasis,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    viewer_seat_id: Uuid,
    disclosure: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    opening_receipt_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    checkpoint_id: Uuid,
    through_event_sequence: Amount,
    view_ref: NativeArtifactRef,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    receipt_id: Uuid,
}
fn disclosure(kind: NativeDisclosure) -> &'static str {
    match kind {
        NativeDisclosure::LiveParticipant => "live",
        NativeDisclosure::CompletedParticipant => "completed",
    }
}
/// Exact issued SQL identity and its verified private registry/public snapshot.
pub struct IssuedNativePlayerView<'a> {
    receipt_id: Uuid,
    view: RestoredNativeView<'a>,
    live_deadline: Option<String>,
    pub(super) disclosure: NativeDisclosure,
    pub(super) checkpoint_ref: NativeArtifactRef,
}
impl<'a> IssuedNativePlayerView<'a> {
    /// Consume a read-only issuance and discard its private registry and locators.
    /// This moves only the public projection; native state remains borrowed.
    pub fn into_snapshot(self) -> crate::player_wire::PlayerSnapshot {
        let mut snapshot = self.view.into_snapshot();
        if let Some(deadline) = self.live_deadline {
            if snapshot.decisions.is_empty()
                && let Some(turn) = &mut snapshot.turn
                && turn.priority_seat_ids.as_slice() == [snapshot.viewer_seat_id]
            {
                turn.priority_deadline_at = Some(deadline.clone());
            }
            for decision in &mut snapshot.decisions {
                if decision
                    .responding_seat_ids
                    .contains(&snapshot.viewer_seat_id)
                {
                    decision.deadline_at = Some(deadline.clone());
                }
            }
        }
        snapshot
    }
    /// Attach the operational deadline only to a live response. The durable
    /// View keeps its original decision handles across cached reads and retries.
    pub(super) fn with_live_deadline(
        mut self,
        repository: &NativeSourceRepository,
        checkpoint: &NativeCheckpoint,
        viewer: Uuid,
    ) -> Result<Self, NativeSourceError> {
        if checkpoint.response_seat()? == Some(viewer)
            && let Some(wait) = repository.opening_response_wait(checkpoint.basis().game_id)?
        {
            self.live_deadline = Some(wait.deadline_for(checkpoint, viewer)?.to_owned());
        }
        Ok(self)
    }
    /// Original SQL View identity for trusted native command preparation.
    pub fn receipt_id(&self) -> Uuid {
        self.receipt_id
    }
    /// Exact original registry and public snapshot, not a new handle allocation.
    pub fn view(&self) -> &RestoredNativeView<'a> {
        &self.view
    }
}
/// Existing human API name for the same actor-neutral issued player View.
/// Authority is still supplied by distinct human/controller access types.
pub type IssuedNativeHumanView<'a> = IssuedNativePlayerView<'a>;
/// Internal frame selected by current binding or the verified historical observer.
pub(super) struct ViewFrame<'a> {
    pub(super) checkpoint: &'a NativeCheckpoint,
    pub(super) binding: NativeBinding,
    pub(super) opening_receipt_id: Uuid,
    pub(super) checkpoint_id: Uuid,
    pub(super) permit: Option<crate::native_game::projection::HistoricalProjectionPermit<'a>>,
}
/// A retained cut's claims, checked against SQL's existing semantic winner.
pub(super) struct ArchivedViewClaim<'a> {
    pub(super) basis: &'a PlayerBasis,
    pub(super) viewer: Uuid,
    pub(super) disclosure: NativeDisclosure,
    pub(super) opening: Uuid,
    pub(super) checkpoint: Uuid,
    pub(super) through: &'a Amount,
    pub(super) receipt: Uuid,
    pub(super) reference: &'a NativeArtifactRef,
}
impl NativeSourceRepository {
    /// Authenticate a retained View identity without issuing a replacement or
    /// executing the original native runtime. Private bytes are checked separately.
    pub(super) fn archived_view_source(
        &self,
        claim: ArchivedViewClaim<'_>,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<NativeViewExpected, NativeSourceError> {
        let b = claim.basis;
        let expected = NativeViewExpected::new(
            claim.reference.clone(),
            b.clone(),
            claim.viewer,
            claim.disclosure,
            claim.through.clone(),
        )
        .map_err(|error| NativeSourceError(Failure::View(error)))?;
        let record = self.view_record("read_native_view_source", &json!({
            "p_game":b.game_id,"p_context":b.context_id,"p_epoch":b.epoch,
            "p_revision":b.revision,"p_viewer":claim.viewer,"p_disclosure":disclosure(claim.disclosure)
        }), Some(budget))?.ok_or_else(invalid)?;
        if record.version != "native-view-v1"
            || record.basis != *b
            || record.viewer_seat_id != claim.viewer
            || record.disclosure != disclosure(claim.disclosure)
            || record.opening_receipt_id != claim.opening
            || record.checkpoint_id != claim.checkpoint
            || record.through_event_sequence != *claim.through
            || record.receipt_id != claim.receipt
            || record.view_ref != *claim.reference
        {
            return Err(invalid());
        }
        Ok(expected)
    }

    /// Read an existing View or retain/publish a new proposal and load SQL's winner.
    /// Current caller authority comes from human_access; refresh it before delivery.
    /// This is not a stream lifetime lease, card/display qualification or HTTP route.
    /// # Errors
    /// Rejects mismatched access/native cut/frontier, unauthorized full disclosure,
    /// transport/issuance drift or private artifact/registry/projection corruption.
    /// Concurrent proposals never replace the accepted winner. Cached reads do not
    /// create another proposal; publication always reloads the actual winning ref.
    pub fn issue_human_view<'a>(
        &self,
        store: &NativeArtifactStore,
        access: &NativeHumanAccess,
        recovered: &'a RecoveredNativeGame,
        metadata: &NativeProjectionMetadata,
        display: &dyn NativeDisplaySource,
    ) -> Result<IssuedNativeHumanView<'a>, NativeSourceError> {
        let binding = access.bind(recovered)?;
        if metadata.through_event_sequence != *recovered.source().last_event_sequence()
            || metadata.disclosure == NativeDisclosure::CompletedParticipant
                && access.completed_disclosure().is_none()
        {
            return Err(invalid());
        }
        let (receipt_id, view) = self.issue_verified_view(
            store,
            ViewFrame {
                checkpoint: recovered.checkpoint(),
                binding,
                opening_receipt_id: recovered.opening().receipt_ids().0,
                checkpoint_id: recovered.source().checkpoint_ids().0,
                permit: None,
            },
            metadata,
            display,
            None,
        )?;
        Ok(IssuedNativeHumanView {
            receipt_id,
            view,
            live_deadline: None,
            disclosure: metadata.disclosure,
            checkpoint_ref: recovered.checkpoint().reference().clone(),
        })
    }
    /// Issue only the live player perspective of an exact source-bound AI seat.
    /// Caller owns fresh local/canonical authority checks around composition.
    pub(super) fn issue_controller_view<'a>(
        &self,
        store: &NativeArtifactStore,
        access: &NativeControllerAccess,
        proof: &crate::controller_possession::PossessedController,
        recovered: &'a RecoveredNativeGame,
        metadata: &NativeProjectionMetadata,
        display: &dyn NativeDisplaySource,
    ) -> Result<IssuedNativePlayerView<'a>, NativeSourceError> {
        if !access.matches_controller(proof)
            || metadata.disclosure != NativeDisclosure::LiveParticipant
            || metadata.through_event_sequence != *recovered.source().last_event_sequence()
        {
            return Err(invalid());
        }
        let binding = access.bind(recovered)?;
        let (receipt_id, view) = self.issue_verified_view(
            store,
            ViewFrame {
                checkpoint: recovered.checkpoint(),
                binding,
                opening_receipt_id: recovered.opening().receipt_ids().0,
                checkpoint_id: recovered.source().checkpoint_ids().0,
                permit: None,
            },
            metadata,
            display,
            None,
        )?;
        Ok(IssuedNativePlayerView {
            receipt_id,
            view,
            live_deadline: None,
            disclosure: metadata.disclosure,
            checkpoint_ref: recovered.checkpoint().reference().clone(),
        })
    }
    pub(super) fn issue_verified_view<'a>(
        &self,
        store: &NativeArtifactStore,
        frame: ViewFrame<'a>,
        metadata: &NativeProjectionMetadata,
        display: &dyn NativeDisplaySource,
        mut budget: Option<&mut NativeRecoveryBudget>,
    ) -> Result<(Uuid, RestoredNativeView<'a>), NativeSourceError> {
        let basis = frame.checkpoint.basis().clone();
        let viewer = frame.binding.viewer;
        let kind = disclosure(metadata.disclosure);
        let mut arguments = json!({"p_game":basis.game_id,"p_context":basis.context_id,"p_epoch":basis.epoch,"p_revision":basis.revision,"p_viewer":viewer,"p_disclosure":kind});
        let mut record =
            self.view_record("read_native_view_source", &arguments, budget.as_deref_mut())?;
        if record.is_none() {
            let proposal = match frame.permit.as_ref() {
                Some(permit) => crate::native_game::input_codec::prepare_historical_view(
                    frame.checkpoint.state(),
                    frame.binding.clone(),
                    metadata,
                    display,
                    permit,
                ),
                None => prepare_view(
                    frame.checkpoint.state(),
                    frame.binding.clone(),
                    metadata,
                    display,
                ),
            }
            .map_err(|error| NativeSourceError(Failure::View(error)))?;
            // Retention writes then independently reads back the private bytes.
            let bytes = proposal
                .reference()
                .size()
                .checked_mul(2)
                .ok_or_else(invalid)?;
            charge_view(budget.as_deref_mut(), bytes)?;
            let reference = proposal
                .retain(store)
                .map_err(|error| NativeSourceError(Failure::View(error)))?;
            arguments["p_view_ref"] = json!(reference.as_str());
            record = self.view_record(
                "publish_native_view_source",
                &arguments,
                budget.as_deref_mut(),
            )?;
        }
        let record = record.ok_or_else(invalid)?;
        if record.version != "native-view-v1"
            || record.basis != basis
            || record.viewer_seat_id != viewer
            || record.disclosure != kind
            || record.opening_receipt_id != frame.opening_receipt_id
            || record.checkpoint_id != frame.checkpoint_id
            || record.through_event_sequence != metadata.through_event_sequence
        {
            return Err(invalid());
        }
        let expected = NativeViewExpected::new(
            record.view_ref,
            basis,
            viewer,
            metadata.disclosure,
            record.through_event_sequence,
        )
        .map_err(|error| NativeSourceError(Failure::View(error)))?;
        charge_view(budget, expected.reference().size())?;
        let view = match frame.permit.as_ref() {
            Some(permit) => crate::native_game::input_codec::load_historical_view(
                store,
                &expected,
                frame.checkpoint.state(),
                frame.binding,
                display,
                permit,
            ),
            None => load_view(
                store,
                &expected,
                frame.checkpoint.state(),
                frame.binding,
                display,
            ),
        }
        .map_err(|error| NativeSourceError(Failure::View(error)))?;
        Ok((record.receipt_id, view))
    }
    fn view_record(
        &self,
        name: &str,
        arguments: &serde_json::Value,
        budget: Option<&mut NativeRecoveryBudget>,
    ) -> Result<Option<Record>, NativeSourceError> {
        let response: ObjectResponse<Record> = self.native_record(name, arguments, budget)?;
        match response.0 {
            ApiResponse::Success { data } => Ok(Some(data)),
            ApiResponse::Fail { data }
                if data.len() == 1
                    && data.get("view").is_some_and(|value| value == "unavailable") =>
            {
                Ok(None)
            }
            _ => Err(invalid()),
        }
    }
}
fn charge_view(
    budget: Option<&mut NativeRecoveryBudget>,
    bytes: u64,
) -> Result<(), NativeSourceError> {
    if let Some(budget) = budget {
        budget
            .charge_work(1, bytes)
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
    }
    Ok(())
}
