//! Native frames and current-operation evidence for supported checks and cleanup.
//!
//! The executor owns the resumable process and all live game state. These values
//! are privileged native data, not public views, durable replay or complete LKI.

use spellwake_contracts::names::EnglishCardName;
use std::collections::BTreeSet;

use crate::{
    object_identity::{GameObject, ObjectId},
    priority::SeatId,
    turn::TurnPosition,
};

/// Parent boundary to which the actual check result returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckBoundary {
    Priority,
    /// The exact position includes the current cleanup repetition.
    Cleanup(TurnPosition),
}

/// Local event identity within one bound executor/game history.
///
/// Priority starts its first state-based-action event at pass 0. Cleanup reserves
/// pass 0 for discard and starts its first state-based-action event at pass 1,
/// even when no discard is needed. No-excess cleanup emits no discard record.
/// Further state-based-action events increment the pass with checked arithmetic.
/// All APNAP owner replies for one simultaneous event retain the same identity.
/// Exhausting the pass is an operational error, never a fabricated successor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CheckEventId {
    pub(crate) started_revision: u64,
    pub(crate) pass: u64,
}

impl CheckEventId {
    /// Executor revision at the start of the containing check process.
    pub fn started_revision(&self) -> u64 {
        self.started_revision
    }

    /// Event slot within that process, following the documented schedule.
    pub fn pass(&self) -> u64 {
        self.pass
    }
}

/// Immutable executor-derived domain for the active player's cleanup discard.
///
/// A reply selects exactly `required` distinct current hand objects. Under
/// CleanupDiscard it also supplies final top-to-bottom Graveyard order; under
/// CleanupSelect it supplies original identities before separate final ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupDiscardChoice {
    pub(crate) player: SeatId,
    pub(crate) revision: u64,
    pub(crate) boundary: CheckBoundary,
    pub(crate) event: CheckEventId,
    pub(crate) domain: Vec<ObjectId>,
    pub(crate) required: usize,
}

impl CleanupDiscardChoice {
    pub fn chooser(&self) -> SeatId {
        self.player
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn boundary(&self) -> CheckBoundary {
        self.boundary
    }

    pub fn event(&self) -> CheckEventId {
        self.event
    }

    /// Complete eligible exact-current-object domain, without an implied order.
    pub fn domain(&self) -> &[ObjectId] {
        &self.domain
    }

    /// Exact finite physical card count required by this choice.
    pub fn required(&self) -> usize {
        self.required
    }
}

/// Immutable executor-derived permutation domain for one owner's simultaneous
/// graveyard arrivals. The reply gives final top-to-bottom entrant order above
/// the unchanged old pile; it cannot reorder the old pile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraveyardOrderChoice {
    pub(crate) player: SeatId,
    pub(crate) revision: u64,
    pub(crate) boundary: CheckBoundary,
    pub(crate) event: CheckEventId,
    pub(crate) domain: Vec<ObjectId>,
}

impl GraveyardOrderChoice {
    pub fn chooser(&self) -> SeatId {
        self.player
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn boundary(&self) -> CheckBoundary {
        self.boundary
    }

    pub fn event(&self) -> CheckEventId {
        self.event
    }

    /// Complete exact-current-object permutation domain, without an implied order.
    pub fn domain(&self) -> &[ObjectId] {
        &self.domain
    }
}

/// A conflict identity scoped to one event, controller and frozen cohort.
/// It is neither a physical object nor a card/name identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LegendConflictId {
    pub(crate) event: CheckEventId,
    pub(crate) controller: SeatId,
    pub(crate) ordinal: u64,
}
impl LegendConflictId {
    /// Owning simultaneous check event.
    pub fn event(&self) -> CheckEventId {
        self.event
    }
    /// Controller whose frozen cohort this identity describes.
    pub fn controller(&self) -> SeatId {
        self.controller
    }
    /// Deterministic zero-based index among this controller's maximal cohorts.
    pub fn ordinal(&self) -> u64 {
        self.ordinal
    }
}

/// One immutable maximal common-name cohort at the simultaneous check cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegendConflict {
    pub(crate) id: LegendConflictId,
    pub(crate) comparison_names: BTreeSet<EnglishCardName>,
    pub(crate) domain: Vec<ObjectId>,
}
impl LegendConflict {
    /// Context-bound identity of this frozen cohort.
    pub fn id(&self) -> LegendConflictId {
        self.id
    }
    /// Normalized names explaining this cohort, not the objects' actual names.
    pub fn comparison_names(&self) -> &BTreeSet<EnglishCardName> {
        &self.comparison_names
    }
    /// Full frozen domain, including objects already doomed by other actions.
    pub fn domain(&self) -> &[ObjectId] {
        &self.domain
    }
}

/// All outstanding conflicts of the next APNAP controller. The player chooses
/// which conflict to answer first and one exact object to keep in that conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegendChoice {
    pub(crate) player: SeatId,
    pub(crate) revision: u64,
    pub(crate) boundary: CheckBoundary,
    pub(crate) event: CheckEventId,
    pub(crate) conflicts: Vec<LegendConflict>,
}
impl LegendChoice {
    /// Next APNAP controller whose unresolved conflicts are offered.
    pub fn chooser(&self) -> SeatId {
        self.player
    }
    /// Exact executor revision binding this immutable frame.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Parent priority or cleanup boundary of the check process.
    pub fn boundary(&self) -> CheckBoundary {
        self.boundary
    }
    /// Simultaneous check event shared by all these frozen conflicts.
    pub fn event(&self) -> CheckEventId {
        self.event
    }
    /// Every still-unanswered original cohort for this controller.
    pub fn conflicts(&self) -> &[LegendConflict] {
        &self.conflicts
    }
}

