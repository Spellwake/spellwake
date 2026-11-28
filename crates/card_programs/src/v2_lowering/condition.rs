//! Boolean condition lowerers from `v2::ir::ConditionExpr` into toolbox `ConditionExpr`,
//! `ResolutionCondition`, and `StaticSourceCondition`.

use std::collections::BTreeMap;

use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    characteristics::StaticSourceCondition,
    mana::ManaType,
    program::{
        AffectedPlayers, AttachedCountKind, BodyTarget, CombatFilter, Comparison, ControllerFilter,
        CounterSubjectRef, EventValueKind, MatchObjectRef, ObjectClause,
        ObjectFilter as EngineObjectFilter, ObjectFilterFlags, OpponentTurnStat, PhaseCheck,
        PlayerStatExpr, ResolutionCondition, ResolvingPlayer, SourceStateExpr, SourceZoneKind,
        ValueExpr as EngineValueExpr, ValueZone,
    },
    toolbox::ConditionExpr as EngineConditionExpr,
};
use spellwake_oracle_compiler::{ast, v2::ir as v2};

use crate::v2_lowering::{
    atoms::{
        color_to_mana_type, lower_color_set, lower_compare_op, lower_counter_kind,
        lower_keyword_kind_plain, lower_source_zone_filter, lower_stat_kind, lower_step,
        lower_value_zone, lower_world_affected_players, lower_world_resolving_player,
        lower_zone_kind_to_source_zone_kind,
    },
    ctx::LowerCtx,
    error::{LowerResult, LoweringError},
    filter::{lower_object_filter, lower_permanent_selector, scan_filter_player_target},
    value::{
        lookup_party_roles, lower_value_expr, lower_value_object_ref, scan_value_expr_player_target,
    },
};

fn scan_cond_player_target(player: v2::PlayerRef) -> Option<BodyTarget> {
    match player {
        v2::PlayerRef::TargetOpponent => Some(BodyTarget::SINGLE_OPPONENT),
        v2::PlayerRef::TargetPlayer => Some(BodyTarget::SINGLE_PLAYER),
        _ => None,
    }
}

fn scan_cond_zone_target(owner: v2::ZoneOwner) -> Option<BodyTarget> {
    match owner {
        v2::ZoneOwner::TargetOpponent => Some(BodyTarget::SINGLE_OPPONENT),
        v2::ZoneOwner::TargetPlayer => Some(BodyTarget::SINGLE_PLAYER),
        _ => None,
    }
}

/// Scan a [`v2::ConditionExpr`] for an embedded `TargetOpponent` or `TargetPlayer` reference
/// that must be registered on the enclosing [`crate::v2_lowering::ctx::BodyAcc`].
// Exported for effect and branch-condition callers registering targets embedded in conditions.
#[allow(dead_code)]
#[must_use]
pub(crate) fn scan_condition_player_target(cond: &v2::ConditionExpr) -> Option<BodyTarget> {
    match cond {
        v2::ConditionExpr::And(a, b) | v2::ConditionExpr::Or(a, b) => {
            scan_condition_player_target(a).or_else(|| scan_condition_player_target(b))
        }
        v2::ConditionExpr::Not(inner) => scan_condition_player_target(inner),
        v2::ConditionExpr::Compare { lhs, rhs, .. } => {
            scan_value_expr_player_target(lhs).or_else(|| scan_value_expr_player_target(rhs))
        }
        v2::ConditionExpr::Controls { player, filter, .. }
        | v2::ConditionExpr::SacrificedPermanentThisTurn { player, filter } => {
            scan_cond_player_target(*player).or_else(|| scan_filter_player_target(filter))
        }
        v2::ConditionExpr::ControlsMoreThan {
            player,
            other,
            filter,
        } => scan_cond_player_target(*player)
            .or_else(|| scan_cond_player_target(*other))
            .or_else(|| scan_filter_player_target(filter)),
        v2::ConditionExpr::ControlsSubject { player, .. }
        | v2::ConditionExpr::PlayerHasCounters { player, .. }
        | v2::ConditionExpr::PlayerCheck { player, .. } => scan_cond_player_target(*player),
        v2::ConditionExpr::PlayerTurnEvent { player, filter, .. } => {
            scan_cond_player_target(*player)
                .or_else(|| filter.as_deref().and_then(scan_filter_player_target))
        }
        v2::ConditionExpr::PermanentEnteredThisTurn {
            filter, controller, ..
        } => scan_cond_player_target(*controller).or_else(|| scan_filter_player_target(filter)),
        v2::ConditionExpr::ZoneHasCount { filter, zone, .. } => {
            scan_cond_zone_target(zone.owner).or_else(|| scan_filter_player_target(filter))
        }
        v2::ConditionExpr::ZoneMetric { zone, .. }
        | v2::ConditionExpr::InZone { zone, .. }
        | v2::ConditionExpr::CastFromZone { zone, .. } => scan_cond_zone_target(zone.owner),
        v2::ConditionExpr::SubjectMatches { filter, .. }
        | v2::ConditionExpr::SubjectAttachedTo { target: filter, .. }
        | v2::ConditionExpr::AttachedCount { filter, .. }
        | v2::ConditionExpr::TargetsFilter { filter, .. }
        | v2::ConditionExpr::PriorActionMatched { filter, .. }
        | v2::ConditionExpr::FilterCheck { filter, .. } => scan_filter_player_target(filter),
        _ => None,
    }
}

const fn opponent_zone_to_value_zone(zone: v2::ZoneKind) -> Option<ValueZone> {
    match zone {
        v2::ZoneKind::Battlefield => Some(ValueZone::Battlefield),
        v2::ZoneKind::Graveyard => Some(ValueZone::Graveyard(ResolvingPlayer::Controller)),
        v2::ZoneKind::Hand => Some(ValueZone::Hand(ResolvingPlayer::Controller)),
        v2::ZoneKind::Library => Some(ValueZone::Library(ResolvingPlayer::Controller)),
        v2::ZoneKind::Exile => Some(ValueZone::Exile(Some(ResolvingPlayer::Controller))),
        v2::ZoneKind::CommandZone | v2::ZoneKind::Stack | v2::ZoneKind::OutsideTheGame => None,
    }
}

/// Returns `Some(true)` for existential (`AnyOpponent` / singular `Opponent` zone),
/// `Some(false)` for universal (`EachOpponent`), or `None` if `expr` is not an opponent-quantified scalar.
fn opponent_expr_existential(expr: &v2::ValueExpr) -> Option<bool> {
    match expr {
        v2::ValueExpr::PlayerStat { player, .. }
        | v2::ValueExpr::PlayerCounterCount { player, .. } => match player {
            v2::PlayerRef::AnyOpponent => Some(true),
            v2::PlayerRef::EachOpponent => Some(false),
            _ => None,
        },
        v2::ValueExpr::CounterCount { subject, .. } => match subject.as_ref() {
            v2::SubjectRef::Player(v2::PlayerRef::AnyOpponent) => Some(true),
            v2::SubjectRef::Player(v2::PlayerRef::EachOpponent) => Some(false),
            _ => None,
        },
        v2::ValueExpr::Count(filter)
            if filter.zones.len() == 1
                && !filter.flags.contains(v2::FilterFlags::EXILED_WITH_SELF) =>
        {
            match filter.zones[0].owner {
                v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents => Some(true),
                _ => None,
            }
        }
        _ => None,
    }
}

