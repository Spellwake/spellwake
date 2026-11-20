//! Typed, nonrecursive scalar conditions and admission-only control-flow facts.
use super::{BodyContext, CreatureTypeSlot, DefinitionError};
use spellwake_contracts::RulesInteger;
use std::collections::BTreeSet;

/// A signed scalar read at the current resolving instruction, not an effect amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionValue {
    /// An exact signed integer, with no machine-integer truncation.
    Literal(RulesInteger),
    /// Current life of the resolving entry's captured controller.
    ControllerLife,
    /// Complete occurrence count captured by the actual triggered stack entry.
    MatchedOccurrences,
}
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
    pub left: ConditionValue,
    /// Relation to test.
    pub comparison: Comparison,
    /// Second signed operand.
    pub right: ConditionValue,
}
impl ResolutionCondition {
    pub(super) fn validate(&self, context: BodyContext) -> Result<(), DefinitionError> {
        for value in [&self.left, &self.right] {
            match (value, context) {
                (ConditionValue::MatchedOccurrences, BodyContext::Unbound) => {
                    return Err(DefinitionError::UnboundTriggerCondition);
                }
                (ConditionValue::Literal(_) | ConditionValue::ControllerLife, _)
                | (ConditionValue::MatchedOccurrences, BodyContext::TriggerBound { .. }) => {}
            }
        }
        Ok(())
    }
}

/// Static facts only. The live resolution cursor retains the sole local values.
#[derive(Clone, Default)]
pub(super) struct FlowFacts {
    pub definitely: BTreeSet<CreatureTypeSlot>,
    pub possibly: BTreeSet<CreatureTypeSlot>,
    pub donor_moved: bool,
    /// A reachable predecessor may have removed the bound spell from the stack.
    pub target_spell_departed: bool,
}
impl FlowFacts {
    pub fn merge_into(&self, destination: &mut Option<Self>) {
        if let Some(previous) = destination {
            previous
                .definitely
                .retain(|slot| self.definitely.contains(slot));
            previous.possibly.extend(self.possibly.iter().copied());
            previous.donor_moved |= self.donor_moved;
            previous.target_spell_departed |= self.target_spell_departed;
        } else {
            *destination = Some(self.clone());
        }
    }
}
