//! Entry counters are installed on the new incarnation before its first battlefield observation.
use super::*;
use super::{occurrence_flow::RulesSnapshot, permanent_counters::controlled_counter_multiplier};
use crate::program::EntryCounterAbility;

/// A token template may need history from before the token was created.
pub(super) fn definition_or_token_matches(
    program: &Program,
    matches: fn(&Program) -> bool,
) -> bool {
    matches(program)
        || program
            .triggered
            .iter()
            .map(|a| &a.body)
            .chain(program.activated.iter().map(|a| &a.body))
            .chain(match &program.kind {
                ProgramKind::Spell(s) => Some(&s.body),
                ProgramKind::Permanent(_) => None,
            })
            .flat_map(|b| &b.instructions)
            .any(|op| match op {
                Instruction::MutateObject(spec) => match &spec.op {
                    crate::toolbox::ObjectMutation::CreateTokenAndAttach { definition }
                    | crate::toolbox::ObjectMutation::CreateAuraTokenAttached {
                        definition, ..
                    } => definition_or_token_matches(definition.program(), matches),
                    _ => false,
                },
                Instruction::CreateTokens { creations } => creations
                    .iter()
                    .any(|c| definition_or_token_matches(c.definition.program(), matches)),
                _ => false,
            })
}
pub(super) fn needs_damage_history(program: &Program) -> bool {
    definition_or_token_matches(program, |p| {
        p.entry_counter_abilities.iter().any(|a| {
            matches!(
                a,
                EntryCounterAbility::Bloodthirst(_) | EntryCounterAbility::BloodthirstX
            )
        })
    })
}

fn filter_uses_damage_pairs(filter: crate::program::ObjectFilter) -> bool {
    filter.has_flag(crate::program::ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN)
}

fn subject_uses_damage_pairs(subject: &crate::program::EffectSubject) -> bool {
    matches!(
        subject,
        crate::program::EffectSubject::Matching(filter)
            | crate::program::EffectSubject::TargetAndSameNameMatching(filter)
            if filter_uses_damage_pairs(*filter)
    )
}

fn value_expr_uses_damage_pairs(expr: &crate::program::ValueExpr) -> bool {
    use crate::program::{PlayerStatExpr, ValueExpr};
    match expr {
        ValueExpr::Count { filter, .. }
        | ValueExpr::OpponentMaxCount { filter, .. }
        | ValueExpr::OpponentMinCount { filter, .. }
        | ValueExpr::DistinctColors { filter }
        | ValueExpr::Aggregate { filter, .. }
        | ValueExpr::ObjectMatches { filter, .. } => filter_uses_damage_pairs(**filter),
        ValueExpr::PlayerStat(
            PlayerStatExpr::PlayerControlsMost { filter, .. }
            | PlayerStatExpr::AttackedMatchingThisTurn { filter, .. }
            | PlayerStatExpr::ControlsGreatestStat { filter, .. }
            | PlayerStatExpr::GraveyardArrivalsMatchingThisTurn { filter, .. }
            | PlayerStatExpr::SacrificedMatchingThisTurn { filter, .. },
        ) => filter_uses_damage_pairs(**filter),
        ValueExpr::Scale { expr, .. } | ValueExpr::DivConst { expr, .. } => {
            value_expr_uses_damage_pairs(expr)
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            value_expr_uses_damage_pairs(a) || value_expr_uses_damage_pairs(b)
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            value_expr_uses_damage_pairs(left)
                || value_expr_uses_damage_pairs(right)
                || value_expr_uses_damage_pairs(then_value)
                || value_expr_uses_damage_pairs(else_value)
        }
        _ => false,
    }
}

fn condition_uses_damage_pairs(cond: &crate::program::ResolutionCondition) -> bool {
    value_expr_uses_damage_pairs(&cond.left) || value_expr_uses_damage_pairs(&cond.right)
}

fn condition_expr_uses_damage_pairs(cond: &crate::toolbox::ConditionExpr) -> bool {
    match cond {
        crate::toolbox::ConditionExpr::Resolution(c) => condition_uses_damage_pairs(c),
        crate::toolbox::ConditionExpr::Compare { left, right, .. } => {
            value_expr_uses_damage_pairs(left) || value_expr_uses_damage_pairs(right)
        }
        crate::toolbox::ConditionExpr::And(cs) | crate::toolbox::ConditionExpr::Or(cs) => {
            cs.iter().any(condition_expr_uses_damage_pairs)
        }
        crate::toolbox::ConditionExpr::Not(c) => condition_expr_uses_damage_pairs(c),
        _ => false,
    }
}

fn static_cond_uses_damage_pairs(cond: &crate::characteristics::StaticSourceCondition) -> bool {
    use crate::characteristics::StaticSourceCondition;
    match cond {
        StaticSourceCondition::Eval(c) => condition_uses_damage_pairs(c),
        StaticSourceCondition::Not(inner) => static_cond_uses_damage_pairs(inner),
        StaticSourceCondition::And(a, b) | StaticSourceCondition::Or(a, b) => {
            static_cond_uses_damage_pairs(a) || static_cond_uses_damage_pairs(b)
        }
        _ => false,
    }
}

fn restriction_uses_damage_pairs(res: &crate::activated::ActivationRestriction) -> bool {
    use crate::activated::ActivationRestriction;
    match res {
        ActivationRestriction::Condition(c) => condition_uses_damage_pairs(c),
        ActivationRestriction::All(list) => list.iter().any(restriction_uses_damage_pairs),
        _ => false,
    }
}

fn trigger_cond_uses_damage_pairs(cond: &crate::program::TriggerCondition) -> bool {
    use crate::program::TriggerCondition;
    match cond {
        TriggerCondition::Eval(c) => condition_uses_damage_pairs(c),
        TriggerCondition::Not(inner) => trigger_cond_uses_damage_pairs(inner),
        TriggerCondition::And(a, b) | TriggerCondition::Or(a, b) => {
            trigger_cond_uses_damage_pairs(a) || trigger_cond_uses_damage_pairs(b)
        }
        _ => false,
    }
}

fn body_uses_damage_pairs(body: &crate::program::ExecutableBody) -> bool {
    let target_uses = match body.target {
        BodyTarget::FilteredPermanent(f)
        | BodyTarget::FilteredOwnGraveyardCard(f)
        | BodyTarget::FilteredAnyGraveyardCard { filter: f, .. } => filter_uses_damage_pairs(f),
        _ => false,
    };
    if let Some(modes) = body.triggered_modes()
        && modes.options.iter().any(body_uses_damage_pairs)
    {
        return true;
    }
    target_uses
        || body.instructions.iter().any(|op| match op {
            Instruction::ZoneOp(spec) => match &spec.source {
                crate::toolbox::ObjectQuery::ZoneScan { filter, .. }
                | crate::toolbox::ObjectQuery::Filtered { filter, .. }
                | crate::toolbox::ObjectQuery::BoundFiltered { filter, .. } => {
                    filter_uses_damage_pairs(**filter)
                }
                crate::toolbox::ObjectQuery::EffectSubject(subject) => {
                    subject_uses_damage_pairs(subject)
                }
                _ => false,
            },
            Instruction::MutateObject(spec) => match &spec.source {
                crate::toolbox::ObjectQuery::EffectSubject(subject) => {
                    subject_uses_damage_pairs(subject)
                }
                _ => false,
            },
            Instruction::ApplyContinuous(spec) => match &spec.selector {
                Some(crate::toolbox::ObjectQuery::EffectSubject(subject)) => {
                    subject_uses_damage_pairs(subject)
                }
                _ => false,
            },
            Instruction::DealDamage(spec) => {
                let amount_uses = matches!(
                    &spec.amount,
                    crate::program::DamageAmountSpec::Expr(e) if value_expr_uses_damage_pairs(e)
                );
                let source_uses = match &spec.source {
                    crate::toolbox::DamageSourceQuery::Objects(q) => match q {
                        crate::toolbox::ObjectQuery::ZoneScan { filter, .. }
                        | crate::toolbox::ObjectQuery::Filtered { filter, .. }
                        | crate::toolbox::ObjectQuery::BoundFiltered { filter, .. } => {
                            filter_uses_damage_pairs(**filter)
                        }
                        crate::toolbox::ObjectQuery::EffectSubject(subject) => {
                            subject_uses_damage_pairs(subject)
                        }
                        _ => false,
                    },
                    crate::toolbox::DamageSourceQuery::EntrySource => false,
                };
                let recipient_uses = match &spec.recipients {
                    crate::toolbox::DamageTargetQuery::Objects(q)
                    | crate::toolbox::DamageTargetQuery::Both { objects: q, .. } => match q {
                        crate::toolbox::ObjectQuery::ZoneScan { filter, .. }
                        | crate::toolbox::ObjectQuery::Filtered { filter, .. }
                        | crate::toolbox::ObjectQuery::BoundFiltered { filter, .. } => {
                            filter_uses_damage_pairs(**filter)
                        }
                        crate::toolbox::ObjectQuery::EffectSubject(subject) => {
                            subject_uses_damage_pairs(subject)
                        }
                        _ => false,
                    },
                    crate::toolbox::DamageTargetQuery::Relative(rel) => matches!(
                        rel.as_ref(),
                        crate::program::DamageRecipientsRef::Group(
                            crate::program::DamageGroup::Filtered(f)
                                | crate::program::DamageGroup::FilteredAndPlayers { filter: f, .. }
                        ) if filter_uses_damage_pairs(*f)
                    ),
                    _ => false,
                };
                amount_uses || source_uses || recipient_uses
            }
            Instruction::ScheduleDelayedStepTrigger { body, .. }
            | Instruction::EmitReflexiveTrigger { body, .. }
            | Instruction::FixedTargetClause(body) => body_uses_damage_pairs(body),
            Instruction::ForEachPlayerQuery { body, .. } => body_uses_damage_pairs(body),
            Instruction::MutatePlayer(spec) => match &spec.op {
                crate::toolbox::PlayerMutation::OfferAnyPlayer {
                    cost_body,
                    paid_body,
                    declined_body,
                } => {
                    body_uses_damage_pairs(cost_body)
                        || body_uses_damage_pairs(paid_body)
                        || declined_body.as_deref().is_some_and(body_uses_damage_pairs)
                }
                _ => false,
            },
            Instruction::ChooseOption { options, .. } => {
                options.iter().any(|b| body_uses_damage_pairs(&b.body))
            }
            Instruction::ScheduleDelayedEventTrigger { trigger, body, .. } => {
                trigger_event_uses_damage_pairs(&trigger.event)
                    || trigger
                        .intervening_if
                        .as_ref()
                        .is_some_and(trigger_cond_uses_damage_pairs)
                    || body_uses_damage_pairs(body)
            }
            Instruction::BranchCondition { condition, .. } => {
                condition_expr_uses_damage_pairs(condition)
            }
            _ => false,
        })
}

