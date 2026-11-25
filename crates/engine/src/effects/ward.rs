//! Ward retains the exact opposing entry and payer even if the permanent leaves.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, TriggerRelated, ZoneReason};
use crate::program::{ObjectFilter, TriggerEvent};

impl EffectWindow {
    pub(super) fn permanent_target_events(
        &self,
        id: StackEntryId,
        current: &EvaluatedCharacteristics,
    ) -> Result<Vec<RulesOccurrence>, EffectError> {
        let entry = self
            .window
            .entries()
            .iter()
            .find(|s| s.id() == id)
            .ok_or(EffectError::InconsistentProgram)?;
        let is_spell = matches!(
            entry.kind(),
            StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
        );
        let instant_or_sorcery = self.stack_copiable_card_types(id).is_some_and(|types| {
            crate::program::SpellFilter::instant_or_sorcery().matches_card_types(types)
        });
        let mut objects = BTreeSet::new();
        for target in entry.targets().targets() {
            let TargetRef::Object(object) = target.choice().target else {
                continue;
            };
            let Some(sheet) = current.get(object) else {
                continue;
            };
            let Some(controller) = sheet.controller else {
                continue;
            };
            if sheet.zone != ObjectZone::Battlefield {
                continue;
            }
            let excluded = BTreeSet::new();
            let has_targeted_watcher = current.objects.values().any(|watcher| {
                watcher.zone == ObjectZone::Battlefield
                    && watcher
                        .printed_text
                        .triggered
                        .iter()
                        .filter(|ability| watcher.abilities.triggered.contains(&ability.id))
                        .chain(watcher.granted_triggers.values())
                        .any(|ability| {
                            let TriggerEvent::Targeted {
                                subject,
                                by_opponent,
                                spell_only,
                                instant_or_sorcery_only,
                            } = ability.condition.event
                            else {
                                return false;
                            };
                            if by_opponent && controller == entry.controller() {
                                return false;
                            }
                            if spell_only && !is_spell {
                                return false;
                            }
                            if instant_or_sorcery_only && !instant_or_sorcery {
                                return false;
                            }
                            if subject == ObjectFilter::self_object() {
                                watcher.object == object
                            } else if subject == ObjectFilter::any().attached_to_self() {
                                watcher.attached_to == Some(TargetRef::Object(object))
                            } else if let Some(watcher_ctrl) = watcher.controller {
                                self.matches_object_filter(
                                    subject,
                                    sheet,
                                    watcher_ctrl,
                                    Some(watcher.object),
                                    &excluded,
                                    current,
                                )
                            } else {
                                false
                            }
                        })
            });
            if has_targeted_watcher {
                objects.insert(object);
            }
        }
        Ok(objects
            .into_iter()
            .map(|object| RulesOccurrence::PermanentTargeted {
                object,
                entry: id,
                controller: entry.controller(),
                instant_or_sorcery,
            })
            .collect())
    }

    pub(super) fn counter_ward_source(
        &mut self,
        ward: StackEntryId,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let binding = self
            .execution_bindings(ward)
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let [member] = binding.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let TriggerRelated::TargetingEntry(target) = member.related else {
            return Err(EffectError::InconsistentProgram);
        };
        let _ = self.counter_captured_stack_entry(ward, index, target, append)?;
        Ok(())
    }

    pub(super) fn counter_triggering_stack_object(
        &mut self,
        entry: StackEntryId,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let binding = self
            .execution_bindings(entry)
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let Some(member) = binding.causes.members().first() else {
            return Err(EffectError::InconsistentProgram);
        };
        let target = match &member.related {
            TriggerRelated::TargetingEntry(target) | TriggerRelated::SpellCopied(target) => *target,
            TriggerRelated::SpellCast(cast) => cast.entry(),
            _ => return Err(EffectError::InconsistentProgram),
        };
        let countered = self.counter_captured_stack_entry(entry, index, target, append)?;
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .results
            .last_action_succeeded = countered;
        Ok(())
    }

    fn counter_captured_stack_entry(
        &mut self,
        source_entry: StackEntryId,
        index: usize,
        target: StackEntryId,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        if !self.window.entries().iter().any(|s| s.id() == target) {
            return Ok(false);
        }
        if self.spell_cannot_be_countered(target)? {
            return Ok(false);
        }
        let previous = self.rules_snapshot(None)?;
        let (window, exit) = self.window.counter_captured_entry(target)?;
        self.window = window;
        self.payloads.remove(&target);
        self.emit_from(
            EventCause::CounterSpell {
                entry: source_entry,
                instruction: index,
                target,
            },
            &previous,
            None,
            append,
            |before, after, controllers| {
                Ok(vec![match exit {
                    Some(exit) => super::occurrence_flow::exit_occurrence_at(
                        before,
                        after,
                        exit,
                        ZoneReason::Countered,
                        controllers,
                    )?,
                    None => RulesOccurrence::AbilityCountered { entry: target },
                }])
            },
        )?;
        Ok(true)
    }
}
