//! Strictly-typed lowering from `v2::ir::StaticAbility` to native `spellwake_engine`
//! static abilities (`StaticContinuousAbility`, `CastReduction`, `AttackRequirement`,
//! `SelfFlashPermission`, `GraveyardSelfCast`, `EntryTap`, `TemporaryRuleKind`, etc.).
//!
//! Every unhandled `v2::ir::StaticAbility` variant or modifier fails closed with
//! `Err(LoweringError::Unsupported("v2:..."))`. `StaticFlagKind::CostReductionRule`
//! explicitly returns `Err(LoweringError::Unsupported("v2:static-cost-reduction-rule"))`.

use std::collections::BTreeSet;
use std::num::NonZeroU8;

use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    activated::ActivatedAbilityId,
    characteristics::{
        AbilityChange, AbilityRemoval, ActivatedCostScope, BlockingRestriction, CastPermissionRule,
        Color, ColorOperation, ControllerSelector, DefiningPart, DefiningParts,
        InstructionSubtypeOperand, KeywordAbility, NumericExpression, NumericPair,
        PermanentSelector, PowerToughness, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        SpellCostModifier, SpellDiscount, StaticColorOperand, StaticContinuousAbility,
        StaticDamageKind, StaticDamageOp, StaticDamageRecipient, StaticDamageRule, StaticDrawRule,
        StaticLandSubtypeOperand, StaticModifierRule, StaticPart, StaticParts, StaticRestriction,
        StaticSourceCondition, StaticSubtypeOperand, StaticTypeChange, StaticUntapRule, StaticZone,
        SubtypeOperation, TriggerMultiplierEvent, TriggerMultiplierRule, TypeChangeSpec,
        TypeOperation,
    },
    land::EntryTap,
    mana::ManaCost,
    program::{
        AffectedPlayers, AttackCostSymbol, AttackRequirement, CastReduction,
        ColorMask as EngineColorMask, GraveyardSelfCast, Instruction, NextSpellCastKeyword,
        ObjectFilter, PlayerRelation, Program, ProgramKind, ResolvingPlayer, SelfFlashPermission,
        SpellFilter, TemporaryRuleKind, ValueExpr as EngineValueExpr,
    },
    toolbox::{ApplyContinuousSpec, MutateObjectSpec},
};
use spellwake_oracle_compiler::{
    ast::{CardTypeMask as AstCardTypeMask, ControllerRelation},
    v2::ir as v2,
};

use crate::v2_lowering::{
    atoms::{
        lower_affected_players, lower_card_type_set, lower_color_mask, lower_color_set,
        lower_counter_kind, lower_keyword_abilities, lower_keyword_ability, lower_resolving_player,
        lower_source_zone_filter, lower_step, lower_subject_to_query,
    },
    card::lower_granted_face_with_restrictions,
    condition::{lower_resolution_condition, lower_static_source_condition},
    cost::{lower_additional_cast_costs, lower_alternative_cast_costs, lower_v2_fixed_mana_cost},
    ctx::{BodyAcc, LowerCtx},
    error::{LowerResult, LoweringError},
    filter::{lower_object_filter, lower_permanent_selector, lower_spell_filter},
    keywords::{GrantedKeywordParts, lower_granted_keywords},
    replacement::{
        build_others_enter_with_counters_rule, build_self_entry_counter_ability,
        lower_conditional_replacement_into_program, lower_damage_recipient,
        lower_damage_source_filter, set_entry_tap,
    },
    value::{lower_pt_delta, lower_value_const, lower_value_expr},
};

fn compound_subject_has_ambiguous_trailing_controller(parts: &[v2::SubjectRef]) -> bool {
    let mut seen_unscoped_group = false;
    for part in parts {
        if let v2::SubjectRef::Group(f) | v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) = part {
            if f.controller.is_none() && f.owner.is_none() {
                seen_unscoped_group = true;
            } else if seen_unscoped_group {
                return true;
            }
        }
    }
    false
}

/// Lower a `v2::StaticAbility` directly onto a permanent or spell [`Program`].
///
/// # Errors
/// Returns [`LoweringError::Unsupported`] whenever the static ability has no faithful
/// native engine representation.
pub(crate) fn lower_static_into_program(
    ability: &v2::StaticAbility,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    // Fan out `SubjectRef::Compound(parts)` onto one static ability per conjunct whenever no
    // earlier unscoped group could be missing a shared trailing controller/owner modifier.
    match ability {
        v2::StaticAbility::ContinuousModifyPT {
            subject: v2::SubjectRef::Compound(parts),
            power,
            toughness,
            grant_keywords,
            scale,
            condition,
        } if !parts.is_empty() && !compound_subject_has_ambiguous_trailing_controller(parts) => {
            for part in parts {
                lower_static_into_program(
                    &v2::StaticAbility::ContinuousModifyPT {
                        subject: part.clone(),
                        power: *power,
                        toughness: *toughness,
                        grant_keywords: grant_keywords.clone(),
                        scale: scale.clone(),
                        condition: condition.clone(),
                    },
                    program,
                    ctx,
                )?;
            }
            return Ok(());
        }
        v2::StaticAbility::GrantKeywords {
            subject: v2::SubjectRef::Compound(parts),
            keywords,
            condition,
        } if !parts.is_empty() && !compound_subject_has_ambiguous_trailing_controller(parts) => {
            for part in parts {
                lower_static_into_program(
                    &v2::StaticAbility::GrantKeywords {
                        subject: part.clone(),
                        keywords: keywords.clone(),
                        condition: condition.clone(),
                    },
                    program,
                    ctx,
                )?;
            }
            return Ok(());
        }
        v2::StaticAbility::GrantAbility {
            subject: v2::SubjectRef::Compound(parts),
            ability: face,
            condition,
        } if !parts.is_empty() && !compound_subject_has_ambiguous_trailing_controller(parts) => {
            for part in parts {
                lower_static_into_program(
                    &v2::StaticAbility::GrantAbility {
                        subject: part.clone(),
                        ability: face.clone(),
                        condition: condition.clone(),
                    },
                    program,
                    ctx,
                )?;
            }
            return Ok(());
        }
        v2::StaticAbility::RuleRestriction {
            subject: Some(v2::SubjectRef::Compound(parts)),
            rule,
            condition,
        } if !parts.is_empty() && !compound_subject_has_ambiguous_trailing_controller(parts) => {
            for part in parts {
                lower_static_into_program(
                    &v2::StaticAbility::RuleRestriction {
                        subject: Some(part.clone()),
                        rule: rule.clone(),
                        condition: condition.clone(),
                    },
                    program,
                    ctx,
                )?;
            }
            return Ok(());
        }
        v2::StaticAbility::SubjectRule {
            subject: v2::SubjectRef::Compound(parts),
            rule,
            condition,
        } if !parts.is_empty() && !compound_subject_has_ambiguous_trailing_controller(parts) => {
            for part in parts {
                lower_static_into_program(
                    &v2::StaticAbility::SubjectRule {
                        subject: part.clone(),
                        rule: rule.clone(),
                        condition: condition.clone(),
                    },
                    program,
                    ctx,
                )?;
            }
            return Ok(());
        }
        v2::StaticAbility::RemoveAllAbilities {
            subject: v2::SubjectRef::Compound(parts),
        } if !parts.is_empty() && !compound_subject_has_ambiguous_trailing_controller(parts) => {
            for part in parts {
                lower_static_into_program(
                    &v2::StaticAbility::RemoveAllAbilities {
                        subject: part.clone(),
                    },
                    program,
                    ctx,
                )?;
            }
            return Ok(());
        }
        v2::StaticAbility::SetBasePT {
            subject: v2::SubjectRef::Compound(parts),
            power,
            toughness,
        } if !parts.is_empty() && !compound_subject_has_ambiguous_trailing_controller(parts) => {
            for part in parts {
                lower_static_into_program(
                    &v2::StaticAbility::SetBasePT {
                        subject: part.clone(),
                        power: *power,
                        toughness: *toughness,
                    },
                    program,
                    ctx,
                )?;
            }
            return Ok(());
        }
        v2::StaticAbility::EntersWithCountersStatic {
            subject: v2::SubjectRef::Compound(parts),
            kind,
            count,
            additional,
            condition,
        } if !parts.is_empty() && !compound_subject_has_ambiguous_trailing_controller(parts) => {
            for part in parts {
                lower_static_into_program(
                    &v2::StaticAbility::EntersWithCountersStatic {
                        subject: part.clone(),
                        kind: kind.clone(),
                        count: count.clone(),
                        additional: *additional,
                        condition: condition.clone(),
                    },
                    program,
                    ctx,
                )?;
            }
            return Ok(());
        }
        _ => {}
    }

    match ability {
        v2::StaticAbility::ContinuousModifyPT {
            subject,
            power,
            toughness,
            grant_keywords,
            scale,
            condition,
        } => {
            let selector = lower_subject_to_permanent_selector(subject, ctx)?;
            let pair = lower_static_pt_pair(*power, *toughness, scale.as_deref(), ctx)?;
            let mut parts = Vec::new();
            let zero_delta = matches!(
                (&pair.power, &pair.toughness),
                (NumericExpression::Literal(p), NumericExpression::Literal(t))
                    if *p == RulesInteger::from(0) && *t == RulesInteger::from(0)
            );
            if !zero_delta || grant_keywords.is_empty() {
                parts.push(StaticPart::ModifyPT(pair));
            }
            if !grant_keywords.is_empty() {
                let granted = lower_granted_keywords(grant_keywords, ctx)?;
                append_granted_keywords_static(&mut parts, granted);
            }
            let cond = lower_opt_static_cond(condition.as_ref(), ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector,
                    parts: StaticParts(parts),
                },
                cond,
            )
        }
        v2::StaticAbility::GrantKeywords {
            subject,
            keywords,
            condition,
        } => lower_grant_keywords_into_program(subject, keywords, condition.as_ref(), program, ctx),
        v2::StaticAbility::GrantAbility {
            subject,
            ability: face,
            condition,
        } => lower_grant_ability_into_program(subject, face, condition.as_ref(), program, ctx),
        v2::StaticAbility::CostModifier {
            subject,
            delta,
            less,
            scale,
            min_mana,
            condition,
        } => lower_cost_modifier_into_program(
            subject,
            delta,
            *less,
            scale.as_ref(),
            *min_mana,
            condition.as_ref(),
            program,
            ctx,
        ),
        v2::StaticAbility::RuleRestriction {
            subject,
            rule,
            condition,
        } => lower_rule_restriction_into_program(
            subject.as_ref(),
            rule,
            condition.as_ref(),
            program,
            ctx,
        ),
        v2::StaticAbility::PlayerRestriction {
            player,
            restriction,
            condition,
        } => lower_player_restriction_into_program(
            *player,
            *restriction,
            condition.as_ref(),
            program,
            ctx,
        ),
        v2::StaticAbility::SkipStep { player, step } => {
            let rel = lower_static_player_relation(*player)?;
            let engine_step = lower_step(*step)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::SkipStep {
                        player: rel,
                        step: engine_step,
                    },
                },
                None,
            )
        }
        v2::StaticAbility::SetSubtypes {
            subject,
            subtypes,
            keep_existing,
        } => lower_set_subtypes_into_program(subject, subtypes, *keep_existing, program, ctx),
        v2::StaticAbility::RemoveAllAbilities { subject } => {
            let selector = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector,
                    parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))]),
                },
                None,
            )
        }
        v2::StaticAbility::SetBasePT {
            subject,
            power,
            toughness,
        } => {
            let selector = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector,
                    parts: StaticParts(vec![StaticPart::SetPT(NumericPair {
                        power: NumericExpression::Literal(RulesInteger::from(i64::from(*power))),
                        toughness: NumericExpression::Literal(RulesInteger::from(i64::from(
                            *toughness,
                        ))),
                    })]),
                },
                None,
            )
        }
        v2::StaticAbility::AdditionalCost(costs) => {
            lower_additional_cast_costs(costs, program, ctx)
        }
        v2::StaticAbility::AlternativeCost(costs) => {
            lower_alternative_cast_costs(costs, program, ctx)
        }
        v2::StaticAbility::DynamicBasePT { power, toughness } => {
            if power.is_none() && toughness.is_none() {
                return Err(LoweringError::Unsupported("v2:static-empty-dynamic-pt"));
            }
            let p_expr = match power {
                Some(expr) => Some(Box::new(lower_world_numeric_expr(expr, ctx)?)),
                None => None,
            };
            let t_expr = match toughness {
                Some(expr) => Some(Box::new(lower_world_numeric_expr(expr, ctx)?)),
                None => None,
            };
            push_static_ability(
                program,
                StaticContinuousAbility::CharacteristicDefining {
                    id: ctx.alloc_static_id(),
                    parts: DefiningParts(vec![DefiningPart::PowerToughness {
                        power: p_expr,
                        toughness: t_expr,
                    }]),
                },
                None,
            )
        }
        v2::StaticAbility::EntersWithCountersStatic {
            subject,
            kind,
            count,
            additional: _,
            condition,
        } => {
            let ek = lower_counter_kind(kind)?;
            if !ek.is_permanent() {
                return Err(LoweringError::Unsupported(
                    "v2:static-enters-counters-nonpermanent",
                ));
            }
            if matches!(subject, v2::SubjectRef::SelfObject) {
                if !matches!(program.kind, ProgramKind::Permanent(_)) {
                    return Err(LoweringError::Unsupported(
                        "v2:static-self-entry-counters-nonpermanent",
                    ));
                }
                if let Some(cond) = condition {
                    let rc = lower_resolution_condition(cond, ctx)?;
                    let amt = lower_value_expr(count, ctx)?;
                    program.entry_counter_abilities.push(
                        spellwake_engine::program::EntryCounterAbility::ConditionalCounter {
                            condition: Box::new(rc),
                            kind: ek,
                            amount: amt,
                        },
                    );
                } else {
                    let ab = build_self_entry_counter_ability(ek, count, ctx)?;
                    program.entry_counter_abilities.push(ab);
                }
                return Ok(());
            }
            let selector = lower_subject_to_permanent_selector(subject, ctx)?;
            let rule = build_others_enter_with_counters_rule(selector, ek, count, ctx)?;
            let cond = lower_opt_static_cond(condition.as_ref(), ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ctx.alloc_static_id(),
                    rule,
                },
                cond,
            )
        }
        v2::StaticAbility::ControlEnchanted { controller } => {
            if *controller != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported(
                    "v2:static-control-enchanted-controller",
                ));
            }
            let selector = PermanentSelector {
                attached_to_source: true,
                ..PermanentSelector::default()
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector,
                    parts: StaticParts(vec![StaticPart::ControlSourceController]),
                },
                None,
            )
        }
        v2::StaticAbility::ActivationCostModifier {
            delta,
            less,
            scale,
            condition,
        } => {
            // `v2` collapses `"This cost is reduced by {M}"` (single ability on self) and
            // `"Activated abilities cost {M} less to activate"` (global) into this variant
            // when `less == true`. Fail closed rather than conflating self vs global scope.
            if *less {
                return Err(LoweringError::Unsupported(
                    "v2:static-activation-cost-reduction-ambiguous-scope",
                ));
            }
            let delta_u8 = lower_pure_generic_mana_u8(delta, "v2:static-activation-cost-mana")?;
            let scale_expr = match scale {
                Some(e) => {
                    let ve = lower_value_expr(e, ctx)?;
                    if !ve.is_world_evaluable() {
                        return Err(LoweringError::Unsupported(
                            "v2:static-activation-cost-scale-nonworld",
                        ));
                    }
                    Some(Box::new(ve))
                }
                None => None,
            };
            let cond = lower_opt_static_cond(condition.as_ref(), ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ctx.alloc_static_id(),
                    rule: StaticModifierRule::ActivatedAbilityCost {
                        scope: ActivatedCostScope::Any,
                        controller: PlayerRelation::AnyParticipant,
                        delta: delta_u8,
                        less: false,
                        scale: scale_expr,
                        minimum_total_mana: 0,
                    },
                },
                cond,
            )
        }
        v2::StaticAbility::PreventDamageStatic {
            combat_only,
            source,
            target,
        } => {
            let (src_flt, src_ctrl) = match source {
                None => (None, PlayerRelation::AnyParticipant),
                Some(
                    v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f),
                ) => lower_damage_source_filter(Some(f), ctx)?,
                Some(v2::SubjectRef::SelfObject) => (
                    Some(ObjectFilter::self_object()),
                    PlayerRelation::AnyParticipant,
                ),
                Some(_) => {
                    return Err(LoweringError::Unsupported(
                        "v2:static-prevent-damage-source",
                    ));
                }
            };
            let recipient = match target {
                None => StaticDamageRecipient::Any,
                Some(t) => lower_damage_recipient(t, ctx)?,
            };
            let kind = if *combat_only {
                StaticDamageKind::CombatOnly
            } else {
                StaticDamageKind::Any
            };
            push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticDamageRule::ModifyDamage {
                        source: src_flt,
                        source_controller: src_ctrl,
                        recipient,
                        kind,
                        op: StaticDamageOp::PreventAll,
                    },
                },
                None,
            )
        }
        v2::StaticAbility::AdditionalLandPlayStatic { player, extra } => {
            let nz = NonZeroU8::new(*extra).ok_or(LoweringError::Unsupported(
                "v2:static-zero-additional-land-plays",
            ))?;
            let permission = match player {
                v2::PlayerRef::You => CastPermissionRule::AdditionalLandPlays(nz),
                v2::PlayerRef::EachPlayer | v2::PlayerRef::AnyPlayer => {
                    CastPermissionRule::AllPlayersAdditionalLandPlays(nz)
                }
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:static-additional-land-play-player",
                    ));
                }
            };
            push_static_ability(
                program,
                StaticContinuousAbility::CastPermission {
                    id: ctx.alloc_static_id(),
                    permission,
                },
                None,
            )
        }
        v2::StaticAbility::CantCastSpellsMatching {
            player,
            filter,
            condition,
        } => {
            let rel = lower_static_player_relation(*player)?;
            let sf = lower_spell_filter(filter, ctx)?;
            let cond = lower_opt_static_cond(condition.as_ref(), ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::ProhibitCasting {
                        player: rel,
                        filter: Box::new(sf),
                    },
                },
                cond,
            )
        }
        v2::StaticAbility::MaxAttackersPerCombat { max, target } => {
            if *max == 1 && target.is_none() {
                program.combat_limits.one_attacker = true;
                Ok(())
            } else {
                Err(LoweringError::Unsupported("v2:static-max-attackers"))
            }
        }
        v2::StaticAbility::RevealOpeningHand { .. } => {
            Err(LoweringError::Unsupported("v2:static-reveal-opening-hand"))
        }
        v2::StaticAbility::LegendRuleDoesntApply(opt_filter) => {
            let rule = match opt_filter {
                None => StaticModifierRule::IgnoreLegendRule,
                Some(f) if is_controller_only_filter(f, ControllerRelation::You) => {
                    StaticModifierRule::IgnoreLegendRuleForController
                }
                Some(_) => {
                    return Err(LoweringError::Unsupported("v2:static-legend-rule-filtered"));
                }
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ctx.alloc_static_id(),
                    rule,
                },
                None,
            )
        }
        v2::StaticAbility::ConditionalReplacement {
            condition,
            replacement,
        } => lower_conditional_replacement_into_program(condition, replacement, program, ctx),
        v2::StaticAbility::AttackTax {
            target,
            cost,
            per_creature,
        } => {
            if !*per_creature {
                return Err(LoweringError::Unsupported(
                    "v2:static-attack-tax-not-per-creature",
                ));
            }
            let protect_player = match target {
                v2::SubjectRef::Player(v2::PlayerRef::You) => true,
                _ => {
                    return Err(LoweringError::Unsupported("v2:static-attack-tax-target"));
                }
            };
            let symbol = match cost {
                v2::Cost::Mana(mc) => {
                    let n = lower_pure_generic_mana_u8(mc, "v2:static-attack-tax-cost")?;
                    if n == 1 {
                        AttackCostSymbol::OneGeneric
                    } else {
                        AttackCostSymbol::Generic(n)
                    }
                }
                _ => {
                    return Err(LoweringError::Unsupported("v2:static-attack-tax-cost"));
                }
            };
            push_attack_requirement(
                program,
                AttackRequirement::PayPerAttacker {
                    protect_player,
                    symbol,
                },
            );
            Ok(())
        }
        v2::StaticAbility::BindVariable { var, expr, inner } => {
            lower_bind_variable_into_program(*var, expr, inner, program, ctx)
        }
        v2::StaticAbility::NextSpellModifier { filter, effect } => {
            lower_permanent_next_spell_modifier(filter, effect, program, ctx)
        }
        v2::StaticAbility::VisibilityRule { player, kind } => {
            lower_visibility_rule_into_program(*player, *kind, program, ctx)
        }
        v2::StaticAbility::ZoneCastPermission {
            player,
            filter,
            subject,
            zone,
            mode,
            condition,
        } => lower_zone_cast_permission_into_program(
            *player,
            filter.as_ref(),
            subject.as_ref(),
            zone.as_ref(),
            mode,
            condition.as_ref(),
            program,
            ctx,
        ),
        v2::StaticAbility::TriggerModifier {
            kind,
            filter,
            permanent_filter,
            extra_count,
        } => lower_trigger_modifier_into_program(
            *kind,
            filter,
            permanent_filter.as_ref(),
            *extra_count,
            program,
            ctx,
        ),
        v2::StaticAbility::HandSize { player, rule } => {
            lower_hand_size_into_program(*player, rule, program, ctx)
        }
        v2::StaticAbility::SubjectRule {
            subject,
            rule,
            condition,
        } => lower_subject_rule_into_program(subject, rule, condition.as_ref(), program, ctx),
        v2::StaticAbility::GlobalFlag(flag) => lower_global_flag_into_program(*flag, program, ctx),
        v2::StaticAbility::LevelStructure(_) => {
            Err(LoweringError::Unsupported("v2:static-level-structure"))
        }
        v2::StaticAbility::FilterRule { filter, kind } => {
            lower_filter_rule_into_program(filter, *kind, program, ctx)
        }
        v2::StaticAbility::Compound(parts) => {
            if parts.is_empty() {
                return Err(LoweringError::Unsupported("v2:static-empty-compound"));
            }
            if let Some(combined_rule) = try_combine_win_lose_compound(parts, ctx)? {
                return push_static_ability(program, combined_rule.0, combined_rule.1);
            }
            for part in parts {
                lower_static_into_program(part, program, ctx)?;
            }
            Ok(())
        }
    }
}

