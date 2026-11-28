//! Composable filter lowerers from `v2::ir::ObjectFilter` and `v2::ir::SubjectRef` into
//! engine `ObjectFilter`, `PermanentSelector`, and `SpellFilter`.

use std::{collections::BTreeSet, num::NonZeroU32};

use spellwake_contracts::names::EnglishCardName;
use spellwake_engine::{
    characteristics::{
        Color, ColorFilter, CombatSelector, ControllerSelector, KeywordAbility, NameFilter,
        OwnerSelector, PermanentSelector, SelectorSubtype,
    },
    program::{
        BodyTarget, CardType, CardTypeMask as EngineCardTypeMask, CombatFilter, Comparison,
        ControllerFilter, DynamicStatAnchor, ExactNameKey, ManaValueFilter, ObjectClause,
        ObjectColorFilter, ObjectFilter as EngineObjectFilter, ObjectFilterFlags, PowerFilter,
        SingleSubtype, SourceRelation, SpellCastZoneFilter, SpellFilter, SpellSpecialFilter,
        SpellTargetShape, SubtypeFilter, SubtypeList, ToughnessFilter,
    },
};
use spellwake_oracle_compiler::{ast, v2::ir as v2};

use crate::v2_lowering::{
    atoms::{
        lower_card_type_set, lower_color_mask, lower_color_set, lower_compare_op,
        lower_counter_kind, lower_engine_type_mask, lower_keyword_kind_plain,
        lower_static_controller, lower_supertype_mask, lower_supertype_set,
    },
    ctx::LowerCtx,
    error::{LowerResult, LoweringError},
    value::{lower_value_const, scan_value_expr_player_target},
};

/// Scan a [`v2::ObjectFilter`] for an embedded `TargetOpponent` or `TargetPlayer` controller,
/// owner, zone owner, stat-bound expression, or disjunctive branch that requires registering a
/// player target on the enclosing [`crate::v2_lowering::ctx::BodyAcc`].
#[must_use]
pub(crate) fn scan_filter_player_target(flt: &v2::ObjectFilter) -> Option<BodyTarget> {
    let rel_target = |rel: ast::ControllerRelation| match rel {
        ast::ControllerRelation::TargetOpponent => Some(BodyTarget::SINGLE_OPPONENT),
        ast::ControllerRelation::TargetPlayer => Some(BodyTarget::SINGLE_PLAYER),
        _ => None,
    };
    if let Some(target) = flt.controller.and_then(rel_target) {
        return Some(target);
    }
    if let Some(target) = flt.owner.and_then(rel_target) {
        return Some(target);
    }
    for z in &flt.zones {
        match z.owner {
            v2::ZoneOwner::TargetOpponent => return Some(BodyTarget::SINGLE_OPPONENT),
            v2::ZoneOwner::TargetPlayer => return Some(BodyTarget::SINGLE_PLAYER),
            _ => {}
        }
    }
    for bound in &flt.stat_bounds {
        if let Some(target) = scan_value_expr_player_target(&bound.value) {
            return Some(target);
        }
    }
    for pred in &flt.predicates {
        if let v2::FilterPredicate::AnyOf(subs) = pred
            && let Some(target) = subs.iter().find_map(scan_filter_player_target)
        {
            return Some(target);
        }
    }
    None
}

/// Resolve a single subtype token into a typed [`SingleSubtype`] using card-type hints first.
fn resolve_single_subtype(
    raw: &str,
    type_hint: ast::CardTypeMask,
    ctx: &LowerCtx<'_>,
) -> LowerResult<SingleSubtype> {
    if raw.eq_ignore_ascii_case("aura") {
        return Ok(SingleSubtype::Aura);
    }
    let bits = type_hint.0;
    if (bits & ast::CardTypeMask::LAND.0) != 0
        && let Some(lt) = LowerCtx::lookup_land_type(raw)
    {
        return Ok(SingleSubtype::Land(lt));
    }
    if (bits & ast::CardTypeMask::ARTIFACT.0) != 0
        && let Some(at) = LowerCtx::lookup_artifact_subtype(raw)
    {
        return Ok(SingleSubtype::Artifact(at));
    }
    if (bits & ast::CardTypeMask::ENCHANTMENT.0) != 0
        && let Some(et) = LowerCtx::lookup_enchantment_subtype(raw)
    {
        return Ok(SingleSubtype::Enchantment(et));
    }
    if (bits & ast::CardTypeMask::PLANESWALKER.0) != 0
        && let Some(pw) = LowerCtx::lookup_planeswalker_subtype(raw)
    {
        return Ok(SingleSubtype::Planeswalker(pw));
    }
    if (bits & (ast::CardTypeMask::INSTANT.0 | ast::CardTypeMask::SORCERY.0)) != 0
        && let Some(sp) = LowerCtx::lookup_spell_subtype(raw)
    {
        return Ok(SingleSubtype::Spell(sp));
    }
    if (bits & (ast::CardTypeMask::CREATURE.0 | ast::CardTypeMask::KINDRED.0)) != 0
        && let Some(ct) = ctx.lookup_creature_type(raw)
    {
        return Ok(SingleSubtype::Creature(ct));
    }

    // Deterministic category priority fallback when no explicit card-type hint matched.
    if let Some(ct) = ctx.lookup_creature_type(raw) {
        return Ok(SingleSubtype::Creature(ct));
    }
    if let Some(lt) = LowerCtx::lookup_land_type(raw) {
        return Ok(SingleSubtype::Land(lt));
    }
    if let Some(at) = LowerCtx::lookup_artifact_subtype(raw) {
        return Ok(SingleSubtype::Artifact(at));
    }
    if let Some(et) = LowerCtx::lookup_enchantment_subtype(raw) {
        return Ok(SingleSubtype::Enchantment(et));
    }
    if let Some(pw) = LowerCtx::lookup_planeswalker_subtype(raw) {
        return Ok(SingleSubtype::Planeswalker(pw));
    }
    if let Some(sp) = LowerCtx::lookup_spell_subtype(raw) {
        return Ok(SingleSubtype::Spell(sp));
    }

    Err(LoweringError::Unsupported("v2:filter:unknown-subtype"))
}

fn single_subtype_to_selector(sub: SingleSubtype) -> LowerResult<SelectorSubtype> {
    match sub {
        SingleSubtype::Creature(ct) => Ok(SelectorSubtype::Creature(ct)),
        SingleSubtype::Land(lt) => Ok(SelectorSubtype::Land(lt)),
        SingleSubtype::Artifact(at) => Ok(SelectorSubtype::Artifact(at)),
        SingleSubtype::Enchantment(et) => Ok(SelectorSubtype::Enchantment(et)),
        SingleSubtype::Planeswalker(pw) => Ok(SelectorSubtype::Planeswalker(pw)),
        SingleSubtype::Aura => Ok(SelectorSubtype::Aura),
        SingleSubtype::Spell(_) => Err(LoweringError::Unsupported(
            "v2:selector:spell-subtype-on-permanent",
        )),
    }
}

fn lower_owner_relation(rel: ast::ControllerRelation) -> LowerResult<ControllerFilter> {
    match rel {
        ast::ControllerRelation::You | ast::ControllerRelation::OwnerYou => {
            Ok(ControllerFilter::OwnerYou)
        }
        ast::ControllerRelation::Opponent
        | ast::ControllerRelation::Opponents
        | ast::ControllerRelation::OwnerOpponent => Ok(ControllerFilter::OwnerOpponent),
        ast::ControllerRelation::NotYou => Ok(ControllerFilter::NotYou),
        ast::ControllerRelation::ThatPlayer | ast::ControllerRelation::OwnerThatPlayer => {
            Ok(ControllerFilter::RelatedPlayer)
        }
        ast::ControllerRelation::TargetPlayer => Ok(ControllerFilter::TargetPlayer),
        ast::ControllerRelation::TargetOpponent => Ok(ControllerFilter::TargetOpponent),
        ast::ControllerRelation::ActivePlayer => Ok(ControllerFilter::ActivePlayer),
        ast::ControllerRelation::DefendingPlayer => Ok(ControllerFilter::DefendingPlayer),
        _ => Err(LoweringError::Unsupported("v2:filter:owner-relation")),
    }
}

fn lower_combat_filter(combat: ast::CombatState) -> CombatFilter {
    match combat {
        ast::CombatState::Attacking => CombatFilter::Attacking,
        ast::CombatState::Blocking => CombatFilter::Blocking,
        ast::CombatState::AttackingOrBlocking => CombatFilter::AttackingOrBlocking,
        ast::CombatState::Blocked => CombatFilter::Blocked,
        ast::CombatState::Unblocked => CombatFilter::Unblocked,
        ast::CombatState::BlockingSelf => CombatFilter::BlockingSource,
        ast::CombatState::BlockedBySelf => CombatFilter::BlockedBySource,
        ast::CombatState::BlockingOrBlockedBySelf => CombatFilter::BlockingOrBlockedBySource,
        ast::CombatState::NotAttacking => CombatFilter::NotAttacking,
    }
}

fn lower_combat_selector(combat: ast::CombatState) -> CombatSelector {
    match combat {
        ast::CombatState::Attacking => CombatSelector::Attacking,
        ast::CombatState::Blocking => CombatSelector::Blocking,
        ast::CombatState::AttackingOrBlocking => CombatSelector::AttackingOrBlocking,
        ast::CombatState::Blocked => CombatSelector::Blocked,
        ast::CombatState::Unblocked => CombatSelector::Unblocked,
        ast::CombatState::BlockingSelf => CombatSelector::BlockingSource,
        ast::CombatState::BlockedBySelf => CombatSelector::BlockedBySource,
        ast::CombatState::BlockingOrBlockedBySelf => CombatSelector::BlockingOrBlockedBySource,
        ast::CombatState::NotAttacking => CombatSelector::NotAttacking,
    }
}

fn is_plain_anchor_count_filter(f: &v2::ObjectFilter) -> bool {
    f.forbidden_types.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.owner.is_none()
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
        && f.predicates.is_empty()
}

fn lower_dynamic_stat_anchor(
    expr: &v2::ValueExpr,
    ctx: &LowerCtx<'_>,
) -> Option<DynamicStatAnchor> {
    match expr {
        v2::ValueExpr::Var(v2::VarName::X) => Some(DynamicStatAnchor::ChosenX),
        v2::ValueExpr::ObjectStat { subject, stat } => match (subject.as_ref(), stat) {
            (
                v2::SubjectRef::SelfObject | v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSource),
                ast::StatKind::Power,
            ) => Some(DynamicStatAnchor::SourcePower),
            (
                v2::SubjectRef::SelfObject | v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSource),
                ast::StatKind::Toughness,
            ) => Some(DynamicStatAnchor::SourceToughness),
            (
                v2::SubjectRef::SelfObject | v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSource),
                ast::StatKind::ManaValue,
            ) => Some(DynamicStatAnchor::SourceManaValue),
            (v2::SubjectRef::Anaphora(v2::AnaphoraRef::SacrificedObject), ast::StatKind::Power) => {
                Some(DynamicStatAnchor::LastSacrificedPower)
            }
            (
                v2::SubjectRef::Anaphora(v2::AnaphoraRef::SacrificedObject),
                ast::StatKind::Toughness,
            ) => Some(DynamicStatAnchor::LastSacrificedToughness),
            (
                v2::SubjectRef::Anaphora(v2::AnaphoraRef::SacrificedObject),
                ast::StatKind::ManaValue,
            ) => Some(DynamicStatAnchor::LastSacrificedManaValue),
            (v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget), ast::StatKind::Power) => {
                Some(DynamicStatAnchor::TargetPower)
            }
            (v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget), ast::StatKind::Toughness) => {
                Some(DynamicStatAnchor::TargetToughness)
            }
            (v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget), ast::StatKind::ManaValue) => {
                Some(DynamicStatAnchor::TargetManaValue)
            }
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::ThatObject,
                ),
                stat_kind,
            ) => match (ctx.has_object_target, ctx.in_trigger, stat_kind) {
                (true, _, ast::StatKind::Power) => Some(DynamicStatAnchor::TargetPower),
                (true, _, ast::StatKind::Toughness) => Some(DynamicStatAnchor::TargetToughness),
                (true, _, ast::StatKind::ManaValue) => Some(DynamicStatAnchor::TargetManaValue),
                (false, true, ast::StatKind::Power) => Some(DynamicStatAnchor::TriggerSubjectPower),
                (false, true, ast::StatKind::Toughness) => {
                    Some(DynamicStatAnchor::TriggerSubjectToughness)
                }
                (false, true, ast::StatKind::ManaValue) => {
                    Some(DynamicStatAnchor::TriggerSubjectManaValue)
                }
                (false, false, ast::StatKind::Power) => Some(DynamicStatAnchor::SourcePower),
                (false, false, ast::StatKind::Toughness) => {
                    Some(DynamicStatAnchor::SourceToughness)
                }
                (false, false, ast::StatKind::ManaValue) => {
                    Some(DynamicStatAnchor::SourceManaValue)
                }
                (_, _, ast::StatKind::Loyalty) => None,
            },
            _ => None,
        },
        v2::ValueExpr::CounterCount { subject, kind }
            if matches!(
                subject.as_ref(),
                v2::SubjectRef::SelfObject | v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSource)
            ) || (!ctx.has_object_target
                && !ctx.in_trigger
                && matches!(
                    subject.as_ref(),
                    v2::SubjectRef::Anaphora(
                        v2::AnaphoraRef::It
                            | v2::AnaphoraRef::ThatCreature
                            | v2::AnaphoraRef::ThatPermanent
                            | v2::AnaphoraRef::ThatObject
                    )
                )) =>
        {
            lower_counter_kind(kind)
                .ok()
                .map(DynamicStatAnchor::SourceCounters)
        }
        v2::ValueExpr::PlayerStat {
            player: v2::PlayerRef::You,
            stat,
        } => match stat {
            ast::PlayerStatKind::LifeTotal => Some(DynamicStatAnchor::ControllerLife),
            ast::PlayerStatKind::CardsInHand => Some(DynamicStatAnchor::ControllerHandSize),
            ast::PlayerStatKind::CardsInGraveyard => {
                Some(DynamicStatAnchor::ControllerGraveyardCount)
            }
            ast::PlayerStatKind::CardsInLibrary => None,
        },
        v2::ValueExpr::Count(f) if is_plain_anchor_count_filter(f) => {
            if f.controller.is_none()
                && f.all_types.is_empty()
                && f.any_types.is_empty()
                && f.subtypes.is_empty()
                && let [zone] = f.zones.as_slice()
                && matches!(zone.owner, v2::ZoneOwner::Default | v2::ZoneOwner::You)
            {
                return match zone.zone {
                    v2::ZoneKind::Graveyard => Some(DynamicStatAnchor::ControllerGraveyardCount),
                    v2::ZoneKind::Hand => Some(DynamicStatAnchor::ControllerHandSize),
                    _ => None,
                };
            }
            let controlled_bf = (f.controller == Some(ast::ControllerRelation::You)
                && f.zones.is_empty())
                || (f.controller.is_none()
                    && matches!(
                        f.zones.as_slice(),
                        [v2::ZoneRef {
                            zone: v2::ZoneKind::Battlefield,
                            owner: v2::ZoneOwner::You
                        }]
                    ));
            if !controlled_bf {
                return None;
            }
            if f.subtypes.len() == 1 && f.all_types.is_empty() && f.any_types.is_empty() {
                return ctx
                    .lookup_creature_type(&f.subtypes[0])
                    .map(DynamicStatAnchor::ControllerSubtypeCount);
            }
            if f.subtypes.is_empty() {
                let all = lower_card_type_set(f.all_types);
                let any = lower_card_type_set(f.any_types);
                if all.len() == 1 && any.is_empty() {
                    return all
                        .into_iter()
                        .next()
                        .map(DynamicStatAnchor::ControllerControlledCount);
                }
                if any.len() == 1 && all.is_empty() {
                    return any
                        .into_iter()
                        .next()
                        .map(DynamicStatAnchor::ControllerControlledCount);
                }
            }
            None
        }
        v2::ValueExpr::PriorActionCount(
            v2::PriorActionKind::ThatMany
            | v2::PriorActionKind::ExiledThisWay
            | v2::PriorActionKind::SacrificedThisWay
            | v2::PriorActionKind::DiscardedThisWay
            | v2::PriorActionKind::MilledThisWay
            | v2::PriorActionKind::ReturnedThisWay
            | v2::PriorActionKind::RevealedThisWay
            | v2::PriorActionKind::SearchedThisWay,
        ) => Some(DynamicStatAnchor::LastMovedCount),
        _ => None,
    }
}