fn try_lower_opponent_extremum(
    expr: &v2::ValueExpr,
    want_max: bool,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<EngineValueExpr>> {
    match expr {
        v2::ValueExpr::PlayerStat {
            player: v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent,
            stat,
        } => {
            let lowered = match stat {
                ast::PlayerStatKind::LifeTotal => EngineValueExpr::PlayerStat(if want_max {
                    PlayerStatExpr::OpponentMaxLife
                } else {
                    PlayerStatExpr::OpponentMinLife
                }),
                ast::PlayerStatKind::CardsInHand => {
                    let zone = ValueZone::Hand(ResolvingPlayer::Controller);
                    let filter = Box::new(EngineObjectFilter::any());
                    if want_max {
                        EngineValueExpr::OpponentMaxCount { zone, filter }
                    } else {
                        EngineValueExpr::OpponentMinCount { zone, filter }
                    }
                }
                ast::PlayerStatKind::CardsInGraveyard => {
                    let zone = ValueZone::Graveyard(ResolvingPlayer::Controller);
                    let filter = Box::new(EngineObjectFilter::any());
                    if want_max {
                        EngineValueExpr::OpponentMaxCount { zone, filter }
                    } else {
                        EngineValueExpr::OpponentMinCount { zone, filter }
                    }
                }
                ast::PlayerStatKind::CardsInLibrary => {
                    let zone = ValueZone::Library(ResolvingPlayer::Controller);
                    let filter = Box::new(EngineObjectFilter::any());
                    if want_max {
                        EngineValueExpr::OpponentMaxCount { zone, filter }
                    } else {
                        EngineValueExpr::OpponentMinCount { zone, filter }
                    }
                }
            };
            Ok(Some(lowered))
        }
        v2::ValueExpr::PlayerCounterCount {
            player: v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent,
            kind,
        } => {
            if !want_max {
                return Ok(None);
            }
            let ck = lower_counter_kind(kind)?;
            Ok(Some(EngineValueExpr::PlayerStat(
                PlayerStatExpr::OpponentMaxCounter(ck),
            )))
        }
        v2::ValueExpr::CounterCount { subject, kind }
            if matches!(
                subject.as_ref(),
                v2::SubjectRef::Player(v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent)
            ) =>
        {
            if !want_max {
                return Ok(None);
            }
            let ck = lower_counter_kind(kind)?;
            Ok(Some(EngineValueExpr::PlayerStat(
                PlayerStatExpr::OpponentMaxCounter(ck),
            )))
        }
        v2::ValueExpr::Count(filter)
            if filter.zones.len() == 1
                && !filter.flags.contains(v2::FilterFlags::EXILED_WITH_SELF)
                && matches!(
                    filter.zones[0].owner,
                    v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents
                ) =>
        {
            let Some(zone) = opponent_zone_to_value_zone(filter.zones[0].zone) else {
                return Ok(None);
            };
            let mut cleaned = filter.as_ref().clone();
            cleaned.zones.clear();
            let obj_filter = Box::new(lower_object_filter(&cleaned, ctx)?);
            Ok(Some(if want_max {
                EngineValueExpr::OpponentMaxCount {
                    zone,
                    filter: obj_filter,
                }
            } else {
                EngineValueExpr::OpponentMinCount {
                    zone,
                    filter: obj_filter,
                }
            }))
        }
        _ => Ok(None),
    }
}

fn is_zero_const(expr: &v2::ValueExpr) -> bool {
    matches!(expr, v2::ValueExpr::Const(0))
}

fn is_nonneg_count_expr(expr: &v2::ValueExpr) -> bool {
    matches!(
        expr,
        v2::ValueExpr::PlayerStat {
            stat: ast::PlayerStatKind::CardsInHand
                | ast::PlayerStatKind::CardsInGraveyard
                | ast::PlayerStatKind::CardsInLibrary,
            ..
        } | v2::ValueExpr::Count(_)
            | v2::ValueExpr::PlayerCounterCount { .. }
            | v2::ValueExpr::CounterCount { .. }
    )
}

/// Returns `Some(true)` for existential (`AnyPlayer`), `Some(false)` for universal
/// (`EachPlayer`), or `None` if `expr` is not a player-quantified scalar over all players.
fn any_player_expr_existential(expr: &v2::ValueExpr) -> Option<bool> {
    match expr {
        v2::ValueExpr::PlayerStat { player, .. }
        | v2::ValueExpr::PlayerCounterCount { player, .. } => match player {
            v2::PlayerRef::AnyPlayer => Some(true),
            v2::PlayerRef::EachPlayer => Some(false),
            _ => None,
        },
        v2::ValueExpr::CounterCount { subject, .. } => match subject.as_ref() {
            v2::SubjectRef::Player(v2::PlayerRef::AnyPlayer) => Some(true),
            v2::SubjectRef::Player(v2::PlayerRef::EachPlayer) => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn controller_counter_subject(ck: spellwake_engine::effects::CounterKind) -> CounterSubjectRef {
    match ck {
        spellwake_engine::effects::CounterKind::Energy => CounterSubjectRef::ControllerEnergy,
        spellwake_engine::effects::CounterKind::Experience => {
            CounterSubjectRef::ControllerExperience
        }
        spellwake_engine::effects::CounterKind::Poison => CounterSubjectRef::ControllerPoison,
        other => CounterSubjectRef::PlayerCounter(ResolvingPlayer::Controller, other),
    }
}

fn try_lower_player_pair_extremum(
    expr: &v2::ValueExpr,
    want_max: bool,
) -> LowerResult<Option<(EngineValueExpr, EngineValueExpr)>> {
    match expr {
        v2::ValueExpr::PlayerStat {
            player: v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer,
            stat,
        } => {
            let pair = match stat {
                ast::PlayerStatKind::LifeTotal => (
                    EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    EngineValueExpr::PlayerStat(if want_max {
                        PlayerStatExpr::OpponentMaxLife
                    } else {
                        PlayerStatExpr::OpponentMinLife
                    }),
                ),
                ast::PlayerStatKind::CardsInHand => {
                    let zone = ValueZone::Hand(ResolvingPlayer::Controller);
                    let filter = Box::new(EngineObjectFilter::any());
                    (
                        EngineValueExpr::count_hand(ResolvingPlayer::Controller),
                        if want_max {
                            EngineValueExpr::OpponentMaxCount { zone, filter }
                        } else {
                            EngineValueExpr::OpponentMinCount { zone, filter }
                        },
                    )
                }
                ast::PlayerStatKind::CardsInGraveyard => {
                    let zone = ValueZone::Graveyard(ResolvingPlayer::Controller);
                    let filter = Box::new(EngineObjectFilter::any());
                    (
                        EngineValueExpr::count_graveyard(
                            ResolvingPlayer::Controller,
                            EngineObjectFilter::any(),
                        ),
                        if want_max {
                            EngineValueExpr::OpponentMaxCount { zone, filter }
                        } else {
                            EngineValueExpr::OpponentMinCount { zone, filter }
                        },
                    )
                }
                ast::PlayerStatKind::CardsInLibrary => {
                    let zone = ValueZone::Library(ResolvingPlayer::Controller);
                    let filter = Box::new(EngineObjectFilter::any());
                    (
                        EngineValueExpr::PlayerStat(PlayerStatExpr::PlayerLibrarySize(
                            ResolvingPlayer::Controller,
                        )),
                        if want_max {
                            EngineValueExpr::OpponentMaxCount { zone, filter }
                        } else {
                            EngineValueExpr::OpponentMinCount { zone, filter }
                        },
                    )
                }
            };
            Ok(Some(pair))
        }
        v2::ValueExpr::PlayerCounterCount {
            player: v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer,
            kind,
        } => {
            if !want_max {
                return Ok(None);
            }
            let ck = lower_counter_kind(kind)?;
            Ok(Some((
                EngineValueExpr::CounterCount {
                    subject: controller_counter_subject(ck),
                    kind: Some(ck),
                },
                EngineValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
            )))
        }
        v2::ValueExpr::CounterCount { subject, kind }
            if matches!(
                subject.as_ref(),
                v2::SubjectRef::Player(v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer)
            ) =>
        {
            if !want_max {
                return Ok(None);
            }
            let ck = lower_counter_kind(kind)?;
            Ok(Some((
                EngineValueExpr::CounterCount {
                    subject: controller_counter_subject(ck),
                    kind: Some(ck),
                },
                EngineValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
            )))
        }
        _ => Ok(None),
    }
}

fn lower_compare_condition(
    lhs: &v2::ValueExpr,
    op: ast::CompareOp,
    rhs: &v2::ValueExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    let lhs_opp = opponent_expr_existential(lhs);
    let rhs_opp = opponent_expr_existential(rhs);
    if let (Some(existential), None) = (lhs_opp, rhs_opp) {
        let want_max = match op {
            ast::CompareOp::Ge | ast::CompareOp::Gt => Some(existential),
            ast::CompareOp::Le | ast::CompareOp::Lt => Some(!existential),
            ast::CompareOp::Eq if is_zero_const(rhs) && is_nonneg_count_expr(lhs) => {
                Some(!existential)
            }
            ast::CompareOp::Eq => None,
        };
        if let Some(want_max) = want_max
            && let Some(left) = try_lower_opponent_extremum(lhs, want_max, ctx)?
        {
            return Ok(ResolutionCondition {
                left,
                comparison: lower_compare_op(op),
                right: lower_value_expr(rhs, ctx)?,
            });
        }
    }
    if let (None, Some(existential)) = (lhs_opp, rhs_opp) {
        let want_max = match op {
            ast::CompareOp::Le | ast::CompareOp::Lt => Some(existential),
            ast::CompareOp::Ge | ast::CompareOp::Gt => Some(!existential),
            ast::CompareOp::Eq if is_zero_const(lhs) && is_nonneg_count_expr(rhs) => {
                Some(!existential)
            }
            ast::CompareOp::Eq => None,
        };
        if let Some(want_max) = want_max
            && let Some(right) = try_lower_opponent_extremum(rhs, want_max, ctx)?
        {
            return Ok(ResolutionCondition {
                left: lower_value_expr(lhs, ctx)?,
                comparison: lower_compare_op(op),
                right,
            });
        }
    }

    let lhs_any = any_player_expr_existential(lhs);
    let rhs_any = any_player_expr_existential(rhs);
    if let (Some(existential), None) = (lhs_any, rhs_any)
        && rhs_opp.is_none()
    {
        let want_max = match op {
            ast::CompareOp::Ge | ast::CompareOp::Gt => Some(existential),
            ast::CompareOp::Le | ast::CompareOp::Lt => Some(!existential),
            ast::CompareOp::Eq if is_zero_const(rhs) && is_nonneg_count_expr(lhs) => {
                Some(!existential)
            }
            ast::CompareOp::Eq => None,
        };
        if let Some(want_max) = want_max
            && let Some((ctrl_expr, opp_expr)) = try_lower_player_pair_extremum(lhs, want_max)?
        {
            let cmp = lower_compare_op(op);
            let right = lower_value_expr(rhs, ctx)?;
            let ctrl_rc = ResolutionCondition {
                left: ctrl_expr,
                comparison: cmp,
                right: right.clone(),
            };
            let opp_rc = ResolutionCondition {
                left: opp_expr,
                comparison: cmp,
                right,
            };
            return Ok(if existential {
                rc_or(ctrl_rc, opp_rc)
            } else {
                rc_and(ctrl_rc, opp_rc)
            });
        }
    }
    if let (None, Some(existential)) = (lhs_any, rhs_any)
        && lhs_opp.is_none()
    {
        let want_max = match op {
            ast::CompareOp::Le | ast::CompareOp::Lt => Some(existential),
            ast::CompareOp::Ge | ast::CompareOp::Gt => Some(!existential),
            ast::CompareOp::Eq if is_zero_const(lhs) && is_nonneg_count_expr(rhs) => {
                Some(!existential)
            }
            ast::CompareOp::Eq => None,
        };
        if let Some(want_max) = want_max
            && let Some((ctrl_expr, opp_expr)) = try_lower_player_pair_extremum(rhs, want_max)?
        {
            let cmp = lower_compare_op(op);
            let left = lower_value_expr(lhs, ctx)?;
            let ctrl_rc = ResolutionCondition {
                left: left.clone(),
                comparison: cmp,
                right: ctrl_expr,
            };
            let opp_rc = ResolutionCondition {
                left,
                comparison: cmp,
                right: opp_expr,
            };
            return Ok(if existential {
                rc_or(ctrl_rc, opp_rc)
            } else {
                rc_and(ctrl_rc, opp_rc)
            });
        }
    }

    Ok(ResolutionCondition {
        left: lower_value_expr(lhs, ctx)?,
        comparison: lower_compare_op(op),
        right: lower_value_expr(rhs, ctx)?,
    })
}

const fn negate_comparison(op: Comparison) -> Comparison {
    match op {
        Comparison::Equal => Comparison::NotEqual,
        Comparison::NotEqual => Comparison::Equal,
        Comparison::Less => Comparison::GreaterOrEqual,
        Comparison::LessOrEqual => Comparison::Greater,
        Comparison::Greater => Comparison::LessOrEqual,
        Comparison::GreaterOrEqual => Comparison::Less,
    }
}

fn rc_ge_one(left: EngineValueExpr) -> ResolutionCondition {
    ResolutionCondition {
        left,
        comparison: Comparison::GreaterOrEqual,
        right: EngineValueExpr::literal(RulesInteger::from(1)),
    }
}

fn rc_eq_zero(left: EngineValueExpr) -> ResolutionCondition {
    ResolutionCondition {
        left,
        comparison: Comparison::Equal,
        right: EngineValueExpr::literal(RulesInteger::from(0)),
    }
}

fn rc_ge_u16(left: EngineValueExpr, min_count: u16) -> ResolutionCondition {
    ResolutionCondition {
        left,
        comparison: Comparison::GreaterOrEqual,
        right: EngineValueExpr::literal(RulesInteger::from(i64::from(min_count))),
    }
}

fn rc_and(a: ResolutionCondition, b: ResolutionCondition) -> ResolutionCondition {
    let zero = EngineValueExpr::literal(RulesInteger::from(0));
    let one = EngineValueExpr::literal(RulesInteger::from(1));
    let inner = EngineValueExpr::conditional(b.left, b.comparison, b.right, one, zero.clone());
    let combined = EngineValueExpr::conditional(a.left, a.comparison, a.right, inner, zero);
    rc_ge_one(combined)
}

fn rc_or(a: ResolutionCondition, b: ResolutionCondition) -> ResolutionCondition {
    let zero = EngineValueExpr::literal(RulesInteger::from(0));
    let one = EngineValueExpr::literal(RulesInteger::from(1));
    let inner = EngineValueExpr::conditional(b.left, b.comparison, b.right, one.clone(), zero);
    let combined = EngineValueExpr::conditional(a.left, a.comparison, a.right, one, inner);
    rc_ge_one(combined)
}

fn is_static_self_subject(subject: &v2::SubjectRef) -> bool {
    matches!(
        subject,
        v2::SubjectRef::SelfObject
            | v2::SubjectRef::Anaphora(
                v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatObject
                    | v2::AnaphoraRef::ThatSource
            )
    )
}

fn is_resolved_self_subject(subject: &v2::SubjectRef, ctx: &LowerCtx<'_>) -> bool {
    matches!(subject, v2::SubjectRef::SelfObject)
        || (!ctx.has_object_target && !ctx.in_trigger && is_static_self_subject(subject))
}

fn lower_match_object_ref(
    subject: &v2::SubjectRef,
    ctx: &LowerCtx<'_>,
) -> LowerResult<MatchObjectRef> {
    match subject {
        v2::SubjectRef::SelfObject => Ok(MatchObjectRef::AbilitySource),
        v2::SubjectRef::Anaphora(anaph) => match anaph {
            v2::AnaphoraRef::EquippedCreature
            | v2::AnaphoraRef::EnchantedCreature
            | v2::AnaphoraRef::EnchantedPermanent => Ok(MatchObjectRef::AttachedHost),
            v2::AnaphoraRef::RevealedCard => Ok(MatchObjectRef::RevealedTop),
            v2::AnaphoraRef::DiscardedObject => Ok(MatchObjectRef::DiscardedCard),
            v2::AnaphoraRef::DestroyedObject => Ok(MatchObjectRef::DestructionTarget),
            v2::AnaphoraRef::SacrificedObject => Ok(MatchObjectRef::SacrificedCreature),
            v2::AnaphoraRef::ExiledObject | v2::AnaphoraRef::TheExiledCard => {
                Ok(MatchObjectRef::LastMovedToExile)
            }
            v2::AnaphoraRef::SameTarget => Ok(MatchObjectRef::TargetObject),
            v2::AnaphoraRef::ThatSource => Ok(MatchObjectRef::AbilitySource),
            v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatCard
            | v2::AnaphoraRef::ThatSpell
            | v2::AnaphoraRef::ThatToken
            | v2::AnaphoraRef::ThatLand
            | v2::AnaphoraRef::ThatObject => {
                if ctx.has_object_target {
                    Ok(MatchObjectRef::TargetObject)
                } else if ctx.in_trigger {
                    Ok(MatchObjectRef::TriggerSubject)
                } else {
                    Ok(MatchObjectRef::AbilitySource)
                }
            }
            _ => Err(LoweringError::Unsupported("v2:cond:match-anaphora")),
        },
        _ => Err(LoweringError::Unsupported("v2:cond:match-subject")),
    }
}

fn lower_counter_subject_ref(
    subject: &v2::SubjectRef,
    ctx: &LowerCtx<'_>,
) -> LowerResult<CounterSubjectRef> {
    match subject {
        v2::SubjectRef::SelfObject => Ok(CounterSubjectRef::AbilitySource),
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::EquippedCreature
            | v2::AnaphoraRef::EnchantedCreature
            | v2::AnaphoraRef::EnchantedPermanent,
        ) => Ok(CounterSubjectRef::AttachedHost),
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget) => {
            Ok(CounterSubjectRef::TargetObject)
        }
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatObject
            | v2::AnaphoraRef::ThatSource,
        ) => {
            if ctx.has_object_target {
                Ok(CounterSubjectRef::TargetObject)
            } else if ctx.in_trigger {
                Ok(CounterSubjectRef::TriggerSubject)
            } else {
                Ok(CounterSubjectRef::AbilitySource)
            }
        }
        _ => Err(LoweringError::Unsupported("v2:cond:counter-subject")),
    }
}

/// Lower a [`v2::ConditionExpr`] into a toolbox [`EngineConditionExpr`].
pub(crate) fn lower_condition_expr(
    cond: &v2::ConditionExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineConditionExpr> {
    match cond {
        v2::ConditionExpr::And(a, b) => Ok(EngineConditionExpr::And(vec![
            lower_condition_expr(a, ctx)?,
            lower_condition_expr(b, ctx)?,
        ])),
        v2::ConditionExpr::Or(a, b) => Ok(EngineConditionExpr::Or(vec![
            lower_condition_expr(a, ctx)?,
            lower_condition_expr(b, ctx)?,
        ])),
        v2::ConditionExpr::Not(inner) => Ok(EngineConditionExpr::Not(Box::new(
            lower_condition_expr(inner, ctx)?,
        ))),
        v2::ConditionExpr::Compare { lhs, op, rhs } => {
            let rc = lower_compare_condition(lhs, *op, rhs, ctx)?;
            Ok(EngineConditionExpr::Compare {
                left: Box::new(rc.left),
                op: rc.comparison,
                right: Box::new(rc.right),
            })
        }
        other => {
            let rc = lower_resolution_condition(other, ctx)?;
            Ok(EngineConditionExpr::Compare {
                left: Box::new(rc.left),
                op: rc.comparison,
                right: Box::new(rc.right),
            })
        }
    }
}

/// Lower a [`v2::ConditionExpr`] into a scalar [`ResolutionCondition`].
pub(crate) fn lower_resolution_condition(
    cond: &v2::ConditionExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    match cond {
        v2::ConditionExpr::Compare { lhs, op, rhs } => lower_compare_condition(lhs, *op, rhs, ctx),
        v2::ConditionExpr::Not(inner) => {
            let mut rc = lower_resolution_condition(inner, ctx)?;
            rc.comparison = negate_comparison(rc.comparison);
            Ok(rc)
        }
        v2::ConditionExpr::And(a, b) => {
            let ra = lower_resolution_condition(a, ctx)?;
            let rb = lower_resolution_condition(b, ctx)?;
            Ok(rc_and(ra, rb))
        }
        v2::ConditionExpr::Or(a, b) => {
            let ra = lower_resolution_condition(a, ctx)?;
            let rb = lower_resolution_condition(b, ctx)?;
            Ok(rc_or(ra, rb))
        }
        v2::ConditionExpr::Controls {
            player,
            filter,
            min_count,
        } => lower_controls_condition(*player, filter, *min_count, ctx),
        v2::ConditionExpr::ControlsSubject { player, subject } => {
            lower_controls_subject_condition(*player, subject, ctx)
        }
        v2::ConditionExpr::ControlsMoreThan {
            player,
            other,
            filter,
        } => lower_controls_more_than_condition(*player, *other, filter, ctx),
        v2::ConditionExpr::HasKeyword { subject, keyword } => {
            let subj = lower_match_object_ref(subject, ctx)?;
            let filter = if *keyword == v2::KeywordKind::Toxic {
                EngineObjectFilter::any().with_flags(ObjectFilterFlags::HAS_TOXIC)
            } else {
                let ab = lower_keyword_kind_plain(*keyword)?;
                EngineObjectFilter::any().with_keyword(ab)
            };
            Ok(rc_ge_one(EngineValueExpr::object_matches(subj, filter)))
        }
        v2::ConditionExpr::SubjectState { subject, state } => {
            lower_subject_state_condition(subject, *state, ctx)
        }
        v2::ConditionExpr::SubjectMatches { subject, filter } => {
            let subj = lower_match_object_ref(subject, ctx)?;
            let obj_filter = lower_object_filter(filter, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::object_matches(subj, obj_filter)))
        }
        v2::ConditionExpr::SubjectAttachedTo { subject, target } => {
            if !is_resolved_self_subject(subject, ctx) {
                return Err(LoweringError::Unsupported("v2:cond:subject-attached-to"));
            }
            let obj_filter = lower_object_filter(target, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::object_matches(
                MatchObjectRef::AttachedHost,
                obj_filter,
            )))
        }
        v2::ConditionExpr::AttachedCount {
            subject,
            filter,
            min_count,
        } => lower_attached_count_condition(subject, filter, *min_count, ctx),
        v2::ConditionExpr::SharesTypeWith {
            subject,
            other,
            kind,
        } => lower_shares_type_condition(subject, other, *kind, ctx),
        v2::ConditionExpr::HasGreaterStatThan {
            subject,
            other,
            stat,
        } => {
            let stat_kind = stat.map(lower_stat_kind).ok_or(LoweringError::Unsupported(
                "v2:cond:greater-stat-unspecified",
            ))?;
            let lhs =
                EngineValueExpr::object_stat(lower_value_object_ref(subject, ctx)?, stat_kind);
            let rhs = EngineValueExpr::object_stat(lower_value_object_ref(other, ctx)?, stat_kind);
            Ok(ResolutionCondition {
                left: lhs,
                comparison: Comparison::Greater,
                right: rhs,
            })
        }
        v2::ConditionExpr::TargetsFilter { subject, filter } => {
            if ctx.has_object_target
                && matches!(
                    subject.as_ref(),
                    v2::SubjectRef::SelfObject
                        | v2::SubjectRef::Anaphora(
                            v2::AnaphoraRef::It
                                | v2::AnaphoraRef::ThatSpell
                                | v2::AnaphoraRef::ThatSource
                        )
                )
            {
                let obj_filter = lower_object_filter(filter, ctx)?;
                Ok(rc_ge_one(EngineValueExpr::object_matches(
                    MatchObjectRef::TargetObject,
                    obj_filter,
                )))
            } else {
                Err(LoweringError::Unsupported("v2:cond:targets-filter"))
            }
        }
        v2::ConditionExpr::ManaCostContains { .. } => {
            Err(LoweringError::Unsupported("v2:cond:mana-cost-contains"))
        }
        v2::ConditionExpr::SubjectCombatWith { .. } => {
            Err(LoweringError::Unsupported("v2:cond:subject-combat-with"))
        }
        v2::ConditionExpr::InZone { subject, zone } => {
            if !is_resolved_self_subject(subject, ctx) {
                return Err(LoweringError::Unsupported("v2:cond:in-zone-non-self"));
            }
            if !matches!(
                zone.owner,
                v2::ZoneOwner::Default
                    | v2::ZoneOwner::You
                    | v2::ZoneOwner::Any
                    | v2::ZoneOwner::All
                    | v2::ZoneOwner::ItsOwner
            ) {
                return Err(LoweringError::Unsupported("v2:cond:in-zone-owner"));
            }
            let sz = lower_zone_kind_to_source_zone_kind(zone.zone)?;
            Ok(rc_ge_one(EngineValueExpr::SourceState(
                SourceStateExpr::InZone(sz),
            )))
        }
        v2::ConditionExpr::CastFromZone { subject, zone } => {
            if !matches!(
                subject.as_ref(),
                v2::SubjectRef::SelfObject
                    | v2::SubjectRef::Anaphora(
                        v2::AnaphoraRef::It
                            | v2::AnaphoraRef::ThatSpell
                            | v2::AnaphoraRef::ThatPermanent
                            | v2::AnaphoraRef::ThatCard
                    )
            ) {
                return Err(LoweringError::Unsupported("v2:cond:cast-from-zone-subject"));
            }
            let state = match zone.zone {
                v2::ZoneKind::Graveyard => SourceStateExpr::CastFromGraveyard,
                v2::ZoneKind::Exile => SourceStateExpr::CastFromExile,
                v2::ZoneKind::Hand => SourceStateExpr::WasCast {
                    by_controller: false,
                    from_hand: true,
                },
                _ => return Err(LoweringError::Unsupported("v2:cond:cast-from-zone")),
            };
            Ok(rc_ge_one(EngineValueExpr::SourceState(state)))
        }
        v2::ConditionExpr::ZoneHasCount {
            filter,
            zone,
            min_count,
        } => {
            let mut cleaned = filter.as_ref().clone();
            cleaned.zones.clear();
            let linked_exile = zone.zone == v2::ZoneKind::Exile
                && cleaned.flags.contains(v2::FilterFlags::EXILED_WITH_SELF);
            if !linked_exile
                && matches!(
                    zone.owner,
                    v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents
                )
            {
                let opp_vz = opponent_zone_to_value_zone(zone.zone)
                    .ok_or(LoweringError::Unsupported("v2:value-zone:unsupported-zone"))?;
                let obj_filter = lower_object_filter(&cleaned, ctx)?;
                return Ok(rc_ge_u16(
                    EngineValueExpr::OpponentMaxCount {
                        zone: opp_vz,
                        filter: Box::new(obj_filter),
                    },
                    *min_count,
                ));
            }
            let vz = if linked_exile {
                cleaned.flags.0 &= !v2::FilterFlags::EXILED_WITH_SELF.0;
                ValueZone::LinkedExile
            } else {
                lower_value_zone(*zone, ctx)?
            };
            let obj_filter = lower_object_filter(&cleaned, ctx)?;
            Ok(rc_ge_u16(
                EngineValueExpr::Count {
                    zone: vz,
                    filter: Box::new(obj_filter),
                    controller_scope: None,
                },
                *min_count,
            ))
        }
        v2::ConditionExpr::ZoneMetric {
            zone,
            kind,
            min_count,
        } => match kind {
            v2::ZoneMetricKind::DistinctCardTypes => match (zone.zone, zone.owner) {
                (v2::ZoneKind::Graveyard, v2::ZoneOwner::Default | v2::ZoneOwner::You) => {
                    Ok(rc_ge_u16(
                        EngineValueExpr::PlayerStat(PlayerStatExpr::OwnGraveyardDistinctCardTypes),
                        *min_count,
                    ))
                }
                (
                    v2::ZoneKind::Graveyard,
                    v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::EachPlayer,
                ) => Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::AllGraveyardsDistinctCardTypes),
                    *min_count,
                )),
                _ => Err(LoweringError::Unsupported("v2:cond:zone-metric-distinct")),
            },
            v2::ZoneMetricKind::IsEmpty => {
                if matches!(
                    zone.owner,
                    v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents
                ) {
                    let opp_vz = opponent_zone_to_value_zone(zone.zone)
                        .ok_or(LoweringError::Unsupported("v2:value-zone:unsupported-zone"))?;
                    return Ok(rc_eq_zero(EngineValueExpr::OpponentMinCount {
                        zone: opp_vz,
                        filter: Box::new(EngineObjectFilter::any()),
                    }));
                }
                let vz = lower_value_zone(*zone, ctx)?;
                Ok(rc_eq_zero(EngineValueExpr::Count {
                    zone: vz,
                    filter: Box::new(EngineObjectFilter::any()),
                    controller_scope: None,
                }))
            }
            v2::ZoneMetricKind::DistinctPermanentTypes => Err(LoweringError::Unsupported(
                "v2:cond:zone-metric-permanent-types",
            )),
        },
        v2::ConditionExpr::PlayerTurnEvent {
            player,
            event,
            filter,
            min_count,
        } => lower_player_turn_event_condition(*player, *event, filter.as_deref(), *min_count, ctx),
        v2::ConditionExpr::PlayerHasCounters {
            player,
            kind,
            min_count,
        } => {
            let ck = lower_counter_kind(kind)?;
            if *player == v2::PlayerRef::AnyPlayer {
                let ctrl = rc_ge_u16(
                    EngineValueExpr::CounterCount {
                        subject: controller_counter_subject(ck),
                        kind: Some(ck),
                    },
                    *min_count,
                );
                let opp = rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
                    *min_count,
                );
                return Ok(rc_or(ctrl, opp));
            }
            if matches!(
                player,
                v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent
            ) {
                return Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
                    *min_count,
                ));
            }
            let subj = if *player == v2::PlayerRef::You {
                controller_counter_subject(ck)
            } else {
                CounterSubjectRef::PlayerCounter(lower_world_resolving_player(*player, ctx)?, ck)
            };
            Ok(rc_ge_u16(
                EngineValueExpr::CounterCount {
                    subject: subj,
                    kind: Some(ck),
                },
                *min_count,
            ))
        }
        v2::ConditionExpr::PermanentEnteredThisTurn {
            filter,
            controller,
            min_count,
        } => lower_permanent_entered_this_turn_condition(filter, *controller, *min_count, ctx),
        v2::ConditionExpr::SacrificedPermanentThisTurn { player, filter } => {
            let players = lower_world_affected_players(*player, ctx)?;
            let obj_filter = lower_object_filter(filter, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::SacrificedMatchingThisTurn {
                    players,
                    filter: Box::new(obj_filter),
                },
            )))
        }
        v2::ConditionExpr::PriorActionMatched { action, filter } => match action {
            v2::PriorActionKind::ExiledThisWay
            | v2::PriorActionKind::SacrificedThisWay
            | v2::PriorActionKind::DestroyedThisWay
            | v2::PriorActionKind::DiscardedThisWay
            | v2::PriorActionKind::RevealedThisWay
            | v2::PriorActionKind::ReturnedThisWay
            | v2::PriorActionKind::MilledThisWay
            | v2::PriorActionKind::SearchedThisWay
            | v2::PriorActionKind::CreatedThisWay => {
                let obj_filter = lower_object_filter(filter, ctx)?;
                Ok(rc_ge_one(EngineValueExpr::LastActionMatchingCount(
                    Box::new(obj_filter),
                )))
            }
            v2::PriorActionKind::DealtDamageThisWay if ctx.last_damage_was_self => {
                let obj_filter = lower_object_filter(filter, ctx)?
                    .with_flags(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN);
                Ok(rc_ge_one(EngineValueExpr::Count {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj_filter),
                    controller_scope: None,
                }))
            }
            _ => Err(LoweringError::Unsupported("v2:cond:prior-action-matched")),
        },
        v2::ConditionExpr::ColoredManaSpent { color, min_count } => {
            let set = lower_color_set(*color);
            if set.len() != 1 {
                return Err(LoweringError::Unsupported("v2:cond:multi-color-mana-spent"));
            }
            let Some(c) = set.into_iter().next() else {
                return Err(LoweringError::Unsupported("v2:cond:empty-mana-spent"));
            };
            Ok(rc_ge_u16(
                EngineValueExpr::SourceState(SourceStateExpr::ManaTypeSpentToCast(
                    color_to_mana_type(c),
                )),
                u16::from(*min_count),
            ))
        }
        v2::ConditionExpr::Flag(flag) => lower_condition_flag(*flag),
        v2::ConditionExpr::PlayerCheck { player, kind } => {
            lower_player_check_condition(*player, *kind, ctx)
        }
        v2::ConditionExpr::SubjectCheck { subject, kind } => {
            lower_subject_check_condition(subject, kind, ctx)
        }
        v2::ConditionExpr::FilterCheck {
            filter,
            kind,
            value,
        } => lower_filter_check_condition(filter, *kind, *value, ctx),
        v2::ConditionExpr::EventCheck(kind) => lower_event_check_condition(kind),
    }
}