/// Lower a duration-scoped `v2::StaticAbility` into `Instruction::ApplyContinuous` on `acc`.
///
/// # Errors
/// Returns [`LoweringError::Unsupported`] whenever the static ability has no faithful
/// temporary continuous representation.
pub(crate) fn lower_temporary_static(
    ability: &v2::StaticAbility,
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match ability {
        v2::StaticAbility::Compound(parts) => {
            if parts.is_empty() {
                return Err(LoweringError::Unsupported("v2:temp-static-empty-compound"));
            }
            for part in parts {
                lower_temporary_static(part, duration, acc, ctx)?;
            }
            Ok(())
        }
        v2::StaticAbility::NextSpellModifier { filter, effect } => {
            lower_temp_next_spell_modifier(filter, effect, duration, acc, ctx)
        }
        v2::StaticAbility::ContinuousModifyPT {
            subject,
            power,
            toughness,
            grant_keywords,
            scale,
            condition: None,
        } => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            let p_expr = lower_pt_delta(*power, scale.as_deref(), ctx)?;
            let t_expr = lower_pt_delta(*toughness, scale.as_deref(), ctx)?;
            let granted = lower_granted_keywords(grant_keywords, ctx)?;
            let has_non_static_grant = !granted.extra_triggers.is_empty()
                || !granted.extra_activated.is_empty()
                || granted.ability_change.granted_exalted > 0
                || granted.ability_change.granted_toxic.is_some()
                || !granted.ability_change.granted_combat_keywords.is_empty();
            let spec = match (&p_expr, &t_expr) {
                (EngineValueExpr::Literal(p), EngineValueExpr::Literal(t)) => {
                    let mut parts = Vec::new();
                    if *p != RulesInteger::from(0)
                        || *t != RulesInteger::from(0)
                        || grant_keywords.is_empty()
                    {
                        parts.push(ResolvedPartSpec::ModifyPT(PowerToughness {
                            power: p.clone(),
                            toughness: t.clone(),
                        }));
                    }
                    append_granted_keywords_resolved(&mut parts, granted);
                    ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), duration)
                }
                _ if !has_non_static_grant => ApplyContinuousSpec::dynamic_boost(
                    query,
                    p_expr,
                    t_expr,
                    granted.ability_change.granted_keywords,
                    BTreeSet::new(),
                    duration,
                ),
                _ => {
                    let mut parts = vec![ResolvedPartSpec::ModifyPTExpr {
                        power: Box::new(p_expr),
                        toughness: Box::new(t_expr),
                    }];
                    append_granted_keywords_resolved(&mut parts, granted);
                    ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), duration)
                }
            };
            acc.push(Instruction::ApplyContinuous(Box::new(spec)));
            Ok(())
        }
        v2::StaticAbility::GrantKeywords {
            subject,
            keywords,
            condition: None,
        } => {
            if keywords.is_empty() {
                return Err(LoweringError::Unsupported("v2:temp-static-empty-keywords"));
            }
            if let v2::SubjectRef::Player(player) = subject {
                let mut kws = BTreeSet::new();
                for kw in keywords {
                    kws.extend(crate::v2_lowering::atoms::lower_keyword_abilities(kw, ctx)?);
                }
                let players = lower_affected_players(*player, acc, ctx)?;
                acc.push(Instruction::ApplyContinuous(Box::new(
                    ApplyContinuousSpec::temporary_rule(
                        TemporaryRuleKind::PlayerGrantKeyword {
                            players,
                            keywords: kws,
                        },
                        duration,
                    ),
                )));
                return Ok(());
            }
            let query = lower_subject_to_query(subject, acc, ctx)?;
            let granted = lower_granted_keywords(keywords, ctx)?;
            let mut parts = Vec::new();
            append_granted_keywords_resolved(&mut parts, granted);
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), duration),
            )));
            Ok(())
        }
        v2::StaticAbility::GrantAbility {
            subject,
            ability: face,
            condition: None,
        } => lower_temp_grant_ability(subject, face, duration, acc, ctx),
        v2::StaticAbility::SetSubtypes {
            subject,
            subtypes,
            keep_existing,
        } => lower_temp_set_subtypes(subject, subtypes, *keep_existing, duration, acc, ctx),
        v2::StaticAbility::SubjectRule {
            subject,
            rule,
            condition: None,
        } => lower_temp_subject_rule(subject, rule, duration, acc, ctx),
        v2::StaticAbility::PreventDamageStatic {
            combat_only,
            source,
            target,
        } => {
            let src_flt = match source {
                None => None,
                Some(
                    v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f),
                ) => Some(lower_object_filter(f, ctx)?),
                Some(v2::SubjectRef::SelfObject) => Some(ObjectFilter::self_object()),
                Some(_) => {
                    return Err(LoweringError::Unsupported("v2:temp-prevent-damage-source"));
                }
            };
            let recipient = match target {
                None => StaticDamageRecipient::Any,
                Some(t) => lower_damage_recipient(t, ctx)?,
            };
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::PreventDamageShield {
                        amount: None,
                        source: src_flt,
                        source_chosen_color: false,
                        source_chosen_type: false,
                        source_is_target: false,
                        half_round_down: false,
                        recipient,
                        combat_only: *combat_only,
                        one_shot: false,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::RuleRestriction {
            subject,
            rule,
            condition: None,
        } => lower_temp_rule_restriction(subject.as_ref(), rule, duration, acc, ctx),
        v2::StaticAbility::PlayerRestriction {
            player,
            restriction,
            condition: None,
        } => lower_temp_player_restriction(*player, *restriction, duration, acc, ctx),
        v2::StaticAbility::CantCastSpellsMatching {
            player,
            filter,
            condition: None,
        } => {
            let players = lower_affected_players(*player, acc, ctx)?;
            let ef = lower_object_filter(filter, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::CantCastSpells {
                        players,
                        zone: None,
                        filter: Some(ef),
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::CostModifier {
            subject,
            delta,
            less,
            scale: None,
            min_mana: None,
            condition: None,
        } => {
            let n = lower_pure_generic_mana_u8(delta, "v2:temp-static-cost-modifier-mana")?;
            let players = match subject.controller {
                None => AffectedPlayers::EachPlayer,
                Some(ControllerRelation::You) => AffectedPlayers::One(ResolvingPlayer::Controller),
                Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                    AffectedPlayers::EachOpponent
                }
                Some(_) => {
                    return Err(LoweringError::Unsupported(
                        "v2:temp-static-cost-modifier-controller",
                    ));
                }
            };
            let mut stripped = subject.clone();
            stripped.controller = None;
            let ef = lower_object_filter(&stripped, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::SpellCostModifier {
                        players,
                        filter: ef,
                        amount: Box::new(EngineValueExpr::Literal(RulesInteger::from(i64::from(
                            n,
                        )))),
                        increase: !*less,
                        next_only: false,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::AdditionalLandPlayStatic { player, extra } => {
            if *extra == 0 {
                return Err(LoweringError::Unsupported(
                    "v2:temp-static-zero-additional-land-plays",
                ));
            }
            let rp = lower_resolving_player(*player, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::AdditionalLandPlay {
                        player: rp,
                        count: *extra,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::ZoneCastPermission {
            player,
            filter,
            subject: None,
            zone,
            mode,
            condition: None,
        } => lower_temp_zone_cast_permission(
            *player,
            filter.as_ref(),
            zone.as_ref(),
            mode,
            duration,
            acc,
            ctx,
        ),
        v2::StaticAbility::RemoveAllAbilities { subject } => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                        AbilityRemoval::All,
                    ))]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::SetBasePT {
            subject,
            power,
            toughness,
        } => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::SetPT(PowerToughness {
                        power: RulesInteger::from(i64::from(*power)),
                        toughness: RulesInteger::from(i64::from(*toughness)),
                    })]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::LevelStructure(v2::LevelMarker::ClassLevel(lvl)) if *lvl >= 2 => {
            acc.push(Instruction::MutateObject(Box::new(
                MutateObjectSpec::advance_class_level(*lvl),
            )));
            Ok(())
        }
        v2::StaticAbility::BindVariable {
            var: v2::VarName::X,
            expr,
            inner,
        } => {
            let rewritten = subst_var_x_in_static(inner, expr)
                .ok_or(LoweringError::Unsupported("v2:temp-static-unsupported"))?;
            lower_temporary_static(&rewritten, duration, acc, ctx)
        }
        v2::StaticAbility::GlobalFlag(v2::StaticFlagKind::CostReductionRule) => {
            Err(LoweringError::Unsupported("v2:static-cost-reduction-rule"))
        }
        _ => Err(LoweringError::Unsupported("v2:temp-static-unsupported")),
    }
}

/// Insert a [`StaticContinuousAbility`] into `program.continuous` or
/// `program.additional_battlefield_abilities`, registering any optional
/// [`StaticSourceCondition`] under its `StaticAbilityId`.
pub(super) fn push_static_ability(
    program: &mut Program,
    ability: StaticContinuousAbility,
    condition: Option<StaticSourceCondition>,
) -> LowerResult<()> {
    if !matches!(program.kind, ProgramKind::Permanent(_)) {
        // Non-permanents may only carry statics that do not require a permanent body
        // (such as CharacteristicDefining or specific cast-time rules).
        let allowed_on_spell = matches!(
            &ability,
            StaticContinuousAbility::CharacteristicDefining { .. }
                | StaticContinuousAbility::CommandZoneActive { .. }
                | StaticContinuousAbility::CostModifier {
                    modifier: SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { .. }
                        | SpellCostModifier::HighManaValueProtected { .. },
                    ..
                }
                | StaticContinuousAbility::CastPermission {
                    permission: CastPermissionRule::GraveyardCreatureCast
                        | CastPermissionRule::GraveyardSelfConditional { .. }
                        | CastPermissionRule::ColorlessHandSpellsHaveCascade { .. }
                        | CastPermissionRule::GrantSpellCascade { .. }
                        | CastPermissionRule::ControllerSpellsCannotBeCountered(_)
                        | CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(_)
                        | CastPermissionRule::AllSpellsCannotBeCounteredFiltered(_),
                    ..
                }
        );
        if !allowed_on_spell {
            return Err(LoweringError::Unsupported("v2:static-requires-permanent"));
        }
    }

    let id = ability.id();
    if program.continuous.is_none() {
        program.continuous = Some(ability);
    } else if ability.allowed_as_additional() {
        program.additional_battlefield_abilities.push(ability);
    } else if program
        .continuous
        .as_ref()
        .is_some_and(StaticContinuousAbility::allowed_as_additional)
    {
        if let Some(prev) = program.continuous.take() {
            program.additional_battlefield_abilities.push(prev);
        }
        program.continuous = Some(ability);
    } else {
        return Err(LoweringError::Unsupported(
            "v2:static-multiple-primary-abilities",
        ));
    }

    if let Some(cond) = condition {
        program.static_conditions.insert(id, cond);
    }
    Ok(())
}

/// Convert a `v2::SubjectRef` in static-ability subject position into a [`PermanentSelector`].
pub(super) fn lower_subject_to_permanent_selector(
    subject: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<PermanentSelector> {
    match subject {
        v2::SubjectRef::SelfObject => Ok(PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        }),
        v2::SubjectRef::Anaphora(anaphora) => match anaphora {
            v2::AnaphoraRef::EnchantedCreature | v2::AnaphoraRef::EquippedCreature => {
                Ok(PermanentSelector {
                    attached_to_source: true,
                    required_types: BTreeSet::from([spellwake_engine::program::CardType::Creature]),
                    ..PermanentSelector::default()
                })
            }
            v2::AnaphoraRef::EnchantedPermanent => Ok(PermanentSelector {
                attached_to_source: true,
                ..PermanentSelector::default()
            }),
            _ => lower_permanent_selector(subject, ctx),
        },
        v2::SubjectRef::BothPairedCreatures => Ok(PermanentSelector {
            controller: ControllerSelector::SourcePair,
            ..PermanentSelector::default()
        }),
        v2::SubjectRef::All(_) | v2::SubjectRef::Each(_) | v2::SubjectRef::Group(_) => {
            lower_permanent_selector(subject, ctx)
        }
        _ => Err(LoweringError::Unsupported("v2:static-subject-selector")),
    }
}

fn lower_opt_static_cond(
    cond: Option<&v2::ConditionExpr>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<StaticSourceCondition>> {
    match cond {
        None => Ok(None),
        Some(c) => Ok(Some(lower_static_source_condition(c, ctx)?)),
    }
}

fn lower_world_numeric_expr(
    expr: &v2::ValueExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<NumericExpression> {
    if let Some(n) = lower_value_const(expr) {
        return Ok(NumericExpression::Literal(RulesInteger::from(i64::from(n))));
    }
    let ve = lower_value_expr(expr, ctx)?;
    if !ve.is_world_evaluable() {
        return Err(LoweringError::Unsupported("v2:static-nonworld-value-expr"));
    }
    Ok(NumericExpression::Eval(Box::new(ve)))
}

fn lower_static_pt_pair(
    power: v2::PtDelta,
    toughness: v2::PtDelta,
    scale: Option<&v2::ValueExpr>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<NumericPair> {
    match scale {
        None => {
            let p = match power {
                v2::PtDelta::Fixed(n) => {
                    NumericExpression::Literal(RulesInteger::from(i64::from(n)))
                }
                v2::PtDelta::PlusX | v2::PtDelta::MinusX => {
                    return Err(LoweringError::Unsupported("v2:static-unbound-x-pt"));
                }
            };
            let t = match toughness {
                v2::PtDelta::Fixed(n) => {
                    NumericExpression::Literal(RulesInteger::from(i64::from(n)))
                }
                v2::PtDelta::PlusX | v2::PtDelta::MinusX => {
                    return Err(LoweringError::Unsupported("v2:static-unbound-x-pt"));
                }
            };
            Ok(NumericPair {
                power: p,
                toughness: t,
            })
        }
        Some(scale_expr) => {
            let base = lower_world_numeric_expr(scale_expr, ctx)?;
            let scale_axis = |delta: v2::PtDelta| -> NumericExpression {
                match delta {
                    v2::PtDelta::Fixed(0) => NumericExpression::Literal(RulesInteger::from(0)),
                    v2::PtDelta::Fixed(1) | v2::PtDelta::PlusX => base.clone(),
                    v2::PtDelta::MinusX => NumericExpression::Scale {
                        expr: Box::new(base.clone()),
                        multiplier: RulesInteger::from(-1),
                        offset: RulesInteger::from(0),
                    },
                    v2::PtDelta::Fixed(k) => NumericExpression::Scale {
                        expr: Box::new(base.clone()),
                        multiplier: RulesInteger::from(i64::from(k)),
                        offset: RulesInteger::from(0),
                    },
                }
            };
            Ok(NumericPair {
                power: scale_axis(power),
                toughness: scale_axis(toughness),
            })
        }
    }
}

fn lower_grant_keywords_into_program(
    subject: &v2::SubjectRef,
    keywords: &[v2::KeywordSpec],
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if keywords.is_empty() {
        return Err(LoweringError::Unsupported("v2:static-empty-keywords"));
    }

    // Player-scoped static keywords (e.g. "You have hexproof", "You have shroud").
    if let v2::SubjectRef::Player(player) = subject {
        if *player != v2::PlayerRef::You {
            return Err(LoweringError::Unsupported(
                "v2:static-player-grant-keyword-non-you",
            ));
        }
        let cond = lower_opt_static_cond(condition, ctx)?;
        for kw in keywords {
            let rule = match kw.kind {
                v2::KeywordKind::Hexproof if kw.param.is_none() => {
                    StaticRestriction::ControllerHasHexproof
                }
                v2::KeywordKind::Shroud if kw.param.is_none() => {
                    StaticRestriction::ControllerHasShroud
                }
                _ => {
                    return Err(LoweringError::Unsupported("v2:static-player-grant-keyword"));
                }
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule,
                },
                cond.clone(),
            )?;
        }
        return Ok(());
    }

    // Spell-scoped static keywords (e.g. "Creature spells you cast have convoke/flash/cascade").
    if let v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) = subject
        && f.any_spell
    {
        let any_player = match f.controller {
            None => true,
            Some(ControllerRelation::You) => false,
            Some(_) => {
                return Err(LoweringError::Unsupported(
                    "v2:static-grant-spell-keyword-controller",
                ));
            }
        };
        let matching = lower_spell_filter(f, ctx)?;
        let cond = lower_opt_static_cond(condition, ctx)?;
        for kw in keywords {
            if kw.kind == v2::KeywordKind::Cascade && kw.param.is_none() {
                push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ctx.alloc_static_id(),
                        permission: CastPermissionRule::GrantSpellCascade {
                            any_player,
                            matching,
                            count: NonZeroU8::MIN,
                        },
                    },
                    cond.clone(),
                )?;
            } else {
                for engine_kw in lower_keyword_abilities(kw, ctx)? {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ctx.alloc_static_id(),
                            permission: CastPermissionRule::GrantSpellKeyword {
                                any_player,
                                matching,
                                keyword: engine_kw,
                            },
                        },
                        cond.clone(),
                    )?;
                }
            }
        }
        return Ok(());
    }

    let selector = lower_subject_to_permanent_selector(subject, ctx)?;
    let mut parts = Vec::new();
    let mut remaining_kws = Vec::with_capacity(keywords.len());
    for kw in keywords {
        match (kw.kind, &kw.param) {
            (v2::KeywordKind::Protection, Some(v2::KeywordParam::ChosenColor)) => {
                parts.push(StaticPart::GrantProtectionChosenColor);
            }
            (v2::KeywordKind::Protection, Some(v2::KeywordParam::ChosenPlayer)) => {
                parts.push(StaticPart::GrantProtectionChosenPlayer);
            }
            _ => remaining_kws.push(kw.clone()),
        }
    }
    if !remaining_kws.is_empty() {
        let granted = lower_granted_keywords(&remaining_kws, ctx)?;
        append_granted_keywords_static(&mut parts, granted);
    }
    let cond = lower_opt_static_cond(condition, ctx)?;
    push_static_ability(
        program,
        StaticContinuousAbility::Battlefield {
            id: ctx.alloc_static_id(),
            selector,
            parts: StaticParts(parts),
        },
        cond,
    )
}

