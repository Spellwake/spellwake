//! Universal `ZoneOp` pipeline (`Source -> Select -> Move/Reveal/Look -> Bind/Tag`).
//!
//! Consolidates zone movement, exile, return, search, reveal, look, destroy,
//! sacrifice, discard, mill, and shuffle into a single orthogonal specification.

use crate::{
    effects::CounterKind,
    program::{DefinitionError, ValueExpr},
    toolbox::{
        choice::ChoiceCategory,
        env::BindingId,
        query::{ObjectQuery, PlayerSelector},
        tags::{TagKey, TagValue},
    },
};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

/// Optional constraint enforced on multi-object selection prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionConstraint {
    /// Selected objects must have pairwise distinct card names.
    DistinctNames,
    /// Selected objects must have pairwise distinct mana values.
    DistinctManaValues,
    /// Selected objects must have pairwise distinct powers.
    DistinctPowers,
    /// Selected objects must share at least one card type.
    SharedCardType,
}

/// How objects are selected from a [`ZoneOpSpec::source`] query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneSelection {
    /// Operate on every object returned by `source`.
    All,
    /// Select `count` objects uniformly at random using the deterministic entropy tape.
    Random {
        count: Box<ValueExpr>,
        chooser: PlayerSelector,
    },
    /// Prompt `chooser` to pick between `min` and `max` objects from `source`.
    Prompt {
        chooser: PlayerSelector,
        min: Box<ValueExpr>,
        max: Box<ValueExpr>,
        constraint: Option<SelectionConstraint>,
        category: ChoiceCategory,
        hidden: bool,
    },
}

/// Controller under which an object enters the battlefield.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneController {
    /// Each moved object's owner.
    Owner,
    /// First player resolved by the selector (e.g. resolving controller, target player, bound seat).
    Player(Box<PlayerSelector>),
}

/// Graveyard-entry rules semantics (CR 701.7, 701.17, 701.8, 701.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraveyardSemantics {
    /// Plain zone change to owner's graveyard.
    Move,
    /// Destruction (respects indestructible and regeneration when `allow_regenerate` is true).
    Destroy { allow_regenerate: bool },
    /// Sacrifice (bypasses indestructible/regeneration; emits sacrifice occurrence).
    Sacrifice,
    /// Discard from hand (emits discard occurrence).
    Discard,
    /// Mill from library (emits mill occurrence).
    Mill,
}

/// Placement position when moving objects into a library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryOrderPos {
    Top,
    Bottom,
    /// `N`th from the top (`1` = top, `2` = second from top, etc.).
    NthFromTop(Box<ValueExpr>),
    /// Put into owner's library and shuffle that library.
    ShuffleInto,
}

/// Destination and entry modifiers for [`ZoneOpSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneDest {
    Battlefield {
        controller: ZoneController,
        tapped: bool,
        transformed: bool,
        face_down: bool,
        entering_counters: Vec<(CounterKind, Box<ValueExpr>)>,
        attach_to: Option<Box<ObjectQuery>>,
    },
    Graveyard {
        semantics: GraveyardSemantics,
    },
    Exile {
        face_down: bool,
        viewer: Option<PlayerSelector>,
        link_to_source: bool,
    },
    Hand,
    Library {
        position: LibraryOrderPos,
    },
    /// Reveal selected objects without moving them; still binds them and their LKI snapshots.
    RevealOnly,
    /// Look at selected objects privately without moving them; still binds them.
    LookOnly {
        viewer: PlayerSelector,
    },
}

