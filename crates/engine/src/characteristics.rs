//! Closed, dependency-free numeric layer-7 subset (CR 613.4b–d).
//!
//! Only literal operations on a fixed battlefield incarnation are representable.
//! No CDAs, dynamic expressions/selectors, copy effects, other layers or durations.
//! This ordering is therefore not an algorithm for general layer dependencies.
use crate::{card_identity::CardObjectId, stack_window::StackEntryId};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

/// Exact numeric pair; negative power/toughness is not clamped to zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerToughness {
    pub power: RulesInteger,
    pub toughness: RulesInteger,
}

/// These literal, fixed-target operations cannot depend on another effect's
/// applicability or generated values. Setting changes both properties only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerToughnessOperation {
    Set(PowerToughness),
    Modify(PowerToughness),
    Switch,
}

/// Physical counts on one incarnation, not copiable characteristics. The same
/// shape is used for a simultaneous placement batch; zero total is not an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerToughnessCounters {
    pub plus_one: RulesInteger,
    pub minus_one: RulesInteger,
}
impl Default for PowerToughnessCounters {
    fn default() -> Self {
        Self {
            plus_one: 0.into(),
            minus_one: 0.into(),
        }
    }
}
impl PowerToughnessCounters {
    pub(crate) fn valid_placement(&self) -> bool {
        self.plus_one >= 0.into()
            && self.minus_one >= 0.into()
            && (self.plus_one.is_positive() || self.minus_one.is_positive())
    }
    fn has_opposites(&self) -> bool {
        self.plus_one.is_positive() && self.minus_one.is_positive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContinuousEffectId(u64);

/// A resolved, duration-free effect. Source exit does not remove it. Applicability
/// ends at target incarnation departure; this is not a source-dependent static.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerToughnessEffect {
    pub id: ContinuousEffectId,
    pub timestamp: u64,
    pub source: StackEntryId,
    pub target: CardObjectId,
    pub operation: PowerToughnessOperation,
}

/// Read-only computation, never another independently mutated PT authority.
/// Defined equals copiable in this subset. Base is after 7b (CR 208.4), not printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerToughnessEvaluation {
    pub object: CardObjectId,
    pub copiable: PowerToughness,
    pub base: PowerToughness,
    pub derived: PowerToughness,
    /// In actual sublayer/application order, not one global timestamp ordering.
    pub applied_effects: Vec<ContinuousEffectId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacteristicsError {
    EffectIdentityExhausted,
    InvalidCounterPlacement,
}

/// Nested canonical state under EffectWindow. Construction/mutation is private to
/// the kernel; a renderer never supplies final stats, timestamps or applicability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PowerToughnessState {
    next_effect: u64,
    effects: BTreeMap<ContinuousEffectId, PowerToughnessEffect>,
    counters: BTreeMap<CardObjectId, PowerToughnessCounters>,
}
impl Default for PowerToughnessState {
    fn default() -> Self {
        Self {
            next_effect: 1,
            effects: BTreeMap::new(),
            counters: BTreeMap::new(),
        }
    }
}
impl PowerToughnessState {
    pub(crate) fn add_effect(
        &mut self,
        source: StackEntryId,
        target: CardObjectId,
        operation: PowerToughnessOperation,
    ) -> Result<(), CharacteristicsError> {
        let next = self
            .next_effect
            .checked_add(1)
            .ok_or(CharacteristicsError::EffectIdentityExhausted)?;
        let id = ContinuousEffectId(self.next_effect);
        self.effects.insert(
            id,
            PowerToughnessEffect {
                id,
                timestamp: self.next_effect,
                source,
                target,
                operation,
            },
        );
        self.next_effect = next;
        Ok(())
    }
    pub(crate) fn place_counters(
        &mut self,
        target: CardObjectId,
        placement: &PowerToughnessCounters,
    ) -> Result<(), CharacteristicsError> {
        if !placement.valid_placement() {
            return Err(CharacteristicsError::InvalidCounterPlacement);
        }
        let current = self.counters.entry(target).or_default();
        current.plus_one = current.plus_one.plus(&placement.plus_one);
        current.minus_one = current.minus_one.plus(&placement.minus_one);
        Ok(())
    }
    pub(crate) fn counters(&self, object: CardObjectId) -> PowerToughnessCounters {
        self.counters.get(&object).cloned().unwrap_or_default()
    }
    pub(crate) fn effect(&self, id: ContinuousEffectId) -> Option<&PowerToughnessEffect> {
        self.effects.get(&id)
    }

