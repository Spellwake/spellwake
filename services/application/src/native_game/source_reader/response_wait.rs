//! Current uninterrupted multiplayer opening wait, observed on the database clock.
use super::originals::decode_error;
use super::*;
use crate::{
    admission_ports::HostCandidate, native_game::input_codec::PreparedNativeResponseTimeout,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wait {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    decision_ref: NativeArtifactRef,
    policy: String,
    expires_at: String,
    due: bool,
}
/// A service-role observation at a specific native opening checkpoint.
/// Only a currently due response can be proposed through this capability.
pub(super) struct NativeOpeningResponseWait(Wait);
impl NativeOpeningResponseWait {
    /// Whether this exact accepted cut still owns a due response.
    pub(super) fn due_at(&self, checkpoint: &NativeCheckpoint) -> bool {
        self.0.due && checkpoint.reference() == &self.0.decision_ref
    }
    /// Match the exact native chooser and propose the seat's departure.
    /// # Errors
    /// Rejects an early observation, foreign checkpoint, or unsupported response.
    pub(super) fn prepare(
        &self,
        checkpoint: &NativeCheckpoint,
    ) -> Result<PreparedNativeResponseTimeout, NativeSourceError> {
        if !self.due_at(checkpoint) || checkpoint.opening_response_seat()? != Some(self.0.seat_id) {
            return Err(invalid());
        }
        let binding = checkpoint
            .binding(self.0.seat_id)
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e)))?;
        let scope = NativeScope::new(checkpoint.state(), binding)
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e.into())))?;
        scope
            .prepare_response_timeout(self.0.seat_id)
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e.into())))
    }

    pub(super) fn frame(
        &self,
        source: &RecoveredNativeGame,
        next: &NativeCheckpoint,
        base: &HostCandidate,
    ) -> Result<HostCandidate, NativeSourceError> {
        let (kind, parent, input) = next.retained_input().ok_or_else(invalid)?;
        if !matches!(kind, NativeCheckpointInput::ResponseTimeout) || parent != &self.0.decision_ref
        {
            return Err(invalid());
        }
        let proposal = self.prepare(source.checkpoint())?;
        let expected_input = NativeArtifactRef::for_bytes(
            next.basis().game_id,
            next.basis().context_id,
            NativeArtifactRole::Input,
            proposal.record(),
        )
        .map_err(|e| NativeSourceError(Failure::Checkpoint(e.into())))?;
        if input != &expected_input || next.candidate() != proposal.candidate() {
            return Err(invalid());
        }
        let mut frame: serde_json::Value =
            serde_json::from_str(base.original_json()).map_err(decode_error)?;
        frame["nativeEliminations"] = json!([{"seatId":self.0.seat_id,"sourceRevision":source.checkpoint().basis().revision,
            "reason":"native_response_timeout","decisionRef":format!("multiplayer-response-v1:{}",parent.as_str()),"expiryRef":input,"expiredAt":self.0.expires_at}]);
        HostCandidate::from_bytes(&serde_json::to_vec(&frame).map_err(decode_error)?).map_err(
            |error| {
                NativeSourceError(Failure::Backend(BackendError::caused(
                    "response timeout frame",
                    error,
                )))
            },
        )
    }
}

impl NativeCheckpoint {
    /// Identify only a live, supported opening choice on an originally multiplayer table.
    pub(super) fn opening_response_seat(&self) -> Result<Option<Uuid>, NativeSourceError> {
        if self.state().outcome().is_some() {
            return Ok(None);
        }
        let Some(choice) = self.state().pending_decision() else {
            return Ok(None);
        };
        if self.roster.len() != 3
            || !matches!(
                choice.frame(),
                spellwake_engine::game::RuleFrame::Opening(_)
            )
        {
            return Ok(None);
        }
        if choice.basis() != self.state().basis() {
            return Err(invalid());
        }
        let seat = self
            .roster
            .iter()
            .find(|s| s.native_seat == choice.chooser())
            .ok_or_else(invalid)?;
        Ok(Some(seat.seat_id))
    }
}

impl NativeSourceRepository {
    /// Read the database-owned deadline at an uninterrupted three-to-six-seat opening.
    /// # Errors
    /// Rejects malformed, foreign or inconsistent observations and transport failures.
    pub(super) fn opening_response_wait(
        &self,
        game: Uuid,
    ) -> Result<Option<NativeOpeningResponseWait>, NativeSourceError> {
        if game.is_nil() {
            return Err(invalid());
        }
        let response: ObjectResponse<Wait> = self.native_record(
            "read_native_opening_response_wait",
            &json!({"p_game":game}),
            None,
        )?;
        decode_wait(game, response)
    }
}

fn decode_wait(
    game: Uuid,
    response: ObjectResponse<Wait>,
) -> Result<Option<NativeOpeningResponseWait>, NativeSourceError> {
    match response.0 {
        ApiResponse::Success { data }
            if data.policy == "multiplayer-response-v1"
                && data.decision_ref.game_id() == game
                && data.decision_ref.role() == NativeArtifactRole::State
                && parse_utc_microsecond(&data.expires_at).is_some() =>
        {
            Ok(Some(NativeOpeningResponseWait(data)))
        }
        ApiResponse::Fail { data }
            if data.len() == 1 && data.get("game").is_some_and(|v| v == "unavailable") =>
        {
            Ok(None)
        }
        _ => Err(invalid()),
    }
}