/// Complete universal zone-operation pipeline specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneOpSpec {
    pub source: ObjectQuery,
    pub selection: ZoneSelection,
    pub destination: ZoneDest,
    /// Bind post-move object IDs (or revealed/looked IDs) into [`crate::toolbox::ExecEnv`].
    pub bind_moved: Option<BindingId>,
    /// Bind pre-move LKI [`crate::toolbox::ObjectSnapshot`]s into [`crate::toolbox::ExecEnv`].
    pub bind_snapshots: Option<BindingId>,
    /// Bind objects in `source` that were NOT selected into [`crate::toolbox::ExecEnv`].
    pub bind_unchosen: Option<BindingId>,
    /// Bind `ExecValue::Bool(!moved.is_empty())` into [`crate::toolbox::ExecEnv`].
    pub bind_succeeded: Option<BindingId>,
    /// Optional metadata tag attached to every moved/selected object in [`crate::toolbox::ObjectTagStore`].
    pub tag_moved: Option<(TagKey, TagValue)>,
    /// Optional player whose library is shuffled after the operation completes.
    pub shuffle_library: Option<PlayerSelector>,
}

impl ZoneOpSpec {
    /// Whether any part of this zone operation references an announced target.
    #[must_use]
    pub fn uses_target(&self) -> bool {
        if self.source.uses_target() {
            return true;
        }
        let sel_uses = match &self.selection {
            ZoneSelection::All => false,
            ZoneSelection::Random { count, chooser } => {
                count.uses_target() || chooser.uses_target()
            }
            ZoneSelection::Prompt {
                chooser, min, max, ..
            } => chooser.uses_target() || min.uses_target() || max.uses_target(),
        };
        if sel_uses {
            return true;
        }
        let dest_uses = match &self.destination {
            ZoneDest::Battlefield {
                controller,
                entering_counters,
                attach_to,
                ..
            } => {
                matches!(controller, ZoneController::Player(p) if p.uses_target())
                    || entering_counters.iter().any(|(_, e)| e.uses_target())
                    || attach_to.as_ref().is_some_and(|q| q.uses_target())
            }
            ZoneDest::Exile { viewer, .. } => {
                viewer.as_ref().is_some_and(PlayerSelector::uses_target)
            }
            ZoneDest::Library {
                position: LibraryOrderPos::NthFromTop(expr),
            } => expr.uses_target(),
            ZoneDest::LookOnly { viewer } => viewer.uses_target(),
            ZoneDest::Graveyard { .. }
            | ZoneDest::Hand
            | ZoneDest::Library { .. }
            | ZoneDest::RevealOnly => false,
        };
        dest_uses
            || self
                .shuffle_library
                .as_ref()
                .is_some_and(PlayerSelector::uses_target)
    }

    /// Validate nested queries, filters, and value expressions against `vocabulary`.
    ///
    /// # Errors
    /// Returns [`DefinitionError::InvalidInstruction`] if any nested query or expression is invalid.
    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.source.validate_vocabulary(vocabulary)?;
        match &self.selection {
            ZoneSelection::All => {}
            ZoneSelection::Random { count, chooser } => {
                count
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                chooser.validate_vocabulary(vocabulary)?;
            }
            ZoneSelection::Prompt {
                chooser, min, max, ..
            } => {
                chooser.validate_vocabulary(vocabulary)?;
                min.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                max.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
            }
        }
        match &self.destination {
            ZoneDest::Battlefield {
                controller,
                entering_counters,
                attach_to,
                ..
            } => {
                if let ZoneController::Player(p) = controller {
                    p.validate_vocabulary(vocabulary)?;
                }
                for (_, expr) in entering_counters {
                    expr.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                if let Some(q) = attach_to {
                    q.validate_vocabulary(vocabulary)?;
                }
            }
            ZoneDest::Exile { viewer, .. } => {
                if let Some(v) = viewer {
                    v.validate_vocabulary(vocabulary)?;
                }
            }
            ZoneDest::Library {
                position: LibraryOrderPos::NthFromTop(expr),
            } => {
                expr.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
            }
            ZoneDest::LookOnly { viewer } => viewer.validate_vocabulary(vocabulary)?,
            ZoneDest::Graveyard { .. }
            | ZoneDest::Hand
            | ZoneDest::Library { .. }
            | ZoneDest::RevealOnly => {}
        }
        if let Some(shuf) = &self.shuffle_library {
            shuf.validate_vocabulary(vocabulary)?;
        }
        Ok(())
    }
}
