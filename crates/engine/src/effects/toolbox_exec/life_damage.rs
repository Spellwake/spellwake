//! Execution engine for `Instruction::AdjustLife`, `Instruction::DealDamage`,
//! and `Instruction::MutatePlayer`.

use super::{EffectError, EffectWindow, RuleStepContext, StepOutcome};
use crate::{
    damage::{DamageInput, DamageKind, DamageOrigin, DamageRecipient, DamageSource},
    effects::RuleCompletion,
    life::LifeAdjustment,
    object_identity::{GameObject, ObjectId, ObjectZone},
    occurrence::{EventCause, PlayerLossReason, RulesOccurrence},
    priority::SeatId,
    program::{AffectedPlayers, DamageAmountSpec, DamageRecipientsRef, ResolvingPlayer},
    stack_window::{StackEntry, StackKind},
    targets::TargetRef,
    toolbox::{
        AdjustLifeSpec, DamageSourceQuery, DamageTargetQuery, DealDamageSpec, ExecValue, LifeOp,
        MutatePlayerSpec, PlayerMutation,
    },
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};

impl EffectWindow {
    /// Execute `Instruction::AdjustLife`.
    pub(in crate::effects) fn execute_adjust_life(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        spec: &AdjustLifeSpec,
    ) -> Result<StepOutcome, EffectError> {
        let controller = entry.controller();
        let cursor_snap = self.resolution_cursor().cloned();
        let seats = self.eval_player_selector_at(
            controller,
            Some(entry),
            &spec.players,
            cursor_snap.as_ref(),
        )?;
        let mut total_changed: RulesInteger = 0.into();
        match &spec.op {
            LifeOp::Gain(expr) => {
                let amt = self.resolve_value_expr(controller, expr)?;
                if amt.is_positive() && !seats.is_empty() {
                    let adjustments: BTreeMap<SeatId, LifeAdjustment> = seats
                        .iter()
                        .copied()
                        .map(|p| (p, LifeAdjustment::Gain(amt.clone())))
                        .collect();
                    let count = RulesInteger::from_usize(adjustments.len());
                    self.adjust_players_at_cut(entry.id(), index, adjustments, ctx.append)?;
                    total_changed = amt.times(&count);
                }
            }
            LifeOp::Lose(expr) => {
                let amt = self.resolve_value_expr(controller, expr)?;
                if amt.is_positive() && !seats.is_empty() {
                    let adjustments: BTreeMap<SeatId, LifeAdjustment> = seats
                        .iter()
                        .copied()
                        .map(|p| (p, LifeAdjustment::Lose(amt.clone())))
                        .collect();
                    let count = RulesInteger::from_usize(adjustments.len());
                    self.adjust_players_at_cut(entry.id(), index, adjustments, ctx.append)?;
                    total_changed = amt.times(&count);
                }
            }
            LifeOp::Set(expr) => {
                let amt = self.resolve_value_expr(controller, expr)?;
                if !seats.is_empty() {
                    let adjustments: BTreeMap<SeatId, LifeAdjustment> = seats
                        .iter()
                        .copied()
                        .map(|p| (p, LifeAdjustment::Set(amt.clone())))
                        .collect();
                    self.adjust_players_at_cut(entry.id(), index, adjustments, ctx.append)?;
                    total_changed = amt;
                }
            }
            LifeOp::Pay(expr) => {
                let cost = self.resolve_value_expr(controller, expr)?.max(0.into());
                let evaluated = self.evaluated_characteristics()?;
                let mut all_paid = !seats.is_empty();
                for seat in seats {
                    if !cost.is_positive()
                        || (!evaluated.life_and_creature_costs_prohibited()
                            && self.life.get(seat).is_some_and(|total| *total >= cost))
                    {
                        if cost.is_positive() {
                            self.pay_life_at_cut(
                                seat,
                                cost.clone(),
                                EventCause::Instruction {
                                    entry: entry.id(),
                                    instruction: index,
                                },
                                None,
                                ctx.append,
                            )?;
                            total_changed = total_changed.plus(&cost);
                        }
                    } else {
                        all_paid = false;
                    }
                }
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = all_paid;
                }
            }
            LifeOp::Drain {
                amount,
                gain_to,
                per_loss,
            } => {
                let amt = self.resolve_value_expr(controller, amount)?;
                let gainers = self.eval_player_selector_at(
                    controller,
                    Some(entry),
                    gain_to,
                    cursor_snap.as_ref(),
                )?;
                if amt.is_positive() {
                    let before = self.rules_snapshot(None)?;
                    let prohibited = self.life_gain_prohibitions(&before.view().evaluated)?;
                    let mut components: Vec<(
                        SeatId,
                        crate::life::LifeChangeKind,
                        Option<crate::life::LifelinkOrigin>,
                        RulesInteger,
                    )> = Vec::new();
                    let mut total_lost: RulesInteger = 0.into();
                    for seat in &seats {
                        if self.window.participants().contains(seat) {
                            components.push((
                                *seat,
                                crate::life::LifeChangeKind::Loss,
                                None,
                                amt.clone(),
                            ));
                            total_lost = total_lost.plus(&amt);
                        }
                    }
                    let gain_amt = if *per_loss {
                        total_lost.clone()
                    } else if !seats.is_empty() {
                        amt.clone()
                    } else {
                        0.into()
                    };
                    if gain_amt.is_positive() {
                        for gainer in gainers {
                            if self.window.participants().contains(&gainer)
                                && !prohibited.contains(&gainer)
                            {
                                components.push((
                                    gainer,
                                    crate::life::LifeChangeKind::Gain,
                                    None,
                                    gain_amt.clone(),
                                ));
                            }
                        }
                    }
                    if !components.is_empty() {
                        let changes = self
                            .life
                            .apply_life_components(components)
                            .map_err(|_| EffectError::InconsistentProgram)?;
                        self.emit_from(
                            EventCause::Instruction {
                                entry: entry.id(),
                                instruction: index,
                            },
                            &before,
                            None,
                            ctx.append,
                            |_, _, _| {
                                Ok(changes
                                    .into_iter()
                                    .map(RulesOccurrence::LifeChanged)
                                    .collect())
                            },
                        )?;
                    }
                    total_changed = total_lost;
                }
            }
            LifeOp::LoseHalf { round_up } => {
                let mut adjustments: BTreeMap<SeatId, LifeAdjustment> = BTreeMap::new();
                for seat in seats {
                    if let Some(current) = self.life.get(seat).cloned()
                        && current.is_positive()
                    {
                        let half = if *round_up {
                            current.nonnegative_half_rounded_up()
                        } else {
                            current.nonnegative_half_rounded_down()
                        };
                        if half.is_positive() {
                            total_changed = total_changed.plus(&half);
                            adjustments.insert(seat, LifeAdjustment::Lose(half));
                        }
                    }
                }
                if !adjustments.is_empty() {
                    self.adjust_players_at_cut(entry.id(), index, adjustments, ctx.append)?;
                }
            }
        }
        if let Some(binding) = spec.bind_amount
            && let Some(cursor) = self.resolution_cursor_mut()
        {
            cursor.env.bind(binding, ExecValue::Int(total_changed));
        }
        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }
}

