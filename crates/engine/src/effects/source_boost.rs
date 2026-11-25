//! Resolving a targetless activated ability changes its original source (CR 113.7a).
use super::*;
use crate::{
    characteristics::{
        AbilityChange, PowerToughness, ResolvedDuration, ResolvedPart, ResolvedParts,
    },
    occurrence::{EventCause, RulesOccurrence},
    stack_window::StackEntry,
};
impl EffectWindow {
    pub(super) fn resolve_source_boost(
        &mut self,
        entry: &StackEntry,
        index: usize,
        stats: Option<PowerToughness>,
        keywords: BTreeSet<KeywordAbility>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let (StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source }) =
            entry.kind()
        else {
            return Err(EffectError::InconsistentProgram);
        };
        let id = source.id();
        if !self
            .window
            .cards()
            .get(id)
            .is_some_and(|o| o.zone() == ObjectZone::Battlefield)
        {
            return Ok(());
        }
        self.modify_objects(entry, index, [id].into(), stats, keywords, append)
    }

    pub(super) fn modify_group(
        &mut self,
        entry: &StackEntry,
        index: usize,
        group: crate::program::EffectSubject,
        stats: Option<PowerToughness>,
        keywords: BTreeSet<KeywordAbility>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let affected = self.effect_group(entry, group)?;
        if affected.is_empty() {
            return Ok(());
        }
        self.modify_objects(entry, index, affected, stats, keywords, append)
    }

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
        }
    }

    pub(super) fn untap_and_control_creatures(
        &mut self,
        entry: &StackEntry,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let affected = before
            .view()
            .evaluated
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield && p.is_creature())
            .map(|p| p.object)
            .collect::<BTreeSet<_>>();
        if affected.is_empty() {
            return Ok(());
        }
        let tapped = affected
            .iter()
            .copied()
            .filter(|id| self.window.cards().get(*id).and_then(GameObject::tapped) == Some(true))
            .collect::<Vec<_>>();
        let cause = EventCause::Instruction {
            entry: entry.id(),
            instruction: index,
        };
        if !tapped.is_empty() {
            let occurrences = self.untap_for_effect_with_stun(entry.id(), &tapped)?;
            self.emit_from(cause.clone(), &before, None, append, |_, _, _| {
                Ok(occurrences)
            })?;
        }
        let before = self.rules_snapshot(None)?;
        let parts = ResolvedParts(vec![
            ResolvedPart::Control(entry.controller()),
            ResolvedPart::Abilities(AbilityChange::grant_keywords([KeywordAbility::Haste])),
        ]);
        let effect = self.characteristics.add_effect_for_objects(
            entry.id(),
            affected,
            parts,
            ResolvedDuration::UntilEndOfTurn,
        )?;
        self.emit_from(cause, &before, None, append, |old, new, _| {
            let appearances = occurrence_flow::characteristic_appearance_changes(
                old,
                new,
                effect.affected.iter().copied(),
            )?;
            Ok(vec![RulesOccurrence::ContinuousEffectCreated {
                effect,
                appearances,
            }])
        })
    }

    pub(super) fn modify_objects(
        &mut self,
        entry: &StackEntry,
        index: usize,
        affected: BTreeSet<ObjectId>,
        stats: Option<PowerToughness>,
        keywords: BTreeSet<KeywordAbility>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let mut parts = Vec::new();
        if let Some(stats) = stats {
            parts.push(ResolvedPart::ModifyPT(stats));
        }
        if !keywords.is_empty() {
            parts.push(ResolvedPart::Abilities(AbilityChange::grant_keywords(
                keywords,
            )));
        }
        self.modify_objects_until_cleanup(entry, index, affected, ResolvedParts(parts), append)
    }

    pub(super) fn modify_objects_until_cleanup(
        &mut self,
        entry: &StackEntry,
        index: usize,
        affected: BTreeSet<ObjectId>,
        parts: ResolvedParts,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.modify_objects_for_duration(
            entry,
            index,
            affected,
            parts,
            ResolvedDuration::UntilEndOfTurn,
            append,
        )
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
