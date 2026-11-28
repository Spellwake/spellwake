//! Strictly-typed lowering from `v2::ir::Replacement` to native `spellwake_engine`
//! replacement effects (`EntryTap`, `EntryCounterAbility`, `EntryCopyRule`,
//! `ZoneReplacementAbility`, `StaticContinuousAbility::{DamageRule, Modifier, EventReplacement}`,
//! and `TemporaryRuleKind`).
//!
//! Every unhandled `v2::ir::Replacement` variant or modifier fails closed with
//! `Err(LoweringError::Unsupported("v2:..."))`. No raw text inspection is performed.

use std::collections::BTreeSet;

use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    characteristics::{
        DamagePreventFollowup, DamagePreventMode, DamageRedirectTarget, KeywordAbility,
        ReplaceableEvent, ResolvedDuration, StaticContinuousAbility, StaticDamageKind,
        StaticDamageOp, StaticDamageRecipient, StaticDamageRule, StaticModifierRule,
        StaticReplacementAction, StaticRestriction, StaticSourceCondition,
    },
    effects::CounterKind as EngineCounterKind,
    land::{EntryTap, LandType},
    program::{
        BodyTarget, Comparison, EffectSubject, EntryCopyRule, EntryCopySpecExt, EntryCopyZone,
        EntryCounterAbility, Instruction, ObjectFilter, PlayerRelation, Program, ProgramKind,
        PublicZoneDestination, ResolutionCondition, TemporaryRuleKind,
        ValueExpr as EngineValueExpr, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementChoice, ZoneReplacementExtra,
        ZoneReplacementSubject,
    },
    toolbox::ApplyContinuousSpec,
};
use spellwake_oracle_compiler::{
    ast::{CardTypeMask as AstCardTypeMask, ControllerRelation},
    v2::ir as v2,
};

use crate::v2_lowering::{
    atoms::{
        lower_card_type_set, lower_color_set, lower_counter_kind, lower_keyword_ability,
        lower_step, lower_supertype_set, lower_world_affected_players,
    },
    card::lower_granted_face,
    condition::{lower_resolution_condition, lower_static_source_condition},
    ctx::{BodyAcc, LowerCtx},
    effect::lower_effect_body,
    error::{LowerResult, LoweringError},
    filter::{lower_object_filter, lower_permanent_selector},
    static_ability::{
        lower_static_into_program, lower_subject_to_permanent_selector, push_static_ability,
    },
    token::lower_predefined_token_def,
    value::{lower_value_const, lower_value_expr},
};

/// Lower a `v2::Replacement` directly onto a permanent or spell [`Program`].
///
/// # Errors
/// Returns [`LoweringError::Unsupported`] whenever the replacement has no faithful native
/// engine representation or conflicts with an already-installed entry rule.
pub(crate) fn lower_replacement_into_program(
    rep: &v2::Replacement,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    lower_replacement_with_condition(rep, None, program, ctx)
}

/// Lower a `v2::StaticAbility::ConditionalReplacement` onto `program`.
pub(super) fn lower_conditional_replacement_into_program(
    condition: &v2::ConditionExpr,
    rep: &v2::Replacement,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    lower_replacement_with_condition(rep, Some(condition), program, ctx)
}

/// Lower a duration-scoped `v2::Replacement` into instructions on `acc` (for
/// `Effect::ApplyReplacement`).
///
/// # Errors
/// Returns [`LoweringError::Unsupported`] if the replacement cannot be represented as a
/// duration-scoped temporary rule or temporary static ability.
pub(crate) fn lower_temporary_replacement(
    rep: &v2::Replacement,
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match rep {
        v2::Replacement::Compound(items) => {
            if items.is_empty() {
                return Err(LoweringError::Unsupported("v2:temp-replacement-empty"));
            }
            for item in items {
                lower_temporary_replacement(item, duration, acc, ctx)?;
            }
            Ok(())
        }
        v2::Replacement::Rule(v2::ReplacementRuleKind::LeaveBattlefieldExileInstead) => {
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::exile_if_leaves_battlefield(EffectSubject::AbilitySource),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::Replacement::DamageRule(v2::DamageReplacement::RedirectNext { amount, from, to }) => {
            let (from_rec, to_tgt) = if let (
                v2::SubjectRef::TargetObject {
                    filter: f1,
                    count: v2::TargetCount::Exact(1),
                    another: false,
                },
                v2::SubjectRef::TargetObject {
                    filter: f2,
                    count: v2::TargetCount::Exact(1),
                    another: true,
                },
            ) = (from, to)
                && f1 == f2
            {
                let (bt, _) = crate::v2_lowering::atoms::lower_subject_to_target(from, ctx)?;
                acc.register_target(bt, v2::TargetCount::Exact(2))?;
                ctx.has_object_target = true;
                (
                    StaticDamageRecipient::TargetObject,
                    DamageRedirectTarget::SecondTargetObject,
                )
            } else {
                register_damage_subject_target(from, acc, ctx)?;
                register_damage_subject_target(to, acc, ctx)?;
                (
                    lower_damage_recipient(from, ctx)?,
                    lower_damage_redirect_target(to)?,
                )
            };
            let amt = lower_value_expr(amount, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::RedirectNextDamage {
                        source: None,
                        source_is_target: false,
                        from: from_rec,
                        to: to_tgt,
                        combat_only: false,
                        amount: Some(Box::new(amt)),
                        one_shot: true,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::Replacement::DamageRule(v2::DamageReplacement::PreventFixed {
            source,
            target,
            amount,
        }) => {
            if let Some(t) = target {
                register_damage_subject_target(t, acc, ctx)?;
            }
            let src_flt = match source {
                Some(f) => Some(lower_object_filter(f, ctx)?),
                None => None,
            };
            let recipient = match target {
                Some(t) => lower_damage_recipient(t, ctx)?,
                None => StaticDamageRecipient::Any,
            };
            let amt = lower_value_expr(amount, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::PreventDamageShield {
                        amount: Some(Box::new(amt)),
                        source: src_flt,
                        source_chosen_color: false,
                        source_chosen_type: false,
                        source_is_target: false,
                        half_round_down: false,
                        recipient,
                        combat_only: false,
                        one_shot: true,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::Replacement::EventInstead { event, instead } => {
            if let Some(temp_rule) = try_lower_temp_exile_replacement(event, instead, ctx)? {
                acc.push(Instruction::ApplyContinuous(Box::new(
                    ApplyContinuousSpec::temporary_rule(temp_rule, duration),
                )));
                return Ok(());
            }
            if let Some((ability, extra)) = try_lower_zone_replacement(event, instead, true, ctx)? {
                if extra.is_some() {
                    return Err(LoweringError::Unsupported(
                        "v2:temp-zone-replacement-with-extra",
                    ));
                }
                acc.push(Instruction::ApplyContinuous(Box::new(
                    ApplyContinuousSpec::temporary_rule(
                        TemporaryRuleKind::TemporaryZoneReplacement(Box::new(ability)),
                        duration,
                    ),
                )));
                return Ok(());
            }
            if let Some(static_ability) =
                try_lower_event_replacement_static(event, instead, None, ctx)?
            {
                acc.push(Instruction::ApplyContinuous(Box::new(
                    ApplyContinuousSpec::temporary_rule(
                        TemporaryRuleKind::temporary_static(static_ability),
                        duration,
                    ),
                )));
                return Ok(());
            }
            Err(LoweringError::Unsupported(
                "v2:temp-replacement-event-instead",
            ))
        }
        v2::Replacement::ConditionalEnters { condition, inner } => {
            let static_cond = lower_static_source_condition(condition, ctx)?;
            let static_ability = lower_replacement_to_static_ability(inner, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::conditional_temporary_static(static_cond, static_ability),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::Replacement::LoseLifeInsteadOfGain(_)
        | v2::Replacement::ModifyDamage { .. }
        | v2::Replacement::TokenRule { .. }
        | v2::Replacement::CounterRule { .. }
        | v2::Replacement::DamageRule(_)
        | v2::Replacement::PlayerRule { .. }
        | v2::Replacement::EntersWithCounters {
            subject: Some(_), ..
        } => {
            let static_ability = lower_replacement_to_static_ability(rep, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::temporary_static(static_ability),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::Replacement::EntersTapped { .. }
        | v2::Replacement::EntersWithCounters { subject: None, .. }
        | v2::Replacement::AsEnters(_)
        | v2::Replacement::EnterAsCopy { .. }
        | v2::Replacement::Rule(_) => Err(LoweringError::Unsupported(
            "v2:temp-replacement-unsupported",
        )),
    }
}

fn lower_replacement_with_condition(
    rep: &v2::Replacement,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match rep {
        v2::Replacement::EntersTapped { unless } => {
            lower_enters_tapped(unless.as_ref(), condition, program, ctx)
        }
        v2::Replacement::EntersWithCounters {
            subject,
            kind,
            count,
            additional: _,
        } => lower_enters_with_counters(subject.as_ref(), kind, count, condition, program, ctx),
        v2::Replacement::AsEnters(effects) => {
            if condition.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-conditional-as-enters",
                ));
            }
            lower_as_enters(effects, program, ctx)
        }
        v2::Replacement::EnterAsCopy {
            optional,
            filter,
            exceptions,
        } => {
            if condition.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-conditional-enter-copy",
                ));
            }
            lower_enter_as_copy(*optional, filter, exceptions, program, ctx)
        }
        v2::Replacement::LoseLifeInsteadOfGain(_)
        | v2::Replacement::ModifyDamage { .. }
        | v2::Replacement::TokenRule { .. }
        | v2::Replacement::CounterRule { .. }
        | v2::Replacement::PlayerRule { .. } => {
            let ability = lower_replacement_to_static_ability(rep, ctx)?;
            let cond = match condition {
                Some(c) => Some(lower_static_source_condition(c, ctx)?),
                None => None,
            };
            push_static_ability(program, ability, cond)
        }
        v2::Replacement::DamageRule(damage_rep) => {
            lower_damage_replacement_into_program(damage_rep, condition, program, ctx)
        }
        v2::Replacement::Rule(rule_kind) => {
            lower_replacement_rule_kind(rule_kind, condition, program, ctx)
        }
        v2::Replacement::Compound(items) => {
            if items.is_empty() {
                return Err(LoweringError::Unsupported("v2:replacement-empty-compound"));
            }
            for item in items {
                lower_replacement_with_condition(item, condition, program, ctx)?;
            }
            Ok(())
        }
        v2::Replacement::ConditionalEnters {
            condition: inner_cond,
            inner,
        } => {
            if let Some(outer_cond) = condition {
                let merged = v2::ConditionExpr::And(
                    Box::new(outer_cond.clone()),
                    Box::new(inner_cond.clone()),
                );
                lower_replacement_with_condition(inner, Some(&merged), program, ctx)
            } else {
                lower_replacement_with_condition(inner, Some(inner_cond), program, ctx)
            }
        }
        v2::Replacement::EventInstead { event, instead } => {
            lower_event_instead_into_program(event, instead, condition, program, ctx)
        }
    }
}

fn lower_enters_tapped(
    unless: Option<&v2::ConditionExpr>,
    outer_cond: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let ProgramKind::Permanent(perm) = &mut program.kind else {
        return Err(LoweringError::Unsupported(
            "v2:replacement-enters-tapped-nonpermanent",
        ));
    };

    match (unless, outer_cond) {
        (None, None) => {
            match perm.entry_tap {
                None => perm.entry_tap = Some(EntryTap::Always),
                Some(EntryTap::Always) => {}
                Some(_) => {
                    return Err(LoweringError::Unsupported(
                        "v2:replacement-entry-tap-conflict",
                    ));
                }
            }
            Ok(())
        }
        (Some(cond), None) => {
            if program.entry_tap_unless.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-duplicate-entry-tap-unless",
                ));
            }
            let rc = lower_resolution_condition(cond, ctx)?;
            if !rc.is_world_evaluable() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-entry-tap-unless-nonworld",
                ));
            }
            program.entry_tap_unless = Some(Box::new(rc));
            Ok(())
        }
        (None, Some(if_cond)) => {
            // "If <cond>, ~ enters tapped" <=> enters untapped unless <cond> is true <=> enters tapped unless !<cond>.
            if program.entry_tap_unless.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-duplicate-entry-tap-unless",
                ));
            }
            let rc = lower_resolution_condition(if_cond, ctx)?;
            if !rc.is_world_evaluable() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-conditional-enters-tapped-nonworld",
                ));
            }
            program.entry_tap_unless = Some(Box::new(invert_resolution_condition(rc)));
            Ok(())
        }
        (Some(_), Some(_)) => Err(LoweringError::Unsupported(
            "v2:replacement-nested-enters-tapped-condition",
        )),
    }
}

