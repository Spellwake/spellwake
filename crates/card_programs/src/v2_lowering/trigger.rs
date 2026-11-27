//! Strictly-typed lowering from `v2::ir::TriggerSpec` to `spellwake_engine::program::TriggeredAbility`.
//!
//! Every unhandled `v2::ir::TriggerEvent` variant or modifier fails closed with
//! `Err(LoweringError::Unsupported("v2:..."))`. No raw text inspection is performed.

use std::num::NonZeroU16;

use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    effects::CounterKind as EngineCounterKind,
    life::LifeChangeKind,
    program::{
        AttackTriggerMode, BlockTriggerKind, BodyTarget, CastEntryKind, Comparison,
        CounterTriggerKind as EngineCounterTriggerKind, DamageTriggerRecipient, DiesTriggerMode,
        EventValueKind, ExecutableBody, Instruction, ModalUniqueness, ObjectFilter, PlayerRelation,
        PlayerTriggerAction, ResolutionCondition, SpellFilter, StateThresholdKind,
        StatusTriggerKind, TriggerCondition, TriggerEvent as EngineTriggerEvent, TriggerGrouping,
        TriggerPlayerScope, TriggerSpec as EngineTriggerSpec, TriggerZone, TriggeredAbility,
        TriggeredModes, ValueExpr as EngineValueExpr, ValueZone,
    },
    toolbox::ConditionExpr as EngineConditionExpr,
};
use spellwake_oracle_compiler::{
    ast::{CardTypeMask as AstCardTypeMask, ControllerRelation, TurnStepAst},
    v2::ir as v2,
};

use crate::v2_lowering::{
    atoms::{lower_counter_kind, lower_step},
    condition::{lower_condition_expr, lower_resolution_condition},
    cost::{extract_modal_choice, lower_choice_count},
    ctx::LowerCtx,
    effect::lower_effect_body,
    error::{LowerResult, LoweringError},
    filter::{lower_object_filter, lower_spell_filter},
};

