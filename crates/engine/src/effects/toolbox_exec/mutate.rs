//! Execution engine for universal `Instruction::DrawCards` and `Instruction::MutateObject`.

use super::super::progress_rules::{RuleStepContext, StepOutcome};
use super::super::*;
use crate::{
    characteristics::{ResolvedPart, ResolvedParts},
    effects::CounterKind,
    occurrence::{EventCause, RulesOccurrence, StatusReason},
    stack_window::StackEntry,
    targets::TargetRef,
    toolbox::{
        DrawCardsSpec, ExecValue, MutateObjectSpec, ObjectMutation, PromptChoiceFrame,
        PromptChoiceReply, ResolvedPromptDomain, ZoneSelection,
    },
};
use std::collections::BTreeSet;

impl EffectWindow {
    pub(super) fn execute_draw_cards(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        spec: &DrawCardsSpec,
    ) -> Result<StepOutcome, EffectError> {
        let controller = entry.controller();
        let seats = self.eval_player_selector_at(
            controller,
            Some(entry),
            &spec.players,
            self.resolution_cursor(),
        )?;
        let source_id = self.entry_source_object(Some(entry));
        let amount = self.resolve_value_expr_with_source(
            controller,
            &spec.amount,
            self.resolution_cursor(),
            source_id,
        )?;
        let origin = DrawOrigin::Instruction {
            entry: entry.id(),
            instruction: index,
        };
        let mut drawn_ids = Vec::new();
        for seat in seats {
            if !amount.is_positive() {
                continue;
            }
            match self.draw_with_replacements(seat, amount.clone(), origin, ctx.append)? {
                Ok(completed) => {
                    for ev in &completed.events {
                        if let crate::draw::DrawEvent::Drawn { after, .. } = ev {
                            drawn_ids.push(after.id());
                        }
                    }
                    if self.won_by_effect.is_some() {
                        ctx.draws.append(completed);
                        return Ok(StepOutcome::Complete(RuleCompletion::GameEnded));
                    }
                    ctx.draws.append(completed);
                }
                Err(frame) => {
                    ctx.draws.append(self.take_draw_receipts());
                    return Ok(StepOutcome::Pending(frame));
                }
            }
        }
        if let Some(cur) = self.resolution_cursor_mut() {
            if let Some(bind) = spec.bind_drawn {
                cur.env.bind(bind, ExecValue::Objects(drawn_ids.clone()));
            }
            cur.results.last_action_succeeded = !drawn_ids.is_empty();
            cur.results.last_action_count = Some(RulesInteger::from_usize(drawn_ids.len()));
            cur.results.last_moved_objects = drawn_ids;
        }
        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }

