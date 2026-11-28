//! Numeric expression lowerers from `v2::ir::ValueExpr` and `v2::ir::PtDelta` into
//! `spellwake_engine::program::ValueExpr`.

use std::num::NonZeroU16;

use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    program::{
        BodyTarget, Comparison, CounterSubjectRef, EventValueKind,
        ObjectFilter as EngineObjectFilter, ObjectStatKind, PlayerStatExpr, ResolvingPlayer,
        SourceStateExpr, StatAggregate, ValueExpr as EngineValueExpr, ValueObjectRef, ValueZone,
    },
    toolbox::ObjectQuery,
};
use spellwake_oracle_compiler::{ast, v2::ir as v2};

use crate::v2_lowering::{
    atoms::{
        lower_color_mask, lower_counter_kind, lower_stat_kind, lower_value_zone,
        lower_world_affected_players, lower_world_query_zone, lower_world_resolving_player,
    },
    ctx::LowerCtx,
    error::{LowerResult, LoweringError},
    filter::{lower_object_filter, scan_filter_player_target, split_among_prior_cards},
};

fn scan_player_ref_target(player: v2::PlayerRef) -> Option<BodyTarget> {
    match player {
        v2::PlayerRef::TargetOpponent => Some(BodyTarget::SINGLE_OPPONENT),
        v2::PlayerRef::TargetPlayer => Some(BodyTarget::SINGLE_PLAYER),
        _ => None,
    }
}

fn scan_zone_owner_target(owner: v2::ZoneOwner) -> Option<BodyTarget> {
    match owner {
        v2::ZoneOwner::TargetOpponent => Some(BodyTarget::SINGLE_OPPONENT),
        v2::ZoneOwner::TargetPlayer => Some(BodyTarget::SINGLE_PLAYER),
        _ => None,
    }
}

fn scan_subject_player_target(subject: &v2::SubjectRef) -> Option<BodyTarget> {
    match subject {
        v2::SubjectRef::Player(p) | v2::SubjectRef::HandOf(p) => scan_player_ref_target(*p),
        v2::SubjectRef::TargetPlayer { opponent_only, .. } => Some(if *opponent_only {
            BodyTarget::SINGLE_OPPONENT
        } else {
            BodyTarget::SINGLE_PLAYER
        }),
        v2::SubjectRef::Each(flt) | v2::SubjectRef::All(flt) | v2::SubjectRef::Group(flt) => {
            scan_filter_player_target(flt)
        }
        v2::SubjectRef::Counted { filter, count } => scan_filter_player_target(filter)
            .or_else(|| scan_value_expr_player_target(&count.count)),
        v2::SubjectRef::Compound(parts) => parts.iter().find_map(scan_subject_player_target),
        _ => None,
    }
}

/// Scan a [`v2::ValueExpr`] for an embedded `TargetOpponent` or `TargetPlayer` reference
/// that must be registered on the enclosing [`crate::v2_lowering::ctx::BodyAcc`].
#[must_use]
pub(crate) fn scan_value_expr_player_target(expr: &v2::ValueExpr) -> Option<BodyTarget> {
    match expr {
        v2::ValueExpr::Count(flt)
        | v2::ValueExpr::AggregateStat { filter: flt, .. }
        | v2::ValueExpr::ManaSpentFromFilter(flt)
        | v2::ValueExpr::DistinctCounterKinds(flt)
        | v2::ValueExpr::DistinctCardTypesAmong(flt) => scan_filter_player_target(flt),
        v2::ValueExpr::PlayerStat { player, .. }
        | v2::ValueExpr::PlayerCounterCount { player, .. }
        | v2::ValueExpr::DomainCount(player)
        | v2::ValueExpr::PartySize(player)
        | v2::ValueExpr::Speed(player)
        | v2::ValueExpr::CommanderCastCount(player)
        | v2::ValueExpr::DamageDealtToPlayerThisTurn(player) => scan_player_ref_target(*player),
        v2::ValueExpr::EventCountThisTurn {
            player: Some(p), ..
        } => scan_player_ref_target(*p),
        v2::ValueExpr::CounterCount { subject, .. }
        | v2::ValueExpr::ObjectStat { subject, .. }
        | v2::ValueExpr::ColorCount(subject)
        | v2::ValueExpr::StatDifference(subject) => scan_subject_player_target(subject),
        v2::ValueExpr::DistinctCardTypes(zone) | v2::ValueExpr::DistinctManaValues(zone) => {
            scan_zone_owner_target(zone.owner)
        }
        v2::ValueExpr::Add(a, b) | v2::ValueExpr::Sub(a, b) | v2::ValueExpr::Mul(a, b) => {
            scan_value_expr_player_target(a).or_else(|| scan_value_expr_player_target(b))
        }
        v2::ValueExpr::Div { expr: inner, .. } => scan_value_expr_player_target(inner),
        _ => None,
    }
}

