//! Atomic `v2::ir` to `spellwake-engine` lowerers: mana, types, colors, supertypes,
//! counters, static keywords, durations, turn steps, comparisons, players, zones, and subjects.

use std::collections::{BTreeMap, BTreeSet};

use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    characteristics::{
        Color, HexproofQuality, KeywordAbility, ProtectionFromQuality, ResolvedDuration,
        SubtypeProtection,
    },
    effects::{CounterKind as EngineCounterKind, CustomCounterKey},
    face_down::FaceDownKind,
    mana::{HybridAlternative, HybridSymbol, ManaCost, ManaType},
    program::{
        AffectedPlayers, BodyTarget, CardType, CardTypeMask as EngineCardTypeMask,
        ColorMask as EngineColorMask, Comparison, ControllerFilter, EffectManaYield,
        ImpulsePlayDuration, Instruction, ObjectFilter as EngineObjectFilter, ObjectStatKind,
        ResolvingPlayer, SourceZoneFilter, SourceZoneKind, Supertype,
        SupertypeMask as EngineSupertypeMask, TargetZone, ValueExpr as EngineValueExpr, ValueZone,
    },
    toolbox::{
        GraveyardSemantics, LibraryOrderPos, ObjectQuery, PlayerSelector, QueryZone,
        ZoneController, ZoneDest as EngineZoneDest, ZoneSelection,
    },
    turn::Step,
};
use spellwake_oracle_compiler::{ast, v2::ir as v2};

use crate::v2_lowering::{
    ctx::{BodyAcc, LowerCtx},
    error::{LowerResult, LoweringError},
    filter::{
        lower_object_filter, lower_spell_filter, scan_filter_player_target, split_among_prior_cards,
    },
    value::{lower_value_const, lower_value_expr, scan_value_expr_player_target},
};

/// Extract a single concrete [`ManaType`] from a [`v2::ManaSymbol`].
#[must_use]
pub(crate) fn lower_single_mana_type(symbol: &v2::ManaSymbol) -> Option<ManaType> {
    match symbol {
        v2::ManaSymbol::White => Some(ManaType::White),
        v2::ManaSymbol::Blue => Some(ManaType::Blue),
        v2::ManaSymbol::Black => Some(ManaType::Black),
        v2::ManaSymbol::Red => Some(ManaType::Red),
        v2::ManaSymbol::Green => Some(ManaType::Green),
        v2::ManaSymbol::Colorless => Some(ManaType::Colourless),
        v2::ManaSymbol::Snow
        | v2::ManaSymbol::Generic(_)
        | v2::ManaSymbol::VariableX
        | v2::ManaSymbol::Tap
        | v2::ManaSymbol::Untap
        | v2::ManaSymbol::Energy
        | v2::ManaSymbol::Ticket
        | v2::ManaSymbol::Pawprint
        | v2::ManaSymbol::AnyPhyrexian
        | v2::ManaSymbol::Hybrid(_, _)
        | v2::ManaSymbol::Phyrexian(_) => None,
    }
}

/// Convert a single [`Color`] into its corresponding colored [`ManaType`].
#[must_use]
pub(crate) const fn color_to_mana_type(color: Color) -> ManaType {
    match color {
        Color::White => ManaType::White,
        Color::Blue => ManaType::Blue,
        Color::Black => ManaType::Black,
        Color::Red => ManaType::Red,
        Color::Green => ManaType::Green,
    }
}

/// Lower a [`v2::ManaCost`] into an engine [`ManaCost`], count of `{X}` symbols, and hybrid list.
///
/// Each hybrid or Phyrexian symbol contributes one unit of its primary mana type into
/// `cost.required` so that selecting [`spellwake_engine::mana::HybridSide::Second`] during
/// casting payment replaces that unit with the alternative side cleanly.
pub(crate) fn lower_mana_cost(
    cost: &v2::ManaCost,
) -> LowerResult<(ManaCost, u8, Vec<HybridSymbol>)> {
    let mut generic = RulesInteger::from(0);
    let mut required: BTreeMap<ManaType, RulesInteger> = BTreeMap::new();
    let mut x_count: u8 = 0;
    let mut hybrids: Vec<HybridSymbol> = Vec::new();

    for sym in &cost.symbols {
        match sym {
            v2::ManaSymbol::White => add_mana_unit(&mut required, ManaType::White, 1),
            v2::ManaSymbol::Blue => add_mana_unit(&mut required, ManaType::Blue, 1),
            v2::ManaSymbol::Black => add_mana_unit(&mut required, ManaType::Black, 1),
            v2::ManaSymbol::Red => add_mana_unit(&mut required, ManaType::Red, 1),
            v2::ManaSymbol::Green => add_mana_unit(&mut required, ManaType::Green, 1),
            v2::ManaSymbol::Colorless => add_mana_unit(&mut required, ManaType::Colourless, 1),
            v2::ManaSymbol::Generic(amount) => {
                generic = generic.plus(&RulesInteger::from(i64::from(*amount)));
            }
            v2::ManaSymbol::VariableX => {
                x_count = x_count
                    .checked_add(1)
                    .ok_or(LoweringError::Unsupported("v2:mana-cost:x-overflow"))?;
            }
            v2::ManaSymbol::Hybrid(lhs, rhs) => {
                if let (Some(first), Some(second)) =
                    (lower_single_mana_type(lhs), lower_single_mana_type(rhs))
                {
                    add_mana_unit(&mut required, first, 1);
                    hybrids.push(HybridSymbol {
                        first,
                        alternative: HybridAlternative::Mana(second),
                    });
                } else if let (v2::ManaSymbol::Generic(2), Some(first)) =
                    (lhs.as_ref(), lower_single_mana_type(rhs))
                {
                    add_mana_unit(&mut required, first, 1);
                    hybrids.push(HybridSymbol {
                        first,
                        alternative: HybridAlternative::TwoGeneric,
                    });
                } else if let (Some(first), v2::ManaSymbol::Generic(2)) =
                    (lower_single_mana_type(lhs), rhs.as_ref())
                {
                    add_mana_unit(&mut required, first, 1);
                    hybrids.push(HybridSymbol {
                        first,
                        alternative: HybridAlternative::TwoGeneric,
                    });
                } else {
                    return Err(LoweringError::Unsupported("v2:mana-cost:unsupported-hybrid"));
                }
            }
            v2::ManaSymbol::Phyrexian(inner) => {
                let first = lower_single_mana_type(inner)
                    .ok_or(LoweringError::Unsupported("v2:mana-cost:unsupported-phyrexian"))?;
                add_mana_unit(&mut required, first, 1);
                hybrids.push(HybridSymbol {
                    first,
                    alternative: HybridAlternative::TwoLife,
                });
            }
            v2::ManaSymbol::Snow => {
                return Err(LoweringError::Unsupported("v2:mana-cost:snow"));
            }
            v2::ManaSymbol::Tap => {
                return Err(LoweringError::Unsupported("v2:mana-cost:tap-symbol"));
            }
            v2::ManaSymbol::Untap => {
                return Err(LoweringError::Unsupported("v2:mana-cost:untap-symbol"));
            }
            v2::ManaSymbol::Energy => {
                return Err(LoweringError::Unsupported("v2:mana-cost:energy-symbol"));
            }
            v2::ManaSymbol::Ticket => {
                return Err(LoweringError::Unsupported("v2:mana-cost:ticket-symbol"));
            }
            v2::ManaSymbol::Pawprint => {
                return Err(LoweringError::Unsupported("v2:mana-cost:pawprint-symbol"));
            }
            v2::ManaSymbol::AnyPhyrexian => {
                return Err(LoweringError::Unsupported("v2:mana-cost:any-phyrexian"));
            }
        }
    }

    Ok((ManaCost { generic, required }, x_count, hybrids))
}

fn add_mana_unit(map: &mut BTreeMap<ManaType, RulesInteger>, mana_type: ManaType, count: i64) {
    let entry = map.entry(mana_type).or_insert_with(|| RulesInteger::from(0));
    *entry = entry.plus(&RulesInteger::from(count));
}

/// Lower a concrete produced mana sequence (e.g. `{G}{G}` or `{C}{C}`) into a non-empty bundle map.
pub(crate) fn lower_mana_bundle(
    cost: &v2::ManaCost,
) -> LowerResult<BTreeMap<ManaType, RulesInteger>> {
    let mut bundle: BTreeMap<ManaType, RulesInteger> = BTreeMap::new();
    for sym in &cost.symbols {
        let mana_type = match sym {
            v2::ManaSymbol::White => ManaType::White,
            v2::ManaSymbol::Blue => ManaType::Blue,
            v2::ManaSymbol::Black => ManaType::Black,
            v2::ManaSymbol::Red => ManaType::Red,
            v2::ManaSymbol::Green => ManaType::Green,
            v2::ManaSymbol::Colorless => ManaType::Colourless,
            v2::ManaSymbol::Generic(_) => {
                return Err(LoweringError::Unsupported("v2:mana-bundle:generic"));
            }
            v2::ManaSymbol::VariableX => {
                return Err(LoweringError::Unsupported("v2:mana-bundle:variable-x"));
            }
            v2::ManaSymbol::Snow => {
                return Err(LoweringError::Unsupported("v2:mana-bundle:snow"));
            }
            v2::ManaSymbol::Hybrid(_, _) => {
                return Err(LoweringError::Unsupported("v2:mana-bundle:hybrid"));
            }
            v2::ManaSymbol::Phyrexian(_) | v2::ManaSymbol::AnyPhyrexian => {
                return Err(LoweringError::Unsupported("v2:mana-bundle:phyrexian"));
            }
            v2::ManaSymbol::Tap
            | v2::ManaSymbol::Untap
            | v2::ManaSymbol::Energy
            | v2::ManaSymbol::Ticket
            | v2::ManaSymbol::Pawprint => {
                return Err(LoweringError::Unsupported("v2:mana-bundle:non-mana-symbol"));
            }
        };
        add_mana_unit(&mut bundle, mana_type, 1);
    }
    if bundle.is_empty() {
        return Err(LoweringError::Unsupported("v2:mana-bundle:empty"));
    }
    Ok(bundle)
}

fn all_five_mana_colors() -> BTreeSet<ManaType> {
    BTreeSet::from([
        ManaType::White,
        ManaType::Blue,
        ManaType::Black,
        ManaType::Red,
        ManaType::Green,
    ])
}

fn single_type_bundle(bundle: &BTreeMap<ManaType, RulesInteger>) -> Option<(ManaType, RulesInteger)> {
    if bundle.len() == 1 {
        bundle.iter().next().map(|(k, v)| (*k, v.clone()))
    } else {
        None
    }
}

fn lower_bundle_choices(
    bundles: Vec<BTreeMap<ManaType, RulesInteger>>,
) -> LowerResult<EffectManaYield> {
    if bundles.is_empty() {
        return Err(LoweringError::Unsupported("v2:mana-spec:empty-choice"));
    }
    if bundles.len() == 1
        && let Some(only) = bundles.first().cloned()
    {
        return Ok(EffectManaYield::Fixed(only));
    }
    // Collapse uniform single-type choices (e.g. `{W}`, `{U}`, or `{R}{R}`, `{G}{G}`) to `Choice`.
    let mut uniform_amount: Option<RulesInteger> = None;
    let mut colors = BTreeSet::new();
    let mut all_uniform = true;
    for b in &bundles {
        if let Some((mt, amt)) = single_type_bundle(b) {
            if let Some(ref prev) = uniform_amount {
                if *prev != amt {
                    all_uniform = false;
                    break;
                }
            } else {
                uniform_amount = Some(amt);
            }
            colors.insert(mt);
        } else {
            all_uniform = false;
            break;
        }
    }
    if all_uniform
        && let Some(amt) = uniform_amount
    {
        return Ok(EffectManaYield::Choice {
            colors,
            amount: EngineValueExpr::literal(amt),
        });
    }
    Ok(EffectManaYield::ChoiceBundle { options: bundles })
}

fn collect_choice_bundles(
    spec: &v2::ManaSpec,
    out: &mut Vec<BTreeMap<ManaType, RulesInteger>>,
) -> LowerResult<()> {
    match spec {
        v2::ManaSpec::Exact(cost) => {
            if cost.symbols.len() == 1
                && let v2::ManaSymbol::Hybrid(a, b) = &cost.symbols[0]
                && let (Some(m1), Some(m2)) =
                    (lower_single_mana_type(a), lower_single_mana_type(b))
            {
                out.push(BTreeMap::from([(m1, RulesInteger::from(1))]));
                out.push(BTreeMap::from([(m2, RulesInteger::from(1))]));
                return Ok(());
            }
            out.push(lower_mana_bundle(cost)?);
            Ok(())
        }
        v2::ManaSpec::Choice(costs) => {
            if costs.is_empty() {
                return Err(LoweringError::Unsupported("v2:mana-spec:empty-choice"));
            }
            for cost in costs {
                out.push(lower_mana_bundle(cost)?);
            }
            Ok(())
        }
        v2::ManaSpec::Either(lhs, rhs) => {
            collect_choice_bundles(lhs, out)?;
            collect_choice_bundles(rhs, out)?;
            Ok(())
        }
        v2::ManaSpec::AnyColor { .. }
        | v2::ManaSpec::DifferentColors(_)
        | v2::ManaSpec::ChosenColor(_)
        | v2::ManaSpec::ThatColor(_)
        | v2::ManaSpec::Scaled { .. } => {
            Err(LoweringError::Unsupported("v2:mana-spec:non-bundle-choice"))
        }
    }
}