fn invert_resolution_condition(mut rc: ResolutionCondition) -> ResolutionCondition {
    rc.comparison = match rc.comparison {
        Comparison::Equal => Comparison::NotEqual,
        Comparison::NotEqual => Comparison::Equal,
        Comparison::Less => Comparison::GreaterOrEqual,
        Comparison::LessOrEqual => Comparison::Greater,
        Comparison::Greater => Comparison::LessOrEqual,
        Comparison::GreaterOrEqual => Comparison::Less,
    };
    rc
}

fn lower_enters_with_counters(
    subject: Option<&v2::SubjectRef>,
    kind: &v2::CounterKind,
    count: &v2::ValueExpr,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let ek = lower_counter_kind(kind)?;
    if !ek.is_permanent() {
        return Err(LoweringError::Unsupported(
            "v2:replacement-nonpermanent-counter",
        ));
    }

    let is_self = subject.is_none() || matches!(subject, Some(v2::SubjectRef::SelfObject));
    if is_self {
        if !matches!(program.kind, ProgramKind::Permanent(_)) {
            return Err(LoweringError::Unsupported(
                "v2:replacement-entry-counters-nonpermanent",
            ));
        }
        if let Some(cond) = condition {
            let rc = lower_resolution_condition(cond, ctx)?;
            let amt = lower_value_expr(count, ctx)?;
            program
                .entry_counter_abilities
                .push(EntryCounterAbility::ConditionalCounter {
                    condition: Box::new(rc),
                    kind: ek,
                    amount: amt,
                });
            return Ok(());
        }
        let ability = build_self_entry_counter_ability(ek, count, ctx)?;
        program.entry_counter_abilities.push(ability);
        return Ok(());
    }

    let Some(subj) = subject else {
        return Err(LoweringError::Unsupported(
            "v2:replacement-enters-counters-subject",
        ));
    };
    let selector = lower_subject_to_permanent_selector(subj, ctx)?;
    let rule = build_others_enter_with_counters_rule(selector, ek, count, ctx)?;
    let static_cond = match condition {
        Some(c) => Some(lower_static_source_condition(c, ctx)?),
        None => None,
    };
    push_static_ability(
        program,
        StaticContinuousAbility::Modifier {
            id: ctx.alloc_static_id(),
            rule,
        },
        static_cond,
    )
}

pub(super) fn build_self_entry_counter_ability(
    ek: EngineCounterKind,
    count: &v2::ValueExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EntryCounterAbility> {
    if matches!(count, v2::ValueExpr::Var(v2::VarName::X)) && ek == EngineCounterKind::PlusOne {
        return Ok(EntryCounterAbility::ChosenXPlusOne);
    }
    if let Some(n) = lower_value_const(count) {
        if n <= 0 {
            return Err(LoweringError::Unsupported(
                "v2:replacement-nonpositive-entry-counters",
            ));
        }
        let ri = RulesInteger::from(i64::from(n));
        return Ok(match ek {
            EngineCounterKind::PlusOne => EntryCounterAbility::FixedPlusOne(ri),
            EngineCounterKind::MinusOne => EntryCounterAbility::FixedMinusOne(ri),
            EngineCounterKind::Charge => EntryCounterAbility::FixedCharge(ri),
            EngineCounterKind::Loyalty => EntryCounterAbility::FixedLoyalty(ri),
            other
                if other.is_permanent()
                    && !matches!(
                        other,
                        EngineCounterKind::PlusOne | EngineCounterKind::MinusOne
                    ) =>
            {
                EntryCounterAbility::FixedNamed {
                    kind: other,
                    amount: ri,
                }
            }
            other => EntryCounterAbility::DynamicCounter {
                kind: other,
                amount: EngineValueExpr::Literal(ri),
            },
        });
    }
    let expr = lower_value_expr(count, ctx)?;
    Ok(EntryCounterAbility::DynamicCounter {
        kind: ek,
        amount: expr,
    })
}

pub(super) fn build_others_enter_with_counters_rule(
    selector: spellwake_engine::characteristics::PermanentSelector,
    ek: EngineCounterKind,
    count: &v2::ValueExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<StaticModifierRule> {
    if let Some(n) = lower_value_const(count) {
        if n <= 0 {
            return Err(LoweringError::Unsupported(
                "v2:replacement-others-counters-nonpositive",
            ));
        }
        if let Ok(n_u8) = u8::try_from(n) {
            return Ok(StaticModifierRule::OthersEnterWithCounters {
                selector: Box::new(selector),
                kind: ek,
                count: n_u8,
            });
        }
    }
    let expr = lower_value_expr(count, ctx)?;
    if !expr.is_world_evaluable() {
        return Err(LoweringError::Unsupported(
            "v2:replacement-others-counters-nonworld",
        ));
    }
    Ok(StaticModifierRule::OthersEnterWithCountersExpr {
        selector: Box::new(selector),
        kind: ek,
        amount: Box::new(expr),
    })
}

fn lower_as_enters(
    effects: &[v2::Effect],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if effects.is_empty() {
        return Err(LoweringError::Unsupported("v2:replacement-as-enters-empty"));
    }
    let mut i = 0usize;
    while i < effects.len() {
        // Two-effect shockland / MDFC bolt-land / reveal-land entry replacement pairs:
        // `[Optional { actor: You, body: [pay_life | reveal_hand] }, IfPrevious { succeeded: false, body: [enters_tapped] }]`
        if let (
            v2::Effect::Optional {
                actor: v2::PlayerRef::You,
                body: opt_body,
            },
            Some(v2::Effect::IfPrevious {
                succeeded: false,
                body: else_body,
            }),
        ) = (&effects[i], effects.get(i + 1))
            && is_self_enters_tapped_body(else_body)
        {
            if let Some(life) = extract_optional_pay_life_u8(opt_body) {
                set_entry_tap(program, EntryTap::UnlessPayLife(life))?;
                i += 2;
                continue;
            }
            if let Some(tap) = extract_optional_hand_reveal_entry_tap(opt_body, ctx) {
                set_entry_tap(program, tap)?;
                i += 2;
                continue;
            }
            if let Some(tap) = extract_optional_behold_entry_tap(opt_body, ctx) {
                set_entry_tap(program, tap)?;
                i += 2;
                continue;
            }
        }

        match &effects[i] {
            v2::Effect::PromptChoice {
                chooser: v2::PlayerRef::You,
                kind,
            } => {
                let tap = match kind {
                    v2::ChoiceKind::ChooseCharacteristic(ch) => match ch {
                        v2::CharacteristicKind::Color => EntryTap::ChooseColor,
                        v2::CharacteristicKind::CreatureType => EntryTap::ChooseCreatureType,
                        v2::CharacteristicKind::CardType => EntryTap::ChooseCardType,
                        v2::CharacteristicKind::NonlandCardName => EntryTap::ChooseNonlandCardName,
                        v2::CharacteristicKind::BasicLandType => EntryTap::ChooseBasicLandType,
                        v2::CharacteristicKind::OddOrEven => EntryTap::ChooseOddOrEven,
                        v2::CharacteristicKind::CardName
                        | v2::CharacteristicKind::BasicLandTypes(_)
                        | v2::CharacteristicKind::BinaryMode
                        | v2::CharacteristicKind::Number => {
                            return Err(LoweringError::Unsupported(
                                "v2:replacement-as-enters-choose-characteristic",
                            ));
                        }
                    },
                    v2::ChoiceKind::ChooseObject(v2::SubjectRef::Player(p)) => match p {
                        v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                            EntryTap::ChooseOpponent
                        }
                        v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
                            EntryTap::ChoosePlayer
                        }
                        _ => {
                            return Err(LoweringError::Unsupported(
                                "v2:replacement-as-enters-choose-player",
                            ));
                        }
                    },
                    _ => {
                        return Err(LoweringError::Unsupported(
                            "v2:replacement-as-enters-choice",
                        ));
                    }
                };
                set_entry_tap(program, tap)?;
            }
            v2::Effect::ApplyReplacement {
                replacement,
                duration: v2::Duration::Permanent | v2::Duration::Instant,
            } => {
                lower_replacement_into_program(replacement, program, ctx)?;
            }
            v2::Effect::ApplyReplacement {
                replacement,
                duration: v2::Duration::UntilEndOfTurn,
            } if matches!(
                replacement.as_ref(),
                v2::Replacement::EntersTapped { .. }
                    | v2::Replacement::EntersWithCounters { .. }
                    | v2::Replacement::ConditionalEnters { .. }
            ) =>
            {
                lower_replacement_into_program(replacement, program, ctx)?;
            }
            v2::Effect::ApplyContinuous {
                effect,
                duration: v2::Duration::Permanent,
            } => {
                lower_static_into_program(effect, program, ctx)?;
            }
            v2::Effect::MutateObject {
                subject,
                mutation: v2::ObjectMutation::PutCounters { kind, count },
                duration: v2::Duration::Instant | v2::Duration::Permanent,
            } if is_self_or_entry_anaphora(subject) => {
                lower_enters_with_counters(None, kind, count, None, program, ctx)?;
            }
            v2::Effect::MutateObject {
                subject,
                mutation: v2::ObjectMutation::Tap,
                duration: v2::Duration::Instant | v2::Duration::Permanent,
            } if is_self_or_entry_anaphora(subject) => {
                set_entry_tap(program, EntryTap::Always)?;
            }
            v2::Effect::Unless {
                body,
                payer: Some(v2::PlayerRef::You) | None,
                cost: Some(v2::Cost::PayLife(v2::ValueExpr::Const(n))),
                condition: None,
            } if *n > 0 && *n <= i32::from(u8::MAX) && is_self_enters_tapped_body(body) => {
                set_entry_tap(program, EntryTap::UnlessPayLife(*n as u8))?;
            }
            v2::Effect::Unless {
                body,
                payer: Some(v2::PlayerRef::You) | None,
                cost:
                    Some(v2::Cost::RevealFromHand {
                        count: v2::ValueExpr::Const(1),
                        filter,
                    }),
                condition: None,
            } if is_self_enters_tapped_body(body) => {
                let tap = extract_reveal_filter_entry_tap(filter, true, ctx).ok_or(
                    LoweringError::Unsupported("v2:replacement-as-enters-effect"),
                )?;
                set_entry_tap(program, tap)?;
            }
            v2::Effect::Unless {
                body,
                payer: Some(v2::PlayerRef::You) | None,
                cost: None,
                condition: Some(cond),
            } if is_self_enters_tapped_body(body) => {
                lower_enters_tapped(Some(cond), None, program, ctx)?;
            }
            v2::Effect::UnlessEffect { body, unless } if is_self_enters_tapped_body(body) => {
                if let Some(life) = extract_optional_pay_life_u8(unless) {
                    set_entry_tap(program, EntryTap::UnlessPayLife(life))?;
                } else if let Some(tap) = extract_optional_hand_reveal_entry_tap(unless, ctx) {
                    set_entry_tap(program, tap)?;
                } else if let Some(tap) = extract_optional_behold_entry_tap(unless, ctx) {
                    set_entry_tap(program, tap)?;
                } else {
                    return Err(LoweringError::Unsupported(
                        "v2:replacement-as-enters-effect",
                    ));
                }
            }
            v2::Effect::BranchCondition {
                condition,
                then_body,
                else_body,
            } if is_self_enters_tapped_body(then_body) && else_body.is_empty() => {
                lower_enters_tapped(None, Some(condition), program, ctx)?;
            }
            v2::Effect::BranchCondition {
                condition,
                then_body,
                else_body,
            } if then_body.is_empty() && is_self_enters_tapped_body(else_body) => {
                lower_enters_tapped(Some(condition), None, program, ctx)?;
            }
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-as-enters-effect",
                ));
            }
        }
        i += 1;
    }
    Ok(())
}

