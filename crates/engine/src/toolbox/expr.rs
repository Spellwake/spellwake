//! Universal composable boolean [`ConditionExpr`] algebra.

use crate::{
    program::{Comparison, DefinitionError, ObjectFilter, ValueExpr},
    toolbox::{
        env::BindingId,
        query::ObjectQuery,
        tags::{TagKey, TagValue},
    },
};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;

/// Composable boolean expression evaluated against live game state and [`crate::toolbox::ExecEnv`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionExpr {
    /// Constant boolean literal.
    Always(bool),
    /// Numeric comparison between any two [`ValueExpr`]s.
    Compare {
        left: Box<ValueExpr>,
        op: Comparison,
        right: Box<ValueExpr>,
    },
    /// Read a boolean flag from [`crate::toolbox::ExecEnv`] (defaults to `false` if unset).
    BoundBool(BindingId),
    /// True if `query` resolves to at least one object.
    NonEmpty(Box<ObjectQuery>),
    /// True if every object returned by `query` (and at least one when `require_non_empty`) matches `filter`.
    AllMatch {
        query: Box<ObjectQuery>,
        filter: Box<ObjectFilter>,
        require_non_empty: bool,
    },
    /// True if at least one object returned by `query` matches `filter`.
    AnyMatch {
        query: Box<ObjectQuery>,
        filter: Box<ObjectFilter>,
    },
    /// True if at least one object returned by `query` carries `key` (matching `value` if specified).
    HasTag {
        query: Box<ObjectQuery>,
        key: TagKey,
        value: Option<TagValue>,
    },
    /// Logical conjunction (short-circuiting).
    And(Vec<ConditionExpr>),
    /// Logical disjunction (short-circuiting).
    Or(Vec<ConditionExpr>),
    /// Logical negation.
    Not(Box<ConditionExpr>),
    /// Typed resolution-time scalar comparison condition.
    Resolution(Box<crate::program::ResolutionCondition>),
}

impl ConditionExpr {
    /// Whether this condition depends on an announced target.
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Always(_) | Self::BoundBool(_) => false,
            Self::Compare { left, right, .. } => left.uses_target() || right.uses_target(),
            Self::Resolution(cond) => cond.left.uses_target() || cond.right.uses_target(),
            Self::NonEmpty(q) | Self::HasTag { query: q, .. } => q.uses_target(),
            Self::AllMatch { query, filter, .. } | Self::AnyMatch { query, filter } => {
                query.uses_target() || filter.uses_target_permanent()
            }
            Self::And(cs) | Self::Or(cs) => cs.iter().any(Self::uses_target),
            Self::Not(c) => c.uses_target(),
        }
    }

    /// Validate all nested expressions and queries against `vocabulary`.
    ///
    /// # Errors
    /// Returns [`DefinitionError::InvalidInstruction`] if any nested filter or expression is invalid.
    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Always(_) | Self::BoundBool(_) => Ok(()),
            Self::Compare { left, right, .. } => {
                left.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                right
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)
            }
            Self::Resolution(cond) => {
                cond.left
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                cond.right
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)
            }
            Self::NonEmpty(q) | Self::HasTag { query: q, .. } => q.validate_vocabulary(vocabulary),
            Self::AllMatch { query, filter, .. } | Self::AnyMatch { query, filter } => {
                query.validate_vocabulary(vocabulary)?;
                filter
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)
            }
            Self::And(cs) | Self::Or(cs) => {
                for c in cs {
                    c.validate_vocabulary(vocabulary)?;
                }
                Ok(())
            }
            Self::Not(c) => c.validate_vocabulary(vocabulary),
        }
    }
}
