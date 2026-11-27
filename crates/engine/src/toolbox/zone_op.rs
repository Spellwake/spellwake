//! Universal `ZoneOp` pipeline (`Source -> Select -> Move/Reveal/Look -> Bind/Tag`).
//!
//! Consolidates zone movement, exile, return, search, reveal, look, destroy,
//! sacrifice, discard, mill, and shuffle into a single orthogonal specification.

use crate::{
    characteristics::ResolvedDuration,
    effects::CounterKind,
    object_identity::ObjectId,
    program::{
        AffectedPlayers, CardType, CardTypeMask, DefinitionError, DelayedReturnTiming, Instruction,
        LibraryPlacement, LinkedExileGroup, LookTopKeepDest, LookTopRestDest, ObjectClause,
        ObjectFilter, PermanentTypeFilter, ResolvingPlayer, ValueExpr,
    },
    toolbox::{
        choice::{ChoiceCategory, PromptChoiceSpec, PromptDomain},
        continuous::ApplyContinuousSpec,
        env::BindingId,
        query::{ObjectQuery, PlayerSelector, QuerySlice, QueryZone},
        tags::{TagKey, TagValue},
    },
};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use std::collections::BTreeSet;

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
    /// Selected objects must each be assignable to a distinct card type.
    DistinctCardTypes,
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

impl ZoneSelection {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::All => false,
            Self::Random { count, chooser } => count.uses_target() || chooser.uses_target(),
            Self::Prompt {
                chooser, min, max, ..
            } => chooser.uses_target() || min.uses_target() || max.uses_target(),
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::All => Ok(()),
            Self::Random { count, chooser } => {
                count
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                chooser.validate_vocabulary(vocabulary)
            }
            Self::Prompt {
                chooser, min, max, ..
            } => {
                chooser.validate_vocabulary(vocabulary)?;
                min.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                max.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)
            }
        }
    }
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
    /// Put on the bottom of the owner's library in a random order.
    BottomRandom,
    /// `N`th from the top (`1` = top, `2` = second from top, etc.).
    NthFromTop(Box<ValueExpr>),
    /// Put into owner's library and shuffle that library.
    ShuffleInto,
    /// Prompt `chooser` to choose top or bottom of owner's library.
    ChoiceTopOrBottom {
        chooser: Box<PlayerSelector>,
    },
}

impl LibraryOrderPos {
    /// Convert a legacy [`LibraryPlacement`] into a universal [`LibraryOrderPos`].
    #[must_use]
    pub fn from_placement(placement: LibraryPlacement) -> Self {
        match placement {
            LibraryPlacement::Top => Self::Top,
            LibraryPlacement::Bottom => Self::Bottom,
            LibraryPlacement::BottomRandom => Self::BottomRandom,
            LibraryPlacement::FromTop(offset) => Self::NthFromTop(Box::new(ValueExpr::literal(
                spellwake_contracts::RulesInteger::from_usize(usize::from(offset)),
            ))),
            LibraryPlacement::ChoiceTopOrBottom { chooser } => Self::ChoiceTopOrBottom {
                chooser: Box::new(PlayerSelector::Resolving(chooser)),
            },
        }
    }
}

/// Destination and entry modifiers for [`ZoneOpSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneDest {
    /// No zone movement (for pure shuffle, bind, or tagging operations).
    None,
    Battlefield {
        controller: ZoneController,
        tapped: bool,
        attacking: bool,
        transformed: bool,
        face_down: Option<crate::face_down::FaceDownKind>,
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
        delayed_return: Option<DelayedReturnTiming>,
        link_group: Option<LinkedExileGroup>,
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
    /// Specialized delayed, cohort, or hand/graveyard return zone operation.
    Delayed(DelayedZoneAction),
    /// Atomically swap all cards between the zone scanned by `source` and `other` (CR 701.10).
    SwapZones {
        other: QueryZone,
    },
}

/// Specialized delayed, cohort, or hand/graveyard return semantics executed via `ZoneOp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelayedZoneAction {
    ReturnGraveyardToHand(ObjectId),
    ReturnPermanentToHand(ObjectId),
    ReturnGraveyardCard(ObjectId),
    ExileObjects(Vec<ObjectId>),
    SacrificeObjects(Vec<ObjectId>),
    ReturnExile(Vec<ObjectId>),
    ReturnExileWithCounters(Vec<ObjectId>),
    ReturnExiledTargetCohort {
        next_end_step: bool,
        owner_controls: bool,
    },
    ReturnChosenXGraveyardCreatures,
    ReturnGraveyardCreatureWithCorpse(RulesInteger),
    PutHandPermanentsOntoBattlefield {
        player: ResolvingPlayer,
        types: BTreeSet<CardType>,
        maximum: u16,
        tapped: bool,
    },
}

