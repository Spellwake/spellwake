//! Pure world-snapshot evaluator for [`ResolutionCondition`] and [`ValueExpr`], shared by
//! trigger intervening-if checks (`TriggerCondition::Eval`) and static continuous abilities
//! (`StaticSourceCondition::Eval`).
use super::{CharacteristicState, Color, ObjectCharacteristics};
use crate::{
    combat::{BlockStatus, CombatState},
    effects::CounterKind,
    life::LifeState,
    object_identity::{CardId, GameObjects, ObjectId, ObjectZone},
    priority::SeatId,
    program::{
        AttachedCountKind, CardType, CombatFilter, ControllerFilter, CounterSubjectRef,
        DynamicStatAnchor, EventValueKind, ManaValueFilter, MatchObjectRef, ObjectClause,
        ObjectFilter, ObjectFilterFlags, ObjectStatKind, PlayerStatExpr, PowerFilter, Program,
        ResolutionCondition, ResolvingPlayer, SourceRelation, SourceStateExpr, StatAggregate,
        ToughnessFilter, ValueExpr, ValueObjectRef, ValueZone,
    },
    targets::TargetRef,
};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeId, names::NameRules};
use std::collections::{BTreeMap, BTreeSet};

/// Borrowed immutable world cut shared by static characteristic evaluation and trigger discovery.
#[derive(Clone, Copy)]
pub(crate) struct WorldEvalCtx<'a> {
    pub active_player: SeatId,
    pub participants: &'a [SeatId],
    pub life: Option<&'a LifeState>,
    pub combat: Option<&'a CombatState>,
    pub cards: &'a GameObjects,
    pub programs: &'a BTreeMap<CardId, Program>,
    pub state: &'a CharacteristicState,
    pub objects: &'a BTreeMap<ObjectId, ObjectCharacteristics>,
    pub source: ObjectId,
    pub controller: SeatId,
    pub name_rules: &'a NameRules,
    pub trigger_subject: Option<ObjectId>,
    pub related_player: Option<SeatId>,
    pub defending_player: Option<SeatId>,
}

/// CR 700.8: compute maximum bipartite matching of controlled creatures onto the four party roles
/// (`[Cleric, Rogue, Warrior, Wizard]`), returning an exact count in `0..=4`.
pub(crate) fn compute_party_size<'a>(
    creatures: impl Iterator<Item = &'a BTreeSet<CreatureTypeId>>,
    roles: [CreatureTypeId; 4],
) -> usize {
    // 16-state DP bitmask: bit `m` is set iff subset `m` of the 4 party roles is achievable.
    let mut reachable: u16 = 1; // empty set (mask 0)
    for subtypes in creatures {
        let mut role_bits: u8 = 0;
        for (i, role) in roles.iter().enumerate() {
            if subtypes.contains(role) {
                role_bits |= 1 << i;
            }
        }
        if role_bits == 0 {
            continue;
        }
        let prev = reachable;
        for mask in 0u8..16 {
            if (prev & (1u16 << mask)) != 0 {
                let mut avail = role_bits & !mask;
                while avail != 0 {
                    let bit = avail.isolate_lowest_one();
                    reachable |= 1u16 << (mask | bit);
                    avail &= !bit;
                }
            }
        }
        if (reachable & (1u16 << 0b1111)) != 0 {
            return 4;
        }
    }
    let mut best = 0usize;
    for mask in 1u8..16 {
        if (reachable & (1u16 << mask)) != 0 {
            best = best.max(mask.count_ones() as usize);
        }
    }
    best
}

/// Evaluate a world-evaluable [`ResolutionCondition`] against a borrowed world cut.
pub(crate) fn eval_world_condition(
    ctx: &WorldEvalCtx<'_>,
    condition: &ResolutionCondition,
) -> bool {
    let left = eval_world_value_expr(ctx, &condition.left);
    let right = eval_world_value_expr(ctx, &condition.right);
    condition.comparison.evaluate(&left, &right)
}

fn resolve_world_player(ctx: &WorldEvalCtx<'_>, role: ResolvingPlayer) -> Option<SeatId> {
    match role {
        ResolvingPlayer::Controller => Some(ctx.controller),
        ResolvingPlayer::ActivePlayer => Some(ctx.active_player),
        ResolvingPlayer::EnchantedPlayer => match ctx.state.attachments.get(&ctx.source) {
            Some(TargetRef::Player(seat)) => Some(*seat),
            _ => None,
        },
        ResolvingPlayer::AttachedHostController => match ctx.state.attachments.get(&ctx.source) {
            Some(TargetRef::Player(seat)) => Some(*seat),
            Some(TargetRef::Object(host)) => ctx.objects.get(host).and_then(|o| o.controller),
            _ => None,
        },
        ResolvingPlayer::UniqueRelatedPlayer => ctx.related_player,
        ResolvingPlayer::DefendingPlayer => ctx.defending_player,
        ResolvingPlayer::TargetController | ResolvingPlayer::TriggerSubjectController => ctx
            .trigger_subject
            .and_then(|id| ctx.objects.get(&id).and_then(|o| o.controller))
            .or(ctx.related_player),
        ResolvingPlayer::TargetOwner | ResolvingPlayer::TriggerSubjectOwner => ctx
            .trigger_subject
            .and_then(|id| ctx.objects.get(&id).map(|o| o.owner))
            .or(ctx.related_player),
        ResolvingPlayer::SourceChosenPlayer => {
            ctx.state.entry_player_choices.get(&ctx.source).copied()
        }
        _ => None,
    }
}

fn object_mana_value(
    ctx: &WorldEvalCtx<'_>,
    object: &ObjectCharacteristics,
) -> Option<RulesInteger> {
    if let Some(physical) = ctx.cards.get(object.object) {
        if physical.face_down().is_some() {
            return Some(0.into());
        }
        if physical.transformed() && ctx.state.copied_definition(object.object).is_none() {
            return physical
                .card_id()
                .and_then(|id| ctx.programs.get(&id))
                .map(Program::mana_value);
        }
        return ctx
            .state
            .copiable_definition(physical, ctx.programs)
            .map(|(p, _)| p.mana_value());
    }
    Some(object.defined.mana_value.clone())
}