fn apply_exclude_self(clause: &mut ObjectClause, in_trigger: bool) -> LowerResult<()> {
    clause.source_relation = match clause.source_relation {
        SourceRelation::Any => {
            if in_trigger {
                SourceRelation::OtherThanTriggerSource
            } else {
                SourceRelation::OtherThanAbilitySource
            }
        }
        SourceRelation::OtherThanAbilitySource | SourceRelation::OtherThanTriggerSource => {
            clause.source_relation
        }
        SourceRelation::SharingNameWithAbilitySource
        | SourceRelation::OtherThanAbilitySourceSharingName => {
            SourceRelation::OtherThanAbilitySourceSharingName
        }
        _ => {
            return Err(LoweringError::Unsupported(
                "v2:filter:conflicting-source-relation",
            ));
        }
    };
    Ok(())
}

fn resolve_subtype_or_batch(
    raw: &str,
    type_hint: ast::CardTypeMask,
    ctx: &LowerCtx<'_>,
) -> LowerResult<Vec<SingleSubtype>> {
    if raw.eq_ignore_ascii_case("outlaw") {
        let mut out = Vec::with_capacity(5);
        for member in ["Assassin", "Mercenary", "Pirate", "Rogue", "Warlock"] {
            let ct = ctx
                .lookup_creature_type(member)
                .ok_or(LoweringError::Unsupported("v2:filter:unknown-subtype"))?;
            out.push(SingleSubtype::Creature(ct));
        }
        return Ok(out);
    }
    Ok(vec![resolve_single_subtype(raw, type_hint, ctx)?])
}

fn apply_subtypes_to_clause(
    clause: &mut ObjectClause,
    filter: &v2::ObjectFilter,
    ctx: &LowerCtx<'_>,
) -> LowerResult<()> {
    let type_hint = filter.all_types.union(filter.any_types);
    let mut req_subs: Vec<SingleSubtype> = Vec::with_capacity(filter.subtypes.len());
    let mut effective_match_any = filter.subtypes_match_any;
    for raw in &filter.subtypes {
        let expanded = resolve_subtype_or_batch(raw, type_hint, ctx)?;
        if expanded.len() > 1 {
            if filter.subtypes.len() == 1 || filter.subtypes_match_any {
                effective_match_any = true;
            } else {
                return Err(LoweringError::Unsupported("v2:filter:unknown-subtype"));
            }
        }
        for sub in expanded {
            if !req_subs.contains(&sub) {
                req_subs.push(sub);
            }
        }
    }

    let mut forb_subs: Vec<SingleSubtype> = Vec::new();
    for raw in &filter.forbidden_subtypes {
        for sub in resolve_subtype_or_batch(raw, type_hint, ctx)? {
            match sub {
                SingleSubtype::Aura => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::NON_AURA);
                }
                SingleSubtype::Enchantment(spellwake_engine::program::EnchantmentSubtype::Saga) => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::NON_SAGA);
                }
                other => {
                    if !forb_subs.contains(&other) {
                        forb_subs.push(other);
                    }
                }
            }
        }
    }

    if !req_subs.is_empty() && !forb_subs.is_empty() {
        return Err(LoweringError::Unsupported(
            "v2:filter:both-required-and-forbidden-subtypes",
        ));
    }

    if !req_subs.is_empty() {
        clause.subtypes = build_positive_subtype_filter(&req_subs, effective_match_any)?;
    } else if !forb_subs.is_empty() {
        clause.subtypes = build_negative_subtype_filter(&forb_subs)?;
    }

    Ok(())
}

fn build_positive_subtype_filter(
    subs: &[SingleSubtype],
    match_any: bool,
) -> LowerResult<SubtypeFilter> {
    if subs.len() == 1 {
        return Ok(match subs[0] {
            SingleSubtype::Creature(ct) => SubtypeFilter::Has(ct),
            SingleSubtype::Land(lt) => SubtypeFilter::Land(lt),
            SingleSubtype::Artifact(at) => SubtypeFilter::Artifact(at),
            SingleSubtype::Enchantment(et) => SubtypeFilter::Enchantment(et),
            SingleSubtype::Planeswalker(pw) => SubtypeFilter::Planeswalker(pw),
            SingleSubtype::Spell(sp) => SubtypeFilter::Spell(sp),
            SingleSubtype::Aura => SubtypeFilter::Aura,
        });
    }
    if match_any && subs.iter().all(|s| matches!(s, SingleSubtype::Land(_))) {
        let mut mask = 0u32;
        for s in subs {
            if let SingleSubtype::Land(lt) = s {
                mask |= lt.bit();
            }
        }
        return Ok(SubtypeFilter::AnyLandMask(mask));
    }
    let list = SubtypeList::from_slice(subs)
        .ok_or(LoweringError::Unsupported("v2:filter:too-many-subtypes"))?;
    Ok(if match_any {
        SubtypeFilter::AnyOfList(list)
    } else {
        SubtypeFilter::AllOfList(list)
    })
}

fn build_negative_subtype_filter(subs: &[SingleSubtype]) -> LowerResult<SubtypeFilter> {
    if subs.len() == 1 {
        return match subs[0] {
            SingleSubtype::Creature(ct) => Ok(SubtypeFilter::Lacks(ct)),
            SingleSubtype::Land(lt) => Ok(SubtypeFilter::NotLand(lt)),
            other => {
                let list = SubtypeList::from_slice(&[other])
                    .ok_or(LoweringError::Unsupported("v2:filter:forbidden-subtype"))?;
                Ok(SubtypeFilter::NoneOfList(list))
            }
        };
    }
    if subs.iter().all(|s| matches!(s, SingleSubtype::Land(_))) {
        let mut mask = 0u32;
        for s in subs {
            if let SingleSubtype::Land(lt) = s {
                mask |= lt.bit();
            }
        }
        return Ok(SubtypeFilter::NotLandMask(mask));
    }
    let list = SubtypeList::from_slice(subs).ok_or(LoweringError::Unsupported(
        "v2:filter:too-many-forbidden-subtypes",
    ))?;
    Ok(SubtypeFilter::NoneOfList(list))
}

fn apply_colors_to_clause(clause: &mut ObjectClause, filter: &v2::ObjectFilter) -> LowerResult<()> {
    let both_colors = filter.flags.contains(v2::FilterFlags::BOTH_COLORS);
    if both_colors && filter.any_colors.0.count_ones() >= 2 {
        return Err(LoweringError::Unsupported("v2:filter:both-colors"));
    }

    let any_mask = lower_color_mask(filter.any_colors);
    let forbid_mask = lower_color_mask(filter.forbidden_colors);

    clause.color = match filter.color_arity {
        None => {
            if any_mask.is_empty() && forbid_mask.is_empty() {
                ObjectColorFilter::Any
            } else {
                ObjectColorFilter::Mask {
                    any_of: any_mask,
                    none_of: forbid_mask,
                }
            }
        }
        Some(ast::ColorArity::Colorless) => {
            if !any_mask.is_empty() {
                return Err(LoweringError::Unsupported("v2:filter:colorless-with-color"));
            }
            ObjectColorFilter::Colorless
        }
        Some(ast::ColorArity::Colored) => {
            if any_mask.is_empty() && forbid_mask.is_empty() {
                ObjectColorFilter::Colored
            } else if !any_mask.is_empty() {
                // Requiring any specific color already implies the object is colored.
                ObjectColorFilter::Mask {
                    any_of: any_mask,
                    none_of: forbid_mask,
                }
            } else {
                return Err(LoweringError::Unsupported(
                    "v2:filter:colored-with-forbidden-color",
                ));
            }
        }
        Some(ast::ColorArity::Multicolored) => {
            if any_mask.is_empty() && forbid_mask.is_empty() {
                ObjectColorFilter::Multicolored
            } else {
                return Err(LoweringError::Unsupported(
                    "v2:filter:multicolored-with-mask",
                ));
            }
        }
        Some(ast::ColorArity::Monocolored) => {
            if any_mask.is_empty() && forbid_mask.is_empty() {
                ObjectColorFilter::Monocolored
            } else {
                return Err(LoweringError::Unsupported(
                    "v2:filter:monocolored-with-mask",
                ));
            }
        }
    };
    Ok(())
}

fn apply_stat_bounds_to_clause(
    clause: &mut ObjectClause,
    bounds: &[v2::StatBound],
    ctx: &LowerCtx<'_>,
) -> LowerResult<()> {
    for bound in bounds {
        // Check self-relative power/toughness comparisons first ("toughness greater than its power").
        if let v2::ValueExpr::ObjectStat {
            subject,
            stat: rhs_stat,
        } = bound.value.as_ref()
            && matches!(
                subject.as_ref(),
                v2::SubjectRef::Anaphora(v2::AnaphoraRef::It)
            )
        {
            match (bound.stat, bound.op, rhs_stat) {
                (ast::StatKind::Power, ast::CompareOp::Gt, ast::StatKind::Toughness)
                | (ast::StatKind::Toughness, ast::CompareOp::Lt, ast::StatKind::Power) => {
                    clause.flags = clause
                        .flags
                        .union(ObjectFilterFlags::POWER_GREATER_THAN_TOUGHNESS);
                    continue;
                }
                (ast::StatKind::Toughness, ast::CompareOp::Gt, ast::StatKind::Power)
                | (ast::StatKind::Power, ast::CompareOp::Lt, ast::StatKind::Toughness) => {
                    clause.flags = clause
                        .flags
                        .union(ObjectFilterFlags::TOUGHNESS_GREATER_THAN_POWER);
                    continue;
                }
                (ast::StatKind::Power, ast::CompareOp::Eq, ast::StatKind::Toughness)
                | (ast::StatKind::Toughness, ast::CompareOp::Eq, ast::StatKind::Power) => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::SAME_TOTAL_PT);
                    continue;
                }
                _ => {}
            }
        }

        match bound.stat {
            ast::StatKind::Power => {
                if clause.power != PowerFilter::Any {
                    return Err(LoweringError::Unsupported(
                        "v2:filter:multiple-power-bounds",
                    ));
                }
                clause.power = lower_power_filter(bound.op, bound.value.as_ref(), ctx)?;
            }
            ast::StatKind::Toughness => {
                if clause.toughness != ToughnessFilter::Any {
                    return Err(LoweringError::Unsupported(
                        "v2:filter:multiple-toughness-bounds",
                    ));
                }
                clause.toughness = lower_toughness_filter(bound.op, bound.value.as_ref(), ctx)?;
            }
            ast::StatKind::ManaValue => {
                if clause.mana_value != ManaValueFilter::Any {
                    return Err(LoweringError::Unsupported(
                        "v2:filter:multiple-mana-value-bounds",
                    ));
                }
                clause.mana_value = lower_mana_value_filter(bound.op, bound.value.as_ref(), ctx)?;
            }
            ast::StatKind::Loyalty => {
                return Err(LoweringError::Unsupported("v2:filter:loyalty-stat-bound"));
            }
        }
    }
    Ok(())
}