fn lower_controls_condition(
    player: v2::PlayerRef,
    filter: &v2::ObjectFilter,
    min_count: u16,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    let mut cleaned = filter.clone();
    cleaned.zones.clear();
    let obj_filter = lower_object_filter(&cleaned, ctx)?;

    match player {
        v2::PlayerRef::You => Ok(rc_ge_u16(
            EngineValueExpr::count_battlefield(obj_filter.controlled()),
            min_count,
        )),
        v2::PlayerRef::AnyOpponent => Ok(rc_ge_u16(
            EngineValueExpr::OpponentMaxCount {
                zone: ValueZone::Battlefield,
                filter: Box::new(obj_filter),
            },
            min_count,
        )),
        v2::PlayerRef::EachOpponent => Ok(rc_ge_u16(
            EngineValueExpr::OpponentMinCount {
                zone: ValueZone::Battlefield,
                filter: Box::new(obj_filter),
            },
            min_count,
        )),
        v2::PlayerRef::AnyPlayer => Ok(rc_or(
            rc_ge_u16(
                EngineValueExpr::count_battlefield(obj_filter.controlled()),
                min_count,
            ),
            rc_ge_u16(
                EngineValueExpr::OpponentMaxCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj_filter),
                },
                min_count,
            ),
        )),
        v2::PlayerRef::EachPlayer => Ok(rc_and(
            rc_ge_u16(
                EngineValueExpr::count_battlefield(obj_filter.controlled()),
                min_count,
            ),
            rc_ge_u16(
                EngineValueExpr::OpponentMinCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj_filter),
                },
                min_count,
            ),
        )),
        other => {
            let rp = lower_world_resolving_player(other, ctx)?;
            Ok(rc_ge_u16(
                EngineValueExpr::count_player_battlefield(rp, obj_filter),
                min_count,
            ))
        }
    }
}

