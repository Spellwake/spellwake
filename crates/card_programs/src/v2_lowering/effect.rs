//! Strictly-typed `v2::ir` effect sequence and control-flow lowering.
//!
//! Lowers all 25 `v2::Effect` variants into `spellwake_engine::program::ExecutableBody`,
//! including structured branching (`BranchCondition`, `Jump`), `IfPrevious` binding checks,
//! `OptionalSequence`, `EmitReflexiveTrigger`, `Unless`, `ForEach`, `BindVariable`,
//! `DelayedTrigger`, `ScopedDuration`, `ExchangeControl`, and `FixedTargetClause`
//! decomposition when sequential effects require distinct targets.

use std::collections::BTreeSet;
use std::mem;

use spellwake_engine::{
    characteristics::{
        AbilityChange, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        StaticDamageRecipient,
    },
    program::{
        AffectedPlayers, BodyTarget, DamageAmountSpec, ExecutableBody, Instruction,
        ObjectFilter as EngineObjectFilter, ResolvingPlayer, TargetZone, TemporaryRuleKind,
        ValueExpr as EngineValueExpr,
    },
    toolbox::{
        AdjustLifeSpec, ApplyContinuousSpec, BindingId, ChoiceCategory,
        ConditionExpr as EngineConditionExpr, ControlOp, DamageSourceQuery, DamageTargetQuery,
        DealDamageSpec, ExecValue, GraveyardSemantics, LifeOp as EngineLifeOp, MutateObjectSpec,
        MutatePlayerSpec, ObjectMutation as EngineObjectMutation, ObjectQuery,
        PlayerMutation as EnginePlayerMutation, PlayerSelector, PromptChoiceSpec, PromptDomain,
        QueryZone, SubtypeChoiceKind, TagKey, TagValue, ZoneDest as EngineZoneDest, ZoneOpSpec,
    },
    turn::Step,
};
use spellwake_oracle_compiler::{ast, v2::ir as v2};

use crate::v2_lowering::{
    atoms, condition,
    ctx::{BodyAcc, LowerCtx},
    error::{LowerResult, LoweringError},
    filter, mutation, replacement, static_ability, token, trigger, value, zone_op,
};

/// Lower a sequence of `v2::Effect` instructions into an existing [`BodyAcc`].
///
/// Handles multi-target clauses automatically by promoting distinct-target sequential
/// effects into `Instruction::FixedTargetClause` blocks when `acc.register_target`
/// signals `v2:multiple-distinct-targets`.
pub(crate) fn lower_effects_into(
    effects: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let mut i = 0;
    while i < effects.len() {
        // 1. Fuse `[base_effect, BranchCondition { cond, then: [ReplacePrevious(alt), ..], else: [] }]`.
        if let Some((rewritten, next_i)) = try_fuse_replace_previous(effects, i) {
            lower_step_with_target_dispatch(&rewritten, acc, ctx)?;
            i = next_i;
            continue;
        }

        // 2. Fuse `[Destroy { can_regenerate: true }, MutateObject(Anaphora, CantBeRegenerated)]`.
        if let Some((fused, next_i)) = try_fuse_destroy_no_regen(effects, i) {
            lower_step_with_target_dispatch(&fused, acc, ctx)?;
            i = next_i;
            continue;
        }

        // 3. Fuse `[MutateObject(GrantKeywords([Flashback])), MutatePlayer(FlashbackCostEqualsManaCost)]`.
        if is_flashback_grant_plus_cost_clause(effects, i) {
            lower_step_with_target_dispatch(&effects[i], acc, ctx)?;
            i += 2;
            continue;
        }

        // 4. Fuse `[SearchLibrary { shuffle: false }, (Reveal)?, (Move|Exile)?, Shuffle, (Move to Top)?]`.
        if let Some((fused, next_i)) = try_fuse_search_library_sequence(effects, i) {
            lower_step_with_target_dispatch(&fused, acc, ctx)?;
            i = next_i;
            continue;
        }

        // 5. Fuse `Optional` with inline mana payment or follow-up `IfPrevious` / `ReflexiveTrigger`.
        if let Some(next_i) = try_lower_optional_sequence(effects, i, acc, ctx)? {
            i = next_i;
            continue;
        }

        // 6. Fuse `[BranchCondition { condition, then_body, else_body: [] }, IfPrevious { succeeded: false, body }]`
        //    into a single two-armed `BranchCondition`.
        if let v2::Effect::BranchCondition {
            condition,
            then_body,
            else_body,
        } = &effects[i]
            && else_body.is_empty()
            && let Some(v2::Effect::IfPrevious {
                succeeded: false,
                body: otherwise_body,
            }) = effects.get(i + 1)
        {
            let fused = v2::Effect::BranchCondition {
                condition: condition.clone(),
                then_body: then_body.clone(),
                else_body: otherwise_body.clone(),
            };
            lower_step_with_target_dispatch(&fused, acc, ctx)?;
            i += 2;
            continue;
        }

        lower_step_with_target_dispatch(&effects[i], acc, ctx)?;
        i += 1;
    }
    normalize_body_target_bounds(acc);
    Ok(())
}

/// Normalize target bounds so `x_target_count`, `up_to_x_target_count`, and graveyard
/// `up_to_one` satisfy `ExecutableBody::validate_in_with_facts` invariants.
fn normalize_body_target_bounds(acc: &mut BodyAcc) {
    if (acc.x_target_count || acc.up_to_x_target_count) && acc.target_bounds.is_none() {
        let supports_bounds = matches!(
            acc.target,
            BodyTarget::Object {
                zone: TargetZone::Battlefield
                    | TargetZone::OwnGraveyard
                    | TargetZone::AnyGraveyard { .. },
                ..
            }
        );
        if supports_bounds {
            acc.target_bounds = Some((0, u16::MAX));
        }
    }
    if acc.up_to_one
        && acc.target_bounds.is_none()
        && matches!(
            acc.target,
            BodyTarget::Object {
                zone: TargetZone::OwnGraveyard | TargetZone::AnyGraveyard { .. },
                ..
            }
        )
        && acc.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::ZoneOp(spec)
                    if matches!(spec.source, ObjectQuery::Targets)
                        && matches!(
                            spec.destination,
                            EngineZoneDest::Battlefield { .. }
                                | EngineZoneDest::Hand
                                | EngineZoneDest::Exile { .. }
                                | EngineZoneDest::Library { .. }
                        )
            )
        })
    {
        acc.up_to_one = false;
        acc.target_bounds = Some((0, 1));
    }
}

/// Register any `TargetOpponent` or `TargetPlayer` reference embedded inside a `v2::ValueExpr`.
pub(crate) fn register_value_expr_player_targets(
    expr: &v2::ValueExpr,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match expr {
        v2::ValueExpr::Count(f)
        | v2::ValueExpr::AggregateStat { filter: f, .. }
        | v2::ValueExpr::ManaSpentFromFilter(f)
        | v2::ValueExpr::DistinctCounterKinds(f)
        | v2::ValueExpr::DistinctCardTypesAmong(f) => {
            register_filter_player_targets(f, acc, ctx)
        }
        v2::ValueExpr::PlayerCounterCount { player, .. }
        | v2::ValueExpr::DomainCount(player)
        | v2::ValueExpr::PartySize(player)
        | v2::ValueExpr::PlayerStat { player, .. }
        | v2::ValueExpr::Speed(player)
        | v2::ValueExpr::CommanderCastCount(player)
        | v2::ValueExpr::DamageDealtToPlayerThisTurn(player) => {
            register_player_ref_target(*player, acc, ctx)
        }
        v2::ValueExpr::EventCountThisTurn {
            player: Some(p), ..
        } => register_player_ref_target(*p, acc, ctx),
        v2::ValueExpr::DistinctCardTypes(z) | v2::ValueExpr::DistinctManaValues(z) => {
            register_zone_owner_target(z.owner, acc, ctx)
        }
        v2::ValueExpr::CounterCount { subject, .. }
        | v2::ValueExpr::ColorCount(subject)
        | v2::ValueExpr::ObjectStat { subject, .. }
        | v2::ValueExpr::StatDifference(subject) => {
            register_subject_filter_player_targets(subject, acc, ctx)
        }
        v2::ValueExpr::Add(a, b)
        | v2::ValueExpr::Sub(a, b)
        | v2::ValueExpr::Mul(a, b) => {
            register_value_expr_player_targets(a, acc, ctx)?;
            register_value_expr_player_targets(b, acc, ctx)
        }
        v2::ValueExpr::Div { expr: inner, .. } => {
            register_value_expr_player_targets(inner, acc, ctx)
        }
        _ => Ok(()),
    }
}

