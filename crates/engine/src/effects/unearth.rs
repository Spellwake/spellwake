//! Unearth retains the returned incarnation for its one delayed end-step trigger.
use super::*;
use crate::{
    characteristics::{AbilityChange, ResolvedDuration, ResolvedPart, ResolvedParts},
    object_identity::ObjectMovement,
    occurrence::{EventCause, RulesOccurrence, ZoneReason},
};

impl EffectWindow {
    pub(super) fn resolve_unearth(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        index: usize,
        operation: &Instruction,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let reason = match operation {
            Instruction::MutateObject(spec) => match &spec.op {
                crate::toolbox::ObjectMutation::Unearth => ZoneReason::UnearthReturn,
                crate::toolbox::ObjectMutation::KeywordZoneAction(
                    crate::toolbox::KeywordZoneOp::ExileUnearthed(_),
                ) => ZoneReason::UnearthExile,
                _ => return Err(EffectError::InconsistentProgram),
            },
            _ => return Err(EffectError::InconsistentProgram),
        };
        let is_unearth = reason == ZoneReason::UnearthReturn;
        let cause = EventCause::Instruction {
            entry: entry.id(),
            instruction: index,
        };
        let (object, destination) = match operation {
            Instruction::MutateObject(spec) => match &spec.op {
                crate::toolbox::ObjectMutation::Unearth => {
                    let StackKind::ActivatedAbility { source } = entry.kind() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    if self
                        .window
                        .cards()
                        .get(source.id())
                        .is_none_or(|c| c.zone() != ObjectZone::Graveyard(entry.controller()))
                    {
                        return Ok(());
                    }
                    (
                        source.id(),
                        ObjectDestination::Battlefield {
                            controller: entry.controller(),
                        },
                    )
                }
                crate::toolbox::ObjectMutation::KeywordZoneAction(
                    crate::toolbox::KeywordZoneOp::ExileUnearthed(object),
                ) => {
                    if self
                        .window
                        .cards()
                        .get(*object)
                        .is_none_or(|c| c.zone() != ObjectZone::Battlefield)
                    {
                        return Ok(());
                    }
                    (*object, ObjectDestination::Exile)
                }
                _ => return Err(EffectError::InconsistentProgram),
            },
            _ => return Err(EffectError::InconsistentProgram),
        };
        let before = self.rules_snapshot(None)?;
        let source = before
            .view()
            .appearance(object)
            .ok_or(EffectError::UnknownObject)?;
        let card = source
            .object
            .card_id()
            .ok_or(EffectError::InconsistentProgram)?;
        self.window =
            self.window
                .record_resolution_move(self.window.revision(), object, destination)?;
        let after = self
            .window
            .cards()
            .current_identity(card)
            .ok_or(EffectError::InconsistentProgram)?;
        if is_unearth {
            self.stage_resolution_entry_choice(object, after)?;
            self.characteristics.register_battlefield_entry(after)?;
            self.apply_entry_counters(&before, after, PowerToughnessCounters::default(), 0)?;
            self.window.mark_unearthed(after)?;
        }
        let movement = ObjectMovement {
            before: source.object.clone(),
            after: self
                .window
                .cards()
                .get(after)
                .ok_or(EffectError::InconsistentProgram)?
                .clone(),
        };
        self.emit_from(
            cause.clone(),
            &before,
            None,
            append,
            |old, new, controllers| {
                Ok(vec![zone_occurrence_at(
                    old,
                    new,
                    &movement,
                    reason,
                    controllers,
                )?])
            },
        )?;
        if is_unearth {
            let before_haste = self.rules_snapshot(None)?;
            let parts = ResolvedParts(vec![ResolvedPart::Abilities(AbilityChange {
                granted_keywords: BTreeSet::from([KeywordAbility::Haste]),
                ..Default::default()
            })]);
            let effect = self.characteristics.add_effect(
                entry.id(),
                after,
                parts,
                ResolvedDuration::NoStatedDuration,
            )?;
            self.emit_from(cause, &before_haste, None, append, |old, new, _| {
                Ok(vec![RulesOccurrence::ContinuousEffectCreated {
                    appearances: characteristic_appearance_changes(old, new, [after])?,
                    effect,
                }])
            })?;
            self.triggers
                .delay_unearth(source, after, entry.controller());
        }
        Ok(())
    }
}