/// Lower a [`v2::ManaSpec`] into an [`EffectManaYield`].
pub(crate) fn lower_mana_spec(
    spec: &v2::ManaSpec,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EffectManaYield> {
    match spec {
        v2::ManaSpec::Exact(cost) => {
            if cost.symbols.len() == 1
                && let v2::ManaSymbol::Hybrid(a, b) = &cost.symbols[0]
                && let (Some(m1), Some(m2)) =
                    (lower_single_mana_type(a), lower_single_mana_type(b))
            {
                return Ok(EffectManaYield::Choice {
                    colors: BTreeSet::from([m1, m2]),
                    amount: EngineValueExpr::literal(RulesInteger::from(1)),
                });
            }
            Ok(EffectManaYield::Fixed(lower_mana_bundle(cost)?))
        }
        v2::ManaSpec::Choice(costs) => {
            let mut bundles = Vec::with_capacity(costs.len());
            for cost in costs {
                bundles.push(lower_mana_bundle(cost)?);
            }
            lower_bundle_choices(bundles)
        }
        v2::ManaSpec::AnyColor {
            amount,
            single_color,
        } => {
            let lowered = lower_value_expr(amount, ctx)?;
            if *single_color || lower_value_const(amount) == Some(1) {
                Ok(EffectManaYield::Choice {
                    colors: all_five_mana_colors(),
                    amount: lowered,
                })
            } else {
                Err(LoweringError::Unsupported("v2:mana-spec:any-combination"))
            }
        }
        v2::ManaSpec::DifferentColors(amount) => {
            let lowered = lower_value_expr(amount, ctx)?;
            Ok(EffectManaYield::DistinctColors { amount: lowered })
        }
        v2::ManaSpec::ChosenColor(amount) => {
            let lowered = lower_value_expr(amount, ctx)?;
            Ok(EffectManaYield::SourceChosenColor { amount: lowered })
        }
        v2::ManaSpec::ThatColor(_) => Err(LoweringError::Unsupported("v2:mana-spec:that-color")),
        v2::ManaSpec::Either(lhs, rhs) => {
            if let (
                v2::ManaSpec::Scaled {
                    base: b1,
                    scale: s1,
                },
                v2::ManaSpec::Scaled {
                    base: b2,
                    scale: s2,
                },
            ) = (lhs.as_ref(), rhs.as_ref())
                && s1 == s2
            {
                let mut bundles = Vec::new();
                if collect_choice_bundles(b1, &mut bundles).is_ok()
                    && collect_choice_bundles(b2, &mut bundles).is_ok()
                {
                    let scaled = lower_value_expr(s1, ctx)?;
                    return lower_scaled_bundles(bundles, scaled);
                }
            }
            let mut bundles = Vec::new();
            collect_choice_bundles(lhs, &mut bundles)?;
            collect_choice_bundles(rhs, &mut bundles)?;
            lower_bundle_choices(bundles)
        }
        v2::ManaSpec::Scaled { base, scale } => {
            let lowered_scale = lower_value_expr(scale, ctx)?;
            match base.as_ref() {
                v2::ManaSpec::Exact(_) | v2::ManaSpec::Choice(_) | v2::ManaSpec::Either(_, _) => {
                    let mut bundles = Vec::new();
                    collect_choice_bundles(base, &mut bundles)?;
                    lower_scaled_bundles(bundles, lowered_scale)
                }
                v2::ManaSpec::AnyColor {
                    amount,
                    single_color,
                } => {
                    if !*single_color {
                        return Err(LoweringError::Unsupported(
                            "v2:mana-spec:scaled-any-combination",
                        ));
                    }
                    let factor = lower_value_const(amount)
                        .filter(|&k| k > 0)
                        .ok_or(LoweringError::Unsupported("v2:mana-spec:scaled-nonconst-base"))?;
                    let total = scale_expr(lowered_scale, factor);
                    Ok(EffectManaYield::Choice {
                        colors: all_five_mana_colors(),
                        amount: total,
                    })
                }
                v2::ManaSpec::ChosenColor(amount) => {
                    let factor = lower_value_const(amount)
                        .filter(|&k| k > 0)
                        .ok_or(LoweringError::Unsupported("v2:mana-spec:scaled-nonconst-base"))?;
                    let total = scale_expr(lowered_scale, factor);
                    Ok(EffectManaYield::SourceChosenColor { amount: total })
                }
                v2::ManaSpec::DifferentColors(amount) => {
                    if lower_value_const(amount) != Some(1) {
                        return Err(LoweringError::Unsupported(
                            "v2:mana-spec:scaled-different-colors",
                        ));
                    }
                    Ok(EffectManaYield::DistinctColors {
                        amount: lowered_scale,
                    })
                }
                v2::ManaSpec::ThatColor(_) | v2::ManaSpec::Scaled { .. } => {
                    Err(LoweringError::Unsupported("v2:mana-spec:nested-scaled"))
                }
            }
        }
    }
}

fn scale_expr(expr: EngineValueExpr, factor: i32) -> EngineValueExpr {
    if factor == 1 {
        expr
    } else {
        EngineValueExpr::Scale {
            expr: Box::new(expr),
            factor: RulesInteger::from(i64::from(factor)),
        }
    }
}

fn lower_scaled_bundles(
    bundles: Vec<BTreeMap<ManaType, RulesInteger>>,
    scale: EngineValueExpr,
) -> LowerResult<EffectManaYield> {
    if bundles.is_empty() {
        return Err(LoweringError::Unsupported("v2:mana-spec:empty-scaled"));
    }
    if bundles.len() == 1 {
        let Some((kind, unit)) = single_type_bundle(&bundles[0]) else {
            return Err(LoweringError::Unsupported("v2:mana-spec:scaled-multi-type"));
        };
        let amount = if unit == RulesInteger::from(1) {
            scale
        } else {
            EngineValueExpr::Scale {
                expr: Box::new(scale),
                factor: unit,
            }
        };
        return Ok(EffectManaYield::Counted { kind, amount });
    }
    let mut uniform_unit: Option<RulesInteger> = None;
    let mut colors = BTreeSet::new();
    for b in &bundles {
        let Some((mt, unit)) = single_type_bundle(b) else {
            return Err(LoweringError::Unsupported(
                "v2:mana-spec:scaled-multi-type-choice",
            ));
        };
        if let Some(ref prev) = uniform_unit {
            if *prev != unit {
                return Err(LoweringError::Unsupported(
                    "v2:mana-spec:scaled-nonuniform-choice",
                ));
            }
        } else {
            uniform_unit = Some(unit);
        }
        colors.insert(mt);
    }
    let Some(unit) = uniform_unit else {
        return Err(LoweringError::Unsupported("v2:mana-spec:empty-scaled"));
    };
    let amount = if unit == RulesInteger::from(1) {
        scale
    } else {
        EngineValueExpr::Scale {
            expr: Box::new(scale),
            factor: unit,
        }
    };
    Ok(EffectManaYield::Choice { colors, amount })
}

/// Convert an [`ast::CardTypeMask`] into a deterministic [`BTreeSet<CardType>`].
#[must_use]
pub(crate) fn lower_card_type_set(mask: ast::CardTypeMask) -> BTreeSet<CardType> {
    let mut set = BTreeSet::new();
    let bits = mask.0;
    if (bits & ast::CardTypeMask::BATTLE.0) != 0 {
        set.insert(CardType::Battle);
    }
    if (bits & ast::CardTypeMask::PLANESWALKER.0) != 0 {
        set.insert(CardType::Planeswalker);
    }
    if (bits & ast::CardTypeMask::LAND.0) != 0 {
        set.insert(CardType::Land);
    }
    if (bits & ast::CardTypeMask::ARTIFACT.0) != 0 {
        set.insert(CardType::Artifact);
    }
    if (bits & ast::CardTypeMask::CREATURE.0) != 0 {
        set.insert(CardType::Creature);
    }
    if (bits & ast::CardTypeMask::ENCHANTMENT.0) != 0 {
        set.insert(CardType::Enchantment);
    }
    if (bits & ast::CardTypeMask::KINDRED.0) != 0 {
        set.insert(CardType::Kindred);
    }
    if (bits & ast::CardTypeMask::INSTANT.0) != 0 {
        set.insert(CardType::Instant);
    }
    if (bits & ast::CardTypeMask::SORCERY.0) != 0 {
        set.insert(CardType::Sorcery);
    }
    set
}

/// Convert an [`ast::CardTypeMask`] into an engine [`EngineCardTypeMask`].
#[must_use]
pub(crate) fn lower_engine_type_mask(mask: ast::CardTypeMask) -> EngineCardTypeMask {
    let mut out = EngineCardTypeMask::EMPTY;
    let bits = mask.0;
    if (bits & ast::CardTypeMask::BATTLE.0) != 0 {
        out = out.union(EngineCardTypeMask::BATTLE);
    }
    if (bits & ast::CardTypeMask::PLANESWALKER.0) != 0 {
        out = out.union(EngineCardTypeMask::PLANESWALKER);
    }
    if (bits & ast::CardTypeMask::LAND.0) != 0 {
        out = out.union(EngineCardTypeMask::LAND);
    }
    if (bits & ast::CardTypeMask::ARTIFACT.0) != 0 {
        out = out.union(EngineCardTypeMask::ARTIFACT);
    }
    if (bits & ast::CardTypeMask::CREATURE.0) != 0 {
        out = out.union(EngineCardTypeMask::CREATURE);
    }
    if (bits & ast::CardTypeMask::ENCHANTMENT.0) != 0 {
        out = out.union(EngineCardTypeMask::ENCHANTMENT);
    }
    if (bits & ast::CardTypeMask::KINDRED.0) != 0 {
        out = out.union(EngineCardTypeMask::KINDRED);
    }
    if (bits & ast::CardTypeMask::INSTANT.0) != 0 {
        out = out.union(EngineCardTypeMask::INSTANT);
    }
    if (bits & ast::CardTypeMask::SORCERY.0) != 0 {
        out = out.union(EngineCardTypeMask::SORCERY);
    }
    out
}

/// Convert an [`ast::ColorMask`] into an engine [`EngineColorMask`].
#[must_use]
pub(crate) fn lower_color_mask(mask: ast::ColorMask) -> EngineColorMask {
    let mut out = EngineColorMask::EMPTY;
    let bits = mask.0;
    if (bits & ast::ColorMask::WHITE.0) != 0 {
        out = out.union(EngineColorMask::WHITE);
    }
    if (bits & ast::ColorMask::BLUE.0) != 0 {
        out = out.union(EngineColorMask::BLUE);
    }
    if (bits & ast::ColorMask::BLACK.0) != 0 {
        out = out.union(EngineColorMask::BLACK);
    }
    if (bits & ast::ColorMask::RED.0) != 0 {
        out = out.union(EngineColorMask::RED);
    }
    if (bits & ast::ColorMask::GREEN.0) != 0 {
        out = out.union(EngineColorMask::GREEN);
    }
    out
}

/// Convert an [`ast::ColorMask`] into a deterministic [`BTreeSet<Color>`].
#[must_use]
pub(crate) fn lower_color_set(mask: ast::ColorMask) -> BTreeSet<Color> {
    let mut set = BTreeSet::new();
    let bits = mask.0;
    if (bits & ast::ColorMask::WHITE.0) != 0 {
        set.insert(Color::White);
    }
    if (bits & ast::ColorMask::BLUE.0) != 0 {
        set.insert(Color::Blue);
    }
    if (bits & ast::ColorMask::BLACK.0) != 0 {
        set.insert(Color::Black);
    }
    if (bits & ast::ColorMask::RED.0) != 0 {
        set.insert(Color::Red);
    }
    if (bits & ast::ColorMask::GREEN.0) != 0 {
        set.insert(Color::Green);
    }
    set
}

/// Convert an [`ast::SupertypeMask`] into a deterministic [`BTreeSet<Supertype>`].
///
/// Note: `ast::SupertypeMask::HISTORIC` is a batching predicate rather than a card supertype;
/// callers inspecting filter masks must handle `HISTORIC` explicitly via `ObjectFilterFlags::HISTORIC`.
#[must_use]
pub(crate) fn lower_supertype_set(mask: ast::SupertypeMask) -> BTreeSet<Supertype> {
    let mut set = BTreeSet::new();
    let bits = mask.0;
    if (bits & ast::SupertypeMask::BASIC.0) != 0 {
        set.insert(Supertype::Basic);
    }
    if (bits & ast::SupertypeMask::LEGENDARY.0) != 0 {
        set.insert(Supertype::Legendary);
    }
    if (bits & ast::SupertypeMask::SNOW.0) != 0 {
        set.insert(Supertype::Snow);
    }
    if (bits & ast::SupertypeMask::WORLD.0) != 0 {
        set.insert(Supertype::World);
    }
    set
}

/// Convert non-historic [`ast::SupertypeMask`] bits into an engine [`EngineSupertypeMask`].
pub(crate) fn lower_supertype_mask(mask: ast::SupertypeMask) -> LowerResult<EngineSupertypeMask> {
    let mut out = EngineSupertypeMask::EMPTY;
    let mut rem = mask.0 & !ast::SupertypeMask::HISTORIC.0;
    if (rem & ast::SupertypeMask::BASIC.0) != 0 {
        out = out.union(EngineSupertypeMask::BASIC);
        rem &= !ast::SupertypeMask::BASIC.0;
    }
    if (rem & ast::SupertypeMask::LEGENDARY.0) != 0 {
        out = out.union(EngineSupertypeMask::LEGENDARY);
        rem &= !ast::SupertypeMask::LEGENDARY.0;
    }
    if (rem & ast::SupertypeMask::SNOW.0) != 0 {
        out = out.union(EngineSupertypeMask::SNOW);
        rem &= !ast::SupertypeMask::SNOW.0;
    }
    if (rem & ast::SupertypeMask::WORLD.0) != 0 {
        out = out.union(EngineSupertypeMask::WORLD);
        rem &= !ast::SupertypeMask::WORLD.0;
    }
    if rem != 0 {
        return Err(LoweringError::Unsupported("v2:supertype:unknown-bit"));
    }
    Ok(out)
}

/// Convert an [`ast::StatKind`] into an engine [`ObjectStatKind`].
#[must_use]
pub(crate) const fn lower_stat_kind(stat: ast::StatKind) -> ObjectStatKind {
    match stat {
        ast::StatKind::Power => ObjectStatKind::Power,
        ast::StatKind::Toughness => ObjectStatKind::Toughness,
        ast::StatKind::ManaValue => ObjectStatKind::ManaValue,
        ast::StatKind::Loyalty => ObjectStatKind::Loyalty,
    }
}

/// Lower a [`v2::CounterKind`] into a concrete engine [`EngineCounterKind`].
pub(crate) fn lower_counter_kind(kind: &v2::CounterKind) -> LowerResult<EngineCounterKind> {
    match kind {
        v2::CounterKind::PlusOnePlusOne => Ok(EngineCounterKind::PlusOne),
        v2::CounterKind::MinusOneMinusOne => Ok(EngineCounterKind::MinusOne),
        v2::CounterKind::Loyalty => Ok(EngineCounterKind::Loyalty),
        v2::CounterKind::Time => Ok(EngineCounterKind::Time),
        v2::CounterKind::Energy => Ok(EngineCounterKind::Energy),
        v2::CounterKind::Poison => Ok(EngineCounterKind::Poison),
        v2::CounterKind::Experience => Ok(EngineCounterKind::Experience),
        v2::CounterKind::Finality => Ok(EngineCounterKind::Finality),
        v2::CounterKind::Stun => Ok(EngineCounterKind::Stun),
        v2::CounterKind::Charge => Ok(EngineCounterKind::Charge),
        v2::CounterKind::Spore => Ok(EngineCounterKind::Spore),
        v2::CounterKind::Slumber => CustomCounterKey::from_label("slumber")
            .map(EngineCounterKind::Custom)
            .ok_or(LoweringError::Unsupported("v2:counter:slumber")),
        v2::CounterKind::Lore => Ok(EngineCounterKind::Lore),
        v2::CounterKind::Shield => Ok(EngineCounterKind::Shield),
        v2::CounterKind::Oil => Ok(EngineCounterKind::Oil),
        v2::CounterKind::StunOrKeyword(kw) => lower_keyword_counter_kind(*kw),
        v2::CounterKind::Pt(1, 1) => Ok(EngineCounterKind::PlusOne),
        v2::CounterKind::Pt(-1, -1) => Ok(EngineCounterKind::MinusOne),
        v2::CounterKind::Pt(power, toughness) => Ok(EngineCounterKind::PtDelta {
            power: *power,
            toughness: *toughness,
        }),
        v2::CounterKind::Any => Err(LoweringError::Unsupported("v2:counter:unspecified-any")),
        v2::CounterKind::Named(name) => lower_named_counter_kind(name),
    }
}

fn lower_keyword_counter_kind(kw: v2::KeywordKind) -> LowerResult<EngineCounterKind> {
    match kw {
        v2::KeywordKind::Flying => Ok(EngineCounterKind::Flying),
        v2::KeywordKind::FirstStrike => Ok(EngineCounterKind::FirstStrike),
        v2::KeywordKind::DoubleStrike => Ok(EngineCounterKind::DoubleStrike),
        v2::KeywordKind::Deathtouch => Ok(EngineCounterKind::Deathtouch),
        v2::KeywordKind::Haste => Ok(EngineCounterKind::Haste),
        v2::KeywordKind::Hexproof => Ok(EngineCounterKind::Hexproof),
        v2::KeywordKind::Indestructible => Ok(EngineCounterKind::Indestructible),
        v2::KeywordKind::Lifelink => Ok(EngineCounterKind::Lifelink),
        v2::KeywordKind::Menace => Ok(EngineCounterKind::Menace),
        v2::KeywordKind::Reach => Ok(EngineCounterKind::Reach),
        v2::KeywordKind::Shadow => Ok(EngineCounterKind::Shadow),
        v2::KeywordKind::Trample => Ok(EngineCounterKind::Trample),
        v2::KeywordKind::Vigilance => Ok(EngineCounterKind::Vigilance),
        _ => Err(LoweringError::Unsupported("v2:counter:unsupported-keyword")),
    }
}

fn lower_named_counter_kind(raw: &str) -> LowerResult<EngineCounterKind> {
    let lower = raw.to_ascii_lowercase();
    let known = match lower.as_str() {
        "page" => Some(EngineCounterKind::Page),
        "quest" => Some(EngineCounterKind::Quest),
        "verse" => Some(EngineCounterKind::Verse),
        "suspect" => Some(EngineCounterKind::Suspect),
        "charge" => Some(EngineCounterKind::Charge),
        "storage" => Some(EngineCounterKind::Storage),
        "lore" => Some(EngineCounterKind::Lore),
        "loyalty" => Some(EngineCounterKind::Loyalty),
        "defense" => Some(EngineCounterKind::Defense),
        "time" => Some(EngineCounterKind::Time),
        "fade" => Some(EngineCounterKind::Fade),
        "age" => Some(EngineCounterKind::Age),
        "indestructible" => Some(EngineCounterKind::Indestructible),
        "corpse" => Some(EngineCounterKind::Corpse),
        "vow" => Some(EngineCounterKind::Vow),
        "blessing" => Some(EngineCounterKind::Blessing),
        "despair" => Some(EngineCounterKind::Despair),
        "hatchling" => Some(EngineCounterKind::Hatchling),
        "study" => Some(EngineCounterKind::Study),
        "eyeball" => Some(EngineCounterKind::Eyeball),
        "slime" => Some(EngineCounterKind::Slime),
        "oil" => Some(EngineCounterKind::Oil),
        "level" => Some(EngineCounterKind::Level),
        "spore" => Some(EngineCounterKind::Spore),
        "ki" => Some(EngineCounterKind::Ki),
        "soul" => Some(EngineCounterKind::Soul),
        "fuse" => Some(EngineCounterKind::Fuse),
        "depletion" => Some(EngineCounterKind::Depletion),
        "bounty" => Some(EngineCounterKind::Bounty),
        "flood" => Some(EngineCounterKind::Flood),
        "flame" => Some(EngineCounterKind::Flame),
        "hour" => Some(EngineCounterKind::Hour),
        "brick" => Some(EngineCounterKind::Brick),
        "plague" => Some(EngineCounterKind::Plague),
        "doom" => Some(EngineCounterKind::Doom),
        "pressure" => Some(EngineCounterKind::Pressure),
        "feather" => Some(EngineCounterKind::Feather),
        "fate" => Some(EngineCounterKind::Fate),
        "infection" => Some(EngineCounterKind::Infection),
        "ice" => Some(EngineCounterKind::Ice),
        "tide" => Some(EngineCounterKind::Tide),
        "growth" => Some(EngineCounterKind::Growth),
        "divinity" => Some(EngineCounterKind::Divinity),
        "fungus" => Some(EngineCounterKind::Fungus),
        "hit" => Some(EngineCounterKind::Hit),
        "gold" => Some(EngineCounterKind::Gold),
        "blood" => Some(EngineCounterKind::Blood),
        "cage" => Some(EngineCounterKind::Cage),
        "coin" => Some(EngineCounterKind::Coin),
        "credit" => Some(EngineCounterKind::Credit),
        "death" => Some(EngineCounterKind::Death),
        "delay" => Some(EngineCounterKind::Delay),
        "devotion" => Some(EngineCounterKind::Devotion),
        "egg" => Some(EngineCounterKind::Egg),
        "eon" => Some(EngineCounterKind::Eon),
        "fury" => Some(EngineCounterKind::Fury),
        "incubation" => Some(EngineCounterKind::Incubation),
        "knowledge" => Some(EngineCounterKind::Knowledge),
        "luck" => Some(EngineCounterKind::Luck),
        "mine" => Some(EngineCounterKind::Mine),
        "music" => Some(EngineCounterKind::Music),
        "muster" => Some(EngineCounterKind::Muster),
        "net" => Some(EngineCounterKind::Net),
        "omen" => Some(EngineCounterKind::Omen),
        "ore" => Some(EngineCounterKind::Ore),
        "pain" => Some(EngineCounterKind::Pain),
        "petal" => Some(EngineCounterKind::Petal),
        "phylactery" => Some(EngineCounterKind::Phylactery),
        "pin" => Some(EngineCounterKind::Pin),
        "plot" => Some(EngineCounterKind::Plot),
        "prey" => Some(EngineCounterKind::Prey),
        "pupa" => Some(EngineCounterKind::Pupa),
        "shell" => Some(EngineCounterKind::Shell),
        "silver" => Some(EngineCounterKind::Silver),
        "sleep" => Some(EngineCounterKind::Sleep),
        "soot" => Some(EngineCounterKind::Soot),
        "stash" => Some(EngineCounterKind::Stash),
        "strife" => Some(EngineCounterKind::Strife),
        "task" => Some(EngineCounterKind::Task),
        "tower" => Some(EngineCounterKind::Tower),
        "training" => Some(EngineCounterKind::Training),
        "trap" => Some(EngineCounterKind::Trap),
        "treasure" => Some(EngineCounterKind::Treasure),
        "valor" => Some(EngineCounterKind::Valor),
        "velocity" => Some(EngineCounterKind::Velocity),
        "vitality" => Some(EngineCounterKind::Vitality),
        "void" => Some(EngineCounterKind::Void),
        "vortex" => Some(EngineCounterKind::Vortex),
        "wage" => Some(EngineCounterKind::Wage),
        "winch" => Some(EngineCounterKind::Winch),
        "wind" => Some(EngineCounterKind::Wind),
        "wish" => Some(EngineCounterKind::Wish),
        "flying" => Some(EngineCounterKind::Flying),
        "first strike" => Some(EngineCounterKind::FirstStrike),
        "double strike" => Some(EngineCounterKind::DoubleStrike),
        "deathtouch" => Some(EngineCounterKind::Deathtouch),
        "haste" => Some(EngineCounterKind::Haste),
        "hexproof" => Some(EngineCounterKind::Hexproof),
        "lifelink" => Some(EngineCounterKind::Lifelink),
        "menace" => Some(EngineCounterKind::Menace),
        "reach" => Some(EngineCounterKind::Reach),
        "shadow" => Some(EngineCounterKind::Shadow),
        "trample" => Some(EngineCounterKind::Trample),
        "vigilance" => Some(EngineCounterKind::Vigilance),
        "stun" => Some(EngineCounterKind::Stun),
        "shield" => Some(EngineCounterKind::Shield),
        "finality" => Some(EngineCounterKind::Finality),
        "poison" => Some(EngineCounterKind::Poison),
        "experience" => Some(EngineCounterKind::Experience),
        "energy" => Some(EngineCounterKind::Energy),
        "rad" => Some(EngineCounterKind::Rad),
        _ => None,
    };
    if let Some(counter) = known {
        return Ok(counter);
    }
    CustomCounterKey::from_label(&lower)
        .map(EngineCounterKind::Custom)
        .ok_or(LoweringError::Unsupported("v2:counter:invalid-custom-name"))
}

/// Lower a bare [`v2::KeywordKind`] with no parameter into a static [`KeywordAbility`].
pub(crate) fn lower_keyword_kind_plain(kind: v2::KeywordKind) -> LowerResult<KeywordAbility> {
    match kind {
        v2::KeywordKind::Flying => Ok(KeywordAbility::Flying),
        v2::KeywordKind::FirstStrike => Ok(KeywordAbility::FirstStrike),
        v2::KeywordKind::DoubleStrike => Ok(KeywordAbility::DoubleStrike),
        v2::KeywordKind::Deathtouch => Ok(KeywordAbility::Deathtouch),
        v2::KeywordKind::Haste => Ok(KeywordAbility::Haste),
        v2::KeywordKind::Hexproof => Ok(KeywordAbility::Hexproof),
        v2::KeywordKind::Indestructible => Ok(KeywordAbility::Indestructible),
        v2::KeywordKind::Lifelink => Ok(KeywordAbility::Lifelink),
        v2::KeywordKind::Menace => Ok(KeywordAbility::Menace),
        v2::KeywordKind::Reach => Ok(KeywordAbility::Reach),
        v2::KeywordKind::Trample => Ok(KeywordAbility::Trample),
        v2::KeywordKind::Vigilance => Ok(KeywordAbility::Vigilance),
        v2::KeywordKind::Defender => Ok(KeywordAbility::Defender),
        v2::KeywordKind::Flash => Ok(KeywordAbility::Flash),
        v2::KeywordKind::Shroud => Ok(KeywordAbility::Shroud),
        v2::KeywordKind::Convoke => Ok(KeywordAbility::Convoke),
        v2::KeywordKind::Delve => Ok(KeywordAbility::Delve),
        v2::KeywordKind::Wither => Ok(KeywordAbility::Wither),
        v2::KeywordKind::Infect => Ok(KeywordAbility::Infect),
        v2::KeywordKind::Fear => Ok(KeywordAbility::Fear),
        v2::KeywordKind::Intimidate => Ok(KeywordAbility::Intimidate),
        v2::KeywordKind::Skulk => Ok(KeywordAbility::Skulk),
        v2::KeywordKind::Shadow => Ok(KeywordAbility::Shadow),
        v2::KeywordKind::Horsemanship => Ok(KeywordAbility::Horsemanship),
        v2::KeywordKind::Banding => Ok(KeywordAbility::Banding),
        v2::KeywordKind::Changeling => Ok(KeywordAbility::Changeling),
        v2::KeywordKind::Devoid => Ok(KeywordAbility::Devoid),
        v2::KeywordKind::SplitSecond => Ok(KeywordAbility::SplitSecond),
        v2::KeywordKind::Rebound => Ok(KeywordAbility::Rebound),
        v2::KeywordKind::Retrace => Ok(KeywordAbility::Retrace),
        v2::KeywordKind::Partner => Ok(KeywordAbility::Partner),
        v2::KeywordKind::Plainswalk => Ok(KeywordAbility::Plainswalk),
        v2::KeywordKind::Islandwalk => Ok(KeywordAbility::Islandwalk),
        v2::KeywordKind::Swampwalk => Ok(KeywordAbility::Swampwalk),
        v2::KeywordKind::Mountainwalk => Ok(KeywordAbility::Mountainwalk),
        v2::KeywordKind::Forestwalk => Ok(KeywordAbility::Forestwalk),
        v2::KeywordKind::Improvise => Ok(KeywordAbility::Improvise),
        v2::KeywordKind::JumpStart => Ok(KeywordAbility::JumpStart),
        v2::KeywordKind::Decayed => Ok(KeywordAbility::Decayed),
        v2::KeywordKind::Daybound => Ok(KeywordAbility::Daybound),
        v2::KeywordKind::Nightbound => Ok(KeywordAbility::Nightbound),
        v2::KeywordKind::Bargain => Ok(KeywordAbility::Bargain),
        v2::KeywordKind::StartYourEngines => Ok(KeywordAbility::StartYourEngines),
        v2::KeywordKind::Phasing => Ok(KeywordAbility::Phasing),
        v2::KeywordKind::UmbraArmor => Ok(KeywordAbility::UmbraArmor),
        v2::KeywordKind::ChooseABackground => Ok(KeywordAbility::ChooseABackground),
        v2::KeywordKind::DoctorsCompanion => Ok(KeywordAbility::DoctorsCompanion),
        _ => Err(LoweringError::Unsupported("v2:keyword:non-static")),
    }
}

fn single_color_from_mask(mask: ast::ColorMask) -> Option<Color> {
    let set = lower_color_set(mask);
    if set.len() == 1 {
        set.into_iter().next()
    } else {
        None
    }
}

fn single_card_type_from_filter(f: &v2::ObjectFilter) -> Option<CardType> {
    let all = lower_card_type_set(f.all_types);
    let any = lower_card_type_set(f.any_types);
    if all.len() == 1 && any.is_empty() {
        all.into_iter().next()
    } else if any.len() == 1 && all.is_empty() {
        any.into_iter().next()
    } else {
        None
    }
}

fn has_only_basic_quality_fields(f: &v2::ObjectFilter) -> bool {
    f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.forbidden_colors.is_empty()
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

fn protection_from_color(color: Color) -> KeywordAbility {
    match color {
        Color::White => KeywordAbility::ProtectionFromWhite,
        Color::Blue => KeywordAbility::ProtectionFromBlue,
        Color::Black => KeywordAbility::ProtectionFromBlack,
        Color::Red => KeywordAbility::ProtectionFromRed,
        Color::Green => KeywordAbility::ProtectionFromGreen,
    }
}

/// Lower a [`v2::KeywordSpec`] into a static engine [`KeywordAbility`].
pub(crate) fn lower_keyword_ability(
    spec: &v2::KeywordSpec,
    ctx: &LowerCtx<'_>,
) -> LowerResult<KeywordAbility> {
    match (&spec.kind, &spec.param) {
        (kind, None) => lower_keyword_kind_plain(*kind),
        (v2::KeywordKind::Hexproof, Some(param)) => lower_hexproof_from(param, ctx),
        (v2::KeywordKind::Protection, Some(param)) => lower_protection_from(param, ctx),
        (v2::KeywordKind::Landwalk, Some(v2::KeywordParam::Filter(filter))) => {
            lower_landwalk_filter(filter)
        }
        _ => Err(LoweringError::Unsupported("v2:keyword:parameterized")),
    }
}

fn lower_hexproof_from(
    param: &v2::KeywordParam,
    ctx: &LowerCtx<'_>,
) -> LowerResult<KeywordAbility> {
    match param {
        v2::KeywordParam::Color(mask) => {
            let color = single_color_from_mask(*mask)
                .ok_or(LoweringError::Unsupported("v2:keyword:hexproof-multi-color"))?;
            Ok(KeywordAbility::HexproofFrom(HexproofQuality::Color(color)))
        }
        v2::KeywordParam::Filter(f) => {
            if !has_only_basic_quality_fields(f) || !f.forbidden_subtypes.is_empty() {
                return Err(LoweringError::Unsupported("v2:keyword:hexproof-filter"));
            }
            if f.is_ability
                && f.all_types.is_empty()
                && f.any_types.is_empty()
                && f.subtypes.is_empty()
                && f.any_colors.is_empty()
                && f.color_arity.is_none()
            {
                return Ok(KeywordAbility::HexproofFrom(
                    HexproofQuality::ActivatedOrTriggeredAbility,
                ));
            }
            if let Some(arity) = f.color_arity
                && f.all_types.is_empty()
                && f.any_types.is_empty()
                && f.subtypes.is_empty()
                && f.any_colors.is_empty()
            {
                return match arity {
                    ast::ColorArity::Monocolored => Ok(KeywordAbility::HexproofFrom(
                        HexproofQuality::Monocolored,
                    )),
                    ast::ColorArity::Multicolored => Ok(KeywordAbility::HexproofFrom(
                        HexproofQuality::Multicolored,
                    )),
                    ast::ColorArity::Colorless | ast::ColorArity::Colored => {
                        Err(LoweringError::Unsupported("v2:keyword:hexproof-color-arity"))
                    }
                };
            }
            if !f.any_colors.is_empty()
                && f.all_types.is_empty()
                && f.any_types.is_empty()
                && f.subtypes.is_empty()
                && f.color_arity.is_none()
            {
                let color = single_color_from_mask(f.any_colors)
                    .ok_or(LoweringError::Unsupported("v2:keyword:hexproof-multi-color"))?;
                return Ok(KeywordAbility::HexproofFrom(HexproofQuality::Color(color)));
            }
            if f.subtypes.len() == 1
                && f.any_colors.is_empty()
                && f.color_arity.is_none()
                && f.all_types.is_empty()
                && f.any_types.is_empty()
            {
                let ct = ctx
                    .lookup_creature_type(&f.subtypes[0])
                    .ok_or(LoweringError::Unsupported("v2:keyword:hexproof-subtype"))?;
                return Ok(KeywordAbility::HexproofFrom(HexproofQuality::Subtype(ct)));
            }
            if f.subtypes.is_empty()
                && f.any_colors.is_empty()
                && f.color_arity.is_none()
                && let Some(ct) = single_card_type_from_filter(f)
            {
                return Ok(KeywordAbility::HexproofFrom(HexproofQuality::CardType(ct)));
            }
            Err(LoweringError::Unsupported("v2:keyword:hexproof-filter"))
        }
        _ => Err(LoweringError::Unsupported("v2:keyword:hexproof-param")),
    }
}

fn lower_protection_from(
    param: &v2::KeywordParam,
    ctx: &LowerCtx<'_>,
) -> LowerResult<KeywordAbility> {
    match param {
        v2::KeywordParam::Everything => Ok(KeywordAbility::ProtectionFrom(
            ProtectionFromQuality::Everything,
        )),
        v2::KeywordParam::Color(mask) => {
            let color = single_color_from_mask(*mask).ok_or(LoweringError::Unsupported(
                "v2:keyword:protection-multi-color",
            ))?;
            Ok(protection_from_color(color))
        }
        v2::KeywordParam::Filter(f) => {
            if !has_only_basic_quality_fields(f) {
                return Err(LoweringError::Unsupported("v2:keyword:protection-filter"));
            }
            if let Some(arity) = f.color_arity
                && f.all_types.is_empty()
                && f.any_types.is_empty()
                && f.subtypes.is_empty()
                && f.forbidden_subtypes.is_empty()
                && f.any_colors.is_empty()
            {
                return match arity {
                    ast::ColorArity::Multicolored => {
                        Ok(KeywordAbility::ProtectionFromMulticolored)
                    }
                    ast::ColorArity::Monocolored => Ok(KeywordAbility::ProtectionFrom(
                        ProtectionFromQuality::Monocolored,
                    )),
                    ast::ColorArity::Colorless => Ok(KeywordAbility::ProtectionFrom(
                        ProtectionFromQuality::Colorless,
                    )),
                    ast::ColorArity::Colored => {
                        Err(LoweringError::Unsupported("v2:keyword:protection-colored"))
                    }
                };
            }
            if !f.any_colors.is_empty()
                && f.all_types.is_empty()
                && f.any_types.is_empty()
                && f.subtypes.is_empty()
                && f.forbidden_subtypes.is_empty()
                && f.color_arity.is_none()
            {
                let color = single_color_from_mask(f.any_colors).ok_or(
                    LoweringError::Unsupported("v2:keyword:protection-multi-color"),
                )?;
                return Ok(protection_from_color(color));
            }
            let creatures_only = f.all_types == ast::CardTypeMask::CREATURE
                || f.any_types == ast::CardTypeMask::CREATURE;
            let no_other_types = (f.all_types.is_empty() && f.any_types.is_empty())
                || (creatures_only
                    && (f.all_types.0 | f.any_types.0) == ast::CardTypeMask::CREATURE.0);
            if f.subtypes.len() == 1
                && f.forbidden_subtypes.is_empty()
                && f.any_colors.is_empty()
                && f.color_arity.is_none()
                && no_other_types
            {
                let subtype = ctx
                    .lookup_creature_type(&f.subtypes[0])
                    .ok_or(LoweringError::Unsupported("v2:keyword:protection-subtype"))?;
                return Ok(KeywordAbility::ProtectionFromSubtype(SubtypeProtection {
                    subtype,
                    creatures_only,
                    exclude_subtype: false,
                }));
            }
            if f.forbidden_subtypes.len() == 1
                && f.subtypes.is_empty()
                && f.any_colors.is_empty()
                && f.color_arity.is_none()
                && no_other_types
            {
                let subtype = ctx
                    .lookup_creature_type(&f.forbidden_subtypes[0])
                    .ok_or(LoweringError::Unsupported("v2:keyword:protection-subtype"))?;
                return Ok(KeywordAbility::ProtectionFromSubtype(SubtypeProtection {
                    subtype,
                    creatures_only,
                    exclude_subtype: true,
                }));
            }
            if f.subtypes.is_empty()
                && f.forbidden_subtypes.is_empty()
                && f.any_colors.is_empty()
                && f.color_arity.is_none()
                && let Some(ct) = single_card_type_from_filter(f)
            {
                return Ok(KeywordAbility::ProtectionFrom(
                    ProtectionFromQuality::CardType(ct),
                ));
            }
            Err(LoweringError::Unsupported("v2:keyword:protection-filter"))
        }
        _ => Err(LoweringError::Unsupported("v2:keyword:protection-param")),
    }
}

fn lower_landwalk_filter(f: &v2::ObjectFilter) -> LowerResult<KeywordAbility> {
    if !f.forbidden_types.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || f.controller.is_some()
        || f.owner.is_some()
        || !f.zones.is_empty()
        || f.combat.is_some()
        || f.tapped.is_some()
        || f.token.is_some()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || !f.flags.is_empty()
        || !f.predicates.is_empty()
    {
        return Err(LoweringError::Unsupported("v2:keyword:landwalk-filter"));
    }

    let types = f.all_types.0 | f.any_types.0;
    let is_land_or_empty = types == 0 || types == ast::CardTypeMask::LAND.0;
    let is_artifact_land =
        types == (ast::CardTypeMask::ARTIFACT.0 | ast::CardTypeMask::LAND.0);

    if is_artifact_land
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
    {
        return Ok(KeywordAbility::ArtifactLandwalk);
    }

    if !is_land_or_empty {
        return Err(LoweringError::Unsupported("v2:keyword:landwalk-types"));
    }

    if f.subtypes.is_empty() {
        if f.forbidden_supertypes == ast::SupertypeMask::BASIC && f.required_supertypes.is_empty()
        {
            return Ok(KeywordAbility::NonbasicLandwalk);
        }
        if f.required_supertypes == ast::SupertypeMask::LEGENDARY
            && f.forbidden_supertypes.is_empty()
        {
            return Ok(KeywordAbility::LegendaryLandwalk);
        }
        if f.required_supertypes == ast::SupertypeMask::SNOW && f.forbidden_supertypes.is_empty() {
            return Ok(KeywordAbility::SnowLandwalk);
        }
        return Err(LoweringError::Unsupported("v2:keyword:landwalk-supertype"));
    }

    if f.subtypes.len() == 1 && f.forbidden_supertypes.is_empty() {
        let Some(lt) = LowerCtx::lookup_land_type(&f.subtypes[0]) else {
            return Err(LoweringError::Unsupported("v2:keyword:landwalk-subtype"));
        };
        let snow = f.required_supertypes == ast::SupertypeMask::SNOW;
        if !snow && !f.required_supertypes.is_empty() {
            return Err(LoweringError::Unsupported("v2:keyword:landwalk-supertype"));
        }
        return match (snow, lt) {
            (false, spellwake_engine::land::LandType::Plains) => Ok(KeywordAbility::Plainswalk),
            (false, spellwake_engine::land::LandType::Island) => Ok(KeywordAbility::Islandwalk),
            (false, spellwake_engine::land::LandType::Swamp) => Ok(KeywordAbility::Swampwalk),
            (false, spellwake_engine::land::LandType::Mountain) => {
                Ok(KeywordAbility::Mountainwalk)
            }
            (false, spellwake_engine::land::LandType::Forest) => Ok(KeywordAbility::Forestwalk),
            (false, spellwake_engine::land::LandType::Desert) => Ok(KeywordAbility::Desertwalk),
            (true, spellwake_engine::land::LandType::Forest) => {
                Ok(KeywordAbility::SnowForestwalk)
            }
            (true, spellwake_engine::land::LandType::Swamp) => Ok(KeywordAbility::SnowSwampwalk),
            _ => Err(LoweringError::Unsupported("v2:keyword:landwalk-subtype")),
        };
    }

    Err(LoweringError::Unsupported("v2:keyword:landwalk-filter"))
}

fn is_controls_self_cond(cond: &v2::ConditionExpr) -> bool {
    match cond {
        v2::ConditionExpr::Flag(v2::ConditionFlag::ControlsSelf) => true,
        v2::ConditionExpr::ControlsSubject {
            player: v2::PlayerRef::You,
            subject,
        } => matches!(subject.as_ref(), v2::SubjectRef::SelfObject),
        _ => false,
    }
}

fn is_source_tapped_cond(cond: &v2::ConditionExpr) -> bool {
    matches!(
        cond,
        v2::ConditionExpr::SubjectState {
            subject,
            state: v2::ObjectStateKind::Tapped,
        } if matches!(subject.as_ref(), v2::SubjectRef::SelfObject)
    )
}

/// Lower a [`v2::Duration`] into an engine [`ResolvedDuration`].
pub(crate) fn lower_duration(dur: &v2::Duration) -> LowerResult<ResolvedDuration> {
    match dur {
        v2::Duration::UntilEndOfTurn => Ok(ResolvedDuration::UntilEndOfTurn),
        v2::Duration::UntilEndOfCombat => Ok(ResolvedDuration::UntilEndOfCombat),
        v2::Duration::UntilYourNextTurn => Ok(ResolvedDuration::UntilControllerNextTurn),
        v2::Duration::UntilEndOfNextTurn => Ok(ResolvedDuration::ThroughControllerNextTurn),
        v2::Duration::UntilSourceLeaves => Ok(ResolvedDuration::WhileAbilitySourceOnBattlefield),
        v2::Duration::Permanent | v2::Duration::ForTheRestOfTheGame => {
            Ok(ResolvedDuration::NoStatedDuration)
        }
        v2::Duration::ForAsLongAs(cond) => {
            if is_controls_self_cond(cond) {
                return Ok(ResolvedDuration::WhileControllingAbilitySource);
            }
            if is_source_tapped_cond(cond) {
                return Ok(ResolvedDuration::WhileAbilitySourceTapped);
            }
            if let v2::ConditionExpr::And(a, b) = cond.as_ref()
                && ((is_controls_self_cond(a) && is_source_tapped_cond(b))
                    || (is_source_tapped_cond(a) && is_controls_self_cond(b)))
            {
                return Ok(ResolvedDuration::WhileControllingTappedAbilitySource);
            }
            if let v2::ConditionExpr::InZone { subject, zone } = cond.as_ref()
                && matches!(subject.as_ref(), v2::SubjectRef::SelfObject)
                && zone.zone == v2::ZoneKind::Battlefield
            {
                return Ok(ResolvedDuration::WhileAbilitySourceOnBattlefield);
            }
            Err(LoweringError::Unsupported("v2:duration:for-as-long-as"))
        }
        v2::Duration::Instant => Err(LoweringError::Unsupported("v2:duration:instant")),
        v2::Duration::UntilYourNextEndStep => {
            Err(LoweringError::Unsupported("v2:duration:until-next-end-step"))
        }
        v2::Duration::UntilYourNextUpkeep => {
            Err(LoweringError::Unsupported("v2:duration:until-next-upkeep"))
        }
        v2::Duration::ForAsLongAsExiled => {
            Err(LoweringError::Unsupported("v2:duration:for-as-long-as-exiled"))
        }
    }
}

/// Lower a [`v2::Duration`] into an [`ImpulsePlayDuration`].
pub(crate) fn lower_impulse_duration(dur: &v2::Duration) -> LowerResult<ImpulsePlayDuration> {
    match dur {
        v2::Duration::UntilEndOfTurn | v2::Duration::Instant => Ok(ImpulsePlayDuration::ThisTurn),
        v2::Duration::UntilEndOfNextTurn => Ok(ImpulsePlayDuration::UntilEndOfNextTurn),
        v2::Duration::UntilYourNextEndStep => Ok(ImpulsePlayDuration::UntilNextEndStep),
        v2::Duration::UntilYourNextUpkeep => Ok(ImpulsePlayDuration::UntilNextUpkeep),
        v2::Duration::ForAsLongAsExiled
        | v2::Duration::ForTheRestOfTheGame
        | v2::Duration::Permanent => Ok(ImpulsePlayDuration::WhileExiled),
        v2::Duration::UntilYourNextTurn => Err(LoweringError::Unsupported(
            "v2:impulse-duration:until-your-next-turn",
        )),
        v2::Duration::UntilEndOfCombat => Err(LoweringError::Unsupported(
            "v2:impulse-duration:until-end-of-combat",
        )),
        v2::Duration::UntilSourceLeaves => Err(LoweringError::Unsupported(
            "v2:impulse-duration:until-source-leaves",
        )),
        v2::Duration::ForAsLongAs(_) => Err(LoweringError::Unsupported(
            "v2:impulse-duration:for-as-long-as",
        )),
    }
}

/// Lower an [`ast::TurnStepAst`] into a concrete engine [`Step`].
pub(crate) fn lower_step(step: ast::TurnStepAst) -> LowerResult<Step> {
    match step {
        ast::TurnStepAst::UntapStep => Ok(Step::Untap),
        ast::TurnStepAst::Upkeep => Ok(Step::Upkeep),
        ast::TurnStepAst::DrawStep => Ok(Step::Draw),
        ast::TurnStepAst::PrecombatMain => Ok(Step::PrecombatMain),
        ast::TurnStepAst::PostcombatMain => Ok(Step::PostcombatMain),
        ast::TurnStepAst::BeginningOfCombat => Ok(Step::BeginningCombat),
        ast::TurnStepAst::EndOfCombat => Ok(Step::EndCombat),
        ast::TurnStepAst::EndStep => Ok(Step::End),
        ast::TurnStepAst::CleanupStep => Ok(Step::Cleanup),
        ast::TurnStepAst::MainPhase => {
            Err(LoweringError::Unsupported("v2:step:generic-main-phase"))
        }
    }
}

/// Lower an [`ast::CompareOp`] into an engine [`Comparison`].
#[must_use]
pub(crate) fn lower_compare_op(op: ast::CompareOp) -> Comparison {
    match op {
        ast::CompareOp::Lt => Comparison::Less,
        ast::CompareOp::Le => Comparison::LessOrEqual,
        ast::CompareOp::Gt => Comparison::Greater,
        ast::CompareOp::Ge => Comparison::GreaterOrEqual,
        ast::CompareOp::Eq => Comparison::Equal,
    }
}

/// Lower a [`v2::PlayerRef`] into a single [`ResolvingPlayer`], registering target capabilities on `acc`.
pub(crate) fn lower_resolving_player(
    player: v2::PlayerRef,
    acc: &mut BodyAcc,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ResolvingPlayer> {
    match player {
        v2::PlayerRef::You => Ok(ResolvingPlayer::Controller),
        v2::PlayerRef::TargetPlayer => {
            acc.register_target(BodyTarget::SINGLE_PLAYER, v2::TargetCount::Exact(1))?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        v2::PlayerRef::TargetOpponent => {
            acc.register_target(BodyTarget::SINGLE_OPPONENT, v2::TargetCount::Exact(1))?;
            Ok(ResolvingPlayer::TargetPlayer)
        }
        v2::PlayerRef::EachPlayer
        | v2::PlayerRef::EachOpponent
        | v2::PlayerRef::EachOtherPlayer => {
            if ctx.in_loop_player {
                Ok(ResolvingPlayer::LoopPlayer)
            } else {
                Err(LoweringError::Unsupported("v2:player:group-as-single"))
            }
        }
        v2::PlayerRef::EachOtherOpponent => {
            Err(LoweringError::Unsupported("v2:player:each-other-opponent"))
        }
        v2::PlayerRef::ThatPlayer => {
            if ctx.in_loop_player {
                Ok(ResolvingPlayer::LoopPlayer)
            } else if acc.target_set {
                match acc.target {
                    BodyTarget::Player { .. } => Ok(ResolvingPlayer::TargetPlayer),
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        ..
                    }
                    | BodyTarget::AnyTarget(_) => Ok(ResolvingPlayer::TargetController),
                    BodyTarget::Object {
                        zone:
                            TargetZone::OwnGraveyard
                            | TargetZone::AnyGraveyard { .. }
                            | TargetZone::Exile { .. },
                        ..
                    } => Ok(ResolvingPlayer::TargetOwner),
                    BodyTarget::Spell { .. } | BodyTarget::StackEntry(_) => {
                        Ok(ResolvingPlayer::TargetSpellController)
                    }
                    BodyTarget::None
                    | BodyTarget::Object {
                        zone: TargetZone::RecentlyDiscardedOrFromLibrary,
                        ..
                    } => {
                        if ctx.in_trigger {
                            Ok(if ctx.trigger_has_participant {
                                ResolvingPlayer::UniqueRelatedPlayer
                            } else {
                                ResolvingPlayer::TriggerSubjectController
                            })
                        } else if let Some(rp) = ctx.last_player_target {
                            Ok(rp)
                        } else {
                            Err(LoweringError::Unsupported("v2:player:unbound-that-player"))
                        }
                    }
                }
            } else if ctx.in_trigger {
                Ok(if ctx.trigger_has_participant {
                    ResolvingPlayer::UniqueRelatedPlayer
                } else {
                    ResolvingPlayer::TriggerSubjectController
                })
            } else if let Some(rp) = ctx.last_player_target {
                Ok(rp)
            } else {
                Err(LoweringError::Unsupported("v2:player:unbound-that-player"))
            }
        }
        v2::PlayerRef::DefendingPlayer => Ok(ResolvingPlayer::DefendingPlayer),
        v2::PlayerRef::ActivePlayer => Ok(ResolvingPlayer::ActivePlayer),
        v2::PlayerRef::EnchantedPlayer => Ok(ResolvingPlayer::EnchantedPlayer),
        v2::PlayerRef::ItsController
        | v2::PlayerRef::ThatObjectController
        | v2::PlayerRef::ThatSpellController => {
            if acc.target_set {
                match acc.target {
                    BodyTarget::Spell { .. } | BodyTarget::StackEntry(_) => {
                        return Ok(ResolvingPlayer::TargetSpellController);
                    }
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        ..
                    }
                    | BodyTarget::AnyTarget(_) => {
                        return Ok(ResolvingPlayer::TargetController);
                    }
                    BodyTarget::Object {
                        zone:
                            TargetZone::OwnGraveyard
                            | TargetZone::AnyGraveyard { .. }
                            | TargetZone::Exile { .. },
                        ..
                    } => {
                        return Ok(ResolvingPlayer::TargetOwner);
                    }
                    _ => {}
                }
            }
            if ctx.in_trigger {
                Ok(ResolvingPlayer::TriggerSubjectController)
            } else {
                Ok(ResolvingPlayer::Controller)
            }
        }
        v2::PlayerRef::ItsOwner => {
            if acc.target_set {
                match acc.target {
                    BodyTarget::Object {
                        zone:
                            TargetZone::Battlefield
                            | TargetZone::OwnGraveyard
                            | TargetZone::AnyGraveyard { .. }
                            | TargetZone::Exile { .. },
                        ..
                    }
                    | BodyTarget::Spell {
                        include_abilities: false,
                        ..
                    }
                    | BodyTarget::AnyTarget(_) => return Ok(ResolvingPlayer::TargetOwner),
                    _ => {}
                }
            }
            if ctx.in_trigger {
                Ok(ResolvingPlayer::TriggerSubjectOwner)
            } else {
                Ok(ResolvingPlayer::SourceOwner)
            }
        }
        v2::PlayerRef::ChosenPlayer => Ok(ResolvingPlayer::ChosenPlayer),
        v2::PlayerRef::AnyOpponent => Err(LoweringError::Unsupported("v2:player:any-opponent")),
        v2::PlayerRef::AnyPlayer => Err(LoweringError::Unsupported("v2:player:any-player")),
        v2::PlayerRef::LowestLifePlayer => {
            Err(LoweringError::Unsupported("v2:player:lowest-life"))
        }
        v2::PlayerRef::HighestLifePlayer => {
            Err(LoweringError::Unsupported("v2:player:highest-life"))
        }
        v2::PlayerRef::MostObjectsPlayer => {
            Err(LoweringError::Unsupported("v2:player:most-objects"))
        }
        v2::PlayerRef::SearchedLibraryPlayer => {
            Err(LoweringError::Unsupported("v2:player:searched-library"))
        }
    }
}

/// Lower a [`v2::PlayerRef`] into [`AffectedPlayers`], registering target capabilities on `acc`.
pub(crate) fn lower_affected_players(
    player: v2::PlayerRef,
    acc: &mut BodyAcc,
    ctx: &LowerCtx<'_>,
) -> LowerResult<AffectedPlayers> {
    match player {
        v2::PlayerRef::EachPlayer => Ok(AffectedPlayers::EachPlayer),
        v2::PlayerRef::EachOpponent => Ok(AffectedPlayers::EachOpponent),
        v2::PlayerRef::EachOtherPlayer => Ok(AffectedPlayers::EachOtherPlayer),
        v2::PlayerRef::EachOtherOpponent => {
            Err(LoweringError::Unsupported("v2:player:each-other-opponent"))
        }
        other => Ok(AffectedPlayers::One(lower_resolving_player(other, acc, ctx)?)),
    }
}

/// Lower a [`v2::PlayerRef`] into a composable [`PlayerSelector`].
pub(crate) fn lower_player_ref(
    player: v2::PlayerRef,
    acc: &mut BodyAcc,
    ctx: &LowerCtx<'_>,
) -> LowerResult<PlayerSelector> {
    match player {
        v2::PlayerRef::EachPlayer => Ok(PlayerSelector::Group(AffectedPlayers::EachPlayer)),
        v2::PlayerRef::EachOpponent => Ok(PlayerSelector::Group(AffectedPlayers::EachOpponent)),
        v2::PlayerRef::EachOtherPlayer => {
            Ok(PlayerSelector::Group(AffectedPlayers::EachOtherPlayer))
        }
        v2::PlayerRef::EachOtherOpponent => {
            Err(LoweringError::Unsupported("v2:player:each-other-opponent"))
        }
        v2::PlayerRef::ItsController | v2::PlayerRef::ThatObjectController => {
            if let Some(binding) = ctx.in_loop_object {
                return Ok(PlayerSelector::ControllersOf(Box::new(ObjectQuery::Bound(
                    binding,
                ))));
            }
            if acc.target_set
                && matches!(
                    acc.target,
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield
                            | TargetZone::OwnGraveyard
                            | TargetZone::AnyGraveyard { .. }
                            | TargetZone::Exile { .. },
                        ..
                    } | BodyTarget::Spell { .. }
                        | BodyTarget::StackEntry(_)
                        | BodyTarget::AnyTarget(_)
                )
            {
                return Ok(PlayerSelector::Resolving(lower_resolving_player(
                    player, acc, ctx,
                )?));
            }
            if let Some(binding) = ctx.last_moved_binding {
                return Ok(PlayerSelector::ControllersOf(Box::new(ObjectQuery::Bound(
                    binding,
                ))));
            }
            Ok(PlayerSelector::Resolving(lower_resolving_player(
                player, acc, ctx,
            )?))
        }
        v2::PlayerRef::ItsOwner => {
            if let Some(binding) = ctx.in_loop_object {
                return Ok(PlayerSelector::OwnersOf(Box::new(ObjectQuery::Bound(
                    binding,
                ))));
            }
            if acc.target_set
                && matches!(
                    acc.target,
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield
                            | TargetZone::OwnGraveyard
                            | TargetZone::AnyGraveyard { .. }
                            | TargetZone::Exile { .. },
                        ..
                    } | BodyTarget::Spell {
                        include_abilities: false,
                        ..
                    } | BodyTarget::AnyTarget(_)
                )
            {
                return Ok(PlayerSelector::Resolving(lower_resolving_player(
                    player, acc, ctx,
                )?));
            }
            if let Some(binding) = ctx.last_moved_binding {
                return Ok(PlayerSelector::OwnersOf(Box::new(ObjectQuery::Bound(
                    binding,
                ))));
            }
            Ok(PlayerSelector::Resolving(lower_resolving_player(
                player, acc, ctx,
            )?))
        }
        v2::PlayerRef::ThatPlayer => {
            if let Ok(rp) = lower_resolving_player(player, acc, ctx) {
                return Ok(PlayerSelector::Resolving(rp));
            }
            if let Some(binding) = ctx.in_loop_object.or(ctx.last_moved_binding) {
                return Ok(PlayerSelector::ControllersOf(Box::new(ObjectQuery::Bound(
                    binding,
                ))));
            }
            Err(LoweringError::Unsupported("v2:player:unbound-that-player"))
        }
        other => Ok(PlayerSelector::Resolving(lower_resolving_player(
            other, acc, ctx,
        )?)),
    }
}

