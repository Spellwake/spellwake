//! Resolution of a mutating creature spell (CR 702.140).
use super::*;

/// The controller of a resolving spell decides the physical order at the merge cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationFrame {
    revision: u64,
    chooser: SeatId,
    entry: StackEntryId,
    spell: ObjectId,
    host: ObjectId,
}
impl MutationFrame {
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn spell(&self) -> ObjectId {
        self.spell
    }
    pub fn host(&self) -> ObjectId {
        self.host
    }
}

impl EffectWindow {
    pub(super) fn legal_mutation_target(
        &self,
        owner: SeatId,
        target: TargetRef,
        current: &EvaluatedCharacteristics,
    ) -> bool {
        let TargetRef::Object(id) = target else {
            return false;
        };
        let Some(view) = current.get(id) else {
            return false;
        };
        view.zone == ObjectZone::Battlefield
            && view.is_creature()
            && self
                .window
                .cards()
                .get(id)
                .is_some_and(|o| o.owner() == owner)
            && !self
                .creature_vocabulary
                .resolve_label("Human")
                .is_some_and(|human| view.current_creature_subtypes.contains(&human))
    }

    pub(super) fn mutating_spell(
        &self,
        entry: &crate::stack_window::StackEntry,
    ) -> Option<ObjectId> {
        let object = self.window.spell_object(entry)?;
        (entry.paid_conditional_cost()
            && !entry.targets().targets().is_empty()
            && self.copiable_program(object).is_some_and(|p| {
                matches!(
                    p.conditional_cost,
                    Some(crate::program::ConditionalCastCost::Mutate(_))
                )
            }))
        .then_some(object)
    }

    pub(super) fn mutation_frame(&self) -> Option<MutationFrame> {
        let entry = self.window.resolving()?;
        let spell = self.mutating_spell(entry)?;
        let [bound] = entry.targets().targets() else {
            return None;
        };
        let TargetRef::Object(host) = bound.choice().target else {
            return None;
        };
        Some(MutationFrame {
            revision: self.revision,
            chooser: entry.controller(),
            entry: entry.id(),
            spell,
            host,
        })
    }

    pub(super) fn resume_mutation(
        &self,
        expected: u64,
        frame: &MutationFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.mutation_frame().as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if frame.chooser != actor {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::MutateOnTop(on_top) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .clone();
        let before = self.rules_snapshot(None)?;
        let spell = self
            .copiable_program(frame.spell)
            .ok_or(EffectError::InconsistentProgram)?;
        let host = self
            .window
            .cards()
            .get(frame.host)
            .ok_or(EffectError::InconsistentProgram)?;
        let host_base = host
            .base_program(&self.programs)
            .ok_or(EffectError::InconsistentProgram)?;
        // If a face-up spell goes over a face-down component, the latter
        // supplies its face-down characteristics. Underneath a face-down host,
        // the actual face remains available when the permanent is turned up.
        let face_down_under = host.face_down().filter(|_| on_top);
        let base = face_down_under.map_or(host_base, |kind| kind.definition());
        let combine = |old: &Program| {
            if on_top {
                spell.with_component_abilities(old)
            } else {
                old.with_component_abilities(spell)
            }
        };
        let top_face = if on_top {
            spell
        } else {
            host.displayed_face(host_base)
        };
        let combined = combine(base)?;
        let mut next = self.clone();
        next.revision = revision;
        if face_down_under.is_none() {
            next.characteristics.update_copy_components(
                frame.host,
                on_top.then_some(spell),
                combine,
            )?;
        }
        next.window.merge_resolving_spell(
            frame.entry,
            frame.spell,
            frame.host,
            on_top,
            combined,
            top_face.clone(),
        )?;
        let mut append = RulesJournal::default();
        next.emit_from(
            crate::occurrence::EventCause::ResolutionCompleted {
                entry: entry.id(),
                completion: crate::occurrence::EntryCompletion::Resolved,
            },
            &before,
            None,
            &mut append,
            |_, _, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::Mutated {
                    permanent: frame.host,
                }])
            },
        )?;
        next.complete_at_cut(
            &entry,
            crate::occurrence::EntryCompletion::Resolved,
            None,
            &mut append,
        )?;
        Ok(RuleProgress::new(
            next,
            RuleResult::Complete {
                completion: RuleCompletion::Resolution,
                checks: CheckJournal::default(),
                journal: DamageJournal::default(),
                declarations: vec![],
                assignments: vec![],
                permanent_entry: None,
                draws: DrawJournal::default(),
            },
            append,
        ))
    }
}