fn append_granted_keywords_static(parts: &mut Vec<StaticPart>, granted: GrantedKeywordParts) {
    if granted.has_ability_change() {
        parts.push(StaticPart::Abilities(granted.ability_change));
    }
    for trig in granted.extra_triggers {
        parts.push(StaticPart::GrantTrigger(Box::new(trig)));
    }
    for (act, restriction) in granted.extra_activated {
        if let Some(res) = restriction {
            parts.push(StaticPart::GrantActivatedRestricted {
                ability: Box::new(act),
                restriction: Box::new(res),
            });
        } else {
            parts.push(StaticPart::GrantActivated(Box::new(act)));
        }
    }
}

fn append_granted_keywords_resolved(
    parts: &mut Vec<ResolvedPartSpec>,
    granted: GrantedKeywordParts,
) {
    if granted.has_ability_change() {
        parts.push(ResolvedPartSpec::Abilities(granted.ability_change));
    }
    for trig in granted.extra_triggers {
        parts.push(ResolvedPartSpec::GrantTrigger(Box::new(trig)));
    }
    for (act, restriction) in granted.extra_activated {
        if let Some(res) = restriction {
            parts.push(ResolvedPartSpec::GrantActivatedRestricted {
                ability: Box::new(act),
                restriction: Box::new(res),
            });
        } else {
            parts.push(ResolvedPartSpec::GrantActivated(Box::new(act)));
        }
    }
}

fn lower_temp_grant_ability(
    subject: &v2::SubjectRef,
    face: &v2::CompiledFaceV2,
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let query = lower_subject_to_query(subject, acc, ctx)?;
    let (granted, activated_restricted) = lower_granted_face_with_restrictions(face, ctx)?;
    if !granted.statics.is_empty() {
        return Err(LoweringError::Unsupported("v2:temp-grant-nested-static"));
    }
    let mut parts = Vec::new();
    if !granted.keywords.is_empty() {
        parts.push(ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(
            granted.keywords,
        )));
    }
    for trig in granted.triggered {
        parts.push(ResolvedPartSpec::GrantTrigger(Box::new(trig)));
    }
    for mut act in granted.activated {
        act.id = ActivatedAbilityId(0);
        parts.push(ResolvedPartSpec::GrantActivated(Box::new(act)));
    }
    for (mut act, res) in activated_restricted {
        act.id = ActivatedAbilityId(0);
        parts.push(ResolvedPartSpec::GrantActivatedRestricted {
            ability: Box::new(act),
            restriction: Box::new(res),
        });
    }
    for mana_ab in granted.mana {
        parts.push(ResolvedPartSpec::GrantMana(Box::new(mana_ab.output)));
    }
    if parts.is_empty() {
        return Err(LoweringError::Unsupported("v2:temp-grant-empty-ability"));
    }
    acc.push(Instruction::ApplyContinuous(Box::new(
        ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), duration),
    )));
    Ok(())
}

fn lower_temp_set_subtypes(
    subject: &v2::SubjectRef,
    subtypes: &[String],
    keep_existing: bool,
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if subtypes.is_empty() {
        return Err(LoweringError::Unsupported("v2:temp-set-subtypes-empty"));
    }
    let query = lower_subject_to_query(subject, acc, ctx)?;
    let mut creature_subs = BTreeSet::new();
    let mut land_subs = BTreeSet::new();
    let mut artifact_subs = BTreeSet::new();
    let mut enchantment_subs = BTreeSet::new();

    for raw in subtypes {
        if let Some(lt) = LowerCtx::lookup_land_type(raw) {
            land_subs.insert(lt);
        } else if let Some(ct) = ctx.lookup_creature_type(raw) {
            creature_subs.insert(ct);
        } else if let Some(at) = LowerCtx::lookup_artifact_subtype(raw) {
            artifact_subs.insert(at);
        } else if let Some(et) = LowerCtx::lookup_enchantment_subtype(raw) {
            enchantment_subs.insert(et);
        } else {
            return Err(LoweringError::Unsupported("v2:temp-set-subtypes-unknown"));
        }
    }

    let base = TypeChangeSpec {
        land_subtypes: if land_subs.is_empty() {
            SubtypeOperation::Keep
        } else if keep_existing {
            SubtypeOperation::Add(land_subs)
        } else {
            SubtypeOperation::Set(land_subs)
        },
        supertypes: TypeOperation::Keep,
        types: TypeOperation::Keep,
        subtypes: if creature_subs.is_empty() {
            SubtypeOperation::Keep
        } else if keep_existing {
            SubtypeOperation::Add(InstructionSubtypeOperand::Literal(creature_subs))
        } else {
            SubtypeOperation::Set(InstructionSubtypeOperand::Literal(creature_subs))
        },
    };

    let part = if artifact_subs.is_empty() && enchantment_subs.is_empty() {
        ResolvedPartSpec::Type(base)
    } else {
        ResolvedPartSpec::TypeWithSubtypes {
            base,
            artifact_subtypes: if artifact_subs.is_empty() {
                SubtypeOperation::Keep
            } else if keep_existing {
                SubtypeOperation::Add(artifact_subs)
            } else {
                SubtypeOperation::Set(artifact_subs)
            },
            enchantment_subtypes: if enchantment_subs.is_empty() {
                SubtypeOperation::Keep
            } else if keep_existing {
                SubtypeOperation::Add(enchantment_subs)
            } else {
                SubtypeOperation::Set(enchantment_subs)
            },
        }
    };

    acc.push(Instruction::ApplyContinuous(Box::new(
        ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(vec![part]), duration),
    )));
    Ok(())
}

fn selector_allowed_in_spell_temp_static(selector: &PermanentSelector) -> bool {
    !selector.attached_to_source
        && !selector.exclude_source
        && !matches!(
            selector.controller,
            ControllerSelector::SourceItself
                | ControllerSelector::SelfAtMaxSpeed
                | ControllerSelector::SourcePair
        )
}

