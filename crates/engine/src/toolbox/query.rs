//! Universal composable `PlayerSelector` and `ObjectQuery` algebra.
//!
//! Reuses existing [`ObjectFilter`], [`ResolvingPlayer`], [`AffectedPlayers`],
//! [`EffectSubject`], and [`ValueExpr`] without duplicating filter semantics.

use crate::{
    object_identity::ObjectId,
    program::{
        AffectedPlayers, DefinitionError, EffectSubject, LinkedExileGroup, ObjectFilter,
        ResolvingPlayer, ValueExpr,
    },
    toolbox::{
        env::BindingId,
        tags::{TagKey, TagValue},
    },
};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

/// Composable player selector resolving to an ordered, deduplicated list of seats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerSelector {
    /// Single player resolved by the standard resolution player reference.
    Resolving(ResolvingPlayer),
    /// Explicit seat identity (used for internal/delayed triggers such as monarch combat steal).
    Fixed(crate::priority::SeatId),
    /// Player group (each player, each opponent, controller, etc.).
    Group(AffectedPlayers),
    /// Players bound in [`crate::toolbox::ExecEnv`] (or controllers of bound objects).
    Bound(BindingId),
    /// Owners of every object returned by the nested [`ObjectQuery`].
    OwnersOf(Box<ObjectQuery>),
    /// Controllers of every object returned by the nested [`ObjectQuery`].
    ControllersOf(Box<ObjectQuery>),
}

impl PlayerSelector {
    /// Whether this selector depends on an announced target of the resolving entry.
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Resolving(p) | Self::Group(AffectedPlayers::One(p)) => matches!(
                p,
                ResolvingPlayer::TargetPlayer
                    | ResolvingPlayer::TargetSpellController
                    | ResolvingPlayer::TargetController
                    | ResolvingPlayer::TargetOwner
            ),
            Self::Group(_) | Self::Bound(_) | Self::Fixed(_) => false,
            Self::OwnersOf(q) | Self::ControllersOf(q) => q.uses_target(),
        }
    }

    /// Validate any creature-type vocabulary references inside nested queries.
    ///
    /// # Errors
    /// Returns [`DefinitionError::InvalidInstruction`] if a nested filter references an unknown subtype.
    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Resolving(_) | Self::Group(_) | Self::Bound(_) | Self::Fixed(_) => Ok(()),
            Self::OwnersOf(q) | Self::ControllersOf(q) => q.validate_vocabulary(vocabulary),
        }
    }
}

/// Zone targeted by an [`ObjectQuery::ZoneScan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryZone {
    Battlefield,
    BattlefieldControlledBy(Box<PlayerSelector>),
    Hand(Box<PlayerSelector>),
    Library(Box<PlayerSelector>),
    Graveyard(Box<PlayerSelector>),
    Exile,
    ExileOwnedBy(Box<PlayerSelector>),
    Stack,
    Command,
    AllZones,
}

impl QueryZone {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Hand(p)
            | Self::Library(p)
            | Self::Graveyard(p)
            | Self::ExileOwnedBy(p)
            | Self::BattlefieldControlledBy(p) => p.uses_target(),
            Self::Battlefield | Self::Exile | Self::Stack | Self::Command | Self::AllZones => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Hand(p)
            | Self::Library(p)
            | Self::Graveyard(p)
            | Self::ExileOwnedBy(p)
            | Self::BattlefieldControlledBy(p) => p.validate_vocabulary(vocabulary),
            Self::Battlefield | Self::Exile | Self::Stack | Self::Command | Self::AllZones => {
                Ok(())
            }
        }
    }
}

/// Slice or prefix extraction applied to an ordered zone scan (such as a library).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuerySlice {
    /// Every matching object in the zone.
    All,
    /// First `N` objects from the top of the ordered zone (filtered after slicing).
    Top(Box<ValueExpr>),
    /// Last `N` objects from the bottom of the ordered zone (filtered after slicing).
    Bottom(Box<ValueExpr>),
    /// Scan from the top until `count` objects match `filter`.
    /// When `include_non_matches` is true, returns all scanned objects up through the final match;
    /// otherwise returns only the matched objects.
    UntilMatch {
        filter: Box<ObjectFilter>,
        count: Box<ValueExpr>,
        include_non_matches: bool,
    },
}

