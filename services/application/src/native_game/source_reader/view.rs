//! Issue/load the semantic SQL first-winner View, never regenerate its handles.
use super::rpc::RecordBudget;
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
    pub(super) disclosure: NativeDisclosure,
    pub(super) checkpoint_ref: NativeArtifactRef,
}
impl<'a> IssuedNativePlayerView<'a> {
    /// Consume a read-only issuance and discard its private registry and locators.
    /// This moves only the public projection; native state remains borrowed.
    pub fn into_snapshot(self) -> crate::player_wire::PlayerSnapshot {
        self.view.into_snapshot()
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
    pub(super) archive: bool,
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
        }), &mut RecordBudget::Exclusive(budget))?.ok_or_else(invalid)?;
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
                archive: false,
            },
            metadata,
            display,
            RecordBudget::Unmetered,
        )?;
        Ok(IssuedNativeHumanView {
            receipt_id,
            view,
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
                archive: false,
            },
            metadata,
            display,
            RecordBudget::Unmetered,
        )?;
        Ok(IssuedNativePlayerView {
            receipt_id,
            view,
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
        mut budget: RecordBudget<'_>,
    ) -> Result<(Uuid, RestoredNativeView<'a>), NativeSourceError> {
        let basis = frame.checkpoint.basis().clone();
        let viewer = frame.binding.viewer;
        let kind = disclosure(metadata.disclosure);
        let mut arguments = json!({"p_game":basis.game_id,"p_context":basis.context_id,"p_epoch":basis.epoch,"p_revision":basis.revision,"p_viewer":viewer,"p_disclosure":kind});
        let mut record = self.view_record("read_native_view_source", &arguments, &mut budget)?;
        let retained = if record.is_none() {
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
            budget.charge(1, bytes)?;
            let retained = if frame.archive {
                let artifact = proposal
                    .retain_for_archive(store)
                    .map_err(|error| NativeSourceError(Failure::View(error)))?;
                arguments["p_view_ref"] = json!(artifact.reference().as_str());
                Some(artifact)
            } else {
                let reference = proposal
                    .retain(store)
                    .map_err(|error| NativeSourceError(Failure::View(error)))?;
                arguments["p_view_ref"] = json!(reference.as_str());
                None
            };
            record = self.view_record("publish_native_view_source", &arguments, &mut budget)?;
            retained
        } else {
            None
        };
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
        budget.charge(1, expected.reference().size())?;
        // Archive builders may race. Reuse authenticated bytes only when SQL
        // accepted our exact artifact. An earlier winner is opened independently.
        let retained = retained.filter(|artifact| artifact.reference() == expected.reference());
        let source = frame.checkpoint.state();
        let view = match (retained, frame.permit.as_ref()) {
            (Some(artifact), Some(permit)) => {
                crate::native_game::input_codec::restore_historical_view(
                    artifact,
                    &expected,
                    source,
                    frame.binding,
                    display,
                    permit,
                )
            }
            (Some(artifact), None) => crate::native_game::input_codec::restore_view(
                artifact,
                &expected,
                source,
                frame.binding,
                display,
            ),
            (None, Some(permit)) => crate::native_game::input_codec::load_historical_view(
                store,
                &expected,
                source,
                frame.binding,
                display,
                permit,
            ),
            (None, None) => load_view(store, &expected, source, frame.binding, display),
        }
        .map_err(|error| NativeSourceError(Failure::View(error)))?;
        Ok((record.receipt_id, view))
    }
    fn view_record(
        &self,
        name: &str,
        arguments: &serde_json::Value,
        budget: &mut RecordBudget<'_>,
    ) -> Result<Option<Record>, NativeSourceError> {
        let response: ObjectResponse<Record> =
            self.native_record_metered(name, arguments, budget)?;
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