/// Register any `TargetOpponent` or `TargetPlayer` embedded inside a non-targeting `ObjectFilter`.
pub(crate) fn register_filter_player_targets(
    f: &v2::ObjectFilter,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if let Some(rel) = f.controller {
        register_controller_rel_target(rel, acc, ctx)?;
    }
    if let Some(rel) = f.owner {
        register_controller_rel_target(rel, acc, ctx)?;
    }
    for z in &f.zones {
        register_zone_owner_target(z.owner, acc, ctx)?;
    }
    for sb in &f.stat_bounds {
        register_value_expr_player_targets(&sb.value, acc, ctx)?;
    }
    for pred in &f.predicates {
        match pred {
            v2::FilterPredicate::AnyOf(subs) => {
                for sub in subs {
                    register_filter_player_targets(sub, acc, ctx)?;
                }
            }
            v2::FilterPredicate::Exclude(subj) => {
                register_subject_filter_player_targets(subj, acc, ctx)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Register player targets embedded inside non-targeting group/counted subjects or player subjects.
pub(crate) fn register_subject_filter_player_targets(
    subject: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match subject {
        v2::SubjectRef::Each(f)
        | v2::SubjectRef::All(f)
        | v2::SubjectRef::Group(f)
        | v2::SubjectRef::Counted { filter: f, .. } => {
            register_filter_player_targets(f, acc, ctx)
        }
        v2::SubjectRef::Player(p) | v2::SubjectRef::HandOf(p) => {
            register_player_ref_target(*p, acc, ctx)
        }
        v2::SubjectRef::Compound(parts) => {
            for part in parts {
                if !matches!(
                    part,
                    v2::SubjectRef::TargetObject { .. }
                        | v2::SubjectRef::AnyTarget
                        | v2::SubjectRef::TargetPlayer { .. }
                ) {
                    register_subject_filter_player_targets(part, acc, ctx)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn register_player_ref_target(
    player: v2::PlayerRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match player {
        v2::PlayerRef::TargetOpponent => {
            acc.register_target(BodyTarget::SINGLE_OPPONENT, v2::TargetCount::Exact(1))?;
            ctx.last_player_target = Some(ResolvingPlayer::TargetPlayer);
            Ok(())
        }
        v2::PlayerRef::TargetPlayer => {
            acc.register_target(BodyTarget::SINGLE_PLAYER, v2::TargetCount::Exact(1))?;
            ctx.last_player_target = Some(ResolvingPlayer::TargetPlayer);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn register_zone_owner_target(
    owner: v2::ZoneOwner,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match owner {
        v2::ZoneOwner::TargetOpponent => {
            acc.register_target(BodyTarget::SINGLE_OPPONENT, v2::TargetCount::Exact(1))?;
            ctx.last_player_target = Some(ResolvingPlayer::TargetPlayer);
            Ok(())
        }
        v2::ZoneOwner::TargetPlayer => {
            acc.register_target(BodyTarget::SINGLE_PLAYER, v2::TargetCount::Exact(1))?;
            ctx.last_player_target = Some(ResolvingPlayer::TargetPlayer);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn register_controller_rel_target(
    rel: ast::ControllerRelation,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match rel {
        ast::ControllerRelation::TargetOpponent => {
            acc.register_target(BodyTarget::SINGLE_OPPONENT, v2::TargetCount::Exact(1))?;
            ctx.last_player_target = Some(ResolvingPlayer::TargetPlayer);
            Ok(())
        }
        ast::ControllerRelation::TargetPlayer => {
            acc.register_target(BodyTarget::SINGLE_PLAYER, v2::TargetCount::Exact(1))?;
            ctx.last_player_target = Some(ResolvingPlayer::TargetPlayer);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn lower_step_with_target_dispatch(
    eff: &v2::Effect,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let already_fixed_clauses = acc
        .instructions
        .iter()
        .any(|ins| matches!(ins, Instruction::FixedTargetClause(_)));

    if already_fixed_clauses {
        let saved_ctx = CtxFlowSnapshot::capture(ctx);
        let mut sub_acc = BodyAcc::new();
        match lower_one_effect(eff, &mut sub_acc, ctx) {
            Ok(()) => {
                if sub_acc.instructions.is_empty() {
                    return Ok(());
                }
                if sub_acc.target_set {
                    let sub_body = sub_acc.into_body()?;
                    if sub_body.permanent_target_bounds().is_some()
                        || !sub_body.instructions.iter().any(Instruction::uses_mode_target)
                    {
                        return Err(LoweringError::Unsupported("v2:multiple-distinct-targets"));
                    }
                    acc.push(Instruction::FixedTargetClause(Box::new(sub_body)));
                } else {
                    splice_untargeted_instructions(acc, sub_acc.instructions);
                }
                return Ok(());
            }
            Err(err @ LoweringError::Unsupported(
                "v2:subject:unbound-anaphora"
                | "v2:subject:unbound-same-target"
                | "v2:player:unbound-that-player"
                | "v2:player:unbound-its-controller"
                | "v2:player:unbound-its-owner"
                | "v2:player:unbound-spell-controller",
            )) => {
                saved_ctx.restore(ctx);
                if let Some(Instruction::FixedTargetClause(last_body)) = acc.instructions.last_mut()
                {
                    let mut clause_acc = BodyAcc::new();
                    clause_acc.target = last_body.target;
                    clause_acc.target_set = true;
                    clause_acc.up_to_one = last_body.is_up_to_one_target();
                    clause_acc.instructions = last_body.instructions.clone();
                    if lower_one_effect(eff, &mut clause_acc, ctx).is_ok()
                        && let Ok(updated) = clause_acc.into_body()
                        && updated.permanent_target_bounds().is_none()
                        && updated.instructions.iter().any(Instruction::uses_mode_target)
                    {
                        **last_body = updated;
                        return Ok(());
                    }
                    saved_ctx.restore(ctx);
                }
                return Err(err);
            }
            Err(err) => return Err(err),
        }
    }

    if !acc.target_set {
        return lower_one_effect(eff, acc, ctx);
    }

    let saved_acc = acc.clone();
    let saved_ctx = CtxFlowSnapshot::capture(ctx);
    match lower_one_effect(eff, acc, ctx) {
        Ok(()) => Ok(()),
        Err(LoweringError::Unsupported("v2:multiple-distinct-targets")) => {
            *acc = saved_acc;
            saved_ctx.restore(ctx);

            // Handle follow-up `It fights target ...` or `It deals damage equal to its power to target ...`.
            if try_promote_anaphoric_two_target_followup(eff, acc, ctx)? {
                return Ok(());
            }

            let mut sub_acc = BodyAcc::new();
            lower_one_effect(eff, &mut sub_acc, ctx)?;
            if !sub_acc.target_set || sub_acc.instructions.is_empty() {
                return Err(LoweringError::Unsupported("v2:multiple-distinct-targets"));
            }
            let prior_body = mem::take(acc).into_body()?;
            let next_body = sub_acc.into_body()?;
            if prior_body.permanent_target_bounds().is_some()
                || next_body.permanent_target_bounds().is_some()
                || !prior_body.instructions.iter().any(Instruction::uses_mode_target)
                || !next_body.instructions.iter().any(Instruction::uses_mode_target)
            {
                return Err(LoweringError::Unsupported("v2:multiple-distinct-targets"));
            }
            acc.push(Instruction::FixedTargetClause(Box::new(prior_body)));
            acc.push(Instruction::FixedTargetClause(Box::new(next_body)));
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn try_promote_anaphoric_two_target_followup(
    eff: &v2::Effect,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<bool> {
    if !acc.target_set
        || acc.target_bounds.is_some()
        || acc.x_target_count
        || acc.up_to_x_target_count
    {
        return Ok(false);
    }
    match eff {
        v2::Effect::MutateObject {
            subject,
            mutation: v2::ObjectMutation::Fight(other),
            duration,
        } if matches!(duration, v2::Duration::Instant | v2::Duration::Permanent)
            && is_prior_target_anaphora(subject)
            && matches!(other.as_ref(), v2::SubjectRef::TargetObject { .. }) =>
        {
            if !matches!(
                acc.target,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            ) {
                return Ok(false);
            }
            let (bt2, tc2) = atoms::lower_subject_to_target(other, ctx)?;
            if !matches!(
                bt2,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            ) || !matches!(tc2, v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1))
            {
                return Ok(false);
            }
            prepend_remember_permanent_target(acc);
            let prior_body = mem::take(acc).into_body()?;
            let mut second_acc = BodyAcc::new();
            second_acc.register_target(bt2, tc2)?;
            second_acc.push(Instruction::MutateObject(Box::new(
                MutateObjectSpec::remember_permanent_target(),
            )));
            let second_body = second_acc.into_body()?;
            acc.push(Instruction::FixedTargetClause(Box::new(prior_body)));
            acc.push(Instruction::FixedTargetClause(Box::new(second_body)));
            acc.push(Instruction::DealDamage(Box::new(DealDamageSpec::fight(
                ObjectQuery::LinkedTarget { index: 0 },
                ObjectQuery::LinkedTarget { index: 1 },
            ))));
            Ok(true)
        }
        v2::Effect::DealDamage {
            source,
            target,
            amount,
            divided_by: None,
        } if is_prior_target_anaphora(source)
            && is_power_of_source_expr(amount, source)
            && acc.target.guarantees_creature() =>
        {
            let (bt2, tc2) = atoms::lower_subject_to_target(target, ctx)?;
            let valid_recip = matches!(
                bt2,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                } | BodyTarget::Player { .. }
                    | BodyTarget::AnyTarget(_)
            );
            if !valid_recip || !matches!(tc2, v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1))
            {
                return Ok(false);
            }
            prepend_remember_permanent_target(acc);
            let prior_body = mem::take(acc).into_body()?;
            let mut second_acc = BodyAcc::new();
            second_acc.register_target(bt2, tc2)?;
            second_acc.push(Instruction::MutateObject(Box::new(
                MutateObjectSpec::remember_permanent_target(),
            )));
            let second_body = second_acc.into_body()?;
            acc.push(Instruction::FixedTargetClause(Box::new(prior_body)));
            acc.push(Instruction::FixedTargetClause(Box::new(second_body)));
            acc.push(Instruction::DealDamage(Box::new(
                DealDamageSpec::creature_power_to_other(false),
            )));
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn prepend_remember_permanent_target(acc: &mut BodyAcc) {
    for ins in &mut acc.instructions {
        ins.shift_jumps(1, 0);
    }
    acc.instructions.insert(
        0,
        Instruction::MutateObject(Box::new(MutateObjectSpec::remember_permanent_target())),
    );
}

fn is_prior_target_anaphora(subj: &v2::SubjectRef) -> bool {
    matches!(
        subj,
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::It
                | v2::AnaphoraRef::ThatCreature
                | v2::AnaphoraRef::ThatPermanent
                | v2::AnaphoraRef::SameTarget
        )
    )
}

fn is_power_of_source_expr(amount: &v2::ValueExpr, source: &v2::SubjectRef) -> bool {
    matches!(
        amount,
        v2::ValueExpr::ObjectStat {
            subject,
            stat: ast::StatKind::Power,
        } if is_prior_target_anaphora(subject) || subject.as_ref() == source
    )
}

fn try_fuse_replace_previous(effects: &[v2::Effect], i: usize) -> Option<(v2::Effect, usize)> {
    let mut j = i + 1;
    let mut current = effects[i].clone();
    let mut fused = false;
    while let Some(v2::Effect::BranchCondition {
        condition,
        then_body,
        else_body,
    }) = effects.get(j)
    {
        if !else_body.is_empty() {
            break;
        }
        let Some(v2::Effect::ReplacePrevious(alt_effects)) = then_body.first() else {
            break;
        };
        let mut merged_then = alt_effects.clone();
        merged_then.extend_from_slice(&then_body[1..]);
        current = v2::Effect::BranchCondition {
            condition: condition.clone(),
            then_body: merged_then,
            else_body: vec![current],
        };
        fused = true;
        j += 1;
    }
    fused.then_some((current, j))
}

fn try_fuse_destroy_no_regen(effects: &[v2::Effect], i: usize) -> Option<(v2::Effect, usize)> {
    let v2::Effect::ZoneOp {
        actor,
        op: v2::ZoneOpKind::Destroy {
            can_regenerate: true,
        },
        subject,
        from,
        to,
        modifiers,
        except,
        count,
    } = &effects[i]
    else {
        return None;
    };
    let Some(v2::Effect::MutateObject {
        subject: mut_subj,
        mutation: v2::ObjectMutation::CantBeRegenerated,
        duration,
    }) = effects.get(i + 1)
    else {
        return None;
    };
    if !matches!(duration, v2::Duration::Instant | v2::Duration::UntilEndOfTurn) {
        return None;
    }
    let matches_anaphora = matches!(
        mut_subj,
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::It
                | v2::AnaphoraRef::Them
                | v2::AnaphoraRef::ThatCreature
                | v2::AnaphoraRef::ThatPermanent
                | v2::AnaphoraRef::ThoseCreatures
                | v2::AnaphoraRef::ThosePermanents
                | v2::AnaphoraRef::SameTarget
        ) | v2::SubjectRef::Player(v2::PlayerRef::ThatPlayer)
    );
    if !matches_anaphora {
        return None;
    }
    Some((
        v2::Effect::ZoneOp {
            actor: *actor,
            op: v2::ZoneOpKind::Destroy {
                can_regenerate: false,
            },
            subject: subject.clone(),
            from: from.clone(),
            to: to.clone(),
            modifiers: modifiers.clone(),
            except: except.clone(),
            count: count.clone(),
        },
        i + 2,
    ))
}

fn is_flashback_grant_plus_cost_clause(effects: &[v2::Effect], i: usize) -> bool {
    let v2::Effect::MutateObject {
        mutation: v2::ObjectMutation::GrantKeywords(kws),
        duration,
        ..
    } = &effects[i]
    else {
        return false;
    };
    if kws.len() != 1
        || kws[0].kind != v2::KeywordKind::Flashback
        || kws[0].param.is_some()
        || !matches!(duration, v2::Duration::Instant | v2::Duration::UntilEndOfTurn)
    {
        return false;
    }
    matches!(
        effects.get(i + 1),
        Some(v2::Effect::MutatePlayer {
            mutation: v2::PlayerMutation::Action(v2::PlayerActionKind::FlashbackCostEqualsManaCost),
            ..
        })
    )
}

fn is_search_anaphora(subj: &v2::SubjectRef) -> bool {
    matches!(
        subj,
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::It
                | v2::AnaphoraRef::Them
                | v2::AnaphoraRef::ThatCard
                | v2::AnaphoraRef::ThoseCards
                | v2::AnaphoraRef::ThatLand
                | v2::AnaphoraRef::ThatCreature
                | v2::AnaphoraRef::ThatPermanent
                | v2::AnaphoraRef::ThoseCreatures
                | v2::AnaphoraRef::ThosePermanents
                | v2::AnaphoraRef::ThatObject
                | v2::AnaphoraRef::RevealedCard
        )
    )
}

fn is_empty_entry_modifiers(mods: &v2::EntryModifiers) -> bool {
    !mods.tapped
        && !mods.attacking
        && !mods.face_down
        && !mods.transformed
        && mods.with_counter.is_none()
        && mods.attached_to.is_none()
}

fn from_is_empty_or_library(from: &[v2::ZoneRef]) -> bool {
    from.iter()
        .all(|z| matches!(z.zone, v2::ZoneKind::Library | v2::ZoneKind::Graveyard))
}

fn is_pure_search_shuffle(eff: &v2::Effect) -> bool {
    matches!(
        eff,
        v2::Effect::ZoneOp {
            op: v2::ZoneOpKind::Shuffle,
            subject: v2::SubjectRef::Player(_) | v2::SubjectRef::TargetPlayer { .. },
            from,
            to,
            modifiers,
            except: None,
            count: None,
            ..
        } if from.is_empty()
            && to
                .as_ref()
                .is_none_or(|d| d.zone == v2::ZoneKind::Library)
            && is_empty_entry_modifiers(modifiers)
    )
}

fn is_conditional_search_shuffle(eff: &v2::Effect) -> bool {
    match eff {
        v2::Effect::IfPrevious {
            succeeded: true,
            body,
        } => body.len() == 1 && is_pure_search_shuffle(&body[0]),
        v2::Effect::BranchCondition {
            condition,
            then_body,
            else_body,
        } => {
            else_body.is_empty()
                && then_body.len() == 1
                && is_pure_search_shuffle(&then_body[0])
                && matches!(
                    condition,
                    v2::ConditionExpr::EventCheck(v2::EventCheckKind::PriorObjectEvent(
                        v2::PriorActionKind::SearchedThisWay
                    )) | v2::ConditionExpr::PriorActionMatched {
                        action: v2::PriorActionKind::SearchedThisWay,
                        ..
                    }
                )
        }
        _ => false,
    }
}

fn is_energy_only_mana_cost(cost: &v2::Cost) -> bool {
    matches!(
        cost,
        v2::Cost::Mana(mc)
            if !mc.symbols.is_empty()
                && mc
                    .symbols
                    .iter()
                    .all(|sym| matches!(sym, v2::ManaSymbol::Energy))
    )
}

struct SearchFusionState {
    merged_reveal: bool,
    merged_to: Option<v2::ZoneDest>,
    merged_mods: v2::EntryModifiers,
    seen_reveal: bool,
    seen_move: bool,
    found_shuffle: bool,
}

impl SearchFusionState {
    fn absorb_step(&mut self, eff: &v2::Effect) -> bool {
        match eff {
            v2::Effect::ZoneOp {
                op: v2::ZoneOpKind::Reveal,
                subject: r_subj,
                from: r_from,
                to: None,
                modifiers: r_mods,
                except: None,
                count: None,
                ..
            } if !self.seen_reveal
                && is_search_anaphora(r_subj)
                && from_is_empty_or_library(r_from)
                && is_empty_entry_modifiers(r_mods) =>
            {
                self.merged_reveal = true;
                self.seen_reveal = true;
                true
            }
            v2::Effect::ZoneOp {
                op: v2::ZoneOpKind::Move,
                subject: m_subj,
                from: m_from,
                to: Some(m_dest),
                modifiers: m_mods,
                except: None,
                count: None,
                ..
            } if !self.seen_move
                && self.merged_to.is_none()
                && is_search_anaphora(m_subj)
                && from_is_empty_or_library(m_from) =>
            {
                self.merged_to = Some(m_dest.clone());
                self.merged_mods = m_mods.clone();
                self.seen_move = true;
                true
            }
            v2::Effect::ZoneOp {
                op:
                    v2::ZoneOpKind::Exile {
                        face_down: false,
                        counters: None,
                    },
                subject: e_subj,
                from: e_from,
                to: e_to,
                modifiers: e_mods,
                except: None,
                count: None,
                ..
            } if !self.seen_move
                && self.merged_to.is_none()
                && is_search_anaphora(e_subj)
                && from_is_empty_or_library(e_from)
                && e_to
                    .as_ref()
                    .is_none_or(|d| d.zone == v2::ZoneKind::Exile) =>
            {
                self.merged_to = Some(v2::ZoneDest {
                    zone: v2::ZoneKind::Exile,
                    owner: v2::ZoneOwner::Default,
                    library_position: None,
                    library_order: None,
                    under_control: None,
                });
                self.merged_mods = e_mods.clone();
                self.seen_move = true;
                true
            }
            eff if is_pure_search_shuffle(eff) || is_conditional_search_shuffle(eff) => {
                self.found_shuffle = true;
                true
            }
            _ => false,
        }
    }

    fn try_absorb_post_shuffle_top(&mut self, eff: Option<&v2::Effect>) -> bool {
        if self.merged_to.is_none()
            && let Some(v2::Effect::ZoneOp {
                op: v2::ZoneOpKind::Move,
                subject: m_subj,
                from: m_from,
                to: Some(m_dest),
                modifiers: m_mods,
                except: None,
                count: None,
                ..
            }) = eff
            && is_search_anaphora(m_subj)
            && from_is_empty_or_library(m_from)
            && m_dest.zone == v2::ZoneKind::Library
            && matches!(
                m_dest.library_position,
                Some(v2::LibraryPosition::Top) | None
            )
        {
            self.merged_to = Some(m_dest.clone());
            self.merged_mods = m_mods.clone();
            true
        } else {
            false
        }
    }
}

fn try_fuse_search_library_sequence(
    effects: &[v2::Effect],
    i: usize,
) -> Option<(v2::Effect, usize)> {
    let (opt_actor, search_eff, inner_tail) = match &effects[i] {
        eff @ v2::Effect::ZoneOp {
            op: v2::ZoneOpKind::SearchLibrary { shuffle: false, .. },
            ..
        } => (None, eff, &[][..]),
        v2::Effect::Optional { actor, body } => {
            let first = body.first()?;
            if !matches!(
                first,
                v2::Effect::ZoneOp {
                    op: v2::ZoneOpKind::SearchLibrary { shuffle: false, .. },
                    ..
                }
            ) {
                return None;
            }
            (Some(*actor), first, &body[1..])
        }
        _ => return None,
    };

    let v2::Effect::ZoneOp {
        actor,
        op:
            v2::ZoneOpKind::SearchLibrary {
                reveal,
                shuffle: false,
                exact,
            },
        subject,
        from,
        to,
        modifiers,
        except,
        count,
    } = search_eff
    else {
        return None;
    };

    let mut state = SearchFusionState {
        merged_reveal: *reveal,
        merged_to: to.clone(),
        merged_mods: modifiers.clone(),
        seen_reveal: false,
        seen_move: false,
        found_shuffle: false,
    };

    // Absorb any reveal/move already nested inside `Optional { body }` after `SearchLibrary`.
    // If `inner_tail` already contains `Shuffle` or any unrelated effect, let `lower_optional`
    // handle `body` internally via its own `lower_effects_into` pass.
    for inner_eff in inner_tail {
        if !state.absorb_step(inner_eff) || state.found_shuffle {
            return None;
        }
    }

    let mut j = i + 1;

    // Also handle `Optional { [SearchLibrary, ...] }` followed by `IfPrevious { succeeded: true, body }`
    // where the entire `IfPrevious` body completes the search-reveal-move-shuffle sequence.
    if opt_actor.is_some()
        && let Some(v2::Effect::IfPrevious {
            succeeded: true,
            body: if_body,
        }) = effects.get(j)
    {
        let mut idx = 0;
        while idx < if_body.len() {
            if !state.absorb_step(&if_body[idx]) {
                break;
            }
            idx += 1;
            if state.found_shuffle {
                break;
            }
        }
        if state.found_shuffle {
            if state.try_absorb_post_shuffle_top(if_body.get(idx)) {
                idx += 1;
            }
            if idx == if_body.len() {
                j += 1;
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else {
        while j < effects.len() {
            if !state.absorb_step(&effects[j]) {
                break;
            }
            j += 1;
            if state.found_shuffle {
                break;
            }
        }

        if !state.found_shuffle {
            return None;
        }

        if state.try_absorb_post_shuffle_top(effects.get(j)) {
            j += 1;
        }
    }

    let fused_search = v2::Effect::ZoneOp {
        actor: *actor,
        op: v2::ZoneOpKind::SearchLibrary {
            reveal: state.merged_reveal,
            shuffle: true,
            exact: *exact,
        },
        subject: subject.clone(),
        from: from.clone(),
        to: state.merged_to,
        modifiers: state.merged_mods,
        except: except.clone(),
        count: count.clone(),
    };

    let out = match opt_actor {
        Some(opt_p) => v2::Effect::Optional {
            actor: opt_p,
            body: vec![fused_search],
        },
        None => fused_search,
    };
    Some((out, j))
}

fn try_lower_optional_sequence(
    effects: &[v2::Effect],
    i: usize,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<usize>> {
    let v2::Effect::Optional { actor, body } = &effects[i] else {
        return Ok(None);
    };

    // Sub-case 0: `Optional { actor: AnyPlayer | AnyOpponent, body }` with optional follow-up
    // `IfPrevious` branches lowers to `PlayerMutation::OfferAnyPlayer`.
    if matches!(
        *actor,
        v2::PlayerRef::AnyPlayer | v2::PlayerRef::AnyOpponent
    ) {
        let (paid_effs, declined_effs, next_i) = match effects.get(i + 1) {
            Some(v2::Effect::IfPrevious {
                succeeded: true,
                body: paid,
            }) => match effects.get(i + 2) {
                Some(v2::Effect::IfPrevious {
                    succeeded: false,
                    body: declined,
                }) => (Some(paid.as_slice()), Some(declined.as_slice()), i + 3),
                _ => (Some(paid.as_slice()), None, i + 2),
            },
            Some(v2::Effect::IfPrevious {
                succeeded: false,
                body: declined,
            }) => (None, Some(declined.as_slice()), i + 2),
            _ => (None, None, i + 1),
        };
        lower_offer_any_player(*actor, body, paid_effs, declined_effs, acc, ctx)?;
        return Ok(Some(next_i));
    }

    // Sub-case 1: `Optional` starts with `MutatePlayer(PayCost(Mana | ScaledMana))`.
    if let Some(v2::Effect::MutatePlayer {
        player,
        mutation: v2::PlayerMutation::PayCost(cost @ (v2::Cost::Mana(_) | v2::Cost::ScaledMana { .. })),
        duration,
    }) = body.first()
        && matches!(duration, v2::Duration::Instant | v2::Duration::Permanent)
        && !is_energy_only_mana_cost(cost)
    {
        let payer_ref = if *player == v2::PlayerRef::You {
            *actor
        } else {
            *player
        };
        if body.len() > 1 {
            lower_optional_mana_payment(
                payer_ref,
                cost,
                Some(&body[1..]),
                None,
                false,
                acc,
                ctx,
            )?;
            return Ok(Some(i + 1));
        }
        match effects.get(i + 1) {
            Some(v2::Effect::IfPrevious {
                succeeded: true,
                body: then_body,
            }) => {
                if let Some(v2::Effect::IfPrevious {
                    succeeded: false,
                    body: else_body,
                }) = effects.get(i + 2)
                {
                    lower_optional_mana_payment(
                        payer_ref,
                        cost,
                        Some(then_body),
                        Some(else_body),
                        false,
                        acc,
                        ctx,
                    )?;
                    return Ok(Some(i + 3));
                }
                lower_optional_mana_payment(
                    payer_ref,
                    cost,
                    Some(then_body),
                    None,
                    false,
                    acc,
                    ctx,
                )?;
                return Ok(Some(i + 2));
            }
            Some(v2::Effect::ReflexiveTrigger(then_body)) => {
                lower_optional_mana_payment(
                    payer_ref,
                    cost,
                    Some(then_body),
                    None,
                    true,
                    acc,
                    ctx,
                )?;
                return Ok(Some(i + 2));
            }
            Some(v2::Effect::IfPrevious {
                succeeded: false,
                body: else_body,
            }) => {
                lower_optional_mana_payment(
                    payer_ref,
                    cost,
                    None,
                    Some(else_body),
                    false,
                    acc,
                    ctx,
                )?;
                return Ok(Some(i + 2));
            }
            _ => {
                lower_optional_mana_payment(payer_ref, cost, None, None, false, acc, ctx)?;
                return Ok(Some(i + 1));
            }
        }
    }

    // Sub-case 2: non-mana `Optional` followed by `IfPrevious { succeeded: true, body }`.
    if let Some(v2::Effect::IfPrevious {
        succeeded: true,
        body: follow,
    }) = effects.get(i + 1)
    {
        lower_optional_with_if_previous(*actor, body, follow, acc, ctx)?;
        return Ok(Some(i + 2));
    }

    // Sub-case 3: non-mana `Optional` followed by `ReflexiveTrigger(follow)`.
    if let Some(v2::Effect::ReflexiveTrigger(follow)) = effects.get(i + 1) {
        lower_optional_with_reflexive(*actor, body, follow, acc, ctx)?;
        return Ok(Some(i + 2));
    }

    Ok(None)
}

fn lower_offer_any_player(
    actor: v2::PlayerRef,
    cost_effs: &[v2::Effect],
    paid_effs: Option<&[v2::Effect]>,
    declined_effs: Option<&[v2::Effect]>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if cost_effs.is_empty() {
        return Err(LoweringError::Unsupported("v2:optional-empty-body"));
    }
    let players = match actor {
        v2::PlayerRef::AnyPlayer => AffectedPlayers::EachPlayer,
        v2::PlayerRef::AnyOpponent => AffectedPlayers::EachOpponent,
        _ => return Err(LoweringError::Unsupported("v2:player:any-player")),
    };
    let rewritten_cost = rewrite_offer_cost_effects(actor, cost_effs);
    let cost_body = lower_offer_sub_body(&rewritten_cost, true, acc, ctx)?;
    if cost_body.instructions.is_empty() {
        return Err(LoweringError::Unsupported("v2:optional-empty-body"));
    }
    let paid_body = match paid_effs {
        Some(effs) if !effs.is_empty() => lower_offer_sub_body(effs, true, acc, ctx)?,
        _ => ExecutableBody::new(BodyTarget::None, Vec::new()),
    };
    let declined_body = match declined_effs {
        Some(effs) if !effs.is_empty() => {
            Some(Box::new(lower_offer_sub_body(effs, false, acc, ctx)?))
        }
        _ => None,
    };
    acc.push(Instruction::MutatePlayer(Box::new(MutatePlayerSpec::new(
        PlayerSelector::Group(players),
        EnginePlayerMutation::OfferAnyPlayer {
            cost_body: Box::new(cost_body),
            paid_body: Box::new(paid_body),
            declined_body,
        },
    ))));
    Ok(())
}

fn lower_offer_sub_body(
    effects: &[v2::Effect],
    in_loop_player: bool,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ExecutableBody> {
    let mut sub_acc = acc.clone();
    sub_acc.instructions.clear();
    let prev_loop = ctx.in_loop_player;
    ctx.in_loop_player = in_loop_player;
    let res = lower_effects_into(effects, &mut sub_acc, ctx);
    ctx.in_loop_player = prev_loop;
    res?;
    if !acc.target_set && sub_acc.target_set {
        acc.target = sub_acc.target;
        acc.target_set = true;
        acc.target_bounds = sub_acc.target_bounds;
        acc.up_to_one = sub_acc.up_to_one;
        acc.x_target_count = sub_acc.x_target_count;
        acc.up_to_x_target_count = sub_acc.up_to_x_target_count;
    }
    Ok(ExecutableBody::new(BodyTarget::None, sub_acc.instructions))
}

fn rewrite_offer_actor_ref(p: v2::PlayerRef, offer_actor: v2::PlayerRef) -> v2::PlayerRef {
    if p == offer_actor || p == v2::PlayerRef::You {
        v2::PlayerRef::ThatPlayer
    } else {
        p
    }
}

fn rewrite_offer_zone_owner(owner: v2::ZoneOwner) -> v2::ZoneOwner {
    match owner {
        v2::ZoneOwner::You | v2::ZoneOwner::Any => v2::ZoneOwner::ThatPlayer,
        other => other,
    }
}

fn rewrite_offer_subject_ref(subj: &v2::SubjectRef, offer_actor: v2::PlayerRef) -> v2::SubjectRef {
    match subj {
        v2::SubjectRef::Player(p) if *p == offer_actor => {
            v2::SubjectRef::Player(v2::PlayerRef::ThatPlayer)
        }
        v2::SubjectRef::HandOf(p) if *p == offer_actor || *p == v2::PlayerRef::You => {
            v2::SubjectRef::HandOf(v2::PlayerRef::ThatPlayer)
        }
        other => other.clone(),
    }
}

fn rewrite_offer_cost_effects(
    offer_actor: v2::PlayerRef,
    effects: &[v2::Effect],
) -> Vec<v2::Effect> {
    effects
        .iter()
        .map(|eff| match eff {
            v2::Effect::ZoneOp {
                actor,
                op,
                subject,
                from,
                to,
                modifiers,
                except,
                count,
            } => v2::Effect::ZoneOp {
                actor: rewrite_offer_actor_ref(*actor, offer_actor),
                op: op.clone(),
                subject: rewrite_offer_subject_ref(subject, offer_actor),
                from: from
                    .iter()
                    .copied()
                    .map(|mut z| {
                        z.owner = rewrite_offer_zone_owner(z.owner);
                        z
                    })
                    .collect(),
                to: to.clone(),
                modifiers: modifiers.clone(),
                except: except.clone(),
                count: count.clone(),
            },
            v2::Effect::AdjustLife { player, amount, op } => v2::Effect::AdjustLife {
                player: rewrite_offer_actor_ref(*player, offer_actor),
                amount: amount.clone(),
                op: *op,
            },
            v2::Effect::MutatePlayer {
                player,
                mutation,
                duration,
            } => v2::Effect::MutatePlayer {
                player: rewrite_offer_actor_ref(*player, offer_actor),
                mutation: mutation.clone(),
                duration: duration.clone(),
            },
            v2::Effect::DealDamage {
                source,
                target,
                amount,
                divided_by,
            } => v2::Effect::DealDamage {
                source: source.clone(),
                target: rewrite_offer_subject_ref(target, offer_actor),
                amount: amount.clone(),
                divided_by: *divided_by,
            },
            v2::Effect::CreateToken {
                controller,
                count,
                spec,
                modifiers,
            } => v2::Effect::CreateToken {
                controller: rewrite_offer_actor_ref(*controller, offer_actor),
                count: count.clone(),
                spec: spec.clone(),
                modifiers: modifiers.clone(),
            },
            other => other.clone(),
        })
        .collect()
}

fn lower_optional_mana_payment(
    payer: v2::PlayerRef,
    cost: &v2::Cost,
    paid_body: Option<&[v2::Effect]>,
    declined_body: Option<&[v2::Effect]>,
    reflexive: bool,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let (fixed, extra_generic) = zone_op::lower_unless_mana_cost(cost, ctx)?;
    let payer_rp = atoms::lower_resolving_player(payer, acc, ctx)?;

    if paid_body.is_none() && declined_body.is_none() {
        let paid_slot = ctx.alloc_binding()?;
        acc.push(Instruction::BindEnv {
            binding: paid_slot,
            value: Box::new(ExecValue::Bool(false)),
        });
        let prompt_idx = acc.instructions.len();
        acc.push(Instruction::PromptChoice(Box::new(
            PromptChoiceSpec::offer_dynamic_mana_payment(
                payer_rp,
                fixed,
                extra_generic,
                prompt_idx + 1,
                prompt_idx + 2,
            ),
        )));
        acc.push(Instruction::BindEnv {
            binding: paid_slot,
            value: Box::new(ExecValue::Bool(true)),
        });
        ctx.last_succeeded_binding = Some(paid_slot);
        return Ok(());
    }

    let prompt_idx = acc.instructions.len();
    acc.push(Instruction::PromptChoice(Box::new(
        PromptChoiceSpec::offer_dynamic_mana_payment(
            payer_rp,
            fixed,
            extra_generic,
            prompt_idx + 1,
            0,
        ),
    )));

    let paid_start = acc.instructions.len();
    if let Some(then_effs) = paid_body {
        if reflexive {
            let sub_body = lower_effect_body(then_effs, ctx)?;
            acc.push(Instruction::EmitReflexiveTrigger {
                body: Box::new(sub_body),
            });
        } else {
            lower_effects_into(then_effs, acc, ctx)?;
        }
    }

    if let Some(else_effs) = declined_body {
        let jump_idx = if paid_body.is_some() {
            let idx = acc.instructions.len();
            acc.push(Instruction::Jump { destination: 0 });
            Some(idx)
        } else {
            None
        };
        let declined_start = acc.instructions.len();
        lower_effects_into(else_effs, acc, ctx)?;
        let after_all = acc.instructions.len();
        if let Some(j_idx) = jump_idx
            && let Some(Instruction::Jump { destination }) = acc.instructions.get_mut(j_idx)
        {
            *destination = after_all;
        }
        if let Some(Instruction::PromptChoice(spec)) = acc.instructions.get_mut(prompt_idx)
            && let Some((_, paid_dest, declined_dest)) = spec.as_mana_payment_mut()
        {
            *paid_dest = if paid_body.is_some() {
                paid_start
            } else {
                after_all
            };
            *declined_dest = declined_start;
        }
    } else {
        let after_paid = acc.instructions.len();
        if let Some(Instruction::PromptChoice(spec)) = acc.instructions.get_mut(prompt_idx)
            && let Some((_, paid_dest, declined_dest)) = spec.as_mana_payment_mut()
        {
            *paid_dest = paid_start;
            *declined_dest = after_paid;
        }
    }
    Ok(())
}

fn lower_optional_with_if_previous(
    actor: v2::PlayerRef,
    body: &[v2::Effect],
    follow: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if body.is_empty() || follow.is_empty() {
        return Err(LoweringError::Unsupported("v2:optional-empty-body"));
    }
    let saved_acc = acc.clone();
    let saved_ctx = CtxFlowSnapshot::capture(ctx);
    let mut combined = Vec::with_capacity(body.len() + follow.len());
    combined.extend_from_slice(body);
    combined.extend_from_slice(follow);
    match lower_optional(actor, &combined, acc, ctx) {
        Ok(()) => Ok(()),
        Err(LoweringError::Unsupported("v2:optional-nested-control-flow")) => {
            *acc = saved_acc;
            saved_ctx.restore(ctx);

            let prev_succeeded = ctx.last_succeeded_binding;
            let chooser = atoms::lower_resolving_player(actor, acc, ctx)?;
            let prior_moved = ctx.last_moved_binding;
            let prior_unchosen = ctx.last_unchosen_binding;

            let opt_idx = acc.instructions.len();
            acc.push(Instruction::OptionalSequence {
                chooser,
                instructions: 0,
            });
            lower_effects_into(body, acc, ctx)?;

            let ok_slot = if ctx.last_succeeded_binding != prev_succeeded
                && let Some(existing) = ctx.last_succeeded_binding
            {
                existing
            } else {
                let slot = ctx.alloc_binding()?;
                acc.push(Instruction::BindEnv {
                    binding: slot,
                    value: Box::new(ExecValue::Bool(true)),
                });
                ctx.last_succeeded_binding = Some(slot);
                slot
            };

            let count = acc.instructions.len().saturating_sub(opt_idx + 1);
            if count == 0 {
                return Err(LoweringError::Unsupported("v2:optional-empty-body"));
            }
            validate_optional_slice(&acc.instructions[(opt_idx + 1)..])?;
            if let Some(Instruction::OptionalSequence { instructions, .. }) =
                acc.instructions.get_mut(opt_idx)
            {
                *instructions = count;
            }
            maybe_preinit_optional_unchosen(acc, opt_idx, prior_moved, prior_unchosen, ctx);

            let branch_idx = acc.instructions.len();
            acc.push(Instruction::BranchCondition {
                condition: Box::new(EngineConditionExpr::BoundBool(ok_slot)),
                destination: 0,
            });
            lower_effects_into(follow, acc, ctx)?;
            let after_follow = acc.instructions.len();
            if let Some(Instruction::BranchCondition { destination, .. }) =
                acc.instructions.get_mut(branch_idx)
            {
                *destination = after_follow;
            }
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn lower_optional_with_reflexive(
    actor: v2::PlayerRef,
    body: &[v2::Effect],
    follow: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if body.is_empty() || follow.is_empty() {
        return Err(LoweringError::Unsupported("v2:optional-empty-body"));
    }
    let chooser = atoms::lower_resolving_player(actor, acc, ctx)?;
    let prior_moved = ctx.last_moved_binding;
    let prior_unchosen = ctx.last_unchosen_binding;

    let opt_idx = acc.instructions.len();
    acc.push(Instruction::OptionalSequence {
        chooser,
        instructions: 0,
    });
    lower_effects_into(body, acc, ctx)?;
    let sub_body = lower_effect_body(follow, ctx)?;
    acc.push(Instruction::EmitReflexiveTrigger {
        body: Box::new(sub_body),
    });

    let count = acc.instructions.len().saturating_sub(opt_idx + 1);
    if count == 0 {
        return Err(LoweringError::Unsupported("v2:optional-empty-body"));
    }
    validate_optional_slice(&acc.instructions[(opt_idx + 1)..])?;
    if let Some(Instruction::OptionalSequence { instructions, .. }) =
        acc.instructions.get_mut(opt_idx)
    {
        *instructions = count;
    }
    maybe_preinit_optional_unchosen(acc, opt_idx, prior_moved, prior_unchosen, ctx);
    Ok(())
}

fn validate_optional_slice(slice: &[Instruction]) -> LowerResult<()> {
    for ins in slice {
        if matches!(
            ins,
            Instruction::BranchCondition { .. }
                | Instruction::Jump { .. }
                | Instruction::OptionalSequence { .. }
        ) {
            return Err(LoweringError::Unsupported("v2:optional-nested-control-flow"));
        }
    }
    if let Some(Instruction::PromptChoice(spec)) = slice.first()
        && spec.as_mana_payment().is_some()
        && slice.len() < 2
    {
        return Err(LoweringError::Unsupported("v2:optional-nested-control-flow"));
    }
    Ok(())
}

fn maybe_preinit_optional_unchosen(
    acc: &mut BodyAcc,
    opt_idx: usize,
    prior_moved: Option<BindingId>,
    prior_unchosen: Option<BindingId>,
    ctx: &LowerCtx<'_>,
) {
    if let (Some(src), Some(uid)) = (prior_moved, ctx.last_unchosen_binding)
        && Some(uid) != prior_unchosen
    {
        let mut init_spec = ZoneOpSpec::move_all(ObjectQuery::Bound(src), EngineZoneDest::None);
        init_spec.bind_moved = Some(uid);
        for ins in &mut acc.instructions {
            ins.shift_jumps(1, opt_idx);
        }
        acc.instructions
            .insert(opt_idx, Instruction::ZoneOp(Box::new(init_spec)));
    }
}

/// Lower a sequence of `v2::Effect` instructions into a standalone [`ExecutableBody`].
pub(crate) fn lower_effect_body(
    effects: &[v2::Effect],
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ExecutableBody> {
    if effects.is_empty() {
        return Err(LoweringError::Unsupported("v2:empty-effect-body"));
    }
    let mut acc = BodyAcc::new();
    lower_effects_into(effects, &mut acc, ctx)?;
    acc.into_body()
}

#[derive(Clone, Copy)]
struct CtxFlowSnapshot {
    next_binding_slot: u16,
    last_moved_binding: Option<BindingId>,
    last_unchosen_binding: Option<BindingId>,
    last_succeeded_binding: Option<BindingId>,
}

impl CtxFlowSnapshot {
    fn capture(ctx: &LowerCtx<'_>) -> Self {
        Self {
            next_binding_slot: ctx.next_binding_slot,
            last_moved_binding: ctx.last_moved_binding,
            last_unchosen_binding: ctx.last_unchosen_binding,
            last_succeeded_binding: ctx.last_succeeded_binding,
        }
    }

    fn restore(self, ctx: &mut LowerCtx<'_>) {
        ctx.next_binding_slot = self.next_binding_slot;
        ctx.last_moved_binding = self.last_moved_binding;
        ctx.last_unchosen_binding = self.last_unchosen_binding;
        ctx.last_succeeded_binding = self.last_succeeded_binding;
    }
}

fn splice_untargeted_instructions(acc: &mut BodyAcc, mut incoming: Vec<Instruction>) {
    let offset = acc.instructions.len();
    if offset > 0 {
        let delta = isize::try_from(offset).unwrap_or(isize::MAX);
        for ins in &mut incoming {
            ins.shift_jumps(delta, 0);
        }
    }
    acc.instructions.extend(incoming);
}

fn lower_one_effect(
    eff: &v2::Effect,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match eff {
        v2::Effect::ZoneOp {
            actor,
            op,
            subject,
            from,
            to,
            modifiers,
            except,
            count,
        } => zone_op::lower_zone_op(
            *actor,
            op,
            subject,
            from,
            to.as_ref(),
            modifiers,
            except.as_deref(),
            count.as_ref(),
            acc,
            ctx,
        ),
        v2::Effect::DealDamage {
            source,
            target,
            amount,
            divided_by,
        } => lower_deal_damage(source, target, amount, *divided_by, acc, ctx),
        v2::Effect::PreventDamage {
            amount,
            combat_only,
            source,
            target,
            duration,
        } => lower_prevent_damage(
            amount.as_ref(),
            *combat_only,
            source.as_ref(),
            target.as_ref(),
            duration,
            acc,
            ctx,
        ),
        v2::Effect::AdjustLife { player, amount, op } => {
            lower_adjust_life(*player, amount, *op, acc, ctx)
        }
        v2::Effect::MutateObject {
            subject,
            mutation: obj_mut,
            duration,
        } => mutation::lower_object_mutation(subject, obj_mut, duration, acc, ctx),
        v2::Effect::MutatePlayer {
            player,
            mutation: player_mut,
            duration,
        } => mutation::lower_player_mutation(*player, player_mut, duration, acc, ctx),
        v2::Effect::CreateToken {
            controller,
            count,
            spec,
            modifiers,
        } => token::lower_token_creation(*controller, count, spec, modifiers, acc, ctx),
        v2::Effect::CopyObject {
            actor,
            subject,
            count,
            modifiers,
            exceptions,
        } => token::lower_copy_object(*actor, subject, count, modifiers, exceptions, acc, ctx),
        v2::Effect::ApplyContinuous { effect, duration } => {
            let dur = atoms::lower_duration(duration)?;
            static_ability::lower_temporary_static(effect, dur, acc, ctx)
        }
        v2::Effect::ApplyReplacement {
            replacement: repl,
            duration,
        } => {
            let dur = atoms::lower_duration(duration)?;
            replacement::lower_temporary_replacement(repl, dur, acc, ctx)
        }
        v2::Effect::PromptChoice { chooser, kind } => {
            lower_prompt_choice(*chooser, kind, acc, ctx)
        }
        v2::Effect::Optional { actor, body } => lower_optional(*actor, body, acc, ctx),
        v2::Effect::BranchCondition {
            condition: cond,
            then_body,
            else_body,
        } => lower_branch_condition(cond, then_body, else_body, acc, ctx),
        v2::Effect::IfPrevious { succeeded, body } => lower_if_previous(*succeeded, body, acc, ctx),
        v2::Effect::ReflexiveTrigger(body) => lower_reflexive_trigger(body, acc, ctx),
        v2::Effect::ReplacePrevious(_) => {
            Err(LoweringError::Unsupported("v2:effect-replace-previous"))
        }
        v2::Effect::Unless {
            body,
            payer,
            cost,
            condition: cond,
        } => lower_unless(body, *payer, cost.as_ref(), cond.as_ref(), acc, ctx),
        v2::Effect::UnlessEffect { .. } => {
            Err(LoweringError::Unsupported("v2:effect-unless-effect"))
        }
        v2::Effect::ForEach { over, body } => lower_for_each(over, body, acc, ctx),
        v2::Effect::BindVariable { var, expr, body } => {
            lower_bind_variable(*var, expr, body, acc, ctx)
        }
        v2::Effect::DelayedTrigger { trigger: dt, body } => {
            lower_delayed_trigger(dt, body, None, acc, ctx)
        }
        v2::Effect::ScopedDuration { duration, body } => {
            lower_scoped_duration(duration, body, acc, ctx)
        }
        v2::Effect::StartingWith { player, body } => lower_starting_with(*player, body, acc, ctx),
        v2::Effect::ExchangeControl { subject } => lower_exchange_control(subject, acc, ctx),
        v2::Effect::Restriction(_) => {
            Err(LoweringError::Unsupported("v2:effect-inline-restriction"))
        }
    }
}

fn lower_deal_damage(
    source: &v2::SubjectRef,
    target: &v2::SubjectRef,
    amount: &v2::ValueExpr,
    divided_by: Option<v2::PlayerRef>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    register_value_expr_player_targets(amount, acc, ctx)?;
    let amt = value::lower_value_expr(amount, ctx)?;

    if let Some(divider) = divided_by {
        if !matches!(source, v2::SubjectRef::SelfObject) {
            return Err(LoweringError::Unsupported("v2:divided-damage-non-self"));
        }
        if divider != v2::PlayerRef::You {
            return Err(LoweringError::Unsupported(
                "v2:divided-damage-non-controller",
            ));
        }
        let (bt, t_count) = atoms::lower_subject_to_target(target, ctx)?;
        acc.register_target(bt, t_count)?;
        let max_targets = match t_count {
            v2::TargetCount::Exact(n) | v2::TargetCount::UpTo(n) => Some(n),
            v2::TargetCount::AnyNumber => None,
            v2::TargetCount::VariableX | v2::TargetCount::UpToX => {
                return Err(LoweringError::Unsupported("v2:divided-damage-x-targets"));
            }
        };
        acc.push(Instruction::DealDamage(Box::new(DealDamageSpec::divided(
            amt,
            max_targets,
        ))));
        return Ok(());
    }

    if divided_by.is_none()
        && !acc.target_set
        && matches!(source, v2::SubjectRef::TargetObject { .. })
        && is_power_of_source_expr(amount, source)
        && let Ok((dealer_bt, dealer_tc)) = atoms::lower_subject_to_target(source, ctx)
        && dealer_bt.guarantees_creature()
        && matches!(
            dealer_tc,
            v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1)
        )
        && let Ok((recip_bt, recip_tc)) = atoms::lower_subject_to_target(target, ctx)
        && matches!(
            recip_bt,
            BodyTarget::Object {
                zone: TargetZone::Battlefield,
                ..
            } | BodyTarget::Player { .. }
                | BodyTarget::AnyTarget(_)
        )
        && matches!(
            recip_tc,
            v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1)
        )
        && dealer_bt != recip_bt
    {
        let mut c1 = BodyAcc::new();
        c1.register_target(dealer_bt, dealer_tc)?;
        c1.push(Instruction::MutateObject(Box::new(
            MutateObjectSpec::remember_permanent_target(),
        )));
        let mut c2 = BodyAcc::new();
        c2.register_target(recip_bt, recip_tc)?;
        c2.push(Instruction::MutateObject(Box::new(
            MutateObjectSpec::remember_permanent_target(),
        )));
        let b1 = c1.into_body()?;
        let b2 = c2.into_body()?;
        acc.push(Instruction::FixedTargetClause(Box::new(b1)));
        acc.push(Instruction::FixedTargetClause(Box::new(b2)));
        acc.push(Instruction::DealDamage(Box::new(
            DealDamageSpec::creature_power_to_other(false),
        )));
        return Ok(());
    }

    let src_query = match source {
        v2::SubjectRef::SelfObject => DamageSourceQuery::EntrySource,
        other => DamageSourceQuery::Objects(atoms::lower_subject_to_query(other, acc, ctx)?),
    };

    let recipients = lower_damage_recipients(target, acc, ctx)?;
    acc.push(Instruction::DealDamage(Box::new(DealDamageSpec::new(
        src_query,
        recipients,
        DamageAmountSpec::Expr(amt),
    ))));
    Ok(())
}

fn lower_damage_recipients(
    target: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<DamageTargetQuery> {
    match target {
        v2::SubjectRef::AnyTarget => {
            acc.register_target(BodyTarget::SINGLE_ANY_TARGET, v2::TargetCount::Exact(1))?;
            Ok(DamageTargetQuery::Targets)
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
            if matches!(count, v2::TargetCount::Exact(1)) {
                Ok(DamageTargetQuery::Players(PlayerSelector::Resolving(
                    ResolvingPlayer::TargetPlayer,
                )))
            } else if let Some((min_targets, max_targets)) = damage_announced_bounds(*count) {
                acc.target_bounds = None;
                acc.up_to_one = false;
                Ok(DamageTargetQuery::AnnouncedTargets {
                    min_targets,
                    max_targets,
                })
            } else {
                Ok(DamageTargetQuery::Targets)
            }
        }
        v2::SubjectRef::TargetObject { .. } => {
            let (bt, tc) = atoms::lower_subject_to_target(target, ctx)?;
            acc.register_target(bt, tc)?;
            Ok(DamageTargetQuery::Targets)
        }
        v2::SubjectRef::Player(player_ref) => Ok(DamageTargetQuery::Players(
            atoms::lower_player_ref(*player_ref, acc, ctx)?,
        )),
        v2::SubjectRef::Compound(parts) => {
            if let Ok((bt, tc)) = atoms::lower_subject_to_target(target, ctx)
                && !matches!(bt, BodyTarget::None)
            {
                acc.register_target(bt, tc)?;
                if matches!(bt, BodyTarget::Player { .. } | BodyTarget::AnyTarget(_))
                    && let Some((min_targets, max_targets)) = damage_announced_bounds(tc)
                {
                    acc.target_bounds = None;
                    acc.up_to_one = false;
                    return Ok(DamageTargetQuery::AnnouncedTargets {
                        min_targets,
                        max_targets,
                    });
                }
                return Ok(DamageTargetQuery::Targets);
            }
            let mut obj_queries = Vec::new();
            let mut player_sels = Vec::new();
            for part in parts {
                match part {
                    v2::SubjectRef::Player(p) => {
                        player_sels.push(atoms::lower_player_ref(*p, acc, ctx)?);
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
                        player_sels.push(PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer));
                    }
                    other => {
                        obj_queries.push(atoms::lower_subject_to_query(other, acc, ctx)?);
                    }
                }
            }
            let obj_query = match obj_queries.len() {
                0 => None,
                1 => obj_queries.into_iter().next(),
                _ => Some(ObjectQuery::Union(obj_queries)),
            };
            let player_sel = match player_sels.len() {
                0 => None,
                1 => player_sels.into_iter().next(),
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:damage-compound-multiple-player-selectors",
                    ));
                }
            };
            match (obj_query, player_sel) {
                (Some(objects), Some(players)) => Ok(DamageTargetQuery::Both { objects, players }),
                (Some(objects), None) => Ok(DamageTargetQuery::Objects(objects)),
                (None, Some(players)) => Ok(DamageTargetQuery::Players(players)),
                (None, None) => Err(LoweringError::Unsupported("v2:damage-compound-empty")),
            }
        }
        other => {
            register_subject_filter_player_targets(other, acc, ctx)?;
            Ok(DamageTargetQuery::Objects(
                atoms::lower_subject_to_query(other, acc, ctx)?,
            ))
        }
    }
}

fn damage_announced_bounds(count: v2::TargetCount) -> Option<(u16, u16)> {
    match count {
        v2::TargetCount::Exact(n) if n > 1 => Some((n, n)),
        v2::TargetCount::UpTo(n) if n >= 1 => Some((0, n)),
        v2::TargetCount::AnyNumber => Some((0, u16::MAX)),
        _ => None,
    }
}

fn lower_prevent_damage(
    amount: Option<&v2::ValueExpr>,
    combat_only: bool,
    source: Option<&v2::SubjectRef>,
    target: Option<&v2::SubjectRef>,
    duration: &v2::Duration,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let dur = match duration {
        v2::Duration::Instant => ResolvedDuration::UntilEndOfTurn,
        other => atoms::lower_duration(other)?,
    };

    if amount.is_none() && combat_only && source.is_none() && target.is_none() {
        acc.push(Instruction::ApplyContinuous(Box::new(
            ApplyContinuousSpec::temporary_rule(
                TemporaryRuleKind::prevent_all_combat_damage(),
                dur,
            ),
        )));
        return Ok(());
    }

    if let Some(expr) = amount {
        register_value_expr_player_targets(expr, acc, ctx)?;
    }
    let lowered_amount = amount
        .map(|expr| value::lower_value_expr(expr, ctx).map(Box::new))
        .transpose()?;

    let (source_filter, source_is_target) = match source {
        None => (None, false),
        Some(v2::SubjectRef::SelfObject) => (Some(EngineObjectFilter::self_object()), false),
        Some(v2::SubjectRef::Each(f) | v2::SubjectRef::All(f) | v2::SubjectRef::Group(f)) => {
            register_filter_player_targets(f, acc, ctx)?;
            (Some(filter::lower_object_filter(f, ctx)?), false)
        }
        Some(subj @ v2::SubjectRef::TargetObject { .. }) => {
            let (bt, tc) = atoms::lower_subject_to_target(subj, ctx)?;
            acc.register_target(bt, tc)?;
            (None, true)
        }
        Some(_) => {
            return Err(LoweringError::Unsupported("v2:prevent-damage-source"));
        }
    };

    let recipient = match target {
        None => StaticDamageRecipient::Any,
        Some(v2::SubjectRef::Player(v2::PlayerRef::You)) => StaticDamageRecipient::Controller,
        Some(v2::SubjectRef::Player(v2::PlayerRef::EachOpponent)) => {
            StaticDamageRecipient::Opponents
        }
        Some(v2::SubjectRef::Player(v2::PlayerRef::EachPlayer)) => StaticDamageRecipient::Players,
        Some(v2::SubjectRef::SelfObject) => StaticDamageRecipient::SelfObject,
        Some(v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::EnchantedCreature
            | v2::AnaphoraRef::EnchantedPermanent
            | v2::AnaphoraRef::EquippedCreature,
        )) => StaticDamageRecipient::AttachedHost,
        Some(v2::SubjectRef::AnyTarget) => {
            acc.register_target(BodyTarget::SINGLE_ANY_TARGET, v2::TargetCount::Exact(1))?;
            StaticDamageRecipient::TargetAny
        }
        Some(v2::SubjectRef::TargetPlayer {
            opponent_only,
            count,
        }) => {
            acc.register_target(
                BodyTarget::Player {
                    opponents_only: *opponent_only,
                },
                *count,
            )?;
            StaticDamageRecipient::TargetAny
        }
        Some(subj @ v2::SubjectRef::TargetObject { .. }) => {
            let (bt, tc) = atoms::lower_subject_to_target(subj, ctx)?;
            acc.register_target(bt, tc)?;
            StaticDamageRecipient::TargetObject
        }
        Some(v2::SubjectRef::Each(f) | v2::SubjectRef::All(f) | v2::SubjectRef::Group(f)) => {
            StaticDamageRecipient::MatchingPermanent(filter::lower_object_filter(f, ctx)?)
        }
        Some(_) => {
            return Err(LoweringError::Unsupported("v2:prevent-damage-target"));
        }
    };

    acc.push(Instruction::ApplyContinuous(Box::new(
        ApplyContinuousSpec::temporary_rule(
            TemporaryRuleKind::PreventDamageShield {
                amount: lowered_amount,
                source: source_filter,
                source_chosen_color: false,
                source_chosen_type: false,
                source_is_target,
                half_round_down: false,
                recipient,
                combat_only,
                one_shot: false,
            },
            dur,
        ),
    )));
    Ok(())
}

fn lower_adjust_life(
    player: v2::PlayerRef,
    amount: &v2::ValueExpr,
    op: v2::LifeOp,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let ps = atoms::lower_player_ref(player, acc, ctx)?;
    register_value_expr_player_targets(amount, acc, ctx)?;
    let amt = value::lower_value_expr(amount, ctx)?;
    let engine_op = match op {
        v2::LifeOp::Gain => EngineLifeOp::Gain(amt),
        v2::LifeOp::Lose => EngineLifeOp::Lose(amt),
        v2::LifeOp::Set => EngineLifeOp::Set(amt),
    };
    acc.push(Instruction::AdjustLife(Box::new(AdjustLifeSpec::new(
        ps, engine_op,
    ))));
    Ok(())
}

fn lower_prompt_choice(
    chooser: v2::PlayerRef,
    kind: &v2::ChoiceKind,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let chooser_sel = atoms::lower_player_ref(chooser, acc, ctx)?;
    match kind {
        v2::ChoiceKind::ChooseCharacteristic(ck) => {
            let domain = match ck {
                v2::CharacteristicKind::Color => PromptDomain::color(),
                v2::CharacteristicKind::CardType => PromptDomain::card_type(Vec::new()),
                v2::CharacteristicKind::CreatureType => PromptDomain::Subtype {
                    kind: SubtypeChoiceKind::Creature,
                    excluding: BTreeSet::new(),
                    legacy_slot: None,
                },
                v2::CharacteristicKind::BasicLandType => PromptDomain::basic_land_type(),
                v2::CharacteristicKind::CardName => PromptDomain::card_name(false),
                v2::CharacteristicKind::NonlandCardName => PromptDomain::card_name(true),
                v2::CharacteristicKind::OddOrEven => {
                    PromptDomain::labeled_option(vec!["Odd".to_string(), "Even".to_string()])
                }
                v2::CharacteristicKind::BasicLandTypes(_) => {
                    return Err(LoweringError::Unsupported(
                        "v2:choice-multiple-basic-land-types",
                    ));
                }
                v2::CharacteristicKind::BinaryMode => {
                    return Err(LoweringError::Unsupported("v2:choice-binary-mode"));
                }
                v2::CharacteristicKind::Number => {
                    return Err(LoweringError::Unsupported("v2:choice-unbounded-number"));
                }
            };
            let bind_to = ctx.alloc_binding()?;
            acc.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                chooser: chooser_sel,
                domain,
                category: ChoiceCategory::Generic,
                bind_to,
                bind_unchosen: None,
                tag_partition: None,
                auto_resolve_trivial: false,
                option_branches: Vec::new(),
            })));
            Ok(())
        }
        v2::ChoiceKind::ChooseBranch(branches) => {
            lower_option_branches(chooser_sel, branches, ChoiceCategory::Modal, 1, 1, acc, ctx)
        }
        v2::ChoiceKind::VillainousChoice { options } => lower_option_branches(
            chooser_sel,
            options,
            ChoiceCategory::VillainousChoice,
            1,
            1,
            acc,
            ctx,
        ),
        v2::ChoiceKind::Modal { count, modes } => {
            let mode_count = u8::try_from(modes.len())
                .map_err(|_| LoweringError::Unsupported("v2:modal-too-many-modes"))?;
            let (min, max) = match count {
                v2::ChoiceCount::Exact(n) => (*n, *n),
                v2::ChoiceCount::UpTo(n) => (0, *n),
                v2::ChoiceCount::OneOrBoth => (1, 2.min(mode_count)),
                v2::ChoiceCount::OneOrMore => (1, mode_count),
                v2::ChoiceCount::AnyNumber => (0, mode_count),
                v2::ChoiceCount::VariableX => {
                    return Err(LoweringError::Unsupported("v2:modal-variable-x"));
                }
            };
            let bodies: Vec<Vec<v2::Effect>> = modes.iter().map(|m| m.body.clone()).collect();
            lower_option_branches(
                chooser_sel,
                &bodies,
                ChoiceCategory::Modal,
                min,
                max,
                acc,
                ctx,
            )
        }
        v2::ChoiceKind::Spree { .. } => Err(LoweringError::Unsupported("v2:choice-spree-inline")),
        v2::ChoiceKind::ChooseObject(subj) => {
            match subj {
                v2::SubjectRef::Player(p) => {
                    let selector = match p {
                        v2::PlayerRef::AnyOpponent | v2::PlayerRef::EachOpponent => {
                            PlayerSelector::Group(AffectedPlayers::EachOpponent)
                        }
                        v2::PlayerRef::AnyPlayer | v2::PlayerRef::EachPlayer => {
                            PlayerSelector::Group(AffectedPlayers::EachPlayer)
                        }
                        v2::PlayerRef::EachOtherPlayer => {
                            PlayerSelector::Group(AffectedPlayers::EachOtherPlayer)
                        }
                        other => atoms::lower_player_ref(*other, acc, ctx)?,
                    };
                    let bind_to = ctx.alloc_binding()?;
                    acc.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                        chooser: chooser_sel,
                        domain: PromptDomain::Players {
                            selector,
                            min: 1,
                            max: 1,
                            random: false,
                        },
                        category: ChoiceCategory::Generic,
                        bind_to,
                        bind_unchosen: None,
                        tag_partition: None,
                        auto_resolve_trivial: false,
                        option_branches: Vec::new(),
                    })));
                    return Ok(());
                }
                v2::SubjectRef::TargetPlayer {
                    opponent_only,
                    count,
                } => {
                    let (min, max) = match count {
                        v2::TargetCount::Exact(n) => {
                            let v = u8::try_from(*n).map_err(|_| {
                                LoweringError::Unsupported("v2:choose-player-count")
                            })?;
                            (v, v)
                        }
                        v2::TargetCount::UpTo(n) => {
                            let v = u8::try_from(*n).map_err(|_| {
                                LoweringError::Unsupported("v2:choose-player-count")
                            })?;
                            (0, v)
                        }
                        _ => {
                            return Err(LoweringError::Unsupported("v2:choose-player-count"));
                        }
                    };
                    let selector = PlayerSelector::Group(if *opponent_only {
                        AffectedPlayers::EachOpponent
                    } else {
                        AffectedPlayers::EachPlayer
                    });
                    let bind_to = ctx.alloc_binding()?;
                    acc.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                        chooser: chooser_sel,
                        domain: PromptDomain::Players {
                            selector,
                            min,
                            max,
                            random: false,
                        },
                        category: ChoiceCategory::Generic,
                        bind_to,
                        bind_unchosen: None,
                        tag_partition: None,
                        auto_resolve_trivial: false,
                        option_branches: Vec::new(),
                    })));
                    return Ok(());
                }
                _ => {}
            }
            let (query, min, max) = match subj {
                v2::SubjectRef::Counted { filter: f, count } => {
                    let has_among_prior = f
                        .predicates
                        .iter()
                        .any(|p| matches!(p, v2::FilterPredicate::AmongPriorCards));
                    let mut f_clean = f.as_ref().clone();
                    if has_among_prior {
                        f_clean
                            .predicates
                            .retain(|p| !matches!(p, v2::FilterPredicate::AmongPriorCards));
                    }
                    register_filter_player_targets(&f_clean, acc, ctx)?;
                    let ef = filter::lower_object_filter(&f_clean, ctx)?;
                    let q = if has_among_prior {
                        let src_slot = ctx.last_moved_binding.ok_or(
                            LoweringError::Unsupported("v2:filter:pred:among-prior-cards"),
                        )?;
                        ObjectQuery::BoundFiltered {
                            binding: src_slot,
                            filter: Box::new(ef),
                        }
                    } else {
                        let zone = if f_clean.zones.is_empty() {
                            QueryZone::Battlefield
                        } else if f_clean.zones.len() == 1 {
                            atoms::lower_zone_ref(f_clean.zones[0], acc, ctx)?
                        } else {
                            return Err(LoweringError::Unsupported("v2:choose-object-multi-zone"));
                        };
                        ObjectQuery::zone(zone, ef)
                    };
                    let max_e = if count.any_number {
                        EngineValueExpr::literal(i64::from(u16::MAX))
                    } else {
                        register_value_expr_player_targets(&count.count, acc, ctx)?;
                        value::lower_value_expr(&count.count, ctx)?
                    };
                    let min_e = if count.up_to || count.any_number {
                        EngineValueExpr::literal(0)
                    } else {
                        max_e.clone()
                    };
                    (q, min_e, max_e)
                }
                other => {
                    let q = atoms::lower_subject_to_query(other, acc, ctx)?;
                    (q, EngineValueExpr::literal(1), EngineValueExpr::literal(1))
                }
            };
            let bind_to = ctx.alloc_binding()?;
            let bind_unchosen = ctx.alloc_binding()?;
            acc.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                chooser: chooser_sel,
                domain: PromptDomain::Objects {
                    query,
                    min: Box::new(min),
                    max: Box::new(max),
                    constraint: None,
                    max_total_power: None,
                    hidden: false,
                },
                category: ChoiceCategory::Generic,
                bind_to,
                bind_unchosen: Some(bind_unchosen),
                tag_partition: None,
                auto_resolve_trivial: true,
                option_branches: Vec::new(),
            })));
            ctx.last_moved_binding = Some(bind_to);
            ctx.last_unchosen_binding = Some(bind_unchosen);
            Ok(())
        }
        v2::ChoiceKind::ChooseKeywordAndGrant {
            subject,
            keywords,
            duration,
        } => {
            if keywords.len() < 2 || keywords.len() > usize::from(u8::MAX) {
                return Err(LoweringError::Unsupported(
                    "v2:choose-keyword-and-grant-count",
                ));
            }
            let dur = atoms::lower_duration(duration)?;
            let query = atoms::lower_subject_to_query(subject, acc, ctx)?;
            let mut labels = Vec::with_capacity(keywords.len());
            let mut option_branches = Vec::with_capacity(keywords.len());
            for kw in keywords {
                let engine_kw = atoms::lower_keyword_ability(kw, ctx)?;
                labels.push(format!("{:?}", kw.kind));
                option_branches.push(ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ApplyContinuous(Box::new(
                        ApplyContinuousSpec::resolved_parts(
                            query.clone(),
                            ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                AbilityChange::grant_keywords([engine_kw]),
                            )]),
                            dur,
                        ),
                    ))],
                ));
            }
            let bind_to = ctx.alloc_binding()?;
            acc.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                chooser: chooser_sel,
                domain: PromptDomain::Options {
                    labels,
                    min: 1,
                    max: 1,
                    allow_repeat: false,
                },
                category: ChoiceCategory::Generic,
                bind_to,
                bind_unchosen: None,
                tag_partition: None,
                auto_resolve_trivial: false,
                option_branches,
            })));
            Ok(())
        }
    }
}

