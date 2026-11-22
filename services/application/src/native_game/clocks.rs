//! Actual native waiting ownership and private clock intent. SQL owns wall time.
use super::*;
use crate::{
    admission_ports::OpeningClockObservation, amount::Amount, match_history::Availability,
};
use serde::Serialize;
use spellwake_engine::{
    checks::CheckDecision, effects::PendingWorkKind, game::RuleFrame, priority::PriorityPhase,
    stack_window::WindowPhase,
};

#[derive(Clone, Copy)]
pub(super) enum Waiting {
    Terminal,
    Decision(Uuid),
    Priority(Uuid),
    Blocked,
}
/// Keep clock eligibility aligned with the maintained public decision families.
pub(super) fn supported_wait(source: &GameState) -> Result<(), NativeBridgeError> {
    let searching = matches!(source.pending_decision().map(|decision|decision.frame()), Some(RuleFrame::LibraryTop(frame)) if frame.kind() == spellwake_engine::effects::LibraryChoiceKind::Search);
    if (source.execution().pending_work_kind() == Some(PendingWorkKind::LibraryTop) && !searching)
        || source.execution().has_check_replacement_work()
    {
        return Err(NativeBridgeError::Unsupported);
    }
    Ok(())
}
pub(super) fn waiting(
    source: &GameState,
    binding: &NativeBinding,
) -> Result<Waiting, NativeBridgeError> {
    binding.validate_source(source)?;
    if source.outcome().is_some() {
        return Ok(Waiting::Terminal);
    }
    if !source.pending_eliminations().is_empty() {
        return Err(NativeBridgeError::Unsupported);
    }
    supported_wait(source)?;
    let seat = |native| {
        binding
            .seats
            .iter()
            .find(|seat| seat.native_seat == native)
            .map(|seat| seat.seat_id)
            .ok_or(NativeBridgeError::InvalidBinding)
    };
    if let Some(decision) = source.pending_decision() {
        // Clock eligibility must not charge a chooser for an unsupported frame.
        // Public projection separately checks the viewer before its frame map.
        if matches!(
            decision.frame(),
            RuleFrame::Checks(CheckDecision::CleanupSelect(_))
        ) {
            return Err(NativeBridgeError::Unsupported);
        }
        if matches!(decision.frame(), RuleFrame::LibraryTop(frame) if frame.kind() != spellwake_engine::effects::LibraryChoiceKind::Search)
        {
            return Err(NativeBridgeError::Unsupported);
        }
        if decision.basis() != source.basis() {
            return Err(NativeBridgeError::InvalidBinding);
        }
        return Ok(Waiting::Decision(seat(decision.chooser())?));
    }
    if source.pending_turn_work().is_some() || source.execution().pending_work_kind().is_some() {
        return Ok(Waiting::Blocked);
    }
    match source.execution().window().phase() {
        WindowPhase::Priority(PriorityPhase::Ready { holder }) => {
            Ok(Waiting::Priority(seat(holder)?))
        }
        _ => Ok(Waiting::Blocked),
    }
}
pub(super) fn initial_waiter(
    source: &GameState,
    binding: &NativeBinding,
) -> Result<Uuid, NativeBridgeError> {
    if source.basis().revision != 0
        || !matches!(source.execution().window().phase(), WindowPhase::Opening)
        || !source
            .pending_decision()
            .is_some_and(|decision| matches!(decision.frame(), RuleFrame::Opening(_)))
    {
        return Err(NativeBridgeError::Unsupported);
    }
    match waiting(source, binding)? {
        Waiting::Decision(seat) => Ok(seat),
        _ => Err(NativeBridgeError::Unsupported),
    }
}
impl NativeCheckpoint {
    /// Derive the exact original opening decision observation without a native input.
    /// # Errors
    /// Rejects noninitial checkpoints, unsupported/terminal roots and inconsistent
    /// native roster/basis. Retain the checkpoint before durable acceptance.
    pub fn opening_clock_observation(
        &self,
    ) -> Result<OpeningClockObservation, NativeCheckpointError> {
        if self.retained_input().is_some() {
            return Err(NativeCheckpointError::Invalid);
        }
        let viewer = self
            .roster
            .first()
            .ok_or(NativeCheckpointError::Invalid)?
            .seat_id;
        let binding = self.binding(viewer)?;
        let seat = initial_waiter(self.state(), &binding)?;
        OpeningClockObservation::from_native(seat, self.reference().as_str())
            .map_err(|_| NativeCheckpointError::Invalid)
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClockEligibility {
    pub(crate) reason: &'static str,
    pub(crate) accepted_revision: Amount,
    pub(crate) proof_ref: NativeArtifactRef,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClockIntentRow {
    pub(crate) seat_id: Uuid,
    pub(crate) clock_revision: Amount,
    pub(crate) remaining_ms: Amount,
    pub(crate) paused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) eligibility: Option<ClockEligibility>,
}
/// Closed private clock array derived from an actual accepted source/successor.
/// It grants no lease, admission, expiration authority or caller-owned timestamp.
#[derive(Serialize)]
#[serde(transparent)]
pub struct NativeClockIntent(Vec<ClockIntentRow>);
impl NativeClockIntent {
    pub(crate) fn rows(&self) -> &[ClockIntentRow] {
        &self.0
    }
}
#[derive(Clone, Copy)]
struct ObservedBank<'a> {
    seat: Uuid,
    remaining: &'a Amount,
    revision: &'a Amount,
    paused: bool,
    policy: &'a str,
}
fn bank_intent(
    bank: ObservedBank<'_>,
    actor: Waiting,
    prior: &Amount,
    state: &NativeArtifactRef,
) -> Result<ClockIntentRow, NativeCheckpointError> {
    let (paused, eligibility) = match bank.policy {
        "bo1-bank-inactive-v1" if bank.remaining.is_zero() && bank.paused => (true, None),
        "bo3-overall-bank-v1" if !bank.remaining.is_zero() => {
            let reason = match actor {
                Waiting::Decision(owner) if owner == bank.seat => Some("native_decision"),
                Waiting::Priority(owner) if owner == bank.seat => Some("native_priority"),
                _ => None,
            };
            let eligibility =
                if reason.is_some() || !bank.paused && !matches!(actor, Waiting::Terminal) {
                    Some(ClockEligibility {
                        reason: reason.unwrap_or("native_wait"),
                        accepted_revision: prior.clone(),
                        proof_ref: state.clone(),
                    })
                } else {
                    None
                };
            (reason.is_none(), eligibility)
        }
        _ => {
            return Err(NativeCheckpointError::Binding(
                NativeBridgeError::Unsupported,
            ));
        }
    };
    Ok(ClockIntentRow {
        seat_id: bank.seat,
        clock_revision: bank.revision.clone(),
        remaining_ms: bank.remaining.clone(),
        paused,
        eligibility,
    })
}
impl RecoveredNativeGame {
    /// Compose exact clock intent for a genuine linked player/entropy successor.
    /// Stored banks and clock revisions are copied unchanged. Only SQL computes
    /// elapsed debit at its locked acceptance timestamp.
    /// # Errors
    /// Rejects foreign/unlinked bases, unavailable source, unsupported native
    /// waiting boundaries, unknown policies and already exhausted BO3 banks.
    pub fn clock_intent(
        &self,
        next: &NativeCheckpoint,
    ) -> Result<NativeClockIntent, NativeCheckpointError> {
        let prior = self.checkpoint();
        let source = self.source();
        let basis = prior.basis();
        let (_, parent, _) = next
            .retained_input()
            .ok_or(NativeCheckpointError::Invalid)?;
        if source.game_id() != basis.game_id
            || source.context_ids().0 != basis.context_id
            || source.accepted_revision() != &Amount::from(basis.revision.get())
            || source.match_availability() != Availability::Available
            || source.operational_basis().3 != Availability::Available
            || parent != prior.reference()
            || next.initial_reference() != prior.initial_reference()
            || next.basis().game_id != basis.game_id
            || next.basis().context_id != basis.context_id
            || next.basis().epoch != basis.epoch
            || basis.revision.get().checked_add(1) != Some(next.basis().revision.get())
        {
            return Err(NativeCheckpointError::Invalid);
        }
        let viewer = next
            .roster
            .first()
            .ok_or(NativeCheckpointError::Invalid)?
            .seat_id;
        let actor = waiting(next.state(), &next.binding(viewer)?)?;
        let mut rows = Vec::with_capacity(source.clocks().len());
        for (seat, remaining, revision, was_paused, _) in source.clocks() {
            let policy = source
                .clock_policy(seat)
                .ok_or(NativeCheckpointError::Invalid)?;
            rows.push(bank_intent(
                ObservedBank {
                    seat,
                    remaining,
                    revision,
                    paused: was_paused,
                    policy,
                },
                actor,
                &Amount::from(basis.revision.get()),
                next.reference(),
            )?);
        }
        if rows.len() != next.roster.len()
            || rows
                .iter()
                .any(|row| !next.roster.iter().any(|seat| seat.seat_id == row.seat_id))
        {
            return Err(NativeCheckpointError::Invalid);
        }
        Ok(NativeClockIntent(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admission_ports::HostCandidate;
    use serde_json::{Value, json};
    #[test]
    fn running_bank_has_distinct_blocked_and_terminal_ending_evidence() {
        let seat = Uuid::from_u128(3);
        let remaining = Amount::from(1700000);
        let revision = Amount::from(5);
        let prior = Amount::from(4);
        let state = NativeArtifactRef::for_bytes(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            NativeArtifactRole::State,
            b"fixture-next-state",
        )
        .unwrap();
        let bank = ObservedBank {
            seat,
            remaining: &remaining,
            revision: &revision,
            paused: false,
            policy: "bo3-overall-bank-v1",
        };
        let blocked = bank_intent(bank, Waiting::Blocked, &prior, &state).unwrap();
        let terminal = bank_intent(bank, Waiting::Terminal, &prior, &state).unwrap();
        assert!(blocked.paused && terminal.paused);
        let evidence = blocked.eligibility.as_ref().unwrap();
        assert_eq!(evidence.reason, "native_wait");
        assert_eq!(evidence.accepted_revision, prior);
        assert!(evidence.proof_ref == state);
        assert!(terminal.eligibility.is_none());
        assert_eq!(blocked.remaining_ms, remaining);
        assert_eq!(terminal.remaining_ms, remaining);
        assert_eq!(blocked.clock_revision, revision);
        assert_eq!(terminal.clock_revision, revision);
        let paused = bank_intent(
            ObservedBank {
                paused: true,
                ..bank
            },
            Waiting::Blocked,
            &prior,
            &state,
        )
        .unwrap();
        assert!(paused.eligibility.is_none());
        assert!(
            bank_intent(
                ObservedBank {
                    policy: "future-policy-v2",
                    ..bank
                },
                Waiting::Blocked,
                &prior,
                &state
            )
            .is_err()
        );
    }
    #[test]
    fn native_clock_match_rejects_fabricated_owner_bank_proof_and_timeout_fields() {
        let game = Uuid::from_u128(1);
        let context = Uuid::from_u128(2);
        let seat = Uuid::from_u128(3);
        let state = NativeArtifactRef::for_bytes(
            game,
            context,
            NativeArtifactRole::State,
            b"fixture-state",
        )
        .unwrap();
        let intent = NativeClockIntent(vec![ClockIntentRow {
            seat_id: seat,
            clock_revision: Amount::from(5),
            remaining_ms: Amount::from(1700000),
            paused: false,
            eligibility: Some(ClockEligibility {
                reason: "native_decision",
                accepted_revision: Amount::from(4),
                proof_ref: state.clone(),
            }),
        }]);
        let valid = json!({"contextId":context,"stateRef":state,"factsRef":"fixture:facts","events":["fixture:event"],"facts":[],"clocks":intent,"participation":[]});
        let parse =
            |value: &Value| HostCandidate::from_bytes(&serde_json::to_vec(value).unwrap()).unwrap();
        assert!(parse(&valid).matches_native_clocks(&intent));
        assert!(parse(&valid).valid_for(&Amount::from(4)));
        for (key, value) in [
            ("seatId", json!(Uuid::from_u128(9))),
            ("remainingMs", json!("1800000")),
            ("clockRevision", json!("6")),
            ("paused", json!(true)),
        ] {
            let mut bad = valid.clone();
            bad["clocks"][0][key] = value;
            assert!(!parse(&bad).matches_native_clocks(&intent));
        }
        for (key, value) in [
            ("reason", json!("native_priority")),
            ("acceptedRevision", json!("5")),
            ("proofRef", json!("fixture:other")),
        ] {
            let mut bad = valid.clone();
            bad["clocks"][0]["eligibility"][key] = value;
            assert!(!parse(&bad).matches_native_clocks(&intent));
        }
        let mut bad = valid.clone();
        bad["nativeEliminations"] = json!([]);
        assert!(!parse(&bad).matches_native_clocks(&intent));
        bad = valid.clone();
        bad["bankTimeout"] = json!({"kind":"overall_bank","seatId":seat,"clockRevision":"5","proofRef":"fixture:expiry"});
        assert!(!parse(&bad).matches_native_clocks(&intent));
    }
}