fn lower_controls_subject_condition(
    player: v2::PlayerRef,
    subject: &v2::SubjectRef,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    let subj = lower_match_object_ref(subject, ctx)?;
    let ctrl = match player {
        v2::PlayerRef::You => ControllerFilter::You,
        v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => ControllerFilter::Opponent,
        v2::PlayerRef::TargetPlayer => ControllerFilter::TargetPlayer,
        v2::PlayerRef::TargetOpponent => ControllerFilter::TargetOpponent,
        v2::PlayerRef::DefendingPlayer => ControllerFilter::DefendingPlayer,
        v2::PlayerRef::ActivePlayer => ControllerFilter::ActivePlayer,
        v2::PlayerRef::ThatPlayer => ControllerFilter::RelatedPlayer,
        _ => {
            return Err(LoweringError::Unsupported(
                "v2:cond:controls-subject-player",
            ));
        }
    };
    let mut clause = ObjectClause::any();
    clause.controller = ctrl;
    Ok(rc_ge_one(EngineValueExpr::object_matches(
        subj,
        EngineObjectFilter::Clause(clause),
    )))
}

fn lower_controls_more_than_condition(
    player: v2::PlayerRef,
    other: v2::PlayerRef,
    filter: &v2::ObjectFilter,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    let is_opp_quant =
        |p: v2::PlayerRef| matches!(p, v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent);
    if is_opp_quant(player) && is_opp_quant(other) {
        return Err(LoweringError::Unsupported("v2:player:any-opponent"));
    }

    let mut cleaned = filter.clone();
    cleaned.zones.clear();
    let obj_filter = lower_object_filter(&cleaned, ctx)?;

    let side = |p: v2::PlayerRef, is_left: bool| -> LowerResult<EngineValueExpr> {
        match p {
            v2::PlayerRef::You => Ok(EngineValueExpr::count_battlefield(obj_filter.controlled())),
            v2::PlayerRef::AnyOpponent => Ok(if is_left {
                EngineValueExpr::OpponentMaxCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj_filter),
                }
            } else {
                EngineValueExpr::OpponentMinCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj_filter),
                }
            }),
            v2::PlayerRef::EachOpponent => Ok(if is_left {
                EngineValueExpr::OpponentMinCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj_filter),
                }
            } else {
                EngineValueExpr::OpponentMaxCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj_filter),
                }
            }),
            other_p => {
                let rp = lower_world_resolving_player(other_p, ctx)?;
                Ok(EngineValueExpr::count_player_battlefield(rp, obj_filter))
            }
        }
    };

    Ok(ResolutionCondition {
        left: side(player, true)?,
        comparison: Comparison::Greater,
        right: side(other, false)?,
    })
}

fn lower_subject_state_condition(
    subject: &v2::SubjectRef,
    state: v2::ObjectStateKind,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    if is_resolved_self_subject(subject, ctx) {
        match state {
            v2::ObjectStateKind::AttackingAlone => {
                return Ok(rc_ge_one(EngineValueExpr::SourceState(
                    SourceStateExpr::AttackingAlone,
                )));
            }
            v2::ObjectStateKind::Saddled => {
                return Ok(rc_ge_one(EngineValueExpr::SourceState(
                    SourceStateExpr::Saddled,
                )));
            }
            v2::ObjectStateKind::Paired => {
                return Ok(rc_ge_one(EngineValueExpr::SourceState(
                    SourceStateExpr::Paired,
                )));
            }
            v2::ObjectStateKind::Monstrous => {
                return Ok(rc_ge_one(EngineValueExpr::SourceState(
                    SourceStateExpr::Monstrous,
                )));
            }
            v2::ObjectStateKind::AttackedOrBlockedThisTurn => {
                return Ok(rc_ge_one(EngineValueExpr::SourceState(
                    SourceStateExpr::AttackedOrBlockedThisTurn,
                )));
            }
            _ => {}
        }
    }

    let subj = lower_match_object_ref(subject, ctx)?;
    let filter = match state {
        v2::ObjectStateKind::Tapped => EngineObjectFilter::any().tapped(),
        v2::ObjectStateKind::Untapped => EngineObjectFilter::Clause(ObjectClause::any().untapped()),
        v2::ObjectStateKind::Attacking => EngineObjectFilter::any().combat(CombatFilter::Attacking),
        v2::ObjectStateKind::Blocking => EngineObjectFilter::any().combat(CombatFilter::Blocking),
        v2::ObjectStateKind::Blocked => EngineObjectFilter::any().combat(CombatFilter::Blocked),
        v2::ObjectStateKind::Unblocked => EngineObjectFilter::any().combat(CombatFilter::Unblocked),
        v2::ObjectStateKind::Equipped => {
            EngineObjectFilter::any().with_flags(ObjectFilterFlags::EQUIPPED)
        }
        v2::ObjectStateKind::Enchanted => {
            EngineObjectFilter::any().with_flags(ObjectFilterFlags::ENCHANTED)
        }
        v2::ObjectStateKind::Modified => {
            EngineObjectFilter::any().with_flags(ObjectFilterFlags::MODIFIED)
        }
        v2::ObjectStateKind::FaceDown => {
            EngineObjectFilter::any().with_flags(ObjectFilterFlags::FACE_DOWN)
        }
        v2::ObjectStateKind::Saddled => {
            EngineObjectFilter::any().with_flags(ObjectFilterFlags::SADDLED)
        }
        v2::ObjectStateKind::Monstrous => {
            EngineObjectFilter::any().with_flags(ObjectFilterFlags::MONSTROUS)
        }
        v2::ObjectStateKind::Paired => {
            return Err(LoweringError::Unsupported("v2:cond:state-paired-non-self"));
        }
        v2::ObjectStateKind::AttackedOrBlockedThisTurn => {
            return Err(LoweringError::Unsupported(
                "v2:cond:state-attacked-or-blocked-non-self",
            ));
        }
        v2::ObjectStateKind::AttackingAlone => {
            return Err(LoweringError::Unsupported(
                "v2:cond:state-attacking-alone-non-self",
            ));
        }
    };
    Ok(rc_ge_one(EngineValueExpr::object_matches(subj, filter)))
}