/// Constant-fold a [`v2::ValueExpr`] when it depends solely on integer literals.
#[must_use]
pub(crate) fn lower_value_const(expr: &v2::ValueExpr) -> Option<i32> {
    match expr {
        v2::ValueExpr::Const(n) => Some(*n),
        v2::ValueExpr::Add(a, b) => lower_value_const(a)?.checked_add(lower_value_const(b)?),
        v2::ValueExpr::Sub(a, b) => lower_value_const(a)?.checked_sub(lower_value_const(b)?),
        v2::ValueExpr::Mul(a, b) => lower_value_const(a)?.checked_mul(lower_value_const(b)?),
        v2::ValueExpr::Div {
            expr: inner,
            denom,
            rounding,
        } => {
            let num = lower_value_const(inner)?;
            if *denom <= 0 {
                return None;
            }
            match rounding {
                ast::Rounding::Down => Some(num.div_euclid(*denom)),
                ast::Rounding::Up => {
                    let adjusted = num.checked_add(denom.checked_sub(1)?)?;
                    Some(adjusted.div_euclid(*denom))
                }
            }
        }
        _ => None,
    }
}

/// Lower a [`v2::SubjectRef`] into an engine [`ValueObjectRef`] for reading an object statistic.
pub(crate) fn lower_value_object_ref(
    subject: &v2::SubjectRef,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ValueObjectRef> {
    match subject {
        v2::SubjectRef::SelfObject => Ok(ValueObjectRef::AbilitySource),
        v2::SubjectRef::TargetObject { .. } if ctx.has_object_target => {
            Ok(ValueObjectRef::TargetObject)
        }
        v2::SubjectRef::Anaphora(anaph) => match anaph {
            v2::AnaphoraRef::EquippedCreature
            | v2::AnaphoraRef::EnchantedCreature
            | v2::AnaphoraRef::EnchantedPermanent => Ok(ValueObjectRef::AttachedHost),
            v2::AnaphoraRef::SacrificedObject => {
                if ctx.last_moved_binding.is_some() {
                    Ok(ValueObjectRef::LastSacrificedCreature)
                } else {
                    Ok(ValueObjectRef::SacrificedCreature)
                }
            }
            v2::AnaphoraRef::ExiledObject | v2::AnaphoraRef::TheExiledCard => {
                Ok(ValueObjectRef::ExiledCostCreature)
            }
            v2::AnaphoraRef::RevealedCard => Ok(ValueObjectRef::RevealedTop),
            v2::AnaphoraRef::DestroyedObject => Ok(ValueObjectRef::DestructionTarget),
            v2::AnaphoraRef::ThatSpell => Ok(ValueObjectRef::TriggeringSpell),
            v2::AnaphoraRef::SameTarget => Ok(ValueObjectRef::TargetObject),
            v2::AnaphoraRef::ThatSource => Ok(ValueObjectRef::AbilitySource),
            v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatCard
            | v2::AnaphoraRef::ThatToken
            | v2::AnaphoraRef::ThatLand
            | v2::AnaphoraRef::ThatObject => {
                if ctx.has_object_target {
                    Ok(ValueObjectRef::TargetObject)
                } else if ctx.in_trigger {
                    Ok(ValueObjectRef::TriggerSubject)
                } else {
                    Ok(ValueObjectRef::AbilitySource)
                }
            }
            _ => Err(LoweringError::Unsupported("v2:value:object-stat-anaphora")),
        },
        _ => Err(LoweringError::Unsupported("v2:value:object-stat-subject")),
    }
}

/// Lower a [`v2::ValueExpr`] into an engine [`EngineValueExpr`].
pub(crate) fn lower_value_expr(
    expr: &v2::ValueExpr,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    match expr {
        v2::ValueExpr::Const(n) => Ok(EngineValueExpr::literal(RulesInteger::from(i64::from(*n)))),
        v2::ValueExpr::Var(v2::VarName::X) => {
            if let Some(slot) = ctx.bound_x_slot {
                Ok(EngineValueExpr::BoundInt(slot))
            } else {
                Ok(EngineValueExpr::EventValue(EventValueKind::ChosenX))
            }
        }
        v2::ValueExpr::Var(v2::VarName::Y) => Err(LoweringError::Unsupported("v2:value:var-y")),
        v2::ValueExpr::PriorActionCount(kind) => lower_prior_action_count(*kind, ctx),
        v2::ValueExpr::Count(filter) => lower_count_expr(filter, ctx),
        v2::ValueExpr::CounterCount { subject, kind } => {
            lower_counter_count_expr(subject, kind, ctx)
        }
        v2::ValueExpr::PlayerCounterCount { player, kind } => {
            lower_player_counter_count_expr(*player, kind, ctx)
        }
        v2::ValueExpr::DistinctCardTypes(zone) => match (zone.zone, zone.owner) {
            (v2::ZoneKind::Graveyard, v2::ZoneOwner::Default | v2::ZoneOwner::You) => Ok(
                EngineValueExpr::PlayerStat(PlayerStatExpr::OwnGraveyardDistinctCardTypes),
            ),
            (
                v2::ZoneKind::Graveyard,
                v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::EachPlayer,
            ) => Ok(EngineValueExpr::PlayerStat(
                PlayerStatExpr::AllGraveyardsDistinctCardTypes,
            )),
            _ => Err(LoweringError::Unsupported(
                "v2:value:distinct-card-types-zone",
            )),
        },
        v2::ValueExpr::DomainCount(player) => {
            if *player == v2::PlayerRef::You {
                Ok(EngineValueExpr::DomainCount)
            } else {
                Err(LoweringError::Unsupported("v2:value:domain-other-player"))
            }
        }
        v2::ValueExpr::Devotion(colors) => Ok(EngineValueExpr::Devotion(lower_color_mask(*colors))),
        v2::ValueExpr::PartySize(player) => {
            if *player != v2::PlayerRef::You {
                return Err(LoweringError::Unsupported("v2:value:party-other-player"));
            }
            let roles = lookup_party_roles(ctx)?;
            Ok(EngineValueExpr::PartyCount(Box::new(roles)))
        }
        v2::ValueExpr::TimesKicked => {
            Ok(EngineValueExpr::EventValue(EventValueKind::KickerCount(0)))
        }
        v2::ValueExpr::ColorCount(subject) => match subject.as_ref() {
            v2::SubjectRef::Each(filter)
            | v2::SubjectRef::All(filter)
            | v2::SubjectRef::Group(filter) => {
                if filter
                    .zones
                    .iter()
                    .any(|z| z.zone != v2::ZoneKind::Battlefield)
                {
                    return Err(LoweringError::Unsupported(
                        "v2:value:color-count-non-battlefield",
                    ));
                }
                let lowered = lower_object_filter(filter, ctx)?;
                Ok(EngineValueExpr::DistinctColors {
                    filter: Box::new(lowered),
                })
            }
            _ => Err(LoweringError::Unsupported("v2:value:color-count-subject")),
        },
        v2::ValueExpr::ObjectStat { subject, stat } => {
            let stat_kind = lower_stat_kind(*stat);
            if let Some(binding) = ctx.in_loop_object
                && matches!(
                    subject.as_ref(),
                    v2::SubjectRef::Anaphora(
                        v2::AnaphoraRef::It
                            | v2::AnaphoraRef::ThatCreature
                            | v2::AnaphoraRef::ThatPermanent
                            | v2::AnaphoraRef::ThatCard
                            | v2::AnaphoraRef::ThatObject
                    )
                )
            {
                return Ok(EngineValueExpr::BoundStat {
                    binding,
                    stat: stat_kind,
                    op: StatAggregate::Sum,
                });
            }
            if matches!(
                subject.as_ref(),
                v2::SubjectRef::Anaphora(v2::AnaphoraRef::SacrificedObject)
            ) {
                if let Some(binding) = ctx.last_moved_binding {
                    return Ok(EngineValueExpr::BoundStat {
                        binding,
                        stat: stat_kind,
                        op: StatAggregate::Sum,
                    });
                }
                let cost_subject = match stat_kind {
                    ObjectStatKind::ManaValue => ValueObjectRef::SacrificedPermanent,
                    ObjectStatKind::Power | ObjectStatKind::Toughness => {
                        ValueObjectRef::SacrificedCreature
                    }
                    ObjectStatKind::Loyalty => {
                        return Err(LoweringError::Unsupported("v2:value:sacrificed-loyalty"));
                    }
                };
                return Ok(EngineValueExpr::ObjectStat {
                    subject: cost_subject,
                    stat: stat_kind,
                });
            }
            if let Some(binding) = ctx.last_moved_binding
                && matches!(
                    subject.as_ref(),
                    v2::SubjectRef::Anaphora(
                        v2::AnaphoraRef::ExiledObject
                            | v2::AnaphoraRef::TheExiledCard
                            | v2::AnaphoraRef::DiscardedObject
                    )
                )
            {
                return Ok(EngineValueExpr::BoundStat {
                    binding,
                    stat: stat_kind,
                    op: StatAggregate::Sum,
                });
            }
            if !ctx.has_object_target
                && !ctx.in_trigger
                && let Some(binding) = ctx.last_moved_binding
                && matches!(
                    subject.as_ref(),
                    v2::SubjectRef::Anaphora(
                        v2::AnaphoraRef::It
                            | v2::AnaphoraRef::ThatCreature
                            | v2::AnaphoraRef::ThatPermanent
                            | v2::AnaphoraRef::ThatCard
                            | v2::AnaphoraRef::ThatToken
                            | v2::AnaphoraRef::ThatLand
                            | v2::AnaphoraRef::ThatObject
                    )
                )
            {
                return Ok(EngineValueExpr::BoundStat {
                    binding,
                    stat: stat_kind,
                    op: StatAggregate::Sum,
                });
            }
            let subj = lower_value_object_ref(subject, ctx)?;
            Ok(EngineValueExpr::ObjectStat {
                subject: subj,
                stat: stat_kind,
            })
        }
        v2::ValueExpr::AggregateStat { op, stat, filter } => {
            lower_aggregate_stat_expr(*op, *stat, filter, ctx)
        }
        v2::ValueExpr::PlayerStat { player, stat } => lower_player_stat_expr(*player, *stat, ctx),
        v2::ValueExpr::OpponentsCount => Ok(EngineValueExpr::OpponentsCount),
        v2::ValueExpr::EventCountThisTurn {
            event,
            player,
            exclude_first,
        } => lower_event_count_this_turn(*event, *player, *exclude_first, ctx),
        v2::ValueExpr::ManaSpentFromFilter(filter) => lower_mana_spent_from_filter(filter),
        v2::ValueExpr::Speed(player) => {
            if *player == v2::PlayerRef::You {
                Ok(EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerSpeed))
            } else {
                Err(LoweringError::Unsupported("v2:value:speed-other-player"))
            }
        }
        v2::ValueExpr::SnowManaSpent => Ok(EngineValueExpr::EventValue(
            EventValueKind::SnowManaSpentToCast,
        )),
        v2::ValueExpr::DistinctManaValues(zone) => {
            if zone.zone != v2::ZoneKind::Graveyard {
                return Err(LoweringError::Unsupported(
                    "v2:value:distinct-mv-non-graveyard",
                ));
            }
            let player = match zone.owner {
                v2::ZoneOwner::Default | v2::ZoneOwner::You => ResolvingPlayer::Controller,
                v2::ZoneOwner::TargetPlayer | v2::ZoneOwner::TargetOpponent => {
                    ResolvingPlayer::TargetPlayer
                }
                v2::ZoneOwner::DefendingPlayer => ResolvingPlayer::DefendingPlayer,
                v2::ZoneOwner::ThatPlayer => {
                    lower_world_resolving_player(v2::PlayerRef::ThatPlayer, ctx)?
                }
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:value:distinct-mv-graveyard-owner",
                    ));
                }
            };
            Ok(EngineValueExpr::DistinctManaValuesInGraveyard(player))
        }
        v2::ValueExpr::ManaPaidThisWay => {
            Ok(EngineValueExpr::EventValue(EventValueKind::ManaSpentToCast))
        }
        v2::ValueExpr::ColorsOfManaSpent => Ok(EngineValueExpr::EventValue(
            EventValueKind::ColorsSpentToCast,
        )),
        v2::ValueExpr::TargetsBeyondFirst => {
            Err(LoweringError::Unsupported("v2:value:targets-beyond-first"))
        }
        v2::ValueExpr::CommanderCastCount(_) => {
            Err(LoweringError::Unsupported("v2:value:commander-cast-count"))
        }
        v2::ValueExpr::DamageDealtToOpponentsThisTurn { .. } => Err(LoweringError::Unsupported(
            "v2:value:damage-dealt-to-opponents",
        )),
        v2::ValueExpr::MutatedCount => Err(LoweringError::Unsupported("v2:value:mutated-count")),
        v2::ValueExpr::DistinctCounterKinds(_) => Err(LoweringError::Unsupported(
            "v2:value:distinct-counter-kinds",
        )),
        v2::ValueExpr::DistinctCardTypesAmong(filter) => lower_distinct_card_types_among(filter),
        v2::ValueExpr::PlayersWhoDidThisTurn { .. } => Err(LoweringError::Unsupported(
            "v2:value:players-who-did-this-turn",
        )),
        v2::ValueExpr::DamageDealtToPlayerThisTurn(_) => Err(LoweringError::Unsupported(
            "v2:value:damage-dealt-to-player",
        )),
        v2::ValueExpr::StatDifference(_) => {
            Err(LoweringError::Unsupported("v2:value:stat-difference"))
        }
        v2::ValueExpr::HighestLifeTotal => {
            let you = EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerLife);
            let opp = EngineValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife);
            Ok(EngineValueExpr::conditional(
                you.clone(),
                Comparison::GreaterOrEqual,
                opp.clone(),
                you,
                opp,
            ))
        }
        v2::ValueExpr::ConvokedCount => Err(LoweringError::Unsupported("v2:value:convoked-count")),
        v2::ValueExpr::VisitedAttractionsThisTurn => {
            Err(LoweringError::Unsupported("v2:value:visited-attractions"))
        }
        v2::ValueExpr::Add(a, b) => Ok(EngineValueExpr::Add(
            Box::new(lower_value_expr(a, ctx)?),
            Box::new(lower_value_expr(b, ctx)?),
        )),
        v2::ValueExpr::Sub(a, b) => Ok(EngineValueExpr::Sub(
            Box::new(lower_value_expr(a, ctx)?),
            Box::new(lower_value_expr(b, ctx)?),
        )),
        v2::ValueExpr::Mul(a, b) => {
            if let Some(k) = lower_value_const(b) {
                Ok(EngineValueExpr::Scale {
                    expr: Box::new(lower_value_expr(a, ctx)?),
                    factor: RulesInteger::from(i64::from(k)),
                })
            } else if let Some(k) = lower_value_const(a) {
                Ok(EngineValueExpr::Scale {
                    expr: Box::new(lower_value_expr(b, ctx)?),
                    factor: RulesInteger::from(i64::from(k)),
                })
            } else {
                Err(LoweringError::Unsupported("v2:value:non-const-mul"))
            }
        }
        v2::ValueExpr::Div {
            expr: inner,
            denom,
            rounding,
        } => {
            let d = u16::try_from(*denom)
                .ok()
                .and_then(NonZeroU16::new)
                .ok_or(LoweringError::Unsupported("v2:value:invalid-divisor"))?;
            Ok(EngineValueExpr::DivConst {
                expr: Box::new(lower_value_expr(inner, ctx)?),
                divisor: d,
                round_up: matches!(rounding, ast::Rounding::Up),
            })
        }
    }
}

