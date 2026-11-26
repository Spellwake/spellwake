//! Execution engine for the Universal Rules Toolbox primitives.

mod eval;
mod prompt;
mod zone_op;

use super::progress_rules::{RuleStepContext, StepOutcome};
use super::*;
use crate::{
    characteristics::StaticContinuousAbility,
    emblem::EmblemDefinition,
    program::{Instruction, TemporaryRuleKind},
    stack_window::StackEntry,
    toolbox::{
        ApplyContinuousSpec, ChoiceRecord, ExecValue, PromptChoiceFrame, SideDeckOpKind,
        SideDeckOpSpec, StateTrackOp, StateTrackValue,
    },
};
use std::collections::{BTreeMap, BTreeSet};

impl EffectWindow {
    /// Read-only access to the generic object metadata tag and partition store.
    #[must_use]
    pub fn object_tags(&self) -> &crate::toolbox::ObjectTagStore {
        &self.object_tags
    }

    /// Read-only access to the generic state-track store.
    #[must_use]
    pub fn state_tracks(&self) -> &crate::toolbox::StateTrackStore {
        &self.state_tracks
    }

    /// Read-only access to the generic supplemental side-deck store.
    #[must_use]
    pub fn side_decks(&self) -> &crate::toolbox::SideDeckStore {
        &self.side_decks
    }

    /// Mutable access to install supplemental side-decks during setup/tests.
    pub fn side_decks_mut(&mut self) -> &mut crate::toolbox::SideDeckStore {
        &mut self.side_decks
    }

    /// Read-only access to the recorded interactive choice history.
    #[must_use]
    pub fn choice_history(&self) -> &[ChoiceRecord] {
        &self.choice_history
    }