fn is_self_or_entry_anaphora(subject: &v2::SubjectRef) -> bool {
    matches!(
        subject,
        v2::SubjectRef::SelfObject
            | v2::SubjectRef::Anaphora(
                v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatObject
            )
    )
}

fn is_self_enters_tapped_body(body: &[v2::Effect]) -> bool {
    let [eff] = body else {
        return false;
    };
    match eff {
        v2::Effect::ApplyReplacement {
            replacement,
            duration: v2::Duration::Instant | v2::Duration::Permanent | v2::Duration::UntilEndOfTurn,
        } => matches!(
            replacement.as_ref(),
            v2::Replacement::EntersTapped { unless: None }
        ),
        v2::Effect::ApplyContinuous {
            effect,
            duration: v2::Duration::Instant | v2::Duration::Permanent | v2::Duration::UntilEndOfTurn,
        } => matches!(
            effect.as_ref(),
            v2::StaticAbility::SubjectRule {
                subject,
                rule: v2::SubjectStaticRule::EntersTapped,
                condition: None,
            } if is_self_or_entry_anaphora(subject)
        ),
        v2::Effect::MutateObject {
            subject,
            mutation: v2::ObjectMutation::Tap,
            duration: v2::Duration::Instant | v2::Duration::Permanent | v2::Duration::UntilEndOfTurn,
        } if is_self_or_entry_anaphora(subject) => true,
        _ => false,
    }
}

fn extract_optional_pay_life_u8(body: &[v2::Effect]) -> Option<u8> {
    let [eff] = body else {
        return None;
    };
    match eff {
        v2::Effect::MutatePlayer {
            player: v2::PlayerRef::You,
            mutation: v2::PlayerMutation::PayCost(v2::Cost::PayLife(v2::ValueExpr::Const(n))),
            duration: v2::Duration::Instant | v2::Duration::Permanent,
        } if *n > 0 => u8::try_from(*n).ok(),
        _ => None,
    }
}

fn extract_optional_behold_entry_tap(body: &[v2::Effect], ctx: &LowerCtx<'_>) -> Option<EntryTap> {
    let [eff] = body else {
        return None;
    };
    let v2::Effect::MutatePlayer {
        player: v2::PlayerRef::You,
        mutation: v2::PlayerMutation::Behold(filter),
        duration: v2::Duration::Instant | v2::Duration::Permanent,
    } = eff
    else {
        return None;
    };
    if !filter.zones.is_empty()
        || !filter.forbidden_types.is_empty()
        || !filter.required_supertypes.is_empty()
        || !filter.forbidden_supertypes.is_empty()
        || !filter.forbidden_subtypes.is_empty()
        || !filter.any_colors.is_empty()
        || !filter.forbidden_colors.is_empty()
        || filter.color_arity.is_some()
        || filter.controller.is_some()
        || filter.owner.is_some()
        || filter.combat.is_some()
        || filter.tapped.is_some()
        || filter.token.is_some()
        || !filter.required_keywords.is_empty()
        || !filter.forbidden_keywords.is_empty()
        || !filter.stat_bounds.is_empty()
        || !filter.flags.is_empty()
        || !filter.predicates.is_empty()
    {
        return None;
    }
    let types = filter.all_types.union(filter.any_types);
    if (types.is_empty() || types == AstCardTypeMask::CREATURE)
        && filter.subtypes.len() == 1
        && !filter.subtypes_match_any
        && let Some(ct) = ctx.lookup_creature_type(&filter.subtypes[0])
    {
        return Some(EntryTap::UnlessRevealHandSubtype {
            subtype: ct,
            or_control: true,
        });
    }
    None
}

fn extract_optional_hand_reveal_entry_tap(
    body: &[v2::Effect],
    ctx: &LowerCtx<'_>,
) -> Option<EntryTap> {
    let [eff] = body else {
        return None;
    };
    let v2::Effect::ZoneOp {
        actor: v2::PlayerRef::You,
        op: v2::ZoneOpKind::Reveal,
        subject,
        from,
        to: None,
        modifiers,
        except: None,
        count,
    } = eff
    else {
        return None;
    };
    if modifiers.tapped
        || modifiers.attacking
        || modifiers.face_down
        || modifiers.transformed
        || modifiers.with_counter.is_some()
        || modifiers.attached_to.is_some()
    {
        return None;
    }
    if let Some(c) = count
        && (c.up_to || c.any_number || !matches!(c.count, v2::ValueExpr::Const(1)))
    {
        return None;
    }
    let is_own_hand = |zr: &v2::ZoneRef| {
        zr.zone == v2::ZoneKind::Hand
            && matches!(zr.owner, v2::ZoneOwner::You | v2::ZoneOwner::Default)
    };
    if let v2::SubjectRef::Compound(parts) = subject
        && parts.len() == 2
        && let (
            v2::SubjectRef::Group(f1) | v2::SubjectRef::All(f1) | v2::SubjectRef::Each(f1),
            v2::SubjectRef::Group(f2) | v2::SubjectRef::All(f2) | v2::SubjectRef::Each(f2),
        ) = (&parts[0], &parts[1])
    {
        let from_hand_pair = (from.len() == 1 && is_own_hand(&from[0]))
            || (f1.zones.iter().any(is_own_hand) && f2.zones.iter().any(is_own_hand));
        if from_hand_pair
            && let (
                Some(EntryTap::UnlessRevealHandLandType(b1, _)),
                Some(EntryTap::UnlessRevealHandLandType(b2, _)),
            ) = (
                extract_reveal_filter_entry_tap(f1, true, ctx),
                extract_reveal_filter_entry_tap(f2, true, ctx),
            )
        {
            return Some(EntryTap::UnlessRevealHandLandType(b1, b2));
        }
        return None;
    }
    let filter = match subject {
        v2::SubjectRef::Group(f) | v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) => f.as_ref(),
        v2::SubjectRef::Counted { filter, count: c }
            if !c.up_to && !c.any_number && matches!(c.count, v2::ValueExpr::Const(1)) =>
        {
            filter.as_ref()
        }
        _ => return None,
    };
    let from_hand = match (from.as_slice(), filter.zones.as_slice()) {
        ([zr], []) if is_own_hand(zr) => true,
        ([], [zr]) if is_own_hand(zr) => true,
        ([zr1], [zr2]) if is_own_hand(zr1) && is_own_hand(zr2) => true,
        _ => false,
    };
    if !from_hand {
        return None;
    }
    extract_reveal_filter_entry_tap(filter, false, ctx)
}

