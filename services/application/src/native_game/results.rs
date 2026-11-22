//! Derived turn starts and root results, never immediate participation rewards.
//! Exact original reconstruction supplies the operands; no client counts or native
//! Debug strings are accepted. This is result evidence, not a full event codec.
use super::*;
use crate::{match_history::Outcome, player_wire::NativeRevision};
use serde::Serialize;
use spellwake_engine::{
    draw::GameOutcome,
    game::{CandidateBase, GameConclusionCause, TurnTransition},
    turn::{Step, TurnPosition},
};

#[derive(Clone)]
pub(super) struct NativeActivity {
    seats: Vec<(NativeSeatBinding, u64)>,
}
impl NativeActivity {
    pub(super) fn initial(
        candidate: &NativeCandidate,
        roster: &[NativeSeatBinding],
    ) -> Result<Self, NativeCheckpointError> {
        let basis = candidate.state().basis();
        if !matches!(candidate.base(), CandidateBase::Initialize { game, context } if *game == basis.game && *context == basis.context)
            || basis.epoch != 0
            || basis.revision != 0
            || candidate.state().turn().is_some()
            || !candidate.turn_transitions().is_empty()
        {
            return Err(NativeCheckpointError::Invalid);
        }
        Ok(Self {
            seats: roster.iter().map(|seat| (*seat, 0)).collect(),
        })
    }
    pub(super) fn own_turns(&self, seat: Uuid) -> Option<u64> {
        self.seats
            .iter()
            .find(|(binding, _)| binding.seat_id == seat)
            .map(|(_, count)| *count)
    }
    pub(super) fn advance(
        &self,
        prior: &GameState,
        candidate: &NativeCandidate,
    ) -> Result<Self, NativeCheckpointError> {
        let next = candidate.state();
        if !matches!(candidate.base(), CandidateBase::Advance(base) if base == prior.basis())
            || next.basis().game != prior.basis().game
            || next.basis().context != prior.basis().context
            || next.basis().epoch != prior.basis().epoch
            || prior.basis().revision.checked_add(1) != Some(next.basis().revision)
        {
            return Err(NativeCheckpointError::Invalid);
        }
        let prior_window = prior.execution().window();
        let next_window = next.execution().window();
        let prior_position = prior.turn().map(|turn| turn.position());
        let mut active = prior_window.active_player();
        let next_active = next_window.active_player();
        if prior_position.is_none()
            && prior.opening().is_some_and(|opening| !opening.complete())
            && prior_window.participants().contains(&active)
            && !next_window.participants().contains(&active)
        {
            // A departed opening starter has not started a turn. Rebase onto the
            // first surviving seat in the original rotation before counting starts.
            let order = prior_window.original_participants();
            let index = order
                .iter()
                .position(|seat| *seat == active)
                .ok_or(NativeCheckpointError::Invalid)?;
            active = order
                .iter()
                .cycle()
                .skip(index + 1)
                .take(order.len())
                .copied()
                .find(|seat| next_window.participants().contains(seat))
                .ok_or(NativeCheckpointError::Invalid)?;
            if active != next_active {
                return Err(NativeCheckpointError::Invalid);
            }
        }
        self.follow_calendar(
            prior_position,
            active,
            candidate.turn_transitions(),
            next.turn().map(|turn| turn.position()),
            next_active,
        )
    }
    fn follow_calendar(
        &self,
        mut position: Option<TurnPosition>,
        mut active: SeatId,
        transitions: &[TurnTransition],
        next_position: Option<TurnPosition>,
        next_active: SeatId,
    ) -> Result<Self, NativeCheckpointError> {
        if ![active, next_active].iter().all(|player| {
            self.seats
                .iter()
                .any(|(seat, _)| seat.native_seat == *player)
        }) {
            return Err(NativeCheckpointError::Invalid);
        }
        let mut activity = self.clone();
        for transition in transitions {
            if transition.from != position || transition.active_before != active {
                return Err(NativeCheckpointError::Invalid);
            }
            let began = match position {
                None => {
                    if transition.to != first_position(1) || transition.active_after != active {
                        return Err(NativeCheckpointError::Invalid);
                    }
                    true
                }
                Some(previous) if previous.number == transition.to.number => {
                    if previous == transition.to || transition.active_after != active {
                        return Err(NativeCheckpointError::Invalid);
                    }
                    false
                }
                Some(previous) => {
                    if previous.number.checked_add(1) != Some(transition.to.number)
                        || transition.to != first_position(transition.to.number)
                    {
                        return Err(NativeCheckpointError::Invalid);
                    }
                    true
                }
            };
            if began {
                activity.credit_turn(transition.active_after)?;
            }
            position = Some(transition.to);
            active = transition.active_after;
        }
        if position != next_position || active != next_active {
            return Err(NativeCheckpointError::Invalid);
        }
        Ok(activity)
    }
    fn credit_turn(&mut self, active: SeatId) -> Result<(), NativeCheckpointError> {
        let (_, count) = self
            .seats
            .iter_mut()
            .find(|(seat, _)| seat.native_seat == active)
            .ok_or(NativeCheckpointError::Invalid)?;
        *count = count.checked_add(1).ok_or(NativeCheckpointError::Invalid)?;
        Ok(())
    }
    fn rows(&self) -> Vec<TurnStarts> {
        self.seats
            .iter()
            .map(|(seat, count)| TurnStarts {
                seat_id: seat.seat_id,
                own_turns: (*count).into(),
            })
            .collect()
    }
}
fn first_position(number: u64) -> TurnPosition {
    TurnPosition {
        number,
        step: Step::Untap,
        cleanup_iteration: 0,
        additional_damage: false,
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TurnStarts {
    seat_id: Uuid,
    own_turns: NativeRevision,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalResult {
    outcome: Outcome,
    winner_seat_ids: Vec<Uuid>,
    losing_seat_ids: Vec<Uuid>,
    reason: &'static str,
}
/// Actual bound native root conclusion; not a durable result or source admission.
pub struct NativeTerminal {
    basis: PlayerBasis,
    result: TerminalResult,
}
impl NativeTerminal {
    pub(super) fn derive(
        state: &GameState,
        binding: &NativeBinding,
    ) -> Result<Option<Self>, NativeCheckpointError> {
        binding.validate_source(state)?;
        let Some(conclusion) = state.conclusion() else {
            return Ok(None);
        };
        if !(2..=6).contains(&binding.seats.len()) {
            return Err(NativeCheckpointError::Invalid);
        }
        let map = |native| {
            binding
                .seats
                .iter()
                .find(|seat| seat.native_seat == native)
                .map(|seat| seat.seat_id)
                .ok_or(NativeCheckpointError::Invalid)
        };
        let (outcome, winner_seat_ids) = match conclusion.outcome() {
            GameOutcome::Winner(winner) => (Outcome::Win, vec![map(winner)?]),
            GameOutcome::Draw => (Outcome::Draw, vec![]),
        };
        let losing_seat_ids = conclusion
            .losing_players()
            .map(map)
            .collect::<Result<Vec<_>, _>>()?;
        if winner_seat_ids.len() + losing_seat_ids.len() != binding.seats.len() {
            return Err(NativeCheckpointError::Invalid);
        }
        let reason = match conclusion.cause() {
            GameConclusionCause::Conceded { .. } => "native_concession",
            GameConclusionCause::CheckedLosses { .. } => "native_checked_losses",
            GameConclusionCause::WonByEffect { .. } => "native_won_by_effect",
        };
        Ok(Some(Self {
            basis: binding.public_basis.clone(),
            result: TerminalResult {
                outcome,
                winner_seat_ids,
                losing_seat_ids,
                reason,
            },
        }))
    }
    /// Exact native/app basis from which this result was derived.
    pub fn basis(&self) -> &PlayerBasis {
        &self.basis
    }
    /// Native win or draw; never an inferred operational no-contest.
    pub fn outcome(&self) -> Outcome {
        self.result.outcome
    }
    /// Exact mapped native winner set, empty on draw.
    pub fn winner_seat_ids(&self) -> &[Uuid] {
        &self.result.winner_seat_ids
    }
    /// Exact deterministic native losing-player provenance, including concession.
    pub fn losing_seat_ids(&self) -> &[Uuid] {
        &self.result.losing_seat_ids
    }
    /// Stable normalized cause derived directly from the native conclusion.
    pub fn reason(&self) -> &'static str {
        self.result.reason
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Results<'a> {
    version: &'static str,
    basis: &'a PlayerBasis,
    initial_ref: &'a NativeArtifactRef,
    state_ref: &'a NativeArtifactRef,
    terminal: Option<&'a TerminalResult>,
    turn_starts: Vec<TurnStarts>,
}
/// Original private result evidence; preparation is not database acceptance.
pub struct PreparedNativeResults(evidence::PreparedNativeEvidence);
impl PreparedNativeResults {
    /// Computed scoped Facts identity of these exact original bytes.
    pub fn reference(&self) -> &NativeArtifactRef {
        self.0.reference()
    }
    /// Canonical private result evidence; never a player snapshot or event archive.
    pub fn original_bytes(&self) -> &[u8] {
        self.0.original_bytes()
    }
    /// Retain and read back exactly this result evidence in the private Store.
    /// # Errors
    /// Rejects unavailable/private Storage or changed bytes/reference.
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
    /// Exact own turns begun, reconstructed from the full original chain.
    /// AI seat counts do not belong to their sponsor; this grants no rewards.
    pub fn own_turns(&self, seat: Uuid) -> Option<u64> {
        self.activity.own_turns(seat)
    }
    /// Derive an actual root terminal result at this checkpoint's exact basis.
    /// # Errors
    /// Rejects inconsistent binding or a terminal outside the two-player boundary.
    pub fn terminal(&self) -> Result<Option<NativeTerminal>, NativeCheckpointError> {
        let viewer = self
            .roster
            .first()
            .ok_or(NativeCheckpointError::Invalid)?
            .seat_id;
        NativeTerminal::derive(self.state(), &self.binding(viewer)?)
    }
    /// Produce the strict private native-results-v1 evidence from actual operands.
    /// This does not qualify complete feature facts, event journals or clock output.
    /// # Errors
    /// Rejects an inconsistent result, JSON encoding or artifact identity.
    pub fn prepare_results(&self) -> Result<PreparedNativeResults, NativeCheckpointError> {
        let terminal = self.terminal()?;
        let payload = Results {
            version: "native-results-v1",
            basis: self.basis(),
            initial_ref: self.initial_reference(),
            state_ref: self.reference(),
            terminal: terminal.as_ref().map(|value| &value.result),
            turn_starts: self.activity.rows(),
        };
        let bytes = serde_json::to_vec(&payload).map_err(NativeCheckpointError::Json)?;
        Ok(PreparedNativeResults(
            evidence::PreparedNativeEvidence::new(self.basis(), NativeArtifactRole::Facts, bytes)?,
        ))
    }
}

#[derive(Serialize)]
struct ActivityResults<'a> {
    version: &'static str,
    result: &'a serde_json::value::RawValue,
    activity: Vec<super::cast_activity::NativeCastActivity>,
}
impl NativeCheckpoint {
    /// Prepare private all-seat physical-cast delta evidence with unchanged V1
    /// result content. This does not activate or fully qualify any objective.
    /// # Errors
    /// Rejects invalid native attribution, count overflow, encoding or identity.
    pub fn prepare_activity_results(&self) -> Result<PreparedNativeResults, NativeCheckpointError> {
        let original = self.prepare_results()?;
        let result = serde_json::from_slice(original.original_bytes())
            .map_err(NativeCheckpointError::Json)?;
        let payload = ActivityResults {
            version: "native-results-activity-v1",
            result,
            activity: self.cast_activity()?,
        };
        let bytes = serde_json::to_vec(&payload).map_err(NativeCheckpointError::Json)?;
        Ok(PreparedNativeResults(
            evidence::PreparedNativeEvidence::new(self.basis(), NativeArtifactRole::Facts, bytes)?,
        ))
    }
    pub(super) fn prepare_results_schema(
        &self,
        schema: crate::admission_ports::NativeEventSchema,
    ) -> Result<PreparedNativeResults, NativeCheckpointError> {
        match schema {
            crate::admission_ports::NativeEventSchema::TransitionV1 => self.prepare_results(),
            crate::admission_ports::NativeEventSchema::ActivityV1 => {
                self.prepare_activity_results()
            }
        }
    }
}

#[cfg(test)]
mod tests;
