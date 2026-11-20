//! Reference-only source9 bridge. The immutable source object, not a bare
//! Registry or coverage DTO, owns the authenticated input/native relationship.
use super::SelectionSource;
use crate::current_profile::{NativeSelectionRegistryWitness, registry_witness_sealed};
use serde_json::Value;

impl registry_witness_sealed::Sealed for SelectionSource {}
impl NativeSelectionRegistryWitness for SelectionSource {
    fn native_coverage(&self) -> Result<Value, String> {
        serde_json::to_value(self.registry().coverage())
            .map_err(|error| format!("native Registry9 coverage serialization failed: {error}"))
    }
    fn validate_source(&self, vocabulary_id: &str) -> Result<(), String> {
        self.validate_witness(vocabulary_id)
    }
}

#[cfg(test)]
mod tests;