    pub(super) fn execute_mutate_object(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        spec: &MutateObjectSpec,
    ) -> Result<StepOutcome, EffectError> {
        let controller = entry.controller();
        let source_id = self.entry_source_object(Some(entry));
        match &spec.op {
            ObjectMutation::SolveCase => {
                let src = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                };
                if let Some(id) = src
                    && self
                        .window
                        .cards()
                        .get(id)
                        .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                    && !self.characteristics.solved_cases.contains(&id)
                {
                    let before = self.rules_snapshot(None)?;
                    self.characteristics.solved_cases.insert(id);
                    self.emit_from(
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |_, _, _| Ok(vec![RulesOccurrence::CaseSolved { object: id }]),
                    )?;
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            ObjectMutation::SetPrepared => {
                let src = match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                };
                if let Some(id) = src
                    && self
                        .window
                        .cards()
                        .get(id)
                        .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                {
                    self.characteristics.prepared.insert(id);
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            ObjectMutation::AdvanceClassLevel(level) => {
                if let StackKind::ActivatedAbility { source } = entry.kind() {
                    let id = source.id();
                    if self
                        .window
                        .cards()
                        .get(id)
                        .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                    {
                        let before = self.rules_snapshot(None)?;
                        self.characteristics.class_levels.insert(id, *level);
                        self.emit_from(
                            EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            &before,
                            None,
                            ctx.append,
                            |_, _, _| {
                                Ok(vec![RulesOccurrence::ClassLevelReached {
                                    object: id,
                                    level: *level,
                                }])
                            },
                        )?;
                    }
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            ObjectMutation::ReselectAttackDefender => {
                if let Some(domain) = self.attack_defender_frame()? {
                    return Ok(StepOutcome::Pending(
                        EffectDecision::ReselectAttackDefender(domain),
                    ));
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            ObjectMutation::SoulbondPair { entrant_only } => {
                if let Some(frame) = self.soulbond_selection(*entrant_only)? {
                    return Ok(StepOutcome::Pending(EffectDecision::ResolutionSelect(
                        frame,
                    )));
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            ObjectMutation::DistributeCounters { kind, total } => {
                let assessment = self
                    .window
                    .target_assessment()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let division = match self.payloads.get(&entry.id()) {
                    Some(StackPayload::Instructions(bound)) => bound.target_division.clone(),
                    _ => std::collections::BTreeMap::new(),
                };
                let total_targets = entry.targets().targets().len();
                let mut amounts = Vec::new();
                for row in assessment.results() {
                    if row.legality != crate::targets::TargetLegality::Legal {
                        continue;
                    }
                    let TargetRef::Object(object) = row.choice.target else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let share = division.get(&row.choice.id).copied().unwrap_or_else(|| {
                        if total_targets <= 1 {
                            *total
                        } else {
                            let base = *total / u16::try_from(total_targets).unwrap_or(1);
                            let rem = *total % u16::try_from(total_targets).unwrap_or(1);
                            if row.choice.id == TargetId(0) {
                                base.saturating_add(rem)
                            } else {
                                base
                            }
                        }
                    });
                    if share > 0 {
                        amounts.push((object, i64::from(share).into()));
                    }
                }
                if !amounts.is_empty() {
                    self.place_divided_counters(
                        *kind,
                        amounts,
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        ctx.append,
                    )?;
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            ObjectMutation::Earthbend { amount } => {
                let target = match self.instruction_target(entry)?.choice().target {
                    TargetRef::Object(id) => id,
                    _ => return Err(EffectError::InconsistentProgram),
                };
                let n = self.resolve_value_expr(controller, amount)?;
                if self
                    .window
                    .cards()
                    .get(target)
                    .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                {
                    let before = self.rules_snapshot(None)?;
                    let parts = ResolvedParts(vec![
                        ResolvedPart::Type(crate::characteristics::TypeChange {
                            land_subtypes: crate::characteristics::SubtypeOperation::Keep,
                            supertypes: crate::characteristics::TypeOperation::Keep,
                            types: crate::characteristics::TypeOperation::Add(BTreeSet::from([
                                CardType::Creature,
                            ])),
                            subtypes: crate::characteristics::SubtypeOperation::Keep,
                        }),
                        ResolvedPart::Abilities(
                            crate::characteristics::AbilityChange::grant_keywords([
                                KeywordAbility::Haste,
                            ]),
                        ),
                        ResolvedPart::SetPT(crate::characteristics::PowerToughness {
                            power: 0.into(),
                            toughness: 0.into(),
                        }),
                    ]);
                    let effect = self.characteristics.add_effect(
                        entry.id(),
                        target,
                        parts,
                        crate::characteristics::ResolvedDuration::NoStatedDuration,
                    )?;
                    self.emit_from(
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |old, new, _| {
                            let appearances = occurrence_flow::characteristic_appearance_changes(
                                old,
                                new,
                                effect.affected.iter().copied(),
                            )?;
                            Ok(vec![RulesOccurrence::ContinuousEffectCreated {
                                effect,
                                appearances,
                            }])
                        },
                    )?;
                    if n.is_positive() {
                        let before_counters = self.rules_snapshot(None)?;
                        let raw = crate::characteristics::PowerToughnessCounters {
                            plus_one: n.clone(),
                            minus_one: 0.into(),
                        };
                        let counters = self.adjusted_pt_counters(
                            &before_counters.view().evaluated,
                            target,
                            raw,
                            None,
                        )?;
                        if counters.valid_placement() {
                            let previous = self.characteristics.counters(target);
                            self.characteristics.place_counters(target, &counters)?;
                            let after_pt = self.characteristics.counters(target);
                            self.emit_from(
                                EventCause::Instruction {
                                    entry: entry.id(),
                                    instruction: index,
                                },
                                &before_counters,
                                None,
                                ctx.append,
                                |_, _, _| {
                                    Ok(vec![RulesOccurrence::PermanentCounterChanged {
                                        object: target,
                                        kind: CounterKind::PlusOne,
                                        before: previous.plus_one,
                                        after: after_pt.plus_one,
                                    }])
                                },
                            )?;
                        }
                    }
                    let snapshot = self.rules_snapshot(None)?;
                    let source_appearance = match entry.kind() {
                        StackKind::ActivatedAbility { source }
                        | StackKind::TriggeredAbility { source } => snapshot
                            .view()
                            .appearance(source.id())
                            .or_else(|| self.departed_reflexive_sources.get(&source.id()).cloned())
                            .or_else(|| {
                                self.execution_bindings(entry.id())
                                    .and_then(|b| b.trigger.as_ref())
                                    .map(|t| t.source.clone())
                            }),
                        StackKind::CardSpell(id) => snapshot.view().appearance(*id),
                        _ => None,
                    }
                    .or_else(|| snapshot.view().appearance(target))
                    .ok_or(EffectError::InconsistentProgram)?;
                    self.triggers
                        .delay_earthbend_return(target, source_appearance, controller);
                }
                self.advance_instruction()?;
                return Ok(StepOutcome::Continue);
            }
            ObjectMutation::BecomeMonstrous { .. }
            | ObjectMutation::UnlockDoor { .. }
            | ObjectMutation::Enlist
            | ObjectMutation::Explore => {
                return Err(EffectError::InconsistentProgram);
            }
            _ => {}
        }
        let candidates = self.eval_object_query_at(
            controller,
            Some(entry),
            &spec.source,
            self.resolution_cursor(),
        )?;

        let pending_reply = self
            .resolution_cursor_mut()
            .and_then(|c| c.pending_prompt_reply.take());
        let selected: Vec<ObjectId> =
            if let Some(PromptChoiceReply::Objects(chosen)) = pending_reply {
                if let ZoneSelection::Prompt {
                    constraint: Some(constraint),
                    ..
                } = &spec.selection
                    && !self.satisfies_selection_constraint(&chosen, *constraint)?
                {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                chosen
            } else {
                match &spec.selection {
                    ZoneSelection::All => candidates,
                    ZoneSelection::Random { count, .. } => {
                        let n = self
                            .resolve_value_expr_with_source(
                                controller,
                                count,
                                self.resolution_cursor(),
                                source_id,
                            )?
                            .to_usize()
                            .unwrap_or(0);
                        candidates.into_iter().take(n).collect()
                    }
                    ZoneSelection::Prompt {
                        chooser,
                        min,
                        max,
                        constraint,
                        category,
                        hidden,
                    } => {
                        let chooser_seat = self
                            .eval_player_selector_at(
                                controller,
                                Some(entry),
                                chooser,
                                self.resolution_cursor(),
                            )?
                            .into_iter()
                            .next()
                            .unwrap_or(controller);
                        let min_n = self
                            .resolve_value_expr_with_source(
                                controller,
                                min,
                                self.resolution_cursor(),
                                source_id,
                            )?
                            .to_usize()
                            .unwrap_or(0);
                        let max_n = self
                            .resolve_value_expr_with_source(
                                controller,
                                max,
                                self.resolution_cursor(),
                                source_id,
                            )?
                            .to_usize()
                            .unwrap_or(0);
                        let clamped_max = max_n.min(candidates.len());
                        let clamped_min = min_n.min(clamped_max);
                        if candidates.is_empty() || clamped_max == 0 {
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
                            candidates
                        } else {
                            let frame = PromptChoiceFrame {
                                revision: self.revision,
                                entry: entry.id(),
                                instruction: index,
                                chooser: chooser_seat,
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
                        }
                    }
                }
            };

        let cause = EventCause::Instruction {
            entry: entry.id(),
            instruction: index,
        };
        let mut mutated = Vec::new();
        let mut succeeded = false;

        match &spec.op {
            ObjectMutation::SetTapped(tap) => {
                let changing: Vec<ObjectId> = selected
                    .into_iter()
                    .filter(|id| {
                        self.window.cards().get(*id).and_then(GameObject::tapped) == Some(!*tap)
                    })
                    .collect();
                if !changing.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let occurrences = if *tap {
                        let (window, changes) =
                            self.window.tap_for_effect(entry.id(), &changing)?;
                        self.window = window;
                        occurrence_flow::status_occurrences(changes, StatusReason::Effect)
                    } else {
                        self.untap_for_effect_with_stun(entry.id(), &changing)?
                    };
                    self.emit_from(cause, &before, None, ctx.append, |_, _, _| Ok(occurrences))?;
                    mutated = changing;
                    succeeded = true;
                }
            }
            ObjectMutation::ToggleTapped => {
                let to_tap: Vec<ObjectId> = selected
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.window.cards().get(*id).and_then(GameObject::tapped) == Some(false)
                    })
                    .collect();
                let to_untap: Vec<ObjectId> = selected
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.window.cards().get(*id).and_then(GameObject::tapped) == Some(true)
                    })
                    .collect();
                if !to_tap.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let (window, changes) = self.window.tap_for_effect(entry.id(), &to_tap)?;
                    self.window = window;
                    let occ = occurrence_flow::status_occurrences(changes, StatusReason::Effect);
                    self.emit_from(cause.clone(), &before, None, ctx.append, |_, _, _| Ok(occ))?;
                    mutated.extend(to_tap);
                }
                if !to_untap.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let occ = self.untap_for_effect_with_stun(entry.id(), &to_untap)?;
                    self.emit_from(cause, &before, None, ctx.append, |_, _, _| Ok(occ))?;
                    mutated.extend(to_untap);
                }
                succeeded = !mutated.is_empty();
            }
            ObjectMutation::PhaseOut => {
                if !selected.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let changed = self
                        .window
                        .phase_out(&selected, &self.characteristics.attachments)?;
                    if !changed.is_empty() {
                        if let Some(combat) = &mut self.combat {
                            combat.reconcile(|combatant, _| {
                                changed.binary_search(&combatant.object()).is_err()
                            });
                        }
                        mutated = changed.clone();
                        succeeded = true;
                        self.emit_from(cause, &before, None, ctx.append, |_, _, _| {
                            Ok(vec![RulesOccurrence::Phased {
                                objects: changed,
                                out: true,
                            }])
                        })?;
                    }
                }
            }
            ObjectMutation::Transform => {
                if !selected.is_empty() {
                    self.transform_objects(entry, index, selected.clone(), ctx.append)?;
                    mutated = selected;
                    succeeded = true;
                }
            }
            ObjectMutation::TurnFaceUp => {
                for id in selected {
                    let Some(card) = self.window.cards().get(id).cloned() else {
                        continue;
                    };
                    if card.zone() != ObjectZone::Battlefield || card.face_down().is_none() {
                        continue;
                    }
                    let underlying = self
                        .printed_face(&card)
                        .ok_or(EffectError::InconsistentProgram)?;
                    if underlying.has_type(CardType::Instant)
                        || underlying.has_type(CardType::Sorcery)
                    {
                        continue;
                    }
                    let before = self.rules_snapshot(None)?;
                    self.window.set_orientation(id, None)?;
                    mutated.push(id);
                    self.emit_from(cause.clone(), &before, None, ctx.append, |old, new, _| {
                        Ok(vec![RulesOccurrence::TurnedFaceUpByEffect {
                            before: Box::new(
                                old.appearance(id).ok_or(EffectError::InconsistentProgram)?,
                            ),
                            after: Box::new(
                                new.appearance(id).ok_or(EffectError::InconsistentProgram)?,
                            ),
                        }])
                    })?;
                }
                succeeded = !mutated.is_empty();
            }
            ObjectMutation::TurnFaceDown => {
                for id in selected {
                    let Some(card) = self.window.cards().get(id).cloned() else {
                        continue;
                    };
                    if card.zone() != ObjectZone::Battlefield
                        || card.face_down().is_some()
                        || card.is_token()
                    {
                        continue;
                    }
                    if card
                        .card_id()
                        .and_then(|cid| self.programs.get(&cid))
                        .is_some_and(|p| p.back_face().is_some())
                    {
                        continue;
                    }
                    self.window
                        .set_orientation(id, Some(crate::face_down::FaceDownKind::Morph))?;
                    mutated.push(id);
                }
                succeeded = !mutated.is_empty();
            }
            ObjectMutation::Regenerate => {
                if !selected.is_empty() {
                    self.create_regeneration_shields(
                        entry.id(),
                        index,
                        selected.clone(),
                        ctx.append,
                    )?;
                    mutated = selected;
                    succeeded = true;
                }
            }
            ObjectMutation::Goad { by } | ObjectMutation::GoadPermanent { by } => {
                let permanent = matches!(spec.op, ObjectMutation::GoadPermanent { .. });
                let goader = self
                    .eval_player_selector_at(controller, Some(entry), by, self.resolution_cursor())?
                    .into_iter()
                    .next()
                    .unwrap_or(controller);
                for id in selected {
                    if self
                        .window
                        .cards()
                        .get(id)
                        .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                    {
                        self.goaded.entry(id).or_default().insert(goader);
                        if permanent {
                            self.permanent_goaded.entry(id).or_default().insert(goader);
                        }
                        mutated.push(id);
                    }
                }
                succeeded = !mutated.is_empty();
            }
            ObjectMutation::Exert => {
                for id in selected {
                    if self
                        .window
                        .cards()
                        .get(id)
                        .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                    {
                        let before = self.rules_snapshot(None)?;
                        self.skip_next_controller_untap.insert(id);
                        self.characteristics.exerted_objects_this_turn.insert(id);
                        mutated.push(id);
                        self.emit_from(cause.clone(), &before, None, ctx.append, |_, _, _| {
                            Ok(vec![RulesOccurrence::KeywordActionPerformed {
                                player: controller,
                                action: crate::program::PlayerTriggerAction::Exerted,
                            }])
                        })?;
                    }
                }
                succeeded = !mutated.is_empty();
            }
            ObjectMutation::Attach { host } => {
                let host_opt = self
                    .eval_object_query_at(controller, Some(entry), host, self.resolution_cursor())?
                    .into_iter()
                    .next();
                if let Some(host_id) = host_opt {
                    let before = self.rules_snapshot(None)?;
                    let mut changes = Vec::new();
                    for att_id in selected {
                        if let Some(change) =
                            self.attach_if_legal(entry.id(), att_id, TargetRef::Object(host_id))?
                        {
                            mutated.push(att_id);
                            changes.push(change);
                        }
                    }
                    if !changes.is_empty() {
                        succeeded = true;
                        self.emit_from(cause, &before, None, ctx.append, |_, _, _| Ok(changes))?;
                    }
                }
            }
            ObjectMutation::AttachToPlayer { player } => {
                let seat_opt = self
                    .eval_player_selector_at(
                        controller,
                        Some(entry),
                        player,
                        self.resolution_cursor(),
                    )?
                    .into_iter()
                    .next();
                if let Some(seat) = seat_opt {
                    let before = self.rules_snapshot(None)?;
                    let mut changes = Vec::new();
                    for att_id in selected {
                        if let Some(change) =
                            self.attach_if_legal(entry.id(), att_id, TargetRef::Player(seat))?
                        {
                            mutated.push(att_id);
                            changes.push(change);
                        }
                    }
                    if !changes.is_empty() {
                        succeeded = true;
                        self.emit_from(cause, &before, None, ctx.append, |_, _, _| Ok(changes))?;
                    }
                }
            }
            ObjectMutation::Unattach => {
                if !selected.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let mut changes = Vec::new();
                    for obj_id in selected {
                        if self.window.cards().phased_out_under(obj_id).is_none()
                            && let Some(host) = self.characteristics.detach(obj_id)
                        {
                            mutated.push(obj_id);
                            changes.push(RulesOccurrence::AttachmentChanged {
                                source: obj_id,
                                before: Some(host),
                                after: None,
                            });
                        }
                    }
                    if !changes.is_empty() {
                        succeeded = true;
                        self.emit_from(cause, &before, None, ctx.append, |_, _, _| Ok(changes))?;
                    }
                }
            }
            ObjectMutation::AddCounters { kind, amount } => {
                let n = self.resolve_value_expr_with_source(
                    controller,
                    amount,
                    self.resolution_cursor(),
                    source_id,
                )?;
                if n.is_positive() && !selected.is_empty() {
                    let pairs: Vec<(ObjectId, RulesInteger)> =
                        selected.iter().map(|&id| (id, n.clone())).collect();
                    self.place_divided_counters(*kind, pairs, cause, ctx.append)?;
                    mutated = selected;
                    succeeded = true;
                }
            }
            ObjectMutation::RemoveCounters { kind, amount } => {
                let total = self.remove_counters_from_objects(
                    entry,
                    index,
                    &selected,
                    *kind,
                    Some(amount),
                    ctx.append,
                )?;
                if total.is_positive() {
                    mutated = selected;
                    succeeded = true;
                }
                if let Some(cur) = self.resolution_cursor_mut() {
                    cur.results.last_action_count = Some(total);
                    cur.results.last_action_succeeded = succeeded;
                }
            }
            ObjectMutation::ClearCounters { kind } => {
                let total = self.remove_counters_from_objects(
                    entry, index, &selected, *kind, None, ctx.append,
                )?;
                if total.is_positive() {
                    mutated = selected;
                    succeeded = true;
                }
            }
            ObjectMutation::DoubleCounters { kind } => {
                for id in selected {
                    let pt_c = self.characteristics.counters(id);
                    let kinds: Vec<CounterKind> = match kind {
                        Some(k) => vec![*k],
                        None => self
                            .characteristics
                            .present_permanent_counter_kinds(id)
                            .into_iter()
                            .collect(),
                    };
                    for k in kinds {
                        let current = match k {
                            CounterKind::PlusOne => pt_c.plus_one.clone(),
                            CounterKind::MinusOne => pt_c.minus_one.clone(),
                            other => self
                                .characteristics
                                .named_counters
                                .get(&other)
                                .and_then(|m| m.get(&id))
                                .cloned()
                                .unwrap_or_else(|| 0.into()),
                        };
                        if current.is_positive() {
                            self.place_divided_counters(
                                k,
                                vec![(id, current)],
                                cause.clone(),
                                ctx.append,
                            )?;
                            succeeded = true;
                        }
                    }
                    mutated.push(id);
                }
            }
            ObjectMutation::MoveCounters { to, kind, amount } => {
                let to_opt = self
                    .eval_object_query_at(controller, Some(entry), to, self.resolution_cursor())?
                    .into_iter()
                    .next();
                if let Some(to_id) = to_opt {
                    let cap = match amount {
                        Some(expr) => Some(self.resolve_value_expr_with_source(
                            controller,
                            expr,
                            self.resolution_cursor(),
                            source_id,
                        )?),
                        None => None,
                    };
                    for from_id in selected {
                        if from_id == to_id {
                            continue;
                        }
                        let kinds: Vec<CounterKind> = match kind {
                            Some(k) => vec![*k],
                            None => self
                                .characteristics
                                .present_permanent_counter_kinds(from_id)
                                .into_iter()
                                .collect(),
                        };
                        let mut moved_map = std::collections::BTreeMap::new();
                        for k in kinds {
                            let removed = self.remove_counters_from_objects(
                                entry,
                                index,
                                &[from_id],
                                Some(k),
                                cap.as_ref()
                                    .map(|c| crate::program::ValueExpr::literal(c.clone()))
                                    .as_ref(),
                                ctx.append,
                            )?;
                            if removed.is_positive() {
                                moved_map.insert(k, removed);
                            }
                        }
                        if !moved_map.is_empty() {
                            self.place_moved_counters(
                                entry.id(),
                                index,
                                to_id,
                                &moved_map,
                                ctx.append,
                            )?;
                            mutated.push(from_id);
                            succeeded = true;
                        }
                    }
                }
            }
            ObjectMutation::AddOneOfEachCounterKind => {
                for id in selected {
                    let kinds: Vec<CounterKind> = self
                        .characteristics
                        .present_permanent_counter_kinds(id)
                        .into_iter()
                        .collect();
                    for k in kinds {
                        self.place_divided_counters(
                            k,
                            vec![(id, RulesInteger::from(1))],
                            cause.clone(),
                            ctx.append,
                        )?;
                        succeeded = true;
                    }
                    mutated.push(id);
                }
            }
            ObjectMutation::TransferCountersFromTriggeringLki => {
                let lki_counters: std::collections::BTreeMap<CounterKind, RulesInteger> = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .and_then(|binding| {
                        binding
                            .causes
                            .members()
                            .first()
                            .and_then(|m| match &m.related {
                                crate::occurrence::TriggerRelated::ZoneChange {
                                    before, ..
                                } => before.battlefield_all_counters.clone(),
                                _ => None,
                            })
                            .or_else(|| binding.source.battlefield_all_counters.clone())
                    })
                    .unwrap_or_default();
                for id in selected {
                    if self
                        .window
                        .cards()
                        .get(id)
                        .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                    {
                        for (kind, count) in &lki_counters {
                            if count.is_positive() {
                                self.place_divided_counters(
                                    *kind,
                                    vec![(id, count.clone())],
                                    cause.clone(),
                                    ctx.append,
                                )?;
                                succeeded = true;
                            }
                        }
                        mutated.push(id);
                    }
                }
            }
            ObjectMutation::GrantFlashback => {
                let evaluated = self.evaluated_characteristics()?;
                for id in selected {
                    if evaluated.get(id).is_some_and(|card| {
                        card.zone == ObjectZone::Graveyard(controller)
                            && (card.has_type(CardType::Instant)
                                || card.has_type(CardType::Sorcery))
                    }) {
                        self.granted_flashback.insert(id);
                        mutated.push(id);
                        succeeded = true;
                    }
                }
            }
            ObjectMutation::GainControl {
                controller: new_ctrl,
                duration,
            } => {
                if let Some(resolved_dur) = self.resolve_duration(entry, *duration)? {
                    let seat = self
                        .eval_player_selector_at(
                            controller,
                            Some(entry),
                            new_ctrl,
                            self.resolution_cursor(),
                        )?
                        .into_iter()
                        .next()
                        .unwrap_or(controller);
                    let affected: BTreeSet<ObjectId> = selected.iter().copied().collect();
                    if !affected.is_empty() {
                        self.modify_objects_for_duration(
                            entry,
                            index,
                            affected,
                            ResolvedParts(vec![ResolvedPart::Control(seat)]),
                            resolved_dur,
                            ctx.append,
                        )?;
                        mutated = selected;
                        succeeded = true;
                    }
                }
            }
            ObjectMutation::SkipNextUntap => {
                if !selected.is_empty() {
                    self.skip_next_controller_untap
                        .extend(selected.iter().copied());
                    mutated = selected;
                    succeeded = true;
                }
            }
            ObjectMutation::RemoveFromCombat => {
                if !selected.is_empty() {
                    let affected: BTreeSet<ObjectId> = selected.iter().copied().collect();
                    if let Some(combat) = &mut self.combat {
                        combat.reconcile(|combatant, _| !affected.contains(&combatant.object()));
                    }
                    mutated = selected;
                    succeeded = true;
                }
            }
            ObjectMutation::SolveCase
            | ObjectMutation::SetPrepared
            | ObjectMutation::BecomeMonstrous { .. }
            | ObjectMutation::UnlockDoor { .. }
            | ObjectMutation::Earthbend { .. }
            | ObjectMutation::DistributeCounters { .. }
            | ObjectMutation::Enlist
            | ObjectMutation::AdvanceClassLevel(_)
            | ObjectMutation::ReselectAttackDefender
            | ObjectMutation::SoulbondPair { .. }
            | ObjectMutation::Explore => unreachable!("handled before candidate selection"),
            _ => return Err(EffectError::InconsistentProgram),
        }

        if let Some(cur) = self.resolution_cursor_mut() {
            if let Some(bind) = spec.bind_mutated {
                cur.env.bind(bind, ExecValue::Objects(mutated.clone()));
            }
            if let Some(bind) = spec.bind_succeeded {
                cur.env.bind(bind, ExecValue::Bool(succeeded));
            }
            if !matches!(spec.op, ObjectMutation::RemoveCounters { .. }) {
                cur.results.last_action_succeeded = succeeded;
                cur.results.last_action_count = Some(RulesInteger::from_usize(mutated.len()));
            }
            cur.results.last_moved_objects = mutated;
        }

        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }
}
