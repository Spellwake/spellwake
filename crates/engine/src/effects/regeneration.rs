//! CR 701.19. Shield creation and regeneration are two separate events.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, StatusReason};

impl EffectWindow {
    pub(super) fn create_regeneration_shield(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        targeted: bool,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let object = if targeted {
            let TargetRef::Object(id) = self.instruction_target(entry)?.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            id
        } else {
            match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    source.id()
                }
                _ => return Err(EffectError::InconsistentProgram),
            }
        };
        if !self
            .window
            .cards()
            .get(object)
            .is_some_and(|o| o.zone() == ObjectZone::Battlefield)
        {
            return Ok(());
        }
        self.create_regeneration_shields(entry.id(), index, vec![object], append)
    }

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
        Ok(occurrences)
    }
}
