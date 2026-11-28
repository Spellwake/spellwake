//! Strictly-typed `v2::ir` zone-operation lowering.
//!
//! Lowers all 16 `v2::ZoneOpKind` variants into native `spellwake_engine`
//! toolbox instructions (`Instruction::ZoneOp`, `Instruction::DrawCards`,
//! `Instruction::SearchLibrary`, `Instruction::StackOp`,
//! `Instruction::PromptChoice`, `Instruction::ForEachPlayerQuery`).

use std::collections::BTreeSet;
use std::num::NonZeroU8;

use spellwake_contracts::names::EnglishCardName;
use spellwake_engine::{
    land::LandType,
    mana::ManaCost,
    program::{
        BodyTarget, CardType, DelayedReturnTiming, ExecutableBody, Instruction, LibrarySearch,
        MoveSubjectRef, ObjectFilter as EngineObjectFilter, ResolvingPlayer,
        SearchBattlefieldPostEntry, SearchCardMatcher, SearchCount, SearchDestination,
        SearchExileLink, SearchSource, TargetZone, ValueExpr as EngineValueExpr,
    },
    toolbox::{
        BindingId, ChoiceCategory, CounterDestination, DrawCardsSpec, GraveyardSemantics,
        LibraryOrderPos, ObjectQuery, PlayerSelector, PromptChoiceSpec, PromptDomain, QuerySlice,
        QueryZone, SelectionConstraint, StackOpKind, StackOpSpec, StackTargetSelector, TagKey,
        ZoneDest as EngineZoneDest, ZoneOpSpec, ZoneSelection,
    },
};
use spellwake_oracle_compiler::{ast, v2::ir as v2};

use crate::v2_lowering::{
    atoms,
    ctx::{BodyAcc, LowerCtx},
    effect,
    error::{LowerResult, LoweringError},
    filter, value,
};

