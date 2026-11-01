//! Attempted creature damage, finite shields and literal fixed-target multipliers.
//! No durations, source restrictions, optional prevention, redirection, unpreventable
//! damage, simultaneous sources or general replacements are represented here.
use crate::{card_identity::CardObjectId, stack_window::StackEntryId};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PreventionShieldId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DamageMultiplierId(u64);
/// Distinct effect families may have the same numeric ordinal without aliasing.
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

/// An active resolved effect; source exit is not an expiration condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreventionShield {
    pub id: PreventionShieldId,
    pub source: StackEntryId,
    pub target: CardObjectId,
    pub remaining: RulesInteger,
}
/// Resolved no-duration effect, not a static source's global ability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageMultiplier {
    pub id: DamageMultiplierId,
    pub source: StackEntryId,
    pub target: CardObjectId,
    pub factor: RulesInteger,
}
/// Current-operation birth evidence, even if a later instruction consumes/prunes it.
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
/// Proposal identity and original quantity, not an occurrence of dealt damage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAttempt {
    pub id: DamageAttemptId,
    pub source: StackEntryId,
    pub target: CardObjectId,
    pub amount: RulesInteger,
}
/// Exactly one owned active continuation. Journals are not needed to resume it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveDamageAttempt {
    attempt: DamageAttempt,
    remaining: RulesInteger,
    used: BTreeSet<DamageEffectId>,
    stage: u64,
}
impl ActiveDamageAttempt {
    pub fn attempt(&self) -> &DamageAttempt {
        &self.attempt
    }
    pub fn remaining(&self) -> &RulesInteger {
        &self.remaining
    }
    pub fn used_effects(&self) -> &BTreeSet<DamageEffectId> {
        &self.used
    }
    pub fn stage(&self) -> u64 {
        self.stage
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreventionApplication {
    pub attempt: DamageAttemptId,
    pub shield: PreventionShieldId,
    pub shield_source: StackEntryId,
    pub target: CardObjectId,
    pub stage: u64,
    pub amount: RulesInteger,
    pub damage_remaining: RulesInteger,
    pub shield_remaining: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageMultiplication {
    pub attempt: DamageAttemptId,
    pub multiplier: DamageMultiplierId,
    pub multiplier_source: StackEntryId,
    pub target: CardObjectId,
    pub stage: u64,
    pub before: RulesInteger,
    pub factor: RulesInteger,
    pub after: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DamageModification {
    Prevention(PreventionApplication),
    Multiplication(DamageMultiplication),
}
/// Only strictly positive quantities are emitted by the executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedDamage {
    pub attempt: DamageAttemptId,
    pub source: StackEntryId,
    pub target: CardObjectId,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageOutcome {
    Dealt(RulesInteger),
    FullyPrevented,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageCompletion {
    pub attempt: DamageAttemptId,
    pub outcome: DamageOutcome,
}
/// Damage-specific append for one accepted native operation/input. This is not a
/// complete replay/event journal, seat view or durable acceptance receipt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DamageJournal {
    pub created: Vec<DamageEffectCreation>,
    pub attempts: Vec<DamageAttempt>,
    pub preventions: Vec<PreventionApplication>,
    pub multiplications: Vec<DamageMultiplication>,
    pub applied: Vec<AppliedDamage>,
    pub completions: Vec<DamageCompletion>,
}
impl DamageJournal {
    pub(crate) fn append(&mut self, mut other: Self) {
        self.created.append(&mut other.created);
        self.attempts.append(&mut other.attempts);
        self.preventions.append(&mut other.preventions);
        self.multiplications.append(&mut other.multiplications);
        self.applied.append(&mut other.applied);
        self.completions.append(&mut other.completions);
    }
    pub(crate) fn record(&mut self, modification: DamageModification) {
        match modification {
            DamageModification::Prevention(fact) => self.preventions.push(fact),
            DamageModification::Multiplication(fact) => self.multiplications.push(fact),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageError {
    InvalidAmount,
    InvalidFactor,
    ShieldIdentityExhausted,
    MultiplierIdentityExhausted,
    AttemptIdentityExhausted,
    StageExhausted,
    AttemptAlreadyActive,
    NoActiveAttempt,
    InapplicableEffect,
    EffectsStillApplicable,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DamageState {
    next_shield: u64,
    next_multiplier: u64,
    next_attempt: u64,
    shields: BTreeMap<PreventionShieldId, PreventionShield>,
    multipliers: BTreeMap<DamageMultiplierId, DamageMultiplier>,
    active: Option<ActiveDamageAttempt>,
}
impl Default for DamageState {
    fn default() -> Self {
        Self {
            next_shield: 1,
            next_multiplier: 1,
            next_attempt: 1,
            shields: BTreeMap::new(),
            multipliers: BTreeMap::new(),
            active: None,
        }
    }
}
impl DamageState {
    pub(crate) fn add_multiplier(
        &mut self,
        source: StackEntryId,
        target: CardObjectId,
        factor: RulesInteger,
    ) -> Result<DamageMultiplier, DamageError> {
        if !factor.is_positive() {
            return Err(DamageError::InvalidFactor);
        }
        let next = self
            .next_multiplier
            .checked_add(1)
            .ok_or(DamageError::MultiplierIdentityExhausted)?;
        let id = DamageMultiplierId(self.next_multiplier);
        let effect = DamageMultiplier {
            id,
            source,
            target,
            factor,
        };
        self.multipliers.insert(id, effect.clone());
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
        source: StackEntryId,
        target: CardObjectId,
        amount: RulesInteger,
    ) -> Result<PreventionShield, DamageError> {
        if !amount.is_positive() {
            return Err(DamageError::InvalidAmount);
        }
        let next = self
            .next_shield
            .checked_add(1)
            .ok_or(DamageError::ShieldIdentityExhausted)?;
        let id = PreventionShieldId(self.next_shield);
        let shield = PreventionShield {
            id,
            source,
            target,
            remaining: amount,
        };
        self.shields.insert(id, shield.clone());
        self.next_shield = next;
        Ok(shield)
    }
    pub(crate) fn begin(
        &mut self,
        source: StackEntryId,
        target: CardObjectId,
        amount: RulesInteger,
    ) -> Result<DamageAttempt, DamageError> {
        if !amount.is_positive() {
            return Err(DamageError::InvalidAmount);
        }
        if self.active.is_some() {
            return Err(DamageError::AttemptAlreadyActive);
        }
        let next = self
            .next_attempt
            .checked_add(1)
            .ok_or(DamageError::AttemptIdentityExhausted)?;
        let attempt = DamageAttempt {
            id: DamageAttemptId(self.next_attempt),
            source,
            target,
            amount: amount.clone(),
        };
        self.active = Some(ActiveDamageAttempt {
            attempt: attempt.clone(),
            remaining: amount,
            used: BTreeSet::new(),
            stage: 0,
        });
        self.next_attempt = next;
        Ok(attempt)
    }
    pub(crate) fn active(&self) -> Option<&ActiveDamageAttempt> {
        self.active.as_ref()
    }
    pub(crate) fn shield(&self, id: PreventionShieldId) -> Option<&PreventionShield> {
        self.shields.get(&id)
    }
    pub(crate) fn shields(&self) -> impl Iterator<Item = &PreventionShield> {
        self.shields.values()
    }
    pub(crate) fn applicable(&self) -> Vec<DamageEffectId> {
        let Some(active) = &self.active else {
            return Vec::new();
        };
        if !active.remaining.is_positive() {
            return Vec::new();
        }
        self.shields
            .values()
            .filter(|shield| {
                shield.target == active.attempt.target
                    && shield.remaining.is_positive()
                    && !active.used.contains(&DamageEffectId::Prevention(shield.id))
            })
            .map(|shield| DamageEffectId::Prevention(shield.id))
            .chain(
                self.multipliers
                    .values()
                    .filter(|effect| {
                        effect.target == active.attempt.target
                            && !active.used.contains(&DamageEffectId::Multiplier(effect.id))
                    })
                    .map(|effect| DamageEffectId::Multiplier(effect.id)),
            )
            .collect()
    }
    pub(crate) fn apply(&mut self, id: DamageEffectId) -> Result<DamageModification, DamageError> {
        if !self.applicable().contains(&id) {
            return Err(DamageError::InapplicableEffect);
        }
        let active = self.active.as_mut().ok_or(DamageError::NoActiveAttempt)?;
        let stage = active
            .stage
            .checked_add(1)
            .ok_or(DamageError::StageExhausted)?;
        let fact = match id {
            DamageEffectId::Prevention(shield_id) => {
                let shield = self
                    .shields
                    .get_mut(&shield_id)
                    .ok_or(DamageError::InapplicableEffect)?;
                let amount = std::cmp::min(&active.remaining, &shield.remaining).clone();
                active.remaining = active.remaining.minus(&amount);
                shield.remaining = shield.remaining.minus(&amount);
                let fact = PreventionApplication {
                    attempt: active.attempt.id,
                    shield: shield_id,
                    shield_source: shield.source,
                    target: shield.target,
                    stage,
                    amount,
                    damage_remaining: active.remaining.clone(),
                    shield_remaining: shield.remaining.clone(),
                };
                if !shield.remaining.is_positive() {
                    self.shields.remove(&shield_id);
                }
                DamageModification::Prevention(fact)
            }
            DamageEffectId::Multiplier(multiplier_id) => {
                let effect = self
                    .multipliers
                    .get(&multiplier_id)
                    .ok_or(DamageError::InapplicableEffect)?;
                let before = active.remaining.clone();
                active.remaining = before.times(&effect.factor);
                DamageModification::Multiplication(DamageMultiplication {
                    attempt: active.attempt.id,
                    multiplier: multiplier_id,
                    multiplier_source: effect.source,
                    target: effect.target,
                    stage,
                    before,
                    factor: effect.factor.clone(),
                    after: active.remaining.clone(),
                })
            }
        };
        active.used.insert(id);
        active.stage = stage;
        Ok(fact)
    }
    pub(crate) fn finish(
        &mut self,
    ) -> Result<(Option<AppliedDamage>, DamageCompletion), DamageError> {
        if !self.applicable().is_empty() {
            return Err(DamageError::EffectsStillApplicable);
        }
        let active = self.active.take().ok_or(DamageError::NoActiveAttempt)?;
        let applied = active.remaining.is_positive().then(|| AppliedDamage {
            attempt: active.attempt.id,
            source: active.attempt.source,
            target: active.attempt.target,
            amount: active.remaining.clone(),
        });
        let outcome = if applied.is_some() {
            DamageOutcome::Dealt(active.remaining)
        } else {
            DamageOutcome::FullyPrevented
        };
        Ok((
            applied,
            DamageCompletion {
                attempt: active.attempt.id,
                outcome,
            },
        ))
    }
    pub(crate) fn retain_battlefield(&mut self, live: &BTreeSet<CardObjectId>) {
        self.shields
            .retain(|_, shield| live.contains(&shield.target));
        self.multipliers
            .retain(|_, effect| live.contains(&effect.target));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        card_identity::{CardDestination, CardId, CardObjects},
        priority::SeatId,
        stack_window::StackWindow,
    };
    fn identities() -> (StackEntryId, CardObjectId) {
        let objects = CardObjects::new(0)
            .seed_card(0, CardId(1), SeatId(0), CardDestination::Hand)
            .unwrap()
            .next;
        let target = objects.current_identity(CardId(1)).unwrap();
        let window = StackWindow::new(vec![SeatId(0), SeatId(1)], SeatId(0), objects).unwrap();
        let window = window.finish_checks(window.revision()).unwrap();
        let window = window
            .record_card_spell(window.revision(), SeatId(0), target)
            .unwrap();
        (window.entries()[0].id(), target)
    }
    #[test]
    fn checked_identity_and_stage_exhaustion_reject_before_mutation() {
        let (source, target) = identities();
        let mut shields = DamageState {
            next_shield: u64::MAX,
            ..Default::default()
        };
        let saved = shields.clone();
        assert_eq!(
            shields.add_shield(source, target, 1.into()),
            Err(DamageError::ShieldIdentityExhausted)
        );
        assert_eq!(shields, saved);
        let mut attempts = DamageState {
            next_attempt: u64::MAX,
            ..Default::default()
        };
        let saved = attempts.clone();
        assert_eq!(
            attempts.begin(source, target, 1.into()),
            Err(DamageError::AttemptIdentityExhausted)
        );
        assert_eq!(attempts, saved);
        let mut state = DamageState::default();
        state.add_shield(source, target, 2.into()).unwrap();
        state.begin(source, target, 3.into()).unwrap();
        state.active.as_mut().unwrap().stage = u64::MAX;
        let id = state.applicable()[0];
        let saved = state.clone();
        assert_eq!(state.apply(id), Err(DamageError::StageExhausted));
        assert_eq!(state, saved);
    }
    #[test]
    fn unsupported_initial_zero_and_negative_amounts_allocate_no_state() {
        let (source, target) = identities();
        let mut state = DamageState::default();
        let saved = state.clone();
        for amount in [0, -1] {
            assert_eq!(
                state.add_shield(source, target, amount.into()),
                Err(DamageError::InvalidAmount)
            );
            assert_eq!(
                state.begin(source, target, amount.into()),
                Err(DamageError::InvalidAmount)
            );
        }
        assert_eq!(state, saved);
    }
    #[test]
    fn multiplier_allocation_and_stage_exhaustion_preserve_input() {
        let (source, target) = identities();
        let mut state = DamageState {
            next_multiplier: u64::MAX,
            ..Default::default()
        };
        let saved = state.clone();
        assert_eq!(
            state.add_multiplier(source, target, 2.into()),
            Err(DamageError::MultiplierIdentityExhausted)
        );
        assert_eq!(state, saved);
        for factor in [0, -1] {
            assert_eq!(
                state.add_multiplier(source, target, factor.into()),
                Err(DamageError::InvalidFactor)
            );
            assert_eq!(state, saved);
        }
        let mut state = DamageState::default();
        state.add_multiplier(source, target, 1.into()).unwrap();
        state.begin(source, target, 3.into()).unwrap();
        state.active.as_mut().unwrap().stage = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.apply(state.applicable()[0]),
            Err(DamageError::StageExhausted)
        );
        assert_eq!(state, saved);
    }
}