fn lower_option_branches(
    chooser: PlayerSelector,
    branches: &[Vec<v2::Effect>],
    category: ChoiceCategory,
    min: u8,
    max: u8,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if branches.len() < 2 || branches.len() > usize::from(u8::MAX) {
        return Err(LoweringError::Unsupported("v2:choice-branch-count"));
    }
    if min > max || usize::from(max) > branches.len() {
        return Err(LoweringError::Unsupported("v2:choice-branch-bounds"));
    }
    let mut labels = Vec::with_capacity(branches.len());
    let mut option_branches = Vec::with_capacity(branches.len());
    for (idx, branch) in branches.iter().enumerate() {
        labels.push(format!("Option {}", idx + 1));
        option_branches.push(lower_effect_body(branch, ctx)?);
    }
    let bind_to = ctx.alloc_binding()?;
    acc.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
        chooser,
        domain: PromptDomain::Options {
            labels,
            min,
            max,
            allow_repeat: false,
        },
        category,
        bind_to,
        bind_unchosen: None,
        tag_partition: None,
        auto_resolve_trivial: false,
        option_branches,
    })));
    Ok(())
}

fn lower_optional(
    actor: v2::PlayerRef,
    body: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if body.is_empty() {
        return Err(LoweringError::Unsupported("v2:optional-empty-body"));
    }
    if matches!(actor, v2::PlayerRef::AnyPlayer | v2::PlayerRef::AnyOpponent) {
        return lower_offer_any_player(actor, body, None, None, acc, ctx);
    }
    if !ctx.in_loop_player
        && matches!(
            actor,
            v2::PlayerRef::EachPlayer
                | v2::PlayerRef::EachOpponent
                | v2::PlayerRef::EachOtherPlayer
        )
    {
        let selector = atoms::lower_player_ref(actor, acc, ctx)?;
        let prev = ctx.in_loop_player;
        ctx.in_loop_player = true;
        let sub_res = (|| {
            let mut sub_acc = BodyAcc::new();
            lower_optional(v2::PlayerRef::ThatPlayer, body, &mut sub_acc, ctx)?;
            sub_acc.into_body()
        })();
        ctx.in_loop_player = prev;
        let sub_body = sub_res?;
        acc.push(Instruction::ForEachPlayerQuery {
            selector: Box::new(selector),
            bind_player: BindingId::LOOP_PLAYER,
            body: Box::new(sub_body),
        });
        return Ok(());
    }
    let chooser = atoms::lower_resolving_player(actor, acc, ctx)?;
    let prior_moved = ctx.last_moved_binding;
    let prior_unchosen = ctx.last_unchosen_binding;
    let opt_idx = acc.instructions.len();
    acc.push(Instruction::OptionalSequence {
        chooser,
        instructions: 0,
    });
    lower_effects_into(body, acc, ctx)?;
    let count = acc.instructions.len().saturating_sub(opt_idx + 1);
    if count == 0 {
        return Err(LoweringError::Unsupported("v2:optional-empty-body"));
    }
    validate_optional_slice(&acc.instructions[(opt_idx + 1)..])?;
    if let Some(Instruction::OptionalSequence { instructions, .. }) = acc.instructions.get_mut(opt_idx)
    {
        *instructions = count;
    }
    maybe_preinit_optional_unchosen(acc, opt_idx, prior_moved, prior_unchosen, ctx);
    Ok(())
}

