use super::super::progress_rules::{RuleStepContext, StepOutcome};
use super::super::*;
use crate::occurrence::{EventCause, TriggerRelated};
use crate::{
    stack_window::StackEntry,
    toolbox::{StackOpKind, StackOpSpec, StackTargetSelector},
};
use std::collections::BTreeSet;

impl EffectWindow {
    pub(super) fn execute_non_targeted_stack_op(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        spec: &StackOpSpec,
    ) -> Result<StepOutcome, EffectError> {
        match (&spec.target, &spec.op) {
            (
                StackTargetSelector::Triggering,
                StackOpKind::Counter {
                    ward_source: true, ..
                },
            ) => {
                self.counter_ward_source(entry.id(), index, ctx.append)?;
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            (StackTargetSelector::Triggering, StackOpKind::Counter { .. }) => {
                self.counter_triggering_stack_object(entry.id(), index, ctx.append)?;
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            (
                StackTargetSelector::Matching {
                    filter,
                    include_spells,
                    include_abilities,
                    opponents_only,
                },
                StackOpKind::Counter { .. },
            ) => {
                let evaluated = self.evaluated_characteristics()?;
                let resolver = entry.controller();
                let candidates: Vec<StackEntryId> = self
                    .window
                    .entries()
                    .iter()
                    .rev()
                    .filter(|s| s.id() != entry.id())
                    .filter(|s| !*opponents_only || s.controller() != resolver)
                    .filter(|s| match s.kind() {
                        StackKind::CardSpell(obj_id) => {
                            if !*include_spells {
                                return false;
                            }
                            match filter.as_deref() {
                                None => true,
                                Some(sf) => evaluated.get(*obj_id).is_some_and(|view| {
                                    self.matches_object_filter(
                                        sf.object,
                                        view,
                                        resolver,
                                        None,
                                        &BTreeSet::new(),
                                        &evaluated,
                                    )
                                }),
                            }
                        }
                        StackKind::SpellCopy { .. } => *include_spells && filter.is_none(),
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => {
                            if !*include_abilities {
                                return false;
                            }
                            match filter.as_deref() {
                                None => true,
                                Some(sf) => evaluated.get(source.id()).is_some_and(|view| {
                                    self.matches_object_filter(
                                        sf.object,
                                        view,
                                        resolver,
                                        None,
                                        &BTreeSet::new(),
                                        &evaluated,
                                    )
                                }),
                            }
                        }
                        StackKind::Speed
                        | StackKind::Monarch
                        | StackKind::DelayedCopy { .. }
                        | StackKind::Emblem { .. } => *include_abilities && filter.is_none(),
                    })
                    .map(|s| s.id())
                    .collect();
                let mut countered_count = 0usize;
                for target_id in candidates {
                    if self.counter_captured_stack_entry(
                        entry.id(),
                        index,
                        target_id,
                        ctx.append,
                    )? {
                        countered_count += 1;
                    }
                }
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = countered_count > 0;
                    cursor.results.last_action_count =
                        Some(RulesInteger::from_usize(countered_count));
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            (StackTargetSelector::CopyTarget(copy_target), StackOpKind::Copy { count, .. }) => {
                let copies = self
                    .resolve_value_expr(entry.controller(), count)?
                    .max(0.into())
                    .to_usize()
                    .unwrap_or(0)
                    .min(64);
                let target_entry_id = match copy_target {
                    crate::program::StackCopyTarget::ResolvingSpell => Some(entry.id()),
                    crate::program::StackCopyTarget::TargetSpell
                    | crate::program::StackCopyTarget::TargetAbility
                    | crate::program::StackCopyTarget::TargetSpellOrAbility => self
                        .instruction_target(entry)
                        .ok()
                        .and_then(|bound| match bound.choice().target {
                            TargetRef::Stack(id) => Some(id),
                            _ => None,
                        }),
                    crate::program::StackCopyTarget::TriggeringSpell
                    | crate::program::StackCopyTarget::TriggeringAbility => self
                        .execution_bindings(entry.id())
                        .and_then(|b| b.trigger.as_ref())
                        .and_then(|t| t.causes.members().first())
                        .and_then(|member| match &member.related {
                            TriggerRelated::SpellCast(cast) => Some(cast.entry()),
                            TriggerRelated::TargetingEntry(id)
                            | TriggerRelated::SpellCopied(id) => Some(*id),
                            _ => None,
                        })
                        .filter(|&id| id != entry.id()),
                };
                if copies > 0
                    && let Some(target_id) = target_entry_id
                    && let Some(target_entry) = self
                        .window
                        .entries()
                        .iter()
                        .chain(self.window.resolving())
                        .find(|e| e.id() == target_id)
                        .cloned()
                {
                    let is_spell = matches!(
                        target_entry.kind(),
                        StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                    );
                    let copy_prohibited = is_spell
                        && self
                            .window
                            .spell_object(&target_entry)
                            .and_then(|obj| self.copiable_program(obj))
                            .is_some_and(|program| program.cannot_be_copied);
                    if !copy_prohibited {
                        for _ in 0..copies {
                            let before = self.rules_snapshot(None)?;
                            let occ = if is_spell {
                                let Ok(copy_src) = self.capture_copy_source(target_id, 0.into())
                                else {
                                    break;
                                };
                                self.create_copy(&copy_src, entry.controller(), vec![])?
                            } else if let Some(payload) = self.payloads.get(&target_id).cloned() {
                                self.create_ability_copy(
                                    &target_entry,
                                    payload,
                                    entry.controller(),
                                    vec![],
                                )?
                            } else {
                                break;
                            };
                            self.emit_from(
                                EventCause::Instruction {
                                    entry: entry.id(),
                                    instruction: index,
                                },
                                &before,
                                None,
                                ctx.append,
                                |_, _, _| Ok(vec![occ]),
                            )?;
                        }
                    }
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            _ => {
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
        }
    }
}