fn trigger_event_uses_damage_pairs(event: &crate::program::TriggerEvent) -> bool {
    match event {
        crate::program::TriggerEvent::Dies { mode, subject, .. } => {
            matches!(
                mode,
                crate::program::DiesTriggerMode::DamagedBySource
                    | crate::program::DiesTriggerMode::DamagedByAttachedHost
            ) || filter_uses_damage_pairs(*subject)
        }
        _ => false,
    }
}

fn static_uses_damage_pairs(ability: &crate::characteristics::StaticContinuousAbility) -> bool {
    use crate::characteristics::{
        CastPermissionRule, NumericExpression, StaticContinuousAbility, StaticDamageOp,
        StaticDamageRule, StaticModifierRule, StaticPart, StaticReplacementAction,
    };
    match ability {
        StaticContinuousAbility::EventReplacement {
            condition, action, ..
        } => {
            condition
                .as_deref()
                .is_some_and(static_cond_uses_damage_pairs)
                || match action {
                    StaticReplacementAction::ReplaceWithBody { body, .. } => {
                        body_uses_damage_pairs(body)
                    }
                    StaticReplacementAction::ModifyAmount { add, .. } => {
                        value_expr_uses_damage_pairs(add)
                    }
                    _ => false,
                }
        }
        StaticContinuousAbility::DamageRule { rule, .. } => match rule {
            StaticDamageRule::ModifyDamage {
                op: StaticDamageOp::AddExpr(e) | StaticDamageOp::PreventUpToExpr(e),
                ..
            } => value_expr_uses_damage_pairs(e),
            StaticDamageRule::ConditionalModifyDamage { condition, op, .. } => {
                static_cond_uses_damage_pairs(condition)
                    || matches!(
                        op,
                        StaticDamageOp::AddExpr(e) | StaticDamageOp::PreventUpToExpr(e)
                            if value_expr_uses_damage_pairs(e)
                    )
            }
            _ => false,
        },
        StaticContinuousAbility::Modifier { rule, .. } => match rule {
            StaticModifierRule::ConditionalModifyLifeGain { condition, .. } => {
                condition_uses_damage_pairs(condition)
            }
            StaticModifierRule::OthersEnterWithCountersExpr { amount, .. } => {
                value_expr_uses_damage_pairs(amount)
            }
            _ => false,
        },
        StaticContinuousAbility::CastPermission {
            permission: CastPermissionRule::GraveyardSelfConditional { condition },
            ..
        } => condition_uses_damage_pairs(condition),
        StaticContinuousAbility::Battlefield { parts, .. } => {
            parts.0.iter().any(|part| match part {
                StaticPart::GrantActivated(ab)
                | StaticPart::GrantActivatedRestricted { ability: ab, .. } => {
                    body_uses_damage_pairs(&ab.body)
                }
                StaticPart::GrantTrigger(trig) => {
                    trigger_event_uses_damage_pairs(&trig.condition.event)
                        || trig
                            .condition
                            .intervening_if
                            .as_ref()
                            .is_some_and(trigger_cond_uses_damage_pairs)
                        || body_uses_damage_pairs(&trig.body)
                }
                StaticPart::ModifyPT(pair) | StaticPart::SetPT(pair) => {
                    matches!(
                        &pair.power,
                        NumericExpression::Eval(e) if value_expr_uses_damage_pairs(e)
                    ) || matches!(
                        &pair.toughness,
                        NumericExpression::Eval(e) if value_expr_uses_damage_pairs(e)
                    )
                }
                _ => false,
            })
        }
        _ => false,
    }
}

fn program_tree_uses_damage_pairs(program: &Program) -> bool {
    let direct = definition_or_token_matches(program, |p| {
        p.triggered.iter().any(|ability| {
            trigger_event_uses_damage_pairs(&ability.condition.event)
                || ability
                    .condition
                    .intervening_if
                    .as_ref()
                    .is_some_and(trigger_cond_uses_damage_pairs)
                || body_uses_damage_pairs(&ability.body)
        }) || p
            .activated
            .iter()
            .any(|ability| body_uses_damage_pairs(&ability.body))
            || p.activation_modes.values().any(|modes| {
                modes.prefix.as_deref().is_some_and(body_uses_damage_pairs)
                    || modes.suffix.as_deref().is_some_and(body_uses_damage_pairs)
                    || modes.additional.iter().any(body_uses_damage_pairs)
            })
            || match &p.kind {
                ProgramKind::Spell(spell) => {
                    body_uses_damage_pairs(&spell.body)
                        || spell.modes.as_ref().is_some_and(|modes| {
                            modes.prefix.as_deref().is_some_and(body_uses_damage_pairs)
                                || modes.suffix.as_deref().is_some_and(body_uses_damage_pairs)
                                || modes.additional.iter().any(body_uses_damage_pairs)
                        })
                }
                ProgramKind::Permanent(_) => false,
            }
            || p.continuous
                .iter()
                .chain(&p.additional_battlefield_abilities)
                .any(static_uses_damage_pairs)
            || p.static_conditions
                .values()
                .any(static_cond_uses_damage_pairs)
            || p.activation_restrictions
                .values()
                .any(restriction_uses_damage_pairs)
            || p.conditional_cost.as_ref().is_some_and(|cc| match cc {
                crate::program::ConditionalCastCost::Alternative { condition, .. } => condition
                    .as_deref()
                    .is_some_and(condition_uses_damage_pairs),
                crate::program::ConditionalCastCost::Condition { condition, .. } => {
                    condition_uses_damage_pairs(condition)
                }
                _ => false,
            })
            || p.cast_timing.as_ref().is_some_and(|r| match r {
                crate::program::CastTimingRestriction::Condition(c) => {
                    condition_uses_damage_pairs(c)
                }
                _ => false,
            })
            || p.zone_replacements.iter().any(|rep| {
                matches!(
                    rep.subject,
                    crate::program::ZoneReplacementSubject::DamagedBySourceThisTurn
                )
            })
    });
    if direct {
        return true;
    }
    let sub_programs = [
        program.nightbound_back.as_deref(),
        program.transform_back.as_deref(),
        program.modal_back.as_deref(),
        program.flip_back.as_deref(),
        program.prototype.as_deref(),
        program.adventure.as_deref(),
        program.prepare_spell.as_deref(),
        program.split.as_ref().map(|s| &s.right),
        program.siege_back.as_deref(),
        program.disturb.as_ref().map(|d| &d.back),
    ];
    sub_programs
        .into_iter()
        .flatten()
        .any(program_tree_uses_damage_pairs)
}

/// Whether this program (or any face/token it can create) reads `creature_damage_pairs`.
pub(super) fn needs_creature_damage_pairs(program: &Program) -> bool {
    program_tree_uses_damage_pairs(program)
}

