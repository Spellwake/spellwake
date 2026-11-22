//! An entry-copy choice precedes general entry replacements and uses the unchanged donor world.
use super::*;
use crate::program::{EntryCopyRule, Supertype};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EntryCopyResult {
    pub(super) definition: Arc<Program>,
    pub(super) extra_counters: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct EntryCopyPool {
    programs: BTreeMap<ObjectId, Arc<Program>>,
    graves: Vec<ObjectId>,
    creatures: Vec<ObjectId>,
    controlled: BTreeMap<SeatId, Vec<ObjectId>>,
    battlefield_replacements: BTreeMap<SeatId, Vec<(ObjectId, ObjectId)>>,
    token_replacements: BTreeMap<SeatId, Vec<(ObjectId, ObjectId)>>,
}

impl EntryCopyPool {
    pub(super) fn has_battlefield_copy_rules(
        current: &EvaluatedCharacteristics,
        for_tokens: bool,
    ) -> bool {
        use crate::characteristics::{StaticContinuousAbility as Ability, StaticModifierRule};
        current.objects.values().any(|source| {
            source.has_battlefield_rule(|rule| {
                matches!(
                    rule,
                    Ability::Modifier {
                        rule: StaticModifierRule::EnterControlledCreaturesAsSourceCopy,
                        ..
                    }
                )
            }) || (!for_tokens
                && source.has_battlefield_rule(|rule| {
                    matches!(
                        rule,
                        Ability::Modifier {
                            rule: StaticModifierRule::EnterControlledCreaturesAsEnchantedCopy,
                            ..
                        }
                    )
                }))
        })
    }

    pub(super) fn capture(
        window: &EffectWindow,
        current: &EvaluatedCharacteristics,
    ) -> Result<Self, EffectError> {
        let mut pool = Self::default();
        for object in window.window.cards().iter() {
            let Some(card) = current.get(object.id()) else {
                continue;
            };
            match card.zone {
                ObjectZone::Graveyard(_) if card.is_creature() && object.card_id().is_some() => {
                    pool.graves.push(object.id())
                }
                ObjectZone::Battlefield
                    if card.is_creature() || card.has_type(CardType::Planeswalker) =>
                {
                    let controller = card.controller.ok_or(EffectError::InconsistentProgram)?;
                    if card.is_creature() {
                        pool.creatures.push(object.id());
                    }
                    pool.controlled
                        .entry(controller)
                        .or_default()
                        .push(object.id());
                }
                _ => continue,
            }
            let donor = window
                .copiable_program(object.id())
                .ok_or(EffectError::InconsistentProgram)?;
            pool.programs.insert(object.id(), Arc::new(donor.clone()));
        }
        for aura in current.objects.values().filter(|view| view.has_battlefield_rule(|rule| matches!(rule,
            crate::characteristics::StaticContinuousAbility::Modifier { rule: crate::characteristics::StaticModifierRule::EnterControlledCreaturesAsEnchantedCopy, .. }
        ))) {
            let (Some(TargetRef::Object(host)), Some(player)) = (aura.attached_to, aura.controller) else { continue; };
            if current.get(host).is_none_or(|view| view.zone != ObjectZone::Battlefield || !view.is_creature()) {
                continue;
            }
            pool.battlefield_replacements.entry(player).or_default().push((aura.object, host));
        }
        for source in current.objects.values().filter(|view| view.has_battlefield_rule(|rule| matches!(rule,
            crate::characteristics::StaticContinuousAbility::Modifier { rule: crate::characteristics::StaticModifierRule::EnterControlledCreaturesAsSourceCopy, .. }
        ))) {
            let player = source.controller.ok_or(EffectError::InconsistentProgram)?;
            if let std::collections::btree_map::Entry::Vacant(entry) = pool.programs.entry(source.object) {
                let program = window.copiable_program(source.object).ok_or(EffectError::InconsistentProgram)?;
                entry.insert(Arc::new(program.clone()));
            }
            pool.battlefield_replacements.entry(player).or_default().push((source.object, source.object));
            pool.token_replacements.entry(player).or_default().push((source.object, source.object));
        }
        Ok(pool)
    }
    pub(super) fn battlefield_donors(&self, controller: SeatId) -> &[(ObjectId, ObjectId)] {
        self.battlefield_replacements
            .get(&controller)
            .map_or(&[], Vec::as_slice)
    }
    pub(super) fn token_donors(&self, controller: SeatId) -> &[(ObjectId, ObjectId)] {
        self.token_replacements
            .get(&controller)
            .map_or(&[], Vec::as_slice)
    }

    pub(super) fn battlefield_copy(
        &self,
        donor: ObjectId,
        previous: Option<&EntryCopyResult>,
    ) -> Result<EntryCopyResult, EffectError> {
        let definition = self
            .programs
            .get(&donor)
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        Ok(EntryCopyResult {
            definition,
            extra_counters: previous.map_or(0, |copy| copy.extra_counters),
        })
    }

    pub(super) fn domain(&self, rule: &EntryCopyRule, controller: SeatId) -> &[ObjectId] {
        match rule {
            EntryCopyRule::GraveyardCreature => &self.graves,
            EntryCopyRule::BattlefieldCreatureWithAbility(_) => &self.creatures,
            EntryCopyRule::ControlledCreatureOrPlaneswalker => {
                self.controlled.get(&controller).map_or(&[], Vec::as_slice)
            }
        }
    }
    pub(super) fn select(
        &self,
        rule: &EntryCopyRule,
        controller: SeatId,
        donor: ObjectId,
        previous: Option<&EntryCopyResult>,
    ) -> Result<EntryCopyResult, EffectError> {
        if !self.domain(rule, controller).contains(&donor) {
            return Err(EffectError::IllegalTarget);
        }
        let mut definition = self
            .programs
            .get(&donor)
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        let mut extra_counters = previous.map_or(0, |copy| copy.extra_counters);
        if *rule == EntryCopyRule::ControlledCreatureOrPlaneswalker {
            Arc::make_mut(&mut definition)
                .supertypes
                .remove(&Supertype::Legendary);
            extra_counters = extra_counters
                .checked_add(1)
                .ok_or(EffectError::InconsistentProgram)?;
        }
        if let EntryCopyRule::BattlefieldCreatureWithAbility(ability) = rule {
            let definition = Arc::make_mut(&mut definition);
            let mut ability = (**ability).clone();
            let next = definition
                .activated
                .iter()
                .try_fold(0_u64, |candidate, ability| {
                    ability.id.0.checked_add(1).map(|next| next.max(candidate))
                })
                .ok_or(EffectError::InconsistentProgram)?;
            ability.id = crate::activated::ActivatedAbilityId(next);
            definition.activated.push(ability);
        }
        Ok(EntryCopyResult {
            definition,
            extra_counters,
        })
    }
}

impl EffectWindow {
    pub(super) fn prospective_entry_face(&self, object: ObjectId) -> Option<&Program> {
        let program = self.copiable_program(object)?;
        if self.linked_return_work.as_ref().is_some_and(|work| {
            matches!(work.origin, LinkedReturnOrigin::GraveyardTransformed { .. })
                && work.objects.contains(&object)
        }) {
            return program.transform_back.as_deref();
        }
        Some(program)
    }

    pub(super) fn prospective_definition(&self, object: ObjectId) -> Option<&Program> {
        self.prospective_entry_copies
            .get(&object)
            .map(|copy| copy.definition.as_ref())
            .or_else(|| self.prospective_entry_face(object))
    }
}