fn extract_reveal_filter_entry_tap(
    filter: &v2::ObjectFilter,
    allow_zones_empty: bool,
    ctx: &LowerCtx<'_>,
) -> Option<EntryTap> {
    if !allow_zones_empty
        && !filter.zones.is_empty()
        && !filter.zones.iter().all(|zr| {
            zr.zone == v2::ZoneKind::Hand
                && matches!(zr.owner, v2::ZoneOwner::You | v2::ZoneOwner::Default)
        })
    {
        return None;
    }
    if !filter.forbidden_types.is_empty()
        || !filter.required_supertypes.is_empty()
        || !filter.forbidden_supertypes.is_empty()
        || !filter.forbidden_subtypes.is_empty()
        || !filter.any_colors.is_empty()
        || !filter.forbidden_colors.is_empty()
        || filter.color_arity.is_some()
        || filter.controller.is_some()
        || !matches!(
            filter.owner,
            None | Some(ControllerRelation::You | ControllerRelation::OwnerYou)
        )
        || filter.combat.is_some()
        || filter.tapped.is_some()
        || filter.token.is_some()
        || !filter.required_keywords.is_empty()
        || !filter.forbidden_keywords.is_empty()
        || !filter.stat_bounds.is_empty()
        || !filter.flags.is_empty()
        || !filter.predicates.is_empty()
    {
        return None;
    }
    let types = filter.all_types.union(filter.any_types);
    if filter.subtypes.len() == 2
        && filter.subtypes_match_any
        && (types.is_empty() || types == AstCardTypeMask::LAND)
    {
        let b1 = LowerCtx::lookup_land_type(&filter.subtypes[0]).and_then(LandType::basic)?;
        let b2 = LowerCtx::lookup_land_type(&filter.subtypes[1]).and_then(LandType::basic)?;
        return Some(EntryTap::UnlessRevealHandLandType(b1, b2));
    }
    if filter.subtypes.len() == 1 && !filter.subtypes_match_any {
        if (types.is_empty() || types == AstCardTypeMask::LAND)
            && let Some(b) =
                LowerCtx::lookup_land_type(&filter.subtypes[0]).and_then(LandType::basic)
        {
            return Some(EntryTap::UnlessRevealHandLandType(b, b));
        }
        if types.is_empty()
            && let Some(ct) = ctx.lookup_creature_type(&filter.subtypes[0])
        {
            return Some(EntryTap::UnlessRevealHandSubtype {
                subtype: ct,
                or_control: false,
            });
        }
    }
    None
}

pub(super) fn set_entry_tap(program: &mut Program, tap: EntryTap) -> LowerResult<()> {
    let ProgramKind::Permanent(perm) = &mut program.kind else {
        return Err(LoweringError::Unsupported(
            "v2:replacement-entry-tap-nonpermanent",
        ));
    };
    match perm.entry_tap {
        None => {
            perm.entry_tap = Some(tap);
            Ok(())
        }
        Some(existing) if existing == tap => Ok(()),
        Some(_) => Err(LoweringError::Unsupported(
            "v2:replacement-entry-tap-conflict",
        )),
    }
}

fn lower_enter_as_copy(
    optional: bool,
    filter: &v2::ObjectFilter,
    exceptions: &[v2::CopyException],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if !matches!(program.kind, ProgramKind::Permanent(_)) {
        return Err(LoweringError::Unsupported(
            "v2:replacement-enter-copy-nonpermanent",
        ));
    }
    if program.entry_copy.is_some() {
        return Err(LoweringError::Unsupported(
            "v2:replacement-duplicate-entry-copy",
        ));
    }

    let zone = match filter.zones.as_slice() {
        []
        | [
            v2::ZoneRef {
                zone: v2::ZoneKind::Battlefield,
                ..
            },
        ] => EntryCopyZone::Battlefield,
        [
            v2::ZoneRef {
                zone: v2::ZoneKind::Graveyard,
                owner: v2::ZoneOwner::You | v2::ZoneOwner::Default,
            },
        ] => EntryCopyZone::OwnGraveyard,
        [
            v2::ZoneRef {
                zone: v2::ZoneKind::Graveyard,
                owner: v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents,
            },
        ] => EntryCopyZone::OpponentGraveyard,
        [
            v2::ZoneRef {
                zone: v2::ZoneKind::Graveyard,
                owner: v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::EachPlayer,
            },
        ] => EntryCopyZone::AnyGraveyard,
        _ => {
            return Err(LoweringError::Unsupported("v2:replacement-enter-copy-zone"));
        }
    };

    let mut stripped_filter = filter.clone();
    stripped_filter.zones.clear();
    let engine_filter = lower_object_filter(&stripped_filter, ctx)?;
    let mut spec = EntryCopySpecExt::new(zone, engine_filter);
    spec.optional = optional;

    for exc in exceptions {
        match exc {
            v2::CopyException::AddKeywords(kws) => {
                for kw in kws {
                    spec.extra_keywords.insert(lower_keyword_ability(kw, ctx)?);
                }
            }
            v2::CopyException::NotLegendary => {
                spec.nonlegendary = true;
            }
            v2::CopyException::AddTypes(mask) => {
                spec.extra_types.extend(lower_card_type_set(*mask));
            }
            v2::CopyException::AddAbility(face) => {
                let granted = lower_granted_face(face, ctx)?;
                spec.extra_keywords.extend(granted.keywords);
                spec.extra_activated.extend(granted.activated);
                spec.extra_triggered.extend(granted.triggered);
                spec.extra_static.extend(granted.statics);
                spec.extra_mana.extend(granted.mana);
            }
            v2::CopyException::KeepOwnName => {
                spec.keep_name = true;
            }
            v2::CopyException::EntersWithCounters {
                kind,
                count,
                condition,
            } => {
                let ek = lower_counter_kind(kind)?;
                if !ek.is_permanent() {
                    return Err(LoweringError::Unsupported(
                        "v2:entry-copy-nonpermanent-counter",
                    ));
                }
                let n = lower_value_const(count)
                    .and_then(|v| u8::try_from(v).ok())
                    .filter(|&v| v > 0)
                    .ok_or(LoweringError::Unsupported("v2:entry-copy-counter-count"))?;
                if let Some(cond) = condition {
                    let rc = lower_resolution_condition(cond, ctx)?;
                    if !rc.is_world_evaluable() {
                        return Err(LoweringError::Unsupported(
                            "v2:entry-copy-counter-cond-nonworld",
                        ));
                    }
                    spec.conditional_counters.push((rc, ek, n));
                } else {
                    spec.extra_counters.push((ek, n));
                }
            }
            v2::CopyException::AddSupertypes {
                supertypes,
                keep_existing_types,
            } => {
                if !*keep_existing_types {
                    return Err(LoweringError::Unsupported(
                        "v2:entry-copy-replace-supertypes",
                    ));
                }
                spec.extra_supertypes
                    .extend(lower_supertype_set(*supertypes));
            }
            v2::CopyException::SetColor(mask) => {
                spec.override_colors = Some(lower_color_set(*mask));
            }
            v2::CopyException::SetBasePTAndTypes {
                power,
                toughness,
                subtypes,
                types,
                keep_existing_types,
            } => {
                match (power, toughness) {
                    (Some(p), Some(t)) => {
                        spec.override_pt = Some((
                            RulesInteger::from(i64::from(*p)),
                            RulesInteger::from(i64::from(*t)),
                        ));
                    }
                    (None, None) => {}
                    _ => {
                        return Err(LoweringError::Unsupported("v2:entry-copy-partial-pt"));
                    }
                }
                if !*keep_existing_types && !types.is_empty() {
                    return Err(LoweringError::Unsupported("v2:entry-copy-replace-types"));
                }
                spec.extra_types.extend(lower_card_type_set(*types));
                for raw in subtypes {
                    if let Some(ct) = ctx.lookup_creature_type(raw) {
                        spec.extra_creature_subtypes.insert(ct);
                    } else if let Some(at) = LowerCtx::lookup_artifact_subtype(raw) {
                        spec.extra_artifact_subtypes.insert(at);
                    } else if let Some(et) = LowerCtx::lookup_enchantment_subtype(raw) {
                        spec.extra_enchantment_subtypes.insert(et);
                    } else {
                        return Err(LoweringError::Unsupported("v2:entry-copy-unknown-subtype"));
                    }
                }
            }
            v2::CopyException::HasThisAbility => {
                return Err(LoweringError::Unsupported("v2:entry-copy-has-this-ability"));
            }
            v2::CopyException::AddKeywordsConditional { .. } => {
                return Err(LoweringError::Unsupported(
                    "v2:entry-copy-conditional-keywords",
                ));
            }
            v2::CopyException::KeepOwnOtherAbilities => {
                return Err(LoweringError::Unsupported(
                    "v2:entry-copy-keep-own-other-abilities",
                ));
            }
        }
    }

    program.entry_copy = Some(EntryCopyRule::MatchingExt(Box::new(spec)));
    Ok(())
}