impl ZoneDest {
    /// Plain face-up exile with no links or delayed return.
    #[must_use]
    pub const fn exile() -> Self {
        Self::Exile {
            face_down: false,
            viewer: None,
            link_to_source: false,
            delayed_return: None,
            link_group: None,
        }
    }

    /// Plain battlefield entry under `controller` with default flags.
    #[must_use]
    pub const fn battlefield(controller: ZoneController) -> Self {
        Self::Battlefield {
            controller,
            tapped: false,
            attacking: false,
            transformed: false,
            face_down: None,
            entering_counters: Vec::new(),
            attach_to: None,
        }
    }

    /// Face-down battlefield entry (2/2 colorless nameless creature, CR 708.2) under `controller`.
    #[must_use]
    pub const fn battlefield_face_down(controller: ZoneController, tapped: bool) -> Self {
        Self::Battlefield {
            controller,
            tapped,
            attacking: false,
            transformed: false,
            face_down: Some(crate::face_down::FaceDownKind::Manifest),
            entering_counters: Vec::new(),
            attach_to: None,
        }
    }

    /// Cloaked face-down battlefield entry (2/2 colorless nameless creature with ward {2}, CR 701.58a) under `controller`.
    #[must_use]
    pub const fn battlefield_cloaked(controller: ZoneController, tapped: bool) -> Self {
        Self::Battlefield {
            controller,
            tapped,
            attacking: false,
            transformed: false,
            face_down: Some(crate::face_down::FaceDownKind::Cloak),
            entering_counters: Vec::new(),
            attach_to: None,
        }
    }

