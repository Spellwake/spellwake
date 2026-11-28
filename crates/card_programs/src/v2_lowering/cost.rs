//! Activated ability, mana ability, casting cost, and activation restriction lowering for `v2_lowering`.
//!
//! Strictly typed over `v2::ir`: zero string parsing, zero regex, zero raw `oracle_text` inspection.
//! Every unhandled cost atom, mana spec, or restriction fails closed with `LoweringError::Unsupported`.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU8;

use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    activated::{
        ActivatedAbility, ActivationRestriction as EngineActivationRestriction, CostExileZone,
        CounterCostDelta, DiscardCostMode, ExtraActivationCost, GraveyardExileCost, LifeCostAmount,
        SelectedSacrificeFilter, SourceCost, TapCostCount, TapCostFilter,
    },
    characteristics::{ControllerSelector, PermanentSelector},
    mana::{
        AbilityManaClause, ChosenTypeManaScope, CounterManaYield, DynamicManaYield,
        HybridAlternative, HybridSymbol, ManaAbility, ManaCost, ManaOutput, ManaOutputModifier,
        ManaOutputRestriction, ManaRestriction, ManaSacrificeCost, ManaSideEffect,
        ManaSpecialAction, ManaSpellKeyword, ManaSpendPredicate, ManaType, SpecialColorYield,
        SpellCastMode, SpellColorRule, SpellManaClause,
    },
    program::{
        BodyTarget, CardType, ConditionalCastCost, ExecutableBody, Instruction, ModalUniqueness,
        ObjectClause, ObjectFilter, PlaneswalkerSubtype, Program, ProgramKind, SpellModes,
        ValueExpr as EngineValueExpr,
    },
    toolbox::MutateObjectSpec,
    turn::Step,
};
use spellwake_oracle_compiler::ast::{
    CardTypeMask as AstCardTypeMask, ColorArity, CompareOp, ControllerRelation, StatKind,
    SupertypeMask,
};
use spellwake_oracle_compiler::v2::ir as v2;

use crate::v2_lowering::{
    atoms::{lower_card_type_set, lower_color_set, lower_supertype_set},
    condition::lower_resolution_condition,
    ctx::LowerCtx,
    effect::lower_effect_body,
    error::{LowerResult, LoweringError},
    filter::{lower_object_filter, lower_permanent_selector},
    value::lower_value_expr,
};

/// Convert a typed `v2::ManaCost` into `(ManaCost, x_count, hybrids, snow_count)`.
pub(crate) fn lower_v2_mana_cost(
    mc: &v2::ManaCost,
) -> LowerResult<(ManaCost, u8, Vec<HybridSymbol>, u8)> {
    let mut cost = ManaCost::free();
    let mut x_count: u8 = 0;
    let mut hybrids = Vec::new();
    let mut snow_count: u8 = 0;

    for sym in &mc.symbols {
        match sym {
            v2::ManaSymbol::White => add_mana(&mut cost, ManaType::White, 1),
            v2::ManaSymbol::Blue => add_mana(&mut cost, ManaType::Blue, 1),
            v2::ManaSymbol::Black => add_mana(&mut cost, ManaType::Black, 1),
            v2::ManaSymbol::Red => add_mana(&mut cost, ManaType::Red, 1),
            v2::ManaSymbol::Green => add_mana(&mut cost, ManaType::Green, 1),
            v2::ManaSymbol::Colorless => add_mana(&mut cost, ManaType::Colourless, 1),
            v2::ManaSymbol::Generic(n) => {
                cost.generic = cost.generic.plus(&i64::from(*n).into());
            }
            v2::ManaSymbol::VariableX => {
                x_count = x_count
                    .checked_add(1)
                    .ok_or(LoweringError::Unsupported("v2:mana:x-overflow"))?;
            }
            v2::ManaSymbol::Snow => {
                snow_count = snow_count
                    .checked_add(1)
                    .ok_or(LoweringError::Unsupported("v2:mana:snow-overflow"))?;
            }
            v2::ManaSymbol::Hybrid(a, b) => {
                let (first, alt) = lower_hybrid_pair(a, b)?;
                hybrids.push(HybridSymbol {
                    first,
                    alternative: alt,
                });
                add_mana(&mut cost, first, 1);
            }
            v2::ManaSymbol::Phyrexian(inner) => {
                let first = lower_single_mana_symbol(inner)?;
                hybrids.push(HybridSymbol {
                    first,
                    alternative: HybridAlternative::TwoLife,
                });
                add_mana(&mut cost, first, 1);
            }
            v2::ManaSymbol::Tap
            | v2::ManaSymbol::Untap
            | v2::ManaSymbol::Energy
            | v2::ManaSymbol::Ticket
            | v2::ManaSymbol::Pawprint
            | v2::ManaSymbol::AnyPhyrexian => {
                return Err(LoweringError::Unsupported("v2:mana:special-symbol-in-cost"));
            }
        }
    }

    Ok((cost, x_count, hybrids, snow_count))
}

/// Convert a typed `v2::ManaCost` requiring zero X symbols, zero hybrids, and zero snow symbols.
pub(crate) fn lower_v2_fixed_mana_cost(mc: &v2::ManaCost) -> LowerResult<ManaCost> {
    let (cost, x, hybrids, snow) = lower_v2_mana_cost(mc)?;
    if x != 0 || !hybrids.is_empty() || snow != 0 {
        return Err(LoweringError::Unsupported("v2:mana:non-fixed-mana-cost"));
    }
    Ok(cost)
}

/// Convert a typed `v2::ManaCost` representing produced mana (`Add {G}{G}`) into a mana bundle.
pub(crate) fn lower_v2_mana_bundle(
    mc: &v2::ManaCost,
) -> LowerResult<BTreeMap<ManaType, RulesInteger>> {
    let mut bundle: BTreeMap<ManaType, RulesInteger> = BTreeMap::new();
    for sym in &mc.symbols {
        let (mt, qty) = match sym {
            v2::ManaSymbol::White => (ManaType::White, 1i64),
            v2::ManaSymbol::Blue => (ManaType::Blue, 1),
            v2::ManaSymbol::Black => (ManaType::Black, 1),
            v2::ManaSymbol::Red => (ManaType::Red, 1),
            v2::ManaSymbol::Green => (ManaType::Green, 1),
            v2::ManaSymbol::Colorless => (ManaType::Colourless, 1),
            v2::ManaSymbol::Generic(n) if *n > 0 => (ManaType::Colourless, i64::from(*n)),
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:mana:unsupported-output-symbol",
                ));
            }
        };
        let entry = bundle.entry(mt).or_insert_with(|| 0.into());
        *entry = entry.plus(&qty.into());
    }
    if bundle.is_empty() {
        return Err(LoweringError::Unsupported("v2:mana:empty-output-bundle"));
    }
    Ok(bundle)
}

/// Lower a `v2::Ability::Activated` into either `program.mana` (CR 605.1a pure mana ability)
/// or `program.activated` plus auxiliary restriction/cost/hybrid/mode maps.
pub(crate) fn lower_activated_ability(
    cost: &[v2::Cost],
    body: &[v2::Effect],
    restrictions: &[v2::ActivationRestriction],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    // Separate inline `Effect::Restriction` statements and inline self-ability
    // `ActivationCostModifier` clauses from executable body effects.
    let mut all_restrictions: Vec<v2::ActivationRestriction> = restrictions.to_vec();
    let mut exec_effects: Vec<v2::Effect> = Vec::with_capacity(body.len());
    let mut inline_cost_reduction: Option<EngineValueExpr> = None;
    for eff in body {
        if let v2::Effect::Restriction(rs) = eff {
            all_restrictions.extend(rs.iter().cloned());
        } else if let v2::Effect::ApplyContinuous {
            effect,
            duration: v2::Duration::Permanent | v2::Duration::Instant,
        } = eff
            && let v2::StaticAbility::ActivationCostModifier {
                delta,
                less: true,
                scale,
                condition: None,
            } = effect.as_ref()
        {
            let red = lower_inline_activation_cost_reduction(delta, scale.as_ref(), ctx)?;
            if inline_cost_reduction.replace(red).is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:activated:duplicate-cost-reduction",
                ));
            }
        } else {
            exec_effects.push(eff.clone());
        }
    }

    // 1. Detect pure mana abilities per CR 605.1a (non-targeted, non-loyalty, produces mana).
    if inline_cost_reduction.is_none()
        && let Some(mana_outputs) =
            try_lower_pure_mana_abilities(cost, &exec_effects, &all_restrictions, ctx)?
    {
        for output in mana_outputs {
            program.mana.push(ManaAbility {
                id: ctx.alloc_mana_id(),
                output,
            });
        }
        return Ok(());
    }

    // 2. Standard activated ability lowering.
    let restriction = lower_activation_restrictions(&all_restrictions, ctx)?;
    let from_graveyard = is_graveyard_activated_ability(cost, &exec_effects, program);
    let (mana_cost, source_cost, hybrids, extra_costs) =
        lower_activation_cost_parts(cost, from_graveyard, ctx)?;

    // 2a. Class level-up activation (`{cost}: Level N`, CR 716.2a).
    if let [
        v2::Effect::ApplyContinuous {
            effect,
            duration: v2::Duration::Permanent | v2::Duration::Instant,
        },
    ] = exec_effects.as_slice()
        && let v2::StaticAbility::LevelStructure(v2::LevelMarker::ClassLevel(lvl)) = effect.as_ref()
        && *lvl >= 2
    {
        let id = ctx.alloc_activated_id();
        if !hybrids.is_empty() {
            program.activation_hybrid_symbols.insert(id, hybrids);
        }
        if !extra_costs.is_empty() {
            program.extra_activation_costs.insert(id, extra_costs);
        }
        if let Some(red) = inline_cost_reduction {
            program.activation_cost_reductions.insert(id, red);
        }
        let with_sorcery = combine_restrictions(restriction, EngineActivationRestriction::Sorcery);
        let full_res = combine_restrictions(
            Some(with_sorcery),
            EngineActivationRestriction::ClassLevelExact(*lvl - 1),
        );
        program.activation_restrictions.insert(id, full_res);
        program.activated.push(ActivatedAbility {
            id,
            mana_cost,
            source_cost,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::MutateObject(Box::new(
                    MutateObjectSpec::advance_class_level(*lvl),
                ))],
            ),
        });
        return Ok(());
    }

    // Check for top-level modal activated ability (`Choose one - ...`).
    let prev_self_antecedent = ctx.self_antecedent;
    ctx.self_antecedent = cost_establishes_self_antecedent(cost, &source_cost, &extra_costs);
    if let Some((prefix_effs, choice_count, modes, suffix_effs)) =
        extract_modal_choice(&exec_effects)
    {
        let modal_res: LowerResult<()> = (|| {
            let (min_modes, max_modes) = lower_choice_count(choice_count, modes.len())?;
            let prefix = if prefix_effs.is_empty() {
                None
            } else {
                let pb = lower_effect_body(prefix_effs, ctx)?;
                if pb.target != BodyTarget::None
                    || pb
                        .instructions
                        .iter()
                        .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
                {
                    return Err(LoweringError::Unsupported(
                        "v2:activated:modal-prefix-target",
                    ));
                }
                Some(Box::new(pb))
            };
            let suffix = if suffix_effs.is_empty() {
                None
            } else {
                let sb = lower_effect_body(suffix_effs, ctx)?;
                if sb.target != BodyTarget::None
                    || sb
                        .instructions
                        .iter()
                        .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
                {
                    return Err(LoweringError::Unsupported(
                        "v2:activated:modal-suffix-target",
                    ));
                }
                Some(Box::new(sb))
            };

            let mut lowered_modes: Vec<ExecutableBody> = Vec::with_capacity(modes.len());
            for mode in modes {
                let mb = lower_effect_body(&mode.body, ctx)?;
                if mb.instructions.is_empty() {
                    return Err(LoweringError::Unsupported("v2:activated:modal-empty-mode"));
                }
                lowered_modes.push(mb);
            }
            if lowered_modes.len() < 2 {
                return Err(LoweringError::Unsupported("v2:activated:modal-too-few"));
            }

            let first = lowered_modes.remove(0);
            let id = ctx.alloc_activated_id();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if !extra_costs.is_empty() {
                program.extra_activation_costs.insert(id, extra_costs);
            }
            if let Some(red) = inline_cost_reduction {
                program.activation_cost_reductions.insert(id, red);
            }
            if let Some(res) = restriction {
                program.activation_restrictions.insert(id, res);
            }
            program.activation_modes.insert(
                id,
                SpellModes {
                    minimum: min_modes,
                    maximum: max_modes,
                    conditional_maximum: None,
                    allow_repeat: false,
                    distinct_targets: false,
                    uniqueness: ModalUniqueness::None,
                    mode_costs: Vec::new(),
                    escalate_mana: None,
                    prefix,
                    suffix,
                    additional: lowered_modes,
                },
            );
            program.activated.push(ActivatedAbility {
                id,
                mana_cost,
                source_cost,
                body: first,
            });
            Ok(())
        })();
        ctx.self_antecedent = prev_self_antecedent;
        return modal_res;
    }

    let lowered_body_res = lower_effect_body(&exec_effects, ctx);
    ctx.self_antecedent = prev_self_antecedent;
    let lowered_body = lowered_body_res?;
    let id = ctx.alloc_activated_id();
    if !hybrids.is_empty() {
        program.activation_hybrid_symbols.insert(id, hybrids);
    }
    if !extra_costs.is_empty() {
        program.extra_activation_costs.insert(id, extra_costs);
    }
    if let Some(red) = inline_cost_reduction {
        program.activation_cost_reductions.insert(id, red);
    }
    if let Some(res) = restriction {
        program.activation_restrictions.insert(id, res);
    }
    program.activated.push(ActivatedAbility {
        id,
        mana_cost,
        source_cost,
        body: lowered_body,
    });
    Ok(())
}