fn value_expr_uses_damage_dealt_by(expr: &crate::program::ValueExpr) -> bool {
    use crate::program::{PlayerStatExpr, ValueExpr};
    match expr {
        ValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn { .. }) => true,
        ValueExpr::Scale { expr, .. } | ValueExpr::DivConst { expr, .. } => {
            value_expr_uses_damage_dealt_by(expr)
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            value_expr_uses_damage_dealt_by(a) || value_expr_uses_damage_dealt_by(b)
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            value_expr_uses_damage_dealt_by(left)
                || value_expr_uses_damage_dealt_by(right)
                || value_expr_uses_damage_dealt_by(then_value)
                || value_expr_uses_damage_dealt_by(else_value)
        }
        _ => false,
    }
}

fn condition_uses_damage_dealt_by(cond: &crate::program::ResolutionCondition) -> bool {
    value_expr_uses_damage_dealt_by(&cond.left) || value_expr_uses_damage_dealt_by(&cond.right)
}

fn condition_expr_uses_damage_dealt_by(cond: &crate::toolbox::ConditionExpr) -> bool {
    match cond {
        crate::toolbox::ConditionExpr::Resolution(c) => condition_uses_damage_dealt_by(c),
        crate::toolbox::ConditionExpr::Compare { left, right, .. } => {
            value_expr_uses_damage_dealt_by(left) || value_expr_uses_damage_dealt_by(right)
        }
        crate::toolbox::ConditionExpr::And(cs) | crate::toolbox::ConditionExpr::Or(cs) => {
            cs.iter().any(condition_expr_uses_damage_dealt_by)
        }
        crate::toolbox::ConditionExpr::Not(c) => condition_expr_uses_damage_dealt_by(c),
        _ => false,
    }
}

fn static_cond_uses_damage_dealt_by(cond: &crate::characteristics::StaticSourceCondition) -> bool {
    use crate::characteristics::StaticSourceCondition;
    match cond {
        StaticSourceCondition::Eval(c) => condition_uses_damage_dealt_by(c),
        StaticSourceCondition::Not(inner) => static_cond_uses_damage_dealt_by(inner),
        StaticSourceCondition::And(a, b) | StaticSourceCondition::Or(a, b) => {
            static_cond_uses_damage_dealt_by(a) || static_cond_uses_damage_dealt_by(b)
        }
        _ => false,
    }
}

fn restriction_uses_damage_dealt_by(res: &crate::activated::ActivationRestriction) -> bool {
    use crate::activated::ActivationRestriction;
    match res {
        ActivationRestriction::Condition(c) => condition_uses_damage_dealt_by(c),
        ActivationRestriction::All(list) => list.iter().any(restriction_uses_damage_dealt_by),
        _ => false,
    }
}

fn trigger_cond_uses_damage_dealt_by(cond: &crate::program::TriggerCondition) -> bool {
    use crate::program::TriggerCondition;
    match cond {
        TriggerCondition::Eval(c) => condition_uses_damage_dealt_by(c),
        TriggerCondition::Not(inner) => trigger_cond_uses_damage_dealt_by(inner),
        TriggerCondition::And(a, b) | TriggerCondition::Or(a, b) => {
            trigger_cond_uses_damage_dealt_by(a) || trigger_cond_uses_damage_dealt_by(b)
        }
        _ => false,
    }
}

fn static_uses_damage_dealt_by(ability: &crate::characteristics::StaticContinuousAbility) -> bool {
    use crate::characteristics::{
        CastPermissionRule, NumericExpression, StaticContinuousAbility, StaticDamageOp,
        StaticDamageRule, StaticModifierRule, StaticPart, StaticReplacementAction,
    };
    match ability {
        StaticContinuousAbility::EventReplacement { action, .. } => match action {
            StaticReplacementAction::ReplaceWithBody { body, .. } => {
                body_uses_damage_dealt_by(body)
            }
            StaticReplacementAction::ModifyAmount { add, .. } => {
                value_expr_uses_damage_dealt_by(add)
            }
            _ => false,
        },
        StaticContinuousAbility::DamageRule { rule, .. } => match rule {
            StaticDamageRule::ModifyDamage {
                op: StaticDamageOp::AddExpr(e) | StaticDamageOp::PreventUpToExpr(e),
                ..
            } => value_expr_uses_damage_dealt_by(e),
            StaticDamageRule::ConditionalModifyDamage { condition, op, .. } => {
                static_cond_uses_damage_dealt_by(condition)
                    || matches!(
                        op,
                        StaticDamageOp::AddExpr(e) | StaticDamageOp::PreventUpToExpr(e)
                            if value_expr_uses_damage_dealt_by(e)
                    )
            }
            _ => false,
        },
        StaticContinuousAbility::Modifier { rule, .. } => match rule {
            StaticModifierRule::ConditionalModifyLifeGain { condition, .. } => {
                condition_uses_damage_dealt_by(condition)
            }
            StaticModifierRule::OthersEnterWithCountersExpr { amount, .. } => {
                value_expr_uses_damage_dealt_by(amount)
            }
            _ => false,
        },
        StaticContinuousAbility::CastPermission {
            permission: CastPermissionRule::GraveyardSelfConditional { condition },
            ..
        } => condition_uses_damage_dealt_by(condition),
        StaticContinuousAbility::Battlefield { parts, .. } => {
            parts.0.iter().any(|part| match part {
                StaticPart::GrantActivated(ab)
                | StaticPart::GrantActivatedRestricted { ability: ab, .. } => {
                    body_uses_damage_dealt_by(&ab.body)
                }
                StaticPart::GrantTrigger(trig) => {
                    trig.condition
                        .intervening_if
                        .as_ref()
                        .is_some_and(trigger_cond_uses_damage_dealt_by)
                        || body_uses_damage_dealt_by(&trig.body)
                }
                StaticPart::ModifyPT(pair) | StaticPart::SetPT(pair) => {
                    matches!(
                        &pair.power,
                        NumericExpression::Eval(e) if value_expr_uses_damage_dealt_by(e)
                    ) || matches!(
                        &pair.toughness,
                        NumericExpression::Eval(e) if value_expr_uses_damage_dealt_by(e)
                    )
                }
                _ => false,
            })
        }
        _ => false,
    }
}

fn body_uses_damage_dealt_by(body: &crate::program::ExecutableBody) -> bool {
    if let Some(modes) = body.triggered_modes()
        && modes.options.iter().any(body_uses_damage_dealt_by)
    {
        return true;
    }
    body.instructions.iter().any(|op| match op {
        Instruction::BranchCondition { condition, .. } => condition_expr_uses_damage_dealt_by(condition),
        Instruction::ForEachPlayerQuery { body, .. } => body_uses_damage_dealt_by(body),
        Instruction::ScheduleDelayedStepTrigger { body, .. }
        | Instruction::ScheduleDelayedEventTrigger { body, .. }
        | Instruction::EmitReflexiveTrigger { body, .. }
        | Instruction::FixedTargetClause(body) => body_uses_damage_dealt_by(body),
        Instruction::ChooseOption { options, .. } => {
            options.iter().any(|b| body_uses_damage_dealt_by(&b.body))
        }
        Instruction::DrawCards(spec) => value_expr_uses_damage_dealt_by(&spec.amount),
        Instruction::AdjustLife(spec) => match &spec.op {
            crate::toolbox::LifeOp::Gain(v)
            | crate::toolbox::LifeOp::Lose(v)
            | crate::toolbox::LifeOp::Set(v)
            | crate::toolbox::LifeOp::Pay(v)
            | crate::toolbox::LifeOp::Drain { amount: v, .. } => value_expr_uses_damage_dealt_by(v),
            crate::toolbox::LifeOp::LoseHalf { .. }
            | crate::toolbox::LifeOp::ExchangeTotals { .. }
            | crate::toolbox::LifeOp::ExchangeWithStat { .. } => false,
        },
        Instruction::DealDamage(spec) => {
            matches!(
                &spec.amount,
                crate::program::DamageAmountSpec::Expr(v) if value_expr_uses_damage_dealt_by(v)
            ) || matches!(
                &spec.recipients,
                crate::toolbox::DamageTargetQuery::Divided { total, .. } if value_expr_uses_damage_dealt_by(total)
            )
        }
        Instruction::MutatePlayer(spec) => match &spec.op {
            crate::toolbox::PlayerMutation::AddCounter { amount, .. }
            | crate::toolbox::PlayerMutation::PayCounter { amount, .. }
            | crate::toolbox::PlayerMutation::BindResolutionX(amount) => {
                value_expr_uses_damage_dealt_by(amount)
            }
            crate::toolbox::PlayerMutation::OfferAnyPlayer {
                cost_body,
                paid_body,
                declined_body,
            } => {
                body_uses_damage_dealt_by(cost_body)
                    || body_uses_damage_dealt_by(paid_body)
                    || declined_body
                        .as_deref()
                        .is_some_and(body_uses_damage_dealt_by)
            }
            _ => false,
        },
        Instruction::MutateObject(spec) => match &spec.op {
            crate::toolbox::ObjectMutation::AddCounters { amount, .. }
            | crate::toolbox::ObjectMutation::RemoveCounters { amount, .. }
            | crate::toolbox::ObjectMutation::MoveCounters {
                amount: Some(amount),
                ..
            } => value_expr_uses_damage_dealt_by(amount),
            _ => false,
        },
        Instruction::ApplyContinuous(spec) => match &spec.payload {
            crate::toolbox::ContinuousPayload::DynamicBoost {
                power, toughness, ..
            } => {
                value_expr_uses_damage_dealt_by(power) || value_expr_uses_damage_dealt_by(toughness)
            }
            _ => false,
        },
        Instruction::CreateTokens { creations } => creations
            .iter()
            .any(|c| value_expr_uses_damage_dealt_by(&c.amount)),
        _ => false,
    })
}