fn matches_world_clause(
    ctx: &WorldEvalCtx<'_>,
    clause: ObjectClause,
    p: &ObjectCharacteristics,
) -> bool {
    let id = p.object;
    let source_ok = match clause.source_relation {
        SourceRelation::Any | SourceRelation::MatchesNamedAgendaSlot => true,
        SourceRelation::SharingNameWithAbilitySource => {
            ctx.objects.get(&ctx.source).is_some_and(|src| {
                ctx.name_rules
                    .shares_name(&p.current_names, &src.current_names)
            })
        }
        SourceRelation::SelfObject => id == ctx.source,
        SourceRelation::AttachedToSelf => {
            ctx.state.attachments.get(&ctx.source) == Some(&TargetRef::Object(id))
        }
        SourceRelation::OtherThanAbilitySource | SourceRelation::OtherThanTriggerSource => {
            id != ctx.source
        }
        SourceRelation::OtherThanBound(bound) => id != bound,
    };
    if !source_ok {
        return false;
    }
    let controller_ok = match clause.controller {
        ControllerFilter::Any => true,
        ControllerFilter::You => p.controller == Some(ctx.controller),
        ControllerFilter::Opponent | ControllerFilter::NotYou => {
            p.controller.is_some_and(|seat| seat != ctx.controller)
        }
        ControllerFilter::OwnerYou => p.owner == ctx.controller,
        ControllerFilter::OwnerOpponent => p.owner != ctx.controller,
        ControllerFilter::RelatedPlayer
        | ControllerFilter::TargetPlayer
        | ControllerFilter::TargetOpponent => ctx
            .related_player
            .is_some_and(|seat| p.controller == Some(seat)),
        ControllerFilter::Player(seat) => p.controller == Some(seat),
    };
    if !controller_ok {
        return false;
    }
    if let Some(expected_token) = clause.token
        && p.is_token != expected_token
    {
        return false;
    }
    let combat_ok = match clause.combat {
        CombatFilter::Any => true,
        CombatFilter::Attacking => ctx.combat.is_some_and(|c| c.attacker(id).is_some()),
        CombatFilter::Blocking => ctx.combat.is_some_and(|c| c.blocker(id).is_some()),
        CombatFilter::AttackingOrBlocking => ctx
            .combat
            .is_some_and(|c| c.attacker(id).is_some() || c.blocker(id).is_some()),
        CombatFilter::Blocked => ctx.combat.is_some_and(|c| {
            c.attacker(id)
                .is_some_and(|a| a.block_status() == BlockStatus::Blocked)
        }),
        CombatFilter::Unblocked => ctx.combat.is_some_and(|c| {
            c.attacker(id)
                .is_some_and(|a| a.block_status() == BlockStatus::Unblocked)
        }),
        CombatFilter::BlockingSource => ctx.combat.is_some_and(|c| {
            c.blocker(id)
                .is_some_and(|b| b.attackers().contains(&ctx.source))
        }),
        CombatFilter::BlockedBySource => ctx.combat.is_some_and(|c| {
            c.blocker(ctx.source)
                .is_some_and(|b| b.attackers().contains(&id))
        }),
        CombatFilter::BlockingOrBlockedBySource => ctx.combat.is_some_and(|c| {
            c.blocker(id)
                .is_some_and(|b| b.attackers().contains(&ctx.source))
                || c.blocker(ctx.source)
                    .is_some_and(|b| b.attackers().contains(&id))
        }),
        CombatFilter::NotAttacking => !ctx.combat.is_some_and(|c| c.attacker(id).is_some()),
    };
    if !combat_ok {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS)
        && ctx.state.total_permanent_counters(id).is_positive()
    {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::WITH_ANY_COUNTER) && !p.has_any_counter {
        return false;
    }
    if clause
        .flags
        .contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
        && !p.has_plus_one_counter
    {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::EQUIPPED)
        && !ctx.objects.values().any(|att| {
            att.zone == ObjectZone::Battlefield
                && att.attached_to == Some(TargetRef::Object(id))
                && matches!(
                    att.attachment_kind,
                    Some(crate::attachment::AttachmentKind::Equipment)
                )
        })
    {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::ENCHANTED)
        && !ctx.objects.values().any(|att| {
            att.zone == ObjectZone::Battlefield
                && att.attached_to == Some(TargetRef::Object(id))
                && matches!(
                    att.attachment_kind,
                    Some(crate::attachment::AttachmentKind::Aura(_))
                )
        })
    {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::MODIFIED) {
        let modified = p.has_any_counter
            || ctx.objects.values().any(|att| {
                att.zone == ObjectZone::Battlefield
                    && att.attached_to == Some(TargetRef::Object(id))
                    && (matches!(
                        att.attachment_kind,
                        Some(crate::attachment::AttachmentKind::Equipment)
                    ) || (matches!(
                        att.attachment_kind,
                        Some(crate::attachment::AttachmentKind::Aura(_))
                    ) && att.controller == p.controller))
            });
        if !modified {
            return false;
        }
    }
    if clause
        .flags
        .contains(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN)
        && !ctx
            .state
            .object_damage_taken_this_turn
            .get(&id)
            .is_some_and(RulesInteger::is_positive)
    {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::ATTACKED_THIS_TURN)
        && !ctx.state.attacked_objects_this_turn.contains(&id)
    {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::ENTERED_THIS_TURN)
        && !ctx.state.entered_objects_this_turn.contains(&id)
    {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::COMMANDER_ONLY) && p.commander_owner.is_none() {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::FACE_DOWN)
        && !ctx
            .cards
            .get(id)
            .is_some_and(|card| card.face_down().is_some())
    {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::EXILED_WITH_SELF)
        && !(ctx
            .state
            .imprinted_cards
            .get(&ctx.source)
            .is_some_and(|list| list.contains(&id))
            || ctx
                .state
                .ability_donors_in_exile
                .get(&ctx.source)
                .is_some_and(|list| list.contains(&id)))
    {
        return false;
    }
    if clause
        .flags
        .contains(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN)
        && !ctx
            .state
            .creature_damage_pairs
            .as_ref()
            .is_some_and(|pairs| pairs.contains(&(ctx.source, id)))
    {
        return false;
    }
    if clause.flags.contains(ObjectFilterFlags::DIED_THIS_TURN)
        && !ctx.state.died_this_turn.contains(&id)
    {
        return false;
    }
    if clause
        .flags
        .contains(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_COLOR)
        && !ctx
            .state
            .entry_color_choices
            .get(&ctx.source)
            .is_some_and(|color| p.current_colors.contains(color))
    {
        return false;
    }
    let eval_anchor = |anchor: DynamicStatAnchor| -> Option<RulesInteger> {
        match anchor {
            DynamicStatAnchor::ChosenX => None,
            DynamicStatAnchor::SourcePower => ctx.objects.get(&ctx.source).and_then(|o| {
                o.power_toughness
                    .as_ref()
                    .map(|pt| pt.derived.power.clone())
            }),
            DynamicStatAnchor::SourceToughness => ctx.objects.get(&ctx.source).and_then(|o| {
                o.power_toughness
                    .as_ref()
                    .map(|pt| pt.derived.toughness.clone())
            }),
            DynamicStatAnchor::SourceManaValue => ctx
                .objects
                .get(&ctx.source)
                .and_then(|o| object_mana_value(ctx, o)),
            DynamicStatAnchor::SourceCounters(kind) => {
                Some(ctx.state.permanent_counter_count(ctx.source, kind))
            }
            DynamicStatAnchor::ControllerLife => {
                ctx.life.and_then(|l| l.get(ctx.controller)).cloned()
            }
            DynamicStatAnchor::ControllerHandSize => Some(RulesInteger::from_usize(
                ctx.objects
                    .values()
                    .filter(|o| o.zone == ObjectZone::Hand(ctx.controller))
                    .count(),
            )),
            DynamicStatAnchor::ControllerGraveyardCount => Some(RulesInteger::from_usize(
                ctx.objects
                    .values()
                    .filter(|o| o.zone == ObjectZone::Graveyard(ctx.controller))
                    .count(),
            )),
            DynamicStatAnchor::ControllerControlledCount(ct) => Some(RulesInteger::from_usize(
                ctx.objects
                    .values()
                    .filter(|o| {
                        o.zone == ObjectZone::Battlefield
                            && o.controller == Some(ctx.controller)
                            && o.has_type(ct)
                    })
                    .count(),
            )),
            DynamicStatAnchor::ControllerSubtypeCount(sub) => Some(RulesInteger::from_usize(
                ctx.objects
                    .values()
                    .filter(|o| {
                        o.zone == ObjectZone::Battlefield
                            && o.controller == Some(ctx.controller)
                            && o.current_creature_subtypes.contains(&sub)
                    })
                    .count(),
            )),
            DynamicStatAnchor::TriggerSubjectPower => ctx
                .trigger_subject
                .and_then(|tid| ctx.objects.get(&tid))
                .and_then(|o| {
                    o.power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.power.clone())
                }),
            DynamicStatAnchor::TriggerSubjectManaValue => ctx
                .trigger_subject
                .and_then(|tid| ctx.objects.get(&tid))
                .and_then(|o| object_mana_value(ctx, o)),
        }
    };
    if let PowerFilter::CompareAnchor { op, anchor } = clause.power
        && !p
            .power_toughness
            .as_ref()
            .zip(eval_anchor(anchor))
            .is_some_and(|(stats, bound)| op.evaluate(&stats.derived.power, &bound))
    {
        return false;
    }
    if let ToughnessFilter::CompareAnchor { op, anchor } = clause.toughness
        && !p
            .power_toughness
            .as_ref()
            .zip(eval_anchor(anchor))
            .is_some_and(|(stats, bound)| op.evaluate(&stats.derived.toughness, &bound))
    {
        return false;
    }
    if let ManaValueFilter::CompareAnchor { op, anchor } = clause.mana_value
        && !object_mana_value(ctx, p)
            .zip(eval_anchor(anchor))
            .is_some_and(|(mv, bound)| op.evaluate(&mv, &bound))
    {
        return false;
    }
    if matches!(clause.power, PowerFilter::AtMostControlledCreatureCount)
        && !p.power_toughness.as_ref().is_some_and(|stats| {
            let count = ctx
                .objects
                .values()
                .filter(|c| {
                    c.zone == ObjectZone::Battlefield
                        && c.is_creature()
                        && c.controller == Some(ctx.controller)
                })
                .count();
            stats.derived.power <= RulesInteger::from_usize(count)
        })
    {
        return false;
    }
    let excluded = BTreeSet::new();
    clause.matches_static_characteristics(p, &excluded, || object_mana_value(ctx, p))
}

