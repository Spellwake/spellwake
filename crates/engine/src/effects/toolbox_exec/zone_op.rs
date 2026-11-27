//! Execution of the universal `ZoneOp` pipeline (`Source -> Select -> Move -> Bind/Tag`).

use crate::{
    characteristics::{KeywordAbility, PowerToughnessCounters},
    effects::StackPayload,
    effects::{
        CounterKind, EffectDecision, EffectError, EffectWindow, LinkedReturnOrigin,
        progress_rules::{RuleStepContext, StepOutcome},
        zone_replacement,
    },
    object_identity::{GameObject, ObjectDestination, ObjectId, ObjectZone},
    occurrence::{EventCause, RulesOccurrence, ZoneOccurrence, ZoneReason},
    priority::SeatId,
    program::{
        BodyTarget, DelayedReturnTiming, ExecutableBody, Instruction, LibraryPlacement,
        PublicZoneDestination, TemporaryRuleKind, ZoneActionKind,
    },
    stack_window::{StackEntry, StackKind},
    targets::TargetRef,
    toolbox::{
        ChoiceCategory, ExecValue, GraveyardSemantics, LibraryOrderPos, ObjectSnapshot,
        PromptChoiceFrame, PromptChoiceReply, ResolvedPromptDomain, TagKey, TagValue,
        ZoneController, ZoneDest, ZoneOpSpec, ZoneSelection,
    },
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

fn query_scans_library(query: &crate::toolbox::ObjectQuery) -> bool {
    match query {
        crate::toolbox::ObjectQuery::ZoneScan {
            zone: crate::toolbox::QueryZone::Library(_),
            ..
        } => true,
        crate::toolbox::ObjectQuery::Filtered { source, .. }
        | crate::toolbox::ObjectQuery::Take { source, .. } => query_scans_library(source),
        _ => false,
    }
}

impl EffectWindow {
    fn reorder_library_cards(
        &mut self,
        owner: SeatId,
        entry: crate::stack_window::StackEntryId,
        ids: &[ObjectId],
        to_top: bool,
        nth_from_top: Option<usize>,
    ) -> Result<(), EffectError> {
        let library = self.window.cards().library(owner);
        let present: Vec<ObjectId> = ids
            .iter()
            .copied()
            .filter(|id| library.contains(id))
            .collect();
        if present.is_empty() {
            return Ok(());
        }
        let max_idx = present
            .iter()
            .filter_map(|id| library.iter().position(|c| c == id))
            .max()
            .ok_or(EffectError::InvalidLibraryPartition)?;
        let prefix_len = match nth_from_top {
            Some(offset) => (max_idx + 1)
                .max(offset.saturating_add(present.len()))
                .min(library.len()),
            None => max_idx + 1,
        };
        let frozen = library
            .get(..prefix_len)
            .ok_or(EffectError::InvalidLibraryPartition)?
            .to_vec();
        let present_set: BTreeSet<ObjectId> = present.iter().copied().collect();
        let others: Vec<ObjectId> = frozen
            .iter()
            .copied()
            .filter(|id| !present_set.contains(id))
            .collect();
        let (top, bottom): (Vec<ObjectId>, Vec<ObjectId>) = if let Some(offset) = nth_from_top {
            let insert_at = offset.saturating_sub(1).min(others.len());
            let mut combined = Vec::with_capacity(frozen.len());
            combined.extend_from_slice(&others[..insert_at]);
            combined.extend_from_slice(&present);
            combined.extend_from_slice(&others[insert_at..]);
            (combined, Vec::new())
        } else if to_top {
            let mut combined = Vec::with_capacity(frozen.len());
            combined.extend_from_slice(&present);
            combined.extend_from_slice(&others);
            (combined, Vec::new())
        } else {
            (others, present)
        };
        let (window, _) = self.window.record_library_top(
            self.window.revision(),
            entry,
            crate::stack_window::LibraryTopCommit {
                player: owner,
                frozen: &frozen,
                top: &top,
                bottom: &bottom,
                moves_application: &[],
            },
        )?;
        self.window = window;
        Ok(())
    }

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

    fn apply_exile_if_leaves_replacement(&self, moves: &mut [(ObjectId, ObjectDestination)]) {
        for (obj_id, dest) in moves {
            if !matches!(dest, ObjectDestination::Exile)
                && let Some(card) = self
                    .window
                    .cards()
                    .get(*obj_id)
                    .filter(|c| c.zone() == ObjectZone::Battlefield)
                && (card.is_unearthed()
                    || card.has_corpse_return()
                    || self.temporary_rules.iter().any(|active| {
                        matches!(
                            active.rule,
                            TemporaryRuleKind::ExileIfLeavesBattlefield { .. }
                        ) && active.resolved_objects.contains(obj_id)
                    }))
            {
                *dest = ObjectDestination::Exile;
            }
        }
    }

    pub(in crate::effects) fn execute_zone_op(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        spec: &ZoneOpSpec,
    ) -> Result<StepOutcome, EffectError> {
        let controller = entry.controller();
        let source_id = self.entry_source_object(Some(entry));
        if matches!(spec.source, crate::toolbox::ObjectQuery::DestroyedThisWay)
            && matches!(spec.destination, ZoneDest::Battlefield { .. })
        {
            let cards = std::mem::take(
                &mut self
                    .resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .results
                    .destroyed_cards_to_graveyard,
            );
            self.begin_linked_cohort_return(
                LinkedReturnOrigin::Graveyard {
                    player: None,
                    controller: Some(controller),
                    subtype_counters: None,
                    entry_additions: crate::program::EntryAdditions::default(),
                    entry: entry.id(),
                    instruction: index,
                    tapped: false,
                    forest: false,
                    corpse: false,
                },
                cards,
                ctx.append,
            )?;
            return Ok(StepOutcome::Continue);
        }

        let chosen_library_pos: Option<LibraryOrderPos> = if let ZoneDest::Library {
            position: LibraryOrderPos::ChoiceTopOrBottom { chooser },
        } = &spec.destination
        {
            let reply = self
                .resolution_cursor_mut()
                .and_then(|c| c.pending_prompt_reply.take());
            if let Some(PromptChoiceReply::Options(idxs)) = reply {
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
                self.record_choice_and_emit(
                    entry,
                    index,
                    super::prompt::ChoiceEventMeta {
                        chooser: actor,
                        category: ChoiceCategory::Generic,
                        option_indices: idxs.clone(),
                        chosen_count: idxs.len(),
                    },
                    ctx.append,
                )?;
                let pick = idxs.first().copied().unwrap_or(0);
                Some(if pick == 0 {
                    LibraryOrderPos::Top
                } else {
                    LibraryOrderPos::Bottom
                })
            } else {
                let candidates_check = self.eval_object_query_at(
                    controller,
                    Some(entry),
                    &spec.source,
                    self.resolution_cursor(),
                )?;
                if candidates_check.is_empty() {
                    Some(LibraryOrderPos::Top)
                } else {
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
                    let frame = PromptChoiceFrame {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: actor,
                        category: ChoiceCategory::Generic,
                        domain: ResolvedPromptDomain::Options {
                            labels: vec![
                                "Put on top of library".to_owned(),
                                "Put on bottom of library".to_owned(),
                            ],
                            min: 1,
                            max: 1,
                            allow_repeat: false,
                        },
                    };
                    if let Some(cur) = self.resolution_cursor_mut() {
                        cur.pending_prompt_frame = Some(frame.clone());
                    }
                    return Ok(StepOutcome::Pending(EffectDecision::PromptChoice(frame)));
                }
            }
        } else {
            None
        };

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
                constraint,
                category,
                hidden,
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
                    if let Some(constraint) = *constraint
                        && !self.satisfies_selection_constraint(&ids, constraint)?
                    {
                        return Err(EffectError::SelectionOutsideDomain);
                    }
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
                } else if clamped_min == clamped_max
                    && clamped_min == candidates.len()
                    && match *constraint {
                        None => true,
                        Some(constraint) => {
                            self.satisfies_selection_constraint(&candidates, constraint)?
                        }
                    }
                {
                    candidates.clone()
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
        let before_lki: Vec<_> = selected
            .iter()
            .filter_map(|id| evaluated.get(*id).cloned())
            .collect();
        let before_controllers: Vec<SeatId> = selected
            .iter()
            .filter_map(|&oid| {
                self.window
                    .cards()
                    .get(oid)
                    .and_then(|o| o.controller().or(Some(o.owner())))
            })
            .collect();

        let mut moved_ids = Vec::new();
        let mut shuffle_owners: BTreeSet<SeatId> = BTreeSet::new();
        let resolving_spell_id = match entry.kind() {
            StackKind::CardSpell(id)
                if self
                    .window
                    .cards()
                    .get(*id)
                    .is_some_and(|c| c.zone() == ObjectZone::Stack) =>
            {
                Some(*id)
            }
            _ => None,
        };

        match &spec.destination {
            ZoneDest::None => {
                moved_ids = selected;
            }
            ZoneDest::RevealOnly => {
                if selected.is_empty() {
                    if query_scans_library(&spec.source)
                        && let Some(cur) = self.resolution_cursor_mut()
                    {
                        cur.results.revealed_top_card = None;
                        cur.results.revealed_top_types.clear();
                    }
                } else if let Some(&first) = selected.first() {
                    let first_in_library = self
                        .window
                        .cards()
                        .get(first)
                        .is_some_and(|c| matches!(c.zone(), ObjectZone::Library(_)));
                    if first_in_library {
                        let revealed_types: BTreeSet<crate::program::CardType> = selected
                            .iter()
                            .filter(|&&id| {
                                self.window
                                    .cards()
                                    .get(id)
                                    .is_some_and(|c| matches!(c.zone(), ObjectZone::Library(_)))
                            })
                            .filter_map(|&id| evaluated.get(id))
                            .flat_map(|c| c.current_types.iter().copied())
                            .collect();
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.results.revealed_top_card = Some(first);
                            cur.results.revealed_top_types = revealed_types;
                        }
                    } else if let Some(cur) = self.resolution_cursor_mut() {
                        cur.results.revealed_top_card = Some(first);
                    }
                }
                let library_cards: Vec<GameObject> = selected
                    .iter()
                    .copied()
                    .filter_map(|id| {
                        self.window
                            .cards()
                            .get(id)
                            .filter(|c| matches!(c.zone(), ObjectZone::Library(_)))
                            .cloned()
                    })
                    .collect();
                if !library_cards.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    self.emit_from(
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |_, _, _| {
                            Ok(library_cards
                                .iter()
                                .cloned()
                                .map(|object| RulesOccurrence::LibraryCardRevealed { object })
                                .collect())
                        },
                    )?;
                }
                let hand_cards: Vec<GameObject> = selected
                    .iter()
                    .copied()
                    .filter_map(|id| {
                        self.window
                            .cards()
                            .get(id)
                            .filter(|c| matches!(c.zone(), ObjectZone::Hand(_)))
                            .cloned()
                    })
                    .collect();
                if !hand_cards.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    self.emit_from(
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |_, _, _| {
                            Ok(hand_cards
                                .iter()
                                .cloned()
                                .map(|object| RulesOccurrence::HandCardRevealed { object })
                                .collect())
                        },
                    )?;
                }
                moved_ids = selected;
            }
            ZoneDest::LookOnly { .. } => {
                moved_ids = selected;
            }
            ZoneDest::Graveyard { semantics } => {
                let mut to_move = Vec::new();
                for id in selected {
                    if self.window.cards().get(id).is_none() {
                        continue;
                    }
                    if let GraveyardSemantics::Destroy { allow_regenerate } = semantics {
                        if !self
                            .window
                            .cards()
                            .get(id)
                            .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                        {
                            continue;
                        }
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
                self.apply_exile_if_leaves_replacement(&mut to_move);
                if !to_move.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let (window, movements) = self.window.record_linked_return_moves(&to_move)?;
                    self.window = window;
                    moved_ids = movements.iter().map(|m| m.after.id()).collect();
                    if matches!(semantics, GraveyardSemantics::Destroy { .. }) {
                        let destroyed_cards: Vec<ObjectId> = movements
                            .iter()
                            .flat_map(crate::object_identity::ObjectMovement::arrivals)
                            .filter(|id| {
                                self.window.cards().get(*id).is_some_and(|card| {
                                    card.card_id().is_some()
                                        && matches!(card.zone(), ObjectZone::Graveyard(_))
                                })
                            })
                            .collect();
                        if let Some(cur) = self.resolution_cursor_mut() {
                            cur.results.destroyed_cards_to_graveyard = destroyed_cards;
                        }
                    }
                    let moved_refs = movements.clone();
                    let sem = *semantics;
                    self.emit_from(
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |before_snap, after_snap, _| {
                            let mut occs = Vec::with_capacity(moved_refs.len());
                            for mv in &moved_refs {
                                if let (Some(b), Some(a)) = (
                                    before_snap.appearance(mv.before.id()),
                                    after_snap.appearance(mv.after.id()),
                                ) {
                                    let reason = match sem {
                                        GraveyardSemantics::Move => {
                                            ZoneReason::PutLibraryCohortIntoGraveyard
                                        }
                                        GraveyardSemantics::Destroy { .. } => ZoneReason::Destroyed,
                                        GraveyardSemantics::Sacrifice => ZoneReason::Sacrifice {
                                            player: mv
                                                .before
                                                .controller()
                                                .unwrap_or_else(|| mv.before.owner()),
                                        },
                                        GraveyardSemantics::Discard => ZoneReason::Discard {
                                            player: mv.before.owner(),
                                        },
                                        GraveyardSemantics::Mill => ZoneReason::Mill {
                                            player: mv.before.owner(),
                                        },
                                    };
                                    occs.push(RulesOccurrence::ZoneChange(Box::new(
                                        ZoneOccurrence {
                                            additional_arrivals: Vec::new(),
                                            madness: false,
                                            before: b,
                                            after: a,
                                            reason,
                                        },
                                    )));
                                }
                            }
                            Ok(occs)
                        },
                    )?;
                }
            }
            ZoneDest::Exile {
                face_down,
                viewer,
                link_to_source,
                delayed_return,
                link_group,
            } => {
                let skip_until_leaves =
                    matches!(delayed_return, Some(DelayedReturnTiming::UntilSourceLeaves))
                        && !source_id.is_some_and(|src| {
                            self.window
                                .cards()
                                .get(src)
                                .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                        });
                let mut deferred_self = false;
                let mut already_exiled = Vec::new();
                let to_move: Vec<(ObjectId, ObjectDestination)> = if skip_until_leaves {
                    Vec::new()
                } else {
                    selected
                        .iter()
                        .copied()
                        .filter(|id| {
                            if Some(*id) == resolving_spell_id {
                                deferred_self = true;
                                false
                            } else if let Some(card) = self.window.cards().get(*id) {
                                if card.zone() == ObjectZone::Exile {
                                    already_exiled.push(*id);
                                    false
                                } else {
                                    true
                                }
                            } else {
                                false
                            }
                        })
                        .map(|id| (id, ObjectDestination::Exile))
                        .collect()
                };
                if deferred_self
                    && let Some(StackPayload::Instructions(payload)) =
                        self.payloads.get_mut(&entry.id())
                {
                    payload.exile_on_resolve = true;
                    if let Some(sid) = resolving_spell_id {
                        moved_ids.push(sid);
                    }
                }
                if !to_move.is_empty() || !already_exiled.is_empty() {
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
                    let mut newly_exiled = Vec::new();
                    if !to_move.is_empty() {
                        let before = self.rules_snapshot(None)?;
                        let (window, movements) =
                            self.window.record_linked_return_moves(&to_move)?;
                        self.window = window;
                        newly_exiled = movements.iter().map(|m| m.after.id()).collect();
                        moved_ids.extend(newly_exiled.iter().copied());
                        if matches!(
                            self.payloads
                                .get(&entry.id())
                                .and_then(StackPayload::instructions)
                                .and_then(|ops| ops.get(index + 1)),
                            Some(Instruction::ZoneOp(next_spec))
                                if matches!(
                                    next_spec.destination,
                                    ZoneDest::Delayed(
                                        crate::toolbox::DelayedZoneAction::ReturnExiledTargetCohort {
                                            ..
                                        }
                                    )
                                )
                        ) {
                            let flickered: Vec<ObjectId> = movements
                                .iter()
                                .filter(|m| m.after.zone() == ObjectZone::Exile)
                                .flat_map(crate::object_identity::ObjectMovement::arrivals)
                                .filter(|id| {
                                    self.window
                                        .cards()
                                        .get(*id)
                                        .is_some_and(|card| card.card_id().is_some())
                                })
                                .collect();
                            if let Some(cur) = self.resolution_cursor_mut() {
                                cur.impulse.flickered = flickered;
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
                    moved_ids.extend(already_exiled.iter().copied());
                    for &exiled_id in newly_exiled.iter().chain(&already_exiled) {
                        if *face_down {
                            self.face_down_exile.insert(exiled_id, viewer_seat);
                        }
                        if *link_to_source && let Some(src) = source_id {
                            let linked = self.characteristics.linked_exiles.entry(src).or_default();
                            if !linked.contains(&exiled_id) {
                                linked.push(exiled_id);
                            }
                            self.object_tags.set(
                                exiled_id,
                                TagKey::LinkedSource(src),
                                TagValue::Object(src),
                            );
                        }
                        if let Some(group) = link_group
                            && let Some(src) = source_id
                        {
                            self.independent_exile_links
                                .entry((src, *group))
                                .or_default()
                                .insert(exiled_id);
                        }
                    }
                    if let Some(timing) = delayed_return
                        && !newly_exiled.is_empty()
                    {
                        match timing {
                            DelayedReturnTiming::Immediately => {
                                self.begin_linked_cohort_return(
                                    LinkedReturnOrigin::ChosenExile {
                                        controller: None,
                                        tapped: false,
                                        entry: entry.id(),
                                        instruction: index,
                                    },
                                    newly_exiled.clone(),
                                    ctx.append,
                                )?;
                            }
                            DelayedReturnTiming::NextEndStep => {
                                let snapshot = self.rules_snapshot(None)?;
                                let appearance = source_id
                                    .and_then(|sid| {
                                        snapshot.view().appearance(sid).or_else(|| {
                                            self.departed_reflexive_sources.get(&sid).cloned()
                                        })
                                    })
                                    .or_else(|| {
                                        self.execution_bindings(entry.id())
                                            .and_then(|b| b.trigger.as_ref())
                                            .map(|t| t.source.clone())
                                    });
                                if let Some(app) = appearance {
                                    self.triggers.delay_step_body(
                                        app,
                                        entry.controller(),
                                        crate::turn::Step::End,
                                        false,
                                        Box::new(ExecutableBody::new(
                                            BodyTarget::None,
                                            vec![Instruction::ZoneOp(Box::new(
                                                ZoneOpSpec::return_fixed_to_battlefield(
                                                    newly_exiled.clone(),
                                                    false,
                                                ),
                                            ))],
                                        )),
                                    );
                                }
                            }
                            DelayedReturnTiming::UntilSourceLeaves => {
                                if !*link_to_source && let Some(src) = source_id {
                                    self.characteristics
                                        .linked_exiles
                                        .entry(src)
                                        .or_default()
                                        .extend(newly_exiled.iter().copied());
                                }
                            }
                            DelayedReturnTiming::LinkedGroup(group) => {
                                if link_group.is_none()
                                    && let Some(src) = source_id
                                {
                                    self.independent_exile_links
                                        .entry((src, *group))
                                        .or_default()
                                        .extend(newly_exiled.iter().copied());
                                }
                            }
                        }
                    }
                }
            }
            ZoneDest::Hand => {
                let mut deferred_self = false;
                let mut to_move: Vec<(ObjectId, ObjectDestination)> = selected
                    .iter()
                    .copied()
                    .filter(|id| {
                        if Some(*id) == resolving_spell_id {
                            deferred_self = true;
                            false
                        } else {
                            self.window.cards().get(*id).is_some()
                        }
                    })
                    .map(|id| (id, ObjectDestination::Hand))
                    .collect();
                if deferred_self
                    && let Some(StackPayload::Instructions(payload)) =
                        self.payloads.get_mut(&entry.id())
                {
                    payload.return_to_hand_on_resolve = true;
                    if let Some(sid) = resolving_spell_id {
                        moved_ids.push(sid);
                    }
                }
                self.apply_exile_if_leaves_replacement(&mut to_move);
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
                let effective_pos = chosen_library_pos.as_ref().unwrap_or(position);
                let mut nth_offset: Option<usize> = None;
                let (dest, placement) = match effective_pos {
                    LibraryOrderPos::Top => (ObjectDestination::LibraryTop, LibraryPlacement::Top),
                    LibraryOrderPos::Bottom | LibraryOrderPos::ShuffleInto => {
                        (ObjectDestination::LibraryBottom, LibraryPlacement::Bottom)
                    }
                    LibraryOrderPos::BottomRandom => (
                        ObjectDestination::LibraryBottom,
                        LibraryPlacement::BottomRandom,
                    ),
                    LibraryOrderPos::NthFromTop(expr) => {
                        let offset = self
                            .resolve_value_expr_with_source(
                                controller,
                                expr,
                                self.resolution_cursor(),
                                source_id,
                            )?
                            .to_usize()
                            .unwrap_or(0);
                        nth_offset = Some(offset);
                        let offset_u8 = u8::try_from(offset).unwrap_or(u8::MAX);
                        (
                            ObjectDestination::LibraryFromTop(offset_u8),
                            LibraryPlacement::FromTop(offset_u8),
                        )
                    }
                    LibraryOrderPos::ChoiceTopOrBottom { .. } => {
                        (ObjectDestination::LibraryTop, LibraryPlacement::Top)
                    }
                };
                let mut deferred_self = false;
                let mut same_zone_by_owner: BTreeMap<SeatId, Vec<ObjectId>> = BTreeMap::new();
                let mut to_move = Vec::new();
                for id in selected {
                    if Some(id) == resolving_spell_id {
                        deferred_self = true;
                        if matches!(effective_pos, LibraryOrderPos::ShuffleInto)
                            && let Some(obj) = self.window.cards().get(id)
                        {
                            shuffle_owners.insert(obj.owner());
                        }
                        continue;
                    }
                    if let Some(obj) = self.window.cards().get(id) {
                        if matches!(effective_pos, LibraryOrderPos::ShuffleInto) {
                            shuffle_owners.insert(obj.owner());
                        }
                        if obj.zone() == ObjectZone::Library(obj.owner()) {
                            same_zone_by_owner.entry(obj.owner()).or_default().push(id);
                        } else {
                            to_move.push((id, dest));
                        }
                    }
                }
                if deferred_self
                    && let Some(StackPayload::Instructions(payload)) =
                        self.payloads.get_mut(&entry.id())
                {
                    payload.library_on_resolve = Some(placement);
                    if let Some(sid) = resolving_spell_id {
                        moved_ids.push(sid);
                    }
                }
                for (owner, ids) in same_zone_by_owner {
                    match effective_pos {
                        LibraryOrderPos::Top | LibraryOrderPos::ChoiceTopOrBottom { .. } => {
                            self.reorder_library_cards(owner, entry.id(), &ids, true, None)?;
                        }
                        LibraryOrderPos::Bottom | LibraryOrderPos::BottomRandom => {
                            self.reorder_library_cards(owner, entry.id(), &ids, false, None)?;
                        }
                        LibraryOrderPos::NthFromTop(_) => {
                            self.reorder_library_cards(owner, entry.id(), &ids, true, nth_offset)?;
                        }
                        LibraryOrderPos::ShuffleInto => {
                            self.reorder_library_cards(owner, entry.id(), &ids, false, None)?;
                        }
                    }
                    moved_ids.extend(ids);
                }
                self.apply_exile_if_leaves_replacement(&mut to_move);
                if !to_move.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let (window, movements) = self.window.record_linked_return_moves(&to_move)?;
                    self.window = window;
                    moved_ids.extend(movements.iter().map(|m| m.after.id()));
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
                attacking,
                transformed,
                face_down,
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
                let mut evaluated_counters: Vec<(CounterKind, RulesInteger)> = Vec::new();
                for (ck, expr) in entering_counters {
                    let amt = self.resolve_value_expr_with_source(
                        controller,
                        expr,
                        self.resolution_cursor(),
                        source_id,
                    )?;
                    if amt.is_positive() {
                        evaluated_counters.push((*ck, amt));
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
                    let mut entering_controllers = BTreeMap::new();
                    let mut intents = Vec::new();
                    for &(obj_id, dest) in &to_move {
                        if let ObjectDestination::Battlefield { controller: seat } = dest {
                            entering_controllers.insert(obj_id, seat);
                            intents.push(zone_replacement::ZoneMoveIntent {
                                before: obj_id,
                                reason: ZoneReason::ReturnFromGraveyardToBattlefield,
                            });
                        }
                    }
                    let mut replacement_batch = None;
                    if !intents.is_empty() {
                        let mut batch = self.begin_zone_replacements_with_entry(
                            zone_replacement::AuthorizedZoneBatch {
                                cause: EventCause::Instruction {
                                    entry: entry.id(),
                                    instruction: index,
                                },
                                action: ZoneActionKind::EffectEnterBattlefield,
                                intents,
                            },
                            Some(&entering_controllers),
                        )?;
                        if batch.progress(self.revision)?.is_none() {
                            for (obj_id, dest) in &mut to_move {
                                if let Ok(replaced) = batch.destination(*obj_id) {
                                    match replaced {
                                        PublicZoneDestination::Battlefield
                                        | PublicZoneDestination::Command => {}
                                        PublicZoneDestination::Exile => {
                                            *dest = ObjectDestination::Exile;
                                        }
                                        PublicZoneDestination::Graveyard => {
                                            *dest = ObjectDestination::Graveyard;
                                        }
                                        PublicZoneDestination::Hand => {
                                            *dest = ObjectDestination::Hand;
                                        }
                                        PublicZoneDestination::Library => {
                                            *dest = ObjectDestination::LibraryTop;
                                        }
                                    }
                                }
                            }
                            replacement_batch = Some(batch);
                        }
                    }

                    let before = self.rules_snapshot(None)?;
                    let mut face_up_origins = BTreeMap::new();
                    if let Some(kind) = *face_down {
                        for (obj_id, dest) in &to_move {
                            if matches!(dest, ObjectDestination::Battlefield { .. })
                                && let Some(card) = self.window.cards().get(*obj_id).cloned()
                            {
                                face_up_origins.insert(*obj_id, card);
                                self.window.set_orientation(*obj_id, Some(kind))?;
                            }
                        }
                    }
                    let (window, mut movements) =
                        self.window.record_linked_return_moves(&to_move)?;
                    for movement in &mut movements {
                        if let Some(original) = face_up_origins.remove(&movement.before.id()) {
                            movement.before = original;
                        }
                    }
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
                        if *attacking
                            && self.combat.is_some()
                            && self
                                .window
                                .cards()
                                .get(new_id)
                                .and_then(GameObject::controller)
                                == Some(self.window.active_player())
                            && let Ok(current_eval) = self.evaluated_characteristics()
                            && let Ok(defenders) =
                                self.attack_destinations(self.window.active_player(), &current_eval)
                            && let Some((&target, &seat)) = defenders.first_key_value()
                        {
                            let _ = self.put_attacking_at(new_id, seat, target);
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
                    if let Some(batch) = replacement_batch.as_ref() {
                        self.apply_zone_replacement_extras_from_movements(
                            Some(batch),
                            &movements,
                            EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            ctx.append,
                        )?;
                    }
                }
            }
            ZoneDest::SwapZones { other } => {
                let crate::toolbox::ObjectQuery::ZoneScan {
                    zone: from_zone, ..
                } = &spec.source
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                fn zone_player(
                    z: &crate::toolbox::QueryZone,
                ) -> Option<&crate::toolbox::PlayerSelector> {
                    match z {
                        crate::toolbox::QueryZone::Hand(p)
                        | crate::toolbox::QueryZone::Library(p)
                        | crate::toolbox::QueryZone::Graveyard(p)
                        | crate::toolbox::QueryZone::ExileOwnedBy(p)
                        | crate::toolbox::QueryZone::BattlefieldControlledBy(p) => Some(p.as_ref()),
                        _ => None,
                    }
                }
                let seat = match zone_player(from_zone).or_else(|| zone_player(other)) {
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
                let to_dest =
                    |z: &crate::toolbox::QueryZone| -> Option<crate::program::SubjectDestination> {
                        match z {
                            crate::toolbox::QueryZone::Hand(_) => {
                                Some(crate::program::SubjectDestination::Hand)
                            }
                            crate::toolbox::QueryZone::Graveyard(_) => {
                                Some(crate::program::SubjectDestination::Graveyard)
                            }
                            crate::toolbox::QueryZone::Library(_) => Some(
                                crate::program::SubjectDestination::Library(LibraryPlacement::Top),
                            ),
                            crate::toolbox::QueryZone::Exile
                            | crate::toolbox::QueryZone::ExileOwnedBy(_) => {
                                Some(crate::program::SubjectDestination::Exile {
                                    delayed_return: None,
                                })
                            }
                            _ => None,
                        }
                    };
                let (Some(from_dest), Some(other_dest)) = (to_dest(from_zone), to_dest(other))
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let from_cards: Vec<ObjectId> =
                    if matches!(from_zone, crate::toolbox::QueryZone::Exile) {
                        selected
                            .into_iter()
                            .filter(|id| {
                                self.window
                                    .cards()
                                    .get(*id)
                                    .is_some_and(|c| c.owner() == seat)
                            })
                            .collect()
                    } else {
                        selected
                    };
                let mut to_cards = self.collect_zone_objects(
                    controller,
                    Some(entry),
                    other,
                    self.resolution_cursor(),
                )?;
                if matches!(other, crate::toolbox::QueryZone::Exile) {
                    to_cards.retain(|id| {
                        self.window
                            .cards()
                            .get(*id)
                            .is_some_and(|c| c.owner() == seat)
                    });
                }
                if !from_cards.is_empty() {
                    let moved = self.move_objects_to_subject_destination_ids(
                        entry,
                        index,
                        seat,
                        &from_cards,
                        other_dest,
                        ctx.append,
                    )?;
                    moved_ids.extend(moved);
                }
                if !to_cards.is_empty() {
                    let moved = self.move_objects_to_subject_destination_ids(
                        entry, index, seat, &to_cards, from_dest, ctx.append,
                    )?;
                    moved_ids.extend(moved);
                }
            }
            ZoneDest::Delayed(_) => return Err(EffectError::InconsistentProgram),
        }

        if let Some((tag_key, tag_val)) = &spec.tag_moved {
            for &id in &moved_ids {
                self.object_tags.set(id, tag_key.clone(), tag_val.clone());
            }
        }

        let target_lkis: Vec<_> = if matches!(spec.source, crate::toolbox::ObjectQuery::Targets) {
            before_lki
                .iter()
                .filter_map(|lki| {
                    self.capture_target_lki(lki.object, &evaluated)
                        .map(|t| (lki.object, t))
                })
                .collect()
        } else {
            Vec::new()
        };

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
            if matches!(spec.source, crate::toolbox::ObjectQuery::Targets) {
                for (oid, tlki) in target_lkis {
                    cur.results.target_lki.insert(oid, tlki);
                }
                if matches!(spec.destination, ZoneDest::Exile { .. })
                    && let Some(first) = before_lki.first()
                    && let Some(ctrl) = first.controller
                {
                    cur.exiled_target = Some(crate::effects::DepartedTarget {
                        controller: ctrl,
                        power: first
                            .power_toughness
                            .as_ref()
                            .map(|pt| pt.derived.power.clone()),
                    });
                }
            }
            cur.results.last_action_lki = before_lki;
            cur.results.last_moved_controllers = before_controllers;
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
        if !shuffle_owners.is_empty() {
            let seats: std::collections::VecDeque<SeatId> = self
                .window
                .apnap_players()?
                .filter(|seat| shuffle_owners.contains(seat))
                .collect();
            if !seats.is_empty() {
                self.begin_library_shuffles(seats)?;
                return Ok(StepOutcome::Continue);
            }
        }

        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }
}