/// Resolve the four canonical party creature types (`Cleric`, `Rogue`, `Warrior`, `Wizard`).
pub(crate) fn lookup_party_roles(
    ctx: &LowerCtx<'_>,
) -> LowerResult<[spellwake_contracts::creature_types::CreatureTypeId; 4]> {
    let cleric = ctx
        .lookup_creature_type("Cleric")
        .ok_or(LoweringError::Unsupported("v2:value:party-vocab-missing"))?;
    let rogue = ctx
        .lookup_creature_type("Rogue")
        .ok_or(LoweringError::Unsupported("v2:value:party-vocab-missing"))?;
    let warrior = ctx
        .lookup_creature_type("Warrior")
        .ok_or(LoweringError::Unsupported("v2:value:party-vocab-missing"))?;
    let wizard = ctx
        .lookup_creature_type("Wizard")
        .ok_or(LoweringError::Unsupported("v2:value:party-vocab-missing"))?;
    Ok([cleric, rogue, warrior, wizard])
}

fn lower_prior_action_count(
    kind: v2::PriorActionKind,
    ctx: &LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    match kind {
        v2::PriorActionKind::ThatMany => {
            if ctx.last_moved_binding.is_none()
                && let Some(ev) = ctx.trigger_damage_event
            {
                Ok(EngineValueExpr::EventValue(ev))
            } else {
                Ok(EngineValueExpr::EventValue(EventValueKind::LastActionCount))
            }
        }
        v2::PriorActionKind::ExiledThisWay
        | v2::PriorActionKind::SacrificedThisWay
        | v2::PriorActionKind::DiscardedThisWay
        | v2::PriorActionKind::RevealedThisWay
        | v2::PriorActionKind::ReturnedThisWay
        | v2::PriorActionKind::MilledThisWay
        | v2::PriorActionKind::SearchedThisWay
        | v2::PriorActionKind::CreatedThisWay => {
            Ok(EngineValueExpr::EventValue(EventValueKind::LastActionCount))
        }
        v2::PriorActionKind::DestroyedThisWay => Ok(EngineValueExpr::EventValue(
            EventValueKind::DestroyedPermanents,
        )),
        v2::PriorActionKind::TappedThisWay => Ok(EngineValueExpr::EventValue(
            EventValueKind::PaidCreatureTaps,
        )),
        v2::PriorActionKind::DamagePreventedThisWay => Ok(EngineValueExpr::EventValue(
            EventValueKind::DamagePreventedThisWay,
        )),
        v2::PriorActionKind::DealtDamageThisWay => {
            if let Some(ev) = ctx.trigger_damage_event {
                Ok(EngineValueExpr::EventValue(ev))
            } else if ctx.in_trigger {
                Ok(EngineValueExpr::EventValue(
                    EventValueKind::CombatDamageToPlayer,
                ))
            } else {
                Err(LoweringError::Unsupported(
                    "v2:value:prior-dealt-damage-amount",
                ))
            }
        }
        v2::PriorActionKind::EnergyPaidThisWay => {
            Err(LoweringError::Unsupported("v2:value:prior-energy-paid"))
        }
        v2::PriorActionKind::CastThisWay => {
            Err(LoweringError::Unsupported("v2:value:prior-cast-count"))
        }
        v2::PriorActionKind::ExcessDamageThisWay => {
            Err(LoweringError::Unsupported("v2:value:prior-excess-damage"))
        }
        v2::PriorActionKind::LookedAtScryingThisWay => {
            Err(LoweringError::Unsupported("v2:value:prior-scry-count"))
        }
        v2::PriorActionKind::CountersRemovedThisWay => Err(LoweringError::Unsupported(
            "v2:value:prior-counters-removed",
        )),
        v2::PriorActionKind::LifePaidAsEntered => {
            Err(LoweringError::Unsupported("v2:value:life-paid-as-entered"))
        }
    }
}