fn matches_world_object_filter(
    ctx: &WorldEvalCtx<'_>,
    filter: ObjectFilter,
    p: &ObjectCharacteristics,
) -> bool {
    match filter {
        ObjectFilter::Clause(a) => matches_world_clause(ctx, a, p),
        ObjectFilter::Either(a, b) => {
            matches_world_clause(ctx, a, p) || matches_world_clause(ctx, b, p)
        }
    }
}

fn count_zone_for_seat(
    ctx: &WorldEvalCtx<'_>,
    zone: ValueZone,
    filter: ObjectFilter,
    seat: SeatId,
) -> usize {
    match zone {
        ValueZone::Battlefield => {
            let scoped = filter.of_player(seat);
            ctx.objects
                .values()
                .filter(|o| {
                    o.zone == ObjectZone::Battlefield && matches_world_object_filter(ctx, scoped, o)
                })
                .count()
        }
        ValueZone::Graveyard(_) => ctx
            .objects
            .values()
            .filter(|o| {
                o.zone == ObjectZone::Graveyard(seat) && matches_world_object_filter(ctx, filter, o)
            })
            .count(),
        ValueZone::Hand(_) => ctx
            .objects
            .values()
            .filter(|o| {
                o.zone == ObjectZone::Hand(seat) && matches_world_object_filter(ctx, filter, o)
            })
            .count(),
        ValueZone::Library(_) => ctx
            .objects
            .values()
            .filter(|o| {
                o.zone == ObjectZone::Library(seat) && matches_world_object_filter(ctx, filter, o)
            })
            .count(),
        ValueZone::Exile(_) => ctx
            .objects
            .values()
            .filter(|o| {
                o.zone == ObjectZone::Exile
                    && o.owner == seat
                    && matches_world_object_filter(ctx, filter, o)
            })
            .count(),
        ValueZone::AllGraveyards => ctx
            .objects
            .values()
            .filter(|o| {
                matches!(o.zone, ObjectZone::Graveyard(_))
                    && matches_world_object_filter(ctx, filter, o)
            })
            .count(),
    }
}