/// Lower a [`v2::PlayerRef`] in a pure expression/condition context without target registration.
pub(crate) fn lower_world_resolving_player(
    player: v2::PlayerRef,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ResolvingPlayer> {
    match player {
        v2::PlayerRef::You => Ok(ResolvingPlayer::Controller),
        v2::PlayerRef::TargetPlayer | v2::PlayerRef::TargetOpponent => {
            Ok(ResolvingPlayer::TargetPlayer)
        }
        v2::PlayerRef::DefendingPlayer => Ok(ResolvingPlayer::DefendingPlayer),
        v2::PlayerRef::ActivePlayer => Ok(ResolvingPlayer::ActivePlayer),
        v2::PlayerRef::EnchantedPlayer => Ok(ResolvingPlayer::EnchantedPlayer),
        v2::PlayerRef::ChosenPlayer => Ok(ResolvingPlayer::SourceChosenPlayer),
        v2::PlayerRef::ThatPlayer => {
            if ctx.in_loop_player {
                Ok(ResolvingPlayer::LoopPlayer)
            } else if ctx.in_trigger {
                Ok(if ctx.trigger_has_participant {
                    ResolvingPlayer::UniqueRelatedPlayer
                } else {
                    ResolvingPlayer::TriggerSubjectController
                })
            } else if let Some(rp) = ctx.last_player_target {
                Ok(rp)
            } else {
                Ok(ResolvingPlayer::TargetPlayer)
            }
        }
        v2::PlayerRef::ItsController | v2::PlayerRef::ThatObjectController => {
            if ctx.has_object_target {
                Ok(ResolvingPlayer::TargetController)
            } else if ctx.in_trigger {
                Ok(ResolvingPlayer::TriggerSubjectController)
            } else {
                Ok(ResolvingPlayer::Controller)
            }
        }
        v2::PlayerRef::ItsOwner => {
            if ctx.has_object_target {
                Ok(ResolvingPlayer::TargetOwner)
            } else if ctx.in_trigger {
                Ok(ResolvingPlayer::TriggerSubjectOwner)
            } else {
                Ok(ResolvingPlayer::SourceOwner)
            }
        }
        v2::PlayerRef::ThatSpellController => {
            if ctx.has_object_target {
                Ok(ResolvingPlayer::TargetSpellController)
            } else if ctx.in_trigger {
                Ok(ResolvingPlayer::TriggerSubjectController)
            } else {
                Ok(ResolvingPlayer::Controller)
            }
        }
        v2::PlayerRef::EachPlayer
        | v2::PlayerRef::EachOpponent
        | v2::PlayerRef::EachOtherPlayer => {
            if ctx.in_loop_player {
                Ok(ResolvingPlayer::LoopPlayer)
            } else {
                Err(LoweringError::Unsupported("v2:player:world-group-as-single"))
            }
        }
        v2::PlayerRef::EachOtherOpponent => {
            Err(LoweringError::Unsupported("v2:player:each-other-opponent"))
        }
        v2::PlayerRef::AnyOpponent => Err(LoweringError::Unsupported("v2:player:any-opponent")),
        v2::PlayerRef::AnyPlayer => Err(LoweringError::Unsupported("v2:player:any-player")),
        v2::PlayerRef::LowestLifePlayer => {
            Err(LoweringError::Unsupported("v2:player:lowest-life"))
        }
        v2::PlayerRef::HighestLifePlayer => {
            Err(LoweringError::Unsupported("v2:player:highest-life"))
        }
        v2::PlayerRef::MostObjectsPlayer => {
            Err(LoweringError::Unsupported("v2:player:most-objects"))
        }
        v2::PlayerRef::SearchedLibraryPlayer => {
            Err(LoweringError::Unsupported("v2:player:searched-library"))
        }
    }
}

/// Lower a [`v2::PlayerRef`] into [`AffectedPlayers`] in an expression/condition context.
pub(crate) fn lower_world_affected_players(
    player: v2::PlayerRef,
    ctx: &LowerCtx<'_>,
) -> LowerResult<AffectedPlayers> {
    match player {
        v2::PlayerRef::EachPlayer | v2::PlayerRef::AnyPlayer => Ok(AffectedPlayers::EachPlayer),
        v2::PlayerRef::EachOpponent | v2::PlayerRef::AnyOpponent => {
            Ok(AffectedPlayers::EachOpponent)
        }
        v2::PlayerRef::EachOtherPlayer => Ok(AffectedPlayers::EachOtherPlayer),
        v2::PlayerRef::EachOtherOpponent => {
            Err(LoweringError::Unsupported("v2:player:each-other-opponent"))
        }
        other => Ok(AffectedPlayers::One(lower_world_resolving_player(
            other, ctx,
        )?)),
    }
}

/// Lower an [`ast::ControllerRelation`] into a [`ControllerFilter`].
pub(crate) fn lower_static_controller(
    rel: ast::ControllerRelation,
) -> LowerResult<ControllerFilter> {
    match rel {
        ast::ControllerRelation::You => Ok(ControllerFilter::You),
        ast::ControllerRelation::NotYou => Ok(ControllerFilter::NotYou),
        ast::ControllerRelation::Opponent | ast::ControllerRelation::Opponents => {
            Ok(ControllerFilter::Opponent)
        }
        ast::ControllerRelation::TargetOpponent => Ok(ControllerFilter::TargetOpponent),
        ast::ControllerRelation::TargetPlayer => Ok(ControllerFilter::TargetPlayer),
        ast::ControllerRelation::ThatPlayer => Ok(ControllerFilter::RelatedPlayer),
        ast::ControllerRelation::DefendingPlayer => Ok(ControllerFilter::DefendingPlayer),
        ast::ControllerRelation::ActivePlayer => Ok(ControllerFilter::ActivePlayer),
        ast::ControllerRelation::OwnerYou => Ok(ControllerFilter::OwnerYou),
        ast::ControllerRelation::OwnerOpponent => Ok(ControllerFilter::OwnerOpponent),
        ast::ControllerRelation::YourTeam => {
            Err(LoweringError::Unsupported("v2:controller:your-team"))
        }
        ast::ControllerRelation::ItsController => {
            Err(LoweringError::Unsupported("v2:controller:its-controller"))
        }
        ast::ControllerRelation::OwnerThatPlayer => {
            Err(LoweringError::Unsupported("v2:controller:owner-that-player"))
        }
        ast::ControllerRelation::ChosenPlayer => {
            Err(LoweringError::Unsupported("v2:controller:chosen-player"))
        }
        ast::ControllerRelation::ThatPlayerOrPlaneswalkerController => Err(
            LoweringError::Unsupported("v2:controller:player-or-planeswalker"),
        ),
        ast::ControllerRelation::NextPlayerInDirection => {
            Err(LoweringError::Unsupported("v2:controller:next-direction"))
        }
        ast::ControllerRelation::PlayerToLeft => {
            Err(LoweringError::Unsupported("v2:controller:player-to-left"))
        }
        ast::ControllerRelation::PlayerToRight => {
            Err(LoweringError::Unsupported("v2:controller:player-to-right"))
        }
    }
}

fn lower_zone_owner_selector(
    owner: v2::ZoneOwner,
    acc: &mut BodyAcc,
    ctx: &LowerCtx<'_>,
) -> LowerResult<PlayerSelector> {
    match owner {
        v2::ZoneOwner::Default | v2::ZoneOwner::You => {
            Ok(PlayerSelector::Resolving(ResolvingPlayer::Controller))
        }
        v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents => {
            Ok(PlayerSelector::Group(AffectedPlayers::EachOpponent))
        }
        v2::ZoneOwner::EachPlayer | v2::ZoneOwner::Any | v2::ZoneOwner::All => {
            Ok(PlayerSelector::Group(AffectedPlayers::EachPlayer))
        }
        v2::ZoneOwner::TargetOpponent => {
            acc.register_target(BodyTarget::SINGLE_OPPONENT, v2::TargetCount::Exact(1))?;
            Ok(PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer))
        }
        v2::ZoneOwner::TargetPlayer => {
            acc.register_target(BodyTarget::SINGLE_PLAYER, v2::TargetCount::Exact(1))?;
            Ok(PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer))
        }
        v2::ZoneOwner::DefendingPlayer => {
            Ok(PlayerSelector::Resolving(ResolvingPlayer::DefendingPlayer))
        }
        v2::ZoneOwner::ThatPlayer => lower_player_ref(v2::PlayerRef::ThatPlayer, acc, ctx),
        v2::ZoneOwner::ItsOwner
        | v2::ZoneOwner::TheirOwners
        | v2::ZoneOwner::ThatCreatureOwner => lower_player_ref(v2::PlayerRef::ItsOwner, acc, ctx),
    }
}

