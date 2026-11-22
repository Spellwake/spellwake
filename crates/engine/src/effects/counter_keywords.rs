//! Counters on exact object incarnations and their related battlefield triggers.
use super::*;
use crate::{
    object_identity::ObjectMovement,
    occurrence::{EventCause, RulesOccurrence, TriggerRelated, ZoneReason},
    program::{CounterKeyword, TriggerEvent, TriggerKeywordEvent},
    stack_window::StackEntry,
};

impl EffectWindow {
    /// Positive time counters in exile or on the battlefield (CR 122).
    pub fn time_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.time_counters
    }
    /// Positive fade counters on the battlefield.
    pub fn fade_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.fade
    }
    pub(super) fn set_time_counters(&mut self, object: ObjectId, amount: RulesInteger) {
        if amount.is_positive() {
            self.characteristics.time_counters.insert(object, amount);
        } else {
            self.characteristics.time_counters.remove(&object);
        }
    }
    pub(super) fn apply_fading_vanishing_entry(
        &mut self,
        object: ObjectId,
        multiplier: &RulesInteger,
    ) -> Result<(), EffectError> {
        // The hypothetical battlefield characteristics already include global ability
        // removal. Current admission supports removal of all or all nonmana abilities.
        if !self.copiable_program(object).is_some_and(|p| {
            p.triggered.iter().any(|a| {
                matches!(
                    a.condition.event,
                    TriggerEvent::Keyword(TriggerKeywordEvent::Counter(
                        CounterKeyword::Fading(_) | CounterKeyword::Vanishing(Some(_))
                    ))
                )
            })
        }) {
            return Ok(());
        }
        let evaluated = self.evaluated_characteristics()?;
        let sheet = evaluated.get(object).ok_or(EffectError::UnknownObject)?;
        let (mut fade, mut time) = (RulesInteger::from(0), RulesInteger::from(0));
        for ability in &sheet.printed_text.triggered {
            if !sheet.abilities.triggered.contains(&ability.id) {
                continue;
            }
            match ability.condition.event {
                TriggerEvent::Keyword(TriggerKeywordEvent::Counter(CounterKeyword::Fading(n))) => {
                    fade = fade.plus(&i64::from(n).into())
                }
                TriggerEvent::Keyword(TriggerKeywordEvent::Counter(CounterKeyword::Vanishing(
                    Some(n),
                ))) => time = time.plus(&i64::from(n).into()),
                _ => {}
            }
        }
        if fade.is_positive() {
            self.characteristics
                .fade
                .insert(object, fade.times(multiplier));
        }
        if time.is_positive() {
            self.set_time_counters(object, time.times(multiplier));
        }
        Ok(())
    }
    /// Publish an actual time-counter removal to both battlefield and exile triggers.
    pub(super) fn remove_time_counter(
        &mut self,
        object: ObjectId,
        entry: StackEntryId,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        let Some(before) = self.characteristics.time_counters.get(&object).cloned() else {
            return Ok(false);
        };
        let view = self.rules_snapshot(None)?;
        let after = before.minus(&1.into());
        self.set_time_counters(object, after.clone());
        self.emit_from(
            EventCause::Instruction { entry, instruction },
            &view,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::TimeCounterRemoved {
                    object,
                    before,
                    after,
                }])
            },
        )?;
        Ok(true)
    }
    pub(super) fn requires_source_sacrifice(
        &self,
        instruction: &Instruction,
    ) -> Result<bool, EffectError> {
        match instruction {
            Instruction::SacrificeDelayedObjects(_)
            | Instruction::EndStepCast {
                kind: crate::program::EndStepCast::Blitz,
                ..
            }
            | Instruction::SacrificeEvoked
            | Instruction::SacrificeStateTriggerSource
            | Instruction::SacrificeUpkeepSource(_)
            | Instruction::ResolveCounterKeyword(CounterKeyword::VanishingLastCounter) => Ok(true),
            Instruction::ResolveCounterKeyword(CounterKeyword::Fading(_)) => {
                let Some(StackKind::TriggeredAbility { source }) =
                    self.window.resolving().map(StackEntry::kind)
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                Ok(!self.characteristics.fade.contains_key(&source.id()))
            }
            _ => Ok(false),
        }
    }
    pub(super) fn resolve_counter_keyword(
        &mut self,
        keyword: CounterKeyword,
        entry: &StackEntry,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let StackKind::TriggeredAbility { source } = entry.kind() else {
            return Err(EffectError::InconsistentProgram);
        };
        let object = source.id();
        match keyword {
            CounterKeyword::SuspendLastCounter => Err(EffectError::InconsistentProgram), // Owned by the cast choice.
            CounterKeyword::SuspendUpkeep
                if !self
                    .window
                    .cards()
                    .get(object)
                    .is_some_and(|o| o.zone() == ObjectZone::Exile) =>
            {
                Ok(())
            }
            CounterKeyword::VanishingLastCounter => Err(EffectError::InconsistentProgram), // Owned by forced sacrifice selection.
            CounterKeyword::SuspendUpkeep | CounterKeyword::Vanishing(_) => {
                self.remove_time_counter(object, entry.id(), index, append)?;
                Ok(())
            }
            CounterKeyword::Fading(_) => {
                let before = self
                    .characteristics
                    .fade
                    .get(&object)
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram)?;
                let view = self.rules_snapshot(None)?;
                let after = before.minus(&1.into());
                if after.is_positive() {
                    self.characteristics.fade.insert(object, after.clone());
                } else {
                    self.characteristics.fade.remove(&object);
                }
                self.emit_from(
                    EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &view,
                    None,
                    append,
                    |_, _, _| {
                        Ok(vec![RulesOccurrence::FadeCounterRemoved {
                            object,
                            before,
                            after,
                        }])
                    },
                )
            }
            CounterKeyword::Persist | CounterKeyword::Undying => {
                self.return_after_death(keyword, entry, index, append)
            }
        }
    }
    fn return_after_death(
        &mut self,
        keyword: CounterKeyword,
        entry: &StackEntry,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let [cause] = binding.causes.members() else {
            return Err(EffectError::InconsistentProgram);
        };
        let TriggerRelated::ZoneChange { before, after } = &cause.related else {
            return Err(EffectError::InconsistentProgram);
        };
        if !keyword.eligible_at_death(before) {
            return Ok(());
        }
        let grave = after.object.id();
        let Some(original) = self.window.cards().get(grave).cloned() else {
            return Ok(());
        };
        let owner = original.owner();
        let Some(card) = original.card_id() else {
            return Ok(());
        };
        if original.zone() != ObjectZone::Graveyard(owner) {
            return Ok(());
        }
        let snapshot = self.rules_snapshot(None)?;
        self.window = self.window.record_resolution_move(
            self.window.revision(),
            grave,
            ObjectDestination::Battlefield { controller: owner },
        )?;
        let object = self
            .window
            .cards()
            .current_identity(card)
            .ok_or(EffectError::InconsistentProgram)?;
        self.stage_resolution_entry_choice(grave, object)?;
        self.characteristics.register_battlefield_entry(object)?;
        let (reason, extra) = if keyword == CounterKeyword::Persist {
            (
                ZoneReason::PersistReturn,
                PowerToughnessCounters {
                    plus_one: 0.into(),
                    minus_one: 1.into(),
                },
            )
        } else {
            (
                ZoneReason::UndyingReturn,
                PowerToughnessCounters {
                    plus_one: 1.into(),
                    minus_one: 0.into(),
                },
            )
        };
        let added = self.apply_entry_counters(&snapshot, object, extra, 0)?;
        let movement = ObjectMovement {
            before: original,
            after: self
                .window
                .cards()
                .get(object)
                .ok_or(EffectError::UnknownObject)?
                .clone(),
        };
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            },
            &snapshot,
            None,
            append,
            |old, new, controllers| {
                Ok(vec![
                    zone_occurrence_at(old, new, &movement, reason, controllers)?,
                    RulesOccurrence::CountersPlaced {
                        object,
                        before: PowerToughnessCounters::default(),
                        added,
                        after: new.characteristics.counters(object),
                        before_appearance: Box::new(
                            old.appearance(grave).ok_or(EffectError::UnknownObject)?,
                        ),
                        after_appearance: Box::new(
                            new.appearance(object).ok_or(EffectError::UnknownObject)?,
                        ),
                    },
                ])
            },
        )
    }
}
