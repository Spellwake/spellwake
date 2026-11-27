//! Universal composable `DrawCards` and `MutateObject` primitives.
//!
//! Replaces dozens of legacy draw, tap/untap, phase-out, transform, face-down/up,
//! regenerate, goad, exert, attach/unattach, counter manipulation, and control
//! macros with two orthogonal, query-driven primitives.

use crate::{
    characteristics::ResolvedDuration,
    effects::CounterKind,
    program::{DefinitionError, ValueExpr},
    toolbox::{
        env::BindingId,
        query::{ObjectQuery, PlayerSelector},
        zone_op::ZoneSelection,
    },
};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

/// Universal card-draw specification (`players` each draw `amount` cards).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawCardsSpec {
    pub players: PlayerSelector,
    pub amount: ValueExpr,
    /// Optional binding receiving the drawn object IDs in [`crate::toolbox::ExecEnv`].
    pub bind_drawn: Option<BindingId>,
}

impl DrawCardsSpec {
    /// Construct a plain draw spec for `players` drawing `amount` cards.
    #[must_use]
    pub fn simple(players: PlayerSelector, amount: ValueExpr) -> Self {
        Self {
            players,
            amount,
            bind_drawn: None,
        }
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.players.uses_target() || self.amount.uses_target()
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.players.validate_vocabulary(vocabulary)?;
        self.amount
            .validate_vocabulary(vocabulary)
            .map_err(|_| DefinitionError::InvalidInstruction)
    }
}

/// Orthogonal permanent/object state mutation executed by [`MutateObjectSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectMutation {
    /// Tap (`true`) or untap (`false`) each selected permanent.
    SetTapped(bool),
    /// Flip tapped status of each selected permanent (tap if untapped, untap if tapped).
    ToggleTapped,
    /// Phase out each selected permanent (CR 702.26).
    PhaseOut,
    /// Transform each selected double-faced permanent (CR 701.28).
    Transform,
    /// Turn each selected face-down permanent face up (CR 708).
    TurnFaceUp,
    /// Turn each selected face-up permanent face down as a 2/2 creature (CR 708).
    TurnFaceDown,
    /// Create a regeneration shield on each selected permanent (CR 701.15).
    Regenerate,
    /// Goad each selected creature under the first player resolved by `by` (CR 701.38).
    Goad { by: PlayerSelector },
    /// Exert each selected permanent (skips next untap step + emits Exert occurrence).
    Exert,
    /// Attach each selected attachment (Aura/Equipment/Fortification) to the first object in `host`.
    Attach { host: ObjectQuery },
    /// Attach each selected attachment to the first player in `player`.
    AttachToPlayer { player: PlayerSelector },
    /// Unattach each selected attachment from whatever it is attached to.
    Unattach,
    /// Put `amount` counters of `kind` on each selected object.
    AddCounters {
        kind: CounterKind,
        amount: ValueExpr,
    },
    /// Remove up to `amount` counters of `kind` (or any kind if `None`) from each selected object.
    RemoveCounters {
        kind: Option<CounterKind>,
        amount: ValueExpr,
    },
    /// Remove all counters of `kind` (or all counters if `None`) from each selected object.
    ClearCounters { kind: Option<CounterKind> },
    /// Double all counters of `kind` (or every counter kind if `None`) on each selected object.
    DoubleCounters { kind: Option<CounterKind> },
    /// Move counters from selected object(s) to the first object resolved by `to`.
    MoveCounters {
        to: ObjectQuery,
        kind: Option<CounterKind>,
        amount: Option<ValueExpr>,
    },
    /// Put one additional counter of each kind already on each selected object.
    AddOneOfEachCounterKind,
    /// Change controller of each selected permanent to `controller` for `duration`.
    GainControl {
        controller: PlayerSelector,
        duration: ResolvedDuration,
    },
    /// Mark each selected permanent not to untap during its controller's next untap step.
    SkipNextUntap,
    /// Remove each selected permanent from combat (CR 506.4).
    RemoveFromCombat,
    /// Copy all counters recorded on the triggering LKI snapshot onto each selected permanent.
    TransferCountersFromTriggeringLki,
    /// Grant flashback (equal to mana cost) until end of turn to each selected graveyard instant/sorcery.
    GrantFlashback,
}