fn lower_temp_subject_rule(
    subject: &v2::SubjectRef,
    rule: &v2::SubjectStaticRule,
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match rule {
        v2::SubjectStaticRule::CanAttackWithDefender => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::CanAttackAsThoughNoDefender,
                    )]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::DoesntUntap => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange {
                        doesnt_untap: true,
                        ..AbilityChange::default()
                    })]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::MustAttack => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::MustAttackEachCombatIfAble,
                    )]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::CanBlockAdditional(n) => {
            if *n == 0 {
                return Err(LoweringError::Unsupported(
                    "v2:temp-can-block-additional-zero",
                ));
            }
            let query = lower_subject_to_query(subject, acc, ctx)?;
            let parts = (0..*n)
                .map(|_| {
                    ResolvedPartSpec::BlockingRestriction(BlockingRestriction::AdditionalBlock)
                })
                .collect();
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), duration),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::CanBlockAnyNumber => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::CanBlockAnyNumber,
                    )]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::AssignDamageByToughness => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::AssignDamageByToughness,
                    )]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::AssignDamageAsUnblocked => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::AssignDamageAsThoughUnblocked,
                    )]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::AllMustBlock => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                        BlockingRestriction::AllDefendingCreaturesMustBlock,
                    )]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::RemoveTypes(mask) => {
            let removed = lower_card_type_set(*mask);
            if removed.is_empty()
                || removed.iter().any(|t| {
                    matches!(
                        t,
                        spellwake_engine::program::CardType::Instant
                            | spellwake_engine::program::CardType::Sorcery
                    )
                })
            {
                return Err(LoweringError::Unsupported("v2:temp-remove-types"));
            }
            let query = lower_subject_to_query(subject, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Remove(removed),
                        subtypes: SubtypeOperation::Keep,
                    })]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::SetColors {
            colors,
            keep_existing,
        } => {
            let query = lower_subject_to_query(subject, acc, ctx)?;
            let color_set = lower_color_set(*colors);
            let op = if *keep_existing {
                ColorOperation::Add(color_set)
            } else {
                ColorOperation::Set(color_set)
            };
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Color(op)]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::AllCreatureTypes => {
            if let Ok(sel) = lower_subject_to_permanent_selector(subject, ctx)
                && selector_allowed_in_spell_temp_static(&sel)
            {
                acc.push(Instruction::ApplyContinuous(Box::new(
                    ApplyContinuousSpec::temporary_rule(
                        TemporaryRuleKind::temporary_static(StaticContinuousAbility::Battlefield {
                            id: ctx.alloc_static_id(),
                            selector: sel,
                            parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                                land_subtypes: SubtypeOperation::Keep,
                                supertypes: TypeOperation::Keep,
                                types: TypeOperation::Keep,
                                subtypes: SubtypeOperation::Add(
                                    StaticSubtypeOperand::AllCreatureTypes,
                                ),
                            })]),
                        }),
                        duration,
                    ),
                )));
                return Ok(());
            }
            let query = lower_subject_to_query(subject, acc, ctx)?;
            let all_types: BTreeSet<_> = ctx
                .vocab
                .members()
                .iter()
                .filter_map(|m| ctx.vocab.resolve_label(m))
                .collect();
            if all_types.is_empty() {
                return Err(LoweringError::Unsupported(
                    "v2:temp-all-creature-types-empty",
                ));
            }
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(
                    query,
                    ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Add(InstructionSubtypeOperand::Literal(
                            all_types,
                        )),
                    })]),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::EntersTapped => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            if !selector_allowed_in_spell_temp_static(&sel) {
                return Err(LoweringError::Unsupported("v2:temp-static-unsupported"));
            }
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                        id: ctx.alloc_static_id(),
                        rule: StaticRestriction::OthersEnterTapped(Box::new(sel)),
                    }),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::EntersUntapped => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            if !selector_allowed_in_spell_temp_static(&sel) {
                return Err(LoweringError::Unsupported("v2:temp-static-unsupported"));
            }
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                        id: ctx.alloc_static_id(),
                        rule: StaticRestriction::EnterUntapped(Box::new(sel)),
                    }),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::UntapDuringEachOtherUntap => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            if !selector_allowed_in_spell_temp_static(&sel) {
                return Err(LoweringError::Unsupported("v2:temp-static-unsupported"));
            }
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::temporary_static(StaticContinuousAbility::UntapRule {
                        id: ctx.alloc_static_id(),
                        rule: StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(Box::new(sel)),
                    }),
                    duration,
                ),
            )));
            Ok(())
        }
        v2::SubjectStaticRule::IgnoreKeywordForBlocking(v2::KeywordKind::Hexproof) => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            if !selector_allowed_in_spell_temp_static(&sel) {
                return Err(LoweringError::Unsupported("v2:temp-static-unsupported"));
            }
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::temporary_static(StaticContinuousAbility::Restriction {
                        id: ctx.alloc_static_id(),
                        rule: StaticRestriction::IgnoreOpponentsHexproof(Box::new(sel)),
                    }),
                    duration,
                ),
            )));
            Ok(())
        }
        _ => Err(LoweringError::Unsupported("v2:temp-static-unsupported")),
    }
}

fn lower_grant_ability_into_program(
    subject: &v2::SubjectRef,
    face: &v2::CompiledFaceV2,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let selector = lower_subject_to_permanent_selector(subject, ctx)?;
    let (granted, activated_restricted) = lower_granted_face_with_restrictions(face, ctx)?;
    let mut parts = Vec::new();

    if !granted.keywords.is_empty() {
        parts.push(StaticPart::Abilities(AbilityChange::grant_keywords(
            granted.keywords,
        )));
    }
    for trig in granted.triggered {
        parts.push(StaticPart::GrantTrigger(Box::new(trig)));
    }
    for mut act in granted.activated {
        // Engine validation requires `ability.id.0 == 0` for `StaticPart::GrantActivated`.
        act.id = ActivatedAbilityId(0);
        parts.push(StaticPart::GrantActivated(Box::new(act)));
    }
    for (mut act, res) in activated_restricted {
        act.id = ActivatedAbilityId(0);
        parts.push(StaticPart::GrantActivatedRestricted {
            ability: Box::new(act),
            restriction: Box::new(res),
        });
    }
    for mana_ab in granted.mana {
        parts.push(StaticPart::GrantMana(Box::new(mana_ab.output)));
    }
    for st in granted.statics {
        match st {
            StaticContinuousAbility::Restriction { rule, .. } => {
                parts.push(StaticPart::GrantRestriction(Box::new(rule)));
            }
            _ => {
                return Err(LoweringError::Unsupported("v2:static-grant-nested-static"));
            }
        }
    }

    if parts.is_empty() {
        return Err(LoweringError::Unsupported("v2:static-grant-empty-ability"));
    }

    let cond = lower_opt_static_cond(condition, ctx)?;
    push_static_ability(
        program,
        StaticContinuousAbility::Battlefield {
            id: ctx.alloc_static_id(),
            selector,
            parts: StaticParts(parts),
        },
        cond,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_cost_modifier_into_program(
    subject: &v2::ObjectFilter,
    delta: &v2::ManaCost,
    less: bool,
    scale: Option<&v2::ValueExpr>,
    min_mana: Option<u8>,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if min_mana.is_some() {
        return Err(LoweringError::Unsupported(
            "v2:static-cost-modifier-min-mana",
        ));
    }
    let generic_u8 = lower_pure_generic_mana_u8(delta, "v2:static-cost-modifier-colored")?;

    let is_self_cost_mod = matches!(program.kind, ProgramKind::Spell(_))
        || (less && is_bare_any_spell(subject) && (scale.is_some() || condition.is_some()));

    if is_self_cost_mod {
        if !less {
            return Err(LoweringError::Unsupported("v2:static-self-cost-increase"));
        }
        match (scale, condition) {
            (None, Some(cond)) => {
                let rc = lower_resolution_condition(cond, ctx)?;
                program.cast_reductions.push(CastReduction::Conditional {
                    condition: Box::new(rc),
                    discount: generic_u8,
                });
                return Ok(());
            }
            (Some(scale_expr), None) => {
                let mut ve = lower_value_expr(scale_expr, ctx)?;
                if !ve.is_world_evaluable() {
                    return Err(LoweringError::Unsupported(
                        "v2:static-self-cost-scale-nonworld",
                    ));
                }
                if generic_u8 > 1 {
                    ve = EngineValueExpr::Scale {
                        expr: Box::new(ve),
                        factor: RulesInteger::from(i64::from(generic_u8)),
                    };
                }
                program
                    .cast_reductions
                    .push(CastReduction::Dynamic(Box::new(ve)));
                return Ok(());
            }
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:static-self-cost-modifier-shape",
                ));
            }
        }
    }

    let (any_player, opponents_only) = match subject.controller {
        None => (true, false),
        Some(ControllerRelation::You) => (false, false),
        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => (false, true),
        Some(_) => {
            return Err(LoweringError::Unsupported(
                "v2:static-cost-modifier-controller",
            ));
        }
    };
    let matching = lower_spell_filter(subject, ctx)?;
    let cond = lower_opt_static_cond(condition, ctx)?;

    let modifier = if less {
        if opponents_only {
            return Err(LoweringError::Unsupported(
                "v2:static-cost-discount-opponents-only",
            ));
        }
        let amount = match scale {
            None => {
                let nz = NonZeroU8::new(generic_u8)
                    .ok_or(LoweringError::Unsupported("v2:static-cost-discount-zero"))?;
                SpellDiscount::Fixed(nz)
            }
            Some(scale_expr) => {
                let mut ve = lower_value_expr(scale_expr, ctx)?;
                if !ve.is_world_evaluable() {
                    return Err(LoweringError::Unsupported(
                        "v2:static-cost-discount-scale-nonworld",
                    ));
                }
                if generic_u8 > 1 {
                    ve = EngineValueExpr::Scale {
                        expr: Box::new(ve),
                        factor: RulesInteger::from(i64::from(generic_u8)),
                    };
                }
                SpellDiscount::Dynamic(Box::new(ve))
            }
        };
        SpellCostModifier::Discount {
            any_player,
            matching,
            amount,
        }
    } else {
        if scale.is_some() {
            return Err(LoweringError::Unsupported("v2:static-cost-increase-scaled"));
        }
        let nz = NonZeroU8::new(generic_u8)
            .ok_or(LoweringError::Unsupported("v2:static-cost-increase-zero"))?;
        SpellCostModifier::Increase {
            any_player,
            opponents_only,
            matching,
            amount: nz,
        }
    };

    push_static_ability(
        program,
        StaticContinuousAbility::CostModifier {
            id: ctx.alloc_static_id(),
            modifier,
        },
        cond,
    )
}

fn lower_rule_restriction_into_program(
    subject: Option<&v2::SubjectRef>,
    rule: &v2::RuleRestrictionKind,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match rule {
        v2::RuleRestrictionKind::CantBeCountered => match subject {
            None | Some(v2::SubjectRef::SelfObject) => {
                if condition.is_some() {
                    return Err(LoweringError::Unsupported(
                        "v2:static-conditional-self-uncounterable",
                    ));
                }
                program.cannot_be_countered = true;
                Ok(())
            }
            Some(v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f)) => {
                let sf = lower_spell_filter(f, ctx)?;
                let perm = match f.controller {
                    Some(ControllerRelation::You) => {
                        CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(Box::new(sf))
                    }
                    None => CastPermissionRule::AllSpellsCannotBeCounteredFiltered(Box::new(sf)),
                    Some(_) => {
                        return Err(LoweringError::Unsupported(
                            "v2:static-uncounterable-controller",
                        ));
                    }
                };
                let cond = lower_opt_static_cond(condition, ctx)?;
                push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ctx.alloc_static_id(),
                        permission: perm,
                    },
                    cond,
                )
            }
            Some(_) => Err(LoweringError::Unsupported(
                "v2:static-uncounterable-subject",
            )),
        },
        v2::RuleRestrictionKind::DamageCantBePrevented => {
            if matches!(subject, Some(v2::SubjectRef::SelfObject)) && condition.is_none() {
                program
                    .printed_keywords
                    .insert(KeywordAbility::DamageCannotBePrevented);
                return Ok(());
            }
            if subject.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:static-damage-cant-be-prevented-subject",
                ));
            }
            let cond = lower_opt_static_cond(condition, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticDamageRule::DamageCannotBePrevented,
                },
                cond,
            )
        }
        v2::RuleRestrictionKind::ActivatedAbilitiesCantBeActivated => {
            let non_mana_only = matches!(
                subject,
                Some(v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f))
                    if f.predicates.contains(&v2::FilterPredicate::NotManaAbility)
            );
            let sel = lower_opt_subject_to_permanent_selector(subject, ctx)?;
            let cond = lower_opt_static_cond(condition, ctx)?;
            let restriction = if non_mana_only {
                StaticRestriction::PreventMatchingNonManaActivations(Box::new(sel))
            } else {
                StaticRestriction::PreventMatchingActivations(Box::new(sel))
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: restriction,
                },
                cond,
            )
        }
        v2::RuleRestrictionKind::CantBecomeUntapped => {
            let sel = lower_opt_subject_to_permanent_selector(subject, ctx)?;
            let cond = lower_opt_static_cond(condition, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::CannotBeUntapped(Box::new(sel)),
                },
                cond,
            )
        }
        v2::RuleRestrictionKind::CantBeTargetedBy(flt) => {
            let sel = lower_opt_subject_to_permanent_selector(subject, ctx)?;
            let (auras_only, colors) = lower_cant_be_targeted_by_filter(flt)?;
            let cond = lower_opt_static_cond(condition, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::CantBeTargetedBy {
                        selector: Box::new(sel),
                        auras_only,
                        colors,
                    },
                },
                cond,
            )
        }
        v2::RuleRestrictionKind::AreGoaded => {
            if matches!(
                subject,
                Some(v2::SubjectRef::Anaphora(v2::AnaphoraRef::EnchantedCreature))
            ) && condition.is_none()
            {
                push_attack_requirement(program, AttackRequirement::EnchantedCreatureIsGoaded);
                Ok(())
            } else {
                Err(LoweringError::Unsupported("v2:static-are-goaded"))
            }
        }
        v2::RuleRestrictionKind::CantBeCast => {
            Err(LoweringError::Unsupported("v2:static-rule-cant-be-cast"))
        }
        v2::RuleRestrictionKind::CantAttackUnless(unless_cond)
        | v2::RuleRestrictionKind::CantBlockUnless(unless_cond)
        | v2::RuleRestrictionKind::CantAttackOrBlockUnless(unless_cond) => {
            let restriction = match rule {
                v2::RuleRestrictionKind::CantAttackUnless(_) => BlockingRestriction::CannotAttack,
                v2::RuleRestrictionKind::CantBlockUnless(_) => BlockingRestriction::CannotBlock,
                v2::RuleRestrictionKind::CantAttackOrBlockUnless(_) => {
                    BlockingRestriction::CannotAttackOrBlock
                }
                _ => unreachable!(),
            };
            let sel = lower_opt_subject_to_permanent_selector(subject, ctx)?;
            let negated = v2::ConditionExpr::Not(Box::new(unless_cond.as_ref().clone()));
            let combined = match condition {
                Some(outer) => v2::ConditionExpr::And(Box::new(outer.clone()), Box::new(negated)),
                None => negated,
            };
            let cond = lower_static_source_condition(&combined, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(restriction)]),
                },
                Some(cond),
            )
        }
        v2::RuleRestrictionKind::CantBeEnchantedBy(_) => Err(LoweringError::Unsupported(
            "v2:static-rule-cant-be-enchanted-by",
        )),
        v2::RuleRestrictionKind::CantPhaseOut => {
            Err(LoweringError::Unsupported("v2:static-rule-cant-phase-out"))
        }
        // Combat blocking / attacking restrictions mapped onto `StaticPart::BlockingRestriction`.
        combat_rule => {
            let sel = lower_opt_subject_to_permanent_selector(subject, ctx)?;
            let parts = lower_combat_rule_to_static_parts(combat_rule, ctx)?;
            let cond = lower_opt_static_cond(condition, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(parts),
                },
                cond,
            )
        }
    }
}

