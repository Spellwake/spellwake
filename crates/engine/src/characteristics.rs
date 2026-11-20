//! Exact-world evaluation for admitted copy, control, text, type, color, ability
//! and PT buckets, including finite defining abilities in all physical zones.
//!
//! Canonical state owns physical counters, resolved records and one timestamp
//! clock. Evaluation, generated-operation probes and ordering use only scratch
//! values borrowed from the caller's actual physical/definition/state cut.
mod colors;
mod evaluation;
#[cfg(test)]
#[path = "characteristics/tests.rs"]
pub(crate) mod frozen_tests;
mod ordering;
#[cfg(test)]
pub(crate) mod private_qualification;
mod text;
pub(crate) use text::rewrite_program as rewrite_program_creature_types;
mod types;
mod world;

pub use colors::{Color, ColorFilter, ColorOperation, StaticColorOperand};
pub(crate) use evaluation::{count_permanents, evaluate};
pub use types::*;

pub use crate::program::CardType;
use crate::{
    object_identity::{CardId, GameObjects, ObjectId, ObjectZone},
    stack_window::StackEntryId,
};
use std::collections::{BTreeMap, BTreeSet};

/// Nested canonical state under EffectWindow, never a final-stat cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CharacteristicState {
    departed_controllers: BTreeSet<crate::priority::SeatId>,
    next_effect: u64,
    next_timestamp: u64,
    object_timestamps: BTreeMap<ObjectId, CharacteristicTimestamp>,
    resolved: BTreeMap<ContinuousEffectId, ResolvedContinuousEffect>,
    counters: BTreeMap<ObjectId, PowerToughnessCounters>,
    next_world_tenure: u64,
    world_tenures: BTreeMap<ObjectId, WorldTenure>,
}
impl Default for CharacteristicState {
    fn default() -> Self {
        Self {
            departed_controllers: BTreeSet::new(),
            next_effect: 1,
            next_timestamp: 1,
            object_timestamps: BTreeMap::new(),
            resolved: BTreeMap::new(),
            counters: BTreeMap::new(),
            next_world_tenure: 1,
            world_tenures: BTreeMap::new(),
        }
    }
}
impl CharacteristicState {
    pub(crate) fn copiable_definition<'a>(
        &'a self,
        physical: &'a crate::object_identity::GameObject,
        programs: &'a BTreeMap<CardId, crate::program::Program>,
    ) -> Option<(&'a crate::program::Program, Option<CharacteristicTimestamp>)> {
        let original = physical.base_program(programs)?;
        if physical.zone() == ObjectZone::Battlefield
            && let Some((program, timestamp)) = self.copied_definition(physical.id())
        {
            return Some((program, Some(timestamp)));
        }
        Some((original, None))
    }
    /// Last independent layer-1 snapshot and its creating timestamp. The caller
    /// supplies a live battlefield incarnation; no donor reference is retained.
    pub(crate) fn copied_definition(
        &self,
        object: ObjectId,
    ) -> Option<(&crate::program::Program, CharacteristicTimestamp)> {
        self.resolved
            .values()
            .filter(|effect| effect.affected.contains(&object))
            .filter_map(|effect| match effect.parts.0.as_slice() {
                [ResolvedPart::Copy(program)] => Some((program.as_ref(), effect.timestamp)),
                _ => None,
            })
            .max_by_key(|(_, timestamp)| *timestamp)
    }
    /// Admit exactly the trusted initial BF order without deriving history from
    /// registry or definition IDs. Validate the complete batch before allocation.
    pub(crate) fn register_initial_battlefield(
        &mut self,
        cards: &GameObjects,
        order: &[CardId],
    ) -> Result<(), CharacteristicsError> {
        if !self.object_timestamps.is_empty() || !self.resolved.is_empty() {
            return Err(CharacteristicsError::InitialBattlefieldAlreadyRegistered);
        }
        let battlefield: BTreeMap<_, _> = cards
            .iter()
            .filter(|object| object.zone() == ObjectZone::Battlefield)
            .filter_map(|object| object.card_id().map(|card| (card, object.id())))
            .collect();
        let supplied: BTreeSet<_> = order.iter().copied().collect();
        if supplied.len() != order.len() || supplied != battlefield.keys().copied().collect() {
            return Err(CharacteristicsError::InvalidInitialBattlefieldOrder);
        }
        let order = order
            .iter()
            .map(|card| battlefield[card])
            .collect::<Vec<_>>();
        self.register_battlefield_entries(&order)
    }

    /// Trusted single entry through the same complete-order allocator.
    pub(crate) fn register_battlefield_entry(
        &mut self,
        object: ObjectId,
    ) -> Result<CharacteristicTimestamp, CharacteristicsError> {
        let timestamp = CharacteristicTimestamp(self.next_timestamp);
        self.register_battlefield_entries(&[object])?;
        Ok(timestamp)
    }

    /// Allocate distinct relative timestamps in the complete selected order.
    /// Duplicate members, prior registrations and overflow reject before mutation.
    /// World tenure is reconciled separately at the one actual publication cut.
    pub(crate) fn register_battlefield_entries(
        &mut self,
        order: &[ObjectId],
    ) -> Result<(), CharacteristicsError> {
        let mut seen = BTreeSet::new();
        if order
            .iter()
            .any(|id| !seen.insert(*id) || self.object_timestamps.contains_key(id))
        {
            return Err(CharacteristicsError::DuplicateBattlefieldTimestamp);
        }
        let count =
            u64::try_from(order.len()).map_err(|_| CharacteristicsError::TimestampExhausted)?;
        let next = self
            .next_timestamp
            .checked_add(count)
            .ok_or(CharacteristicsError::TimestampExhausted)?;
        for (id, value) in order.iter().zip(self.next_timestamp..next) {
            self.object_timestamps
                .insert(*id, CharacteristicTimestamp(value));
        }
        self.next_timestamp = next;
        Ok(())
    }

    pub(crate) fn add_effect(
        &mut self,
        creator: StackEntryId,
        target: ObjectId,
        mut parts: ResolvedParts,
        duration: ResolvedDuration,
    ) -> Result<ResolvedContinuousEffect, CharacteristicsError> {
        parts
            .validate()
            .map_err(CharacteristicsError::InvalidParts)?;
        parts.0.sort_by_key(ResolvedPart::bucket);
        let next_effect = self
            .next_effect
            .checked_add(1)
            .ok_or(CharacteristicsError::EffectIdentityExhausted)?;
        let next_timestamp = self
            .next_timestamp
            .checked_add(1)
            .ok_or(CharacteristicsError::TimestampExhausted)?;
        let id = ContinuousEffectId(self.next_effect);
        let effect = ResolvedContinuousEffect {
            id,
            timestamp: CharacteristicTimestamp(self.next_timestamp),
            creator,
            affected: BTreeSet::from([target]),
            parts,
            duration,
        };
        self.resolved.insert(id, effect.clone());
        self.next_effect = next_effect;
        self.next_timestamp = next_timestamp;
        Ok(effect)
    }
    pub(crate) fn place_counters(
        &mut self,
        target: ObjectId,
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
    pub(crate) fn counters(&self, object: ObjectId) -> PowerToughnessCounters {
        self.counters.get(&object).cloned().unwrap_or_default()
    }
    pub(crate) fn next_effect_sequence(&self) -> u64 {
        self.next_effect
    }
    pub(crate) fn next_timestamp_sequence(&self) -> u64 {
        self.next_timestamp
    }
    pub(crate) fn object_timestamp(&self, object: ObjectId) -> Option<CharacteristicTimestamp> {
        self.object_timestamps.get(&object).copied()
    }
    pub(crate) fn object_timestamps(
        &self,
    ) -> impl Iterator<Item = (ObjectId, CharacteristicTimestamp)> + '_ {
        self.object_timestamps
            .iter()
            .map(|(object, stamp)| (*object, *stamp))
    }
    pub(crate) fn effects(&self) -> impl Iterator<Item = &ResolvedContinuousEffect> {
        self.resolved.values()
    }
    pub(crate) fn effect(&self, id: ContinuousEffectId) -> Option<&ResolvedContinuousEffect> {
        self.resolved.get(&id)
    }
    pub(crate) fn expire_until_end_of_turn(&mut self) -> Vec<ContinuousEffectId> {
        let expired = self
            .resolved
            .values()
            .filter(|effect| effect.duration == ResolvedDuration::UntilEndOfTurn)
            .map(|effect| effect.id)
            .collect();
        self.resolved
            .retain(|_, effect| effect.duration != ResolvedDuration::UntilEndOfTurn);
        expired
    }

    /// CR 800.4a ends the entire continuous effect granting a departing player
    /// control, including masked effects and other buckets in that same record.
    /// Independent effects survive regardless of the creating entry's controller.
    pub(crate) fn end_departed_control(
        &mut self,
        players: &BTreeSet<crate::priority::SeatId>,
    ) -> Vec<ResolvedContinuousEffect> {
        self.departed_controllers.extend(players);
        let mut ended = Vec::new();
        self.resolved.retain(|_, effect| {
            if effect.parts.0.iter().any(
                |part| matches!(part, ResolvedPart::Control(player) if players.contains(player)),
            ) {
                ended.push(effect.clone());
                false
            } else {
                true
            }
        });
        ended
    }

    /// Compute from pre-check state and apply in the same SBA batch as death.
    pub(crate) fn counter_cancellations(&self) -> Vec<(ObjectId, PowerToughnessCounters)> {
        self.counters
            .iter()
            .filter(|(_, counts)| counts.plus_one.is_positive() && counts.minus_one.is_positive())
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
        updates: Vec<(ObjectId, PowerToughnessCounters)>,
    ) {
        for (object, counts) in updates {
            if counts.plus_one.is_positive() || counts.minus_one.is_positive() {
                self.counters.insert(object, counts);
            } else {
                self.counters.remove(&object);
            }
        }
    }
    pub(crate) fn retain_battlefield(&mut self, live: &BTreeSet<ObjectId>) {
        self.resolved.retain(|_, effect| {
            effect.affected.retain(|object| live.contains(object));
            !effect.affected.is_empty()
        });
        self.counters.retain(|object, _| live.contains(object));
        self.object_timestamps
            .retain(|object, _| live.contains(object));
        // World departures remain until the actual publisher reconciles history
        // and records Lost; ordinary battlefield pruning cannot erase that fact.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_exhaustion_and_invalid_counter_counts_never_change_owned_state() {
        use crate::{
            object_identity::{CardId, GameObjects, ObjectDestination},
            priority::SeatId,
            stack_window::StackWindow,
        };
        let objects = GameObjects::new(0)
            .seed_card(0, CardId(1), SeatId(0), ObjectDestination::Hand)
            .unwrap()
            .next;
        let card = objects.current_identity(CardId(1)).unwrap();
        let window = StackWindow::new(vec![SeatId(0), SeatId(1)], SeatId(0), objects).unwrap();
        let window = window.finish_checks(window.revision()).unwrap();
        let window = window
            .record_card_spell(window.revision(), SeatId(0), card)
            .unwrap();
        let source = window.entries()[0].id();
        let mut state = CharacteristicState {
            next_effect: u64::MAX,
            ..Default::default()
        };
        let saved = state.clone();
        assert_eq!(
            state.add_effect(
                source,
                card,
                ResolvedParts(vec![ResolvedPart::SwitchPT]),
                ResolvedDuration::NoStatedDuration
            ),
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
