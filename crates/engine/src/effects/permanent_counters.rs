//! Counter placement by a resolving permanent ability.
use super::*;
use crate::stack_window::StackEntry;
use crate::{
    characteristics::PowerToughnessCounters,
    occurrence::{EventCause, RulesOccurrence},
};

fn double_count(
    current: &crate::characteristics::EvaluatedCharacteristics,
    controller: SeatId,
) -> usize {
    current.controlled_rules(controller, |ability| {
        matches!(
            ability,
            crate::characteristics::StaticContinuousAbility::Modifier {
                rule: crate::characteristics::StaticModifierRule::DoublePlacedCounters,
                ..
            }
        )
    })
}

fn doubling_factor(count: usize) -> RulesInteger {
    (0..count).fold(1.into(), |multiplier: RulesInteger, _| {
        multiplier.plus(&multiplier)
    })
}

/// One factor of two per applicable live replacement, from the pre-placement cut.
pub(super) fn controlled_counter_multiplier(
    current: &crate::characteristics::EvaluatedCharacteristics,
    controller: SeatId,
) -> RulesInteger {
    doubling_factor(double_count(current, controller))
}

pub(super) fn effect_counter_multiplier(
    current: &crate::characteristics::EvaluatedCharacteristics,
    object: ObjectId,
) -> RulesInteger {
    current
        .get(object)
        .filter(|permanent| permanent.zone == ObjectZone::Battlefield)
        .and_then(|permanent| permanent.controller)
        .map_or_else(
            || 1.into(),
            |controller| controlled_counter_multiplier(current, controller),
        )
}

impl EffectWindow {
    pub(super) fn loyalty_transfer_source(
        &self,
        entry: &StackEntry,
    ) -> Result<Option<ObjectId>, EffectError> {
        let StackKind::TriggeredAbility { source } = entry.kind() else {
            return Err(EffectError::InconsistentProgram);
        };
        let source = source.id();
        Ok(self
            .current_characteristics(source)?
            .filter(|permanent| permanent.zone == ObjectZone::Battlefield)
            .filter(|_| {
                self.counters_of_kind(source, CounterKind::Loyalty)
                    .is_positive()
            })
            .map(|_| source))
    }

    pub(super) fn loyalty_transfer_pair(
        &self,
        entry: &StackEntry,
    ) -> Result<Option<(ObjectId, ObjectId)>, EffectError> {
        let Some(source) = self.loyalty_transfer_source(entry)? else {
            return Ok(None);
        };
        use crate::occurrence::TriggerRelated;
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|bindings| bindings.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let [cause] = binding.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let destination = match &cause.related {
            TriggerRelated::ZoneChange { after, .. }
            | TriggerRelated::TokenCreated { after, .. } => after.object.id(),
            _ => return Err(EffectError::InconsistentProgram),
        };
        Ok(self
            .window
            .cards()
            .get(destination)
            .filter(|object| destination != source && object.zone() == ObjectZone::Battlefield)
            .map(|_| (source, destination)))
    }