fn lower_attached_count_condition(
    subject: &v2::SubjectRef,
    filter: &v2::ObjectFilter,
    min_count: u16,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    let simple = filter.forbidden_types.is_empty()
        && filter.required_supertypes.is_empty()
        && filter.forbidden_supertypes.is_empty()
        && filter.forbidden_subtypes.is_empty()
        && filter.any_colors.is_empty()
        && filter.forbidden_colors.is_empty()
        && filter.color_arity.is_none()
        && filter.controller.is_none()
        && filter.owner.is_none()
        && filter.zones.is_empty()
        && filter.combat.is_none()
        && filter.tapped.is_none()
        && filter.token.is_none()
        && filter.required_keywords.is_empty()
        && filter.forbidden_keywords.is_empty()
        && filter.stat_bounds.is_empty()
        && filter.flags.is_empty()
        && filter.predicates.is_empty();
    if !simple {
        return Err(LoweringError::Unsupported("v2:cond:attached-count-filter"));
    }

    if is_resolved_self_subject(subject, ctx) {
        if filter.subtypes.len() == 1 && filter.subtypes[0].eq_ignore_ascii_case("equipment") {
            return Ok(rc_ge_u16(
                EngineValueExpr::AttachedCount(AttachedCountKind::EquipmentOnSource),
                min_count,
            ));
        }
        if filter.is_empty()
            || (filter.subtypes.len() == 2
                && filter
                    .subtypes
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case("aura"))
                && filter
                    .subtypes
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case("equipment")))
        {
            return Ok(rc_ge_u16(
                EngineValueExpr::AttachedCount(AttachedCountKind::AurasAndEquipmentOnSource),
                min_count,
            ));
        }
        return Err(LoweringError::Unsupported("v2:cond:attached-count-filter"));
    }
    if matches!(subject, v2::SubjectRef::Player(v2::PlayerRef::ThatPlayer))
        && filter.subtypes.len() == 1
        && filter.subtypes[0].eq_ignore_ascii_case("curse")
    {
        return Ok(rc_ge_u16(
            EngineValueExpr::AttachedCount(AttachedCountKind::CursesOnRelatedPlayer),
            min_count,
        ));
    }
    Err(LoweringError::Unsupported("v2:cond:attached-count-subject"))
}

fn lower_shares_type_condition(
    subject: &v2::SubjectRef,
    other: &v2::SubjectRef,
    kind: v2::SharedProperty,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    let shared_stat = match kind {
        v2::SharedProperty::ManaValue => Some(ast::StatKind::ManaValue),
        v2::SharedProperty::Power => Some(ast::StatKind::Power),
        v2::SharedProperty::Toughness => Some(ast::StatKind::Toughness),
        _ => None,
    };
    if let Some(stat) = shared_stat {
        let stat_kind = lower_stat_kind(stat);
        let lhs = EngineValueExpr::object_stat(lower_value_object_ref(subject, ctx)?, stat_kind);
        let rhs = EngineValueExpr::object_stat(lower_value_object_ref(other, ctx)?, stat_kind);
        return Ok(ResolutionCondition {
            left: lhs,
            comparison: Comparison::Equal,
            right: rhs,
        });
    }

    let subj = lower_match_object_ref(subject, ctx)?;
    let is_source_ref = matches!(
        other,
        v2::SubjectRef::SelfObject | v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSource)
    ) || (!ctx.has_object_target
        && !ctx.in_trigger
        && matches!(
            other,
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
    let flag = if is_source_ref {
        match kind {
            v2::SharedProperty::Color => ObjectFilterFlags::SHARES_COLOR_WITH_SOURCE,
            v2::SharedProperty::CreatureType => ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_SOURCE,
            v2::SharedProperty::CardType => ObjectFilterFlags::SHARES_CARD_TYPE_WITH_SOURCE,
            v2::SharedProperty::Name => ObjectFilterFlags::SAME_NAME_AS_SOURCE,
            _ => return Err(LoweringError::Unsupported("v2:cond:shares-type-with")),
        }
    } else {
        match (other, kind) {
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::SameTarget
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatObject,
                ),
                v2::SharedProperty::Color,
            ) if ctx.has_object_target
                || matches!(other, v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget)) =>
            {
                ObjectFilterFlags::SHARES_COLOR_WITH_TARGET
            }
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::SameTarget
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatObject,
                ),
                v2::SharedProperty::CreatureType,
            ) if ctx.has_object_target
                || matches!(other, v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget)) =>
            {
                ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_TARGET
            }
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::SameTarget
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatObject,
                ),
                v2::SharedProperty::CardType,
            ) if ctx.has_object_target
                || matches!(other, v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget)) =>
            {
                ObjectFilterFlags::SHARES_CARD_TYPE_WITH_TARGET
            }
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::SameTarget
                    | v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatObject,
                ),
                v2::SharedProperty::Name,
            ) if ctx.has_object_target
                || matches!(other, v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget)) =>
            {
                ObjectFilterFlags::SAME_NAME_AS_TARGET
            }
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatObject,
                ),
                v2::SharedProperty::Color,
            ) if ctx.in_trigger && ctx.last_moved_binding.is_none() => {
                ObjectFilterFlags::SHARES_COLOR_WITH_TRIGGER
            }
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatObject,
                ),
                v2::SharedProperty::CreatureType,
            ) if ctx.in_trigger && ctx.last_moved_binding.is_none() => {
                ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_TRIGGER
            }
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatObject,
                ),
                v2::SharedProperty::CardType,
            ) if ctx.in_trigger && ctx.last_moved_binding.is_none() => {
                ObjectFilterFlags::SHARES_CARD_TYPE_WITH_TRIGGER
            }
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::ThatCreature
                    | v2::AnaphoraRef::ThatPermanent
                    | v2::AnaphoraRef::ThatCard
                    | v2::AnaphoraRef::ThatSpell
                    | v2::AnaphoraRef::ThatLand
                    | v2::AnaphoraRef::ThatToken
                    | v2::AnaphoraRef::It
                    | v2::AnaphoraRef::ThatObject,
                ),
                v2::SharedProperty::Name,
            ) if ctx.in_trigger && ctx.last_moved_binding.is_none() => {
                ObjectFilterFlags::SAME_NAME_AS_TRIGGER
            }
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::SacrificedObject
                    | v2::AnaphoraRef::ExiledObject
                    | v2::AnaphoraRef::TheExiledCard
                    | v2::AnaphoraRef::DiscardedObject
                    | v2::AnaphoraRef::RevealedCard,
                ),
                v2::SharedProperty::Color,
            ) => ObjectFilterFlags::SHARES_COLOR_WITH_LAST_SACRIFICED,
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::SacrificedObject
                    | v2::AnaphoraRef::ExiledObject
                    | v2::AnaphoraRef::TheExiledCard
                    | v2::AnaphoraRef::DiscardedObject
                    | v2::AnaphoraRef::RevealedCard,
                ),
                v2::SharedProperty::CreatureType,
            ) => ObjectFilterFlags::SHARES_CREATURE_TYPE_WITH_LAST_MOVED,
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::SacrificedObject
                    | v2::AnaphoraRef::ExiledObject
                    | v2::AnaphoraRef::TheExiledCard
                    | v2::AnaphoraRef::DiscardedObject
                    | v2::AnaphoraRef::RevealedCard,
                ),
                v2::SharedProperty::CardType,
            ) => ObjectFilterFlags::SHARES_CARD_TYPE_WITH_LAST_MOVED,
            (
                v2::SubjectRef::Anaphora(
                    v2::AnaphoraRef::SacrificedObject
                    | v2::AnaphoraRef::ExiledObject
                    | v2::AnaphoraRef::TheExiledCard
                    | v2::AnaphoraRef::DiscardedObject
                    | v2::AnaphoraRef::RevealedCard,
                ),
                v2::SharedProperty::Name,
            ) => ObjectFilterFlags::SAME_NAME_AS_LAST_MOVED,
            _ => return Err(LoweringError::Unsupported("v2:cond:shares-type-with")),
        }
    };
    Ok(rc_ge_one(EngineValueExpr::object_matches(
        subj,
        EngineObjectFilter::any().with_flags(flag),
    )))
}

fn lower_player_turn_event_condition(
    player: v2::PlayerRef,
    event: v2::TurnEventKind,
    filter: Option<&v2::ObjectFilter>,
    min_count: u16,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    match event {
        v2::TurnEventKind::SpellCast => {
            if let Some(f) = filter {
                let mut cleaned = f.clone();
                let from_zone = match cleaned.zones.as_slice() {
                    [] => None,
                    [only] => {
                        let z = *only;
                        cleaned.zones.clear();
                        Some(lower_source_zone_filter(z)?)
                    }
                    _ => {
                        return Err(LoweringError::Unsupported("v2:cond:spell-cast-multi-zone"));
                    }
                };
                let players = lower_world_affected_players(player, ctx)?;
                let obj_filter = lower_object_filter(&cleaned, ctx)?;
                return Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                        players,
                        filter: Box::new(obj_filter),
                        from_zone,
                    }),
                    min_count,
                ));
            }
            match player {
                v2::PlayerRef::You => Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                    min_count,
                )),
                v2::PlayerRef::AnyOpponent if min_count == 1 => {
                    Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                        PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::CastSpells),
                    )))
                }
                other => {
                    let players = lower_world_affected_players(other, ctx)?;
                    Ok(rc_ge_u16(
                        EngineValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                            players,
                            filter: Box::new(EngineObjectFilter::any()),
                            from_zone: None,
                        }),
                        min_count,
                    ))
                }
            }
        }
        v2::TurnEventKind::CardDrawn => {
            if filter.is_some() {
                return Err(LoweringError::Unsupported("v2:cond:filtered-card-drawn"));
            }
            match player {
                v2::PlayerRef::You => Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::CardsDrawnThisTurn),
                    min_count,
                )),
                v2::PlayerRef::AnyOpponent if min_count == 1 => {
                    Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                        PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::DrewCards),
                    )))
                }
                _ => Err(LoweringError::Unsupported("v2:cond:card-drawn-player")),
            }
        }
        v2::TurnEventKind::CardDiscarded => {
            if filter.is_some() {
                return Err(LoweringError::Unsupported("v2:cond:filtered-discard"));
            }
            match player {
                v2::PlayerRef::You => Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerDiscardedThisTurn),
                    min_count,
                )),
                v2::PlayerRef::AnyOpponent if min_count == 1 => {
                    Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                        PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::Discarded),
                    )))
                }
                _ => Err(LoweringError::Unsupported("v2:cond:card-discarded-player")),
            }
        }
        v2::TurnEventKind::LifeGained => {
            if filter.is_some() {
                return Err(LoweringError::Unsupported("v2:cond:filtered-life-gained"));
            }
            match player {
                v2::PlayerRef::You => Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn),
                    min_count,
                )),
                v2::PlayerRef::AnyOpponent if min_count == 1 => {
                    Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                        PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::GainedLife),
                    )))
                }
                _ => Err(LoweringError::Unsupported("v2:cond:life-gained-player")),
            }
        }
        v2::TurnEventKind::LifeLost => {
            if filter.is_some() {
                return Err(LoweringError::Unsupported("v2:cond:filtered-life-lost"));
            }
            match player {
                v2::PlayerRef::You => Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
                    min_count,
                )),
                v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                    if min_count == 1 {
                        Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                            PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::LostLife),
                        )))
                    } else {
                        Ok(rc_ge_u16(
                            EngineValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
                            min_count,
                        ))
                    }
                }
                _ => Err(LoweringError::Unsupported("v2:cond:life-lost-player")),
            }
        }
        v2::TurnEventKind::CreatureDied => {
            if let Some(f) = filter {
                let controller = match player {
                    v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => None,
                    other => Some(lower_world_resolving_player(other, ctx)?),
                };
                let obj_filter = lower_object_filter(f, ctx)?;
                return Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(
                        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                            filter: Box::new(obj_filter),
                            from_battlefield_only: true,
                            controller,
                        },
                    ),
                    min_count,
                ));
            }
            match player {
                v2::PlayerRef::You => Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::ControlledCreatureDiedThisTurn),
                    min_count,
                )),
                v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn),
                    min_count,
                )),
                other => {
                    let rp = lower_world_resolving_player(other, ctx)?;
                    Ok(rc_ge_u16(
                        EngineValueExpr::PlayerStat(
                            PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                                filter: Box::new(EngineObjectFilter::creature()),
                                from_battlefield_only: true,
                                controller: Some(rp),
                            },
                        ),
                        min_count,
                    ))
                }
            }
        }
    }
}