fn lower_branch_condition(
    cond: &v2::ConditionExpr,
    then_body: &[v2::Effect],
    else_body: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if then_body.is_empty() && else_body.is_empty() {
        return Err(LoweringError::Unsupported("v2:branch-both-empty"));
    }
    let lowered_cond = condition::lower_condition_expr(cond, ctx)?;

    if then_body.is_empty() {
        let negated = EngineConditionExpr::Not(Box::new(lowered_cond));
        let branch_idx = acc.instructions.len();
        acc.push(Instruction::BranchCondition {
            condition: Box::new(negated),
            destination: 0,
        });
        lower_effects_into(else_body, acc, ctx)?;
        let after_else = acc.instructions.len();
        if let Some(Instruction::BranchCondition { destination, .. }) =
            acc.instructions.get_mut(branch_idx)
        {
            *destination = after_else;
        }
        return Ok(());
    }

    let branch_idx = acc.instructions.len();
    acc.push(Instruction::BranchCondition {
        condition: Box::new(lowered_cond),
        destination: 0,
    });
    lower_effects_into(then_body, acc, ctx)?;

    if else_body.is_empty() {
        let after_then = acc.instructions.len();
        if let Some(Instruction::BranchCondition { destination, .. }) =
            acc.instructions.get_mut(branch_idx)
        {
            *destination = after_then;
        }
        return Ok(());
    }

    let jump_idx = acc.instructions.len();
    acc.push(Instruction::Jump { destination: 0 });
    let else_start = acc.instructions.len();
    if let Some(Instruction::BranchCondition { destination, .. }) =
        acc.instructions.get_mut(branch_idx)
    {
        *destination = else_start;
    }
    lower_effects_into(else_body, acc, ctx)?;
    let after_else = acc.instructions.len();
    if let Some(Instruction::Jump { destination }) = acc.instructions.get_mut(jump_idx) {
        *destination = after_else;
    }
    Ok(())
}

