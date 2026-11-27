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
        if counters.plus_one.is_positive() {
            counters.plus_one = if self.permanent_counter_prevented(
                battlefield_before,
                object,
                CounterKind::PlusOne,
            ) {
                0.into()
            } else {
                self.apply_static_counter_modifiers(
                    battlefield_before,
                    object,
                    CounterKind::PlusOne,
                    counters.plus_one,
                )
            };
        }
        if counters.minus_one.is_positive() {
            counters.minus_one = if self.permanent_counter_prevented(
                battlefield_before,
                object,
                CounterKind::MinusOne,
            ) {
                0.into()
            } else {
                self.apply_static_counter_modifiers(
                    battlefield_before,
                    object,
                    CounterKind::MinusOne,
                    counters.minus_one,
                )
            };
        }
        Ok(counters)
    }

    pub(super) fn apply_static_counter_modifiers(
        &self,
        battlefield_before: &crate::characteristics::EvaluatedCharacteristics,
        object: ObjectId,
        kind: CounterKind,
        mut amount: RulesInteger,
    ) -> RulesInteger {
        if !amount.is_positive()
            || self.permanent_counter_prevented(battlefield_before, object, kind)
        {
            return 0.into();
        }
        let mut total_add = 0i64;
        let mut factors: Vec<u8> = Vec::new();
        let mut halve_count: usize = 0;
        let target_ctrl = battlefield_before.get(object).and_then(|p| p.controller);
        for source in battlefield_before
            .objects
            .values()
            .filter(|perm| perm.zone == ObjectZone::Battlefield)
        {
            let Some(source_ctrl) = source.controller else {
                continue;
            };
            for ability in source
                .printed_text
                .continuous
                .iter()
                .chain(&source.printed_text.additional_battlefield_abilities)
            {
                let active = source.abilities.continuous == Some(ability.id())
                    || source
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ability.id());
                if !active {
                    continue;
                }
                if let crate::characteristics::StaticContinuousAbility::Modifier {
                    rule: crate::characteristics::StaticModifierRule::HalveOpponentCounters,
                    ..
                } = ability
                    && target_ctrl.is_some_and(|c| c != source_ctrl)
                {
                    halve_count += 1;
                }
                if let crate::characteristics::StaticContinuousAbility::Modifier {
                    rule:
                        crate::characteristics::StaticModifierRule::ModifyCounters {
                            selector,
                            counter_kind,
                            add,
                            multiply,
                        },
                    ..
                } = ability
                    && counter_kind.is_none_or(|k| k == kind)
                    && crate::characteristics::selector_matches_permanent(
                        self.window.cards(),
                        battlefield_before,
                        source.object,
                        source_ctrl,
                        selector,
                        object,
                        &self.name_rules,
                    )
                {
                    total_add = total_add.saturating_add(i64::from(*add));
                    if *multiply > 1 {
                        factors.push(*multiply);
                    }
                }
            }
        }
        if total_add > 0 {
            amount = amount.plus(&RulesInteger::from(total_add));
        }
        for factor in factors {
            amount = amount.times(&RulesInteger::from(i64::from(factor)));
        }
        for _ in 0..halve_count {
            amount = amount.nonnegative_half_rounded_down();
        }
        amount
    }

    /// Place counters moved from another permanent onto `destination`, applying replacement modifiers
    /// and emitting counter-placed occurrences under the originating instruction cause.
    pub(super) fn place_moved_counters(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        destination: ObjectId,
        moved: &BTreeMap<CounterKind, RulesInteger>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if moved.is_empty()
            || !self
                .window
                .cards()
                .get(destination)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
        {
            return Ok(());
        }
        let cause = EventCause::Instruction { entry, instruction };
        let before = self.rules_snapshot(None)?;
        let mut pt_raw = PowerToughnessCounters::default();
        let mut named: Vec<(CounterKind, RulesInteger)> = Vec::new();
        for (&kind, amount) in moved {
            if !amount.is_positive()
                || self.permanent_counter_prevented(&before.view().evaluated, destination, kind)
            {
                continue;
            }
            match kind {
                CounterKind::PlusOne => pt_raw.plus_one = pt_raw.plus_one.plus(amount),
                CounterKind::MinusOne => pt_raw.minus_one = pt_raw.minus_one.plus(amount),
                other => named.push((other, amount.clone())),
            }
        }
        let mut pt_record = None;
        if pt_raw.valid_placement() {
            let added =
                self.adjusted_pt_counters(&before.view().evaluated, destination, pt_raw, None)?;
            if added.valid_placement() {
                let previous = self.characteristics.counters(destination);
                self.characteristics.place_counters(destination, &added)?;
                let after = self.characteristics.counters(destination);
                pt_record = Some((previous, added, after));
            }
        }
        let mut named_occurrences = Vec::with_capacity(named.len());
        for (kind, amount) in named {
            let old = self.counters_of_kind(destination, kind);
            let multiplied =
                effect_counter_multiplier(&before.view().evaluated, destination).times(&amount);
            let added = self.apply_static_counter_modifiers(
                &before.view().evaluated,
                destination,
                kind,
                multiplied,
            );
            if !added.is_positive() {
                continue;
            }
            let after = old.plus(&added);
            self.characteristics
                .named_permanent_counters_mut(kind)
                .ok_or(EffectError::InconsistentProgram)?
                .insert(destination, after.clone());
            if kind.keyword_counter_ability().is_some() {
                self.characteristics
                    .refresh_keyword_counter_grant(entry, destination, kind)?;
            }
            named_occurrences.push(match kind {
                CounterKind::Loyalty => RulesOccurrence::LoyaltyCountersChanged {
                    object: destination,
                    before: old,
                    after,
                },
                CounterKind::Charge => RulesOccurrence::ChargeCountersChanged {
                    object: destination,
                    before: old,
                    after,
                },
                CounterKind::Storage => RulesOccurrence::StorageCountersChanged {
                    object: destination,
                    before: old,
                    after,
                },
                CounterKind::Suspect => RulesOccurrence::SuspectCountersChanged {
                    object: destination,
                    before: old,
                    after,
                },
                _ => RulesOccurrence::PermanentCounterChanged {
                    object: destination,
                    kind,
                    before: old,
                    after,
                },
            });
        }
        if pt_record.is_none() && named_occurrences.is_empty() {
            return Ok(());
        }
        self.emit_from(cause, &before, None, append, |old, new, _| {
            let mut occurrences =
                Vec::with_capacity(named_occurrences.len() + usize::from(pt_record.is_some()));
            if let Some((prev, added, after)) = pt_record.clone() {
                occurrences.push(RulesOccurrence::CountersPlaced {
                    object: destination,
                    before_appearance: Box::new(
                        old.appearance(destination)
                            .ok_or(EffectError::InconsistentProgram)?,
                    ),
                    after_appearance: Box::new(
                        new.appearance(destination)
                            .ok_or(EffectError::InconsistentProgram)?,
                    ),
                    before: prev,
                    added,
                    after,
                });
            }
            occurrences.extend(named_occurrences.clone());
            Ok(occurrences)
        })
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
        self.place_divided_counters(CounterKind::PlusOne, selected, cause, append)
    }

    pub(super) fn place_divided_counters(
        &mut self,
        kind: CounterKind,
        selected: Vec<(ObjectId, RulesInteger)>,
        cause: EventCause,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let selected: Vec<(ObjectId, RulesInteger)> = selected
            .into_iter()
            .filter(|(_, amount)| amount.is_positive())
            .collect();
        if selected.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        if matches!(kind, CounterKind::PlusOne | CounterKind::MinusOne) {
            let selected = selected
                .into_iter()
                .map(|(object, amount)| {
                    let added = if kind == CounterKind::PlusOne {
                        PowerToughnessCounters {
                            plus_one: amount,
                            minus_one: 0.into(),
                        }
                    } else {
                        PowerToughnessCounters {
                            plus_one: 0.into(),
                            minus_one: amount,
                        }
                    };
                    self.adjusted_pt_counters(&before.view().evaluated, object, added, None)
                        .map(|counters| (object, counters))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let selected: Vec<_> = selected
                .into_iter()
                .filter(|(_, added)| added.valid_placement())
                .collect();
            for (object, added) in &selected {
                self.characteristics.place_counters(*object, added)?;
            }
            return self.emit_from(cause, &before, None, append, |old, new, _| {
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
            });
        }
        let mut occurrences = Vec::with_capacity(selected.len());
        for (object, amount) in selected {
            let old = self.counters_of_kind(object, kind);
            let multiplied =
                effect_counter_multiplier(&before.view().evaluated, object).times(&amount);
            let added = self.apply_static_counter_modifiers(
                &before.view().evaluated,
                object,
                kind,
                multiplied,
            );
            if !added.is_positive() {
                continue;
            }
            let after = old.plus(&added);
            self.characteristics
                .named_permanent_counters_mut(kind)
                .ok_or(EffectError::InconsistentProgram)?
                .insert(object, after.clone());
            if kind.keyword_counter_ability().is_some()
                && let EventCause::Instruction { entry: eid, .. } = cause
            {
                self.characteristics
                    .refresh_keyword_counter_grant(eid, object, kind)?;
            }
            occurrences.push(if kind == CounterKind::Loyalty {
                RulesOccurrence::LoyaltyCountersChanged {
                    object,
                    before: old,
                    after,
                }
            } else {
                RulesOccurrence::PermanentCounterChanged {
                    object,
                    kind,
                    before: old,
                    after,
                }
            });
        }
        self.emit_from(cause, &before, None, append, |_, _, _| Ok(occurrences))
    }
}