    /// Convert a [`crate::program::SubjectDestination`] into an orthogonal [`ZoneDest`].
    #[must_use]
    pub fn from_subject_dest(dest: crate::program::SubjectDestination) -> Self {
        use crate::program::SubjectDestination;
        match dest {
            SubjectDestination::Battlefield {
                tapped,
                attacking,
                transformed,
                under_owner_control,
                with_counter,
                attach_to_self,
            } => Self::Battlefield {
                controller: if under_owner_control {
                    ZoneController::Owner
                } else {
                    ZoneController::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    )))
                },
                tapped,
                attacking,
                transformed,
                face_down: None,
                entering_counters: with_counter
                    .map(|(kind, count)| {
                        (
                            kind,
                            Box::new(ValueExpr::literal(RulesInteger::from_usize(usize::from(
                                count,
                            )))),
                        )
                    })
                    .into_iter()
                    .collect(),
                attach_to: attach_to_self.then(|| Box::new(ObjectQuery::Source)),
            },
            SubjectDestination::Hand => Self::Hand,
            SubjectDestination::Graveyard => Self::Graveyard {
                semantics: GraveyardSemantics::Move,
            },
            SubjectDestination::Exile { delayed_return } => Self::Exile {
                face_down: false,
                viewer: None,
                link_to_source: false,
                delayed_return,
                link_group: None,
            },
            SubjectDestination::Library(placement) => Self::Library {
                position: LibraryOrderPos::from_placement(placement),
            },
        }
    }
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
    /// Construct a `ZoneOpSpec` with no optional bindings, tags, or post-shuffle.
    #[must_use]
    pub fn simple(source: ObjectQuery, selection: ZoneSelection, destination: ZoneDest) -> Self {
        Self {
            source,
            selection,
            destination,
            bind_moved: None,
            bind_snapshots: None,
            bind_unchosen: None,
            bind_succeeded: None,
            tag_moved: None,
            shuffle_library: None,
        }
    }

    /// Construct a `ZoneOpSpec` that moves all objects from `source` to `destination`.
    #[must_use]
    pub fn move_all(source: ObjectQuery, destination: ZoneDest) -> Self {
        Self::simple(source, ZoneSelection::All, destination)
    }

    /// Convert a [`PermanentTypeFilter`] over `zone` into a composable [`ObjectQuery`].
    #[must_use]
    pub fn permanent_type_filter_query(
        zone: QueryZone,
        filter: PermanentTypeFilter,
    ) -> ObjectQuery {
        let mut clause = ObjectClause::any();
        if !filter.any_types.is_empty() {
            let mut mask = CardTypeMask::EMPTY;
            for ct in &filter.any_types {
                mask = mask.with(*ct);
            }
            clause = clause.any_types(mask);
        }
        if !filter.required_types.is_empty() {
            let mut mask = CardTypeMask::EMPTY;
            for ct in &filter.required_types {
                mask = mask.with(*ct);
            }
            clause = clause.required_types(mask);
        }
        if !filter.forbidden_types.is_empty() {
            let mut mask = CardTypeMask::EMPTY;
            for ct in &filter.forbidden_types {
                mask = mask.with(*ct);
            }
            clause = clause.forbid_types(mask);
        }
        let base = ObjectQuery::zone(zone, ObjectFilter::Clause(clause));
        match filter.object_filter {
            Some(obj) => ObjectQuery::filtered(base, obj),
            None => base,
        }
    }

    /// Construct a resolution-time group sacrifice prompt (`players` each sacrifice `amount` matching permanents).
    #[must_use]
    pub fn sacrifice(
        players: AffectedPlayers,
        amount: ValueExpr,
        filter: PermanentTypeFilter,
    ) -> Self {
        Self {
            source: Self::permanent_type_filter_query(
                QueryZone::BattlefieldControlledBy(Box::new(PlayerSelector::Group(players))),
                filter,
            ),
            selection: ZoneSelection::Prompt {
                chooser: PlayerSelector::Group(players),
                min: Box::new(amount.clone()),
                max: Box::new(amount),
                constraint: None,
                category: ChoiceCategory::Generic,
                hidden: false,
            },
            destination: ZoneDest::Graveyard {
                semantics: GraveyardSemantics::Sacrifice,
            },
            bind_moved: None,
            bind_snapshots: None,
            bind_unchosen: None,
            bind_succeeded: None,
            tag_moved: None,
            shuffle_library: None,
        }
    }

    /// Construct a resolution-time group discard prompt (`players` each discard `amount` cards).
    #[must_use]
    pub fn discard(players: AffectedPlayers, amount: ValueExpr) -> Self {
        Self {
            source: ObjectQuery::ZoneScan {
                zone: QueryZone::Hand(Box::new(PlayerSelector::Group(players))),
                filter: Box::new(ObjectFilter::any()),
                slice: QuerySlice::All,
            },
            selection: ZoneSelection::Prompt {
                chooser: PlayerSelector::Group(players),
                min: Box::new(amount.clone()),
                max: Box::new(amount),
                constraint: None,
                category: ChoiceCategory::Generic,
                hidden: true,
            },
            destination: ZoneDest::Graveyard {
                semantics: GraveyardSemantics::Discard,
            },
            bind_moved: None,
            bind_snapshots: None,
            bind_unchosen: None,
            bind_succeeded: None,
            tag_moved: None,
            shuffle_library: None,
        }
    }

    /// Inspect whether this spec represents a plain group discard constructed by [`Self::discard`].
    #[must_use]
    pub fn as_group_discard(&self) -> Option<(AffectedPlayers, &ValueExpr)> {
        if self.bind_moved.is_some()
            || self.bind_snapshots.is_some()
            || self.bind_unchosen.is_some()
            || self.bind_succeeded.is_some()
            || self.tag_moved.is_some()
            || self.shuffle_library.is_some()
        {
            return None;
        }
        if !matches!(
            self.destination,
            ZoneDest::Graveyard {
                semantics: GraveyardSemantics::Discard
            }
        ) {
            return None;
        }
        let ZoneSelection::Prompt {
            chooser: PlayerSelector::Group(players),
            min,
            max,
            constraint: None,
            ..
        } = &self.selection
        else {
            return None;
        };
        if min != max {
            return None;
        }
        if matches!(
            &self.source,
            ObjectQuery::ZoneScan {
                zone: QueryZone::Hand(_),
                filter,
                slice: QuerySlice::All,
            } if *filter.as_ref() == ObjectFilter::any()
        ) {
            Some((*players, max.as_ref()))
        } else {
            None
        }
    }

    /// Inspect whether this spec represents a plain group sacrifice constructed by [`Self::sacrifice`].
    #[must_use]
    pub fn as_group_sacrifice(
        &self,
    ) -> Option<(
        AffectedPlayers,
        &ValueExpr,
        ObjectFilter,
        Option<ObjectFilter>,
    )> {
        if self.bind_moved.is_some()
            || self.bind_snapshots.is_some()
            || self.bind_unchosen.is_some()
            || self.bind_succeeded.is_some()
            || self.tag_moved.is_some()
            || self.shuffle_library.is_some()
        {
            return None;
        }
        if !matches!(
            self.destination,
            ZoneDest::Graveyard {
                semantics: GraveyardSemantics::Sacrifice
            }
        ) {
            return None;
        }
        let ZoneSelection::Prompt {
            chooser: PlayerSelector::Group(players),
            min,
            max,
            constraint: None,
            ..
        } = &self.selection
        else {
            return None;
        };
        if min != max {
            return None;
        }
        match &self.source {
            ObjectQuery::ZoneScan {
                zone: QueryZone::BattlefieldControlledBy(_),
                filter,
                slice: QuerySlice::All,
            } => Some((*players, max.as_ref(), **filter, None)),
            ObjectQuery::Filtered {
                source,
                filter: extra,
            } => match source.as_ref() {
                ObjectQuery::ZoneScan {
                    zone: QueryZone::BattlefieldControlledBy(_),
                    filter,
                    slice: QuerySlice::All,
                } => Some((*players, max.as_ref(), **filter, Some(**extra))),
                _ => None,
            },
            _ => None,
        }
    }

    /// Exile the top `count` cards of each `owners` library, optionally face down.
    #[must_use]
    pub fn exile_top_library(owners: PlayerSelector, count: ValueExpr, face_down: bool) -> Self {
        Self {
            source: ObjectQuery::ZoneScan {
                zone: QueryZone::Library(Box::new(owners)),
                filter: Box::new(ObjectFilter::any()),
                slice: QuerySlice::Top(Box::new(count)),
            },
            selection: ZoneSelection::All,
            destination: ZoneDest::Exile {
                face_down,
                viewer: face_down.then_some(PlayerSelector::Resolving(ResolvingPlayer::Controller)),
                link_to_source: false,
                delayed_return: None,
                link_group: None,
            },
            bind_moved: None,
            bind_snapshots: None,
            bind_unchosen: None,
            bind_succeeded: None,
            tag_moved: None,
            shuffle_library: None,
        }
    }

    /// Construct a pure library shuffle for `player`.
    #[must_use]
    pub fn shuffle(player: PlayerSelector) -> Self {
        Self {
            source: ObjectQuery::Empty,
            selection: ZoneSelection::All,
            destination: ZoneDest::None,
            bind_moved: None,
            bind_snapshots: None,
            bind_unchosen: None,
            bind_succeeded: None,
            tag_moved: None,
            shuffle_library: Some(player),
        }
    }

    /// Atomically swap all cards between `from` and `to` (CR 701.10).
    #[must_use]
    pub fn swap_zones(from: QueryZone, to: QueryZone) -> Self {
        Self::move_all(
            ObjectQuery::ZoneScan {
                zone: from,
                filter: Box::new(ObjectFilter::any()),
                slice: QuerySlice::All,
            },
            ZoneDest::SwapZones { other: to },
        )
    }

    #[must_use]
    pub fn is_delayed_special_op(&self) -> bool {
        matches!(self.destination, ZoneDest::Delayed(_))
    }

    #[must_use]
    pub fn return_delayed_graveyard_to_hand(object: ObjectId) -> Self {
        Self::move_all(
            ObjectQuery::Fixed(vec![object]),
            ZoneDest::Delayed(DelayedZoneAction::ReturnGraveyardToHand(object)),
        )
    }

    #[must_use]
    pub fn return_delayed_permanent_to_hand(object: ObjectId) -> Self {
        Self::move_all(
            ObjectQuery::Fixed(vec![object]),
            ZoneDest::Delayed(DelayedZoneAction::ReturnPermanentToHand(object)),
        )
    }

    #[must_use]
    pub fn return_delayed_graveyard_card(object: ObjectId) -> Self {
        Self::move_all(
            ObjectQuery::Fixed(vec![object]),
            ZoneDest::Delayed(DelayedZoneAction::ReturnGraveyardCard(object)),
        )
    }

    #[must_use]
    pub fn exile_delayed_objects(objects: Vec<ObjectId>) -> Self {
        Self::move_all(
            ObjectQuery::Fixed(objects.clone()),
            ZoneDest::Delayed(DelayedZoneAction::ExileObjects(objects)),
        )
    }

    #[must_use]
    pub fn sacrifice_delayed_objects(objects: Vec<ObjectId>) -> Self {
        Self::move_all(
            ObjectQuery::Fixed(objects.clone()),
            ZoneDest::Delayed(DelayedZoneAction::SacrificeObjects(objects)),
        )
    }

    #[must_use]
    pub fn return_delayed_exile(objects: Vec<ObjectId>) -> Self {
        Self::move_all(
            ObjectQuery::Fixed(objects.clone()),
            ZoneDest::Delayed(DelayedZoneAction::ReturnExile(objects)),
        )
    }

    #[must_use]
    pub fn return_delayed_exile_with_counters(objects: Vec<ObjectId>) -> Self {
        Self::move_all(
            ObjectQuery::Fixed(objects.clone()),
            ZoneDest::Delayed(DelayedZoneAction::ReturnExileWithCounters(objects)),
        )
    }

    #[must_use]
    pub fn return_exiled_target_cohort(next_end_step: bool, owner_controls: bool) -> Self {
        Self::move_all(
            ObjectQuery::Empty,
            ZoneDest::Delayed(DelayedZoneAction::ReturnExiledTargetCohort {
                next_end_step,
                owner_controls,
            }),
        )
    }

    #[must_use]
    pub fn return_chosen_x_graveyard_creatures() -> Self {
        Self::move_all(
            ObjectQuery::Empty,
            ZoneDest::Delayed(DelayedZoneAction::ReturnChosenXGraveyardCreatures),
        )
    }

    #[must_use]
    pub fn return_graveyard_creature_with_corpse(amount: RulesInteger) -> Self {
        Self::move_all(
            ObjectQuery::Empty,
            ZoneDest::Delayed(DelayedZoneAction::ReturnGraveyardCreatureWithCorpse(amount)),
        )
    }

    #[must_use]
    pub fn put_hand_permanents_onto_battlefield(
        player: ResolvingPlayer,
        types: BTreeSet<CardType>,
        maximum: u16,
        tapped: bool,
    ) -> Self {
        Self::move_all(
            ObjectQuery::Empty,
            ZoneDest::Delayed(DelayedZoneAction::PutHandPermanentsOntoBattlefield {
                player,
                types,
                maximum,
                tapped,
            }),
        )
    }

    /// Construct a delayed return of fixed object IDs to the battlefield under owner's control.
    #[must_use]
    pub fn return_fixed_to_battlefield(objects: Vec<ObjectId>, transformed: bool) -> Self {
        Self::move_all(
            ObjectQuery::Fixed(objects),
            ZoneDest::Battlefield {
                controller: ZoneController::Owner,
                tapped: false,
                attacking: false,
                transformed,
                face_down: None,
                entering_counters: Vec::new(),
                attach_to: None,
            },
        )
    }

    /// Whether any part of this `ZoneOpSpec` depends on an announced target.
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
            ZoneDest::Library {
                position: LibraryOrderPos::ChoiceTopOrBottom { chooser },
            } => chooser.uses_target(),
            ZoneDest::LookOnly { viewer } => viewer.uses_target(),
            ZoneDest::Delayed(action) => match action {
                DelayedZoneAction::PutHandPermanentsOntoBattlefield { player, .. } => {
                    PlayerSelector::Resolving(*player).uses_target()
                }
                DelayedZoneAction::ReturnChosenXGraveyardCreatures
                | DelayedZoneAction::ReturnGraveyardCreatureWithCorpse(_) => true,
                _ => false,
            },
            ZoneDest::SwapZones { other } => other.uses_target(),
            ZoneDest::None
            | ZoneDest::Graveyard { .. }
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
    /// Return all cards destroyed by the immediately preceding destroy instruction onto the
    /// battlefield under the resolving controller's control.
    #[must_use]
    pub fn return_destroyed_this_way() -> Self {
        Self::move_all(
            ObjectQuery::DestroyedThisWay,
            ZoneDest::Battlefield {
                controller: ZoneController::Player(Box::new(PlayerSelector::Resolving(
                    crate::program::ResolvingPlayer::Controller,
                ))),
                tapped: false,
                attacking: false,
                transformed: false,
                face_down: None,
                entering_counters: Vec::new(),
                attach_to: None,
            },
        )
    }

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
            ZoneDest::Library {
                position: LibraryOrderPos::ChoiceTopOrBottom { chooser },
            } => chooser.validate_vocabulary(vocabulary)?,
            ZoneDest::LookOnly { viewer } => viewer.validate_vocabulary(vocabulary)?,
            ZoneDest::SwapZones { other } => other.validate_vocabulary(vocabulary)?,
            ZoneDest::None
            | ZoneDest::Graveyard { .. }
            | ZoneDest::Hand
            | ZoneDest::Library { .. }
            | ZoneDest::RevealOnly
            | ZoneDest::Delayed(_) => {}
        }
        if let Some(shuf) = &self.shuffle_library {
            shuf.validate_vocabulary(vocabulary)?;
        }
        Ok(())
    }

    /// Lower a look-top-N choose pipeline into composable toolbox instructions.
    // All 12 parameters correspond directly to the orthogonal look-top-choose pipeline fields.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn look_top_choose_instructions(
        library_owner: ResolvingPlayer,
        chooser: ResolvingPlayer,
        count: ValueExpr,
        filter: Option<ObjectFilter>,
        max_picks: u8,
        take_all_matching: bool,
        keep_dest: LookTopKeepDest,
        rest_dest: LookTopRestDest,
        reveal_kept: bool,
        reveal_all: bool,
        reveal_only: bool,
        secondary: Option<(ObjectFilter, bool, bool, u8)>,
    ) -> Vec<Instruction> {
        look_top_choose_instructions(
            library_owner,
            chooser,
            count,
            filter,
            max_picks,
            take_all_matching,
            keep_dest,
            rest_dest,
            reveal_kept,
            reveal_all,
            reveal_only,
            secondary,
        )
    }

    /// Lower a reveal-from-top-until-N-matching pipeline into composable toolbox instructions.
    // All 9 parameters correspond directly to the orthogonal reveal-until-match pipeline fields.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn reveal_until_match_instructions(
        library_owner: ResolvingPlayer,
        chooser: ResolvingPlayer,
        filter: ObjectFilter,
        count: ValueExpr,
        match_dest: LookTopKeepDest,
        rest_dest: LookTopRestDest,
        keep_all_revealed: bool,
        optional_keep: bool,
        exile_instead_of_reveal: bool,
    ) -> Vec<Instruction> {
        reveal_until_match_instructions(
            library_owner,
            chooser,
            filter,
            count,
            match_dest,
            rest_dest,
            keep_all_revealed,
            optional_keep,
            exile_instead_of_reveal,
        )
    }
}

