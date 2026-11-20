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
    /// An authorized resolving sacrifice instruction, never a sacrifice cost.
    EffectSacrifice,
    /// An authorized resolving discard instruction, never cleanup discard.
    EffectDiscard,
    /// An authorized resolving mill instruction, never a mill cost.
    EffectMill,
    /// An authorized resolving surveil instruction.
    EffectSurveil,
    /// A source sacrificed as a cost.
    SacrificeCost,
    /// Cleanup rule discard.
    CleanupDiscard,
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
    /// Owner's graveyard.
    Graveyard,
    /// Exile.
    Exile,
}
impl PublicZoneDestination {
    pub(crate) fn physical(self) -> ObjectDestination {
        match self {
            Self::Graveyard => ObjectDestination::Graveyard,
            Self::Exile => ObjectDestination::Exile,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneReplacementSubject {
    /// Every eligible object.
    AnyObject,
    /// The source's exact incarnation.
    SourceSelf,
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
pub(super) fn validate(program: &super::Program) -> Result<(), DefinitionError> {
    if program.zone_replacements.is_empty() {
        return Ok(());
    }
    if !matches!(program.kind, ProgramKind::Permanent(_)) {
        return Err(DefinitionError::UnsupportedZoneReplacementScope);
    }
    let mut ids = BTreeSet::new();
    for ability in &program.zone_replacements {
        if !ids.insert(ability.id) {
            return Err(DefinitionError::DuplicateAbility);
        }
        let ZoneActionPredicate::OneOf(actions) = &ability.actions else {
            return Err(DefinitionError::UnsupportedZoneReplacementScope);
        };
        if actions.is_empty() || ability.destination == ability.replace_with {
            return Err(DefinitionError::InvalidZoneReplacementDefinition);
        }
        if actions.iter().any(|action| {
            !matches!(
                action,
                ZoneActionKind::EffectSacrifice
                    | ZoneActionKind::EffectDiscard
                    | ZoneActionKind::EffectMill
                    | ZoneActionKind::EffectSurveil
                    | ZoneActionKind::CleanupDiscard
                    | ZoneActionKind::StateBased
            )
        }) {
            return Err(DefinitionError::UnsupportedZoneReplacementScope);
        }
    }
    Ok(())
}
