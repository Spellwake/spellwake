//! Shared exact-byte retention mechanics for privately prepared native evidence.
use super::*;

pub(super) struct PreparedNativeEvidence {
    reference: NativeArtifactRef,
    bytes: Vec<u8>,
}
impl PreparedNativeEvidence {
    pub(super) fn new(
        basis: &PlayerBasis,
        role: NativeArtifactRole,
        bytes: Vec<u8>,
    ) -> Result<Self, NativeCheckpointError> {
        let reference =
            NativeArtifactRef::for_bytes(basis.game_id, basis.context_id, role, &bytes)?;
        Ok(Self { reference, bytes })
    }
    pub(super) fn reference(&self) -> &NativeArtifactRef {
        &self.reference
    }
    pub(super) fn original_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(super) fn retain(
        &self,
        store: &NativeArtifactStore,
    ) -> Result<NativeArtifactRef, NativeCheckpointError> {
        let reference = store.retain(
            self.reference.game_id(),
            self.reference.context_id(),
            self.reference.role(),
            &self.bytes,
        )?;
        if reference != self.reference {
            return Err(NativeCheckpointError::Invalid);
        }
        Ok(reference)
    }
    pub(super) fn verify_retained(
        &self,
        store: &NativeArtifactStore,
        reference: &str,
    ) -> Result<(), NativeCheckpointError> {
        if reference != self.reference.as_str() {
            return Err(NativeCheckpointError::Invalid);
        }
        let actual = store.read(
            &self.reference,
            self.reference.game_id(),
            self.reference.context_id(),
            self.reference.role(),
        )?;
        if actual.bytes() != self.bytes {
            return Err(NativeCheckpointError::Invalid);
        }
        Ok(())
    }
}