impl QuerySlice {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::All => false,
            Self::Top(expr) | Self::Bottom(expr) => expr.uses_target(),
            Self::UntilMatch { filter, count, .. } => {
                filter.uses_target_permanent() || count.uses_target()
            }
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::All => Ok(()),
            Self::Top(expr) | Self::Bottom(expr) => expr
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::UntilMatch { filter, count, .. } => {
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                count
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)
            }
        }
    }
}

/// Universal composable object selector resolving to an ordered, deduplicated list of `ObjectId`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectQuery {
    /// Empty object set (for pure shuffle, tag, or bind operations).
    Empty,
    /// Explicit live object IDs (e.g. delayed return/exile/sacrifice triggers).
    Fixed(Vec<ObjectId>),
    /// Scan a zone, optionally slicing top/bottom/until-match, and filter with [`ObjectFilter`].
    ZoneScan {
        zone: QueryZone,
        filter: Box<ObjectFilter>,
        slice: QuerySlice,
    },
    /// Objects stored in [`crate::toolbox::ExecEnv`] at `binding`.
    Bound(BindingId),
    /// Objects stored in [`crate::toolbox::ExecEnv`] at `binding`, filtered by `filter`.
    BoundFiltered {
        binding: BindingId,
        filter: Box<ObjectFilter>,
    },
    /// Filter any nested [`ObjectQuery`] with an [`ObjectFilter`].
    Filtered {
        source: Box<ObjectQuery>,
        filter: Box<ObjectFilter>,
    },
    /// Take at most `count` leading objects from a nested [`ObjectQuery`].
    Take {
        source: Box<ObjectQuery>,
        count: Box<ValueExpr>,
    },
    /// Every still-legal object target announced for the resolving spell or ability.
    Targets,
    /// The resolving spell or ability's source object.
    Source,
    /// Triggering subject object(s) of the resolving triggered ability.
    TriggerSubject,
    /// Permanent or player-attached host of the resolving source.
    AttachedHost,
    /// Objects moved by the preceding instruction in this resolution.
    LastMoved,
    /// Cards destroyed by the preceding destroy instruction in this resolution.
    DestroyedThisWay,
    /// Tokens created earlier during this resolution.
    CreatedTokens,
    /// Top/revealed card remembered during this resolution.
    Revealed,
    /// Remembered/linked target pair element (`0` = first, `1` = second).
    LinkedTarget { index: u8 },
    /// Cards currently in exile linked to the resolving source (optionally scoped by group).
    LinkedExile { group: Option<LinkedExileGroup> },
    /// Composable [`EffectSubject`] adapter (triggering attacker/group/target, same-name/color sets).
    EffectSubject(Box<EffectSubject>),
    /// Objects bearing a metadata tag or partition membership in [`crate::toolbox::ObjectTagStore`].
    Tagged {
        key: TagKey,
        value: Option<TagValue>,
        zone: Option<QueryZone>,
        filter: Option<Box<ObjectFilter>>,
    },
    /// Ordered set union of sub-queries.
    Union(Vec<ObjectQuery>),
    /// Objects in `left` that are not in `right`.
    Difference(Box<ObjectQuery>, Box<ObjectQuery>),
    /// Objects present in both `left` and `right`.
    Intersect(Box<ObjectQuery>, Box<ObjectQuery>),
}

impl ObjectQuery {
    /// Convenience constructor for scanning all matching objects in a zone.
    #[must_use]
    pub fn zone(zone: QueryZone, filter: ObjectFilter) -> Self {
        Self::ZoneScan {
            zone,
            filter: Box::new(filter),
            slice: QuerySlice::All,
        }
    }

    /// Convenience constructor wrapping an existing query with an [`ObjectFilter`].
    #[must_use]
    pub fn filtered(source: Self, filter: ObjectFilter) -> Self {
        Self::Filtered {
            source: Box::new(source),
            filter: Box::new(filter),
        }
    }

