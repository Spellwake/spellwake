//! Current uninterrupted multiplayer opening wait, observed on the database clock.
use super::originals::decode_error;
use super::*;
use crate::admission_ports::{HostCandidate, TurnResponseProtocol};

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
pub struct NativeOpeningResponseWait(Wait);
impl NativeOpeningResponseWait {
    /// Immutable server deadline for scheduling a future check.
    pub fn expires_at(&self) -> &str {
        &self.0.expires_at
    }
    pub(super) fn deadline_for(
        &self,
        checkpoint: &NativeCheckpoint,
        viewer: Uuid,
    ) -> Result<&str, NativeSourceError> {
        if self.0.seat_id != viewer || &self.0.decision_ref != checkpoint.reference() {
            return Err(invalid());
        }
        Ok(self.expires_at())
    }
    /// Whether the observation reached the database deadline.
    pub fn due(&self) -> bool {
        self.0.due
    }
    /// Match the exact native chooser and propose the seat's departure.
    /// # Errors
    /// Rejects an early observation, foreign checkpoint, or unsupported response.
    pub fn prepare(
        &self,
        checkpoint: &NativeCheckpoint,
    ) -> Result<PreparedNativeClockExpiry, NativeSourceError> {
        if !self.due()
            || checkpoint.reference() != &self.0.decision_ref
            || checkpoint.response_seat()? != Some(self.0.seat_id)
        {
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

/// Rebinding an opaque response handle does not alone identify a new rules choice.
#[derive(PartialEq, Eq)]
pub(super) struct LogicalWait {
    holder: SeatId,
    position: Option<spellwake_engine::turn::TurnPosition>,
    frame: Option<Box<spellwake_engine::game::RuleFrame>>,
}
impl NativeCheckpoint {
    pub(super) fn turn_response_protocol(
        &self,
    ) -> Option<crate::admission_ports::TurnResponseProtocol> {
        (self.roster.len() >= 3).then_some(TurnResponseProtocol::V2)
    }
    pub(super) fn logical_wait(&self) -> Option<LogicalWait> {
        use spellwake_engine::{priority::PriorityPhase, stack_window::WindowPhase};
        let state = self.state();
        let (holder, frame) = if let Some(decision) = state.pending_decision() {
            (decision.chooser(), Some(Box::new(decision.frame().clone())))
        } else if let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        {
            (holder, None)
        } else {
            return None;
        };
        Some(LogicalWait {
            holder,
            frame,
            position: state.turn().map(|turn| turn.position()),
        })
    }
    pub(super) fn turn_response_after(
        &self,
        previous: Option<&LogicalWait>,
    ) -> Result<Option<crate::admission_ports::TurnResponse>, NativeSourceError> {
        self.turn_response_for(previous, TurnResponseProtocol::V2)
    }

    pub(super) fn turn_response_for(
        &self,
        previous: Option<&LogicalWait>,
        protocol: TurnResponseProtocol,
    ) -> Result<Option<crate::admission_ports::TurnResponse>, NativeSourceError> {
        let Some(mut wait) = self.turn_response()? else {
            return Ok(None);
        };
        let current = self.logical_wait().ok_or_else(invalid)?;
        if protocol == TurnResponseProtocol::V1 {
            // SQL255's writer could expire the final two seats at any waiting
            // cut. With three or more survivors its combat support was narrower.
            let execution = self.state().execution();
            let cleanup_work = match current.frame.as_deref() {
                Some(spellwake_engine::game::RuleFrame::Checks(
                    spellwake_engine::checks::CheckDecision::Legend(f),
                )) => matches!(
                    f.boundary(),
                    spellwake_engine::checks::CheckBoundary::Cleanup(_)
                ),
                Some(spellwake_engine::game::RuleFrame::ZoneReplacement(_)) => {
                    execution.has_check_replacement_work()
                        && current
                            .position
                            .is_some_and(|pos| pos.step == spellwake_engine::turn::Step::Cleanup)
                        && current.holder == execution.window().turn_owner()
                }
                _ => false,
            };
            if wait.remaining_seat_ids.len() > 2
                && (cleanup_work
                    || matches!(
                        current.frame.as_deref(),
                        Some(
                            spellwake_engine::game::RuleFrame::Combat(_)
                                | spellwake_engine::game::RuleFrame::Checks(
                                    spellwake_engine::checks::CheckDecision::GraveyardOrder(_)
                                )
                        )
                    )
                    || execution.combat().is_some_and(|combat| {
                        current.frame.is_some()
                            || execution.window().turn_owner() == current.holder
                            || combat.attackers().any(|a| {
                                a.combatant().controller() == current.holder
                                    || a.defender() == current.holder
                                        && a.block_status()
                                            == spellwake_engine::combat::BlockStatus::NotDeclared
                            })
                    }))
            {
                return Ok(None);
            }
            wait.same_native_wait = previous == Some(&current);
        } else {
            wait.same_native_wait = previous.is_some_and(|old| {
                old.holder == current.holder
                    && old.position == current.position
                    && match (old.frame.as_deref(), current.frame.as_deref()) {
                        (Some(before), Some(after)) => before.same_response_obligation(after),
                        (None, None) => true,
                        _ => false,
                    }
            });
        }
        Ok(Some(wait))
    }
    pub(super) fn turn_response(
        &self,
    ) -> Result<Option<crate::admission_ports::TurnResponse>, NativeSourceError> {
        use crate::admission_ports::{TurnResponse, TurnResponseKind};
        use crate::native_game::clocks::{Waiting, waiting};
        if self.roster.len() < 3 || self.opening_response_seat()?.is_some() {
            return Ok(None);
        }
        let binding = self
            .binding(self.roster[0].seat_id)
            .map_err(|e| NativeSourceError(Failure::Checkpoint(e)))?;
        let (kind, seat_id) = match waiting(self.state(), &binding) {
            Ok(Waiting::Decision(seat)) => (TurnResponseKind::Decision, seat),
            Ok(Waiting::Priority(seat)) => (TurnResponseKind::Priority, seat),
            Ok(Waiting::Terminal | Waiting::Blocked) | Err(NativeBridgeError::Unsupported) => {
                return Ok(None);
            }
            Err(e) => return Err(NativeSourceError(Failure::Checkpoint(e.into()))),
        };
        // A public deadline may fence a real player action. Issue it only if
        // the engine can also produce the departure at this exact immutable root.
        let native = self
            .roster
            .iter()
            .find(|seat| seat.seat_id == seat_id)
            .ok_or_else(invalid)?
            .native_seat;
        match self.state().expire_response(self.state().basis(), native) {
            Ok(_) => {}
            Err(
                GameError::UnsupportedConcessionScope | GameError::UnsupportedResponseExpiryScope,
            ) => return Ok(None),
            Err(error) => {
                return Err(NativeSourceError(Failure::Checkpoint(
                    NativeBridgeError::Native(Box::new(error)).into(),
                )));
            }
        }
        let participants = self.state().execution().window().participants();
        let remaining_seat_ids = self
            .roster
            .iter()
            .filter(|seat| participants.contains(&seat.native_seat))
            .map(|seat| seat.seat_id)
            .collect();
        Ok(Some(TurnResponse {
            kind,
            seat_id,
            remaining_seat_ids,
            same_native_wait: false,
        }))
    }
    pub(super) fn response_seat(&self) -> Result<Option<Uuid>, NativeSourceError> {
        if let Some(seat) = self.opening_response_seat()? {
            return Ok(Some(seat));
        }
        Ok(self.turn_response()?.map(|wait| wait.seat_id))
    }
    /// Identify only a live, supported opening choice on an originally multiplayer table.
    pub(super) fn opening_response_seat(&self) -> Result<Option<Uuid>, NativeSourceError> {
        let Some(choice) = self.state().pending_decision() else {
            return Ok(None);
        };
        if self.roster.len() < 3
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
    pub fn opening_response_wait(
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

    /// A trusted worker can start a clock on a recovered pre-protocol checkpoint.
    /// SQL owns the adoption instant and rechecks the host fence under the game lock.
    pub(super) fn adopt_legacy_response(
        &self,
        source: &RecoveredNativeGame,
        owner: &NativeHostOwner,
    ) -> Result<Option<NativeOpeningResponseWait>, NativeSourceError> {
        use crate::admission_ports::TurnResponseKind;
        let checkpoint = source.checkpoint();
        // Recovery verified the retained native state. Starting a prospective clock
        // makes no assertion about event codecs absent from older history.
        let (kind, seat) = if let Some(seat) = checkpoint.opening_response_seat()? {
            ("opening", seat)
        } else if let Some(wait) = checkpoint.turn_response()? {
            (
                match wait.kind {
                    TurnResponseKind::Priority => "priority",
                    TurnResponseKind::Decision => "decision",
                },
                wait.seat_id,
            )
        } else {
            return Ok(None);
        };
        let living = checkpoint.state().execution().window().participants();
        let remaining: Vec<_> = checkpoint
            .roster
            .iter()
            .filter(|s| living.contains(&s.native_seat))
            .map(|s| s.seat_id)
            .collect();
        let game = checkpoint.basis().game_id;
        let (observed, fence, _, _, _) = source.source().operational_basis();
        if observed != owner.as_str() {
            return Err(invalid());
        }
        let response = self.native_record("adopt_native_response_wait", &json!({
            "p_game":game,"p_checkpoint":source.source().checkpoint_ids().0,"p_state":checkpoint.reference(),
            "p_owner":owner.as_str(),"p_fence":fence,"p_runtime":source.opening().runtime_id(),
            "p_programs":source.opening().program_source_id(),"p_kind":kind,"p_seat":seat,"p_remaining":remaining,
        }), None)?;
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