fn lower_replacement_to_static_ability(
    rep: &v2::Replacement,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<StaticContinuousAbility> {
    match rep {
        v2::Replacement::LoseLifeInsteadOfGain(player) => {
            let rel = lower_replacement_player_relation(*player)?;
            Ok(StaticContinuousAbility::Modifier {
                id: ctx.alloc_static_id(),
                rule: StaticModifierRule::ModifyLifeGain {
                    player: rel,
                    add: 0,
                    multiply: 1,
                    lose_instead: true,
                },
            })
        }
        v2::Replacement::ModifyDamage {
            source,
            target,
            add,
            factor,
        } => {
            let op = lower_modify_damage_op(*add, *factor)?;
            let (src_flt, src_ctrl) = lower_damage_source_filter(source.as_ref(), ctx)?;
            let recipient = match target {
                Some(t) => lower_damage_recipient(t, ctx)?,
                None => StaticDamageRecipient::Any,
            };
            Ok(StaticContinuousAbility::DamageRule {
                id: ctx.alloc_static_id(),
                rule: StaticDamageRule::ModifyDamage {
                    source: src_flt,
                    source_controller: src_ctrl,
                    recipient,
                    kind: StaticDamageKind::Any,
                    op,
                },
            })
        }
        v2::Replacement::TokenRule {
            controller,
            filter,
            rule,
        } => {
            let ctrl = lower_replacement_player_relation(*controller)?;
            let mod_rule = match rule {
                v2::TokenReplacementKind::Multiply(factor) => {
                    if *factor <= 1 {
                        return Err(LoweringError::Unsupported(
                            "v2:replacement-token-multiply-noop",
                        ));
                    }
                    match filter {
                        None => StaticModifierRule::ModifyTokens {
                            controller: ctrl,
                            add: 0,
                            multiply: *factor,
                        },
                        Some(f) => StaticModifierRule::ModifyTokensFiltered {
                            filter: Box::new(lower_object_filter(f, ctx)?),
                            controller: ctrl,
                            add: 0,
                            multiply: *factor,
                        },
                    }
                }
                v2::TokenReplacementKind::Bonus { spec, count } => {
                    let n = lower_value_const(count)
                        .and_then(|v| u8::try_from(v).ok())
                        .filter(|&v| v > 0)
                        .ok_or(LoweringError::Unsupported(
                            "v2:replacement-token-bonus-count",
                        ))?;
                    let token_def = match spec.as_ref() {
                        v2::TokenSpec::Predefined(p) => lower_predefined_token_def(*p, ctx.vocab)?,
                        v2::TokenSpec::Creature { .. } | v2::TokenSpec::CopyOf { .. } => {
                            return Err(LoweringError::Unsupported(
                                "v2:replacement-token-bonus-spec",
                            ));
                        }
                    };
                    match filter {
                        None => StaticModifierRule::BonusTokenOnCreate {
                            controller: ctrl,
                            token: Box::new(token_def),
                            count: n,
                            per_created: false,
                        },
                        Some(f) => StaticModifierRule::BonusTokenFiltered {
                            filter: Box::new(lower_object_filter(f, ctx)?),
                            controller: ctrl,
                            token: Box::new(token_def),
                            count: n,
                            per_created: false,
                        },
                    }
                }
                v2::TokenReplacementKind::Replace(_) => {
                    return Err(LoweringError::Unsupported("v2:replacement-token-replace"));
                }
            };
            Ok(StaticContinuousAbility::Modifier {
                id: ctx.alloc_static_id(),
                rule: mod_rule,
            })
        }
        v2::Replacement::CounterRule {
            filter,
            kind,
            add,
            factor,
        } => {
            let selector =
                lower_permanent_selector(&v2::SubjectRef::All(Box::new(filter.clone())), ctx)?;
            let counter_kind = match kind {
                None | Some(v2::CounterKind::Any) => None,
                Some(k) => {
                    let ek = lower_counter_kind(k)?;
                    if !ek.is_permanent() {
                        return Err(LoweringError::Unsupported(
                            "v2:replacement-counter-rule-nonpermanent",
                        ));
                    }
                    Some(ek)
                }
            };
            if *add < 0 {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-counter-rule-negative-add",
                ));
            }
            let add_u8 = u8::try_from(*add)
                .map_err(|_| LoweringError::Unsupported("v2:replacement-counter-rule-overflow"))?;
            let multiply = if *factor == 0 { 1 } else { *factor };
            if add_u8 == 0 && multiply == 1 {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-counter-rule-noop",
                ));
            }
            Ok(StaticContinuousAbility::Modifier {
                id: ctx.alloc_static_id(),
                rule: StaticModifierRule::ModifyCounters {
                    selector: Box::new(selector),
                    counter_kind,
                    add: add_u8,
                    multiply,
                },
            })
        }
        v2::Replacement::DamageRule(damage_rep) => {
            let rule = lower_damage_replacement_rule(damage_rep, ctx)?;
            Ok(StaticContinuousAbility::DamageRule {
                id: ctx.alloc_static_id(),
                rule,
            })
        }
        v2::Replacement::PlayerRule { player, rule } => {
            lower_player_replacement_static(*player, rule, ctx)
        }
        v2::Replacement::EntersWithCounters {
            subject: Some(subj),
            kind,
            count,
            ..
        } => {
            let ek = lower_counter_kind(kind)?;
            if !ek.is_permanent() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-nonpermanent-counter",
                ));
            }
            let selector = lower_subject_to_permanent_selector(subj, ctx)?;
            let rule = build_others_enter_with_counters_rule(selector, ek, count, ctx)?;
            Ok(StaticContinuousAbility::Modifier {
                id: ctx.alloc_static_id(),
                rule,
            })
        }
        _ => Err(LoweringError::Unsupported(
            "v2:replacement-not-static-ability",
        )),
    }
}

fn lower_damage_replacement_into_program(
    damage_rep: &v2::DamageReplacement,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if let v2::DamageReplacement::UnpreventableBySource(v2::SubjectRef::SelfObject) = damage_rep {
        if condition.is_some() {
            return Err(LoweringError::Unsupported(
                "v2:replacement-conditional-self-unpreventable",
            ));
        }
        program
            .printed_keywords
            .insert(KeywordAbility::DamageCannotBePrevented);
        return Ok(());
    }
    let rule = lower_damage_replacement_rule(damage_rep, ctx)?;
    let cond = match condition {
        Some(c) => Some(lower_static_source_condition(c, ctx)?),
        None => None,
    };
    push_static_ability(
        program,
        StaticContinuousAbility::DamageRule {
            id: ctx.alloc_static_id(),
            rule,
        },
        cond,
    )
}

fn lower_damage_replacement_rule(
    damage_rep: &v2::DamageReplacement,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<StaticDamageRule> {
    match damage_rep {
        v2::DamageReplacement::ModifyDynamic {
            source,
            target,
            add,
        } => {
            let (src_flt, src_ctrl) = lower_damage_source_filter(source.as_ref(), ctx)?;
            let recipient = match target {
                Some(t) => lower_damage_recipient(t, ctx)?,
                None => StaticDamageRecipient::Any,
            };
            let expr = lower_value_expr(add, ctx)?;
            if !expr.is_world_evaluable() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-damage-dynamic-nonworld",
                ));
            }
            Ok(StaticDamageRule::ModifyDamage {
                source: src_flt,
                source_controller: src_ctrl,
                recipient,
                kind: StaticDamageKind::Any,
                op: StaticDamageOp::AddExpr(Box::new(expr)),
            })
        }
        v2::DamageReplacement::Cap {
            source,
            target,
            threshold,
            capped_to,
        } => {
            if *capped_to < 0 || (*threshold != *capped_to && *threshold != *capped_to + 1) {
                return Err(LoweringError::Unsupported("v2:replacement-damage-cap"));
            }
            let cap_u8 = u8::try_from(*capped_to)
                .map_err(|_| LoweringError::Unsupported("v2:replacement-damage-cap-overflow"))?;
            let (src_flt, src_ctrl) = lower_damage_source_filter(source.as_ref(), ctx)?;
            let recipient = match target {
                Some(t) => lower_damage_recipient(t, ctx)?,
                None => StaticDamageRecipient::Any,
            };
            Ok(StaticDamageRule::ModifyDamage {
                source: src_flt,
                source_controller: src_ctrl,
                recipient,
                kind: StaticDamageKind::Any,
                op: StaticDamageOp::CapAt(cap_u8),
            })
        }
        v2::DamageReplacement::UnpreventableBySource(subj) => match subj {
            v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) if f.is_empty() => {
                Ok(StaticDamageRule::DamageCannotBePrevented)
            }
            _ => Err(LoweringError::Unsupported(
                "v2:replacement-damage-unpreventable-source",
            )),
        },
        v2::DamageReplacement::PreventAll { target } => {
            let recipient = lower_damage_recipient(target, ctx)?;
            Ok(StaticDamageRule::ModifyDamage {
                source: None,
                source_controller: PlayerRelation::AnyParticipant,
                recipient,
                kind: StaticDamageKind::Any,
                op: StaticDamageOp::PreventAll,
            })
        }
        v2::DamageReplacement::PreventWithRider {
            target,
            condition,
            rider,
        } => {
            let recipient = lower_damage_recipient(target, ctx)?;
            let body = lower_effect_body(rider, ctx)?;
            if body.target != BodyTarget::None {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-prevent-rider-targeted",
                ));
            }
            let op = StaticDamageOp::PreventAndFollowup {
                mode: DamagePreventMode::All,
                followup: DamagePreventFollowup::InlineBody(Box::new(body)),
            };
            if let Some(cond) = condition {
                let sc = lower_static_source_condition(cond, ctx)?;
                Ok(StaticDamageRule::ConditionalModifyDamage {
                    condition: Box::new(sc),
                    source: None,
                    source_controller: PlayerRelation::AnyParticipant,
                    recipient,
                    kind: StaticDamageKind::Any,
                    op,
                })
            } else {
                Ok(StaticDamageRule::ModifyDamage {
                    source: None,
                    source_controller: PlayerRelation::AnyParticipant,
                    recipient,
                    kind: StaticDamageKind::Any,
                    op,
                })
            }
        }
        v2::DamageReplacement::PreventAllBut {
            source,
            target,
            remaining,
        } => {
            if *remaining != 0 {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-damage-prevent-all-but",
                ));
            }
            let (src_flt, src_ctrl) = lower_damage_source_filter(source.as_ref(), ctx)?;
            let recipient = match target {
                Some(t) => lower_damage_recipient(t, ctx)?,
                None => StaticDamageRecipient::Any,
            };
            Ok(StaticDamageRule::ModifyDamage {
                source: src_flt,
                source_controller: src_ctrl,
                recipient,
                kind: StaticDamageKind::Any,
                op: StaticDamageOp::PreventAll,
            })
        }
        v2::DamageReplacement::PreventFixed {
            source,
            target,
            amount,
        } => {
            let (src_flt, src_ctrl) = lower_damage_source_filter(source.as_ref(), ctx)?;
            let recipient = match target {
                Some(t) => lower_damage_recipient(t, ctx)?,
                None => StaticDamageRecipient::Any,
            };
            let op = if let Some(n) = lower_value_const(amount) {
                if n <= 0 {
                    return Err(LoweringError::Unsupported(
                        "v2:replacement-prevent-fixed-nonpositive",
                    ));
                }
                let n_u8 = u8::try_from(n).map_err(|_| {
                    LoweringError::Unsupported("v2:replacement-prevent-fixed-overflow")
                })?;
                StaticDamageOp::PreventUpTo(n_u8)
            } else {
                let expr = lower_value_expr(amount, ctx)?;
                if !expr.is_world_evaluable() {
                    return Err(LoweringError::Unsupported(
                        "v2:replacement-prevent-fixed-nonworld",
                    ));
                }
                StaticDamageOp::PreventUpToExpr(Box::new(expr))
            };
            Ok(StaticDamageRule::ModifyDamage {
                source: src_flt,
                source_controller: src_ctrl,
                recipient,
                kind: StaticDamageKind::Any,
                op,
            })
        }
        v2::DamageReplacement::RedirectAll { from, to } => {
            let from_rec = lower_damage_recipient(from, ctx)?;
            let to_tgt = lower_damage_redirect_target(to)?;
            Ok(StaticDamageRule::RedirectDamage {
                source: None,
                from: from_rec,
                to: to_tgt,
                combat_only: false,
            })
        }
        v2::DamageReplacement::RedirectNext { .. } => Err(LoweringError::Unsupported(
            "v2:replacement-redirect-next-static",
        )),
        v2::DamageReplacement::ReplaceNext { .. } => Err(LoweringError::Unsupported(
            "v2:replacement-damage-replace-next",
        )),
    }
}

