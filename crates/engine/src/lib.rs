//! Pure rules mechanisms. No accounts, persistence, networking or full card execution.
pub mod card_identity;
pub mod characteristics;
pub mod damage;
pub mod game;
pub mod mana;
pub mod priority;
pub mod stack_window;
pub mod targets;

use spellwake_contracts::RulesInteger;
use std::collections::BTreeSet;

/// Opaque target identity local to one decision, not a seat authorization token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetId(pub u32);

/// Caller-supplied eligible targets from the current rules decision.
/// This mechanism does not establish targeting legality by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistributionDecision {
    pub revision: u64,
    pub total: RulesInteger,
    pub minimum_targets: usize,
    pub maximum_targets: usize,
    pub eligible_targets: BTreeSet<TargetId>,
}

/// An allocation preserves the exact quantity and the decision-local target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    pub target: TargetId,
    pub amount: RulesInteger,
}

/// Constructible only by validation against a specific decision revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedDistribution {
    revision: u64,
    allocations: Vec<Allocation>,
}
impl ValidatedDistribution {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn allocations(&self) -> &[Allocation] {
        &self.allocations
    }
}

/// Rules rejection is distinct from an invalid internal decision configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributionError {
    InvalidDecision,
    StaleRevision,
    TargetCount,
    IneligibleTarget,
    DuplicateTarget,
    NonpositiveAmount,
    IncorrectTotal,
}
impl DistributionError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidDecision => "invalid-decision",
            Self::StaleRevision => "stale-revision",
            Self::TargetCount => "target-count",
            Self::IneligibleTarget => "ineligible-target",
            Self::DuplicateTarget => "duplicate-target",
            Self::NonpositiveAmount => "nonpositive-amount",
            Self::IncorrectTotal => "incorrect-total",
        }
    }
}

/// Validate a positive distribution when choices are made, before any effects or costs.
/// CR 601.2d requires at least one assigned to each target. Resolution-time target
/// rechecking and damage replacement/prevention belong to later engine mechanisms.
pub fn validate_distribution(
    decision: &DistributionDecision,
    expected_revision: u64,
    allocations: Vec<Allocation>,
) -> Result<ValidatedDistribution, DistributionError> {
    if !decision.total.is_positive()
        || decision.minimum_targets == 0
        || decision.maximum_targets < decision.minimum_targets
    {
        return Err(DistributionError::InvalidDecision);
    }
    if expected_revision != decision.revision {
        return Err(DistributionError::StaleRevision);
    }
    if !(decision.minimum_targets..=decision.maximum_targets).contains(&allocations.len()) {
        return Err(DistributionError::TargetCount);
    }
    let mut selected = BTreeSet::new();
    let mut sum = RulesInteger::from(0);
    for allocation in &allocations {
        if !decision.eligible_targets.contains(&allocation.target) {
            return Err(DistributionError::IneligibleTarget);
        }
        if !selected.insert(allocation.target) {
            return Err(DistributionError::DuplicateTarget);
        }
        if !allocation.amount.is_positive() {
            return Err(DistributionError::NonpositiveAmount);
        }
        sum = sum.plus(&allocation.amount);
    }
    if sum != decision.total {
        return Err(DistributionError::IncorrectTotal);
    }
    Ok(ValidatedDistribution {
        revision: decision.revision,
        allocations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_never_mutates_the_decision_and_is_deterministic() {
        let decision = DistributionDecision {
            revision: 9,
            total: 3.into(),
            minimum_targets: 1,
            maximum_targets: 3,
            eligible_targets: BTreeSet::from([TargetId(1), TargetId(2)]),
        };
        let original = decision.clone();
        let choice = vec![
            Allocation {
                target: TargetId(1),
                amount: 2.into(),
            },
            Allocation {
                target: TargetId(2),
                amount: 1.into(),
            },
        ];
        let first = validate_distribution(&decision, 9, choice.clone()).unwrap();
        assert_eq!(first, validate_distribution(&decision, 9, choice).unwrap());
        assert_eq!(first.revision(), 9);
        assert_eq!(first.allocations().len(), 2);
        assert_eq!(decision, original);
        assert_eq!(
            validate_distribution(&decision, 8, vec![]),
            Err(DistributionError::StaleRevision)
        );
        assert_eq!(decision, original);
    }
}

pub mod effects;
