//! Bind authenticated SQL timeout provenance to an actual reconstructed bank input.
//! SQL owns elapsed-time authorization and acceptance. This reader rechecks the
//! retained source operands; metadata alone never creates a native conclusion.
use super::*;
use serde_json::value::RawValue;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Timeout {
    kind: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    clock_revision: Amount,
    proof_ref: NativeArtifactRef,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BeforeClock {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    remaining_ms: Amount,
    clock_revision: Amount,
    paused: bool,
    last_accounted_at: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::native_game::source_reader) struct BankExpiryEvidence {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    original_request_id: Uuid,
    original: Box<RawValue>,
    #[serde(deserialize_with = "crate::provider_json::object")]
    timeout: Timeout,
    #[serde(deserialize_with = "crate::provider_json::object")]
    before_clock: BeforeClock,
    accepted_at: String,
}
impl BankExpiryEvidence {
    pub(in crate::native_game::source_reader) fn verify(
        &self,
        checkpoint: &NativeCheckpoint,
    ) -> Result<(), NativeCheckpointError> {
        let Some((NativeCheckpointInput::BankExpiry, _, input)) = checkpoint.retained_input()
        else {
            return Err(NativeCheckpointError::Invalid);
        };
        let original = NativeSystemOriginal::from_bytes(
            self.original_request_id,
            self.original.get().as_bytes(),
        )
        .map_err(|_| NativeCheckpointError::Invalid)?;
        let host = original
            .host_fields()
            .ok_or(NativeCheckpointError::Invalid)?;
        let next = checkpoint.basis();
        let terminal = checkpoint
            .terminal()?
            .ok_or(NativeCheckpointError::Invalid)?;
        let [loser] = terminal.losing_seat_ids() else {
            return Err(NativeCheckpointError::Invalid);
        };
        if host.kind != NativeHostInputKind::Clock
            || host.input_ref != input
            || host.basis.game_id != next.game_id
            || host.basis.context_id != next.context_id
            || host.basis.epoch != next.epoch
            || host.basis.revision.get().checked_add(1) != Some(next.revision.get())
            || terminal.reason() != "native_bank_expired"
            || self.timeout.kind != "overall_bank"
            || self.timeout.seat_id != *loser
            || self.timeout.proof_ref != *input
            || self.before_clock.seat_id != *loser
            || self.before_clock.clock_revision != self.timeout.clock_revision
        {
            return Err(NativeCheckpointError::Invalid);
        }
        let from = parse_utc_microsecond(&self.before_clock.last_accounted_at)
            .ok_or(NativeCheckpointError::Invalid)?;
        let to = parse_utc_microsecond(&self.accepted_at).ok_or(NativeCheckpointError::Invalid)?;
        if to < from {
            return Err(NativeCheckpointError::Invalid);
        }
        // Same floor-to-millisecond operands as SQL clock_elapsed_ms; no local now.
        let elapsed = to
            .timestamp_millis()
            .checked_sub(from.timestamp_millis())
            .and_then(|value| u64::try_from(value).ok())
            .ok_or(NativeCheckpointError::Invalid)?;
        if !self.before_clock.remaining_ms.is_zero()
            && (self.before_clock.paused || Amount::from(elapsed) < self.before_clock.remaining_ms)
        {
            return Err(NativeCheckpointError::Invalid);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;