fn lower_if_previous(
    succeeded: bool,
    body: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if body.is_empty() {
        return Err(LoweringError::Unsupported("v2:if-previous-empty-body"));
    }
    let Some(binding) = ctx.last_succeeded_binding else {
        return Err(LoweringError::Unsupported(
            "v2:if-previous-without-prior-binding",
        ));
    };
    let cond = if succeeded {
        EngineConditionExpr::BoundBool(binding)
    } else {
        EngineConditionExpr::Not(Box::new(EngineConditionExpr::BoundBool(binding)))
    };
    let branch_idx = acc.instructions.len();
    acc.push(Instruction::BranchCondition {
        condition: Box::new(cond),
        destination: 0,
    });
    lower_effects_into(body, acc, ctx)?;
    let after_body = acc.instructions.len();
    if let Some(Instruction::BranchCondition { destination, .. }) =
        acc.instructions.get_mut(branch_idx)
    {
        *destination = after_body;
    }
    Ok(())
}

fn lower_reflexive_trigger(
    body: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let sub_body = lower_effect_body(body, ctx)?;
    if let Some(ok_binding) = ctx.last_succeeded_binding {
        let after_emit = acc.instructions.len() + 2;
        acc.push(Instruction::BranchCondition {
            condition: Box::new(EngineConditionExpr::BoundBool(ok_binding)),
            destination: after_emit,
        });
    }
    acc.push(Instruction::EmitReflexiveTrigger {
        body: Box::new(sub_body),
    });
    Ok(())
}