fn lower_permanent_entered_this_turn_condition(
    filter: &v2::ObjectFilter,
    controller: v2::PlayerRef,
    min_count: u16,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    let plain_no_extras = filter.forbidden_types.is_empty()
        && filter.required_supertypes.is_empty()
        && filter.forbidden_supertypes.is_empty()
        && filter.subtypes.is_empty()
        && filter.forbidden_subtypes.is_empty()
        && filter.any_colors.is_empty()
        && filter.forbidden_colors.is_empty()
        && filter.color_arity.is_none()
        && filter.controller.is_none()
        && filter.owner.is_none()
        && filter.zones.is_empty()
        && filter.combat.is_none()
        && filter.tapped.is_none()
        && filter.token.is_none()
        && filter.required_keywords.is_empty()
        && filter.forbidden_keywords.is_empty()
        && filter.stat_bounds.is_empty()
        && filter.flags.is_empty()
        && filter.predicates.is_empty();

    if !plain_no_extras {
        return Err(LoweringError::Unsupported(
            "v2:cond:filtered-entered-this-turn",
        ));
    }

    let is_land = (filter.all_types == ast::CardTypeMask::LAND && filter.any_types.is_empty())
        || (filter.any_types == ast::CardTypeMask::LAND && filter.all_types.is_empty());
    let is_creature = (filter.all_types == ast::CardTypeMask::CREATURE
        && filter.any_types.is_empty())
        || (filter.any_types == ast::CardTypeMask::CREATURE && filter.all_types.is_empty());

    if is_land {
        let rp = lower_world_resolving_player(controller, ctx)?;
        return Ok(rc_ge_u16(
            EngineValueExpr::PlayerStat(PlayerStatExpr::LandsEnteredThisTurn(rp)),
            min_count,
        ));
    }
    if controller == v2::PlayerRef::AnyOpponent && is_creature && min_count == 1 {
        return Ok(rc_ge_one(EngineValueExpr::PlayerStat(
            PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::CreatureEntered),
        )));
    }
    if controller == v2::PlayerRef::You {
        if filter.all_types.is_empty() && filter.any_types.is_empty() {
            return Ok(rc_ge_u16(
                EngineValueExpr::PlayerStat(PlayerStatExpr::EnteredPermanentThisTurn),
                min_count,
            ));
        }
        let all = crate::v2_lowering::atoms::lower_card_type_set(filter.all_types);
        let any = crate::v2_lowering::atoms::lower_card_type_set(filter.any_types);
        if all.len() == 1
            && any.is_empty()
            && let Some(ct) = all.iter().next().copied()
        {
            return Ok(rc_ge_u16(
                EngineValueExpr::PlayerStat(PlayerStatExpr::EnteredTypeThisTurn(ct)),
                min_count,
            ));
        }
        if any.len() == 1
            && all.is_empty()
            && let Some(ct) = any.into_iter().next()
        {
            return Ok(rc_ge_u16(
                EngineValueExpr::PlayerStat(PlayerStatExpr::EnteredTypeThisTurn(ct)),
                min_count,
            ));
        }
    }

    Err(LoweringError::Unsupported(
        "v2:cond:permanent-entered-this-turn",
    ))
}

fn lower_condition_flag(flag: v2::ConditionFlag) -> LowerResult<ResolutionCondition> {
    match flag {
        v2::ConditionFlag::PermanentDiedThisTurn => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
            PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                filter: Box::new(EngineObjectFilter::any()),
                from_battlefield_only: true,
                controller: None,
            },
        ))),
        v2::ConditionFlag::PermanentLeftThisTurn => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
            PlayerStatExpr::PermanentLeftBattlefieldThisTurn,
        ))),
        v2::ConditionFlag::HasCitysBlessing => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
            PlayerStatExpr::ControllerHasCityBlessing,
        ))),
        v2::ConditionFlag::HasInitiative => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
            PlayerStatExpr::HasInitiative,
        ))),
        v2::ConditionFlag::GiftWasPromised => Ok(rc_ge_one(EngineValueExpr::SourceState(
            SourceStateExpr::GiftPromised,
        ))),
        v2::ConditionFlag::WasBargained => Ok(rc_ge_one(EngineValueExpr::SourceState(
            SourceStateExpr::Bargained,
        ))),
        v2::ConditionFlag::WonCoinFlip => Ok(rc_ge_one(EngineValueExpr::EventValue(
            EventValueKind::LastCoinFlipWon,
        ))),
        v2::ConditionFlag::TributeNotPaid => Ok(rc_eq_zero(EngineValueExpr::SourceState(
            SourceStateExpr::TributePaid,
        ))),
        v2::ConditionFlag::AdditionalCostPaid => Ok(rc_ge_one(EngineValueExpr::EventValue(
            EventValueKind::AdditionalCostPaid,
        ))),
        v2::ConditionFlag::ManaSpentNoColor => Ok(rc_eq_zero(EngineValueExpr::SourceState(
            SourceStateExpr::ColorsSpentToCast,
        ))),
        v2::ConditionFlag::IsFirstCombatPhase => Ok(ResolutionCondition {
            left: EngineValueExpr::PlayerStat(PlayerStatExpr::CombatPhasesThisTurn),
            comparison: Comparison::Equal,
            right: EngineValueExpr::literal(RulesInteger::from(1)),
        }),
        v2::ConditionFlag::ControlsSelf => Ok(rc_ge_one(EngineValueExpr::object_matches(
            MatchObjectRef::AbilitySource,
            EngineObjectFilter::any().controlled(),
        ))),
        v2::ConditionFlag::IsYourTurn => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
            PlayerStatExpr::ControllerIsActivePlayer,
        ))),
        v2::ConditionFlag::IsDay => Ok(ResolutionCondition {
            left: EngineValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
            comparison: Comparison::Equal,
            right: EngineValueExpr::literal(RulesInteger::from(1)),
        }),
        v2::ConditionFlag::IsNight => Ok(ResolutionCondition {
            left: EngineValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
            comparison: Comparison::Equal,
            right: EngineValueExpr::literal(RulesInteger::from(2)),
        }),
        v2::ConditionFlag::CreatureAttackingYou => {
            Ok(rc_ge_one(EngineValueExpr::CountAttackingController))
        }
        v2::ConditionFlag::CardsExiledWithSelf => Ok(rc_ge_one(EngineValueExpr::Count {
            zone: ValueZone::LinkedExile,
            filter: Box::new(EngineObjectFilter::any()),
            controller_scope: None,
        })),
        v2::ConditionFlag::NoCreaturesAttackedThisTurn => Ok(rc_eq_zero(
            EngineValueExpr::PlayerStat(PlayerStatExpr::AttackedMatchingThisTurn {
                players: AffectedPlayers::EachPlayer,
                filter: Box::new(EngineObjectFilter::creature()),
            }),
        )),
        v2::ConditionFlag::NoCreaturesOnBattlefield => Ok(rc_eq_zero(
            EngineValueExpr::count_battlefield(EngineObjectFilter::creature()),
        )),
        v2::ConditionFlag::WasKicked => Ok(rc_ge_one(EngineValueExpr::EventValue(
            EventValueKind::KickerCount(0),
        ))),
        v2::ConditionFlag::WasCastFromHand => Ok(rc_ge_one(EngineValueExpr::SourceState(
            SourceStateExpr::WasCast {
                by_controller: false,
                from_hand: true,
            },
        ))),
        v2::ConditionFlag::YouCastIt => Ok(rc_ge_one(EngineValueExpr::SourceState(
            SourceStateExpr::WasCast {
                by_controller: true,
                from_hand: false,
            },
        ))),
        v2::ConditionFlag::HasEnduringStory => Err(LoweringError::Unsupported(
            "v2:cond:flag:has-enduring-story",
        )),
        v2::ConditionFlag::CastUsingTeamwork => Err(LoweringError::Unsupported(
            "v2:cond:flag:cast-using-teamwork",
        )),
        v2::ConditionFlag::FirstTimeEachTurn => Err(LoweringError::Unsupported(
            "v2:cond:flag:first-time-each-turn",
        )),
        v2::ConditionFlag::IsNotManaAbility => Err(LoweringError::Unsupported(
            "v2:cond:flag:is-not-mana-ability",
        )),
        v2::ConditionFlag::LesserManaValueSpell => Err(LoweringError::Unsupported(
            "v2:cond:flag:lesser-mana-value-spell",
        )),
        v2::ConditionFlag::NoOtherCreatureHasGreaterPower => Err(LoweringError::Unsupported(
            "v2:cond:flag:no-other-greater-power",
        )),
    }
}

fn lower_player_check_condition(
    player: v2::PlayerRef,
    kind: v2::PlayerConditionKind,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    match kind {
        v2::PlayerConditionKind::AttackedThisTurn => match player {
            v2::PlayerRef::You => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::ControllerAttackedThisTurn,
            ))),
            v2::PlayerRef::AnyOpponent => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::Attacked),
            ))),
            other => {
                let players = lower_world_affected_players(other, ctx)?;
                Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                    PlayerStatExpr::AttackedMatchingThisTurn {
                        players,
                        filter: Box::new(EngineObjectFilter::creature()),
                    },
                )))
            }
        },
        v2::PlayerConditionKind::IsMonarch => match player {
            v2::PlayerRef::You => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::ControllerIsMonarch,
            ))),
            v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => Ok(rc_ge_one(
                EngineValueExpr::PlayerStat(PlayerStatExpr::OpponentIsMonarch),
            )),
            _ => Err(LoweringError::Unsupported("v2:cond:player:monarch")),
        },
        v2::PlayerConditionKind::HasFullParty => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported("v2:cond:player:full-party"));
            }
            let roles = lookup_party_roles(ctx)?;
            Ok(rc_ge_u16(EngineValueExpr::PartyCount(Box::new(roles)), 4))
        }
        v2::PlayerConditionKind::Poisoned => match player {
            v2::PlayerRef::You => Ok(rc_ge_one(EngineValueExpr::CounterCount {
                subject: CounterSubjectRef::ControllerPoison,
                kind: Some(spellwake_engine::effects::CounterKind::Poison),
            })),
            v2::PlayerRef::AnyOpponent => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::AnyOpponentPoisoned,
            ))),
            other => {
                let rp = lower_world_resolving_player(other, ctx)?;
                Ok(rc_ge_one(EngineValueExpr::CounterCount {
                    subject: CounterSubjectRef::PlayerCounter(
                        rp,
                        spellwake_engine::effects::CounterKind::Poison,
                    ),
                    kind: Some(spellwake_engine::effects::CounterKind::Poison),
                }))
            }
        },
        v2::PlayerConditionKind::CardsLeftGraveyardThisTurn => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported(
                    "v2:cond:player:cards-left-gy-other",
                ));
            }
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::CardsLeftGraveyardThisTurn,
            )))
        }
        v2::PlayerConditionKind::LandEnteredThisTurn => {
            let rp = lower_world_resolving_player(player, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::LandsEnteredThisTurn(rp),
            )))
        }
        v2::PlayerConditionKind::CompletedDungeon => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported(
                    "v2:cond:player:completed-dungeon-other",
                ));
            }
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::CompletedDungeons,
            )))
        }
        v2::PlayerConditionKind::DescendedThisTurn => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported("v2:cond:player:descended-other"));
            }
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::DescendedThisTurn,
            )))
        }
        v2::PlayerConditionKind::CastSpellFromHandThisTurn => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported(
                    "v2:cond:player:cast-from-hand-other",
                ));
            }
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::CastSpellsFromHandThisTurn,
            )))
        }
        v2::PlayerConditionKind::ControlsCommander => match player {
            v2::PlayerRef::You => Ok(rc_ge_one(EngineValueExpr::count_battlefield(
                EngineObjectFilter::any().controlled().commander(),
            ))),
            v2::PlayerRef::AnyOpponent => Ok(rc_ge_one(EngineValueExpr::OpponentMaxCount {
                zone: ValueZone::Battlefield,
                filter: Box::new(EngineObjectFilter::any().commander()),
            })),
            v2::PlayerRef::EachOpponent => Ok(rc_ge_one(EngineValueExpr::OpponentMinCount {
                zone: ValueZone::Battlefield,
                filter: Box::new(EngineObjectFilter::any().commander()),
            })),
            other => {
                let rp = lower_world_resolving_player(other, ctx)?;
                Ok(rc_ge_one(EngineValueExpr::count_player_battlefield(
                    rp,
                    EngineObjectFilter::any().commander(),
                )))
            }
        },
        v2::PlayerConditionKind::CommittedCrimeThisTurn => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported("v2:cond:player:crime-other"));
            }
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::CommittedCrimeThisTurn,
            )))
        }
        v2::PlayerConditionKind::LostLifeLastTurn => {
            let rp = lower_world_resolving_player(player, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::PlayerLifeLostLastTurn(rp),
            )))
        }
        v2::PlayerConditionKind::LostLifeThisTurn => match player {
            v2::PlayerRef::You => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::ControllerLifeLostThisTurn,
            ))),
            v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                    PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::LostLife),
                )))
            }
            _ => Err(LoweringError::Unsupported(
                "v2:cond:player:lost-life-this-turn-other",
            )),
        },
        v2::PlayerConditionKind::PlayedLandThisTurn => match player {
            v2::PlayerRef::You => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::PlayedLandThisTurn,
            ))),
            v2::PlayerRef::AnyOpponent => Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::OpponentStatThisTurn(OpponentTurnStat::PlayedLand),
            ))),
            _ => Err(LoweringError::Unsupported(
                "v2:cond:player:played-land-other",
            )),
        },
        v2::PlayerConditionKind::NoOpponentHasMoreLifeThan => {
            let rp = lower_world_resolving_player(player, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::PlayerHasMostLife {
                    player: rp,
                    allow_tie: true,
                },
            )))
        }
        v2::PlayerConditionKind::ControlsMostCreatures => {
            let rp = lower_world_resolving_player(player, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::PlayerControlsMost {
                    player: rp,
                    zone: ValueZone::Battlefield,
                    filter: Box::new(EngineObjectFilter::creature()),
                    allow_tie: false,
                },
            )))
        }
        v2::PlayerConditionKind::DealtDamageThisTurn { combat_only } => {
            if !combat_only
                && matches!(
                    player,
                    v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent
                )
            {
                Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                    PlayerStatExpr::OpponentDealtDamageThisTurn {
                        noncombat_only: false,
                    },
                )))
            } else {
                Err(LoweringError::Unsupported("v2:cond:player:dealt-damage"))
            }
        }
        v2::PlayerConditionKind::LifeAboveStarting { margin } => {
            if player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported(
                    "v2:cond:player:life-above-starting-other",
                ));
            }
            let threshold = EngineValueExpr::Add(
                Box::new(EngineValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerStartingLife,
                )),
                Box::new(EngineValueExpr::literal(RulesInteger::from(i64::from(
                    margin,
                )))),
            );
            Ok(ResolutionCondition {
                left: EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                comparison: Comparison::GreaterOrEqual,
                right: threshold,
            })
        }
        v2::PlayerConditionKind::AttackedWithCount { min_count } => match player {
            v2::PlayerRef::You => Ok(rc_ge_u16(
                EngineValueExpr::PlayerStat(PlayerStatExpr::AttackingCreaturesThisTurn),
                min_count,
            )),
            other => {
                let players = lower_world_affected_players(other, ctx)?;
                Ok(rc_ge_u16(
                    EngineValueExpr::PlayerStat(PlayerStatExpr::AttackedMatchingThisTurn {
                        players,
                        filter: Box::new(EngineObjectFilter::creature()),
                    }),
                    min_count,
                ))
            }
        },
        v2::PlayerConditionKind::AttackedThisStep => {
            Err(LoweringError::Unsupported("v2:cond:player:attacked-step"))
        }
        v2::PlayerConditionKind::PlayedFromExileThisTurn => Err(LoweringError::Unsupported(
            "v2:cond:player:played-from-exile",
        )),
        v2::PlayerConditionKind::NoOtherCreaturesAttacking => Err(LoweringError::Unsupported(
            "v2:cond:player:no-other-attacking",
        )),
        v2::PlayerConditionKind::AttackedWithTotalPower { .. } => Err(LoweringError::Unsupported(
            "v2:cond:player:attacked-total-power",
        )),
    }
}