fn lower_combat_rule_to_static_parts(
    rule: &v2::RuleRestrictionKind,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Vec<StaticPart>> {
    match rule {
        v2::RuleRestrictionKind::CantAttack => Ok(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CannotAttack,
        )]),
        v2::RuleRestrictionKind::CantBlock => Ok(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CannotBlock,
        )]),
        v2::RuleRestrictionKind::CantAttackOrBlock => Ok(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CannotAttackOrBlock,
        )]),
        v2::RuleRestrictionKind::CantBeBlocked => Ok(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CannotBeBlocked,
        )]),
        v2::RuleRestrictionKind::CantBeBlockedBy(f) => Ok(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CannotBeBlockedByMatching(lower_object_filter(f, ctx)?),
        )]),
        v2::RuleRestrictionKind::CantBeBlockedExceptBy(f) => {
            Ok(vec![StaticPart::BlockingRestriction(
                BlockingRestriction::OnlyBlockedByMatching(lower_object_filter(f, ctx)?),
            )])
        }
        v2::RuleRestrictionKind::CantBeBlockedByMoreThan(n) => {
            Ok(vec![StaticPart::BlockingRestriction(
                BlockingRestriction::CannotBeBlockedByMoreThan(*n),
            )])
        }
        v2::RuleRestrictionKind::CantBeBlockedExceptByCount(2) => Ok(vec![StaticPart::Abilities(
            AbilityChange::grant_keywords([KeywordAbility::Menace]),
        )]),
        v2::RuleRestrictionKind::CantBeBlockedExceptByCount(3) => {
            Ok(vec![StaticPart::BlockingRestriction(
                BlockingRestriction::AtLeastThreeBlockers,
            )])
        }
        v2::RuleRestrictionKind::CantBeBlockedExceptByCount(_) => Err(LoweringError::Unsupported(
            "v2:static-cant-be-blocked-except-by-count",
        )),
        v2::RuleRestrictionKind::CantAttackUnlessDefendingControls(f) => {
            Ok(vec![StaticPart::BlockingRestriction(
                BlockingRestriction::CannotAttackUnlessDefenderControls(lower_object_filter(
                    f, ctx,
                )?),
            )])
        }
        v2::RuleRestrictionKind::CanBlockOnly(f) => Ok(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CanBlockOnlyMatching(lower_object_filter(f, ctx)?),
        )]),
        v2::RuleRestrictionKind::MustBeBlocked => Ok(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::MustBeBlocked,
        )]),
        v2::RuleRestrictionKind::CantAttackAlone => Ok(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CannotAttackAlone,
        )]),
        v2::RuleRestrictionKind::CantBlockAlone => Ok(vec![StaticPart::BlockingRestriction(
            BlockingRestriction::CannotBlockAlone,
        )]),
        v2::RuleRestrictionKind::CantAttackOrBlockAlone => Ok(vec![
            StaticPart::BlockingRestriction(BlockingRestriction::CannotAttackAlone),
            StaticPart::BlockingRestriction(BlockingRestriction::CannotBlockAlone),
        ]),
        v2::RuleRestrictionKind::CantBlockSubject(subj) => match subj.as_ref() {
            v2::SubjectRef::SelfObject => Ok(vec![StaticPart::BlockingRestriction(
                BlockingRestriction::CannotBlockSource,
            )]),
            v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => {
                Ok(vec![StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotBlockMatching(lower_object_filter(f, ctx)?),
                )])
            }
            _ => Err(LoweringError::Unsupported("v2:static-cant-block-subject")),
        },
        v2::RuleRestrictionKind::CantBlockOrBeBlockedBy(f) => {
            let ef = lower_object_filter(f, ctx)?;
            Ok(vec![
                StaticPart::BlockingRestriction(BlockingRestriction::CannotBlockMatching(ef)),
                StaticPart::BlockingRestriction(BlockingRestriction::CannotBeBlockedByMatching(ef)),
            ])
        }
        v2::RuleRestrictionKind::CantAttackTarget(tgt) => match tgt.as_ref() {
            v2::SubjectRef::Player(v2::PlayerRef::You) => {
                Ok(vec![StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotAttackSourceController,
                )])
            }
            _ => Err(LoweringError::Unsupported("v2:static-cant-attack-target")),
        },
        _ => Err(LoweringError::Unsupported("v2:static-rule-restriction")),
    }
}

fn lower_player_restriction_into_program(
    player: v2::PlayerRef,
    restriction: v2::PlayerRestrictionKind,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let cond = lower_opt_static_cond(condition, ctx)?;
    let ability = match restriction {
        v2::PlayerRestrictionKind::CantCastSpells => {
            let rel = lower_static_player_relation(player)?;
            StaticContinuousAbility::Restriction {
                id: ctx.alloc_static_id(),
                rule: StaticRestriction::ProhibitCasting {
                    player: rel,
                    filter: Box::new(SpellFilter::any()),
                },
            }
        }
        v2::PlayerRestrictionKind::CantActivateAbilities => {
            return Err(LoweringError::Unsupported(
                "v2:static-player-cant-activate-abilities",
            ));
        }
        v2::PlayerRestrictionKind::CastOnlyAsSorcery => match player {
            v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::OpponentsSorceryOnly,
                }
            }
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:static-cast-only-as-sorcery-player",
                ));
            }
        },
        v2::PlayerRestrictionKind::PlaysWithHandRevealed => {
            let opponents_only = match player {
                v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => true,
                v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => false,
                _ => {
                    return Err(LoweringError::Unsupported("v2:static-reveal-hands-player"));
                }
            };
            StaticContinuousAbility::CastPermission {
                id: ctx.alloc_static_id(),
                permission: CastPermissionRule::RevealHands { opponents_only },
            }
        }
        v2::PlayerRestrictionKind::CantGainLife => {
            let rule = match player {
                v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                    StaticRestriction::OpponentsCannotGainLife
                }
                v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
                    StaticRestriction::PlayersCannotGainLife
                }
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:static-cant-gain-life-player",
                    ));
                }
            };
            StaticContinuousAbility::Restriction {
                id: ctx.alloc_static_id(),
                rule,
            }
        }
        v2::PlayerRestrictionKind::CantSearchLibraries => {
            let rel = lower_static_player_relation(player)?;
            StaticContinuousAbility::Restriction {
                id: ctx.alloc_static_id(),
                rule: StaticRestriction::PlayersCantSearchLibraries(rel),
            }
        }
        v2::PlayerRestrictionKind::CantDrawMoreThanOne => match player {
            v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                StaticContinuousAbility::DrawRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticDrawRule::OpponentsAtMostOne,
                }
            }
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:static-cant-draw-more-than-one-player",
                ));
            }
        },
        v2::PlayerRestrictionKind::CantCastMoreThanOnePerTurn => {
            let rule = match player {
                v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
                    StaticRestriction::OneSpellPerTurn
                }
                v2::PlayerRef::You => StaticRestriction::ControllerOneSpellPerTurn,
                v2::PlayerRef::EnchantedPlayer => StaticRestriction::EnchantedPlayerCastLimit,
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:static-one-spell-per-turn-player",
                    ));
                }
            };
            StaticContinuousAbility::Restriction {
                id: ctx.alloc_static_id(),
                rule,
            }
        }
        v2::PlayerRestrictionKind::CantUntapMoreThan(_) => {
            return Err(LoweringError::Unsupported("v2:static-cant-untap-more-than"));
        }
        v2::PlayerRestrictionKind::NoMaxHandSize => {
            let rule = match player {
                v2::PlayerRef::You => StaticDrawRule::NoMaximumHandSize,
                v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
                    StaticDrawRule::AllPlayersNoMaximumHandSize
                }
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:static-no-max-hand-size-player",
                    ));
                }
            };
            StaticContinuousAbility::DrawRule {
                id: ctx.alloc_static_id(),
                rule,
            }
        }
        v2::PlayerRestrictionKind::CantWinGame => {
            return Err(LoweringError::Unsupported("v2:static-cant-win-game-alone"));
        }
        v2::PlayerRestrictionKind::CantLoseGame => match player {
            v2::PlayerRef::You => StaticContinuousAbility::Restriction {
                id: ctx.alloc_static_id(),
                rule: StaticRestriction::ControllerCantLoseGame,
            },
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:static-cant-lose-game-player",
                ));
            }
        },
        v2::PlayerRestrictionKind::CantPlayFromHand => {
            return Err(LoweringError::Unsupported("v2:static-cant-play-from-hand"));
        }
    };
    push_static_ability(program, ability, cond)
}

fn lower_set_subtypes_into_program(
    subject: &v2::SubjectRef,
    subtypes: &[String],
    keep_existing: bool,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if subtypes.is_empty() {
        return Err(LoweringError::Unsupported("v2:static-set-subtypes-empty"));
    }
    let selector = lower_subject_to_permanent_selector(subject, ctx)?;
    let mut creature_subs = BTreeSet::new();
    let mut land_subs = BTreeSet::new();
    let mut artifact_subs = BTreeSet::new();
    let mut enchantment_subs = BTreeSet::new();

    for raw in subtypes {
        if let Some(lt) = LowerCtx::lookup_land_type(raw) {
            land_subs.insert(lt);
        } else if let Some(ct) = ctx.lookup_creature_type(raw) {
            creature_subs.insert(ct);
        } else if let Some(at) = LowerCtx::lookup_artifact_subtype(raw) {
            artifact_subs.insert(at);
        } else if let Some(et) = LowerCtx::lookup_enchantment_subtype(raw) {
            enchantment_subs.insert(et);
        } else {
            return Err(LoweringError::Unsupported("v2:static-set-subtypes-unknown"));
        }
    }

    let base = StaticTypeChange {
        land_subtypes: if land_subs.is_empty() {
            SubtypeOperation::Keep
        } else if keep_existing {
            SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(land_subs))
        } else {
            SubtypeOperation::Set(StaticLandSubtypeOperand::Literal(land_subs))
        },
        supertypes: TypeOperation::Keep,
        types: TypeOperation::Keep,
        subtypes: if creature_subs.is_empty() {
            SubtypeOperation::Keep
        } else if keep_existing {
            SubtypeOperation::Add(StaticSubtypeOperand::Literal(creature_subs))
        } else {
            SubtypeOperation::Set(StaticSubtypeOperand::Literal(creature_subs))
        },
    };

    let part = if artifact_subs.is_empty() && enchantment_subs.is_empty() {
        StaticPart::Type(base)
    } else {
        StaticPart::TypeWithSubtypes {
            base,
            artifact_subtypes: if artifact_subs.is_empty() {
                SubtypeOperation::Keep
            } else if keep_existing {
                SubtypeOperation::Add(artifact_subs)
            } else {
                SubtypeOperation::Set(artifact_subs)
            },
            enchantment_subtypes: if enchantment_subs.is_empty() {
                SubtypeOperation::Keep
            } else if keep_existing {
                SubtypeOperation::Add(enchantment_subs)
            } else {
                SubtypeOperation::Set(enchantment_subs)
            },
        }
    };

    push_static_ability(
        program,
        StaticContinuousAbility::Battlefield {
            id: ctx.alloc_static_id(),
            selector,
            parts: StaticParts(vec![part]),
        },
        None,
    )
}

fn lower_bind_variable_into_program(
    var: v2::VarName,
    expr: &v2::ValueExpr,
    inner: &v2::StaticAbility,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if var != v2::VarName::X {
        return Err(LoweringError::Unsupported("v2:static-bind-non-x-var"));
    }
    let rewritten = subst_var_x_in_static(inner, expr)
        .ok_or(LoweringError::Unsupported("v2:static-bind-variable"))?;
    lower_static_into_program(&rewritten, program, ctx)
}

fn subst_var_x_in_value(val: &v2::ValueExpr, replacement: &v2::ValueExpr) -> (v2::ValueExpr, bool) {
    match val {
        v2::ValueExpr::Var(v2::VarName::X) => (replacement.clone(), true),
        v2::ValueExpr::Add(a, b) => {
            let (na, ca) = subst_var_x_in_value(a, replacement);
            let (nb, cb) = subst_var_x_in_value(b, replacement);
            (v2::ValueExpr::Add(Box::new(na), Box::new(nb)), ca || cb)
        }
        v2::ValueExpr::Sub(a, b) => {
            let (na, ca) = subst_var_x_in_value(a, replacement);
            let (nb, cb) = subst_var_x_in_value(b, replacement);
            (v2::ValueExpr::Sub(Box::new(na), Box::new(nb)), ca || cb)
        }
        v2::ValueExpr::Mul(a, b) => {
            let (na, ca) = subst_var_x_in_value(a, replacement);
            let (nb, cb) = subst_var_x_in_value(b, replacement);
            (v2::ValueExpr::Mul(Box::new(na), Box::new(nb)), ca || cb)
        }
        v2::ValueExpr::Div {
            expr,
            denom,
            rounding,
        } => {
            let (ne, ce) = subst_var_x_in_value(expr, replacement);
            (
                v2::ValueExpr::Div {
                    expr: Box::new(ne),
                    denom: *denom,
                    rounding: *rounding,
                },
                ce,
            )
        }
        other => (other.clone(), false),
    }
}

fn rewrite_x_mana_cost_to_generic(mc: &v2::ManaCost) -> Option<v2::ManaCost> {
    if mc.symbols.is_empty() {
        return None;
    }
    let mut x_count: u8 = 0;
    for sym in &mc.symbols {
        if matches!(sym, v2::ManaSymbol::VariableX) {
            x_count = x_count.checked_add(1)?;
        } else {
            return None;
        }
    }
    if x_count == 0 {
        return None;
    }
    Some(v2::ManaCost {
        symbols: vec![v2::ManaSymbol::Generic(u16::from(x_count))],
    })
}

fn subst_var_x_in_replacement(
    rep: &v2::Replacement,
    expr: &v2::ValueExpr,
) -> Option<v2::Replacement> {
    match rep {
        v2::Replacement::EntersWithCounters {
            subject,
            kind,
            count,
            additional,
        } => {
            let (nc, changed) = subst_var_x_in_value(count, expr);
            changed.then(|| v2::Replacement::EntersWithCounters {
                subject: subject.clone(),
                kind: kind.clone(),
                count: nc,
                additional: *additional,
            })
        }
        v2::Replacement::DamageRule(v2::DamageReplacement::ModifyDynamic {
            source,
            target,
            add,
        }) => {
            let (na, changed) = subst_var_x_in_value(add, expr);
            changed.then(|| {
                v2::Replacement::DamageRule(v2::DamageReplacement::ModifyDynamic {
                    source: source.clone(),
                    target: target.clone(),
                    add: na,
                })
            })
        }
        v2::Replacement::DamageRule(v2::DamageReplacement::PreventFixed {
            source,
            target,
            amount,
        }) => {
            let (na, changed) = subst_var_x_in_value(amount, expr);
            changed.then(|| {
                v2::Replacement::DamageRule(v2::DamageReplacement::PreventFixed {
                    source: source.clone(),
                    target: target.clone(),
                    amount: na,
                })
            })
        }
        v2::Replacement::ConditionalEnters { condition, inner } => {
            let ni = subst_var_x_in_replacement(inner, expr)?;
            Some(v2::Replacement::ConditionalEnters {
                condition: condition.clone(),
                inner: Box::new(ni),
            })
        }
        _ => None,
    }
}

fn subst_var_x_in_static(
    inner: &v2::StaticAbility,
    expr: &v2::ValueExpr,
) -> Option<v2::StaticAbility> {
    match inner {
        v2::StaticAbility::ContinuousModifyPT {
            subject,
            power,
            toughness,
            grant_keywords,
            scale: None,
            condition,
        } if matches!(power, v2::PtDelta::PlusX | v2::PtDelta::MinusX)
            || matches!(toughness, v2::PtDelta::PlusX | v2::PtDelta::MinusX) =>
        {
            Some(v2::StaticAbility::ContinuousModifyPT {
                subject: subject.clone(),
                power: *power,
                toughness: *toughness,
                grant_keywords: grant_keywords.clone(),
                scale: Some(Box::new(expr.clone())),
                condition: condition.clone(),
            })
        }
        v2::StaticAbility::ContinuousModifyPT {
            subject,
            power,
            toughness,
            grant_keywords,
            scale: Some(existing_scale),
            condition,
        } => {
            let (ns, changed) = subst_var_x_in_value(existing_scale, expr);
            changed.then(|| v2::StaticAbility::ContinuousModifyPT {
                subject: subject.clone(),
                power: *power,
                toughness: *toughness,
                grant_keywords: grant_keywords.clone(),
                scale: Some(Box::new(ns)),
                condition: condition.clone(),
            })
        }
        v2::StaticAbility::DynamicBasePT { power, toughness } => {
            let (np, cp) = match power {
                Some(e) => {
                    let (ne, c) = subst_var_x_in_value(e, expr);
                    (Some(ne), c)
                }
                None => (None, false),
            };
            let (nt, ct) = match toughness {
                Some(e) => {
                    let (ne, c) = subst_var_x_in_value(e, expr);
                    (Some(ne), c)
                }
                None => (None, false),
            };
            (cp || ct).then_some(v2::StaticAbility::DynamicBasePT {
                power: np,
                toughness: nt,
            })
        }
        v2::StaticAbility::CostModifier {
            subject,
            delta,
            less,
            scale,
            min_mana,
            condition,
        } => {
            if scale.is_none()
                && let Some(new_delta) = rewrite_x_mana_cost_to_generic(delta)
            {
                return Some(v2::StaticAbility::CostModifier {
                    subject: subject.clone(),
                    delta: new_delta,
                    less: *less,
                    scale: Some(expr.clone()),
                    min_mana: *min_mana,
                    condition: condition.clone(),
                });
            }
            if let Some(s) = scale {
                let (ns, changed) = subst_var_x_in_value(s, expr);
                if changed {
                    return Some(v2::StaticAbility::CostModifier {
                        subject: subject.clone(),
                        delta: delta.clone(),
                        less: *less,
                        scale: Some(ns),
                        min_mana: *min_mana,
                        condition: condition.clone(),
                    });
                }
            }
            None
        }
        v2::StaticAbility::ActivationCostModifier {
            delta,
            less,
            scale,
            condition,
        } => {
            if scale.is_none()
                && let Some(new_delta) = rewrite_x_mana_cost_to_generic(delta)
            {
                return Some(v2::StaticAbility::ActivationCostModifier {
                    delta: new_delta,
                    less: *less,
                    scale: Some(expr.clone()),
                    condition: condition.clone(),
                });
            }
            if let Some(s) = scale {
                let (ns, changed) = subst_var_x_in_value(s, expr);
                if changed {
                    return Some(v2::StaticAbility::ActivationCostModifier {
                        delta: delta.clone(),
                        less: *less,
                        scale: Some(ns),
                        condition: condition.clone(),
                    });
                }
            }
            None
        }
        v2::StaticAbility::EntersWithCountersStatic {
            subject,
            kind,
            count,
            additional,
            condition,
        } => {
            let (nc, changed) = subst_var_x_in_value(count, expr);
            changed.then(|| v2::StaticAbility::EntersWithCountersStatic {
                subject: subject.clone(),
                kind: kind.clone(),
                count: nc,
                additional: *additional,
                condition: condition.clone(),
            })
        }
        v2::StaticAbility::ConditionalReplacement {
            condition,
            replacement,
        } => {
            let nr = subst_var_x_in_replacement(replacement, expr)?;
            Some(v2::StaticAbility::ConditionalReplacement {
                condition: condition.clone(),
                replacement: Box::new(nr),
            })
        }
        v2::StaticAbility::Compound(parts) => {
            let mut any_changed = false;
            let mut new_parts = Vec::with_capacity(parts.len());
            for part in parts {
                if let Some(rewritten) = subst_var_x_in_static(part, expr) {
                    new_parts.push(rewritten);
                    any_changed = true;
                } else {
                    new_parts.push(part.clone());
                }
            }
            any_changed.then_some(v2::StaticAbility::Compound(new_parts))
        }
        _ => None,
    }
}

