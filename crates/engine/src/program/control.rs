//! Typed scalar conditions and admission-only control-flow facts.
use super::{BodyContext, CreatureTypeSlot, DefinitionError, ValueExpr};
use spellwake_contracts::RulesInteger;
use std::collections::BTreeSet;

/// An exact integer comparison. Boolean compositions use forward instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// Left equals right.
    Equal,
    /// Left differs from right.
    NotEqual,
    /// Left is less than right.
    Less,
    /// Left is less than or equal to right.
    LessOrEqual,
    /// Left is greater than right.
    Greater,
    /// Left is greater than or equal to right.
    GreaterOrEqual,
}
impl Comparison {
    pub(crate) fn evaluate(self, left: &RulesInteger, right: &RulesInteger) -> bool {
        match self {
            Self::Equal => left == right,
            Self::NotEqual => left != right,
            Self::Less => left < right,
            Self::LessOrEqual => left <= right,
            Self::Greater => left > right,
            Self::GreaterOrEqual => left >= right,
        }
    }
}
/// A resolution-time condition. This does not implement an intervening-if clause
/// or record a cost payment; those have their own discovery/resolution contexts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionCondition {
    /// First signed operand.
    pub left: ValueExpr,
    /// Relation to test.
    pub comparison: Comparison,
    /// Second signed operand.
    pub right: ValueExpr,
}
impl ResolutionCondition {
    pub(crate) fn validate(
        &self,
        context: BodyContext<'_>,
        vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.left.validate_condition(context, vocabulary)?;
        self.right.validate_condition(context, vocabulary)
    }

    /// True if both sides can be evaluated purely against a world snapshot without resolution/stack state.
    #[must_use]
    pub fn is_world_evaluable(&self) -> bool {
        self.left.is_world_evaluable() && self.right.is_world_evaluable()
    }

    /// True if both sides can be evaluated at trigger time using a world snapshot plus trigger event context.
    #[must_use]
    pub fn is_trigger_world_evaluable(&self) -> bool {
        self.left.is_trigger_world_evaluable() && self.right.is_trigger_world_evaluable()
    }

    /// Validate that both sides are trigger-world-evaluable and reference only known creature types.
    pub(crate) fn validate_trigger_world(
        &self,
        vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.left.validate_trigger_world_condition(vocabulary)?;
        self.right.validate_trigger_world_condition(vocabulary)
    }

    /// Validate that both sides are purely world-evaluable and reference only known creature types.
    pub fn validate_world(
        &self,
        vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.left.validate_world_condition(vocabulary)?;
        self.right.validate_world_condition(vocabulary)
    }

    /// Rewrite printed creature-type words when a text-changing effect applies.
    pub(crate) fn rewrite_creature_type(
        &mut self,
        from: spellwake_contracts::creature_types::CreatureTypeId,
        to: spellwake_contracts::creature_types::CreatureTypeId,
    ) {
        self.left.rewrite_creature_type(from, to);
        self.right.rewrite_creature_type(from, to);
    }
}

/// Static facts only. The live resolution cursor retains the sole local values.
#[derive(Clone, Default)]
pub(super) struct FlowFacts {
    pub(super) exact_creature_target: bool,
    pub(super) has_object_or_spell_target: bool,
    pub definitely: BTreeSet<CreatureTypeSlot>,
    pub possibly: BTreeSet<CreatureTypeSlot>,
    pub donor_moved: bool,
    pub definitely_exiled_target: bool,
    pub maybe_exiled_target: bool,
    pub remembered_permanent: bool,
    pub definitely_discarded: bool,
    pub maybe_discarded: bool,
    pub definitely_destroyed: bool,
    pub maybe_destroyed: bool,
    pub checked_destruction_target: bool,
    pub maybe_checked_destruction_target: bool,
    pub moved_library_cards: bool,
    pub maybe_moved_library_cards: bool,
    pub definitely_milled: bool,
    pub definitely_milled_top: bool,
    pub definitely_exiled_milled: bool,
    pub definitely_checked_top: bool,
    pub maybe_checked_top: bool,
    pub definitely_sacrificed: bool,
    pub maybe_sacrificed: bool,
    pub definitely_selected_graveyard_exile: bool,
    pub returned_graveyard_target: bool,
    pub defined_resolution_x: bool,
}
impl FlowFacts {
    /// Promote any `maybe_*` facts produced by a prior optional action onto `definitely_*`
    /// when control enters the fallthrough branch of `BranchIfFalse(LastActionSucceeded >= 1)`.
    pub fn promote_maybe_to_definite(&mut self) {
        self.definitely_exiled_target |= self.maybe_exiled_target;
        self.definitely_discarded |= self.maybe_discarded;
        self.definitely_destroyed |= self.maybe_destroyed;
        self.checked_destruction_target |= self.maybe_checked_destruction_target;
        self.moved_library_cards |= self.maybe_moved_library_cards;
        self.definitely_checked_top |= self.maybe_checked_top;
        self.definitely_sacrificed |= self.maybe_sacrificed;
    }

    /// Snapshot current `definitely_*` facts into `maybe_*` so an optional branch keeps
    /// them available for guarded follow-ups (`If you do, ...`).
    pub fn record_maybe_from_definite(&mut self) {
        self.maybe_exiled_target |= self.definitely_exiled_target;
        self.maybe_discarded |= self.definitely_discarded;
        self.maybe_destroyed |= self.definitely_destroyed;
        self.maybe_checked_destruction_target |= self.checked_destruction_target;
        self.maybe_moved_library_cards |= self.moved_library_cards;
        self.maybe_checked_top |= self.definitely_checked_top;
        self.maybe_sacrificed |= self.definitely_sacrificed;
    }

    pub fn merge_into(&self, destination: &mut Option<Self>) {
        if let Some(previous) = destination {
            previous
                .definitely
                .retain(|slot| self.definitely.contains(slot));
            previous.possibly.extend(self.possibly.iter().copied());
            previous.donor_moved |= self.donor_moved;
            previous.definitely_exiled_target &= self.definitely_exiled_target;
            previous.maybe_exiled_target |= self.maybe_exiled_target;
            previous.remembered_permanent &= self.remembered_permanent;
            previous.definitely_discarded &= self.definitely_discarded;
            previous.maybe_discarded |= self.maybe_discarded;
            previous.definitely_destroyed &= self.definitely_destroyed;
            previous.maybe_destroyed |= self.maybe_destroyed;
            previous.checked_destruction_target &= self.checked_destruction_target;
            previous.maybe_checked_destruction_target |= self.maybe_checked_destruction_target;
            previous.moved_library_cards &= self.moved_library_cards;
            previous.maybe_moved_library_cards |= self.maybe_moved_library_cards;
            previous.definitely_milled &= self.definitely_milled;
            previous.definitely_milled_top &= self.definitely_milled_top;
            previous.definitely_checked_top &= self.definitely_checked_top;
            previous.maybe_checked_top |= self.maybe_checked_top;
            previous.definitely_sacrificed &= self.definitely_sacrificed;
            previous.maybe_sacrificed |= self.maybe_sacrificed;
            previous.definitely_selected_graveyard_exile &=
                self.definitely_selected_graveyard_exile;
            previous.definitely_exiled_milled &= self.definitely_exiled_milled;
            previous.returned_graveyard_target |= self.returned_graveyard_target;
            previous.defined_resolution_x &= self.defined_resolution_x;
        } else {
            *destination = Some(self.clone());
        }
    }
}
