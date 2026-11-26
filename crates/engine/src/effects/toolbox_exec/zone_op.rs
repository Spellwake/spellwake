//! Execution of the universal `ZoneOp` pipeline (`Source -> Select -> Move -> Bind/Tag`).

use crate::{
    characteristics::{KeywordAbility, PowerToughnessCounters},
    effects::{
        CounterKind, EffectDecision, EffectError, EffectWindow,
        progress_rules::{RuleStepContext, StepOutcome},
    },
    object_identity::{ObjectDestination, ObjectId, ObjectZone},
    occurrence::{EventCause, RulesOccurrence, ZoneOccurrence, ZoneReason},
    priority::SeatId,
    program::LibraryPlacement,
    stack_window::StackEntry,
    targets::TargetRef,
    toolbox::{
        ExecValue, GraveyardSemantics, LibraryOrderPos, ObjectSnapshot, PromptChoiceFrame,
        PromptChoiceReply, ResolvedPromptDomain, TagKey, TagValue, ZoneController, ZoneDest,
        ZoneOpSpec, ZoneSelection,
    },
};
use spellwake_contracts::RulesInteger;
use std::collections::BTreeSet;

impl EffectWindow {
    fn emit_zone_op_moves(
        &mut self,
        entry: &StackEntry,
        index: usize,
        before: &crate::effects::occurrence_flow::RulesSnapshot,
        movements: &[crate::object_identity::ObjectMovement],
        reason: ZoneReason,
        append: &mut crate::occurrence::RulesJournal,
    ) -> Result<(), EffectError> {
        let moved_refs = movements.to_vec();
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            },
            before,
            None,
            append,
            |before_snap, after_snap, _| {
                let mut occs = Vec::with_capacity(moved_refs.len());
                for mv in &moved_refs {
                    if let (Some(b), Some(a)) = (
                        before_snap.appearance(mv.before.id()),
                        after_snap.appearance(mv.after.id()),
                    ) {
                        occs.push(RulesOccurrence::ZoneChange(Box::new(ZoneOccurrence {
                            additional_arrivals: Vec::new(),
                            madness: false,
                            before: b,
                            after: a,
                            reason,
                        })));
                    }
                }
                Ok(occs)
            },
        )
    }

    pub(super) fn execute_zone_op(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        spec: &ZoneOpSpec,
    ) -> Result<StepOutcome, EffectError> {
        let controller = entry.controller();
        let source_id = self.entry_source_object(Some(entry));
        let candidates = self.eval_object_query_at(
            controller,
            Some(entry),
            &spec.source,
            self.resolution_cursor(),
        )?;

        let (selected, unchosen): (Vec<ObjectId>, Vec<ObjectId>) = match &spec.selection {
            ZoneSelection::All => (candidates, Vec::new()),
            ZoneSelection::Random { count, .. } => {
                let want = self
                    .resolve_value_expr_with_source(
                        controller,
                        count,
                        self.resolution_cursor(),
                        source_id,
                    )?
                    .to_usize()
                    .unwrap_or(0)
                    .min(candidates.len());
                let mut pool = candidates.clone();
                let mut picked = Vec::with_capacity(want);
                let mut seed = self.revision ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                for _ in 0..want {
                    if pool.is_empty() {
                        break;
                    }
                    seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    let idx = usize::try_from(seed % (pool.len() as u64)).unwrap_or(0);
                    picked.push(pool.remove(idx));
                }
                (picked, pool)
            }
            ZoneSelection::Prompt {
                chooser,
                min,
                max,
                category,
                hidden,
                ..
            } => {
                let actor = self
                    .eval_player_selector_at(
                        controller,
                        Some(entry),
                        chooser,
                        self.resolution_cursor(),
                    )?
                    .into_iter()
                    .next()
                    .unwrap_or(controller);
                let raw_min = self
                    .resolve_value_expr_with_source(
                        controller,
                        min,
                        self.resolution_cursor(),
                        source_id,
                    )?
                    .to_usize()
                    .unwrap_or(0);
                let raw_max = self
                    .resolve_value_expr_with_source(
                        controller,
                        max,
                        self.resolution_cursor(),
                        source_id,
                    )?
                    .to_usize()
                    .unwrap_or(candidates.len());
                let clamped_max = raw_max.min(candidates.len());
                let clamped_min = raw_min.min(clamped_max);
                let reply = self
                    .resolution_cursor_mut()
                    .and_then(|c| c.pending_prompt_reply.take());
                let chosen = if let Some(PromptChoiceReply::Objects(ids)) = reply {
                    self.record_choice_and_emit(
                        entry,
                        index,
                        super::prompt::ChoiceEventMeta {
                            chooser: actor,
                            category: *category,
                            option_indices: Vec::new(),
                            chosen_count: ids.len(),
                        },
                        ctx.append,
                    )?;
                    ids
                } else if candidates.is_empty() && clamped_min == 0 {
                    Vec::new()
                } else {
                    let frame = PromptChoiceFrame {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: actor,
                        category: *category,
                        domain: ResolvedPromptDomain::Objects {
                            candidates,
                            min: clamped_min,
                            max: clamped_max,
                            hidden: *hidden,
                        },
                    };
                    if let Some(cur) = self.resolution_cursor_mut() {
                        cur.pending_prompt_frame = Some(frame.clone());
                    }
                    return Ok(StepOutcome::Pending(EffectDecision::PromptChoice(frame)));
                };
                let chosen_set: BTreeSet<ObjectId> = chosen.iter().copied().collect();
                let rest: Vec<ObjectId> = candidates
                    .into_iter()
                    .filter(|id| !chosen_set.contains(id))
                    .collect();
                (chosen, rest)
            }
        };

        let evaluated = self.evaluated_characteristics()?;
        let snapshots: Vec<ObjectSnapshot> = selected
            .iter()
            .filter_map(|&id| self.capture_object_snapshot(id, &evaluated))
            .collect();

        let mut moved_ids = Vec::new();
        let mut shuffle_owners: BTreeSet<SeatId> = BTreeSet::new();

        match &spec.destination {
            ZoneDest::RevealOnly | ZoneDest::LookOnly { .. } => {
                moved_ids = selected;
            }
            ZoneDest::Graveyard { semantics } => {
                let mut to_move = Vec::new();
                for id in selected {
                    if self.window.cards().get(id).is_none() {
                        continue;
                    }
                    if let GraveyardSemantics::Destroy { allow_regenerate } = semantics {
                        if evaluated
                            .get(id)
                            .is_some_and(|c| c.has_keyword(KeywordAbility::Indestructible))
                        {
                            continue;
                        }
                        if *allow_regenerate
                            && let Some(shields) = self.regeneration_shields.get_mut(&id)
                            && let Some(first) = shields.iter().next().copied()
                        {
                            shields.remove(&first);
                            if shields.is_empty() {
                                self.regeneration_shields.remove(&id);
                            }
                            self.damage.remove(&id);
                            let _ = self.window.enter_tapped(id);
                            continue;
                        }
                    }
                    to_move.push((id, ObjectDestination::Graveyard));
                }
                if !to_move.is_empty() {
                    let reason = match semantics {
                        GraveyardSemantics::Move => ZoneReason::PutLibraryCohortIntoGraveyard,
                        GraveyardSemantics::Destroy { .. } => ZoneReason::Destroyed,
                        GraveyardSemantics::Sacrifice => {
                            ZoneReason::Sacrifice { player: controller }
                        }
                        GraveyardSemantics::Discard => ZoneReason::Discard { player: controller },
                        GraveyardSemantics::Mill => ZoneReason::Mill { player: controller },
                    };
                    let before = self.rules_snapshot(None)?;
                    let (window, movements) = self.window.record_linked_return_moves(&to_move)?;
                    self.window = window;
                    moved_ids = movements.iter().map(|m| m.after.id()).collect();
                    self.emit_zone_op_moves(entry, index, &before, &movements, reason, ctx.append)?;
                }
            }
            ZoneDest::Exile {
                face_down,
                viewer,
                link_to_source,
            } => {
                let to_move: Vec<(ObjectId, ObjectDestination)> = selected
                    .iter()
                    .copied()
                    .filter(|id| self.window.cards().get(*id).is_some())
                    .map(|id| (id, ObjectDestination::Exile))
                    .collect();
                if !to_move.is_empty() {
                    let viewer_seat = match viewer {
                        Some(sel) => self
                            .eval_player_selector_at(
                                controller,
                                Some(entry),
                                sel,
                                self.resolution_cursor(),
                            )?
                            .into_iter()
                            .next()
                            .unwrap_or(controller),
                        None => controller,
                    };
                    let before = self.rules_snapshot(None)?;
                    let (window, movements) = self.window.record_linked_return_moves(&to_move)?;
                    self.window = window;
                    moved_ids = movements.iter().map(|m| m.after.id()).collect();
                    for &new_id in &moved_ids {
                        if *face_down {
                            self.face_down_exile.insert(new_id, viewer_seat);
                        }
                        if *link_to_source && let Some(src) = source_id {
                            self.linked_exiles.entry(src).or_default().push(new_id);
                            self.object_tags.set(
                                new_id,
                                TagKey::LinkedSource(src),
                                TagValue::Object(src),
                            );
                        }
                    }
                    self.emit_zone_op_moves(
                        entry,
                        index,
                        &before,
                        &movements,
                        ZoneReason::ExiledByEffect,
                        ctx.append,
                    )?;
                }
            }
            ZoneDest::Hand => {
                let to_move: Vec<(ObjectId, ObjectDestination)> = selected
                    .iter()
                    .copied()
                    .filter(|id| self.window.cards().get(*id).is_some())
                    .map(|id| (id, ObjectDestination::Hand))
                    .collect();
                if !to_move.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let (window, movements) = self.window.record_linked_return_moves(&to_move)?;
                    self.window = window;
                    moved_ids = movements.iter().map(|m| m.after.id()).collect();
                    self.emit_zone_op_moves(
                        entry,
                        index,
                        &before,
                        &movements,
                        ZoneReason::ReturnToOwnersHand,
                        ctx.append,
                    )?;
                }
            }
            ZoneDest::Library { position } => {
                let (dest, placement) = match position {
                    LibraryOrderPos::Top => (ObjectDestination::LibraryTop, LibraryPlacement::Top),
                    LibraryOrderPos::Bottom | LibraryOrderPos::ShuffleInto => {
                        (ObjectDestination::LibraryBottom, LibraryPlacement::Bottom)
                    }
                    LibraryOrderPos::NthFromTop(expr) => {
                        let offset = self
                            .resolve_value_expr_with_source(
                                controller,
                                expr,
                                self.resolution_cursor(),
                                source_id,
                            )?
                            .to_usize()
                            .unwrap_or(1)
                            .saturating_sub(1);
                        let offset_u8 = u8::try_from(offset).unwrap_or(u8::MAX);
                        (
                            ObjectDestination::LibraryFromTop(offset_u8),
                            LibraryPlacement::FromTop(offset_u8),
                        )
                    }
                };
                let mut to_move = Vec::new();
                for id in selected {
                    if let Some(obj) = self.window.cards().get(id) {
                        if matches!(position, LibraryOrderPos::ShuffleInto) {
                            shuffle_owners.insert(obj.owner());
                        }
                        to_move.push((id, dest));
                    }
                }
                if !to_move.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let (window, movements) = self.window.record_linked_return_moves(&to_move)?;
                    self.window = window;
                    moved_ids = movements.iter().map(|m| m.after.id()).collect();
                    self.emit_zone_op_moves(
                        entry,
                        index,
                        &before,
                        &movements,
                        ZoneReason::PutIntoOwnersLibrary(placement),
                        ctx.append,
                    )?;
                }
            }
            ZoneDest::Battlefield {
                controller: dest_ctrl,
                tapped,
                transformed,
                face_down: _,
                entering_counters,
                attach_to,
            } => {
                let explicit_ctrl = match dest_ctrl {
                    ZoneController::Owner => None,
                    ZoneController::Player(sel) => self
                        .eval_player_selector_at(
                            controller,
                            Some(entry),
                            sel,
                            self.resolution_cursor(),
                        )?
                        .into_iter()
                        .next(),
                };
                let attach_host = match attach_to {
                    Some(q) => self
                        .eval_object_query_at(controller, Some(entry), q, self.resolution_cursor())?
                        .into_iter()
                        .next(),
                    None => None,
                };
                let mut evaluated_counters = Vec::with_capacity(entering_counters.len());
                for (kind, expr) in entering_counters {
                    let amount = self.resolve_value_expr_with_source(
                        controller,
                        expr,
                        self.resolution_cursor(),
                        source_id,
                    )?;
                    if amount.is_positive() {
                        evaluated_counters.push((*kind, amount));
                    }
                }
                let mut to_move = Vec::new();
                for id in selected {
                    if let Some(obj) = self.window.cards().get(id) {
                        let seat = explicit_ctrl.unwrap_or_else(|| obj.owner());
                        to_move.push((id, ObjectDestination::Battlefield { controller: seat }));
                    }
                }
                if !to_move.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let (window, mut movements) =
                        self.window.record_linked_return_moves(&to_move)?;
                    self.window = window;
                    for mv in &mut movements {
                        if mv.after.zone() != ObjectZone::Battlefield {
                            continue;
                        }
                        let new_id = mv.after.id();
                        self.characteristics.register_battlefield_entry(new_id)?;
                        if *tapped && let Ok(updated) = self.window.enter_tapped(new_id) {
                            mv.after = updated;
                        }
                        if *transformed
                            && self.window.transform(new_id).is_ok()
                            && let Some(updated) = self.window.cards().get(new_id).cloned()
                        {
                            mv.after = updated;
                        }
                        for (ck, amount) in &evaluated_counters {
                            match *ck {
                                CounterKind::PlusOne => {
                                    let _ = self.characteristics.place_counters(
                                        new_id,
                                        &PowerToughnessCounters {
                                            plus_one: amount.clone(),
                                            minus_one: 0.into(),
                                        },
                                    );
                                }
                                CounterKind::MinusOne => {
                                    let _ = self.characteristics.place_counters(
                                        new_id,
                                        &PowerToughnessCounters {
                                            plus_one: 0.into(),
                                            minus_one: amount.clone(),
                                        },
                                    );
                                }
                                other => {
                                    if let Some(map) =
                                        self.characteristics.named_permanent_counters_mut(other)
                                    {
                                        let cur =
                                            map.get(&new_id).cloned().unwrap_or_else(|| 0.into());
                                        map.insert(new_id, cur.plus(amount));
                                    }
                                }
                            }
                        }
                        if let Some(host) = attach_host {
                            let _ =
                                self.attach_if_legal(entry.id(), new_id, TargetRef::Object(host));
                        }
                    }
                    moved_ids = movements.iter().map(|m| m.after.id()).collect();
                    self.emit_zone_op_moves(
                        entry,
                        index,
                        &before,
                        &movements,
                        ZoneReason::ReturnFromGraveyardToBattlefield,
                        ctx.append,
                    )?;
                }
            }
        }

        if let Some((tag_key, tag_val)) = &spec.tag_moved {
            for &id in &moved_ids {
                self.object_tags.set(id, tag_key.clone(), tag_val.clone());
            }
        }

        let succeeded = !moved_ids.is_empty();
        if let Some(cur) = self.resolution_cursor_mut() {
            if let Some(slot) = spec.bind_moved {
                cur.env.bind(slot, ExecValue::Objects(moved_ids.clone()));
            }
            if let Some(slot) = spec.bind_snapshots {
                cur.env.bind(slot, ExecValue::ObjectSnapshots(snapshots));
            }
            if let Some(slot) = spec.bind_unchosen {
                cur.env.bind(slot, ExecValue::Objects(unchosen));
            }
            if let Some(slot) = spec.bind_succeeded {
                cur.env.bind(slot, ExecValue::Bool(succeeded));
            }
            cur.results.last_moved_objects = moved_ids;
            cur.results.last_action_succeeded = succeeded;
            cur.results.last_action_count = Some(RulesInteger::from_usize(
                cur.results.last_moved_objects.len(),
            ));
        }

        if let Some(shuf_sel) = &spec.shuffle_library {
            for seat in self.eval_player_selector_at(
                controller,
                Some(entry),
                shuf_sel,
                self.resolution_cursor(),
            )? {
                shuffle_owners.insert(seat);
            }
        }
        for seat in shuffle_owners {
            let _ = self.begin_shuffle(seat, false);
        }

        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }
}