fn keep_dest_to_zone_dest(
    dest: LookTopKeepDest,
    chooser: ResolvingPlayer,
) -> (ZoneDest, Option<(ResolvedDuration, bool, bool, bool)>) {
    match dest {
        LookTopKeepDest::Hand => (ZoneDest::Hand, None),
        LookTopKeepDest::Battlefield { tapped, attacking } => (
            ZoneDest::Battlefield {
                controller: ZoneController::Player(Box::new(PlayerSelector::Resolving(chooser))),
                tapped,
                attacking,
                transformed: false,
                face_down: None,
                entering_counters: Vec::new(),
                attach_to: None,
            },
            None,
        ),
        LookTopKeepDest::LibraryTop => (
            ZoneDest::Library {
                position: LibraryOrderPos::Top,
            },
            None,
        ),
        LookTopKeepDest::Graveyard => (
            ZoneDest::Graveyard {
                semantics: GraveyardSemantics::Move,
            },
            None,
        ),
        LookTopKeepDest::Exile {
            playable,
            free,
            cast_only,
            any_color,
            duration,
        } => (
            ZoneDest::exile(),
            playable.then_some((duration, free, cast_only, any_color)),
        ),
        LookTopKeepDest::ExileLinked {
            face_down,
            playable,
            free,
            cast_only,
            any_color,
            duration,
        } => (
            ZoneDest::Exile {
                face_down,
                viewer: face_down.then_some(PlayerSelector::Resolving(chooser)),
                link_to_source: true,
                delayed_return: None,
                link_group: None,
            },
            playable.then_some((duration, free, cast_only, any_color)),
        ),
    }
}

