//! Correlate the accepted opening timeout with the reconstructed native departure.
use super::*;
use serde_json::value::RawValue;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::native_game::source_reader) struct ResponseExpiryEvidence {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    original_request_id: Uuid,
    original: Box<RawValue>,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    decision_ref: NativeArtifactRef,
    expires_at: String,
    accepted_at: String,
}
impl ResponseExpiryEvidence {
    pub(in crate::native_game::source_reader) fn verify(
        &self,
        next: &NativeCheckpoint,
    ) -> Result<(), NativeCheckpointError> {
        let error = NativeCheckpointError::Invalid;
        let Some((NativeCheckpointInput::ResponseTimeout, parent, input)) = next.retained_input()
        else {
            return Err(error);
        };
        let original = NativeSystemOriginal::from_bytes(
            self.original_request_id,
            self.original.get().as_bytes(),
        )
        .map_err(|_| NativeCheckpointError::Invalid)?;
        let host = original
            .host_fields()
            .ok_or(NativeCheckpointError::Invalid)?;
        let departing = next
            .roster
            .iter()
            .find(|s| s.seat_id == self.seat_id)
            .ok_or(NativeCheckpointError::Invalid)?;
        let expires =
            parse_utc_microsecond(&self.expires_at).ok_or(NativeCheckpointError::Invalid)?;
        let accepted =
            parse_utc_microsecond(&self.accepted_at).ok_or(NativeCheckpointError::Invalid)?;
        let remaining = next.state().execution().window().participants();
        let correct_departure = match next.state().conclusion().map(|end| end.cause()) {
            None => !remaining.contains(&departing.native_seat),
            Some(spellwake_engine::game::GameConclusionCause::Conceded { player }) => {
                *player == departing.native_seat
            }
            _ => false,
        };
        if host.kind != NativeHostInputKind::Clock
            || host.basis.revision.get().checked_add(1) != Some(next.basis().revision.get())
            || host.basis.game_id != next.basis().game_id
            || host.basis.context_id != next.basis().context_id
            || host.basis.epoch != next.basis().epoch
            || host.input_ref != input
            || &self.decision_ref != parent
            || next.roster.len() < 2
            || !correct_departure
            || accepted < expires
        {
            return Err(error);
        }
        Ok(())
    }
}
