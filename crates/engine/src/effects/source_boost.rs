//! Resolving a targetless activated ability changes its original source (CR 113.7a).
use super::*;
use crate::{
    characteristics::{ResolvedDuration, ResolvedParts},
    occurrence::{EventCause, RulesOccurrence},
    stack_window::StackEntry,
};
impl EffectWindow {
    pub(super) fn effect_group(
        &self,
        entry: &StackEntry,
        group: crate::program::EffectSubject,
    ) -> Result<BTreeSet<ObjectId>, EffectError> {
        use crate::program::EffectSubject;
        let view = self.evaluated_characteristics()?;
        match group {
            EffectSubject::TriggeringAttacker
            | EffectSubject::TriggeringAttackGroup
            | EffectSubject::TriggeringTarget => {
                let binding = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?;
                let [cause] = binding.causes.members() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let triggering = match &cause.related {
                    crate::occurrence::TriggerRelated::AttackedAlone(id)
                    | crate::occurrence::TriggerRelated::Combatant(id) => std::slice::from_ref(id),
                    crate::occurrence::TriggerRelated::AttackGroup { creatures, .. } => {
                        creatures.as_slice()
                    }
                    crate::occurrence::TriggerRelated::TargetedPermanent(id) => {
                        std::slice::from_ref(id)
                    }
                    _ => return Err(EffectError::InconsistentProgram),
                };
                Ok(view
                    .objects
                    .values()
                    .filter(|p| p.zone == ObjectZone::Battlefield && triggering.contains(&p.object))
                    .map(|p| p.object)
                    .collect())
            }
            EffectSubject::TriggerSubject => {
                let Some(binding) = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                else {
                    return Ok(BTreeSet::new());
                };
                let ids: BTreeSet<ObjectId> = binding
                    .causes
                    .members()
                    .iter()
                    .filter_map(|m| m.subject_object_id())
                    .collect();
                Ok(view
                    .objects
                    .values()
                    .filter(|p| p.zone == ObjectZone::Battlefield && ids.contains(&p.object))
                    .map(|p| p.object)
                    .collect())
            }
            EffectSubject::AbilitySource => Ok(view
                .objects
                .values()
                .filter(|p| {
                    p.zone == ObjectZone::Battlefield
                        && matches!(
                            entry.kind(),
                            StackKind::TriggeredAbility { source }
                                | StackKind::ActivatedAbility { source }
                                if source.id() == p.object
                        )
                })
                .map(|p| p.object)
                .collect()),
            EffectSubject::TargetObject => {
                if let Ok(target) = self.instruction_target(entry) {
                    if let TargetRef::Object(id) = target.choice().target
                        && view
                            .get(id)
                            .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                    {
                        return Ok(BTreeSet::from([id]));
                    }
                    return Ok(BTreeSet::new());
                }
                if let Ok(selected) = self.legal_permanent_targets() {
                    return Ok(selected
                        .into_iter()
                        .filter(|id| {
                            view.get(*id)
                                .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                        })
                        .collect());
                }
                Ok(BTreeSet::new())
            }
            EffectSubject::CreatedTokens => Ok(self
                .resolution_cursor()
                .map(|cursor| {
                    cursor
                        .last_created_tokens
                        .iter()
                        .copied()
                        .filter(|id| {
                            view.get(*id)
                                .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                        })
                        .collect()
                })
                .unwrap_or_default()),
            EffectSubject::LastMoved => Ok(self
                .resolution_cursor()
                .map(|cursor| {
                    let mut ids: BTreeSet<ObjectId> = cursor
                        .results
                        .last_moved_objects
                        .iter()
                        .copied()
                        .filter(|id| {
                            view.get(*id)
                                .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                        })
                        .collect();
                    if let Some(id) = cursor.results.returned_target_permanent
                        && view
                            .get(id)
                            .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                    {
                        ids.insert(id);
                    }
                    ids
                })
                .unwrap_or_default()),
            EffectSubject::Matching(filter) => {
                let mut excluded = BTreeSet::new();
                if let Some(slot) = filter.chosen_type_slot() {
                    let kind = *self
                        .resolution_cursor()
                        .and_then(|cursor| cursor.values.get(&slot))
                        .ok_or(EffectError::InconsistentProgram)?;
                    excluded.insert(kind);
                } else if filter.uses_outside_named_slots()
                    && let Some(cursor) = self.resolution_cursor()
                {
                    excluded.extend(cursor.values.values().copied());
                }
                let ability_source = match entry.kind() {
                    StackKind::TriggeredAbility { source }
                    | StackKind::ActivatedAbility { source } => Some(source.id()),
                    _ => None,
                };
                Ok(view
                    .objects
                    .values()
                    .filter(|p| {
                        p.zone == ObjectZone::Battlefield
                            && self.matches_object_filter(
                                filter,
                                p,
                                entry.controller(),
                                ability_source,
                                &excluded,
                                &view,
                            )
                    })
                    .map(|p| p.object)
                    .collect())
            }
            EffectSubject::TargetAndSameNameMatching(filter) => {
                let targets: Vec<ObjectId> = if let Ok(target) = self.instruction_target(entry) {
                    if let TargetRef::Object(id) = target.choice().target
                        && view
                            .get(id)
                            .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                    {
                        vec![id]
                    } else {
                        Vec::new()
                    }
                } else if let Ok(selected) = self.legal_permanent_targets() {
                    selected
                        .into_iter()
                        .filter(|id| {
                            view.get(*id)
                                .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                if targets.is_empty() {
                    return Ok(BTreeSet::new());
                }
                let mut excluded = BTreeSet::new();
                if let Some(slot) = filter.chosen_type_slot() {
                    let kind = *self
                        .resolution_cursor()
                        .and_then(|cursor| cursor.values.get(&slot))
                        .ok_or(EffectError::InconsistentProgram)?;
                    excluded.insert(kind);
                } else if filter.uses_outside_named_slots()
                    && let Some(cursor) = self.resolution_cursor()
                {
                    excluded.extend(cursor.values.values().copied());
                }
                let ability_source = match entry.kind() {
                    StackKind::TriggeredAbility { source }
                    | StackKind::ActivatedAbility { source } => Some(source.id()),
                    _ => None,
                };
                let mut affected: BTreeSet<ObjectId> = targets.iter().copied().collect();
                for &target_id in &targets {
                    let Some(target_view) = view.get(target_id) else {
                        continue;
                    };
                    for p in view.objects.values() {
                        if p.zone == ObjectZone::Battlefield
                            && p.object != target_id
                            && self
                                .name_rules
                                .shares_name(&target_view.current_names, &p.current_names)
                            && self.matches_object_filter(
                                filter,
                                p,
                                entry.controller(),
                                ability_source,
                                &excluded,
                                &view,
                            )
                        {
                            affected.insert(p.object);
                        }
                    }
                }
                Ok(affected)
            }
            EffectSubject::MatchingOtherThanTarget(filter) => {
                let mut excluded_targets = BTreeSet::new();
                if let Ok(target) = self.instruction_target(entry) {
                    if let TargetRef::Object(id) = target.choice().target {
                        excluded_targets.insert(id);
                    }
                } else if let Ok(selected) = self.legal_permanent_targets() {
                    excluded_targets.extend(selected);
                }
                let mut excluded = BTreeSet::new();
                if let Some(slot) = filter.chosen_type_slot() {
                    let kind = *self
                        .resolution_cursor()
                        .and_then(|cursor| cursor.values.get(&slot))
                        .ok_or(EffectError::InconsistentProgram)?;
                    excluded.insert(kind);
                } else if filter.uses_outside_named_slots()
                    && let Some(cursor) = self.resolution_cursor()
                {
                    excluded.extend(cursor.values.values().copied());
                }
                let ability_source = match entry.kind() {
                    StackKind::TriggeredAbility { source }
                    | StackKind::ActivatedAbility { source } => Some(source.id()),
                    _ => None,
                };
                Ok(view
                    .objects
                    .values()
                    .filter(|p| {
                        p.zone == ObjectZone::Battlefield
                            && !excluded_targets.contains(&p.object)
                            && self.matches_object_filter(
                                filter,
                                p,
                                entry.controller(),
                                ability_source,
                                &excluded,
                                &view,
                            )
                    })
                    .map(|p| p.object)
                    .collect())
            }
            EffectSubject::TargetAndSharingColorMatching(filter) => {
                let targets: Vec<ObjectId> = if let Ok(target) = self.instruction_target(entry) {
                    if let TargetRef::Object(id) = target.choice().target
                        && view
                            .get(id)
                            .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                    {
                        vec![id]
                    } else {
                        Vec::new()
                    }
                } else if let Ok(selected) = self.legal_permanent_targets() {
                    selected
                        .into_iter()
                        .filter(|id| {
                            view.get(*id)
                                .is_some_and(|p| p.zone == ObjectZone::Battlefield)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                if targets.is_empty() {
                    return Ok(BTreeSet::new());
                }
                let mut excluded = BTreeSet::new();
                if let Some(slot) = filter.chosen_type_slot() {
                    let kind = *self
                        .resolution_cursor()
                        .and_then(|cursor| cursor.values.get(&slot))
                        .ok_or(EffectError::InconsistentProgram)?;
                    excluded.insert(kind);
                } else if filter.uses_outside_named_slots()
                    && let Some(cursor) = self.resolution_cursor()
                {
                    excluded.extend(cursor.values.values().copied());
                }
                let ability_source = match entry.kind() {
                    StackKind::TriggeredAbility { source }
                    | StackKind::ActivatedAbility { source } => Some(source.id()),
                    _ => None,
                };
                let mut affected: BTreeSet<ObjectId> = targets.iter().copied().collect();
                for &target_id in &targets {
                    let Some(target_view) = view.get(target_id) else {
                        continue;
                    };
                    for p in view.objects.values() {
                        if p.zone == ObjectZone::Battlefield
                            && p.object != target_id
                            && !target_view.current_colors.is_disjoint(&p.current_colors)
                            && self.matches_object_filter(
                                filter,
                                p,
                                entry.controller(),
                                ability_source,
                                &excluded,
                                &view,
                            )
                        {
                            affected.insert(p.object);
                        }
                    }
                }
                Ok(affected)
            }
        }
    }

    pub(super) fn modify_objects_for_duration(
        &mut self,
        entry: &StackEntry,
        index: usize,
        affected: BTreeSet<ObjectId>,
        parts: ResolvedParts,
        duration: ResolvedDuration,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if affected.is_empty() {
            return Ok(());
        }
        let Some(duration) = self.resolve_duration(entry, duration)? else {
            return Ok(());
        };
        let before = self.rules_snapshot(None)?;
        let effect =
            self.characteristics
                .add_effect_for_objects(entry.id(), affected, parts, duration)?;
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            },
            &before,
            None,
            append,
            |previous, current, _| {
                let appearances = occurrence_flow::characteristic_appearance_changes(
                    previous,
                    current,
                    effect.affected.iter().copied(),
                )?;
                Ok(vec![RulesOccurrence::ContinuousEffectCreated {
                    effect,
                    appearances,
                }])
            },
        )
    }
}
