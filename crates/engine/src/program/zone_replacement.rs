//! Closed authored predicates for the bounded public zone replacement slice.
use crate::{
    object_identity::ObjectDestination,
    program::{DefinitionError, ProgramKind},
};
use std::collections::BTreeSet;

/// Definition-local identity; source incarnation distinguishes actual instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZoneReplacementAbilityId(pub u64);
/// Authored action categories. Unsupported categories remain explicitly inadmissible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoneActionKind {
    /// A resolving effect destroys one or more permanents.
    EffectDestroy,
    /// A resolving effect exiles a battlefield cohort.
    EffectExile,
    /// An authorized resolving sacrifice instruction, never a sacrifice cost.
    EffectSacrifice,
    /// A resolving effect returns chosen controlled permanents to hand.
    EffectReturnSelected,
    /// A card from a looked-at library segment would enter hand.
    EffectLookToHand,
    /// An authorized resolving discard instruction, never cleanup discard.
    EffectDiscard,
    /// An authorized resolving mill instruction, never a mill cost.
    EffectMill,
    /// A card milled while paying an ordinary activated ability cost.
    MillCost,
    /// An authorized resolving surveil instruction.
    EffectSurveil,
    /// An effect puts a card or permanent onto the battlefield.
    EffectEnterBattlefield,
    /// A source sacrificed as a cost.
    SacrificeCost,
    /// An activated ability exiles its source as a cost.
    ExileCost,
    /// A searched card is put into its owner’s graveyard.
    EffectSearchToGraveyard,
    /// A revealed card put into the graveyard as part of a library partition.
    EffectRevealedToGraveyard,
    /// Move a graveyard card into its owner’s library before shuffling.
    EffectShuffleGraveyard,
    /// A resolving effect moves cards or permanents into their owners’ libraries.
    EffectPutIntoLibrary,
    /// Cleanup rule discard.
    CleanupDiscard,
    /// A player discards from hand as a cost while casting or activating.
    DiscardCost,
    /// State-based movement.
    StateBased,
    /// Surveil instruction movement.
    Surveil,
    /// Mill instruction movement.
    Mill,
    /// Draw movement.
    Draw,
    /// Countered spell movement.
    Countered,
    /// Resolving spell's exit.
    ResolutionExit,
    /// Return to hand.
    ReturnToHand,
    /// Land play.
    LandPlay,
    /// Cast announcement.
    CastAnnouncement,
}
/// General predicate syntax; admission never narrows unsupported members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneActionPredicate {
    /// Anywhere scope, deliberately unsupported by this initial slice.
    Any,
    /// Exact nonempty action set.
    OneOf(BTreeSet<ZoneActionKind>),
}
/// Public destinations supported as modifications of one candidate event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublicZoneDestination {
    /// The command zone, as selected by a designated commander’s owner.
    Command,
    /// Owner's hand.
    Hand,
    /// Owner's graveyard.
    Graveyard,
    /// Exile.
    Exile,
    /// The battlefield; the actual entering controller is retained by the owning move.
    Battlefield,
    /// Owner’s library.
    Library,
}
impl PublicZoneDestination {
    pub(crate) fn physical(
        self,
        entering_controller: Option<crate::priority::SeatId>,
    ) -> Option<ObjectDestination> {
        match self {
            Self::Command => Some(ObjectDestination::Command),
            Self::Library => Some(ObjectDestination::LibraryTop),
            Self::Hand => Some(ObjectDestination::Hand),
            Self::Graveyard => Some(ObjectDestination::Graveyard),
            Self::Exile => Some(ObjectDestination::Exile),
            Self::Battlefield => {
                entering_controller.map(|controller| ObjectDestination::Battlefield { controller })
            }
        }
    }
}
/// Physical origin filter, independent of current card types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneObjectKind {
    /// Physical card.
    Card,
    /// Token.
    Token,
    /// Either origin.
    Either,
}
/// Bounded object relation to the actual replacement source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneReplacementSubject {
    /// Every eligible object.
    AnyObject,
    /// The source's exact incarnation.
    SourceSelf,
    /// Objects matching an orthogonal [`super::ObjectFilter`] and optional owner relation.
    Matching {
        filter: Box<super::ObjectFilter>,
        owner_relation: Option<super::PlayerRelation>,
    },
    /// Creatures dealt damage by the replacement source earlier this turn (CR 614.6).
    DamagedBySourceThisTurn,
}
/// Optionality always belongs to the affected object controller or owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneReplacementChoice {
    /// Apply when selected.
    Mandatory,
    /// Ask even for a singleton application.
    OptionalAffectedPlayer,
}
/// Copiable permanent ability functioning only on the battlefield.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneReplacementAbility {
    /// Definition-local identity, unique within its Program.
    pub id: ZoneReplacementAbilityId,
    /// Exact attempted-action predicate.
    pub actions: ZoneActionPredicate,
    /// Physical card/token origin filter.
    pub objects: ZoneObjectKind,
    /// Relation to the actual current source incarnation.
    pub subject: ZoneReplacementSubject,
    /// Current candidate destination to replace.
    pub destination: PublicZoneDestination,
    /// New candidate destination; no intermediate physical movement occurs.
    pub replace_with: PublicZoneDestination,
    /// Mandatory or affected-player optional application.
    pub choice: ZoneReplacementChoice,
}
pub(super) fn validate(
    program: &super::Program,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Result<(), DefinitionError> {
    if program.zone_replacements.is_empty() {
        return Ok(());
    }
    let is_permanent = matches!(program.kind, ProgramKind::Permanent(_));
    let mut ids = BTreeSet::new();
    for ability in &program.zone_replacements {
        if !ids.insert(ability.id) {
            return Err(DefinitionError::DuplicateAbility);
        }
        if let ZoneReplacementSubject::Matching { filter, .. } = &ability.subject {
            filter.validate_vocabulary(vocabulary)?;
        }
        if !is_permanent
            && !(ability.subject == ZoneReplacementSubject::SourceSelf
                && ability.destination == PublicZoneDestination::Graveyard
                && matches!(
                    ability.replace_with,
                    PublicZoneDestination::Exile | PublicZoneDestination::Library
                ))
        {
            return Err(DefinitionError::UnsupportedZoneReplacementScope);
        }
        if ability.actions == ZoneActionPredicate::Any
            && ability.destination == PublicZoneDestination::Graveyard
            && matches!(
                ability.replace_with,
                PublicZoneDestination::Exile | PublicZoneDestination::Library
            )
            && ability.choice == ZoneReplacementChoice::Mandatory
        {
            continue;
        }
        let ZoneActionPredicate::OneOf(actions) = &ability.actions else {
            return Err(DefinitionError::UnsupportedZoneReplacementScope);
        };
        if actions.is_empty() || ability.destination == ability.replace_with {
            return Err(DefinitionError::InvalidZoneReplacementDefinition);
        }
        if ability.destination == PublicZoneDestination::Command
            || ability.replace_with == PublicZoneDestination::Command
            || (ability.replace_with == PublicZoneDestination::Library
                && !(ability.destination == PublicZoneDestination::Graveyard
                    && ability.choice == ZoneReplacementChoice::Mandatory))
            || (ability.destination == PublicZoneDestination::Library
                && !actions.iter().all(|action| {
                    matches!(
                        action,
                        ZoneActionKind::EffectShuffleGraveyard
                            | ZoneActionKind::EffectPutIntoLibrary
                    )
                }))
            || ability.replace_with == PublicZoneDestination::Battlefield
            || (ability.destination == PublicZoneDestination::Battlefield
                && actions != &BTreeSet::from([ZoneActionKind::EffectEnterBattlefield]))
        {
            return Err(DefinitionError::UnsupportedZoneReplacementScope);
        }
        if (ability.destination == PublicZoneDestination::Hand
            || ability.replace_with == PublicZoneDestination::Hand)
            && actions != &BTreeSet::from([ZoneActionKind::EffectReturnSelected])
            && actions != &BTreeSet::from([ZoneActionKind::EffectEnterBattlefield])
            && actions != &BTreeSet::from([ZoneActionKind::EffectLookToHand])
            && actions != &BTreeSet::from([ZoneActionKind::SacrificeCost])
            && actions != &BTreeSet::from([ZoneActionKind::ExileCost])
            && actions != &BTreeSet::from([ZoneActionKind::EffectRevealedToGraveyard])
            && actions != &BTreeSet::from([ZoneActionKind::EffectSearchToGraveyard])
            && !actions.iter().all(|action| {
                matches!(
                    action,
                    ZoneActionKind::EffectShuffleGraveyard | ZoneActionKind::EffectPutIntoLibrary
                )
            })
        {
            return Err(DefinitionError::UnsupportedZoneReplacementScope);
        }
        if actions.iter().any(|action| {
            !matches!(
                action,
                ZoneActionKind::EffectExile
                    | ZoneActionKind::EffectDestroy
                    | ZoneActionKind::EffectSacrifice
                    | ZoneActionKind::EffectReturnSelected
                    | ZoneActionKind::EffectLookToHand
                    | ZoneActionKind::EffectDiscard
                    | ZoneActionKind::EffectPutIntoLibrary
                    | ZoneActionKind::EffectShuffleGraveyard
                    | ZoneActionKind::EffectRevealedToGraveyard
                    | ZoneActionKind::EffectSearchToGraveyard
                    | ZoneActionKind::EffectMill
                    | ZoneActionKind::MillCost
                    | ZoneActionKind::EffectSurveil
                    | ZoneActionKind::EffectEnterBattlefield
                    | ZoneActionKind::SacrificeCost
                    | ZoneActionKind::ExileCost
                    | ZoneActionKind::DiscardCost
                    | ZoneActionKind::CleanupDiscard
                    | ZoneActionKind::StateBased
            )
        }) {
            return Err(DefinitionError::UnsupportedZoneReplacementScope);
        }
    }
    Ok(())
}