/// Lower a [`v2::ZoneRef`] into a [`QueryZone`], registering any target player on `acc`.
pub(crate) fn lower_zone_ref(
    zone: v2::ZoneRef,
    acc: &mut BodyAcc,
    ctx: &LowerCtx<'_>,
) -> LowerResult<QueryZone> {
    match zone.zone {
        v2::ZoneKind::Battlefield => match zone.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer => Ok(QueryZone::Battlefield),
            other => Ok(QueryZone::BattlefieldControlledBy(Box::new(
                lower_zone_owner_selector(other, acc, ctx)?,
            ))),
        },
        v2::ZoneKind::Hand => Ok(QueryZone::Hand(Box::new(lower_zone_owner_selector(
            zone.owner, acc, ctx,
        )?))),
        v2::ZoneKind::Graveyard => Ok(QueryZone::Graveyard(Box::new(lower_zone_owner_selector(
            zone.owner, acc, ctx,
        )?))),
        v2::ZoneKind::Library => Ok(QueryZone::Library(Box::new(lower_zone_owner_selector(
            zone.owner, acc, ctx,
        )?))),
        v2::ZoneKind::Exile => match zone.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer => Ok(QueryZone::Exile),
            other => Ok(QueryZone::ExileOwnedBy(Box::new(
                lower_zone_owner_selector(other, acc, ctx)?,
            ))),
        },
        v2::ZoneKind::Stack => match zone.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer => Ok(QueryZone::Stack),
            _ => Err(LoweringError::Unsupported("v2:zone:scoped-stack")),
        },
        v2::ZoneKind::CommandZone => match zone.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::You
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer => Ok(QueryZone::Command),
            _ => Err(LoweringError::Unsupported("v2:zone:scoped-command")),
        },
        v2::ZoneKind::OutsideTheGame => {
            Err(LoweringError::Unsupported("v2:zone:outside-the-game"))
        }
    }
}

