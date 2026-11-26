//! CR 701.19. Shield creation and regeneration are two separate events.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, StatusReason};

impl EffectWindow {
    pub(super) fn create_regeneration_shields(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        subjects: Vec<ObjectId>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if subjects.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let occurrences = subjects
            .into_iter()
            .map(|object| {
                self.regeneration_shields
                    .entry(object)
                    .or_default()
                    .insert((entry, instruction));
                RulesOccurrence::RegenerationShieldCreated {
                    object,
                    entry,
                    instruction,
                }
            })
            .collect();
        self.emit_from(
            EventCause::Instruction { entry, instruction },
            &before,
            None,
            append,
            |_, _, _| Ok(occurrences),
        )
    }

    pub(super) fn apply_regenerations(
        &mut self,
        batch: Option<&ZoneReplacementBatch>,
    ) -> Result<Vec<RulesOccurrence>, EffectError> {
        let mut occurrences = Vec::new();
        let Some(batch) = batch else {
            return Ok(occurrences);
        };
        for (object, entry, instruction) in batch.regenerations() {
            let shields = self
                .regeneration_shields
                .get_mut(&object)
                .ok_or(EffectError::InconsistentProgram)?;
            if !shields.remove(&(entry, instruction)) {
                return Err(EffectError::InconsistentProgram);
            }
            if shields.is_empty() {
                self.regeneration_shields.remove(&object);
            }
            let removed = self.damage.remove(&object).unwrap_or_else(|| 0.into());
            self.deathtouch_damage.remove(&object);
            let in_combat = self.combat.as_ref().is_some_and(|combat| {
                combat.attacker(object).is_some() || combat.blocker(object).is_some()
            });
            if in_combat && let Some(combat) = &mut self.combat {
                combat.reconcile(|combatant, _| combatant.object() != object);
            }
            let before = self
                .window
                .cards()
                .get(object)
                .filter(|o| o.zone() == ObjectZone::Battlefield)
                .cloned()
                .ok_or(EffectError::InconsistentProgram)?;
            if before.tapped() == Some(false) {
                let after = self.window.enter_tapped(object)?;
                occurrences.push(RulesOccurrence::StatusChange {
                    change: crate::object_identity::ObjectStatusChange { before, after },
                    reason: StatusReason::Regeneration,
                });
            }
            occurrences.push(RulesOccurrence::PermanentRegenerated {
                object,
                entry,
                instruction,
                removed_damage: removed,
                removed_from_combat: in_combat,
            });
        }
        // CR 122.1c: Destruction replaced by removing a shield counter.
        for object in batch.shield_counter_saves() {
            let previous = self.counters_of_kind(object, CounterKind::Shield);
            if previous.is_positive() {
                let after = previous.minus(&1.into());
                if let Some(map) = self
                    .characteristics
                    .named_permanent_counters_mut(CounterKind::Shield)
                {
                    if after.is_positive() {
                        map.insert(object, after.clone());
                    } else {
                        map.remove(&object);
                    }
                }
                occurrences.push(RulesOccurrence::PermanentCounterChanged {
                    object,
                    kind: CounterKind::Shield,
                    before: previous,
                    after,
                });
            }
        }
        // CR 702.89a: Umbra armor replaces destruction of the enchanted permanent with
        // removing all damage marked on it and destroying the Aura.
        let umbra_saves: Vec<(ObjectId, ObjectId)> = batch.umbra_armor_saves().collect();
        for (object, aura) in umbra_saves {
            self.damage.remove(&object);
            self.deathtouch_damage.remove(&object);
            let can_destroy_aura = self
                .evaluated_characteristics()?
                .get(aura)
                .is_some_and(|p| {
                    p.zone == ObjectZone::Battlefield
                        && !p.has_keyword(KeywordAbility::Indestructible)
                });
            if can_destroy_aura {
                let before_move = self.rules_snapshot(None)?;
                let (window, moved) = self.window.destroy_umbra_aura(aura)?;
                self.window = window;
                self.characteristics.attachments.remove(&aura);
                if let Some(movement) = moved {
                    let after_move = self.rules_snapshot(None)?;
                    occurrences.push(super::occurrence_flow::zone_occurrence_at(
                        &before_move.view(),
                        &after_move.view(),
                        &movement,
                        crate::occurrence::ZoneReason::Destroyed,
                        &[],
                    )?);
                }
            }
        }
        Ok(occurrences)
    }
}