/// Infer graveyard activation (`CR 113.6m`) when an activated ability explicitly moves or exiles
/// `SelfObject` from `Graveyard` in its effect body, or when a Spell card (instant/sorcery, which
/// cannot exist on the battlefield) has `Cost::ExileSelf`.
fn is_graveyard_activated_ability(
    cost: &[v2::Cost],
    body: &[v2::Effect],
    program: &Program,
) -> bool {
    if matches!(
        program.kind,
        spellwake_engine::program::ProgramKind::Spell(_)
    ) && cost.iter().any(|c| matches!(c, v2::Cost::ExileSelf))
    {
        return true;
    }
    body_moves_self_from_graveyard(body)
}

fn body_moves_self_from_graveyard(effects: &[v2::Effect]) -> bool {
    effects.iter().any(|eff| match eff {
        v2::Effect::ZoneOp {
            op: v2::ZoneOpKind::Move | v2::ZoneOpKind::Exile { .. },
            subject: v2::SubjectRef::SelfObject,
            from,
            ..
        } => from.iter().any(|z| z.zone == v2::ZoneKind::Graveyard),
        v2::Effect::Optional { body, .. }
        | v2::Effect::ScopedDuration { body, .. }
        | v2::Effect::BindVariable { body, .. }
        | v2::Effect::ForEach { body, .. } => body_moves_self_from_graveyard(body),
        v2::Effect::BranchCondition {
            then_body,
            else_body,
            ..
        } => body_moves_self_from_graveyard(then_body) || body_moves_self_from_graveyard(else_body),
        _ => false,
    })
}

/// True when an activated ability's cost explicitly names its source object (e.g. "Sacrifice this
/// creature:", "Discard this card:", "Exile this artifact:", "Remove a counter from this permanent:"),
/// establishing `SelfObject` as a grammatical antecedent for `"it"` in the effect body.
fn cost_establishes_self_antecedent(
    cost: &[v2::Cost],
    source_cost: &SourceCost,
    extra_costs: &[ExtraActivationCost],
) -> bool {
    if matches!(
        source_cost,
        SourceCost::HandDiscardSelf(_)
            | SourceCost::Sacrifice
            | SourceCost::TapSacrifice
            | SourceCost::Exile { .. }
            | SourceCost::ReturnSelfToHand { .. }
            | SourceCost::Counter { .. }
            | SourceCost::Discard {
                sacrifice_source: true,
                ..
            }
    ) || extra_costs.iter().any(|c| {
        matches!(
            c,
            ExtraActivationCost::SacrificeSource
                | ExtraActivationCost::ExileSource { .. }
                | ExtraActivationCost::ExertSource
                | ExtraActivationCost::Unattach { self_source: true }
        )
    }) {
        return true;
    }
    cost.iter().any(|c| {
        matches!(
            c,
            v2::Cost::SacrificeSelf
                | v2::Cost::ExileSelf
                | v2::Cost::ExertSelf
                | v2::Cost::UnattachSelf
                | v2::Cost::ReturnToHand {
                    subject: v2::SubjectRef::SelfObject,
                }
                | v2::Cost::PutCounters {
                    subject: v2::SubjectRef::SelfObject,
                    ..
                }
                | v2::Cost::RemoveCounters {
                    subject: Some(v2::SubjectRef::SelfObject) | None,
                    ..
                }
                | v2::Cost::RemoveEitherCounter {
                    subject: Some(v2::SubjectRef::SelfObject) | None,
                    ..
                }
                | v2::Cost::PutZone {
                    subject: v2::SubjectRef::SelfObject,
                    ..
                }
        )
    })
}

fn lower_inline_activation_cost_reduction(
    delta: &v2::ManaCost,
    scale: Option<&v2::ValueExpr>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    let mut generic_total: u8 = 0;
    for sym in &delta.symbols {
        match sym {
            v2::ManaSymbol::Generic(n) => {
                generic_total = generic_total
                    .checked_add(
                        u8::try_from(*n)
                            .map_err(|_| LoweringError::Unsupported("v2:cost-overflow"))?,
                    )
                    .ok_or(LoweringError::Unsupported(
                        "v2:activated:cost-reduction-overflow",
                    ))?;
            }
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:activated:cost-reduction-non-generic",
                ));
            }
        }
    }
    if generic_total == 0 {
        return Err(LoweringError::Unsupported(
            "v2:activated:cost-reduction-zero",
        ));
    }
    match scale {
        None => Ok(EngineValueExpr::Literal(RulesInteger::from(i64::from(
            generic_total,
        )))),
        Some(scale_expr) => {
            let mut ve = lower_value_expr(scale_expr, ctx)?;
            if !ve.is_world_evaluable() {
                return Err(LoweringError::Unsupported(
                    "v2:activated:cost-reduction-nonworld",
                ));
            }
            if generic_total > 1 {
                ve = EngineValueExpr::Scale {
                    expr: Box::new(ve),
                    factor: RulesInteger::from(i64::from(generic_total)),
                };
            }
            Ok(ve)
        }
    }
}