fn lower_count_expr(
    filter: &v2::ObjectFilter,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    let (stripped_prior, had_among_prior) = split_among_prior_cards(filter);
    if had_among_prior && stripped_prior.zones.is_empty() {
        let Some(binding) = ctx.last_moved_binding else {
            return Err(LoweringError::Unsupported(
                "v2:filter:pred:among-prior-cards",
            ));
        };
        let obj = lower_object_filter(&stripped_prior, ctx)?;
        return Ok(EngineValueExpr::BoundCount {
            binding,
            filter: Some(Box::new(obj)),
        });
    }

    // Check if the count specifically filters objects affected by the immediately preceding action.
    let has_prior_action = filter
        .predicates
        .iter()
        .any(|p| matches!(p, v2::FilterPredicate::AffectedByPriorAction(_)));
    if has_prior_action && filter.zones.is_empty() {
        let mut stripped = filter.clone();
        stripped
            .predicates
            .retain(|p| !matches!(p, v2::FilterPredicate::AffectedByPriorAction(_)));
        let obj = lower_object_filter(&stripped, ctx)?;
        return Ok(EngineValueExpr::LastActionMatchingCount(Box::new(obj)));
    }

    if filter.zones.is_empty() {
        if matches!(
            filter.controller,
            Some(ast::ControllerRelation::TargetOpponent | ast::ControllerRelation::TargetPlayer)
        ) {
            let mut cleaned = filter.clone();
            cleaned.controller = None;
            let obj = lower_object_filter(&cleaned, ctx)?;
            return Ok(EngineValueExpr::Count {
                zone: ValueZone::Battlefield,
                filter: Box::new(obj),
                controller_scope: Some(ResolvingPlayer::TargetPlayer),
            });
        }
        let obj = lower_object_filter(filter, ctx)?;
        return Ok(EngineValueExpr::Count {
            zone: ValueZone::Battlefield,
            filter: Box::new(obj),
            controller_scope: None,
        });
    }

    // Build one `Count` per listed zone and sum multi-zone counts cleanly via `Add`.
    let mut sum_expr: Option<EngineValueExpr> = None;
    for &zone in &filter.zones {
        let mut cleaned = filter.clone();
        cleaned.zones.clear();

        let (vz, controller_scope) = if zone.zone == v2::ZoneKind::Exile
            && cleaned.flags.contains(v2::FilterFlags::EXILED_WITH_SELF)
        {
            cleaned.flags.0 &= !v2::FilterFlags::EXILED_WITH_SELF.0;
            (ValueZone::LinkedExile, None)
        } else if zone.zone == v2::ZoneKind::Battlefield {
            let scope = match zone.owner {
                v2::ZoneOwner::Default
                | v2::ZoneOwner::Any
                | v2::ZoneOwner::All
                | v2::ZoneOwner::EachPlayer => None,
                v2::ZoneOwner::You => Some(ResolvingPlayer::Controller),
                v2::ZoneOwner::TargetPlayer | v2::ZoneOwner::TargetOpponent => {
                    Some(ResolvingPlayer::TargetPlayer)
                }
                v2::ZoneOwner::DefendingPlayer => Some(ResolvingPlayer::DefendingPlayer),
                v2::ZoneOwner::ThatPlayer => Some(lower_world_resolving_player(
                    v2::PlayerRef::ThatPlayer,
                    ctx,
                )?),
                v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents => {
                    if let Some(existing) = cleaned.controller
                        && !matches!(
                            existing,
                            ast::ControllerRelation::Opponent | ast::ControllerRelation::Opponents
                        )
                    {
                        return Err(LoweringError::Unsupported(
                            "v2:value:conflicting-count-controller",
                        ));
                    }
                    cleaned.controller = Some(ast::ControllerRelation::Opponent);
                    None
                }
                v2::ZoneOwner::ItsOwner
                | v2::ZoneOwner::TheirOwners
                | v2::ZoneOwner::ThatCreatureOwner => {
                    Some(lower_world_resolving_player(v2::PlayerRef::ItsOwner, ctx)?)
                }
            };
            (ValueZone::Battlefield, scope)
        } else {
            (lower_value_zone(zone, ctx)?, None)
        };

        let obj = lower_object_filter(&cleaned, ctx)?;
        let term = EngineValueExpr::Count {
            zone: vz,
            filter: Box::new(obj),
            controller_scope,
        };
        sum_expr = Some(match sum_expr {
            None => term,
            Some(prev) => EngineValueExpr::Add(Box::new(prev), Box::new(term)),
        });
    }

    sum_expr.ok_or(LoweringError::Unsupported("v2:value:empty-count-zones"))
}

