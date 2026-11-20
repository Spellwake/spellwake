//! Shared simultaneous source/recipient damage math and finite modifications.
//!
//! The executor owns the active batch, validates actual world/source context, and
//! commits final results to the sole marked-damage and life state. This module
//! grants no priority and owns no cards, scheduler or active continuation.
#[cfg(test)]
pub(crate) mod private_qualification;
use crate::{
    object_identity::{GameObject, ObjectId},
    priority::SeatId,
    stack_window::StackEntryId,
    turn::TurnPosition,
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PreventionShieldId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DamageMultiplierId(u64);
/// Effect families retain distinct identities with equal numeric ordinals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageEffectId {
    Prevention(PreventionShieldId),
    Multiplier(DamageMultiplierId),
}
impl From<PreventionShieldId> for DamageEffectId {
    fn from(id: PreventionShieldId) -> Self {
        Self::Prevention(id)
    }
}
impl From<DamageMultiplierId> for DamageEffectId {
    fn from(id: DamageMultiplierId) -> Self {
        Self::Multiplier(id)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DamageAttemptId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DamageBatchId(u64);

/// Actual source identity, distinct from an instruction's execution origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageSourceId {
    Object(ObjectId),
    SpellCopy(StackEntryId),
}
/// Bounded readonly source evidence, not complete characteristics or LKI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageSource {
    Object {
        object: GameObject,
        controller: SeatId,
    },
    SpellCopy {
        entry: StackEntryId,
        controller: SeatId,
    },
}
impl DamageSource {
    pub fn id(&self) -> DamageSourceId {
        match self {
            Self::Object { object, .. } => DamageSourceId::Object(object.id()),
            Self::SpellCopy { entry, .. } => DamageSourceId::SpellCopy(*entry),
        }
    }
    pub fn controller(&self) -> SeatId {
        match self {
            Self::Object { controller, .. } | Self::SpellCopy { controller, .. } => *controller,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageRecipient {
    Creature(ObjectId),
    Player(SeatId),
}
/// Fixed identity for a future effect, independent of current creature status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageEffectTarget {
    Permanent(ObjectId),
    Player(SeatId),
}
impl DamageEffectTarget {
    /// Match identity after the executor admits an actual damage recipient.
    pub fn matches(self, recipient: DamageRecipient) -> bool {
        match (self, recipient) {
            (Self::Permanent(expected), DamageRecipient::Creature(actual)) => expected == actual,
            (Self::Player(expected), DamageRecipient::Player(actual)) => expected == actual,
            _ => false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Combat,
    Noncombat,
}
/// Sole parent metadata after its cursor moves into the active batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageOrigin {
    Resolution {
        entry: StackEntryId,
        instruction: usize,
    },
    Combat {
        position: TurnPosition,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSourceFilter {
    Any,
    Exact(DamageSourceId),
}
impl DamageSourceFilter {
    fn matches(self, source: DamageSourceId) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => expected == source,
        }
    }
}
/// Creator provenance survives creator exit and never supplies source matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreventionShield {
    pub id: PreventionShieldId,
    pub creator: StackEntryId,
    pub target: DamageEffectTarget,
    pub remaining: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageMultiplier {
    pub id: DamageMultiplierId,
    pub creator: StackEntryId,
    pub target: DamageEffectTarget,
    pub factor: RulesInteger,
    pub filter: DamageSourceFilter,
}
/// Current-operation birth evidence, including effects later consumed or pruned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageEffectCreation {
    Prevention {
        shield: PreventionShield,
        instruction: usize,
    },
    Multiplier {
        multiplier: DamageMultiplier,
        instruction: usize,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageInput {
    pub source: DamageSource,
    pub target: DamageRecipient,
    pub amount: RulesInteger,
}
/// A positive original contribution, not an occurrence of damage dealt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAttempt {
    pub id: DamageAttemptId,
    pub batch: DamageBatchId,
    pub source: DamageSource,
    pub target: DamageRecipient,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageBatchStarted {
    pub id: DamageBatchId,
    pub origin: DamageOrigin,
    pub kind: DamageKind,
    pub components: Vec<DamageAttemptId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageScope {
    Component(DamageAttemptId),
    Recipient(DamageRecipient),
}
/// An effect and the exact represented event scope it would modify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DamageApplication {
    pub effect: DamageEffectId,
    pub scope: DamageScope,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAllocation {
    pub attempt: DamageAttemptId,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageComponent {
    attempt: DamageAttempt,
    remaining: RulesInteger,
    used_multipliers: BTreeSet<DamageMultiplierId>,
}
impl DamageComponent {
    pub fn attempt(&self) -> &DamageAttempt {
        &self.attempt
    }
    pub fn remaining(&self) -> &RulesInteger {
        &self.remaining
    }
    pub fn used_multipliers(&self) -> &BTreeSet<DamageMultiplierId> {
        &self.used_multipliers
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageBatchPhase {
    Selecting,
    Allocating { application: DamageApplication },
}
/// The executor's sole active value. Frames and journals cannot resume it alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveDamageBatch {
    started: DamageBatchStarted,
    components: BTreeMap<DamageAttemptId, DamageComponent>,
    consumed_shields: BTreeSet<DamageApplication>,
    stage: u64,
    phase: DamageBatchPhase,
}
impl ActiveDamageBatch {
    pub fn started(&self) -> &DamageBatchStarted {
        &self.started
    }
    pub fn id(&self) -> DamageBatchId {
        self.started.id
    }
    pub fn origin(&self) -> DamageOrigin {
        self.started.origin
    }
    pub fn kind(&self) -> DamageKind {
        self.started.kind
    }
    pub fn components(&self) -> impl Iterator<Item = &DamageComponent> {
        self.components.values()
    }
    pub fn component(&self, id: DamageAttemptId) -> Option<&DamageComponent> {
        self.components.get(&id)
    }
    pub fn consumed_shields(&self) -> &BTreeSet<DamageApplication> {
        &self.consumed_shields
    }
    pub fn stage(&self) -> u64 {
        self.stage
    }
    pub fn phase(&self) -> &DamageBatchPhase {
        &self.phase
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreventionApplication {
    pub batch: DamageBatchId,
    pub shield: PreventionShieldId,
    pub creator: StackEntryId,
    pub target: DamageRecipient,
    pub stage: u64,
    pub allocations: Vec<DamageAllocation>,
    pub amount: RulesInteger,
    pub damage_remaining: BTreeMap<DamageAttemptId, RulesInteger>,
    pub shield_remaining: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageMultiplication {
    pub batch: DamageBatchId,
    pub attempt: DamageAttemptId,
    pub multiplier: DamageMultiplierId,
    pub creator: StackEntryId,
    pub target: DamageRecipient,
    pub stage: u64,
    pub before: RulesInteger,
    pub factor: RulesInteger,
    pub after: RulesInteger,
}
/// Only strictly positive applied quantities are emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedDamage {
    pub batch: DamageBatchId,
    pub attempt: DamageAttemptId,
    pub source: DamageSource,
    pub target: DamageRecipient,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageOutcome {
    Dealt(RulesInteger),
    FullyPrevented,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageCompletion {
    pub batch: DamageBatchId,
    pub attempt: DamageAttemptId,
    pub outcome: DamageOutcome,
}
/// Appends for one accepted operation; not durable replay or resumption state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DamageJournal {
    pub batches: Vec<DamageBatchStarted>,
    pub created: Vec<DamageEffectCreation>,
    pub attempts: Vec<DamageAttempt>,
    pub preventions: Vec<PreventionApplication>,
    pub multiplications: Vec<DamageMultiplication>,
    pub applied: Vec<AppliedDamage>,
    pub completions: Vec<DamageCompletion>,
}
impl DamageJournal {
    pub(crate) fn append(&mut self, mut other: Self) {
        self.batches.append(&mut other.batches);
        self.created.append(&mut other.created);
        self.attempts.append(&mut other.attempts);
        self.preventions.append(&mut other.preventions);
        self.multiplications.append(&mut other.multiplications);
        self.applied.append(&mut other.applied);
        self.completions.append(&mut other.completions);
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageError {
    InvalidAmount,
    InvalidFactor,
    EmptyBatch,
    DuplicateSourceRecipient,
    UnsupportedSimultaneousScope,
    ShieldIdentityExhausted,
    MultiplierIdentityExhausted,
    BatchIdentityExhausted,
    AttemptIdentityExhausted,
    StageExhausted,
    WrongPhase,
    InapplicableEffect,
    InvalidAllocation,
    EffectsStillApplicable,
}
/// Effect registries and checked allocators. Active work lives in PendingWork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DamageState {
    next_shield: u64,
    next_multiplier: u64,
    next_batch: u64,
    next_attempt: u64,
    shields: BTreeMap<PreventionShieldId, PreventionShield>,
    multipliers: BTreeMap<DamageMultiplierId, DamageMultiplier>,
}
impl Default for DamageState {
    fn default() -> Self {
        Self {
            next_shield: 1,
            next_multiplier: 1,
            next_batch: 1,
            next_attempt: 1,
            shields: BTreeMap::new(),
            multipliers: BTreeMap::new(),
        }
    }
}
impl DamageState {
    pub(crate) fn counters(&self) -> (u64, u64, u64, u64) {
        (
            self.next_shield,
            self.next_multiplier,
            self.next_batch,
            self.next_attempt,
        )
    }
    pub(crate) fn add_multiplier(
        &mut self,
        creator: StackEntryId,
        target: DamageEffectTarget,
        factor: RulesInteger,
        filter: DamageSourceFilter,
    ) -> Result<DamageMultiplier, DamageError> {
        if !factor.is_positive() {
            return Err(DamageError::InvalidFactor);
        }
        let next = self
            .next_multiplier
            .checked_add(1)
            .ok_or(DamageError::MultiplierIdentityExhausted)?;
        let effect = DamageMultiplier {
            id: DamageMultiplierId(self.next_multiplier),
            creator,
            target,
            factor,
            filter,
        };
        self.multipliers.insert(effect.id, effect.clone());
        self.next_multiplier = next;
        Ok(effect)
    }
    pub(crate) fn multiplier(&self, id: DamageMultiplierId) -> Option<&DamageMultiplier> {
        self.multipliers.get(&id)
    }
    pub(crate) fn multipliers(&self) -> impl Iterator<Item = &DamageMultiplier> {
        self.multipliers.values()
    }
    pub(crate) fn add_shield(
        &mut self,
        creator: StackEntryId,
        target: DamageEffectTarget,
        amount: RulesInteger,
    ) -> Result<PreventionShield, DamageError> {
        if !amount.is_positive() {
            return Err(DamageError::InvalidAmount);
        }
        let next = self
            .next_shield
            .checked_add(1)
            .ok_or(DamageError::ShieldIdentityExhausted)?;
        let shield = PreventionShield {
            id: PreventionShieldId(self.next_shield),
            creator,
            target,
            remaining: amount,
        };
        self.shields.insert(shield.id, shield.clone());
        self.next_shield = next;
        Ok(shield)
    }
    pub(crate) fn shield(&self, id: PreventionShieldId) -> Option<&PreventionShield> {
        self.shields.get(&id)
    }
    pub(crate) fn shields(&self) -> impl Iterator<Item = &PreventionShield> {
        self.shields.values()
    }

    /// Validate the proposal and every checked allocation before counters move.
    pub(crate) fn begin_batch(
        &mut self,
        origin: DamageOrigin,
        kind: DamageKind,
        inputs: Vec<DamageInput>,
    ) -> Result<(ActiveDamageBatch, DamageJournal), DamageError> {
        if inputs.is_empty() {
            return Err(DamageError::EmptyBatch);
        }
        let mut pairs = BTreeSet::new();
        for input in &inputs {
            if !input.amount.is_positive() {
                return Err(DamageError::InvalidAmount);
            }
            if !pairs.insert((input.source.id(), input.target)) {
                return Err(DamageError::DuplicateSourceRecipient);
            }
        }
        self.validate_supported_scope(&inputs)?;
        let next_batch = self
            .next_batch
            .checked_add(1)
            .ok_or(DamageError::BatchIdentityExhausted)?;
        let id = DamageBatchId(self.next_batch);
        let mut next_attempt = self.next_attempt;
        let mut components = BTreeMap::new();
        let mut attempts = Vec::with_capacity(inputs.len());
        for input in inputs {
            let attempt_id = DamageAttemptId(next_attempt);
            next_attempt = next_attempt
                .checked_add(1)
                .ok_or(DamageError::AttemptIdentityExhausted)?;
            let attempt = DamageAttempt {
                id: attempt_id,
                batch: id,
                source: input.source,
                target: input.target,
                amount: input.amount,
            };
            components.insert(
                attempt_id,
                DamageComponent {
                    remaining: attempt.amount.clone(),
                    attempt: attempt.clone(),
                    used_multipliers: BTreeSet::new(),
                },
            );
            attempts.push(attempt);
        }
        let started = DamageBatchStarted {
            id,
            origin,
            kind,
            components: attempts.iter().map(|a| a.id).collect(),
        };
        let batch = ActiveDamageBatch {
            started: started.clone(),
            components,
            consumed_shields: BTreeSet::new(),
            stage: 0,
            phase: DamageBatchPhase::Selecting,
        };
        let journal = DamageJournal {
            batches: vec![started],
            attempts,
            ..Default::default()
        };
        self.next_batch = next_batch;
        self.next_attempt = next_attempt;
        Ok((batch, journal))
    }
    fn validate_supported_scope(&self, inputs: &[DamageInput]) -> Result<(), DamageError> {
        let mut recipients: BTreeMap<DamageRecipient, BTreeSet<DamageSourceId>> = BTreeMap::new();
        for input in inputs {
            recipients
                .entry(input.target)
                .or_default()
                .insert(input.source.id());
        }
        for (recipient, sources) in recipients {
            if sources.len() < 2 {
                continue;
            }
            let shields = self
                .shields
                .values()
                .filter(|s| s.target.matches(recipient) && s.remaining.is_positive())
                .count();
            let multiplier = self.multipliers.values().any(|m| {
                m.target.matches(recipient)
                    && sources.iter().any(|&source| m.filter.matches(source))
            });
            if shields > 1 || (shields > 0 && multiplier) {
                return Err(DamageError::UnsupportedSimultaneousScope);
            }
        }
        Ok(())
    }
    /// Current applications retain each source lineage and recipient event scope.
    pub(crate) fn applicable(&self, batch: &ActiveDamageBatch) -> Vec<DamageApplication> {
        if let DamageBatchPhase::Allocating { application } = batch.phase {
            return vec![application];
        }
        let mut applications = Vec::new();
        for shield in self.shields.values() {
            let Some(component) = batch.components.values().find(|component| {
                shield.target.matches(component.attempt.target) && component.remaining.is_positive()
            }) else {
                continue;
            };
            let application = DamageApplication {
                effect: shield.id.into(),
                scope: DamageScope::Recipient(component.attempt.target),
            };
            if shield.remaining.is_positive() && !batch.consumed_shields.contains(&application) {
                applications.push(application);
            }
        }
        for effect in self.multipliers.values() {
            for component in batch.components.values() {
                if effect.target.matches(component.attempt.target)
                    && component.remaining.is_positive()
                    && effect.filter.matches(component.attempt.source.id())
                    && !component.used_multipliers.contains(&effect.id)
                {
                    applications.push(DamageApplication {
                        effect: effect.id.into(),
                        scope: DamageScope::Component(component.attempt.id),
                    });
                }
            }
        }
        applications
    }
    /// Select without debiting capacity or advancing the modification stage.
    pub(crate) fn select_prevention(
        &self,
        batch: &mut ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<(), DamageError> {
        if batch.phase != DamageBatchPhase::Selecting {
            return Err(DamageError::WrongPhase);
        }
        self.prevention_domain(batch, application)?;
        batch.phase = DamageBatchPhase::Allocating { application };
        Ok(())
    }
    /// A singleton eligible component or covering capacity fixes the partition.
    pub(crate) fn forced_allocation(
        &self,
        batch: &ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<Option<Vec<DamageAllocation>>, DamageError> {
        let (shield, eligible, required) = self.prevention_domain(batch, application)?;
        let total = eligible
            .values()
            .fold(0.into(), |sum: RulesInteger, amount| sum.plus(amount));
        if eligible.len() == 1 {
            return Ok(Some(vec![DamageAllocation {
                attempt: *eligible
                    .keys()
                    .next()
                    .ok_or(DamageError::InapplicableEffect)?,
                amount: required,
            }]));
        }
        if shield.remaining >= total {
            return Ok(Some(
                eligible
                    .into_iter()
                    .map(|(attempt, amount)| DamageAllocation { attempt, amount })
                    .collect(),
            ));
        }
        Ok(None)
    }
    /// Reconstruct the exact eligible quantities and required total from owned state.
    pub(crate) fn prevention_domain(
        &self,
        batch: &ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<
        (
            &PreventionShield,
            BTreeMap<DamageAttemptId, RulesInteger>,
            RulesInteger,
        ),
        DamageError,
    > {
        match batch.phase {
            DamageBatchPhase::Selecting => {}
            DamageBatchPhase::Allocating {
                application: selected,
            } if selected == application => {}
            DamageBatchPhase::Allocating { .. } => return Err(DamageError::WrongPhase),
        }
        let (DamageEffectId::Prevention(id), DamageScope::Recipient(recipient)) =
            (application.effect, application.scope)
        else {
            return Err(DamageError::InapplicableEffect);
        };
        let shield = self
            .shields
            .get(&id)
            .ok_or(DamageError::InapplicableEffect)?;
        if !shield.target.matches(recipient)
            || !shield.remaining.is_positive()
            || batch.consumed_shields.contains(&application)
        {
            return Err(DamageError::InapplicableEffect);
        }
        let eligible: BTreeMap<_, _> = batch
            .components
            .values()
            .filter(|c| c.attempt.target == recipient && c.remaining.is_positive())
            .map(|c| (c.attempt.id, c.remaining.clone()))
            .collect();
        if eligible.is_empty() {
            return Err(DamageError::InapplicableEffect);
        }
        let total = eligible
            .values()
            .fold(0.into(), |sum: RulesInteger, amount| sum.plus(amount));
        let required = std::cmp::min(&shield.remaining, &total).clone();
        Ok((shield, eligible, required))
    }
    /// Validate every sparse row and late error before changing either owner.
    pub(crate) fn apply_prevention(
        &mut self,
        batch: &mut ActiveDamageBatch,
        application: DamageApplication,
        allocations: Vec<DamageAllocation>,
    ) -> Result<PreventionApplication, DamageError> {
        let (shield, mut remaining, required) = self.prevention_domain(batch, application)?;
        let DamageScope::Recipient(target) = application.scope else {
            return Err(DamageError::InapplicableEffect);
        };
        let stage = batch
            .stage
            .checked_add(1)
            .ok_or(DamageError::StageExhausted)?;
        let mut selected = BTreeMap::new();
        let mut total = RulesInteger::from(0);
        for allocation in allocations {
            if !allocation.amount.is_positive() {
                return Err(DamageError::InvalidAllocation);
            }
            let current = remaining
                .get(&allocation.attempt)
                .ok_or(DamageError::InvalidAllocation)?;
            if allocation.amount > *current
                || selected
                    .insert(allocation.attempt, allocation.amount.clone())
                    .is_some()
            {
                return Err(DamageError::InvalidAllocation);
            }
            total = total.plus(&allocation.amount);
        }
        if total != required {
            return Err(DamageError::InvalidAllocation);
        }
        for (attempt, amount) in &selected {
            let current = remaining
                .get_mut(attempt)
                .ok_or(DamageError::InvalidAllocation)?;
            *current = current.minus(amount);
        }
        let fact = PreventionApplication {
            batch: batch.id(),
            shield: shield.id,
            creator: shield.creator,
            target,
            stage,
            allocations: selected
                .into_iter()
                .map(|(attempt, amount)| DamageAllocation { attempt, amount })
                .collect(),
            amount: total,
            damage_remaining: remaining.clone(),
            shield_remaining: shield.remaining.minus(&required),
        };
        // No fallible operation follows the first mutation of either owner.
        for (attempt, amount) in remaining {
            if let Some(component) = batch.components.get_mut(&attempt) {
                component.remaining = amount;
            }
        }
        if fact.shield_remaining.is_positive() {
            if let Some(shield) = self.shields.get_mut(&fact.shield) {
                shield.remaining = fact.shield_remaining.clone();
            }
        } else {
            self.shields.remove(&fact.shield);
        }
        batch.consumed_shields.insert(application);
        batch.stage = stage;
        batch.phase = DamageBatchPhase::Selecting;
        Ok(fact)
    }
    pub(crate) fn apply_multiplier(
        &self,
        batch: &mut ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<DamageMultiplication, DamageError> {
        if batch.phase != DamageBatchPhase::Selecting {
            return Err(DamageError::WrongPhase);
        }
        let (DamageEffectId::Multiplier(id), DamageScope::Component(attempt)) =
            (application.effect, application.scope)
        else {
            return Err(DamageError::InapplicableEffect);
        };
        let effect = self
            .multipliers
            .get(&id)
            .ok_or(DamageError::InapplicableEffect)?;
        let component = batch
            .components
            .get(&attempt)
            .ok_or(DamageError::InapplicableEffect)?;
        if !effect.target.matches(component.attempt.target)
            || !component.remaining.is_positive()
            || component.used_multipliers.contains(&id)
            || !effect.filter.matches(component.attempt.source.id())
        {
            return Err(DamageError::InapplicableEffect);
        }
        let stage = batch
            .stage
            .checked_add(1)
            .ok_or(DamageError::StageExhausted)?;
        let fact = DamageMultiplication {
            batch: batch.id(),
            attempt,
            multiplier: id,
            creator: effect.creator,
            target: component.attempt.target,
            stage,
            before: component.remaining.clone(),
            factor: effect.factor.clone(),
            after: component.remaining.times(&effect.factor),
        };
        if let Some(component) = batch.components.get_mut(&attempt) {
            component.remaining = fact.after.clone();
            component.used_multipliers.insert(id);
        }
        batch.stage = stage;
        Ok(fact)
    }
    /// Prepare component results without mutating marks, life or parent state.
    pub(crate) fn finish_batch(
        &self,
        batch: &ActiveDamageBatch,
    ) -> Result<(Vec<AppliedDamage>, Vec<DamageCompletion>), DamageError> {
        if batch.phase != DamageBatchPhase::Selecting {
            return Err(DamageError::WrongPhase);
        }
        if !self.applicable(batch).is_empty() {
            return Err(DamageError::EffectsStillApplicable);
        }
        let mut applied = Vec::new();
        let mut completions = Vec::new();
        for component in batch.components.values() {
            let attempt = &component.attempt;
            let outcome = if component.remaining.is_positive() {
                applied.push(AppliedDamage {
                    batch: batch.id(),
                    attempt: attempt.id,
                    source: attempt.source.clone(),
                    target: attempt.target,
                    amount: component.remaining.clone(),
                });
                DamageOutcome::Dealt(component.remaining.clone())
            } else {
                DamageOutcome::FullyPrevented
            };
            completions.push(DamageCompletion {
                batch: batch.id(),
                attempt: attempt.id,
                outcome,
            });
        }
        Ok((applied, completions))
    }
    pub(crate) fn retain_recipients(&mut self, live_battlefield: &BTreeSet<ObjectId>) {
        let live = |target: DamageEffectTarget| match target {
            DamageEffectTarget::Permanent(object) => live_battlefield.contains(&object),
            DamageEffectTarget::Player(_) => true,
        };
        self.shields.retain(|_, shield| live(shield.target));
        self.multipliers
            .retain(|_, multiplier| live(multiplier.target));
    }
}

#[cfg(test)]
mod tests;
