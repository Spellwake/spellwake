//! CR 702.43, 702.46 and 702.58 use current graveyard targets and the source's final battlefield counters.
use super::*;
use crate::{
    occurrence::{EventCause, RulesOccurrence, TriggerRelated, ZoneReason},
    program::TransferKeyword,
    stack_window::StackEntry,
};

impl EffectWindow {
    pub(super) fn exile_top_for_ingest(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        player: SeatId,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let Some(&top) = self.window.cards().library(player).first() else {
            return Ok(());
        };
        let before = self
            .window
            .cards()
            .get(top)
            .cloned()
            .ok_or(EffectError::UnknownObject)?;
        let card = before.card_id().ok_or(EffectError::InconsistentProgram)?;
        let snapshot = self.rules_snapshot(None)?;
        self.window = self.window.record_resolution_move(
            self.window.revision(),
            top,
            ObjectDestination::Exile,
        )?;
        let current = self
            .window
            .cards()
            .current_identity(card)
            .and_then(|id| self.window.cards().get(id))
            .ok_or(EffectError::UnknownObject)?;
        let movement = crate::object_identity::ObjectMovement {
            before,
            after: current.clone(),
        };
        self.emit_from(
            EventCause::Instruction { entry, instruction },
            &snapshot,
            None,
            append,
            |old, new, controllers| {
                Ok(vec![zone_occurrence_at(
                    old,
                    new,
                    &movement,
                    ZoneReason::Ingest { player },
                    controllers,
                )?])
            },
        )
    }

    pub(super) fn transfer_target(
        &self,
        keyword: TransferKeyword,
        target: TargetRef,
        world: &EvaluatedCharacteristics,
    ) -> bool {
        let TargetRef::Object(id) = target else {
            return false;
        };
        let Some(sheet) = world.get(id) else {
            return false;
        };
        match keyword {
            TransferKeyword::Modular(_) => {
                sheet.has_type(CardType::Artifact) && sheet.is_creature()
            }
            TransferKeyword::Soulshift(limit) => {
                self.creature_vocabulary
                    .resolve_label("Spirit")
                    .is_some_and(|spirit| sheet.current_creature_subtypes.contains(&spirit))
                    && self
                        .copiable_program(id)
                        .is_some_and(|card| card.mana_value() <= i64::from(limit).into())
            }
            TransferKeyword::Graft(_) => false,
        }
    }
    pub(super) fn graft_pair(&self, entry: StackEntryId) -> Option<(ObjectId, ObjectId)> {
        let binding = self.execution_bindings(entry)?.trigger.as_ref()?;
        let source = binding.source.object.id();
        let [cause] = binding.causes.members() else {
            return None;
        };
        let target = match &cause.related {
            TriggerRelated::ZoneChange { after, .. }
            | TriggerRelated::TokenCreated { after, .. } => after.object.id(),
            _ => return None,
        };
        let present = |id| {
            self.window
                .cards()
                .get(id)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
        };
        (source != target
            && present(source)
            && present(target)
            && self.characteristics.counters(source).plus_one.is_positive())
        .then_some((source, target))
    }
    pub(super) fn resolve_transfer(
        &mut self,
        key: TransferKeyword,
        entry: &StackEntry,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let cause = EventCause::Instruction {
            entry: entry.id(),
            instruction: index,
        };
        match key {
            TransferKeyword::Graft(_) => {
                let Some((source, target)) = self.graft_pair(entry.id()) else {
                    return Ok(());
                };
                let old = self.rules_snapshot(None)?;
                self.characteristics.take_plus_one_counter(source)?;
                let one = PowerToughnessCounters {
                    plus_one: 1.into(),
                    minus_one: 0.into(),
                };
                let one = self.adjusted_pt_counters(&old.view().evaluated, target, one, None)?;
                self.characteristics.place_counters(target, &one)?;
                self.emit_from(cause, &old, None, append, |before, after, _| {
                    Ok(vec![
                        RulesOccurrence::CounterRemovedForGraft {
                            object: source,
                            before: before.characteristics.counters(source),
                            after: after.characteristics.counters(source),
                        },
                        RulesOccurrence::CountersPlaced {
                            object: target,
                            before: before.characteristics.counters(target),
                            added: one,
                            after: after.characteristics.counters(target),
                            before_appearance: Box::new(
                                before
                                    .appearance(target)
                                    .ok_or(EffectError::UnknownObject)?,
                            ),
                            after_appearance: Box::new(
                                after.appearance(target).ok_or(EffectError::UnknownObject)?,
                            ),
                        },
                    ])
                })
            }
            TransferKeyword::Modular(_) => {
                let [bound] = entry.targets().targets() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let TargetRef::Object(target) = bound.choice().target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let last = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .and_then(|b| b.source.battlefield_counters.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?;
                if !last.plus_one.is_positive() {
                    return Ok(());
                }
                self.add_source_counters(
                    target,
                    entry.id(),
                    index,
                    last.plus_one.clone(),
                    super::resolution_choices::CounterPlacement::Ordinary,
                    append,
                )
            }
            TransferKeyword::Soulshift(_) => {
                let [bound] = entry.targets().targets() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let old = self.rules_snapshot(None)?;
                let (window, exit) = self.window.record_targeted_resolution_move_with_exit(
                    self.window.revision(),
                    bound.choice().id,
                    ObjectDestination::Hand,
                )?;
                self.window = window;
                self.emit_from(cause, &old, None, append, |before, after, controllers| {
                    Ok(vec![exit_occurrence_at(
                        before,
                        after,
                        exit,
                        ZoneReason::ReturnToOwnersHand,
                        controllers,
                    )?])
                })
            }
        }
    }
}