fn lower_unless(
    body: &[v2::Effect],
    payer: Option<v2::PlayerRef>,
    cost: Option<&v2::Cost>,
    cond: Option<&v2::ConditionExpr>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if body.is_empty() {
        return Err(LoweringError::Unsupported("v2:unless-empty-body"));
    }
    match (cost, cond) {
        (None, Some(cond_expr)) => {
            let lowered = condition::lower_condition_expr(cond_expr, ctx)?;
            let branch_idx = acc.instructions.len();
            acc.push(Instruction::BranchCondition {
                condition: Box::new(EngineConditionExpr::Not(Box::new(lowered))),
                destination: 0,
            });
            lower_effects_into(body, acc, ctx)?;
            let after_body = acc.instructions.len();
            if let Some(Instruction::BranchCondition { destination, .. }) =
                acc.instructions.get_mut(branch_idx)
            {
                *destination = after_body;
            }
            Ok(())
        }
        (Some(cost_ir), None) => {
            // Lower `body` into a staging accumulator first so its target is registered
            // before resolving `payer` (e.g. `Counter target spell unless its controller pays {2}`).
            let mut body_acc = acc.clone();
            body_acc.instructions.clear();
            lower_effects_into(body, &mut body_acc, ctx)?;
            if !acc.target_set && body_acc.target_set {
                acc.target = body_acc.target;
                acc.target_set = true;
                acc.target_bounds = body_acc.target_bounds;
                acc.up_to_one = body_acc.up_to_one;
                acc.x_target_count = body_acc.x_target_count;
                acc.up_to_x_target_count = body_acc.up_to_x_target_count;
            }

            if matches!(cost_ir, v2::Cost::Mana(_) | v2::Cost::ScaledMana { .. })
                && !is_energy_only_mana_cost(cost_ir)
            {
                let (fixed, extra_generic) = zone_op::lower_unless_mana_cost(cost_ir, ctx)?;
                let payer_rp = match payer {
                    Some(p) => atoms::lower_resolving_player(p, acc, ctx)?,
                    None => ResolvingPlayer::TargetController,
                };
                let prompt_idx = acc.instructions.len();
                let body_start = prompt_idx + 1;
                acc.push(Instruction::PromptChoice(Box::new(
                    PromptChoiceSpec::offer_dynamic_mana_payment(
                        payer_rp,
                        fixed,
                        extra_generic,
                        0,
                        body_start,
                    ),
                )));
                splice_untargeted_instructions(acc, body_acc.instructions);
                let after_body = acc.instructions.len();
                if let Some(Instruction::PromptChoice(spec)) = acc.instructions.get_mut(prompt_idx)
                    && let Some((_, paid_dest, _)) = spec.as_mana_payment_mut()
                {
                    *paid_dest = after_body;
                }
                return Ok(());
            }

            // Non-mana unless cost (`Sacrifice`, `Discard`, `PayLife`, `RemoveCounters`, etc.).
            let payer_v2 = payer.unwrap_or(if acc.target_set {
                v2::PlayerRef::ItsController
            } else {
                v2::PlayerRef::You
            });
            let payer_rp = atoms::lower_resolving_player(payer_v2, acc, ctx)?;
            let prev_succeeded = ctx.last_succeeded_binding;
            let mut cost_acc = acc.clone();
            cost_acc.instructions.clear();
            mutation::lower_player_mutation(
                payer_v2,
                &v2::PlayerMutation::PayCost(cost_ir.clone()),
                &v2::Duration::Instant,
                &mut cost_acc,
                ctx,
            )?;
            if cost_acc.instructions.is_empty() {
                return Err(LoweringError::Unsupported("v2:unless-pay-non-mana-cost"));
            }
            validate_optional_slice(&cost_acc.instructions)?;
            if !acc.target_set && cost_acc.target_set {
                acc.target = cost_acc.target;
                acc.target_set = true;
                acc.target_bounds = cost_acc.target_bounds;
                acc.up_to_one = cost_acc.up_to_one;
                acc.x_target_count = cost_acc.x_target_count;
                acc.up_to_x_target_count = cost_acc.up_to_x_target_count;
            }

            let cost_len = cost_acc.instructions.len();
            let (paid_slot, append_true_bind) = if ctx.last_succeeded_binding != prev_succeeded
                && let Some(ok_slot) = ctx.last_succeeded_binding
            {
                (ok_slot, false)
            } else {
                (ctx.alloc_binding()?, true)
            };

            acc.push(Instruction::BindEnv {
                binding: paid_slot,
                value: Box::new(ExecValue::Bool(false)),
            });
            acc.push(Instruction::OptionalSequence {
                chooser: payer_rp,
                instructions: if append_true_bind {
                    cost_len + 1
                } else {
                    cost_len
                },
            });
            splice_untargeted_instructions(acc, cost_acc.instructions);
            if append_true_bind {
                acc.push(Instruction::BindEnv {
                    binding: paid_slot,
                    value: Box::new(ExecValue::Bool(true)),
                });
            }

            let branch_idx = acc.instructions.len();
            acc.push(Instruction::BranchCondition {
                condition: Box::new(EngineConditionExpr::Not(Box::new(
                    EngineConditionExpr::BoundBool(paid_slot),
                ))),
                destination: 0,
            });
            splice_untargeted_instructions(acc, body_acc.instructions);
            let after_body = acc.instructions.len();
            if let Some(Instruction::BranchCondition { destination, .. }) =
                acc.instructions.get_mut(branch_idx)
            {
                *destination = after_body;
            }
            Ok(())
        }
        _ => Err(LoweringError::Unsupported("v2:unless-ambiguous-clause")),
    }
}