fn program_tree_uses_damage_dealt_by(program: &Program) -> bool {
    let direct = definition_or_token_matches(program, |p| {
        p.triggered.iter().any(|ability| {
            ability
                .condition
                .intervening_if
                .as_ref()
                .is_some_and(trigger_cond_uses_damage_dealt_by)
                || body_uses_damage_dealt_by(&ability.body)
        }) || p
            .activated
            .iter()
            .any(|ability| body_uses_damage_dealt_by(&ability.body))
            || p.activation_modes.values().any(|modes| {
                modes
                    .prefix
                    .as_deref()
                    .is_some_and(body_uses_damage_dealt_by)
                    || modes
                        .suffix
                        .as_deref()
                        .is_some_and(body_uses_damage_dealt_by)
                    || modes.additional.iter().any(body_uses_damage_dealt_by)
            })
            || match &p.kind {
                ProgramKind::Spell(spell) => {
                    body_uses_damage_dealt_by(&spell.body)
                        || spell.modes.as_ref().is_some_and(|modes| {
                            modes
                                .prefix
                                .as_deref()
                                .is_some_and(body_uses_damage_dealt_by)
                                || modes
                                    .suffix
                                    .as_deref()
                                    .is_some_and(body_uses_damage_dealt_by)
                                || modes.additional.iter().any(body_uses_damage_dealt_by)
                        })
                }
                ProgramKind::Permanent(_) => false,
            }
            || p.continuous
                .iter()
                .chain(&p.additional_battlefield_abilities)
                .any(static_uses_damage_dealt_by)
            || p.static_conditions
                .values()
                .any(static_cond_uses_damage_dealt_by)
            || p.activation_restrictions
                .values()
                .any(restriction_uses_damage_dealt_by)
            || p.activation_cost_reductions
                .values()
                .any(value_expr_uses_damage_dealt_by)
            || p.zone_replacement_extras.values().any(|extra| {
                extra
                    .followup_body
                    .as_deref()
                    .is_some_and(body_uses_damage_dealt_by)
            })
            || p.conditional_cost.as_ref().is_some_and(|cc| match cc {
                crate::program::ConditionalCastCost::Alternative { condition, .. } => condition
                    .as_deref()
                    .is_some_and(condition_uses_damage_dealt_by),
                _ => false,
            })
            || p.cast_timing.as_ref().is_some_and(|r| match r {
                crate::program::CastTimingRestriction::Condition(c) => {
                    condition_uses_damage_dealt_by(c)
                }
                _ => false,
            })
    });
    if direct {
        return true;
    }
    let sub_programs = [
        program.nightbound_back.as_deref(),
        program.transform_back.as_deref(),
        program.modal_back.as_deref(),
        program.flip_back.as_deref(),
        program.prototype.as_deref(),
        program.adventure.as_deref(),
        program.prepare_spell.as_deref(),
        program.split.as_ref().map(|s| &s.right),
        program.siege_back.as_deref(),
        program.disturb.as_ref().map(|d| &d.back),
    ];
    sub_programs
        .into_iter()
        .flatten()
        .any(program_tree_uses_damage_dealt_by)
}

/// Whether this program (or any face/token it can create) reads per-source `DamageDealtThisTurn`.
pub(super) fn needs_damage_dealt_by(program: &Program) -> bool {
    program_tree_uses_damage_dealt_by(program)
}

