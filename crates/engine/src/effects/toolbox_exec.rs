//! Execution engine for the Universal Rules Toolbox primitives.

mod eval;
mod life_damage;
mod mutate;
mod prompt;
mod stack_op;
mod zone_op;

use super::progress_rules::{RuleStepContext, StepOutcome};
use super::*;
use crate::{
    characteristics::StaticContinuousAbility,
    emblem::EmblemDefinition,
    occurrence::{EventCause, RulesOccurrence},
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
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        spec: &ApplyContinuousSpec,
    ) -> Result<StepOutcome, EffectError> {
        use crate::toolbox::ContinuousPayload;
        let controller = entry.controller();
        let source_object = self.entry_source_object(Some(entry));
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
            let definition = match &spec.payload {
                ContinuousPayload::Emblem(def) => (**def).clone(),
                ContinuousPayload::Static(rule) => {
                    let label = spec
                        .emblem_label
                        .clone()
                        .unwrap_or_else(|| "Emblem".to_owned());
                    EmblemDefinition::Authored {
                        label,
                        continuous: vec![(**rule).clone()],
                        triggered: Vec::new(),
                    }
                }
                _ => return Err(EffectError::InconsistentProgram),
            };
            let seats = self.eval_player_selector_at(
                controller,
                Some(entry),
                emblem_players,
                self.resolution_cursor(),
            )?;
            if !seats.is_empty() {
                let before = self.rules_snapshot(None)?;
                let mut created = Vec::new();
                for seat in seats {
                    let emblem = self
                        .characteristics
                        .create_emblem(seat, definition.clone())?;
                    created.push(crate::occurrence::RulesOccurrence::EmblemCreated { emblem });
                }
                self.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| Ok(created),
                )?;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }

        let locked_objects: Option<BTreeSet<ObjectId>> = match &spec.selector {
            Some(q) => Some(
                self.eval_object_query_at(controller, Some(entry), q, self.resolution_cursor())?
                    .into_iter()
                    .collect(),
            ),
            None => None,
        };

        match &spec.payload {
            ContinuousPayload::Static(rule) => {
                let Some(resolved_dur) = self.resolve_duration(entry, spec.duration)? else {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                };
                if matches!(rule.as_ref(), StaticContinuousAbility::Battlefield { .. }) {
                    self.characteristics.add_temporary_static_locked(
                        controller,
                        source_object,
                        resolved_dur,
                        (**rule).clone(),
                        locked_objects,
                    )?;
                } else {
                    let id = self.next_temp_rule_id;
                    self.next_temp_rule_id = self.next_temp_rule_id.saturating_add(1);
                    let rule_kind = if spec.one_shot {
                        TemporaryRuleKind::OneShotEventReplacement(rule.clone())
                    } else {
                        TemporaryRuleKind::TemporaryStatic(rule.clone())
                    };
                    let bound_target = self
                        .instruction_target(entry)
                        .ok()
                        .map(|t| t.choice().target);
                    let second_bound_target =
                        entry.targets().targets().get(1).map(|t| t.choice().target);
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
            }
            ContinuousPayload::TemporaryRule(rule_kind) => {
                self.install_temporary_rule(entry, index, (**rule_kind).clone(), spec.duration)?;
            }
            ContinuousPayload::ResolvedParts(parts_spec) => {
                let affected = locked_objects.unwrap_or_default();
                if !affected.is_empty() {
                    let locals = self
                        .resolution_state()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .locals()
                        .clone();
                    for obj_id in affected {
                        let Some(card) = self
                            .window
                            .cards()
                            .get(obj_id)
                            .filter(|c| c.zone() == ObjectZone::Battlefield)
                        else {
                            continue;
                        };
                        let owner = card.owner();
                        let parts = self.materialize_resolved_parts(
                            entry,
                            parts_spec,
                            &locals,
                            owner,
                            source_object,
                        )?;
                        self.modify_objects_for_duration(
                            entry,
                            index,
                            [obj_id].into(),
                            parts,
                            spec.duration,
                            ctx.append,
                        )?;
                    }
                }
            }
            ContinuousPayload::DynamicBoost {
                power,
                toughness,
                keywords,
                suppress_keywords,
            } => {
                let affected = locked_objects.unwrap_or_default();
                if !affected.is_empty() {
                    let dp = self.resolve_value_expr_with_source(
                        controller,
                        power,
                        self.resolution_cursor(),
                        source_object,
                    )?;
                    let dt = self.resolve_value_expr_with_source(
                        controller,
                        toughness,
                        self.resolution_cursor(),
                        source_object,
                    )?;
                    let mut parts = Vec::new();
                    if dp != RulesInteger::from(0) || dt != RulesInteger::from(0) {
                        parts.push(crate::characteristics::ResolvedPart::ModifyPT(
                            crate::characteristics::PowerToughness {
                                power: dp,
                                toughness: dt,
                            },
                        ));
                    }
                    if !keywords.is_empty() || !suppress_keywords.is_empty() {
                        parts.push(crate::characteristics::ResolvedPart::Abilities(
                            crate::characteristics::AbilityChange {
                                granted_keywords: keywords.clone(),
                                suppressed_keywords: suppress_keywords.clone(),
                                ..Default::default()
                            },
                        ));
                    }
                    if !parts.is_empty() {
                        self.modify_objects_for_duration(
                            entry,
                            index,
                            affected,
                            crate::characteristics::ResolvedParts(parts),
                            spec.duration,
                            ctx.append,
                        )?;
                    }
                }
            }
            ContinuousPayload::DoublePt { power, toughness } => {
                if let Some(resolved_dur) = self.resolve_duration(entry, spec.duration)? {
                    let affected = locked_objects.unwrap_or_default();
                    let evaluated = self.evaluated_characteristics()?;
                    for object in affected {
                        if let Some(pt) = evaluated
                            .get(object)
                            .filter(|o| o.zone == ObjectZone::Battlefield)
                            .and_then(|o| o.power_toughness.clone())
                        {
                            let dp = if *power { pt.derived.power } else { 0.into() };
                            let dt = if *toughness {
                                pt.derived.toughness
                            } else {
                                0.into()
                            };
                            self.modify_objects_for_duration(
                                entry,
                                index,
                                [object].into(),
                                crate::characteristics::ResolvedParts(vec![
                                    crate::characteristics::ResolvedPart::ModifyPT(
                                        crate::characteristics::PowerToughness {
                                            power: dp,
                                            toughness: dt,
                                        },
                                    ),
                                ]),
                                resolved_dur,
                                ctx.append,
                            )?;
                        }
                    }
                }
            }
            ContinuousPayload::CombatAction(kind) => {
                let affected: Vec<ObjectId> =
                    locked_objects.unwrap_or_default().into_iter().collect();
                self.apply_combat_action(entry, index, &affected, *kind, ctx.append)?;
            }
            ContinuousPayload::PlayPermission {
                player,
                cast_only,
                free,
                any_color,
            } => {
                let grantee = self
                    .eval_player_selector_at(
                        controller,
                        Some(entry),
                        player,
                        self.resolution_cursor(),
                    )?
                    .into_iter()
                    .next()
                    .unwrap_or(controller);
                let play_duration = match spec.duration {
                    crate::characteristics::ResolvedDuration::UntilEndOfTurn
                    | crate::characteristics::ResolvedDuration::UntilEndOfCombat => {
                        CardPlayDuration::ThroughTurn
                    }
                    crate::characteristics::ResolvedDuration::ThroughControllerNextTurn
                    | crate::characteristics::ResolvedDuration::UntilControllerNextTurn
                    | crate::characteristics::ResolvedDuration::ThroughNextTurn { .. }
                    | crate::characteristics::ResolvedDuration::UntilPlayerNextTurn { .. } => {
                        if self.window.active_player() == grantee {
                            CardPlayDuration::ThroughNextOwnTurnSkipCurrent(grantee)
                        } else {
                            CardPlayDuration::ThroughNextOwnTurn(grantee)
                        }
                    }
                    _ => CardPlayDuration::WhileInZone,
                };
                for obj in locked_objects.unwrap_or_default() {
                    let Some(card) = self.window.cards().get(obj) else {
                        continue;
                    };
                    if card.card_id().is_none() {
                        continue;
                    }
                    let zone = match card.zone() {
                        ObjectZone::Exile => CardPlayZone::Exile,
                        ObjectZone::Graveyard(_) => CardPlayZone::Graveyard,
                        ObjectZone::Hand(_) => CardPlayZone::Hand,
                        _ => continue,
                    };
                    self.card_play_permissions.insert(CardPlayPermission {
                        zone,
                        object: obj,
                        player: grantee,
                        duration: play_duration,
                        any_color: *any_color,
                        free: *free,
                        cast_only: *cast_only,
                    });
                }
            }
            ContinuousPayload::PreventDamage(prev_spec) => {
                return self.apply_turn_prevention_spec(ctx, entry, index, prev_spec);
            }
            ContinuousPayload::SkipTurns { player, count } => {
                let seats = self.eval_player_selector_at(
                    controller,
                    Some(entry),
                    player,
                    self.resolution_cursor(),
                )?;
                let n = self
                    .resolve_value_expr_with_source(
                        controller,
                        count,
                        self.resolution_cursor(),
                        source_object,
                    )?
                    .to_usize()
                    .unwrap_or(0);
                let n_u16 = u16::try_from(n).unwrap_or(u16::MAX);
                if n_u16 > 0 {
                    for seat in seats {
                        if self.window.participants().contains(&seat) {
                            let rem = self.skip_turns_remaining.entry(seat).or_insert(0);
                            *rem = rem.saturating_add(n_u16);
                        }
                    }
                }
            }
            ContinuousPayload::Emblem(def) => {
                self.resolve_create_emblem(entry, index, (**def).clone(), ctx.append)?;
            }
            ContinuousPayload::SkipStep { player, step } => {
                let seats = self.eval_player_selector_at(
                    controller,
                    Some(entry),
                    player,
                    self.resolution_cursor(),
                )?;
                for seat in seats {
                    if self.window.participants().contains(&seat) {
                        let rem = self.skip_steps_remaining.entry((seat, *step)).or_insert(0);
                        *rem = rem.saturating_add(1);
                    }
                }
            }
            ContinuousPayload::TargetPreventionShield { amount } => {
                let target = self.instruction_target(entry)?.choice().target;
                let protected = match target {
                    TargetRef::Object(id) => DamageEffectTarget::Permanent(id),
                    TargetRef::Player(seat) => DamageEffectTarget::Player(seat),
                    TargetRef::Stack(_) => return Err(EffectError::InconsistentProgram),
                };
                let before = self.rules_snapshot(None)?;
                let shield = self
                    .damage_state
                    .add_shield(entry.id(), protected, amount.clone())?;
                let created = DamageEffectCreation::Prevention {
                    shield,
                    instruction: index,
                };
                ctx.journal.created.push(created.clone());
                self.emit_from(
                    EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| Ok(vec![RulesOccurrence::DamageEffectCreated(created)]),
                )?;
            }
            ContinuousPayload::DamageMultiplier { factor } => {
                let target = self.instruction_target(entry)?.choice().target;
                let TargetRef::Object(obj) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                if !self
                    .window
                    .cards()
                    .get(obj)
                    .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let before = self.rules_snapshot(None)?;
                let multiplier = self.damage_state.add_multiplier(
                    entry.id(),
                    DamageEffectTarget::Permanent(obj),
                    factor.clone(),
                    DamageSourceFilter::Any,
                )?;
                let created = DamageEffectCreation::Multiplier {
                    multiplier,
                    instruction: index,
                };
                ctx.journal.created.push(created.clone());
                self.emit_from(
                    EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |_, _, _| Ok(vec![RulesOccurrence::DamageEffectCreated(created.clone())]),
                )?;
            }
            ContinuousPayload::BlockDeclarations(rule) => {
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), controller)?;
                if let Some(combat) = &mut self.combat {
                    match rule {
                        crate::program::BlockDeclarationRule::EachOpponentOneBlocker => {
                            combat.one_blocker_for.extend(opponents)
                        }
                        crate::program::BlockDeclarationRule::ControllerChooses => {
                            combat.blocking_chooser = Some(controller)
                        }
                    }
                }
            }
            ContinuousPayload::ChosenColorProtection { recipients } => {
                let Some(color) = self
                    .resolution_cursor_mut()
                    .and_then(|cursor| cursor.chosen_effect_color.take())
                else {
                    let frame = EffectDecision::ResolutionManaColor(ResolutionManaColorChoice {
                        revision: self.revision,
                        entry: entry.id(),
                        instruction: index,
                        chooser: controller,
                        colors: mana_domains::colored_mana(),
                        split: None,
                    });
                    return Ok(StepOutcome::Pending(frame));
                };
                let Some(resolved_dur) = self.resolve_duration(entry, spec.duration)? else {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                };
                use crate::characteristics::Color;
                let color = Color::from_mana(color).ok_or(EffectError::InconsistentProgram)?;
                let keyword = match color {
                    Color::White => KeywordAbility::ProtectionFromWhite,
                    Color::Blue => KeywordAbility::ProtectionFromBlue,
                    Color::Black => KeywordAbility::ProtectionFromBlack,
                    Color::Red => KeywordAbility::ProtectionFromRed,
                    Color::Green => KeywordAbility::ProtectionFromGreen,
                };
                let parts = crate::characteristics::ResolvedParts(vec![
                    crate::characteristics::ResolvedPart::Abilities(
                        crate::characteristics::AbilityChange::grant_keywords([keyword]),
                    ),
                ]);
                let affected: BTreeSet<ObjectId> = match recipients {
                    crate::program::ProtectionRecipients::TargetPermanent => {
                        match self.instruction_target(entry)?.choice().target {
                            TargetRef::Object(id)
                                if self
                                    .window
                                    .cards()
                                    .get(id)
                                    .is_some_and(|c| c.zone() == ObjectZone::Battlefield) =>
                            {
                                BTreeSet::from([id])
                            }
                            _ => BTreeSet::new(),
                        }
                    }
                    crate::program::ProtectionRecipients::AbilitySource => match source_object {
                        Some(id)
                            if self
                                .window
                                .cards()
                                .get(id)
                                .is_some_and(|c| c.zone() == ObjectZone::Battlefield) =>
                        {
                            BTreeSet::from([id])
                        }
                        _ => BTreeSet::new(),
                    },
                    crate::program::ProtectionRecipients::ControllerAndPermanents => {
                        let before = self.rules_snapshot(None)?;
                        let affected = self.effect_group(
                            entry,
                            crate::program::EffectSubject::Matching(
                                crate::program::ObjectFilter::any().controlled(),
                            ),
                        )?;
                        let effect = if affected.is_empty() {
                            None
                        } else {
                            Some(self.characteristics.add_effect_for_objects(
                                entry.id(),
                                affected,
                                parts,
                                resolved_dur,
                            )?)
                        };
                        self.characteristics
                            .player_color_protection
                            .entry(controller)
                            .or_default()
                            .insert(color);
                        self.emit_from(
                            EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            &before,
                            None,
                            ctx.append,
                            |old, current, _| {
                                let mut rows = Vec::new();
                                if let Some(effect) = effect {
                                    let appearances = characteristic_appearance_changes(
                                        old,
                                        current,
                                        effect.affected.iter().copied(),
                                    )?;
                                    rows.push(RulesOccurrence::ContinuousEffectCreated {
                                        effect,
                                        appearances,
                                    });
                                }
                                rows.push(RulesOccurrence::PlayerProtectionGranted {
                                    player: controller,
                                    color,
                                });
                                Ok(rows)
                            },
                        )?;
                        self.advance_instruction()?;
                        return Ok(StepOutcome::Continue);
                    }
                    crate::program::ProtectionRecipients::Matching(filter) => {
                        let evaluated = self.evaluated_characteristics()?;
                        evaluated
                            .objects
                            .values()
                            .filter(|p| {
                                p.zone == ObjectZone::Battlefield
                                    && self.object_passes_filter(
                                        p.object,
                                        **filter,
                                        controller,
                                        source_object,
                                        &evaluated,
                                    )
                            })
                            .map(|p| p.object)
                            .collect()
                    }
                };
                if !affected.is_empty() {
                    self.modify_objects_for_duration(
                        entry,
                        index,
                        affected,
                        parts,
                        resolved_dur,
                        ctx.append,
                    )?;
                }
            }
            ContinuousPayload::ModifyAnyNumber { parts } => {
                let selected = if entry.targets().targets().is_empty() {
                    BTreeSet::new()
                } else {
                    self.legal_permanent_targets()?
                };
                if !selected.is_empty() {
                    let locals = self
                        .resolution_state()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .locals()
                        .clone();
                    let materialized = self.materialize_resolved_parts(
                        entry,
                        parts,
                        &locals,
                        controller,
                        source_object,
                    )?;
                    let before = self.rules_snapshot(None)?;
                    let effect = self.characteristics.add_effect_for_objects(
                        entry.id(),
                        selected,
                        materialized,
                        spec.duration,
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
                            let appearances = characteristic_appearance_changes(
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
                }
            }
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
                    if binding == crate::toolbox::BindingId::LOOP_PLAYER {
                        cur.results.loop_player = match value.as_ref() {
                            ExecValue::Players(seats) => seats.first().copied(),
                            _ => None,
                        };
                    }
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
                let mut expanded =
                    Vec::with_capacity(seats.len() * (1 + body.instructions.len()) + 1);
                for seat in seats {
                    expanded.push(Instruction::BindEnv {
                        binding: bind_player,
                        value: Box::new(ExecValue::Players(vec![seat])),
                    });
                    expanded.extend(body.instructions.iter().cloned());
                }
                if bind_player == crate::toolbox::BindingId::LOOP_PLAYER {
                    expanded.push(Instruction::BindEnv {
                        binding: bind_player,
                        value: Box::new(ExecValue::Players(Vec::new())),
                    });
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
            Instruction::ApplyContinuous(spec) => {
                self.execute_apply_continuous(ctx, entry, index, &spec)
            }
            Instruction::DrawCards(spec) => self.execute_draw_cards(ctx, entry, index, &spec),
            Instruction::MutateObject(spec) if spec.is_pass_c_op() => {
                Ok(StepOutcome::Next(Instruction::MutateObject(spec)))
            }
            Instruction::MutateObject(spec) => self.execute_mutate_object(ctx, entry, index, &spec),
            Instruction::AdjustLife(spec) => self.execute_adjust_life(ctx, entry, index, &spec),
            Instruction::DealDamage(spec) => self.execute_deal_damage(ctx, entry, index, &spec),
            Instruction::MutatePlayer(spec) if spec.is_pass_c_or_selection_op() => {
                Ok(StepOutcome::Next(Instruction::MutatePlayer(spec)))
            }
            Instruction::MutatePlayer(spec) => self.execute_mutate_player(ctx, entry, index, &spec),
            Instruction::StackOp(spec) if !spec.requires_stack_target() => {
                self.execute_non_targeted_stack_op(ctx, entry, index, &spec)
            }
            other => Ok(StepOutcome::Next(other)),
        }
    }
}