fn lower_visibility_rule_into_program(
    player: v2::PlayerRef,
    kind: v2::VisibilityKind,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let permission = match kind {
        v2::VisibilityKind::LookAtTop(zr) => {
            if player == v2::PlayerRef::You
                && zr.zone == v2::ZoneKind::Library
                && matches!(zr.owner, v2::ZoneOwner::You | v2::ZoneOwner::Default)
            {
                CastPermissionRule::SeeLibraryTop { public: false }
            } else {
                return Err(LoweringError::Unsupported("v2:static-visibility-look-top"));
            }
        }
        v2::VisibilityKind::LookAtExiledWithSelf => {
            if player == v2::PlayerRef::You {
                CastPermissionRule::LookAtSourceExiledCards
            } else {
                return Err(LoweringError::Unsupported(
                    "v2:static-visibility-exiled-non-you",
                ));
            }
        }
        v2::VisibilityKind::PlayWithTopRevealed => match player {
            v2::PlayerRef::You => CastPermissionRule::SeeLibraryTop { public: true },
            v2::PlayerRef::EachPlayer | v2::PlayerRef::AnyPlayer => {
                CastPermissionRule::AllPlayersRevealLibraryTop
            }
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:static-visibility-top-revealed-player",
                ));
            }
        },
    };
    push_static_ability(
        program,
        StaticContinuousAbility::CastPermission {
            id: ctx.alloc_static_id(),
            permission,
        },
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_zone_cast_permission_into_program(
    player: v2::PlayerRef,
    filter: Option<&v2::ObjectFilter>,
    subject: Option<&v2::SubjectRef>,
    zone: Option<&v2::ZoneRef>,
    mode: &v2::CastPermissionMode,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let cond = lower_opt_static_cond(condition, ctx)?;
    match mode {
        v2::CastPermissionMode::AsThoughFlash { extra_cost } => {
            let is_self = matches!(subject, Some(v2::SubjectRef::SelfObject))
                || (subject.is_none() && filter.is_none() && player == v2::PlayerRef::You);
            if is_self {
                match (extra_cost.as_deref(), condition) {
                    (Some(v2::Cost::Mana(mc)), None) => {
                        let mana = lower_v2_fixed_mana_cost(mc)?;
                        program.conditional_flash = Some(SelfFlashPermission::PayExtraMana(mana));
                        return Ok(());
                    }
                    (None, Some(c)) => {
                        let rc = lower_resolution_condition(c, ctx)?;
                        program.conditional_flash =
                            Some(SelfFlashPermission::Condition(Box::new(rc)));
                        return Ok(());
                    }
                    (None, None) => {
                        program.printed_keywords.insert(KeywordAbility::Flash);
                        return Ok(());
                    }
                    _ => {
                        return Err(LoweringError::Unsupported("v2:static-self-flash-complex"));
                    }
                }
            }
            if extra_cost.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:static-flash-permission-extra-cost",
                ));
            }
            let any_player = match player {
                v2::PlayerRef::You => false,
                v2::PlayerRef::EachPlayer | v2::PlayerRef::AnyPlayer => true,
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:static-flash-permission-player",
                    ));
                }
            };
            let matching = match filter {
                Some(f) => lower_spell_filter(f, ctx)?,
                None => SpellFilter::any(),
            };
            push_static_ability(
                program,
                StaticContinuousAbility::CastPermission {
                    id: ctx.alloc_static_id(),
                    permission: CastPermissionRule::Flash {
                        any_player,
                        matching,
                    },
                },
                cond,
            )
        }
        v2::CastPermissionMode::PlayFromZone {
            top_only,
            lands,
            spells,
        } => lower_play_from_zone_permission(
            player, filter, subject, zone, *top_only, *lands, *spells, false, condition, cond,
            program, ctx,
        ),
        v2::CastPermissionMode::CastFromZone => lower_play_from_zone_permission(
            player, filter, subject, zone, false, false, true, false, condition, cond, program, ctx,
        ),
        v2::CastPermissionMode::OncePerTurn { free, alt_cost } => {
            if alt_cost.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:static-once-per-turn-alt-cost",
                ));
            }
            if !*free && zone.is_some() {
                return lower_play_from_zone_permission(
                    player, filter, subject, zone, false, false, true, true, condition, cond,
                    program, ctx,
                );
            }
            if *free && zone.is_none() {
                let each_player = match player {
                    v2::PlayerRef::You => false,
                    v2::PlayerRef::EachPlayer | v2::PlayerRef::AnyPlayer => true,
                    _ => {
                        return Err(LoweringError::Unsupported("v2:static-once-per-turn-player"));
                    }
                };
                let sf = match filter {
                    Some(f) => lower_spell_filter(f, ctx)?,
                    None => SpellFilter::any(),
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CostModifier {
                        id: ctx.alloc_static_id(),
                        modifier: SpellCostModifier::AlternativeForMatching {
                            filter: sf,
                            mana: ManaCost::free(),
                            once_each_turn: true,
                            own_turn_only: false,
                            pay_life_mv: false,
                            each_player,
                        },
                    },
                    cond,
                );
            }
            Err(LoweringError::Unsupported(
                "v2:static-zone-cast-once-per-turn",
            ))
        }
        v2::CastPermissionMode::FirstSpellFree => {
            if zone.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:static-first-spell-free-with-zone",
                ));
            }
            let each_player = match player {
                v2::PlayerRef::You => false,
                v2::PlayerRef::EachPlayer | v2::PlayerRef::AnyPlayer => true,
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:static-first-spell-free-player",
                    ));
                }
            };
            let sf = match filter {
                Some(f) => lower_spell_filter(f, ctx)?,
                None => SpellFilter::any(),
            };
            push_static_ability(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ctx.alloc_static_id(),
                    modifier: SpellCostModifier::FirstSpellRule {
                        filter: sf,
                        own_turn_only: false,
                        each_player,
                        free: true,
                        pay_life_mv: false,
                        discount: 0,
                    },
                },
                cond,
            )
        }
        v2::CastPermissionMode::WithoutPaying => {
            if zone.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:static-without-paying-with-zone",
                ));
            }
            let each_player = match player {
                v2::PlayerRef::You => false,
                v2::PlayerRef::EachPlayer | v2::PlayerRef::AnyPlayer => true,
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:static-without-paying-player",
                    ));
                }
            };
            let sf = match filter {
                Some(f) => lower_spell_filter(f, ctx)?,
                None => SpellFilter::any(),
            };
            push_static_ability(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ctx.alloc_static_id(),
                    modifier: SpellCostModifier::AlternativeForMatching {
                        filter: sf,
                        mana: ManaCost::free(),
                        once_each_turn: false,
                        own_turn_only: false,
                        pay_life_mv: false,
                        each_player,
                    },
                },
                cond,
            )
        }
        v2::CastPermissionMode::AlternativeCost(costs) => {
            if zone.is_some() {
                return Err(LoweringError::Unsupported("v2:static-alt-cost-with-zone"));
            }
            let [v2::Cost::Mana(mc)] = costs.as_slice() else {
                return Err(LoweringError::Unsupported("v2:static-zone-cast-alt-cost"));
            };
            let each_player = match player {
                v2::PlayerRef::You => false,
                v2::PlayerRef::EachPlayer | v2::PlayerRef::AnyPlayer => true,
                _ => {
                    return Err(LoweringError::Unsupported("v2:static-alt-cost-player"));
                }
            };
            let sf = match filter {
                Some(f) => lower_spell_filter(f, ctx)?,
                None => SpellFilter::any(),
            };
            let mana = lower_v2_fixed_mana_cost(mc)?;
            push_static_ability(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ctx.alloc_static_id(),
                    modifier: SpellCostModifier::AlternativeForMatching {
                        filter: sf,
                        mana,
                        once_each_turn: false,
                        own_turn_only: false,
                        pay_life_mv: false,
                        each_player,
                    },
                },
                cond,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_play_from_zone_permission(
    player: v2::PlayerRef,
    filter: Option<&v2::ObjectFilter>,
    subject: Option<&v2::SubjectRef>,
    zone: Option<&v2::ZoneRef>,
    _top_only: bool,
    lands: bool,
    spells: bool,
    once_each_turn: bool,
    raw_cond: Option<&v2::ConditionExpr>,
    static_cond: Option<StaticSourceCondition>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if player != v2::PlayerRef::You {
        return Err(LoweringError::Unsupported(
            "v2:static-play-from-zone-player",
        ));
    }
    let Some(zr) = zone else {
        return Err(LoweringError::Unsupported(
            "v2:static-play-from-zone-missing",
        ));
    };

    if matches!(subject, Some(v2::SubjectRef::SelfObject)) {
        match zr.zone {
            v2::ZoneKind::Graveyard => {
                if let Some(c) = raw_cond {
                    let rc = lower_resolution_condition(c, ctx)?;
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ctx.alloc_static_id(),
                            permission: CastPermissionRule::GraveyardSelfConditional {
                                condition: Box::new(rc),
                            },
                        },
                        None,
                    );
                }
                program.graveyard_self_cast = Some(GraveyardSelfCast::Unconditional);
                return Ok(());
            }
            v2::ZoneKind::Exile if raw_cond.is_none() => {
                program.cast_from_own_exile = true;
                return Ok(());
            }
            _ => {
                return Err(LoweringError::Unsupported("v2:static-self-zone-cast"));
            }
        }
    }

    if !lands && !spells {
        return Err(LoweringError::Unsupported("v2:static-play-from-zone-kind"));
    }

    let ef = match filter {
        Some(f) => lower_object_filter(f, ctx)?,
        None if lands && !spells => ObjectFilter::land(),
        None => ObjectFilter::any(),
    };

    let permission = match zr.zone {
        v2::ZoneKind::Library
            if matches!(zr.owner, v2::ZoneOwner::You | v2::ZoneOwner::Default) =>
        {
            if once_each_turn {
                CastPermissionRule::PlayLibraryTopFiltered {
                    lands_allowed: lands,
                    filter: Box::new(ef),
                    once_each_turn: true,
                    own_turn_only: false,
                }
            } else {
                CastPermissionRule::PlayLibraryTopMatching {
                    lands_allowed: lands,
                    filter: Box::new(ef),
                }
            }
        }
        v2::ZoneKind::Graveyard
            if matches!(zr.owner, v2::ZoneOwner::You | v2::ZoneOwner::Default) =>
        {
            CastPermissionRule::GraveyardMatching {
                lands_allowed: lands,
                filter: Box::new(ef),
                once_each_turn,
            }
        }
        v2::ZoneKind::Exile => {
            let linked = filter
                .is_some_and(|f| f.flags.contains(v2::FilterFlags::EXILED_WITH_SELF))
                || matches!(
                    subject,
                    Some(v2::SubjectRef::Anaphora(
                        v2::AnaphoraRef::TheExiledCard | v2::AnaphoraRef::ExiledObject
                    ))
                );
            CastPermissionRule::ExileMatchingExt {
                filter: Box::new(ef),
                linked_to_source: linked,
                free: false,
                any_color: false,
                lands_allowed: lands,
                once_each_turn,
                own_turn_only: false,
            }
        }
        _ => {
            return Err(LoweringError::Unsupported("v2:static-play-from-zone-kind"));
        }
    };

    push_static_ability(
        program,
        StaticContinuousAbility::CastPermission {
            id: ctx.alloc_static_id(),
            permission,
        },
        static_cond,
    )
}

fn lower_trigger_modifier_into_program(
    kind: v2::TriggerModifierKind,
    filter: &v2::ObjectFilter,
    permanent_filter: Option<&v2::ObjectFilter>,
    extra_count: u8,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match kind {
        v2::TriggerModifierKind::EntersDontTrigger => {
            let types = filter.all_types.union(filter.any_types);
            let is_creatures = types == AstCardTypeMask::CREATURE
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty()
                && filter.controller.is_none()
                && filter.predicates.is_empty();
            if !is_creatures {
                return Err(LoweringError::Unsupported(
                    "v2:static-enters-dont-trigger-filtered",
                ));
            }
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::SuppressEntersTriggers,
                },
                None,
            )
        }
        v2::TriggerModifierKind::AdditionalOnAbility => {
            if extra_count == 0 {
                return Err(LoweringError::Unsupported(
                    "v2:static-trigger-modifier-zero",
                ));
            }
            let triggering_source = if filter.is_empty() {
                None
            } else {
                Some(lower_permanent_selector(
                    &v2::SubjectRef::All(Box::new(filter.clone())),
                    ctx,
                )?)
            };
            let exclude_self = filter.flags.contains(v2::FilterFlags::EXCLUDE_SELF);
            push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ctx.alloc_static_id(),
                    rule: StaticModifierRule::AdditionalTrigger(Box::new(TriggerMultiplierRule {
                        additional: extra_count,
                        event: TriggerMultiplierEvent::Any,
                        triggering_source,
                        exclude_self,
                        exclude_sacrifice_triggers: false,
                    })),
                },
                None,
            )
        }
        v2::TriggerModifierKind::AdditionalTriggers => {
            // `v2::TriggerModifierKind::AdditionalTriggers` collapses entering, dying,
            // attacking, and combat-damage trigger doublers without recording the event kind.
            let _ = permanent_filter;
            Err(LoweringError::Unsupported(
                "v2:static-trigger-modifier-event-unspecified",
            ))
        }
    }
}

fn lower_hand_size_into_program(
    player: v2::PlayerRef,
    rule: &v2::HandSizeRule,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match rule {
        v2::HandSizeRule::Delta(delta) => {
            if *delta == 0 {
                return Err(LoweringError::Unsupported("v2:static-hand-size-zero-delta"));
            }
            let delta_i8 = i8::try_from(*delta)
                .map_err(|_| LoweringError::Unsupported("v2:static-hand-size-delta-overflow"))?;
            let rel = lower_static_player_relation(player)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ctx.alloc_static_id(),
                    rule: StaticModifierRule::ModifyMaxHandSize {
                        player: rel,
                        delta: delta_i8,
                    },
                },
                None,
            )
        }
        v2::HandSizeRule::Set(val) => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported("v2:static-hand-size-set-player"));
            }
            let n = lower_value_const(val)
                .and_then(|v| u8::try_from(v).ok())
                .ok_or(LoweringError::Unsupported(
                    "v2:static-hand-size-set-dynamic",
                ))?;
            push_static_ability(
                program,
                StaticContinuousAbility::DrawRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticDrawRule::MaxHandSize(n),
                },
                None,
            )
        }
    }
}