fn lower_player_replacement_static(
    player: v2::PlayerRef,
    rule: &v2::PlayerReplacementKind,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<StaticContinuousAbility> {
    match rule {
        v2::PlayerReplacementKind::SkipStep { step, optional } => {
            if *optional {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-optional-skip-step",
                ));
            }
            let rel = lower_replacement_player_relation(player)?;
            let engine_step = lower_step(*step)?;
            Ok(StaticContinuousAbility::Restriction {
                id: ctx.alloc_static_id(),
                rule: StaticRestriction::SkipStep {
                    player: rel,
                    step: engine_step,
                },
            })
        }
        v2::PlayerReplacementKind::SkipExtraTurn => {
            let affected = lower_world_affected_players(player, ctx)?;
            Ok(StaticContinuousAbility::EventReplacement {
                id: ctx.alloc_static_id(),
                event: ReplaceableEvent::ExtraTurn { player: affected },
                condition: None,
                action: StaticReplacementAction::Skip,
            })
        }
        v2::PlayerReplacementKind::LoseLifeMultiply {
            factor,
            during_your_turn,
        } => {
            if *factor <= 1 {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-lose-life-multiply-noop",
                ));
            }
            let affected = lower_world_affected_players(player, ctx)?;
            let condition = if *during_your_turn {
                Some(Box::new(StaticSourceCondition::ControllersTurn))
            } else {
                None
            };
            Ok(StaticContinuousAbility::EventReplacement {
                id: ctx.alloc_static_id(),
                event: ReplaceableEvent::LifeLoss { player: affected },
                condition,
                action: StaticReplacementAction::ModifyAmount {
                    add: Box::new(EngineValueExpr::Literal(RulesInteger::from(0))),
                    multiply: *factor,
                },
            })
        }
        v2::PlayerReplacementKind::ModifyLifeGain { add, factor } => {
            let rel = lower_replacement_player_relation(player)?;
            if *add < 0 {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-life-gain-negative-add",
                ));
            }
            let add_u8 = u8::try_from(*add)
                .map_err(|_| LoweringError::Unsupported("v2:replacement-life-gain-overflow"))?;
            let multiply = if *factor == 0 { 1 } else { *factor };
            if add_u8 == 0 && multiply == 1 {
                return Err(LoweringError::Unsupported("v2:replacement-life-gain-noop"));
            }
            Ok(StaticContinuousAbility::Modifier {
                id: ctx.alloc_static_id(),
                rule: StaticModifierRule::ModifyLifeGain {
                    player: rel,
                    add: add_u8,
                    multiply,
                    lose_instead: false,
                },
            })
        }
        v2::PlayerReplacementKind::MultiplyMill { factor } => {
            if *factor <= 1 {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-multiply-mill-noop",
                ));
            }
            let affected = lower_world_affected_players(player, ctx)?;
            Ok(StaticContinuousAbility::EventReplacement {
                id: ctx.alloc_static_id(),
                event: ReplaceableEvent::Mill { player: affected },
                condition: None,
                action: StaticReplacementAction::ModifyAmount {
                    add: Box::new(EngineValueExpr::Literal(RulesInteger::from(0))),
                    multiply: *factor,
                },
            })
        }
    }
}

fn lower_replacement_rule_kind(
    rule_kind: &v2::ReplacementRuleKind,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match rule_kind {
        v2::ReplacementRuleKind::LeaveBattlefieldExileInstead => {
            if condition.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-conditional-leave-bf-exile",
                ));
            }
            if !matches!(program.kind, ProgramKind::Permanent(_)) {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-leave-bf-exile-nonpermanent",
                ));
            }
            // Cover graveyard, hand, and library exits to exile.
            program.zone_replacements.push(ZoneReplacementAbility {
                id: ctx.alloc_replacement_id(),
                actions: ZoneActionPredicate::Any,
                objects: ZoneObjectKind::Either,
                subject: ZoneReplacementSubject::SourceSelf,
                destination: PublicZoneDestination::Graveyard,
                replace_with: PublicZoneDestination::Exile,
                choice: ZoneReplacementChoice::Mandatory,
            });
            program.zone_replacements.push(ZoneReplacementAbility {
                id: ctx.alloc_replacement_id(),
                actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                    ZoneActionKind::EffectReturnSelected,
                ])),
                objects: ZoneObjectKind::Either,
                subject: ZoneReplacementSubject::SourceSelf,
                destination: PublicZoneDestination::Hand,
                replace_with: PublicZoneDestination::Exile,
                choice: ZoneReplacementChoice::Mandatory,
            });
            program.zone_replacements.push(ZoneReplacementAbility {
                id: ctx.alloc_replacement_id(),
                actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                    ZoneActionKind::EffectPutIntoLibrary,
                ])),
                objects: ZoneObjectKind::Either,
                subject: ZoneReplacementSubject::SourceSelf,
                destination: PublicZoneDestination::Library,
                replace_with: PublicZoneDestination::Exile,
                choice: ZoneReplacementChoice::Mandatory,
            });
            Ok(())
        }
        v2::ReplacementRuleKind::OpeningHandOnBattlefield => {
            if condition.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-conditional-opening-hand",
                ));
            }
            if !matches!(program.kind, ProgramKind::Permanent(_)) {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-opening-hand-nonpermanent",
                ));
            }
            program.opening_hand_battlefield = true;
            Ok(())
        }
        v2::ReplacementRuleKind::BecomesDayOnEnter => {
            let cond = match condition {
                Some(c) => Some(lower_static_source_condition(c, ctx)?),
                None => None,
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ctx.alloc_static_id(),
                    rule: StaticModifierRule::BecomesDayIfNeither,
                },
                cond,
            )
        }
        v2::ReplacementRuleKind::LifeFloor(floor) => {
            if *floor <= 0 {
                return Err(LoweringError::Unsupported(
                    "v2:replacement-life-floor-nonpositive",
                ));
            }
            let floor_u8 = u8::try_from(*floor)
                .map_err(|_| LoweringError::Unsupported("v2:replacement-life-floor-overflow"))?;
            let cond = match condition {
                Some(c) => Some(lower_static_source_condition(c, ctx)?),
                None => None,
            };
            push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticDamageRule::MinimumLifeTotal {
                        floor: floor_u8,
                        require_creature: false,
                    },
                },
                cond,
            )
        }
        v2::ReplacementRuleKind::ExploreAgain(flt) => {
            let ef = lower_object_filter(flt, ctx)?;
            let cond = match condition {
                Some(c) => Some(lower_static_source_condition(c, ctx)?),
                None => None,
            };
            push_static_ability(
                program,
                StaticContinuousAbility::EventReplacement {
                    id: ctx.alloc_static_id(),
                    event: ReplaceableEvent::Explore { filter: ef },
                    condition: cond.map(Box::new),
                    action: StaticReplacementAction::ModifyAmount {
                        add: Box::new(EngineValueExpr::Literal(RulesInteger::from(1))),
                        multiply: 1,
                    },
                },
                None,
            )
        }
        v2::ReplacementRuleKind::EntersWithChoiceCounter { .. } => Err(LoweringError::Unsupported(
            "v2:replacement-enters-with-choice-counter",
        )),
        v2::ReplacementRuleKind::EntersWithKeywords(_) => Err(LoweringError::Unsupported(
            "v2:replacement-enters-with-keywords",
        )),
        v2::ReplacementRuleKind::EscapesWithCounters { .. } => Err(LoweringError::Unsupported(
            "v2:replacement-escapes-with-counters",
        )),
        v2::ReplacementRuleKind::ManaOverride(_) => {
            Err(LoweringError::Unsupported("v2:replacement-mana-override"))
        }
    }
}

fn lower_event_instead_into_program(
    event: &v2::TriggerEvent,
    instead: &[v2::Effect],
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let is_permanent = matches!(program.kind, ProgramKind::Permanent(_));
    if condition.is_none()
        && let Some((ability, extra)) =
            try_lower_zone_replacement(event, instead, is_permanent, ctx)?
    {
        let id = ability.id;
        program.zone_replacements.push(ability);
        if let Some(ex) = extra {
            program.zone_replacement_extras.insert(id, ex);
        }
        return Ok(());
    }

    let static_cond = match condition {
        Some(c) => Some(lower_static_source_condition(c, ctx)?),
        None => None,
    };
    if let Some(ability) = try_lower_event_replacement_static(event, instead, static_cond, ctx)? {
        return push_static_ability(program, ability, None);
    }

    Err(LoweringError::Unsupported("v2:replacement-event-instead"))
}