fn lower_counter_count_expr(
    subject: &v2::SubjectRef,
    kind: &v2::CounterKind,
    ctx: &LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    let engine_kind = match kind {
        v2::CounterKind::Any => None,
        other => Some(lower_counter_kind(other)?),
    };
    let subj_ref = match subject {
        v2::SubjectRef::SelfObject => CounterSubjectRef::AbilitySource,
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::EquippedCreature
            | v2::AnaphoraRef::EnchantedCreature
            | v2::AnaphoraRef::EnchantedPermanent,
        ) => CounterSubjectRef::AttachedHost,
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget) => CounterSubjectRef::TargetObject,
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatObject
            | v2::AnaphoraRef::ThatSource,
        ) => {
            if ctx.has_object_target {
                CounterSubjectRef::TargetObject
            } else if ctx.in_trigger {
                CounterSubjectRef::TriggerSubject
            } else {
                CounterSubjectRef::AbilitySource
            }
        }
        v2::SubjectRef::Player(player) => {
            let concrete =
                engine_kind.ok_or(LoweringError::Unsupported("v2:value:player-any-counter"))?;
            return lower_player_counter_subject_expr(*player, concrete, ctx);
        }
        _ => {
            return Err(LoweringError::Unsupported("v2:value:counter-count-subject"));
        }
    };
    Ok(EngineValueExpr::CounterCount {
        subject: subj_ref,
        kind: engine_kind,
    })
}