fn lower_subject_check_condition(
    subject: &v2::SubjectRef,
    kind: &v2::SubjectConditionKind,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    match kind {
        v2::SubjectConditionKind::EnteredThisTurn => {
            if is_resolved_self_subject(subject, ctx) {
                Ok(rc_ge_one(EngineValueExpr::SourceState(
                    SourceStateExpr::EnteredThisTurn,
                )))
            } else {
                let subj = lower_match_object_ref(subject, ctx)?;
                Ok(rc_ge_one(EngineValueExpr::object_matches(
                    subj,
                    EngineObjectFilter::any().entered_this_turn(),
                )))
            }
        }
        v2::SubjectConditionKind::WasCast => {
            if is_resolved_self_subject(subject, ctx) {
                Ok(rc_ge_one(EngineValueExpr::SourceState(
                    SourceStateExpr::WasCast {
                        by_controller: false,
                        from_hand: false,
                    },
                )))
            } else {
                Err(LoweringError::Unsupported("v2:cond:subject:was-cast"))
            }
        }
        v2::SubjectConditionKind::HasCounters(opt_ck) => {
            let ck = match opt_ck {
                None | Some(v2::CounterKind::Any) => None,
                Some(other) => Some(lower_counter_kind(other)?),
            };
            let subj = lower_counter_subject_ref(subject, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::CounterCount {
                subject: subj,
                kind: ck,
            }))
        }
        v2::SubjectConditionKind::HasChosenName => {
            let subj = lower_match_object_ref(subject, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::object_matches(
                subj,
                EngineObjectFilter::any().with_flags(ObjectFilterFlags::MATCHES_CHOSEN_NAME),
            )))
        }
        v2::SubjectConditionKind::DealtDamageThisTurn { combat_only } => {
            let vr = lower_value_object_ref(subject, ctx)?;
            Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                PlayerStatExpr::DamageDealtThisTurn {
                    subject: vr,
                    to_opponent_only: false,
                    combat_only: *combat_only,
                },
            )))
        }
        v2::SubjectConditionKind::DamageDealtThisTurn { min_damage } => {
            let vr = lower_value_object_ref(subject, ctx)?;
            Ok(rc_ge_u16(
                EngineValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn {
                    subject: vr,
                    to_opponent_only: false,
                    combat_only: false,
                }),
                *min_damage,
            ))
        }
        v2::SubjectConditionKind::AttackedThisCombat { or_blocked } => {
            if is_resolved_self_subject(subject, ctx) && *or_blocked {
                return Ok(rc_ge_one(EngineValueExpr::SourceState(
                    SourceStateExpr::AttackedOrBlockedThisCombat,
                )));
            }
            let subj = lower_match_object_ref(subject, ctx)?;
            let filter = if *or_blocked {
                EngineObjectFilter::any().attacking_or_blocking()
            } else {
                EngineObjectFilter::any().attacking()
            };
            Ok(rc_ge_one(EngineValueExpr::object_matches(subj, filter)))
        }
        v2::SubjectConditionKind::HasntDealtDamageYet { combat_only } => {
            let vr = lower_value_object_ref(subject, ctx)?;
            Ok(rc_eq_zero(EngineValueExpr::PlayerStat(
                PlayerStatExpr::DamageDealtThisTurn {
                    subject: vr,
                    to_opponent_only: false,
                    combat_only: *combat_only,
                },
            )))
        }
        v2::SubjectConditionKind::RemainsExiled => {
            if is_resolved_self_subject(subject, ctx) {
                Ok(rc_ge_one(EngineValueExpr::SourceState(
                    SourceStateExpr::InZone(SourceZoneKind::Exile),
                )))
            } else {
                Err(LoweringError::Unsupported("v2:cond:subject:remains-exiled"))
            }
        }
        v2::SubjectConditionKind::InCommandZoneOrBattlefield => {
            if is_resolved_self_subject(subject, ctx) {
                Ok(rc_or(
                    rc_ge_one(EngineValueExpr::SourceState(SourceStateExpr::InZone(
                        SourceZoneKind::Command,
                    ))),
                    rc_ge_one(EngineValueExpr::SourceState(SourceStateExpr::InZone(
                        SourceZoneKind::Battlefield,
                    ))),
                ))
            } else {
                Err(LoweringError::Unsupported(
                    "v2:cond:subject:command-or-battlefield",
                ))
            }
        }
        v2::SubjectConditionKind::NotCastOrNoManaSpent => Err(LoweringError::Unsupported(
            "v2:cond:subject:not-cast-or-no-mana",
        )),
        v2::SubjectConditionKind::HadCountersOnIt => Err(LoweringError::Unsupported(
            "v2:cond:subject:had-counters-on-it",
        )),
        v2::SubjectConditionKind::AttackedBattleThisTurn => Err(LoweringError::Unsupported(
            "v2:cond:subject:attacked-battle",
        )),
    }
}