impl EffectWindow {
    /// Actual damage dealt to each player this turn in games with a printed bloodthirst ability.
    pub fn damage_to_players_this_turn(&self) -> Option<&BTreeMap<SeatId, RulesInteger>> {
        self.damage_to_players_this_turn.as_ref()
    }
    /// Loyalty counters on current permanent incarnations.
    pub fn loyalty_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.loyalty
    }
    /// Player designated to protect this current battle incarnation.
    pub fn battle_protector(&self, battle: ObjectId) -> Option<SeatId> {
        self.characteristics.battle_protectors.get(&battle).copied()
    }
    /// Lore counters on current permanent incarnations.
    pub fn lore_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.lore
    }
    /// Storage counters on live objects.
    pub fn storage_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.storage
    }
    /// Suspect counters by current incarnation.
    pub fn suspect_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.suspect_counters
    }
    /// Charge counters by current incarnation.
    pub fn charge_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.charge
    }

    fn apply_entry_status(
        &mut self,
        object: ObjectId,
        before: &RulesSnapshot,
    ) -> Result<(), EffectError> {
        use crate::land::EntryTap;
        let has_entry_tap = self
            .copiable_program(object)
            .and_then(Program::permanent)
            .and_then(|p| p.entry_tap)
            .is_some();
        let mut tapped = false;
        if has_entry_tap {
            let current = self
                .current_characteristics(object)?
                .ok_or(EffectError::InconsistentProgram)?;
            if let Some(rule) = current.abilities.entry_tap {
                let controller = current.controller.ok_or(EffectError::InconsistentProgram)?;
                let previous = before.view();
                let lands =
                    previous.evaluated.objects.values().filter(|p| {
                        p.zone == ObjectZone::Battlefield && p.has_type(CardType::Land)
                    });
                let mut controlled = lands.clone().filter(|p| p.controller == Some(controller));
                let opponents = || {
                    self.combat_rules
                        .opponents(previous.window.participants(), controller)
                };
                tapped = match rule {
                    EntryTap::ReadAhead { .. } => false,
                    EntryTap::Tribute(_) => {
                        if matches!(
                            self.entry_choices.get(&object).copied(),
                            Some(crate::land::EntryChoice::OptionIndex(0))
                        ) {
                            self.characteristics.tribute_paid.insert(object);
                        }
                        false
                    }
                    EntryTap::Always => true,
                    EntryTap::AlwaysChooseColorExcept(excluded) => {
                        let chosen = self
                            .entry_color_choices
                            .get(&object)
                            .ok_or(EffectError::EntryColorChoiceRequired)?;
                        if *chosen == excluded || *chosen == ManaType::Colourless {
                            return Err(EffectError::IllegalEntryColor);
                        }
                        true
                    }
                    EntryTap::ChooseColor => {
                        let chosen = self
                            .entry_color_choices
                            .get(&object)
                            .ok_or(EffectError::EntryColorChoiceRequired)?;
                        if *chosen == ManaType::Colourless {
                            return Err(EffectError::IllegalEntryColor);
                        }
                        false
                    }
                    EntryTap::ChooseColorExcept(excluded) => {
                        let chosen = self
                            .entry_color_choices
                            .get(&object)
                            .ok_or(EffectError::EntryColorChoiceRequired)?;
                        if *chosen == excluded || *chosen == ManaType::Colourless {
                            return Err(EffectError::IllegalEntryColor);
                        }
                        false
                    }
                    EntryTap::ChooseOpponent => {
                        let Some(crate::land::EntryChoice::Opponent(player)) =
                            self.entry_choices.get(&object).copied()
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if !opponents()?.contains(&player) {
                            return Err(EffectError::IllegalTarget);
                        }
                        false
                    }
                    EntryTap::ChoosePlayer => {
                        let Some(crate::land::EntryChoice::Opponent(player)) =
                            self.entry_choices.get(&object).copied()
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if !previous.window.participants().contains(&player) {
                            return Err(EffectError::IllegalTarget);
                        }
                        false
                    }
                    EntryTap::ChooseCardType
                    | EntryTap::ChooseBasicLandType
                    | EntryTap::ChooseOddOrEven
                    | EntryTap::ChooseKhansOrDragons
                    | EntryTap::ChooseOptions(_)
                    | EntryTap::Unleash
                    | EntryTap::Riot => {
                        let Some(crate::land::EntryChoice::OptionIndex(idx)) =
                            self.entry_choices.get(&object).copied()
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if !rule
                            .option_labels()
                            .is_some_and(|labels| usize::from(idx) < labels.len())
                        {
                            return Err(EffectError::IllegalTarget);
                        }
                        self.characteristics
                            .entry_option_choices
                            .insert(object, idx);
                        false
                    }
                    EntryTap::ChooseModalFace => {
                        if let Some(crate::land::EntryChoice::OptionIndex(idx)) =
                            self.entry_choices.get(&object).copied()
                        {
                            if idx >= 2 {
                                return Err(EffectError::IllegalTarget);
                            }
                            if idx == 1 {
                                self.window.enter_transformed(object)?;
                            }
                        }
                        false
                    }
                    EntryTap::ChosenOpponentControls => {
                        if let Some(crate::land::EntryChoice::Opponent(player)) =
                            self.entry_choices.remove(&object)
                        {
                            self.window.set_entering_controller(object, player)?;
                        } else if !opponents()?.is_empty() {
                            return Err(EffectError::SelectionRequired);
                        }
                        false
                    }
                    EntryTap::ChooseNonlandCardName => {
                        if !matches!(
                            self.entry_choices.get(&object),
                            Some(crate::land::EntryChoice::CardName(_))
                        ) {
                            return Err(EffectError::SelectionRequired);
                        }
                        false
                    }
                    EntryTap::ChooseCreatureType => {
                        let chosen = self
                            .entry_creature_type_choices
                            .get(&object)
                            .ok_or(EffectError::SelectionRequired)?;
                        if self.creature_vocabulary.label(*chosen).is_none() {
                            return Err(EffectError::IllegalTarget);
                        }
                        false
                    }
                    EntryTap::SiegeProtector => {
                        let Some(crate::land::EntryChoice::Opponent(player)) =
                            self.entry_choices.remove(&object)
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if !opponents()?.contains(&player) {
                            return Err(EffectError::IllegalTarget);
                        }
                        self.characteristics
                            .battle_protectors
                            .insert(object, player);
                        false
                    }
                    EntryTap::ChooseTwoPlayers => {
                        let Some(crate::land::EntryChoice::Players(players)) =
                            self.entry_choices.remove(&object)
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if !self.valid_distinct_players(players) {
                            return Err(EffectError::IllegalTarget);
                        }
                        self.characteristics
                            .chosen_entry_players
                            .insert(object, players);
                        false
                    }
                    EntryTap::UnlessTwoOtherLands => controlled.count() < 2,
                    EntryTap::UnlessTwoOrFewerOtherLands => controlled.count() > 2,
                    EntryTap::UnlessTwoBasicLands => {
                        controlled
                            .filter(|p| {
                                p.current_supertypes
                                    .contains(&crate::program::Supertype::Basic)
                            })
                            .count()
                            < 2
                    }
                    EntryTap::UnlessLandType(a, b) => !controlled.any(|p| {
                        p.current_land_subtypes.contains(&a.land_type())
                            || p.current_land_subtypes.contains(&b.land_type())
                    }),
                    EntryTap::UnlessRevealHandSubtype {
                        subtype,
                        or_control,
                    } => {
                        if or_control
                            && previous.evaluated.objects.values().any(|p| {
                                p.zone == ObjectZone::Battlefield
                                    && p.controller == Some(controller)
                                    && p.current_creature_subtypes.contains(&subtype)
                            })
                        {
                            self.entry_choices.remove(&object);
                            false
                        } else {
                            let Some(crate::land::EntryChoice::Reveal(revealed)) =
                                self.entry_choices.remove(&object)
                            else {
                                return Err(EffectError::SelectionRequired);
                            };
                            if let Some(revealed) = revealed {
                                let card = previous
                                    .cards
                                    .get(revealed)
                                    .ok_or(EffectError::IllegalTarget)?;
                                let current_card = self
                                    .window
                                    .cards()
                                    .get(revealed)
                                    .ok_or(EffectError::IllegalTarget)?;
                                let characteristics = previous
                                    .evaluated
                                    .get(revealed)
                                    .ok_or(EffectError::IllegalTarget)?;
                                if card.card_id().is_none()
                                    || card.zone() != ObjectZone::Hand(controller)
                                    || current_card.zone() != ObjectZone::Hand(controller)
                                    || !characteristics.current_creature_subtypes.contains(&subtype)
                                {
                                    return Err(EffectError::IllegalTarget);
                                }
                                self.pending_entry_reveals.push(card.clone());
                                false
                            } else {
                                true
                            }
                        }
                    }
                    EntryTap::UnlessRevealHandLandType(a, b) => {
                        let Some(crate::land::EntryChoice::Reveal(revealed)) =
                            self.entry_choices.remove(&object)
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if let Some(revealed) = revealed {
                            let card = previous
                                .cards
                                .get(revealed)
                                .ok_or(EffectError::IllegalTarget)?;
                            let current_card = self
                                .window
                                .cards()
                                .get(revealed)
                                .ok_or(EffectError::IllegalTarget)?;
                            let characteristics = previous
                                .evaluated
                                .get(revealed)
                                .ok_or(EffectError::IllegalTarget)?;
                            if card.card_id().is_none()
                                || card.zone() != ObjectZone::Hand(controller)
                                || current_card.zone() != ObjectZone::Hand(controller)
                                || !characteristics.has_type(CardType::Land)
                                || !(characteristics
                                    .current_land_subtypes
                                    .contains(&a.land_type())
                                    || characteristics
                                        .current_land_subtypes
                                        .contains(&b.land_type()))
                            {
                                return Err(EffectError::IllegalTarget);
                            }
                            self.pending_entry_reveals.push(card.clone());
                            false
                        } else {
                            true
                        }
                    }
                    EntryTap::UnlessTwoOpponents => opponents()?.len() < 2,
                    EntryTap::UnlessOpponentLandsEight => {
                        let opponents = opponents()?;
                        lands
                            .filter(|p| p.controller.is_some_and(|seat| opponents.contains(&seat)))
                            .count()
                            < 8
                    }
                    EntryTap::UnlessPlayerLifeThirteenOrLess => {
                        !previous.window.participants().iter().any(|seat| {
                            previous
                                .life
                                .get(*seat)
                                .is_some_and(|life| *life <= 13.into())
                        })
                    }
                    EntryTap::UnlessPayLife(amount) => {
                        let Some(crate::land::EntryChoice::PayLife(pay)) =
                            self.entry_choices.remove(&object)
                        else {
                            return Err(EffectError::SelectionRequired);
                        };
                        if pay {
                            let cost: RulesInteger = i64::from(amount).into();
                            if !cost.is_positive()
                                || self.life.get(controller).is_none_or(|total| total < &cost)
                            {
                                return Err(EffectError::InvalidCastResource);
                            }
                            self.life
                                .apply_adjustments(&BTreeMap::from([(
                                    controller,
                                    crate::life::LifeAdjustment::Lose(cost.clone()),
                                )]))
                                .map_err(|_| EffectError::InconsistentProgram)?;
                            let lost = self
                                .characteristics
                                .life_lost_this_turn
                                .entry(controller)
                                .or_insert_with(|| 0.into());
                            *lost = lost.plus(&cost);
                            false
                        } else {
                            true
                        }
                    }
                };
            }
        }
        if !tapped
            && let Some(cond) = self
                .copiable_program(object)
                .and_then(|p| p.entry_tap_unless.as_deref())
                .cloned()
            && let Some(current) = self.current_characteristics(object)?
            && let Some(controller) = current.controller
        {
            let previous = before.view();
            let ctx = crate::characteristics::WorldEvalCtx {
                active_player: previous.window.active_player(),
                step: self.current_step,
                participants: previous.window.participants(),
                life: Some(previous.life),
                combat: self.combat.as_ref(),
                cards: previous.cards,
                programs: &self.programs,
                state: &self.characteristics,
                objects: &previous.evaluated.objects,
                source: object,
                controller,
                name_rules: &self.name_rules,
                trigger_subject: None,
                related_player: None,
                defending_player: None,
            };
            if !crate::characteristics::eval_world_condition(&ctx, &cond) {
                tapped = true;
            }
        }
        if !tapped {
            let previous = before.view();
            let has_others_tapped_rule = previous.evaluated.objects.values().any(|perm| {
                perm.zone == ObjectZone::Battlefield
                    && perm.object != object
                    && Self::permanent_has_restriction(perm, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticRestriction::OthersEnterTapped(_)
                        )
                    })
            }) || self.temporary_rules.iter().any(|active| {
                self.active_temporary_static(active, &previous.evaluated)
                    .is_some_and(|ab| {
                        matches!(
                            ab,
                            crate::characteristics::StaticContinuousAbility::Restriction {
                                rule: crate::characteristics::StaticRestriction::OthersEnterTapped(
                                    _
                                ),
                                ..
                            }
                        )
                    })
            });
            if has_others_tapped_rule {
                let evaluated_now = self.evaluated_characteristics()?;
                tapped = previous.evaluated.objects.values().any(|perm| {
                    if perm.zone != ObjectZone::Battlefield || perm.object == object {
                        return false;
                    }
                    let Some(perm_ctrl) = perm.controller else {
                        return false;
                    };
                    Self::permanent_has_restriction(perm, |rule| {
                        if let crate::characteristics::StaticRestriction::OthersEnterTapped(
                            selector,
                        ) = rule
                        {
                            crate::characteristics::selector_matches_permanent(
                                self.window.cards(),
                                &evaluated_now,
                                perm.object,
                                perm_ctrl,
                                selector,
                                object,
                                &self.name_rules,
                            )
                        } else {
                            false
                        }
                    })
                }) || self.temporary_rules.iter().any(|active| {
                    let Some(crate::characteristics::StaticContinuousAbility::Restriction {
                        rule:
                            crate::characteristics::StaticRestriction::OthersEnterTapped(selector),
                        ..
                    }) = self.active_temporary_static(active, &previous.evaluated)
                    else {
                        return false;
                    };
                    let src = active.source_object.unwrap_or(object);
                    crate::characteristics::selector_matches_permanent(
                        self.window.cards(),
                        &evaluated_now,
                        src,
                        active.controller,
                        selector,
                        object,
                        &self.name_rules,
                    )
                });
            }
        }
        if tapped {
            let previous = before.view();
            let has_enter_untapped_rule = previous.evaluated.objects.values().any(|perm| {
                perm.zone == ObjectZone::Battlefield
                    && Self::permanent_has_restriction(perm, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticRestriction::EnterUntapped(_)
                        )
                    })
            }) || self.temporary_rules.iter().any(|active| {
                self.active_temporary_static(active, &previous.evaluated)
                    .is_some_and(|ab| {
                        matches!(
                            ab,
                            crate::characteristics::StaticContinuousAbility::Restriction {
                                rule: crate::characteristics::StaticRestriction::EnterUntapped(_),
                                ..
                            }
                        )
                    })
            });
            if has_enter_untapped_rule {
                let evaluated_now = self.evaluated_characteristics()?;
                let forced_untapped = previous.evaluated.objects.values().any(|perm| {
                    if perm.zone != ObjectZone::Battlefield {
                        return false;
                    }
                    let Some(perm_ctrl) = perm.controller else {
                        return false;
                    };
                    Self::permanent_has_restriction(perm, |rule| {
                        if let crate::characteristics::StaticRestriction::EnterUntapped(selector) =
                            rule
                        {
                            crate::characteristics::selector_matches_permanent(
                                self.window.cards(),
                                &evaluated_now,
                                perm.object,
                                perm_ctrl,
                                selector,
                                object,
                                &self.name_rules,
                            )
                        } else {
                            false
                        }
                    })
                }) || self.temporary_rules.iter().any(|active| {
                    let Some(crate::characteristics::StaticContinuousAbility::Restriction {
                        rule: crate::characteristics::StaticRestriction::EnterUntapped(selector),
                        ..
                    }) = self.active_temporary_static(active, &previous.evaluated)
                    else {
                        return false;
                    };
                    let src = active.source_object.unwrap_or(object);
                    crate::characteristics::selector_matches_permanent(
                        self.window.cards(),
                        &evaluated_now,
                        src,
                        active.controller,
                        selector,
                        object,
                        &self.name_rules,
                    )
                });
                if forced_untapped {
                    tapped = false;
                }
            }
        }
        if tapped {
            self.window.enter_tapped(object)?;
        }
        Ok(())
    }

    pub(super) fn apply_entry_counters(
        &mut self,
        before: &RulesSnapshot,
        object: ObjectId,
        placement: PowerToughnessCounters,
        mana_colors: u8,
    ) -> Result<PowerToughnessCounters, EffectError> {
        self.apply_spell_entry_counters(before, object, placement, mana_colors, None)
    }
    pub(super) fn apply_return_entry_counters(
        &mut self,
        before: &RulesSnapshot,
        object: ObjectId,
    ) -> Result<PowerToughnessCounters, EffectError> {
        self.apply_entry_counter_batch(
            before,
            object,
            PowerToughnessCounters::default(),
            0,
            None,
            true,
        )
    }
    pub(super) fn apply_spell_entry_counters(
        &mut self,
        before: &RulesSnapshot,
        object: ObjectId,
        placement: PowerToughnessCounters,
        mana_colors: u8,
        spell: Option<&crate::stack_window::StackEntry>,
    ) -> Result<PowerToughnessCounters, EffectError> {
        self.apply_entry_counter_batch(before, object, placement, mana_colors, spell, false)
    }
    fn apply_entry_counter_batch(
        &mut self,
        before: &RulesSnapshot,
        object: ObjectId,
        mut placement: PowerToughnessCounters,
        mana_colors: u8,
        spell: Option<&crate::stack_window::StackEntry>,
        return_with_counters: bool,
    ) -> Result<PowerToughnessCounters, EffectError> {
        if let Some(count) = self.prospective_devour_counters.remove(&object) {
            placement.plus_one = placement.plus_one.plus(&RulesInteger::from_usize(count));
        }
        self.apply_entry_status(object, before)?;
        if self
            .copiable_program(object)
            .is_some_and(|program| program.enters_prepared)
        {
            self.characteristics.prepared.insert(object);
        }
        let controller = self
            .current_characteristics(object)?
            .and_then(|current| current.controller)
            .ok_or(EffectError::InconsistentProgram)?;
        let extra_counters = self.copy_entry_counters.remove(&object).unwrap_or(0)
            + usize::from(return_with_counters);
        if extra_counters > 0
            && self
                .current_characteristics(object)?
                .is_some_and(|current| current.is_creature())
        {
            placement.plus_one = placement
                .plus_one
                .plus(&RulesInteger::from_usize(extra_counters));
        }
        let grant_entry = spell
            .map(crate::stack_window::StackEntry::id)
            .or_else(|| {
                self.window
                    .resolving()
                    .map(crate::stack_window::StackEntry::id)
            })
            .unwrap_or_else(StackEntryId::entry_replacement_creator);
        if let Some(entry_tap) = self
            .copiable_program(object)
            .and_then(|p| p.permanent())
            .and_then(|p| p.entry_tap)
        {
            match (entry_tap, self.entry_choices.get(&object).copied()) {
                (
                    crate::land::EntryTap::Unleash,
                    Some(crate::land::EntryChoice::OptionIndex(1)),
                )
                | (crate::land::EntryTap::Riot, Some(crate::land::EntryChoice::OptionIndex(0))) => {
                    placement.plus_one = placement.plus_one.plus(&1.into());
                }
                (crate::land::EntryTap::Riot, Some(crate::land::EntryChoice::OptionIndex(1))) => {
                    self.characteristics.add_effect_for_objects(
                        grant_entry,
                        BTreeSet::from([object]),
                        crate::characteristics::ResolvedParts(vec![
                            crate::characteristics::ResolvedPart::Abilities(
                                crate::characteristics::AbilityChange::grant_keywords([
                                    KeywordAbility::Haste,
                                ]),
                            ),
                        ]),
                        crate::characteristics::ResolvedDuration::NoStatedDuration,
                    )?;
                }
                _ => {}
            }
        }
        let previous = before.view();
        if let Some(entrant) = self.current_characteristics(object)? {
            let current_eval = self.evaluated_characteristics()?;
            for source in previous.evaluated.objects.values() {
                if source.object == object || source.zone != ObjectZone::Battlefield {
                    continue;
                }
                let Some(source_controller) = source.controller else {
                    continue;
                };
                if entrant.is_creature()
                    && source_controller == controller
                    && source.has_battlefield_rule(|rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticContinuousAbility::Modifier {
                                rule: crate::characteristics::StaticModifierRule::OtherCreaturesEnterWithSourceCounters,
                                ..
                            }
                        )
                    })
                {
                    placement.plus_one = placement.plus_one.plus(&source.plus_one_counters);
                }
                for ability in source.active_battlefield_rules() {
                    let entry_spec: Option<(
                        &crate::characteristics::PermanentSelector,
                        CounterKind,
                        RulesInteger,
                    )> = match ability {
                        crate::characteristics::StaticContinuousAbility::Modifier {
                            rule:
                                crate::characteristics::StaticModifierRule::OthersEnterWithCounters {
                                    selector,
                                    kind,
                                    count,
                                },
                            ..
                        } => Some((selector.as_ref(), *kind, i64::from(*count).into())),
                        crate::characteristics::StaticContinuousAbility::Modifier {
                            rule:
                                crate::characteristics::StaticModifierRule::OthersEnterWithCountersExpr {
                                    selector,
                                    kind,
                                    amount,
                                },
                            ..
                        } => {
                            let ctx = crate::characteristics::WorldEvalCtx {
                                active_player: previous.window.active_player(),
                                step: self.current_step,
                                participants: previous.window.participants(),
                                life: Some(previous.life),
                                combat: self.combat.as_ref(),
                                cards: previous.cards,
                                programs: &self.programs,
                                state: &self.characteristics,
                                objects: &previous.evaluated.objects,
                                source: source.object,
                                controller: source_controller,
                                name_rules: &self.name_rules,
                                trigger_subject: None,
                                related_player: None,
                                defending_player: None,
                            };
                            let val = crate::characteristics::eval_world_value_expr(&ctx, amount)
                                .max(0.into());
                            Some((selector.as_ref(), *kind, val))
                        }
                        _ => None,
                    };
                    if let Some((selector, kind, n)) = entry_spec
                        && n.is_positive()
                        && crate::characteristics::selector_matches_permanent(
                            self.window.cards(),
                            &current_eval,
                            source.object,
                            source_controller,
                            selector,
                            object,
                            &self.name_rules,
                        )
                        && !self.permanent_counter_prevented(&current_eval, object, kind)
                    {
                        match kind {
                            CounterKind::PlusOne => {
                                placement.plus_one = placement.plus_one.plus(&n);
                            }
                            CounterKind::MinusOne => {
                                placement.minus_one = placement.minus_one.plus(&n);
                            }
                            other => {
                                let mult =
                                    controlled_counter_multiplier(&previous.evaluated, controller);
                                let added = n.times(&mult);
                                if let Some(map) =
                                    self.characteristics.named_permanent_counters_mut(other)
                                {
                                    let entry = map.entry(object).or_insert_with(|| 0.into());
                                    *entry = entry.plus(&added);
                                }
                                self.characteristics.refresh_keyword_counter_grant(
                                    grant_entry,
                                    object,
                                    other,
                                )?;
                            }
                        }
                    }
                }
            }
            if entrant.is_creature() {
                let extra = previous
                    .evaluated
                    .objects
                    .values()
                    .filter(|source| {
                        source.zone == ObjectZone::Graveyard(controller)
                            && source.abilities.entry_counter_abilities
                    })
                    .flat_map(|source| &source.printed_text.entry_counter_abilities)
                    .filter(|ability| {
                        matches!(ability,
                            EntryCounterAbility::GraveyardControlledSubtypePlusOne(subtype)
                                if entrant.current_creature_subtypes.contains(subtype)
                        )
                    })
                    .count();
                placement.plus_one = placement.plus_one.plus(&RulesInteger::from_usize(extra));
            }
        }
        let multiplier = controlled_counter_multiplier(&previous.evaluated, controller);
        if let Some(current) = self.current_characteristics(object)?
            && let Some(last) = current.final_saga_chapter()
        {
            let number = if matches!(
                current.abilities.entry_tap,
                Some(crate::land::EntryTap::ReadAhead { .. })
            ) {
                let Some(crate::land::EntryChoice::Chapter(value)) =
                    self.entry_choices.remove(&object)
                else {
                    return Err(EffectError::SelectionRequired);
                };
                if !(1..=last).contains(&value) {
                    return Err(EffectError::IllegalTarget);
                }
                value
            } else {
                1
            };
            self.characteristics.lore.insert(
                object,
                RulesInteger::from(i64::from(number)).times(&multiplier),
            );
        }
        if self
            .current_characteristics(object)?
            .is_some_and(|view| view.has_type(CardType::Battle))
        {
            self.characteristics
                .battle_protectors
                .entry(object)
                .or_insert(controller);
            if let Some(printed) = self
                .copiable_program(object)
                .and_then(|program| program.printed_defense.as_ref())
            {
                self.characteristics
                    .defense
                    .insert(object, printed.times(&multiplier));
            }
        }
        if let Some(current) = self.current_characteristics(object)?
            && current.has_type(CardType::Planeswalker)
        {
            let controller = current.controller.ok_or(EffectError::InconsistentProgram)?;
            let extra = before
                .view()
                .evaluated
                .controlled_rules(controller, |ability| {
                    matches!(
                        ability,
                        crate::characteristics::StaticContinuousAbility::Modifier {
                            rule: crate::characteristics::StaticModifierRule::ExtraEntryLoyalty,
                            ..
                        }
                    )
                });
            let loyalty = current
                .defined
                .printed_loyalty
                .unwrap_or_else(|| 0.into())
                .plus(&RulesInteger::from_usize(extra + extra_counters));
            if loyalty.is_positive() {
                self.characteristics
                    .loyalty
                    .insert(object, loyalty.times(&multiplier));
            }
        }
        let has_entry_ability = !self
            .copiable_program(object)
            .ok_or(EffectError::InconsistentProgram)?
            .entry_counter_abilities
            .is_empty();
        let mut charge = RulesInteger::from(0);
        let mut suspect = RulesInteger::from(0);
        let mut extra_loyalty = RulesInteger::from(0);
        if has_entry_ability
            && let Some(current) = self.current_characteristics(object)?
            && current.abilities.entry_counter_abilities
        {
            let controller = current.controller.ok_or(EffectError::InconsistentProgram)?;
            let total_damage = self
                .damage_to_players_this_turn
                .iter()
                .flat_map(|map| map.iter())
                .filter(|(player, _)| **player != controller)
                .fold(RulesInteger::from(0), |total, (_, damage)| {
                    total.plus(damage)
                });
            for ability in &current.printed_text.entry_counter_abilities {
                match ability {
                    EntryCounterAbility::KickedOpposingNonbasicLands(index) => {
                        if spell
                            .and_then(|entry| entry.kicker_choices().get(index))
                            .is_some_and(RulesInteger::is_positive)
                        {
                            let opponents = self
                                .combat_rules
                                .opponents(previous.window.participants(), controller)?;
                            let count = previous
                                .evaluated
                                .objects
                                .values()
                                .filter(|object| {
                                    object.zone == ObjectZone::Battlefield
                                        && object.has_type(CardType::Land)
                                        && !object
                                            .current_supertypes
                                            .contains(&crate::program::Supertype::Basic)
                                        && object
                                            .controller
                                            .is_some_and(|seat| opponents.contains(&seat))
                                })
                                .count();
                            placement.plus_one =
                                placement.plus_one.plus(&RulesInteger::from_usize(count));
                        }
                    }
                    EntryCounterAbility::ChargePerKicker(index) => {
                        if let Some(times) =
                            spell.and_then(|entry| entry.kicker_choices().get(index))
                        {
                            charge = charge.plus(times);
                        }
                    }
                    EntryCounterAbility::Devour(_) => {} // Counters bound to the completed sacrifices are staged before movement.
                    EntryCounterAbility::ChosenXPlusOne => {
                        if let Some(x) = spell.and_then(|entry| entry.chosen_x()) {
                            placement.plus_one = placement.plus_one.plus(x);
                        }
                    }
                    EntryCounterAbility::SuspectForGreatestCreatureCount => {
                        let mut counts = BTreeMap::<SeatId, usize>::new();
                        for permanent in before
                            .view()
                            .evaluated
                            .objects
                            .values()
                            .filter(|o| o.zone == ObjectZone::Battlefield && o.is_creature())
                        {
                            if let Some(player) = permanent.controller {
                                *counts.entry(player).or_default() += 1;
                            }
                        }
                        suspect = suspect.plus(&RulesInteger::from_usize(
                            counts.values().copied().max().unwrap_or(0),
                        ));
                    }
                    EntryCounterAbility::FixedCharge(n) => charge = charge.plus(n),
                    EntryCounterAbility::FixedLoyalty(n) => extra_loyalty = extra_loyalty.plus(n),
                    EntryCounterAbility::FixedNamed { kind, amount } => {
                        if amount.is_positive() {
                            let added = amount.times(&multiplier);
                            if let Some(map) =
                                self.characteristics.named_permanent_counters_mut(*kind)
                            {
                                let entry = map.entry(object).or_insert_with(|| 0.into());
                                *entry = entry.plus(&added);
                            }
                            self.characteristics.refresh_keyword_counter_grant(
                                grant_entry,
                                object,
                                *kind,
                            )?;
                        }
                    }
                    EntryCounterAbility::FixedPlusOne(n) => {
                        placement.plus_one = placement.plus_one.plus(n)
                    }
                    EntryCounterAbility::FixedMinusOne(n) => {
                        placement.minus_one = placement.minus_one.plus(n)
                    }
                    EntryCounterAbility::DynamicCounter { kind, amount }
                    | EntryCounterAbility::ConditionalCounter { kind, amount, .. } => {
                        if let EntryCounterAbility::ConditionalCounter { condition, .. } = ability {
                            let left = self.resolve_value_expr_with_source(
                                controller,
                                &condition.left,
                                None,
                                Some(object),
                            )?;
                            let right = self.resolve_value_expr_with_source(
                                controller,
                                &condition.right,
                                None,
                                Some(object),
                            )?;
                            if !condition.comparison.evaluate(&left, &right) {
                                continue;
                            }
                        }
                        let resolved = if amount.is_chosen_x() {
                            spell
                                .and_then(|entry| entry.chosen_x().cloned())
                                .unwrap_or_else(|| 0.into())
                        } else {
                            self.resolve_value_expr_with_source(
                                controller,
                                amount,
                                None,
                                Some(object),
                            )?
                        };
                        if resolved.is_positive() {
                            match *kind {
                                CounterKind::PlusOne => {
                                    placement.plus_one = placement.plus_one.plus(&resolved);
                                }
                                CounterKind::MinusOne => {
                                    placement.minus_one = placement.minus_one.plus(&resolved);
                                }
                                CounterKind::Charge => {
                                    charge = charge.plus(&resolved);
                                }
                                CounterKind::Loyalty => {
                                    extra_loyalty = extra_loyalty.plus(&resolved);
                                }
                                other => {
                                    let added = resolved.times(&multiplier);
                                    if let Some(map) =
                                        self.characteristics.named_permanent_counters_mut(other)
                                    {
                                        let entry = map.entry(object).or_insert_with(|| 0.into());
                                        *entry = entry.plus(&added);
                                    }
                                    self.characteristics.refresh_keyword_counter_grant(
                                        grant_entry,
                                        object,
                                        other,
                                    )?;
                                }
                            }
                        }
                    }
                    EntryCounterAbility::Bloodthirst(n) if total_damage.is_positive() => {
                        placement.plus_one = placement.plus_one.plus(n)
                    }
                    EntryCounterAbility::Bloodthirst(_) => {}
                    EntryCounterAbility::MorbidPlusOne(n) => {
                        if self.creatures_died_this_turn.is_positive() {
                            placement.plus_one = placement.plus_one.plus(n);
                        }
                    }
                    EntryCounterAbility::GraveyardControlledSubtypePlusOne(_) => {}
                    EntryCounterAbility::ControlledAndGraveyardSubtypePlusOne(subtype) => {
                        let count = previous
                            .evaluated
                            .objects
                            .values()
                            .filter(|object| {
                                object.current_creature_subtypes.contains(subtype)
                                    && (object.zone == ObjectZone::Graveyard(controller)
                                        || object.zone == ObjectZone::Battlefield
                                            && object.controller == Some(controller))
                            })
                            .count();
                        placement.plus_one =
                            placement.plus_one.plus(&RulesInteger::from_usize(count));
                    }
                    EntryCounterAbility::BloodthirstX => {
                        placement.plus_one = placement.plus_one.plus(&total_damage)
                    }
                    EntryCounterAbility::Sunburst => {
                        let color_count = i64::from(mana_colors).into();
                        if current.has_type(CardType::Creature) {
                            placement.plus_one = placement.plus_one.plus(&color_count);
                        } else {
                            charge = charge.plus(&color_count);
                        }
                    }
                    EntryCounterAbility::Amplify(n) => {
                        let shared = previous
                            .evaluated
                            .objects
                            .values()
                            .filter(|card| {
                                card.zone == ObjectZone::Hand(controller)
                                    && card.object != object
                                    && card.is_creature()
                                    && !card
                                        .current_creature_subtypes
                                        .is_disjoint(&current.current_creature_subtypes)
                            })
                            .count();
                        if shared > 0 {
                            placement.plus_one = placement.plus_one.plus(
                                &RulesInteger::from_usize(shared)
                                    .times(&RulesInteger::from(i64::from(*n))),
                            );
                        }
                    }
                }
            }
        }
        if let Some(current) = self.current_characteristics(object)?
            && let Some(crate::land::EntryTap::Tribute(n)) = current.abilities.entry_tap
            && self.characteristics.tribute_paid.contains(&object)
        {
            placement.plus_one = placement.plus_one.plus(&RulesInteger::from(i64::from(n)));
        }
        self.apply_fading_vanishing_entry(object, &multiplier)?;
        if placement.valid_placement() {
            let entering = self
                .current_characteristics(object)?
                .ok_or(EffectError::InconsistentProgram)?;
            placement =
                self.adjusted_pt_counters(&previous.evaluated, object, placement, Some(&entering))?;
            if placement.valid_placement() {
                self.characteristics.place_counters(object, &placement)?;
            }
        }
        if suspect.is_positive()
            && !self.permanent_counter_prevented(&previous.evaluated, object, CounterKind::Suspect)
        {
            self.characteristics
                .suspect_counters
                .insert(object, suspect.times(&multiplier));
        }
        if charge.is_positive()
            && !self.permanent_counter_prevented(&previous.evaluated, object, CounterKind::Charge)
        {
            self.characteristics
                .charge
                .insert(object, charge.times(&multiplier));
        }
        if extra_loyalty.is_positive()
            && !self.permanent_counter_prevented(&previous.evaluated, object, CounterKind::Loyalty)
        {
            let loyalty = self
                .characteristics
                .loyalty
                .entry(object)
                .or_insert_with(|| 0.into());
            *loyalty = loyalty.plus(&extra_loyalty.times(&multiplier));
        }
        Ok(placement)
    }
}

