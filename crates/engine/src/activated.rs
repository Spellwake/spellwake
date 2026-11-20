//! Authored targeted non-mana abilities on represented permanent sources.
//! The executor shares existing costs, targets, stack payloads and resolution.
use crate::{
    effects::EffectWindow,
    mana::{ManaCost, ManaPaymentRecord},
    object_identity::{GameObject, ObjectId},
    priority::SeatId,
    program::ExecutableBody,
    stack_window::StackEntryId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActivatedAbilityId(pub u64);
/// Cost paid from the actual source, distinct from a resolution instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceCost {
    /// Only the separately represented mana cost is paid; no physical source change.
    None,
    Tap,
    Sacrifice,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivatedAbility {
    pub id: ActivatedAbilityId,
    pub mana_cost: ManaCost,
    pub source_cost: SourceCost,
    pub body: ExecutableBody,
}
/// Current privileged activation evidence. Source is the actual pre-cost snapshot,
/// not complete LKI or a physical card placed on the stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityActivationRecord {
    pub entry: StackEntryId,
    pub ability: ActivatedAbilityId,
    pub controller: SeatId,
    pub source: GameObject,
    pub source_cost: SourceCost,
    pub resulting_source: ObjectId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityActivationResult {
    pub activation: AbilityActivationRecord,
    pub payment: ManaPaymentRecord,
}
pub type AbilityActivationTransition =
    crate::occurrence::RulesTransition<EffectWindow, AbilityActivationResult>;