fn lower_power_filter(
    op: ast::CompareOp,
    val: &v2::ValueExpr,
    ctx: &LowerCtx<'_>,
) -> LowerResult<PowerFilter> {
    if let Some(n) = lower_value_const(val) {
        return match op {
            ast::CompareOp::Ge => i16::try_from(n)
                .map(PowerFilter::AtLeast)
                .map_err(|_| LoweringError::Unsupported("v2:filter:power-range")),
            ast::CompareOp::Gt => n
                .checked_add(1)
                .and_then(|v| i16::try_from(v).ok())
                .map(PowerFilter::AtLeast)
                .ok_or(LoweringError::Unsupported("v2:filter:power-range")),
            ast::CompareOp::Le => u16::try_from(n)
                .map(PowerFilter::AtMost)
                .map_err(|_| LoweringError::Unsupported("v2:filter:negative-power-max")),
            ast::CompareOp::Lt => n
                .checked_sub(1)
                .and_then(|v| u16::try_from(v).ok())
                .map(PowerFilter::AtMost)
                .ok_or(LoweringError::Unsupported("v2:filter:negative-power-max")),
            ast::CompareOp::Eq => i16::try_from(n)
                .map(PowerFilter::Equal)
                .map_err(|_| LoweringError::Unsupported("v2:filter:power-range")),
        };
    }
    if matches!(val, v2::ValueExpr::Var(v2::VarName::X)) {
        return Ok(match op {
            ast::CompareOp::Le => PowerFilter::AtMostX,
            ast::CompareOp::Eq => PowerFilter::EqualX,
            ast::CompareOp::Ge => PowerFilter::AtLeastX,
            ast::CompareOp::Lt | ast::CompareOp::Gt => PowerFilter::CompareAnchor {
                op: lower_compare_op(op),
                anchor: DynamicStatAnchor::ChosenX,
            },
        });
    }
    if let Some(anchor) = lower_dynamic_stat_anchor(val, ctx) {
        return Ok(PowerFilter::CompareAnchor {
            op: lower_compare_op(op),
            anchor,
        });
    }
    Err(LoweringError::Unsupported("v2:filter:dynamic-power-bound"))
}

fn lower_toughness_filter(
    op: ast::CompareOp,
    val: &v2::ValueExpr,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ToughnessFilter> {
    if let Some(n) = lower_value_const(val) {
        return match op {
            ast::CompareOp::Ge => i16::try_from(n)
                .map(ToughnessFilter::AtLeast)
                .map_err(|_| LoweringError::Unsupported("v2:filter:toughness-range")),
            ast::CompareOp::Gt => n
                .checked_add(1)
                .and_then(|v| i16::try_from(v).ok())
                .map(ToughnessFilter::AtLeast)
                .ok_or(LoweringError::Unsupported("v2:filter:toughness-range")),
            ast::CompareOp::Le => u16::try_from(n)
                .map(ToughnessFilter::AtMost)
                .map_err(|_| LoweringError::Unsupported("v2:filter:negative-toughness-max")),
            ast::CompareOp::Lt => n
                .checked_sub(1)
                .and_then(|v| u16::try_from(v).ok())
                .map(ToughnessFilter::AtMost)
                .ok_or(LoweringError::Unsupported(
                    "v2:filter:negative-toughness-max",
                )),
            ast::CompareOp::Eq => i16::try_from(n)
                .map(ToughnessFilter::Equal)
                .map_err(|_| LoweringError::Unsupported("v2:filter:toughness-range")),
        };
    }
    if matches!(val, v2::ValueExpr::Var(v2::VarName::X)) {
        return Ok(match op {
            ast::CompareOp::Le => ToughnessFilter::AtMostX,
            ast::CompareOp::Eq => ToughnessFilter::EqualX,
            ast::CompareOp::Ge => ToughnessFilter::AtLeastX,
            ast::CompareOp::Lt | ast::CompareOp::Gt => ToughnessFilter::CompareAnchor {
                op: lower_compare_op(op),
                anchor: DynamicStatAnchor::ChosenX,
            },
        });
    }
    if let Some(anchor) = lower_dynamic_stat_anchor(val, ctx) {
        return Ok(ToughnessFilter::CompareAnchor {
            op: lower_compare_op(op),
            anchor,
        });
    }
    Err(LoweringError::Unsupported(
        "v2:filter:dynamic-toughness-bound",
    ))
}

fn lower_mana_value_filter(
    op: ast::CompareOp,
    val: &v2::ValueExpr,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ManaValueFilter> {
    if let Some(n) = lower_value_const(val) {
        return match op {
            ast::CompareOp::Ge => u32::try_from(n)
                .map(ManaValueFilter::AtLeast)
                .map_err(|_| LoweringError::Unsupported("v2:filter:negative-mv")),
            ast::CompareOp::Gt => n
                .checked_add(1)
                .and_then(|v| u32::try_from(v).ok())
                .map(ManaValueFilter::AtLeast)
                .ok_or(LoweringError::Unsupported("v2:filter:negative-mv")),
            ast::CompareOp::Le => u32::try_from(n)
                .map(ManaValueFilter::AtMost)
                .map_err(|_| LoweringError::Unsupported("v2:filter:negative-mv")),
            ast::CompareOp::Lt => n
                .checked_sub(1)
                .and_then(|v| u32::try_from(v).ok())
                .map(ManaValueFilter::AtMost)
                .ok_or(LoweringError::Unsupported("v2:filter:negative-mv")),
            ast::CompareOp::Eq => u32::try_from(n)
                .map(ManaValueFilter::Equal)
                .map_err(|_| LoweringError::Unsupported("v2:filter:negative-mv")),
        };
    }
    if matches!(val, v2::ValueExpr::Var(v2::VarName::X)) {
        return Ok(match op {
            ast::CompareOp::Le => ManaValueFilter::AtMostX,
            ast::CompareOp::Eq => ManaValueFilter::EqualX,
            ast::CompareOp::Ge => ManaValueFilter::AtLeastX,
            ast::CompareOp::Lt | ast::CompareOp::Gt => ManaValueFilter::CompareAnchor {
                op: lower_compare_op(op),
                anchor: DynamicStatAnchor::ChosenX,
            },
        });
    }
    if let Some(anchor) = lower_dynamic_stat_anchor(val, ctx) {
        return Ok(ManaValueFilter::CompareAnchor {
            op: lower_compare_op(op),
            anchor,
        });
    }
    Err(LoweringError::Unsupported("v2:filter:dynamic-mv-bound"))
}

fn apply_flags_to_clause(
    clause: &mut ObjectClause,
    flags: v2::FilterFlags,
    in_trigger: bool,
) -> LowerResult<()> {
    let mut rem = flags.0;

    let mut take = |bit: v2::FilterFlags| -> bool {
        if (rem & bit.0) != 0 {
            rem &= !bit.0;
            true
        } else {
            false
        }
    };

    if take(v2::FilterFlags::EXCLUDE_SELF) {
        apply_exclude_self(clause, in_trigger)?;
    }
    if take(v2::FilterFlags::ATTACKED_THIS_TURN) {
        clause.flags = clause.flags.union(ObjectFilterFlags::ATTACKED_THIS_TURN);
    }
    if take(v2::FilterFlags::DEALT_DAMAGE_THIS_TURN) {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
    }
    if take(v2::FilterFlags::DIED_THIS_TURN) {
        clause.flags = clause.flags.union(ObjectFilterFlags::DIED_THIS_TURN);
    }
    if take(v2::FilterFlags::ENTERED_THIS_TURN) {
        clause.flags = clause.flags.union(ObjectFilterFlags::ENTERED_THIS_TURN);
    }
    if take(v2::FilterFlags::MODIFIED) {
        clause.flags = clause.flags.union(ObjectFilterFlags::MODIFIED);
    }
    if take(v2::FilterFlags::EQUIPPED) {
        clause.flags = clause.flags.union(ObjectFilterFlags::EQUIPPED);
    }
    if take(v2::FilterFlags::ENCHANTED) {
        clause.flags = clause.flags.union(ObjectFilterFlags::ENCHANTED);
    }
    if take(v2::FilterFlags::FACE_DOWN) {
        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_DOWN);
    }
    if take(v2::FilterFlags::FACE_UP) {
        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_UP);
    }
    if take(v2::FilterFlags::IS_COMMANDER) {
        clause.flags = clause.flags.union(ObjectFilterFlags::COMMANDER_ONLY);
    }
    if take(v2::FilterFlags::NOT_COMMANDER) {
        clause.flags = clause.flags.union(ObjectFilterFlags::NOT_COMMANDER);
    }
    if take(v2::FilterFlags::WITH_ANY_COUNTER) {
        clause.flags = clause.flags.union(ObjectFilterFlags::WITH_ANY_COUNTER);
    }
    if take(v2::FilterFlags::WITHOUT_COUNTERS) {
        clause.flags = clause.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
    }
    if take(v2::FilterFlags::EXILED_WITH_SELF) {
        clause.flags = clause.flags.union(ObjectFilterFlags::EXILED_WITH_SELF);
    }
    if take(v2::FilterFlags::KICKED) {
        clause.flags = clause.flags.union(ObjectFilterFlags::KICKED);
    }
    if take(v2::FilterFlags::GOADED) {
        clause.flags = clause.flags.union(ObjectFilterFlags::GOADED);
    }
    if take(v2::FilterFlags::SUSPECTED) {
        clause.flags = clause.flags.union(ObjectFilterFlags::SUSPECTED);
    }
    if take(v2::FilterFlags::SUSPENDED) {
        clause.flags = clause.flags.union(ObjectFilterFlags::SUSPENDED);
    }
    if take(v2::FilterFlags::HAS_ADVENTURE) {
        clause.flags = clause.flags.union(ObjectFilterFlags::HAS_ADVENTURE);
    }
    if take(v2::FilterFlags::HAS_X_IN_MANA_COST) {
        clause.flags = clause.flags.union(ObjectFilterFlags::HAS_X_IN_COST);
    }
    if take(v2::FilterFlags::RENOWNED) {
        clause.flags = clause.flags.union(ObjectFilterFlags::RENOWNED);
    }
    // `LKI_PAST_TENSE` is a grammatical marker ("that you controlled"); engine LKI evaluation
    // uses last-known controller automatically.
    let _ = take(v2::FilterFlags::LKI_PAST_TENSE);
    // `BOTH_COLORS` was already validated in `apply_colors_to_clause`.
    let _ = take(v2::FilterFlags::BOTH_COLORS);

    if rem != 0 {
        return Err(LoweringError::Unsupported(flag_error_code(rem)));
    }
    Ok(())
}

const fn flag_error_code(rem: u32) -> &'static str {
    if (rem & v2::FilterFlags::DIFFERENT_NAMES.0) != 0 {
        "v2:filter:flag:different-names"
    } else if (rem & v2::FilterFlags::TRANSFORMED.0) != 0 {
        "v2:filter:flag:transformed"
    } else if (rem & v2::FilterFlags::WARPED.0) != 0 {
        "v2:filter:flag:warped"
    } else if (rem & v2::FilterFlags::DIFFERENT_POWERS.0) != 0 {
        "v2:filter:flag:different-powers"
    } else if (rem & v2::FilterFlags::AT_RANDOM.0) != 0 {
        "v2:filter:flag:at-random"
    } else if (rem & v2::FilterFlags::SINGLE_TARGET.0) != 0 {
        "v2:filter:flag:single-target"
    } else if (rem & v2::FilterFlags::STICKERED.0) != 0 {
        "v2:filter:flag:stickered"
    } else if (rem & v2::FilterFlags::NOT_ON_BATTLEFIELD.0) != 0 {
        "v2:filter:flag:not-on-battlefield"
    } else {
        "v2:filter:flag:unknown"
    }
}