fn body_grants_next_spell_storm(body: &crate::program::ExecutableBody) -> bool {
    if let Some(modes) = body.triggered_modes()
        && modes.options.iter().any(body_grants_next_spell_storm)
    {
        return true;
    }
    body.instructions.iter().any(|op| match op {
        Instruction::ApplyContinuous(spec) => matches!(
            &spec.payload,
            crate::toolbox::ContinuousPayload::TemporaryRule(rule)
                if matches!(
                    rule.as_ref(),
                    crate::program::TemporaryRuleKind::NextSpellCastKeyword {
                        keyword: crate::program::NextSpellCastKeyword::Storm,
                        ..
                    }
                )
        ),
        Instruction::ScheduleDelayedStepTrigger { body, .. }
        | Instruction::ScheduleDelayedEventTrigger { body, .. }
        | Instruction::EmitReflexiveTrigger { body, .. }
        | Instruction::FixedTargetClause(body)
        | Instruction::ForEach { body, .. }
        | Instruction::ForEachPlayerQuery { body, .. } => body_grants_next_spell_storm(body),
        Instruction::ChooseOption { options, .. } => options
            .iter()
            .any(|b| body_grants_next_spell_storm(&b.body)),
        Instruction::MutatePlayer(spec) => match &spec.op {
            crate::toolbox::PlayerMutation::OfferAnyPlayer {
                cost_body,
                paid_body,
                declined_body,
            } => {
                body_grants_next_spell_storm(cost_body)
                    || body_grants_next_spell_storm(paid_body)
                    || declined_body
                        .as_deref()
                        .is_some_and(body_grants_next_spell_storm)
            }
            _ => false,
        },
        _ => false,
    })
}