/// Lower a [`v2::ZoneRef`] into a [`QueryZone`] without target registration (for pure queries).
pub(crate) fn lower_world_query_zone(
    zone: v2::ZoneRef,
    ctx: &LowerCtx<'_>,
) -> LowerResult<QueryZone> {
    let owner_sel = |owner: v2::ZoneOwner| -> LowerResult<PlayerSelector> {
        match owner {
            v2::ZoneOwner::Default | v2::ZoneOwner::You => {
                Ok(PlayerSelector::Resolving(ResolvingPlayer::Controller))
            }
            v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents => {
                Ok(PlayerSelector::Group(AffectedPlayers::EachOpponent))
            }
            v2::ZoneOwner::EachPlayer | v2::ZoneOwner::Any | v2::ZoneOwner::All => {
                Ok(PlayerSelector::Group(AffectedPlayers::EachPlayer))
            }
            v2::ZoneOwner::TargetPlayer | v2::ZoneOwner::TargetOpponent => {
                Ok(PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer))
            }
            v2::ZoneOwner::DefendingPlayer => {
                Ok(PlayerSelector::Resolving(ResolvingPlayer::DefendingPlayer))
            }
            v2::ZoneOwner::ThatPlayer => Ok(PlayerSelector::Resolving(
                lower_world_resolving_player(v2::PlayerRef::ThatPlayer, ctx)?,
            )),
            v2::ZoneOwner::ItsOwner
            | v2::ZoneOwner::TheirOwners
            | v2::ZoneOwner::ThatCreatureOwner => Ok(PlayerSelector::Resolving(
                lower_world_resolving_player(v2::PlayerRef::ItsOwner, ctx)?,
            )),
        }
    };
    match zone.zone {
        v2::ZoneKind::Battlefield => match zone.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer => Ok(QueryZone::Battlefield),
            other => Ok(QueryZone::BattlefieldControlledBy(Box::new(owner_sel(
                other,
            )?))),
        },
        v2::ZoneKind::Hand => Ok(QueryZone::Hand(Box::new(owner_sel(zone.owner)?))),
        v2::ZoneKind::Graveyard => Ok(QueryZone::Graveyard(Box::new(owner_sel(zone.owner)?))),
        v2::ZoneKind::Library => Ok(QueryZone::Library(Box::new(owner_sel(zone.owner)?))),
        v2::ZoneKind::Exile => match zone.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer => Ok(QueryZone::Exile),
            other => Ok(QueryZone::ExileOwnedBy(Box::new(owner_sel(other)?))),
        },
        v2::ZoneKind::Stack => match zone.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer => Ok(QueryZone::Stack),
            _ => Err(LoweringError::Unsupported("v2:zone:scoped-stack")),
        },
        v2::ZoneKind::CommandZone => match zone.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::You
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer => Ok(QueryZone::Command),
            _ => Err(LoweringError::Unsupported("v2:zone:scoped-command")),
        },
        v2::ZoneKind::OutsideTheGame => {
            Err(LoweringError::Unsupported("v2:zone:outside-the-game"))
        }
    }
}