fn apply_predicates_to_clause(
    clause: &mut ObjectClause,
    predicates: &[v2::FilterPredicate],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<(ObjectClause, ObjectClause)>> {
    let mut deferred_any_of: Vec<&[v2::ObjectFilter]> = Vec::new();
    let mut deferred_exclude_basic_land = false;

    for pred in predicates {
        match pred {
            v2::FilterPredicate::AnyOf(subs) => {
                deferred_any_of.push(subs.as_slice());
            }
            v2::FilterPredicate::AttachedTo(subj) => match subj.as_ref() {
                v2::SubjectRef::SelfObject
                | v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSource) => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::ATTACHED_TO_SOURCE);
                }
                v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget) => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::ATTACHED_TO_TARGET);
                }
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatObject,
                ) => {
                    let flag = if ctx.has_object_target {
                        ObjectFilterFlags::ATTACHED_TO_TARGET
                    } else if ctx.in_trigger {
                        ObjectFilterFlags::ATTACHED_TO_TRIGGER
                    } else {
                        ObjectFilterFlags::ATTACHED_TO_SOURCE
                    };
                    clause.flags = clause.flags.union(flag);
                }
                _ => {
                    return Err(LoweringError::Unsupported("v2:filter:pred:attached-to"));
                }
            },
            v2::FilterPredicate::SharesPropertyWith {
                property,
                reference,
                negated,
            } => {
                apply_shares_property(clause, *property, reference, *negated, ctx)?;
            }
            v2::FilterPredicate::AffectedByPriorAction(kind) => match kind {
                v2::PriorActionKind::ExiledThisWay
                | v2::PriorActionKind::SacrificedThisWay
                | v2::PriorActionKind::DestroyedThisWay
                | v2::PriorActionKind::DiscardedThisWay
                | v2::PriorActionKind::RevealedThisWay
                | v2::PriorActionKind::ReturnedThisWay
                | v2::PriorActionKind::MilledThisWay
                | v2::PriorActionKind::SearchedThisWay
                | v2::PriorActionKind::CreatedThisWay => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::IN_LAST_MOVED);
                }
                v2::PriorActionKind::DealtDamageThisWay if ctx.last_damage_was_self => {
                    clause.flags = clause
                        .flags
                        .union(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN);
                }
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:filter:pred:prior-action-kind",
                    ));
                }
            },
            v2::FilterPredicate::Exclude(subj) => match subj.as_ref() {
                v2::SubjectRef::SelfObject => apply_exclude_self(clause, ctx.in_trigger)?,
                v2::SubjectRef::Group(ex_flt) => {
                    if is_basic_land_only_filter(ex_flt) {
                        deferred_exclude_basic_land = true;
                    } else {
                        apply_excluded_group_filter(clause, ex_flt, ctx)?;
                    }
                }
                _ => return Err(LoweringError::Unsupported("v2:filter:pred:exclude")),
            },
            v2::FilterPredicate::HasCounterKind(ck) => match ck {
                v2::CounterKind::Any => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::WITH_ANY_COUNTER);
                }
                v2::CounterKind::PlusOnePlusOne => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER);
                }
                v2::CounterKind::MinusOneMinusOne => {
                    clause.flags = clause
                        .flags
                        .union(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER);
                }
                other => {
                    let kind = lower_counter_kind(other)?;
                    if let Some(existing) = clause.required_counter_kind {
                        if existing != kind {
                            return Err(LoweringError::Unsupported(
                                "v2:filter:pred:multiple-counter-kinds",
                            ));
                        }
                    } else {
                        clause.required_counter_kind = Some(kind);
                    }
                }
            },
            v2::FilterPredicate::HasNamedAbility(kw) => {
                if *kw == v2::KeywordKind::Toxic {
                    clause.flags = clause.flags.union(ObjectFilterFlags::HAS_TOXIC);
                } else {
                    let ability = lower_keyword_kind_plain(*kw)?;
                    if let Some(existing) = clause.keyword {
                        if existing != ability {
                            return Err(LoweringError::Unsupported(
                                "v2:filter:pred:multiple-keywords",
                            ));
                        }
                    } else {
                        clause.keyword = Some(ability);
                    }
                }
            }
            v2::FilterPredicate::NotOwnedByController => {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::NOT_OWNED_BY_CONTROLLER);
            }
            v2::FilterPredicate::HasBasicLandType => {
                clause.flags = clause.flags.union(ObjectFilterFlags::HAS_BASIC_LAND_TYPE);
            }
            v2::FilterPredicate::OfChosenColor => {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_COLOR);
            }
            v2::FilterPredicate::OfChosenCreatureType => {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_TYPE);
            }
            v2::FilterPredicate::OfChosenName => {
                clause.flags = clause.flags.union(ObjectFilterFlags::MATCHES_CHOSEN_NAME);
            }
            v2::FilterPredicate::NamedSelf => {
                clause.source_relation = match clause.source_relation {
                    SourceRelation::Any | SourceRelation::SharingNameWithAbilitySource => {
                        SourceRelation::SharingNameWithAbilitySource
                    }
                    SourceRelation::OtherThanAbilitySource
                    | SourceRelation::OtherThanTriggerSource
                    | SourceRelation::OtherThanAbilitySourceSharingName => {
                        SourceRelation::OtherThanAbilitySourceSharingName
                    }
                    _ => {
                        return Err(LoweringError::Unsupported(
                            "v2:filter:pred:conflicting-named-self",
                        ));
                    }
                };
            }
            v2::FilterPredicate::Named(name) => {
                let key = ExactNameKey::from_name(name)
                    .ok_or(LoweringError::Unsupported("v2:filter:pred:invalid-name"))?;
                if let Some(existing) = clause.exact_name {
                    if existing != key {
                        return Err(LoweringError::Unsupported("v2:filter:pred:multiple-names"));
                    }
                } else {
                    clause.exact_name = Some(key);
                }
            }
            v2::FilterPredicate::DealtDamageBySelfThisTurn => {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN);
            }
            v2::FilterPredicate::OwnedAndControlledByYou => {
                clause.controller = ControllerFilter::You;
                clause.owner = ControllerFilter::OwnerYou;
            }
            v2::FilterPredicate::NthSpellCastThisTurn(_) => {
                return Err(LoweringError::Unsupported("v2:filter:pred:nth-spell"));
            }
            v2::FilterPredicate::TargetsSubject(subj) => {
                apply_combat_targets_subject(clause, subj, ctx)?;
            }
            v2::FilterPredicate::AttackedPlayerThisTurn(player) => {
                if clause.combat != CombatFilter::Attacking {
                    return Err(LoweringError::Unsupported(
                        "v2:filter:pred:attacked-player-this-turn",
                    ));
                }
                clause.combat = match player {
                    v2::PlayerRef::You => CombatFilter::AttackingController,
                    v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                        CombatFilter::AttackingOpponent
                    }
                    v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
                        CombatFilter::AttackingPlayer
                    }
                    _ => {
                        return Err(LoweringError::Unsupported(
                            "v2:filter:pred:attacked-player-this-turn",
                        ));
                    }
                };
            }
            v2::FilterPredicate::FromOutsideHand => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:from-outside-hand",
                ));
            }
            v2::FilterPredicate::NotManaAbility => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:not-mana-ability",
                ));
            }
            v2::FilterPredicate::HasManaAbility => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:has-mana-ability",
                ));
            }
            v2::FilterPredicate::HasNameSticker => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:has-name-sticker",
                ));
            }
            v2::FilterPredicate::AmongPriorCards => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:among-prior-cards",
                ));
            }
            v2::FilterPredicate::OfChosenPlayer(_) => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:of-chosen-player",
                ));
            }
            v2::FilterPredicate::ManaCostIn(_) => {
                return Err(LoweringError::Unsupported("v2:filter:pred:mana-cost-in"));
            }
            v2::FilterPredicate::InChosenPile => {
                return Err(LoweringError::Unsupported("v2:filter:pred:in-chosen-pile"));
            }
            v2::FilterPredicate::DrawnThisTurn => {
                return Err(LoweringError::Unsupported("v2:filter:pred:drawn-this-turn"));
            }
            v2::FilterPredicate::KickedWith(_) => {
                return Err(LoweringError::Unsupported("v2:filter:pred:kicked-with"));
            }
            v2::FilterPredicate::GreatestStat { .. } => {
                return Err(LoweringError::Unsupported("v2:filter:pred:greatest-stat"));
            }
            v2::FilterPredicate::LeastStat { .. } => {
                return Err(LoweringError::Unsupported("v2:filter:pred:least-stat"));
            }
            v2::FilterPredicate::SameController => {
                return Err(LoweringError::Unsupported("v2:filter:pred:same-controller"));
            }
            v2::FilterPredicate::OfThatColor => {
                return Err(LoweringError::Unsupported("v2:filter:pred:of-that-color"));
            }
            v2::FilterPredicate::LesserManaValue { .. } => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:lesser-mana-value",
                ));
            }
        }
    }

    // Apply `AnyOf` predicates after all conjunctive predicates so every outer predicate is
    // faithfully propagated into both branches of `ObjectFilter::Either`.
    let mut split: Option<(ObjectClause, ObjectClause)> = None;
    for subs in deferred_any_of {
        if subs.is_empty() {
            return Err(LoweringError::Unsupported("v2:filter:pred:empty-any-of"));
        }
        if subs.len() == 1 {
            let sub_f = lower_object_filter(&subs[0], ctx)?;
            match (split, sub_f) {
                (None, EngineObjectFilter::Clause(c)) => {
                    *clause = merge_clauses(*clause, c)?;
                }
                (None, EngineObjectFilter::Either(a, b)) => {
                    split = Some((merge_clauses(*clause, a)?, merge_clauses(*clause, b)?));
                }
                (Some((s1, s2)), EngineObjectFilter::Clause(c)) => {
                    split = Some((merge_clauses(s1, c)?, merge_clauses(s2, c)?));
                }
                (Some(_), EngineObjectFilter::Either(_, _)) => {
                    return Err(LoweringError::Unsupported("v2:filter:pred:nested-any-of"));
                }
            }
        } else if subs.len() == 2 && split.is_none() {
            let f1 = lower_object_filter(&subs[0], ctx)?;
            let f2 = lower_object_filter(&subs[1], ctx)?;
            let (EngineObjectFilter::Clause(c1), EngineObjectFilter::Clause(c2)) = (f1, f2) else {
                return Err(LoweringError::Unsupported("v2:filter:pred:nested-any-of"));
            };
            split = Some((merge_clauses(*clause, c1)?, merge_clauses(*clause, c2)?));
        } else {
            return Err(LoweringError::Unsupported("v2:filter:pred:any-of-arity"));
        }
    }

    if deferred_exclude_basic_land {
        // De Morgan expansion: `NOT (Basic AND Land)` <=> `(NOT Land) OR (Land AND NOT Basic)`.
        let nonland = ObjectClause {
            forbidden_types: EngineCardTypeMask::LAND,
            ..Default::default()
        };
        let nonbasic_land = ObjectClause {
            required_types: EngineCardTypeMask::LAND,
            forbidden_supertypes: lower_supertype_mask(ast::SupertypeMask::BASIC)?,
            ..Default::default()
        };
        if split.is_some() {
            return Err(LoweringError::Unsupported("v2:filter:pred:complex-exclude"));
        }
        split = Some((
            merge_clauses(*clause, nonland)?,
            merge_clauses(*clause, nonbasic_land)?,
        ));
    }

    Ok(split)
}

fn is_basic_land_only_filter(ex: &v2::ObjectFilter) -> bool {
    let is_land = (ex.all_types == ast::CardTypeMask::LAND && ex.any_types.is_empty())
        || (ex.any_types == ast::CardTypeMask::LAND && ex.all_types.is_empty());
    is_land
        && ex.required_supertypes == ast::SupertypeMask::BASIC
        && ex.forbidden_types.is_empty()
        && ex.forbidden_supertypes.is_empty()
        && ex.subtypes.is_empty()
        && ex.forbidden_subtypes.is_empty()
        && ex.any_colors.is_empty()
        && ex.forbidden_colors.is_empty()
        && ex.color_arity.is_none()
        && ex.controller.is_none()
        && ex.owner.is_none()
        && ex.zones.is_empty()
        && ex.combat.is_none()
        && ex.tapped.is_none()
        && ex.token.is_none()
        && ex.required_keywords.is_empty()
        && ex.forbidden_keywords.is_empty()
        && ex.stat_bounds.is_empty()
        && ex.flags.is_empty()
        && ex.predicates.is_empty()
}

fn is_self_combat_ref(subj: &v2::SubjectRef, ctx: &LowerCtx<'_>) -> bool {
    matches!(
        subj,
        v2::SubjectRef::SelfObject | v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSource)
    ) || (!ctx.has_object_target
        && (ctx.self_antecedent || !ctx.in_trigger)
        && matches!(
            subj,
            v2::SubjectRef::Anaphora(
                v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatObject
            )
        ))
}

fn is_plain_planeswalker_controlled_by(subj: &v2::SubjectRef, want_you: bool) -> bool {
    let (v2::SubjectRef::Group(flt) | v2::SubjectRef::All(flt) | v2::SubjectRef::Each(flt)) = subj
    else {
        return false;
    };
    let pw_only = (flt.all_types == ast::CardTypeMask::PLANESWALKER && flt.any_types.is_empty())
        || (flt.any_types == ast::CardTypeMask::PLANESWALKER && flt.all_types.is_empty());
    let ctrl_ok = if want_you {
        flt.controller == Some(ast::ControllerRelation::You)
    } else {
        matches!(
            flt.controller,
            Some(ast::ControllerRelation::Opponent | ast::ControllerRelation::Opponents)
        )
    };
    pw_only
        && ctrl_ok
        && flt.zones.is_empty()
        && flt.subtypes.is_empty()
        && is_plain_anchor_count_filter(flt)
}

