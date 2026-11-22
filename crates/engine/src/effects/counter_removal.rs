//! Allocate removals against one current battlefield incarnation, then publish one rules cut.

use super::*;
use crate::occurrence::{EventCause, RulesOccurrence};

/// Frozen counter-removal quantities for one resolving instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterRemovalChoice {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    object: ObjectId,
    maximum: u8,
    domain: BTreeMap<CounterKind, u8>,
}

impl CounterRemovalChoice {
    /// Player who makes the allocation.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }

    /// Targeted battlefield incarnation from which the counters will be removed.
    pub fn object(&self) -> ObjectId {
        self.object
    }

    /// Maximum number of counters that may be removed in total.
    pub fn maximum(&self) -> u8 {
        self.maximum
    }

    /// Available quantities by kind, bounded by the instruction's maximum.
    pub fn domain(&self) -> &BTreeMap<CounterKind, u8> {
        &self.domain
    }
}

impl EffectWindow {
    pub(super) fn counter_removal_choice(&self) -> Result<CounterRemovalChoice, EffectError> {
        let Instruction::RemoveCreatureCounters { maximum } = self.current_instruction()? else {
            return Err(EffectError::WrongResolutionFrame);
        };
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let entry = self
            .window
            .resolving()
            .filter(|entry| entry.id() == cursor.entry)
            .ok_or(EffectError::WrongResolutionFrame)?;
        let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target else {
            return Err(EffectError::InconsistentProgram);
        };
        let mut domain = BTreeMap::new();
        for kind in CounterKind::PERMANENT {
            let available = self
                .counters_of_kind(object, kind)
                .min(i64::from(*maximum).into());
            if available.is_positive() {
                let count = available
                    .to_usize()
                    .and_then(|value| u8::try_from(value).ok())
                    .ok_or(EffectError::InconsistentProgram)?;
                domain.insert(kind, count);
            }
        }
        Ok(CounterRemovalChoice {
            revision: self.revision,
            entry: entry.id(),
            instruction: cursor.instruction,
            chooser: entry.controller(),
            object,
            maximum: *maximum,
            domain,
        })
    }

    pub(super) fn remove_counters(
        &mut self,
        frame: &CounterRemovalChoice,
        selected: &BTreeMap<CounterKind, u8>,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if selected
            .values()
            .map(|count| u32::from(*count))
            .sum::<u32>()
            > frame.maximum.into()
            || selected.iter().any(|(kind, count)| {
                *count == 0
                    || frame
                        .domain
                        .get(kind)
                        .is_none_or(|available| count > available)
            })
        {
            return Err(EffectError::InvalidSelectionCount);
        }
        if selected.is_empty() {
            return Ok(());
        }

        let object = frame.object;
        let before = self.rules_snapshot(None)?;
        let mut changes = Vec::new();
        let removed_pt = PowerToughnessCounters {
            plus_one: i64::from(
                selected
                    .get(&CounterKind::PlusOne)
                    .copied()
                    .unwrap_or_default(),
            )
            .into(),
            minus_one: i64::from(
                selected
                    .get(&CounterKind::MinusOne)
                    .copied()
                    .unwrap_or_default(),
            )
            .into(),
        };
        if removed_pt != PowerToughnessCounters::default() {
            let previous = self.characteristics.counters(object);
            let after = self
                .characteristics
                .take_power_toughness_counters(object, &removed_pt)?;
            changes.push(RulesOccurrence::PowerToughnessCountersRemoved {
                object,
                before: previous,
                after,
            });
        }
        for (&kind, &removed) in selected {
            if matches!(kind, CounterKind::PlusOne | CounterKind::MinusOne) {
                continue;
            }
            let counts = self
                .characteristics
                .named_permanent_counters_mut(kind)
                .ok_or(EffectError::InconsistentProgram)?;
            let previous = counts
                .get(&object)
                .cloned()
                .ok_or(EffectError::InconsistentProgram)?;
            let after = previous.minus(&i64::from(removed).into());
            if after.is_positive() {
                counts.insert(object, after.clone());
            } else {
                counts.remove(&object);
                if kind == CounterKind::Vow {
                    self.characteristics.vow_protected_players.remove(&object);
                }
                if kind == CounterKind::Indestructible {
                    self.characteristics
                        .clear_indestructible_counter_grant(object);
                }
            }
            changes.push(match kind {
                CounterKind::Time => RulesOccurrence::TimeCounterRemoved {
                    object,
                    before: previous,
                    after,
                },
                CounterKind::Fade => RulesOccurrence::FadeCounterRemoved {
                    object,
                    before: previous,
                    after,
                },
                CounterKind::Loyalty => RulesOccurrence::LoyaltyCountersChanged {
                    object,
                    before: previous,
                    after,
                },
                CounterKind::Charge => RulesOccurrence::ChargeCountersChanged {
                    object,
                    before: previous,
                    after,
                },
                CounterKind::Storage => RulesOccurrence::StorageCountersChanged {
                    object,
                    before: previous,
                    after,
                },
                CounterKind::Suspect => RulesOccurrence::SuspectCountersChanged {
                    object,
                    before: previous,
                    after,
                },
                _ => RulesOccurrence::PermanentCounterChanged {
                    object,
                    kind,
                    before: previous,
                    after,
                },
            });
        }
        self.emit_from(
            EventCause::Instruction {
                entry: frame.entry,
                instruction: frame.instruction,
            },
            &before,
            None,
            journal,
            |_, _, _| Ok(changes),
        )
    }
}