/// Lower `Effect::ZoneOp` into native toolbox instructions on `acc`.
// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_zone_op(
    actor: v2::PlayerRef,
    op: &v2::ZoneOpKind,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if actor == v2::PlayerRef::AnyOpponent {
        effect::emit_choose_any_opponent(acc, ctx)?;
        return lower_zone_op(
            v2::PlayerRef::ChosenPlayer,
            op,
            subject,
            from,
            to,
            modifiers,
            except,
            count,
            acc,
            ctx,
        );
    }
    match actor {
        v2::PlayerRef::TargetPlayer => {
            acc.register_target(BodyTarget::SINGLE_PLAYER, v2::TargetCount::Exact(1))?;
            ctx.last_player_target = Some(ResolvingPlayer::TargetPlayer);
        }
        v2::PlayerRef::TargetOpponent => {
            acc.register_target(BodyTarget::SINGLE_OPPONENT, v2::TargetCount::Exact(1))?;
            ctx.last_player_target = Some(ResolvingPlayer::TargetPlayer);
        }
        v2::PlayerRef::ItsController
        | v2::PlayerRef::ItsOwner
        | v2::PlayerRef::ThatObjectController
        | v2::PlayerRef::ThatSpellController
            if matches!(subject, v2::SubjectRef::TargetObject { .. }) =>
        {
            let (target, t_count) = atoms::lower_subject_to_target(subject, ctx)?;
            acc.register_target(target, t_count)?;
            ctx.has_object_target = true;
            if let Ok(rp) = atoms::lower_resolving_player(actor, acc, ctx) {
                ctx.last_player_target = Some(rp);
            }
        }
        v2::PlayerRef::ThatPlayer
            if !acc.target_set
                && !ctx.in_loop_player
                && !ctx.in_trigger
                && ctx.last_player_target.is_none()
                && matches!(subject, v2::SubjectRef::TargetObject { .. }) =>
        {
            let (target, t_count) = atoms::lower_subject_to_target(subject, ctx)?;
            acc.register_target(target, t_count)?;
            ctx.has_object_target = true;
            if let Ok(rp) = atoms::lower_resolving_player(actor, acc, ctx) {
                ctx.last_player_target = Some(rp);
            }
        }
        _ => {}
    }

    if !ctx.in_loop_player
        && is_multi_player_ref(actor)
        && should_loop_zone_op_per_player(op, subject, from, to)
    {
        let group_sel = atoms::lower_player_ref(actor, acc, ctx)?;
        let rewritten_from: Vec<v2::ZoneRef> = from
            .iter()
            .copied()
            .map(|z| rewrite_loop_zone_owner(z, actor))
            .collect();
        let prev_loop = ctx.in_loop_player;
        ctx.in_loop_player = true;
        let sub_res = (|| {
            let mut sub_acc = BodyAcc::new();
            lower_zone_op(
                v2::PlayerRef::ThatPlayer,
                op,
                subject,
                &rewritten_from,
                to,
                modifiers,
                except,
                count,
                &mut sub_acc,
                ctx,
            )?;
            sub_acc.into_body()
        })();
        ctx.in_loop_player = prev_loop;
        let sub_body = sub_res?;
        acc.push(Instruction::ForEachPlayerQuery {
            selector: Box::new(group_sel),
            bind_player: BindingId::LOOP_PLAYER,
            body: Box::new(sub_body),
        });
        return Ok(());
    }
    match op {
        v2::ZoneOpKind::Move => {
            lower_move(actor, subject, from, to, modifiers, except, count, acc, ctx)
        }
        v2::ZoneOpKind::Destroy { can_regenerate } => lower_destroy(
            actor,
            *can_regenerate,
            subject,
            from,
            to,
            modifiers,
            except,
            count,
            acc,
            ctx,
        ),
        v2::ZoneOpKind::Exile {
            face_down,
            counters,
        } => lower_exile(
            actor,
            *face_down,
            counters.as_ref(),
            subject,
            from,
            to,
            modifiers,
            except,
            count,
            acc,
            ctx,
        ),
        v2::ZoneOpKind::Sacrifice => {
            lower_sacrifice(actor, subject, from, to, modifiers, except, count, acc, ctx)
        }
        v2::ZoneOpKind::Discard { hand, random } => lower_discard(
            actor, *hand, *random, subject, from, to, modifiers, except, count, acc, ctx,
        ),
        v2::ZoneOpKind::Draw => {
            lower_draw(actor, subject, from, to, modifiers, except, count, acc, ctx)
        }
        v2::ZoneOpKind::Mill => {
            lower_mill(actor, subject, from, to, modifiers, except, count, acc, ctx)
        }
        v2::ZoneOpKind::SearchLibrary {
            reveal,
            shuffle,
            exact,
        } => lower_search_library(
            actor, *reveal, *shuffle, *exact, subject, from, to, modifiers, except, count, acc, ctx,
        ),
        v2::ZoneOpKind::Shuffle => {
            lower_shuffle(actor, subject, from, to, modifiers, except, count, acc, ctx)
        }
        v2::ZoneOpKind::LookTop { reveal } => lower_look_top(
            actor, *reveal, subject, from, to, modifiers, except, count, acc, ctx,
        ),
        v2::ZoneOpKind::LookAt => {
            lower_look_at(actor, subject, from, to, modifiers, except, count, acc, ctx)
        }
        v2::ZoneOpKind::Reveal => {
            lower_reveal(actor, subject, from, to, modifiers, except, count, acc, ctx)
        }
        v2::ZoneOpKind::CounterSpell { unless_pay } => lower_counter_spell(
            actor,
            unless_pay.as_deref(),
            subject,
            from,
            to,
            modifiers,
            except,
            count,
            acc,
            ctx,
        ),
        v2::ZoneOpKind::SeparateIntoPiles {
            pile_count,
            face_down_piles,
        } => lower_separate_into_piles(
            actor,
            *pile_count,
            *face_down_piles,
            subject,
            from,
            to,
            modifiers,
            except,
            count,
            acc,
            ctx,
        ),
        v2::ZoneOpKind::ChoosePile => Err(LoweringError::Unsupported("v2:zone-op-choose-pile")),
        v2::ZoneOpKind::ExileUntilReveal {
            filter: stop_filter,
        } => lower_exile_until_reveal(
            actor,
            stop_filter,
            subject,
            from,
            to,
            modifiers,
            except,
            count,
            acc,
            ctx,
        ),
    }
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_move(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let Some(dest_ir) = to else {
        return Err(LoweringError::Unsupported("v2:zone-move-missing-dest"));
    };
    let destination = atoms::lower_zone_dest(dest_ir, modifiers, acc, ctx)?;
    let built = build_source_and_selection(actor, subject, from, except, count, false, acc, ctx)?;
    emit_zone_op_with_prior(built, destination, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_destroy(
    actor: v2::PlayerRef,
    can_regenerate: bool,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:destroy-with-modifiers")?;
    if let Some(dest) = to
        && dest.zone != v2::ZoneKind::Graveyard
    {
        return Err(LoweringError::Unsupported("v2:destroy-non-graveyard-dest"));
    }
    let built = build_source_and_selection(actor, subject, from, except, count, false, acc, ctx)?;
    let destination = EngineZoneDest::Graveyard {
        semantics: GraveyardSemantics::Destroy {
            allow_regenerate: can_regenerate,
        },
    };
    emit_zone_op_with_prior(built, destination, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_exile(
    actor: v2::PlayerRef,
    face_down: bool,
    counters: Option<&(v2::CounterKind, v2::ValueExpr)>,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if counters.is_some() {
        return Err(LoweringError::Unsupported("v2:exile-with-counters"));
    }
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
    let is_face_down = face_down || modifiers.face_down;
    let built = build_source_and_selection(actor, subject, from, except, count, false, acc, ctx)?;
    let destination = EngineZoneDest::Exile {
        face_down: is_face_down,
        viewer: is_face_down.then_some(PlayerSelector::Resolving(ResolvingPlayer::Controller)),
        link_to_source: false,
        delayed_return: None,
        link_group: None,
    };
    emit_zone_op_with_prior(built, destination, None, acc, ctx)
}

/// Lower `Effect::ZoneOp` (`Exile`) wrapped in `Duration::UntilSourceLeaves` (e.g. Banishing Light).
// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_exile_until_source_leaves(
    actor: v2::PlayerRef,
    face_down: bool,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
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
    let is_face_down = face_down || modifiers.face_down;
    let built = build_source_and_selection(actor, subject, from, except, count, false, acc, ctx)?;
    let destination = EngineZoneDest::Exile {
        face_down: is_face_down,
        viewer: is_face_down.then_some(PlayerSelector::Resolving(ResolvingPlayer::Controller)),
        link_to_source: true,
        delayed_return: Some(DelayedReturnTiming::UntilSourceLeaves),
        link_group: None,
    };
    emit_zone_op_with_prior(built, destination, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_sacrifice(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:sacrifice-with-modifiers")?;
    if let Some(dest) = to
        && dest.zone != v2::ZoneKind::Graveyard
    {
        return Err(LoweringError::Unsupported(
            "v2:sacrifice-non-graveyard-dest",
        ));
    }
    if !from.is_empty() && from.iter().any(|z| z.zone != v2::ZoneKind::Battlefield) {
        return Err(LoweringError::Unsupported(
            "v2:sacrifice-non-battlefield-zone",
        ));
    }

    let destination = EngineZoneDest::Graveyard {
        semantics: GraveyardSemantics::Sacrifice,
    };

    if matches!(subject, v2::SubjectRef::OneOfThem(_)) || subject_has_among_prior(subject) {
        let built =
            build_source_and_selection(actor, subject, from, except, count, false, acc, ctx)?;
        return emit_zone_op_with_prior(built, destination, None, acc, ctx);
    }

    let (eff_count, choice_filter) = extract_sacrifice_choice_shape(subject, count);
    if let Some((cs_opt, opt_filter)) = choice_filter {
        let (clean_filter, chosen_player, is_random) = match opt_filter {
            Some(f) => {
                let (f1, cp) = strip_of_chosen_player(f);
                let (f2, rnd) = strip_at_random(&f1);
                (Some(f2), cp, rnd)
            }
            None => (None, None, false),
        };
        let engine_filter = match clean_filter.as_ref() {
            Some(f) => filter::lower_object_filter(f, ctx)?,
            None => EngineObjectFilter::any(),
        };
        let constraint = clean_filter
            .as_ref()
            .and_then(selection_constraint_from_filter);
        let (min, max) = match cs_opt {
            Some(cs) => lower_count_bounds(cs, acc, ctx)?,
            None => (EngineValueExpr::literal(1), EngineValueExpr::literal(1)),
        };
        if is_random && cs_opt.is_some_and(|cs| cs.up_to || cs.any_number) {
            return Err(LoweringError::Unsupported("v2:sacrifice-random-up-to"));
        }
        let exc_q = except
            .map(|e| atoms::lower_subject_to_query(e, acc, ctx))
            .transpose()?;

        if is_multi_player_ref(actor) {
            let group_sel = atoms::lower_player_ref(actor, acc, ctx)?;
            let loop_player = PlayerSelector::Bound(BindingId::LOOP_PLAYER);
            let prev_loop = ctx.in_loop_player;
            ctx.in_loop_player = true;
            let chooser_res = match chosen_player {
                None | Some(v2::PlayerRef::ThatPlayer) => Ok(loop_player.clone()),
                Some(cp) if is_multi_player_ref(cp) => Ok(loop_player.clone()),
                Some(cp) => atoms::lower_player_ref(cp, acc, ctx),
            };
            ctx.in_loop_player = prev_loop;
            let chooser = chooser_res?;
            let mut src = ObjectQuery::zone(
                QueryZone::BattlefieldControlledBy(Box::new(loop_player)),
                engine_filter,
            );
            if let Some(eq) = exc_q {
                src = ObjectQuery::difference(src, eq);
            }
            let selection = if is_random {
                ZoneSelection::Random {
                    count: Box::new(max),
                    chooser,
                }
            } else {
                ZoneSelection::Prompt {
                    chooser,
                    min: Box::new(min),
                    max: Box::new(max),
                    constraint,
                    category: ChoiceCategory::Generic,
                    hidden: false,
                }
            };
            let mut inner_spec = ZoneOpSpec::simple(src, selection, destination);
            let moved = ctx.alloc_binding()?;
            let ok = ctx.alloc_binding()?;
            inner_spec.bind_moved = Some(moved);
            inner_spec.bind_succeeded = Some(ok);
            ctx.last_moved_binding = Some(moved);
            ctx.last_succeeded_binding = Some(ok);
            acc.push(Instruction::ForEachPlayerQuery {
                selector: Box::new(group_sel),
                bind_player: BindingId::LOOP_PLAYER,
                body: Box::new(ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ZoneOp(Box::new(inner_spec))],
                )),
            });
            return Ok(());
        }

        let actor_sel = atoms::lower_player_ref(actor, acc, ctx)?;
        let chooser = resolve_choice_player(actor, chosen_player, &actor_sel, acc, ctx)?;
        let mut src = ObjectQuery::zone(
            QueryZone::BattlefieldControlledBy(Box::new(actor_sel)),
            engine_filter,
        );
        if let Some(eq) = exc_q {
            src = ObjectQuery::difference(src, eq);
        }
        let selection = if is_random {
            ZoneSelection::Random {
                count: Box::new(max),
                chooser,
            }
        } else {
            ZoneSelection::Prompt {
                chooser,
                min: Box::new(min),
                max: Box::new(max),
                constraint,
                category: ChoiceCategory::Generic,
                hidden: false,
            }
        };
        return emit_zone_op(src, selection, destination, None, acc, ctx);
    }

    if eff_count.is_some() {
        let built =
            build_source_and_selection(actor, subject, from, except, eff_count, false, acc, ctx)?;
        return emit_zone_op_with_prior(built, destination, None, acc, ctx);
    }

    let source = build_source_query(subject, from, except, acc, ctx)?;
    emit_zone_op(source, ZoneSelection::All, destination, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_discard(
    actor: v2::PlayerRef,
    whole_hand: bool,
    random: bool,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:discard-with-modifiers")?;
    if let Some(dest) = to
        && dest.zone != v2::ZoneKind::Graveyard
    {
        return Err(LoweringError::Unsupported("v2:discard-non-graveyard-dest"));
    }
    if !from.is_empty() && from.iter().any(|z| z.zone != v2::ZoneKind::Hand) {
        return Err(LoweringError::Unsupported("v2:discard-non-hand-zone"));
    }

    let destination = EngineZoneDest::Graveyard {
        semantics: GraveyardSemantics::Discard,
    };

    if whole_hand || matches!(subject, v2::SubjectRef::HandOf(_)) {
        let hand_owner = match subject {
            v2::SubjectRef::HandOf(p) => atoms::lower_player_ref(*p, acc, ctx)?,
            _ => atoms::lower_player_ref(actor, acc, ctx)?,
        };
        let mut src = ObjectQuery::zone(
            QueryZone::Hand(Box::new(hand_owner)),
            EngineObjectFilter::any(),
        );
        if let Some(exc) = except {
            let exc_q = atoms::lower_subject_to_query(exc, acc, ctx)?;
            src = ObjectQuery::difference(src, exc_q);
        }
        return emit_zone_op(src, ZoneSelection::All, destination, None, acc, ctx);
    }

    // Explicit targeted, anaphoric, prior-card, or one-of-them discard.
    if matches!(subject, v2::SubjectRef::OneOfThem(_)) || subject_has_among_prior(subject) {
        let built =
            build_source_and_selection(actor, subject, from, except, count, true, acc, ctx)?;
        return emit_zone_op_with_prior(built, destination, None, acc, ctx);
    }
    if matches!(
        subject,
        v2::SubjectRef::Anaphora(_) | v2::SubjectRef::TargetObject { .. }
    ) && count.is_none()
        && !random
    {
        let src = build_source_query(subject, from, except, acc, ctx)?;
        return emit_zone_op(src, ZoneSelection::All, destination, None, acc, ctx);
    }

    let (eff_count, filter_opt) = match subject {
        v2::SubjectRef::Counted {
            filter: f,
            count: c,
        } => (count.or(Some(c)), Some(f.as_ref())),
        v2::SubjectRef::Each(f) | v2::SubjectRef::All(f) | v2::SubjectRef::Group(f) => {
            (count, Some(f.as_ref()))
        }
        v2::SubjectRef::Player(p) if *p == actor => {
            if count.is_none() {
                return Err(LoweringError::Unsupported("v2:discard-missing-count"));
            }
            (count, None)
        }
        v2::SubjectRef::SelfObject => {
            if count.is_none() && !random {
                let src = build_source_query(subject, from, except, acc, ctx)?;
                return emit_zone_op(src, ZoneSelection::All, destination, None, acc, ctx);
            }
            return Err(LoweringError::Unsupported(
                "v2:discard-self-counted-or-random",
            ));
        }
        _ => {
            return Err(LoweringError::Unsupported("v2:discard-unsupported-subject"));
        }
    };

    let (clean_filter, chosen_player, filter_random) = match filter_opt {
        Some(f) => {
            let (f1, cp) = strip_of_chosen_player(f);
            let (f2, rnd) = strip_at_random(&f1);
            (Some(f2), cp, rnd)
        }
        None => (None, None, false),
    };
    let is_random = random || filter_random;

    let engine_filter = match clean_filter.as_ref() {
        Some(f) => filter::lower_object_filter(f, ctx)?,
        None => EngineObjectFilter::any(),
    };
    let constraint = clean_filter
        .as_ref()
        .and_then(selection_constraint_from_filter);
    let exc_q = except
        .map(|e| atoms::lower_subject_to_query(e, acc, ctx))
        .transpose()?;

    if is_random {
        let amt = match eff_count {
            Some(cs) => {
                if cs.up_to || cs.any_number {
                    return Err(LoweringError::Unsupported("v2:discard-random-up-to"));
                }
                effect::register_value_expr_player_targets(&cs.count, acc, ctx)?;
                value::lower_value_expr(&cs.count, ctx)?
            }
            None => EngineValueExpr::literal(1),
        };
        if is_multi_player_ref(actor) {
            let group_sel = atoms::lower_player_ref(actor, acc, ctx)?;
            let loop_player = PlayerSelector::Bound(BindingId::LOOP_PLAYER);
            let mut src = ObjectQuery::zone(
                QueryZone::Hand(Box::new(loop_player.clone())),
                engine_filter,
            );
            if let Some(eq) = exc_q {
                src = ObjectQuery::difference(src, eq);
            }
            let mut inner_spec = ZoneOpSpec::simple(
                src,
                ZoneSelection::Random {
                    count: Box::new(amt),
                    chooser: loop_player,
                },
                destination,
            );
            let moved = ctx.alloc_binding()?;
            let ok = ctx.alloc_binding()?;
            inner_spec.bind_moved = Some(moved);
            inner_spec.bind_succeeded = Some(ok);
            ctx.last_moved_binding = Some(moved);
            ctx.last_succeeded_binding = Some(ok);
            acc.push(Instruction::ForEachPlayerQuery {
                selector: Box::new(group_sel),
                bind_player: BindingId::LOOP_PLAYER,
                body: Box::new(ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::ZoneOp(Box::new(inner_spec))],
                )),
            });
            return Ok(());
        }

        let actor_sel = atoms::lower_player_ref(actor, acc, ctx)?;
        let chooser = resolve_choice_player(actor, chosen_player, &actor_sel, acc, ctx)?;
        let mut src = ObjectQuery::zone(QueryZone::Hand(Box::new(actor_sel)), engine_filter);
        if let Some(eq) = exc_q {
            src = ObjectQuery::difference(src, eq);
        }
        return emit_zone_op(
            src,
            ZoneSelection::Random {
                count: Box::new(amt),
                chooser,
            },
            destination,
            None,
            acc,
            ctx,
        );
    }

    let (min, max) = match eff_count {
        Some(cs) => lower_count_bounds(cs, acc, ctx)?,
        None => (EngineValueExpr::literal(1), EngineValueExpr::literal(1)),
    };

    if is_multi_player_ref(actor) {
        let group_sel = atoms::lower_player_ref(actor, acc, ctx)?;
        let loop_player = PlayerSelector::Bound(BindingId::LOOP_PLAYER);
        let prev_loop = ctx.in_loop_player;
        ctx.in_loop_player = true;
        let chooser_res = match chosen_player {
            None | Some(v2::PlayerRef::ThatPlayer) => Ok((loop_player.clone(), true)),
            Some(cp) if is_multi_player_ref(cp) => Ok((loop_player.clone(), true)),
            Some(cp) => atoms::lower_player_ref(cp, acc, ctx).map(|c| (c, false)),
        };
        ctx.in_loop_player = prev_loop;
        let (chooser, hidden) = chooser_res?;
        let mut src = ObjectQuery::zone(QueryZone::Hand(Box::new(loop_player)), engine_filter);
        if let Some(eq) = exc_q {
            src = ObjectQuery::difference(src, eq);
        }
        let mut inner_spec = ZoneOpSpec::simple(
            src,
            ZoneSelection::Prompt {
                chooser,
                min: Box::new(min),
                max: Box::new(max),
                constraint,
                category: ChoiceCategory::Generic,
                hidden,
            },
            destination,
        );
        let moved = ctx.alloc_binding()?;
        let ok = ctx.alloc_binding()?;
        inner_spec.bind_moved = Some(moved);
        inner_spec.bind_succeeded = Some(ok);
        ctx.last_moved_binding = Some(moved);
        ctx.last_succeeded_binding = Some(ok);
        acc.push(Instruction::ForEachPlayerQuery {
            selector: Box::new(group_sel),
            bind_player: BindingId::LOOP_PLAYER,
            body: Box::new(ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::ZoneOp(Box::new(inner_spec))],
            )),
        });
        return Ok(());
    }

    let actor_sel = atoms::lower_player_ref(actor, acc, ctx)?;
    let chooser = resolve_choice_player(actor, chosen_player, &actor_sel, acc, ctx)?;
    let hidden = chooser == actor_sel;
    let mut src = ObjectQuery::zone(QueryZone::Hand(Box::new(actor_sel)), engine_filter);
    if let Some(eq) = exc_q {
        src = ObjectQuery::difference(src, eq);
    }
    let selection = ZoneSelection::Prompt {
        chooser,
        min: Box::new(min),
        max: Box::new(max),
        constraint,
        category: ChoiceCategory::Generic,
        hidden,
    };
    emit_zone_op(src, selection, destination, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_draw(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:draw-with-modifiers")?;
    let from_ok = from.iter().all(|z| z.zone == v2::ZoneKind::Library);
    let to_ok = to.is_none_or(|dest| {
        dest.zone == v2::ZoneKind::Hand
            && dest.library_position.is_none()
            && dest.under_control.is_none()
    });
    if !from_ok || !to_ok || except.is_some() {
        return Err(LoweringError::Unsupported("v2:draw-with-zone-clauses"));
    }
    let cs = extract_generic_library_count(actor, subject, count, "v2:draw-unsupported-subject")?;
    let (amount, is_up_to_one) = if cs.any_number {
        return Err(LoweringError::Unsupported("v2:draw-any-number"));
    } else if cs.up_to {
        if value::lower_value_const(&cs.count) == Some(1) {
            (EngineValueExpr::literal(1), true)
        } else {
            return Err(LoweringError::Unsupported("v2:draw-up-to-multi"));
        }
    } else {
        effect::register_value_expr_player_targets(&cs.count, acc, ctx)?;
        (value::lower_value_expr(&cs.count, ctx)?, false)
    };

    let players = atoms::lower_player_ref(actor, acc, ctx)?;
    let drawn_bind = ctx.alloc_binding()?;
    let draw_ins = Instruction::DrawCards(Box::new(DrawCardsSpec {
        players,
        amount,
        bind_drawn: Some(drawn_bind),
    }));
    ctx.last_moved_binding = Some(drawn_bind);

    if is_up_to_one {
        let chooser = atoms::lower_resolving_player(actor, acc, ctx)?;
        acc.push(Instruction::OptionalSequence {
            chooser,
            instructions: 1,
        });
    }
    acc.push(draw_ins);
    Ok(())
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_mill(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:mill-with-modifiers")?;
    if !from.is_empty() && from.iter().any(|z| z.zone != v2::ZoneKind::Library) {
        return Err(LoweringError::Unsupported("v2:mill-non-library-zone"));
    }
    if let Some(dest) = to
        && dest.zone != v2::ZoneKind::Graveyard
    {
        return Err(LoweringError::Unsupported("v2:mill-non-graveyard-dest"));
    }
    if except.is_some() {
        return Err(LoweringError::Unsupported("v2:mill-with-except"));
    }

    let cs = extract_generic_library_count(actor, subject, count, "v2:mill-unsupported-subject")?;
    let amount = if cs.up_to || cs.any_number {
        return Err(LoweringError::Unsupported("v2:mill-variable-bounds"));
    } else {
        effect::register_value_expr_player_targets(&cs.count, acc, ctx)?;
        value::lower_value_expr(&cs.count, ctx)?
    };

    let players = atoms::lower_player_ref(actor, acc, ctx)?;
    let source = ObjectQuery::ZoneScan {
        zone: QueryZone::Library(Box::new(players)),
        filter: Box::new(EngineObjectFilter::any()),
        slice: QuerySlice::Top(Box::new(amount)),
    };
    let destination = EngineZoneDest::Graveyard {
        semantics: GraveyardSemantics::Mill,
    };
    emit_zone_op(source, ZoneSelection::All, destination, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_search_library(
    actor: v2::PlayerRef,
    reveal: bool,
    shuffle: bool,
    exact: bool,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if !shuffle {
        return Err(LoweringError::Unsupported("v2:search-library-no-shuffle"));
    }
    if except.is_some() {
        return Err(LoweringError::Unsupported("v2:search-library-with-except"));
    }

    let source = lower_search_source(from, acc)?;
    let (matcher, subj_count) = lower_search_matcher(subject, acc, ctx)?;
    let eff_count = count.or(subj_count);
    let search_count = lower_search_count(eff_count, exact, acc, ctx)?;
    let destination = lower_search_destination(to, modifiers)?;

    if is_multi_player_ref(actor) {
        let group_sel = atoms::lower_player_ref(actor, acc, ctx)?;
        let search = LibrarySearch {
            searcher: ResolvingPlayer::LoopPlayer,
            source,
            matcher,
            mana_value: None,
            count: search_count,
            pair_constraint: None,
            destination,
            reveal,
        };
        acc.push(Instruction::ForEachPlayerQuery {
            selector: Box::new(group_sel),
            bind_player: BindingId::LOOP_PLAYER,
            body: Box::new(ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SearchLibrary {
                    filter: Box::new(search),
                }],
            )),
        });
        return Ok(());
    }

    let searcher = atoms::lower_resolving_player(actor, acc, ctx)?;
    acc.push(Instruction::SearchLibrary {
        filter: Box::new(LibrarySearch {
            searcher,
            source,
            matcher,
            mana_value: None,
            count: search_count,
            pair_constraint: None,
            destination,
            reveal,
        }),
    });
    Ok(())
}

/// Lower a fused split-destination library search (e.g. Kodama's Reach / Cultivate /
/// Flare of Cultivation) using an explicit engine [`SearchDestination`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_split_search_library(
    actor: v2::PlayerRef,
    reveal: bool,
    exact: bool,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    destination: SearchDestination,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    if actor == v2::PlayerRef::AnyOpponent {
        effect::emit_choose_any_opponent(acc, ctx)?;
        return lower_split_search_library(
            v2::PlayerRef::ChosenPlayer,
            reveal,
            exact,
            subject,
            from,
            except,
            count,
            destination,
            acc,
            ctx,
        );
    }
    if except.is_some() {
        return Err(LoweringError::Unsupported("v2:search-library-with-except"));
    }
    let source = lower_search_source(from, acc)?;
    let (matcher, subj_count) = lower_search_matcher(subject, acc, ctx)?;
    let eff_count = count.or(subj_count);
    let search_count = lower_search_count(eff_count, exact, acc, ctx)?;
    if is_multi_player_ref(actor) {
        let group_sel = atoms::lower_player_ref(actor, acc, ctx)?;
        let search = LibrarySearch {
            searcher: ResolvingPlayer::LoopPlayer,
            source,
            matcher,
            mana_value: None,
            count: search_count,
            pair_constraint: None,
            destination,
            reveal,
        };
        acc.push(Instruction::ForEachPlayerQuery {
            selector: Box::new(group_sel),
            bind_player: BindingId::LOOP_PLAYER,
            body: Box::new(ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SearchLibrary {
                    filter: Box::new(search),
                }],
            )),
        });
        return Ok(());
    }
    let searcher = atoms::lower_resolving_player(actor, acc, ctx)?;
    acc.push(Instruction::SearchLibrary {
        filter: Box::new(LibrarySearch {
            searcher,
            source,
            matcher,
            mana_value: None,
            count: search_count,
            pair_constraint: None,
            destination,
            reveal,
        }),
    });
    Ok(())
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_shuffle(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:shuffle-with-modifiers")?;
    if let Some(dest) = to
        && dest.zone != v2::ZoneKind::Library
    {
        return Err(LoweringError::Unsupported("v2:shuffle-non-library-dest"));
    }

    let is_pure_library_shuffle = from.is_empty()
        && except.is_none()
        && count.is_none()
        && matches!(
            subject,
            v2::SubjectRef::Player(_) | v2::SubjectRef::TargetPlayer { .. }
        );
    if is_pure_library_shuffle {
        let players = match subject {
            v2::SubjectRef::Player(p) => atoms::lower_player_ref(*p, acc, ctx)?,
            v2::SubjectRef::TargetPlayer {
                opponent_only,
                count: tc,
            } => {
                acc.register_target(
                    BodyTarget::Player {
                        opponents_only: *opponent_only,
                    },
                    *tc,
                )?;
                PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer)
            }
            _ => atoms::lower_player_ref(actor, acc, ctx)?,
        };
        acc.push(Instruction::ZoneOp(Box::new(ZoneOpSpec::shuffle(players))));
        return Ok(());
    }

    let built = build_source_and_selection(actor, subject, from, except, count, false, acc, ctx)?;
    let destination = EngineZoneDest::Library {
        position: LibraryOrderPos::ShuffleInto,
    };
    emit_zone_op_with_prior(built, destination, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_look_top(
    actor: v2::PlayerRef,
    reveal: bool,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:look-top-with-modifiers")?;
    if to.is_some() || except.is_some() {
        return Err(LoweringError::Unsupported(
            "v2:look-top-with-dest-or-except",
        ));
    }

    let cs =
        extract_generic_library_count(actor, subject, count, "v2:look-top-unsupported-subject")?;
    let amount = if cs.up_to || cs.any_number {
        return Err(LoweringError::Unsupported("v2:look-top-variable-bounds"));
    } else {
        effect::register_value_expr_player_targets(&cs.count, acc, ctx)?;
        value::lower_value_expr(&cs.count, ctx)?
    };

    let viewer = atoms::lower_player_ref(actor, acc, ctx)?;
    let lib_zone = if let Some(zone_ref) = from.first() {
        if from.len() > 1 || zone_ref.zone != v2::ZoneKind::Library {
            return Err(LoweringError::Unsupported("v2:look-top-non-library-zone"));
        }
        atoms::lower_zone_ref(*zone_ref, acc, ctx)?
    } else {
        QueryZone::Library(Box::new(viewer.clone()))
    };

    let source = ObjectQuery::ZoneScan {
        zone: lib_zone,
        filter: Box::new(EngineObjectFilter::any()),
        slice: QuerySlice::Top(Box::new(amount)),
    };
    let destination = if reveal {
        EngineZoneDest::RevealOnly
    } else {
        EngineZoneDest::LookOnly { viewer }
    };
    emit_zone_op(source, ZoneSelection::All, destination, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_look_at(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:look-at-with-modifiers")?;
    if to.is_some() {
        return Err(LoweringError::Unsupported("v2:look-at-with-dest"));
    }
    let viewer = atoms::lower_player_ref(actor, acc, ctx)?;
    if matches!(
        subject,
        v2::SubjectRef::Player(_) | v2::SubjectRef::TargetPlayer { .. }
    ) {
        return lower_player_zone_inspect(
            actor,
            subject,
            from,
            except,
            count,
            EngineZoneDest::LookOnly { viewer },
            acc,
            ctx,
        );
    }
    let built = build_source_and_selection(actor, subject, from, except, count, false, acc, ctx)?;
    emit_zone_op_with_prior(built, EngineZoneDest::LookOnly { viewer }, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_reveal(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:reveal-with-modifiers")?;
    if to.is_some() {
        return Err(LoweringError::Unsupported("v2:reveal-with-dest"));
    }
    if matches!(
        subject,
        v2::SubjectRef::Player(_) | v2::SubjectRef::TargetPlayer { .. }
    ) {
        return lower_player_zone_inspect(
            actor,
            subject,
            from,
            except,
            count,
            EngineZoneDest::RevealOnly,
            acc,
            ctx,
        );
    }
    let hidden_prompt = from.iter().any(|z| z.zone == v2::ZoneKind::Hand)
        || matches!(
            subject,
            v2::SubjectRef::Counted { filter, .. }
                | v2::SubjectRef::Group(filter)
                | v2::SubjectRef::Each(filter)
                | v2::SubjectRef::All(filter)
                if filter.zones.iter().any(|z| z.zone == v2::ZoneKind::Hand)
        );
    let built =
        build_source_and_selection(actor, subject, from, except, count, hidden_prompt, acc, ctx)?;
    emit_zone_op_with_prior(built, EngineZoneDest::RevealOnly, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_player_zone_inspect(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    destination: EngineZoneDest,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let player_sel = match subject {
        v2::SubjectRef::Player(p) => atoms::lower_player_ref(*p, acc, ctx)?,
        v2::SubjectRef::TargetPlayer {
            opponent_only,
            count: tc,
        } => {
            acc.register_target(
                BodyTarget::Player {
                    opponents_only: *opponent_only,
                },
                *tc,
            )?;
            ctx.last_player_target = Some(ResolvingPlayer::TargetPlayer);
            PlayerSelector::Resolving(ResolvingPlayer::TargetPlayer)
        }
        _ => return Err(LoweringError::Unsupported("v2:subject:player-as-object")),
    };

    let mut source = if !from.is_empty() {
        scan_zones_with_filter(from, EngineObjectFilter::any(), acc, ctx)?
    } else {
        ObjectQuery::zone(
            QueryZone::Hand(Box::new(player_sel)),
            EngineObjectFilter::any(),
        )
    };
    if let Some(exc) = except {
        let exc_q = atoms::lower_subject_to_query(exc, acc, ctx)?;
        source = ObjectQuery::difference(source, exc_q);
    }

    let selection = match count {
        Some(cs) => {
            let (min, max) = lower_count_bounds(cs, acc, ctx)?;
            let chooser = atoms::lower_player_ref(actor, acc, ctx)?;
            let hidden = from.is_empty() || from.iter().all(|z| z.zone == v2::ZoneKind::Hand);
            ZoneSelection::Prompt {
                chooser,
                min: Box::new(min),
                max: Box::new(max),
                constraint: None,
                category: ChoiceCategory::Generic,
                hidden,
            }
        }
        None => ZoneSelection::All,
    };
    emit_zone_op(source, selection, destination, None, acc, ctx)
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_counter_spell(
    actor: v2::PlayerRef,
    unless_pay: Option<&v2::Cost>,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:counter-spell-with-modifiers")?;
    if actor != v2::PlayerRef::You {
        return Err(LoweringError::Unsupported(
            "v2:counter-spell-non-controller",
        ));
    }
    if !from.is_empty() && from.iter().any(|z| z.zone != v2::ZoneKind::Stack) {
        return Err(LoweringError::Unsupported(
            "v2:counter-spell-non-stack-zone",
        ));
    }
    if except.is_some() || count.is_some() {
        return Err(LoweringError::Unsupported(
            "v2:counter-spell-except-or-count",
        ));
    }

    let destination = match to {
        None => CounterDestination::Graveyard,
        Some(dest) => match (dest.zone, dest.library_position) {
            (v2::ZoneKind::Graveyard, None) => CounterDestination::Graveyard,
            (v2::ZoneKind::Exile, None) => CounterDestination::Exile,
            (v2::ZoneKind::Library, Some(v2::LibraryPosition::Top) | None) => {
                CounterDestination::Library { top: true }
            }
            (v2::ZoneKind::Library, Some(v2::LibraryPosition::Bottom)) => {
                CounterDestination::Library { top: false }
            }
            _ => return Err(LoweringError::Unsupported("v2:counter-spell-dest")),
        },
    };

    let (selector, default_payer) = match subject {
        v2::SubjectRef::TargetObject {
            count: v2::TargetCount::Exact(1) | v2::TargetCount::UpTo(1),
            another: false,
            ..
        } => {
            let (target, t_count) = atoms::lower_subject_to_target(subject, ctx)?;
            acc.register_target(target, t_count)?;
            (
                StackTargetSelector::Targeted,
                Some(ResolvingPlayer::TargetSpellController),
            )
        }
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::ThatSpell | v2::AnaphoraRef::It)
            if ctx.in_trigger =>
        {
            (
                StackTargetSelector::Triggering,
                Some(ResolvingPlayer::UniqueRelatedPlayer),
            )
        }
        v2::SubjectRef::All(f) | v2::SubjectRef::Each(f) | v2::SubjectRef::Group(f) => {
            let sf = filter::lower_spell_filter(f, ctx)?;
            let opponents_only = matches!(
                f.controller,
                Some(ast::ControllerRelation::Opponent | ast::ControllerRelation::Opponents)
            );
            let include_abilities = f.is_ability;
            let include_spells = !f.is_ability || f.any_spell;
            (
                StackTargetSelector::Matching {
                    filter: Some(Box::new(sf)),
                    include_spells,
                    include_abilities,
                    opponents_only,
                },
                None,
            )
        }
        _ => return Err(LoweringError::Unsupported("v2:counter-spell-subject")),
    };

    if let Some(cost) = unless_pay {
        let Some(payer) = default_payer else {
            return Err(LoweringError::Unsupported(
                "v2:counter-unless-pay-non-single",
            ));
        };
        let (fixed, extra_generic) = lower_unless_mana_cost(cost, ctx)?;
        let counter_idx = acc.instructions.len() + 1;
        let skip_idx = acc.instructions.len() + 2;
        acc.push(Instruction::PromptChoice(Box::new(
            PromptChoiceSpec::offer_dynamic_mana_payment(
                payer,
                fixed,
                extra_generic,
                skip_idx,
                counter_idx,
            ),
        )));
    }

    acc.push(Instruction::StackOp(Box::new(StackOpSpec {
        target: selector,
        op: StackOpKind::Counter {
            destination,
            ward_source: false,
        },
    })));
    Ok(())
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_separate_into_piles(
    actor: v2::PlayerRef,
    pile_count: u8,
    face_down_piles: u8,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:separate-piles-with-modifiers")?;
    if face_down_piles > 0 {
        return Err(LoweringError::Unsupported("v2:separate-piles-face-down"));
    }
    if pile_count < 2 {
        return Err(LoweringError::Unsupported("v2:separate-piles-count"));
    }
    if to.is_some() || count.is_some() {
        return Err(LoweringError::Unsupported(
            "v2:separate-piles-dest-or-count",
        ));
    }
    let chooser = atoms::lower_player_ref(actor, acc, ctx)?;
    let query = build_source_query(subject, from, except, acc, ctx)?;
    let bind_to = ctx.alloc_binding()?;
    acc.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
        chooser,
        domain: PromptDomain::Partition {
            query,
            pile_count,
            min_per_pile: 0,
        },
        category: ChoiceCategory::PileSplit,
        bind_to,
        bind_unchosen: None,
        tag_partition: Some(TagKey::Partition(bind_to.0)),
        auto_resolve_trivial: false,
        option_branches: Vec::new(),
    })));
    ctx.last_moved_binding = Some(bind_to);
    Ok(())
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn lower_exile_until_reveal(
    actor: v2::PlayerRef,
    stop_filter: &v2::ObjectFilter,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    reject_modifiers(modifiers, "v2:exile-until-reveal-with-modifiers")?;
    if except.is_some() {
        return Err(LoweringError::Unsupported(
            "v2:exile-until-reveal-with-except",
        ));
    }
    if let Some(dest) = to
        && dest.zone != v2::ZoneKind::Exile
    {
        return Err(LoweringError::Unsupported(
            "v2:exile-until-reveal-non-exile-dest",
        ));
    }

    effect::register_filter_player_targets(stop_filter, acc, ctx)?;
    let ef = filter::lower_object_filter(stop_filter, ctx)?;
    let eff_count = match (count, subject) {
        (Some(c), v2::SubjectRef::Player(p)) if *p == actor => Some(c),
        (None, v2::SubjectRef::Player(p)) if *p == actor => None,
        (Some(c), v2::SubjectRef::Counted { filter, .. })
            if is_plain_generic_card_filter(filter) =>
        {
            Some(c)
        }
        (None, v2::SubjectRef::Counted { filter, count: c })
            if is_plain_generic_card_filter(filter) =>
        {
            Some(c)
        }
        _ => {
            return Err(LoweringError::Unsupported(
                "v2:exile-until-reveal-unsupported-subject",
            ));
        }
    };
    let match_count = match eff_count {
        None => EngineValueExpr::literal(1),
        Some(cs) if cs.up_to || cs.any_number => {
            return Err(LoweringError::Unsupported(
                "v2:exile-until-reveal-variable-bounds",
            ));
        }
        Some(cs) => {
            effect::register_value_expr_player_targets(&cs.count, acc, ctx)?;
            value::lower_value_expr(&cs.count, ctx)?
        }
    };

    let lib_zone = if let Some(zone_ref) = from.first() {
        if from.len() > 1 || zone_ref.zone != v2::ZoneKind::Library {
            return Err(LoweringError::Unsupported(
                "v2:exile-until-reveal-non-library-zone",
            ));
        }
        atoms::lower_zone_ref(*zone_ref, acc, ctx)?
    } else {
        let players = atoms::lower_player_ref(actor, acc, ctx)?;
        QueryZone::Library(Box::new(players))
    };

    let source = ObjectQuery::ZoneScan {
        zone: lib_zone,
        filter: Box::new(EngineObjectFilter::any()),
        slice: QuerySlice::UntilMatch {
            filter: Box::new(ef),
            count: Box::new(match_count),
            include_non_matches: true,
        },
    };
    let scanned_slot = ctx.alloc_binding()?;
    let mut exile_spec = ZoneOpSpec::move_all(source, EngineZoneDest::exile());
    exile_spec.bind_moved = Some(scanned_slot);
    acc.push(Instruction::ZoneOp(Box::new(exile_spec)));

    let matched_slot = ctx.alloc_binding()?;
    let matched_ok = ctx.alloc_binding()?;
    let mut matched_spec = ZoneOpSpec::move_all(
        ObjectQuery::BoundFiltered {
            binding: scanned_slot,
            filter: Box::new(ef),
        },
        EngineZoneDest::None,
    );
    matched_spec.bind_moved = Some(matched_slot);
    matched_spec.bind_succeeded = Some(matched_ok);
    acc.push(Instruction::ZoneOp(Box::new(matched_spec)));

    let rest_slot = ctx.alloc_binding()?;
    let mut rest_spec = ZoneOpSpec::move_all(
        ObjectQuery::difference(
            ObjectQuery::Bound(scanned_slot),
            ObjectQuery::Bound(matched_slot),
        ),
        EngineZoneDest::None,
    );
    rest_spec.bind_moved = Some(rest_slot);
    acc.push(Instruction::ZoneOp(Box::new(rest_spec)));

    ctx.last_moved_binding = Some(matched_slot);
    ctx.last_unchosen_binding = Some(rest_slot);
    ctx.last_succeeded_binding = Some(matched_ok);
    Ok(())
}

struct BuiltZoneSource {
    source: ObjectQuery,
    selection: ZoneSelection,
    prior_source_binding: Option<BindingId>,
}

// Parameters mirror the orthogonal fields of `v2::Effect::ZoneOp` and lowering accumulators.
#[allow(clippy::too_many_arguments)]
fn build_source_and_selection(
    actor: v2::PlayerRef,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    except: Option<&v2::SubjectRef>,
    count: Option<&v2::CountSpec>,
    hidden_prompt: bool,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<BuiltZoneSource> {
    if let v2::SubjectRef::OneOfThem(val_expr) = subject {
        let mut source = if let Some(src_slot) = ctx.last_moved_binding {
            ObjectQuery::Bound(src_slot)
        } else if acc.target_set && atoms::target_is_object_like(acc.target) {
            ObjectQuery::Targets
        } else {
            return Err(LoweringError::Unsupported("v2:subject:one-of-them"));
        };
        if let Some(exc) = except {
            let exc_q = atoms::lower_subject_to_query(exc, acc, ctx)?;
            source = ObjectQuery::difference(source, exc_q);
        }
        let (min, max) = match count {
            Some(cs) => lower_count_bounds(cs, acc, ctx)?,
            None => {
                effect::register_value_expr_player_targets(val_expr, acc, ctx)?;
                let amt = value::lower_value_expr(val_expr, ctx)?;
                (amt.clone(), amt)
            }
        };
        let chooser = atoms::lower_player_ref(actor, acc, ctx)?;
        return Ok(BuiltZoneSource {
            source,
            selection: ZoneSelection::Prompt {
                chooser,
                min: Box::new(min),
                max: Box::new(max),
                constraint: None,
                category: ChoiceCategory::Generic,
                hidden: hidden_prompt,
            },
            prior_source_binding: None,
        });
    }

    let eff_count = match (count, subject) {
        (Some(c), _) => Some(c),
        (None, v2::SubjectRef::Counted { count: c, .. }) => Some(c),
        _ => None,
    };

    if let v2::SubjectRef::Counted { filter: f, .. }
    | v2::SubjectRef::Each(f)
    | v2::SubjectRef::All(f)
    | v2::SubjectRef::Group(f) = subject
    {
        let (f1, chosen_player) = strip_of_chosen_player(f);
        let (f2, among_prior) = strip_among_prior_cards(&f1);
        let (f3, prior_action_bound) = strip_prior_action_for_binding(
            &f2,
            ctx.last_moved_binding.is_some() && from.is_empty(),
        );
        let (f_clean, is_random) = strip_at_random(&f3);
        let from_prior = among_prior || prior_action_bound;

        if chosen_player.is_some()
            || from_prior
            || is_random
            || matches!(subject, v2::SubjectRef::Counted { .. })
        {
            effect::register_filter_player_targets(&f_clean, acc, ctx)?;
            let ef = filter::lower_object_filter(&f_clean, ctx)?;
            let prior_source_binding = if from_prior {
                Some(ctx.last_moved_binding.ok_or(LoweringError::Unsupported(
                    "v2:filter:pred:among-prior-cards",
                ))?)
            } else {
                None
            };
            let mut source = if let Some(src_slot) = prior_source_binding {
                ObjectQuery::BoundFiltered {
                    binding: src_slot,
                    filter: Box::new(ef),
                }
            } else if !from.is_empty() {
                scan_zones_with_filter(from, ef, acc, ctx)?
            } else {
                lower_group_with_clean_filter(&f_clean, ef, acc, ctx)?
            };
            if let Some(exc) = except {
                let exc_q = atoms::lower_subject_to_query(exc, acc, ctx)?;
                source = ObjectQuery::difference(source, exc_q);
            }

            let constraint = selection_constraint_from_filter(&f_clean);
            let actor_sel = atoms::lower_player_ref(actor, acc, ctx)?;
            let chooser = resolve_choice_player(actor, chosen_player, &actor_sel, acc, ctx)?;

            let selection = if let Some(cs) = eff_count {
                if is_random {
                    if cs.up_to || cs.any_number {
                        return Err(LoweringError::Unsupported("v2:zone-op-random-up-to"));
                    }
                    effect::register_value_expr_player_targets(&cs.count, acc, ctx)?;
                    let amt = value::lower_value_expr(&cs.count, ctx)?;
                    ZoneSelection::Random {
                        count: Box::new(amt),
                        chooser,
                    }
                } else {
                    let (min, max) = lower_count_bounds(cs, acc, ctx)?;
                    ZoneSelection::Prompt {
                        chooser,
                        min: Box::new(min),
                        max: Box::new(max),
                        constraint,
                        category: ChoiceCategory::Generic,
                        hidden: hidden_prompt,
                    }
                }
            } else if is_random {
                ZoneSelection::Random {
                    count: Box::new(EngineValueExpr::literal(1)),
                    chooser,
                }
            } else if chosen_player.is_some()
                || (from_prior && matches!(subject, v2::SubjectRef::Group(_)))
            {
                ZoneSelection::Prompt {
                    chooser,
                    min: Box::new(EngineValueExpr::literal(1)),
                    max: Box::new(EngineValueExpr::literal(1)),
                    constraint,
                    category: ChoiceCategory::Generic,
                    hidden: hidden_prompt,
                }
            } else {
                ZoneSelection::All
            };

            return Ok(BuiltZoneSource {
                source,
                selection,
                prior_source_binding,
            });
        }
    }

    let source = build_source_query(subject, from, except, acc, ctx)?;

    let Some(cs) = eff_count else {
        return Ok(BuiltZoneSource {
            source,
            selection: ZoneSelection::All,
            prior_source_binding: None,
        });
    };

    let constraint = match subject {
        v2::SubjectRef::Counted { filter: f, .. }
        | v2::SubjectRef::Each(f)
        | v2::SubjectRef::All(f)
        | v2::SubjectRef::Group(f) => selection_constraint_from_filter(f),
        _ => None,
    };
    let chooser = atoms::lower_player_ref(actor, acc, ctx)?;
    let (min, max) = lower_count_bounds(cs, acc, ctx)?;
    Ok(BuiltZoneSource {
        source,
        selection: ZoneSelection::Prompt {
            chooser,
            min: Box::new(min),
            max: Box::new(max),
            constraint,
            category: ChoiceCategory::Generic,
            hidden: hidden_prompt,
        },
        prior_source_binding: None,
    })
}

fn emit_zone_op_with_prior(
    built: BuiltZoneSource,
    destination: EngineZoneDest,
    shuffle_library: Option<PlayerSelector>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let Some(src_slot) = built.prior_source_binding else {
        return emit_zone_op(
            built.source,
            built.selection,
            destination,
            shuffle_library,
            acc,
            ctx,
        );
    };

    match built.selection {
        ZoneSelection::Prompt {
            chooser,
            min,
            max,
            constraint,
            category,
            hidden,
        } => {
            let chosen_slot = ctx.alloc_binding()?;
            acc.push(Instruction::PromptChoice(Box::new(PromptChoiceSpec {
                chooser,
                domain: PromptDomain::Objects {
                    query: built.source,
                    min,
                    max,
                    constraint,
                    max_total_power: None,
                    hidden,
                },
                category,
                bind_to: chosen_slot,
                bind_unchosen: None,
                tag_partition: None,
                auto_resolve_trivial: true,
                option_branches: Vec::new(),
            })));

            let moved_id = ctx.alloc_binding()?;
            let ok_id = ctx.alloc_binding()?;
            let mut move_spec = ZoneOpSpec::move_all(ObjectQuery::Bound(chosen_slot), destination);
            move_spec.bind_moved = Some(moved_id);
            move_spec.bind_succeeded = Some(ok_id);
            move_spec.shuffle_library = shuffle_library;
            acc.push(Instruction::ZoneOp(Box::new(move_spec)));

            let rest_slot = ctx.alloc_binding()?;
            let mut rest_spec = ZoneOpSpec::move_all(
                ObjectQuery::difference(
                    ObjectQuery::Bound(src_slot),
                    ObjectQuery::Bound(chosen_slot),
                ),
                EngineZoneDest::None,
            );
            rest_spec.bind_moved = Some(rest_slot);
            acc.push(Instruction::ZoneOp(Box::new(rest_spec)));

            ctx.last_moved_binding = Some(moved_id);
            ctx.last_succeeded_binding = Some(ok_id);
            ctx.last_unchosen_binding = Some(rest_slot);
            Ok(())
        }
        ZoneSelection::All => {
            let matched_slot = ctx.alloc_binding()?;
            let mut snap_spec = ZoneOpSpec::move_all(built.source, EngineZoneDest::None);
            snap_spec.bind_moved = Some(matched_slot);
            acc.push(Instruction::ZoneOp(Box::new(snap_spec)));

            let moved_id = ctx.alloc_binding()?;
            let ok_id = ctx.alloc_binding()?;
            let mut move_spec = ZoneOpSpec::move_all(ObjectQuery::Bound(matched_slot), destination);
            move_spec.bind_moved = Some(moved_id);
            move_spec.bind_succeeded = Some(ok_id);
            move_spec.shuffle_library = shuffle_library;
            acc.push(Instruction::ZoneOp(Box::new(move_spec)));

            let rest_slot = ctx.alloc_binding()?;
            let mut rest_spec = ZoneOpSpec::move_all(
                ObjectQuery::difference(
                    ObjectQuery::Bound(src_slot),
                    ObjectQuery::Bound(matched_slot),
                ),
                EngineZoneDest::None,
            );
            rest_spec.bind_moved = Some(rest_slot);
            acc.push(Instruction::ZoneOp(Box::new(rest_spec)));

            ctx.last_moved_binding = Some(moved_id);
            ctx.last_succeeded_binding = Some(ok_id);
            ctx.last_unchosen_binding = Some(rest_slot);
            Ok(())
        }
        ZoneSelection::Random { count, chooser } => {
            let picked_slot = ctx.alloc_binding()?;
            let mut pick_spec = ZoneOpSpec::simple(
                built.source,
                ZoneSelection::Random { count, chooser },
                EngineZoneDest::None,
            );
            pick_spec.bind_moved = Some(picked_slot);
            acc.push(Instruction::ZoneOp(Box::new(pick_spec)));

            let moved_id = ctx.alloc_binding()?;
            let ok_id = ctx.alloc_binding()?;
            let mut move_spec = ZoneOpSpec::move_all(ObjectQuery::Bound(picked_slot), destination);
            move_spec.bind_moved = Some(moved_id);
            move_spec.bind_succeeded = Some(ok_id);
            move_spec.shuffle_library = shuffle_library;
            acc.push(Instruction::ZoneOp(Box::new(move_spec)));

            let rest_slot = ctx.alloc_binding()?;
            let mut rest_spec = ZoneOpSpec::move_all(
                ObjectQuery::difference(
                    ObjectQuery::Bound(src_slot),
                    ObjectQuery::Bound(picked_slot),
                ),
                EngineZoneDest::None,
            );
            rest_spec.bind_moved = Some(rest_slot);
            acc.push(Instruction::ZoneOp(Box::new(rest_spec)));

            ctx.last_moved_binding = Some(moved_id);
            ctx.last_succeeded_binding = Some(ok_id);
            ctx.last_unchosen_binding = Some(rest_slot);
            Ok(())
        }
    }
}

fn lower_group_with_clean_filter(
    f: &v2::ObjectFilter,
    ef: EngineObjectFilter,
    acc: &mut BodyAcc,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ObjectQuery> {
    if f.zones.is_empty() {
        if f.any_spell {
            return Ok(ObjectQuery::zone(QueryZone::Stack, ef));
        }
        if f.any_card && f.all_types.is_empty() && f.any_types.is_empty() {
            return Err(LoweringError::Unsupported("v2:subject:zoneless-card-group"));
        }
        return Ok(ObjectQuery::zone(QueryZone::Battlefield, ef));
    }
    scan_zones_with_filter(&f.zones, ef, acc, ctx)
}

fn strip_of_chosen_player(f: &v2::ObjectFilter) -> (v2::ObjectFilter, Option<v2::PlayerRef>) {
    let mut found: Option<v2::PlayerRef> = None;
    for pred in &f.predicates {
        if let v2::FilterPredicate::OfChosenPlayer(p) = pred {
            if let Some(prev) = found {
                if prev != *p {
                    return (f.clone(), None);
                }
            } else {
                found = Some(*p);
            }
        }
    }
    let Some(player) = found else {
        return (f.clone(), None);
    };
    let mut cleaned = f.clone();
    cleaned
        .predicates
        .retain(|p| !matches!(p, v2::FilterPredicate::OfChosenPlayer(_)));
    (cleaned, Some(player))
}

fn strip_among_prior_cards(f: &v2::ObjectFilter) -> (v2::ObjectFilter, bool) {
    if !f
        .predicates
        .iter()
        .any(|p| matches!(p, v2::FilterPredicate::AmongPriorCards))
    {
        return (f.clone(), false);
    }
    let mut cleaned = f.clone();
    cleaned
        .predicates
        .retain(|p| !matches!(p, v2::FilterPredicate::AmongPriorCards));
    (cleaned, true)
}

fn is_bindable_prior_action(kind: v2::PriorActionKind) -> bool {
    matches!(
        kind,
        v2::PriorActionKind::RevealedThisWay
            | v2::PriorActionKind::ExiledThisWay
            | v2::PriorActionKind::LookedAtScryingThisWay
            | v2::PriorActionKind::MilledThisWay
            | v2::PriorActionKind::DiscardedThisWay
            | v2::PriorActionKind::SacrificedThisWay
            | v2::PriorActionKind::ReturnedThisWay
            | v2::PriorActionKind::SearchedThisWay
    )
}

fn strip_prior_action_for_binding(
    f: &v2::ObjectFilter,
    can_bind_prior: bool,
) -> (v2::ObjectFilter, bool) {
    if !can_bind_prior
        || !f.zones.is_empty()
        || !f.predicates.iter().any(|p| {
            matches!(
                p,
                v2::FilterPredicate::AffectedByPriorAction(k) if is_bindable_prior_action(*k)
            )
        })
    {
        return (f.clone(), false);
    }
    let mut cleaned = f.clone();
    cleaned.predicates.retain(|p| {
        !matches!(
            p,
            v2::FilterPredicate::AffectedByPriorAction(k) if is_bindable_prior_action(*k)
        )
    });
    (cleaned, true)
}

fn strip_at_random(f: &v2::ObjectFilter) -> (v2::ObjectFilter, bool) {
    if !f.flags.contains(v2::FilterFlags::AT_RANDOM) {
        return (f.clone(), false);
    }
    let mut cleaned = f.clone();
    cleaned.flags = v2::FilterFlags(cleaned.flags.0 & !v2::FilterFlags::AT_RANDOM.0);
    (cleaned, true)
}

fn subject_has_among_prior(subject: &v2::SubjectRef) -> bool {
    match subject {
        v2::SubjectRef::Counted { filter: f, .. }
        | v2::SubjectRef::Each(f)
        | v2::SubjectRef::All(f)
        | v2::SubjectRef::Group(f) => f.predicates.iter().any(|p| {
            matches!(
                p,
                v2::FilterPredicate::AmongPriorCards
                    | v2::FilterPredicate::AffectedByPriorAction(_)
            )
        }),
        _ => false,
    }
}

fn resolve_choice_player(
    actor: v2::PlayerRef,
    chosen: Option<v2::PlayerRef>,
    actor_sel: &PlayerSelector,
    acc: &mut BodyAcc,
    ctx: &LowerCtx<'_>,
) -> LowerResult<PlayerSelector> {
    match chosen {
        None | Some(v2::PlayerRef::ThatPlayer) => Ok(actor_sel.clone()),
        Some(p) if p == actor => Ok(actor_sel.clone()),
        Some(p) => atoms::lower_player_ref(p, acc, ctx),
    }
}

fn build_source_query(
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    except: Option<&v2::SubjectRef>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<ObjectQuery> {
    effect::register_subject_filter_player_targets(subject, acc, ctx)?;
    let mut query = if from.is_empty() {
        atoms::lower_subject_to_query(subject, acc, ctx)?
    } else {
        match subject {
            v2::SubjectRef::Each(f)
            | v2::SubjectRef::All(f)
            | v2::SubjectRef::Group(f)
            | v2::SubjectRef::Counted { filter: f, .. } => {
                let ef = filter::lower_object_filter(f, ctx)?;
                scan_zones_with_filter(from, ef, acc, ctx)?
            }
            v2::SubjectRef::HandOf(p) => {
                let ps = atoms::lower_player_ref(*p, acc, ctx)?;
                ObjectQuery::zone(QueryZone::Hand(Box::new(ps)), EngineObjectFilter::any())
            }
            other => atoms::lower_subject_to_query(other, acc, ctx)?,
        }
    };

    if let Some(exc) = except {
        let exc_q = atoms::lower_subject_to_query(exc, acc, ctx)?;
        query = ObjectQuery::difference(query, exc_q);
    }
    Ok(query)
}

fn scan_zones_with_filter(
    zones: &[v2::ZoneRef],
    ef: EngineObjectFilter,
    acc: &mut BodyAcc,
    ctx: &LowerCtx<'_>,
) -> LowerResult<ObjectQuery> {
    if zones.len() == 1 {
        let qz = atoms::lower_zone_ref(zones[0], acc, ctx)?;
        return Ok(ObjectQuery::zone(qz, ef));
    }
    let mut parts = Vec::with_capacity(zones.len());
    for z in zones {
        let qz = atoms::lower_zone_ref(*z, acc, ctx)?;
        parts.push(ObjectQuery::zone(qz, ef));
    }
    Ok(ObjectQuery::Union(parts))
}

fn emit_zone_op(
    source: ObjectQuery,
    selection: ZoneSelection,
    destination: EngineZoneDest,
    shuffle_library: Option<PlayerSelector>,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<()> {
    let moved_id = ctx.alloc_binding()?;
    let ok_id = ctx.alloc_binding()?;
    let unchosen_id = if matches!(
        selection,
        ZoneSelection::Prompt { .. } | ZoneSelection::Random { .. }
    ) {
        Some(ctx.alloc_binding()?)
    } else {
        None
    };

    let mut spec = ZoneOpSpec::simple(source, selection, destination);
    spec.bind_moved = Some(moved_id);
    spec.bind_succeeded = Some(ok_id);
    spec.bind_unchosen = unchosen_id;
    spec.shuffle_library = shuffle_library;

    ctx.last_moved_binding = Some(moved_id);
    ctx.last_succeeded_binding = Some(ok_id);
    if let Some(uid) = unchosen_id {
        ctx.last_unchosen_binding = Some(uid);
    }

    acc.push(Instruction::ZoneOp(Box::new(spec)));
    Ok(())
}

type SacrificeChoiceShape<'a> = (
    Option<&'a v2::CountSpec>,
    Option<(Option<&'a v2::CountSpec>, Option<&'a v2::ObjectFilter>)>,
);

fn extract_sacrifice_choice_shape<'a>(
    subject: &'a v2::SubjectRef,
    count: Option<&'a v2::CountSpec>,
) -> SacrificeChoiceShape<'a> {
    match subject {
        v2::SubjectRef::Counted {
            filter: f,
            count: c,
        } => {
            let eff = count.unwrap_or(c);
            (Some(eff), Some((Some(eff), Some(f.as_ref()))))
        }
        v2::SubjectRef::Group(f) => {
            if let Some(c) = count {
                (Some(c), Some((Some(c), Some(f.as_ref()))))
            } else if f.flags.contains(v2::FilterFlags::AT_RANDOM)
                || f.predicates
                    .iter()
                    .any(|p| matches!(p, v2::FilterPredicate::OfChosenPlayer(_)))
            {
                (None, Some((None, Some(f.as_ref()))))
            } else {
                (None, None)
            }
        }
        v2::SubjectRef::Each(f) | v2::SubjectRef::All(f) => {
            if let Some(c) = count {
                (Some(c), Some((Some(c), Some(f.as_ref()))))
            } else {
                (None, None)
            }
        }
        _ => (count, None),
    }
}

fn lower_count_bounds(
    cs: &v2::CountSpec,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(EngineValueExpr, EngineValueExpr)> {
    if cs.any_number {
        return Ok((
            EngineValueExpr::literal(0),
            EngineValueExpr::literal(i64::from(u16::MAX)),
        ));
    }
    effect::register_value_expr_player_targets(&cs.count, acc, ctx)?;
    let max = value::lower_value_expr(&cs.count, ctx)?;
    let min = if cs.up_to {
        EngineValueExpr::literal(0)
    } else {
        max.clone()
    };
    Ok((min, max))
}

fn selection_constraint_from_filter(f: &v2::ObjectFilter) -> Option<SelectionConstraint> {
    if f.flags.contains(v2::FilterFlags::DIFFERENT_NAMES) {
        Some(SelectionConstraint::DistinctNames)
    } else if f.flags.contains(v2::FilterFlags::DIFFERENT_POWERS) {
        Some(SelectionConstraint::DistinctPowers)
    } else {
        None
    }
}

fn is_multi_player_ref(player: v2::PlayerRef) -> bool {
    matches!(
        player,
        v2::PlayerRef::EachPlayer
            | v2::PlayerRef::EachOpponent
            | v2::PlayerRef::EachOtherPlayer
            | v2::PlayerRef::EachOtherOpponent
    )
}

fn filter_references_that_player(f: &v2::ObjectFilter) -> bool {
    matches!(
        f.controller,
        Some(ast::ControllerRelation::ThatPlayer | ast::ControllerRelation::OwnerThatPlayer)
    ) || matches!(
        f.owner,
        Some(ast::ControllerRelation::ThatPlayer | ast::ControllerRelation::OwnerThatPlayer)
    ) || f
        .zones
        .iter()
        .any(|z| matches!(z.owner, v2::ZoneOwner::ThatPlayer))
        || f.predicates.iter().any(|p| match p {
            v2::FilterPredicate::OfChosenPlayer(v2::PlayerRef::ThatPlayer) => true,
            v2::FilterPredicate::AnyOf(subs) => subs.iter().any(filter_references_that_player),
            _ => false,
        })
}

fn subject_references_that_player(subject: &v2::SubjectRef) -> bool {
    match subject {
        v2::SubjectRef::Player(v2::PlayerRef::ThatPlayer)
        | v2::SubjectRef::HandOf(v2::PlayerRef::ThatPlayer) => true,
        v2::SubjectRef::Each(f)
        | v2::SubjectRef::All(f)
        | v2::SubjectRef::Group(f)
        | v2::SubjectRef::Counted { filter: f, .. } => filter_references_that_player(f),
        v2::SubjectRef::Compound(parts) => parts.iter().any(subject_references_that_player),
        _ => false,
    }
}

fn should_loop_zone_op_per_player(
    op: &v2::ZoneOpKind,
    subject: &v2::SubjectRef,
    from: &[v2::ZoneRef],
    to: Option<&v2::ZoneDest>,
) -> bool {
    if matches!(
        op,
        v2::ZoneOpKind::Sacrifice
            | v2::ZoneOpKind::Discard { .. }
            | v2::ZoneOpKind::SearchLibrary { .. }
            | v2::ZoneOpKind::CounterSpell { .. }
            | v2::ZoneOpKind::ChoosePile
    ) {
        return false;
    }
    let has_that_player = from
        .iter()
        .any(|z| matches!(z.owner, v2::ZoneOwner::ThatPlayer))
        || to.is_some_and(|d| {
            matches!(d.owner, v2::ZoneOwner::ThatPlayer)
                || matches!(d.under_control, Some(v2::ZoneOwner::ThatPlayer))
        })
        || subject_references_that_player(subject);
    if has_that_player {
        return true;
    }
    matches!(
        op,
        v2::ZoneOpKind::Exile { .. }
            | v2::ZoneOpKind::Move
            | v2::ZoneOpKind::LookTop { .. }
            | v2::ZoneOpKind::LookAt
            | v2::ZoneOpKind::Reveal
            | v2::ZoneOpKind::ExileUntilReveal { .. }
    ) && from.iter().any(|z| {
        matches!(
            z.zone,
            v2::ZoneKind::Library | v2::ZoneKind::Hand | v2::ZoneKind::Graveyard
        ) && matches!(
            z.owner,
            v2::ZoneOwner::EachPlayer
                | v2::ZoneOwner::Opponent
                | v2::ZoneOwner::Opponents
                | v2::ZoneOwner::ThatPlayer
        )
    })
}

fn rewrite_loop_zone_owner(mut z: v2::ZoneRef, actor: v2::PlayerRef) -> v2::ZoneRef {
    let matches_actor = matches!(
        (actor, z.owner),
        (
            v2::PlayerRef::EachPlayer | v2::PlayerRef::EachOtherPlayer,
            v2::ZoneOwner::EachPlayer
        ) | (
            v2::PlayerRef::EachOpponent | v2::PlayerRef::EachOtherOpponent,
            v2::ZoneOwner::Opponent | v2::ZoneOwner::Opponents,
        )
    );
    if matches_actor {
        z.owner = v2::ZoneOwner::ThatPlayer;
    }
    z
}

fn lower_search_source(from: &[v2::ZoneRef], acc: &mut BodyAcc) -> LowerResult<SearchSource> {
    if from.is_empty() {
        return Ok(SearchSource::OwnLibrary);
    }
    if from.len() == 1 {
        let z = from[0];
        if z.zone != v2::ZoneKind::Library {
            return Err(LoweringError::Unsupported("v2:search-non-library-zone"));
        }
        return match z.owner {
            v2::ZoneOwner::Default
            | v2::ZoneOwner::You
            | v2::ZoneOwner::ThatPlayer
            | v2::ZoneOwner::EachPlayer => Ok(SearchSource::OwnLibrary),
            v2::ZoneOwner::TargetOpponent => {
                acc.register_target(BodyTarget::SINGLE_OPPONENT, v2::TargetCount::Exact(1))?;
                Ok(SearchSource::TargetOpponentLibrary)
            }
            v2::ZoneOwner::TargetPlayer => {
                acc.register_target(BodyTarget::SINGLE_PLAYER, v2::TargetCount::Exact(1))?;
                Ok(SearchSource::TargetPlayerLibrary)
            }
            _ => Err(LoweringError::Unsupported("v2:search-library-owner")),
        };
    }
    if from.len() == 2 {
        let has_own_lib = from.iter().any(|z| {
            z.zone == v2::ZoneKind::Library
                && matches!(z.owner, v2::ZoneOwner::Default | v2::ZoneOwner::You)
        });
        let has_own_gy = from.iter().any(|z| {
            z.zone == v2::ZoneKind::Graveyard
                && matches!(z.owner, v2::ZoneOwner::Default | v2::ZoneOwner::You)
        });
        if has_own_lib && has_own_gy {
            return Ok(SearchSource::OwnLibraryOrGraveyard);
        }
    }
    Err(LoweringError::Unsupported(
        "v2:search-multi-zone-unsupported",
    ))
}

fn lower_search_matcher<'a>(
    subject: &'a v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(SearchCardMatcher, Option<&'a v2::CountSpec>)> {
    let (filter_opt, count_opt) = match subject {
        v2::SubjectRef::Counted { filter: f, count } => (Some(f.as_ref()), Some(count)),
        v2::SubjectRef::Each(f) | v2::SubjectRef::All(f) | v2::SubjectRef::Group(f) => {
            (Some(f.as_ref()), None)
        }
        _ => return Err(LoweringError::Unsupported("v2:search-matcher-subject")),
    };

    let Some(f) = filter_opt else {
        return Ok((SearchCardMatcher::Any, count_opt));
    };

    if f.is_empty() || is_any_card_only_filter(f) {
        return Ok((SearchCardMatcher::Any, count_opt));
    }

    if is_basic_land_only_filter(f) {
        return Ok((SearchCardMatcher::BasicLand, count_opt));
    }

    if let Some(land_matcher) = try_pure_land_subtypes_matcher(f) {
        return Ok((land_matcher, count_opt));
    }

    if let Some(named_matcher) = try_named_search_matcher(f, ctx)? {
        return Ok((named_matcher, count_opt));
    }

    if let Some(same_name_matcher) = try_same_name_search_matcher(f, acc, ctx)? {
        return Ok((same_name_matcher, count_opt));
    }

    let ef = filter::lower_object_filter(f, ctx)?;
    Ok((SearchCardMatcher::Filtered(Box::new(ef)), count_opt))
}

fn try_same_name_search_matcher(
    f: &v2::ObjectFilter,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<SearchCardMatcher>> {
    let mut same_name_refs: Vec<&v2::SubjectRef> = Vec::new();
    for pred in &f.predicates {
        if let v2::FilterPredicate::SharesPropertyWith {
            property: v2::SharedProperty::Name,
            reference,
            negated: false,
        } = pred
        {
            same_name_refs.push(reference.as_ref());
        }
    }
    if same_name_refs.len() != 1 {
        return Ok(None);
    }
    let Some(move_ref) = lower_same_name_reference(same_name_refs[0], acc, ctx)? else {
        return Ok(None);
    };

    let mut rest = f.clone();
    rest.predicates.retain(|p| {
        !matches!(
            p,
            v2::FilterPredicate::SharesPropertyWith {
                property: v2::SharedProperty::Name,
                negated: false,
                ..
            }
        )
    });
    let extra = if rest.is_empty() || is_any_card_only_filter(&rest) {
        None
    } else {
        effect::register_filter_player_targets(&rest, acc, ctx)?;
        Some(Box::new(filter::lower_object_filter(&rest, ctx)?))
    };

    Ok(Some(SearchCardMatcher::SameNameAs {
        reference: move_ref,
        extra,
    }))
}

fn lower_same_name_reference(
    reference: &v2::SubjectRef,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<MoveSubjectRef>> {
    match reference {
        v2::SubjectRef::SelfObject => Ok(Some(MoveSubjectRef::AbilitySource)),
        v2::SubjectRef::TargetObject { .. } => {
            let (bt, tc) = atoms::lower_subject_to_target(reference, ctx)?;
            if !matches!(
                bt,
                BodyTarget::Object {
                    zone: TargetZone::Battlefield,
                    ..
                }
            ) {
                return Ok(None);
            }
            acc.register_target(bt, tc)?;
            ctx.has_object_target = true;
            Ok(Some(MoveSubjectRef::TargetObject))
        }
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::EnchantedCreature
            | v2::AnaphoraRef::EnchantedPermanent
            | v2::AnaphoraRef::EquippedCreature,
        ) => Ok(Some(MoveSubjectRef::AttachedHost)),
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::RevealedCard) => {
            Ok(Some(MoveSubjectRef::RevealedCard))
        }
        v2::SubjectRef::Anaphora(v2::AnaphoraRef::SameTarget) => {
            if acc.target_set
                && matches!(
                    acc.target,
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        ..
                    }
                )
            {
                Ok(Some(MoveSubjectRef::TargetObject))
            } else {
                Ok(None)
            }
        }
        v2::SubjectRef::Anaphora(
            v2::AnaphoraRef::It
            | v2::AnaphoraRef::ThatCreature
            | v2::AnaphoraRef::ThatPermanent
            | v2::AnaphoraRef::ThatCard
            | v2::AnaphoraRef::ThatLand
            | v2::AnaphoraRef::ThatObject
            | v2::AnaphoraRef::ThatSpell
            | v2::AnaphoraRef::SacrificedObject
            | v2::AnaphoraRef::ExiledObject
            | v2::AnaphoraRef::DestroyedObject
            | v2::AnaphoraRef::DiscardedObject,
        ) => {
            if acc.target_set
                && matches!(
                    acc.target,
                    BodyTarget::Object {
                        zone: TargetZone::Battlefield,
                        ..
                    }
                )
            {
                Ok(Some(MoveSubjectRef::TargetObject))
            } else if ctx.in_trigger {
                Ok(Some(MoveSubjectRef::TriggeringObject))
            } else if ctx.last_moved_binding.is_some() {
                Ok(Some(MoveSubjectRef::LastMoved))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

fn try_named_search_matcher(
    f: &v2::ObjectFilter,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<Option<SearchCardMatcher>> {
    let mut named_self = false;
    let mut explicit_names: Vec<&str> = Vec::new();
    for pred in &f.predicates {
        match pred {
            v2::FilterPredicate::NamedSelf => named_self = true,
            v2::FilterPredicate::Named(name) => explicit_names.push(name.as_str()),
            _ => {}
        }
    }
    if !named_self && explicit_names.is_empty() {
        return Ok(None);
    }

    let mut rest = f.clone();
    rest.predicates.retain(|p| {
        !matches!(
            p,
            v2::FilterPredicate::NamedSelf | v2::FilterPredicate::Named(_)
        )
    });
    let rest_is_plain_card = rest.is_empty() || is_any_card_only_filter(&rest);

    if named_self && explicit_names.is_empty() {
        if rest_is_plain_card {
            let name = EnglishCardName::new(ctx.face_name.to_string())
                .map_err(|_| LoweringError::Unsupported("v2:search-invalid-self-name"))?;
            return Ok(Some(SearchCardMatcher::AnyOfNames(vec![name])));
        }
        let extra = filter::lower_object_filter(&rest, ctx)?;
        return Ok(Some(SearchCardMatcher::SameNameAs {
            reference: MoveSubjectRef::AbilitySource,
            extra: Some(Box::new(extra)),
        }));
    }

    if !named_self && rest_is_plain_card {
        let mut names = Vec::with_capacity(explicit_names.len());
        for raw in explicit_names {
            let name = EnglishCardName::new(raw.to_string())
                .map_err(|_| LoweringError::Unsupported("v2:search-invalid-card-name"))?;
            if !names.contains(&name) {
                names.push(name);
            }
        }
        if !names.is_empty() {
            return Ok(Some(SearchCardMatcher::AnyOfNames(names)));
        }
    }

    Ok(None)
}

fn is_any_card_only_filter(f: &v2::ObjectFilter) -> bool {
    f.any_card
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
        && f.combat.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
        && f.predicates.is_empty()
}

fn is_basic_land_only_filter(f: &v2::ObjectFilter) -> bool {
    let land_type = f.all_types == ast::CardTypeMask::LAND && f.any_types.is_empty()
        || f.any_types == ast::CardTypeMask::LAND && f.all_types.is_empty();
    land_type
        && f.required_supertypes == ast::SupertypeMask::BASIC
        && f.forbidden_types.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.stat_bounds.is_empty()
        && f.flags.is_empty()
        && f.predicates.is_empty()
}

fn try_pure_land_subtypes_matcher(f: &v2::ObjectFilter) -> Option<SearchCardMatcher> {
    if f.subtypes.is_empty()
        || !f.forbidden_types.is_empty()
        || !f.required_supertypes.is_empty()
        || !f.forbidden_supertypes.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || !f.stat_bounds.is_empty()
        || !f.flags.is_empty()
        || !f.predicates.is_empty()
    {
        return None;
    }
    let types = atoms::lower_card_type_set(f.all_types.union(f.any_types));
    let require_land_type = if types.is_empty() {
        false
    } else if types.len() == 1 && types.contains(&CardType::Land) {
        true
    } else {
        return None;
    };
    let mut subtypes: BTreeSet<LandType> = BTreeSet::new();
    for raw in &f.subtypes {
        subtypes.insert(LowerCtx::lookup_land_type(raw)?);
    }
    Some(SearchCardMatcher::LandSubtypes {
        subtypes,
        require_land_type,
    })
}

fn lower_search_count(
    count: Option<&v2::CountSpec>,
    exact: bool,
    acc: &mut BodyAcc,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<SearchCount> {
    let Some(cs) = count else {
        return Ok(if exact {
            SearchCount::Exact(NonZeroU8::MIN)
        } else {
            SearchCount::UpTo(1)
        });
    };
    if cs.any_number {
        return Err(LoweringError::Unsupported("v2:search-library-any-number"));
    }
    if exact && !cs.up_to {
        let Some(val) = value::lower_value_const(&cs.count) else {
            return Err(LoweringError::Unsupported(
                "v2:search-library-exact-dynamic",
            ));
        };
        let n = u8::try_from(val)
            .ok()
            .and_then(NonZeroU8::new)
            .ok_or(LoweringError::Unsupported("v2:search-library-exact-range"))?;
        return Ok(SearchCount::Exact(n));
    }
    if let Some(val) = value::lower_value_const(&cs.count)
        && let Ok(n) = u8::try_from(val)
        && n > 0
    {
        return Ok(SearchCount::UpTo(n));
    }
    effect::register_value_expr_player_targets(&cs.count, acc, ctx)?;
    let expr = value::lower_value_expr(&cs.count, ctx)?;
    Ok(SearchCount::UpToExpr(Box::new(expr)))
}

fn lower_search_destination(
    to: Option<&v2::ZoneDest>,
    modifiers: &v2::EntryModifiers,
) -> LowerResult<SearchDestination> {
    if modifiers.face_down
        || modifiers.transformed
        || modifiers.attacking
        || modifiers.with_counter.is_some()
        || modifiers.attached_to.is_some()
    {
        return Err(LoweringError::Unsupported(
            "v2:search-library-unsupported-modifiers",
        ));
    }
    let Some(dest) = to else {
        if modifiers.tapped {
            return Err(LoweringError::Unsupported("v2:search-hand-tapped"));
        }
        return Ok(SearchDestination::Hand);
    };
    if dest.under_control.is_some() {
        return Err(LoweringError::Unsupported("v2:search-custom-controller"));
    }
    match dest.zone {
        v2::ZoneKind::Hand => {
            if modifiers.tapped {
                return Err(LoweringError::Unsupported("v2:search-hand-tapped"));
            }
            Ok(SearchDestination::Hand)
        }
        v2::ZoneKind::Battlefield => Ok(SearchDestination::Battlefield {
            tapped: modifiers.tapped,
            post_entry: SearchBattlefieldPostEntry::None,
        }),
        v2::ZoneKind::Library => {
            if modifiers.tapped {
                return Err(LoweringError::Unsupported("v2:search-library-tapped"));
            }
            match dest.library_position {
                Some(v2::LibraryPosition::Top) | None => Ok(SearchDestination::LibraryTop),
                _ => Err(LoweringError::Unsupported("v2:search-library-position")),
            }
        }
        v2::ZoneKind::Exile => {
            if modifiers.tapped {
                return Err(LoweringError::Unsupported("v2:search-exile-tapped"));
            }
            Ok(SearchDestination::Exile {
                link: SearchExileLink::None,
            })
        }
        v2::ZoneKind::Graveyard => {
            if modifiers.tapped {
                return Err(LoweringError::Unsupported("v2:search-graveyard-tapped"));
            }
            Ok(SearchDestination::Graveyard)
        }
        _ => Err(LoweringError::Unsupported("v2:search-library-dest")),
    }
}

pub(crate) fn lower_unless_mana_cost(
    cost: &v2::Cost,
    ctx: &mut LowerCtx<'_>,
) -> LowerResult<(ManaCost, EngineValueExpr)> {
    match cost {
        v2::Cost::Mana(mc) => {
            let (fixed, x_count, hybrids) = atoms::lower_mana_cost(mc)?;
            if !hybrids.is_empty() {
                return Err(LoweringError::Unsupported("v2:unless-pay-hybrid-mana"));
            }
            let extra_generic = if x_count == 0 {
                EngineValueExpr::literal(0)
            } else if x_count == 1 {
                EngineValueExpr::chosen_x()
            } else {
                EngineValueExpr::Scale {
                    expr: Box::new(EngineValueExpr::chosen_x()),
                    factor: i64::from(x_count).into(),
                }
            };
            Ok((fixed, extra_generic))
        }
        v2::Cost::ScaledMana { base, scale } => {
            let (fixed, x_count, hybrids) = atoms::lower_mana_cost(base)?;
            if !hybrids.is_empty() || x_count > 0 || !fixed.required.is_empty() {
                return Err(LoweringError::Unsupported(
                    "v2:unless-pay-scaled-colored-mana",
                ));
            }
            let scale_expr = value::lower_value_expr(scale, ctx)?;
            let extra_generic = EngineValueExpr::Scale {
                expr: Box::new(scale_expr),
                factor: fixed.generic,
            };
            Ok((ManaCost::free(), extra_generic))
        }
        _ => Err(LoweringError::Unsupported("v2:unless-pay-non-mana-cost")),
    }
}

fn reject_modifiers(modifiers: &v2::EntryModifiers, code: &'static str) -> LowerResult<()> {
    if modifiers.tapped
        || modifiers.attacking
        || modifiers.face_down
        || modifiers.transformed
        || modifiers.with_counter.is_some()
        || modifiers.attached_to.is_some()
    {
        return Err(LoweringError::Unsupported(code));
    }
    Ok(())
}

fn is_plain_generic_card_filter(filter: &v2::ObjectFilter) -> bool {
    *filter
        == v2::ObjectFilter {
            any_card: filter.any_card,
            ..v2::ObjectFilter::default()
        }
}

fn extract_generic_library_count<'a>(
    actor: v2::PlayerRef,
    subject: &'a v2::SubjectRef,
    count: Option<&'a v2::CountSpec>,
    err_code: &'static str,
) -> LowerResult<&'a v2::CountSpec> {
    match (count, subject) {
        (Some(c), v2::SubjectRef::Player(p)) if *p == actor => Ok(c),
        (Some(c), v2::SubjectRef::Counted { filter, .. })
            if is_plain_generic_card_filter(filter) =>
        {
            Ok(c)
        }
        (None, v2::SubjectRef::Counted { filter, count: c })
            if is_plain_generic_card_filter(filter) =>
        {
            Ok(c)
        }
        _ => Err(LoweringError::Unsupported(err_code)),
    }
}