/// Readonly native check boundary. A player reply supplies choices only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckDecision {
    CleanupDiscard(CleanupDiscardChoice),
    /// Replacement-aware hand selection; its vector is not a final Graveyard order.
    CleanupSelect(CleanupDiscardChoice),
    GraveyardOrder(GraveyardOrderChoice),
    Legend(LegendChoice),
}

impl CheckDecision {
    pub fn chooser(&self) -> SeatId {
        match self {
            Self::CleanupDiscard(frame) | Self::CleanupSelect(frame) => frame.chooser(),
            Self::GraveyardOrder(frame) => frame.chooser(),
            Self::Legend(frame) => frame.chooser(),
        }
    }

    pub fn revision(&self) -> u64 {
        match self {
            Self::CleanupDiscard(frame) | Self::CleanupSelect(frame) => frame.revision(),
            Self::GraveyardOrder(frame) => frame.revision(),
            Self::Legend(frame) => frame.revision(),
        }
    }

    pub fn boundary(&self) -> CheckBoundary {
        match self {
            Self::CleanupDiscard(frame) | Self::CleanupSelect(frame) => frame.boundary(),
            Self::GraveyardOrder(frame) => frame.boundary(),
            Self::Legend(frame) => frame.boundary(),
        }
    }

    pub fn event(&self) -> CheckEventId {
        match self {
            Self::CleanupDiscard(frame) | Self::CleanupSelect(frame) => frame.event(),
            Self::GraveyardOrder(frame) => frame.event(),
            Self::Legend(frame) => frame.event(),
        }
    }
}

/// Actual executor result, including completion at a committed-loss boundary.
/// Cleanup discard alone never counts as a performed state-based action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckCompletion {
    pub(crate) boundary: CheckBoundary,
    pub(crate) performed: bool,
    pub(crate) waiting_triggers: bool,
}

impl CheckCompletion {
    pub fn boundary(&self) -> CheckBoundary {
        self.boundary
    }

    /// Whether this process actually performed a supported state-based action.
    pub fn performed(&self) -> bool {
        self.performed
    }

    /// Waiting ordinary instances require same-cleanup priority even when no
    /// state-based action occurred. Cleanup itself leaves their stack empty.
    pub fn waiting_triggers(&self) -> bool {
        self.waiting_triggers
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckChoiceKind {
    CleanupSelected,
    CleanupDiscard,
    GraveyardOrder,
    LegendKeep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckMoveCause {
    CleanupDiscard,
    StateBasedAction,
}

/// Typed payload of an actual accepted choice. A keep is not an ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckChoice {
    /// Original selected hand identities before destination replacement and final ordering.
    CleanupSelected {
        selected: Vec<ObjectId>,
    },
    CleanupDiscard {
        ordered: Vec<ObjectId>,
    },
    GraveyardOrder {
        ordered: Vec<ObjectId>,
    },
    LegendKeep {
        conflict: LegendConflictId,
        domain: Vec<ObjectId>,
        keep: ObjectId,
    },
}
impl CheckChoice {
    /// Discriminator derived from the tagged payload without duplicate state.
    pub fn kind(&self) -> CheckChoiceKind {
        match self {
            Self::CleanupSelected { .. } => CheckChoiceKind::CleanupSelected,
            Self::CleanupDiscard { .. } => CheckChoiceKind::CleanupDiscard,
            Self::GraveyardOrder { .. } => CheckChoiceKind::GraveyardOrder,
            Self::LegendKeep { .. } => CheckChoiceKind::LegendKeep,
        }
    }
}

/// One accepted player choice, which may precede later controllers/owners.
/// Forced singleton graveyard arrangements do not create a choice record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckChoiceRecord {
    pub event: CheckEventId,
    pub boundary: CheckBoundary,
    pub player: SeatId,
    pub choice: CheckChoice,
}

/// Actual committed physical movement. Snapshots preserve the old and new
/// incarnations without retaining another registry or claiming complete LKI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckMoveRecord {
    pub event: CheckEventId,
    pub boundary: CheckBoundary,
    pub cause: CheckMoveCause,
    pub before: GameObject,
    pub after: GameObject,
}

/// Current-operation check facts, appended once by the existing result owner.
/// Movement is emitted only after its simultaneous event commits. This does not
/// contain every game event and is neither resumption state nor durable replay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckJournal {
    pub choices: Vec<CheckChoiceRecord>,
    pub moves: Vec<CheckMoveRecord>,
}

impl CheckJournal {
    pub(crate) fn append(&mut self, mut other: Self) {
        self.choices.append(&mut other.choices);
        self.moves.append(&mut other.moves);
    }
}

/// Check-specific validation and operational failures. Other executor errors
/// retain authority over stale revisions, phases and actors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckError {
    WrongFrame,
    InvalidDiscard,
    InvalidGraveyardOrder,
    InvalidLegendConflict,
    InvalidLegendKeep,
    ConflictCountExhausted,
    ChoiceRequired,
    IterationExhausted,
}