/// Lower a `v2::TriggerSpec` and effect body into one or more engine [`TriggeredAbility`] records.
///
/// Sets `ctx.in_trigger = true` while lowering the trigger body and restores the prior flag
/// afterward. Partitions inline `v2::Effect::Restriction` statements (`OnceEachTurn`, `OnlyIf`,
/// `DuringYourTurn`), lowers modal triggers (`PromptChoice::Modal`) into native [`TriggeredModes`],
/// and guards non-modal and modal bodies with any remaining intervening-if condition.
///
/// # Errors
/// Returns [`LoweringError::Unsupported`] if any trigger event, intervening-if condition,
/// inline restriction, or body effect has no faithful engine representation.
pub(crate) fn lower_triggered_ability(
    spec: &v2::TriggerSpec,
    body: &[v2::Effect],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Vec<TriggeredAbility>> {
    let mut inline_once_each_turn = false;
    let mut extra_conds: Vec<v2::ConditionExpr> = Vec::new();
    let mut exec_effects: Vec<v2::Effect> = Vec::with_capacity(body.len());
    for eff in body {
        if let v2::Effect::Restriction(rs) = eff {
            for r in rs {
                match r {
                    v2::ActivationRestriction::OnceEachTurn => {
                        inline_once_each_turn = true;
                    }
                    v2::ActivationRestriction::OnlyIf(cond) => {
                        extra_conds.push(cond.as_ref().clone());
                    }
                    v2::ActivationRestriction::DuringYourTurn => {
                        extra_conds.push(v2::ConditionExpr::Flag(v2::ConditionFlag::IsYourTurn));
                    }
                    _ => {
                        return Err(LoweringError::Unsupported("v2:trigger-inline-restriction"));
                    }
                }
            }
        } else {
            exec_effects.push(eff.clone());
        }
    }
    if exec_effects.is_empty() {
        return Err(LoweringError::Unsupported("v2:trigger-empty-body"));
    }

    let merged_spec_storage;
    let effective_spec = if extra_conds.is_empty() {
        spec
    } else {
        let mut merged_if = spec.intervening_if.clone();
        for cond in extra_conds {
            merged_if = Some(match merged_if {
                Some(existing) => v2::ConditionExpr::And(Box::new(existing), Box::new(cond)),
                None => cond,
            });
        }
        merged_spec_storage = v2::TriggerSpec {
            events: spec.events.clone(),
            intervening_if: merged_if,
        };
        &merged_spec_storage
    };

    let (specs, mut grouping, _world_cond) = lower_trigger_events(effective_spec, ctx)?;
    if inline_once_each_turn {
        grouping = merge_grouping(grouping, TriggerGrouping::OneOrMoreOnceEachTurn)?;
    }

    let (_, remaining_cond) = extract_first_time_each_turn(effective_spec.intervening_if.as_ref());
    let all_attacks = !effective_spec.events.is_empty()
        && effective_spec
            .events
            .iter()
            .all(|ev| matches!(ev, v2::TriggerEvent::Attacks { .. }));
    let (_, remaining_cond) = if all_attacks {
        extract_attacking_alone(remaining_cond)
    } else {
        (false, remaining_cond)
    };

    let prev_has_participant = ctx.trigger_has_participant;
    let prev_damage_event = ctx.trigger_damage_event;
    ctx.trigger_has_participant =
        !specs.is_empty() && specs.iter().all(|s| trigger_spec_has_participant(s, grouping));
    ctx.trigger_damage_event = infer_trigger_damage_event(&specs);
    let exec_body_res = lower_triggered_body(&exec_effects, remaining_cond, ctx);
    ctx.trigger_has_participant = prev_has_participant;
    ctx.trigger_damage_event = prev_damage_event;
    let exec_body = exec_body_res?;

    let mut out = Vec::with_capacity(specs.len());
    for cond_spec in specs {
        out.push(TriggeredAbility {
            id: ctx.alloc_triggered_id(),
            condition: cond_spec,
            grouping,
            body: exec_body.clone(),
        });
    }
    Ok(out)
}

/// Lower a triggered ability effect body (including modal triggered abilities into native
/// [`TriggeredModes`]) with `ctx.in_trigger = true` and an optional intervening-if guard.
pub(crate) fn lower_triggered_body(
    exec_effects: &[v2::Effect],
    remaining_cond: Option<&v2::ConditionExpr>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ExecutableBody> {
    let prev_in_trigger = ctx.in_trigger;
    ctx.in_trigger = true;
    let result = lower_triggered_body_inner(exec_effects, remaining_cond, ctx);
    ctx.in_trigger = prev_in_trigger;
    result
}

fn lower_triggered_body_inner(
    exec_effects: &[v2::Effect],
    remaining_cond: Option<&v2::ConditionExpr>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ExecutableBody> {
    let guard_cond = match remaining_cond {
        Some(cond) => Some(lower_condition_expr(cond, ctx)?),
        None => None,
    };

    if let Some((prefix_effs, choice_count, modes, suffix_effs)) =
        extract_modal_choice(exec_effects)
    {
        if modes.len() < 2 {
            return Err(LoweringError::Unsupported("v2:trigger:modal-too-few"));
        }
        let (min_modes, max_modes) = lower_choice_count(choice_count, modes.len())?;
        let prefix = if prefix_effs.is_empty() {
            None
        } else {
            let mut pb = lower_effect_body(prefix_effs, ctx)?;
            if pb.target != BodyTarget::None
                || pb.triggered_modes().is_some()
                || pb
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
            {
                return Err(LoweringError::Unsupported("v2:trigger:modal-prefix-target"));
            }
            if let Some(gc) = guard_cond.clone() {
                prepend_intervening_if_guard(&mut pb.instructions, gc);
            }
            Some(Box::new(pb))
        };
        let suffix = if suffix_effs.is_empty() {
            None
        } else {
            let mut sb = lower_effect_body(suffix_effs, ctx)?;
            if sb.target != BodyTarget::None
                || sb.triggered_modes().is_some()
                || sb
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
            {
                return Err(LoweringError::Unsupported("v2:trigger:modal-suffix-target"));
            }
            if let Some(gc) = guard_cond.clone() {
                prepend_intervening_if_guard(&mut sb.instructions, gc);
            }
            Some(Box::new(sb))
        };
        let mut options = Vec::with_capacity(modes.len());
        for m in modes {
            let mut mb = lower_effect_body(&m.body, ctx)?;
            if mb.instructions.is_empty() || mb.triggered_modes().is_some() {
                return Err(LoweringError::Unsupported("v2:trigger:modal-option-invalid"));
            }
            if let Some(gc) = guard_cond.clone() {
                prepend_intervening_if_guard(&mut mb.instructions, gc);
            }
            options.push(mb);
        }
        return Ok(ExecutableBody::with_triggered_modes(TriggeredModes {
            minimum: min_modes,
            maximum: max_modes,
            distinct_targets: false,
            allow_repeat: false,
            uniqueness: ModalUniqueness::None,
            prefix,
            suffix,
            options,
        }));
    }

    let mut exec_body = lower_effect_body(exec_effects, ctx)?;
    if let Some(gc) = guard_cond {
        prepend_intervening_if_guard(&mut exec_body.instructions, gc);
    }
    Ok(exec_body)
}

/// Lower a `v2::TriggerSpec` into one or more engine [`EngineTriggerSpec`] headers,
/// a [`TriggerGrouping`], and an optional world-evaluable [`ResolutionCondition`].
///
/// Multiple trigger specs are returned when a compound `v2::TriggerSpec` mixes incompatible
/// trigger zones (for example, `"When you cast or cycle ~"` uses `TriggerZone::Stack` and
/// `TriggerZone::Any`). Compatible multi-event specs are merged into `TriggerEvent::AnyOf`.
///
/// # Errors
/// Returns [`LoweringError::Unsupported`] when `spec.events` is empty or any event/condition
/// cannot be faithfully represented in the engine.
pub(crate) fn lower_trigger_events(
    spec: &v2::TriggerSpec,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(
    Vec<EngineTriggerSpec>,
    TriggerGrouping,
    Option<ResolutionCondition>,
)> {
    if spec.events.is_empty() {
        return Err(LoweringError::Unsupported("v2:trigger-empty-events"));
    }

    let (once_each_turn, remaining_cond) =
        extract_first_time_each_turn(spec.intervening_if.as_ref());
    let all_attacks = spec
        .events
        .iter()
        .all(|ev| matches!(ev, v2::TriggerEvent::Attacks { .. }));
    let (attacking_alone, remaining_cond) = if all_attacks {
        extract_attacking_alone(remaining_cond)
    } else {
        (false, remaining_cond)
    };

    let (trigger_cond, world_res_cond) = match remaining_cond {
        None => (None, None),
        Some(cond) => {
            let prev = ctx.in_trigger;
            ctx.in_trigger = true;
            let res = lower_resolution_condition(cond, ctx);
            let expr_check = if res.is_err() {
                lower_condition_expr(cond, ctx).map(|_| ())
            } else {
                Ok(())
            };
            ctx.in_trigger = prev;

            match res {
                Ok(rc) if rc.is_trigger_world_evaluable() => {
                    (Some(TriggerCondition::Eval(Box::new(rc.clone()))), Some(rc))
                }
                Ok(rc) => (None, Some(rc)),
                Err(_) => {
                    // Fail closed if the intervening-if cannot even lower to ConditionExpr.
                    expr_check?;
                    (None, None)
                }
            }
        }
    };

    let mut lowered_specs: Vec<EngineTriggerSpec> = Vec::new();
    let mut grouping = if once_each_turn {
        TriggerGrouping::OneOrMoreOnceEachTurn
    } else {
        TriggerGrouping::EachOccurrence
    };

    let mut idx = 0usize;
    while idx < spec.events.len() {
        // Fold adjacent `SpellCast` + `SpellCopied` with matching caster & spell filter into `include_copy: true`.
        if let (
            v2::TriggerEvent::SpellCast {
                caster,
                spell,
                from_outside_hand,
            },
            Some(v2::TriggerEvent::SpellCopied {
                caster: copy_caster,
                spell: copy_spell,
            }),
        ) = (&spec.events[idx], spec.events.get(idx + 1))
            && caster == copy_caster
            && (copy_spell.is_none() || copy_spell == spell)
        {
            let built = lower_spell_cast_event(*caster, spell.as_ref(), *from_outside_hand, true, ctx)?;
            lowered_specs.push(built);
            idx += 2;
            continue;
        }

        let (mut ev_specs, ev_grouping) =
            lower_single_trigger_event(&spec.events[idx], attacking_alone, ctx)?;
        grouping = merge_grouping(grouping, ev_grouping)?;
        lowered_specs.append(&mut ev_specs);
        idx += 1;
    }

    if lowered_specs.is_empty() {
        return Err(LoweringError::Unsupported("v2:trigger-empty-lowered"));
    }

    // Attach any world-evaluable intervening-if condition to each emitted TriggerSpec.
    if let Some(cond) = trigger_cond {
        for s in &mut lowered_specs {
            s.intervening_if = Some(match s.intervening_if.take() {
                Some(existing) => TriggerCondition::And(Box::new(existing), Box::new(cond.clone())),
                None => cond.clone(),
            });
        }
    }

    // Merge specs that share identical zone and intervening-if into a single AnyOf TriggerSpec.
    let merged = merge_compatible_trigger_specs(lowered_specs);
    // When lowering an event-driven delayed trigger (called with `ctx.in_trigger = true`),
    // propagate the damage-event kind so `that much` inside the delayed body binds accurately.
    if ctx.in_trigger
        && let Some(ev) = infer_trigger_damage_event(&merged)
    {
        ctx.trigger_damage_event = Some(ev);
    }
    Ok((merged, grouping, world_res_cond))
}

fn is_player_damage_recipient(recipient: DamageTriggerRecipient) -> bool {
    matches!(
        recipient,
        DamageTriggerRecipient::AnyPlayer
            | DamageTriggerRecipient::Opponent
            | DamageTriggerRecipient::Controller
            | DamageTriggerRecipient::EnchantedPlayer
    )
}

fn trigger_event_has_participant(event: &EngineTriggerEvent) -> bool {
    match event {
        EngineTriggerEvent::AnyOf(events) => {
            !events.is_empty() && events.iter().all(trigger_event_has_participant)
        }
        EngineTriggerEvent::DealsDamage { recipient, .. }
        | EngineTriggerEvent::DealsNoncombatDamage { recipient, .. }
        | EngineTriggerEvent::DealsAtLeastDamage { recipient, .. } => {
            is_player_damage_recipient(*recipient)
        }
        EngineTriggerEvent::SpellCast { .. }
        | EngineTriggerEvent::PlayerAction { .. }
        | EngineTriggerEvent::LifeChanged { .. }
        | EngineTriggerEvent::PlayerTargeted { .. }
        | EngineTriggerEvent::PlayerLosesGame(_)
        | EngineTriggerEvent::AbilityActivated { .. }
        | EngineTriggerEvent::StepBegan { .. }
        | EngineTriggerEvent::PutIntoGraveyard { .. }
        | EngineTriggerEvent::LeavesGraveyard { .. }
        | EngineTriggerEvent::Targeted { .. }
        | EngineTriggerEvent::TargetedByController { .. }
        | EngineTriggerEvent::ChoiceMade { .. } => true,
        EngineTriggerEvent::DealtDamage { linked_player, .. } => *linked_player,
        EngineTriggerEvent::Enters { .. }
        | EngineTriggerEvent::LeavesBattlefield { .. }
        | EngineTriggerEvent::LeavesBattlefieldWithoutDying { .. }
        | EngineTriggerEvent::Dies { .. }
        | EngineTriggerEvent::Exiled { .. }
        | EngineTriggerEvent::ReturnedToHand { .. }
        | EngineTriggerEvent::PutIntoLibrary { .. }
        | EngineTriggerEvent::Attacks { .. }
        | EngineTriggerEvent::BlocksOrBlocked { .. }
        | EngineTriggerEvent::CountersChanged { .. }
        | EngineTriggerEvent::StatusChanged { .. }
        | EngineTriggerEvent::DoorUnlocked { .. }
        | EngineTriggerEvent::EntersWithoutBeingPlayed { .. }
        | EngineTriggerEvent::BecomesAttached { .. }
        | EngineTriggerEvent::Exploits { .. }
        | EngineTriggerEvent::StateThreshold(_)
        | EngineTriggerEvent::Keyword(_)
        | EngineTriggerEvent::Planar(_)
        | EngineTriggerEvent::DayNightChanged
        | EngineTriggerEvent::SchemeSetInMotion
        | EngineTriggerEvent::ManualTabletopClaim { .. } => false,
    }
}

fn trigger_spec_has_participant(spec: &EngineTriggerSpec, grouping: TriggerGrouping) -> bool {
    if matches!(grouping, TriggerGrouping::OneOrMoreTogether) {
        return false;
    }
    trigger_event_has_participant(&spec.event)
}

fn trigger_event_damage_kind(event: &EngineTriggerEvent) -> Option<EventValueKind> {
    match event {
        EngineTriggerEvent::DealtDamage { .. } => Some(EventValueKind::DamageReceived),
        EngineTriggerEvent::DealsDamage { .. }
        | EngineTriggerEvent::DealsNoncombatDamage { .. }
        | EngineTriggerEvent::DealsAtLeastDamage { .. } => {
            Some(EventValueKind::CombatDamageToPlayer)
        }
        EngineTriggerEvent::AnyOf(events) => {
            let first = trigger_event_damage_kind(events.first()?)?;
            events
                .iter()
                .all(|e| trigger_event_damage_kind(e) == Some(first))
                .then_some(first)
        }
        _ => None,
    }
}

fn infer_trigger_damage_event(specs: &[EngineTriggerSpec]) -> Option<EventValueKind> {
    let first = trigger_event_damage_kind(&specs.first()?.event)?;
    specs
        .iter()
        .all(|s| trigger_event_damage_kind(&s.event) == Some(first))
        .then_some(first)
}

fn prepend_intervening_if_guard(instructions: &mut Vec<Instruction>, cond: EngineConditionExpr) {
    for ins in instructions.iter_mut() {
        ins.shift_jumps(1, 0);
    }
    let end_dest = instructions.len() + 1;
    instructions.insert(
        0,
        Instruction::BranchCondition {
            condition: Box::new(cond),
            destination: end_dest,
        },
    );
}

fn merge_grouping(
    current: TriggerGrouping,
    incoming: TriggerGrouping,
) -> LowerResult<TriggerGrouping> {
    match (current, incoming) {
        (a, b) if a == b => Ok(a),
        (TriggerGrouping::EachOccurrence, other) | (other, TriggerGrouping::EachOccurrence) => {
            Ok(other)
        }
        (TriggerGrouping::OneOrMoreOnceEachTurn, TriggerGrouping::OneOrMoreTogether)
        | (TriggerGrouping::OneOrMoreTogether, TriggerGrouping::OneOrMoreOnceEachTurn)
        | (TriggerGrouping::OneOrMoreOnceEachTurn, TriggerGrouping::OneOrMoreByPlayer)
        | (TriggerGrouping::OneOrMoreByPlayer, TriggerGrouping::OneOrMoreOnceEachTurn) => {
            Ok(TriggerGrouping::OneOrMoreOnceEachTurn)
        }
        _ => Err(LoweringError::Unsupported("v2:trigger-incompatible-grouping")),
    }
}

fn merge_compatible_trigger_specs(specs: Vec<EngineTriggerSpec>) -> Vec<EngineTriggerSpec> {
    if specs.len() <= 1 {
        return specs;
    }
    let first_zone = specs[0].zone;
    let first_if = &specs[0].intervening_if;
    let all_compatible = specs
        .iter()
        .all(|s| s.zone == first_zone && &s.intervening_if == first_if);
    if !all_compatible {
        return specs;
    }
    let intervening_if = specs[0].intervening_if.clone();
    let mut events = Vec::new();
    for s in specs {
        match s.event {
            EngineTriggerEvent::AnyOf(inner) => events.extend(inner),
            other => events.push(other),
        }
    }
    vec![EngineTriggerSpec {
        event: EngineTriggerEvent::AnyOf(events),
        zone: first_zone,
        intervening_if,
    }]
}

fn extract_first_time_each_turn(
    cond: Option<&v2::ConditionExpr>,
) -> (bool, Option<&v2::ConditionExpr>) {
    let Some(c) = cond else {
        return (false, None);
    };
    match c {
        v2::ConditionExpr::Flag(v2::ConditionFlag::FirstTimeEachTurn) => (true, None),
        v2::ConditionExpr::And(lhs, rhs) => {
            if matches!(
                lhs.as_ref(),
                v2::ConditionExpr::Flag(v2::ConditionFlag::FirstTimeEachTurn)
            ) {
                (true, Some(rhs.as_ref()))
            } else if matches!(
                rhs.as_ref(),
                v2::ConditionExpr::Flag(v2::ConditionFlag::FirstTimeEachTurn)
            ) {
                (true, Some(lhs.as_ref()))
            } else {
                (false, Some(c))
            }
        }
        _ => (false, Some(c)),
    }
}

fn extract_attacking_alone(cond: Option<&v2::ConditionExpr>) -> (bool, Option<&v2::ConditionExpr>) {
    let Some(c) = cond else {
        return (false, None);
    };
    match c {
        v2::ConditionExpr::SubjectState {
            state: v2::ObjectStateKind::AttackingAlone,
            ..
        } => (true, None),
        v2::ConditionExpr::And(lhs, rhs) => {
            if matches!(
                lhs.as_ref(),
                v2::ConditionExpr::SubjectState {
                    state: v2::ObjectStateKind::AttackingAlone,
                    ..
                }
            ) {
                (true, Some(rhs.as_ref()))
            } else if matches!(
                rhs.as_ref(),
                v2::ConditionExpr::SubjectState {
                    state: v2::ObjectStateKind::AttackingAlone,
                    ..
                }
            ) {
                (true, Some(lhs.as_ref()))
            } else {
                (false, Some(c))
            }
        }
        _ => (false, Some(c)),
    }
}

/// Lower one `v2::TriggerEvent` into one or more engine [`EngineTriggerSpec`] values.
fn lower_single_trigger_event(
    event: &v2::TriggerEvent,
    attacking_alone: bool,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    match event {
        v2::TriggerEvent::ZoneChange { from, to, subject } => {
            lower_zone_change_trigger(*from, *to, subject, ctx)
        }
        v2::TriggerEvent::Dies { subject } => lower_dies_trigger(subject, ctx),
        v2::TriggerEvent::SpellCast {
            caster,
            spell,
            from_outside_hand,
        } => {
            let spec =
                lower_spell_cast_event(*caster, spell.as_ref(), *from_outside_hand, false, ctx)?;
            Ok((vec![spec], TriggerGrouping::EachOccurrence))
        }
        v2::TriggerEvent::SpellCopied { caster, spell } => {
            if spell.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-spell-copied-filtered",
                ));
            }
            let rel = lower_trigger_player_relation(*caster)?;
            Ok((
                vec![EngineTriggerSpec::player_action(
                    rel,
                    PlayerTriggerAction::CopiedSpellOrAbility,
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::TriggerEvent::Attacks { subject, player } => {
            lower_attacks_trigger(subject.as_ref(), *player, attacking_alone, ctx)
        }
        v2::TriggerEvent::DealsDamage {
            combat_only,
            source,
            target,
        } => lower_deals_damage_trigger(*combat_only, source.as_ref(), target.as_ref(), ctx),
        v2::TriggerEvent::StepBegins { step, owner } => {
            let engine_step = lower_step(*step)?;
            let player_scope = lower_step_owner(*owner)?;
            Ok((
                vec![EngineTriggerSpec::new(EngineTriggerEvent::StepBegan {
                    step: engine_step,
                    player: player_scope,
                })],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::TriggerEvent::StepEnds { step, owner } => match step {
            TurnStepAst::EndOfCombat => {
                let engine_step = lower_step(*step)?;
                let player_scope = lower_step_owner(*owner)?;
                Ok((
                    vec![EngineTriggerSpec::new(EngineTriggerEvent::StepBegan {
                        step: engine_step,
                        player: player_scope,
                    })],
                    TriggerGrouping::EachOccurrence,
                ))
            }
            _ => Err(LoweringError::Unsupported("v2:trigger-step-ends")),
        },
        v2::TriggerEvent::PermanentSacrificed { player, subject } => {
            let rel = lower_trigger_player_relation(*player)?;
            let (filters, raw_grouping) = lower_trigger_subject_list(subject, ctx)?;
            let grouping = if raw_grouping == TriggerGrouping::OneOrMoreTogether {
                TriggerGrouping::OneOrMoreByPlayer
            } else {
                raw_grouping
            };
            let specs = filters
                .into_iter()
                .map(|f| EngineTriggerSpec::player_action(rel, PlayerTriggerAction::Sacrificed(f)))
                .collect();
            Ok((specs, grouping))
        }
        v2::TriggerEvent::LifeChanged { player, gained } => {
            let rel = lower_trigger_player_relation(*player)?;
            let kind = if *gained {
                LifeChangeKind::Gain
            } else {
                LifeChangeKind::Loss
            };
            Ok((
                vec![EngineTriggerSpec::life_changed(kind, rel)],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::TriggerEvent::CounterPlaced { subject, kind } => {
            let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
            let engine_kind = match kind {
                None | Some(v2::CounterKind::Any) => None,
                Some(k) => {
                    let ek = lower_counter_kind(k)?;
                    if !ek.is_permanent() {
                        return Err(LoweringError::Unsupported(
                            "v2:trigger-counter-placed-nonpermanent",
                        ));
                    }
                    Some(ek)
                }
            };
            let specs = filters
                .into_iter()
                .map(|f| {
                    EngineTriggerSpec::counters_changed(
                        f,
                        EngineCounterTriggerKind::CounterChanged {
                            kind: engine_kind,
                            added: true,
                            by_controller_only: false,
                        },
                    )
                })
                .collect();
            Ok((specs, grouping))
        }
        v2::TriggerEvent::CounterEvent {
            subject,
            kind,
            event: counter_ev,
        } => lower_counter_event_trigger(subject, kind, *counter_ev, ctx),
        v2::TriggerEvent::LastCounterRemoved { subject, kind } => {
            let ek = lower_counter_kind(kind)?;
            if matches!(subject, v2::SubjectRef::SelfObject) && ek == EngineCounterKind::Defense {
                Ok((
                    vec![EngineTriggerSpec::counters_changed(
                        ObjectFilter::self_object(),
                        EngineCounterTriggerKind::LastDefenseRemoved,
                    )],
                    TriggerGrouping::EachOccurrence,
                ))
            } else {
                Err(LoweringError::Unsupported(
                    "v2:trigger-last-counter-removed",
                ))
            }
        }
        v2::TriggerEvent::BecomesTarget { subject, source } => {
            lower_becomes_target_trigger(subject, source, ctx)
        }
        v2::TriggerEvent::Blocks { subject, blocked } => {
            let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
            let block_kind = match blocked {
                None => BlockTriggerKind::Blocks,
                Some(other) => {
                    let (other_flts, _) = lower_trigger_subject_list(other, ctx)?;
                    if other_flts.len() != 1 {
                        return Err(LoweringError::Unsupported(
                            "v2:trigger-blocks-compound-other",
                        ));
                    }
                    BlockTriggerKind::BlocksMatching(Box::new(other_flts[0]))
                }
            };
            let specs = filters
                .into_iter()
                .map(|f| EngineTriggerSpec::blocks_or_blocked(f, block_kind.clone()))
                .collect();
            Ok((specs, grouping))
        }
        v2::TriggerEvent::BecomesBlocked { subject, by } => {
            let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
            let block_kind = match by {
                None => BlockTriggerKind::BecomesBlocked {
                    capture_defender: false,
                },
                Some(other) => {
                    let (other_flts, _) = lower_trigger_subject_list(other, ctx)?;
                    if other_flts.len() != 1 {
                        return Err(LoweringError::Unsupported(
                            "v2:trigger-becomes-blocked-compound-by",
                        ));
                    }
                    BlockTriggerKind::BlockedByMatching(Box::new(other_flts[0]))
                }
            };
            let specs = filters
                .into_iter()
                .map(|f| EngineTriggerSpec::blocks_or_blocked(f, block_kind.clone()))
                .collect();
            Ok((specs, grouping))
        }
        v2::TriggerEvent::DealtDamage {
            subject,
            combat_only,
        } => {
            if *combat_only {
                if matches!(subject, v2::SubjectRef::SelfObject) {
                    return Ok((
                        vec![EngineTriggerSpec::deals_damage(
                            ObjectFilter::any(),
                            DamageTriggerRecipient::SelfObject,
                            true,
                        )],
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                return Err(LoweringError::Unsupported(
                    "v2:trigger-dealt-combat-damage-non-self",
                ));
            }
            let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
            let specs = filters
                .into_iter()
                .map(|f| {
                    EngineTriggerSpec::new(EngineTriggerEvent::DealtDamage {
                        subject: f,
                        linked_player: false,
                    })
                })
                .collect();
            Ok((specs, grouping))
        }
        v2::TriggerEvent::Cycled { player, self_card } => {
            let rel = lower_trigger_player_relation(*player)?;
            let mut spec = EngineTriggerSpec::player_action(
                rel,
                PlayerTriggerAction::Cycled {
                    self_only: *self_card,
                },
            );
            if *self_card {
                spec = spec.in_zone(TriggerZone::Any);
            }
            Ok((vec![spec], TriggerGrouping::EachOccurrence))
        }
        v2::TriggerEvent::NthCardDrawn { player, nth } => {
            let rel = lower_trigger_player_relation(*player)?;
            let nz = NonZeroU16::new(u16::from(*nth))
                .ok_or(LoweringError::Unsupported("v2:trigger-nth-draw-zero"))?;
            Ok((
                vec![EngineTriggerSpec::player_action(
                    rel,
                    PlayerTriggerAction::DrewCard {
                        nth_each_turn: Some(nz),
                    },
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::TriggerEvent::UnlockDoor {
            player,
            fully_unlocked,
        } => {
            let subject = if *fully_unlocked {
                match player {
                    v2::PlayerRef::You => ObjectFilter::any().controlled(),
                    v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                        ObjectFilter::any().opposing()
                    }
                    v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => ObjectFilter::any(),
                    _ => return Err(LoweringError::Unsupported("v2:trigger-unlock-door-player")),
                }
            } else {
                if *player != v2::PlayerRef::You {
                    return Err(LoweringError::Unsupported("v2:trigger-unlock-door-player"));
                }
                ObjectFilter::self_object()
            };
            Ok((
                vec![EngineTriggerSpec::new(EngineTriggerEvent::DoorUnlocked {
                    subject,
                    fully_unlocked_only: *fully_unlocked,
                    door: None,
                })],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::TriggerEvent::AbilityActivated {
            player,
            non_mana_only,
        } => {
            let rel = lower_trigger_player_relation(*player)?;
            let mut spec = EngineTriggerSpec::new(EngineTriggerEvent::AbilityActivated {
                player: rel,
                source_filter: ObjectFilter::any(),
            });
            if *non_mana_only {
                spec = spec.only_if(TriggerCondition::TriggeringAbilityNotMana);
            }
            Ok((vec![spec], TriggerGrouping::EachOccurrence))
        }
        v2::TriggerEvent::AttacksWithOthers { subject, min_count } => {
            if *min_count == 0 {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-attacks-with-others-zero",
                ));
            }
            let (filters, grouping) = match subject {
                None => (vec![ObjectFilter::self_object()], TriggerGrouping::EachOccurrence),
                Some(subj) => lower_trigger_subject_list(subj, ctx)?,
            };
            let specs = filters
                .into_iter()
                .map(|f| {
                    EngineTriggerSpec::attacks(f, AttackTriggerMode::WithAtLeastOthers(*min_count))
                })
                .collect();
            Ok((specs, grouping))
        }
        v2::TriggerEvent::PlayerAttacksWith {
            player,
            filter,
            min_count,
        } => {
            if *min_count == 0 {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-player-attacks-with-zero",
                ));
            }
            let mut ef = lower_object_filter(filter, ctx)?;
            ef = apply_player_controller_to_filter(ef, *player)?;
            Ok((
                vec![EngineTriggerSpec::attacks(
                    ef,
                    AttackTriggerMode::GroupAtLeast(*min_count),
                )],
                TriggerGrouping::OneOrMoreTogether,
            ))
        }
        v2::TriggerEvent::RolledDie { player, value } => {
            let rel = lower_trigger_player_relation(*player)?;
            let exact = (*value > 0).then_some(u16::from(*value));
            Ok((
                vec![EngineTriggerSpec::player_action(
                    rel,
                    PlayerTriggerAction::RolledDie { exact },
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::TriggerEvent::ClassBecomesLevel { level } => Ok((
            vec![EngineTriggerSpec::status_changed(
                ObjectFilter::self_object(),
                StatusTriggerKind::ClassLevelReached(*level),
            )],
            TriggerGrouping::EachOccurrence,
        )),
        v2::TriggerEvent::LoseControl { .. } => {
            Err(LoweringError::Unsupported("v2:trigger-lose-control"))
        }
        v2::TriggerEvent::Expend { player, amount } => {
            let rel = lower_trigger_player_relation(*player)?;
            Ok((
                vec![EngineTriggerSpec::player_action(
                    rel,
                    PlayerTriggerAction::Expend(u16::from(*amount)),
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::TriggerEvent::BecomesAttached { subject, target } => {
            if !target.is_empty() {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-becomes-attached-filtered-target",
                ));
            }
            let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
            let specs = filters
                .into_iter()
                .map(|f| {
                    EngineTriggerSpec::new(EngineTriggerEvent::BecomesAttached { subject: f })
                })
                .collect();
            Ok((specs, grouping))
        }
        v2::TriggerEvent::ForcedDiscard { player, subject } => {
            let rel = lower_trigger_player_relation(*player)?;
            let card_filter = match subject {
                v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f)
                    if is_any_card_filter(f) =>
                {
                    None
                }
                v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => {
                    Some(lower_object_filter(f, ctx)?)
                }
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:trigger-forced-discard-subject",
                    ));
                }
            };
            Ok((
                vec![EngineTriggerSpec::player_action(
                    rel,
                    PlayerTriggerAction::DiscardedByOpponent(card_filter),
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::TriggerEvent::PlayerAction { player, action } => {
            lower_player_action_trigger(*player, *action)
        }
        v2::TriggerEvent::SubjectState { subject, event: ev } => {
            lower_subject_state_trigger(subject, *ev, ctx)
        }
        v2::TriggerEvent::PlayerObjectEvent {
            player,
            kind,
            filter,
        } => lower_player_object_trigger(*player, *kind, filter.as_ref(), ctx),
        v2::TriggerEvent::Global(kind) => lower_global_trigger(*kind),
    }
}

fn lower_zone_change_trigger(
    from: Option<v2::ZoneKind>,
    to: Option<v2::ZoneKind>,
    subject: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
    let is_self_only = matches!(subject, v2::SubjectRef::SelfObject);
    let subject_owner_rel = infer_subject_owner_relation(subject);

    match (from, to) {
        (from_opt, Some(v2::ZoneKind::Battlefield)) => {
            let from_zone = lower_trigger_from_zone(from_opt)?;
            let cast_origin = infer_cast_entry_kind(subject)?;
            let specs = filters
                .into_iter()
                .map(|f| {
                    EngineTriggerSpec::new(EngineTriggerEvent::Enters {
                        subject: f,
                        from_zone,
                        cast_origin,
                        attached_to_creature: false,
                    })
                })
                .collect();
            Ok((specs, grouping))
        }
        (Some(v2::ZoneKind::Battlefield), None) => {
            let specs = filters
                .into_iter()
                .map(EngineTriggerSpec::leaves_battlefield)
                .collect();
            Ok((specs, grouping))
        }
        (Some(v2::ZoneKind::Battlefield), Some(v2::ZoneKind::Graveyard)) => {
            let specs = filters
                .into_iter()
                .map(|f| {
                    EngineTriggerSpec::new(EngineTriggerEvent::Dies {
                        subject: f,
                        mode: DiesTriggerMode::Permanent,
                    })
                })
                .collect();
            Ok((specs, grouping))
        }
        (Some(v2::ZoneKind::Battlefield), Some(v2::ZoneKind::Hand)) => {
            let specs = filters
                .into_iter()
                .map(|f| EngineTriggerSpec::new(EngineTriggerEvent::ReturnedToHand { subject: f }))
                .collect();
            Ok((specs, grouping))
        }
        (from_opt, Some(v2::ZoneKind::Exile)) => {
            let from_zone = lower_trigger_from_zone(from_opt)?;
            let specs = filters
                .into_iter()
                .map(|f| {
                    EngineTriggerSpec::new(EngineTriggerEvent::Exiled {
                        subject: f,
                        from_zone,
                    })
                })
                .collect();
            Ok((specs, grouping))
        }
        (Some(v2::ZoneKind::Graveyard), None) => {
            let player = subject_owner_rel.unwrap_or(PlayerRelation::AnyParticipant);
            let specs = filters
                .into_iter()
                .map(|f| {
                    EngineTriggerSpec::new(EngineTriggerEvent::LeavesGraveyard {
                        subject: f,
                        player,
                    })
                })
                .collect();
            Ok((specs, grouping))
        }
        (from_opt, Some(v2::ZoneKind::Graveyard)) => {
            let from_library = match from_opt {
                None => false,
                Some(v2::ZoneKind::Library) => true,
                Some(_) => {
                    return Err(LoweringError::Unsupported(
                        "v2:trigger-put-into-gy-from-zone",
                    ));
                }
            };
            let player = subject_owner_rel.unwrap_or(PlayerRelation::AnyParticipant);
            let specs = filters
                .into_iter()
                .map(|f| {
                    let mut spec = EngineTriggerSpec::new(EngineTriggerEvent::PutIntoGraveyard {
                        subject: f,
                        player,
                        from_library,
                        own_turn_only: false,
                    });
                    if is_self_only {
                        spec = spec.in_zone(TriggerZone::Any);
                    }
                    spec
                })
                .collect();
            Ok((specs, grouping))
        }
        (None, Some(v2::ZoneKind::Library)) => {
            let specs = filters
                .into_iter()
                .map(|f| EngineTriggerSpec::new(EngineTriggerEvent::PutIntoLibrary { subject: f }))
                .collect();
            Ok((specs, grouping))
        }
        _ => Err(LoweringError::Unsupported("v2:trigger-zone-change")),
    }
}

fn lower_dies_trigger(
    subject: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    // Detect "Whenever a creature dealt damage by ~ this turn dies".
    if let v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) = subject
        && f.predicates
            .contains(&v2::FilterPredicate::DealtDamageBySelfThisTurn)
    {
        let mut stripped = f.as_ref().clone();
        stripped
            .predicates
            .retain(|p| !matches!(p, v2::FilterPredicate::DealtDamageBySelfThisTurn));
        let ef = lower_object_filter(&stripped, ctx)?;
        let grouping = if matches!(subject, v2::SubjectRef::Group(_)) {
            TriggerGrouping::OneOrMoreTogether
        } else {
            TriggerGrouping::EachOccurrence
        };
        return Ok((
            vec![EngineTriggerSpec::new(EngineTriggerEvent::Dies {
                subject: ef,
                mode: DiesTriggerMode::DamagedBySource,
            })],
            grouping,
        ));
    }

    let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
    let mode = if subject_is_creature_or_self(subject) {
        DiesTriggerMode::Creature
    } else {
        DiesTriggerMode::Permanent
    };
    let specs = filters
        .into_iter()
        .map(|f| EngineTriggerSpec::new(EngineTriggerEvent::Dies { subject: f, mode }))
        .collect();
    Ok((specs, grouping))
}

fn lower_spell_cast_event(
    caster: v2::PlayerRef,
    spell: Option<&v2::ObjectFilter>,
    from_outside_hand: bool,
    include_copy: bool,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineTriggerSpec> {
    let scope = lower_trigger_player_scope(caster)?;
    if spell.is_none() && caster == v2::PlayerRef::You {
        let mut sf = SpellFilter::any();
        if from_outside_hand {
            sf = sf.not_from_owners_hand();
        }
        return Ok(EngineTriggerSpec::new(EngineTriggerEvent::SpellCast {
            caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
            filter: sf,
            self_cast: true,
            include_copy,
            min_mana_value: None,
        })
        .in_zone(TriggerZone::Stack));
    }
    let mut sf = match spell {
        Some(f) => lower_spell_filter(f, ctx)?,
        None => SpellFilter::any(),
    };
    if from_outside_hand {
        sf = sf.not_from_owners_hand();
    }
    Ok(EngineTriggerSpec::new(EngineTriggerEvent::SpellCast {
        caster: scope,
        filter: sf,
        self_cast: false,
        include_copy,
        min_mana_value: None,
    }))
}

fn lower_attacks_trigger(
    subject: Option<&v2::SubjectRef>,
    player: Option<v2::PlayerRef>,
    attacking_alone: bool,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    match (subject, player) {
        // "Whenever you attack" / "Whenever an opponent attacks"
        (None, Some(attacker_player)) => {
            let ef = apply_player_controller_to_filter(ObjectFilter::creature(), attacker_player)?;
            let mode = if attacking_alone {
                AttackTriggerMode::Alone
            } else {
                AttackTriggerMode::GroupAtLeast(1)
            };
            let grouping = if attacking_alone {
                TriggerGrouping::EachOccurrence
            } else {
                TriggerGrouping::OneOrMoreTogether
            };
            Ok((vec![EngineTriggerSpec::attacks(ef, mode)], grouping))
        }
        (None, None) => Err(LoweringError::Unsupported("v2:trigger-attacks-unbounded")),
        (Some(subj), defender_opt) => {
            let (filters, base_grouping) = lower_trigger_subject_list(subj, ctx)?;
            let mode = if attacking_alone {
                if defender_opt.is_some() {
                    return Err(LoweringError::Unsupported(
                        "v2:trigger-attacks-alone-with-defender",
                    ));
                }
                AttackTriggerMode::Alone
            } else {
                match defender_opt {
                    None => match base_grouping {
                        TriggerGrouping::OneOrMoreTogether => AttackTriggerMode::GroupAtLeast(1),
                        _ => AttackTriggerMode::Individual {
                            capture_defender: true,
                        },
                    },
                    Some(v2::PlayerRef::You) => AttackTriggerMode::AtController,
                    Some(v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent) => {
                        AttackTriggerMode::AtOpponent
                    }
                    Some(v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer) => {
                        AttackTriggerMode::AtPlayer
                    }
                    Some(v2::PlayerRef::EnchantedPlayer) => AttackTriggerMode::AtEnchantedPlayer,
                    Some(_) => {
                        return Err(LoweringError::Unsupported("v2:trigger-attacks-defender"));
                    }
                }
            };
            let specs = filters
                .into_iter()
                .map(|f| EngineTriggerSpec::attacks(f, mode))
                .collect();
            Ok((specs, base_grouping))
        }
    }
}

fn lower_deals_damage_trigger(
    combat_only: bool,
    source: Option<&v2::SubjectRef>,
    target: Option<&v2::SubjectRef>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    let (sources, raw_grouping) = match source {
        None => (vec![ObjectFilter::any()], TriggerGrouping::EachOccurrence),
        Some(subj) => lower_trigger_subject_list(subj, ctx)?,
    };
    let recipient = match target {
        None => DamageTriggerRecipient::Any,
        Some(tgt) => lower_damage_trigger_recipient(tgt)?,
    };
    let grouping =
        if raw_grouping == TriggerGrouping::OneOrMoreTogether && is_player_damage_recipient(recipient)
        {
            TriggerGrouping::OneOrMoreByPlayer
        } else {
            raw_grouping
        };
    let specs = sources
        .into_iter()
        .map(|src| EngineTriggerSpec::deals_damage(src, recipient, combat_only))
        .collect();
    Ok((specs, grouping))
}

fn lower_damage_trigger_recipient(target: &v2::SubjectRef) -> LowerResult<DamageTriggerRecipient> {
    match target {
        v2::SubjectRef::SelfObject => Ok(DamageTriggerRecipient::SelfObject),
        v2::SubjectRef::Player(p) => match p {
            v2::PlayerRef::You => Ok(DamageTriggerRecipient::Controller),
            v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                Ok(DamageTriggerRecipient::Opponent)
            }
            v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
                Ok(DamageTriggerRecipient::AnyPlayer)
            }
            v2::PlayerRef::EnchantedPlayer => Ok(DamageTriggerRecipient::EnchantedPlayer),
            _ => Err(LoweringError::Unsupported(
                "v2:trigger-deals-damage-recipient-player",
            )),
        },
        v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => {
            if !is_simple_type_only_filter(f) {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-deals-damage-filtered-recipient",
                ));
            }
            let types = f.all_types.union(f.any_types);
            if types == AstCardTypeMask::CREATURE {
                Ok(DamageTriggerRecipient::Creature)
            } else if types == AstCardTypeMask::PLANESWALKER {
                Ok(DamageTriggerRecipient::Planeswalker)
            } else if types == AstCardTypeMask::BATTLE {
                Ok(DamageTriggerRecipient::Battle)
            } else if types == AstCardTypeMask::PLANESWALKER.union(AstCardTypeMask::BATTLE) {
                Ok(DamageTriggerRecipient::PlaneswalkerOrBattle)
            } else {
                Err(LoweringError::Unsupported(
                    "v2:trigger-deals-damage-recipient-type",
                ))
            }
        }
        v2::SubjectRef::Compound(parts) => {
            let mut has_any_player = false;
            let mut has_opponent = false;
            let mut has_controller = false;
            let mut has_pw = false;
            let mut has_battle = false;
            for part in parts {
                match lower_damage_trigger_recipient(part)? {
                    DamageTriggerRecipient::AnyPlayer => has_any_player = true,
                    DamageTriggerRecipient::Opponent => has_opponent = true,
                    DamageTriggerRecipient::Controller => has_controller = true,
                    DamageTriggerRecipient::Planeswalker => has_pw = true,
                    DamageTriggerRecipient::Battle => has_battle = true,
                    DamageTriggerRecipient::PlaneswalkerOrBattle => {
                        has_pw = true;
                        has_battle = true;
                    }
                    _ => {
                        return Err(LoweringError::Unsupported(
                            "v2:trigger-deals-damage-compound-recipient",
                        ));
                    }
                }
            }
            match (
                has_any_player,
                has_opponent,
                has_controller,
                has_pw,
                has_battle,
            ) {
                (true, false, false, true, false) => {
                    Ok(DamageTriggerRecipient::PlayerOrPlaneswalker)
                }
                (false, true, false, true, false) => {
                    Ok(DamageTriggerRecipient::OpponentOrPlaneswalker)
                }
                (false, false, true, true, false) => {
                    Ok(DamageTriggerRecipient::ControllerOrPlaneswalker)
                }
                (true, false, false, false, true) => Ok(DamageTriggerRecipient::PlayerOrBattle),
                (false, true, false, false, true) => Ok(DamageTriggerRecipient::OpponentOrBattle),
                (true, false, false, true, true) => {
                    Ok(DamageTriggerRecipient::PlayerPlaneswalkerOrBattle)
                }
                (false, false, false, true, true) => {
                    Ok(DamageTriggerRecipient::PlaneswalkerOrBattle)
                }
                _ => Err(LoweringError::Unsupported(
                    "v2:trigger-deals-damage-compound-recipient",
                )),
            }
        }
        _ => Err(LoweringError::Unsupported(
            "v2:trigger-deals-damage-recipient",
        )),
    }
}

fn lower_counter_event_trigger(
    subject: &v2::SubjectRef,
    kind: &v2::CounterKind,
    event: v2::CounterTriggerKind,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    let ek = lower_counter_kind(kind)?;
    if !ek.is_permanent() {
        return Err(LoweringError::Unsupported(
            "v2:trigger-counter-event-nonpermanent",
        ));
    }
    match event {
        v2::CounterTriggerKind::Removed { while_exiled: false } => {
            let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
            let specs = filters
                .into_iter()
                .map(|f| {
                    EngineTriggerSpec::counters_changed(
                        f,
                        EngineCounterTriggerKind::CounterChanged {
                            kind: Some(ek),
                            added: false,
                            by_controller_only: false,
                        },
                    )
                })
                .collect();
            Ok((specs, grouping))
        }
        v2::CounterTriggerKind::Removed { while_exiled: true } => Err(LoweringError::Unsupported(
            "v2:trigger-counter-removed-exiled",
        )),
        v2::CounterTriggerKind::Threshold { min_count } => {
            if !matches!(subject, v2::SubjectRef::SelfObject) {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-counter-threshold-non-self",
                ));
            }
            let amount = u8::try_from(min_count)
                .map_err(|_| LoweringError::Unsupported("v2:trigger-counter-threshold-overflow"))?;
            Ok((
                vec![EngineTriggerSpec::state_threshold(
                    StateThresholdKind::SourceCounters {
                        kind: ek,
                        comparison: Comparison::GreaterOrEqual,
                        amount,
                    },
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::CounterTriggerKind::HasNone => {
            if !matches!(subject, v2::SubjectRef::SelfObject) {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-counter-has-none-non-self",
                ));
            }
            Ok((
                vec![EngineTriggerSpec::state_threshold(
                    StateThresholdKind::SourceCounters {
                        kind: ek,
                        comparison: Comparison::Equal,
                        amount: 0,
                    },
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::CounterTriggerKind::NthPut { .. } => {
            Err(LoweringError::Unsupported("v2:trigger-counter-nth-put"))
        }
    }
}

fn lower_becomes_target_trigger(
    subject: &v2::SubjectRef,
    source: &v2::ObjectFilter,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    // Reject extra source predicates that TriggerEvent::Targeted / PlayerTargeted cannot represent.
    if !source.forbidden_types.is_empty()
        || !source.required_supertypes.is_empty()
        || !source.forbidden_supertypes.is_empty()
        || !source.subtypes.is_empty()
        || !source.forbidden_subtypes.is_empty()
        || !source.any_colors.is_empty()
        || !source.forbidden_colors.is_empty()
        || source.color_arity.is_some()
        || source.owner.is_some()
        || !source.zones.is_empty()
        || source.combat.is_some()
        || source.tapped.is_some()
        || source.token.is_some()
        || !source.required_keywords.is_empty()
        || !source.forbidden_keywords.is_empty()
        || !source.stat_bounds.is_empty()
        || !source.flags.is_empty()
        || !source.predicates.is_empty()
    {
        return Err(LoweringError::Unsupported(
            "v2:trigger-becomes-target-source-filter",
        ));
    }

    let types = source.all_types.union(source.any_types);
    let instant_sorcery_mask = AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY);
    let (spell_only, instant_or_sorcery_only) = if types == instant_sorcery_mask {
        (true, true)
    } else if types.is_empty() {
        if source.any_spell && !source.is_ability {
            (true, false)
        } else if !source.any_spell && source.is_ability {
            return Err(LoweringError::Unsupported(
                "v2:trigger-becomes-target-ability-only",
            ));
        } else {
            (false, false)
        }
    } else {
        return Err(LoweringError::Unsupported(
            "v2:trigger-becomes-target-source-type",
        ));
    };

    let (by_controller, by_opponent) = match source.controller {
        None => (false, false),
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => (false, true),
        Some(ControllerRelation::You) => (true, false),
        Some(_) => {
            return Err(LoweringError::Unsupported(
                "v2:trigger-becomes-target-source-controller",
            ));
        }
    };

    if let v2::SubjectRef::Player(player) = subject {
        if by_controller {
            return Err(LoweringError::Unsupported(
                "v2:trigger-player-targeted-by-controller",
            ));
        }
        let rel = lower_trigger_player_relation(*player)?;
        return Ok((
            vec![EngineTriggerSpec::new(EngineTriggerEvent::PlayerTargeted {
                player: rel,
                by_opponent,
                spell_only,
                instant_or_sorcery_only,
            })],
            TriggerGrouping::EachOccurrence,
        ));
    }

    let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
    let specs = filters
        .into_iter()
        .map(|f| {
            let ev = if by_controller {
                EngineTriggerEvent::TargetedByController {
                    subject: f,
                    spell_only,
                    instant_or_sorcery_only,
                }
            } else {
                EngineTriggerEvent::Targeted {
                    subject: f,
                    by_opponent,
                    spell_only,
                    instant_or_sorcery_only,
                }
            };
            EngineTriggerSpec::new(ev)
        })
        .collect();
    Ok((specs, grouping))
}

fn lower_player_action_trigger(
    player: v2::PlayerRef,
    action: v2::PlayerTriggerKind,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    let rel = lower_trigger_player_relation(player)?;
    let spec = match action {
        v2::PlayerTriggerKind::CardDrawn => EngineTriggerSpec::player_action(
            rel,
            PlayerTriggerAction::DrewCard {
                nth_each_turn: None,
            },
        ),
        v2::PlayerTriggerKind::CardDiscarded => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::Discarded)
        }
        v2::PlayerTriggerKind::LandPlayed => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::LandPlayed)
        }
        v2::PlayerTriggerKind::CommitCrime => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::CommittedCrime)
        }
        v2::PlayerTriggerKind::ScryOrSurveil => EngineTriggerSpec::any_of(vec![
            EngineTriggerEvent::PlayerAction {
                player: rel,
                action: PlayerTriggerAction::Scried,
            },
            EngineTriggerEvent::PlayerAction {
                player: rel,
                action: PlayerTriggerAction::Surveilled,
            },
        ]),
        v2::PlayerTriggerKind::Scry => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::Scried)
        }
        v2::PlayerTriggerKind::Surveil => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::Surveilled)
        }
        v2::PlayerTriggerKind::ActivateExhaust => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::Exhausted)
        }
        v2::PlayerTriggerKind::CreateOrSacrificeToken => EngineTriggerSpec::any_of(vec![
            EngineTriggerEvent::PlayerAction {
                player: rel,
                action: PlayerTriggerAction::TokenCreated,
            },
            EngineTriggerEvent::PlayerAction {
                player: rel,
                action: PlayerTriggerAction::Sacrificed(ObjectFilter::any().token()),
            },
        ]),
        v2::PlayerTriggerKind::SearchesLibrary => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::SearchedLibrary)
        }
        v2::PlayerTriggerKind::ExertsCreature => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::Exerted)
        }
        v2::PlayerTriggerKind::CompletesDungeon => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::CompletedDungeon)
        }
        v2::PlayerTriggerKind::ShufflesLibrary => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::ShuffledLibrary)
        }
        v2::PlayerTriggerKind::LosesGame => {
            EngineTriggerSpec::new(EngineTriggerEvent::PlayerLosesGame(rel))
        }
        v2::PlayerTriggerKind::BecomesMonarch => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::BecameMonarch)
        }
        v2::PlayerTriggerKind::IsAttacked => {
            let mode = match rel {
                PlayerRelation::Controller => AttackTriggerMode::AtController,
                PlayerRelation::Opponent => AttackTriggerMode::AtOpponent,
                PlayerRelation::AnyParticipant => AttackTriggerMode::AtPlayer,
            };
            EngineTriggerSpec::attacks(ObjectFilter::creature(), mode)
        }
        v2::PlayerTriggerKind::WinClash => {
            EngineTriggerSpec::player_action(rel, PlayerTriggerAction::WonClash)
        }
        v2::PlayerTriggerKind::WinCoinFlip => EngineTriggerSpec::player_action(
            rel,
            PlayerTriggerAction::FlippedCoin { won: Some(true) },
        ),
    };
    Ok((vec![spec], TriggerGrouping::EachOccurrence))
}

fn lower_subject_state_trigger(
    subject: &v2::SubjectRef,
    event: v2::SubjectTriggerKind,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    let (filters, grouping) = lower_trigger_subject_list(subject, ctx)?;
    let mut specs = Vec::with_capacity(filters.len());
    for f in filters {
        let spec = match event {
            v2::SubjectTriggerKind::AttacksAndNotBlocked => {
                EngineTriggerSpec::blocks_or_blocked(f, BlockTriggerKind::Unblocked)
            }
            v2::SubjectTriggerKind::BecomesTapped => {
                EngineTriggerSpec::status_changed(f, StatusTriggerKind::BecomesTapped)
            }
            v2::SubjectTriggerKind::BecomesUntapped => {
                EngineTriggerSpec::status_changed(f, StatusTriggerKind::BecomesUntapped)
            }
            v2::SubjectTriggerKind::TurnedFaceUp => {
                EngineTriggerSpec::status_changed(f, StatusTriggerKind::TurnsFaceUp)
            }
            v2::SubjectTriggerKind::Transforms => {
                EngineTriggerSpec::status_changed(f, StatusTriggerKind::Transforms)
            }
            v2::SubjectTriggerKind::Exploits => {
                EngineTriggerSpec::new(EngineTriggerEvent::Exploits { subject: f })
            }
            v2::SubjectTriggerKind::BecomesMonstrous => {
                EngineTriggerSpec::status_changed(f, StatusTriggerKind::BecomesMonstrous)
            }
            v2::SubjectTriggerKind::BecomesCrewed => {
                EngineTriggerSpec::status_changed(f, StatusTriggerKind::Crewed)
            }
            v2::SubjectTriggerKind::BecomesUnattached => {
                return Err(LoweringError::Unsupported("v2:trigger-becomes-unattached"));
            }
            v2::SubjectTriggerKind::DiesOrHauntedDies => {
                return Err(LoweringError::Unsupported("v2:trigger-dies-or-haunted"));
            }
            v2::SubjectTriggerKind::Mutates => {
                EngineTriggerSpec::status_changed(f, StatusTriggerKind::Mutated)
            }
            v2::SubjectTriggerKind::CrewsVehicle => {
                return Err(LoweringError::Unsupported("v2:trigger-crews-vehicle"));
            }
            v2::SubjectTriggerKind::PhasesIn => {
                EngineTriggerSpec::status_changed(f, StatusTriggerKind::Phased { out: false })
            }
            v2::SubjectTriggerKind::PhasesOut => {
                EngineTriggerSpec::status_changed(f, StatusTriggerKind::Phased { out: true })
            }
        };
        specs.push(spec);
    }
    Ok((specs, grouping))
}

fn lower_player_object_trigger(
    player: v2::PlayerRef,
    kind: v2::PlayerObjectTriggerKind,
    filter: Option<&v2::ObjectFilter>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    let rel = lower_trigger_player_relation(player)?;
    match kind {
        v2::PlayerObjectTriggerKind::ControlNo => {
            if rel != PlayerRelation::Controller {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-control-no-non-controller",
                ));
            }
            let Some(f) = filter else {
                return Err(LoweringError::Unsupported("v2:trigger-control-no-unfiltered"));
            };
            let ef = lower_object_filter(f, ctx)?.controlled();
            let cond = ResolutionCondition {
                left: EngineValueExpr::Count {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(ef),
                    controller_scope: None,
                },
                comparison: Comparison::Equal,
                right: EngineValueExpr::Literal(RulesInteger::from(0)),
            };
            Ok((
                vec![EngineTriggerSpec::state_threshold(
                    StateThresholdKind::Eval(Box::new(cond)),
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::PlayerObjectTriggerKind::TappedForMana => {
            let ef = match filter {
                Some(f) => lower_object_filter(f, ctx)?,
                None => ObjectFilter::any(),
            };
            Ok((
                vec![EngineTriggerSpec::player_action(
                    rel,
                    PlayerTriggerAction::PermanentTappedForMana(ef),
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::PlayerObjectTriggerKind::CardDiscarded => {
            let action = match filter {
                Some(f) if !is_any_card_filter(f) => {
                    PlayerTriggerAction::DiscardedMatching(lower_object_filter(f, ctx)?)
                }
                _ => PlayerTriggerAction::Discarded,
            };
            Ok((
                vec![EngineTriggerSpec::player_action(rel, action)],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::PlayerObjectTriggerKind::CardPlayed => {
            let Some(f) = filter else {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-card-played-unfiltered",
                ));
            };
            let types = f.all_types.union(f.any_types);
            if types == AstCardTypeMask::LAND {
                let ef = lower_object_filter(f, ctx)?;
                Ok((
                    vec![EngineTriggerSpec::player_action(
                        rel,
                        PlayerTriggerAction::LandPlayedFiltered(ef),
                    )],
                    TriggerGrouping::EachOccurrence,
                ))
            } else {
                Err(LoweringError::Unsupported(
                    "v2:trigger-card-played-nonland",
                ))
            }
        }
        v2::PlayerObjectTriggerKind::TapOpponentCreature => Err(LoweringError::Unsupported(
            "v2:trigger-tap-opponent-creature",
        )),
        v2::PlayerObjectTriggerKind::CreatesTokens => {
            if filter.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:trigger-creates-tokens-filtered",
                ));
            }
            Ok((
                vec![EngineTriggerSpec::player_action(
                    rel,
                    PlayerTriggerAction::TokenCreated,
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
        v2::PlayerObjectTriggerKind::MillsCard => {
            let ef = match filter {
                Some(f) => lower_object_filter(f, ctx)?,
                None => ObjectFilter::any(),
            };
            Ok((
                vec![EngineTriggerSpec::new(
                    EngineTriggerEvent::PutIntoGraveyard {
                        subject: ef,
                        player: rel,
                        from_library: true,
                        own_turn_only: false,
                    },
                )],
                TriggerGrouping::EachOccurrence,
            ))
        }
    }
}

fn lower_global_trigger(
    kind: v2::GlobalTriggerKind,
) -> LowerResult<(Vec<EngineTriggerSpec>, TriggerGrouping)> {
    let spec = match kind {
        v2::GlobalTriggerKind::DayNightChanged => {
            EngineTriggerSpec::new(EngineTriggerEvent::DayNightChanged)
        }
        v2::GlobalTriggerKind::RingTemptsYou => EngineTriggerSpec::player_action(
            PlayerRelation::Controller,
            PlayerTriggerAction::RingTemptsYou,
        ),
        v2::GlobalTriggerKind::NoCreaturesOnBattlefield => {
            let cond = ResolutionCondition {
                left: EngineValueExpr::Count {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(ObjectFilter::creature()),
                    controller_scope: None,
                },
                comparison: Comparison::Equal,
                right: EngineValueExpr::Literal(RulesInteger::from(0)),
            };
            EngineTriggerSpec::state_threshold(StateThresholdKind::Eval(Box::new(cond)))
        }
    };
    Ok((vec![spec], TriggerGrouping::EachOccurrence))
}

fn lower_trigger_from_zone(zone: Option<v2::ZoneKind>) -> LowerResult<spellwake_engine::program::TriggerFromZone> {
    use spellwake_engine::program::TriggerFromZone;
    match zone {
        None => Ok(TriggerFromZone::Any),
        Some(v2::ZoneKind::Graveyard) => Ok(TriggerFromZone::Graveyard),
        Some(v2::ZoneKind::Hand) => Ok(TriggerFromZone::Hand),
        Some(v2::ZoneKind::Exile) => Ok(TriggerFromZone::Exile),
        Some(v2::ZoneKind::Library) => Ok(TriggerFromZone::Library),
        Some(v2::ZoneKind::Battlefield) => Ok(TriggerFromZone::Battlefield),
        Some(v2::ZoneKind::Stack | v2::ZoneKind::CommandZone | v2::ZoneKind::OutsideTheGame) => {
            Err(LoweringError::Unsupported("v2:trigger-from-zone"))
        }
    }
}

fn lower_trigger_player_relation(player: v2::PlayerRef) -> LowerResult<PlayerRelation> {
    match player {
        v2::PlayerRef::You => Ok(PlayerRelation::Controller),
        v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => Ok(PlayerRelation::Opponent),
        v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => Ok(PlayerRelation::AnyParticipant),
        _ => Err(LoweringError::Unsupported("v2:trigger-player-relation")),
    }
}

fn lower_trigger_player_scope(player: v2::PlayerRef) -> LowerResult<TriggerPlayerScope> {
    match player {
        v2::PlayerRef::You => Ok(TriggerPlayerScope::Relation(PlayerRelation::Controller)),
        v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
            Ok(TriggerPlayerScope::Relation(PlayerRelation::Opponent))
        }
        v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
            Ok(TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant))
        }
        v2::PlayerRef::EnchantedPlayer => Ok(TriggerPlayerScope::EnchantedPlayer),
        v2::PlayerRef::ItsController | v2::PlayerRef::ThatObjectController => {
            Ok(TriggerPlayerScope::AttachedHostController)
        }
        _ => Err(LoweringError::Unsupported("v2:trigger-player-scope")),
    }
}

fn lower_step_owner(owner: v2::StepOwner) -> LowerResult<TriggerPlayerScope> {
    match owner {
        v2::StepOwner::Your | v2::StepOwner::YourNext => {
            Ok(TriggerPlayerScope::Relation(PlayerRelation::Controller))
        }
        v2::StepOwner::Each
        | v2::StepOwner::EachPlayer
        | v2::StepOwner::Next
        | v2::StepOwner::NextTurn
        | v2::StepOwner::The
        | v2::StepOwner::ThatTurn
        | v2::StepOwner::ThatCombat => {
            Ok(TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant))
        }
        v2::StepOwner::EachOpponent => Ok(TriggerPlayerScope::Relation(PlayerRelation::Opponent)),
        v2::StepOwner::EnchantedController => Ok(TriggerPlayerScope::AttachedHostController),
        v2::StepOwner::ThatPlayer
        | v2::StepOwner::DefendingPlayer
        | v2::StepOwner::ChosenPlayer => Err(LoweringError::Unsupported("v2:trigger-step-owner")),
    }
}

fn apply_player_controller_to_filter(
    filter: ObjectFilter,
    player: v2::PlayerRef,
) -> LowerResult<ObjectFilter> {
    match player {
        v2::PlayerRef::You => Ok(filter.controlled()),
        v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => Ok(filter.opposing()),
        v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => Ok(filter),
        _ => Err(LoweringError::Unsupported(
            "v2:trigger-subject-controller-player",
        )),
    }
}

/// Lower a `v2::SubjectRef` occurring in trigger-event subject position into one or more
/// engine [`ObjectFilter`] values and a [`TriggerGrouping`].
fn lower_trigger_subject_list(
    subject: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Vec<ObjectFilter>, TriggerGrouping)> {
    match subject {
        v2::SubjectRef::SelfObject => {
            Ok((vec![ObjectFilter::self_object()], TriggerGrouping::EachOccurrence))
        }
        v2::SubjectRef::Anaphora(anaphora) => match anaphora {
            v2::AnaphoraRef::EnchantedCreature | v2::AnaphoraRef::EquippedCreature => Ok((
                vec![ObjectFilter::creature().attached_to_self()],
                TriggerGrouping::EachOccurrence,
            )),
            v2::AnaphoraRef::EnchantedPermanent => Ok((
                vec![ObjectFilter::any().attached_to_self()],
                TriggerGrouping::EachOccurrence,
            )),
            v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatObject
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatCard
            | v2::AnaphoraRef::ThatToken => {
                if ctx.has_object_target || ctx.in_trigger {
                    Ok((vec![ObjectFilter::any()], TriggerGrouping::EachOccurrence))
                } else {
                    Ok((
                        vec![ObjectFilter::self_object()],
                        TriggerGrouping::EachOccurrence,
                    ))
                }
            }
            v2::AnaphoraRef::ThatCreature => {
                if ctx.has_object_target || ctx.in_trigger {
                    Ok((
                        vec![ObjectFilter::creature()],
                        TriggerGrouping::EachOccurrence,
                    ))
                } else {
                    Err(LoweringError::Unsupported("v2:trigger-subject-anaphora"))
                }
            }
            v2::AnaphoraRef::ThatLand => {
                if ctx.has_object_target || ctx.in_trigger {
                    Ok((vec![ObjectFilter::land()], TriggerGrouping::EachOccurrence))
                } else {
                    Err(LoweringError::Unsupported("v2:trigger-subject-anaphora"))
                }
            }
            _ => Err(LoweringError::Unsupported("v2:trigger-subject-anaphora")),
        },
        v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) => {
            let ef = lower_object_filter(f, ctx)?;
            Ok((vec![ef], TriggerGrouping::EachOccurrence))
        }
        v2::SubjectRef::Group(f) => {
            let ef = lower_object_filter(f, ctx)?;
            Ok((vec![ef], TriggerGrouping::OneOrMoreTogether))
        }
        v2::SubjectRef::Counted { filter, count } => {
            let is_one_or_more = !count.up_to
                && matches!(count.count, v2::ValueExpr::Const(1));
            if !is_one_or_more {
                return Err(LoweringError::Unsupported("v2:trigger-counted-subject"));
            }
            let ef = lower_object_filter(filter, ctx)?;
            Ok((vec![ef], TriggerGrouping::OneOrMoreTogether))
        }
        v2::SubjectRef::Compound(parts) => {
            if parts.is_empty() {
                return Err(LoweringError::Unsupported("v2:trigger-empty-compound-subject"));
            }
            let mut out = Vec::new();
            let mut grouping = TriggerGrouping::EachOccurrence;
            for part in parts {
                let (mut sub_filters, sub_grouping) = lower_trigger_subject_list(part, ctx)?;
                grouping = merge_grouping(grouping, sub_grouping)?;
                out.append(&mut sub_filters);
            }
            Ok((out, grouping))
        }
        v2::SubjectRef::AnyTarget
        | v2::SubjectRef::TargetPlayer { .. }
        | v2::SubjectRef::TargetObject { .. }
        | v2::SubjectRef::Player(_)
        | v2::SubjectRef::BothPairedCreatures
        | v2::SubjectRef::HandOf(_)
        | v2::SubjectRef::OneOfThem(_)
        | v2::SubjectRef::PlayersDealtCombatDamageBy { .. }
        | v2::SubjectRef::PlayerWhoControls { .. } => {
            Err(LoweringError::Unsupported("v2:trigger-subject-kind"))
        }
    }
}

fn infer_cast_entry_kind(subject: &v2::SubjectRef) -> LowerResult<Option<CastEntryKind>> {
    match subject {
        v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => {
            if f.flags.contains(v2::FilterFlags::KICKED) {
                Ok(Some(CastEntryKind::Kicked))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

fn infer_subject_owner_relation(subject: &v2::SubjectRef) -> Option<PlayerRelation> {
    let filter = match subject {
        v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => f.as_ref(),
        _ => return None,
    };
    if let Some(owner) = filter.owner.or(filter.controller) {
        return match owner {
            ControllerRelation::You | ControllerRelation::OwnerYou => {
                Some(PlayerRelation::Controller)
            }
            ControllerRelation::Opponent
            | ControllerRelation::Opponents
            | ControllerRelation::OwnerOpponent => Some(PlayerRelation::Opponent),
            _ => None,
        };
    }
    if let Some(zr) = filter.zones.first() {
        return match zr.owner {
            v2::ZoneOwner::You => Some(PlayerRelation::Controller),
            v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents => Some(PlayerRelation::Opponent),
            v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::EachPlayer => {
                Some(PlayerRelation::AnyParticipant)
            }
            _ => None,
        };
    }
    None
}

fn subject_is_creature_or_self(subject: &v2::SubjectRef) -> bool {
    match subject {
        v2::SubjectRef::SelfObject
        | v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::EnchantedCreature
                | v2::AnaphoraRef::EquippedCreature
                | v2::AnaphoraRef::ThatCreature,
        ) => true,
        v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => {
            let types = f.all_types.union(f.any_types);
            types == AstCardTypeMask::CREATURE
        }
        v2::SubjectRef::Compound(parts) => parts.iter().all(subject_is_creature_or_self),
        _ => false,
    }
}

fn is_any_card_filter(f: &v2::ObjectFilter) -> bool {
    f.all_types.is_empty()
        && f.any_types.is_empty()
        && f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.controller.is_none()
        && f.owner.is_none()
        && f.zones.is_empty()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
        && f.predicates.is_empty()
        && !f.any_permanent
        && !f.any_spell
        && !f.is_ability
}

fn is_simple_type_only_filter(f: &v2::ObjectFilter) -> bool {
    f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.controller.is_none()
        && f.owner.is_none()
        && f.zones.is_empty()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
        && f.predicates.is_empty()
}