fn rest_dest_to_zone_dest(dest: LookTopRestDest) -> ZoneDest {
    match dest {
        LookTopRestDest::BottomRandom => ZoneDest::Library {
            position: LibraryOrderPos::BottomRandom,
        },
        LookTopRestDest::BottomOrder => ZoneDest::Library {
            position: LibraryOrderPos::Bottom,
        },
        LookTopRestDest::TopOrder => ZoneDest::Library {
            position: LibraryOrderPos::Top,
        },
        LookTopRestDest::Graveyard => ZoneDest::Graveyard {
            semantics: GraveyardSemantics::Move,
        },
        LookTopRestDest::Exile => ZoneDest::exile(),
        LookTopRestDest::Hand => ZoneDest::Hand,
    }
}

/// Lower a look-top-N choose pipeline into composable toolbox instructions.
// All 12 parameters correspond directly to the orthogonal look-top-choose pipeline fields.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn look_top_choose_instructions(
    library_owner: ResolvingPlayer,
    chooser: ResolvingPlayer,
    count: ValueExpr,
    filter: Option<ObjectFilter>,
    max_picks: u8,
    take_all_matching: bool,
    keep_dest: LookTopKeepDest,
    rest_dest: LookTopRestDest,
    reveal_kept: bool,
    reveal_all: bool,
    reveal_only: bool,
    secondary: Option<(ObjectFilter, bool, bool, u8)>,
) -> Vec<Instruction> {
    const SLICE_BIND: BindingId = BindingId(10);
    const PRIMARY_CHOSEN_BIND: BindingId = BindingId(11);
    const PRIMARY_MOVED_BIND: BindingId = BindingId(12);
    const SECONDARY_CHOSEN_BIND: BindingId = BindingId(13);

    let mut step1 = ZoneOpSpec::move_all(
        ObjectQuery::ZoneScan {
            zone: QueryZone::Library(Box::new(PlayerSelector::Resolving(library_owner))),
            filter: Box::new(ObjectFilter::any()),
            slice: QuerySlice::Top(Box::new(count)),
        },
        if reveal_all || reveal_only {
            ZoneDest::RevealOnly
        } else {
            ZoneDest::LookOnly {
                viewer: PlayerSelector::Resolving(chooser),
            }
        },
    );
    step1.bind_moved = Some(SLICE_BIND);
    let mut ops = vec![Instruction::ZoneOp(Box::new(step1))];
    if reveal_only {
        return ops;
    }

    let eligible = match filter {
        Some(f) => ObjectQuery::filtered(ObjectQuery::Bound(SLICE_BIND), f),
        None => ObjectQuery::Bound(SLICE_BIND),
    };
    if take_all_matching {
        let mut bind_eligible = ZoneOpSpec::move_all(
            eligible,
            ZoneDest::LookOnly {
                viewer: PlayerSelector::Resolving(chooser),
            },
        );
        bind_eligible.bind_moved = Some(PRIMARY_CHOSEN_BIND);
        ops.push(Instruction::ZoneOp(Box::new(bind_eligible)));
    } else {
        let min_picks: u8 = if filter.is_none()
            && max_picks == 1
            && secondary.is_none()
            && !matches!(keep_dest, LookTopKeepDest::Battlefield { .. })
        {
            1
        } else {
            0
        };
        ops.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
            chooser: PlayerSelector::Resolving(chooser),
            domain: PromptDomain::Objects {
                query: eligible,
                min: Box::new(ValueExpr::literal(i64::from(min_picks))),
                max: Box::new(ValueExpr::literal(i64::from(max_picks))),
                constraint: None,
                max_total_power: None,
                hidden: false,
            },
            category: ChoiceCategory::Generic,
            bind_to: PRIMARY_CHOSEN_BIND,
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: true,
            option_branches: Vec::new(),
        })));
    }

    if reveal_kept && !reveal_all {
        ops.push(Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(
            ObjectQuery::Bound(PRIMARY_CHOSEN_BIND),
            ZoneDest::RevealOnly,
        ))));
    }

    let (keep_zone_dest, play_perm) = keep_dest_to_zone_dest(keep_dest, chooser);
    let mut move_primary =
        ZoneOpSpec::move_all(ObjectQuery::Bound(PRIMARY_CHOSEN_BIND), keep_zone_dest);
    move_primary.bind_moved = Some(PRIMARY_MOVED_BIND);
    ops.push(Instruction::ZoneOp(Box::new(move_primary)));
    if let Some((duration, free, cast_only, any_color)) = play_perm {
        ops.push(Instruction::ApplyContinuous(Box::new(
            ApplyContinuousSpec::exile_play_permission_for_duration(
                PlayerSelector::Resolving(chooser),
                ObjectQuery::Bound(PRIMARY_MOVED_BIND),
                duration,
                free,
                cast_only,
                any_color,
            ),
        )));
    }

    let mut final_rest = ObjectQuery::difference(
        ObjectQuery::Bound(SLICE_BIND),
        ObjectQuery::Bound(PRIMARY_CHOSEN_BIND),
    );
    if let Some((sec_filter, to_bf, enters_tapped, sec_max)) = secondary {
        let rem = final_rest;
        ops.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
            chooser: PlayerSelector::Resolving(chooser),
            domain: PromptDomain::Objects {
                query: ObjectQuery::filtered(rem.clone(), sec_filter),
                min: Box::new(ValueExpr::literal(0)),
                max: Box::new(ValueExpr::literal(i64::from(sec_max))),
                constraint: None,
                max_total_power: None,
                hidden: false,
            },
            category: ChoiceCategory::Generic,
            bind_to: SECONDARY_CHOSEN_BIND,
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: true,
            option_branches: Vec::new(),
        })));
        let sec_dest = if to_bf {
            ZoneDest::Battlefield {
                controller: ZoneController::Player(Box::new(PlayerSelector::Resolving(chooser))),
                tapped: enters_tapped,
                attacking: false,
                transformed: false,
                face_down: None,
                entering_counters: Vec::new(),
                attach_to: None,
            }
        } else {
            ZoneDest::Hand
        };
        ops.push(Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(
            ObjectQuery::Bound(SECONDARY_CHOSEN_BIND),
            sec_dest,
        ))));
        final_rest = ObjectQuery::difference(rem, ObjectQuery::Bound(SECONDARY_CHOSEN_BIND));
    }

    ops.push(Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(
        final_rest,
        rest_dest_to_zone_dest(rest_dest),
    ))));
    ops
}