    /// Convert a legacy [`crate::program::MoveSubjectRef`] into an orthogonal [`ObjectQuery`].
    #[must_use]
    pub fn from_move_subject(subj: crate::program::MoveSubjectRef) -> Self {
        use crate::program::MoveSubjectRef;
        match subj {
            MoveSubjectRef::AbilitySource => Self::Source,
            MoveSubjectRef::TargetObject => Self::Targets,
            MoveSubjectRef::AttachedHost => Self::AttachedHost,
            MoveSubjectRef::TriggeringObject => Self::TriggerSubject,
            MoveSubjectRef::LastMoved => Self::LastMoved,
            MoveSubjectRef::CreatedTokens => Self::CreatedTokens,
            MoveSubjectRef::RevealedCard => Self::Revealed,
            MoveSubjectRef::RememberedFirst => Self::LinkedTarget { index: 0 },
            MoveSubjectRef::RememberedSecond => Self::LinkedTarget { index: 1 },
        }
    }

    /// Whether this query depends on an announced target.
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Targets => true,
            Self::LinkedTarget { .. } => false,
            Self::Empty
            | Self::Fixed(_)
            | Self::Bound(_)
            | Self::Source
            | Self::TriggerSubject
            | Self::AttachedHost
            | Self::LastMoved
            | Self::DestroyedThisWay
            | Self::CreatedTokens
            | Self::Revealed
            | Self::LinkedExile { .. } => false,
            Self::BoundFiltered { filter, .. } => filter.uses_target_permanent(),
            Self::Filtered { source, filter } => {
                source.uses_target() || filter.uses_target_permanent()
            }
            Self::Take { source, count } => source.uses_target() || count.uses_target(),
            Self::EffectSubject(subject) => match **subject {
                EffectSubject::TargetObject
                | EffectSubject::TargetAndSameNameMatching(_)
                | EffectSubject::MatchingOtherThanTarget(_)
                | EffectSubject::TargetAndSharingColorMatching(_) => true,
                EffectSubject::Matching(filter) => filter.uses_target_permanent(),
                _ => false,
            },
            Self::ZoneScan {
                zone,
                filter,
                slice,
            } => zone.uses_target() || filter.uses_target_permanent() || slice.uses_target(),
            Self::Tagged { zone, filter, .. } => {
                zone.as_ref().is_some_and(QueryZone::uses_target)
                    || filter.as_ref().is_some_and(|f| f.uses_target_permanent())
            }
            Self::Union(qs) => qs.iter().any(Self::uses_target),
            Self::Difference(a, b) | Self::Intersect(a, b) => a.uses_target() || b.uses_target(),
        }
    }

    /// Validate all nested filters and value expressions against `vocabulary`.
    ///
    /// # Errors
    /// Returns [`DefinitionError::InvalidInstruction`] if any nested filter or expression is invalid.
    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Empty
            | Self::Fixed(_)
            | Self::Bound(_)
            | Self::Targets
            | Self::Source
            | Self::TriggerSubject
            | Self::AttachedHost
            | Self::LastMoved
            | Self::DestroyedThisWay
            | Self::CreatedTokens
            | Self::Revealed
            | Self::LinkedTarget { .. }
            | Self::LinkedExile { .. } => Ok(()),
            Self::BoundFiltered { filter, .. } => filter
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::Filtered { source, filter } => {
                source.validate_vocabulary(vocabulary)?;
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)
            }
            Self::Take { source, count } => {
                source.validate_vocabulary(vocabulary)?;
                count
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)
            }
            Self::EffectSubject(subject) => subject
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::ZoneScan {
                zone,
                filter,
                slice,
            } => {
                zone.validate_vocabulary(vocabulary)?;
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                slice.validate_vocabulary(vocabulary)
            }
            Self::Tagged { zone, filter, .. } => {
                if let Some(z) = zone {
                    z.validate_vocabulary(vocabulary)?;
                }
                if let Some(f) = filter {
                    f.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                Ok(())
            }
            Self::Union(qs) => {
                for q in qs {
                    q.validate_vocabulary(vocabulary)?;
                }
                Ok(())
            }
            Self::Difference(a, b) | Self::Intersect(a, b) => {
                a.validate_vocabulary(vocabulary)?;
                b.validate_vocabulary(vocabulary)
            }
        }
    }
}

pub(crate) fn filter_uses_target(f: &ObjectFilter) -> bool {
    f.has_target_player() || f.uses_target_permanent()
}
