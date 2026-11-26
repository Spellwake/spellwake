//! Universal `ApplyContinuous` primitive.
//!
//! Unifies temporary spell/ability continuous effects, battlefield layer grants,
//! combat/cast/activation restrictions, cost modifiers, event replacements, and
//! emblems around the single orthogonal [`StaticContinuousAbility`] representation.

use crate::{
    characteristics::{ResolvedDuration, StaticContinuousAbility},
    program::DefinitionError,
    toolbox::{
        expr::ConditionExpr,
        query::{ObjectQuery, PlayerSelector},
    },
};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

/// Specification for `Instruction::ApplyContinuous`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyContinuousSpec {
    /// Optional object set locked at resolution time (CR 611.2c).
    /// When `None`, the embedded `rule`'s own selector applies dynamically.
    pub selector: Option<ObjectQuery>,
    /// The orthogonal continuous, restriction, cost, damage, or replacement rule.
    pub rule: StaticContinuousAbility,
    /// How long the effect persists.
    pub duration: ResolvedDuration,
    /// Optional live condition evaluated before applying the effect.
    pub condition: Option<ConditionExpr>,
    /// If true, consume this effect after its first application (one-shot shield/replacement/discount).
    pub one_shot: bool,
    /// If set, install as a permanent command-zone emblem controlled by the selected player(s).
    pub as_emblem: Option<PlayerSelector>,
    /// Human-readable emblem label when `as_emblem` is `Some`.
    pub emblem_label: Option<String>,
}

impl ApplyContinuousSpec {
    /// Whether this spec depends on an announced target.
    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.selector.as_ref().is_some_and(ObjectQuery::uses_target)
            || self
                .condition
                .as_ref()
                .is_some_and(ConditionExpr::uses_target)
            || self
                .as_emblem
                .as_ref()
                .is_some_and(PlayerSelector::uses_target)
    }

    /// Validate nested queries and conditions against `vocabulary`.
    ///
    /// # Errors
    /// Returns [`DefinitionError::InvalidInstruction`] if any nested query or condition is invalid.
    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        if let Some(q) = &self.selector {
            q.validate_vocabulary(vocabulary)?;
        }
        if let Some(c) = &self.condition {
            c.validate_vocabulary(vocabulary)?;
        }
        if let Some(p) = &self.as_emblem {
            p.validate_vocabulary(vocabulary)?;
        }
        Ok(())
    }
}
