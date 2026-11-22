//! Ward retains the exact opposing entry and payer even if the permanent leaves.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, TriggerRelated, ZoneReason};
use crate::program::{ObjectFilter, SubtypeFilter, TriggerEvent};

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
            let spell_watcher = is_spell
                && current.objects.values().any(|watcher| {
                    watcher.zone == ObjectZone::Battlefield
                        && watcher.printed_text.triggered.iter().any(|ability| {
                            watcher.abilities.triggered.contains(&ability.id)
                                && match ability.condition.event {
                                    TriggerEvent::Targeted {
                                        subject,
                                        by_opponent: false,
                                        spell_only: true,
                                        instant_or_sorcery_only: false,
                                    } if subject == ObjectFilter::self_object() => {
                                        watcher.object == object
                                    }
                                    TriggerEvent::Targeted {
                                        subject,
                                        by_opponent: false,
                                        spell_only: true,
                                        instant_or_sorcery_only: false,
                                    } if subject == ObjectFilter::any().attached_to_self() => {
                                        watcher.attached_to == Some(TargetRef::Object(object))
                                    }
                                    TriggerEvent::Targeted {
                                        subject,
                                        by_opponent: false,
                                        spell_only: true,
                                        instant_or_sorcery_only: true,
                                    } if subject == ObjectFilter::creature().controlled() => {
                                        instant_or_sorcery
                                            && sheet.is_creature()
                                            && watcher.controller == Some(controller)
                                    }
                                    _ => false,
                                }
                        })
                });
            if spell_watcher {
                objects.insert(object);
                continue;
            }
            if controller == entry.controller() {
                continue;
            }
            let has_ward = sheet.printed_text.triggered.iter().any(|ability| {
                matches!(
                    ability.condition.event,
                    TriggerEvent::Targeted { subject, by_opponent: true, spell_only: false, instant_or_sorcery_only: false }
                        if subject == ObjectFilter::self_object()
                ) && sheet.abilities.triggered.contains(&ability.id)
            });
            let has_subtype_watcher = sheet.is_creature() && current.objects.values().any(|watcher| {
                watcher.zone == ObjectZone::Battlefield && watcher.controller == Some(controller)
                    && watcher.printed_text.triggered.iter().any(|ability| {
                        matches!(
                            ability.condition.event,
                            TriggerEvent::Targeted { subject: ObjectFilter::Clause(c), by_opponent: true, spell_only: false, instant_or_sorcery_only: false }
                                if matches!(c.subtypes, SubtypeFilter::Has(subtype) if sheet.current_creature_subtypes.contains(&subtype))
                        ) && watcher.abilities.triggered.contains(&ability.id)
                    })
            });
            if has_ward || has_subtype_watcher {
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
        if !self.window.entries().iter().any(|s| s.id() == target) {
            return Ok(());
        }
        if self.spell_cannot_be_countered(target)? {
            return Ok(());
        }
        let previous = self.rules_snapshot(None)?;
        let (window, exit) = self.window.counter_captured_entry(target)?;
        self.window = window;
        self.payloads.remove(&target);
        self.emit_from(
            EventCause::CounterSpell {
                entry: ward,
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
        )
    }
}