/// Lower a [`v2::ZoneRef`] into a [`ValueZone`] for [`EngineValueExpr::Count`].
pub(crate) fn lower_value_zone(zone: v2::ZoneRef, ctx: &LowerCtx<'_>) -> LowerResult<ValueZone> {
    let single_owner = |owner: v2::ZoneOwner| -> LowerResult<ResolvingPlayer> {
        match owner {
            v2::ZoneOwner::Default | v2::ZoneOwner::You => Ok(ResolvingPlayer::Controller),
            v2::ZoneOwner::TargetPlayer | v2::ZoneOwner::TargetOpponent => {
                Ok(ResolvingPlayer::TargetPlayer)
            }
            v2::ZoneOwner::DefendingPlayer => Ok(ResolvingPlayer::DefendingPlayer),
            v2::ZoneOwner::ThatPlayer => {
                lower_world_resolving_player(v2::PlayerRef::ThatPlayer, ctx)
            }
            v2::ZoneOwner::ItsOwner
            | v2::ZoneOwner::TheirOwners
            | v2::ZoneOwner::ThatCreatureOwner => {
                lower_world_resolving_player(v2::PlayerRef::ItsOwner, ctx)
            }
            v2::ZoneOwner::Opponent
            | v2::ZoneOwner::Opponents
            | v2::ZoneOwner::EachPlayer
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All => {
                Err(LoweringError::Unsupported("v2:value-zone:ambiguous-owner"))
            }
        }
    };

    match zone.zone {
        v2::ZoneKind::Battlefield => Ok(ValueZone::Battlefield),
        v2::ZoneKind::Graveyard => match zone.owner {
            v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::EachPlayer => {
                Ok(ValueZone::AllGraveyards)
            }
            other => Ok(ValueZone::Graveyard(single_owner(other)?)),
        },
        v2::ZoneKind::Hand => match zone.owner {
            v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::EachPlayer => {
                Ok(ValueZone::AllHands)
            }
            v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents => Ok(ValueZone::OpponentsHands),
            other => Ok(ValueZone::Hand(single_owner(other)?)),
        },
        v2::ZoneKind::Library => Ok(ValueZone::Library(single_owner(zone.owner)?)),
        v2::ZoneKind::Exile => match zone.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer => Ok(ValueZone::Exile(None)),
            other => Ok(ValueZone::Exile(Some(single_owner(other)?))),
        },
        v2::ZoneKind::CommandZone | v2::ZoneKind::Stack | v2::ZoneKind::OutsideTheGame => {
            Err(LoweringError::Unsupported("v2:value-zone:unsupported-zone"))
        }
    }
}

/// Lower a [`v2::ZoneKind`] into a [`SourceZoneKind`].
pub(crate) fn lower_zone_kind_to_source_zone_kind(
    zone: v2::ZoneKind,
) -> LowerResult<SourceZoneKind> {
    match zone {
        v2::ZoneKind::Battlefield => Ok(SourceZoneKind::Battlefield),
        v2::ZoneKind::Graveyard => Ok(SourceZoneKind::Graveyard),
        v2::ZoneKind::Hand => Ok(SourceZoneKind::Hand),
        v2::ZoneKind::Exile => Ok(SourceZoneKind::Exile),
        v2::ZoneKind::CommandZone => Ok(SourceZoneKind::Command),
        v2::ZoneKind::Stack => Ok(SourceZoneKind::Stack),
        v2::ZoneKind::Library => Ok(SourceZoneKind::Library),
        v2::ZoneKind::OutsideTheGame => {
            Err(LoweringError::Unsupported("v2:source-zone:outside-the-game"))
        }
    }
}

/// Lower a [`v2::ZoneRef`] into a [`SourceZoneFilter`].
pub(crate) fn lower_source_zone_filter(zone: v2::ZoneRef) -> LowerResult<SourceZoneFilter> {
    match (zone.zone, zone.owner) {
        (v2::ZoneKind::Hand, v2::ZoneOwner::Default | v2::ZoneOwner::You) => {
            Ok(SourceZoneFilter::Hand)
        }
        (v2::ZoneKind::Hand, v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents) => {
            Ok(SourceZoneFilter::OpponentHand)
        }
        (v2::ZoneKind::Hand, v2::ZoneOwner::TargetPlayer | v2::ZoneOwner::TargetOpponent) => {
            Ok(SourceZoneFilter::TargetPlayerHand)
        }
        (v2::ZoneKind::Graveyard, v2::ZoneOwner::Default | v2::ZoneOwner::You) => {
            Ok(SourceZoneFilter::Graveyard)
        }
        (v2::ZoneKind::Graveyard, v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents) => {
            Ok(SourceZoneFilter::OpponentGraveyard)
        }
        (
            v2::ZoneKind::Graveyard,
            v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::EachPlayer,
        ) => Ok(SourceZoneFilter::AnyGraveyard),
        (
            v2::ZoneKind::Graveyard,
            v2::ZoneOwner::TargetPlayer | v2::ZoneOwner::TargetOpponent,
        ) => Ok(SourceZoneFilter::TargetPlayerGraveyard),
        (v2::ZoneKind::Battlefield, v2::ZoneOwner::You) => Ok(SourceZoneFilter::Battlefield),
        (
            v2::ZoneKind::Battlefield,
            v2::ZoneOwner::Default
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer,
        ) => Ok(SourceZoneFilter::AnyBattlefield),
        (
            v2::ZoneKind::Exile,
            v2::ZoneOwner::Default
            | v2::ZoneOwner::You
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All
            | v2::ZoneOwner::EachPlayer,
        ) => Ok(SourceZoneFilter::Exile),
        (v2::ZoneKind::Exile, v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents) => {
            Ok(SourceZoneFilter::OpponentExile)
        }
        (v2::ZoneKind::Library, v2::ZoneOwner::Default | v2::ZoneOwner::You) => {
            Ok(SourceZoneFilter::YourLibrary)
        }
        (
            v2::ZoneKind::CommandZone,
            v2::ZoneOwner::Default
            | v2::ZoneOwner::You
            | v2::ZoneOwner::Any
            | v2::ZoneOwner::All,
        ) => Ok(SourceZoneFilter::CommandZone),
        _ => Err(LoweringError::Unsupported("v2:source-zone:unsupported")),
    }
}