fn apply_combat_targets_subject(
    clause: &mut ObjectClause,
    subj: &v2::SubjectRef,
    ctx: &LowerCtx<'_>,
) -> LowerResult<()> {
    match clause.combat {
        CombatFilter::Blocking => {
            if is_self_combat_ref(subj, ctx) {
                clause.combat = CombatFilter::BlockingSource;
                return Ok(());
            }
            if matches!(
                subj,
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::EnchantedCreature
                        | v2::AnaphoraRef::EquippedCreature
                        | v2::AnaphoraRef::EnchantedPermanent
                )
            ) || (ctx.attached_host_antecedent
                && !ctx.has_object_target
                && matches!(
                    subj,
                    v2::SubjectRef::Anaphora(
                        v2::AnaphoraRef::It
                            | v2::AnaphoraRef::ThatCreature
                            | v2::AnaphoraRef::ThatPermanent
                            | v2::AnaphoraRef::ThatObject
                    )
                ))
            {
                clause.combat = CombatFilter::BlockingAttachedHost;
                return Ok(());
            }
        }
        CombatFilter::Blocked => {
            if is_self_combat_ref(subj, ctx) {
                clause.combat = CombatFilter::BlockedBySource;
                return Ok(());
            }
        }
        CombatFilter::Attacking => match subj {
            v2::SubjectRef::Player(v2::PlayerRef::You) => {
                clause.combat = CombatFilter::AttackingController;
                return Ok(());
            }
            v2::SubjectRef::Player(v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent) => {
                clause.combat = CombatFilter::AttackingOpponent;
                return Ok(());
            }
            v2::SubjectRef::Player(v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer) => {
                clause.combat = CombatFilter::AttackingPlayer;
                return Ok(());
            }
            v2::SubjectRef::Compound(parts) if parts.len() == 2 => {
                let (a, b) = (&parts[0], &parts[1]);
                if (matches!(a, v2::SubjectRef::Player(v2::PlayerRef::You))
                    && is_plain_planeswalker_controlled_by(b, true))
                    || (matches!(b, v2::SubjectRef::Player(v2::PlayerRef::You))
                        && is_plain_planeswalker_controlled_by(a, true))
                {
                    clause.combat = CombatFilter::AttackingControllerOrPlaneswalker;
                    return Ok(());
                }
                if (matches!(
                    a,
                    v2::SubjectRef::Player(
                        v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent
                    )
                ) && is_plain_planeswalker_controlled_by(b, false))
                    || (matches!(
                        b,
                        v2::SubjectRef::Player(
                            v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent
                        )
                    ) && is_plain_planeswalker_controlled_by(a, false))
                {
                    clause.combat = CombatFilter::AttackingOpponentOrPlaneswalker;
                    return Ok(());
                }
            }
            _ => {}
        },
        _ => {}
    }
    Err(LoweringError::Unsupported("v2:filter:pred:targets-subject"))
}

fn apply_shares_property(
    clause: &mut ObjectClause,
    property: v2::SharedProperty,
    reference: &v2::SubjectRef,
    negated: bool,
    ctx: &LowerCtx<'_>,
) -> LowerResult<()> {
    let shared_stat = match property {
        v2::SharedProperty::ManaValue => Some(ast::StatKind::ManaValue),
        v2::SharedProperty::Power => Some(ast::StatKind::Power),
        v2::SharedProperty::Toughness => Some(ast::StatKind::Toughness),
        _ => None,
    };
    if let Some(stat) = shared_stat {
        let stat_expr = v2::ValueExpr::ObjectStat {
            subject: Box::new(reference.clone()),
            stat,
        };
        let Some(anchor) = lower_dynamic_stat_anchor(&stat_expr, ctx) else {
            return Err(LoweringError::Unsupported(
                "v2:filter:pred:shares-property-ref",
            ));
        };
        let op = if negated {
            Comparison::NotEqual
        } else {
            Comparison::Equal
        };
        match stat {
            ast::StatKind::ManaValue => {
                if clause.mana_value != ManaValueFilter::Any {
                    return Err(LoweringError::Unsupported(
                        "v2:filter:multiple-mana-value-bounds",
                    ));
                }
                clause.mana_value = ManaValueFilter::CompareAnchor { op, anchor };
            }
            ast::StatKind::Power => {
                if clause.power != PowerFilter::Any {
                    return Err(LoweringError::Unsupported(
                        "v2:filter:multiple-power-bounds",
                    ));
                }
                clause.power = PowerFilter::CompareAnchor { op, anchor };
            }
            ast::StatKind::Toughness => {
                if clause.toughness != ToughnessFilter::Any {
                    return Err(LoweringError::Unsupported(
                        "v2:filter:multiple-toughness-bounds",
                    ));
                }
                clause.toughness = ToughnessFilter::CompareAnchor { op, anchor };
            }
            ast::StatKind::Loyalty => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:shares-property-ref",
                ));
            }
        }
        return Ok(());
    }

    let is_source_ref = matches!(
        reference,
        v2::SubjectRef::SelfObject | v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSource)
    ) || (!ctx.has_object_target
        && !ctx.in_trigger
        && ctx.last_moved_binding.is_none()
        && matches!(
            reference,
            v2::SubjectRef::Anaphora(
                v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::ThatObject
            )
        ));
    if is_source_ref {
        return match (property, negated) {
            (v2::SharedProperty::Color, false) => {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::SHARES_COLOR_WITH_SOURCE);
                Ok(())
            }
            (v2::SharedProperty::CreatureType, false) => {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_SOURCE);
                Ok(())
            }
            (v2::SharedProperty::CardType, false) => {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::SHARES_CARD_TYPE_WITH_SOURCE);
                Ok(())
            }
            (v2::SharedProperty::Name, false) => {
                clause.flags = clause.flags.union(ObjectFilterFlags::SAME_NAME_AS_SOURCE);
                Ok(())
            }
            (v2::SharedProperty::Name, true) => {
                clause.flags = clause
                    .flags
                    .union(ObjectFilterFlags::DIFFERENT_NAME_FROM_SOURCE);
                Ok(())
            }
            _ => Err(LoweringError::Unsupported(
                "v2:filter:pred:shares-property-source",
            )),
        };
    }

    if property == v2::SharedProperty::Name
        && negated
        && let v2::SubjectRef::Group(ref_flt) = reference
        && ref_flt.flags == v2::FilterFlags::EXCLUDE_SELF
        && ref_flt.controller == Some(ast::ControllerRelation::You)
        && ref_flt.all_types.is_empty()
        && ref_flt.any_types.is_empty()
        && ref_flt.forbidden_types.is_empty()
        && ref_flt.required_supertypes.is_empty()
        && ref_flt.forbidden_supertypes.is_empty()
        && ref_flt.subtypes.is_empty()
        && ref_flt.forbidden_subtypes.is_empty()
        && ref_flt.any_colors.is_empty()
        && ref_flt.forbidden_colors.is_empty()
        && ref_flt.color_arity.is_none()
        && ref_flt.owner.is_none()
        && ref_flt.zones.is_empty()
        && ref_flt.combat.is_none()
        && ref_flt.tapped.is_none()
        && ref_flt.token.is_none()
        && ref_flt.required_keywords.is_empty()
        && ref_flt.forbidden_keywords.is_empty()
        && ref_flt.stat_bounds.is_empty()
        && ref_flt.predicates.is_empty()
    {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::UNIQUE_NAME_AMONG_CONTROLLED);
        return Ok(());
    }

    match reference {
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget) => {
            apply_shares_property_with_target(clause, property, negated)
        }
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatCard
            | v2::AnaphoraRef::ThatSpell
            | v2::AnaphoraRef::ThatLand
            | v2::AnaphoraRef::ThatToken
            | v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatObject,
        ) if ctx.has_object_target => apply_shares_property_with_target(clause, property, negated),
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatCard
            | v2::AnaphoraRef::ThatSpell
            | v2::AnaphoraRef::ThatLand
            | v2::AnaphoraRef::ThatToken
            | v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatObject
            | v2::AnaphoraRef::EquippedCreature
            | v2::AnaphoraRef::EnchantedCreature
            | v2::AnaphoraRef::EnchantedPermanent,
        ) if ctx.in_trigger && ctx.last_moved_binding.is_none() => {
            apply_shares_property_with_trigger(clause, property, negated)
        }
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::SacrificedObject
            | v2::AnaphoraRef::ExiledObject
            | v2::AnaphoraRef::TheExiledCard
            | v2::AnaphoraRef::DiscardedObject
            | v2::AnaphoraRef::RevealedCard,
        ) => apply_shares_property_with_last_moved(clause, property, negated),
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatCard
            | v2::AnaphoraRef::ThatSpell
            | v2::AnaphoraRef::ThatLand
            | v2::AnaphoraRef::ThatToken
            | v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatObject,
        ) if ctx.last_moved_binding.is_some() => {
            apply_shares_property_with_last_moved(clause, property, negated)
        }
        _ => Err(LoweringError::Unsupported(
            "v2:filter:pred:shares-property-ref",
        )),
    }
}

fn apply_shares_property_with_trigger(
    clause: &mut ObjectClause,
    property: v2::SharedProperty,
    negated: bool,
) -> LowerResult<()> {
    match (property, negated) {
        (v2::SharedProperty::Color, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SHARES_COLOR_WITH_TRIGGER);
            Ok(())
        }
        (v2::SharedProperty::CreatureType, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_TRIGGER);
            Ok(())
        }
        (v2::SharedProperty::CardType, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SHARES_CARD_TYPE_WITH_TRIGGER);
            Ok(())
        }
        (v2::SharedProperty::Name, false) => {
            clause.flags = clause.flags.union(ObjectFilterFlags::SAME_NAME_AS_TRIGGER);
            Ok(())
        }
        _ => Err(LoweringError::Unsupported(
            "v2:filter:pred:shares-property-trigger",
        )),
    }
}

fn apply_shares_property_with_last_moved(
    clause: &mut ObjectClause,
    property: v2::SharedProperty,
    negated: bool,
) -> LowerResult<()> {
    match (property, negated) {
        (v2::SharedProperty::Color, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SHARES_COLOR_WITH_LAST_SACRIFICED);
            Ok(())
        }
        (v2::SharedProperty::CreatureType, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_LAST_MOVED);
            Ok(())
        }
        (v2::SharedProperty::CardType, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SHARES_CARD_TYPE_WITH_LAST_MOVED);
            Ok(())
        }
        (v2::SharedProperty::Name, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SAME_NAME_AS_LAST_MOVED);
            Ok(())
        }
        _ => Err(LoweringError::Unsupported(
            "v2:filter:pred:shares-property-last-moved",
        )),
    }
}

fn apply_shares_property_with_target(
    clause: &mut ObjectClause,
    property: v2::SharedProperty,
    negated: bool,
) -> LowerResult<()> {
    match (property, negated) {
        (v2::SharedProperty::Color, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SHARES_COLOR_WITH_TARGET);
            Ok(())
        }
        (v2::SharedProperty::CreatureType, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_TARGET);
            Ok(())
        }
        (v2::SharedProperty::CardType, false) => {
            clause.flags = clause
                .flags
                .union(ObjectFilterFlags::SHARES_CARD_TYPE_WITH_TARGET);
            Ok(())
        }
        (v2::SharedProperty::Name, false) => {
            clause.flags = clause.flags.union(ObjectFilterFlags::SAME_NAME_AS_TARGET);
            Ok(())
        }
        _ => Err(LoweringError::Unsupported(
            "v2:filter:pred:shares-property-target",
        )),
    }
}

fn apply_excluded_group_filter(
    clause: &mut ObjectClause,
    ex: &v2::ObjectFilter,
    ctx: &LowerCtx<'_>,
) -> LowerResult<()> {
    // Only fold simple single-axis exclusions (single forbidden card type, subtype, color,
    // counter kind, keyword, token, tapped, or self exclusion) so we never approximate
    // complex negated conjunctions.
    let no_extra = ex.forbidden_types.is_empty()
        && ex.forbidden_supertypes.is_empty()
        && ex.forbidden_subtypes.is_empty()
        && ex.forbidden_colors.is_empty()
        && ex.color_arity.is_none()
        && ex.controller.is_none()
        && ex.owner.is_none()
        && ex.zones.is_empty()
        && ex.combat.is_none()
        && ex.forbidden_keywords.is_empty()
        && ex.stat_bounds.is_empty();
    if !no_extra {
        return Err(LoweringError::Unsupported("v2:filter:pred:complex-exclude"));
    }
    let has_types = !ex.all_types.is_empty() || !ex.any_types.is_empty();
    let has_supertypes = !ex.required_supertypes.is_empty();
    let has_subs = !ex.subtypes.is_empty();
    let has_colors = !ex.any_colors.is_empty();
    let has_preds = !ex.predicates.is_empty();
    let has_kws = !ex.required_keywords.is_empty();
    let has_token = ex.token.is_some();
    let has_tapped = ex.tapped.is_some();
    let has_flags = !ex.flags.is_empty();
    let active_axes = u8::from(has_types)
        + u8::from(has_supertypes)
        + u8::from(has_subs)
        + u8::from(has_colors)
        + u8::from(has_preds)
        + u8::from(has_kws)
        + u8::from(has_token)
        + u8::from(has_tapped)
        + u8::from(has_flags);
    if active_axes != 1 {
        return Err(LoweringError::Unsupported(
            "v2:filter:pred:multi-axis-exclude",
        ));
    }
    if has_supertypes {
        if ex.required_supertypes.0.count_ones() != 1 {
            return Err(LoweringError::Unsupported("v2:filter:pred:complex-exclude"));
        }
        clause.forbidden_supertypes = clause
            .forbidden_supertypes
            .union(lower_supertype_mask(ex.required_supertypes)?);
        return Ok(());
    }
    if has_preds {
        if let [v2::FilterPredicate::HasCounterKind(ck)] = ex.predicates.as_slice() {
            match ck {
                v2::CounterKind::Any => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
                    return Ok(());
                }
                other => {
                    let kind = lower_counter_kind(other)?;
                    if let Some(existing) = clause.forbidden_counter_kind {
                        if existing != kind {
                            return Err(LoweringError::Unsupported(
                                "v2:filter:pred:multiple-forbidden-counters",
                            ));
                        }
                    } else {
                        clause.forbidden_counter_kind = Some(kind);
                    }
                    return Ok(());
                }
            }
        }
        return Err(LoweringError::Unsupported("v2:filter:pred:complex-exclude"));
    }
    if has_kws {
        if let [kw] = ex.required_keywords.as_slice() {
            let ability = lower_keyword_kind_plain(*kw)?;
            if let Some(existing) = clause.forbidden_keyword {
                if existing != ability {
                    return Err(LoweringError::Unsupported(
                        "v2:filter:pred:multiple-forbidden-keywords",
                    ));
                }
            } else {
                clause.forbidden_keyword = Some(ability);
            }
            return Ok(());
        }
        return Err(LoweringError::Unsupported("v2:filter:pred:complex-exclude"));
    }
    if let Some(tok) = ex.token {
        let want = !tok;
        if clause.token.is_some_and(|prev| prev != want) {
            return Err(LoweringError::Unsupported("v2:filter:pred:token-conflict"));
        }
        clause.token = Some(want);
        return Ok(());
    }
    if let Some(tap) = ex.tapped {
        let want = !tap;
        if clause.tapped.is_some_and(|prev| prev != want) {
            return Err(LoweringError::Unsupported("v2:filter:pred:tapped-conflict"));
        }
        clause.tapped = Some(want);
        return Ok(());
    }
    if has_flags {
        if ex.flags == v2::FilterFlags::EXCLUDE_SELF {
            apply_exclude_self(clause, ctx.in_trigger)?;
            return Ok(());
        }
        return Err(LoweringError::Unsupported("v2:filter:pred:complex-exclude"));
    }
    if has_types {
        if ex.all_types.0.count_ones() + ex.any_types.0.count_ones() > 1 && !ex.all_types.is_empty()
        {
            return Err(LoweringError::Unsupported(
                "v2:filter:pred:conjunctive-type-exclude",
            ));
        }
        let mask = lower_engine_type_mask(ex.all_types.union(ex.any_types));
        clause.forbidden_types = clause.forbidden_types.union(mask);
        return Ok(());
    }
    if has_subs {
        let mut rem = Vec::new();
        for raw in &ex.subtypes {
            let sub = resolve_single_subtype(raw, ast::CardTypeMask::EMPTY, ctx)?;
            match sub {
                SingleSubtype::Aura => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::NON_AURA);
                }
                SingleSubtype::Enchantment(spellwake_engine::program::EnchantmentSubtype::Saga) => {
                    clause.flags = clause.flags.union(ObjectFilterFlags::NON_SAGA);
                }
                other => rem.push(other),
            }
        }
        if !rem.is_empty() {
            if clause.subtypes != SubtypeFilter::Any {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:exclude-subtype-conflict",
                ));
            }
            clause.subtypes = build_negative_subtype_filter(&rem)?;
        }
        return Ok(());
    }
    if has_colors {
        let forb = lower_color_mask(ex.any_colors);
        clause.color = match clause.color {
            ObjectColorFilter::Any => ObjectColorFilter::none_of(forb),
            ObjectColorFilter::Mask { any_of, none_of } => ObjectColorFilter::Mask {
                any_of,
                none_of: none_of.union(forb),
            },
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:filter:pred:exclude-color-conflict",
                ));
            }
        };
        return Ok(());
    }
    Err(LoweringError::Unsupported("v2:filter:pred:empty-exclude"))
}

