//! Resolution-time target identity and occurrence bookkeeping (CR 115.3, 115.5, 608.2b).
//! Full type/color/protection/range/hexproof legality is a mandatory trusted kernel
//! input, not computed here. No client may provide those legality assessments.
use crate::{
    TargetId,
    card_identity::{CardObjectId, CardZone},
    priority::SeatId,
    stack_window::StackEntryId,
};
use std::collections::{BTreeMap, BTreeSet};

/// One occurrence of the word target in a compiled instruction, not one selected object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetClauseId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetRef {
    Card(CardObjectId),
    Player(SeatId),
    Stack(StackEntryId),
}
/// Finite materialized choices only; symbolic large domains remain future work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetClause {
    pub id: TargetClauseId,
    pub minimum: usize,
    pub maximum: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetChoice {
    pub id: TargetId,
    pub clause: TargetClauseId,
    pub target: TargetRef,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundTarget {
    choice: TargetChoice,
    zone: Option<CardZone>,
}
impl BoundTarget {
    pub fn choice(&self) -> TargetChoice {
        self.choice
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TargetPlan {
    targets: Vec<BoundTarget>,
}

/// Same-window lookup owned by the native stack. Not a browser-supplied registry.
pub(crate) trait TargetWorld {
    fn card_zone(&self, card: CardObjectId) -> Option<CardZone>;
    fn player_exists(&self, player: SeatId) -> bool;
    fn stack_exists(&self, entry: StackEntryId) -> bool;
    fn stack_precedes(&self, target: StackEntryId, source: StackEntryId) -> bool;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetError {
    InvalidClause,
    DuplicateClause,
    UnknownClause,
    TargetCount,
    DuplicateKey,
    RepeatedTarget,
    MissingTarget,
    UseStackReference,
    SelfTarget,
    TargetNotYetOnStack,
    MissingCheck,
    DuplicateCheck,
    UnknownCheck,
}
/// Additional CR 608.2b legality for one chosen occurrence, evaluated by the
/// enclosing kernel against this exact window revision and source/controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetCheck {
    pub target: TargetId,
    pub legal: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetLegality {
    Legal,
    ObjectGone,
    ZoneChanged,
    PlayerGone,
    StackEntryGone,
    AdditionalRulesIllegal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetResult {
    pub choice: TargetChoice,
    pub legality: TargetLegality,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionPermission {
    NoTargets,
    SomeTargetsLegal,
    AllTargetsIllegal,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetAssessment {
    results: Vec<TargetResult>,
    permission: ResolutionPermission,
}
impl TargetAssessment {
    pub fn results(&self) -> &[TargetResult] {
        &self.results
    }
    pub fn permission(&self) -> ResolutionPermission {
        self.permission
    }
    pub fn get(&self, id: TargetId) -> Option<&TargetResult> {
        self.results.iter().find(|result| result.choice.id == id)
    }
    pub(crate) fn no_targets() -> Self {
        Self {
            results: Vec::new(),
            permission: ResolutionPermission::NoTargets,
        }
    }
}
impl TargetPlan {
    pub fn targets(&self) -> &[BoundTarget] {
        &self.targets
    }
    pub(crate) fn capture(
        world: &impl TargetWorld,
        source: StackEntryId,
        clauses: Vec<TargetClause>,
        choices: Vec<TargetChoice>,
    ) -> Result<Self, TargetError> {
        let mut definitions = BTreeMap::new();
        for clause in clauses {
            if clause.minimum > clause.maximum {
                return Err(TargetError::InvalidClause);
            }
            if definitions.insert(clause.id, clause).is_some() {
                return Err(TargetError::DuplicateClause);
            }
        }
        let mut keys = BTreeSet::new();
        let mut selected = BTreeSet::new();
        let mut counts = BTreeMap::<TargetClauseId, usize>::new();
        let mut targets = Vec::new();
        for choice in choices {
            if !definitions.contains_key(&choice.clause) {
                return Err(TargetError::UnknownClause);
            }
            if !keys.insert(choice.id) {
                return Err(TargetError::DuplicateKey);
            }
            if !selected.insert((choice.clause, choice.target)) {
                return Err(TargetError::RepeatedTarget);
            }
            let zone = match choice.target {
                TargetRef::Card(card) => {
                    let zone = world.card_zone(card).ok_or(TargetError::MissingTarget)?;
                    if zone == CardZone::Stack {
                        return Err(TargetError::UseStackReference);
                    }
                    Some(zone)
                }
                TargetRef::Player(player) => {
                    if !world.player_exists(player) {
                        return Err(TargetError::MissingTarget);
                    }
                    None
                }
                TargetRef::Stack(entry) => {
                    if entry == source {
                        return Err(TargetError::SelfTarget);
                    }
                    if !world.stack_exists(entry) {
                        return Err(TargetError::MissingTarget);
                    }
                    if !world.stack_precedes(entry, source) {
                        return Err(TargetError::TargetNotYetOnStack);
                    }
                    None
                }
            };
            *counts.entry(choice.clause).or_default() += 1;
            targets.push(BoundTarget { choice, zone });
        }
        for clause in definitions.values() {
            if !(clause.minimum..=clause.maximum).contains(counts.get(&clause.id).unwrap_or(&0)) {
                return Err(TargetError::TargetCount);
            }
        }
        Ok(Self { targets })
    }
    /// CR 707.10c: unchanged targets retain their captured identity/zone even when
    /// already illegal. Only explicitly changed occurrences undergo fresh capture.
    pub(crate) fn copy_with_changes(
        &self,
        world: &impl TargetWorld,
        source: StackEntryId,
        changes: Vec<TargetChoice>,
    ) -> Result<Self, TargetError> {
        let mut next = self.clone();
        let mut keys = BTreeSet::new();
        for choice in changes {
            if !keys.insert(choice.id) {
                return Err(TargetError::DuplicateKey);
            }
            let bound = next
                .targets
                .iter_mut()
                .find(|bound| bound.choice.id == choice.id)
                .ok_or(TargetError::UnknownCheck)?;
            if choice.clause != bound.choice.clause {
                return Err(TargetError::UnknownClause);
            }
            let captured = Self::capture(
                world,
                source,
                vec![TargetClause {
                    id: choice.clause,
                    minimum: 1,
                    maximum: 1,
                }],
                vec![choice],
            )?;
            *bound = captured
                .targets
                .into_iter()
                .next()
                .ok_or(TargetError::MissingTarget)?;
        }
        let mut selected = BTreeSet::new();
        for bound in &next.targets {
            if !selected.insert((bound.choice.clause, bound.choice.target)) {
                return Err(TargetError::RepeatedTarget);
            }
        }
        Ok(next)
    }
    pub(crate) fn assess(
        &self,
        world: &impl TargetWorld,
        checks: Vec<TargetCheck>,
    ) -> Result<TargetAssessment, TargetError> {
        let mut supplied = BTreeMap::new();
        for check in checks {
            if !self
                .targets
                .iter()
                .any(|target| target.choice.id == check.target)
            {
                return Err(TargetError::UnknownCheck);
            }
            if supplied.insert(check.target, check.legal).is_some() {
                return Err(TargetError::DuplicateCheck);
            }
        }
        if supplied.len() != self.targets.len() {
            return Err(TargetError::MissingCheck);
        }
        let mut results = Vec::new();
        for target in &self.targets {
            let identity = match target.choice.target {
                TargetRef::Card(card) => match world.card_zone(card) {
                    None => TargetLegality::ObjectGone,
                    Some(zone) if Some(zone) != target.zone => TargetLegality::ZoneChanged,
                    _ => TargetLegality::Legal,
                },
                TargetRef::Player(player) => {
                    if world.player_exists(player) {
                        TargetLegality::Legal
                    } else {
                        TargetLegality::PlayerGone
                    }
                }
                TargetRef::Stack(entry) => {
                    if world.stack_exists(entry) {
                        TargetLegality::Legal
                    } else {
                        TargetLegality::StackEntryGone
                    }
                }
            };
            let legality = if identity != TargetLegality::Legal {
                identity
            } else if supplied[&target.choice.id] {
                TargetLegality::Legal
            } else {
                TargetLegality::AdditionalRulesIllegal
            };
            results.push(TargetResult {
                choice: target.choice,
                legality,
            });
        }
        let permission = if results.is_empty() {
            ResolutionPermission::NoTargets
        } else if results
            .iter()
            .any(|result| result.legality == TargetLegality::Legal)
        {
            ResolutionPermission::SomeTargetsLegal
        } else {
            ResolutionPermission::AllTargetsIllegal
        };
        Ok(TargetAssessment {
            results,
            permission,
        })
    }
}