impl Program {
    fn with_component_abilities(&self, lower: &Program) -> Result<Program, EffectError> {
        let mut result = self.clone();
        result.printed_keywords.extend(&lower.printed_keywords);
        if let Some(toxic) = &lower.printed_toxic {
            result.printed_toxic = Some(
                result
                    .printed_toxic
                    .as_ref()
                    .map_or_else(|| toxic.clone(), |a| a.plus(toxic)),
            );
        }
        result
            .face_up_abilities
            .extend(lower.face_up_abilities.iter().cloned());
        result
            .entry_counter_abilities
            .extend(lower.entry_counter_abilities.iter().cloned());
        result
            .cast_reductions
            .extend(lower.cast_reductions.iter().cloned());
        result.combat_limits.one_attacker |= lower.combat_limits.one_attacker;
        result.combat_limits.one_blocker |= lower.combat_limits.one_blocker;
        if let Some(req) = lower.attack_requirement {
            if result.attack_requirement.is_none() {
                result.attack_requirement = Some(req);
            } else if result.attack_requirement != Some(req)
                && !result.additional_attack_requirements.contains(&req)
            {
                result.additional_attack_requirements.push(req);
            }
        }
        for &req in &lower.additional_attack_requirements {
            if result.attack_requirement != Some(req)
                && !result.additional_attack_requirements.contains(&req)
            {
                result.additional_attack_requirements.push(req);
            }
        }
        for ability in &lower.triggered {
            let mut ability = ability.clone();
            ability.id = crate::program::TriggeredAbilityId(next_component_id(
                result.triggered.iter().map(|a| a.id.0),
                ability.id.0,
            )?);
            result.triggered.push(ability);
        }
        for ability in &lower.activated {
            let mut added = ability.clone();
            added.id = crate::activated::ActivatedAbilityId(next_component_id(
                result.activated.iter().map(|a| a.id.0),
                ability.id.0,
            )?);
            if let Some(limit) = lower.activation_restrictions.get(&ability.id) {
                result
                    .activation_restrictions
                    .insert(added.id, limit.clone());
            }
            if let Some(cost) = lower.loyalty_costs.get(&ability.id) {
                result.loyalty_costs.insert(added.id, *cost);
            }
            if lower.variable_loyalty_costs.contains(&ability.id) {
                result.variable_loyalty_costs.insert(added.id);
            }
            result.activated.push(added);
        }
        for ability in &lower.mana {
            let mut added = ability.clone();
            added.id = crate::mana::ManaAbilityId(next_component_id(
                result.mana.iter().map(|a| a.id.0),
                ability.id.0,
            )?);
            result.mana.push(added);
        }
        for ability in &lower.zone_replacements {
            let mut added = ability.clone();
            added.id = crate::program::ZoneReplacementAbilityId(next_component_id(
                result.zone_replacements.iter().map(|a| a.id.0),
                ability.id.0,
            )?);
            result.zone_replacements.push(added);
        }
        for ability in lower
            .continuous
            .iter()
            .chain(&lower.additional_battlefield_abilities)
        {
            let id = next_component_id(
                result
                    .continuous
                    .iter()
                    .chain(&result.additional_battlefield_abilities)
                    .map(|a| a.id().0),
                ability.id().0,
            )?;
            let ability = ability
                .clone()
                .with_component_id(crate::characteristics::StaticAbilityId(id));
            if result.continuous.is_none() {
                result.continuous = Some(ability);
            } else {
                result.additional_battlefield_abilities.push(ability);
            }
        }
        Ok(result)
    }
}

fn next_component_id(ids: impl Iterator<Item = u64>, printed: u64) -> Result<u64, EffectError> {
    let mut high = 0;
    let mut used = false;
    for id in ids {
        high = high.max(id);
        used |= id == printed;
    }
    if used {
        high.checked_add(1).ok_or(EffectError::InconsistentProgram)
    } else {
        Ok(printed)
    }
}