/// Evaluate a world-evaluable [`ValueExpr`] against a borrowed world cut.
pub(crate) fn eval_world_value_expr(ctx: &WorldEvalCtx<'_>, expr: &ValueExpr) -> RulesInteger {
    let opponents = || {
        ctx.participants
            .iter()
            .copied()
            .filter(|&p| p != ctx.controller)
    };
    match expr {
        ValueExpr::Literal(v) => v.clone(),
        ValueExpr::Scale { expr, factor } => eval_world_value_expr(ctx, expr).times(factor),
        ValueExpr::DivConst {
            expr,
            divisor,
            round_up,
        } => {
            let value = eval_world_value_expr(ctx, expr);
            let d = usize::from(divisor.get());
            if d == 2 {
                if *round_up {
                    value.nonnegative_half_rounded_up()
                } else {
                    value.nonnegative_half_rounded_down()
                }
            } else if let Some(n) = value.to_usize() {
                RulesInteger::from_usize(if *round_up { n.div_ceil(d) } else { n / d })
            } else {
                0.into()
            }
        }
        ValueExpr::Add(a, b) => eval_world_value_expr(ctx, a).plus(&eval_world_value_expr(ctx, b)),
        ValueExpr::Sub(a, b) => eval_world_value_expr(ctx, a).minus(&eval_world_value_expr(ctx, b)),
        ValueExpr::Conditional {
            left,
            comparison,
            right,
            then_value,
            else_value,
        } => {
            let lhs = eval_world_value_expr(ctx, left);
            let rhs = eval_world_value_expr(ctx, right);
            if comparison.evaluate(&lhs, &rhs) {
                eval_world_value_expr(ctx, then_value)
            } else {
                eval_world_value_expr(ctx, else_value)
            }
        }
        ValueExpr::DomainCount => {
            let mut basic_types = BTreeSet::new();
            for p in ctx.objects.values() {
                if p.zone == ObjectZone::Battlefield
                    && p.controller == Some(ctx.controller)
                    && p.has_type(CardType::Land)
                {
                    for sub in &p.current_land_subtypes {
                        if let Some(basic) = sub.basic() {
                            basic_types.insert(basic);
                        }
                    }
                }
            }
            RulesInteger::from_usize(basic_types.len())
        }
        ValueExpr::OpponentsCount => RulesInteger::from_usize(opponents().count()),
        ValueExpr::Devotion(colors) => {
            let mut total: usize = 0;
            let matches_mana = |mt: crate::mana::ManaType| match mt {
                crate::mana::ManaType::White => colors.contains(Color::White),
                crate::mana::ManaType::Blue => colors.contains(Color::Blue),
                crate::mana::ManaType::Black => colors.contains(Color::Black),
                crate::mana::ManaType::Red => colors.contains(Color::Red),
                crate::mana::ManaType::Green => colors.contains(Color::Green),
                crate::mana::ManaType::Colourless => false,
            };
            for p in ctx.objects.values() {
                if p.zone != ObjectZone::Battlefield || p.controller != Some(ctx.controller) {
                    continue;
                }
                let Some(physical) = ctx.cards.get(p.object) else {
                    continue;
                };
                let Some((program, _)) = ctx.state.copiable_definition(physical, ctx.programs)
                else {
                    continue;
                };
                let Some(cost) = program
                    .permanent()
                    .and_then(|perm| perm.casting_cost.as_ref())
                else {
                    continue;
                };
                for (mt, cnt) in &cost.required {
                    if matches_mana(*mt) {
                        total = total.saturating_add(cnt.to_usize().unwrap_or(0));
                    }
                }
                for hybrid in &program.hybrid_symbols {
                    let first_match = matches_mana(hybrid.first);
                    let second_match = matches!(
                        hybrid.alternative,
                        crate::mana::HybridAlternative::Mana(mt) if matches_mana(mt)
                    );
                    if first_match || second_match {
                        total = total.saturating_add(1);
                    }
                }
            }
            RulesInteger::from_usize(total)
        }
        ValueExpr::Count {
            zone,
            filter,
            controller_scope,
        } => {
            let mut bound_filter = **filter;
            let count = match *zone {
                ValueZone::Battlefield => {
                    if let Some(role) = controller_scope {
                        let Some(seat) = resolve_world_player(ctx, *role) else {
                            return 0.into();
                        };
                        bound_filter = bound_filter.of_player(seat);
                    }
                    ctx.objects
                        .values()
                        .filter(|o| {
                            o.zone == ObjectZone::Battlefield
                                && matches_world_object_filter(ctx, bound_filter, o)
                        })
                        .count()
                }
                ValueZone::Graveyard(role) => {
                    let Some(seat) = resolve_world_player(ctx, role) else {
                        return 0.into();
                    };
                    count_zone_for_seat(ctx, ValueZone::Graveyard(role), bound_filter, seat)
                }
                ValueZone::Hand(role) => {
                    let Some(seat) = resolve_world_player(ctx, role) else {
                        return 0.into();
                    };
                    count_zone_for_seat(ctx, ValueZone::Hand(role), bound_filter, seat)
                }
                ValueZone::Library(role) => {
                    let Some(seat) = resolve_world_player(ctx, role) else {
                        return 0.into();
                    };
                    count_zone_for_seat(ctx, ValueZone::Library(role), bound_filter, seat)
                }
                ValueZone::AllGraveyards => ctx
                    .objects
                    .values()
                    .filter(|o| {
                        matches!(o.zone, ObjectZone::Graveyard(_))
                            && matches_world_object_filter(ctx, bound_filter, o)
                    })
                    .count(),
                ValueZone::Exile(owner_role) => {
                    let req_owner = match owner_role {
                        None => None,
                        Some(role) => match resolve_world_player(ctx, role) {
                            Some(seat) => Some(seat),
                            None => return 0.into(),
                        },
                    };
                    ctx.objects
                        .values()
                        .filter(|o| {
                            o.zone == ObjectZone::Exile
                                && req_owner.is_none_or(|seat| o.owner == seat)
                                && matches_world_object_filter(ctx, bound_filter, o)
                        })
                        .count()
                }
            };
            RulesInteger::from_usize(count)
        }
        ValueExpr::OpponentMaxCount { zone, filter } => {
            let max_count = opponents()
                .map(|opp| count_zone_for_seat(ctx, *zone, **filter, opp))
                .max()
                .unwrap_or(0);
            RulesInteger::from_usize(max_count)
        }
        ValueExpr::OpponentMinCount { zone, filter } => {
            let min_count = opponents()
                .map(|opp| count_zone_for_seat(ctx, *zone, **filter, opp))
                .min()
                .unwrap_or(0);
            RulesInteger::from_usize(min_count)
        }
        ValueExpr::PartyCount(roles) => {
            let creatures = ctx.objects.values().filter_map(|o| {
                (o.zone == ObjectZone::Battlefield
                    && o.controller == Some(ctx.controller)
                    && o.is_creature())
                .then_some(&o.current_creature_subtypes)
            });
            RulesInteger::from_usize(compute_party_size(creatures, **roles))
        }
        ValueExpr::DistinctColors { filter } => {
            let mut colors = BTreeSet::new();
            for o in ctx.objects.values() {
                if o.zone == ObjectZone::Battlefield
                    && matches_world_object_filter(ctx, **filter, o)
                {
                    colors.extend(o.current_colors.iter().copied());
                }
            }
            RulesInteger::from_usize(colors.len())
        }
        ValueExpr::DistinctManaValuesInGraveyard(role) => {
            let Some(seat) = resolve_world_player(ctx, *role) else {
                return 0.into();
            };
            let mut mvs = BTreeSet::new();
            for o in ctx.objects.values() {
                if o.zone == ObjectZone::Graveyard(seat)
                    && let Some(mv) = object_mana_value(ctx, o)
                {
                    mvs.insert(mv);
                }
            }
            RulesInteger::from_usize(mvs.len())
        }
        ValueExpr::SourceState(kind) => match kind {
            SourceStateExpr::EnteredThisTurn => {
                i64::from(ctx.state.entered_objects_this_turn.contains(&ctx.source)).into()
            }
            SourceStateExpr::AttackedThisTurn => {
                i64::from(ctx.state.attacked_objects_this_turn.contains(&ctx.source)).into()
            }
            SourceStateExpr::AttackedOrBlockedThisTurn => i64::from(
                ctx.state.attacked_objects_this_turn.contains(&ctx.source)
                    || ctx.state.blocked_objects_this_turn.contains(&ctx.source),
            )
            .into(),
            SourceStateExpr::AttackedOrBlockedThisCombat => {
                i64::from(ctx.combat.is_some_and(|c| {
                    c.attacker(ctx.source).is_some() || c.blocker(ctx.source).is_some()
                }))
                .into()
            }
            SourceStateExpr::AttackingAlone => {
                i64::from(ctx.combat.is_some_and(|c| {
                    c.attacker(ctx.source).is_some() && c.attackers().count() == 1
                }))
                .into()
            }
            SourceStateExpr::Saddled => i64::from(ctx.state.saddled.contains(&ctx.source)).into(),
            SourceStateExpr::Renowned => i64::from(ctx.state.renowned.contains(&ctx.source)).into(),
            SourceStateExpr::Paired => {
                i64::from(ctx.state.paired_creatures.contains_key(&ctx.source)).into()
            }
            SourceStateExpr::DamageDealtThisTurn => ctx
                .state
                .object_damage_taken_this_turn
                .get(&ctx.source)
                .cloned()
                .unwrap_or_else(|| 0.into()),
            SourceStateExpr::AbilityActivatedThisTurn => i64::from(
                ctx.state
                    .activated_sources_this_turn
                    .get(&ctx.source)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            SourceStateExpr::AbilityResolvedCountThisTurn => i64::from(
                ctx.state
                    .ability_resolutions_by_source_this_turn
                    .get(&ctx.source)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            SourceStateExpr::ManaSpentToCast => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .map_or_else(|| 0.into(), |s| s.total_mana.clone()),
            SourceStateExpr::ColorsSpentToCast => {
                ctx.state.cast_summaries.get(&ctx.source).map_or_else(
                    || 0.into(),
                    |s| RulesInteger::from_usize(s.colors_spent.len()),
                )
            }
            SourceStateExpr::ManaTypeSpentToCast(mt) => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .and_then(|s| s.mana_by_type.get(mt))
                .cloned()
                .unwrap_or_else(|| 0.into()),
            SourceStateExpr::SnowManaSpentToCast => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .map_or_else(|| 0.into(), |s| s.snow_mana.clone()),
            SourceStateExpr::TreasureManaSpentToCast => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .map_or_else(|| 0.into(), |s| s.treasure_mana.clone()),
            SourceStateExpr::CreatureManaSpentToCast => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .map_or_else(|| 0.into(), |s| s.creature_mana.clone()),
            SourceStateExpr::CastFromOutsideHand => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cast_from_outside_hand),
            )
            .into(),
            SourceStateExpr::CastFromExile => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cast_from_exile),
            )
            .into(),
            SourceStateExpr::CastFromGraveyard => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cast_from_graveyard),
            )
            .into(),
            SourceStateExpr::CastDuringMainPhase => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cast_during_own_main),
            )
            .into(),
            SourceStateExpr::Bargained => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.bargained),
            )
            .into(),
            SourceStateExpr::GiftPromised => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.gift_promised),
            )
            .into(),
            SourceStateExpr::Cleaved => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cleaved),
            )
            .into(),
            SourceStateExpr::EvidenceCollected => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.evidence_collected),
            )
            .into(),
            SourceStateExpr::Monstrous => {
                i64::from(ctx.state.monstrous.contains(&ctx.source)).into()
            }
            SourceStateExpr::TributePaid => {
                i64::from(ctx.state.tribute_paid.contains(&ctx.source)).into()
            }
            SourceStateExpr::Suspected => i64::from(
                ctx.state
                    .suspect_counters
                    .get(&ctx.source)
                    .is_some_and(RulesInteger::is_positive),
            )
            .into(),
            SourceStateExpr::Solved => {
                i64::from(ctx.state.solved_cases.contains(&ctx.source)).into()
            }
            SourceStateExpr::Prepared => i64::from(ctx.state.prepared.contains(&ctx.source)).into(),
            SourceStateExpr::Goaded => 0.into(),
            SourceStateExpr::ExertedThisTurn => {
                i64::from(ctx.state.exerted_objects_this_turn.contains(&ctx.source)).into()
            }
            SourceStateExpr::InZone(expected) => {
                use crate::program::SourceZoneKind;
                let in_z = ctx.objects.get(&ctx.source).is_some_and(|o| {
                    matches!(
                        (o.zone, expected),
                        (ObjectZone::Battlefield, SourceZoneKind::Battlefield)
                            | (ObjectZone::Graveyard(_), SourceZoneKind::Graveyard)
                            | (ObjectZone::Hand(_), SourceZoneKind::Hand)
                            | (ObjectZone::Exile, SourceZoneKind::Exile)
                            | (ObjectZone::Command, SourceZoneKind::Command)
                            | (ObjectZone::Stack, SourceZoneKind::Stack)
                            | (ObjectZone::Library(_), SourceZoneKind::Library)
                    )
                });
                i64::from(in_z).into()
            }
            SourceStateExpr::IsRingBearer => i64::from(
                ctx.state.ring_bearer.get(&ctx.controller) == Some(&ctx.source)
                    && ctx.objects.get(&ctx.source).is_some_and(|o| {
                        o.zone == ObjectZone::Battlefield && o.controller == Some(ctx.controller)
                    }),
            )
            .into(),
            SourceStateExpr::EnteredFromGraveyard => {
                i64::from(ctx.state.entered_from_graveyard.contains(&ctx.source)).into()
            }
            SourceStateExpr::PaidAlternative => i64::from(
                ctx.state.paid_alternative_objects.contains(&ctx.source)
                    || ctx
                        .state
                        .cast_summaries
                        .get(&ctx.source)
                        .is_some_and(|s| s.paid_alternative),
            )
            .into(),
        },
        ValueExpr::CountOtherSharingSourceSubtype => {
            let subtypes = ctx
                .objects
                .get(&ctx.source)
                .map(|o| &o.current_creature_subtypes);
            let count = ctx
                .objects
                .values()
                .filter(|o| {
                    o.object != ctx.source
                        && o.zone == ObjectZone::Battlefield
                        && o.controller == Some(ctx.controller)
                        && o.is_creature()
                        && subtypes
                            .is_some_and(|types| !o.current_creature_subtypes.is_disjoint(types))
                })
                .count();
            RulesInteger::from_usize(count)
        }
        ValueExpr::CountAttackingController => {
            RulesInteger::from_usize(ctx.combat.map_or(0, |c| {
                c.attackers()
                    .filter(|a| {
                        a.defender() == ctx.controller && a.defending_planeswalker().is_none()
                    })
                    .count()
            }))
        }
        ValueExpr::Aggregate { filter, stat, op } => {
            let values = ctx.objects.values().filter_map(|o| {
                if o.zone != ObjectZone::Battlefield
                    || !matches_world_object_filter(ctx, **filter, o)
                {
                    return None;
                }
                match stat {
                    ObjectStatKind::Power => o
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.power.clone()),
                    ObjectStatKind::Toughness => o
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.toughness.clone()),
                    ObjectStatKind::ManaValue => object_mana_value(ctx, o),
                    ObjectStatKind::Loyalty => Some(
                        ctx.state
                            .permanent_counter_count(o.object, CounterKind::Loyalty),
                    ),
                }
            });
            match op {
                StatAggregate::Max => values.max().unwrap_or_else(|| 0.into()),
                StatAggregate::Min => values.min().unwrap_or_else(|| 0.into()),
                StatAggregate::Sum => values.fold(0.into(), |acc: RulesInteger, v| acc.plus(&v)),
            }
        }
        ValueExpr::ObjectStat { subject, stat } => match subject {
            ValueObjectRef::AbilitySource | ValueObjectRef::TriggerSource => ctx
                .objects
                .get(&ctx.source)
                .and_then(|o| match stat {
                    ObjectStatKind::Power => o
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.power.clone()),
                    ObjectStatKind::Toughness => o
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.toughness.clone()),
                    ObjectStatKind::ManaValue => object_mana_value(ctx, o),
                    ObjectStatKind::Loyalty => Some(
                        ctx.state
                            .permanent_counter_count(o.object, CounterKind::Loyalty),
                    ),
                })
                .unwrap_or_else(|| 0.into()),
            ValueObjectRef::AttachedHost => ctx
                .objects
                .get(&ctx.source)
                .and_then(|s| match s.attached_to {
                    Some(TargetRef::Object(host)) => ctx.objects.get(&host),
                    _ => None,
                })
                .and_then(|o| match stat {
                    ObjectStatKind::Power => o
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.power.clone()),
                    ObjectStatKind::Toughness => o
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.toughness.clone()),
                    ObjectStatKind::ManaValue => object_mana_value(ctx, o),
                    ObjectStatKind::Loyalty => Some(
                        ctx.state
                            .permanent_counter_count(o.object, CounterKind::Loyalty),
                    ),
                })
                .unwrap_or_else(|| 0.into()),
            ValueObjectRef::TriggerSubject
            | ValueObjectRef::DyingCreature
            | ValueObjectRef::EnteringPermanent
            | ValueObjectRef::TriggeringSpell => ctx
                .trigger_subject
                .and_then(|id| ctx.objects.get(&id))
                .and_then(|o| match stat {
                    ObjectStatKind::Power => o
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.power.clone()),
                    ObjectStatKind::Toughness => o
                        .power_toughness
                        .as_ref()
                        .map(|pt| pt.derived.toughness.clone()),
                    ObjectStatKind::ManaValue => object_mana_value(ctx, o),
                    ObjectStatKind::Loyalty => Some(
                        ctx.state
                            .permanent_counter_count(o.object, CounterKind::Loyalty),
                    ),
                })
                .unwrap_or_else(|| 0.into()),
            _ => 0.into(),
        },
        ValueExpr::CounterCount { subject, kind } => match subject {
            CounterSubjectRef::AbilitySource | CounterSubjectRef::TriggerSource => match kind {
                Some(k) => ctx.state.permanent_counter_count(ctx.source, *k),
                None => ctx.state.total_permanent_counters(ctx.source),
            },
            CounterSubjectRef::TriggerSubject => ctx
                .trigger_subject
                .map(|id| match kind {
                    Some(k) => ctx.state.permanent_counter_count(id, *k),
                    None => ctx.state.total_permanent_counters(id),
                })
                .unwrap_or_else(|| 0.into()),
            CounterSubjectRef::AttachedHost => ctx
                .objects
                .get(&ctx.source)
                .and_then(|s| match s.attached_to {
                    Some(TargetRef::Object(host)) => Some(match kind {
                        Some(k) => ctx.state.permanent_counter_count(host, *k),
                        None => ctx.state.total_permanent_counters(host),
                    }),
                    _ => None,
                })
                .unwrap_or_else(|| 0.into()),
            CounterSubjectRef::ControllerExperience => ctx
                .state
                .experience
                .get(&ctx.controller)
                .cloned()
                .unwrap_or_else(|| 0.into()),
            CounterSubjectRef::ControllerEnergy => ctx
                .state
                .player_counters
                .get(&CounterKind::Energy)
                .and_then(|m| m.get(&ctx.controller))
                .cloned()
                .unwrap_or_else(|| 0.into()),
            CounterSubjectRef::ControllerPoison => ctx
                .state
                .player_counters
                .get(&CounterKind::Poison)
                .and_then(|m| m.get(&ctx.controller))
                .cloned()
                .unwrap_or_else(|| 0.into()),
            _ => 0.into(),
        },
        ValueExpr::AttachedCount(kind) => match kind {
            AttachedCountKind::EquipmentOnSource => RulesInteger::from_usize(
                ctx.objects
                    .values()
                    .filter(|o| {
                        o.zone == ObjectZone::Battlefield
                            && o.attached_to == Some(TargetRef::Object(ctx.source))
                            && o.attachment_kind
                                == Some(crate::attachment::AttachmentKind::Equipment)
                            && o.has_type(CardType::Artifact)
                    })
                    .count(),
            ),
            AttachedCountKind::AurasAndEquipmentOnSource => RulesInteger::from_usize(
                ctx.objects
                    .values()
                    .filter(|o| {
                        o.zone == ObjectZone::Battlefield
                            && o.attached_to == Some(TargetRef::Object(ctx.source))
                            && match o.attachment_kind {
                                Some(crate::attachment::AttachmentKind::Equipment) => {
                                    o.has_type(CardType::Artifact)
                                }
                                Some(crate::attachment::AttachmentKind::Aura(_)) => {
                                    o.has_type(CardType::Enchantment)
                                }
                                _ => false,
                            }
                    })
                    .count(),
            ),
            AttachedCountKind::CursesOnRelatedPlayer => 0.into(),
        },
        ValueExpr::ObjectMatches { subject, filter } => match subject {
            MatchObjectRef::AbilitySource => i64::from(
                ctx.objects
                    .get(&ctx.source)
                    .is_some_and(|o| matches_world_object_filter(ctx, **filter, o)),
            )
            .into(),
            MatchObjectRef::AttachedHost => i64::from(
                ctx.objects
                    .get(&ctx.source)
                    .and_then(|s| match s.attached_to {
                        Some(TargetRef::Object(host)) => ctx.objects.get(&host),
                        _ => None,
                    })
                    .is_some_and(|o| matches_world_object_filter(ctx, **filter, o)),
            )
            .into(),
            MatchObjectRef::TriggerSubject | MatchObjectRef::DyingCreature => i64::from(
                ctx.trigger_subject
                    .and_then(|id| ctx.objects.get(&id))
                    .is_some_and(|o| matches_world_object_filter(ctx, **filter, o)),
            )
            .into(),
            _ => 0.into(),
        },
        ValueExpr::EventValue(kind) => match kind {
            EventValueKind::CreaturesDiedThisTurn => {
                i64::from(ctx.state.creatures_died_this_turn_count).into()
            }
            EventValueKind::SourceAttackedThisTurn => {
                i64::from(ctx.state.attacked_objects_this_turn.contains(&ctx.source)).into()
            }
            EventValueKind::ManaSpentToCast => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .map_or_else(|| 0.into(), |s| s.total_mana.clone()),
            EventValueKind::ColorsSpentToCast => {
                ctx.state.cast_summaries.get(&ctx.source).map_or_else(
                    || 0.into(),
                    |s| RulesInteger::from_usize(s.colors_spent.len()),
                )
            }
            EventValueKind::ManaTypeSpentToCast(mt) => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .and_then(|s| s.mana_by_type.get(mt))
                .cloned()
                .unwrap_or_else(|| 0.into()),
            EventValueKind::SnowManaSpentToCast => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .map_or_else(|| 0.into(), |s| s.snow_mana.clone()),
            EventValueKind::TreasureManaSpentToCast => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .map_or_else(|| 0.into(), |s| s.treasure_mana.clone()),
            EventValueKind::CreatureManaSpentToCast => ctx
                .state
                .cast_summaries
                .get(&ctx.source)
                .map_or_else(|| 0.into(), |s| s.creature_mana.clone()),
            EventValueKind::CastFromOutsideHand => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cast_from_outside_hand),
            )
            .into(),
            EventValueKind::CastFromExile => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cast_from_exile),
            )
            .into(),
            EventValueKind::CastFromGraveyard => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cast_from_graveyard),
            )
            .into(),
            EventValueKind::CastDuringOwnMain => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cast_during_own_main),
            )
            .into(),
            EventValueKind::Bargained => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.bargained),
            )
            .into(),
            EventValueKind::GiftPromised => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.gift_promised),
            )
            .into(),
            EventValueKind::Cleaved => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.cleaved),
            )
            .into(),
            EventValueKind::EvidenceCollected => i64::from(
                ctx.state
                    .cast_summaries
                    .get(&ctx.source)
                    .is_some_and(|s| s.evidence_collected),
            )
            .into(),
            EventValueKind::PaidAlternative => i64::from(
                ctx.state.paid_alternative_objects.contains(&ctx.source)
                    || ctx
                        .state
                        .cast_summaries
                        .get(&ctx.source)
                        .is_some_and(|s| s.paid_alternative),
            )
            .into(),
            EventValueKind::KickerCount(idx) => ctx
                .state
                .kicker_counts
                .get(&ctx.source)
                .and_then(|m| m.get(idx))
                .or_else(|| {
                    ctx.trigger_subject
                        .and_then(|id| ctx.state.kicker_counts.get(&id))
                        .and_then(|m| m.get(idx))
                })
                .cloned()
                .unwrap_or_else(|| {
                    if *idx == 0 && ctx.state.kicked.contains(&ctx.source) {
                        1.into()
                    } else {
                        0.into()
                    }
                }),
            _ => 0.into(),
        },
        ValueExpr::PlayerStat(stat) => match stat {
            PlayerStatExpr::ControllerLife => ctx
                .life
                .and_then(|l| l.get(ctx.controller))
                .cloned()
                .unwrap_or_else(|| 0.into()),
            PlayerStatExpr::ControllerStartingLife => ctx
                .life
                .and_then(|l| l.starting_total(ctx.controller))
                .cloned()
                .unwrap_or_else(|| 0.into()),
            PlayerStatExpr::ControllerLifeAboveStarting => ctx
                .life
                .and_then(|l| {
                    let cur = l.get(ctx.controller)?;
                    let start = l.starting_total(ctx.controller)?;
                    Some(cur.minus(start))
                })
                .unwrap_or_else(|| 0.into()),
            PlayerStatExpr::OpponentMaxLife => ctx
                .life
                .and_then(|l| opponents().filter_map(|p| l.get(p)).max().cloned())
                .unwrap_or_else(|| {
                    ctx.life
                        .and_then(|l| l.get(ctx.controller))
                        .cloned()
                        .unwrap_or_else(|| 0.into())
                }),
            PlayerStatExpr::OpponentMinLife => ctx
                .life
                .and_then(|l| opponents().filter_map(|p| l.get(p)).min().cloned())
                .unwrap_or_else(|| {
                    ctx.life
                        .and_then(|l| l.get(ctx.controller))
                        .cloned()
                        .unwrap_or_else(|| 0.into())
                }),
            PlayerStatExpr::OpponentMaxCounter(kind) => opponents()
                .map(|opp| {
                    if *kind == CounterKind::Experience {
                        ctx.state
                            .experience
                            .get(&opp)
                            .cloned()
                            .unwrap_or_else(|| 0.into())
                    } else {
                        ctx.state
                            .player_counters
                            .get(kind)
                            .and_then(|m| m.get(&opp))
                            .cloned()
                            .unwrap_or_else(|| 0.into())
                    }
                })
                .max()
                .unwrap_or_else(|| 0.into()),
            PlayerStatExpr::OpponentStatThisTurn(stat) => {
                use crate::program::OpponentTurnStat;
                let any_opp = opponents().any(|opp| match stat {
                    OpponentTurnStat::Discarded => {
                        ctx.state
                            .cards_discarded_this_turn
                            .get(&opp)
                            .copied()
                            .unwrap_or(0)
                            > 0
                    }
                    OpponentTurnStat::Sacrificed => {
                        ctx.state
                            .permanents_sacrificed_this_turn
                            .get(&opp)
                            .copied()
                            .unwrap_or(0)
                            > 0
                    }
                    OpponentTurnStat::CastSpells => {
                        ctx.state
                            .spells_cast_this_turn
                            .get(&opp)
                            .copied()
                            .unwrap_or(0)
                            > 0
                    }
                    OpponentTurnStat::DrewCards => {
                        ctx.state
                            .cards_drawn_this_turn_counts
                            .get(&opp)
                            .copied()
                            .unwrap_or(0)
                            > 0
                    }
                    OpponentTurnStat::Attacked => {
                        ctx.state.attacked_this_turn_players.contains(&opp)
                    }
                    OpponentTurnStat::GainedLife => ctx
                        .state
                        .life_gained_this_turn
                        .get(&opp)
                        .is_some_and(RulesInteger::is_positive),
                    OpponentTurnStat::LostLife => ctx
                        .state
                        .life_lost_this_turn
                        .get(&opp)
                        .is_some_and(RulesInteger::is_positive),
                    OpponentTurnStat::PlayedLand => {
                        ctx.state
                            .lands_played_this_turn
                            .get(&opp)
                            .copied()
                            .unwrap_or(0)
                            > 0
                    }
                    OpponentTurnStat::CreatureEntered => {
                        ctx.state
                            .creatures_entered_this_turn
                            .get(&opp)
                            .copied()
                            .unwrap_or(0)
                            > 0
                    }
                    OpponentTurnStat::PermanentLeft => {
                        ctx.state
                            .permanents_left_battlefield_this_turn
                            .get(&opp)
                            .copied()
                            .unwrap_or(0)
                            > 0
                    }
                });
                i64::from(any_opp).into()
            }
            PlayerStatExpr::DayNightState => match ctx.cards.day_night {
                crate::day_night::DayNight::Neither => 0.into(),
                crate::day_night::DayNight::Day => 1.into(),
                crate::day_night::DayNight::Night => 2.into(),
            },
            PlayerStatExpr::MinLibrarySize => RulesInteger::from_usize(
                ctx.participants
                    .iter()
                    .map(|&p| ctx.cards.library(p).len())
                    .min()
                    .unwrap_or(0),
            ),
            PlayerStatExpr::OpponentsLifeLostThisTurn => ctx
                .state
                .life_lost_this_turn
                .iter()
                .filter(|(p, _)| **p != ctx.controller)
                .fold(0.into(), |acc: RulesInteger, (_, v)| acc.plus(v)),
            PlayerStatExpr::CardsDrawnThisTurn => i64::from(
                ctx.state
                    .cards_drawn_this_turn_counts
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::LargestHandSize => {
                let mut sizes: BTreeMap<SeatId, usize> = BTreeMap::new();
                for o in ctx.cards.iter() {
                    if let ObjectZone::Hand(p) = o.zone() {
                        *sizes.entry(p).or_insert(0) += 1;
                    }
                }
                RulesInteger::from_usize(sizes.into_values().max().unwrap_or(0))
            }
            PlayerStatExpr::ControllerIsMonarch => {
                i64::from(ctx.state.monarch == Some(ctx.controller)).into()
            }
            PlayerStatExpr::OpponentIsMonarch => {
                i64::from(ctx.state.monarch.is_some_and(|m| m != ctx.controller)).into()
            }
            PlayerStatExpr::NoMonarch => i64::from(ctx.state.monarch.is_none()).into(),
            PlayerStatExpr::ControlsGreatestCreature => {
                let max = ctx
                    .objects
                    .values()
                    .filter(|p| p.zone == ObjectZone::Battlefield && p.is_creature())
                    .filter_map(|p| {
                        p.power_toughness
                            .as_ref()
                            .map(|pt| (&pt.derived.power, p.controller == Some(ctx.controller)))
                    })
                    .max();
                i64::from(max.is_some_and(|(_, ours)| ours)).into()
            }
            PlayerStatExpr::ControllerIsActivePlayer => {
                i64::from(ctx.active_player == ctx.controller).into()
            }
            PlayerStatExpr::ControllerIsAttacking => i64::from(
                ctx.active_player == ctx.controller
                    && ctx.combat.is_some_and(|c| c.attackers().next().is_some()),
            )
            .into(),
            PlayerStatExpr::ControllerIsDefending => i64::from(
                ctx.combat
                    .is_some_and(|c| c.attackers().any(|a| a.defender() == ctx.controller)),
            )
            .into(),
            PlayerStatExpr::ControllerSpeed => {
                i64::from(ctx.state.speed.get(&ctx.controller).copied().unwrap_or(0)).into()
            }
            PlayerStatExpr::OwnGraveyardDistinctCardTypes => {
                let mut types = BTreeSet::new();
                for o in ctx
                    .objects
                    .values()
                    .filter(|c| c.zone == ObjectZone::Graveyard(ctx.controller))
                {
                    types.extend(o.current_types.iter().copied());
                }
                RulesInteger::from_usize(types.len())
            }
            PlayerStatExpr::AllGraveyardsDistinctCardTypes => {
                let mut types = BTreeSet::new();
                for o in ctx
                    .objects
                    .values()
                    .filter(|c| matches!(c.zone, ObjectZone::Graveyard(_)))
                {
                    types.extend(o.current_types.iter().copied());
                }
                RulesInteger::from_usize(types.len())
            }
            PlayerStatExpr::ControllerLifeLostThisTurn => ctx
                .state
                .life_lost_this_turn
                .get(&ctx.controller)
                .cloned()
                .unwrap_or_else(|| 0.into()),
            PlayerStatExpr::ControllerLifeGainedThisTurn => ctx
                .state
                .life_gained_this_turn
                .get(&ctx.controller)
                .cloned()
                .unwrap_or_else(|| 0.into()),
            PlayerStatExpr::ControllerSpellsCastThisTurn => i64::from(
                ctx.state
                    .spells_cast_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::CastSpellTypeThisTurn(kind) => i64::from(
                ctx.state
                    .cast_spell_types_this_turn
                    .get(&ctx.controller)
                    .and_then(|m| m.get(kind))
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::CastNoncreatureSpellsThisTurn => i64::from(
                ctx.state
                    .noncreature_spells_cast_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::CastSpellsFromHandThisTurn => i64::from(
                ctx.state
                    .spells_cast_from_hand_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::ControllerAttackedThisTurn => i64::from(
                ctx.state
                    .attacked_this_turn_players
                    .contains(&ctx.controller),
            )
            .into(),
            PlayerStatExpr::AttackingCreaturesThisTurn => RulesInteger::from_usize(
                ctx.state
                    .attacking_creatures_this_turn
                    .get(&ctx.controller)
                    .map_or(0, BTreeSet::len),
            ),
            PlayerStatExpr::ControllerDiscardedThisTurn => i64::from(
                ctx.state
                    .cards_discarded_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::ControllerSacrificedThisTurn => i64::from(
                ctx.state
                    .permanents_sacrificed_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::PermanentLeftBattlefieldThisTurn => i64::from(
                ctx.state
                    .permanents_left_battlefield_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::NonlandPermanentLeftBattlefieldThisTurn => i64::from(
                ctx.state
                    .nonland_permanents_left_battlefield_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::EnteredPermanentThisTurn => i64::from(
                ctx.state
                    .permanents_entered_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::EnteredTypeThisTurn(kind) => i64::from(
                ctx.state
                    .entered_types_this_turn
                    .get(&ctx.controller)
                    .and_then(|m| m.get(kind))
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::CreatureEnteredUnderControlThisTurn => i64::from(
                ctx.state
                    .creatures_entered_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::PlayedLandThisTurn => i64::from(
                ctx.state
                    .lands_played_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::CardsLeftGraveyardThisTurn => i64::from(
                ctx.state
                    .cards_left_graveyard_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::CreaturesDiedThisTurn => {
                i64::from(ctx.state.creatures_died_this_turn_count).into()
            }
            PlayerStatExpr::ControlledCreatureDiedThisTurn => i64::from(
                ctx.state
                    .controlled_creatures_died_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::CreatedTokensThisTurn => i64::from(
                ctx.state
                    .tokens_created_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::OpponentDealtDamageThisTurn { noncombat_only } => {
                let set = if *noncombat_only {
                    &ctx.state.players_dealt_noncombat_damage_this_turn
                } else {
                    &ctx.state.players_dealt_damage_this_turn
                };
                i64::from(opponents().any(|opp| set.contains(&opp))).into()
            }
            PlayerStatExpr::DescendedThisTurn => {
                i64::from(ctx.state.descended_this_turn.contains(&ctx.controller)).into()
            }
            PlayerStatExpr::CommittedCrimeThisTurn => i64::from(
                ctx.state
                    .committed_crime_this_turn
                    .contains(&ctx.controller),
            )
            .into(),
            PlayerStatExpr::ControllerHasCityBlessing => {
                i64::from(ctx.state.city_blessing.contains(&ctx.controller)).into()
            }
            PlayerStatExpr::PlayerLife(role) => resolve_world_player(ctx, *role)
                .and_then(|seat| ctx.life?.get(seat).cloned())
                .unwrap_or_else(|| 0.into()),
            PlayerStatExpr::PlayerHandSize(role) => resolve_world_player(ctx, *role).map_or_else(
                || 0.into(),
                |seat| {
                    RulesInteger::from_usize(
                        ctx.cards
                            .iter()
                            .filter(|o| o.zone() == ObjectZone::Hand(seat))
                            .count(),
                    )
                },
            ),
            PlayerStatExpr::PlayerLibrarySize(role) => resolve_world_player(ctx, *role)
                .map_or_else(
                    || 0.into(),
                    |seat| RulesInteger::from_usize(ctx.cards.library(seat).len()),
                ),
            PlayerStatExpr::PlayerGraveyardSize(role) => resolve_world_player(ctx, *role)
                .map_or_else(
                    || 0.into(),
                    |seat| RulesInteger::from_usize(ctx.cards.graveyard(seat).len()),
                ),
            PlayerStatExpr::AnyOpponentPoisoned => i64::from(opponents().any(|opp| {
                ctx.state
                    .player_counters
                    .get(&CounterKind::Poison)
                    .and_then(|m| m.get(&opp))
                    .is_some_and(RulesInteger::is_positive)
            }))
            .into(),
            PlayerStatExpr::CardsShortOfHandSize(_) => 0.into(),
            PlayerStatExpr::LoyaltyActivatedThisTurn => {
                i64::from(ctx.state.loyalty_activators.contains(&ctx.controller)).into()
            }
            PlayerStatExpr::IsOwnMainPhase => {
                i64::from(ctx.active_player == ctx.controller && ctx.combat.is_none()).into()
            }
            PlayerStatExpr::ExhaustActivatedThisTurn => i64::from(
                ctx.state
                    .exhausted
                    .iter()
                    .any(|(obj, _, _, _)| *obj == ctx.source),
            )
            .into(),
            PlayerStatExpr::CompletedDungeons => i64::from(
                ctx.state
                    .dungeons
                    .get(&ctx.controller)
                    .map_or(0, |d| d.completed),
            )
            .into(),
            PlayerStatExpr::RingLevel => i64::from(
                ctx.state
                    .ring_level
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::HasInitiative => {
                i64::from(ctx.state.initiative == Some(ctx.controller)).into()
            }
            PlayerStatExpr::CombatPhasesThisTurn => {
                i64::from(ctx.state.combat_phases_this_turn).into()
            }
            PlayerStatExpr::CardsEnteredGraveyardThisTurn => i64::from(
                ctx.state
                    .cards_entered_graveyard_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::CountersPlacedOnCreaturesThisTurn => i64::from(
                ctx.state
                    .counters_placed_on_creatures_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::LandsEnteredThisTurn(role) => resolve_world_player(ctx, *role)
                .map_or_else(
                    || 0.into(),
                    |seat| {
                        i64::from(
                            ctx.state
                                .lands_entered_this_turn
                                .get(&seat)
                                .copied()
                                .unwrap_or(0),
                        )
                        .into()
                    },
                ),
            PlayerStatExpr::DiceRolledThisTurn => i64::from(
                ctx.state
                    .dice_rolled_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0),
            )
            .into(),
            PlayerStatExpr::OtherSpellsCastManaValueThisTurn => {
                let total = ctx
                    .state
                    .other_spells_mv_this_turn
                    .get(&ctx.controller)
                    .copied()
                    .unwrap_or(0);
                let own_mv = ctx
                    .objects
                    .get(&ctx.source)
                    .filter(|o| o.zone == ObjectZone::Stack && o.controller == Some(ctx.controller))
                    .and_then(|o| object_mana_value(ctx, o))
                    .and_then(|mv| mv.to_usize())
                    .map_or(0u32, |n| u32::try_from(n).unwrap_or(u32::MAX));
                i64::from(total.saturating_sub(own_mv)).into()
            }
        },
    }
}