fn lower_for_each(
    over: &v2::ForEachDomain,
    body: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match over {
        v2::ForEachDomain::Player(player_ref) => {
            let selector = atoms::lower_player_ref(*player_ref, acc, ctx)?;
            let prev = ctx.in_loop_player;
            ctx.in_loop_player = true;
            let sub_body_res = lower_effect_body(body, ctx);
            ctx.in_loop_player = prev;
            let sub_body = sub_body_res?;
            acc.push(Instruction::ForEachPlayerQuery {
                selector: Box::new(selector),
                bind_player: BindingId::LOOP_PLAYER,
                body: Box::new(sub_body),
            });
            Ok(())
        }
        v2::ForEachDomain::Object(obj_filter) => {
            register_filter_player_targets(obj_filter, acc, ctx)?;
            let ef = filter::lower_object_filter(obj_filter, ctx)?;
            let zone = if obj_filter.zones.is_empty() {
                QueryZone::Battlefield
            } else if obj_filter.zones.len() == 1 {
                atoms::lower_zone_ref(obj_filter.zones[0], acc, ctx)?
            } else {
                return Err(LoweringError::Unsupported("v2:for-each-multi-zone"));
            };
            let bind_item = ctx.alloc_binding()?;
            let prev = ctx.in_loop_object;
            ctx.in_loop_object = Some(bind_item);
            let sub_body_res = lower_effect_body(body, ctx);
            ctx.in_loop_object = prev;
            let sub_body = sub_body_res?;
            acc.push(Instruction::ForEach {
                query: Box::new(ObjectQuery::zone(zone, ef)),
                bind_item,
                body: Box::new(sub_body),
            });
            Ok(())
        }
        v2::ForEachDomain::Count(expr) | v2::ForEachDomain::EachUnitOfValue(expr) => {
            register_value_expr_player_targets(expr, acc, ctx)?;
            let count = value::lower_value_expr(expr, ctx)?;
            let sub_body = lower_effect_body(body, ctx)?;
            acc.push(Instruction::RepeatLoop {
                count,
                body: Box::new(sub_body),
            });
            Ok(())
        }
        v2::ForEachDomain::PlayersMatching(_) => Err(LoweringError::Unsupported(
            "v2:for-each-players-matching",
        )),
        v2::ForEachDomain::EachPlayerWhoCant(_) => Err(LoweringError::Unsupported(
            "v2:for-each-player-who-cant",
        )),
        v2::ForEachDomain::EachPlayerWhoDoes(_) => Err(LoweringError::Unsupported(
            "v2:for-each-player-who-does",
        )),
        v2::ForEachDomain::GraveyardWith(_) => {
            Err(LoweringError::Unsupported("v2:for-each-graveyard-with"))
        }
        v2::ForEachDomain::ColorAmong(_) => {
            Err(LoweringError::Unsupported("v2:for-each-color-among"))
        }
        v2::ForEachDomain::CardType => Err(LoweringError::Unsupported("v2:for-each-card-type")),
        v2::ForEachDomain::EachOpponentWithCondition(_) => Err(LoweringError::Unsupported(
            "v2:for-each-opponent-with-condition",
        )),
    }
}

fn lower_bind_variable(
    var: v2::VarName,
    expr: &v2::ValueExpr,
    body: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match var {
        v2::VarName::X => {
            register_value_expr_player_targets(expr, acc, ctx)?;
            let val = value::lower_value_expr(expr, ctx)?;
            acc.push(Instruction::MutatePlayer(Box::new(
                MutatePlayerSpec::bind_resolution_x(val),
            )));
            lower_effects_into(body, acc, ctx)
        }
        v2::VarName::Y => Err(LoweringError::Unsupported("v2:bind-variable-y")),
    }
}

fn lower_delayed_trigger(
    delayed: &v2::DelayedTriggerKind,
    body: &[v2::Effect],
    explicit_expiry: Option<ResolvedDuration>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match delayed {
        v2::DelayedTriggerKind::Step { step, owner } => {
            if explicit_expiry.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:delayed-step-trigger-with-scoped-duration",
                ));
            }
            let engine_step = atoms::lower_step(*step)?;
            let own_step_only = match owner {
                v2::StepOwner::Your | v2::StepOwner::YourNext => true,
                v2::StepOwner::Next
                | v2::StepOwner::Each
                | v2::StepOwner::The
                | v2::StepOwner::NextTurn => false,
                _ => {
                    return Err(LoweringError::Unsupported(
                        "v2:delayed-trigger-step-owner",
                    ));
                }
            };
            let sub_body = lower_effect_body(body, ctx)?;
            acc.push(Instruction::ScheduleDelayedStepTrigger {
                step: engine_step,
                own_step_only,
                body: Box::new(sub_body),
            });
            Ok(())
        }
        v2::DelayedTriggerKind::Event(trig_spec) => {
            let prev_in_trig = ctx.in_trigger;
            ctx.in_trigger = true;
            let lowered = trigger::lower_trigger_events(trig_spec, ctx);
            let sub_body_res = lower_effect_body(body, ctx);
            ctx.in_trigger = prev_in_trig;

            let (specs, _grouping, intervening) = lowered?;
            let sub_body = sub_body_res?;
            if intervening.is_some() {
                return Err(LoweringError::Unsupported(
                    "v2:delayed-event-trigger-intervening-if",
                ));
            }
            if specs.is_empty() {
                return Err(LoweringError::Unsupported("v2:delayed-event-trigger-empty"));
            }
            let expires = explicit_expiry.unwrap_or(ResolvedDuration::UntilEndOfTurn);
            for spec in specs {
                acc.push(Instruction::ScheduleDelayedEventTrigger {
                    trigger: Box::new(spec),
                    body: Box::new(sub_body.clone()),
                    expires,
                    once: false,
                });
            }
            Ok(())
        }
    }
}