/// Lower a reveal-from-top-until-N-matching pipeline into composable toolbox instructions.
// All 9 parameters correspond directly to the orthogonal reveal-until-match pipeline fields.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn reveal_until_match_instructions(
    library_owner: ResolvingPlayer,
    chooser: ResolvingPlayer,
    filter: ObjectFilter,
    count: ValueExpr,
    match_dest: LookTopKeepDest,
    rest_dest: LookTopRestDest,
    keep_all_revealed: bool,
    optional_keep: bool,
    exile_instead_of_reveal: bool,
) -> Vec<Instruction> {
    const SLICE_BIND: BindingId = BindingId(10);
    const PRIMARY_CHOSEN_BIND: BindingId = BindingId(11);
    const PRIMARY_MOVED_BIND: BindingId = BindingId(12);

    let mut step1 = ZoneOpSpec::move_all(
        ObjectQuery::ZoneScan {
            zone: QueryZone::Library(Box::new(PlayerSelector::Resolving(library_owner))),
            filter: Box::new(ObjectFilter::any()),
            slice: QuerySlice::UntilMatch {
                filter: Box::new(filter),
                count: Box::new(count.clone()),
                include_non_matches: true,
            },
        },
        if exile_instead_of_reveal {
            ZoneDest::exile()
        } else {
            ZoneDest::RevealOnly
        },
    );
    step1.bind_moved = Some(SLICE_BIND);
    let mut ops = vec![Instruction::ZoneOp(Box::new(step1))];

    let (match_zone_dest, play_perm) = keep_dest_to_zone_dest(match_dest, chooser);
    if keep_all_revealed {
        let mut move_all = ZoneOpSpec::move_all(ObjectQuery::Bound(SLICE_BIND), match_zone_dest);
        move_all.bind_moved = Some(PRIMARY_MOVED_BIND);
        ops.push(Instruction::ZoneOp(Box::new(move_all)));
        if let Some((duration, free, cast_only, any_color)) = play_perm {
            ops.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::exile_play_permission_for_duration(
                    PlayerSelector::Resolving(chooser),
                    ObjectQuery::Bound(PRIMARY_MOVED_BIND),
                    duration,
                    free,
                    cast_only,
                    any_color,
                ),
            )));
        }
        return ops;
    }

    let matching = ObjectQuery::filtered(ObjectQuery::Bound(SLICE_BIND), filter);
    if optional_keep {
        ops.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
            chooser: PlayerSelector::Resolving(chooser),
            domain: PromptDomain::Objects {
                query: matching,
                min: Box::new(ValueExpr::literal(0)),
                max: Box::new(count),
                constraint: None,
                max_total_power: None,
                hidden: false,
            },
            category: ChoiceCategory::Generic,
            bind_to: PRIMARY_CHOSEN_BIND,
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: true,
            option_branches: Vec::new(),
        })));
    } else {
        let mut bind_matching = ZoneOpSpec::move_all(
            matching,
            ZoneDest::LookOnly {
                viewer: PlayerSelector::Resolving(chooser),
            },
        );
        bind_matching.bind_moved = Some(PRIMARY_CHOSEN_BIND);
        ops.push(Instruction::ZoneOp(Box::new(bind_matching)));
    }

    let mut move_matched =
        ZoneOpSpec::move_all(ObjectQuery::Bound(PRIMARY_CHOSEN_BIND), match_zone_dest);
    move_matched.bind_moved = Some(PRIMARY_MOVED_BIND);
    ops.push(Instruction::ZoneOp(Box::new(move_matched)));
    if let Some((duration, free, cast_only, any_color)) = play_perm {
        ops.push(Instruction::ApplyContinuous(Box::new(
            ApplyContinuousSpec::exile_play_permission_for_duration(
                PlayerSelector::Resolving(chooser),
                ObjectQuery::Bound(PRIMARY_MOVED_BIND),
                duration,
                free,
                cast_only,
                any_color,
            ),
        )));
    }

    ops.push(Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(
        ObjectQuery::difference(
            ObjectQuery::Bound(SLICE_BIND),
            ObjectQuery::Bound(PRIMARY_CHOSEN_BIND),
        ),
        rest_dest_to_zone_dest(rest_dest),
    ))));
    ops
}
