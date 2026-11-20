//! Finite public rule failures, never a conversion of every engine error to blame.
use super::*;
use spellwake_engine::{
    actions::TimingError, combat::CombatError, effects::EffectError, game::GameError,
    mana::ManaPaymentError, opening::OpeningError, stack_window::StackWindowError, turn::TurnError,
};
use std::collections::BTreeMap;

/// Constructed only by this classifier, not deserializable client errors.
pub(super) struct RuleRejection(BTreeMap<String, String>);
impl RuleRejection {
    pub(super) fn from_bridge(error: &NativeBridgeError) -> Option<Self> {
        let pair = match error {
            NativeBridgeError::InvalidIntent => Some(("action", "invalid")),
            NativeBridgeError::InvalidReference | NativeBridgeError::WrongReferenceKind => {
                Some(("source", "invalid"))
            }
            NativeBridgeError::WrongDecision => Some(("decision", "stale")),
            NativeBridgeError::Unsupported => Some(("action", "unsupported")),
            NativeBridgeError::Native(error) => native(error),
            // Invalid binding, stale source and characteristic reads are not
            // established player mistakes. SQL owns verified stale classification.
            _ => None,
        }?;
        Some(Self(BTreeMap::from([(
            pair.0.to_owned(),
            pair.1.to_owned(),
        )])))
    }
    pub(super) fn errors(&self) -> &BTreeMap<String, String> {
        &self.0
    }
}
fn native(error: &GameError) -> Option<(&'static str, &'static str)> {
    match error {
        GameError::UnsupportedConcessionScope => Some(("action", "unsupported")),
        GameError::DecisionRequired => Some(("decision", "not permitted")),
        GameError::NoDecision => Some(("decision", "unavailable")),
        GameError::WrongDecision => Some(("decision", "stale")),
        GameError::WrongChooser => Some(("actor", "not permitted")),
        GameError::Opening(
            OpeningError::IllegalMulligan | OpeningError::InvalidBottom | OpeningError::WrongReply,
        ) => Some(("response", "invalid")),
        GameError::Turn(TurnError::LandLimitReached) => Some(("action", "not permitted")),
        GameError::Effect(error) => effect(error),
        // Unknown/changed source, pending host entropy/work/elimination, internal
        // ordering, observation and exhausted counters remain safe service errors.
        _ => None,
    }
}
fn effect(error: &EffectError) -> Option<(&'static str, &'static str)> {
    match error {
        EffectError::UnknownObject
        | EffectError::NotOwnHand
        | EffectError::NotLand
        | EffectError::NotCastableSpell
        | EffectError::NotAbilitySource
        | EffectError::UnknownActivatedAbility
        | EffectError::NotManaSource
        | EffectError::UnknownManaAbility => Some(("source", "invalid")),
        EffectError::TargetRequired
        | EffectError::UnexpectedTarget
        | EffectError::IllegalTarget => Some(("target", "invalid")),
        EffectError::InsufficientMana => Some(("payment", "not permitted")),
        EffectError::InvalidManaPayment(ManaPaymentError::InvalidCost) => None,
        EffectError::InvalidManaPayment(
            ManaPaymentError::NonPositiveSpend
            | ManaPaymentError::DuplicateType
            | ManaPaymentError::MissingRequired
            | ManaPaymentError::WrongTotal
            | ManaPaymentError::InsufficientMana
            | ManaPaymentError::PaymentRequired,
        ) => Some(("payment", "invalid")),
        EffectError::LegendarySpellPrerequisite
        | EffectError::CreatureTapRestricted
        | EffectError::Timing(
            TimingError::PriorityRequired
            | TimingError::NotPriorityHolder
            | TimingError::NotActivePlayer
            | TimingError::MainPhaseRequired
            | TimingError::StackNotEmpty,
        ) => Some(("action", "not permitted")),
        // Cost and turn state originate from the trusted source, not the intent.
        EffectError::Timing(TimingError::TurnRequired) => None,
        EffectError::InvalidSelectionCount
        | EffectError::DuplicateSelection
        | EffectError::SelectionOutsideDomain
        | EffectError::InvalidGraveyardPermutation
        | EffectError::IllegalOptionalEffect
        | EffectError::IllegalTimestampOrder
        | EffectError::IllegalManaColor
        | EffectError::IllegalCreatureType => Some(("response", "invalid")),
        EffectError::WrongChooser => Some(("actor", "not permitted")),
        EffectError::WrongReplacementChooserFrame
        | EffectError::WrongSelectionFrame
        | EffectError::WrongLibraryTopFrame
        | EffectError::WrongOptionalEffectFrame
        | EffectError::WrongResolutionPaymentFrame
        | EffectError::WrongTimestampOrderFrame
        | EffectError::WrongManaColorFrame => Some(("decision", "stale")),
        EffectError::Combat(error) => match error {
            CombatError::UnknownPlayer | CombatError::InvalidContext => None,
            CombatError::WrongFrame => Some(("decision", "stale")),
            CombatError::DuplicateAttacker
            | CombatError::IllegalAttacker
            | CombatError::IllegalDefender
            | CombatError::DuplicateBlocker
            | CombatError::IllegalBlocker
            | CombatError::IllegalBlockTarget
            | CombatError::AlreadyDeclared
            | CombatError::DuplicateAssignment
            | CombatError::IllegalDamageSource
            | CombatError::IllegalDamageRecipient
            | CombatError::InvalidDamageAmount
            | CombatError::IncorrectDamageTotal => Some(("response", "invalid")),
        },
        EffectError::Window(
            StackWindowError::NotOwnHand
            | StackWindowError::WrongController
            | StackWindowError::SourceTapped
            | StackWindowError::NotInPriority,
        ) => Some(("action", "not permitted")),
        // Not a generic fallback rule rejection: invariants, observer failures,
        // capacity and pending work never become permanent client errors here.
        _ => None,
    }
}
#[cfg(test)]
mod tests;