fn lower_scoped_duration(
    outer_duration: &v2::Duration,
    body: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if body.is_empty() {
        return Err(LoweringError::Unsupported("v2:scoped-duration-empty"));
    }
    if body.len() > 1 {
        for eff in body {
            let wrapped = v2::Effect::ScopedDuration {
                duration: outer_duration.clone(),
                body: vec![eff.clone()],
            };
            lower_step_with_target_dispatch(&wrapped, acc, ctx)?;
        }
        return Ok(());
    }
    for eff in body {
        match eff {
            v2::Effect::MutateObject {
                subject,
                mutation: obj_mut,
                duration: inner_dur,
            } => {
                let effective = merge_scoped_duration(outer_duration, inner_dur)?;
                mutation::lower_object_mutation(subject, obj_mut, effective, acc, ctx)?;
            }
            v2::Effect::MutatePlayer {
                player,
                mutation: player_mut,
                duration: inner_dur,
            } => {
                let effective = if matches!(player_mut, v2::PlayerMutation::GrantPlayPermission { .. })
                    && *inner_dur == v2::Duration::UntilEndOfTurn
                    && matches!(
                        outer_duration,
                        v2::Duration::UntilEndOfNextTurn
                            | v2::Duration::UntilYourNextEndStep
                            | v2::Duration::UntilYourNextTurn
                            | v2::Duration::UntilYourNextUpkeep
                            | v2::Duration::ForAsLongAsExiled
                            | v2::Duration::ForTheRestOfTheGame
                            | v2::Duration::Permanent
                    )
                {
                    outer_duration
                } else {
                    merge_scoped_duration(outer_duration, inner_dur)?
                };
                mutation::lower_player_mutation(*player, player_mut, effective, acc, ctx)?;
            }
            v2::Effect::ApplyContinuous {
                effect,
                duration: inner_dur,
            } => {
                let effective = merge_scoped_duration(outer_duration, inner_dur)?;
                let dur = atoms::lower_duration(effective)?;
                static_ability::lower_temporary_static(effect, dur, acc, ctx)?;
            }
            v2::Effect::ApplyReplacement {
                replacement: repl,
                duration: inner_dur,
            } => {
                let effective = merge_scoped_duration(outer_duration, inner_dur)?;
                let dur = atoms::lower_duration(effective)?;
                replacement::lower_temporary_replacement(repl, dur, acc, ctx)?;
            }
            v2::Effect::PreventDamage {
                amount,
                combat_only,
                source,
                target,
                duration: inner_dur,
            } => {
                let effective = merge_scoped_duration(outer_duration, inner_dur)?;
                lower_prevent_damage(
                    amount.as_ref(),
                    *combat_only,
                    source.as_ref(),
                    target.as_ref(),
                    effective,
                    acc,
                    ctx,
                )?;
            }
            v2::Effect::DelayedTrigger {
                trigger: dt,
                body: inner_body,
            } => {
                let dur = atoms::lower_duration(outer_duration)?;
                lower_delayed_trigger(dt, inner_body, Some(dur), acc, ctx)?;
            }
            v2::Effect::ZoneOp {
                actor,
                op:
                    v2::ZoneOpKind::Exile {
                        face_down,
                        counters: None,
                    },
                subject,
                from,
                to,
                modifiers,
                except,
                count,
            } if *outer_duration == v2::Duration::UntilSourceLeaves => {
                zone_op::lower_exile_until_source_leaves(
                    *actor,
                    *face_down,
                    subject,
                    from,
                    to.as_ref(),
                    modifiers,
                    except.as_deref(),
                    count.as_ref(),
                    acc,
                    ctx,
                )?;
            }
            v2::Effect::ZoneOp {
                actor,
                op:
                    op @ (v2::ZoneOpKind::Destroy { .. }
                    | v2::ZoneOpKind::Exile { counters: None, .. }
                    | v2::ZoneOpKind::Sacrifice),
                subject,
                from,
                to,
                modifiers,
                except,
                count: None,
            } if *outer_duration == v2::Duration::UntilEndOfCombat => {
                lower_delayed_end_of_combat_zone_op(
                    *actor,
                    op,
                    subject,
                    from,
                    to.as_ref(),
                    modifiers,
                    except.as_deref(),
                    acc,
                    ctx,
                )?;
            }
            v2::Effect::BranchCondition {
                condition: cond,
                then_body,
                else_body,
            } => {
                let wrapped_then = if then_body.is_empty() {
                    Vec::new()
                } else {
                    vec![v2::Effect::ScopedDuration {
                        duration: outer_duration.clone(),
                        body: then_body.clone(),
                    }]
                };
                let wrapped_else = if else_body.is_empty() {
                    Vec::new()
                } else {
                    vec![v2::Effect::ScopedDuration {
                        duration: outer_duration.clone(),
                        body: else_body.clone(),
                    }]
                };
                lower_branch_condition(cond, &wrapped_then, &wrapped_else, acc, ctx)?;
            }
            v2::Effect::Optional {
                actor,
                body: inner_body,
            } => {
                let wrapped = vec![v2::Effect::ScopedDuration {
                    duration: outer_duration.clone(),
                    body: inner_body.clone(),
                }];
                lower_optional(*actor, &wrapped, acc, ctx)?;
            }
            v2::Effect::ForEach {
                over,
                body: inner_body,
            } => {
                let wrapped = vec![v2::Effect::ScopedDuration {
                    duration: outer_duration.clone(),
                    body: inner_body.clone(),
                }];
                lower_for_each(over, &wrapped, acc, ctx)?;
            }
            v2::Effect::IfPrevious {
                succeeded,
                body: inner_body,
            } => {
                let wrapped = vec![v2::Effect::ScopedDuration {
                    duration: outer_duration.clone(),
                    body: inner_body.clone(),
                }];
                lower_if_previous(*succeeded, &wrapped, acc, ctx)?;
            }
            v2::Effect::ReflexiveTrigger(inner_body) => {
                let wrapped = vec![v2::Effect::ScopedDuration {
                    duration: outer_duration.clone(),
                    body: inner_body.clone(),
                }];
                lower_reflexive_trigger(&wrapped, acc, ctx)?;
            }
            v2::Effect::ScopedDuration {
                duration: inner_dur,
                body: inner_body,
            } => {
                let effective = merge_scoped_duration(outer_duration, inner_dur)?;
                lower_scoped_duration(effective, inner_body, acc, ctx)?;
            }
            _ => {
                return Err(LoweringError::Unsupported(
                    "v2:scoped-duration-inner-unsupported",
                ));
            }
        }
    }
    Ok(())
}

fn merge_scoped_duration<'a>(
    outer: &'a v2::Duration,
    inner: &'a v2::Duration,
) -> LowerResult<&'a v2::Duration> {
    if matches!(inner, v2::Duration::Instant | v2::Duration::Permanent)
        || inner == outer
        || (*inner == v2::Duration::UntilEndOfTurn
            && matches!(
                outer,
                v2::Duration::UntilYourNextTurn
                    | v2::Duration::UntilYourNextEndStep
                    | v2::Duration::UntilYourNextUpkeep
                    | v2::Duration::UntilEndOfNextTurn
                    | v2::Duration::ForAsLongAs(_)
            ))
    {
        Ok(outer)
    } else {
        Err(LoweringError::Unsupported("v2:scoped-duration-conflict"))
    }
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_delayed_end_of_combat_zone_op(
    actor: v2::PlayerRef,
    op: &v2::ZoneOpKind,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let destination = match op {
        v2::ZoneOpKind::Destroy { can_regenerate } => {
            if !is_empty_entry_modifiers(modifiers) {
                return Err(LoweringError::Unsupported("v2:destroy-with-modifiers"));
            }
            if let Some(dest) = to
                && dest.zone != v2::ZoneKind::Graveyard
            {
                return Err(LoweringError::Unsupported("v2:destroy-non-graveyard-dest"));
            }
            EngineZoneDest::Graveyard {
                semantics: GraveyardSemantics::Destroy {
                    allow_regenerate: *can_regenerate,
                },
            }
        }
        v2::ZoneOpKind::Sacrifice => {
            if !is_empty_entry_modifiers(modifiers) {
                return Err(LoweringError::Unsupported("v2:sacrifice-with-modifiers"));
            }
            if let Some(dest) = to
                && dest.zone != v2::ZoneKind::Graveyard
            {
                return Err(LoweringError::Unsupported("v2:sacrifice-non-graveyard-dest"));
            }
            if !from.is_empty() && from.iter().any(|z| z.zone != v2::ZoneKind::Battlefield) {
                return Err(LoweringError::Unsupported("v2:sacrifice-non-battlefield-zone"));
            }
            EngineZoneDest::Graveyard {
                semantics: GraveyardSemantics::Sacrifice,
            }
        }
        v2::ZoneOpKind::Exile {
            face_down,
            counters: None,
        } => {
            if modifiers.tapped
                || modifiers.attacking
                || modifiers.transformed
                || modifiers.with_counter.is_some()
                || modifiers.attached_to.is_some()
            {
                return Err(LoweringError::Unsupported("v2:exile-with-modifiers"));
            }
            if let Some(dest) = to
                && dest.zone != v2::ZoneKind::Exile
            {
                return Err(LoweringError::Unsupported("v2:exile-non-exile-dest"));
            }
            let is_face_down = *face_down || modifiers.face_down;
            EngineZoneDest::Exile {
                face_down: is_face_down,
                viewer: is_face_down
                    .then_some(PlayerSelector::Resolving(ResolvingPlayer::Controller)),
                link_to_source: false,
                delayed_return: None,
                link_group: None,
            }
        }
        _ => {
            return Err(LoweringError::Unsupported(
                "v2:scoped-duration-inner-unsupported",
            ));
        }
    };

    register_player_ref_target(actor, acc, ctx)?;
    register_subject_filter_player_targets(subject, acc, ctx)?;

    let mut outer_query = if from.is_empty() {
        atoms::lower_subject_to_query(subject, acc, ctx)?
    } else {
        match subject {
            v2::SubjectRef::Each(f) | v2::SubjectRef::All(f) | v2::SubjectRef::Group(f) => {
                let ef = filter::lower_object_filter(f, ctx)?;
                if from.len() == 1 {
                    ObjectQuery::zone(atoms::lower_zone_ref(from[0], acc, ctx)?, ef)
                } else {
                    let mut parts = Vec::with_capacity(from.len());
                    for z in from {
                        parts.push(ObjectQuery::zone(atoms::lower_zone_ref(*z, acc, ctx)?, ef));
                    }
                    ObjectQuery::Union(parts)
                }
            }
            other => atoms::lower_subject_to_query(other, acc, ctx)?,
        }
    };
    if let Some(exc) = except {
        let exc_q = atoms::lower_subject_to_query(exc, acc, ctx)?;
        outer_query = ObjectQuery::difference(outer_query, exc_q);
    }

    let tag_slot = ctx.alloc_binding()?;
    let tag_key = TagKey::Label(format!("v2:eoc:{}", tag_slot.0));
    acc.push(Instruction::SetObjectTag {
        query: Box::new(outer_query),
        key: tag_key.clone(),
        value: Some(TagValue::Unit),
    });

    let tagged_bf = ObjectQuery::Tagged {
        key: tag_key.clone(),
        value: Some(TagValue::Unit),
        zone: Some(QueryZone::Battlefield),
        filter: None,
    };
    let tagged_any = ObjectQuery::Tagged {
        key: tag_key.clone(),
        value: Some(TagValue::Unit),
        zone: None,
        filter: None,
    };
    let delayed_body = ExecutableBody::new(
        BodyTarget::None,
        vec![
            Instruction::ZoneOp(Box::new(ZoneOpSpec::move_all(tagged_bf, destination))),
            Instruction::SetObjectTag {
                query: Box::new(tagged_any),
                key: tag_key,
                value: None,
            },
        ],
    );
    acc.push(Instruction::ScheduleDelayedStepTrigger {
        step: Step::EndCombat,
        own_step_only: false,
        body: Box::new(delayed_body),
    });
    Ok(())
}

fn lower_starting_with(
    player: v2::PlayerRef,
    body: &[v2::Effect],
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if player == v2::PlayerRef::ActivePlayer {
        return lower_effects_into(body, acc, ctx);
    }
    Err(LoweringError::Unsupported("v2:effect-starting-with"))
}

fn lower_exchange_control(
    subject: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    match subject {
        v2::SubjectRef::Compound(parts) if parts.len() == 2 => {
            if !acc.target_set
                && matches!(parts[0], v2::SubjectRef::TargetObject { .. })
                && matches!(parts[1], v2::SubjectRef::TargetObject { .. })
                && let Ok((bt1, tc1)) = atoms::lower_subject_to_target(&parts[0], ctx)
                && let Ok((bt2, tc2)) = atoms::lower_subject_to_target(&parts[1], ctx)
                && matches!(
                    bt1,
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        ..
                    }
                )
                && matches!(
                    bt2,
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        ..
                    }
                )
                && bt1 != bt2
                && matches!(tc1, v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1))
                && matches!(tc2, v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1))
            {
                let mut c1 = BodyAcc::new();
                c1.register_target(bt1, tc1)?;
                c1.push(Instruction::MutateObject(Box::new(
                    MutateObjectSpec::remember_permanent_target(),
                )));
                let mut c2 = BodyAcc::new();
                c2.register_target(bt2, tc2)?;
                c2.push(Instruction::MutateObject(Box::new(
                    MutateObjectSpec::remember_permanent_target(),
                )));
                let b1 = c1.into_body()?;
                let b2 = c2.into_body()?;
                acc.push(Instruction::FixedTargetClause(Box::new(b1)));
                acc.push(Instruction::FixedTargetClause(Box::new(b2)));
                acc.push(Instruction::MutateObject(Box::new(
                    MutateObjectSpec::exchange_control(
                        ObjectQuery::LinkedTarget { index: 0 },
                        ObjectQuery::LinkedTarget { index: 1 },
                    ),
                )));
                return Ok(());
            }
            let first = atoms::lower_subject_to_query(&parts[0], acc, ctx)?;
            let second = atoms::lower_subject_to_query(&parts[1], acc, ctx)?;
            acc.push(Instruction::MutateObject(Box::new(MutateObjectSpec::all(
                first,
                EngineObjectMutation::Control(ControlOp::Exchange {
                    other: Box::new(second),
                }),
            ))));
            Ok(())
        }
        v2::SubjectRef::TargetObject {
            count: v2::TargetCount::Exact(2),
            ..
        } => Err(LoweringError::Unsupported(
            "v2:exchange-control-two-same-targets",
        )),
        _ => Err(LoweringError::Unsupported("v2:exchange-control-subject")),
    }
}