fn lower_subject_rule_into_program(
    subject: &v2::SubjectRef,
    rule: &v2::SubjectStaticRule,
    condition: Option<&v2::ConditionExpr>,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let cond = lower_opt_static_cond(condition, ctx)?;
    match rule {
        v2::SubjectStaticRule::CanAttackWithDefender => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                        BlockingRestriction::CanAttackAsThoughNoDefender,
                    )]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::DoesntUntap => match subject {
            v2::SubjectRef::SelfObject => push_static_ability(
                program,
                StaticContinuousAbility::UntapRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticUntapRule::SourceSkipsUntap,
                },
                cond,
            ),
            v2::SubjectRef::Anaphora(
                v2::AnaphoraRef::EnchantedCreature | v2::AnaphoraRef::EnchantedPermanent,
            ) => push_static_ability(
                program,
                StaticContinuousAbility::UntapRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticUntapRule::EnchantedCreatureSkipsUntap,
                },
                cond,
            ),
            _ => {
                let sel = lower_subject_to_permanent_selector(subject, ctx)?;
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ctx.alloc_static_id(),
                        selector: sel,
                        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
                            doesnt_untap: true,
                            ..AbilityChange::default()
                        })]),
                    },
                    cond,
                )
            }
        },
        v2::SubjectStaticRule::MustAttack => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                        BlockingRestriction::MustAttackEachCombatIfAble,
                    )]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::CanBlockAdditional(n) => {
            if *n == 0 {
                return Err(LoweringError::Unsupported(
                    "v2:static-can-block-additional-zero",
                ));
            }
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            let parts = (0..*n)
                .map(|_| StaticPart::BlockingRestriction(BlockingRestriction::AdditionalBlock))
                .collect();
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(parts),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::CanBlockAnyNumber => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                        BlockingRestriction::CanBlockAnyNumber,
                    )]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::EntersTapped => {
            if matches!(subject, v2::SubjectRef::SelfObject) && cond.is_none() {
                return set_entry_tap(program, EntryTap::Always);
            }
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::OthersEnterTapped(Box::new(sel)),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::EntersUntapped => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::EnterUntapped(Box::new(sel)),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::MayNotUntap => {
            if !matches!(subject, v2::SubjectRef::SelfObject) {
                return Err(LoweringError::Unsupported(
                    "v2:static-may-not-untap-non-self",
                ));
            }
            push_static_ability(
                program,
                StaticContinuousAbility::UntapRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticUntapRule::MayChooseNotToUntapSource,
                },
                cond,
            )
        }
        v2::SubjectStaticRule::AllCreatureTypes => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Add(StaticSubtypeOperand::AllCreatureTypes),
                    })]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::UntapDuringEachOtherUntap => {
            let rule = if matches!(subject, v2::SubjectRef::SelfObject) {
                StaticUntapRule::UntapDuringOtherPlayersUntap
            } else {
                let sel = lower_subject_to_permanent_selector(subject, ctx)?;
                StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(Box::new(sel))
            };
            push_static_ability(
                program,
                StaticContinuousAbility::UntapRule {
                    id: ctx.alloc_static_id(),
                    rule,
                },
                cond,
            )
        }
        v2::SubjectStaticRule::AdditionalTypesChosen | v2::SubjectStaticRule::IsChosenType => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Add(
                            StaticSubtypeOperand::SourceEntryCreatureType,
                        ),
                    })]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::IsChosenColor => {
            Err(LoweringError::Unsupported("v2:static-is-chosen-color"))
        }
        v2::SubjectStaticRule::AssignDamageByToughness => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                        BlockingRestriction::AssignDamageByToughness,
                    )]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::AssignDamageAsUnblocked => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                        BlockingRestriction::AssignDamageAsThoughUnblocked,
                    )]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::AllMustBlock => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                        BlockingRestriction::AllDefendingCreaturesMustBlock,
                    )]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::IgnoreKeywordForBlocking(kw) => {
            if *kw == v2::KeywordKind::Hexproof {
                let sel = lower_subject_to_permanent_selector(subject, ctx)?;
                push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ctx.alloc_static_id(),
                        rule: StaticRestriction::IgnoreOpponentsHexproof(Box::new(sel)),
                    },
                    cond,
                )
            } else {
                Err(LoweringError::Unsupported(
                    "v2:static-ignore-keyword-for-blocking",
                ))
            }
        }
        v2::SubjectStaticRule::CopyActivatedAbilities(flt) => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            let zone = lower_static_zone_from_filter(flt)?;
            let mut stripped = flt.clone();
            stripped.zones.clear();
            let ef = lower_object_filter(&stripped, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::InheritZoneAbilities {
                        zone,
                        filter: Box::new(ef),
                        loyalty_only: false,
                    }]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::RemoveTypes(mask) => {
            let removed = lower_card_type_set(*mask);
            if removed.is_empty() {
                return Err(LoweringError::Unsupported("v2:static-remove-empty-types"));
            }
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Remove(removed),
                        subtypes: SubtypeOperation::Keep,
                    })]),
                },
                cond,
            )
        }
        v2::SubjectStaticRule::SetColors {
            colors,
            keep_existing,
        } => {
            let sel = lower_subject_to_permanent_selector(subject, ctx)?;
            let color_set = lower_color_set(*colors);
            let op = if *keep_existing {
                ColorOperation::Add(StaticColorOperand::Literal(color_set))
            } else {
                ColorOperation::Set(StaticColorOperand::Literal(color_set))
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::Color(op)]),
                },
                cond,
            )
        }
    }
}

fn lower_global_flag_into_program(
    flag: v2::StaticFlagKind,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match flag {
        v2::StaticFlagKind::CostReductionRule => {
            Err(LoweringError::Unsupported("v2:static-cost-reduction-rule"))
        }
        v2::StaticFlagKind::DontLoseForZeroLife => push_static_ability(
            program,
            StaticContinuousAbility::Restriction {
                id: ctx.alloc_static_id(),
                rule: StaticRestriction::ControllerCantLoseForZeroLife,
            },
            None,
        ),
        v2::StaticFlagKind::EntersUnderOpponentControl => {
            set_entry_tap(program, EntryTap::ChosenOpponentControls)
        }
        v2::StaticFlagKind::FlashSacrificeAtCleanup => push_static_ability(
            program,
            StaticContinuousAbility::Restriction {
                id: ctx.alloc_static_id(),
                rule: StaticRestriction::FlashSacrificeAtCleanup,
            },
            None,
        ),
        v2::StaticFlagKind::EntersPrepared => {
            if !matches!(program.kind, ProgramKind::Permanent(_)) {
                return Err(LoweringError::Unsupported(
                    "v2:static-enters-prepared-nonpermanent",
                ));
            }
            program.enters_prepared = true;
            Ok(())
        }
        v2::StaticFlagKind::AllColors => push_static_ability(
            program,
            StaticContinuousAbility::CharacteristicDefining {
                id: ctx.alloc_static_id(),
                parts: DefiningParts(vec![DefiningPart::Colors(BTreeSet::from(Color::ALL))]),
            },
            None,
        ),
        v2::StaticFlagKind::CrewPowerBonus(bonus) => {
            let bonus_u8 = u8::try_from(bonus)
                .ok()
                .filter(|&v| v > 0)
                .ok_or(LoweringError::Unsupported("v2:static-crew-power-bonus"))?;
            push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ctx.alloc_static_id(),
                    rule: StaticModifierRule::CrewAndSaddlePowerBonus {
                        selector: Box::new(PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            ..PermanentSelector::default()
                        }),
                        crews: true,
                        saddles: false,
                        bonus: bonus_u8,
                    },
                },
                None,
            )
        }
        v2::StaticFlagKind::CanBeCommander => Err(LoweringError::Unsupported(
            "v2:static-flag-can-be-commander",
        )),
        v2::StaticFlagKind::MayExertOnAttack => Err(LoweringError::Unsupported(
            "v2:static-flag-may-exert-on-attack",
        )),
        v2::StaticFlagKind::AppliesInAllZones => Err(LoweringError::Unsupported(
            "v2:static-flag-applies-in-all-zones",
        )),
        v2::StaticFlagKind::NamedSourcesCantActivate => Err(LoweringError::Unsupported(
            "v2:static-flag-named-sources-cant-activate",
        )),
        v2::StaticFlagKind::LoyaltyAsInstant => Err(LoweringError::Unsupported(
            "v2:static-flag-loyalty-as-instant",
        )),
        v2::StaticFlagKind::AnyNumberInDeck => Err(LoweringError::Unsupported(
            "v2:static-flag-any-number-in-deck",
        )),
        v2::StaticFlagKind::DraftFaceUp => {
            Err(LoweringError::Unsupported("v2:static-flag-draft-face-up"))
        }
        v2::StaticFlagKind::RetainAttachment => Err(LoweringError::Unsupported(
            "v2:static-flag-retain-attachment",
        )),
    }
}

fn lower_filter_rule_into_program(
    filter: &v2::ObjectFilter,
    kind: v2::FilterStaticKind,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match kind {
        v2::FilterStaticKind::DoubleDamage => {
            let (src_flt, src_ctrl) = lower_damage_source_filter(Some(filter), ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ctx.alloc_static_id(),
                    rule: StaticDamageRule::ModifyDamage {
                        source: src_flt,
                        source_controller: src_ctrl,
                        recipient: StaticDamageRecipient::Any,
                        kind: StaticDamageKind::Any,
                        op: StaticDamageOp::Multiply(2),
                    },
                },
                None,
            )
        }
        v2::FilterStaticKind::AlsoAttacksIfAble => {
            let sel =
                lower_permanent_selector(&v2::SubjectRef::All(Box::new(filter.clone())), ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ctx.alloc_static_id(),
                    selector: sel,
                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                        BlockingRestriction::MustAttackEachCombatIfAble,
                    )]),
                },
                None,
            )
        }
        v2::FilterStaticKind::ActivateAbilitiesAsHaste => {
            let sel =
                lower_permanent_selector(&v2::SubjectRef::All(Box::new(filter.clone())), ctx)?;
            push_static_ability(
                program,
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::ActivateAbilitiesAsThoughHaste(Box::new(sel)),
                },
                None,
            )
        }
        v2::FilterStaticKind::SpendManaAsAnyColor => Err(LoweringError::Unsupported(
            "v2:static-spend-mana-as-any-color",
        )),
        v2::FilterStaticKind::AttachmentRestriction => Err(LoweringError::Unsupported(
            "v2:static-attachment-restriction",
        )),
    }
}

fn try_combine_win_lose_compound(
    parts: &[v2::StaticAbility],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<(StaticContinuousAbility, Option<StaticSourceCondition>)>> {
    if parts.len() != 2 {
        return Ok(None);
    }
    match (&parts[0], &parts[1]) {
        (
            v2::StaticAbility::PlayerRestriction {
                player: v2::PlayerRef::You,
                restriction: v2::PlayerRestrictionKind::CantLoseGame,
                condition: c1,
            },
            v2::StaticAbility::PlayerRestriction {
                player: v2::PlayerRef::EachOpponent | v2::PlayerRef::AnyOpponent,
                restriction: v2::PlayerRestrictionKind::CantWinGame,
                condition: c2,
            },
        ) if c1 == c2 => {
            let cond = lower_opt_static_cond(c1.as_ref(), ctx)?;
            Ok(Some((
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                },
                cond,
            )))
        }
        (
            v2::StaticAbility::PlayerRestriction {
                player: v2::PlayerRef::You,
                restriction: v2::PlayerRestrictionKind::CantWinGame,
                condition: c1,
            },
            v2::StaticAbility::PlayerRestriction {
                player: v2::PlayerRef::EachOpponent | v2::PlayerRef::AnyOpponent,
                restriction: v2::PlayerRestrictionKind::CantLoseGame,
                condition: c2,
            },
        ) if c1 == c2 => {
            let cond = lower_opt_static_cond(c1.as_ref(), ctx)?;
            Ok(Some((
                StaticContinuousAbility::Restriction {
                    id: ctx.alloc_static_id(),
                    rule: StaticRestriction::ControllerCantWinOpponentsCantLose,
                },
                cond,
            )))
        }
        _ => Ok(None),
    }
}