fn lower_player_counter_count_expr(
    player: v2::PlayerRef,
    kind: &v2::CounterKind,
    ctx: &LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    let concrete = lower_counter_kind(kind)?;
    lower_player_counter_subject_expr(player, concrete, ctx)
}

fn lower_player_counter_subject_expr(
    player: v2::PlayerRef,
    concrete: spellwake_engine::effects::CounterKind,
    ctx: &LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    let subject = if player == v2::PlayerRef::You {
        match concrete {
            spellwake_engine::effects::CounterKind::Energy => CounterSubjectRef::ControllerEnergy,
            spellwake_engine::effects::CounterKind::Experience => {
                CounterSubjectRef::ControllerExperience
            }
            spellwake_engine::effects::CounterKind::Poison => CounterSubjectRef::ControllerPoison,
            other => CounterSubjectRef::PlayerCounter(ResolvingPlayer::Controller, other),
        }
    } else {
        let rp = lower_world_resolving_player(player, ctx)?;
        CounterSubjectRef::PlayerCounter(rp, concrete)
    };
    Ok(EngineValueExpr::CounterCount {
        subject,
        kind: Some(concrete),
    })
}

fn lower_aggregate_stat_expr(
    op: v2::AggregateOp,
    stat: ast::StatKind,
    filter: &v2::ObjectFilter,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    let agg_op = match op {
        v2::AggregateOp::Sum => StatAggregate::Sum,
        v2::AggregateOp::Max => StatAggregate::Max,
        v2::AggregateOp::Min => StatAggregate::Min,
    };
    let stat_kind = lower_stat_kind(stat);
    let (stripped_prior, had_among_prior) = split_among_prior_cards(filter);
    if had_among_prior && stripped_prior.zones.is_empty() {
        let Some(binding) = ctx.last_moved_binding else {
            return Err(LoweringError::Unsupported(
                "v2:filter:pred:among-prior-cards",
            ));
        };
        let obj_filter = lower_object_filter(&stripped_prior, ctx)?;
        return Ok(EngineValueExpr::QueryAggregate {
            query: Box::new(ObjectQuery::BoundFiltered {
                binding,
                filter: Box::new(obj_filter),
            }),
            stat: stat_kind,
            op: agg_op,
        });
    }
    let obj_filter = lower_object_filter(filter, ctx)?;

    if filter.zones.is_empty()
        || (filter.zones.len() == 1
            && filter.zones[0].zone == v2::ZoneKind::Battlefield
            && matches!(
                filter.zones[0].owner,
                v2::ZoneOwner::Default
                    | v2::ZoneOwner::Any
                    | v2::ZoneOwner::All
                    | v2::ZoneOwner::EachPlayer
            ))
    {
        return Ok(EngineValueExpr::Aggregate {
            filter: Box::new(obj_filter),
            stat: stat_kind,
            op: agg_op,
        });
    }

    if filter.zones.len() == 1 {
        let qz = lower_world_query_zone(filter.zones[0], ctx)?;
        return Ok(EngineValueExpr::QueryAggregate {
            query: Box::new(ObjectQuery::zone(qz, obj_filter)),
            stat: stat_kind,
            op: agg_op,
        });
    }

    let mut parts = Vec::with_capacity(filter.zones.len());
    for &z in &filter.zones {
        let qz = lower_world_query_zone(z, ctx)?;
        parts.push(ObjectQuery::zone(qz, obj_filter));
    }
    Ok(EngineValueExpr::QueryAggregate {
        query: Box::new(ObjectQuery::Union(parts)),
        stat: stat_kind,
        op: agg_op,
    })
}