fn try_lower_temp_exile_replacement(
    event: &v2::TriggerEvent,
    instead: &[v2::Effect],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<TemporaryRuleKind>> {
    let [
        v2::Effect::ZoneOp {
            op:
                v2::ZoneOpKind::Exile {
                    face_down: false,
                    counters: None,
                },
            subject: dest_subject,
            ..
        },
    ] = instead
    else {
        return Ok(None);
    };
    if !is_replaced_event_anaphora(dest_subject) {
        return Ok(None);
    }
    match event {
        v2::TriggerEvent::Dies { subject } => {
            let eff_subj = lower_replacement_effect_subject(subject, ctx)?;
            Ok(Some(TemporaryRuleKind::exile_if_dies(eff_subj)))
        }
        v2::TriggerEvent::ZoneChange {
            from: Some(v2::ZoneKind::Battlefield),
            to: None,
            subject,
        } => {
            let eff_subj = lower_replacement_effect_subject(subject, ctx)?;
            Ok(Some(TemporaryRuleKind::exile_if_leaves_battlefield(
                eff_subj,
            )))
        }
        _ => Ok(None),
    }
}

fn lower_replacement_effect_subject(
    subject: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EffectSubject> {
    match subject {
        v2::SubjectRef::SelfObject => Ok(EffectSubject::AbilitySource),
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatObject,
        ) => Ok(EffectSubject::TargetObject),
        v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => {
            Ok(EffectSubject::Matching(lower_object_filter(f, ctx)?))
        }
        _ => Err(LoweringError::Unsupported(
            "v2:temp-exile-replacement-subject",
        )),
    }
}

fn try_lower_zone_replacement(
    event: &v2::TriggerEvent,
    instead: &[v2::Effect],
    is_permanent: bool,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<(ZoneReplacementAbility, Option<ZoneReplacementExtra>)>> {
    if instead.is_empty() {
        return Ok(None);
    }
    let (subject_ir, orig_dest, actions, objects) = match event {
        v2::TriggerEvent::Dies { subject } => (
            subject,
            PublicZoneDestination::Graveyard,
            ZoneActionPredicate::OneOf(BTreeSet::from([
                ZoneActionKind::EffectDestroy,
                ZoneActionKind::EffectSacrifice,
                ZoneActionKind::SacrificeCost,
                ZoneActionKind::StateBased,
            ])),
            ZoneObjectKind::Either,
        ),
        v2::TriggerEvent::ZoneChange {
            from: None,
            to: Some(v2::ZoneKind::Graveyard),
            subject,
        } => (
            subject,
            PublicZoneDestination::Graveyard,
            ZoneActionPredicate::Any,
            ZoneObjectKind::Either,
        ),
        v2::TriggerEvent::ZoneChange {
            from: Some(v2::ZoneKind::Battlefield),
            to: Some(v2::ZoneKind::Graveyard),
            subject,
        } => (
            subject,
            PublicZoneDestination::Graveyard,
            ZoneActionPredicate::OneOf(BTreeSet::from([
                ZoneActionKind::EffectDestroy,
                ZoneActionKind::EffectSacrifice,
                ZoneActionKind::SacrificeCost,
                ZoneActionKind::StateBased,
            ])),
            ZoneObjectKind::Either,
        ),
        v2::TriggerEvent::ForcedDiscard { subject, .. } => (
            subject,
            PublicZoneDestination::Graveyard,
            ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectDiscard])),
            ZoneObjectKind::Card,
        ),
        _ => return Ok(None),
    };

    // Extract destination replacement from first effect in `instead` (possibly wrapped in Optional
    // or preceded by a pure reveal of the replaced card, e.g. Darksteel Colossus / Progenitus).
    let (choice, raw_effects): (ZoneReplacementChoice, &[v2::Effect]) = match instead {
        [v2::Effect::Optional { body, .. }] if !body.is_empty() => (
            ZoneReplacementChoice::OptionalAffectedPlayer,
            body.as_slice(),
        ),
        slice => (ZoneReplacementChoice::Mandatory, slice),
    };
    let inner_effects = if raw_effects.len() >= 2
        && matches!(
            &raw_effects[0],
            v2::Effect::ZoneOp {
                op: v2::ZoneOpKind::Reveal,
                subject,
                to: None,
                except: None,
                count: None,
                ..
            } if matches!(subject, v2::SubjectRef::SelfObject) || is_replaced_event_anaphora(subject)
        ) {
        &raw_effects[1..]
    } else {
        raw_effects
    };

    let first = &inner_effects[0];
    let matches_replaced = |s: &v2::SubjectRef| {
        matches!(s, v2::SubjectRef::SelfObject) || is_replaced_event_anaphora(s)
    };
    let (replace_with, exile_counters) = match first {
        v2::Effect::ZoneOp {
            op:
                v2::ZoneOpKind::Exile {
                    face_down: false,
                    counters,
                },
            subject: dest_subj,
            ..
        } if matches_replaced(dest_subj) => {
            let ctr = match counters {
                None => None,
                Some((k, v)) => {
                    let ek = lower_counter_kind(k)?;
                    if !ek.is_permanent() {
                        return Err(LoweringError::Unsupported(
                            "v2:zone-replacement-nonpermanent-counter",
                        ));
                    }
                    let n = lower_value_const(v)
                        .and_then(|c| u8::try_from(c).ok())
                        .filter(|&c| c > 0)
                        .ok_or(LoweringError::Unsupported(
                            "v2:zone-replacement-counter-count",
                        ))?;
                    Some((ek, n))
                }
            };
            (PublicZoneDestination::Exile, ctr)
        }
        v2::Effect::ZoneOp {
            op: v2::ZoneOpKind::Move,
            subject: dest_subj,
            to: Some(dest),
            ..
        } if matches_replaced(dest_subj) => {
            let pub_dest = match dest.zone {
                v2::ZoneKind::Exile => PublicZoneDestination::Exile,
                v2::ZoneKind::Hand => PublicZoneDestination::Hand,
                v2::ZoneKind::Library => PublicZoneDestination::Library,
                v2::ZoneKind::Battlefield => PublicZoneDestination::Battlefield,
                _ => return Ok(None),
            };
            (pub_dest, None)
        }
        v2::Effect::ZoneOp {
            op: v2::ZoneOpKind::Shuffle,
            subject: dest_subj,
            to: Some(dest),
            ..
        } if matches_replaced(dest_subj) && dest.zone == v2::ZoneKind::Library => {
            (PublicZoneDestination::Library, None)
        }
        _ => return Ok(None),
    };

    if replace_with == orig_dest {
        return Ok(None);
    }

    let z_subject = match (event, subject_ir) {
        (v2::TriggerEvent::ForcedDiscard { .. }, v2::SubjectRef::SelfObject) => {
            ZoneReplacementSubject::SourceSelfDiscardByOpponent
        }
        (_, v2::SubjectRef::SelfObject) => ZoneReplacementSubject::SourceSelf,
        (_, v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f)) => {
            if f.is_empty() {
                ZoneReplacementSubject::AnyObject
            } else if f
                .predicates
                .contains(&v2::FilterPredicate::DealtDamageBySelfThisTurn)
            {
                ZoneReplacementSubject::DamagedBySourceThisTurn
            } else {
                let ef = lower_object_filter(f, ctx)?;
                ZoneReplacementSubject::Matching {
                    filter: Box::new(ef),
                    owner_relation: None,
                }
            }
        }
        _ => return Ok(None),
    };

    // Enforce engine `zone_replacement::validate_ability` scope before emitting.
    if !is_permanent
        && !(matches!(
            z_subject,
            ZoneReplacementSubject::SourceSelf
                | ZoneReplacementSubject::SourceSelfDiscardByOpponent
        ) && orig_dest == PublicZoneDestination::Graveyard
            && matches!(
                replace_with,
                PublicZoneDestination::Exile
                    | PublicZoneDestination::Library
                    | PublicZoneDestination::Hand
            ))
    {
        return Err(LoweringError::Unsupported(
            "v2:zone-replacement-nonpermanent-scope",
        ));
    }
    if actions == ZoneActionPredicate::Any && choice != ZoneReplacementChoice::Mandatory {
        return Err(LoweringError::Unsupported(
            "v2:zone-replacement-optional-any-action",
        ));
    }
    if replace_with == PublicZoneDestination::Library
        && !(orig_dest == PublicZoneDestination::Graveyard
            && choice == ZoneReplacementChoice::Mandatory)
    {
        return Err(LoweringError::Unsupported(
            "v2:zone-replacement-library-scope",
        ));
    }
    if replace_with == PublicZoneDestination::Battlefield
        && !(matches!(
            z_subject,
            ZoneReplacementSubject::SourceSelf
                | ZoneReplacementSubject::SourceSelfDiscardByOpponent
        ) && orig_dest == PublicZoneDestination::Graveyard
            && actions
                == ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectDiscard])))
    {
        return Err(LoweringError::Unsupported(
            "v2:zone-replacement-battlefield-scope",
        ));
    }

    let followup_body = if inner_effects.len() > 1 {
        let body = lower_effect_body(&inner_effects[1..], ctx)?;
        if body.target != BodyTarget::None {
            return Err(LoweringError::Unsupported(
                "v2:zone-replacement-targeted-followup",
            ));
        }
        Some(Box::new(body))
    } else {
        None
    };

    let extra = if exile_counters.is_some() || followup_body.is_some() {
        Some(ZoneReplacementExtra {
            exile_counters,
            followup_body,
        })
    } else {
        None
    };

    Ok(Some((
        ZoneReplacementAbility {
            id: ctx.alloc_replacement_id(),
            actions,
            objects,
            subject: z_subject,
            destination: orig_dest,
            replace_with,
            choice,
        },
        extra,
    )))
}

