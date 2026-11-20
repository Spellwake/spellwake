//! Original224 receipt coordinates, distinct from current operational ownership.
use super::*;
use crate::source_protocol::valid_digest;

/// Original accepted artifact coordinates. References alone do not qualify bytes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeOpeningArtifacts {
    #[serde(
        default,
        deserialize_with = "crate::admission_ports::present_event_schema"
    )]
    event_schema_revision: Option<crate::admission_ports::NativeEventSchema>,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    snapshot_ref: NativeArtifactRef,
    context_revision: SourceProof,
    state_ref: NativeArtifactRef,
    facts_ref: NativeArtifactRef,
    event_ref: NativeArtifactRef,
    qualification_ref: SourceProof,
    #[serde(
        default,
        deserialize_with = "crate::admission_ports::present_initial_clock"
    )]
    clock_observation: Option<crate::admission_ports::OpeningClockObservation>,
}
impl NativeOpeningArtifacts {
    /// Historical absence remains unqualified; a marker is only a codec claim.
    pub fn event_schema_revision(&self) -> Option<crate::admission_ports::NativeEventSchema> {
        self.event_schema_revision
    }
    pub(super) fn clock_observation(
        &self,
    ) -> Option<&crate::admission_ports::OpeningClockObservation> {
        self.clock_observation.as_ref()
    }
    pub(super) fn valid(&self, game: Uuid, context: Uuid) -> bool {
        self.context_id == context
            && self
                .clock_observation
                .as_ref()
                .is_none_or(|clock| clock.valid_for(self.state_ref.as_str()))
            && [
                (&self.snapshot_ref, NativeArtifactRole::Initial),
                (&self.state_ref, NativeArtifactRole::State),
                (&self.facts_ref, NativeArtifactRole::Facts),
                (&self.event_ref, NativeArtifactRole::Event),
            ]
            .into_iter()
            .all(|(reference, role)| {
                reference.game_id() == game
                    && reference.context_id() == context
                    && reference.role() == role
            })
            && self.snapshot_ref.size() <= 1_048_576
            && self.state_ref.size() <= 32_768
            && [&self.context_revision, &self.qualification_ref]
                .iter()
                .all(|proof| proof.as_str().trim_matches(' ').chars().count() <= 2048)
    }
    /// Original private initialization artifact, not a public URL.
    pub fn initial(&self) -> &NativeArtifactRef {
        &self.snapshot_ref
    }
    /// Original revision-zero checkpoint, not the current accepted tip.
    pub fn state(&self) -> &NativeArtifactRef {
        &self.state_ref
    }
    /// Original facts artifact, still requiring byte/codec verification.
    pub fn facts(&self) -> &NativeArtifactRef {
        &self.facts_ref
    }
    /// Original event artifact, still requiring byte/codec verification.
    pub fn event(&self) -> &NativeArtifactRef {
        &self.event_ref
    }
    /// Accepted engineering context reference, distinct from native epoch.
    pub fn context_revision(&self) -> &str {
        self.context_revision.as_str()
    }
    /// Retained admission proof text, not executable readiness by itself.
    pub fn qualification_ref(&self) -> &str {
        self.qualification_ref.as_str()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    owner: SourceProof,
    original_basis: Amount,
    #[serde(deserialize_with = "crate::provider_json::object")]
    opening: NativeOpeningArtifacts,
    native_epoch: NativeRevision,
    runtime_id: String,
    program_source_id: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    child_receipt_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    receipt_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    checkpoint_id: Uuid,
}
impl Record {
    fn valid(&self, game: Uuid) -> bool {
        self.version == "native-opening-v1"
            && self.game_id == game
            && self.owner.as_str().trim_matches(' ').chars().count() <= 256
            && valid_digest(&self.runtime_id)
            && valid_digest(&self.program_source_id)
            && self.opening.valid(game, self.context_id)
    }
}
/// Sealed original receipt observation. No mutable current host/fence is inferred.
pub struct NativeOpeningSource {
    record: Record,
}
impl NativeOpeningSource {
    /// Original match, game and context coordinates.
    pub fn ids(&self) -> (Uuid, Uuid, Uuid) {
        (
            self.record.match_id,
            self.record.game_id,
            self.record.context_id,
        )
    }
    /// Parent original receipt, canonical child and revision-zero checkpoint IDs.
    pub fn receipt_ids(&self) -> (Uuid, Uuid, Uuid) {
        (
            self.record.receipt_id,
            self.record.child_receipt_id,
            self.record.checkpoint_id,
        )
    }
    /// Native epoch selected at original opening, never the operational fence.
    pub fn native_epoch(&self) -> NativeRevision {
        self.record.native_epoch
    }
    /// Bare SHA256 identity of the original runtime, not current runner attestation.
    pub fn runtime_id(&self) -> &str {
        &self.record.runtime_id
    }
    /// Bare SHA256 identity of the original program package.
    pub fn program_source_id(&self) -> &str {
        &self.record.program_source_id
    }
    /// Original owner and admission match basis, never a current lease.
    pub fn original_basis(&self) -> (&str, &Amount) {
        (self.record.owner.as_str(), &self.record.original_basis)
    }
    /// Exact original accepted artifact coordinates.
    pub fn artifacts(&self) -> &NativeOpeningArtifacts {
        &self.record.opening
    }
}
impl NativeSourceRepository {
    /// Observe the original accepted opening, including after later game inputs.
    /// # Errors
    /// Rejects nil IDs before IO, malformed/duplicate/positional records, foreign
    /// scope/roles, noncanonical epochs/hashes and unknown failure vocabulary.
    /// Only the declared game-unavailable response becomes absence. SQL verifies
    /// parent/child/checkpoint reciprocity; byte verification remains separate.
    pub fn opening(&self, game: Uuid) -> Result<Option<NativeOpeningSource>, NativeSourceError> {
        self.opening_bounded(game, None)
    }
    /// Original opening observation charged to the caller's shared budget.
    pub(super) fn opening_bounded(
        &self,
        game: Uuid,
        budget: Option<&mut NativeRecoveryBudget>,
    ) -> Result<Option<NativeOpeningSource>, NativeSourceError> {
        if game.is_nil() {
            return Err(invalid());
        }
        let response: ObjectResponse<Record> = self.native_record(
            "read_native_opening_source",
            &json!({"p_game":game}),
            budget,
        )?;
        match response.0 {
            ApiResponse::Success { data } if data.valid(game) => {
                Ok(Some(NativeOpeningSource { record: data }))
            }
            ApiResponse::Fail { data }
                if data.len() == 1
                    && data.get("game").is_some_and(|value| value == "unavailable") =>
            {
                Ok(None)
            }
            _ => Err(invalid()),
        }
    }
}

#[cfg(test)]
mod tests;