impl EffectWindow {
    /// Execute `Instruction::DealDamage`.
    pub(in crate::effects) fn execute_deal_damage(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        spec: &DealDamageSpec,
    ) -> Result<StepOutcome, EffectError> {
        let controller = entry.controller();
        let cursor_snap = self.resolution_cursor().cloned();
        let current = self.evaluated_characteristics()?;
        let legal_target = |t: &&crate::targets::BoundTarget| {
            self.window.target_assessment().is_none_or(|assessment| {
                assessment
                    .get(t.choice().id)
                    .is_some_and(|row| row.legality == crate::targets::TargetLegality::Legal)
            })
        };

        let fixed_targets: Option<Vec<DamageRecipient>> = match &spec.recipients {
            DamageTargetQuery::Targets => {
                let mut out = Vec::new();
                if let Ok(t) = self.instruction_target(entry)
                    && legal_target(&t)
                {
                    match t.choice().target {
                        TargetRef::Player(p) => out.push(DamageRecipient::Player(p)),
                        TargetRef::Object(id) => {
                            if let Some(r) = current.get(id).and_then(|v| v.damage_recipient()) {
                                out.push(r);
                            }
                        }
                        TargetRef::Stack(_) => {}
                    }
                }
                Some(out)
            }
            DamageTargetQuery::Objects(q) => {
                let ids =
                    self.eval_object_query_at(controller, Some(entry), q, cursor_snap.as_ref())?;
                Some(
                    ids.into_iter()
                        .filter_map(|id| current.get(id).and_then(|v| v.damage_recipient()))
                        .collect(),
                )
            }
            DamageTargetQuery::Players(p) => {
                let seats =
                    self.eval_player_selector_at(controller, Some(entry), p, cursor_snap.as_ref())?;
                Some(seats.into_iter().map(DamageRecipient::Player).collect())
            }
            DamageTargetQuery::Both { objects, players } => {
                let mut out: Vec<DamageRecipient> = self
                    .eval_player_selector_at(
                        controller,
                        Some(entry),
                        players,
                        cursor_snap.as_ref(),
                    )?
                    .into_iter()
                    .map(DamageRecipient::Player)
                    .collect();
                let ids = self.eval_object_query_at(
                    controller,
                    Some(entry),
                    objects,
                    cursor_snap.as_ref(),
                )?;
                out.extend(
                    ids.into_iter()
                        .filter_map(|id| current.get(id).and_then(|v| v.damage_recipient())),
                );
                Some(out)
            }
            DamageTargetQuery::Relative(rel) => match rel.as_ref() {
                DamageRecipientsRef::Target => Some(
                    self.instruction_target(entry)
                        .ok()
                        .filter(legal_target)
                        .and_then(|t| match t.choice().target {
                            TargetRef::Player(p) => Some(DamageRecipient::Player(p)),
                            TargetRef::Object(id) => {
                                current.get(id).and_then(|v| v.damage_recipient())
                            }
                            TargetRef::Stack(_) => None,
                        })
                        .into_iter()
                        .collect(),
                ),
                DamageRecipientsRef::TargetController => Some(
                    self.instruction_target(entry)
                        .ok()
                        .filter(legal_target)
                        .and_then(|t| match t.choice().target {
                            TargetRef::Object(id) => current
                                .get(id)
                                .and_then(|v| v.controller)
                                .or_else(|| self.window.cards().get(id).map(GameObject::owner))
                                .map(DamageRecipient::Player),
                            TargetRef::Player(p) => Some(DamageRecipient::Player(p)),
                            TargetRef::Stack(_) => None,
                        })
                        .into_iter()
                        .collect(),
                ),
                DamageRecipientsRef::AbilitySource => Some(match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => current
                        .get(source.id())
                        .and_then(|v| v.damage_recipient())
                        .into_iter()
                        .collect(),
                    _ => Vec::new(),
                }),
                DamageRecipientsRef::TriggeringObject => Some(
                    self.execution_bindings(entry.id())
                        .and_then(|b| b.trigger.as_ref())
                        .map(|t| {
                            t.causes
                                .members()
                                .iter()
                                .filter_map(|m| m.subject_object_id())
                                .filter_map(|id| current.get(id).and_then(|v| v.damage_recipient()))
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                DamageRecipientsRef::AttachedHost => Some(match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => match self.attached_to(source.id())
                    {
                        Some(TargetRef::Object(host)) => current
                            .get(host)
                            .and_then(|v| v.damage_recipient())
                            .into_iter()
                            .collect(),
                        Some(TargetRef::Player(p)) => vec![DamageRecipient::Player(p)],
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                }),
                DamageRecipientsRef::Group(group) => {
                    Some(self.group_damage_recipients(*group, controller)?)
                }
                DamageRecipientsRef::LastMoved => Some(
                    self.resolution_cursor()
                        .map(|c| {
                            let ids: Vec<ObjectId> = if !c.results.last_moved_objects.is_empty() {
                                c.results.last_moved_objects.clone()
                            } else {
                                c.results.returned_target_permanent.into_iter().collect()
                            };
                            ids.into_iter()
                                .filter_map(|id| current.get(id).and_then(|v| v.damage_recipient()))
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                DamageRecipientsRef::CreatedTokens => Some(
                    self.resolution_cursor()
                        .map(|c| {
                            c.last_created_tokens
                                .iter()
                                .filter_map(|&id| {
                                    current.get(id).and_then(|v| v.damage_recipient())
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                DamageRecipientsRef::OtherCreaturesAndEachPlayer
                | DamageRecipientsRef::EachOtherPlayer
                | DamageRecipientsRef::EachOtherOpponent
                | DamageRecipientsRef::EachSourceToItself
                | DamageRecipientsRef::EachSourceController => None,
            },
        };

        let fixed_amount = match &spec.amount {
            DamageAmountSpec::Expr(expr) => Some(self.resolve_value_expr(controller, expr)?),
            DamageAmountSpec::SourcePower | DamageAmountSpec::SourceToughness => None,
        };

        let mut inputs = Vec::new();
        match &spec.source {
            DamageSourceQuery::EntrySource => {
                let dmg_source = self.resolution_damage_source(entry)?;
                let dmg_amount = match &spec.amount {
                    DamageAmountSpec::Expr(_) => fixed_amount.unwrap_or_else(|| 0.into()),
                    DamageAmountSpec::SourcePower => self
                        .entry_source_object(Some(entry))
                        .and_then(|id| current.get(id))
                        .and_then(|v| v.power_toughness.as_ref())
                        .map(|pt| pt.derived.power.clone())
                        .unwrap_or_else(|| 0.into()),
                    DamageAmountSpec::SourceToughness => self
                        .entry_source_object(Some(entry))
                        .and_then(|id| current.get(id))
                        .and_then(|v| v.power_toughness.as_ref())
                        .map(|pt| pt.derived.toughness.clone())
                        .unwrap_or_else(|| 0.into()),
                };
                if dmg_amount.is_positive()
                    && let Some(target_list) = &fixed_targets
                {
                    for recipient in target_list {
                        inputs.push(DamageInput {
                            source: dmg_source.clone(),
                            target: *recipient,
                            amount: dmg_amount.clone(),
                        });
                    }
                }
            }
            DamageSourceQuery::Objects(q) => {
                let source_ids =
                    self.eval_object_query_at(controller, Some(entry), q, cursor_snap.as_ref())?;
                for src_id in source_ids {
                    let src_view = current.get(src_id);
                    let lki = self
                        .resolution_cursor()
                        .and_then(|c| c.results.target_lki.get(&src_id));
                    let src_card = self.window.cards().get(src_id).cloned().or_else(|| {
                        lki.map(|info| {
                            GameObject::synthetic_emblem(info.controller.unwrap_or(info.owner))
                        })
                    });
                    let Some(src_card) = src_card else {
                        continue;
                    };
                    let src_controller = src_view
                        .and_then(|v| v.controller)
                        .or_else(|| lki.and_then(|info| info.controller.or(Some(info.owner))))
                        .unwrap_or(controller);
                    if !self.window.participants().contains(&src_controller) {
                        continue;
                    }
                    let dmg_amount = match &spec.amount {
                        DamageAmountSpec::Expr(_) => {
                            fixed_amount.clone().unwrap_or_else(|| 0.into())
                        }
                        DamageAmountSpec::SourcePower => src_view
                            .and_then(|v| v.power_toughness.as_ref())
                            .map(|pt| pt.derived.power.clone())
                            .or_else(|| lki.and_then(|info| info.power.clone()))
                            .unwrap_or_else(|| 0.into()),
                        DamageAmountSpec::SourceToughness => src_view
                            .and_then(|v| v.power_toughness.as_ref())
                            .map(|pt| pt.derived.toughness.clone())
                            .or_else(|| lki.and_then(|info| info.toughness.clone()))
                            .unwrap_or_else(|| 0.into()),
                    };
                    if !dmg_amount.is_positive() {
                        continue;
                    }
                    let dmg_source = DamageSource::Object {
                        object: src_card,
                        controller: src_controller,
                    };
                    if let Some(target_list) = &fixed_targets {
                        for recipient in target_list {
                            inputs.push(DamageInput {
                                source: dmg_source.clone(),
                                target: *recipient,
                                amount: dmg_amount.clone(),
                            });
                        }
                    } else if let DamageTargetQuery::Relative(rel) = &spec.recipients {
                        let per_source: Vec<DamageRecipient> = match rel.as_ref() {
                            DamageRecipientsRef::OtherCreaturesAndEachPlayer => {
                                let mut list: Vec<DamageRecipient> = self
                                    .window
                                    .participants()
                                    .iter()
                                    .copied()
                                    .map(DamageRecipient::Player)
                                    .collect();
                                list.extend(
                                    current
                                        .objects
                                        .values()
                                        .filter(|o| {
                                            o.zone == ObjectZone::Battlefield
                                                && o.is_creature()
                                                && o.object != src_id
                                        })
                                        .map(|o| DamageRecipient::Creature(o.object)),
                                );
                                list
                            }
                            DamageRecipientsRef::EachOtherPlayer => self
                                .window
                                .participants()
                                .iter()
                                .copied()
                                .filter(|&p| p != src_controller)
                                .map(DamageRecipient::Player)
                                .collect(),
                            DamageRecipientsRef::EachOtherOpponent => self
                                .combat_rules
                                .opponents(self.window.participants(), src_controller)?
                                .into_iter()
                                .map(DamageRecipient::Player)
                                .collect(),
                            DamageRecipientsRef::EachSourceToItself => current
                                .get(src_id)
                                .and_then(|v| v.damage_recipient())
                                .into_iter()
                                .collect(),
                            DamageRecipientsRef::EachSourceController => {
                                vec![DamageRecipient::Player(src_controller)]
                            }
                            _ => Vec::new(),
                        };
                        for recipient in per_source {
                            inputs.push(DamageInput {
                                source: dmg_source.clone(),
                                target: recipient,
                                amount: dmg_amount.clone(),
                            });
                        }
                    }
                }
            }
        }

        if inputs.is_empty() {
            if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = false;
            }
            self.advance_instruction()?;
            return Ok(StepOutcome::Continue);
        }

        if let Some(cursor) = self.resolution_cursor_mut() {
            cursor.results.last_action_succeeded = true;
            cursor.results.dealt_damage_this_resolution = true;
        }
        self.install_damage_batch(
            DamageOrigin::Resolution {
                entry: entry.id(),
                instruction: index,
            },
            DamageKind::Noncombat,
            inputs,
            ctx.journal,
        )?;
        Ok(StepOutcome::Continue)
    }
}

impl EffectWindow {
    /// Execute `Instruction::MutatePlayer`.
    pub(in crate::effects) fn execute_mutate_player(
        &mut self,
        ctx: &mut RuleStepContext<'_>,
        entry: &StackEntry,
        index: usize,
        spec: &MutatePlayerSpec,
    ) -> Result<StepOutcome, EffectError> {
        let controller = entry.controller();
        let cursor_snap = self.resolution_cursor().cloned();
        let seats = self.eval_player_selector_at(
            controller,
            Some(entry),
            &spec.players,
            cursor_snap.as_ref(),
        )?;
        match &spec.op {
            PlayerMutation::AddCounter { kind, amount } => {
                let resolved = self.resolve_value_expr(controller, amount)?;
                let source_id = match entry.kind() {
                    StackKind::CardSpell(id) => crate::damage::DamageSourceId::Object(*id),
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => {
                        crate::damage::DamageSourceId::Object(source.id())
                    }
                    StackKind::Emblem { emblem, .. } => {
                        crate::damage::DamageSourceId::Emblem(*emblem)
                    }
                    StackKind::SpellCopy { .. }
                    | StackKind::Speed
                    | StackKind::Monarch
                    | StackKind::DelayedCopy { .. } => {
                        crate::damage::DamageSourceId::SpellCopy(entry.id())
                    }
                };
                let before = self.rules_snapshot(None)?;
                let mut occurrences = Vec::new();
                for player in seats {
                    if let Some(occ) = self.give_player_counters(
                        player,
                        *kind,
                        &resolved,
                        entry.id(),
                        source_id,
                        controller,
                    )? {
                        occurrences.push(occ);
                    }
                }
                if !occurrences.is_empty() {
                    self.emit_from(
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |_, _, _| Ok(occurrences),
                    )?;
                }
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = resolved.is_positive();
                }
            }
            PlayerMutation::PayCounter { kind, amount } => {
                let cost = self.resolve_value_expr(controller, amount)?.max(0.into());
                let mut all_paid = !seats.is_empty();
                for actor in seats {
                    let before = self.player_counters_of_kind(actor, *kind);
                    if before >= cost {
                        if cost.is_positive() {
                            let after = before.minus(&cost);
                            let before_snapshot = self.rules_snapshot(None)?;
                            let map = self
                                .characteristics
                                .player_counters
                                .entry(*kind)
                                .or_default();
                            if after.is_positive() {
                                map.insert(actor, after.clone());
                            } else {
                                map.remove(&actor);
                            }
                            self.emit_from(
                                EventCause::Instruction {
                                    entry: entry.id(),
                                    instruction: index,
                                },
                                &before_snapshot,
                                None,
                                ctx.append,
                                |_, _, _| {
                                    Ok(vec![RulesOccurrence::PlayerCounterChanged {
                                        player: actor,
                                        kind: *kind,
                                        before: before.clone(),
                                        after: after.clone(),
                                    }])
                                },
                            )?;
                        }
                    } else {
                        all_paid = false;
                    }
                }
                if let Some(cursor) = self.resolution_cursor_mut() {
                    cursor.results.last_action_succeeded = all_paid;
                }
            }
            PlayerMutation::WinGame => {
                if let Some(&winner) = seats.first() {
                    let evaluated = self.evaluated_characteristics()?;
                    if self.window.participants().contains(&winner)
                        && !self.player_cannot_win(&evaluated, winner)?
                    {
                        let source = match entry.kind() {
                            StackKind::CardSpell(id) => *id,
                            StackKind::ActivatedAbility { source }
                            | StackKind::TriggeredAbility { source } => source.id(),
                            StackKind::SpellCopy { .. }
                            | StackKind::Emblem { .. }
                            | StackKind::DelayedCopy { .. }
                            | StackKind::Monarch
                            | StackKind::Speed => ObjectId::SENTINEL,
                        };
                        self.won_by_effect = Some((winner, source));
                        return Ok(StepOutcome::Complete(RuleCompletion::GameEnded));
                    }
                }
            }
            PlayerMutation::LoseGame => {
                let evaluated = self.evaluated_characteristics()?;
                let mut losing = BTreeSet::new();
                for seat in seats {
                    if self.window.participants().contains(&seat)
                        && !self.player_cannot_lose(&evaluated, seat, false)?
                    {
                        let loss_cause = EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        };
                        if !self
                            .try_replace_player_loss(&evaluated, seat, loss_cause, ctx.append)?
                        {
                            losing.insert(seat);
                        }
                    }
                }
                if !losing.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    let mut loss_occs = Vec::with_capacity(losing.len());
                    for &seat in &losing {
                        self.state_based_losses.insert(seat);
                        loss_occs.push(RulesOccurrence::PlayerLost {
                            player: seat,
                            reasons: [PlayerLossReason::Effect].into(),
                        });
                    }
                    self.emit_from(
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |_, _, _| Ok(loss_occs),
                    )?;
                }
            }
            PlayerMutation::DrawGame => {
                let participants = self.window.participants().to_vec();
                for seat in participants {
                    self.state_based_losses.insert(seat);
                }
                return Ok(StepOutcome::Complete(RuleCompletion::GameEnded));
            }
            PlayerMutation::TakeExtraTurn => {
                let before = self.rules_snapshot(None)?;
                let current = &before.view().evaluated;
                for player in seats {
                    let skip_extra_turn = current.objects.values().any(|perm| {
                        let Some(ctrl) = perm.controller else {
                            return false;
                        };
                        perm.zone == ObjectZone::Battlefield
                            && perm.has_battlefield_rule(|ab| {
                                matches!(
                                    ab,
                                    crate::characteristics::StaticContinuousAbility::EventReplacement {
                                        event: crate::characteristics::ReplaceableEvent::ExtraTurn {
                                            player: rel
                                        },
                                        action: crate::characteristics::StaticReplacementAction::Skip,
                                        ..
                                    } if match rel {
                                        AffectedPlayers::One(ResolvingPlayer::Controller) => player == ctrl,
                                        AffectedPlayers::EachOpponent | AffectedPlayers::EachOtherPlayer => player != ctrl,
                                        AffectedPlayers::EachPlayer | AffectedPlayers::One(_) => true,
                                    }
                                )
                            })
                    });
                    if !skip_extra_turn {
                        self.extra_turns.push(player);
                    }
                }
            }
            PlayerMutation::EndTurn => {
                if !seats.contains(&self.window.active_player()) {
                    self.advance_instruction()?;
                    return Ok(StepOutcome::Continue);
                }
                let before = self.rules_snapshot(None)?;
                self.triggers.clear_waiting_at_end_turn();
                let entries = self
                    .window
                    .entries()
                    .iter()
                    .map(|entry| entry.id())
                    .collect();
                let (window, exits) = self.window.exile_stack_to_end_turn(entry.id())?;
                self.window = window;
                self.emit_from(
                    EventCause::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &before,
                    None,
                    ctx.append,
                    |old, current, controllers| {
                        let mut events = vec![RulesOccurrence::StackExiledToEndTurn { entries }];
                        for exit in exits {
                            events.push(crate::effects::exit_occurrence_at(
                                old,
                                current,
                                exit,
                                crate::occurrence::ZoneReason::ExiledByEffect,
                                controllers,
                            )?);
                        }
                        Ok(events)
                    },
                )?;
                self.pending_work = None;
                self.prune_payloads();
                self.ending_turn = true;
                self.additional_combats = 0;
                return Ok(StepOutcome::Complete(RuleCompletion::Resolution));
            }
            PlayerMutation::BecomeMonarch => {
                if let Some(&seat) = seats.first() {
                    self.become_monarch(entry.id(), index, seat, ctx.append)?;
                }
            }
            PlayerMutation::Ascend => {
                for seat in seats {
                    if !self.has_city_blessing(seat) {
                        let evaluated = self.evaluated_characteristics()?;
                        let controlled_count = evaluated
                            .objects
                            .values()
                            .filter(|p| {
                                p.zone == ObjectZone::Battlefield && p.controller == Some(seat)
                            })
                            .take(10)
                            .count();
                        if controlled_count >= 10 {
                            let before = self.rules_snapshot(None)?;
                            self.characteristics.city_blessing.insert(seat);
                            self.emit_from(
                                EventCause::Instruction {
                                    entry: entry.id(),
                                    instruction: index,
                                },
                                &before,
                                None,
                                ctx.append,
                                |_, _, _| {
                                    Ok(vec![RulesOccurrence::CityBlessingGained { player: seat }])
                                },
                            )?;
                        }
                    }
                }
            }
            PlayerMutation::IncreaseSpeed => {
                self.resolve_speed(entry, index, ctx.append)?;
            }
            PlayerMutation::SetDayNight(clock) => {
                let previous = self.day_night();
                if previous != *clock {
                    let before = self.rules_snapshot(None)?;
                    let (_, faces) =
                        self.reconcile_day_night(Some(*clock), &before.view().evaluated)?;
                    self.transform_orders.clear();
                    self.emit_from(
                        EventCause::Instruction {
                            entry: entry.id(),
                            instruction: index,
                        },
                        &before,
                        None,
                        ctx.append,
                        |old, new, _| {
                            Self::day_change_occurrences(previous, *clock, &faces, old, new)
                        },
                    )?;
                }
            }
            PlayerMutation::DisallowDamagePreventionThisTurn => {
                self.damage_state.prevention_disallowed_this_turn = true;
            }
        }
        self.advance_instruction()?;
        Ok(StepOutcome::Continue)
    }
}