/// Lower a [`v2::ZoneDest`] and [`v2::EntryModifiers`] into a toolbox [`EngineZoneDest`].
pub(crate) fn lower_zone_dest(
    dest: &v2::ZoneDest,
    mods: &v2::EntryModifiers,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineZoneDest> {
    match dest.zone {
        v2::ZoneKind::Battlefield => {
            if dest.library_position.is_some() || dest.library_order.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:zone-dest:library-opts-on-battlefield",
                ));
            }
            let controller = match dest.under_control.unwrap_or(dest.owner) {
                v2::ZoneOwner::Default | v2::ZoneOwner::You => {
                    ZoneController::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::Controller,
                    )))
                }
                v2::ZoneOwner::ItsOwner
                | v2::ZoneOwner::TheirOwners
                | v2::ZoneOwner::ThatCreatureOwner => ZoneController::Owner,
                v2::ZoneOwner::TargetPlayer => {
                    acc.register_target(BodyTarget::SINGLE_PLAYER, v2::TargetCount::Exact(1))?;
                    ZoneController::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::TargetPlayer,
                    )))
                }
                v2::ZoneOwner::TargetOpponent => {
                    acc.register_target(BodyTarget::SINGLE_OPPONENT, v2::TargetCount::Exact(1))?;
                    ZoneController::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::TargetPlayer,
                    )))
                }
                v2::ZoneOwner::ThatPlayer => ZoneController::Player(Box::new(
                    lower_player_ref(v2::PlayerRef::ThatPlayer, acc, ctx)?,
                )),
                v2::ZoneOwner::DefendingPlayer => {
                    ZoneController::Player(Box::new(PlayerSelector::Resolving(
                        ResolvingPlayer::DefendingPlayer,
                    )))
                }
                v2::ZoneOwner::Opponent
                | v2::ZoneOwner::Opponents
                | v2::ZoneOwner::EachPlayer
                | v2::ZoneOwner::Any
                | v2::ZoneOwner::All => {
                    return Err(LoweringError::Unsupported(
                        "v2:zone-dest:ambiguous-bf-controller",
                    ));
                }
            };
            let mut entering_counters = Vec::new();
            if let Some((ref ck, count)) = mods.with_counter {
                let kind = lower_counter_kind(ck)?;
                entering_counters.push((
                    kind,
                    Box::new(EngineValueExpr::literal(RulesInteger::from(i64::from(
                        count,
                    )))),
                ));
            }
            let attach_to = if let Some(ref host) = mods.attached_to {
                Some(Box::new(lower_subject_to_query(host, acc, ctx)?))
            } else {
                None
            };
            let face_down = mods.face_down.then_some(FaceDownKind::Manifest);
            Ok(EngineZoneDest::Battlefield {
                controller,
                tapped: mods.tapped,
                attacking: mods.attacking,
                transformed: mods.transformed,
                face_down,
                entering_counters,
                attach_to,
            })
        }
        v2::ZoneKind::Graveyard => {
            if mods.tapped
                || mods.attacking
                || mods.face_down
                || mods.transformed
                || mods.with_counter.is_some()
                || mods.attached_to.is_some()
                || dest.library_position.is_some()
                || dest.library_order.is_some()
                || dest.under_control.is_some()
            {
                return Err(LoweringError::Unsupported(
                    "v2:zone-dest:invalid-graveyard-modifiers",
                ));
            }
            Ok(EngineZoneDest::Graveyard {
                semantics: GraveyardSemantics::Move,
            })
        }
        v2::ZoneKind::Exile => {
            if mods.tapped
                || mods.attacking
                || mods.transformed
                || mods.with_counter.is_some()
                || mods.attached_to.is_some()
                || dest.library_position.is_some()
                || dest.library_order.is_some()
                || dest.under_control.is_some()
            {
                return Err(LoweringError::Unsupported(
                    "v2:zone-dest:invalid-exile-modifiers",
                ));
            }
            Ok(EngineZoneDest::Exile {
                face_down: mods.face_down,
                viewer: None,
                link_to_source: false,
                delayed_return: None,
                link_group: None,
            })
        }
        v2::ZoneKind::Hand => {
            if mods.tapped
                || mods.attacking
                || mods.face_down
                || mods.transformed
                || mods.with_counter.is_some()
                || mods.attached_to.is_some()
                || dest.library_position.is_some()
                || dest.library_order.is_some()
                || dest.under_control.is_some()
            {
                return Err(LoweringError::Unsupported(
                    "v2:zone-dest:invalid-hand-modifiers",
                ));
            }
            Ok(EngineZoneDest::Hand)
        }
        v2::ZoneKind::Library => {
            if mods.tapped
                || mods.attacking
                || mods.face_down
                || mods.transformed
                || mods.with_counter.is_some()
                || mods.attached_to.is_some()
                || dest.under_control.is_some()
            {
                return Err(LoweringError::Unsupported(
                    "v2:zone-dest:invalid-library-modifiers",
                ));
            }
            let position = match (dest.library_position, dest.library_order) {
                (None | Some(v2::LibraryPosition::Top), None | Some(v2::LibraryOrder::AnyOrder)) => {
                    LibraryOrderPos::Top
                }
                (Some(v2::LibraryPosition::Bottom), Some(v2::LibraryOrder::RandomOrder)) => {
                    LibraryOrderPos::BottomRandom
                }
                (
                    Some(v2::LibraryPosition::Bottom),
                    None | Some(v2::LibraryOrder::AnyOrder),
                ) => LibraryOrderPos::Bottom,
                (
                    Some(v2::LibraryPosition::TopOrBottom),
                    None | Some(v2::LibraryOrder::AnyOrder),
                ) => LibraryOrderPos::ChoiceTopOrBottom {
                    chooser: Box::new(PlayerSelector::Resolving(ResolvingPlayer::Controller)),
                },
                (Some(v2::LibraryPosition::Shuffled), None) => LibraryOrderPos::ShuffleInto,
                (
                    Some(v2::LibraryPosition::FromTop(offset)),
                    None | Some(v2::LibraryOrder::AnyOrder),
                ) => LibraryOrderPos::NthFromTop(Box::new(EngineValueExpr::literal(
                    RulesInteger::from(i64::from(offset)),
                ))),
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:zone-dest:unsupported-library-order",
                    ));
                }
            };
            Ok(EngineZoneDest::Library { position })
        }
        v2::ZoneKind::CommandZone => {
            Err(LoweringError::Unsupported("v2:zone-dest:command-zone"))
        }
        v2::ZoneKind::Stack => Err(LoweringError::Unsupported("v2:zone-dest:stack")),
        v2::ZoneKind::OutsideTheGame => {
            Err(LoweringError::Unsupported("v2:zone-dest:outside-the-game"))
        }
    }
}

fn pure_single_type_count(
    subj: &v2::SubjectRef,
    expected: ast::CardTypeMask,
) -> Option<v2::TargetCount> {
    match subj {
        v2::SubjectRef::TargetObject {
            filter,
            count: count @ (v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1)),
            another: false,
        } if is_plain_single_type_filter(filter, expected) => Some(*count),
        _ => None,
    }
}

fn player_single_target_count(subj: &v2::SubjectRef, opp_only: bool) -> Option<v2::TargetCount> {
    match subj {
        v2::SubjectRef::TargetPlayer {
            opponent_only,
            count: count @ (v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1)),
        } if *opponent_only == opp_only => Some(*count),
        v2::SubjectRef::Player(v2::PlayerRef::TargetPlayer) if !opp_only => {
            Some(v2::TargetCount::Exact(1))
        }
        v2::SubjectRef::Player(v2::PlayerRef::TargetOpponent) if opp_only => {
            Some(v2::TargetCount::Exact(1))
        }
        _ => None,
    }
}

fn merge_unit_target_counts(a: v2::TargetCount, b: v2::TargetCount) -> v2::TargetCount {
    if matches!(a, v2::TargetCount::UpTo(1)) || matches!(b, v2::TargetCount::UpTo(1)) {
        v2::TargetCount::UpTo(1)
    } else {
        v2::TargetCount::Exact(1)
    }
}

fn is_plain_single_type_filter(f: &v2::ObjectFilter, expected: ast::CardTypeMask) -> bool {
    let type_ok = (f.all_types == expected && f.any_types.is_empty())
        || (f.any_types == expected && f.all_types.is_empty());
    type_ok
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
        && !f.any_spell
        && !f.is_ability
}

fn subject_is_targeting(subj: &v2::SubjectRef) -> bool {
    match subj {
        v2::SubjectRef::AnyTarget
        | v2::SubjectRef::TargetPlayer { .. }
        | v2::SubjectRef::TargetObject { .. } => true,
        v2::SubjectRef::Player(p) | v2::SubjectRef::HandOf(p) => {
            matches!(p, v2::PlayerRef::TargetPlayer | v2::PlayerRef::TargetOpponent)
        }
        v2::SubjectRef::Compound(parts) => parts.iter().any(subject_is_targeting),
        _ => false,
    }
}

fn target_is_object_like(target: BodyTarget) -> bool {
    matches!(
        target,
        BodyTarget::Object { .. }
            | BodyTarget::AnyTarget(_)
            | BodyTarget::Spell { .. }
            | BodyTarget::StackEntry(_)
    )
}