fn lower_player_stat_expr(
    player: v2::PlayerRef,
    stat: ast::PlayerStatKind,
    ctx: &LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    let rp = lower_world_resolving_player(player, ctx)?;
    let expr = match stat {
        ast::PlayerStatKind::LifeTotal => {
            if rp == ResolvingPlayer::Controller {
                PlayerStatExpr::ControllerLife
            } else {
                PlayerStatExpr::PlayerLife(rp)
            }
        }
        ast::PlayerStatKind::CardsInHand => PlayerStatExpr::PlayerHandSize(rp),
        ast::PlayerStatKind::CardsInLibrary => PlayerStatExpr::PlayerLibrarySize(rp),
        ast::PlayerStatKind::CardsInGraveyard => PlayerStatExpr::PlayerGraveyardSize(rp),
    };
    Ok(EngineValueExpr::PlayerStat(expr))
}

fn lower_event_count_this_turn(
    event: v2::TurnEventKind,
    player: Option<v2::PlayerRef>,
    exclude_first: bool,
    ctx: &LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    let base = match (event, player) {
        (v2::TurnEventKind::SpellCast, None | Some(v2::PlayerRef::You)) => {
            EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn)
        }
        (v2::TurnEventKind::SpellCast, Some(p)) => {
            let players = lower_world_affected_players(p, ctx)?;
            EngineValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                players,
                filter: Box::new(EngineObjectFilter::any()),
                from_zone: None,
            })
        }
        (v2::TurnEventKind::CardDrawn, None | Some(v2::PlayerRef::You)) => {
            EngineValueExpr::PlayerStat(PlayerStatExpr::CardsDrawnThisTurn)
        }
        (v2::TurnEventKind::CardDiscarded, None | Some(v2::PlayerRef::You)) => {
            EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerDiscardedThisTurn)
        }
        (v2::TurnEventKind::LifeGained, None | Some(v2::PlayerRef::You)) => {
            EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn)
        }
        (v2::TurnEventKind::LifeLost, Some(v2::PlayerRef::You)) => {
            EngineValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn)
        }
        (
            v2::TurnEventKind::LifeLost,
            Some(v2::PlayerRef::EachOpponent | v2::PlayerRef::AnyOpponent),
        ) => EngineValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
        (
            v2::TurnEventKind::CreatureDied,
            None | Some(v2::PlayerRef::EachPlayer | v2::PlayerRef::AnyPlayer),
        ) => EngineValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn),
        (v2::TurnEventKind::CreatureDied, Some(v2::PlayerRef::You)) => {
            EngineValueExpr::PlayerStat(PlayerStatExpr::ControlledCreatureDiedThisTurn)
        }
        _ => {
            return Err(LoweringError::Unsupported("v2:value:event-count-this-turn"));
        }
    };

    if exclude_first {
        let zero = EngineValueExpr::literal(RulesInteger::from(0));
        let one = EngineValueExpr::literal(RulesInteger::from(1));
        Ok(EngineValueExpr::conditional(
            base.clone(),
            Comparison::Greater,
            zero.clone(),
            EngineValueExpr::Sub(Box::new(base), Box::new(one)),
            zero,
        ))
    } else {
        Ok(base)
    }
}