    fn execute_apply_continuous(
        &mut self,
        entry: &StackEntry,
        spec: &ApplyContinuousSpec,
    ) -> Result<StepOutcome, EffectError> {
        let controller = entry.controller();
        if let Some(cond) = &spec.condition
            && !self.eval_condition_expr_at(
                controller,
                Some(entry),
                cond,
                self.resolution_cursor(),
            )?
        {
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }

        if let Some(emblem_players) = &spec.as_emblem {
            let seats = self.eval_player_selector_at(
                controller,
                Some(entry),
                emblem_players,
                self.resolution_cursor(),
            )?;
            let label = spec
                .emblem_label
                .clone()
                .unwrap_or_else(|| "Emblem".to_owned());
            for seat in seats {
                self.characteristics.create_emblem(
                    seat,
                    EmblemDefinition::Authored {
                        label: label.clone(),
                        continuous: vec![spec.rule.clone()],
                        triggered: Vec::new(),
                    },
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }

        let Some(resolved_dur) = self.resolve_duration(entry, spec.duration)? else {
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        };
        let source_object = self.entry_source_object(Some(entry));
        let locked_objects: Option<BTreeSet<ObjectId>> = match &spec.selector {
            Some(q) => Some(
                self.eval_object_query_at(controller, Some(entry), q, self.resolution_cursor())?
                    .into_iter()
                    .collect(),
            ),
            None => None,
        };

        if matches!(spec.rule, StaticContinuousAbility::Battlefield { .. }) {
            self.characteristics.add_temporary_static_locked(
                controller,
                source_object,
                resolved_dur,
                spec.rule.clone(),
                locked_objects,
            )?;
        } else {
            let id = self.next_temp_rule_id;
            self.next_temp_rule_id = self.next_temp_rule_id.saturating_add(1);
            let rule_kind = if spec.one_shot {
                TemporaryRuleKind::OneShotEventReplacement(Box::new(spec.rule.clone()))
            } else {
                TemporaryRuleKind::TemporaryStatic(Box::new(spec.rule.clone()))
            };
            let bound_target = self
                .instruction_target(entry)
                .ok()
                .map(|t| t.choice().target);
            let second_bound_target = entry.targets().targets().get(1).map(|t| t.choice().target);
            let source_damage = self.resolution_damage_source(entry).ok();
            self.temporary_rules.push(super::ActiveTemporaryRule {
                id,
                controller,
                source_object,
                source_damage,
                source_entry: Some(entry.id()),
                rule: rule_kind,
                duration: resolved_dur,
                resolved_players: BTreeSet::new(),
                resolved_objects: locked_objects.unwrap_or_default(),
                target: bound_target,
                second_target: second_bound_target,
                consumed: false,
                spell_cast_baselines: BTreeMap::new(),
            });
        }

        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }

    /// Execute any Universal Toolbox [`Instruction`].
    #[inline(never)]
    pub(super) fn progress_toolbox_instruction(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        instruction: Instruction,
    ) -> Result<StepOutcome, EffectError> {
        let controller = entry.controller();
        match instruction {
            Instruction::BindEnv { binding, value } => {
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.env.bind(binding, *value);
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::SetObjectTag { query, key, value } => {
                let ids = self.eval_object_query_at(
                    controller,
                    Some(entry),
                    &query,
                    self.resolution_cursor(),
                )?;
                for id in ids {
                    match &value {
                        Some(v) => self.object_tags.set(id, key.clone(), v.clone()),
                        None => {
                            self.object_tags.remove(id, &key);
                        }
                    }
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::UpdateStateTrack { key, op } => {
                let resolved_keys = self.eval_state_track_keys(
                    controller,
                    Some(entry),
                    &key,
                    self.resolution_cursor(),
                )?;
                let source_id = self.entry_source_object(Some(entry));
                match op {
                    StateTrackOp::SetInt(expr) => {
                        let v = self.resolve_value_expr_with_source(
                            controller,
                            &expr,
                            self.resolution_cursor(),
                            source_id,
                        )?;
                        for k in resolved_keys {
                            self.state_tracks.set(k, StateTrackValue::Int(v.clone()));
                        }
                    }
                    StateTrackOp::AddInt(expr) => {
                        let delta = self.resolve_value_expr_with_source(
                            controller,
                            &expr,
                            self.resolution_cursor(),
                            source_id,
                        )?;
                        for k in resolved_keys {
                            self.state_tracks.add_int(k, &delta);
                        }
                    }
                    StateTrackOp::SetBool(flag) => {
                        for k in resolved_keys {
                            self.state_tracks.set(k, StateTrackValue::Bool(flag));
                        }
                    }
                    StateTrackOp::SetSeat(sel) => {
                        if let Some(seat) = self
                            .eval_player_selector_at(
                                controller,
                                Some(entry),
                                &sel,
                                self.resolution_cursor(),
                            )?
                            .into_iter()
                            .next()
                        {
                            for k in resolved_keys {
                                self.state_tracks.set(k, StateTrackValue::Seat(seat));
                            }
                        }
                    }
                    StateTrackOp::SetLabel(label) => {
                        for k in resolved_keys {
                            self.state_tracks
                                .set(k, StateTrackValue::Label(label.clone()));
                        }
                    }
                    StateTrackOp::Clear => {
                        for k in resolved_keys {
                            self.state_tracks.clear(&k);
                        }
                    }
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::SideDeckOp(spec) => {
                let SideDeckOpSpec {
                    owner,
                    kind,
                    op,
                    bind_succeeded,
                } = *spec;
                let owner_seat = match owner {
                    Some(sel) => self
                        .eval_player_selector_at(
                            controller,
                            Some(entry),
                            &sel,
                            self.resolution_cursor(),
                        )?
                        .into_iter()
                        .next(),
                    None => None,
                };
                let deck = self.side_decks.get_mut_or_default(owner_seat, kind);
                let succeeded = match op {
                    SideDeckOpKind::AdvanceTop => deck.advance_top().is_some(),
                    SideDeckOpKind::CycleToBottom => deck.cycle_to_bottom().is_some(),
                    SideDeckOpKind::RecycleDiscardToBottom => {
                        if deck.discard.is_empty() {
                            false
                        } else {
                            let drained = std::mem::take(&mut deck.discard);
                            deck.cards.extend(drained);
                            true
                        }
                    }
                };
                if let Some(slot) = bind_succeeded
                    && let Some(cur) = self.resolution_cursor_mut()
                {
                    cur.env.bind(slot, ExecValue::Bool(succeeded));
                }
                self.advance_instruction()?;
                Ok(StepOutcome::Continue)
            }
            Instruction::BranchCondition {
                condition,
                destination,
            } => {
                let holds = self.eval_condition_expr_at(
                    controller,
                    Some(entry),
                    &condition,
                    self.resolution_cursor(),
                )?;
                if holds {
                    self.advance_instruction()?;
                } else {
                    self.set_instruction(destination)?;
                }
                Ok(StepOutcome::Continue)
            }
            Instruction::ForEach {
                query,
                bind_item,
                body,
            } => {
                let ids = self.eval_object_query_at(
                    controller,
                    Some(entry),
                    &query,
                    self.resolution_cursor(),
                )?;
                if ids.is_empty() || body.instructions.is_empty() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let mut expanded = Vec::with_capacity(ids.len() * (1 + body.instructions.len()));
                for id in ids {
                    expanded.push(Instruction::BindEnv {
                        binding: bind_item,
                        value: Box::new(ExecValue::Objects(vec![id])),
                    });
                    expanded.extend(body.instructions.iter().cloned());
                }
                self.splice_replacing_current(entry.id(), index, &expanded)?;
                Ok(StepOutcome::Continue)
            }
            Instruction::ForEachPlayerQuery {
                selector,
                bind_player,
                body,
            } => {
                let seats = self.eval_player_selector_at(
                    controller,
                    Some(entry),
                    &selector,
                    self.resolution_cursor(),
                )?;
                if seats.is_empty() || body.instructions.is_empty() {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let mut expanded = Vec::with_capacity(seats.len() * (1 + body.instructions.len()));
                for seat in seats {
                    expanded.push(Instruction::BindEnv {
                        binding: bind_player,
                        value: Box::new(ExecValue::Players(vec![seat])),
                    });
                    expanded.extend(body.instructions.iter().cloned());
                }
                self.splice_replacing_current(entry.id(), index, &expanded)?;
                Ok(StepOutcome::Continue)
            }
            Instruction::PromptChoice(spec) => {
                let actor = self
                    .eval_player_selector_at(
                        controller,
                        Some(entry),
                        &spec.chooser,
                        self.resolution_cursor(),
                    )?
                    .into_iter()
                    .next()
                    .unwrap_or(controller);
                let resolved_domain = self.resolve_prompt_domain(entry, &spec.domain)?;
                if let Some(reply) = self
                    .resolution_cursor_mut()
                    .and_then(|c| c.pending_prompt_reply.take())
                {
                    self.apply_prompt_reply(
                        entry,
                        index,
                        actor,
                        &spec,
                        &resolved_domain,
                        reply,
                        ctx.append,
                    )?;
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                if spec.auto_resolve_trivial
                    && let Some(auto_reply) = Self::trivial_prompt_reply(&resolved_domain)
                {
                    self.apply_prompt_reply(
                        entry,
                        index,
                        actor,
                        &spec,
                        &resolved_domain,
                        auto_reply,
                        ctx.append,
                    )?;
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let frame = PromptChoiceFrame {
                    revision: self.revision,
                    entry: entry.id(),
                    instruction: index,
                    chooser: actor,
                    category: spec.category,
                    domain: resolved_domain,
                };
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.pending_prompt_frame = Some(frame.clone());
                }
                Ok(StepOutcome::Pending(EffectDecision::PromptChoice(frame)))
            }
            Instruction::ZoneOp(spec) => self.execute_zone_op(ctx, entry, index, &spec),
            Instruction::ApplyContinuous(spec) => self.execute_apply_continuous(entry, &spec),
            other => Ok(StepOutcome::Next(other)),
        }
    }
}