    pub(crate) fn evaluate(
        &self,
        object: CardObjectId,
        copiable: PowerToughness,
    ) -> PowerToughnessEvaluation {
        let effects: Vec<_> = self
            .effects
            .values()
            .filter(|effect| effect.target == object)
            .collect();
        let mut base = copiable.clone();
        let mut applied_effects = Vec::new();
        // Creation identity/timestamp is monotonic in this closed sequential universe.
        for effect in &effects {
            if let PowerToughnessOperation::Set(values) = &effect.operation {
                base = values.clone();
                applied_effects.push(effect.id);
            }
        }
        let mut derived = base.clone();
        for effect in &effects {
            if let PowerToughnessOperation::Modify(values) = &effect.operation {
                derived.power = derived.power.plus(&values.power);
                derived.toughness = derived.toughness.plus(&values.toughness);
                applied_effects.push(effect.id);
            }
        }
        let counters = self.counters(object);
        let difference = counters.plus_one.minus(&counters.minus_one);
        derived.power = derived.power.plus(&difference);
        derived.toughness = derived.toughness.plus(&difference);
        for effect in &effects {
            if effect.operation == PowerToughnessOperation::Switch {
                std::mem::swap(&mut derived.power, &mut derived.toughness);
                applied_effects.push(effect.id);
            }
        }
        PowerToughnessEvaluation {
            object,
            copiable,
            base,
            derived,
            applied_effects,
        }
    }

    /// Compute from pre-check state; applying this result must share the SBA batch
    /// with death checks. Eager annihilation during placement would lose evidence.
    pub(crate) fn counter_cancellations(&self) -> Vec<(CardObjectId, PowerToughnessCounters)> {
        self.counters
            .iter()
            .filter(|(_, counts)| counts.has_opposites())
            .map(|(object, counts)| {
                let cancel = std::cmp::min(&counts.plus_one, &counts.minus_one);
                (
                    *object,
                    PowerToughnessCounters {
                        plus_one: counts.plus_one.minus(cancel),
                        minus_one: counts.minus_one.minus(cancel),
                    },
                )
            })
            .collect()
    }
    pub(crate) fn apply_counter_cancellations(
        &mut self,
        updates: Vec<(CardObjectId, PowerToughnessCounters)>,
    ) {
        for (object, counts) in updates {
            if counts.plus_one.is_positive() || counts.minus_one.is_positive() {
                self.counters.insert(object, counts);
            } else {
                self.counters.remove(&object);
            }
        }
    }
    pub(crate) fn retain_battlefield(&mut self, live: &BTreeSet<CardObjectId>) {
        self.effects
            .retain(|_, effect| live.contains(&effect.target));
        self.counters.retain(|object, _| live.contains(object));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_exhaustion_and_invalid_counter_counts_never_change_owned_state() {
        use crate::{
            card_identity::{CardDestination, CardId, CardObjects},
            priority::SeatId,
            stack_window::StackWindow,
        };
        let objects = CardObjects::new(0)
            .seed_card(0, CardId(1), SeatId(0), CardDestination::Hand)
            .unwrap()
            .next;
        let card = objects.current_identity(CardId(1)).unwrap();
        let window = StackWindow::new(vec![SeatId(0), SeatId(1)], SeatId(0), objects).unwrap();
        let window = window.finish_checks(window.revision()).unwrap();
        let window = window
            .record_card_spell(window.revision(), SeatId(0), card)
            .unwrap();
        let source = window.entries()[0].id();
        let mut state = PowerToughnessState {
            next_effect: u64::MAX,
            ..Default::default()
        };
        let saved = state.clone();
        assert_eq!(
            state.add_effect(source, card, PowerToughnessOperation::Switch),
            Err(CharacteristicsError::EffectIdentityExhausted)
        );
        for (plus, minus) in [(-1, 2), (2, -1), (0, 0)] {
            assert_eq!(
                state.place_counters(
                    card,
                    &PowerToughnessCounters {
                        plus_one: plus.into(),
                        minus_one: minus.into()
                    }
                ),
                Err(CharacteristicsError::InvalidCounterPlacement)
            );
        }
        assert_eq!(state, saved);
    }
}