fn lower_mana_spent_from_filter(filter: &v2::ObjectFilter) -> LowerResult<EngineValueExpr> {
    if filter.is_empty() {
        return Ok(EngineValueExpr::SourceState(
            SourceStateExpr::ManaSpentToCast,
        ));
    }
    let clean_others = filter.forbidden_types.is_empty()
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
    if !clean_others {
        return Err(LoweringError::Unsupported(
            "v2:value:mana-spent-from-filter",
        ));
    }

    if filter.subtypes.len() == 1
        && filter.subtypes[0].eq_ignore_ascii_case("treasure")
        && filter.required_supertypes.is_empty()
    {
        return Ok(EngineValueExpr::EventValue(
            EventValueKind::TreasureManaSpentToCast,
        ));
    }
    if filter.subtypes.is_empty()
        && filter.required_supertypes == ast::SupertypeMask::SNOW
        && filter.all_types.is_empty()
        && filter.any_types.is_empty()
    {
        return Ok(EngineValueExpr::EventValue(
            EventValueKind::SnowManaSpentToCast,
        ));
    }
    if filter.subtypes.is_empty()
        && filter.required_supertypes.is_empty()
        && ((filter.all_types == ast::CardTypeMask::CREATURE && filter.any_types.is_empty())
            || (filter.any_types == ast::CardTypeMask::CREATURE && filter.all_types.is_empty()))
    {
        return Ok(EngineValueExpr::EventValue(
            EventValueKind::CreatureManaSpentToCast,
        ));
    }

    Err(LoweringError::Unsupported(
        "v2:value:mana-spent-from-filter",
    ))
}

fn lower_distinct_card_types_among(filter: &v2::ObjectFilter) -> LowerResult<EngineValueExpr> {
    let plain = filter.all_types.is_empty()
        && filter.any_types.is_empty()
        && filter.forbidden_types.is_empty()
        && filter.required_supertypes.is_empty()
        && filter.forbidden_supertypes.is_empty()
        && filter.subtypes.is_empty()
        && filter.forbidden_subtypes.is_empty()
        && filter.any_colors.is_empty()
        && filter.forbidden_colors.is_empty()
        && filter.color_arity.is_none()
        && filter.controller.is_none()
        && filter.owner.is_none()
        && filter.combat.is_none()
        && filter.tapped.is_none()
        && filter.token.is_none()
        && filter.required_keywords.is_empty()
        && filter.forbidden_keywords.is_empty()
        && filter.stat_bounds.is_empty()
        && filter.flags.is_empty()
        && filter.predicates.is_empty();
    if plain && filter.zones.len() == 1 && filter.zones[0].zone == v2::ZoneKind::Graveyard {
        match filter.zones[0].owner {
            v2::ZoneOwner::Default | v2::ZoneOwner::You => {
                return Ok(EngineValueExpr::PlayerStat(
                    PlayerStatExpr::OwnGraveyardDistinctCardTypes,
                ));
            }
            v2::ZoneOwner::Any | v2::ZoneOwner::All | v2::ZoneOwner::EachPlayer => {
                return Ok(EngineValueExpr::PlayerStat(
                    PlayerStatExpr::AllGraveyardsDistinctCardTypes,
                ));
            }
            _ => {}
        }
    }
    Err(LoweringError::Unsupported(
        "v2:value:distinct-card-types-among",
    ))
}

/// Lower a signed P/T delta (`+2`, `-1`, `+X`, `-X`) with an optional scaling expression
/// (`for each ...`) into an [`EngineValueExpr`].
pub(crate) fn lower_pt_delta(
    delta: v2::PtDelta,
    scale: Option<&v2::ValueExpr>,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<EngineValueExpr> {
    match delta {
        v2::PtDelta::Fixed(n) => {
            let Some(sc) = scale else {
                return Ok(EngineValueExpr::literal(RulesInteger::from(i64::from(n))));
            };
            if n == 0 {
                return Ok(EngineValueExpr::literal(RulesInteger::from(0)));
            }
            let lowered_scale = lower_value_expr(sc, ctx)?;
            if n == 1 {
                Ok(lowered_scale)
            } else {
                Ok(EngineValueExpr::Scale {
                    expr: Box::new(lowered_scale),
                    factor: RulesInteger::from(i64::from(n)),
                })
            }
        }
        v2::PtDelta::PlusX => {
            let x = lower_value_expr(&v2::ValueExpr::Var(v2::VarName::X), ctx)?;
            let Some(sc) = scale else {
                return Ok(x);
            };
            let factor = lower_value_const(sc)
                .ok_or(LoweringError::Unsupported("v2:pt-delta:non-const-scaled-x"))?;
            if factor == 1 {
                Ok(x)
            } else {
                Ok(EngineValueExpr::Scale {
                    expr: Box::new(x),
                    factor: RulesInteger::from(i64::from(factor)),
                })
            }
        }
        v2::PtDelta::MinusX => {
            let x = lower_value_expr(&v2::ValueExpr::Var(v2::VarName::X), ctx)?;
            let factor = if let Some(sc) = scale {
                let k = lower_value_const(sc)
                    .ok_or(LoweringError::Unsupported("v2:pt-delta:non-const-scaled-x"))?;
                i64::from(k)
                    .checked_neg()
                    .ok_or(LoweringError::Unsupported("v2:pt-delta:overflow"))?
            } else {
                -1
            };
            Ok(EngineValueExpr::Scale {
                expr: Box::new(x),
                factor: RulesInteger::from(factor),
            })
        }
    }
}