fn merge_clauses(mut base: ObjectClause, extra: ObjectClause) -> LowerResult<ObjectClause> {
    base.any_types = base.any_types.union(extra.any_types);
    base.required_types = base.required_types.union(extra.required_types);
    base.forbidden_types = base.forbidden_types.union(extra.forbidden_types);
    base.required_supertypes = base.required_supertypes.union(extra.required_supertypes);
    base.forbidden_supertypes = base.forbidden_supertypes.union(extra.forbidden_supertypes);
    base.flags = base.flags.union(extra.flags);

    if extra.subtypes != SubtypeFilter::Any {
        if base.subtypes != SubtypeFilter::Any && base.subtypes != extra.subtypes {
            return Err(LoweringError::Unsupported("v2:filter:merge-subtypes"));
        }
        base.subtypes = extra.subtypes;
    }
    if extra.controller != ControllerFilter::Any {
        if base.controller != ControllerFilter::Any && base.controller != extra.controller {
            return Err(LoweringError::Unsupported("v2:filter:merge-controller"));
        }
        base.controller = extra.controller;
    }
    if extra.owner != ControllerFilter::Any {
        if base.owner != ControllerFilter::Any && base.owner != extra.owner {
            return Err(LoweringError::Unsupported("v2:filter:merge-owner"));
        }
        base.owner = extra.owner;
    }
    if extra.source_relation != SourceRelation::Any {
        if base.source_relation != SourceRelation::Any
            && base.source_relation != extra.source_relation
        {
            return Err(LoweringError::Unsupported("v2:filter:merge-source-rel"));
        }
        base.source_relation = extra.source_relation;
    }
    if extra.color != ObjectColorFilter::Any {
        if base.color != ObjectColorFilter::Any && base.color != extra.color {
            return Err(LoweringError::Unsupported("v2:filter:merge-color"));
        }
        base.color = extra.color;
    }
    if extra.combat != CombatFilter::Any {
        if base.combat != CombatFilter::Any && base.combat != extra.combat {
            return Err(LoweringError::Unsupported("v2:filter:merge-combat"));
        }
        base.combat = extra.combat;
    }
    if extra.power != PowerFilter::Any {
        if base.power != PowerFilter::Any && base.power != extra.power {
            return Err(LoweringError::Unsupported("v2:filter:merge-power"));
        }
        base.power = extra.power;
    }
    if extra.toughness != ToughnessFilter::Any {
        if base.toughness != ToughnessFilter::Any && base.toughness != extra.toughness {
            return Err(LoweringError::Unsupported("v2:filter:merge-toughness"));
        }
        base.toughness = extra.toughness;
    }
    if extra.mana_value != ManaValueFilter::Any {
        if base.mana_value != ManaValueFilter::Any && base.mana_value != extra.mana_value {
            return Err(LoweringError::Unsupported("v2:filter:merge-mv"));
        }
        base.mana_value = extra.mana_value;
    }
    if let Some(kw) = extra.keyword {
        if base.keyword.is_some_and(|prev| prev != kw) {
            return Err(LoweringError::Unsupported("v2:filter:merge-keyword"));
        }
        base.keyword = Some(kw);
    }
    if let Some(kw) = extra.forbidden_keyword {
        if base.forbidden_keyword.is_some_and(|prev| prev != kw) {
            return Err(LoweringError::Unsupported(
                "v2:filter:merge-forbidden-keyword",
            ));
        }
        base.forbidden_keyword = Some(kw);
    }
    if let Some(t) = extra.tapped {
        if base.tapped.is_some_and(|prev| prev != t) {
            return Err(LoweringError::Unsupported("v2:filter:merge-tapped"));
        }
        base.tapped = Some(t);
    }
    if let Some(tok) = extra.token {
        if base.token.is_some_and(|prev| prev != tok) {
            return Err(LoweringError::Unsupported("v2:filter:merge-token"));
        }
        base.token = Some(tok);
    }
    if let Some(ck) = extra.required_counter_kind {
        if base.required_counter_kind.is_some_and(|prev| prev != ck) {
            return Err(LoweringError::Unsupported("v2:filter:merge-counter"));
        }
        base.required_counter_kind = Some(ck);
    }
    if let Some(ck) = extra.forbidden_counter_kind {
        if base.forbidden_counter_kind.is_some_and(|prev| prev != ck) {
            return Err(LoweringError::Unsupported(
                "v2:filter:merge-forbidden-counter",
            ));
        }
        base.forbidden_counter_kind = Some(ck);
    }
    if let Some(name) = extra.exact_name {
        if base.exact_name.is_some_and(|prev| prev != name) {
            return Err(LoweringError::Unsupported("v2:filter:merge-name"));
        }
        base.exact_name = Some(name);
    }
    Ok(base)
}

/// Lower a [`v2::ObjectFilter`] into an engine [`EngineObjectFilter`].
pub(crate) fn lower_object_filter(
    filter: &v2::ObjectFilter,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineObjectFilter> {
    if filter.is_ability {
        return Err(LoweringError::Unsupported("v2:filter:is-ability"));
    }

    let mut clause = ObjectClause::any();

    // Card types: preserve exact AND (`all_types`) vs OR (`any_types`) semantics while
    // also populating `any_types` on single-type or creature-guaranteeing clauses so
    // engine helpers like `guarantees_creature()` and `guarantees_damageable()` stay accurate.
    let all_mask = lower_engine_type_mask(filter.all_types);
    let any_mask = lower_engine_type_mask(filter.any_types);
    if !any_mask.is_empty() {
        clause.any_types = any_mask;
        clause.required_types = all_mask;
    } else if !all_mask.is_empty() {
        if all_mask.bits().count_ones() == 1 {
            clause.any_types = all_mask;
        } else {
            clause.required_types = all_mask;
            if all_mask.contains(CardType::Creature) {
                clause.any_types = EngineCardTypeMask::CREATURE;
            } else if all_mask.contains(CardType::Planeswalker) {
                clause.any_types = EngineCardTypeMask::PLANESWALKER;
            } else if all_mask.contains(CardType::Battle) {
                clause.any_types = EngineCardTypeMask::BATTLE;
            }
        }
    } else if filter.any_permanent {
        clause.any_types = EngineCardTypeMask::PERMANENT;
    }

    clause.forbidden_types = lower_engine_type_mask(filter.forbidden_types);

    // Supertypes (with explicit `HISTORIC` handling).
    if (filter.required_supertypes.0 & ast::SupertypeMask::HISTORIC.0) != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::HISTORIC);
    }
    clause.required_supertypes = lower_supertype_mask(filter.required_supertypes)?;
    if (filter.forbidden_supertypes.0 & ast::SupertypeMask::HISTORIC.0) != 0 {
        clause.flags = clause.flags.union(ObjectFilterFlags::NON_HISTORIC);
    }
    clause.forbidden_supertypes = lower_supertype_mask(filter.forbidden_supertypes)?;

    // Subtypes, colors, controller, owner, combat, tapped, token.
    apply_subtypes_to_clause(&mut clause, filter, ctx)?;
    apply_colors_to_clause(&mut clause, filter)?;

    if let Some(rel) = filter.controller {
        clause.controller = lower_static_controller(rel)?;
    }
    if let Some(rel) = filter.owner {
        clause.owner = lower_owner_relation(rel)?;
    }
    if let Some(combat) = filter.combat {
        clause.combat = lower_combat_filter(combat);
    }
    clause.tapped = filter.tapped;
    clause.token = filter.token;

    // Keywords (handling Toxic via `HAS_TOXIC` and 2-keyword OR via `ObjectFilter::Either`).
    let mut keyword_split: Option<(KeywordAbility, KeywordAbility)> = None;
    let mut non_toxic_req: Vec<KeywordAbility> = Vec::new();
    for &kw in &filter.required_keywords {
        if kw == v2::KeywordKind::Toxic && !filter.keywords_match_any {
            clause.flags = clause.flags.union(ObjectFilterFlags::HAS_TOXIC);
        } else {
            let ab = lower_keyword_kind_plain(kw)?;
            if !non_toxic_req.contains(&ab) {
                non_toxic_req.push(ab);
            }
        }
    }
    match non_toxic_req.as_slice() {
        [] => {}
        [only] => clause.keyword = Some(*only),
        [a, b] if filter.keywords_match_any => {
            keyword_split = Some((*a, *b));
        }
        _ => {
            return Err(LoweringError::Unsupported(
                "v2:filter:multiple-required-keywords",
            ));
        }
    }

    match filter.forbidden_keywords.as_slice() {
        [] => {}
        [only] => clause.forbidden_keyword = Some(lower_keyword_kind_plain(*only)?),
        _ => {
            return Err(LoweringError::Unsupported(
                "v2:filter:multiple-forbidden-keywords",
            ));
        }
    }

    // Stat bounds, flags, and relational predicates.
    apply_stat_bounds_to_clause(&mut clause, &filter.stat_bounds, ctx)?;
    apply_flags_to_clause(&mut clause, filter.flags, ctx.in_trigger)?;
    let pred_split = apply_predicates_to_clause(&mut clause, &filter.predicates, ctx)?;

    match (keyword_split, pred_split) {
        (None, None) => Ok(EngineObjectFilter::Clause(clause)),
        (Some((k1, k2)), None) => {
            if clause.keyword.is_some() {
                return Err(LoweringError::Unsupported("v2:filter:keyword-conflict"));
            }
            Ok(EngineObjectFilter::Either(
                clause.with_keyword(k1),
                clause.with_keyword(k2),
            ))
        }
        (None, Some((c1, c2))) => Ok(EngineObjectFilter::Either(c1, c2)),
        (Some(_), Some(_)) => Err(LoweringError::Unsupported("v2:filter:double-disjunction")),
    }
}

/// Extract a single `FilterPredicate::OfChosenPlayer(player)` qualifier from `filter`,
/// returning the cleaned filter alongside the choosing player if present.
///
/// Leaves `filter` untouched and returns `None` if multiple distinct `OfChosenPlayer`
/// predicates exist so downstream lowering fails closed.
// Exported for zone-operation and mutation prompt-selection callers.
#[allow(dead_code)]
#[must_use]
pub(crate) fn split_of_chosen_player(
    filter: &v2::ObjectFilter,
) -> (v2::ObjectFilter, Option<v2::PlayerRef>) {
    let mut chosen: Option<v2::PlayerRef> = None;
    for pred in &filter.predicates {
        if let v2::FilterPredicate::OfChosenPlayer(player) = pred {
            if let Some(prev) = chosen
                && prev != *player
            {
                return (filter.clone(), None);
            }
            chosen = Some(*player);
        }
    }
    let Some(player) = chosen else {
        return (filter.clone(), None);
    };
    let mut cleaned = filter.clone();
    cleaned
        .predicates
        .retain(|p| !matches!(p, v2::FilterPredicate::OfChosenPlayer(_)));
    (cleaned, Some(player))
}