impl ObjectMutation {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Goad { by } => by.uses_target(),
            Self::Attach { host } => host.uses_target(),
            Self::AttachToPlayer { player } => player.uses_target(),
            Self::AddCounters { amount, .. } | Self::RemoveCounters { amount, .. } => {
                amount.uses_target()
            }
            Self::MoveCounters { to, amount, .. } => {
                to.uses_target() || amount.as_ref().is_some_and(ValueExpr::uses_target)
            }
            Self::GainControl { controller, .. } => controller.uses_target(),
            Self::SetTapped(_)
            | Self::ToggleTapped
            | Self::Transform
            | Self::TurnFaceUp
            | Self::TurnFaceDown
            | Self::Regenerate
            | Self::PhaseOut
            | Self::Exert
            | Self::SkipNextUntap
            | Self::RemoveFromCombat
            | Self::Unattach
            | Self::ClearCounters { .. }
            | Self::DoubleCounters { .. }
            | Self::AddOneOfEachCounterKind
            | Self::TransferCountersFromTriggeringLki
            | Self::GrantFlashback => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Goad { by } => by.validate_vocabulary(vocabulary),
            Self::Attach { host } => host.validate_vocabulary(vocabulary),
            Self::AttachToPlayer { player } => player.validate_vocabulary(vocabulary),
            Self::AddCounters { amount, .. } | Self::RemoveCounters { amount, .. } => amount
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::MoveCounters { to, amount, .. } => {
                to.validate_vocabulary(vocabulary)?;
                if let Some(expr) = amount {
                    expr.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                Ok(())
            }
            Self::GainControl { controller, .. } => controller.validate_vocabulary(vocabulary),
            Self::SetTapped(_)
            | Self::ToggleTapped
            | Self::Transform
            | Self::TurnFaceUp
            | Self::TurnFaceDown
            | Self::Regenerate
            | Self::PhaseOut
            | Self::Exert
            | Self::SkipNextUntap
            | Self::RemoveFromCombat
            | Self::Unattach
            | Self::ClearCounters { .. }
            | Self::DoubleCounters { .. }
            | Self::AddOneOfEachCounterKind
            | Self::TransferCountersFromTriggeringLki
            | Self::GrantFlashback => Ok(()),
        }
    }
}

/// Complete universal object-mutation specification (`Source -> Select -> Mutate -> Bind`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutateObjectSpec {
    pub source: ObjectQuery,
    pub selection: ZoneSelection,
    pub op: ObjectMutation,
    pub bind_mutated: Option<BindingId>,
    pub bind_succeeded: Option<BindingId>,
}

impl MutateObjectSpec {
    /// Construct a `MutateObjectSpec` applying `op` to every object returned by `source`.
    #[must_use]
    pub fn all(source: ObjectQuery, op: ObjectMutation) -> Self {
        Self {
            source,
            selection: ZoneSelection::All,
            op,
            bind_mutated: None,
            bind_succeeded: None,
        }
    }

    /// Construct a `MutateObjectSpec` with an explicit [`ZoneSelection`].
    #[must_use]
    pub fn with_selection(
        source: ObjectQuery,
        selection: ZoneSelection,
        op: ObjectMutation,
    ) -> Self {
        Self {
            source,
            selection,
            op,
            bind_mutated: None,
            bind_succeeded: None,
        }
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.source.uses_target() || self.selection.uses_target() || self.op.uses_target()
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.source.validate_vocabulary(vocabulary)?;
        self.selection.validate_vocabulary(vocabulary)?;
        self.op.validate_vocabulary(vocabulary)
    }
}
