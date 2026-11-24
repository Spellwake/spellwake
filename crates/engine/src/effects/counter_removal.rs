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
    minimum: u8,
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

    /// Minimum number of counters that must be removed in total.
    pub fn minimum(&self) -> u8 {
        self.minimum
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
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let entry = self
            .window
            .resolving()
            .filter(|entry| entry.id() == cursor.entry)
            .ok_or(EffectError::WrongResolutionFrame)?;
        let (object, minimum, maximum) = match self.current_instruction()? {
            Instruction::RemoveCreatureCounters { maximum } => {
                let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                (object, 0, *maximum)
            }
            Instruction::RemoveCountersFromPermanent {
                target_self,
                kind: None,
                amount,
            } => {
                let object = if *target_self {
                    match entry.kind() {
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => source.id(),
                        _ => return Err(EffectError::InconsistentProgram),
                    }
                } else {
                    let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    object
                };
                let resolved = self
                    .resolve_value_expr(entry.controller(), amount)?
                    .max(0.into())
                    .min(i64::from(u8::MAX).into())
                    .to_usize()
                    .and_then(|v| u8::try_from(v).ok())
                    .unwrap_or(0);
                let mut total_avail: u32 = 0;
                for kind in self.characteristics.present_permanent_counter_kinds(object) {
                    let avail = self
                        .counters_of_kind(object, kind)
                        .min(i64::from(resolved).into())
                        .to_usize()
                        .unwrap_or(0);
                    total_avail = total_avail.saturating_add(u32::try_from(avail).unwrap_or(0));
                }
                let exact = u8::try_from(total_avail.min(u32::from(resolved))).unwrap_or(resolved);
                (object, exact, exact)
            }
            _ => return Err(EffectError::WrongResolutionFrame),
        };
        let mut domain = BTreeMap::new();
        for kind in self.characteristics.present_permanent_counter_kinds(object) {
            let available = self
                .counters_of_kind(object, kind)
                .min(i64::from(maximum).into());
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
            minimum,
            maximum,
            domain,
        })
    }

    pub(super) fn remove_counters(
        &mut self,
        frame: &CounterRemovalChoice,
        selected: &BTreeMap<CounterKind, u8>,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let total_selected: u32 = selected.values().map(|count| u32::from(*count)).sum();
        if total_selected < u32::from(frame.minimum)
            || total_selected > u32::from(frame.maximum)
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
                if kind.keyword_counter_ability().is_some() {
                    self.characteristics
                        .clear_keyword_counter_grant(object, kind);
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