/// Lower a [`v2::SubjectRef`] into a [`BodyTarget`] and [`v2::TargetCount`].
///
/// Non-targeting subjects return `Ok((BodyTarget::None, TargetCount::Exact(1)))`.
pub(crate) fn lower_subject_to_target(
    subject: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(BodyTarget, v2::TargetCount)> {
    match subject {
        v2::SubjectRef::AnyTarget => {
            ctx.has_object_target = true;
            Ok((BodyTarget::SINGLE_ANY_TARGET, v2::TargetCount::Exact(1)))
        }
        v2::SubjectRef::TargetPlayer {
            opponent_only,
            count,
        } => Ok((
            BodyTarget::Player {
                opponents_only: *opponent_only,
            },
            *count,
        )),
        v2::SubjectRef::Player(v2::PlayerRef::TargetPlayer)
        | v2::SubjectRef::HandOf(v2::PlayerRef::TargetPlayer) => {
            Ok((BodyTarget::SINGLE_PLAYER, v2::TargetCount::Exact(1)))
        }
        v2::SubjectRef::Player(v2::PlayerRef::TargetOpponent)
        | v2::SubjectRef::HandOf(v2::PlayerRef::TargetOpponent) => {
            Ok((BodyTarget::SINGLE_OPPONENT, v2::TargetCount::Exact(1)))
        }
        v2::SubjectRef::TargetObject {
            filter,
            count,
            another,
        } => {
            let mut effective = filter.as_ref().clone();
            if *another {
                effective.flags = effective.flags.union(v2::FilterFlags::EXCLUDE_SELF);
            }
            let target = lower_target_object_filter(&effective, ctx)?;
            if target_is_object_like(target) {
                ctx.has_object_target = true;
            }
            Ok((target, *count))
        }
        v2::SubjectRef::Compound(parts) => {
            if !parts.iter().any(subject_is_targeting) {
                return Ok((BodyTarget::None, v2::TargetCount::Exact(1)));
            }
            let (target, count) = lower_compound_target(parts, ctx)?;
            if target_is_object_like(target) {
                ctx.has_object_target = true;
            }
            Ok((target, count))
        }
        v2::SubjectRef::SelfObject
        | v2::SubjectRef::Anaphora(_)
        | v2::SubjectRef::Player(_)
        | v2::SubjectRef::BothPairedCreatures
        | v2::SubjectRef::HandOf(_)
        | v2::SubjectRef::OneOfThem(_)
        | v2::SubjectRef::Each(_)
        | v2::SubjectRef::All(_)
        | v2::SubjectRef::Group(_)
        | v2::SubjectRef::Counted { .. }
        | v2::SubjectRef::PlayersDealtCombatDamageBy { .. }
        | v2::SubjectRef::PlayerWhoControls { .. } => {
            Ok((BodyTarget::None, v2::TargetCount::Exact(1)))
        }
    }
}

fn lower_target_object_filter(
    filter: &v2::ObjectFilter,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<BodyTarget> {
    let stack_zone = filter.zones.len() == 1 && filter.zones[0].zone == v2::ZoneKind::Stack;
    if filter.is_ability && !filter.any_spell {
        let only_stack_or_empty = filter.zones.is_empty() || stack_zone;
        if !only_stack_or_empty {
            return Err(LoweringError::Unsupported("v2:target:ability-non-stack"));
        }
        if filter.all_types.is_empty()
            && filter.any_types.is_empty()
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.forbidden_subtypes.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.flags.is_empty()
            && filter.predicates.is_empty()
            && filter.stat_bounds.is_empty()
        {
            return match (filter.controller, filter.color_arity) {
                (Some(ast::ControllerRelation::You), None) => {
                    Ok(BodyTarget::controlled_ability(false))
                }
                (Some(ast::ControllerRelation::You), Some(ast::ColorArity::Colorless)) => {
                    Ok(BodyTarget::controlled_ability(true))
                }
                (None, None) => Ok(BodyTarget::SINGLE_STACK_ENTRY),
                _ => Err(LoweringError::Unsupported("v2:target:filtered-ability")),
            };
        }
        return Err(LoweringError::Unsupported("v2:target:filtered-ability"));
    }

    if filter.any_spell && filter.any_permanent {
        return Err(LoweringError::Unsupported("v2:target:spell-or-permanent"));
    }

    if filter.any_spell || stack_zone {
        let mut spell_f = filter.clone();
        let include_abilities = spell_f.is_ability;
        spell_f.is_ability = false;
        let lowered = lower_spell_filter(&spell_f, ctx)?;
        return Ok(if include_abilities {
            BodyTarget::single_stack_object(lowered)
        } else {
            BodyTarget::single_spell(lowered)
        });
    }

    if filter.zones.len() > 1 {
        return Err(LoweringError::Unsupported("v2:target:multi-zone"));
    }

    if let Some(zone) = filter.zones.first().copied() {
        let mut cleaned = filter.clone();
        cleaned.zones.clear();
        return match zone.zone {
            v2::ZoneKind::Battlefield => {
                apply_zone_owner_to_bf_filter(&mut cleaned, zone.owner)?;
                let obj = lower_object_filter(&cleaned, ctx)?;
                Ok(BodyTarget::filtered_permanent(obj))
            }
            v2::ZoneKind::Graveyard => {
                let obj = lower_object_filter(&cleaned, ctx)?;
                match zone.owner {
                    v2::ZoneOwner::Default | v2::ZoneOwner::You => {
                        Ok(BodyTarget::filtered_own_graveyard(obj))
                    }
                    v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents => {
                        Ok(BodyTarget::filtered_any_graveyard(obj, true))
                    }
                    v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::EachPlayer => {
                        Ok(BodyTarget::filtered_any_graveyard(obj, false))
                    }
                    _ => Err(LoweringError::Unsupported("v2:target:scoped-graveyard")),
                }
            }
            v2::ZoneKind::Exile => {
                let mut obj = lower_object_filter(&cleaned, ctx)?;
                match zone.owner {
                    v2::ZoneOwner::Default
                    | v2::ZoneOwner::Any
                    | v2::ZoneOwner::All
                    | v2::ZoneOwner::EachPlayer => {}
                    v2::ZoneOwner::You => {
                        obj = obj.with_owner(ControllerFilter::OwnerYou);
                    }
                    v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents => {
                        obj = obj.with_owner(ControllerFilter::OwnerOpponent);
                    }
                    _ => return Err(LoweringError::Unsupported("v2:target:scoped-exile")),
                }
                Ok(BodyTarget::filtered_exiled(obj))
            }
            v2::ZoneKind::Hand
            | v2::ZoneKind::Library
            | v2::ZoneKind::CommandZone
            | v2::ZoneKind::OutsideTheGame
            | v2::ZoneKind::Stack => {
                Err(LoweringError::Unsupported("v2:target:unsupported-zone"))
            }
        };
    }

    if filter.any_card && filter.all_types.is_empty() && filter.any_types.is_empty() {
        return Err(LoweringError::Unsupported("v2:target:zoneless-card"));
    }

    let obj = lower_object_filter(filter, ctx)?;
    Ok(BodyTarget::filtered_permanent(obj))
}

fn apply_zone_owner_to_bf_filter(
    filter: &mut v2::ObjectFilter,
    owner: v2::ZoneOwner,
) -> LowerResult<()> {
    let rel = match owner {
        v2::ZoneOwner::Default
        | v2::ZoneOwner::Any
        | v2::ZoneOwner::All
        | v2::ZoneOwner::EachPlayer => return Ok(()),
        v2::ZoneOwner::You => ast::ControllerRelation::You,
        v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents => ast::ControllerRelation::Opponent,
        v2::ZoneOwner::TargetOpponent => ast::ControllerRelation::TargetOpponent,
        v2::ZoneOwner::TargetPlayer => ast::ControllerRelation::TargetPlayer,
        v2::ZoneOwner::DefendingPlayer => ast::ControllerRelation::DefendingPlayer,
        v2::ZoneOwner::ThatPlayer => ast::ControllerRelation::ThatPlayer,
        v2::ZoneOwner::ItsOwner
        | v2::ZoneOwner::TheirOwners
        | v2::ZoneOwner::ThatCreatureOwner => {
            return Err(LoweringError::Unsupported("v2:target:bf-owner-ref"));
        }
    };
    if let Some(existing) = filter.controller {
        if existing != rel {
            return Err(LoweringError::Unsupported("v2:target:conflicting-controller"));
        }
    } else {
        filter.controller = Some(rel);
    }
    Ok(())
}

fn lower_compound_target(
    parts: &[v2::SubjectRef],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(BodyTarget, v2::TargetCount)> {
    if parts.len() == 2 {
        let (a, b) = (&parts[0], &parts[1]);
        if let Some((c1, c2)) = pure_single_type_count(a, ast::CardTypeMask::CREATURE)
            .zip(player_single_target_count(b, false))
            .or_else(|| {
                player_single_target_count(a, false)
                    .zip(pure_single_type_count(b, ast::CardTypeMask::CREATURE))
            })
        {
            return Ok((
                BodyTarget::SINGLE_CREATURE_OR_PLAYER,
                merge_unit_target_counts(c1, c2),
            ));
        }
        if let Some((c1, c2)) = pure_single_type_count(a, ast::CardTypeMask::PLANESWALKER)
            .zip(player_single_target_count(b, false))
            .or_else(|| {
                player_single_target_count(a, false)
                    .zip(pure_single_type_count(b, ast::CardTypeMask::PLANESWALKER))
            })
        {
            return Ok((
                BodyTarget::SINGLE_PLAYER_OR_PLANESWALKER,
                merge_unit_target_counts(c1, c2),
            ));
        }
        if let Some((c1, c2)) = pure_single_type_count(a, ast::CardTypeMask::PLANESWALKER)
            .zip(player_single_target_count(b, true))
            .or_else(|| {
                player_single_target_count(a, true)
                    .zip(pure_single_type_count(b, ast::CardTypeMask::PLANESWALKER))
            })
        {
            return Ok((
                BodyTarget::SINGLE_OPPONENT_OR_PLANESWALKER,
                merge_unit_target_counts(c1, c2),
            ));
        }

        // Merge two single-clause object targets in the same zone into `ObjectFilter::Either`.
        if let (
            Ok((
                BodyTarget::Object {
                    zone: z1,
                    filter: EngineObjectFilter::Clause(c1),
                },
                tc1,
            )),
            Ok((
                BodyTarget::Object {
                    zone: z2,
                    filter: EngineObjectFilter::Clause(c2),
                },
                tc2,
            )),
        ) = (
            lower_subject_to_target(a, ctx),
            lower_subject_to_target(b, ctx),
        ) && z1 == z2
            && tc1 == tc2
        {
            return Ok((
                BodyTarget::Object {
                    zone: z1,
                    filter: EngineObjectFilter::Either(c1, c2),
                },
                tc1,
            ));
        }
    }

    if parts.len() == 3 {
        let creature_c = parts
            .iter()
            .find_map(|s| pure_single_type_count(s, ast::CardTypeMask::CREATURE));
        let pw_c = parts
            .iter()
            .find_map(|s| pure_single_type_count(s, ast::CardTypeMask::PLANESWALKER));
        let player_c = parts
            .iter()
            .find_map(|s| player_single_target_count(s, false));
        if let (Some(c1), Some(c2), Some(c3)) = (creature_c, pw_c, player_c) {
            return Ok((
                BodyTarget::SINGLE_ANY_TARGET,
                merge_unit_target_counts(merge_unit_target_counts(c1, c2), c3),
            ));
        }
    }

    Err(LoweringError::Unsupported("v2:target:compound-subject"))
}

/// Lower a [`v2::SubjectRef`] into a composable toolbox [`ObjectQuery`].
pub(crate) fn lower_subject_to_query(
    subject: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ObjectQuery> {
    ctx.has_object_target = acc.target_set && target_is_object_like(acc.target);
    match subject {
        v2::SubjectRef::SelfObject => Ok(ObjectQuery::Source),
        v2::SubjectRef::Anaphora(anaph) => lower_anaphora_to_query(*anaph, acc, ctx),
        v2::SubjectRef::AnyTarget => {
            acc.register_target(BodyTarget::SINGLE_ANY_TARGET, v2::TargetCount::Exact(1))?;
            ctx.has_object_target = true;
            Ok(ObjectQuery::Targets)
        }
        v2::SubjectRef::TargetObject { .. } => {
            let (target, count) = lower_subject_to_target(subject, ctx)?;
            acc.register_target(target, count)?;
            ctx.has_object_target = acc.target_set && target_is_object_like(acc.target);
            Ok(ObjectQuery::Targets)
        }
        v2::SubjectRef::Each(filter)
        | v2::SubjectRef::All(filter)
        | v2::SubjectRef::Group(filter) => lower_group_filter_to_query(filter, acc, ctx),
        v2::SubjectRef::Counted { filter, count } => {
            if count.up_to || count.any_number {
                return Err(LoweringError::Unsupported("v2:subject:counted-choice-query"));
            }
            if let Some(target) = scan_value_expr_player_target(&count.count) {
                acc.register_target(target, v2::TargetCount::Exact(1))?;
            }
            let base = lower_group_filter_to_query(filter, acc, ctx)?;
            let take_count = lower_value_expr(&count.count, ctx)?;
            Ok(ObjectQuery::Take {
                source: Box::new(base),
                count: Box::new(take_count),
            })
        }
        v2::SubjectRef::HandOf(player) => {
            let ps = lower_player_ref(*player, acc, ctx)?;
            Ok(ObjectQuery::zone(
                QueryZone::Hand(Box::new(ps)),
                EngineObjectFilter::any(),
            ))
        }
        v2::SubjectRef::BothPairedCreatures => {
            if acc.target_set {
                Ok(ObjectQuery::TargetsWithPairedPartner)
            } else {
                Err(LoweringError::Unsupported(
                    "v2:subject:both-paired-creatures",
                ))
            }
        }
        v2::SubjectRef::Compound(parts) => {
            if parts.is_empty() {
                return Err(LoweringError::Unsupported("v2:subject:empty-compound"));
            }
            if parts.iter().any(subject_is_targeting) {
                let (target, count) = lower_compound_target(parts, ctx)?;
                acc.register_target(target, count)?;
                ctx.has_object_target = acc.target_set && target_is_object_like(acc.target);
                return Ok(ObjectQuery::Targets);
            }
            let mut queries = Vec::with_capacity(parts.len());
            for part in parts {
                queries.push(lower_subject_to_query(part, acc, ctx)?);
            }
            Ok(ObjectQuery::Union(queries))
        }
        v2::SubjectRef::TargetPlayer { .. } => {
            Err(LoweringError::Unsupported("v2:subject:player-as-object"))
        }
        v2::SubjectRef::Player(_) => {
            Err(LoweringError::Unsupported("v2:subject:player-as-object"))
        }
        v2::SubjectRef::OneOfThem(_) => {
            Err(LoweringError::Unsupported("v2:subject:one-of-them"))
        }
        v2::SubjectRef::PlayersDealtCombatDamageBy { .. } => Err(LoweringError::Unsupported(
            "v2:subject:players-dealt-combat-damage",
        )),
        v2::SubjectRef::PlayerWhoControls { .. } => {
            Err(LoweringError::Unsupported("v2:subject:player-who-controls"))
        }
    }
}

fn lower_anaphora_to_query(
    anaph: v2::AnaphoraRef,
    acc: &BodyAcc,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ObjectQuery> {
    let has_obj_target = acc.target_set && target_is_object_like(acc.target);
    match anaph {
        v2::AnaphoraRef::It
        | v2::AnaphoraRef::Them
        | v2::AnaphoraRef::ThatCreature
        | v2::AnaphoraRef::ThatPermanent
        | v2::AnaphoraRef::ThatCard
        | v2::AnaphoraRef::ThatSpell
        | v2::AnaphoraRef::ThatToken
        | v2::AnaphoraRef::ThatLand
        | v2::AnaphoraRef::ThatObject
        | v2::AnaphoraRef::ThoseCreatures
        | v2::AnaphoraRef::ThosePermanents
        | v2::AnaphoraRef::ThoseCards => {
            if let Some(binding) = ctx.in_loop_object {
                return Ok(ObjectQuery::Bound(binding));
            }
            if let Some(binding) = ctx.last_moved_binding {
                return Ok(ObjectQuery::Bound(binding));
            }
            if has_obj_target {
                return Ok(ObjectQuery::Targets);
            }
            if ctx.in_trigger {
                return Ok(ObjectQuery::TriggerSubject);
            }
            if matches!(anaph, v2::AnaphoraRef::ThatToken) {
                return Ok(ObjectQuery::CreatedTokens);
            }
            for ins in acc.instructions.iter().rev() {
                match ins {
                    Instruction::CreateTokens { .. }
                        if matches!(
                            anaph,
                            v2::AnaphoraRef::Them
                                | v2::AnaphoraRef::ThoseCreatures
                                | v2::AnaphoraRef::ThosePermanents
                                | v2::AnaphoraRef::ThatCreature
                                | v2::AnaphoraRef::ThatPermanent
                                | v2::AnaphoraRef::ThatObject
                                | v2::AnaphoraRef::It
                        ) =>
                    {
                        return Ok(ObjectQuery::CreatedTokens);
                    }
                    Instruction::MutateObject(spec) => {
                        if let Some(binding) = spec.bind_mutated {
                            return Ok(ObjectQuery::Bound(binding));
                        }
                        if matches!(spec.selection, ZoneSelection::All)
                            && !matches!(spec.source, ObjectQuery::Empty)
                        {
                            return Ok(spec.source.clone());
                        }
                        break;
                    }
                    Instruction::ZoneOp(spec) => {
                        if let Some(binding) = spec.bind_moved {
                            return Ok(ObjectQuery::Bound(binding));
                        }
                        return Ok(ObjectQuery::LastMoved);
                    }
                    _ => {}
                }
            }
            if matches!(
                anaph,
                v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatObject
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
            ) {
                return Ok(ObjectQuery::Source);
            }
            Err(LoweringError::Unsupported("v2:subject:unbound-anaphora"))
        }
        v2::AnaphoraRef::SameTarget => {
            if has_obj_target {
                Ok(ObjectQuery::Targets)
            } else if let Some(binding) = ctx.last_moved_binding {
                Ok(ObjectQuery::Bound(binding))
            } else {
                Err(LoweringError::Unsupported("v2:subject:unbound-same-target"))
            }
        }
        v2::AnaphoraRef::TheRest => ctx
            .last_unchosen_binding
            .map(ObjectQuery::Bound)
            .ok_or(LoweringError::Unsupported("v2:subject:unbound-the-rest")),
        v2::AnaphoraRef::TheExiledCard
        | v2::AnaphoraRef::ExiledObject
        | v2::AnaphoraRef::SacrificedObject
        | v2::AnaphoraRef::DiscardedObject => {
            if let Some(binding) = ctx.last_moved_binding {
                Ok(ObjectQuery::Bound(binding))
            } else {
                Ok(ObjectQuery::LastMoved)
            }
        }
        v2::AnaphoraRef::DestroyedObject => {
            if let Some(binding) = ctx.last_moved_binding {
                Ok(ObjectQuery::Bound(binding))
            } else {
                Ok(ObjectQuery::DestroyedThisWay)
            }
        }
        v2::AnaphoraRef::TheCopy | v2::AnaphoraRef::TheCopies => Ok(ObjectQuery::CreatedTokens),
        v2::AnaphoraRef::RevealedCard => {
            if let Some(binding) = ctx.last_moved_binding {
                Ok(ObjectQuery::Bound(binding))
            } else {
                Ok(ObjectQuery::Revealed)
            }
        }
        v2::AnaphoraRef::EquippedCreature
        | v2::AnaphoraRef::EnchantedCreature
        | v2::AnaphoraRef::EnchantedPermanent => Ok(ObjectQuery::AttachedHost),
        v2::AnaphoraRef::ThatSource => Ok(ObjectQuery::Source),
        v2::AnaphoraRef::OnePile => Err(LoweringError::Unsupported("v2:subject:one-pile")),
        v2::AnaphoraRef::OtherPile => Err(LoweringError::Unsupported("v2:subject:other-pile")),
        v2::AnaphoraRef::BlightedCreature => {
            Err(LoweringError::Unsupported("v2:subject:blighted-creature"))
        }
        v2::AnaphoraRef::ThatDamage => Err(LoweringError::Unsupported("v2:subject:that-damage")),
    }
}

fn lower_group_filter_to_query(
    filter: &v2::ObjectFilter,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ObjectQuery> {
    if let Some(target) = scan_filter_player_target(filter) {
        acc.register_target(target, v2::TargetCount::Exact(1))?;
    }
    let (stripped_prior, had_among_prior) = split_among_prior_cards(filter);
    if had_among_prior && stripped_prior.zones.is_empty() {
        let Some(binding) = ctx.last_moved_binding else {
            return Err(LoweringError::Unsupported(
                "v2:filter:pred:among-prior-cards",
            ));
        };
        let ef = lower_object_filter(&stripped_prior, ctx)?;
        return Ok(ObjectQuery::BoundFiltered {
            binding,
            filter: Box::new(ef),
        });
    }

    if filter.zones.is_empty() && filter.flags.contains(v2::FilterFlags::EXILED_WITH_SELF) {
        let mut cleaned = filter.clone();
        cleaned.flags.0 &= !v2::FilterFlags::EXILED_WITH_SELF.0;
        let ef = lower_object_filter(&cleaned, ctx)?;
        return Ok(ObjectQuery::Filtered {
            source: Box::new(ObjectQuery::LinkedExile { group: None }),
            filter: Box::new(ef),
        });
    }

    if filter.zones.is_empty()
        && let Some(prior_kind) = filter.predicates.iter().find_map(|p| match p {
            v2::FilterPredicate::AffectedByPriorAction(k) => Some(*k),
            _ => None,
        })
    {
        let prior_query = match prior_kind {
            v2::PriorActionKind::RevealedThisWay => ctx
                .last_moved_binding
                .map_or(ObjectQuery::Revealed, ObjectQuery::Bound),
            v2::PriorActionKind::DestroyedThisWay => ObjectQuery::DestroyedThisWay,
            v2::PriorActionKind::CreatedThisWay => ObjectQuery::CreatedTokens,
            v2::PriorActionKind::ExiledThisWay
            | v2::PriorActionKind::MilledThisWay
            | v2::PriorActionKind::DiscardedThisWay
            | v2::PriorActionKind::SacrificedThisWay
            | v2::PriorActionKind::ReturnedThisWay
            | v2::PriorActionKind::SearchedThisWay => ctx
                .last_moved_binding
                .map_or(ObjectQuery::LastMoved, ObjectQuery::Bound),
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:prior-action-kind",
                ));
            }
        };
        let mut cleaned = filter.clone();
        cleaned
            .predicates
            .retain(|p| !matches!(p, v2::FilterPredicate::AffectedByPriorAction(_)));
        let ef = lower_object_filter(&cleaned, ctx)?;
        return Ok(ObjectQuery::Filtered {
            source: Box::new(prior_query),
            filter: Box::new(ef),
        });
    }

    let obj_filter = lower_object_filter(filter, ctx)?;
    if filter.zones.is_empty() {
        if filter.any_spell {
            return Ok(ObjectQuery::zone(QueryZone::Stack, obj_filter));
        }
        if filter.any_card && filter.all_types.is_empty() && filter.any_types.is_empty() {
            return Err(LoweringError::Unsupported("v2:subject:zoneless-card-group"));
        }
        return Ok(ObjectQuery::zone(QueryZone::Battlefield, obj_filter));
    }
    if filter.zones.len() == 1 {
        let qz = lower_zone_ref(filter.zones[0], acc, ctx)?;
        return Ok(ObjectQuery::zone(qz, obj_filter));
    }
    let mut parts = Vec::with_capacity(filter.zones.len());
    for z in &filter.zones {
        let qz = lower_zone_ref(*z, acc, ctx)?;
        parts.push(ObjectQuery::zone(qz, obj_filter));
    }
    Ok(ObjectQuery::Union(parts))
}