/// Whether this program (or any face/token it can create) needs `CastTurnHistory`.
pub(super) fn needs_cast_history(program: &Program) -> bool {
    let direct = definition_or_token_matches(program, |p| {
        p.triggered
            .iter()
            .any(|t| t.cast_copy_keyword().is_some() || body_grants_next_spell_storm(&t.body))
            || p.activated
                .iter()
                .any(|a| body_grants_next_spell_storm(&a.body))
            || p.activation_modes.values().any(|modes| {
                modes
                    .prefix
                    .as_deref()
                    .is_some_and(body_grants_next_spell_storm)
                    || modes
                        .suffix
                        .as_deref()
                        .is_some_and(body_grants_next_spell_storm)
                    || modes.additional.iter().any(body_grants_next_spell_storm)
            })
            || match &p.kind {
                ProgramKind::Spell(spell) => {
                    body_grants_next_spell_storm(&spell.body)
                        || spell.modes.as_ref().is_some_and(|modes| {
                            modes
                                .prefix
                                .as_deref()
                                .is_some_and(body_grants_next_spell_storm)
                                || modes
                                    .suffix
                                    .as_deref()
                                    .is_some_and(body_grants_next_spell_storm)
                                || modes.additional.iter().any(body_grants_next_spell_storm)
                        })
                }
                ProgramKind::Permanent(_) => false,
            }
            || matches!(
                p.conditional_cost,
                Some(
                    crate::program::ConditionalCastCost::Prowl(_)
                        | crate::program::ConditionalCastCost::Freerunning(_)
                        | crate::program::ConditionalCastCost::Mayhem(_)
                        | crate::program::ConditionalCastCost::Surge(_)
                        | crate::program::ConditionalCastCost::Spectacle(_)
                )
            )
    });
    if direct {
        return true;
    }
    let sub_programs = [
        program.nightbound_back.as_deref(),
        program.transform_back.as_deref(),
        program.modal_back.as_deref(),
        program.flip_back.as_deref(),
        program.prototype.as_deref(),
        program.adventure.as_deref(),
        program.prepare_spell.as_deref(),
        program.split.as_ref().map(|s| &s.right),
        program.siege_back.as_deref(),
        program.disturb.as_ref().map(|d| &d.back),
    ];
    sub_programs.into_iter().flatten().any(needs_cast_history)
}
