//! Private accepted-transition anchors, not complete native journals or seat views.
use super::*;
use crate::admission_ports::NativeEventSchema;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Transition<'a> {
    version: NativeEventSchema,
    state_ref: &'a NativeArtifactRef,
}
/// Canonical private Event prepared from an actually executed checkpoint.
/// Its State owns the original source/input/basis chain; semantic readers must
/// reconstruct that chain and independently establish SQL creator acceptance.
/// This is neither a public history entry nor a published participant replay.
pub struct PreparedNativeEvent(evidence::PreparedNativeEvidence);
impl PreparedNativeEvent {
    /// Exact scoped Event identity; it grants no disclosure or commit authority.
    pub fn reference(&self) -> &NativeArtifactRef {
        self.0.reference()
    }
    /// Canonical private transition bytes, never a player snapshot.
    pub fn original_bytes(&self) -> &[u8] {
        self.0.original_bytes()
    }
    /// Retain and read back these exact private Event bytes.
    /// # Errors
    /// Rejects unavailable/mismatched private Storage or altered bytes.
    pub fn retain(
        &self,
        store: &NativeArtifactStore,
    ) -> Result<NativeArtifactRef, NativeCheckpointError> {
        self.0.retain(store)
    }
    pub(super) fn verify_retained(
        &self,
        store: &NativeArtifactStore,
        reference: &str,
    ) -> Result<(), NativeCheckpointError> {
        self.0.verify_retained(store, reference)
    }
}
impl NativeCheckpoint {
    /// Compare an independently selected Event ref with this actual cut and
    /// read back its exact private bytes. The caller still owns source/creator
    /// acceptance, actor authorization and aggregate recovery budgeting.
    /// # Errors
    /// Rejects a foreign/mismatched Event before IO, or unavailable/altered bytes.
    pub fn verify_event(
        &self,
        store: &NativeArtifactStore,
        reference: &NativeArtifactRef,
    ) -> Result<(), NativeCheckpointError> {
        self.prepare_event()?
            .verify_retained(store, reference.as_str())
    }
    /// Prepare one versioned Event for this actual native candidate's exact State.
    /// No caller-supplied event, guessed journal or SQL sequence is accepted.
    /// Event construction/retention alone is not durable native acceptance.
    /// # Errors
    /// Rejects JSON encoding, invalid scope or over-budget private artifact bytes.
    pub fn prepare_event(&self) -> Result<PreparedNativeEvent, NativeCheckpointError> {
        self.prepare_event_schema(NativeEventSchema::TransitionV1)
    }
    /// Prepare the additive physical-cast activity Event at this exact State.
    /// # Errors
    /// Rejects encoding or invalid/over-budget artifact identity.
    pub fn prepare_activity_event(&self) -> Result<PreparedNativeEvent, NativeCheckpointError> {
        self.prepare_event_schema(self.activity_schema())
    }
    pub(super) fn prepare_event_schema(
        &self,
        schema: NativeEventSchema,
    ) -> Result<PreparedNativeEvent, NativeCheckpointError> {
        let bytes = serde_json::to_vec(&Transition {
            version: schema,
            state_ref: self.reference(),
        })
        .map_err(NativeCheckpointError::Json)?;
        Ok(PreparedNativeEvent(evidence::PreparedNativeEvidence::new(
            self.basis(),
            NativeArtifactRole::Event,
            bytes,
        )?))
    }
}