/// Strip `FilterPredicate::AmongPriorCards` from `filter`, returning `(cleaned, true)` when present.
#[must_use]
pub(crate) fn split_among_prior_cards(filter: &v2::ObjectFilter) -> (v2::ObjectFilter, bool) {
    let present = filter
        .predicates
        .iter()
        .any(|p| matches!(p, v2::FilterPredicate::AmongPriorCards));
    if !present {
        return (filter.clone(), false);
    }
    let mut cleaned = filter.clone();
    cleaned
        .predicates
        .retain(|p| !matches!(p, v2::FilterPredicate::AmongPriorCards));
    (cleaned, true)
}

/// Strip `FilterFlags::AT_RANDOM` from `filter`, returning `(cleaned, true)` when present.
// Exported for zone-operation random-selection callers.
#[allow(dead_code)]
#[must_use]
pub(crate) fn split_at_random(filter: &v2::ObjectFilter) -> (v2::ObjectFilter, bool) {
    if !filter.flags.contains(v2::FilterFlags::AT_RANDOM) {
        return (filter.clone(), false);
    }
    let mut cleaned = filter.clone();
    cleaned.flags.0 &= !v2::FilterFlags::AT_RANDOM.0;
    (cleaned, true)
}

/// Lower a [`v2::SubjectRef`] into a static [`PermanentSelector`].
pub(crate) fn lower_permanent_selector(
    subject: &v2::SubjectRef,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<PermanentSelector> {
    match subject {
        v2::SubjectRef::SelfObject
        | v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatLand
            | v2::AnaphoraRef::ThatObject
            | v2::AnaphoraRef::ThatSource,
        ) => Ok(PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        }),
        v2::SubjectRef::BothPairedCreatures => Ok(PermanentSelector {
            controller: ControllerSelector::SourcePair,
            ..PermanentSelector::default()
        }),
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::EquippedCreature | v2::AnaphoraRef::EnchantedCreature,
        ) => Ok(PermanentSelector {
            attached_to_source: true,
            required_types: BTreeSet::from([CardType::Creature]),
            ..PermanentSelector::default()
        }),
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::EnchantedPermanent) => Ok(PermanentSelector {
            attached_to_source: true,
            ..PermanentSelector::default()
        }),
        v2::SubjectRef::Each(filter)
        | v2::SubjectRef::All(filter)
        | v2::SubjectRef::Group(filter) => lower_filter_to_permanent_selector(filter, ctx),
        _ => Err(LoweringError::Unsupported(
            "v2:selector:unsupported-subject",
        )),
    }
}

fn lower_filter_to_permanent_selector(
    filter: &v2::ObjectFilter,
    ctx: &LowerCtx<'_>,
) -> LowerResult<PermanentSelector> {
    if filter.is_ability || filter.any_spell {
        return Err(LoweringError::Unsupported("v2:selector:non-permanent"));
    }
    if filter
        .zones
        .iter()
        .any(|z| z.zone != v2::ZoneKind::Battlefield)
    {
        return Err(LoweringError::Unsupported(
            "v2:selector:non-battlefield-zone",
        ));
    }

    let mut sel = PermanentSelector {
        // Card types.
        required_types: lower_card_type_set(filter.all_types),
        any_types: lower_card_type_set(filter.any_types),
        forbidden_types: lower_card_type_set(filter.forbidden_types),
        ..Default::default()
    };

    // Supertypes (including `HISTORIC`).
    if (filter.required_supertypes.0 & ast::SupertypeMask::HISTORIC.0) != 0 {
        sel.historic = Some(true);
    }
    if (filter.forbidden_supertypes.0 & ast::SupertypeMask::HISTORIC.0) != 0 {
        if sel.historic == Some(true) {
            return Err(LoweringError::Unsupported(
                "v2:selector:contradictory-historic",
            ));
        }
        sel.historic = Some(false);
    }
    // Validate supertype bits so unknown bits fail closed cleanly.
    let _ = lower_supertype_mask(filter.required_supertypes)?;
    let _ = lower_supertype_mask(filter.forbidden_supertypes)?;
    sel.required_supertypes = lower_supertype_set(filter.required_supertypes);
    sel.forbidden_supertypes = lower_supertype_set(filter.forbidden_supertypes);

    // Subtypes (full conjunctive, disjunctive, and forbidden support, plus CR 700.12 Outlaw batching).
    let type_hint = filter.all_types.union(filter.any_types);
    let mut effective_match_any = filter.subtypes_match_any;
    for raw in &filter.subtypes {
        let expanded = resolve_subtype_or_batch(raw, type_hint, ctx)?;
        if expanded.len() > 1 {
            if filter.subtypes.len() == 1 || filter.subtypes_match_any {
                effective_match_any = true;
            } else {
                return Err(LoweringError::Unsupported("v2:filter:unknown-subtype"));
            }
        }
        for sub in expanded {
            let sel_sub = single_subtype_to_selector(sub)?;
            if effective_match_any {
                sel.any_subtypes.insert(sel_sub);
            } else {
                sel.required_subtypes.insert(sel_sub);
            }
        }
    }
    for raw in &filter.forbidden_subtypes {
        for sub in resolve_subtype_or_batch(raw, type_hint, ctx)? {
            let sel_sub = single_subtype_to_selector(sub)?;
            if sel_sub == SelectorSubtype::Aura {
                sel.exclude_auras = true;
            } else {
                sel.forbidden_subtypes.insert(sel_sub);
            }
        }
    }

    // Colors.
    if filter.flags.contains(v2::FilterFlags::BOTH_COLORS) && filter.any_colors.0.count_ones() >= 2
    {
        return Err(LoweringError::Unsupported("v2:selector:both-colors"));
    }
    sel.color = lower_selector_color(
        filter.any_colors,
        filter.forbidden_colors,
        filter.color_arity,
    )?;

    // Controller and owner.
    if let Some(rel) = filter.controller {
        sel.controller = match rel {
            ast::ControllerRelation::You => ControllerSelector::SourceController,
            ast::ControllerRelation::Opponent | ast::ControllerRelation::Opponents => {
                ControllerSelector::Opponents
            }
            ast::ControllerRelation::OwnerYou => ControllerSelector::OwnerIsSourceController,
            ast::ControllerRelation::OwnerOpponent => ControllerSelector::OwnerIsOpponent,
            _ => return Err(LoweringError::Unsupported("v2:selector:controller")),
        };
    }
    if let Some(rel) = filter.owner {
        sel.owner = Some(match rel {
            ast::ControllerRelation::You | ast::ControllerRelation::OwnerYou => {
                OwnerSelector::SourceController
            }
            ast::ControllerRelation::Opponent
            | ast::ControllerRelation::Opponents
            | ast::ControllerRelation::OwnerOpponent => OwnerSelector::Opponents,
            _ => return Err(LoweringError::Unsupported("v2:selector:owner")),
        });
    }

    // Combat, tap, and token state.
    if let Some(combat) = filter.combat {
        sel.combat_role = Some(lower_combat_selector(combat));
    }
    match filter.tapped {
        Some(true) => sel.requires_tapped = true,
        Some(false) => sel.requires_untapped = true,
        None => {}
    }
    sel.token = filter.token;

    // Keywords.
    if filter.keywords_match_any {
        for &kw in &filter.required_keywords {
            sel.any_required_keywords
                .insert(lower_keyword_kind_plain(kw)?);
        }
    } else {
        for &kw in &filter.required_keywords {
            if kw == v2::KeywordKind::Toxic {
                sel.requires_toxic = true;
            } else if kw == v2::KeywordKind::Flying {
                sel.requires_flying = true;
            } else {
                let ab = lower_keyword_kind_plain(kw)?;
                if sel.required_keyword.is_none() || sel.required_keyword == Some(ab) {
                    sel.required_keyword = Some(ab);
                } else if sel.extra_required_keyword.is_none()
                    || sel.extra_required_keyword == Some(ab)
                {
                    sel.extra_required_keyword = Some(ab);
                } else {
                    return Err(LoweringError::Unsupported(
                        "v2:selector:too-many-required-keywords",
                    ));
                }
            }
        }
    }
    for &kw in &filter.forbidden_keywords {
        sel.forbidden_keywords.insert(lower_keyword_kind_plain(kw)?);
    }

    // Stat bounds (static numeric bounds + toughness > power).
    for bound in &filter.stat_bounds {
        if let v2::ValueExpr::ObjectStat {
            subject,
            stat: rhs_stat,
        } = bound.value.as_ref()
            && matches!(
                subject.as_ref(),
                v2::SubjectRef::Anaphora(v2::AnaphoraRef::It)
            )
            && ((bound.stat == ast::StatKind::Toughness
                && bound.op == ast::CompareOp::Gt
                && *rhs_stat == ast::StatKind::Power)
                || (bound.stat == ast::StatKind::Power
                    && bound.op == ast::CompareOp::Lt
                    && *rhs_stat == ast::StatKind::Toughness))
        {
            sel.toughness_greater_than_power = true;
            continue;
        }
        let Some(n) = lower_value_const(bound.value.as_ref()) else {
            return Err(LoweringError::Unsupported("v2:selector:dynamic-stat-bound"));
        };
        let n64 = i64::from(n);
        match (bound.stat, bound.op) {
            (ast::StatKind::Power, ast::CompareOp::Ge) => sel.min_power = Some(n64),
            (ast::StatKind::Power, ast::CompareOp::Gt) => {
                sel.min_power = Some(n64.saturating_add(1));
            }
            (ast::StatKind::Power, ast::CompareOp::Le) => sel.max_power = Some(n64),
            (ast::StatKind::Power, ast::CompareOp::Lt) => {
                sel.max_power = Some(n64.saturating_sub(1));
            }
            (ast::StatKind::Power, ast::CompareOp::Eq) => {
                sel.min_power = Some(n64);
                sel.max_power = Some(n64);
            }
            (ast::StatKind::Toughness, ast::CompareOp::Ge) => sel.min_toughness = Some(n64),
            (ast::StatKind::Toughness, ast::CompareOp::Gt) => {
                sel.min_toughness = Some(n64.saturating_add(1));
            }
            (ast::StatKind::Toughness, ast::CompareOp::Le) => sel.max_toughness = Some(n64),
            (ast::StatKind::Toughness, ast::CompareOp::Lt) => {
                sel.max_toughness = Some(n64.saturating_sub(1));
            }
            (ast::StatKind::Toughness, ast::CompareOp::Eq) => {
                sel.min_toughness = Some(n64);
                sel.max_toughness = Some(n64);
            }
            (ast::StatKind::ManaValue, ast::CompareOp::Ge) => {
                sel.min_mana_value = Some(
                    u32::try_from(n)
                        .map_err(|_| LoweringError::Unsupported("v2:selector:negative-mv"))?,
                );
            }
            (ast::StatKind::ManaValue, ast::CompareOp::Gt) => {
                let v = n
                    .checked_add(1)
                    .and_then(|x| u32::try_from(x).ok())
                    .ok_or(LoweringError::Unsupported("v2:selector:negative-mv"))?;
                sel.min_mana_value = Some(v);
            }
            (ast::StatKind::ManaValue, ast::CompareOp::Le) => {
                sel.max_mana_value = Some(
                    u32::try_from(n)
                        .map_err(|_| LoweringError::Unsupported("v2:selector:negative-mv"))?,
                );
            }
            (ast::StatKind::ManaValue, ast::CompareOp::Lt) => {
                let v = n
                    .checked_sub(1)
                    .and_then(|x| u32::try_from(x).ok())
                    .ok_or(LoweringError::Unsupported("v2:selector:negative-mv"))?;
                sel.max_mana_value = Some(v);
            }
            (ast::StatKind::ManaValue, ast::CompareOp::Eq) => {
                let v = u32::try_from(n)
                    .map_err(|_| LoweringError::Unsupported("v2:selector:negative-mv"))?;
                sel.min_mana_value = Some(v);
                sel.max_mana_value = Some(v);
            }
            (ast::StatKind::Loyalty, _) => {
                return Err(LoweringError::Unsupported("v2:selector:loyalty-bound"));
            }
        }
    }

    // Flags.
    let mut rem = filter.flags.0;
    let mut take = |bit: v2::FilterFlags| -> bool {
        if (rem & bit.0) != 0 {
            rem &= !bit.0;
            true
        } else {
            false
        }
    };
    if take(v2::FilterFlags::EXCLUDE_SELF) {
        sel.exclude_source = true;
    }
    if take(v2::FilterFlags::ATTACKED_THIS_TURN) {
        sel.attacked_this_turn = Some(true);
    }
    if take(v2::FilterFlags::DEALT_DAMAGE_THIS_TURN) {
        sel.dealt_damage_this_turn = true;
    }
    if take(v2::FilterFlags::ENTERED_THIS_TURN) {
        sel.entered_this_turn = true;
    }
    if take(v2::FilterFlags::MODIFIED) {
        sel.requires_modified = true;
    }
    if take(v2::FilterFlags::EQUIPPED) {
        sel.equipped_by_at_least = sel.equipped_by_at_least.max(1);
    }
    if take(v2::FilterFlags::ENCHANTED) {
        sel.requires_enchanted = true;
    }
    if take(v2::FilterFlags::FACE_DOWN) {
        sel.face_down = Some(true);
    }
    if take(v2::FilterFlags::FACE_UP) {
        if sel.face_down == Some(true) {
            return Err(LoweringError::Unsupported("v2:selector:contradictory-face"));
        }
        sel.face_down = Some(false);
    }
    if take(v2::FilterFlags::IS_COMMANDER) {
        sel.commander = Some(true);
    }
    if take(v2::FilterFlags::NOT_COMMANDER) {
        if sel.commander == Some(true) {
            return Err(LoweringError::Unsupported(
                "v2:selector:contradictory-commander",
            ));
        }
        sel.commander = Some(false);
    }
    if take(v2::FilterFlags::WITH_ANY_COUNTER) {
        sel.any_counter = true;
    }
    if take(v2::FilterFlags::WITHOUT_COUNTERS) {
        sel.without_counters = true;
    }
    if take(v2::FilterFlags::KICKED) {
        sel.kicked = true;
    }
    if take(v2::FilterFlags::SUSPECTED) {
        sel.suspected = true;
    }
    if take(v2::FilterFlags::RENOWNED) {
        sel.renowned = true;
    }
    let _ = take(v2::FilterFlags::BOTH_COLORS);
    let _ = take(v2::FilterFlags::LKI_PAST_TENSE);
    if rem != 0 {
        return Err(LoweringError::Unsupported("v2:selector:unsupported-flag"));
    }

    // Predicates.
    for pred in &filter.predicates {
        match pred {
            v2::FilterPredicate::AttachedTo(subj)
                if matches!(subj.as_ref(), v2::SubjectRef::SelfObject) =>
            {
                sel.attached_to_source_permanent = true;
            }
            v2::FilterPredicate::Exclude(subj)
                if matches!(subj.as_ref(), v2::SubjectRef::SelfObject) =>
            {
                sel.exclude_source = true;
            }
            v2::FilterPredicate::HasCounterKind(ck) => match ck {
                v2::CounterKind::Any => sel.any_counter = true,
                v2::CounterKind::PlusOnePlusOne => sel.plus_one_counter = true,
                v2::CounterKind::MinusOneMinusOne => sel.minus_one_counter = true,
                other => {
                    let kind = lower_counter_kind(other)?;
                    if sel.required_counter_kind.is_some_and(|prev| prev != kind) {
                        return Err(LoweringError::Unsupported(
                            "v2:selector:multiple-counter-kinds",
                        ));
                    }
                    sel.required_counter_kind = Some(kind);
                }
            },
            v2::FilterPredicate::HasNamedAbility(kw) => {
                if *kw == v2::KeywordKind::Toxic {
                    sel.requires_toxic = true;
                } else if *kw == v2::KeywordKind::Flying {
                    sel.requires_flying = true;
                } else {
                    let ab = lower_keyword_kind_plain(*kw)?;
                    if sel.required_keyword.is_none() || sel.required_keyword == Some(ab) {
                        sel.required_keyword = Some(ab);
                    } else if sel.extra_required_keyword.is_none()
                        || sel.extra_required_keyword == Some(ab)
                    {
                        sel.extra_required_keyword = Some(ab);
                    } else {
                        return Err(LoweringError::Unsupported(
                            "v2:selector:too-many-required-keywords",
                        ));
                    }
                }
            }
            v2::FilterPredicate::OfChosenColor => {
                sel.matches_source_entry_color = true;
            }
            v2::FilterPredicate::OfChosenCreatureType => {
                sel.matches_source_entry_type = true;
            }
            v2::FilterPredicate::NamedSelf => {
                sel.name = NameFilter::SharesWithSource;
            }
            v2::FilterPredicate::Named(name) => {
                let card_name = EnglishCardName::new(name.clone())
                    .map_err(|_| LoweringError::Unsupported("v2:selector:invalid-name"))?;
                sel.name = NameFilter::Named(card_name);
            }
            v2::FilterPredicate::SharesPropertyWith {
                property: v2::SharedProperty::Name,
                reference,
                negated,
            } if matches!(reference.as_ref(), v2::SubjectRef::SelfObject) => {
                sel.name = if *negated {
                    NameFilter::DifferentFromSource
                } else {
                    NameFilter::SharesWithSource
                };
            }
            v2::FilterPredicate::OwnedAndControlledByYou => {
                sel.controller = ControllerSelector::SourceController;
                sel.owner = Some(OwnerSelector::SourceController);
            }
            v2::FilterPredicate::TargetsSubject(subj)
                if sel.combat_role == Some(CombatSelector::Blocking)
                    && is_self_combat_ref(subj, ctx) =>
            {
                sel.combat_role = Some(CombatSelector::BlockingSource);
            }
            v2::FilterPredicate::TargetsSubject(subj)
                if sel.combat_role == Some(CombatSelector::Blocked)
                    && is_self_combat_ref(subj, ctx) =>
            {
                sel.combat_role = Some(CombatSelector::BlockedBySource);
            }
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:selector:unsupported-predicate",
                ));
            }
        }
    }

    Ok(sel)
}

