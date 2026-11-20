//! Facts emitted by one candidate, separate from the accepted game state.
use super::{
    AbilityActivationRecord, CombatAssignmentRecord, CombatDeclarationRecord, DamageJournal,
    DrawJournal, LandPlayRecord, ManaPaymentRecord, ManaProductionRecord, PermanentEntryRecord,
    ShuffleRecord, TurnTransition,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct TransitionFacts {
    pub(super) rules_journal: crate::occurrence::RulesJournal,
    pub(super) check_journal: super::CheckJournal,
    pub(super) permanent_entry: Option<PermanentEntryRecord>,
    pub(super) land_play: Option<LandPlayRecord>,
    pub(super) damage_journal: DamageJournal,
    pub(super) mana_payment: Option<ManaPaymentRecord>,
    pub(super) ability_activation: Option<AbilityActivationRecord>,
    pub(super) mana_production: Option<ManaProductionRecord>,
    pub(super) turn_transitions: Vec<TurnTransition>,
    pub(super) draw_journal: DrawJournal,
    pub(super) shuffle_journal: Vec<ShuffleRecord>,
    pub(super) combat_declarations: Vec<CombatDeclarationRecord>,
    pub(super) combat_assignments: Vec<CombatAssignmentRecord>,
}
