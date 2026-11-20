//! Default individual-turn action timing (CR 116.2a, 117.1a, 305.1–4, 307.1).
//! No flash, timing permissions/prohibitions or dynamic land allowances are inferred.
use crate::{
    object_identity::GameObject,
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackWindow, WindowPhase},
    turn::{Step, TurnScheduler, TurnStage},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingError {
    TurnRequired,
    PriorityRequired,
    NotPriorityHolder,
    NotActivePlayer,
    MainPhaseRequired,
    StackNotEmpty,
}

/// Reads the actual owned state, never an action request's asserted permission.
pub(crate) fn validate_main_phase(
    window: &StackWindow,
    turn: Option<&TurnScheduler>,
    actor: SeatId,
) -> Result<(), TimingError> {
    match window.phase() {
        WindowPhase::Priority(PriorityPhase::Ready { holder }) if holder == actor => {}
        WindowPhase::Priority(PriorityPhase::Ready { .. }) => {
            return Err(TimingError::NotPriorityHolder);
        }
        _ => return Err(TimingError::PriorityRequired),
    }
    let turn = turn.ok_or(TimingError::TurnRequired)?;
    if window.turn_owner() != actor {
        return Err(TimingError::NotActivePlayer);
    }
    if turn.stage() != TurnStage::Priority
        || !matches!(
            turn.position().step,
            Step::PrecombatMain | Step::PostcombatMain
        )
    {
        return Err(TimingError::MainPhaseRequired);
    }
    if !window.entries().is_empty() {
        return Err(TimingError::StackNotEmpty);
    }
    Ok(())
}

/// Privileged current-candidate fact, not a durable or seat-safe event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandPlayRecord {
    pub player: SeatId,
    pub before: GameObject,
    pub after: GameObject,
}

/// Immediate physical entry, before supported state checks may remove the
/// permanent. This is current native evidence, not a public or durable event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentEntryRecord {
    pub entry: StackEntryId,
    pub before: GameObject,
    pub after: GameObject,
}