fn lower_temp_next_spell_modifier(
    filter: &v2::ObjectFilter,
    effect: &v2::StaticAbility,
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let mut stripped = filter.clone();
    stripped.controller = None;
    let ef = lower_object_filter(&stripped, ctx)?;
    let caster_rp = match filter.controller {
        None | Some(ControllerRelation::You) => ResolvingPlayer::Controller,
        Some(ControllerRelation::ThatPlayer) => ResolvingPlayer::UniqueRelatedPlayer,
        Some(_) => {
            return Err(LoweringError::Unsupported(
                "v2:temp-next-spell-modifier-caster",
            ));
        }
    };

    match effect {
        v2::StaticAbility::Compound(parts) => {
            for part in parts {
                lower_temp_next_spell_modifier(filter, part, duration, acc, ctx)?;
            }
            Ok(())
        }
        v2::StaticAbility::RuleRestriction {
            rule: v2::RuleRestrictionKind::CantBeCountered,
            condition: None,
            ..
        } => {
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::CantBeCountered {
                        colors: EngineColorMask::EMPTY,
                        filter: Some(ef),
                        controller_only: true,
                        next_only: true,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::ZoneCastPermission {
            player,
            mode: v2::CastPermissionMode::AsThoughFlash { extra_cost: None },
            condition: None,
            ..
        } => {
            let rp = lower_resolving_player(*player, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::FilteredFlash {
                        player: rp,
                        filter: ef,
                        next_only: true,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::ZoneCastPermission {
            player,
            mode: v2::CastPermissionMode::WithoutPaying,
            condition: None,
            ..
        } => {
            let rp = lower_resolving_player(*player, acc, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::NextSpellModifier {
                        player: rp,
                        filter: Some(ef),
                        free: true,
                        keyword: None,
                        cast_keyword: None,
                        affinity_filter: None,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::CostModifier {
            delta,
            less,
            scale: None,
            min_mana: None,
            condition: None,
            ..
        } => {
            let n = lower_pure_generic_mana_u8(delta, "v2:temp-next-spell-cost-mana")?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::SpellCostModifier {
                        players: AffectedPlayers::One(caster_rp),
                        filter: ef,
                        amount: Box::new(EngineValueExpr::Literal(RulesInteger::from(i64::from(
                            n,
                        )))),
                        increase: !*less,
                        next_only: true,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::StaticAbility::GrantKeywords {
            keywords,
            condition: None,
            ..
        } if !keywords.is_empty() => {
            for kw in keywords {
                let rule = match (kw.kind, &kw.param) {
                    (v2::KeywordKind::Cascade, None) => TemporaryRuleKind::NextSpellModifier {
                        player: caster_rp,
                        filter: Some(ef),
                        free: false,
                        keyword: None,
                        cast_keyword: Some(NextSpellCastKeyword::Cascade),
                        affinity_filter: None,
                    },
                    (v2::KeywordKind::Storm, None) => TemporaryRuleKind::NextSpellModifier {
                        player: caster_rp,
                        filter: Some(ef),
                        free: false,
                        keyword: None,
                        cast_keyword: Some(NextSpellCastKeyword::Storm),
                        affinity_filter: None,
                    },
                    (v2::KeywordKind::Affinity, Some(v2::KeywordParam::Filter(aff_flt))) => {
                        if aff_flt.any_card || aff_flt.any_spell || aff_flt.is_ability {
                            return Err(LoweringError::Unsupported(
                                "v2:temp-next-spell-affinity-non-permanent",
                            ));
                        }
                        let aff_ef = lower_object_filter(aff_flt, ctx)?;
                        TemporaryRuleKind::next_spell_affinity(caster_rp, ef, aff_ef)
                    }
                    _ => {
                        let engine_kw = lower_keyword_ability(kw, ctx)?;
                        TemporaryRuleKind::NextSpellModifier {
                            player: caster_rp,
                            filter: Some(ef),
                            free: false,
                            keyword: Some(engine_kw),
                            cast_keyword: None,
                            affinity_filter: None,
                        }
                    }
                };
                acc.push(Instruction::ApplyContinuous(Box::new(
                    ApplyContinuousSpec::temporary_rule(rule, duration),
                )));
            }
            Ok(())
        }
        _ => Err(LoweringError::Unsupported("v2:temp-next-spell-modifier")),
    }
}

fn lower_temp_rule_restriction(
    subject: Option<&v2::SubjectRef>,
    rule: &v2::RuleRestrictionKind,
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match rule {
        v2::RuleRestrictionKind::DamageCantBePrevented if subject.is_none() => {
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::DamageCantBePrevented,
                    duration,
                ),
            )));
            Ok(())
        }
        v2::RuleRestrictionKind::CantBeCountered => {
            let (filter, controller_only) = match subject {
                None => (None, true),
                Some(
                    v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f),
                ) => {
                    let ctrl_only = match f.controller {
                        Some(ControllerRelation::You) => true,
                        None => false,
                        Some(_) => {
                            return Err(LoweringError::Unsupported(
                                "v2:temp-uncounterable-controller",
                            ));
                        }
                    };
                    let mut stripped = f.as_ref().clone();
                    stripped.controller = None;
                    (Some(lower_object_filter(&stripped, ctx)?), ctrl_only)
                }
                Some(_) => {
                    return Err(LoweringError::Unsupported("v2:temp-uncounterable-subject"));
                }
            };
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::CantBeCountered {
                        colors: EngineColorMask::EMPTY,
                        filter,
                        controller_only,
                        next_only: false,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        combat_rule => {
            let Some(subj) = subject else {
                return Err(LoweringError::Unsupported(
                    "v2:temp-rule-restriction-no-subject",
                ));
            };
            let query = lower_subject_to_query(subj, acc, ctx)?;
            let parts = lower_combat_rule_to_resolved_parts(combat_rule, ctx)?;
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::resolved_parts(query, ResolvedPartsSpec(parts), duration),
            )));
            Ok(())
        }
    }
}

fn lower_combat_rule_to_resolved_parts(
    rule: &v2::RuleRestrictionKind,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Vec<ResolvedPartSpec>> {
    let static_parts = lower_combat_rule_to_static_parts(rule, ctx)?;
    let mut out = Vec::with_capacity(static_parts.len());
    for part in static_parts {
        match part {
            StaticPart::BlockingRestriction(br) => {
                out.push(ResolvedPartSpec::BlockingRestriction(br));
            }
            StaticPart::Abilities(ab) => {
                out.push(ResolvedPartSpec::Abilities(ab));
            }
            _ => {
                return Err(LoweringError::Unsupported("v2:temp-combat-rule-part"));
            }
        }
    }
    Ok(out)
}

fn lower_temp_player_restriction(
    player: v2::PlayerRef,
    restriction: v2::PlayerRestrictionKind,
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let players = lower_affected_players(player, acc, ctx)?;
    let rule = match restriction {
        v2::PlayerRestrictionKind::CantCastSpells => TemporaryRuleKind::CantCastSpells {
            players,
            zone: None,
            filter: None,
        },
        v2::PlayerRestrictionKind::CantActivateAbilities => {
            TemporaryRuleKind::CantActivateAbilities {
                players,
                nonmana_only: false,
            }
        }
        v2::PlayerRestrictionKind::CantGainLife => {
            TemporaryRuleKind::PlayerCantGainLife { players }
        }
        v2::PlayerRestrictionKind::NoMaxHandSize if player == v2::PlayerRef::You => {
            TemporaryRuleKind::temporary_static(StaticContinuousAbility::DrawRule {
                id: ctx.alloc_static_id(),
                rule: StaticDrawRule::NoMaximumHandSize,
            })
        }
        v2::PlayerRestrictionKind::CastOnlyAsSorcery
        | v2::PlayerRestrictionKind::PlaysWithHandRevealed
        | v2::PlayerRestrictionKind::CantSearchLibraries
        | v2::PlayerRestrictionKind::CantDrawMoreThanOne
        | v2::PlayerRestrictionKind::CantCastMoreThanOnePerTurn
        | v2::PlayerRestrictionKind::CantUntapMoreThan(_)
        | v2::PlayerRestrictionKind::NoMaxHandSize
        | v2::PlayerRestrictionKind::CantWinGame
        | v2::PlayerRestrictionKind::CantLoseGame
        | v2::PlayerRestrictionKind::CantPlayFromHand => {
            return Err(LoweringError::Unsupported("v2:temp-player-restriction"));
        }
    };
    acc.push(Instruction::ApplyContinuous(Box::new(
        ApplyContinuousSpec::temporary_rule(rule, duration),
    )));
    Ok(())
}

fn lower_temp_zone_cast_permission(
    player: v2::PlayerRef,
    filter: Option<&v2::ObjectFilter>,
    zone: Option<&v2::ZoneRef>,
    mode: &v2::CastPermissionMode,
    duration: ResolvedDuration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let rp = lower_resolving_player(player, acc, ctx)?;
    match mode {
        v2::CastPermissionMode::AsThoughFlash { extra_cost: None } if zone.is_none() => {
            let ef = match filter {
                Some(f) => lower_object_filter(f, ctx)?,
                None => ObjectFilter::any(),
            };
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::FilteredFlash {
                        player: rp,
                        filter: ef,
                        next_only: false,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        v2::CastPermissionMode::PlayFromZone {
            top_only: false,
            lands: _,
            spells: _,
        }
        | v2::CastPermissionMode::CastFromZone => {
            let Some(zr) = zone else {
                return Err(LoweringError::Unsupported("v2:temp-zone-cast-missing-zone"));
            };
            let src_zone = lower_source_zone_filter(*zr)?;
            let ef = match filter {
                Some(f) => Some(lower_object_filter(f, ctx)?),
                None => None,
            };
            let cast_only = matches!(mode, v2::CastPermissionMode::CastFromZone)
                || !matches!(
                    mode,
                    v2::CastPermissionMode::PlayFromZone { lands: true, .. }
                );
            acc.push(Instruction::ApplyContinuous(Box::new(
                ApplyContinuousSpec::temporary_rule(
                    TemporaryRuleKind::ZoneCastPermission {
                        player: rp,
                        zone: src_zone,
                        filter: ef,
                        cast_only,
                        free: false,
                        any_color: false,
                        once: false,
                        sorcery_only: false,
                    },
                    duration,
                ),
            )));
            Ok(())
        }
        _ => Err(LoweringError::Unsupported("v2:temp-zone-cast-permission")),
    }
}

fn lower_opt_subject_to_permanent_selector(
    subject: Option<&v2::SubjectRef>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<PermanentSelector> {
    match subject {
        None => Ok(PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        }),
        Some(subj) => lower_subject_to_permanent_selector(subj, ctx),
    }
}

fn lower_cant_be_targeted_by_filter(
    flt: &v2::ObjectFilter,
) -> LowerResult<(bool, EngineColorMask)> {
    let auras_only = flt.subtypes.len() == 1 && flt.subtypes[0].eq_ignore_ascii_case("aura");
    let has_other_subtypes = !auras_only && !flt.subtypes.is_empty();
    if has_other_subtypes
        || !flt.forbidden_types.is_empty()
        || !flt.required_supertypes.is_empty()
        || !flt.forbidden_supertypes.is_empty()
        || !flt.forbidden_subtypes.is_empty()
        || !flt.forbidden_colors.is_empty()
        || flt.color_arity.is_some()
        || flt.controller.is_some()
        || flt.owner.is_some()
        || !flt.zones.is_empty()
        || flt.combat.is_some()
        || flt.tapped.is_some()
        || flt.token.is_some()
        || !flt.required_keywords.is_empty()
        || !flt.forbidden_keywords.is_empty()
        || !flt.stat_bounds.is_empty()
        || !flt.flags.is_empty()
        || !flt.predicates.is_empty()
    {
        return Err(LoweringError::Unsupported("v2:static-cant-be-targeted-by"));
    }
    let colors = lower_color_mask(flt.any_colors);
    if !auras_only && colors.is_empty() {
        return Err(LoweringError::Unsupported("v2:static-cant-be-targeted-by"));
    }
    Ok((auras_only, colors))
}

fn lower_static_zone_from_filter(flt: &v2::ObjectFilter) -> LowerResult<StaticZone> {
    match flt.zones.as_slice() {
        []
        | [
            v2::ZoneRef {
                zone: v2::ZoneKind::Battlefield,
                owner: v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::Default,
            },
        ] => Ok(StaticZone::AnyBattlefield),
        [
            v2::ZoneRef {
                zone: v2::ZoneKind::Battlefield,
                owner: v2::ZoneOwner::You,
            },
        ] => Ok(StaticZone::ControlledBattlefield),
        [
            v2::ZoneRef {
                zone: v2::ZoneKind::Graveyard,
                owner: v2::ZoneOwner::You | v2::ZoneOwner::Default,
            },
        ] => Ok(StaticZone::OwnGraveyard),
        [
            v2::ZoneRef {
                zone: v2::ZoneKind::Graveyard,
                owner: v2::ZoneOwner::All | v2::ZoneOwner::Any | v2::ZoneOwner::EachPlayer,
            },
        ] => Ok(StaticZone::AllGraveyards),
        [
            v2::ZoneRef {
                zone: v2::ZoneKind::Exile,
                ..
            },
        ] => {
            if flt.flags.contains(v2::FilterFlags::EXILED_WITH_SELF) {
                Ok(StaticZone::LinkedExile)
            } else {
                Ok(StaticZone::OwnExile)
            }
        }
        [
            v2::ZoneRef {
                zone: v2::ZoneKind::Hand,
                owner: v2::ZoneOwner::You | v2::ZoneOwner::Default,
            },
        ] => Ok(StaticZone::OwnHand),
        _ => Err(LoweringError::Unsupported("v2:static-inherit-zone")),
    }
}

fn lower_static_player_relation(player: v2::PlayerRef) -> LowerResult<PlayerRelation> {
    match player {
        v2::PlayerRef::You => Ok(PlayerRelation::Controller),
        v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => Ok(PlayerRelation::Opponent),
        v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => Ok(PlayerRelation::AnyParticipant),
        _ => Err(LoweringError::Unsupported("v2:static-player-relation")),
    }
}

fn lower_pure_generic_mana_u8(delta: &v2::ManaCost, err_code: &'static str) -> LowerResult<u8> {
    let mana = lower_v2_fixed_mana_cost(delta).map_err(|_| LoweringError::Unsupported(err_code))?;
    if !mana.required.is_empty() {
        return Err(LoweringError::Unsupported(err_code));
    }
    let val_usize = mana
        .generic
        .to_usize()
        .ok_or(LoweringError::Unsupported(err_code))?;
    let val_u8 = u8::try_from(val_usize).map_err(|_| LoweringError::Unsupported(err_code))?;
    if val_u8 == 0 {
        return Err(LoweringError::Unsupported(err_code));
    }
    Ok(val_u8)
}

fn push_attack_requirement(program: &mut Program, req: AttackRequirement) {
    if program.attack_requirement.is_none() {
        program.attack_requirement = Some(req);
    } else {
        program.additional_attack_requirements.push(req);
    }
}

fn is_bare_any_spell(f: &v2::ObjectFilter) -> bool {
    f.any_spell
        && f.all_types.is_empty()
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
        && !f.any_card
        && !f.is_ability
}

/// Lower a permanent static `"The first <filter> spell you cast each turn ..."` into a
/// conditional static spell cost modifier or spell keyword grant gated by
/// `SpellsCastMatchingThisTurn { players: One(Controller), filter } == 0`.
fn lower_permanent_next_spell_modifier(
    filter: &v2::ObjectFilter,
    effect: &v2::StaticAbility,
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if !filter.zones.is_empty() {
        return Err(LoweringError::Unsupported(
            "v2:static-next-spell-modifier-permanent",
        ));
    }
    let any_player = match filter.controller {
        None | Some(ControllerRelation::You) => false,
        _ => {
            return Err(LoweringError::Unsupported(
                "v2:static-next-spell-modifier-permanent",
            ));
        }
    };
    let mut clean_for_obj = filter.clone();
    clean_for_obj.controller = None;
    let obj_filter = lower_object_filter(&clean_for_obj, ctx)?;
    let first_spell_cond =
        StaticSourceCondition::Eval(Box::new(spellwake_engine::program::ResolutionCondition {
            left: EngineValueExpr::PlayerStat(
                spellwake_engine::program::PlayerStatExpr::SpellsCastMatchingThisTurn {
                    players: AffectedPlayers::One(ResolvingPlayer::Controller),
                    filter: Box::new(obj_filter),
                    from_zone: None,
                },
            ),
            comparison: spellwake_engine::program::Comparison::Equal,
            right: EngineValueExpr::literal(0),
        }));
    let matching = lower_spell_filter(filter, ctx)?;

    let is_plain_spell_placeholder = |f: &v2::ObjectFilter| {
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
    };

    match effect {
        v2::StaticAbility::CostModifier {
            subject,
            delta,
            less: true,
            scale,
            min_mana: None,
            condition: None,
        } if subject == filter || is_plain_spell_placeholder(subject) => {
            let generic_u8 = lower_pure_generic_mana_u8(delta, "v2:static-cost-modifier-colored")?;
            let amount = match scale {
                None => {
                    let nz = NonZeroU8::new(generic_u8)
                        .ok_or(LoweringError::Unsupported("v2:static-cost-discount-zero"))?;
                    SpellDiscount::Fixed(nz)
                }
                Some(scale_expr) => {
                    let mut ve = lower_value_expr(scale_expr, ctx)?;
                    if !ve.is_world_evaluable() {
                        return Err(LoweringError::Unsupported(
                            "v2:static-cost-discount-scale-nonworld",
                        ));
                    }
                    if generic_u8 > 1 {
                        ve = EngineValueExpr::Scale {
                            expr: Box::new(ve),
                            factor: RulesInteger::from(i64::from(generic_u8)),
                        };
                    }
                    SpellDiscount::Dynamic(Box::new(ve))
                }
            };
            push_static_ability(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ctx.alloc_static_id(),
                    modifier: SpellCostModifier::Discount {
                        any_player,
                        matching,
                        amount,
                    },
                },
                Some(first_spell_cond),
            )
        }
        v2::StaticAbility::GrantKeywords {
            subject,
            keywords,
            condition: None,
        } if !keywords.is_empty()
            && match subject {
                v2::SubjectRef::Anaphora(v2::AnaphoraRef::It | v2::AnaphoraRef::ThatSpell) => true,
                v2::SubjectRef::Group(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::All(f) => {
                    f.as_ref() == filter || is_plain_spell_placeholder(f)
                }
                _ => false,
            } =>
        {
            for kw in keywords {
                if kw.kind == v2::KeywordKind::Cascade && kw.param.is_none() {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ctx.alloc_static_id(),
                            permission: CastPermissionRule::GrantSpellCascade {
                                any_player,
                                matching,
                                count: NonZeroU8::MIN,
                            },
                        },
                        Some(first_spell_cond.clone()),
                    )?;
                } else {
                    for engine_kw in lower_keyword_abilities(kw, ctx)? {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::CastPermission {
                                id: ctx.alloc_static_id(),
                                permission: CastPermissionRule::GrantSpellKeyword {
                                    any_player,
                                    matching,
                                    keyword: engine_kw,
                                },
                            },
                            Some(first_spell_cond.clone()),
                        )?;
                    }
                }
            }
            Ok(())
        }
        _ => Err(LoweringError::Unsupported(
            "v2:static-next-spell-modifier-permanent",
        )),
    }
}

fn is_controller_only_filter(f: &v2::ObjectFilter, expected: ControllerRelation) -> bool {
    f.controller == Some(expected)
        && f.all_types.is_empty()
        && f.any_types.is_empty()
        && f.forbidden_types.is_empty()
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