    pub(super) fn move_loyalty_to_entering_permanent(
        &mut self,
        entry: &StackEntry,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let Some((source, destination)) = self.loyalty_transfer_pair(entry)? else {
            return Ok(());
        };
        let before = self.rules_snapshot(None)?;
        let pre_effect = before.view().evaluated;
        let source_count = self.counters_of_kind(source, CounterKind::Loyalty);
        let destination_count = self.counters_of_kind(destination, CounterKind::Loyalty);
        let remaining = source_count.minus(&1.into());
        let increased =
            destination_count.plus(&effect_counter_multiplier(&pre_effect, destination));
        if remaining.is_positive() {
            self.characteristics
                .loyalty
                .insert(source, remaining.clone());
        } else {
            self.characteristics.loyalty.remove(&source);
        }
        self.characteristics
            .loyalty
            .insert(destination, increased.clone());
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![
                    RulesOccurrence::LoyaltyCountersChanged {
                        object: source,
                        before: source_count,
                        after: remaining,
                    },
                    RulesOccurrence::LoyaltyCountersChanged {
                        object: destination,
                        before: destination_count,
                        after: increased,
                    },
                ])
            },
        )
    }

    pub(super) fn adjusted_pt_counters(
        &mut self,
        battlefield_before: &crate::characteristics::EvaluatedCharacteristics,
        object: ObjectId,
        mut counters: PowerToughnessCounters,
        entering: Option<&ObjectCharacteristics>,
    ) -> Result<PowerToughnessCounters, EffectError> {
        let Some(permanent) = entering
            .or_else(|| battlefield_before.get(object))
            .filter(|permanent| permanent.zone == ObjectZone::Battlefield)
        else {
            return Ok(counters);
        };
        let controller = permanent
            .controller
            .ok_or(EffectError::InconsistentProgram)?;
        let doublers = double_count(battlefield_before, controller);
        let additional = counters.plus_one.is_positive() && permanent.has_battlefield_rule(|ability| {
            matches!(ability, crate::characteristics::StaticContinuousAbility::Modifier { rule: crate::characteristics::StaticModifierRule::AdditionalSelfPlusOneCounter, .. })
        });
        let doublers_before_addition = if additional && doublers != 0 {
            self.counter_replacement_order(controller, object, counters.clone(), doublers)?
        } else {
            doublers
        };
        let multiplier = doubling_factor(doublers);
        counters.plus_one = counters.plus_one.times(&multiplier);
        counters.minus_one = counters.minus_one.times(&multiplier);
        if additional {
            counters.plus_one = counters
                .plus_one
                .plus(&doubling_factor(doublers - doublers_before_addition));
        }
        Ok(counters)
    }

    /// Put a counter on the exact permanent retained by an entry or attack trigger.
    pub(super) fn put_counter_on_related_permanent(
        &mut self,
        entry: &StackEntry,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|bindings| bindings.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let [member] = binding.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let object = match &member.related {
            crate::occurrence::TriggerRelated::ZoneChange { after, .. } => after.object.id(),
            crate::occurrence::TriggerRelated::TokenCreated { after, .. } => after.object.id(),
            crate::occurrence::TriggerRelated::SubtypeCombatDamage { creature, .. } => *creature,
            crate::occurrence::TriggerRelated::Combatant(object)
            | crate::occurrence::TriggerRelated::AttackedAlone(object) => *object,
            _ => return Err(EffectError::InconsistentProgram),
        };
        if !self
            .window
            .cards()
            .get(object)
            .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
        {
            return Ok(());
        }
        self.place_plus_one_counters(
            vec![object],
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            append,
        )
    }
    pub(super) fn place_ability_counters(
        &mut self,
        entry: &StackEntry,
        instruction: usize,
        operation: &Instruction,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let source = match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                Some(source.id())
            }
            StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                if matches!(
                    operation,
                    Instruction::PutCountersOnControlledCreatures
                        | Instruction::PutCountersOnControlledSubtype(_)
                        | Instruction::PutCountersOnControlledCreaturesAndGrant(_)
                ) =>
            {
                None
            }
            _ => return Err(EffectError::InconsistentProgram),
        };
        let cause = EventCause::Instruction {
            entry: entry.id(),
            instruction,
        };
        if let Instruction::ClearSourceCounters(kind) = operation {
            let source = source.ok_or(EffectError::InconsistentProgram)?;
            let before = self.rules_snapshot(None)?;
            let Some(removed) = self
                .characteristics
                .named_permanent_counters_mut(*kind)
                .ok_or(EffectError::InconsistentProgram)?
                .remove(&source)
            else {
                return Ok(());
            };
            return self.emit_from(cause, &before, None, append, |_, _, _| {
                Ok(vec![RulesOccurrence::PermanentCounterChanged {
                    object: source,
                    kind: *kind,
                    before: removed,
                    after: 0.into(),
                }])
            });
        }
        if let Instruction::PutNamedCounterOnSource { kind, amount } = operation {
            let source = source.ok_or(EffectError::InconsistentProgram)?;
            let before = self.rules_snapshot(None)?;
            if !before
                .view()
                .evaluated
                .get(source)
                .is_some_and(|p| p.zone == ObjectZone::Battlefield)
            {
                return Ok(());
            }
            let old = self.counters_of_kind(source, *kind);
            let after = old.plus(
                &effect_counter_multiplier(&before.view().evaluated, source)
                    .times(&i64::from(*amount).into()),
            );
            self.characteristics
                .named_permanent_counters_mut(*kind)
                .ok_or(EffectError::InconsistentProgram)?
                .insert(source, after.clone());
            if kind.keyword_counter_ability().is_some()
                && let EventCause::Instruction { entry, .. } = cause
            {
                self.characteristics
                    .refresh_keyword_counter_grant(entry, source, *kind)?;
            }
            return self.emit_from(cause, &before, None, append, |_, _, _| {
                Ok(vec![RulesOccurrence::PermanentCounterChanged {
                    object: source,
                    kind: *kind,
                    before: old,
                    after,
                }])
            });
        }
        if *operation == Instruction::PutBlessingCounterOnSource {
            let source = source.ok_or(EffectError::InconsistentProgram)?;
            if !self
                .window
                .cards()
                .get(source)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            {
                return Ok(());
            }
            let before = self.rules_snapshot(None)?;
            let kind = CounterKind::Blessing;
            let previous = self.characteristics.permanent_counter_count(source, kind);
            let next = previous.plus(&effect_counter_multiplier(&before.view().evaluated, source));
            self.characteristics
                .blessing_counters
                .insert(source, next.clone());
            return self.emit_from(cause, &before, None, append, |_, _, _| {
                Ok(vec![RulesOccurrence::PermanentCounterChanged {
                    object: source,
                    kind,
                    before: previous,
                    after: next,
                }])
            });
        }
        if *operation == Instruction::PutCreatureAndOtherPlaneswalkerCounters {
            let source = source.ok_or(EffectError::InconsistentProgram)?;
            let evaluated = self.evaluated_characteristics()?;
            let controlled = evaluated
                .objects
                .values()
                .filter(|object| {
                    object.zone == ObjectZone::Battlefield
                        && object.controller == Some(entry.controller())
                })
                .collect::<Vec<_>>();
            let creatures = controlled
                .iter()
                .filter(|object| object.is_creature())
                .map(|object| object.object)
                .collect::<Vec<_>>();
            let planeswalkers = controlled
                .iter()
                .filter(|object| object.object != source && object.has_type(CardType::Planeswalker))
                .map(|object| object.object)
                .collect::<Vec<_>>();
            return self.place_creature_and_loyalty_counters(
                creatures,
                planeswalkers,
                cause,
                append,
            );
        }
        if matches!(
            operation,
            Instruction::PutQuestCounterOnSource | Instruction::PutStorageCounterOnSource
        ) {
            let source = source.ok_or(EffectError::InconsistentProgram)?;
            if !self
                .window
                .cards()
                .get(source)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            {
                return Ok(());
            }
            let before = self.rules_snapshot(None)?;
            let quest = *operation == Instruction::PutQuestCounterOnSource;
            let counters = if quest {
                &mut self.characteristics.quest_counters
            } else {
                &mut self.characteristics.storage
            };
            let previous = counters.get(&source).cloned().unwrap_or_else(|| 0.into());
            let next = previous.plus(&effect_counter_multiplier(&before.view().evaluated, source));
            counters.insert(source, next.clone());
            self.emit_from(cause, &before, None, append, |_, _, _| {
                Ok(vec![if quest {
                    RulesOccurrence::PermanentCounterChanged {
                        object: source,
                        kind: CounterKind::Quest,
                        before: previous,
                        after: next,
                    }
                } else {
                    RulesOccurrence::StorageCountersChanged {
                        object: source,
                        before: previous,
                        after: next,
                    }
                }])
            })?;
            return Ok(());
        }
        if matches!(
            operation,
            Instruction::PutChargeCounterOnSource | Instruction::PutChosenXChargeCounters
        ) {
            let source = source.ok_or(EffectError::InconsistentProgram)?;
            if !self
                .window
                .cards()
                .get(source)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            {
                return Ok(());
            }
            let before = self.rules_snapshot(None)?;
            let previous = self
                .characteristics
                .charge
                .get(&source)
                .cloned()
                .unwrap_or_else(|| 0.into());
            let amount = if *operation == Instruction::PutChosenXChargeCounters {
                entry
                    .chosen_x()
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram)?
            } else {
                1.into()
            };
            if !amount.is_positive() {
                return Ok(());
            }
            let added = amount.times(&effect_counter_multiplier(&before.view().evaluated, source));
            let next = previous.plus(&added);
            self.characteristics.charge.insert(source, next.clone());
            return self.emit_from(cause, &before, None, append, |_, _, _| {
                Ok(vec![RulesOccurrence::ChargeCountersChanged {
                    object: source,
                    before: previous,
                    after: next,
                }])
            });
        }
        let selected: Vec<_> = self
            .evaluated_characteristics()?
            .objects
            .values()
            .filter(|o| {
                (matches!(
                    operation,
                    Instruction::PutCountersOnControlledCreatures
                        | Instruction::PutCountersOnControlledSubtype(_)
                        | Instruction::PutCountersOnControlledCreaturesAndGrant(_)
                ) || Some(o.object) != source)
                    && o.zone == ObjectZone::Battlefield
                    && o.controller == Some(entry.controller())
                    && o.is_creature()
                    && match operation {
                        Instruction::PutCountersOnControlledSubtype(kind) => {
                            o.current_creature_subtypes.contains(kind)
                        }
                        _ => true,
                    }
            })
            .map(|o| o.object)
            .collect();
        if let Instruction::PutCountersOnControlledCreaturesAndGrant(keywords) = operation {
            let affected = selected.iter().copied().collect();
            self.place_plus_one_counters(selected, cause, append)?;
            self.modify_objects(entry, instruction, affected, None, keywords.clone(), append)
        } else {
            self.place_plus_one_counters(selected, cause, append)
        }
    }

    pub(super) fn place_creature_and_loyalty_counters(
        &mut self,
        creatures: Vec<ObjectId>,
        planeswalkers: Vec<ObjectId>,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if creatures.is_empty() && planeswalkers.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let added = PowerToughnessCounters {
            plus_one: 1.into(),
            minus_one: 0.into(),
        };
        let creatures = creatures
            .into_iter()
            .map(|object| {
                self.adjusted_pt_counters(&before.view().evaluated, object, added.clone(), None)
                    .map(|counters| (object, counters))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (object, added) in &creatures {
            self.characteristics.place_counters(*object, added)?;
        }
        let mut loyalty = Vec::new();
        for object in planeswalkers {
            let old = self
                .characteristics
                .loyalty
                .get(&object)
                .cloned()
                .unwrap_or_else(|| 0.into());
            let added = effect_counter_multiplier(&before.view().evaluated, object);
            let new = old.plus(&added);
            self.characteristics.loyalty.insert(object, new.clone());
            loyalty.push((object, old, new));
        }
        self.emit_from(cause, &before, None, append, |old, new, _| {
            let mut occurrences = creatures
                .into_iter()
                .map(|(object, added)| {
                    Ok(RulesOccurrence::CountersPlaced {
                        object,
                        before: old.characteristics.counters(object),
                        after: new.characteristics.counters(object),
                        added,
                        before_appearance: Box::new(
                            old.appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            new.appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    })
                })
                .collect::<Result<Vec<_>, EffectError>>()?;
            occurrences.extend(loyalty.into_iter().map(|(object, before, after)| {
                RulesOccurrence::LoyaltyCountersChanged {
                    object,
                    before,
                    after,
                }
            }));
            Ok(occurrences)
        })
    }

    pub(super) fn place_plus_one_counters(
        &mut self,
        selected: Vec<ObjectId>,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.place_plus_one_counter_amount(selected, 1.into(), cause, append)
    }

    pub(super) fn place_plus_one_counter_amount(
        &mut self,
        selected: Vec<ObjectId>,
        amount: RulesInteger,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.place_divided_plus_one_counters(
            selected
                .into_iter()
                .map(|object| (object, amount.clone()))
                .collect(),
            cause,
            append,
        )
    }

    pub(super) fn place_divided_plus_one_counters(
        &mut self,
        selected: Vec<(ObjectId, RulesInteger)>,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if selected.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let selected = selected
            .into_iter()
            .map(|(object, amount)| {
                let added = PowerToughnessCounters {
                    plus_one: amount,
                    minus_one: 0.into(),
                };
                self.adjusted_pt_counters(&before.view().evaluated, object, added, None)
                    .map(|counters| (object, counters))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (object, added) in &selected {
            self.characteristics.place_counters(*object, added)?;
        }
        self.emit_from(cause, &before, None, append, |old, new, _| {
            selected
                .into_iter()
                .map(|(object, added)| {
                    Ok(RulesOccurrence::CountersPlaced {
                        object,
                        before: old.characteristics.counters(object),
                        after: new.characteristics.counters(object),
                        added,
                        before_appearance: Box::new(
                            old.appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            new.appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    })
                })
                .collect()
        })
    }
}