/// Lower one `v2::Cost` atom into an engine [`ExtraActivationCost`].
pub(crate) fn lower_extra_cost_atom(
    cost: &v2::Cost,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ExtraActivationCost> {
    match cost {
        v2::Cost::TapSelf => Ok(ExtraActivationCost::TapSource),
        v2::Cost::UntapSelf => Ok(ExtraActivationCost::UntapSource),
        v2::Cost::SacrificeSelf => Ok(ExtraActivationCost::SacrificeSource),
        v2::Cost::ExileSelf => Ok(ExtraActivationCost::ExileSource {
            from_graveyard: false,
            from_hand: false,
        }),
        v2::Cost::ExertSelf => Ok(ExtraActivationCost::ExertSource),
        v2::Cost::UnattachSelf => Ok(ExtraActivationCost::Unattach { self_source: true }),
        v2::Cost::Forage => Ok(ExtraActivationCost::Forage),
        v2::Cost::Mana(mc) if is_energy_only_mana_symbols(mc) => {
            let amt = u16::try_from(mc.symbols.len())
                .map_err(|_| LoweringError::Unsupported("v2:cost:energy-overflow"))?;
            Ok(ExtraActivationCost::PayEnergy(amt))
        }
        v2::Cost::Mana(mc) => {
            let (mana, x, hybrids, snow) = lower_v2_mana_cost(mc)?;
            if x != 0 || !hybrids.is_empty() {
                return Err(LoweringError::Unsupported("v2:cost:extra-complex-mana"));
            }
            if snow > 0 && mana == ManaCost::free() {
                let nz =
                    NonZeroU8::new(snow).ok_or(LoweringError::Unsupported("v2:cost:zero-snow"))?;
                return Ok(ExtraActivationCost::PaySnowMana(nz));
            }
            if snow > 0 {
                return Err(LoweringError::Unsupported("v2:cost:mixed-snow-extra-mana"));
            }
            Ok(ExtraActivationCost::PayMana(mana))
        }
        v2::Cost::PayLife(val) => match val {
            v2::ValueExpr::Const(n) if *n > 0 => {
                let amt = u16::try_from(*n)
                    .map_err(|_| LoweringError::Unsupported("v2:cost:life-overflow"))?;
                Ok(ExtraActivationCost::PayLife(LifeCostAmount::Fixed(amt)))
            }
            v2::ValueExpr::Var(v2::VarName::X) => {
                Ok(ExtraActivationCost::PayLife(LifeCostAmount::ChosenX))
            }
            v2::ValueExpr::Div {
                expr,
                denom: 2,
                rounding: _,
            } if matches!(
                expr.as_ref(),
                v2::ValueExpr::PlayerStat {
                    player: v2::PlayerRef::You,
                    stat: spellwake_oracle_compiler::ast::PlayerStatKind::LifeTotal
                }
            ) =>
            {
                Ok(ExtraActivationCost::PayHalfLife)
            }
            _ => Err(LoweringError::Unsupported("v2:cost:dynamic-life-cost")),
        },
        v2::Cost::PayAnyAmountLife => Ok(ExtraActivationCost::PayLife(LifeCostAmount::ChosenX)),
        v2::Cost::PayEnergy(val) => match val {
            v2::ValueExpr::Const(n) if *n > 0 => {
                let amt = u16::try_from(*n)
                    .map_err(|_| LoweringError::Unsupported("v2:cost:energy-overflow"))?;
                Ok(ExtraActivationCost::PayEnergy(amt))
            }
            other => {
                let expr = lower_value_expr(other, ctx)?;
                Ok(ExtraActivationCost::PayEnergyExpr(expr))
            }
        },
        v2::Cost::Mill(val) => {
            let nz = value_to_nonzero_u8(val, "v2:cost:mill-count")?;
            Ok(ExtraActivationCost::Mill(nz))
        }
        v2::Cost::CollectEvidence(val) => {
            let n = match val {
                v2::ValueExpr::Const(v) if *v > 0 => u16::try_from(*v)
                    .map_err(|_| LoweringError::Unsupported("v2:cost:evidence-overflow"))?,
                _ => return Err(LoweringError::Unsupported("v2:cost:dynamic-evidence")),
            };
            Ok(ExtraActivationCost::CollectEvidence(n))
        }
        v2::Cost::Blight(val) => {
            let nz = value_to_nonzero_u8(val, "v2:cost:blight-count")?;
            Ok(ExtraActivationCost::Blight(nz))
        }
        v2::Cost::Waterbend(mc) => {
            let mana = lower_v2_fixed_mana_cost(mc)?;
            Ok(ExtraActivationCost::Waterbend(mana))
        }
        v2::Cost::DiscardHand => Ok(ExtraActivationCost::DiscardCards {
            filter: ObjectFilter::any(),
            count: None,
            whole_hand: true,
            random: false,
        }),
        v2::Cost::Discard {
            count,
            filter,
            random,
        } => {
            let obj_filter = match filter {
                Some(f) => lower_object_filter(f, ctx)?,
                None => ObjectFilter::any(),
            };
            let opt_count = value_to_opt_nonzero_u8(count, "v2:cost:discard-count")?;
            Ok(ExtraActivationCost::DiscardCards {
                filter: obj_filter,
                count: opt_count,
                whole_hand: false,
                random: *random,
            })
        }
        v2::Cost::Sacrifice { count, filter } => {
            let obj_filter = lower_object_filter(filter, ctx)?;
            let opt_count = value_to_opt_nonzero_u8(count, "v2:cost:sacrifice-count")?;
            Ok(ExtraActivationCost::SacrificePermanent {
                filter: obj_filter,
                count: opt_count,
            })
        }
        v2::Cost::TapUntapped { count, filter } => {
            let exclude_source = filter.flags.contains(v2::FilterFlags::EXCLUDE_SELF);
            let obj_filter = lower_object_filter(filter, ctx)?;
            let opt_count = value_to_opt_nonzero_u8(count, "v2:cost:tap-count")?;
            Ok(ExtraActivationCost::TapPermanent {
                filter: obj_filter,
                count: opt_count,
                exclude_source,
            })
        }
        v2::Cost::UntapTapped { count, filter } => {
            let exclude_source = filter.flags.contains(v2::FilterFlags::EXCLUDE_SELF);
            let obj_filter = lower_object_filter(filter, ctx)?;
            let nz = value_to_nonzero_u8(count, "v2:cost:untap-count")?;
            Ok(ExtraActivationCost::UntapPermanent {
                filter: obj_filter,
                count: nz,
                exclude_source,
            })
        }
        v2::Cost::ReturnToHand { subject } => match subject {
            v2::SubjectRef::SelfObject => Ok(ExtraActivationCost::ReturnSourceToHand),
            v2::SubjectRef::Group(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::All(f) => {
                let exclude_source = f.flags.contains(v2::FilterFlags::EXCLUDE_SELF);
                let obj_filter = lower_object_filter(f, ctx)?;
                Ok(ExtraActivationCost::BouncePermanent {
                    filter: obj_filter,
                    count: NonZeroU8::MIN,
                    exclude_source,
                })
            }
            v2::SubjectRef::Counted { filter, count } => {
                if count.up_to || count.any_number {
                    return Err(LoweringError::Unsupported("v2:cost:variable-bounce-cost"));
                }
                let exclude_source = filter.flags.contains(v2::FilterFlags::EXCLUDE_SELF);
                let obj_filter = lower_object_filter(filter, ctx)?;
                let nz = value_to_nonzero_u8(&count.count, "v2:cost:bounce-count")?;
                Ok(ExtraActivationCost::BouncePermanent {
                    filter: obj_filter,
                    count: nz,
                    exclude_source,
                })
            }
            _ => Err(LoweringError::Unsupported("v2:cost:return-subject")),
        },
        v2::Cost::Exile {
            count,
            filter,
            from,
        } => {
            let zone = lower_cost_exile_zone(from, filter)?;
            let exclude_source = filter.flags.contains(v2::FilterFlags::EXCLUDE_SELF);
            let obj_filter = lower_object_filter(filter, ctx)?;
            let opt_count = value_to_opt_nonzero_u8(count, "v2:cost:exile-count")?;
            Ok(ExtraActivationCost::ExileObjects {
                zone,
                filter: obj_filter,
                count: opt_count,
                exclude_source,
            })
        }
        v2::Cost::ExileTop { zone, count } => {
            if zone.zone != v2::ZoneKind::Library
                || !matches!(zone.owner, v2::ZoneOwner::You | v2::ZoneOwner::Default)
            {
                return Err(LoweringError::Unsupported("v2:cost:exile-top-zone"));
            }
            let opt_count = value_to_opt_nonzero_u8(count, "v2:cost:exile-top-count")?;
            Ok(ExtraActivationCost::ExileTopLibrary {
                count: opt_count,
                face_down: false,
            })
        }
        v2::Cost::RevealFromHand { count, filter } => {
            let obj_filter = lower_object_filter(filter, ctx)?;
            let nz = value_to_nonzero_u8(count, "v2:cost:reveal-count")?;
            Ok(ExtraActivationCost::RevealHand {
                filter: obj_filter,
                count: nz,
                allow_controlled: false,
            })
        }
        v2::Cost::PutCounters {
            kind,
            count,
            subject,
        } => {
            let ck = super::atoms::lower_counter_kind(kind)?;
            if !ck.is_permanent() {
                return Err(LoweringError::Unsupported("v2:cost:non-permanent-counter"));
            }
            let nz = value_to_nonzero_u8(count, "v2:cost:put-counter-count")?;
            match subject {
                v2::SubjectRef::SelfObject => Ok(ExtraActivationCost::SourceCounter {
                    kind: ck,
                    delta: CounterCostDelta::Add(nz),
                }),
                v2::SubjectRef::Group(f) | v2::SubjectRef::Each(f) => {
                    let obj_filter = lower_object_filter(f, ctx)?;
                    Ok(ExtraActivationCost::PutPermanentCounters {
                        kind: ck,
                        count: nz,
                        filter: obj_filter,
                    })
                }
                _ => Err(LoweringError::Unsupported("v2:cost:put-counter-subject")),
            }
        }
        v2::Cost::RemoveCounters {
            kind,
            count,
            subject,
        } => {
            let ck_opt = match kind {
                v2::CounterKind::Any => None,
                other => {
                    let ck = super::atoms::lower_counter_kind(other)?;
                    if !ck.is_permanent() {
                        return Err(LoweringError::Unsupported("v2:cost:non-permanent-counter"));
                    }
                    Some(ck)
                }
            };
            let opt_count = value_to_opt_nonzero_u8(count, "v2:cost:remove-counter-count")?;
            match subject {
                None | Some(v2::SubjectRef::SelfObject) => {
                    let ck = ck_opt.ok_or(LoweringError::Unsupported(
                        "v2:cost:remove-any-source-counter",
                    ))?;
                    let delta = match opt_count {
                        Some(nz) => CounterCostDelta::Remove(nz),
                        None => CounterCostDelta::RemoveX,
                    };
                    Ok(ExtraActivationCost::SourceCounter { kind: ck, delta })
                }
                Some(
                    v2::SubjectRef::Group(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::All(f),
                ) => {
                    let obj_filter = lower_object_filter(f, ctx)?;
                    Ok(ExtraActivationCost::RemovePermanentCounters {
                        kind: ck_opt,
                        count: opt_count,
                        all: false,
                        filter: obj_filter,
                    })
                }
                Some(_) => Err(LoweringError::Unsupported("v2:cost:remove-counter-subject")),
            }
        }
        v2::Cost::Or(a, b) => {
            let left = lower_extra_cost_atom(a, ctx)?;
            let right = lower_extra_cost_atom(b, ctx)?;
            Ok(ExtraActivationCost::Either(Box::new(left), Box::new(right)))
        }
        v2::Cost::RemoveEitherCounter {
            kind1,
            kind2,
            subject,
        } => {
            let left = lower_extra_cost_atom(
                &v2::Cost::RemoveCounters {
                    kind: kind1.clone(),
                    count: v2::ValueExpr::Const(1),
                    subject: subject.clone(),
                },
                ctx,
            )?;
            let right = lower_extra_cost_atom(
                &v2::Cost::RemoveCounters {
                    kind: kind2.clone(),
                    count: v2::ValueExpr::Const(1),
                    subject: subject.clone(),
                },
                ctx,
            )?;
            Ok(ExtraActivationCost::Either(Box::new(left), Box::new(right)))
        }
        v2::Cost::ScaledMana { base, scale } => {
            let Some(amt) = extract_energy_cost_amount_scaled(base, scale, ctx)? else {
                return Err(LoweringError::Unsupported("v2:cost:unsupported-atom"));
            };
            match amt {
                EnergyCostAmt::Fixed(n) => Ok(ExtraActivationCost::PayEnergy(n)),
                EnergyCostAmt::Dynamic(expr) => Ok(ExtraActivationCost::PayEnergyExpr(expr)),
            }
        }
        v2::Cost::PayAnyAmountMana
        | v2::Cost::PutZone { .. }
        | v2::Cost::ChooseObject(_)
        | v2::Cost::Behold(_) => Err(LoweringError::Unsupported("v2:cost:unsupported-atom")),
    }
}

pub(crate) enum EnergyCostAmt {
    Fixed(u16),
    Dynamic(EngineValueExpr),
}

pub(crate) fn is_energy_only_mana_symbols(mc: &v2::ManaCost) -> bool {
    !mc.symbols.is_empty()
        && mc
            .symbols
            .iter()
            .all(|sym| matches!(sym, v2::ManaSymbol::Energy))
}

pub(crate) fn extract_energy_cost_amount_scaled(
    base: &v2::ManaCost,
    scale: &v2::ValueExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<EnergyCostAmt>> {
    if !is_energy_only_mana_symbols(base) {
        return Ok(None);
    }
    let unit = i64::try_from(base.symbols.len())
        .map_err(|_| LoweringError::Unsupported("v2:cost:energy-overflow"))?;
    if let v2::ValueExpr::Const(n) = scale {
        if *n <= 0 {
            return Err(LoweringError::Unsupported("v2:cost:energy-overflow"));
        }
        let total = i64::from(*n)
            .checked_mul(unit)
            .and_then(|v| u16::try_from(v).ok())
            .ok_or(LoweringError::Unsupported("v2:cost:energy-overflow"))?;
        return Ok(Some(EnergyCostAmt::Fixed(total)));
    }
    let mut expr = lower_value_expr(scale, ctx)?;
    if unit > 1 {
        expr = EngineValueExpr::Scale {
            expr: Box::new(expr),
            factor: RulesInteger::from(unit),
        };
    }
    Ok(Some(EnergyCostAmt::Dynamic(expr)))
}

/// Lower mandatory additional casting costs (`As an additional cost to cast this spell, ...`)
/// into `program`.
pub(crate) fn lower_additional_cast_costs(
    costs: &[v2::Cost],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if costs.is_empty() {
        return Err(LoweringError::Unsupported("v2:cost:empty-additional-cast"));
    }
    for atom in costs {
        if let v2::Cost::PayLife(val) = atom
            && program.additional_life_cost.is_none()
        {
            match val {
                v2::ValueExpr::Const(n) if *n > 0 => {
                    program.additional_life_cost = Some(EngineValueExpr::literal(i64::from(*n)));
                    continue;
                }
                v2::ValueExpr::Var(v2::VarName::X) => {
                    program.additional_life_cost = Some(EngineValueExpr::EventValue(
                        spellwake_engine::program::EventValueKind::ChosenX,
                    ));
                    continue;
                }
                _ => {}
            }
        }
        if let v2::Cost::Sacrifice {
            count: v2::ValueExpr::Const(1),
            filter,
        } = atom
            && matches!(program.kind, ProgramKind::Spell(_))
            && !program.sacrifice_creature_cast_cost
            && !program.tap_creatures_cast_cost
            && !program.reveal_colorless_creature_cast_cost
        {
            let obj_filter = lower_object_filter(filter, ctx)?;
            program.sacrifice_creature_cast_cost = true;
            program.sacrifice_cast_cost_filter = if obj_filter == ObjectFilter::creature() {
                None
            } else {
                Some(obj_filter)
            };
            continue;
        }
        let extra = lower_extra_cost_atom(atom, ctx)?;
        if is_battlefield_only_source_cost(&extra) {
            return Err(LoweringError::Unsupported(
                "v2:cost:battlefield-source-cost-on-cast",
            ));
        }
        program.extra_cast_costs.push(extra);
    }
    Ok(())
}

fn lower_alternative_cast_cost_parts(
    costs: &[v2::Cost],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(ManaCost, Vec<ExtraActivationCost>)> {
    let mut mana = ManaCost::free();
    let mut extra = Vec::new();
    for atom in costs {
        if let v2::Cost::Mana(mc) = atom {
            let (part, x, hybrids, snow) = lower_v2_mana_cost(mc)?;
            if x != 0 || !hybrids.is_empty() {
                return Err(LoweringError::Unsupported("v2:cost:alt-cast-complex-mana"));
            }
            merge_mana_cost(&mut mana, &part);
            if let Some(nz) = NonZeroU8::new(snow) {
                extra.push(ExtraActivationCost::PaySnowMana(nz));
            }
        } else {
            let lowered = lower_extra_cost_atom(atom, ctx)?;
            if is_battlefield_only_source_cost(&lowered) {
                return Err(LoweringError::Unsupported(
                    "v2:cost:battlefield-source-cost-on-alt-cast",
                ));
            }
            extra.push(lowered);
        }
    }
    Ok((mana, extra))
}

/// Lower unconditional alternative casting costs (`You may <costs> rather than pay ...`)
/// into `program.conditional_cost`.
pub(crate) fn lower_alternative_cast_costs(
    costs: &[v2::Cost],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let (mana, extra) = lower_alternative_cast_cost_parts(costs, ctx)?;
    if program
        .conditional_cost
        .replace(ConditionalCastCost::Alternative {
            condition: None,
            mana,
            extra,
        })
        .is_some()
    {
        return Err(LoweringError::Unsupported(
            "v2:cost:duplicate-conditional-cost",
        ));
    }
    Ok(())
}

/// Lower conditional alternative casting costs (`If <cond>, you may <costs> rather than pay ...`)
/// into `program.conditional_cost`.
pub(crate) fn lower_conditional_alternative_cast_costs(
    cond: &v2::ConditionExpr,
    costs: &[v2::Cost],
    program: &mut Program,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let res_cond = lower_resolution_condition(cond, ctx)?;
    if !res_cond.is_world_evaluable() {
        return Err(LoweringError::Unsupported("v2:cost:alt-cast-nonworld-cond"));
    }
    let (mana, extra) = lower_alternative_cast_cost_parts(costs, ctx)?;
    if program
        .conditional_cost
        .replace(ConditionalCastCost::Alternative {
            condition: Some(Box::new(res_cond)),
            mana,
            extra,
        })
        .is_some()
    {
        return Err(LoweringError::Unsupported(
            "v2:cost:duplicate-conditional-cost",
        ));
    }
    Ok(())
}

/// Lower typed `v2::ActivationRestriction` entries into an optional engine
/// [`EngineActivationRestriction`].
pub(crate) fn lower_activation_restrictions(
    restrictions: &[v2::ActivationRestriction],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<EngineActivationRestriction>> {
    if restrictions.is_empty() {
        return Ok(None);
    }
    let mut combined: Option<EngineActivationRestriction> = None;
    for r in restrictions {
        let lowered = match r {
            v2::ActivationRestriction::AsSorcery => EngineActivationRestriction::Sorcery,
            v2::ActivationRestriction::DuringYourTurn => {
                EngineActivationRestriction::DuringYourTurn
            }
            v2::ActivationRestriction::DuringYourUpkeep => {
                EngineActivationRestriction::DuringOwnStep(Step::Upkeep)
            }
            v2::ActivationRestriction::DuringCombat => EngineActivationRestriction::DuringCombat,
            v2::ActivationRestriction::DuringEndOfCombat => {
                EngineActivationRestriction::DuringStep(Step::EndCombat)
            }
            v2::ActivationRestriction::BeforeAttackers => {
                EngineActivationRestriction::OwnTurnBeforeAttackers
            }
            v2::ActivationRestriction::OnceEachTurn => EngineActivationRestriction::OnceEachTurn,
            v2::ActivationRestriction::Once => EngineActivationRestriction::Exhaust,
            v2::ActivationRestriction::DuringCombatStep(step) => match step {
                v2::CombatStepKind::DeclareAttackers => {
                    EngineActivationRestriction::DuringStep(Step::DeclareAttackers)
                }
                v2::CombatStepKind::DeclareBlockers => {
                    EngineActivationRestriction::DuringStep(Step::DeclareBlockers)
                }
                v2::CombatStepKind::CombatDamage => {
                    EngineActivationRestriction::DuringStep(Step::CombatDamage)
                }
            },
            v2::ActivationRestriction::AllowedPlayer(p) => match p {
                v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
                    EngineActivationRestriction::AnyPlayer
                }
                v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                    EngineActivationRestriction::OnlyOpponents
                }
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:restriction:unsupported-allowed-player",
                    ));
                }
            },
            v2::ActivationRestriction::OnlyIf(cond) => lower_condition_restriction(cond, ctx)?,
            v2::ActivationRestriction::AsInstant => {
                return Err(LoweringError::Unsupported("v2:restriction:as-instant"));
            }
            v2::ActivationRestriction::ControlledContinuously => {
                return Err(LoweringError::Unsupported(
                    "v2:restriction:controlled-continuously",
                ));
            }
        };
        combined = Some(combine_restrictions(combined, lowered));
    }
    Ok(combined)
}

/// Combine an optional existing activation restriction with an additional restriction.
pub(crate) fn combine_restrictions(
    existing: Option<EngineActivationRestriction>,
    added: EngineActivationRestriction,
) -> EngineActivationRestriction {
    match existing {
        None => added,
        Some(EngineActivationRestriction::All(mut items)) => {
            items.push(added);
            EngineActivationRestriction::All(items)
        }
        Some(prev) => EngineActivationRestriction::All(vec![prev, added]),
    }
}

/// Lower a list of `v2::Cost` atoms into `(ManaCost, SourceCost, Vec<HybridSymbol>, Vec<ExtraActivationCost>)`.
pub(crate) fn lower_activation_cost_parts(
    cost: &[v2::Cost],
    from_graveyard: bool,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(
    ManaCost,
    SourceCost,
    Vec<HybridSymbol>,
    Vec<ExtraActivationCost>,
)> {
    let mut mana = ManaCost::free();
    let mut x_symbols: u8 = 0;
    let mut hybrids: Vec<HybridSymbol> = Vec::new();
    let mut snow_mana: u8 = 0;
    let mut tap_self = false;
    let mut untap_self = false;
    let mut sac_self = false;
    let mut exile_self = false;
    let mut non_mana: Vec<&v2::Cost> = Vec::new();

    for atom in cost {
        match atom {
            v2::Cost::Mana(mc) if !is_energy_only_mana_symbols(mc) => {
                let (part, x, hybs, snow) = lower_v2_mana_cost(mc)?;
                merge_mana_cost(&mut mana, &part);
                x_symbols = x_symbols
                    .checked_add(x)
                    .ok_or(LoweringError::Unsupported("v2:cost:x-overflow"))?;
                hybrids.extend(hybs);
                snow_mana = snow_mana
                    .checked_add(snow)
                    .ok_or(LoweringError::Unsupported("v2:cost:snow-overflow"))?;
            }
            v2::Cost::TapSelf if !tap_self => {
                tap_self = true;
            }
            v2::Cost::UntapSelf if !untap_self => {
                untap_self = true;
            }
            v2::Cost::SacrificeSelf if !sac_self => {
                sac_self = true;
            }
            v2::Cost::ExileSelf if !exile_self => {
                exile_self = true;
            }
            other => {
                non_mana.push(other);
            }
        }
    }

    let mut extra_costs: Vec<ExtraActivationCost> = Vec::new();
    if let Some(nz_snow) = NonZeroU8::new(snow_mana) {
        extra_costs.push(ExtraActivationCost::PaySnowMana(nz_snow));
    }

    // 1. Variable X mana cost uses `SourceCost::GenericX`.
    if x_symbols > 0 {
        if untap_self {
            extra_costs.push(ExtraActivationCost::UntapSource);
        }
        if sac_self {
            extra_costs.push(ExtraActivationCost::SacrificeSource);
        }
        if exile_self {
            extra_costs.push(ExtraActivationCost::ExileSource {
                from_graveyard,
                from_hand: false,
            });
        }
        for atom in non_mana {
            extra_costs.push(lower_extra_cost_atom(atom, ctx)?);
        }
        let sc = if from_graveyard && !tap_self && !exile_self {
            return Err(LoweringError::Unsupported("v2:cost:graveyard-x-mana"));
        } else {
            SourceCost::GenericX {
                symbols: x_symbols,
                tap_source: tap_self,
            }
        };
        return Ok((mana, sc, hybrids, extra_costs));
    }

    // 2. Graveyard activation path when `from_graveyard` is set.
    if from_graveyard {
        if exile_self && !tap_self && !sac_self && !untap_self {
            for atom in non_mana {
                extra_costs.push(lower_extra_cost_atom(atom, ctx)?);
            }
            return Ok((
                mana,
                SourceCost::Exile {
                    tap_source: false,
                    from_graveyard: true,
                },
                hybrids,
                extra_costs,
            ));
        }
        if !tap_self && !sac_self && !untap_self && !exile_self {
            for atom in non_mana {
                extra_costs.push(lower_extra_cost_atom(atom, ctx)?);
            }
            return Ok((
                mana,
                SourceCost::GraveyardSelf { unearth: false },
                hybrids,
                extra_costs,
            ));
        }
    }

    // 3. Simple single-source-cost fast paths for clean engine representation.
    if non_mana.is_empty() {
        let sc = match (tap_self, untap_self, sac_self, exile_self) {
            (false, false, false, false) => SourceCost::None,
            (true, false, false, false) => SourceCost::Tap,
            (false, true, false, false) => SourceCost::Untap,
            (false, false, true, false) => SourceCost::Sacrifice,
            (true, false, true, false) => SourceCost::TapSacrifice,
            (tap, false, false, true) => SourceCost::Exile {
                tap_source: tap,
                from_graveyard: false,
            },
            _ => {
                if tap_self {
                    extra_costs.push(ExtraActivationCost::TapSource);
                }
                if untap_self {
                    extra_costs.push(ExtraActivationCost::UntapSource);
                }
                if sac_self {
                    extra_costs.push(ExtraActivationCost::SacrificeSource);
                }
                if exile_self {
                    extra_costs.push(ExtraActivationCost::ExileSource {
                        from_graveyard: false,
                        from_hand: false,
                    });
                }
                SourceCost::None
            }
        };
        return Ok((mana, sc, hybrids, extra_costs));
    }

    // 4. Single non-mana atom + optional tap/sac maps cleanly to primary `SourceCost` variants.
    if non_mana.len() == 1
        && !untap_self
        && !exile_self
        && let Some(sc) = try_lower_primary_source_cost(non_mana[0], tap_self, sac_self, ctx)?
    {
        return Ok((mana, sc, hybrids, extra_costs));
    }

    // 5. General composable multi-cost fallback using `SourceCost` + `extra_activation_costs`.
    let primary = match (tap_self, sac_self, untap_self, exile_self) {
        (true, true, false, false) => SourceCost::TapSacrifice,
        (true, false, false, false) => SourceCost::Tap,
        (false, true, false, false) => SourceCost::Sacrifice,
        (false, false, true, false) => SourceCost::Untap,
        (tap, false, false, true) => SourceCost::Exile {
            tap_source: tap,
            from_graveyard: false,
        },
        _ => {
            if tap_self {
                extra_costs.push(ExtraActivationCost::TapSource);
            }
            if sac_self {
                extra_costs.push(ExtraActivationCost::SacrificeSource);
            }
            if untap_self {
                extra_costs.push(ExtraActivationCost::UntapSource);
            }
            if exile_self {
                extra_costs.push(ExtraActivationCost::ExileSource {
                    from_graveyard: false,
                    from_hand: false,
                });
            }
            SourceCost::None
        }
    };
    for atom in non_mana {
        extra_costs.push(lower_extra_cost_atom(atom, ctx)?);
    }

    Ok((mana, primary, hybrids, extra_costs))
}

fn try_lower_primary_source_cost(
    atom: &v2::Cost,
    tap_self: bool,
    sac_self: bool,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<SourceCost>> {
    match atom {
        v2::Cost::PayLife(v2::ValueExpr::Const(n)) if !sac_self && *n > 0 => {
            let amt = u16::try_from(*n)
                .map_err(|_| LoweringError::Unsupported("v2:cost:life-overflow"))?;
            Ok(Some(SourceCost::PayLife {
                amount: LifeCostAmount::Fixed(amt),
                tap_source: tap_self,
                sacrifice: None,
            }))
        }
        v2::Cost::PayLife(v2::ValueExpr::Var(v2::VarName::X)) | v2::Cost::PayAnyAmountLife
            if !sac_self =>
        {
            Ok(Some(SourceCost::PayLife {
                amount: LifeCostAmount::ChosenX,
                tap_source: tap_self,
                sacrifice: None,
            }))
        }
        v2::Cost::PayEnergy(v2::ValueExpr::Const(n)) if !sac_self && *n > 0 => {
            let amt = u16::try_from(*n)
                .map_err(|_| LoweringError::Unsupported("v2:cost:energy-overflow"))?;
            Ok(Some(SourceCost::PayEnergy {
                amount: amt,
                tap_source: tap_self,
            }))
        }
        v2::Cost::Mana(mc) if !sac_self && is_energy_only_mana_symbols(mc) => {
            let amt = u16::try_from(mc.symbols.len())
                .map_err(|_| LoweringError::Unsupported("v2:cost:energy-overflow"))?;
            Ok(Some(SourceCost::PayEnergy {
                amount: amt,
                tap_source: tap_self,
            }))
        }
        v2::Cost::ScaledMana { base, scale } if !sac_self && is_energy_only_mana_symbols(base) => {
            if let Some(EnergyCostAmt::Fixed(amt)) =
                extract_energy_cost_amount_scaled(base, scale, ctx)?
            {
                Ok(Some(SourceCost::PayEnergy {
                    amount: amt,
                    tap_source: tap_self,
                }))
            } else {
                Ok(None)
            }
        }
        v2::Cost::Mill(val) if !sac_self => {
            let nz = value_to_nonzero_u8(val, "v2:cost:mill-count")?;
            Ok(Some(SourceCost::Mill {
                count: nz,
                tap_source: tap_self,
            }))
        }
        v2::Cost::ReturnToHand {
            subject: v2::SubjectRef::SelfObject,
        } if !sac_self => Ok(Some(SourceCost::ReturnSelfToHand {
            tap_source: tap_self,
        })),
        v2::Cost::DiscardHand => Ok(Some(SourceCost::Discard {
            mode: DiscardCostMode::WholeHand,
            tap_source: tap_self,
            sacrifice_source: sac_self,
        })),
        v2::Cost::Discard {
            count: v2::ValueExpr::Const(1),
            filter: None,
            random: false,
        } => Ok(Some(SourceCost::Discard {
            mode: DiscardCostMode::SelectedOne,
            tap_source: tap_self,
            sacrifice_source: sac_self,
        })),
        v2::Cost::Discard {
            count: v2::ValueExpr::Const(n),
            filter: Some(f),
            random: false,
        } if *n > 0 && *n <= i32::from(u8::MAX) => {
            let nz = NonZeroU8::new(*n as u8)
                .ok_or(LoweringError::Unsupported("v2:cost:discard-count"))?;
            let obj_filter = lower_object_filter(f, ctx)?;
            Ok(Some(SourceCost::Discard {
                mode: DiscardCostMode::Filtered {
                    filter: obj_filter,
                    count: nz,
                },
                tap_source: tap_self,
                sacrifice_source: sac_self,
            }))
        }
        v2::Cost::Sacrifice {
            count: v2::ValueExpr::Const(n),
            filter,
        } if !sac_self && *n > 0 && *n <= i32::from(u8::MAX) => {
            let obj_filter = lower_object_filter(filter, ctx)?;
            Ok(Some(SourceCost::SacrificeSelected {
                filter: SelectedSacrificeFilter::Matching(obj_filter),
                count: *n as u8,
                tap_source: tap_self,
                from_graveyard: false,
            }))
        }
        v2::Cost::TapUntapped { count, filter } if !sac_self => {
            let exclude_source = filter.flags.contains(v2::FilterFlags::EXCLUDE_SELF);
            let obj_filter = lower_object_filter(filter, ctx)?;
            let tap_count = match count {
                v2::ValueExpr::Const(n) if *n > 0 && *n <= i32::from(u8::MAX) => {
                    let nz = NonZeroU8::new(*n as u8)
                        .ok_or(LoweringError::Unsupported("v2:cost:tap-count"))?;
                    TapCostCount::Fixed(nz)
                }
                v2::ValueExpr::Var(v2::VarName::X) => TapCostCount::ChosenX,
                _ => return Ok(None),
            };
            Ok(Some(SourceCost::TapPermanents {
                filter: TapCostFilter::Matching(obj_filter),
                count: tap_count,
                tap_source: tap_self,
                exclude_source,
            }))
        }
        v2::Cost::Exile {
            count,
            filter,
            from,
        } if !sac_self
            && lower_cost_exile_zone(from, filter).ok() == Some(CostExileZone::Graveyard) =>
        {
            let obj_filter = lower_object_filter(filter, ctx)?;
            let gy_cost = match count {
                v2::ValueExpr::Const(n) if *n > 0 && *n <= i32::from(u8::MAX) => {
                    let nz = NonZeroU8::new(*n as u8)
                        .ok_or(LoweringError::Unsupported("v2:cost:exile-gy-count"))?;
                    GraveyardExileCost::Filtered {
                        filter: obj_filter,
                        count: nz,
                    }
                }
                v2::ValueExpr::Var(v2::VarName::X) if obj_filter == ObjectFilter::any() => {
                    GraveyardExileCost::AnyCountX
                }
                _ => return Ok(None),
            };
            Ok(Some(SourceCost::ExileGraveyard {
                filter: gy_cost,
                tap_source: tap_self,
            }))
        }
        v2::Cost::RemoveCounters {
            kind,
            count,
            subject: None | Some(v2::SubjectRef::SelfObject),
        } => {
            let Ok(ck) = super::atoms::lower_counter_kind(kind) else {
                return Ok(None);
            };
            if !ck.is_permanent() {
                return Ok(None);
            }
            let delta = match count {
                v2::ValueExpr::Const(n) if *n > 0 && *n <= i32::from(u8::MAX) => {
                    let nz = NonZeroU8::new(*n as u8)
                        .ok_or(LoweringError::Unsupported("v2:cost:counter-count"))?;
                    CounterCostDelta::Remove(nz)
                }
                v2::ValueExpr::Var(v2::VarName::X) => CounterCostDelta::RemoveX,
                _ => return Ok(None),
            };
            Ok(Some(SourceCost::Counter {
                kind: ck,
                delta,
                tap_source: tap_self,
                sacrifice_source: sac_self,
            }))
        }
        v2::Cost::PutCounters {
            kind,
            count: v2::ValueExpr::Const(n),
            subject: v2::SubjectRef::SelfObject,
        } if *n > 0 && *n <= i32::from(u8::MAX) => {
            let Ok(ck) = super::atoms::lower_counter_kind(kind) else {
                return Ok(None);
            };
            if !ck.is_permanent() {
                return Ok(None);
            }
            let nz = NonZeroU8::new(*n as u8)
                .ok_or(LoweringError::Unsupported("v2:cost:counter-count"))?;
            Ok(Some(SourceCost::Counter {
                kind: ck,
                delta: CounterCostDelta::Add(nz),
                tap_source: tap_self,
                sacrifice_source: sac_self,
            }))
        }
        _ => Ok(None),
    }
}

/// Detect and lower pure CR 605.1a mana abilities into one or more [`ManaOutput`] entries.
/// Returns `Ok(None)` when the ability is not a pure mana ability (so the caller falls back to
/// `ActivatedAbility`).
fn try_lower_pure_mana_abilities(
    cost: &[v2::Cost],
    body: &[v2::Effect],
    restrictions: &[v2::ActivationRestriction],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<Vec<ManaOutput>>> {
    if body.is_empty() {
        return Ok(None);
    }

    // Extract AddMana + optional spend rule + optional life-gain/damage-self side effect.
    let mut mana_spec: Option<&v2::ManaSpec> = None;
    let mut mana_restriction: Option<ManaOutputRestriction> = None;
    let mut side_effect: Option<ManaSideEffect> = None;

    for eff in body {
        match eff {
            v2::Effect::MutatePlayer {
                player: v2::PlayerRef::You,
                mutation: v2::PlayerMutation::AddMana(spec),
                duration: v2::Duration::Instant,
            } => {
                if mana_spec.is_some() {
                    return Ok(None);
                }
                mana_spec = Some(spec);
            }
            v2::Effect::MutatePlayer {
                player: v2::PlayerRef::You,
                mutation: v2::PlayerMutation::ManaRule(rule),
                duration: _,
            } => {
                if mana_restriction.is_some() {
                    return Ok(None);
                }
                let Some(parsed) = try_lower_mana_spend_rule(rule, ctx) else {
                    return Ok(None);
                };
                mana_restriction = Some(parsed);
            }
            v2::Effect::AdjustLife {
                player: v2::PlayerRef::You,
                amount: v2::ValueExpr::Const(n),
                op: v2::LifeOp::Gain,
            } if *n > 0 => {
                if side_effect.is_some() {
                    return Ok(None);
                }
                side_effect = Some(ManaSideEffect::GainLife(i64::from(*n).into()));
            }
            v2::Effect::DealDamage {
                source: v2::SubjectRef::SelfObject,
                target: v2::SubjectRef::Player(v2::PlayerRef::You),
                amount: v2::ValueExpr::Const(n),
                divided_by: None,
            } if *n > 0 => {
                if side_effect.is_some() {
                    return Ok(None);
                }
                side_effect = Some(ManaSideEffect::DamageController(i64::from(*n).into()));
            }
            _ => return Ok(None),
        }
    }

    let Some(spec) = mana_spec else {
        return Ok(None);
    };

    // Analyze cost structure for ManaAbility compatibility.
    let mut tap_self = false;
    let mut paid_mana: Option<ManaCost> = None;
    let mut sac_cost: Option<ManaSacrificeCost> = None;
    let mut tap_controlled_artifact = false;
    let mut remove_one_charge = false;

    for atom in cost {
        match atom {
            v2::Cost::TapSelf if !tap_self => {
                tap_self = true;
            }
            v2::Cost::Mana(mc) => {
                let Ok(fixed) = lower_v2_fixed_mana_cost(mc) else {
                    return Ok(None);
                };
                if fixed.total().is_positive() {
                    if let Some(prev) = paid_mana.as_mut() {
                        merge_mana_cost(prev, &fixed);
                    } else {
                        paid_mana = Some(fixed);
                    }
                }
            }
            v2::Cost::SacrificeSelf if sac_cost.is_none() => {
                sac_cost = Some(ManaSacrificeCost::Source { tap: false });
            }
            v2::Cost::Sacrifice {
                count: v2::ValueExpr::Const(1),
                filter,
            } if sac_cost.is_none() => {
                if is_plain_creature_filter(filter) {
                    sac_cost = Some(ManaSacrificeCost::ControlledCreature);
                } else if filter.subtypes.len() == 1
                    && !filter.subtypes_match_any
                    && filter.all_types.is_empty()
                    && filter.any_types.is_empty()
                    && filter.forbidden_types.is_empty()
                    && let Some(ct) = ctx.lookup_creature_type(&filter.subtypes[0])
                {
                    sac_cost = Some(ManaSacrificeCost::ControlledSubtype(ct));
                } else {
                    return Ok(None);
                }
            }
            v2::Cost::TapUntapped {
                count: v2::ValueExpr::Const(1),
                filter,
            } if !tap_controlled_artifact && is_plain_controlled_artifact_filter(filter) => {
                tap_controlled_artifact = true;
            }
            v2::Cost::RemoveCounters {
                kind: v2::CounterKind::Charge,
                count: v2::ValueExpr::Const(1),
                subject: None | Some(v2::SubjectRef::SelfObject),
            } if !remove_one_charge => {
                remove_one_charge = true;
            }
            _ => return Ok(None),
        }
    }

    // Normalize `{T}, Sacrifice ~` into `ManaSacrificeCost::Source { tap: true }`.
    if let Some(ManaSacrificeCost::Source { tap }) = sac_cost.as_mut()
        && tap_self
    {
        *tap = true;
    }

    // Must have a recognized mana-ability activation cost (tap, sacrifice, tap-artifact, or charge-remove).
    if !tap_self && sac_cost.is_none() && !tap_controlled_artifact && !remove_one_charge {
        return Ok(None);
    }
    if remove_one_charge {
        if tap_self || sac_cost.is_some() || tap_controlled_artifact || paid_mana.is_some() {
            return Ok(None);
        }
        if !matches!(
            spec,
            v2::ManaSpec::AnyColor {
                amount: v2::ValueExpr::Const(1),
                ..
            }
        ) {
            return Ok(None);
        }
    }

    // Activation restrictions on mana abilities cannot include `AnyPlayer`.
    let act_res = lower_activation_restrictions(restrictions, ctx)?;
    if act_res
        .as_ref()
        .is_some_and(EngineActivationRestriction::is_any_player)
    {
        return Ok(None);
    }

    // Lower the base `ManaOutput`(s) from `v2::ManaSpec`.
    let base_outputs = if remove_one_charge {
        vec![ManaOutput::CounterBased(
            CounterManaYield::RemoveChargeForColor,
        )]
    } else {
        let Some(outs) = try_lower_mana_spec_outputs(spec, ctx)? else {
            return Ok(None);
        };
        outs
    };

    // Apply orthogonal modifiers in valid engine order
    // (tap_artifact -> sacrifice -> restrict -> paid -> side_effect -> activation_condition).
    let mut final_outputs = Vec::with_capacity(base_outputs.len());
    for mut out in base_outputs {
        if tap_controlled_artifact {
            if !matches!(out, ManaOutput::Fixed(_)) {
                return Ok(None);
            }
            out = out.tap_controlled_artifact();
        }
        if let Some(sac) = sac_cost {
            // Engine `ManaOutput::valid` requires sacrifice inner to be Fixed or ChooseColor.
            if !matches!(out, ManaOutput::Fixed(_) | ManaOutput::ChooseColor { .. }) {
                return Ok(None);
            }
            out = out.sacrifice(sac);
        }
        if let Some(rule) = mana_restriction.clone() {
            out = out.with_modifier(ManaOutputModifier::Restrict(rule));
        }
        if let Some(paid) = paid_mana.clone() {
            out = out.paid(paid);
        }
        if let Some(se) = side_effect.clone() {
            out = out.side_effect(se);
        }
        if let Some(res) = act_res.clone() {
            out = out.with_activation_condition(res);
        }
        final_outputs.push(out);
    }

    Ok(Some(final_outputs))
}

fn try_lower_mana_spec_outputs(
    spec: &v2::ManaSpec,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<Vec<ManaOutput>>> {
    match spec {
        v2::ManaSpec::Exact(mc) => {
            let Ok(bundle) = lower_v2_mana_bundle(mc) else {
                return Ok(None);
            };
            Ok(Some(vec![ManaOutput::Fixed(bundle)]))
        }
        v2::ManaSpec::Choice(choices) => {
            if choices.is_empty() {
                return Ok(None);
            }
            // Fast path: all choices are single colored symbols (e.g. `{W}` or `{U}`).
            let mut single_colors = BTreeSet::new();
            let mut all_single_colored = true;
            for ch in choices {
                let Ok(bundle) = lower_v2_mana_bundle(ch) else {
                    all_single_colored = false;
                    break;
                };
                if bundle.len() == 1
                    && let Some((&mt, qty)) = bundle.iter().next()
                    && mt != ManaType::Colourless
                    && *qty == RulesInteger::from(1)
                {
                    single_colors.insert(mt);
                } else {
                    all_single_colored = false;
                    break;
                }
            }
            if all_single_colored && !single_colors.is_empty() {
                return Ok(Some(vec![ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: single_colors,
                }]));
            }
            // General choice among distinct fixed bundles (e.g. `{C}` or `{G}`, `{R}{R}` or `{G}{G}`).
            let mut outputs = Vec::with_capacity(choices.len());
            for ch in choices {
                let Ok(bundle) = lower_v2_mana_bundle(ch) else {
                    return Ok(None);
                };
                outputs.push(ManaOutput::Fixed(bundle));
            }
            Ok(Some(outputs))
        }
        v2::ManaSpec::AnyColor {
            amount,
            single_color,
        } => match amount {
            v2::ValueExpr::Const(n) if *n > 0 => {
                if *n == 1 || *single_color {
                    Ok(Some(vec![ManaOutput::ChooseColor {
                        amount: i64::from(*n).into(),
                        colors: ManaOutput::all_colors(),
                    }]))
                } else if *n == 2 {
                    Ok(Some(vec![ManaOutput::TwoColors { distinct: false }]))
                } else {
                    Ok(None)
                }
            }
            v2::ValueExpr::Count(filter) => {
                let subj = v2::SubjectRef::Each(filter.clone());
                let Ok(sel) = lower_permanent_selector(&subj, ctx) else {
                    return Ok(None);
                };
                Ok(Some(vec![ManaOutput::Counted {
                    mana_type: None,
                    permanents: sel,
                }]))
            }
            v2::ValueExpr::AggregateStat {
                op: v2::AggregateOp::Max,
                stat: StatKind::Power,
                filter,
            } if is_plain_controlled_creature_filter(filter) => {
                Ok(Some(vec![ManaOutput::Dynamic(
                    DynamicManaYield::GreatestCreaturePower,
                )]))
            }
            _ => Ok(None),
        },
        v2::ManaSpec::DifferentColors(v2::ValueExpr::Const(2)) => {
            Ok(Some(vec![ManaOutput::TwoColors { distinct: true }]))
        }
        v2::ManaSpec::Scaled { base, scale } => match scale {
            v2::ValueExpr::Count(filter) => {
                let subj = v2::SubjectRef::Each(filter.clone());
                let Ok(sel) = lower_permanent_selector(&subj, ctx) else {
                    return Ok(None);
                };
                match base.as_ref() {
                    v2::ManaSpec::Exact(mc) => {
                        let Ok(bundle) = lower_v2_mana_bundle(mc) else {
                            return Ok(None);
                        };
                        if bundle.len() == 1
                            && let Some((&mt, qty)) = bundle.iter().next()
                            && *qty == RulesInteger::from(1)
                        {
                            Ok(Some(vec![ManaOutput::Counted {
                                mana_type: Some(mt),
                                permanents: sel,
                            }]))
                        } else {
                            Ok(None)
                        }
                    }
                    v2::ManaSpec::AnyColor {
                        amount: v2::ValueExpr::Const(1),
                        ..
                    } => Ok(Some(vec![ManaOutput::Counted {
                        mana_type: None,
                        permanents: sel,
                    }])),
                    _ => Ok(None),
                }
            }
            v2::ValueExpr::CounterCount {
                subject,
                kind: v2::CounterKind::Charge,
            } if matches!(subject.as_ref(), v2::SubjectRef::SelfObject) => match base.as_ref() {
                v2::ManaSpec::Exact(mc) => {
                    let Ok(bundle) = lower_v2_mana_bundle(mc) else {
                        return Ok(None);
                    };
                    if bundle.len() == 1 && bundle.get(&ManaType::Colourless) == Some(&1.into()) {
                        Ok(Some(vec![ManaOutput::CounterBased(
                            CounterManaYield::SourceChargeCounters,
                        )]))
                    } else {
                        Ok(None)
                    }
                }
                _ => Ok(None),
            },
            _ => Ok(None),
        },
        v2::ManaSpec::Either(a, b) => {
            if let Some(fixed) = extract_entry_chosen_color_pair(a, b) {
                return Ok(Some(vec![ManaOutput::SpecialColor(
                    SpecialColorYield::EntryChosenColor { fixed },
                )]));
            }
            let Some(mut left) = try_lower_mana_spec_outputs(a, ctx)? else {
                return Ok(None);
            };
            let Some(right) = try_lower_mana_spec_outputs(b, ctx)? else {
                return Ok(None);
            };
            left.extend(right);
            Ok(Some(left))
        }
        _ => Ok(None),
    }
}

fn extract_entry_chosen_color_pair(a: &v2::ManaSpec, b: &v2::ManaSpec) -> Option<ManaType> {
    let single_colored = |spec: &v2::ManaSpec| -> Option<ManaType> {
        let v2::ManaSpec::Exact(mc) = spec else {
            return None;
        };
        let bundle = lower_v2_mana_bundle(mc).ok()?;
        if bundle.len() == 1
            && let Some((&mt, qty)) = bundle.iter().next()
            && mt != ManaType::Colourless
            && *qty == RulesInteger::from(1)
        {
            Some(mt)
        } else {
            None
        }
    };
    let is_one_chosen =
        |spec: &v2::ManaSpec| matches!(spec, v2::ManaSpec::ChosenColor(v2::ValueExpr::Const(1)));
    if let Some(mt) = single_colored(a)
        && is_one_chosen(b)
    {
        return Some(mt);
    }
    if let Some(mt) = single_colored(b)
        && is_one_chosen(a)
    {
        return Some(mt);
    }
    None
}

pub(crate) fn try_lower_mana_spend_rule(
    rule: &v2::ManaRuleKind,
    ctx: &mut LowerCtx<'_>,
) -> Option<ManaOutputRestriction> {
    match rule {
        v2::ManaRuleKind::SpendPurpose(purpose) => lower_mana_spend_purpose(purpose, ctx),
        v2::ManaRuleKind::SpendForbidden(purpose) => {
            lower_mana_spend_forbidden(purpose).map(ManaOutputRestriction::Fixed)
        }
        v2::ManaRuleKind::SpendRestriction(sr) if !sr.only_for => {
            if let Some(flt) = &sr.spell_filter
                && sr.cast_from_zone.is_none()
            {
                return lower_mana_spend_forbidden(&v2::ManaSpendPurpose::CastSpells(Some(
                    flt.clone(),
                )))
                .map(ManaOutputRestriction::Fixed);
            }
            if let Some(v2::ZoneKind::Hand) = sr.cast_from_zone
                && sr.spell_filter.is_none()
            {
                return Some(ManaOutputRestriction::Fixed(
                    ManaRestriction::NotSpellFromHand,
                ));
            }
            None
        }
        v2::ManaRuleKind::SpendRestriction(sr) => match (&sr.cast_from_zone, &sr.spell_filter) {
            (Some(zone), None) => {
                let mut pred = ManaSpendPredicate::default();
                let cast_mode = match zone {
                    v2::ZoneKind::Graveyard => SpellCastMode::FromGraveyard,
                    v2::ZoneKind::Exile => SpellCastMode::FromExile,
                    _ => return None,
                };
                pred.spell_clauses.push(SpellManaClause {
                    cast_mode: Some(cast_mode),
                    ..Default::default()
                });
                pred.canonicalize();
                Some(ManaOutputRestriction::Fixed(ManaRestriction::Predicate(
                    pred,
                )))
            }
            (None, Some(flt)) => lower_spell_mana_restriction(flt, ctx),
            (Some(zone), Some(flt)) => {
                let cast_mode = match zone {
                    v2::ZoneKind::Graveyard => SpellCastMode::FromGraveyard,
                    v2::ZoneKind::Exile => SpellCastMode::FromExile,
                    _ => return None,
                };
                let mut pred = ManaSpendPredicate::default();
                let clauses = lower_spell_mana_clauses(flt, Some(cast_mode), ctx)?;
                pred.spell_clauses.extend(clauses);
                pred.canonicalize();
                pred.is_non_empty().then_some(ManaOutputRestriction::Fixed(
                    ManaRestriction::Predicate(pred),
                ))
            }
            (None, None) => None,
        },
        v2::ManaRuleKind::SpendOnlyColorOnX(_) | v2::ManaRuleKind::KeepColorAcrossSteps(_) => None,
    }
}

fn lower_mana_spend_forbidden(purpose: &v2::ManaSpendPurpose) -> Option<ManaRestriction> {
    let v2::ManaSpendPurpose::CastSpells(Some(flt)) = purpose else {
        return None;
    };
    let no_extra = flt.all_types.is_empty()
        && flt.any_types.is_empty()
        && flt.required_supertypes.is_empty()
        && flt.forbidden_supertypes.is_empty()
        && flt.subtypes.is_empty()
        && flt.forbidden_subtypes.is_empty()
        && flt.any_colors.is_empty()
        && flt.forbidden_colors.is_empty()
        && flt.color_arity.is_none()
        && flt.controller.is_none()
        && flt.owner.is_none()
        && flt.combat.is_none()
        && flt.tapped.is_none()
        && flt.token.is_none()
        && flt.required_keywords.is_empty()
        && flt.forbidden_keywords.is_empty()
        && flt.stat_bounds.is_empty()
        && flt.flags.is_empty()
        && flt.predicates.is_empty();
    if !no_extra {
        return None;
    }
    if flt.forbidden_types == AstCardTypeMask::ARTIFACT && flt.zones.is_empty() {
        return Some(ManaRestriction::NotNonartifactSpell);
    }
    if flt.forbidden_types.is_empty()
        && matches!(
            flt.zones.as_slice(),
            [v2::ZoneRef {
                zone: v2::ZoneKind::Hand,
                owner: v2::ZoneOwner::You | v2::ZoneOwner::Default,
            }]
        )
    {
        return Some(ManaRestriction::NotSpellFromHand);
    }
    None
}

fn lower_mana_spend_purpose(
    purpose: &v2::ManaSpendPurpose,
    ctx: &mut LowerCtx<'_>,
) -> Option<ManaOutputRestriction> {
    match purpose {
        v2::ManaSpendPurpose::CastSpells(None) => {
            Some(ManaOutputRestriction::Fixed(ManaRestriction::CastSpell))
        }
        v2::ManaSpendPurpose::CastSpells(Some(flt)) => lower_spell_mana_restriction(flt, ctx),
        v2::ManaSpendPurpose::CastFromZone(zone) => {
            let cast_mode = match zone {
                v2::ZoneKind::Graveyard => SpellCastMode::FromGraveyard,
                v2::ZoneKind::Exile => SpellCastMode::FromExile,
                _ => return None,
            };
            let mut pred = ManaSpendPredicate::default();
            pred.spell_clauses.push(SpellManaClause {
                cast_mode: Some(cast_mode),
                ..Default::default()
            });
            pred.canonicalize();
            Some(ManaOutputRestriction::Fixed(ManaRestriction::Predicate(
                pred,
            )))
        }
        v2::ManaSpendPurpose::ActivateAbilities(None) => Some(ManaOutputRestriction::Fixed(
            ManaRestriction::ActivatedAbility,
        )),
        v2::ManaSpendPurpose::ActivateAbilities(Some(flt)) => {
            if is_chosen_creature_type_only_filter(flt) {
                return None;
            }
            let clauses = lower_ability_mana_clauses(flt, ctx)?;
            if clauses.len() == 1
                && clauses[0].source_creature_type.is_none()
                && !clauses[0].source_colorless
                && clauses[0].kind.is_none()
            {
                if clauses[0].source_card_type == Some(CardType::Artifact) {
                    return Some(ManaOutputRestriction::Fixed(
                        ManaRestriction::ArtifactAbility,
                    ));
                }
                if clauses[0].source_card_type == Some(CardType::Creature) {
                    return Some(ManaOutputRestriction::Fixed(
                        ManaRestriction::CreatureAbility,
                    ));
                }
            }
            let mut pred = ManaSpendPredicate {
                ability_clauses: clauses,
                ..Default::default()
            };
            pred.canonicalize();
            pred.is_non_empty()
                .then_some(ManaOutputRestriction::Fixed(ManaRestriction::Predicate(
                    pred,
                )))
        }
        v2::ManaSpendPurpose::Foretell => {
            let mut pred = ManaSpendPredicate::default();
            pred.special_actions.insert(ManaSpecialAction::Foretell);
            Some(ManaOutputRestriction::Fixed(ManaRestriction::Predicate(
                pred,
            )))
        }
        v2::ManaSpendPurpose::AnyOf(purposes) => {
            if purposes.is_empty() {
                return None;
            }
            // Check for chosen-creature-type spell or ability patterns.
            let mut has_chosen_spell = false;
            let mut has_chosen_ability = false;
            let mut pred = ManaSpendPredicate::default();
            for p in purposes {
                match p {
                    v2::ManaSpendPurpose::CastSpells(Some(flt))
                        if is_chosen_creature_type_only_filter(flt) =>
                    {
                        has_chosen_spell = true;
                    }
                    v2::ManaSpendPurpose::ActivateAbilities(Some(flt))
                        if is_chosen_creature_type_only_filter(flt) =>
                    {
                        has_chosen_ability = true;
                    }
                    other => {
                        append_purpose_to_predicate(other, &mut pred, ctx)?;
                    }
                }
            }
            pred.canonicalize();
            match (has_chosen_spell, has_chosen_ability, pred.is_non_empty()) {
                (true, true, false) => Some(ManaOutputRestriction::ChosenCreatureType(
                    ChosenTypeManaScope::CreatureSpellOrAbility,
                )),
                (true, false, false) => Some(ManaOutputRestriction::ChosenCreatureType(
                    ChosenTypeManaScope::CreatureSpell,
                )),
                (true, false, true) => Some(ManaOutputRestriction::ChosenCreatureTypeOr(pred)),
                (false, false, true) => Some(ManaOutputRestriction::Fixed(
                    ManaRestriction::Predicate(pred),
                )),
                _ => None,
            }
        }
    }
}

fn append_purpose_to_predicate(
    purpose: &v2::ManaSpendPurpose,
    pred: &mut ManaSpendPredicate,
    ctx: &LowerCtx<'_>,
) -> Option<()> {
    match purpose {
        v2::ManaSpendPurpose::CastSpells(None) => {
            pred.allow_any_spell = true;
            Some(())
        }
        v2::ManaSpendPurpose::CastSpells(Some(flt)) => {
            if flt.flags == v2::FilterFlags::HAS_X_IN_MANA_COST
                && flt.all_types.is_empty()
                && flt.any_types.is_empty()
                && flt.forbidden_types.is_empty()
                && flt.subtypes.is_empty()
            {
                pred.allow_cost_containing_x = true;
                return Some(());
            }
            let clauses = lower_spell_mana_clauses(flt, None, ctx)?;
            pred.spell_clauses.extend(clauses);
            Some(())
        }
        v2::ManaSpendPurpose::CastFromZone(zone) => {
            let cast_mode = match zone {
                v2::ZoneKind::Graveyard => SpellCastMode::FromGraveyard,
                v2::ZoneKind::Exile => SpellCastMode::FromExile,
                _ => return None,
            };
            pred.spell_clauses.push(SpellManaClause {
                cast_mode: Some(cast_mode),
                ..Default::default()
            });
            Some(())
        }
        v2::ManaSpendPurpose::ActivateAbilities(None) => {
            pred.allow_any_ability = true;
            Some(())
        }
        v2::ManaSpendPurpose::ActivateAbilities(Some(flt)) => {
            let clauses = lower_ability_mana_clauses(flt, ctx)?;
            pred.ability_clauses.extend(clauses);
            Some(())
        }
        v2::ManaSpendPurpose::Foretell => {
            pred.special_actions.insert(ManaSpecialAction::Foretell);
            Some(())
        }
        v2::ManaSpendPurpose::AnyOf(inner) => {
            for p in inner {
                append_purpose_to_predicate(p, pred, ctx)?;
            }
            Some(())
        }
    }
}

fn is_chosen_creature_type_only_filter(flt: &v2::ObjectFilter) -> bool {
    let types = flt.all_types.union(flt.any_types);
    (types.is_empty() || types == AstCardTypeMask::CREATURE)
        && flt.forbidden_types.is_empty()
        && flt.required_supertypes.is_empty()
        && flt.forbidden_supertypes.is_empty()
        && flt.subtypes.is_empty()
        && flt.forbidden_subtypes.is_empty()
        && flt.any_colors.is_empty()
        && flt.forbidden_colors.is_empty()
        && flt.color_arity.is_none()
        && flt.controller.is_none()
        && flt.owner.is_none()
        && flt.zones.is_empty()
        && flt.combat.is_none()
        && flt.tapped.is_none()
        && flt.token.is_none()
        && flt.required_keywords.is_empty()
        && flt.forbidden_keywords.is_empty()
        && flt.stat_bounds.is_empty()
        && flt.flags.is_empty()
        && flt.predicates == [v2::FilterPredicate::OfChosenCreatureType]
}

fn lower_spell_mana_restriction(
    flt: &v2::ObjectFilter,
    ctx: &LowerCtx<'_>,
) -> Option<ManaOutputRestriction> {
    if is_chosen_creature_type_only_filter(flt) {
        return Some(ManaOutputRestriction::ChosenCreatureType(
            ChosenTypeManaScope::CreatureSpell,
        ));
    }
    if flt.flags == v2::FilterFlags::IS_COMMANDER
        && flt.all_types.is_empty()
        && flt.any_types.is_empty()
        && flt.forbidden_types.is_empty()
        && flt.subtypes.is_empty()
        && flt.predicates.is_empty()
    {
        return Some(ManaOutputRestriction::Fixed(
            ManaRestriction::CommanderSpell,
        ));
    }
    if flt.flags == v2::FilterFlags::HAS_X_IN_MANA_COST
        && flt.all_types.is_empty()
        && flt.any_types.is_empty()
        && flt.forbidden_types.is_empty()
        && flt.subtypes.is_empty()
        && flt.predicates.is_empty()
    {
        return Some(ManaOutputRestriction::Fixed(
            ManaRestriction::CostContainingX,
        ));
    }
    if flt.any_permanent
        && flt.all_types.is_empty()
        && flt.any_types.is_empty()
        && flt.forbidden_types.is_empty()
        && flt.required_supertypes.is_empty()
        && flt.subtypes.is_empty()
        && flt.color_arity.is_none()
        && flt.any_colors.is_empty()
        && flt.flags.is_empty()
        && flt.predicates.is_empty()
    {
        return Some(ManaOutputRestriction::Fixed(
            ManaRestriction::PermanentSpell,
        ));
    }

    // Fast paths for common canonical single restrictions.
    if flt.forbidden_subtypes.is_empty()
        && flt.any_colors.is_empty()
        && flt.forbidden_colors.is_empty()
        && flt.controller.is_none()
        && flt.owner.is_none()
        && flt.zones.is_empty()
        && flt.combat.is_none()
        && flt.tapped.is_none()
        && flt.token.is_none()
        && flt.required_keywords.is_empty()
        && flt.forbidden_keywords.is_empty()
        && flt.stat_bounds.is_empty()
        && flt.flags.is_empty()
        && flt.predicates.is_empty()
    {
        let types = flt.all_types.union(flt.any_types);
        if flt.subtypes.len() == 1
            && !flt.subtypes_match_any
            && flt.forbidden_types.is_empty()
            && flt.required_supertypes.is_empty()
            && flt.color_arity.is_none()
            && (types.is_empty() || types == AstCardTypeMask::CREATURE)
            && let Some(ct) = ctx.lookup_creature_type(&flt.subtypes[0])
        {
            return Some(ManaOutputRestriction::Fixed(
                ManaRestriction::CreatureSpellOfType(Some(ct)),
            ));
        }
        if flt.subtypes.is_empty() && flt.forbidden_supertypes.is_empty() {
            if flt.forbidden_types == AstCardTypeMask::CREATURE
                && types.is_empty()
                && flt.required_supertypes.is_empty()
                && flt.color_arity.is_none()
            {
                return Some(ManaOutputRestriction::Fixed(
                    ManaRestriction::NoncreatureSpell,
                ));
            }
            if flt.forbidden_types.is_empty()
                && flt.required_supertypes.is_empty()
                && flt.color_arity.is_none()
            {
                if types == AstCardTypeMask::CREATURE {
                    return Some(ManaOutputRestriction::Fixed(ManaRestriction::CreatureSpell));
                }
                if types == AstCardTypeMask::ARTIFACT {
                    return Some(ManaOutputRestriction::Fixed(ManaRestriction::ArtifactSpell));
                }
                if types == AstCardTypeMask::ENCHANTMENT {
                    return Some(ManaOutputRestriction::Fixed(
                        ManaRestriction::EnchantmentSpell,
                    ));
                }
                if types == AstCardTypeMask::PLANESWALKER {
                    return Some(ManaOutputRestriction::Fixed(
                        ManaRestriction::PlaneswalkerSpell,
                    ));
                }
                if types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY)
                    && flt.all_types.is_empty()
                {
                    return Some(ManaOutputRestriction::Fixed(
                        ManaRestriction::InstantOrSorcerySpell,
                    ));
                }
            }
            if types.is_empty()
                && flt.forbidden_types.is_empty()
                && flt.required_supertypes == SupertypeMask::LEGENDARY
                && flt.color_arity.is_none()
            {
                return Some(ManaOutputRestriction::Fixed(
                    ManaRestriction::LegendarySpell,
                ));
            }
            if types.is_empty()
                && flt.forbidden_types.is_empty()
                && flt.required_supertypes.is_empty()
            {
                if flt.color_arity == Some(ColorArity::Colorless) {
                    return Some(ManaOutputRestriction::Fixed(
                        ManaRestriction::ColorlessSpell,
                    ));
                }
                if flt.color_arity == Some(ColorArity::Multicolored) {
                    return Some(ManaOutputRestriction::Fixed(
                        ManaRestriction::MulticoloredSpell,
                    ));
                }
            }
        }
    }

    let clauses = lower_spell_mana_clauses(flt, None, ctx)?;
    let mut pred = ManaSpendPredicate {
        spell_clauses: clauses,
        ..Default::default()
    };
    pred.canonicalize();
    pred.is_non_empty()
        .then_some(ManaOutputRestriction::Fixed(ManaRestriction::Predicate(
            pred,
        )))
}

fn lower_spell_mana_clauses(
    flt: &v2::ObjectFilter,
    extra_cast_mode: Option<SpellCastMode>,
    ctx: &LowerCtx<'_>,
) -> Option<Vec<SpellManaClause>> {
    if !flt.forbidden_supertypes.is_empty()
        || !flt.forbidden_subtypes.is_empty()
        || !flt.forbidden_colors.is_empty()
        || flt.controller.is_some()
        || flt.owner.is_some()
        || flt.combat.is_some()
        || flt.tapped.is_some()
        || flt.token.is_some()
        || !flt.forbidden_keywords.is_empty()
        || flt.any_permanent
    {
        return None;
    }

    let mut base = SpellManaClause {
        cast_mode: extra_cast_mode,
        ..Default::default()
    };

    // Supertypes
    if !flt.required_supertypes.is_empty() {
        let supers = lower_supertype_set(flt.required_supertypes);
        if supers.len() != 1 {
            return None;
        }
        base.supertype = supers.into_iter().next();
    }

    // Forbidden card type
    if !flt.forbidden_types.is_empty() {
        let forb = lower_card_type_set(flt.forbidden_types);
        if forb.len() != 1 {
            return None;
        }
        base.exclude_card_type = forb.into_iter().next();
    }

    // Color / arity
    if !flt.any_colors.is_empty() && flt.color_arity.is_some() {
        return None;
    }
    if !flt.any_colors.is_empty() {
        let colors = lower_color_set(flt.any_colors);
        if colors.len() != 1 {
            return None;
        }
        base.color_rule = colors.into_iter().next().map(SpellColorRule::ExactColor);
    } else if let Some(arity) = flt.color_arity {
        base.color_rule = Some(match arity {
            ColorArity::Colorless => SpellColorRule::Colorless,
            ColorArity::Colored => SpellColorRule::Colored,
            ColorArity::Multicolored => SpellColorRule::Multicolored,
            ColorArity::Monocolored => SpellColorRule::ExactColorCount(1),
        });
    }

    // Keywords
    if flt.required_keywords.len() > 1 {
        return None;
    }
    if let Some(kw) = flt.required_keywords.first() {
        base.keyword = Some(match kw {
            v2::KeywordKind::Devoid => ManaSpellKeyword::Devoid,
            v2::KeywordKind::Freerunning => ManaSpellKeyword::Freerunning,
            v2::KeywordKind::Foretell => ManaSpellKeyword::Foretell,
            v2::KeywordKind::Flashback => ManaSpellKeyword::Flashback,
            v2::KeywordKind::Disturb => ManaSpellKeyword::Disturb,
            _ => return None,
        });
    }

    // Zones -> cast mode
    if !flt.zones.is_empty() {
        if base.cast_mode.is_some() || flt.zones.len() != 1 {
            return None;
        }
        base.cast_mode = Some(match flt.zones[0].zone {
            v2::ZoneKind::Graveyard => SpellCastMode::FromGraveyard,
            v2::ZoneKind::Exile => SpellCastMode::FromExile,
            _ => return None,
        });
    }

    // Flags
    let mut rem_flags = flt.flags.0;
    if rem_flags & v2::FilterFlags::FACE_DOWN.0 != 0 {
        if base.cast_mode.is_some() {
            return None;
        }
        base.cast_mode = Some(SpellCastMode::FaceDown);
        rem_flags &= !v2::FilterFlags::FACE_DOWN.0;
    }
    if rem_flags & v2::FilterFlags::KICKED.0 != 0 {
        if base.cast_mode.is_some() {
            return None;
        }
        base.cast_mode = Some(SpellCastMode::Kicked);
        rem_flags &= !v2::FilterFlags::KICKED.0;
    }
    if rem_flags != 0 {
        return None;
    }

    // Predicates
    for pred in &flt.predicates {
        let cm = match pred {
            v2::FilterPredicate::FromOutsideHand => SpellCastMode::NotFromHand,
            v2::FilterPredicate::NotOwnedByController => SpellCastMode::NotOwned,
            v2::FilterPredicate::NthSpellCastThisTurn(2) => SpellCastMode::SecondSpellThisTurn,
            _ => return None,
        };
        if base.cast_mode.replace(cm).is_some() {
            return None;
        }
    }

    // Stat bounds (min mana value)
    if flt.stat_bounds.len() > 1 {
        return None;
    }
    if let Some(sb) = flt.stat_bounds.first() {
        if sb.stat != StatKind::ManaValue {
            return None;
        }
        let v2::ValueExpr::Const(n) = sb.value.as_ref() else {
            return None;
        };
        let min_mv = match sb.op {
            CompareOp::Ge if *n >= 0 => u8::try_from(*n).ok()?,
            CompareOp::Gt if *n >= 0 => u8::try_from(*n + 1).ok()?,
            _ => return None,
        };
        base.min_mana_value = Some(min_mv);
    }

    // Card types (either single conjunctive type or disjunctive any_types)
    let type_variants: Vec<Option<CardType>> = if !flt.all_types.is_empty() {
        if !flt.any_types.is_empty() {
            return None;
        }
        let all = lower_card_type_set(flt.all_types);
        if all.len() != 1 {
            return None;
        }
        vec![all.into_iter().next()]
    } else if !flt.any_types.is_empty() {
        lower_card_type_set(flt.any_types)
            .into_iter()
            .map(Some)
            .collect()
    } else {
        vec![None]
    };

    // Subtypes (single subtype or disjunctive `subtypes_match_any`)
    let mut clauses = Vec::new();
    for ct_opt in &type_variants {
        let mut with_type = base.clone();
        with_type.card_type = *ct_opt;
        if flt.subtypes.is_empty() {
            clauses.push(with_type);
        } else if flt.subtypes.len() == 1 || flt.subtypes_match_any {
            for raw in &flt.subtypes {
                let mut c = with_type.clone();
                if raw.eq_ignore_ascii_case("aura") {
                    c.is_aura = true;
                } else if let Some(ct) = ctx.lookup_creature_type(raw) {
                    c.creature_type = Some(ct);
                } else if let Some(at) = LowerCtx::lookup_artifact_subtype(raw) {
                    c.artifact_subtype = Some(at);
                } else if let Some(et) = LowerCtx::lookup_enchantment_subtype(raw) {
                    c.enchantment_subtype = Some(et);
                } else if let Some(st) = LowerCtx::lookup_spell_subtype(raw) {
                    c.spell_subtype = Some(st);
                } else {
                    let pt = PlaneswalkerSubtype::from_label(raw)?;
                    c.planeswalker_subtype = Some(pt);
                }
                clauses.push(c);
            }
        } else {
            return None;
        }
    }

    if clauses.is_empty() || clauses.iter().all(|c| *c == SpellManaClause::default()) {
        return None;
    }
    Some(clauses)
}

fn lower_ability_mana_clauses(
    flt: &v2::ObjectFilter,
    ctx: &LowerCtx<'_>,
) -> Option<Vec<AbilityManaClause>> {
    if !flt.forbidden_types.is_empty()
        || !flt.required_supertypes.is_empty()
        || !flt.forbidden_supertypes.is_empty()
        || !flt.forbidden_subtypes.is_empty()
        || !flt.any_colors.is_empty()
        || !flt.forbidden_colors.is_empty()
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
        return None;
    }

    let source_colorless = match flt.color_arity {
        None => false,
        Some(ColorArity::Colorless) => true,
        Some(_) => return None,
    };

    let type_variants: Vec<Option<CardType>> = if !flt.all_types.is_empty() {
        if !flt.any_types.is_empty() {
            return None;
        }
        let all = lower_card_type_set(flt.all_types);
        if all.len() != 1 {
            return None;
        }
        vec![all.into_iter().next()]
    } else if !flt.any_types.is_empty() {
        lower_card_type_set(flt.any_types)
            .into_iter()
            .map(Some)
            .collect()
    } else {
        vec![None]
    };

    let mut clauses = Vec::new();
    for ct_opt in &type_variants {
        if flt.subtypes.is_empty() {
            clauses.push(AbilityManaClause {
                source_card_type: *ct_opt,
                source_creature_type: None,
                source_colorless,
                kind: None,
            });
        } else if flt.subtypes.len() == 1 || flt.subtypes_match_any {
            for raw in &flt.subtypes {
                let ct = ctx.lookup_creature_type(raw)?;
                clauses.push(AbilityManaClause {
                    source_card_type: *ct_opt,
                    source_creature_type: Some(ct),
                    source_colorless,
                    kind: None,
                });
            }
        } else {
            return None;
        }
    }

    if clauses.is_empty() || clauses.iter().all(|c| *c == AbilityManaClause::default()) {
        return None;
    }
    Some(clauses)
}

fn lower_condition_restriction(
    cond: &v2::ConditionExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineActivationRestriction> {
    match cond {
        v2::ConditionExpr::SubjectState {
            subject,
            state: v2::ObjectStateKind::Tapped,
        } if matches!(subject.as_ref(), v2::SubjectRef::SelfObject) => {
            Ok(EngineActivationRestriction::SourceIsTapped)
        }
        v2::ConditionExpr::SubjectState {
            subject,
            state: v2::ObjectStateKind::Untapped,
        } if matches!(subject.as_ref(), v2::SubjectRef::SelfObject) => {
            Ok(EngineActivationRestriction::SourceIsUntapped)
        }
        v2::ConditionExpr::Flag(v2::ConditionFlag::HasCitysBlessing) => {
            Ok(EngineActivationRestriction::HasCityBlessing)
        }
        v2::ConditionExpr::Flag(v2::ConditionFlag::IsYourTurn) => {
            Ok(EngineActivationRestriction::DuringYourTurn)
        }
        v2::ConditionExpr::Not(inner)
            if matches!(
                inner.as_ref(),
                v2::ConditionExpr::Flag(v2::ConditionFlag::IsYourTurn)
            ) =>
        {
            Ok(EngineActivationRestriction::DuringOpponentsTurn)
        }
        v2::ConditionExpr::Controls {
            player: v2::PlayerRef::You,
            filter,
            min_count,
        } if *min_count > 0 && *min_count <= u16::from(u8::MAX) => {
            let obj_filter = lower_object_filter(filter, ctx)?;
            Ok(EngineActivationRestriction::ControlsMatching {
                filter: obj_filter,
                minimum: *min_count as u8,
            })
        }
        v2::ConditionExpr::SubjectMatches { subject, filter }
            if matches!(subject.as_ref(), v2::SubjectRef::SelfObject) =>
        {
            let obj_filter = lower_object_filter(filter, ctx)?;
            Ok(EngineActivationRestriction::SourceMatches(obj_filter))
        }
        other => {
            let res_cond = lower_resolution_condition(other, ctx)?;
            Ok(EngineActivationRestriction::Condition(Box::new(res_cond)))
        }
    }
}

fn lower_single_mana_symbol(sym: &v2::ManaSymbol) -> LowerResult<ManaType> {
    match sym {
        v2::ManaSymbol::White => Ok(ManaType::White),
        v2::ManaSymbol::Blue => Ok(ManaType::Blue),
        v2::ManaSymbol::Black => Ok(ManaType::Black),
        v2::ManaSymbol::Red => Ok(ManaType::Red),
        v2::ManaSymbol::Green => Ok(ManaType::Green),
        v2::ManaSymbol::Colorless => Ok(ManaType::Colourless),
        _ => Err(LoweringError::Unsupported("v2:mana:bad-hybrid-symbol")),
    }
}

fn lower_hybrid_pair(
    a: &v2::ManaSymbol,
    b: &v2::ManaSymbol,
) -> LowerResult<(ManaType, HybridAlternative)> {
    match (a, b) {
        (v2::ManaSymbol::Generic(2), second) => {
            let first = lower_single_mana_symbol(second)?;
            if first == ManaType::Colourless {
                return Err(LoweringError::Unsupported("v2:mana:bad-twobrid"));
            }
            Ok((first, HybridAlternative::TwoGeneric))
        }
        (first_sym, v2::ManaSymbol::Generic(2)) => {
            let first = lower_single_mana_symbol(first_sym)?;
            if first == ManaType::Colourless {
                return Err(LoweringError::Unsupported("v2:mana:bad-twobrid"));
            }
            Ok((first, HybridAlternative::TwoGeneric))
        }
        (v2::ManaSymbol::Colorless, second) => {
            let first = lower_single_mana_symbol(second)?;
            Ok((first, HybridAlternative::Mana(ManaType::Colourless)))
        }
        (first_sym, second_sym) => {
            let first = lower_single_mana_symbol(first_sym)?;
            let second = lower_single_mana_symbol(second_sym)?;
            Ok((first, HybridAlternative::Mana(second)))
        }
    }
}

fn add_mana(cost: &mut ManaCost, mt: ManaType, qty: i64) {
    let entry = cost.required.entry(mt).or_insert_with(|| 0.into());
    *entry = entry.plus(&qty.into());
}

pub(crate) fn merge_mana_cost(dst: &mut ManaCost, src: &ManaCost) {
    dst.generic = dst.generic.plus(&src.generic);
    for (&mt, amt) in &src.required {
        let entry = dst.required.entry(mt).or_insert_with(|| 0.into());
        *entry = entry.plus(amt);
    }
}

fn value_to_nonzero_u8(val: &v2::ValueExpr, err_code: &'static str) -> LowerResult<NonZeroU8> {
    match val {
        v2::ValueExpr::Const(n) if *n > 0 && *n <= i32::from(u8::MAX) => {
            NonZeroU8::new(*n as u8).ok_or(LoweringError::Unsupported(err_code))
        }
        _ => Err(LoweringError::Unsupported(err_code)),
    }
}

fn value_to_opt_nonzero_u8(
    val: &v2::ValueExpr,
    err_code: &'static str,
) -> LowerResult<Option<NonZeroU8>> {
    match val {
        v2::ValueExpr::Const(n) if *n > 0 && *n <= i32::from(u8::MAX) => {
            let nz = NonZeroU8::new(*n as u8).ok_or(LoweringError::Unsupported(err_code))?;
            Ok(Some(nz))
        }
        v2::ValueExpr::Var(v2::VarName::X) => Ok(None),
        _ => Err(LoweringError::Unsupported(err_code)),
    }
}

fn lower_cost_exile_zone(
    from: &[v2::ZoneRef],
    filter: &v2::ObjectFilter,
) -> LowerResult<CostExileZone> {
    let zone_ref = from.first().or_else(|| filter.zones.first());
    match zone_ref {
        None => Ok(CostExileZone::Battlefield),
        Some(zr) => match zr.zone {
            v2::ZoneKind::Battlefield => Ok(CostExileZone::Battlefield),
            v2::ZoneKind::Graveyard => Ok(CostExileZone::Graveyard),
            v2::ZoneKind::Hand => Ok(CostExileZone::Hand),
            _ => Err(LoweringError::Unsupported("v2:cost:exile-unsupported-zone")),
        },
    }
}

pub(crate) fn is_battlefield_only_source_cost(cost: &ExtraActivationCost) -> bool {
    match cost {
        ExtraActivationCost::TapSource
        | ExtraActivationCost::UntapSource
        | ExtraActivationCost::SacrificeSource
        | ExtraActivationCost::ExertSource
        | ExtraActivationCost::ReturnSourceToHand
        | ExtraActivationCost::SourceCounter { .. }
        | ExtraActivationCost::DiscardSource
        | ExtraActivationCost::ExileSource { .. }
        | ExtraActivationCost::RevealSource
        | ExtraActivationCost::Unattach { self_source: true } => true,
        ExtraActivationCost::Either(a, b) => {
            is_battlefield_only_source_cost(a) || is_battlefield_only_source_cost(b)
        }
        _ => false,
    }
}

fn is_plain_creature_filter(f: &v2::ObjectFilter) -> bool {
    let types = f.all_types.union(f.any_types);
    types == AstCardTypeMask::CREATURE
        && f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && matches!(f.controller, None | Some(ControllerRelation::You))
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

fn is_plain_controlled_creature_filter(f: &v2::ObjectFilter) -> bool {
    is_plain_creature_filter(f) && f.controller == Some(ControllerRelation::You)
}

fn is_plain_controlled_artifact_filter(f: &v2::ObjectFilter) -> bool {
    let types = f.all_types.union(f.any_types);
    types == AstCardTypeMask::ARTIFACT
        && f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.controller == Some(ControllerRelation::You)
        && f.owner.is_none()
        && f.zones.is_empty()
        && f.combat.is_none()
        && matches!(f.tapped, None | Some(false))
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
        && f.predicates.is_empty()
}

pub(crate) type ModalChoiceSlice<'a> = (
    &'a [v2::Effect],
    v2::ChoiceCount,
    &'a [v2::ModalMode],
    &'a [v2::Effect],
);

pub(crate) fn extract_modal_choice(effects: &[v2::Effect]) -> Option<ModalChoiceSlice<'_>> {
    for (idx, eff) in effects.iter().enumerate() {
        match eff {
            v2::Effect::PromptChoice {
                chooser: v2::PlayerRef::You,
                kind: v2::ChoiceKind::Modal { count, modes },
            } => {
                return Some((
                    &effects[..idx],
                    *count,
                    modes.as_slice(),
                    &effects[idx + 1..],
                ));
            }
            v2::Effect::Optional {
                actor: v2::PlayerRef::You,
                body,
            } => {
                if let [
                    v2::Effect::PromptChoice {
                        chooser: v2::PlayerRef::You,
                        kind: v2::ChoiceKind::Modal { count, modes },
                    },
                ] = body.as_slice()
                {
                    let effective = match *count {
                        v2::ChoiceCount::Exact(1) => v2::ChoiceCount::UpTo(1),
                        v2::ChoiceCount::UpTo(n) => v2::ChoiceCount::UpTo(n),
                        v2::ChoiceCount::AnyNumber => v2::ChoiceCount::AnyNumber,
                        _ => continue,
                    };
                    return Some((
                        &effects[..idx],
                        effective,
                        modes.as_slice(),
                        &effects[idx + 1..],
                    ));
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn lower_choice_count(count: v2::ChoiceCount, mode_len: usize) -> LowerResult<(u8, u8)> {
    let cap = u8::try_from(mode_len)
        .map_err(|_| LoweringError::Unsupported("v2:modal:too-many-modes"))?;
    match count {
        v2::ChoiceCount::Exact(n) if n > 0 && n <= cap => Ok((n, n)),
        v2::ChoiceCount::UpTo(n) if n > 0 => Ok((0, n.min(cap))),
        v2::ChoiceCount::OneOrBoth => Ok((1, 2u8.min(cap))),
        v2::ChoiceCount::OneOrMore => Ok((1, cap)),
        v2::ChoiceCount::AnyNumber => Ok((0, cap)),
        v2::ChoiceCount::VariableX | v2::ChoiceCount::Exact(_) | v2::ChoiceCount::UpTo(_) => Err(
            LoweringError::Unsupported("v2:modal:unsupported-choice-count"),
        ),
    }
}

/// Ensure unused imports remain clean even when helper paths evolve.
#[allow(dead_code)]
fn _unused_engine_check() {
    let _ = (
        std::any::TypeId::of::<CardType>(),
        std::any::TypeId::of::<ControllerSelector>(),
        std::any::TypeId::of::<PermanentSelector>(),
        std::any::TypeId::of::<ObjectClause>(),
        std::any::TypeId::of::<ManaOutputModifier>(),
        std::any::TypeId::of::<ManaOutputRestriction>(),
        std::any::TypeId::of::<ProgramKind>(),
    );
}