fn color_set_bitmask(colors: &BTreeSet<Color>) -> u8 {
    colors.iter().fold(0u8, |acc, c| acc | c.bit())
}

fn lower_selector_color(
    any_colors: ast::ColorMask,
    forbidden_colors: ast::ColorMask,
    arity: Option<ast::ColorArity>,
) -> LowerResult<ColorFilter> {
    let any_set = lower_color_set(any_colors);
    let forb_set = lower_color_set(forbidden_colors);

    if let Some(a) = arity {
        return match a {
            ast::ColorArity::Colorless if any_set.is_empty() => Ok(ColorFilter::Colorless),
            ast::ColorArity::Colored if any_set.is_empty() && forb_set.is_empty() => {
                Ok(ColorFilter::Colored)
            }
            ast::ColorArity::Monocolored if any_set.is_empty() && forb_set.is_empty() => {
                Ok(ColorFilter::Monocolored)
            }
            ast::ColorArity::Multicolored if any_set.is_empty() && forb_set.is_empty() => {
                Ok(ColorFilter::Multicolored)
            }
            _ => Err(LoweringError::Unsupported(
                "v2:selector:color-arity-and-mask",
            )),
        };
    }

    match (any_set.len(), forb_set.len()) {
        (0, 0) => Ok(ColorFilter::Any),
        (1, 0) => Ok(any_set
            .into_iter()
            .next()
            .map_or(ColorFilter::Any, ColorFilter::Has)),
        (_, 0) => Ok(ColorFilter::AnyOf(color_set_bitmask(&any_set))),
        (0, 1) => Ok(forb_set
            .into_iter()
            .next()
            .map_or(ColorFilter::Any, ColorFilter::Lacks)),
        (0, _) => Ok(ColorFilter::LacksAll(color_set_bitmask(&forb_set))),
        (_, _) => Err(LoweringError::Unsupported(
            "v2:selector:both-any-and-forbidden-colors",
        )),
    }
}

/// Lower a [`v2::ObjectFilter`] into a [`SpellFilter`], extracting cast-zone, target-shape,
/// turn-ordinal, and special spell predicates cleanly before delegating object predicates.
pub(crate) fn lower_spell_filter(
    filter: &v2::ObjectFilter,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<SpellFilter> {
    if filter.is_ability {
        return Err(LoweringError::Unsupported("v2:spell-filter:is-ability"));
    }

    let mut cleaned = filter.clone();
    let mut cast_zone = SpellCastZoneFilter::Any;
    let mut target_shape = SpellTargetShape::Any;
    let mut ordinal_this_turn: Option<NonZeroU32> = None;
    let mut special = SpellSpecialFilter::None;

    // Inspect cast-origin zone constraint.
    if cleaned.zones.len() > 1 {
        return Err(LoweringError::Unsupported("v2:spell-filter:multi-zone"));
    }
    if let Some(zone) = cleaned.zones.pop() {
        cast_zone = match zone.zone {
            v2::ZoneKind::Stack => SpellCastZoneFilter::Any,
            v2::ZoneKind::Library => match zone.owner {
                v2::ZoneOwner::Default | v2::ZoneOwner::You => SpellCastZoneFilter::OwnLibrary,
                _ => return Err(LoweringError::Unsupported("v2:spell-filter:library-owner")),
            },
            v2::ZoneKind::Graveyard => SpellCastZoneFilter::Graveyard,
            v2::ZoneKind::Exile => {
                if cleaned.flags.contains(v2::FilterFlags::EXILED_WITH_SELF) {
                    cleaned.flags.0 &= !v2::FilterFlags::EXILED_WITH_SELF.0;
                    SpellCastZoneFilter::LinkedExile
                } else {
                    SpellCastZoneFilter::Exile
                }
            }
            v2::ZoneKind::Hand => SpellCastZoneFilter::Hand,
            v2::ZoneKind::Battlefield
            | v2::ZoneKind::CommandZone
            | v2::ZoneKind::OutsideTheGame => {
                return Err(LoweringError::Unsupported("v2:spell-filter:zone"));
            }
        };
    }

    // Extract `SINGLE_TARGET` flag into `SpellTargetShape`.
    if cleaned.flags.contains(v2::FilterFlags::SINGLE_TARGET) {
        cleaned.flags.0 &= !v2::FilterFlags::SINGLE_TARGET.0;
        target_shape = SpellTargetShape::SingleTarget;
    }

    // Extract spell-specific predicates (`FromOutsideHand`, `NthSpellCastThisTurn`, `TargetsSubject`,
    // `OfChosenCreatureType`) while preserving remaining predicates for `lower_object_filter`.
    let mut remaining_preds = Vec::with_capacity(cleaned.predicates.len());
    for pred in std::mem::take(&mut cleaned.predicates) {
        match pred {
            v2::FilterPredicate::FromOutsideHand => {
                if cast_zone != SpellCastZoneFilter::Any
                    && cast_zone != SpellCastZoneFilter::NotOwnersHand
                {
                    return Err(LoweringError::Unsupported(
                        "v2:spell-filter:conflicting-cast-zone",
                    ));
                }
                cast_zone = SpellCastZoneFilter::NotOwnersHand;
            }
            v2::FilterPredicate::NthSpellCastThisTurn(n) => {
                let ord = NonZeroU32::new(u32::from(n))
                    .ok_or(LoweringError::Unsupported("v2:spell-filter:zero-ordinal"))?;
                if ordinal_this_turn.is_some_and(|prev| prev != ord) {
                    return Err(LoweringError::Unsupported(
                        "v2:spell-filter:multiple-ordinals",
                    ));
                }
                ordinal_this_turn = Some(ord);
            }
            v2::FilterPredicate::TargetsSubject(subj) => {
                let shape = match subj.as_ref() {
                    s if is_self_combat_ref(s, ctx) => SpellTargetShape::TargetingOnlySource,
                    v2::SubjectRef::Player(
                        v2::PlayerRef::AnyPlayer
                        | v2::PlayerRef::You
                        | v2::PlayerRef::EachPlayer
                        | v2::PlayerRef::TargetPlayer
                        | v2::PlayerRef::TargetOpponent
                        | v2::PlayerRef::AnyOpponent
                        | v2::PlayerRef::ThatPlayer,
                    ) => SpellTargetShape::TargetingPlayer,
                    v2::SubjectRef::Group(obj_flt)
                    | v2::SubjectRef::Each(obj_flt)
                    | v2::SubjectRef::All(obj_flt) => {
                        let lowered_obj = lower_object_filter(obj_flt, ctx)?;
                        let compact =
                            spellwake_engine::program::SpellTargetObjectFilter::try_from_object_filter(
                                lowered_obj,
                            )
                            .ok_or(LoweringError::Unsupported(
                                "v2:spell-filter:targets-subject",
                            ))?;
                        SpellTargetShape::TargetingObject(compact)
                    }
                    _ => {
                        return Err(LoweringError::Unsupported(
                            "v2:spell-filter:targets-subject",
                        ));
                    }
                };
                if target_shape != SpellTargetShape::Any
                    && target_shape != shape
                    && !(target_shape == SpellTargetShape::SingleTarget
                        && shape == SpellTargetShape::TargetingOnlySource)
                {
                    return Err(LoweringError::Unsupported(
                        "v2:spell-filter:conflicting-target-shape",
                    ));
                }
                target_shape = shape;
            }
            v2::FilterPredicate::OfChosenCreatureType => {
                // Only promote to `SpellSpecialFilter::ChosenEntryCreatureType` when no other
                // object predicates exist, since `SpellFilter::matches_characteristics` short-circuits
                // on `ChosenEntryCreatureType`. Otherwise retain the predicate so `ObjectFilterFlags::MATCHES_SOURCE_ENTRY_TYPE`
                // composes with every other predicate cleanly.
                let pure_creature_spell = special == SpellSpecialFilter::None
                    && cleaned.all_types == ast::CardTypeMask::CREATURE
                    && cleaned.any_types.is_empty()
                    && cleaned.forbidden_types.is_empty()
                    && cleaned.required_supertypes.is_empty()
                    && cleaned.forbidden_supertypes.is_empty()
                    && cleaned.subtypes.is_empty()
                    && cleaned.forbidden_subtypes.is_empty()
                    && cleaned.any_colors.is_empty()
                    && cleaned.forbidden_colors.is_empty()
                    && cleaned.color_arity.is_none()
                    && cleaned.controller.is_none()
                    && cleaned.owner.is_none()
                    && cleaned.combat.is_none()
                    && cleaned.tapped.is_none()
                    && cleaned.token.is_none()
                    && cleaned.required_keywords.is_empty()
                    && cleaned.forbidden_keywords.is_empty()
                    && cleaned.stat_bounds.is_empty()
                    && cleaned.flags.is_empty()
                    && remaining_preds.is_empty();
                if pure_creature_spell {
                    special = SpellSpecialFilter::ChosenEntryCreatureType;
                } else {
                    remaining_preds.push(v2::FilterPredicate::OfChosenCreatureType);
                }
            }
            other => remaining_preds.push(other),
        }
    }
    cleaned.predicates = remaining_preds;

    let object = lower_object_filter(&cleaned, ctx)?;

    Ok(SpellFilter {
        object,
        cast_zone,
        target_shape,
        ordinal_this_turn,
        special,
    })
}