fn try_lower_event_replacement_static(
    event: &v2::TriggerEvent,
    instead: &[v2::Effect],
    condition: Option<StaticSourceCondition>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<StaticContinuousAbility>> {
    let replaceable = match event {
        v2::TriggerEvent::PlayerAction {
            player,
            action: v2::PlayerTriggerKind::CardDrawn,
        } => ReplaceableEvent::Draw {
            player: lower_world_affected_players(*player, ctx)?,
        },
        v2::TriggerEvent::LifeChanged {
            player,
            gained: true,
        } => ReplaceableEvent::LifeGain {
            player: lower_world_affected_players(*player, ctx)?,
        },
        v2::TriggerEvent::LifeChanged {
            player,
            gained: false,
        } => ReplaceableEvent::LifeLoss {
            player: lower_world_affected_players(*player, ctx)?,
        },
        v2::TriggerEvent::PlayerAction {
            player,
            action: v2::PlayerTriggerKind::Scry,
        } => ReplaceableEvent::Scry {
            player: lower_world_affected_players(*player, ctx)?,
        },
        v2::TriggerEvent::PlayerAction {
            player,
            action: v2::PlayerTriggerKind::LosesGame,
        } => ReplaceableEvent::LoseGame {
            player: lower_world_affected_players(*player, ctx)?,
        },
        v2::TriggerEvent::PlayerObjectEvent {
            player,
            kind: v2::PlayerObjectTriggerKind::MillsCard,
            filter: None,
        } => ReplaceableEvent::Mill {
            player: lower_world_affected_players(*player, ctx)?,
        },
        _ => return Ok(None),
    };

    let (optional, body_slice): (bool, &[v2::Effect]) = match instead {
        [v2::Effect::Optional { body, .. }] => (true, body.as_slice()),
        slice => (false, slice),
    };
    let exec_body = lower_effect_body(body_slice, ctx)?;
    if exec_body.target != BodyTarget::None {
        return Err(LoweringError::Unsupported(
            "v2:replacement-targeted-instead-body",
        ));
    }

    Ok(Some(StaticContinuousAbility::EventReplacement {
        id: ctx.alloc_static_id(),
        event: replaceable,
        condition: condition.map(Box::new),
        action: StaticReplacementAction::ReplaceWithBody {
            optional,
            body: Box::new(exec_body),
        },
    }))
}

fn lower_modify_damage_op(add: i16, factor: u8) -> LowerResult<StaticDamageOp> {
    if factor > 1 && add == 0 {
        return Ok(StaticDamageOp::Multiply(factor));
    }
    if factor <= 1 && add > 0 {
        let add_u8 = u8::try_from(add)
            .map_err(|_| LoweringError::Unsupported("v2:replacement-damage-add-overflow"))?;
        return Ok(StaticDamageOp::Add(add_u8));
    }
    if factor <= 1 && add < 0 {
        let sub_u8 = u8::try_from(-add)
            .map_err(|_| LoweringError::Unsupported("v2:replacement-damage-sub-overflow"))?;
        return Ok(StaticDamageOp::Subtract(sub_u8));
    }
    Err(LoweringError::Unsupported(
        "v2:replacement-modify-damage-op",
    ))
}

pub(super) fn lower_damage_source_filter(
    source: Option<&v2::ObjectFilter>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(Option<ObjectFilter>, PlayerRelation)> {
    let Some(flt) = source else {
        return Ok((None, PlayerRelation::AnyParticipant));
    };
    let ctrl = match flt.controller {
        None => PlayerRelation::AnyParticipant,
        Some(ControllerRelation::You) => PlayerRelation::Controller,
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
            PlayerRelation::Opponent
        }
        Some(_) => {
            return Err(LoweringError::Unsupported(
                "v2:replacement-damage-source-controller",
            ));
        }
    };
    let mut stripped = flt.clone();
    stripped.controller = None;
    let src_flt = if stripped.is_empty() {
        None
    } else {
        Some(lower_object_filter(&stripped, ctx)?)
    };
    Ok((src_flt, ctrl))
}

pub(super) fn lower_damage_recipient(
    target: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<StaticDamageRecipient> {
    match target {
        v2::SubjectRef::SelfObject => Ok(StaticDamageRecipient::SelfObject),
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::EnchantedCreature
            | v2::AnaphoraRef::EnchantedPermanent
            | v2::AnaphoraRef::EquippedCreature,
        ) => Ok(StaticDamageRecipient::AttachedHost),
        v2::SubjectRef::Player(p) => match p {
            v2::PlayerRef::You => Ok(StaticDamageRecipient::Controller),
            v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                Ok(StaticDamageRecipient::Opponents)
            }
            v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
                Ok(StaticDamageRecipient::Players)
            }
            _ => Err(LoweringError::Unsupported(
                "v2:replacement-damage-recipient-player",
            )),
        },
        v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => {
            if f.is_empty() {
                Ok(StaticDamageRecipient::Any)
            } else {
                Ok(StaticDamageRecipient::MatchingPermanent(
                    lower_object_filter(f, ctx)?,
                ))
            }
        }
        v2::SubjectRef::TargetObject { .. } => Ok(StaticDamageRecipient::TargetObject),
        v2::SubjectRef::AnyTarget => Ok(StaticDamageRecipient::TargetAny),
        v2::SubjectRef::Compound(parts) if parts.len() == 2 => {
            lower_compound_damage_recipient(&parts[0], &parts[1], ctx)
        }
        _ => Err(LoweringError::Unsupported(
            "v2:replacement-damage-recipient",
        )),
    }
}

fn is_plain_damage_recipient_filter(f: &v2::ObjectFilter) -> bool {
    f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
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

fn lower_damage_recipient_permanent_clause(
    part: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> Option<spellwake_engine::program::ObjectClause> {
    let filter = match part {
        v2::SubjectRef::SelfObject => ObjectFilter::self_object(),
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::EnchantedCreature | v2::AnaphoraRef::EquippedCreature,
        ) => ObjectFilter::creature().attached_to_self(),
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::EnchantedPermanent) => {
            ObjectFilter::any().attached_to_self()
        }
        v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => {
            lower_object_filter(f, ctx).ok()?
        }
        _ => return None,
    };
    match filter {
        ObjectFilter::Clause(clause) => Some(clause),
        ObjectFilter::Either(_, _) => None,
    }
}

fn lower_compound_damage_recipient(
    a: &v2::SubjectRef,
    b: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<StaticDamageRecipient> {
    match (a, b) {
        (
            v2::SubjectRef::Player(v2::PlayerRef::You),
            v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f),
        )
        | (
            v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f),
            v2::SubjectRef::Player(v2::PlayerRef::You),
        ) if matches!(f.controller, Some(ControllerRelation::You))
            && is_plain_damage_recipient_filter(f) =>
        {
            if f.any_permanent || (f.all_types.is_empty() && f.any_types.is_empty()) {
                return Ok(StaticDamageRecipient::ControllerOrControlledPermanent);
            }
            if f.all_types.union(f.any_types) == AstCardTypeMask::CREATURE {
                return Ok(StaticDamageRecipient::ControllerOrControlledCreatures);
            }
            Err(LoweringError::Unsupported(
                "v2:replacement-damage-compound-recipient",
            ))
        }
        (
            v2::SubjectRef::Player(v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent),
            v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f),
        )
        | (
            v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f),
            v2::SubjectRef::Player(v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent),
        ) if matches!(
            f.controller,
            Some(
                ControllerRelation::Opponent
                    | ControllerRelation::Opponents
                    | ControllerRelation::ThatPlayer
                    | ControllerRelation::ItsController
            )
        ) && is_plain_damage_recipient_filter(f)
            && (f.any_permanent || (f.all_types.is_empty() && f.any_types.is_empty())) =>
        {
            Ok(StaticDamageRecipient::OpponentOrOpponentPermanent)
        }
        (
            v2::SubjectRef::Player(v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer),
            v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f),
        )
        | (
            v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f),
            v2::SubjectRef::Player(v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer),
        ) if f.controller.is_none()
            && is_plain_damage_recipient_filter(f)
            && (f.any_permanent || (f.all_types.is_empty() && f.any_types.is_empty())) =>
        {
            Ok(StaticDamageRecipient::Any)
        }
        _ => {
            if let (Some(c1), Some(c2)) = (
                lower_damage_recipient_permanent_clause(a, ctx),
                lower_damage_recipient_permanent_clause(b, ctx),
            ) {
                return Ok(StaticDamageRecipient::MatchingPermanent(
                    ObjectFilter::Either(c1, c2),
                ));
            }
            Err(LoweringError::Unsupported(
                "v2:replacement-damage-compound-recipient",
            ))
        }
    }
}

fn register_damage_subject_target(
    subj: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match subj {
        v2::SubjectRef::TargetObject { .. } => {
            let (bt, tc) = crate::v2_lowering::atoms::lower_subject_to_target(subj, ctx)?;
            acc.register_target(bt, tc)?;
            ctx.has_object_target = true;
            Ok(())
        }
        v2::SubjectRef::AnyTarget => {
            acc.register_target(BodyTarget::SINGLE_ANY_TARGET, v2::TargetCount::Exact(1))?;
            ctx.has_object_target = true;
            Ok(())
        }
        v2::SubjectRef::TargetPlayer {
            opponent_only,
            count,
        } => {
            acc.register_target(
                BodyTarget::Player {
                    opponents_only: *opponent_only,
                },
                *count,
            )?;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn lower_damage_redirect_target(to: &v2::SubjectRef) -> LowerResult<DamageRedirectTarget> {
    match to {
        v2::SubjectRef::SelfObject => Ok(DamageRedirectTarget::AbilitySource),
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::EnchantedCreature
            | v2::AnaphoraRef::EnchantedPermanent
            | v2::AnaphoraRef::EquippedCreature,
        ) => Ok(DamageRedirectTarget::AttachedHost),
        v2::SubjectRef::Player(v2::PlayerRef::You) => Ok(DamageRedirectTarget::Controller),
        v2::SubjectRef::TargetObject { .. } => Ok(DamageRedirectTarget::TargetObject),
        v2::SubjectRef::AnyTarget => Ok(DamageRedirectTarget::TargetAny),
        v2::SubjectRef::Player(
            v2::PlayerRef::ItsController | v2::PlayerRef::ThatObjectController,
        ) => Ok(DamageRedirectTarget::DamageSourceController),
        _ => Err(LoweringError::Unsupported(
            "v2:replacement-damage-redirect-target",
        )),
    }
}

fn lower_replacement_player_relation(player: v2::PlayerRef) -> LowerResult<PlayerRelation> {
    match player {
        v2::PlayerRef::You => Ok(PlayerRelation::Controller),
        v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => Ok(PlayerRelation::Opponent),
        v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => Ok(PlayerRelation::AnyParticipant),
        _ => Err(LoweringError::Unsupported("v2:replacement-player-relation")),
    }
}

fn is_replaced_event_anaphora(subj: &v2::SubjectRef) -> bool {
    matches!(
        subj,
        v2::SubjectRef::SelfObject
            | v2::SubjectRef::Anaphora(
                v2::AnaphoraRef::It
                    | v2::AnaphoraRef::Them
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatObject
                    | v2::AnaphoraRef::ThoseCreatures
                    | v2::AnaphoraRef::ThosePermanents
                    | v2::AnaphoraRef::ThoseCards
            )
    )
}