fn lower_filter_check_condition(
    filter: &v2::ObjectFilter,
    kind: v2::FilterConditionKind,
    value: u16,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ResolutionCondition> {
    match kind {
        v2::FilterConditionKind::RevealedThisWay => {
            let obj_filter = lower_object_filter(filter, ctx)?;
            Ok(rc_ge_u16(
                EngineValueExpr::LastActionMatchingCount(Box::new(obj_filter)),
                value.max(1),
            ))
        }
        v2::FilterConditionKind::ColorsAmong => {
            let obj_filter = lower_object_filter(filter, ctx)?;
            Ok(rc_ge_u16(
                EngineValueExpr::DistinctColors {
                    filter: Box::new(obj_filter),
                },
                value,
            ))
        }
        v2::FilterConditionKind::OpponentControlsMoreBy => {
            let mut cleaned = filter.clone();
            cleaned.zones.clear();
            let obj_filter = lower_object_filter(&cleaned, ctx)?;
            let opp_max = EngineValueExpr::OpponentMaxCount {
                zone: ValueZone::Battlefield,
                filter: Box::new(obj_filter),
            };
            let you_count = EngineValueExpr::count_battlefield(obj_filter.controlled());
            let rhs = EngineValueExpr::Add(
                Box::new(you_count),
                Box::new(EngineValueExpr::literal(RulesInteger::from(i64::from(
                    value,
                )))),
            );
            Ok(ResolutionCondition {
                left: opp_max,
                comparison: Comparison::GreaterOrEqual,
                right: rhs,
            })
        }
        v2::FilterConditionKind::ManaSpentFrom => Err(LoweringError::Unsupported(
            "v2:cond:filter-check:mana-spent-from",
        )),
        v2::FilterConditionKind::CountersAmong => Err(LoweringError::Unsupported(
            "v2:cond:filter-check:counters-among",
        )),
    }
}

fn lower_event_check_condition(kind: &v2::EventCheckKind) -> LowerResult<ResolutionCondition> {
    match kind {
        v2::EventCheckKind::PriorObjectEvent(act) => match act {
            v2::PriorActionKind::ExiledThisWay
            | v2::PriorActionKind::SacrificedThisWay
            | v2::PriorActionKind::DestroyedThisWay
            | v2::PriorActionKind::DiscardedThisWay
            | v2::PriorActionKind::RevealedThisWay
            | v2::PriorActionKind::ReturnedThisWay
            | v2::PriorActionKind::MilledThisWay
            | v2::PriorActionKind::SearchedThisWay
            | v2::PriorActionKind::CreatedThisWay => Ok(rc_ge_one(EngineValueExpr::EventValue(
                EventValueKind::LastActionSucceeded,
            ))),
            v2::PriorActionKind::DamagePreventedThisWay => Ok(rc_ge_one(
                EngineValueExpr::EventValue(EventValueKind::DamagePreventedThisWay),
            )),
            v2::PriorActionKind::DealtDamageThisWay => Ok(rc_ge_one(EngineValueExpr::EventValue(
                EventValueKind::DealtDamageThisWay { combat_only: false },
            ))),
            _ => Err(LoweringError::Unsupported("v2:cond:event:prior-object")),
        },
        v2::EventCheckKind::ResolutionCountThisTurn(n) => Ok(ResolutionCondition {
            left: EngineValueExpr::SourceState(SourceStateExpr::AbilityResolvedCountThisTurn),
            comparison: Comparison::Equal,
            right: EngineValueExpr::literal(RulesInteger::from(i64::from(*n))),
        }),
        v2::EventCheckKind::DuringStep(step) => {
            if *step == ast::TurnStepAst::MainPhase {
                Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                    PlayerStatExpr::IsInPhase {
                        phase: PhaseCheck::Main,
                        player: None,
                    },
                )))
            } else {
                Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                    PlayerStatExpr::IsInStep {
                        step: lower_step(*step)?,
                        player: None,
                    },
                )))
            }
        }
        v2::EventCheckKind::TiedForExtremeStat { stat, greatest } => {
            if *greatest {
                Ok(rc_ge_one(EngineValueExpr::PlayerStat(
                    PlayerStatExpr::ControlsGreatestStat {
                        player: ResolvingPlayer::Controller,
                        filter: Box::new(EngineObjectFilter::creature()),
                        stat: lower_stat_kind(*stat),
                    },
                )))
            } else {
                Err(LoweringError::Unsupported("v2:cond:event:least-stat"))
            }
        }
        v2::EventCheckKind::ManaCostSpent(mc) => {
            if mc.symbols.is_empty() {
                return Err(LoweringError::Unsupported("v2:cond:event:mana-cost-spent"));
            }
            let mut by_type: BTreeMap<ManaType, u16> = BTreeMap::new();
            let mut snow_count: u16 = 0;
            for sym in &mc.symbols {
                let mt = match sym {
                    v2::ManaSymbol::White => Some(ManaType::White),
                    v2::ManaSymbol::Blue => Some(ManaType::Blue),
                    v2::ManaSymbol::Black => Some(ManaType::Black),
                    v2::ManaSymbol::Red => Some(ManaType::Red),
                    v2::ManaSymbol::Green => Some(ManaType::Green),
                    v2::ManaSymbol::Colorless => Some(ManaType::Colourless),
                    v2::ManaSymbol::Snow => {
                        snow_count = snow_count
                            .checked_add(1)
                            .ok_or(LoweringError::Unsupported("v2:cond:event:mana-cost-spent"))?;
                        None
                    }
                    _ => {
                        return Err(LoweringError::Unsupported("v2:cond:event:mana-cost-spent"));
                    }
                };
                if let Some(mana_type) = mt {
                    let entry = by_type.entry(mana_type).or_insert(0);
                    *entry = entry
                        .checked_add(1)
                        .ok_or(LoweringError::Unsupported("v2:cond:event:mana-cost-spent"))?;
                }
            }
            let mut checks = Vec::with_capacity(by_type.len() + usize::from(snow_count > 0));
            for (mana_type, needed) in by_type {
                checks.push(rc_ge_u16(
                    EngineValueExpr::SourceState(SourceStateExpr::ManaTypeSpentToCast(mana_type)),
                    needed,
                ));
            }
            if snow_count > 0 {
                checks.push(rc_ge_u16(
                    EngineValueExpr::SourceState(SourceStateExpr::SnowManaSpentToCast),
                    snow_count,
                ));
            }
            checks
                .into_iter()
                .reduce(rc_and)
                .ok_or(LoweringError::Unsupported("v2:cond:event:mana-cost-spent"))
        }
        v2::EventCheckKind::KickedWith(_) => {
            Err(LoweringError::Unsupported("v2:cond:event:kicked-with"))
        }
        v2::EventCheckKind::KeywordCostPaid(kw) => match kw {
            v2::KeywordKind::Kicker | v2::KeywordKind::Multikicker => Ok(rc_ge_one(
                EngineValueExpr::EventValue(EventValueKind::KickerCount(0)),
            )),
            v2::KeywordKind::Bargain => Ok(rc_ge_one(EngineValueExpr::EventValue(
                EventValueKind::Bargained,
            ))),
            v2::KeywordKind::Cleave => Ok(rc_ge_one(EngineValueExpr::EventValue(
                EventValueKind::Cleaved,
            ))),
            v2::KeywordKind::Gift => Ok(rc_ge_one(EngineValueExpr::EventValue(
                EventValueKind::GiftPromised,
            ))),
            v2::KeywordKind::Evoke
            | v2::KeywordKind::Dash
            | v2::KeywordKind::Blitz
            | v2::KeywordKind::Warp
            | v2::KeywordKind::Escape
            | v2::KeywordKind::Bestow
            | v2::KeywordKind::Surge
            | v2::KeywordKind::Spectacle
            | v2::KeywordKind::Prowl
            | v2::KeywordKind::Freerunning
            | v2::KeywordKind::Sneak
            | v2::KeywordKind::Mayhem
            | v2::KeywordKind::Mutate
            | v2::KeywordKind::WebSlinging
            | v2::KeywordKind::Harmonize
            | v2::KeywordKind::Emerge
            | v2::KeywordKind::Awaken
            | v2::KeywordKind::Impending => Ok(rc_ge_one(EngineValueExpr::EventValue(
                EventValueKind::PaidAlternative,
            ))),
            v2::KeywordKind::Foretell | v2::KeywordKind::Plot => Ok(rc_ge_one(
                EngineValueExpr::EventValue(EventValueKind::CastFromExile),
            )),
            v2::KeywordKind::Flashback => Ok(rc_ge_one(EngineValueExpr::EventValue(
                EventValueKind::CastFromGraveyard,
            ))),
            v2::KeywordKind::Buyback
            | v2::KeywordKind::Entwine
            | v2::KeywordKind::Spree
            | v2::KeywordKind::Escalate
            | v2::KeywordKind::Offspring
            | v2::KeywordKind::Squad
            | v2::KeywordKind::Casualty => Ok(rc_ge_one(EngineValueExpr::EventValue(
                EventValueKind::AdditionalCostPaid,
            ))),
            _ => Err(LoweringError::Unsupported(
                "v2:cond:event:keyword-cost-paid",
            )),
        },
        v2::EventCheckKind::DidntPutInZone(_) => Err(LoweringError::Unsupported(
            "v2:cond:event:didnt-put-in-zone",
        )),
        v2::EventCheckKind::MostCommonColor(_) => Err(LoweringError::Unsupported(
            "v2:cond:event:most-common-color",
        )),
        v2::EventCheckKind::PlayersLostGame(_) => Err(LoweringError::Unsupported(
            "v2:cond:event:players-lost-game",
        )),
        v2::EventCheckKind::SpellsCastLastTurn { .. } => Err(LoweringError::Unsupported(
            "v2:cond:event:spells-cast-last-turn",
        )),
    }
}

/// Lower a [`v2::ConditionExpr`] into a static [`StaticSourceCondition`].
///
/// Uses direct native [`StaticSourceCondition`] variants whenever available, and falls back
/// to [`StaticSourceCondition::Eval`] only after verifying `is_world_evaluable()` so engine
/// admission validation never fails unexpectedly.
pub(crate) fn lower_static_source_condition(
    cond: &v2::ConditionExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<StaticSourceCondition> {
    let prev_has_target = ctx.has_object_target;
    ctx.has_object_target = false;
    let result = lower_static_source_condition_inner(cond, ctx);
    ctx.has_object_target = prev_has_target;
    result
}

fn lower_static_source_condition_inner(
    cond: &v2::ConditionExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<StaticSourceCondition> {
    match cond {
        v2::ConditionExpr::Not(inner) => Ok(StaticSourceCondition::Not(Box::new(
            lower_static_source_condition_inner(inner, ctx)?,
        ))),
        v2::ConditionExpr::And(a, b) => Ok(StaticSourceCondition::And(
            Box::new(lower_static_source_condition_inner(a, ctx)?),
            Box::new(lower_static_source_condition_inner(b, ctx)?),
        )),
        v2::ConditionExpr::Or(a, b) => Ok(StaticSourceCondition::Or(
            Box::new(lower_static_source_condition_inner(a, ctx)?),
            Box::new(lower_static_source_condition_inner(b, ctx)?),
        )),
        v2::ConditionExpr::Flag(v2::ConditionFlag::IsYourTurn) => {
            Ok(StaticSourceCondition::ControllersTurn)
        }
        v2::ConditionExpr::Flag(v2::ConditionFlag::HasCitysBlessing) => {
            Ok(StaticSourceCondition::ControllerHasCityBlessing)
        }
        v2::ConditionExpr::SubjectState { subject, state } if is_static_self_subject(subject) => {
            match state {
                v2::ObjectStateKind::Tapped => Ok(StaticSourceCondition::SourceIsTapped),
                v2::ObjectStateKind::Untapped => Ok(StaticSourceCondition::SourceIsUntapped),
                v2::ObjectStateKind::Equipped => Ok(StaticSourceCondition::SourceIsEquipped),
                v2::ObjectStateKind::Enchanted => Ok(StaticSourceCondition::SourceIsEnchanted),
                v2::ObjectStateKind::Paired => Ok(StaticSourceCondition::SourcePaired),
                _ => lower_static_via_world_eval(cond, ctx),
            }
        }
        v2::ConditionExpr::SubjectCheck {
            subject,
            kind: v2::SubjectConditionKind::EnteredThisTurn,
        } if is_static_self_subject(subject) => Ok(StaticSourceCondition::SourceEnteredThisTurn),
        v2::ConditionExpr::SubjectCheck {
            subject,
            kind: v2::SubjectConditionKind::HasCounters(opt_ck),
        } if is_static_self_subject(subject) => match opt_ck {
            None | Some(v2::CounterKind::Any) => {
                Ok(StaticSourceCondition::SourceAnyCounterAtLeast(1))
            }
            Some(other) => Ok(StaticSourceCondition::SourceCounterAtLeast {
                kind: lower_counter_kind(other)?,
                minimum: 1,
            }),
        },
        v2::ConditionExpr::HasKeyword { subject, keyword } if is_static_self_subject(subject) => {
            if let Ok(ab) = lower_keyword_kind_plain(*keyword) {
                Ok(StaticSourceCondition::HasKeyword(ab))
            } else {
                lower_static_via_world_eval(cond, ctx)
            }
        }
        v2::ConditionExpr::SubjectAttachedTo { subject, target }
            if is_static_self_subject(subject) =>
        {
            let subj = v2::SubjectRef::All(target.clone());
            if let Ok(sel) = lower_permanent_selector(&subj, ctx) {
                return Ok(StaticSourceCondition::AttachedHostMatches(Box::new(sel)));
            }
            lower_static_via_world_eval(cond, ctx)
        }
        v2::ConditionExpr::Controls {
            player: v2::PlayerRef::You,
            filter,
            min_count,
        } if *min_count >= 1 => {
            let subj = v2::SubjectRef::All(filter.clone());
            if let Ok(sel) = lower_permanent_selector(&subj, ctx) {
                return Ok(StaticSourceCondition::ControlsMatching {
                    selector: Box::new(sel),
                    minimum: *min_count,
                });
            }
            lower_static_via_world_eval(cond, ctx)
        }
        v2::ConditionExpr::Controls {
            player: v2::PlayerRef::AnyOpponent,
            filter,
            min_count,
        } if *min_count >= 1 => {
            let subj = v2::SubjectRef::All(filter.clone());
            if let Ok(sel) = lower_permanent_selector(&subj, ctx) {
                return Ok(StaticSourceCondition::OpponentControlsMatching {
                    selector: Box::new(sel),
                    minimum: *min_count,
                });
            }
            lower_static_via_world_eval(cond, ctx)
        }
        v2::ConditionExpr::ZoneMetric {
            zone:
                v2::ZoneRef {
                    zone: v2::ZoneKind::Graveyard,
                    owner: v2::ZoneOwner::Default | v2::ZoneOwner::You,
                },
            kind: v2::ZoneMetricKind::DistinctCardTypes,
            min_count: 4,
        } => Ok(StaticSourceCondition::Delirium),
        v2::ConditionExpr::EventCheck(v2::EventCheckKind::MostCommonColor(mask)) => {
            let set = lower_color_set(*mask);
            if set.len() == 1
                && let Some(color) = set.into_iter().next()
            {
                return Ok(StaticSourceCondition::MostCommonPermanentColor {
                    color,
                    or_tied: true,
                });
            }
            Err(LoweringError::Unsupported(
                "v2:static-cond:most-common-color",
            ))
        }
        _ => lower_static_via_world_eval(cond, ctx),
    }
}

fn lower_static_via_world_eval(
    cond: &v2::ConditionExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<StaticSourceCondition> {
    let rc = lower_resolution_condition(cond, ctx)?;
    if !rc.is_world_evaluable() {
        return Err(LoweringError::Unsupported(
            "v2:static-cond:not-world-evaluable",
        ));
    }
    Ok(StaticSourceCondition::Eval(Box::new(rc)))
}
