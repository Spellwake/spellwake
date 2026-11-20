//! Single-source input data for private qualification and read-only projection.
//! This file contains no engine mutation, JSON state restore, dispatch or seat/entry operands.

/// A closed, input-only related-player cause fault selected by the private harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelatedPlayerCauseFault {
    /// Remove the two actual retained members, preserving event/key metadata.
    EmptyCauseSet,
    /// Remove only member0's related-player value, retaining both real IDs.
    MissingRelatedPlayer,
    /// Set both real members to one derived nonparticipant.
    ForeignParticipant,
    /// Change only member1's player to a distinct actual captured controller.
    AmbiguousParticipants,
}

/// Rejected preconditions for the cfg(test)-only qualification operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelatedPlayerFaultSetupError {
    /// The source lacks the exact current Controller/free gate and root authority.
    WrongGate,
    /// The current resolving entry has no matching actual trigger/source binding.
    WrongBinding,
    /// The actual binding does not contain exactly two members; identity checks follow.
    InsufficientCauseMembers,
    /// Original members/event/key/actual participant relation is invalid.
    InvalidOriginalCauseSet,
    /// The captured controller is not an actual participant distinct from A.
    ControllerNotDistinctParticipant,
    /// Checked generic seat derivation found no unused representable SeatId.
    /// This defensive branch is unreachable for the fixed three-seat export.
    NoForeignSeat,
}
