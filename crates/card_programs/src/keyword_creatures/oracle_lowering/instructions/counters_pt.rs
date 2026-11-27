use super::*;

fn is_explicit_self_permanent_noun(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "this creature"
            | "this permanent"
            | "this artifact"
            | "this enchantment"
            | "this land"
            | "this planeswalker"
            | "this battle"
            | "this saga"
            | "this room"
            | "this vehicle"
            | "this equipment"
            | "this aura"
            | "this artifact creature"
            | "this enchantment creature"
    )
}

fn strip_battlefield_zone_from_subject(subject: &SubjectRefAst) -> SubjectRefAst {
    match subject {
        SubjectRefAst::InZone {
            subject: inner,
            zone: ZoneFilter::Battlefield,
        } => strip_battlefield_zone_from_subject(inner),
        SubjectRefAst::All(f) if f.zone == Some(ZoneFilter::Battlefield) => {
            let mut clean = f.as_ref().clone();
            clean.zone = None;
            SubjectRefAst::All(Box::new(clean))
        }
        SubjectRefAst::Each(f) if f.zone == Some(ZoneFilter::Battlefield) => {
            let mut clean = f.as_ref().clone();
            clean.zone = None;
            SubjectRefAst::Each(Box::new(clean))
        }
        SubjectRefAst::Group(f) if f.zone == Some(ZoneFilter::Battlefield) => {
            let mut clean = f.as_ref().clone();
            clean.zone = None;
            SubjectRefAst::Group(Box::new(clean))
        }
        SubjectRefAst::TargetObject { filter, count }
            if filter.zone == Some(ZoneFilter::Battlefield) =>
        {
            let mut clean = filter.as_ref().clone();
            clean.zone = None;
            SubjectRefAst::TargetObject {
                filter: Box::new(clean),
                count: *count,
            }
        }
        SubjectRefAst::Compound(items) => SubjectRefAst::Compound(
            items
                .iter()
                .map(strip_battlefield_zone_from_subject)
                .collect(),
        ),
        other => other.clone(),
    }
}

fn value_expr_references_ability_source(expr: &ValueExpr) -> bool {
    match expr {
        ValueExpr::SourceState(_) => true,
        ValueExpr::CounterCount {
            subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
            ..
        } => true,
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            value_expr_references_ability_source(a) || value_expr_references_ability_source(b)
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            value_expr_references_ability_source(inner)
        }
        _ => false,
    }
}

pub(super) fn prior_op_acted_on_ability_source(ops: &[Instruction]) -> bool {
    for op in ops.iter().rev() {
        match op {
            Instruction::MutateObject(spec) => {
                return matches!(&spec.source, spellwake_engine::toolbox::ObjectQuery::Source)
                    || matches!(
                        &spec.source,
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(s)
                            if matches!(s.as_ref(), EffectSubject::AbilitySource)
                    );
            }
            Instruction::ApplyContinuous(spec) => {
                if matches!(
                    &spec.selector,
                    Some(spellwake_engine::toolbox::ObjectQuery::Source)
                ) || matches!(
                    &spec.selector,
                    Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(s))
                        if matches!(s.as_ref(), EffectSubject::AbilitySource)
                ) {
                    return true;
                }
            }
            Instruction::BranchCondition { condition, .. } => {
                if let spellwake_engine::toolbox::ConditionExpr::Resolution(condition) =
                    condition.as_ref()
                    && (value_expr_references_ability_source(&condition.left)
                        || value_expr_references_ability_source(&condition.right))
                {
                    return true;
                }
            }
            Instruction::OptionalSequence { .. } | Instruction::Jump { .. } => {}
            _ => return false,
        }
    }
    false
}

fn value_expr_counter_kind(expr: &ValueExpr) -> Option<CounterKind> {
    match expr {
        ValueExpr::CounterCount { kind: Some(ck), .. } if ck.is_permanent() => Some(*ck),
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            value_expr_counter_kind(a).or_else(|| value_expr_counter_kind(b))
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            value_expr_counter_kind(inner)
        }
        _ => None,
    }
}

pub(super) fn infer_prior_counter_kind(ops: &[Instruction]) -> Option<CounterKind> {
    for op in ops.iter().rev() {
        match op {
            Instruction::MutateObject(spec) => match &spec.op {
                spellwake_engine::toolbox::ObjectMutation::AddCounters { kind, .. }
                    if kind.is_permanent() =>
                {
                    return Some(*kind);
                }
                spellwake_engine::toolbox::ObjectMutation::RemoveCounters {
                    kind: Some(ck),
                    ..
                }
                | spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind: Some(ck) }
                    if ck.is_permanent() =>
                {
                    return Some(*ck);
                }
                _ => {}
            },
            Instruction::BranchCondition { condition, .. } => {
                if let spellwake_engine::toolbox::ConditionExpr::Resolution(condition) =
                    condition.as_ref()
                    && let Some(ck) = value_expr_counter_kind(&condition.left)
                        .or_else(|| value_expr_counter_kind(&condition.right))
                {
                    return Some(ck);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_pure_remember_target_clause(op: &Instruction) -> bool {
    matches!(
        op,
        Instruction::FixedTargetClause(body)
            if !body.instructions.is_empty()
                && body.instructions.iter().all(|inner| {
                    matches!(
                        inner,
                        Instruction::MutateObject(spec)
                            if matches!(
                                spec.op,
                                spellwake_engine::toolbox::ObjectMutation::RememberPermanentTarget
                            )
                    )
                })
    )
}

fn try_append_fixed_first_target_pt_parts(
    builder: &mut BodyBuilder,
    part: ResolvedPartSpec,
    dur: ResolvedDuration,
) -> bool {
    if try_append_fixed_first_target_parts(builder, ResolvedPartsSpec(vec![part.clone()]), dur) {
        return true;
    }
    if builder.target != BodyTarget::None || builder.instructions.len() < 2 {
        return false;
    }
    let first_is_remembered_perm = matches!(
        builder.instructions.first(),
        Some(Instruction::FixedTargetClause(first_body))
            if is_permanent_body_target(first_body.target)
                && first_body.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::MutateObject(spec)
                            if matches!(
                                spec.op,
                                spellwake_engine::toolbox::ObjectMutation::RememberPermanentTarget
                            )
                    )
                })
    );
    if !first_is_remembered_perm {
        return false;
    }
    let rest = &builder.instructions[1..];
    if rest[..rest.len() - 1]
        .iter()
        .all(is_pure_remember_target_clause)
        && let Some(Instruction::BranchCondition {
            condition,
            destination: 0,
        }) = rest.last()
    {
        let cond_clone = condition.clone();
        if let Some(Instruction::FixedTargetClause(first_body)) = builder.instructions.first_mut() {
            let next_dest = first_body.instructions.len() + 2;
            first_body.instructions.push(Instruction::BranchCondition {
                condition: cond_clone,
                destination: next_dest,
            });
            first_body.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    ResolvedPartsSpec(vec![part]),
                    dur,
                ),
            );
            return true;
        }
    }
    false
}

fn try_append_fixed_first_target_counters(
    builder: &mut BodyBuilder,
    kind: CounterKind,
    amount: ValueExpr,
) -> bool {
    if builder.target != BodyTarget::None || builder.instructions.is_empty() {
        return false;
    }
    let first_is_remembered_perm = matches!(
        builder.instructions.first(),
        Some(Instruction::FixedTargetClause(first_body))
            if is_permanent_body_target(first_body.target)
                && first_body
                    .instructions
                    .iter()
                    .any(|op| {
                        matches!(
                            op,
                            Instruction::MutateObject(spec)
                                if matches!(
                                    spec.op,
                                    spellwake_engine::toolbox::ObjectMutation::RememberPermanentTarget
                                )
                        )
                    })
    );
    if !first_is_remembered_perm {
        return false;
    }
    let is_control_flow = |op: &Instruction| {
        matches!(
            op,
            Instruction::BranchCondition { .. }
                | Instruction::Jump { .. }
                | Instruction::OptionalSequence { .. }
                | Instruction::OfferDynamicManaPayment { .. }
        )
    };
    let rest = &builder.instructions[1..];
    if !rest.iter().any(is_control_flow) {
        if let Some(Instruction::FixedTargetClause(first_body)) = builder.instructions.first_mut() {
            first_body.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                    kind, amount,
                ),
            );
            return true;
        }
        return false;
    }
    if !rest.is_empty()
        && !rest[..rest.len() - 1].iter().any(is_control_flow)
        && let Some(Instruction::BranchCondition {
            condition,
            destination: 0,
        }) = rest.last()
        && (matches!(
            condition.as_ref(),
            spellwake_engine::toolbox::ConditionExpr::Resolution(rc)
                if resolution_condition_is_spell_cast_invariant(rc)
        ) || rest[..rest.len() - 1]
            .iter()
            .all(is_pure_remember_target_clause))
    {
        let cond_clone = condition.clone();
        if let Some(Instruction::FixedTargetClause(first_body)) = builder.instructions.first_mut() {
            let next_dest = first_body.instructions.len() + 2;
            first_body.instructions.push(Instruction::BranchCondition {
                condition: cond_clone,
                destination: next_dest,
            });
            first_body.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                    kind, amount,
                ),
            );
            return true;
        }
    }
    false
}

pub(super) fn lower_counters_pt_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::Transform { subject } => {
            let clean_subject = strip_battlefield_zone_from_subject(subject);
            if let SubjectRefAst::Compound(items) = &clean_subject
                && !items.is_empty()
            {
                let mut trial = builder.clone();
                for item in items {
                    lower_instruction(
                        &InstructionAst::Transform {
                            subject: Box::new(item.clone()),
                        },
                        &mut trial,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                *builder = trial;
                return Ok(());
            }
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            if is_self_pronoun(&clean_subject, builder, self_pronoun_valid) {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::transform_source(),
                );
                return Ok(());
            }
            if has_source && let SubjectRefAst::Anaphora(raw) = &clean_subject {
                let low = raw.trim().to_ascii_lowercase();
                let explicit_self_noun = matches!(
                    low.as_str(),
                    "this creature"
                        | "this permanent"
                        | "this artifact"
                        | "this enchantment"
                        | "this land"
                        | "this planeswalker"
                        | "this battle"
                        | "this saga"
                        | "this room"
                        | "this vehicle"
                        | "this equipment"
                        | "this aura"
                        | "this artifact creature"
                        | "this enchantment creature"
                );
                if explicit_self_noun
                    || (builder.target == BodyTarget::None
                        && matches!(low.as_str(), "it" | "him" | "her")
                        && prior_op_acted_on_ability_source(&builder.instructions))
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::transform_source(),
                    );
                    return Ok(());
                }
            }
            let mut trial = builder.clone();
            let eff_subj = match &clean_subject {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, &trial, in_trigger, vocabulary,
                    )?;
                    trial.bind_target(bt)?;
                    Some(EffectSubject::TargetObject)
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, &trial, in_trigger, vocabulary,
                    )?;
                    trial.bind_target(bt)?;
                    Some(EffectSubject::TargetObject)
                }
                SubjectRefAst::AttachedHost(_) if has_source => Some(EffectSubject::Matching(
                    ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                )),
                SubjectRefAst::TriggerSubject if in_trigger => Some(EffectSubject::TriggerSubject),
                SubjectRefAst::Anaphora(raw)
                    if last_instruction_created_tokens(&trial.instructions)
                        || matches!(
                            trial.instructions.last(),
                            Some(Instruction::MutatePlayer(spec))
                                if matches!(
                                    spec.op,
                                    spellwake_engine::toolbox::PlayerMutation::Incubate { .. }
                                )
                        )
                        || (has_prior_token_creation(&trial.instructions)
                            && raw.to_ascii_lowercase().contains("token")) =>
                {
                    Some(EffectSubject::CreatedTokens)
                }
                SubjectRefAst::Anaphora(_) if is_permanent_body_target(trial.target) => {
                    Some(EffectSubject::TargetObject)
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&trial.instructions)
                        || matches!(
                            trial.instructions.last(),
                            Some(Instruction::MutatePlayer(spec))
                                if matches!(
                                    spec.op,
                                    spellwake_engine::toolbox::PlayerMutation::ManifestTop { .. }
                                )
                        ) =>
                {
                    Some(EffectSubject::LastMoved)
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && trial.target == BodyTarget::None =>
                {
                    Some(EffectSubject::TriggerSubject)
                }
                SubjectRefAst::Anaphora(_)
                    if trial.target == BodyTarget::None
                        && let Some(group_obj) = trial.last_group_filter =>
                {
                    Some(EffectSubject::Matching(group_obj))
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut f_clean = f.as_ref().clone();
                    f_clean.zone = None;
                    let obj =
                        lower_object_filter_in_context(&f_clean, &trial, in_trigger, vocabulary)?;
                    bind_filter_player_target(&mut trial, &obj)?;
                    trial.last_group_filter = Some(obj);
                    Some(EffectSubject::Matching(obj))
                }
                SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                        && !f.any_spell
                        && !f.is_ability =>
                {
                    let mut inner = f.as_ref().clone();
                    inner.zone = None;
                    let source_zone = if inner.controller == Some(ControllerRelation::You) {
                        inner.controller = None;
                        SourceZoneFilter::Battlefield
                    } else {
                        SourceZoneFilter::AnyBattlefield
                    };
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter_in_context(&inner, &trial, in_trigger, vocabulary)?
                    };
                    if obj.has_target_player() {
                        None
                    } else {
                        trial.instructions.push(Instruction::ChooseObjects {
                            players: AffectedPlayers::One(ResolvingPlayer::Controller),
                            source_zone,
                            filter: Box::new(obj),
                            min: ValueExpr::literal(1),
                            max: ValueExpr::literal(1),
                            distinct_powers: false,
                            max_total_power: None,
                            unchosen_action: None,
                        });
                        Some(EffectSubject::LastMoved)
                    }
                }
                _ => None,
            };
            if let Some(subj) = eff_subj {
                trial.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::transform_subject(
                        subj,
                    ),
                );
                *builder = trial;
                return Ok(());
            }
            Err("oracle:unsupported-transform")
        }
        InstructionAst::BecomePrepared { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder
                .instructions
                .push(Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::set_prepared(),
                )));
            Ok(())
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale: Some(scale_expr),
        } => {
            let clean_subject = match strip_battlefield_zone_from_subject(subject) {
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer) => {
                    SubjectRefAst::Anaphora("they".to_string())
                }
                other => other,
            };
            let subject = &clean_subject;
            if let SubjectRefAst::Compound(items) = subject
                && !items.is_empty()
            {
                let mut trial = builder.clone();
                for item in items {
                    lower_instruction(
                        &InstructionAst::ModifyPT {
                            subject: Box::new(item.clone()),
                            dx: dx.clone(),
                            dy: dy.clone(),
                            duration: duration.clone(),
                            scale: Some(scale_expr.clone()),
                        },
                        &mut trial,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                *builder = trial;
                return Ok(());
            }
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            let dur = lower_duration(duration)?;
            let amt = lower_value_expr_in_context(
                resolve_x(scale_expr, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let anaphora_created = last_instruction_created_tokens(&builder.instructions)
                || (has_prior_token_creation(&builder.instructions)
                    && matches!(subject, SubjectRefAst::Anaphora(raw) if raw.to_ascii_lowercase().contains("token")))
                || (has_prior_token_creation(&builder.instructions)
                    && !is_permanent_body_target(builder.target)
                    && !has_prior_moved_object(&builder.instructions));
            let anaphora_moved = last_instruction_moved_object(&builder.instructions)
                || (!is_permanent_body_target(builder.target)
                    && has_prior_moved_object(&builder.instructions));
            if let (Some(dp), Some(dt)) = (parse_signed_pt_delta(dx), parse_signed_pt_delta(dy)) {
                if dur == ResolvedDuration::UntilEndOfTurn {
                    let group = match subject {
                        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                            Some(EffectSubject::AbilitySource)
                        }
                        SubjectRefAst::AttachedHost(_) if has_source => {
                            Some(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )))
                        }
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            Some(EffectSubject::TriggerSubject)
                        }
                        SubjectRefAst::Anaphora(raw)
                            if has_source
                                && (is_explicit_self_permanent_noun(raw)
                                    || (builder.target == BodyTarget::None
                                        && matches!(
                                            raw.trim().to_ascii_lowercase().as_str(),
                                            "it" | "him" | "her"
                                        )
                                        && prior_op_acted_on_ability_source(
                                            &builder.instructions,
                                        ))) =>
                        {
                            Some(EffectSubject::AbilitySource)
                        }
                        SubjectRefAst::Anaphora(_)
                            if !is_permanent_body_target(builder.target) && anaphora_created =>
                        {
                            Some(EffectSubject::CreatedTokens)
                        }
                        SubjectRefAst::Anaphora(_)
                            if !is_permanent_body_target(builder.target) && anaphora_moved =>
                        {
                            Some(EffectSubject::LastMoved)
                        }
                        SubjectRefAst::Anaphora(_)
                            if !is_permanent_body_target(builder.target)
                                && let Some(group_obj) = builder.last_group_filter =>
                        {
                            Some(EffectSubject::Matching(group_obj))
                        }
                        SubjectRefAst::Anaphora(raw)
                            if in_trigger
                                && !is_permanent_body_target(builder.target)
                                && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                        {
                            Some(EffectSubject::TriggerSubject)
                        }
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f) => {
                            let obj =
                                lower_object_filter_in_context(f, builder, in_trigger, vocabulary)?;
                            bind_filter_player_target(builder, &obj)?;
                            builder.last_group_filter = Some(obj);
                            Some(EffectSubject::Matching(obj))
                        }
                        _ => None,
                    };
                    if let Some(group) = group {
                        let pe = scale_value_expr(amt.clone(), dp);
                        let te = scale_value_expr(amt, dt);
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                                group,
                                pe,
                                te,
                                BTreeSet::new(),
                                BTreeSet::new(),
                            ),
                        );
                        return Ok(());
                    }
                }
                let pe = scale_value_expr(amt.clone(), dp);
                let te = scale_value_expr(amt, dt);
                let part = ResolvedPartSpec::ModifyPTExpr {
                    power: Box::new(pe),
                    toughness: Box::new(te),
                };
                if matches!(subject, SubjectRefAst::Anaphora(_))
                    && !is_permanent_body_target(builder.target)
                    && anaphora_created
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::CreatedTokens,
                            )),
                            ResolvedPartsSpec(vec![part]),
                            dur,
                        ),
                    );
                    return Ok(());
                }
                if matches!(subject, SubjectRefAst::Anaphora(_))
                    && !is_permanent_body_target(builder.target)
                    && anaphora_moved
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                EffectSubject::LastMoved,
                            )),
                            ResolvedPartsSpec(vec![part]),
                            dur,
                        ),
                    );
                    return Ok(());
                }
                if let SubjectRefAst::Anaphora(raw) = subject
                    && has_source
                    && (is_explicit_self_permanent_noun(raw)
                        || (builder.target == BodyTarget::None
                            && matches!(
                                raw.trim().to_ascii_lowercase().as_str(),
                                "it" | "him" | "her"
                            )
                            && prior_op_acted_on_ability_source(&builder.instructions)))
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            ResolvedPartsSpec(vec![part]),
                            dur,
                        ),
                    );
                    return Ok(());
                }
                if matches!(subject, SubjectRefAst::Anaphora(_))
                    && try_append_fixed_first_target_pt_parts(builder, part.clone(), dur)
                {
                    return Ok(());
                }
                if matches!(subject, SubjectRefAst::Anaphora(_))
                    && matches!(
                        builder.target,
                        BodyTarget::SingleAnyTarget
                            | BodyTarget::SingleCreatureOrPlayer
                            | BodyTarget::SinglePlayerOrPlaneswalker
                            | BodyTarget::SingleOpponentOrPlaneswalker
                    )
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::filtered(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ObjectFilter::creature(),
                            ),
                            ResolvedPartsSpec(vec![part]),
                            dur,
                        ),
                    );
                    return Ok(());
                }
                return lower_modify_pt_part(
                    subject,
                    part,
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            let p_term = parse_scaled_pt_delta_term(dx).ok_or("oracle:non-numeric-pt-delta")?;
            let t_term = parse_scaled_pt_delta_term(dy).ok_or("oracle:non-numeric-pt-delta")?;
            let resolve_scaled_term = |term: ScaledPtDeltaTerm| -> Result<ValueExpr, &'static str> {
                match (term, scale_expr.as_ref()) {
                    (ScaledPtDeltaTerm::Literal(n), ValueExprAst::Const(m)) => {
                        Ok(ValueExpr::literal(n.saturating_mul(i64::from(*m))))
                    }
                    (ScaledPtDeltaTerm::ScaledX(k), ValueExprAst::Const(m)) => {
                        let base_x = match bound_x {
                            Some(expr) => {
                                lower_value_expr_in_context(expr, builder, in_trigger, vocabulary)?
                            }
                            None => ValueExpr::chosen_x(),
                        };
                        Ok(scale_value_expr(base_x, k.saturating_mul(i64::from(*m))))
                    }
                    (ScaledPtDeltaTerm::Literal(n), _) => Ok(ValueExpr::literal(n)),
                    (ScaledPtDeltaTerm::ScaledX(k), _) => Ok(scale_value_expr(amt.clone(), k)),
                }
            };
            let pe = resolve_scaled_term(p_term)?;
            let te = resolve_scaled_term(t_term)?;
            let part = ResolvedPartSpec::ModifyPTExpr {
                power: Box::new(pe),
                toughness: Box::new(te),
            };
            if matches!(subject, SubjectRefAst::Anaphora(_))
                && !is_permanent_body_target(builder.target)
                && anaphora_created
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::CreatedTokens,
                        )),
                        ResolvedPartsSpec(vec![part]),
                        dur,
                    ),
                );
                return Ok(());
            }
            if matches!(subject, SubjectRefAst::Anaphora(_))
                && is_permanent_body_target(builder.target)
                && builder.permanent_target_bounds.is_some()
            {
                builder
                    .instructions
                    .push(super::super::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        ResolvedPartsSpec(vec![part]),
                        dur,
                    ));
                return Ok(());
            }
            if matches!(subject, SubjectRefAst::Anaphora(_))
                && !is_permanent_body_target(builder.target)
                && anaphora_moved
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                            EffectSubject::LastMoved,
                        )),
                        ResolvedPartsSpec(vec![part]),
                        dur,
                    ),
                );
                return Ok(());
            }
            if matches!(subject, SubjectRefAst::Anaphora(_))
                && try_append_fixed_first_target_pt_parts(builder, part.clone(), dur)
            {
                return Ok(());
            }
            if matches!(subject, SubjectRefAst::Anaphora(_))
                && matches!(
                    builder.target,
                    BodyTarget::SingleAnyTarget
                        | BodyTarget::SingleCreatureOrPlayer
                        | BodyTarget::SinglePlayerOrPlaneswalker
                        | BodyTarget::SingleOpponentOrPlaneswalker
                )
            {
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::filtered(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            ObjectFilter::creature(),
                        ),
                        ResolvedPartsSpec(vec![part]),
                        dur,
                    ),
                );
                return Ok(());
            }
            lower_modify_pt_part(
                subject,
                part,
                dur,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale: None,
        } => {
            let clean_subject = match strip_battlefield_zone_from_subject(subject) {
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer) => {
                    SubjectRefAst::Anaphora("they".to_string())
                }
                other => other,
            };
            let subject = &clean_subject;
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            let dur = lower_duration(duration)?;
            let dp = parse_signed_pt_delta(dx);
            let dt = parse_signed_pt_delta(dy);
            let anaphora_created = last_instruction_created_tokens(&builder.instructions)
                || (has_prior_token_creation(&builder.instructions)
                    && matches!(subject, SubjectRefAst::Anaphora(raw) if raw.to_ascii_lowercase().contains("token")))
                || (has_prior_token_creation(&builder.instructions)
                    && !is_permanent_body_target(builder.target)
                    && !has_prior_moved_object(&builder.instructions));
            let anaphora_moved = last_instruction_moved_object(&builder.instructions)
                || (!is_permanent_body_target(builder.target)
                    && has_prior_moved_object(&builder.instructions));
            match (dp, dt) {
                (Some(p), Some(t)) => {
                    let pt = PowerToughness {
                        power: p.into(),
                        toughness: t.into(),
                    };
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && !is_permanent_body_target(builder.target)
                        && anaphora_created
                    {
                        if dur == ResolvedDuration::UntilEndOfTurn {
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(EffectSubject::CreatedTokens, ValueExpr::literal(pt.power), ValueExpr::literal(pt.toughness), BTreeSet::new(), BTreeSet::new()));
                        } else {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                        EffectSubject::CreatedTokens,
                                    )),
                                    ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                                    dur,
                                ),
                            );
                        }
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && is_permanent_body_target(builder.target)
                        && builder.permanent_target_bounds.is_some()
                    {
                        builder.instructions.push(
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                                dur,
                            ),
                        );
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && !is_permanent_body_target(builder.target)
                        && anaphora_moved
                    {
                        if dur == ResolvedDuration::UntilEndOfTurn {
                            builder
                                .instructions
                                .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(EffectSubject::LastMoved, ValueExpr::literal(pt.power), ValueExpr::literal(pt.toughness), BTreeSet::new(), BTreeSet::new()));
                        } else {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                        EffectSubject::LastMoved,
                                    )),
                                    ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                                    dur,
                                ),
                            );
                        }
                        return Ok(());
                    }
                    if let SubjectRefAst::Anaphora(raw) = subject
                        && has_source
                        && (is_explicit_self_permanent_noun(raw)
                            || (builder.target == BodyTarget::None
                                && matches!(
                                    raw.trim().to_ascii_lowercase().as_str(),
                                    "it" | "him" | "her"
                                )
                                && prior_op_acted_on_ability_source(&builder.instructions)))
                    {
                        if dur == ResolvedDuration::UntilEndOfTurn {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                                    EffectSubject::AbilitySource,
                                    ValueExpr::literal(pt.power),
                                    ValueExpr::literal(pt.toughness),
                                    BTreeSet::new(),
                                    BTreeSet::new(),
                                ),
                            );
                        } else {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                    spellwake_engine::toolbox::ObjectQuery::Source,
                                    ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                                    dur,
                                ),
                            );
                        }
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && try_append_fixed_first_target_pt_parts(
                            builder,
                            ResolvedPartSpec::ModifyPT(pt.clone()),
                            dur,
                        )
                    {
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && matches!(
                            builder.target,
                            BodyTarget::SingleAnyTarget
                                | BodyTarget::SingleCreatureOrPlayer
                                | BodyTarget::SinglePlayerOrPlaneswalker
                                | BodyTarget::SingleOpponentOrPlaneswalker
                        )
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::filtered(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    ObjectFilter::creature(),
                                ),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                                dur,
                            ),
                        );
                        return Ok(());
                    }
                    lower_modify_pt_target(
                        subject,
                        pt,
                        dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                }
                _ => {
                    let pe = parse_dynamic_pt_expr(dx, bound_x, vocabulary)
                        .ok_or("oracle:unsupported-dynamic-modify-pt")?;
                    let te = parse_dynamic_pt_expr(dy, bound_x, vocabulary)
                        .ok_or("oracle:unsupported-dynamic-modify-pt")?;
                    let part = ResolvedPartSpec::ModifyPTExpr {
                        power: Box::new(pe),
                        toughness: Box::new(te),
                    };
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && !is_permanent_body_target(builder.target)
                        && anaphora_created
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::CreatedTokens,
                                )),
                                ResolvedPartsSpec(vec![part]),
                                dur,
                            ),
                        );
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && is_permanent_body_target(builder.target)
                        && builder.permanent_target_bounds.is_some()
                    {
                        builder.instructions.push(
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![part]),
                                dur,
                            ),
                        );
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && !is_permanent_body_target(builder.target)
                        && anaphora_moved
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::LastMoved,
                                )),
                                ResolvedPartsSpec(vec![part]),
                                dur,
                            ),
                        );
                        return Ok(());
                    }
                    if let SubjectRefAst::Anaphora(raw) = subject
                        && has_source
                        && (is_explicit_self_permanent_noun(raw)
                            || (builder.target == BodyTarget::None
                                && matches!(
                                    raw.trim().to_ascii_lowercase().as_str(),
                                    "it" | "him" | "her"
                                )
                                && prior_op_acted_on_ability_source(&builder.instructions)))
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Source,
                                ResolvedPartsSpec(vec![part]),
                                dur,
                            ),
                        );
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && try_append_fixed_first_target_pt_parts(builder, part.clone(), dur)
                    {
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && matches!(
                            builder.target,
                            BodyTarget::SingleAnyTarget
                                | BodyTarget::SingleCreatureOrPlayer
                                | BodyTarget::SinglePlayerOrPlaneswalker
                                | BodyTarget::SingleOpponentOrPlaneswalker
                        )
                    {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::filtered(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    ObjectFilter::creature(),
                                ),
                                ResolvedPartsSpec(vec![part]),
                                dur,
                            ),
                        );
                        return Ok(());
                    }
                    lower_modify_pt_part(
                        subject,
                        part,
                        dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                }
            }
        }
        InstructionAst::GrantKeywords {
            subject,
            keywords,
            duration,
        } => {
            let eff_subject = match subject.as_ref() {
                SubjectRefAst::InZone {
                    subject: inner,
                    zone: ZoneFilter::Battlefield,
                } => inner.as_ref(),
                other => other,
            };
            let dur = lower_duration(duration)?;
            if dur == ResolvedDuration::UntilEndOfTurn
                && keywords.len() == 1
                && keywords[0].trim().eq_ignore_ascii_case("flashback")
            {
                match eff_subject {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if matches!(filter.zone, Some(ZoneFilter::YourGraveyard) | None)
                        && (filter.any_types == AstCardTypeMask::INSTANT
                            || filter.any_types == AstCardTypeMask::SORCERY
                            || filter.any_types
                                == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY)) =>
                    {
                        let mut inner = filter.as_ref().clone();
                        inner.zone = None;
                        let obj = lower_object_filter(&inner, vocabulary)?;
                        builder.bind_target(BodyTarget::FilteredOwnGraveyardCard(obj))?;
                        builder
                            .instructions
                            .push(Instruction::MutateObject(Box::new(
                                spellwake_engine::toolbox::MutateObjectSpec::all(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    spellwake_engine::toolbox::ObjectMutation::GrantFlashback,
                                ),
                            )));
                        return Ok(());
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if matches!(f.zone, Some(ZoneFilter::YourGraveyard))
                            && (f.any_types == AstCardTypeMask::INSTANT
                                || f.any_types == AstCardTypeMask::SORCERY
                                || f.any_types
                                    == AstCardTypeMask::INSTANT
                                        .union(AstCardTypeMask::SORCERY)) =>
                    {
                        builder
                            .instructions
                            .push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::all(
                                spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                                    zone: spellwake_engine::toolbox::QueryZone::Graveyard(
                                        Box::new(
                                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                                ResolvingPlayer::Controller,
                                            ),
                                        ),
                                    ),
                                    filter: Box::new(ObjectFilter::any()),
                                    slice: spellwake_engine::toolbox::QuerySlice::All,
                                },
                                spellwake_engine::toolbox::ObjectMutation::GrantFlashback,
                            ),
                        )));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let cleaned_keywords: Vec<String> = keywords
                .iter()
                .map(|k| strip_grant_kw_trailing_note(k))
                .filter(|k| !k.is_empty())
                .collect();
            if cleaned_keywords.is_empty() {
                return Err("oracle:unsupported-grant-kw");
            }
            #[derive(Clone, Copy)]
            enum ChosenQualityGrant {
                ProtectionColor,
                ProtectionColorControllerChoice,
                ProtectionArtifactsOrColor,
                ProtectionColorlessOrColor,
                ProtectionCardType,
                HexproofFromChosenColor,
            }
            let classify_chosen_quality = |k: &str| -> Option<ChosenQualityGrant> {
                let low = k.trim().trim_end_matches('.').to_ascii_lowercase();
                match low.as_str() {
                    "protection from the color of your choice"
                    | "protection from the chosen color"
                    | "protection from a color of your choice"
                    | "protection from one color of your choice" => {
                        Some(ChosenQualityGrant::ProtectionColor)
                    }
                    "protection from the card type of your choice"
                    | "protection from a card type of your choice"
                    | "protection from the chosen card type"
                    | "protection from that card type" => {
                        Some(ChosenQualityGrant::ProtectionCardType)
                    }
                    "protection from the color of its controller's choice"
                    | "protection from the color of that creature's controller's choice"
                    | "protection from the color of that permanent's controller's choice"
                    | "protection from a color of its controller's choice" => {
                        Some(ChosenQualityGrant::ProtectionColorControllerChoice)
                    }
                    "protection from artifacts or from the color of your choice"
                    | "protection from artifacts or the color of your choice"
                    | "protection from the color of your choice or from artifacts"
                    | "protection from the color of your choice or artifacts" => {
                        Some(ChosenQualityGrant::ProtectionArtifactsOrColor)
                    }
                    "protection from colorless or from the color of your choice"
                    | "protection from colorless or the color of your choice"
                    | "protection from the color of your choice or from colorless"
                    | "protection from the color of your choice or colorless" => {
                        Some(ChosenQualityGrant::ProtectionColorlessOrColor)
                    }
                    "hexproof from that color"
                    | "hexproof from the chosen color"
                    | "hexproof from the color of your choice"
                    | "hexproof from a color of your choice" => {
                        Some(ChosenQualityGrant::HexproofFromChosenColor)
                    }
                    _ => None,
                }
            };
            let is_each_opponent_prot = |k: &str| {
                let low = k.trim().trim_end_matches('.').to_ascii_lowercase();
                matches!(
                    low.as_str(),
                    "protection from each opponent"
                        | "protection from each of your opponents"
                        | "protection from your opponents"
                )
            };
            let mut chosen_quality_grants: Vec<ChosenQualityGrant> = Vec::new();
            let mut after_chosen: Vec<String> = Vec::with_capacity(cleaned_keywords.len());
            for k in &cleaned_keywords {
                if let Some(kind) = classify_chosen_quality(k) {
                    chosen_quality_grants.push(kind);
                } else {
                    after_chosen.push(k.clone());
                }
            }
            if chosen_quality_grants.len() > 1 {
                return Err("oracle:unsupported-granted-keyword");
            }
            let chosen_quality = chosen_quality_grants.first().copied();
            let has_each_opp_prot = after_chosen.iter().any(|k| is_each_opponent_prot(k));
            let remaining_keywords: Vec<String> = if has_each_opp_prot {
                after_chosen
                    .iter()
                    .filter(|k| !is_each_opponent_prot(k))
                    .cloned()
                    .collect()
            } else {
                after_chosen
            };
            let mut bound_subject_already = false;
            if let Some(chosen_kind) = chosen_quality {
                let push_chosen_prot =
                    |ops: &mut Vec<Instruction>,
                     recipients: ProtectionRecipients,
                     dur: ResolvedDuration| {
                        ops.push(Instruction::ApplyContinuous(Box::new(
                            spellwake_engine::toolbox::ApplyContinuousSpec::chosen_color_protection(
                                recipients, dur,
                            ),
                        )));
                    };
                let mut handled_prot = false;
                if matches!(chosen_kind, ChosenQualityGrant::ProtectionColor) {
                    handled_prot = match eff_subject {
                        SubjectRefAst::TargetObject {
                            filter,
                            count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, builder, in_trigger, vocabulary,
                            )?;
                            builder.bind_target(bt)?;
                            if *tc == TargetCountAst::UpTo(1) {
                                builder.up_to_one_target = true;
                            }
                            bound_subject_already = true;
                            push_chosen_prot(
                                &mut builder.instructions,
                                ProtectionRecipients::TargetPermanent,
                                dur,
                            );
                            true
                        }
                        SubjectRefAst::TargetOneOf {
                            options,
                            count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                        } => {
                            let bt = lower_target_one_of_permanent_body_target(
                                options, builder, in_trigger, vocabulary,
                            )?;
                            builder.bind_target(bt)?;
                            if *tc == TargetCountAst::UpTo(1) {
                                builder.up_to_one_target = true;
                            }
                            bound_subject_already = true;
                            push_chosen_prot(
                                &mut builder.instructions,
                                ProtectionRecipients::TargetPermanent,
                                dur,
                            );
                            true
                        }
                        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                            push_chosen_prot(
                                &mut builder.instructions,
                                ProtectionRecipients::TargetPermanent,
                                dur,
                            );
                            true
                        }
                        SubjectRefAst::Compound(parts)
                            if parts.len() == 2
                                && matches!(
                                    &parts[0],
                                    SubjectRefAst::Player(PlayerRefAst::You)
                                )
                                && matches!(
                                    &parts[1],
                                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                                            && f.controller == Some(ControllerRelation::You)
                                            && (f.any_permanent || f.any_types.is_empty())
                                            && f.forbidden_types.is_empty()
                                            && f.subtypes.is_empty()
                                            && f.any_colors.is_empty()
                                            && f.forbidden_colors.is_empty()
                                            && f.stat_bounds.is_empty()
                                ) =>
                        {
                            push_chosen_prot(
                                &mut builder.instructions,
                                ProtectionRecipients::ControllerAndPermanents,
                                dur,
                            );
                            true
                        }
                        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                            push_chosen_prot(
                                &mut builder.instructions,
                                ProtectionRecipients::AbilitySource,
                                dur,
                            );
                            true
                        }
                        SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                            push_chosen_prot(
                                &mut builder.instructions,
                                ProtectionRecipients::Matching(Box::new(ObjectFilter::Clause(
                                    ObjectClause::any().attached_to_self(),
                                ))),
                                dur,
                            );
                            true
                        }
                        SubjectRefAst::Anaphora(_)
                            if !is_permanent_body_target(builder.target)
                                && let Some(group_obj) = builder.last_group_filter =>
                        {
                            push_chosen_prot(
                                &mut builder.instructions,
                                ProtectionRecipients::Matching(Box::new(group_obj)),
                                dur,
                            );
                            true
                        }
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f)
                            if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                        {
                            let mut f_clean = f.as_ref().clone();
                            f_clean.zone = None;
                            if let Ok(obj) = lower_object_filter_in_context(
                                &f_clean, builder, in_trigger, vocabulary,
                            ) {
                                bind_filter_player_target(builder, &obj)?;
                                push_chosen_prot(
                                    &mut builder.instructions,
                                    ProtectionRecipients::Matching(Box::new(obj)),
                                    dur,
                                );
                                true
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                }
                if !handled_prot {
                    let kw_options: Vec<(&'static str, KeywordAbility)> = match chosen_kind {
                        ChosenQualityGrant::ProtectionColor
                        | ChosenQualityGrant::ProtectionColorControllerChoice => vec![
                            ("white", KeywordAbility::ProtectionFromWhite),
                            ("blue", KeywordAbility::ProtectionFromBlue),
                            ("black", KeywordAbility::ProtectionFromBlack),
                            ("red", KeywordAbility::ProtectionFromRed),
                            ("green", KeywordAbility::ProtectionFromGreen),
                        ],
                        ChosenQualityGrant::ProtectionArtifactsOrColor => vec![
                            (
                                "artifacts",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::CardType(
                                        CardType::Artifact,
                                    ),
                                ),
                            ),
                            ("white", KeywordAbility::ProtectionFromWhite),
                            ("blue", KeywordAbility::ProtectionFromBlue),
                            ("black", KeywordAbility::ProtectionFromBlack),
                            ("red", KeywordAbility::ProtectionFromRed),
                            ("green", KeywordAbility::ProtectionFromGreen),
                        ],
                        ChosenQualityGrant::ProtectionColorlessOrColor => vec![
                            (
                                "colorless",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::Colorless,
                                ),
                            ),
                            ("white", KeywordAbility::ProtectionFromWhite),
                            ("blue", KeywordAbility::ProtectionFromBlue),
                            ("black", KeywordAbility::ProtectionFromBlack),
                            ("red", KeywordAbility::ProtectionFromRed),
                            ("green", KeywordAbility::ProtectionFromGreen),
                        ],
                        ChosenQualityGrant::ProtectionCardType => vec![
                            (
                                "artifact",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::CardType(
                                        CardType::Artifact,
                                    ),
                                ),
                            ),
                            (
                                "battle",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::CardType(
                                        CardType::Battle,
                                    ),
                                ),
                            ),
                            (
                                "creature",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::CardType(
                                        CardType::Creature,
                                    ),
                                ),
                            ),
                            (
                                "enchantment",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::CardType(
                                        CardType::Enchantment,
                                    ),
                                ),
                            ),
                            (
                                "instant",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::CardType(
                                        CardType::Instant,
                                    ),
                                ),
                            ),
                            (
                                "land",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::CardType(
                                        CardType::Land,
                                    ),
                                ),
                            ),
                            (
                                "planeswalker",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::CardType(
                                        CardType::Planeswalker,
                                    ),
                                ),
                            ),
                            (
                                "sorcery",
                                KeywordAbility::ProtectionFrom(
                                    spellwake_engine::characteristics::ProtectionFromQuality::CardType(
                                        CardType::Sorcery,
                                    ),
                                ),
                            ),
                        ],
                        ChosenQualityGrant::HexproofFromChosenColor => vec![
                            ("white", KeywordAbility::HexproofFrom(HexproofQuality::Color(Color::White))),
                            ("blue", KeywordAbility::HexproofFrom(HexproofQuality::Color(Color::Blue))),
                            ("black", KeywordAbility::HexproofFrom(HexproofQuality::Color(Color::Black))),
                            ("red", KeywordAbility::HexproofFrom(HexproofQuality::Color(Color::Red))),
                            ("green", KeywordAbility::HexproofFrom(HexproofQuality::Color(Color::Green))),
                        ],
                    };
                    enum ChoiceTargetKind {
                        TargetExact,
                        TargetExactN(u16),
                        TargetUpTo(u16),
                        TargetX,
                        TargetAnyNumber,
                        AbilitySource,
                        Group(Box<EffectSubject>),
                        Players(AffectedPlayers),
                        ControllerAndPermanents,
                    }
                    let mut trial = builder.clone();
                    let mut trial_bound = false;
                    let target_kind = match eff_subject {
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::Exact(1),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(bt)?;
                            trial_bound = true;
                            Some(ChoiceTargetKind::TargetExact)
                        }
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::Exact(n),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                            && *n >= 2 =>
                        {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(bt)?;
                            trial_bound = true;
                            Some(ChoiceTargetKind::TargetExactN(*n))
                        }
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::UpTo(max),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                            && *max > 0 =>
                        {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(bt)?;
                            trial_bound = true;
                            Some(ChoiceTargetKind::TargetUpTo(*max))
                        }
                        SubjectRefAst::TargetObject {
                            filter,
                            count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                            && !in_trigger =>
                        {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(bt)?;
                            if *tc == TargetCountAst::UpToX {
                                trial.up_to_x_target_count = true;
                            } else {
                                trial.x_target_count = true;
                            }
                            trial_bound = true;
                            Some(ChoiceTargetKind::TargetX)
                        }
                        SubjectRefAst::TargetObject {
                            filter,
                            count: TargetCountAst::AnyNumber,
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let obj = lower_object_filter_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(BodyTarget::FilteredPermanent(obj))?;
                            trial_bound = true;
                            Some(ChoiceTargetKind::TargetAnyNumber)
                        }
                        SubjectRefAst::TargetOneOf {
                            options,
                            count: TargetCountAst::Exact(1),
                        } => {
                            let bt = lower_target_one_of_permanent_body_target(
                                options, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(bt)?;
                            trial_bound = true;
                            Some(ChoiceTargetKind::TargetExact)
                        }
                        SubjectRefAst::TargetOneOf {
                            options,
                            count: TargetCountAst::UpTo(1),
                        } => {
                            let bt = lower_target_one_of_permanent_body_target(
                                options, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(bt)?;
                            trial_bound = true;
                            Some(ChoiceTargetKind::TargetUpTo(1))
                        }
                        SubjectRefAst::Anaphora(_) if is_permanent_body_target(trial.target) => {
                            Some(ChoiceTargetKind::TargetExact)
                        }
                        s if (self_pronoun_valid || in_trigger)
                            && is_self_pronoun(s, &trial, self_pronoun_valid) =>
                        {
                            Some(ChoiceTargetKind::AbilitySource)
                        }
                        SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                            Some(ChoiceTargetKind::Group(Box::new(EffectSubject::Matching(
                                ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                            ))))
                        }
                        SubjectRefAst::TriggerSubject if in_trigger => Some(
                            ChoiceTargetKind::Group(Box::new(EffectSubject::TriggerSubject)),
                        ),
                        SubjectRefAst::Anaphora(_)
                            if !is_permanent_body_target(trial.target)
                                && has_prior_token_creation(&trial.instructions) =>
                        {
                            Some(ChoiceTargetKind::Group(Box::new(
                                EffectSubject::CreatedTokens,
                            )))
                        }
                        SubjectRefAst::Anaphora(_)
                            if !is_permanent_body_target(trial.target)
                                && last_instruction_moved_object(&trial.instructions) =>
                        {
                            Some(ChoiceTargetKind::Group(Box::new(EffectSubject::LastMoved)))
                        }
                        SubjectRefAst::Anaphora(_)
                            if !is_permanent_body_target(trial.target)
                                && let Some(group_obj) = trial.last_group_filter =>
                        {
                            Some(ChoiceTargetKind::Group(Box::new(EffectSubject::Matching(
                                group_obj,
                            ))))
                        }
                        SubjectRefAst::Anaphora(raw)
                            if in_trigger
                                && trial.target == BodyTarget::None
                                && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                        {
                            Some(ChoiceTargetKind::Group(Box::new(
                                EffectSubject::TriggerSubject,
                            )))
                        }
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f)
                            if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                        {
                            let mut f_clean = f.as_ref().clone();
                            f_clean.zone = None;
                            if let Ok(obj) = lower_object_filter_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            ) {
                                bind_filter_player_target(&mut trial, &obj)?;
                                Some(ChoiceTargetKind::Group(Box::new(EffectSubject::Matching(
                                    obj,
                                ))))
                            } else {
                                None
                            }
                        }
                        SubjectRefAst::Compound(parts)
                            if parts.len() == 2
                                && matches!(
                                    &parts[0],
                                    SubjectRefAst::Player(PlayerRefAst::You)
                                )
                                && matches!(
                                    &parts[1],
                                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                                            && f.controller == Some(ControllerRelation::You)
                                            && (f.any_permanent || f.any_types.is_empty())
                                            && f.forbidden_types.is_empty()
                                            && f.subtypes.is_empty()
                                            && f.any_colors.is_empty()
                                            && f.forbidden_colors.is_empty()
                                            && f.stat_bounds.is_empty()
                                ) =>
                        {
                            Some(ChoiceTargetKind::ControllerAndPermanents)
                        }
                        SubjectRefAst::TargetPlayer {
                            opponent_only,
                            count: TargetCountAst::Exact(1),
                        } => {
                            trial.bind_target(if *opponent_only {
                                BodyTarget::SingleOpponent
                            } else {
                                BodyTarget::SinglePlayer
                            })?;
                            trial_bound = true;
                            Some(ChoiceTargetKind::Players(AffectedPlayers::One(
                                ResolvingPlayer::TargetPlayer,
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                            trial.bind_target(BodyTarget::SinglePlayer)?;
                            trial_bound = true;
                            Some(ChoiceTargetKind::Players(AffectedPlayers::One(
                                ResolvingPlayer::TargetPlayer,
                            )))
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                            trial.bind_target(BodyTarget::SingleOpponent)?;
                            trial_bound = true;
                            Some(ChoiceTargetKind::Players(AffectedPlayers::One(
                                ResolvingPlayer::TargetPlayer,
                            )))
                        }
                        SubjectRefAst::Player(p) => {
                            lower_affected_players_in_context(*p, &mut trial, in_trigger)
                                .ok()
                                .map(ChoiceTargetKind::Players)
                        }
                        _ => None,
                    };
                    if let Some(tk) = target_kind {
                        let chooser = match chosen_kind {
                            ChosenQualityGrant::ProtectionColorControllerChoice => {
                                lower_resolving_player_in_context(
                                    PlayerRefAst::ItsController,
                                    &mut trial,
                                    in_trigger,
                                )?
                            }
                            _ => ResolvingPlayer::Controller,
                        };
                        match &tk {
                            ChoiceTargetKind::TargetExactN(n) => {
                                trial.permanent_target_bounds = Some((*n, *n));
                            }
                            ChoiceTargetKind::TargetUpTo(max) => {
                                trial.permanent_target_bounds = Some((0, *max));
                            }
                            ChoiceTargetKind::TargetX => {
                                trial.permanent_target_bounds = Some((0, u16::MAX));
                            }
                            _ => {}
                        }
                        let mut branches = Vec::with_capacity(kw_options.len());
                        for (label, kw) in kw_options {
                            let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                AbilityChange::grant_keywords(BTreeSet::from([kw])),
                            )]);
                            let ops = match &tk {
                                ChoiceTargetKind::TargetExact
                                | ChoiceTargetKind::TargetExactN(_)
                                | ChoiceTargetKind::TargetUpTo(_)
                                | ChoiceTargetKind::TargetX => {
                                    vec![super::super::toolbox_emit::apply_continuous_parts(
                                        spellwake_engine::toolbox::ObjectQuery::Targets,
                                        parts,
                                        dur,
                                    )]
                                }
                                ChoiceTargetKind::TargetAnyNumber => {
                                    vec![Instruction::ApplyContinuous(Box::new(
                                        spellwake_engine::toolbox::ApplyContinuousSpec::modify_any_number(
                                            parts,
                                            dur,
                                        ),
                                    ))]
                                }
                                ChoiceTargetKind::AbilitySource => {
                                    vec![
                                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                            spellwake_engine::toolbox::ObjectQuery::Source,
                                            parts,
                                            dur,
                                        ),
                                    ]
                                }
                                ChoiceTargetKind::Group(subj) => {
                                    vec![
                                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                            spellwake_engine::toolbox::ObjectQuery::EffectSubject(
                                                subj.clone(),
                                            ),
                                            parts,
                                            dur,
                                        ),
                                    ]
                                }
                                ChoiceTargetKind::Players(players) => {
                                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::PlayerGrantKeyword {
                                            players: *players,
                                            keywords: BTreeSet::from([kw]),
                                        }, dur)]
                                }
                                ChoiceTargetKind::ControllerAndPermanents => vec![
                                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::PlayerGrantKeyword {
                                            players: AffectedPlayers::One(
                                                ResolvingPlayer::Controller,
                                            ),
                                            keywords: BTreeSet::from([kw]),
                                        }, dur),
                                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                        spellwake_engine::toolbox::ObjectQuery::EffectSubject(
                                            Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                                ObjectClause::any().controlled(),
                                            ))),
                                        ),
                                        parts,
                                        dur,
                                    ),
                                ],
                            };
                            branches.push(ChooseOptionBranch {
                                label: label.to_string(),
                                body: ExecutableBody::new(BodyTarget::None, ops),
                            });
                        }
                        trial.instructions.push(Instruction::ChooseOption {
                            chooser,
                            options: branches,
                        });
                        *builder = trial;
                        bound_subject_already = trial_bound;
                        handled_prot = true;
                    }
                }
                if handled_prot {
                    if !has_each_opp_prot && remaining_keywords.is_empty() {
                        return Ok(());
                    }
                } else {
                    return Err("oracle:unsupported-granted-keyword");
                }
            }
            if has_each_opp_prot {
                let mut parts = vec![ResolvedPartSpec::ProtectionFromEachOpponent];
                if !remaining_keywords.is_empty() {
                    let ab = lower_ability_change(&remaining_keywords, vocabulary)?;
                    parts.push(ResolvedPartSpec::Abilities(ab));
                }
                let anaphora_subj = SubjectRefAst::Anaphora("it".to_string());
                let eff_subj = if bound_subject_already {
                    &anaphora_subj
                } else {
                    eff_subject
                };
                return emit_resolved_parts_for_subject(
                    eff_subj,
                    ResolvedPartsSpec(parts),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )
                .map_err(|_| "oracle:unsupported-grant-kw");
            }
            let mut expanded_keywords: Vec<String> = Vec::with_capacity(remaining_keywords.len());
            let mut extra_type_parts: Vec<ResolvedPartSpec> = Vec::new();
            let mut extra_combat_kws: Vec<spellwake_engine::program::CombatTriggerKeyword> =
                Vec::new();
            let mut extra_toxic: Option<RulesInteger> = None;
            fn normalize_prot_item(item: &str) -> &str {
                match item {
                    "instant spells" => "instants",
                    "sorcery spells" => "sorceries",
                    other => other,
                }
            }
            for kw in &remaining_keywords {
                let low = kw.trim().trim_end_matches('.').to_ascii_lowercase();
                if matches!(
                    low.as_str(),
                    "all basic land types" | "every basic land type"
                ) {
                    extra_type_parts.push(ResolvedPartSpec::Type(TypeChangeSpec {
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Keep,
                        land_subtypes: SubtypeOperation::Add(BTreeSet::from([
                            LandType::Plains,
                            LandType::Island,
                            LandType::Swamp,
                            LandType::Mountain,
                            LandType::Forest,
                        ])),
                    }));
                    continue;
                }
                if let Some((kw_head, num_raw)) = low.split_once(' ')
                    && let Ok(n) = num_raw.trim().parse::<i64>()
                    && n > 0
                {
                    if kw_head == "toxic" {
                        let add = RulesInteger::from(n);
                        extra_toxic = Some(match extra_toxic.take() {
                            Some(prev) => prev.plus(&add),
                            None => add,
                        });
                        continue;
                    }
                    let ckw = match kw_head {
                        "bushido" => Some(
                            spellwake_engine::program::CombatTriggerKeyword::Bushido(n.into()),
                        ),
                        "afflict" => Some(
                            spellwake_engine::program::CombatTriggerKeyword::Afflict(n.into()),
                        ),
                        "frenzy" => Some(spellwake_engine::program::CombatTriggerKeyword::Frenzy(
                            n.into(),
                        )),
                        "rampage" => Some(
                            spellwake_engine::program::CombatTriggerKeyword::Rampage(n.into()),
                        ),
                        "poisonous" => Some(
                            spellwake_engine::program::CombatTriggerKeyword::Poisonous(n.into()),
                        ),
                        "renown" => Some(spellwake_engine::program::CombatTriggerKeyword::Renown(
                            n.into(),
                        )),
                        "mobilize" => Some(
                            spellwake_engine::program::CombatTriggerKeyword::Mobilize(n.into()),
                        ),
                        _ => None,
                    };
                    if let Some(ckw) = ckw {
                        extra_combat_kws.push(ckw);
                        continue;
                    }
                }
                if let Some(rest) = low.strip_prefix("protection from ")
                    && (rest.contains(',') || rest.contains(" and from "))
                {
                    let mut parts = Vec::new();
                    let mut ok = true;
                    for comma_seg in rest.split(',') {
                        let seg = comma_seg.trim();
                        let seg = seg.strip_prefix("and ").unwrap_or(seg).trim();
                        if seg.is_empty() {
                            ok = false;
                            break;
                        }
                        for sub in seg.split(" and ") {
                            let item = sub.trim();
                            let item = item.strip_prefix("from ").unwrap_or(item).trim();
                            if item.is_empty() {
                                ok = false;
                                break;
                            }
                            let norm_item = normalize_prot_item(item);
                            parts.push(format!("protection from {norm_item}"));
                        }
                        if !ok {
                            break;
                        }
                    }
                    if ok && parts.len() >= 2 {
                        expanded_keywords.extend(parts);
                        continue;
                    }
                }
                if let Some(rest) = low.strip_prefix("protection from ") {
                    let norm_item = normalize_prot_item(rest.trim());
                    if norm_item != rest.trim() {
                        expanded_keywords.push(format!("protection from {norm_item}"));
                        continue;
                    }
                }
                expanded_keywords.push(kw.clone());
            }
            let (mut extra_parts, kw_remaining) = partition_temp_keyword_grants(&expanded_keywords);
            extra_parts.extend(extra_type_parts);
            let anaphora_after_chosen = SubjectRefAst::Anaphora("it".to_string());
            let grant_subject = if bound_subject_already {
                &anaphora_after_chosen
            } else {
                eff_subject
            };
            if !extra_parts.is_empty() || !extra_combat_kws.is_empty() || extra_toxic.is_some() {
                let mut ab_change = if !kw_remaining.is_empty() {
                    Some(lower_ability_change(&kw_remaining, vocabulary)?)
                } else {
                    None
                };
                if !extra_combat_kws.is_empty() {
                    let ab = ab_change.get_or_insert_with(AbilityChange::default);
                    ab.granted_combat_keywords.extend(extra_combat_kws);
                }
                if let Some(tox) = extra_toxic {
                    let ab = ab_change.get_or_insert_with(AbilityChange::default);
                    ab.granted_toxic = Some(match ab.granted_toxic.take() {
                        Some(prev) => prev.plus(&tox),
                        None => tox,
                    });
                }
                if let Some(ab) = ab_change {
                    extra_parts.push(ResolvedPartSpec::Abilities(ab));
                }
                return emit_resolved_parts_for_subject(
                    grant_subject,
                    ResolvedPartsSpec(extra_parts),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            let ab_change = lower_ability_change(&kw_remaining, vocabulary)?;
            let pure_kws = (ab_change.removal.is_none()
                && ab_change.suppressed_keywords.is_empty()
                && ab_change.granted_exalted == 0
                && !ab_change.grants_unblockable
                && ab_change.granted_triggers.is_empty()
                && ab_change.granted_toxic.is_none()
                && ab_change.granted_combat_keywords.is_empty()
                && !ab_change.granted_keywords.is_empty())
            .then(|| ab_change.granted_keywords.clone());
            match grant_subject {
                s if (self_pronoun_valid || in_trigger)
                    && is_self_pronoun(s, builder, self_pronoun_valid)
                    && dur == ResolvedDuration::UntilEndOfTurn =>
                {
                    if let Some(kws) = pure_kws {
                        if let Some(Instruction::ApplyContinuous(spec)) =
                            builder.instructions.last_mut()
                            && spec.duration == ResolvedDuration::UntilEndOfTurn
                            && (matches!(
                                &spec.selector,
                                Some(spellwake_engine::toolbox::ObjectQuery::Source)
                            ) || matches!(
                                &spec.selector,
                                Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(s))
                                    if matches!(s.as_ref(), EffectSubject::AbilitySource)
                            ))
                            && let spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                                keywords: prev_kws,
                                ..
                            } = &mut spec.payload
                        {
                            prev_kws.extend(kws);
                            return Ok(());
                        }
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(
                                EffectSubject::AbilitySource,
                                ValueExpr::literal(0),
                                ValueExpr::literal(0),
                                kws,
                                BTreeSet::new(),
                            ),
                        );
                        return Ok(());
                    }
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            dur,
                        ),
                    );
                    Ok(())
                }
                s if (self_pronoun_valid || in_trigger)
                    && is_self_pronoun(s, builder, self_pronoun_valid) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]), dur));
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]), dur));
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::UpTo(max),
                } if *max > 0 => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(0, *max, super::super::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]), dur));
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(n),
                } if *n >= 2 => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(*n, *n, super::super::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]), dur));
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::AnyNumber,
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(0, u16::MAX, super::super::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]), dur));
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
                } if !in_trigger => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    if *tc == TargetCountAst::UpToX {
                        builder.up_to_x_target_count = true;
                    } else {
                        builder.x_target_count = true;
                    }
                    builder.push_bounded_target_op(0, u16::MAX, super::super::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]), dur));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                    if builder.permanent_target_bounds.is_some() {
                        builder.instructions.push(
                            super::super::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                        return Ok(());
                    }
                    if let Some(Instruction::ApplyContinuous(spec)) =
                        builder.instructions.last_mut()
                        && matches!(
                            spec.selector,
                            Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                        )
                    {
                        if spec.duration == dur
                            && let spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(
                                parts,
                            ) = &mut spec.payload
                            && !parts
                                .0
                                .iter()
                                .any(|p| matches!(p, ResolvedPartSpec::Abilities(_)))
                        {
                            parts.0.push(ResolvedPartSpec::Abilities(ab_change));
                            return Ok(());
                        }
                        if let Some(kws) = pure_kws
                            && dur == ResolvedDuration::UntilEndOfTurn
                            && spec.duration == ResolvedDuration::UntilEndOfTurn
                            && let spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                                keywords: prev_kws,
                                ..
                            } = &mut spec.payload
                        {
                            prev_kws.extend(kws);
                            return Ok(());
                        }
                    }
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(n),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(*n, *n, super::super::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]), dur));
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(max),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *max > 0 => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(0, *max, super::super::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]), dur));
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && !in_trigger => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    if *tc == TargetCountAst::UpToX {
                        builder.up_to_x_target_count = true;
                    } else {
                        builder.x_target_count = true;
                    }
                    builder.push_bounded_target_op(0, u16::MAX, super::super::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]), dur));
                    Ok(())
                }
                SubjectRefAst::Compound(items) if !items.is_empty() => {
                    for it in items {
                        lower_instruction(
                            &InstructionAst::GrantKeywords {
                                subject: Box::new(it.clone()),
                                keywords: keywords.clone(),
                                duration: duration.clone(),
                            },
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                    }
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::AnyNumber,
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let obj =
                        lower_object_filter_in_context(&f_clean, builder, in_trigger, vocabulary)?;
                    builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
                    builder.instructions.push(Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::modify_any_number(
                            ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            dur,
                        ),
                    )));
                    Ok(())
                }
                SubjectRefAst::Player(p)
                    if pure_kws.as_ref().is_some_and(|kws| {
                        !kws.is_empty() && kws.iter().all(is_player_keyword)
                    }) =>
                {
                    let players = lower_affected_players_in_context(*p, builder, in_trigger)?;
                    let Some(kws) = pure_kws else {
                        return Err("oracle:unsupported-grant-kw");
                    };
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::PlayerGrantKeyword {
                            players,
                            keywords: kws,
                        }, dur));
                    Ok(())
                }
                SubjectRefAst::TargetPlayer {
                    opponent_only,
                    count: TargetCountAst::Exact(1),
                } if pure_kws
                    .as_ref()
                    .is_some_and(|kws| !kws.is_empty() && kws.iter().all(is_player_keyword)) =>
                {
                    builder.bind_target(if *opponent_only {
                        BodyTarget::SingleOpponent
                    } else {
                        BodyTarget::SinglePlayer
                    })?;
                    let Some(kws) = pure_kws else {
                        return Err("oracle:unsupported-grant-kw");
                    };
                    builder.instructions.push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_temporary_rule(TemporaryRuleKind::PlayerGrantKeyword {
                            players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                            keywords: kws,
                        }, dur));
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if is_permanent_body_target(builder.target) =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]), dur));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if is_permanent_body_target(builder.target)
                        && builder.instructions.iter().any(|op| {
                            matches!(
                                op,
                                Instruction::ApplyContinuous(spec)
                                    if matches!(
                                        spec.payload,
                                        spellwake_engine::toolbox::ContinuousPayload::ModifyAnyNumber {
                                            ..
                                        }
                                    )
                            )
                        }) =>
                {
                    builder.instructions.push(Instruction::ApplyContinuous(Box::new(
                        spellwake_engine::toolbox::ApplyContinuousSpec::modify_any_number(
                            ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            dur,
                        ),
                    )));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) | SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if !is_permanent_body_target(builder.target)
                        && (last_instruction_moved_object(&builder.instructions)
                            || matches!(
                                builder.instructions.last(),
                                Some(Instruction::MutateObject(_))
                            )) =>
                {
                    let group = Box::new(EffectSubject::LastMoved);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(*group, ValueExpr::literal(0), ValueExpr::literal(0), kws, BTreeSet::new()));
                    } else {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(group),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) | SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if !is_permanent_body_target(builder.target)
                        && has_prior_token_creation(&builder.instructions) =>
                {
                    let group = Box::new(EffectSubject::CreatedTokens);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(*group, ValueExpr::literal(0), ValueExpr::literal(0), kws, BTreeSet::new()));
                    } else {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(group),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) | SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if !is_permanent_body_target(builder.target)
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(EffectSubject::Matching(group_obj), ValueExpr::literal(0), ValueExpr::literal(0), kws, BTreeSet::new()));
                    } else {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    EffectSubject::Matching(group_obj),
                                )),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) | SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if !is_permanent_body_target(builder.target)
                        && has_prior_moved_object(&builder.instructions) =>
                {
                    let group = Box::new(EffectSubject::LastMoved);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(*group, ValueExpr::literal(0), ValueExpr::literal(0), kws, BTreeSet::new()));
                    } else {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(group),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if try_append_fixed_first_target_parts(
                        builder,
                        ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change.clone())]),
                        dur,
                    ) =>
                {
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    let group = Box::new(EffectSubject::TriggerSubject);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(*group, ValueExpr::literal(0), ValueExpr::literal(0), kws, BTreeSet::new()));
                    } else {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(group),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    let group = Box::new(EffectSubject::TriggerSubject);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(*group, ValueExpr::literal(0), ValueExpr::literal(0), kws, BTreeSet::new()));
                    } else {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(group),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if in_trigger && builder.target == BodyTarget::None =>
                {
                    let group = Box::new(EffectSubject::TriggerSubject);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(*group, ValueExpr::literal(0), ValueExpr::literal(0), kws, BTreeSet::new()));
                    } else {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(group),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    let group = Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause::any().attached_to_self(),
                    )));
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(*group, ValueExpr::literal(0), ValueExpr::literal(0), kws, BTreeSet::new()));
                    } else {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(group),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut f_clean = f.as_ref().clone();
                    f_clean.zone = None;
                    let obj =
                        lower_object_filter_in_context(&f_clean, builder, in_trigger, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    let group = Box::new(EffectSubject::Matching(obj));
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && !obj.has_target_player()
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::modify_group_eot(*group, ValueExpr::literal(0), ValueExpr::literal(0), kws, BTreeSet::new()));
                    } else {
                        builder.instructions.push(
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(group),
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                                dur,
                            ),
                        );
                    }
                    Ok(())
                }
                _ => Err("oracle:unsupported-grant-kw"),
            }
        }
        InstructionAst::SetDynamicPT {
            subject,
            power,
            toughness,
            duration,
        } => {
            if let (Some(p_ast), Some(t_ast)) = (power.as_deref(), toughness.as_deref()) {
                let p_expr = lower_value_expr_in_context(
                    resolve_x(p_ast, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?;
                let t_expr = lower_value_expr_in_context(
                    resolve_x(t_ast, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?;
                let dur = lower_duration(duration)?;
                return lower_modify_pt_part(
                    subject,
                    ResolvedPartSpec::SetPTExpr {
                        power: Box::new(p_expr),
                        toughness: Box::new(t_expr),
                    },
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                );
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::SwitchPowerToughness { subject, duration } => {
            let dur = lower_duration(duration)?;
            match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]), dur));
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(
                        0,
                        1,
                        super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]), dur));
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::UpTo(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(
                        0,
                        1,
                        super::super::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Targets,
                            ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::SelfObject => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                            spellwake_engine::toolbox::ObjectQuery::Source,
                            ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                            dur,
                        ),
                    );
                    Ok(())
                }
                _ => Err("oracle:unsupported-switch-pt"),
            }
        }
        InstructionAst::RemoveAllAbilities { subject, duration } => {
            let dur = lower_duration(duration)?;
            emit_resolved_parts_for_subject(
                subject,
                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                    AbilityRemoval::All,
                ))]),
                dur,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )
            .map_err(|_| "oracle:unsupported-remove-abilities")
        }
        InstructionAst::Suspect { subject } => lower_instruction(
            &InstructionAst::PutCounters {
                subject: subject.clone(),
                counter_kind: "suspect".to_string(),
                count: Box::new(ValueExprAst::Const(1)),
            },
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::SetSuspected { subject, remove } => {
            if *remove {
                lower_instruction(
                    &InstructionAst::RemoveCounters {
                        subject: subject.clone(),
                        counter_kind: "suspect".to_string(),
                        all: true,
                        count: None,
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
            } else {
                lower_instruction(
                    &InstructionAst::PutCounters {
                        subject: subject.clone(),
                        counter_kind: "suspect".to_string(),
                        count: Box::new(ValueExprAst::Const(1)),
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
            }
        }
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } => {
            let kind_low_full = counter_kind.trim().to_ascii_lowercase();
            let kind_trimmed = kind_low_full.trim_end_matches('.').trim();
            if in_trigger
                && matches!(count.as_ref(), ValueExprAst::Const(1))
                && matches!(
                    kind_trimmed,
                    "those"
                        | "all of those"
                        | "all those"
                        | "its"
                        | "all of its"
                        | "all its"
                        | "~'s"
                        | "all of ~'s"
                        | "this creature's"
                        | "this permanent's"
                        | "this artifact's"
                        | "that creature's"
                        | "that permanent's"
                        | "that artifact's"
                        | "the dying creature's"
                        | "all counters on it"
                        | "all of the counters on it"
                        | "all counters from it"
                        | "all of the counters from it"
                        | "the counters on it"
                        | "the counters from it"
                        | "those counters"
                        | "all of those counters"
                        | "its counters"
                        | "all of its counters"
                        | "its counters on it"
                        | "~'s counters"
                        | "all of ~'s counters"
                        | "this creature's counters"
                        | "that creature's counters"
                        | "this permanent's counters"
                        | "that permanent's counters"
                        | "this artifact's counters"
                        | "that artifact's counters"
                        | "the counters that were on it"
                        | "all counters that were on it"
                )
            {
                let eff_subject = match subject.as_ref() {
                    SubjectRefAst::InZone {
                        subject: inner,
                        zone: ZoneFilter::Battlefield,
                    } => inner.as_ref(),
                    other => other,
                };
                let has_source = in_trigger || self_pronoun_valid || builder.has_source;
                let mut trial = builder.clone();
                let target_subj = match eff_subject {
                    s if has_source && is_self_pronoun(s, &trial, self_pronoun_valid) => {
                        Some(EffectSubject::AbilitySource)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if has_source
                            && (is_explicit_self_permanent_noun(raw)
                                || (trial.target == BodyTarget::None
                                    && matches!(
                                        raw.trim().to_ascii_lowercase().as_str(),
                                        "it" | "him" | "her"
                                    )
                                    && prior_op_acted_on_ability_source(&trial.instructions))) =>
                    {
                        Some(EffectSubject::AbilitySource)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(EffectSubject::TriggerSubject)
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, &trial, in_trigger, vocabulary,
                        )?;
                        trial.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            trial.up_to_one_target = true;
                        }
                        Some(EffectSubject::TargetObject)
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: count @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, &trial, in_trigger, vocabulary,
                        )?;
                        trial.bind_target(bt)?;
                        if *count == TargetCountAst::UpTo(1) {
                            trial.up_to_one_target = true;
                        }
                        Some(EffectSubject::TargetObject)
                    }
                    SubjectRefAst::AttachedHost(_) if has_source => Some(EffectSubject::Matching(
                        ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                    )),
                    SubjectRefAst::Anaphora(raw)
                        if has_prior_token_creation(&trial.instructions)
                            && raw.to_ascii_lowercase().contains("token") =>
                    {
                        Some(EffectSubject::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(trial.target) => {
                        Some(EffectSubject::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&trial.instructions)
                            || (has_prior_token_creation(&trial.instructions)
                                && !is_permanent_body_target(trial.target)
                                && !has_prior_moved_object(&trial.instructions)) =>
                    {
                        Some(EffectSubject::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&trial.instructions)
                            || (!is_permanent_body_target(trial.target)
                                && has_prior_moved_object(&trial.instructions)) =>
                    {
                        Some(EffectSubject::LastMoved)
                    }
                    SubjectRefAst::Anaphora(_)
                        if trial.target == BodyTarget::None
                            && let Some(group_obj) = trial.last_group_filter =>
                    {
                        Some(EffectSubject::Matching(group_obj))
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, &trial, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(&mut trial, &obj)?;
                        trial.last_group_filter = Some(obj);
                        Some(EffectSubject::Matching(obj))
                    }
                    _ => None,
                };
                if let Some(eff_subj) = target_subj {
                    trial
                        .instructions
                        .push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::all(
                                spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                                    eff_subj,
                                )),
                                spellwake_engine::toolbox::ObjectMutation::TransferCountersFromTriggeringLki,
                            ),
                        )));
                    *builder = trial;
                    return Ok(());
                }
                return Err("oracle:put-counter-kind");
            }
            if in_trigger
                && self_pronoun_valid
                && matches!(count.as_ref(), ValueExprAst::Const(1))
                && let Some(kind_rest) = kind_trimmed
                    .strip_prefix("all of its ")
                    .or_else(|| kind_trimmed.strip_prefix("all its "))
                    .or_else(|| kind_trimmed.strip_prefix("its "))
                    .or_else(|| kind_trimmed.strip_prefix("all of ~'s "))
                    .or_else(|| kind_trimmed.strip_prefix("~'s "))
                    .or_else(|| kind_trimmed.strip_prefix("this creature's "))
                    .or_else(|| kind_trimmed.strip_prefix("this permanent's "))
                    .or_else(|| kind_trimmed.strip_prefix("this artifact's "))
                && let Some(ck_clean) = Some(
                    kind_rest
                        .trim()
                        .strip_suffix(" counters")
                        .or_else(|| kind_rest.trim().strip_suffix(" counter"))
                        .unwrap_or(kind_rest.trim())
                        .trim(),
                )
                && let Some(ck) = parse_counter_kind(ck_clean)
                && ck.is_permanent()
            {
                let eff_subject = match subject.as_ref() {
                    SubjectRefAst::InZone {
                        subject: inner,
                        zone: ZoneFilter::Battlefield,
                    } => inner.as_ref(),
                    other => other,
                };
                let src_cnt = ValueExpr::CounterCount {
                    subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                    kind: Some(ck),
                };
                let mut trial = builder.clone();
                let target_query = match eff_subject {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, &trial, in_trigger, vocabulary,
                        )?;
                        trial.bind_target(bt)?;
                        if *tc == TargetCountAst::UpTo(1) {
                            trial.up_to_one_target = true;
                        }
                        Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, &trial, in_trigger, vocabulary,
                        )?;
                        trial.bind_target(bt)?;
                        if *tc == TargetCountAst::UpTo(1) {
                            trial.up_to_one_target = true;
                        }
                        Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(trial.target) => {
                        Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, &trial, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(&mut trial, &obj)?;
                        trial.last_group_filter = Some(obj);
                        Some(spellwake_engine::toolbox::ObjectQuery::EffectSubject(
                            Box::new(EffectSubject::Matching(obj)),
                        ))
                    }
                    _ => None,
                };
                if let Some(query) = target_query {
                    trial.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                            query, ck, src_cnt,
                        ),
                    );
                    *builder = trial;
                    return Ok(());
                }
            }
            if matches!(count.as_ref(), ValueExprAst::Const(1))
                && matches!(
                    kind_low_full.as_str(),
                    "each_kind_already_on"
                        | "of each kind already on it"
                        | "each kind already on it"
                        | "another counter of each kind already on it"
                        | "an additional counter of each kind already on it"
                        | "a counter of each kind already on it"
                        | "one of each kind already on it"
                        | "one of each kind on it"
                        | "of each kind already there"
                        | "each kind already there"
                        | "of each kind already on that permanent"
                        | "of each kind already on that creature"
                        | "each kind of counter already on it"
                        | "of each kind of counter already on it"
                        | "of each kind on it"
                        | "another counter of that kind"
                        | "a counter of each kind on it"
                        | "each kind of counter on it"
                        | "of each kind of counter on it"
                        | "each kind of counter on that permanent"
                        | "of each kind of counter on that permanent"
                        | "of each kind of counter already on that permanent"
                        | "each kind of counter on that creature"
                        | "of each kind of counter on that creature"
                        | "of each kind of counter already on that creature"
                        | "another counter of each kind on it"
                        | "an additional counter of each kind on it"
                        | "another counter of each kind already on that permanent"
                        | "another counter of each kind already on that creature"
                        | "a counter of each kind already on that permanent"
                        | "a counter of each kind already on that creature"
                        | "of each kind already on them"
                        | "each kind already on them"
                        | "of each kind on them"
                        | "each kind on them"
                        | "a counter of each kind already on them"
                        | "another counter of each kind already on them"
                        | "an additional counter of each kind already on them"
                )
            {
                let eff_subject = match subject.as_ref() {
                    SubjectRefAst::InZone {
                        subject: inner,
                        zone: ZoneFilter::Battlefield,
                    } => inner.as_ref(),
                    other => other,
                };
                if let SubjectRefAst::Compound(items) = eff_subject
                    && !items.is_empty()
                {
                    let mut trial = builder.clone();
                    for item in items {
                        lower_instruction(
                            &InstructionAst::PutCounters {
                                subject: Box::new(item.clone()),
                                counter_kind: counter_kind.clone(),
                                count: count.clone(),
                            },
                            &mut trial,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                    }
                    *builder = trial;
                    return Ok(());
                }
                let has_source = in_trigger || self_pronoun_valid || builder.has_source;
                let mut trial = builder.clone();
                let target_subj = match eff_subject {
                    s if has_source && is_self_pronoun(s, &trial, self_pronoun_valid) => {
                        Some(EffectSubject::AbilitySource)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if has_source
                            && (is_explicit_self_permanent_noun(raw)
                                || (trial.target == BodyTarget::None
                                    && matches!(
                                        raw.trim().to_ascii_lowercase().as_str(),
                                        "it" | "him" | "her"
                                    )
                                    && prior_op_acted_on_ability_source(&trial.instructions))) =>
                    {
                        Some(EffectSubject::AbilitySource)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(EffectSubject::TriggerSubject)
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, &trial, in_trigger, vocabulary,
                        )?;
                        trial.bind_target(bt)?;
                        if *tc == TargetCountAst::UpTo(1) {
                            trial.up_to_one_target = true;
                        }
                        Some(EffectSubject::TargetObject)
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, &trial, in_trigger, vocabulary,
                        )?;
                        trial.bind_target(bt)?;
                        if *tc == TargetCountAst::UpTo(1) {
                            trial.up_to_one_target = true;
                        }
                        Some(EffectSubject::TargetObject)
                    }
                    SubjectRefAst::AttachedHost(_) if has_source => Some(EffectSubject::Matching(
                        ObjectFilter::Clause(ObjectClause::any().attached_to_self()),
                    )),
                    SubjectRefAst::Anaphora(raw)
                        if has_prior_token_creation(&trial.instructions)
                            && raw.to_ascii_lowercase().contains("token") =>
                    {
                        Some(EffectSubject::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(trial.target) => {
                        Some(EffectSubject::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&trial.instructions)
                            || (has_prior_token_creation(&trial.instructions)
                                && !is_permanent_body_target(trial.target)
                                && !has_prior_moved_object(&trial.instructions)) =>
                    {
                        Some(EffectSubject::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&trial.instructions)
                            || (!is_permanent_body_target(trial.target)
                                && has_prior_moved_object(&trial.instructions)) =>
                    {
                        Some(EffectSubject::LastMoved)
                    }
                    SubjectRefAst::Anaphora(_)
                        if trial.target == BodyTarget::None
                            && let Some(group_obj) = trial.last_group_filter =>
                    {
                        Some(EffectSubject::Matching(group_obj))
                    }
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                            && trial.target == BodyTarget::None =>
                    {
                        Some(EffectSubject::TriggerSubject)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = lower_object_filter_in_context(
                            &f_clean, &trial, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(&mut trial, &obj)?;
                        trial.last_group_filter = Some(obj);
                        Some(EffectSubject::Matching(obj))
                    }
                    _ => None,
                };
                if let Some(eff_subj) = target_subj {
                    trial
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::add_one_of_each_counter(eff_subj));
                    *builder = trial;
                    return Ok(());
                }
                return Err("oracle:put-counter-kind");
            }
            if matches!(count.as_ref(), ValueExprAst::Const(1))
                && let Some(rest_kind) = kind_low_full.strip_prefix("twice that many ")
            {
                let rest_trimmed = rest_kind.trim();
                if !rest_trimmed.is_empty() {
                    return lower_instruction(
                        &InstructionAst::PutCounters {
                            subject: subject.clone(),
                            counter_kind: rest_trimmed.to_string(),
                            count: Box::new(ValueExprAst::Mul(
                                Box::new(ValueExprAst::Const(2)),
                                Box::new(ValueExprAst::PriorResult),
                            )),
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
            }
            let choice_list_stem: Option<&str> = kind_low_full
                .strip_prefix("choice:")
                .or_else(|| {
                    kind_low_full
                        .strip_prefix("your choice of a counter from among ")
                        .or_else(|| {
                            kind_low_full.strip_prefix("a counter of your choice from among ")
                        })
                        .or_else(|| {
                            kind_low_full.strip_prefix("counter of your choice from among ")
                        })
                        .or_else(|| kind_low_full.strip_prefix("one counter from among "))
                        .or_else(|| {
                            kind_low_full
                                .strip_prefix("your choice of a ")
                                .or_else(|| kind_low_full.strip_prefix("your choice of an "))
                        })
                        .and_then(|rest| {
                            if rest.starts_with("counter from among ") {
                                rest.strip_prefix("counter from among ")
                            } else if (kind_low_full.starts_with("your choice of a ")
                                || kind_low_full.starts_with("your choice of an "))
                                && !kind_low_full
                                    .starts_with("your choice of a counter from among ")
                            {
                                rest.contains(" or ").then_some(rest)
                            } else {
                                Some(rest)
                            }
                        })
                })
                .or_else(|| kind_low_full.strip_prefix("a counter from among "))
                .or_else(|| kind_low_full.strip_prefix("counter from among "))
                .or_else(|| kind_low_full.strip_prefix("from among "))
                .or_else(|| {
                    (kind_low_full.contains(" or ")
                        && parse_counter_kind(kind_low_full.as_str()).is_none())
                    .then(|| {
                        kind_low_full
                            .strip_prefix("your choice of ")
                            .or_else(|| kind_low_full.strip_prefix("a choice of "))
                            .or_else(|| kind_low_full.strip_prefix("either "))
                            .unwrap_or(kind_low_full.as_str())
                    })
                });
            if let Some(choice_stem_raw) = choice_list_stem {
                let choice_stem = choice_stem_raw.trim().trim_end_matches('.').trim();
                let mut raw_opts: Vec<&str> = Vec::new();
                for seg in choice_stem.split(" or ") {
                    for sub in seg.split(',') {
                        let t = sub
                            .trim()
                            .trim_start_matches("or ")
                            .trim_start_matches("and ")
                            .trim();
                        if !t.is_empty() {
                            if let Some((a, b)) = t.split_once(" and ") {
                                if !a.trim().is_empty() {
                                    raw_opts.push(a.trim());
                                }
                                if !b.trim().is_empty() {
                                    raw_opts.push(b.trim());
                                }
                            } else {
                                raw_opts.push(t);
                            }
                        }
                    }
                }
                let mut parsed_opts: Vec<(String, CounterKind)> = Vec::new();
                let mut all_valid = raw_opts.len() >= 2 && raw_opts.len() <= 16;
                for raw_opt in &raw_opts {
                    let cleaned = raw_opt
                        .strip_prefix("a ")
                        .or_else(|| raw_opt.strip_prefix("an "))
                        .or_else(|| raw_opt.strip_prefix("one "))
                        .unwrap_or(raw_opt)
                        .trim();
                    let cleaned = cleaned
                        .strip_suffix(" counters")
                        .or_else(|| cleaned.strip_suffix(" counter"))
                        .unwrap_or(cleaned)
                        .trim();
                    if let Some(ck) = parse_counter_kind(cleaned) {
                        parsed_opts.push((cleaned.to_string(), ck));
                    } else {
                        all_valid = false;
                        break;
                    }
                }
                if all_valid {
                    let mut trial = builder.clone();
                    let branch_subject = match subject.as_ref() {
                        SubjectRefAst::TargetObject {
                            filter,
                            count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(bt)?;
                            if *tc == TargetCountAst::UpTo(1) {
                                trial.up_to_one_target = true;
                            }
                            SubjectRefAst::Anaphora("it".to_string())
                        }
                        SubjectRefAst::TargetOneOf {
                            options,
                            count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                        } => {
                            let bt = lower_target_one_of_permanent_body_target(
                                options, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(bt)?;
                            if *tc == TargetCountAst::UpTo(1) {
                                trial.up_to_one_target = true;
                            }
                            SubjectRefAst::Anaphora("it".to_string())
                        }
                        SubjectRefAst::TargetPlayer {
                            opponent_only,
                            count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                        } => {
                            trial.bind_target(if *opponent_only {
                                BodyTarget::SingleOpponent
                            } else {
                                BodyTarget::SinglePlayer
                            })?;
                            if *tc == TargetCountAst::UpTo(1) {
                                trial.up_to_one_target = true;
                            }
                            SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetPlayer) => {
                            trial.bind_target(BodyTarget::SinglePlayer)?;
                            SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                        }
                        SubjectRefAst::Player(PlayerRefAst::TargetOpponent) => {
                            trial.bind_target(BodyTarget::SingleOpponent)?;
                            SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                        }
                        other => other.clone(),
                    };
                    let mut branches = Vec::with_capacity(parsed_opts.len());
                    let mut branches_ok = true;
                    let prior_had_tokens = has_prior_token_creation(&trial.instructions);
                    let prior_had_moved = last_instruction_moved_object(&trial.instructions)
                        || has_prior_moved_object(&trial.instructions);
                    let anaphora_direct_subj =
                        if matches!(branch_subject, SubjectRefAst::Anaphora(_))
                            && !is_permanent_body_target(trial.target)
                        {
                            if prior_had_moved {
                                Some(EffectSubject::LastMoved)
                            } else if prior_had_tokens {
                                Some(EffectSubject::CreatedTokens)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                    for (opt_name, opt_ck) in &parsed_opts {
                        if let Some(ref direct_subj) = anaphora_direct_subj
                            && !opt_ck.is_player_counter()
                        {
                            let Ok(amt_expr) = lower_value_expr_in_context(
                                resolve_x(count, bound_x),
                                &trial,
                                in_trigger,
                                vocabulary,
                            ) else {
                                branches_ok = false;
                                break;
                            };
                            branches.push(ChooseOptionBranch {
                                label: format!("{opt_name} counter"),
                                body: ExecutableBody::new(
                                    BodyTarget::None,
                                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(*direct_subj, *opt_ck, amt_expr)],
                                ),
                            });
                            continue;
                        }
                        let mut sub = BodyBuilder::with_trigger_hint(trial.trigger_hint);
                        sub.target = trial.target;
                        sub.has_source = trial.has_source || in_trigger || self_pronoun_valid;
                        sub.last_group_filter = trial.last_group_filter;
                        if lower_instruction(
                            &InstructionAst::PutCounters {
                                subject: Box::new(branch_subject.clone()),
                                counter_kind: opt_name.clone(),
                                count: count.clone(),
                            },
                            &mut sub,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )
                        .is_err()
                            || sub.target != trial.target
                            || sub.instructions.is_empty()
                        {
                            branches_ok = false;
                            break;
                        }
                        branches.push(ChooseOptionBranch {
                            label: format!("{opt_name} counter"),
                            body: ExecutableBody::new(BodyTarget::None, sub.instructions),
                        });
                    }
                    if branches_ok && branches.len() >= 2 {
                        trial.instructions.push(Instruction::ChooseOption {
                            chooser: ResolvingPlayer::Controller,
                            options: branches,
                        });
                        *builder = trial;
                        return Ok(());
                    }
                }
            }
            if !kind_low_full.contains(" or ")
                && (kind_low_full.contains(" and ") || kind_low_full.contains(", "))
            {
                let mut raw_parts: Vec<&str> = Vec::new();
                for seg in kind_low_full.split(", ") {
                    let seg = seg.trim();
                    if seg.is_empty() {
                        continue;
                    }
                    if let Some(after_and) = seg.strip_prefix("and ") {
                        raw_parts.push(after_and.trim());
                    } else if let Some((lhs, rhs)) = seg.split_once(" and ") {
                        if !lhs.trim().is_empty() {
                            raw_parts.push(lhs.trim());
                        }
                        if !rhs.trim().is_empty() {
                            raw_parts.push(rhs.trim());
                        }
                    } else {
                        raw_parts.push(seg);
                    }
                }
                if raw_parts.len() >= 2 {
                    let mut parsed_items: Vec<(ValueExprAst, String)> = Vec::new();
                    let mut all_valid = true;
                    for (idx, part) in raw_parts.iter().enumerate() {
                        let (item_cnt, rest_after_cnt) = if idx == 0 {
                            if matches!(count.as_ref(), ValueExprAst::Const(1))
                                && let Some((tok, rest)) = part.split_once(' ')
                                && !matches!(tok, "a" | "an" | "one")
                                && let Some(n) = parse_small_number_word(tok)
                                && n > 0
                            {
                                (ValueExprAst::Const(i32::from(n)), rest.trim())
                            } else {
                                ((**count).clone(), *part)
                            }
                        } else if let Some(r) = part
                            .strip_prefix("a ")
                            .or_else(|| part.strip_prefix("an "))
                            .or_else(|| part.strip_prefix("one "))
                        {
                            (ValueExprAst::Const(1), r.trim())
                        } else if let Some(r) = part.strip_prefix("two ") {
                            (ValueExprAst::Const(2), r.trim())
                        } else if let Some(r) = part.strip_prefix("three ") {
                            (ValueExprAst::Const(3), r.trim())
                        } else if let Some((tok, rest)) = part.split_once(' ')
                            && let Some(n) = parse_small_number_word(tok)
                            && n > 0
                        {
                            (ValueExprAst::Const(i32::from(n)), rest.trim())
                        } else {
                            (ValueExprAst::Const(1), *part)
                        };
                        let mut k = rest_after_cnt;
                        while let Some(r) = k
                            .strip_prefix("an additional ")
                            .or_else(|| k.strip_prefix("one additional "))
                            .or_else(|| k.strip_prefix("additional "))
                            .or_else(|| k.strip_prefix("an extra "))
                            .or_else(|| k.strip_prefix("one extra "))
                            .or_else(|| k.strip_prefix("extra "))
                            .or_else(|| k.strip_prefix("more "))
                            .or_else(|| k.strip_prefix("another "))
                            .or_else(|| k.strip_prefix("a "))
                            .or_else(|| k.strip_prefix("an "))
                            .or_else(|| k.strip_prefix("one "))
                        {
                            k = r.trim_start();
                        }
                        k = k
                            .strip_suffix(" counters")
                            .or_else(|| k.strip_suffix(" counter"))
                            .unwrap_or(k)
                            .trim();
                        if parse_counter_kind(k).is_none() {
                            all_valid = false;
                            break;
                        }
                        parsed_items.push((item_cnt, k.to_string()));
                    }
                    if all_valid && parsed_items.len() >= 2 {
                        let follow_subject = match subject.as_ref() {
                            SubjectRefAst::TargetObject {
                                count: TargetCountAst::Exact(1) | TargetCountAst::UpTo(1),
                                ..
                            }
                            | SubjectRefAst::TargetOneOf {
                                count: TargetCountAst::Exact(1) | TargetCountAst::UpTo(1),
                                ..
                            } => Box::new(SubjectRefAst::Anaphora("it".to_string())),
                            SubjectRefAst::TargetPlayer {
                                count: TargetCountAst::Exact(1) | TargetCountAst::UpTo(1),
                                ..
                            }
                            | SubjectRefAst::Player(
                                PlayerRefAst::TargetPlayer | PlayerRefAst::TargetOpponent,
                            ) => Box::new(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
                            _ => subject.clone(),
                        };
                        let mut trial = builder.clone();
                        for (idx, (item_cnt, item_kind)) in parsed_items.into_iter().enumerate() {
                            let item_subj = if idx == 0 {
                                subject.clone()
                            } else {
                                follow_subject.clone()
                            };
                            lower_instruction(
                                &InstructionAst::PutCounters {
                                    subject: item_subj,
                                    counter_kind: item_kind,
                                    count: Box::new(item_cnt),
                                },
                                &mut trial,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                        }
                        *builder = trial;
                        return Ok(());
                    }
                }
            }
            let mut kind_clean = kind_low_full.as_str();
            while let Some(rest) = kind_clean
                .strip_prefix("a number of ")
                .or_else(|| kind_clean.strip_prefix("number of "))
                .or_else(|| kind_clean.strip_prefix("or more "))
                .or_else(|| kind_clean.strip_prefix("that many "))
                .or_else(|| kind_clean.strip_prefix("an additional "))
                .or_else(|| kind_clean.strip_prefix("additional "))
                .or_else(|| kind_clean.strip_prefix("one additional "))
                .or_else(|| kind_clean.strip_prefix("an extra "))
                .or_else(|| kind_clean.strip_prefix("one extra "))
                .or_else(|| kind_clean.strip_prefix("extra "))
                .or_else(|| kind_clean.strip_prefix("more "))
                .or_else(|| kind_clean.strip_prefix("another "))
                .or_else(|| kind_clean.strip_prefix("a "))
                .or_else(|| kind_clean.strip_prefix("an "))
                .or_else(|| kind_clean.strip_prefix("one "))
                .or_else(|| kind_clean.strip_prefix("the "))
            {
                kind_clean = rest.trim_start();
            }
            kind_clean = kind_clean
                .trim_end_matches('.')
                .trim()
                .strip_suffix(" counters")
                .or_else(|| {
                    kind_clean
                        .trim_end_matches('.')
                        .trim()
                        .strip_suffix(" counter")
                })
                .unwrap_or(kind_clean.trim_end_matches('.').trim())
                .trim();
            let ck = if matches!(
                kind_clean,
                "same" | "of that kind" | "that kind" | "that_kind" | "same kind"
            ) {
                infer_prior_counter_kind(&builder.instructions).ok_or("oracle:put-counter-kind")?
            } else {
                parse_counter_kind(kind_clean).ok_or("oracle:put-counter-kind")?
            };
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            fn is_put_counter_player_subject(subj: &SubjectRefAst) -> bool {
                match subj {
                    SubjectRefAst::Player(_) | SubjectRefAst::TargetPlayer { .. } => true,
                    SubjectRefAst::Anaphora(raw) => is_player_anaphora_label(raw),
                    SubjectRefAst::Compound(items) => {
                        !items.is_empty() && items.iter().all(is_put_counter_player_subject)
                    }
                    _ => false,
                }
            }
            if ck.is_player_counter()
                || (matches!(ck, CounterKind::Custom(_))
                    && is_put_counter_player_subject(subject.as_ref()))
            {
                if let SubjectRefAst::Compound(items) = subject.as_ref()
                    && !items.is_empty()
                {
                    let mut trial = builder.clone();
                    for item in items {
                        lower_instruction(
                            &InstructionAst::PutCounters {
                                subject: Box::new(item.clone()),
                                counter_kind: counter_kind.clone(),
                                count: count.clone(),
                            },
                            &mut trial,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                    }
                    *builder = trial;
                    return Ok(());
                }
                if let SubjectRefAst::Player(p) = subject.as_ref() {
                    let players = lower_affected_players_in_context(*p, builder, in_trigger)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::add_player_counter(
                            spellwake_engine::toolbox::PlayerSelector::Group(players),
                            ck,
                            amt,
                        ),
                    );
                    return Ok(());
                }
                if let SubjectRefAst::Anaphora(raw) = subject.as_ref()
                    && is_player_anaphora_label(raw)
                {
                    let low_p = raw.trim().to_ascii_lowercase();
                    let pref = if low_p.ends_with("controller") {
                        PlayerRefAst::ItsController
                    } else if low_p.ends_with("owner") {
                        PlayerRefAst::ItsOwner
                    } else {
                        PlayerRefAst::ThatPlayer
                    };
                    let players = lower_affected_players_in_context(pref, builder, in_trigger)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::add_player_counter(
                            spellwake_engine::toolbox::PlayerSelector::Group(players),
                            ck,
                            amt,
                        ),
                    );
                    return Ok(());
                }
                if let SubjectRefAst::TargetPlayer {
                    opponent_only,
                    count: tc @ (TargetCountAst::Exact(1) | TargetCountAst::UpTo(1)),
                } = subject.as_ref()
                {
                    builder.bind_target(if *opponent_only {
                        BodyTarget::SingleOpponent
                    } else {
                        BodyTarget::SinglePlayer
                    })?;
                    if *tc == TargetCountAst::UpTo(1) {
                        builder.up_to_one_target = true;
                    }
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::add_player_counter(
                            spellwake_engine::toolbox::PlayerSelector::Resolving(
                                ResolvingPlayer::TargetPlayer,
                            ),
                            ck,
                            amt,
                        ),
                    );
                    return Ok(());
                }
                return Err("oracle:put-counter-kind");
            }
            if let SubjectRefAst::InZone {
                subject: inner,
                zone: ZoneFilter::Battlefield,
            } = subject.as_ref()
            {
                return lower_instruction(
                    &InstructionAst::PutCounters {
                        subject: inner.clone(),
                        counter_kind: counter_kind.clone(),
                        count: count.clone(),
                    },
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                );
            }
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            if let SubjectRefAst::InZone {
                subject: inner,
                zone: ZoneFilter::Exile,
            } = subject.as_ref()
            {
                let mut trial = builder.clone();
                let query_opt = match inner.as_ref() {
                    s if has_source && is_self_pronoun(s, &trial, self_pronoun_valid) => {
                        Some(spellwake_engine::toolbox::ObjectQuery::Source)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(spellwake_engine::toolbox::ObjectQuery::TriggerSubject)
                    }
                    SubjectRefAst::Anaphora(raw)
                        if last_instruction_moved_object(&trial.instructions)
                            || (is_last_moved_card_anaphora(raw)
                                && has_prior_moved_object(&trial.instructions)) =>
                    {
                        Some(spellwake_engine::toolbox::ObjectQuery::LastMoved)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                        if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0 && has_source =>
                    {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                        f_clean.zone = None;
                        if is_unrestricted_filter(&f_clean) {
                            Some(spellwake_engine::toolbox::ObjectQuery::LinkedExile {
                                group: None,
                            })
                        } else {
                            let obj = lower_object_filter_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?;
                            bind_filter_player_target(&mut trial, &obj)?;
                            Some(spellwake_engine::toolbox::ObjectQuery::filtered(
                                spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None },
                                obj,
                            ))
                        }
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                        let mut f_clean = f.as_ref().clone();
                        f_clean.zone = None;
                        let obj = if is_unrestricted_filter(&f_clean) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?
                        };
                        bind_filter_player_target(&mut trial, &obj)?;
                        Some(spellwake_engine::toolbox::ObjectQuery::zone(
                            spellwake_engine::toolbox::QueryZone::Exile,
                            obj,
                        ))
                    }
                    _ => None,
                };
                if let Some(query) = query_opt {
                    trial.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                            query, ck, amt,
                        ),
                    );
                    *builder = trial;
                    return Ok(());
                }
            }
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                            ck, amt,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if has_source
                        && (is_explicit_self_permanent_noun(raw)
                            || (builder.target == BodyTarget::None
                                && matches!(
                                    raw.trim().to_ascii_lowercase().as_str(),
                                    "it" | "him" | "her"
                                )
                                && prior_op_acted_on_ability_source(&builder.instructions))) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                            ck, amt,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                            ck, amt,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(n),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(
                        *n,
                        *n,
                        super::super::toolbox_emit::put_counters_on_target(ck, amt),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(max),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *max > 0 => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(
                        0,
                        *max,
                        super::super::toolbox_emit::put_counters_on_target(ck, amt),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                    && builder.instructions.is_empty()
                    && !in_trigger =>
                {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    if *tc == TargetCountAst::UpToX {
                        builder.up_to_x_target_count = true;
                    } else {
                        builder.x_target_count = true;
                    }
                    builder.push_bounded_target_op(
                        0,
                        u16::MAX,
                        super::super::toolbox_emit::put_counters_on_target(ck, amt),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                            ck, amt,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(n),
                } if *n >= 2 => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(
                        *n,
                        *n,
                        super::super::toolbox_emit::put_counters_on_target(ck, amt),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::UpTo(max),
                } if *max > 0 => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(
                        0,
                        *max,
                        super::super::toolbox_emit::put_counters_on_target(ck, amt),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::AnyNumber,
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(
                        0,
                        u16::MAX,
                        super::super::toolbox_emit::put_counters_on_target(ck, amt),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
                } if !in_trigger => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    if *tc == TargetCountAst::UpToX {
                        builder.up_to_x_target_count = true;
                    } else {
                        builder.x_target_count = true;
                    }
                    builder.push_bounded_target_op(
                        0,
                        u16::MAX,
                        super::super::toolbox_emit::put_counters_on_target(ck, amt),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::AnyNumber,
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    builder.push_bounded_target_op(
                        0,
                        u16::MAX,
                        super::super::toolbox_emit::put_counters_on_target(ck, amt),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if last_instruction_created_tokens(&builder.instructions)
                        || (has_prior_token_creation(&builder.instructions)
                            && raw.to_ascii_lowercase().contains("token"))
                        || (has_prior_token_creation(&builder.instructions)
                            && !is_permanent_body_target(builder.target)
                            && !has_prior_moved_object(&builder.instructions)) =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::CreatedTokens, ck, amt));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&builder.instructions)
                        || matches!(
                            builder.instructions.last(),
                            Some(Instruction::MutatePlayer(spec))
                                if matches!(
                                    spec.op,
                                    spellwake_engine::toolbox::PlayerMutation::ManifestTop { .. }
                                        | spellwake_engine::toolbox::PlayerMutation::ManifestDread
                                )
                        )
                        || matches!(
                            builder.instructions.last(),
                            Some(Instruction::RepeatLoop { body, .. })
                                if body.instructions.iter().any(|op| matches!(
                                    op,
                                    Instruction::MutatePlayer(spec)
                                        if matches!(
                                            spec.op,
                                            spellwake_engine::toolbox::PlayerMutation::ManifestTop {
                                                ..
                                            } | spellwake_engine::toolbox::PlayerMutation::ManifestDread
                                        )
                                ))
                        ) =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::LastMoved, ck, amt));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if is_permanent_body_target(builder.target)
                        || matches!(
                            builder.target,
                            BodyTarget::SingleAnyTarget
                                | BodyTarget::SingleCreatureOrPlayer
                                | BodyTarget::SinglePlayerOrPlaneswalker
                                | BodyTarget::SingleOpponentOrPlaneswalker
                        ) =>
                {
                    builder
                        .instructions
                        .push(super::super::toolbox_emit::put_counters_on_target(ck, amt));
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )), ck, amt));
                    Ok(())
                }
                SubjectRefAst::TriggerSubject => {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::TriggerSubject, ck, amt));
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::TriggerSubject, ck, amt));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if try_append_fixed_first_target_counters(builder, ck, amt.clone()) =>
                {
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f)
                    if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                        && matches!(f.zone, None | Some(ZoneFilter::Exile))
                        && has_source =>
                {
                    let mut f_clean = f.as_ref().clone();
                    f_clean.flags.0 &= !AstFilterFlags::EXILED_WITH_SELF.0;
                    f_clean.zone = None;
                    let query = if is_unrestricted_filter(&f_clean) {
                        spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None }
                    } else {
                        let obj = lower_object_filter_in_context(
                            &f_clean, builder, in_trigger, vocabulary,
                        )?;
                        bind_filter_player_target(builder, &obj)?;
                        spellwake_engine::toolbox::ObjectQuery::filtered(
                            spellwake_engine::toolbox::ObjectQuery::LinkedExile { group: None },
                            obj,
                        )
                    };
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_query(
                            query, ck, amt,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut f_clean = f.as_ref().clone();
                    f_clean.zone = None;
                    let obj =
                        lower_object_filter_in_context(&f_clean, builder, in_trigger, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    // All matching permanent groups lower uniformly via MutateObject below.
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::Matching(obj), ck, amt));
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
                } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && !in_trigger => {
                    let mut f_clean = filter.as_ref().clone();
                    f_clean.zone = None;
                    let bt = lower_battlefield_body_target_in_context(
                        &f_clean, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    if *tc == TargetCountAst::UpToX {
                        builder.up_to_x_target_count = true;
                    } else {
                        builder.x_target_count = true;
                    }
                    builder.push_bounded_target_op(
                        0,
                        u16::MAX,
                        super::super::toolbox_emit::put_counters_on_target(ck, amt),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::Matching(group_obj), ck, amt));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if !is_permanent_body_target(builder.target)
                        && (has_prior_moved_object(&builder.instructions)
                            || matches!(
                                builder.instructions.last(),
                                Some(Instruction::MutateObject(_))
                            )) =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::LastMoved, ck, amt));
                    Ok(())
                }
                SubjectRefAst::TargetObject { filter, count }
                    if matches!(
                        filter.zone,
                        Some(
                            ZoneFilter::YourGraveyard
                                | ZoneFilter::OpponentGraveyard
                                | ZoneFilter::OpponentsGraveyards
                                | ZoneFilter::AnyGraveyard
                                | ZoneFilter::AllGraveyards
                                | ZoneFilter::SingleGraveyard
                        )
                    ) && (matches!(count, TargetCountAst::Exact(n) if *n >= 1)
                        || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                        || matches!(count, TargetCountAst::AnyNumber)
                        || (matches!(count, TargetCountAst::ChosenX | TargetCountAst::UpToX)
                            && !in_trigger)) =>
                {
                    let (bounds, is_x, is_up_to_x) = match *count {
                        TargetCountAst::Exact(1) => (None, false, false),
                        TargetCountAst::Exact(n) => {
                            (Some((usize::from(n), usize::from(n))), false, false)
                        }
                        TargetCountAst::UpTo(max) => (Some((0, usize::from(max))), false, false),
                        TargetCountAst::AnyNumber => (Some((0, usize::MAX)), false, false),
                        TargetCountAst::ChosenX => (Some((0, usize::MAX)), true, false),
                        TargetCountAst::UpToX => (Some((0, usize::MAX)), false, true),
                    };
                    let mut inner = filter.as_ref().clone();
                    inner.zone = None;
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    let bt = match filter.zone {
                        Some(ZoneFilter::YourGraveyard) => {
                            BodyTarget::FilteredOwnGraveyardCard(obj)
                        }
                        Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => {
                            BodyTarget::FilteredAnyGraveyardCard {
                                filter: obj,
                                opponents_only: true,
                            }
                        }
                        Some(
                            ZoneFilter::AnyGraveyard
                            | ZoneFilter::AllGraveyards
                            | ZoneFilter::SingleGraveyard,
                        ) => BodyTarget::FilteredAnyGraveyardCard {
                            filter: obj,
                            opponents_only: false,
                        },
                        _ => return Err("oracle:unsupported-put-counters"),
                    };
                    builder.bind_target(bt)?;
                    if let Some(b) = bounds {
                        builder.graveyard_target_bounds = Some(b);
                    }
                    if is_x {
                        builder.x_target_count = true;
                    }
                    if is_up_to_x {
                        builder.up_to_x_target_count = true;
                    }
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                            ck, amt,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if is_permanent_body_target(builder.target) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_target(
                            ck, amt,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if !is_permanent_body_target(builder.target)
                        && (last_instruction_created_tokens(&builder.instructions)
                            || (has_prior_token_creation(&builder.instructions)
                                && !has_prior_moved_object(&builder.instructions))) =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::CreatedTokens, ck, amt));
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::Matching(group_obj), ck, amt));
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if !is_permanent_body_target(builder.target)
                        && has_prior_moved_object(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::LastMoved, ck, amt));
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if in_trigger && builder.target == BodyTarget::None =>
                {
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_subject(EffectSubject::TriggerSubject, ck, amt));
                    Ok(())
                }
                SubjectRefAst::Compound(items) if !items.is_empty() => {
                    for it in items {
                        lower_instruction(
                            &InstructionAst::PutCounters {
                                subject: Box::new(it.clone()),
                                counter_kind: counter_kind.clone(),
                                count: count.clone(),
                            },
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                    }
                    Ok(())
                }
                _ => Err("oracle:unsupported-put-counters"),
            }
        }
        InstructionAst::Proliferate => {
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::proliferate(),
                )));
            Ok(())
        }
        InstructionAst::RemoveCounters {
            subject,
            counter_kind,
            all,
            count,
        } => {
            let ck_low = counter_kind.trim().to_ascii_lowercase();
            let mut ck_clean = ck_low.as_str();
            while let Some(rest) = ck_clean
                .strip_prefix("all ")
                .or_else(|| ck_clean.strip_prefix("any "))
                .or_else(|| ck_clean.strip_prefix("a "))
                .or_else(|| ck_clean.strip_prefix("an "))
                .or_else(|| ck_clean.strip_prefix("one "))
                .or_else(|| ck_clean.strip_prefix("the "))
            {
                ck_clean = rest.trim_start();
            }
            ck_clean = ck_clean
                .strip_suffix(" counters")
                .or_else(|| ck_clean.strip_suffix(" counter"))
                .unwrap_or(ck_clean)
                .trim();
            let kind = if matches!(
                ck_clean,
                "" | "any" | "a" | "counter" | "counters" | "all" | "any kind"
            ) {
                None
            } else if matches!(
                ck_clean,
                "those" | "them" | "all of them" | "of them" | "those counters"
            ) {
                let inferred = infer_prior_counter_kind(&builder.instructions)
                    .ok_or("oracle:unsupported-counter-kind")?;
                Some(inferred)
            } else {
                let ck = parse_counter_kind(ck_clean).ok_or("oracle:unsupported-counter-kind")?;
                if !ck.is_permanent() {
                    return Err("oracle:unsupported-counter-kind");
                }
                Some(ck)
            };
            let amt_opt = if *all {
                None
            } else if let Some(c) = count {
                Some(lower_value_expr_in_context(
                    resolve_x(c, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?)
            } else {
                Some(ValueExpr::literal(1))
            };
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    match (amt_opt, kind) {
                        (None, _) => {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::clear_source_counters(kind),
                            );
                        }
                        (Some(amt), _) => {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters(
                                    spellwake_engine::toolbox::ObjectQuery::Source,
                                    kind,
                                    amt,
                                ),
                            );
                        }
                    }
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target_in_context(
                        filter, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    match (amt_opt, kind) {
                        (None, _) => {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_all(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    spellwake_engine::toolbox::ObjectMutation::ClearCounters {
                                        kind,
                                    },
                                ),
                            );
                        }
                        (Some(amt), _) => {
                            builder.instructions.push(
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters(
                                    spellwake_engine::toolbox::ObjectQuery::Targets,
                                    kind,
                                    amt,
                                ),
                            );
                        }
                    }
                    Ok(())
                }
                SubjectRefAst::TargetObject { filter, count }
                    if filter.zone.is_none()
                        && (matches!(count, TargetCountAst::Exact(n) if *n >= 2)
                            || matches!(count, TargetCountAst::UpTo(max) if *max > 0)
                            || matches!(count, TargetCountAst::AnyNumber)
                            || (matches!(count, TargetCountAst::ChosenX) && !in_trigger)) =>
                {
                    let (min_t, max_t, is_x) = match *count {
                        TargetCountAst::Exact(n) => (n, n, false),
                        TargetCountAst::UpTo(max) => (0, max, false),
                        TargetCountAst::AnyNumber => (0, u16::MAX, false),
                        TargetCountAst::ChosenX => (0, u16::MAX, true),
                        _ => return Err("oracle:unsupported-remove-counters"),
                    };
                    let bt = lower_battlefield_body_target_in_context(
                        filter, builder, in_trigger, vocabulary,
                    )?;
                    builder.bind_target(bt)?;
                    if is_x {
                        builder.x_target_count = true;
                    }
                    builder.push_bounded_target_op(
                        min_t,
                        max_t,
                        super::super::toolbox_emit::remove_counters_from_targets(kind, amt_opt),
                    );
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                    builder
                        .instructions
                        .push(match amt_opt { Some(amt) => crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters_from_subject(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )), kind, amt), None => crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_subject(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )), spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind }) });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder
                        .instructions
                        .push(match amt_opt { Some(amt) => crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters_from_subject(EffectSubject::TriggerSubject, kind, amt), None => crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_subject(EffectSubject::TriggerSubject, spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind }) });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(match amt_opt { Some(amt) => crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters_from_subject(EffectSubject::CreatedTokens, kind, amt), None => crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_subject(EffectSubject::CreatedTokens, spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind }) });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                    builder
                        .instructions
                        .push(match amt_opt { Some(amt) => crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters_from_subject(EffectSubject::TargetObject, kind, amt), None => crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_subject(EffectSubject::TargetObject, spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind }) });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if !is_permanent_body_target(builder.target)
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(match amt_opt { Some(amt) => crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters_from_subject(EffectSubject::LastMoved, kind, amt), None => crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_subject(EffectSubject::LastMoved, spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind }) });
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    builder
                        .instructions
                        .push(match amt_opt { Some(amt) => crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters_from_subject(EffectSubject::TriggerSubject, kind, amt), None => crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_subject(EffectSubject::TriggerSubject, spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind }) });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    builder
                        .instructions
                        .push(match amt_opt { Some(amt) => crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters_from_subject(EffectSubject::Matching(group_obj), kind, amt), None => crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_subject(EffectSubject::Matching(group_obj), spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind }) });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut f_clean = f.as_ref().clone();
                    f_clean.zone = None;
                    let obj =
                        lower_object_filter_in_context(&f_clean, builder, in_trigger, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    builder
                        .instructions
                        .push(match amt_opt { Some(amt) => crate::keyword_creatures::oracle_lowering::toolbox_emit::remove_counters_from_subject(EffectSubject::Matching(obj), kind, amt), None => crate::keyword_creatures::oracle_lowering::toolbox_emit::mutate_subject(EffectSubject::Matching(obj), spellwake_engine::toolbox::ObjectMutation::ClearCounters { kind }) });
                    Ok(())
                }
                SubjectRefAst::Compound(items) if !items.is_empty() => {
                    for it in items {
                        lower_instruction(
                            &InstructionAst::RemoveCounters {
                                subject: Box::new(it.clone()),
                                counter_kind: counter_kind.clone(),
                                all: *all,
                                count: count.clone(),
                            },
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                    }
                    Ok(())
                }
                _ => Err("oracle:unsupported-remove-counters"),
            }
        }
        InstructionAst::Bolster { count } => {
            let ValueExpr::Literal(n) = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?
            else {
                return Err("oracle:dynamic-bolster");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:bolster-ovf")?)
                .map_err(|_| "oracle:bolster-ovf")?;
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::bolster(u),
                )));
            Ok(())
        }
        InstructionAst::Adapt { count } => {
            let ValueExpr::Literal(n) = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?
            else {
                return Err("oracle:dynamic-adapt");
            };
            let u = u16::try_from(n.to_usize().ok_or("oracle:adapt-ovf")?)
                .map_err(|_| "oracle:adapt-ovf")?;
            let skip_dst = builder.instructions.len() + 2;
            builder.instructions.push(Instruction::BranchCondition {
                condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Compare {
                    left: Box::new(ValueExpr::counters(
                        spellwake_engine::program::CounterSubjectRef::AbilitySource,
                        Some(CounterKind::PlusOne),
                    )),
                    op: Comparison::Equal,
                    right: Box::new(ValueExpr::literal(0)),
                }),
                destination: skip_dst,
            });
            builder
                .instructions
                .push(toolbox_emit::put_counters_on_source(
                    CounterKind::PlusOne,
                    ValueExpr::literal(i64::from(u)),
                ));
            Ok(())
        }
        InstructionAst::Explore { subject } => {
            if is_self_pronoun(subject, builder, self_pronoun_valid) {
                builder
                    .instructions
                    .push(Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::explore(
                            spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                MoveSubjectRef::AbilitySource,
                            ),
                        ),
                    )));
                return Ok(());
            }
            match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::explore(
                                spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                    MoveSubjectRef::TargetObject,
                                ),
                            ),
                        )));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    builder
                        .instructions
                        .push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::explore(
                                spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                    MoveSubjectRef::TargetObject,
                                ),
                            ),
                        )));
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder
                        .instructions
                        .push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::explore(
                                spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                    MoveSubjectRef::TriggeringObject,
                                ),
                            ),
                        )));
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                    builder
                        .instructions
                        .push(Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::explore(
                                spellwake_engine::toolbox::ObjectQuery::from_move_subject(
                                    MoveSubjectRef::AttachedHost,
                                ),
                            ),
                        )));
                    Ok(())
                }
                _ => Err("oracle:unsupported-explore"),
            }
        }
        InstructionAst::TurnFaceUp {
            player: PlayerRefAst::You,
            subject,
        } => lower_instruction(
            &InstructionAst::CombatRuleMod {
                subject: subject.clone(),
                rule: "turn_face_up".to_string(),
                duration: DurationAst::Permanent,
            },
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::TurnFaceDown { subject } => lower_instruction(
            &InstructionAst::CombatRuleMod {
                subject: subject.clone(),
                rule: "turn_face_down".to_string(),
                duration: DurationAst::Permanent,
            },
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::Empower { .. } => Err("oracle:unsupported-instruction"),
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } => {
            let kind_low_owned = counter_kind.trim().to_ascii_lowercase();
            let mut kind_clean = kind_low_owned.as_str();
            while let Some(rest) = kind_clean
                .strip_prefix("an additional ")
                .or_else(|| kind_clean.strip_prefix("additional "))
                .or_else(|| kind_clean.strip_prefix("another "))
                .or_else(|| kind_clean.strip_prefix("a "))
                .or_else(|| kind_clean.strip_prefix("an "))
                .or_else(|| kind_clean.strip_prefix("one "))
            {
                kind_clean = rest.trim_start();
            }
            kind_clean = kind_clean
                .trim_end_matches('.')
                .trim()
                .strip_suffix(" counters")
                .or_else(|| {
                    kind_clean
                        .trim_end_matches('.')
                        .trim()
                        .strip_suffix(" counter")
                })
                .unwrap_or(kind_clean.trim_end_matches('.').trim())
                .trim();
            let ck = parse_counter_kind(kind_clean).ok_or("oracle:player-counter-kind")?;
            if !(ck.is_player_counter() || matches!(ck, CounterKind::Custom(_))) {
                return Err("oracle:player-counter-kind");
            }
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::add_player_counter(
                    spellwake_engine::toolbox::PlayerSelector::Group(players),
                    ck,
                    amt,
                ),
            );
            Ok(())
        }
        InstructionAst::Blight { player, count } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::blight(
                            ResolvingPlayer::LoopPlayer,
                            amt,
                        ),
                    ))],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder
                .instructions
                .push(Instruction::MutatePlayer(Box::new(
                    spellwake_engine::toolbox::MutatePlayerSpec::blight(rp, amt),
                )));
            Ok(())
        }
        InstructionAst::Connive { subject, count } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let subj = lower_move_subject_ref(
                subject,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            builder
                .instructions
                .push(Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::connive(
                        spellwake_engine::toolbox::ObjectQuery::from_move_subject(subj),
                        amt,
                    ),
                )));
            Ok(())
        }
        InstructionAst::Support { count } => {
            let ValueExprAst::Const(n) = resolve_x(count, bound_x) else {
                return Err("oracle:non-const-support");
            };
            let max = u16::try_from(*n).map_err(|_| "oracle:non-const-support")?;
            if max == 0 {
                return Err("oracle:non-const-support");
            }
            if self_pronoun_valid {
                builder.bind_target(BodyTarget::FilteredPermanent(ObjectFilter::Clause(
                    ObjectClause {
                        source_relation: SourceRelation::OtherThanAbilitySource,
                        ..ObjectClause::creature()
                    },
                )))?;
            } else {
                builder.bind_target(BodyTarget::SingleCreature)?;
            }
            builder.push_bounded_target_op(
                0,
                max,
                super::super::toolbox_emit::put_counters_on_target(
                    CounterKind::PlusOne,
                    ValueExpr::literal(1),
                ),
            );
            Ok(())
        }
        InstructionAst::Endure { count } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let counter_op = if matches!(&amt, ValueExpr::Literal(_)) || self_pronoun_valid {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                    CounterKind::PlusOne,
                    amt.clone(),
                )
            } else {
                return Err("oracle:non-literal-endure");
            };
            let spirit = vocabulary
                .resolve_label("Spirit")
                .ok_or("oracle:token-unknown-subtype")?;
            let mut spirit_prog =
                Program::creature(None, 0.into(), 0.into(), BTreeSet::from([spirit]));
            spirit_prog.effect_defined_colors = Some(BTreeSet::from([Color::White]));
            let spirit_def = TokenDefinition::new(
                spirit_prog,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Creature(spirit)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:token-def-invalid")?;
            let counter_branch = ChooseOptionBranch {
                label: "Put +1/+1 counters on this creature".to_owned(),
                body: ExecutableBody::new(BodyTarget::None, vec![counter_op]),
            };
            let token_branch = ChooseOptionBranch {
                label: "Create a white Spirit creature token".to_owned(),
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::CreateTokens {
                        creations: vec![TokenCreation {
                            enters_tapped: false,
                            definition: spirit_def,
                            amount: ValueExpr::literal(1),
                            base_size: Some(TokenSize::AtCreation(amt)),
                            recipients: TokenRecipients::Controller,
                            attacking_defender: None,
                        }],
                    }],
                ),
            };
            builder.instructions.push(Instruction::ChooseOption {
                chooser: ResolvingPlayer::Controller,
                options: vec![counter_branch, token_branch],
            });
            Ok(())
        }
        InstructionAst::DoubleStatOrCounters {
            subject,
            kind,
            duration,
        } => {
            if let SubjectRefAst::Compound(items) = subject.as_ref()
                && !items.is_empty()
            {
                for it in items {
                    lower_instruction(
                        &InstructionAst::DoubleStatOrCounters {
                            subject: Box::new(it.clone()),
                            kind: kind.clone(),
                            duration: duration.clone(),
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                return Ok(());
            }
            let low = kind.trim().to_ascii_lowercase();
            if matches!(low.as_str(), "life" | "life total" | "life_total") {
                let rp = extract_resolving_player_from_subject(subject, builder, in_trigger)
                    .map_err(|_| "oracle:unsupported-double-stat")?;
                let amount = if rp == ResolvingPlayer::Controller {
                    ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife)
                } else {
                    ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(rp))
                };
                builder.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::gain_life(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(rp),
                        amount,
                    ),
                );
                return Ok(());
            }
            let counter_double_kind: Option<Option<CounterKind>> = if matches!(
                low.as_str(),
                "counters" | "all_counters" | "each kind of counter" | "each counter"
            ) {
                Some(None)
            } else if let Some(rest) = low.strip_prefix("counters:") {
                let r = rest.trim();
                let r = r
                    .strip_suffix(" counters")
                    .or_else(|| r.strip_suffix(" counter"))
                    .unwrap_or(r)
                    .trim();
                if matches!(r, "" | "all" | "each" | "each kind" | "each kind of") {
                    Some(None)
                } else {
                    parse_counter_kind(r)
                        .filter(|ck| !ck.is_player_counter())
                        .map(Some)
                }
            } else if let Some(ck_raw) = low
                .strip_suffix(" counters")
                .or_else(|| low.strip_suffix(" counter"))
            {
                parse_counter_kind(ck_raw.trim())
                    .filter(|ck| !ck.is_player_counter())
                    .map(Some)
            } else {
                None
            };
            if let Some(ck_filter) = counter_double_kind {
                if ck_filter.is_none()
                    && let SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } = subject.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::double_counters_on_query(spellwake_engine::toolbox::ObjectQuery::Targets, None));
                    return Ok(());
                }
                if ck_filter.is_none()
                    && builder.instructions.is_empty()
                    && let SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::AnyNumber,
                    } = subject.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::double_counters_on_query(spellwake_engine::toolbox::ObjectQuery::Targets, None));
                    return Ok(());
                }
                if ck_filter.is_none()
                    && builder.instructions.is_empty()
                    && !in_trigger
                    && let SubjectRefAst::TargetObject {
                        filter,
                        count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
                    } = subject.as_ref()
                    && filter.zone.is_none()
                {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    if *tc == TargetCountAst::UpToX {
                        builder.up_to_x_target_count = true;
                    } else {
                        builder.x_target_count = true;
                    }
                    builder
                        .instructions
                        .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::double_counters_on_query(spellwake_engine::toolbox::ObjectQuery::Targets, None));
                    return Ok(());
                }
                let eff_subj = match subject.as_ref() {
                    s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                        EffectSubject::AbilitySource
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if filter.zone.is_none() => {
                        let bt = lower_battlefield_body_target(filter, vocabulary)?;
                        builder.bind_target(bt)?;
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&builder.instructions) =>
                    {
                        EffectSubject::CreatedTokens
                    }
                    SubjectRefAst::Anaphora(_)
                        if matches!(
                            builder.target,
                            BodyTarget::SinglePermanent
                                | BodyTarget::SingleCreature
                                | BodyTarget::SinglePlaneswalker
                                | BodyTarget::FilteredPermanent(_)
                        ) =>
                    {
                        EffectSubject::TargetObject
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&builder.instructions) =>
                    {
                        EffectSubject::LastMoved
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => EffectSubject::TriggerSubject,
                    SubjectRefAst::Anaphora(raw)
                        if in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                            && builder.target == BodyTarget::None =>
                    {
                        EffectSubject::TriggerSubject
                    }
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                        EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        ))
                    }
                    SubjectRefAst::Anaphora(_)
                        if builder.target == BodyTarget::None
                            && let Some(group_obj) = builder.last_group_filter =>
                    {
                        EffectSubject::Matching(group_obj)
                    }
                    SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                        if f.zone.is_none() =>
                    {
                        let obj = lower_object_filter(f, vocabulary)?;
                        builder.last_group_filter = Some(obj);
                        EffectSubject::Matching(obj)
                    }
                    _ => return Err("oracle:unsupported-double-stat"),
                };
                builder
                    .instructions
                    .push(crate::keyword_creatures::oracle_lowering::toolbox_emit::double_counters_on_query(spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(eff_subj)), ck_filter));
                return Ok(());
            }
            let (power, toughness) = match low.as_str() {
                "power" => (true, false),
                "toughness" => (false, true),
                "power and toughness" | "power_and_toughness" => (true, true),
                _ => return Err("oracle:unsupported-double-stat"),
            };
            let dur = lower_duration(duration)?;
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::AbilitySource,
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::TargetObject,
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::CreatedTokens,
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePermanent
                            | BodyTarget::SingleCreature
                            | BodyTarget::SinglePlaneswalker
                            | BodyTarget::FilteredPermanent(_)
                    ) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::TargetObject,
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&builder.instructions)
                        || matches!(
                            builder.instructions.last(),
                            Some(Instruction::MutatePlayer(spec))
                                if matches!(
                                    spec.op,
                                    spellwake_engine::toolbox::PlayerMutation::ManifestTop { .. }
                                )
                        ) =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::LastMoved,
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::TriggerSubject,
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::TriggerSubject,
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            )),
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::Matching(group_obj),
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    builder.last_group_filter = Some(obj);
                    builder.instructions.push(
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::double_pt_subject(
                            EffectSubject::Matching(obj),
                            power,
                            toughness,
                            dur,
                        ),
                    );
                    Ok(())
                }
                _ => Err("oracle:unsupported-double-stat"),
            }
        }
        InstructionAst::RemoveKeywords {
            subject,
            keywords,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let suppressed = lower_keyword_list(keywords)?;
            if suppressed.is_empty() {
                return Err("oracle:empty-remove-keywords");
            }
            let ab_change = AbilityChange {
                suppressed_keywords: suppressed,
                ..AbilityChange::default()
            };
            let mut trial = builder.clone();
            emit_resolved_parts_for_subject(
                subject,
                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                dur,
                &mut trial,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )
            .map_err(|err| {
                if err == "oracle:unsupported-temp-static" {
                    "oracle:unsupported-remove-keywords"
                } else {
                    err
                }
            })?;
            *builder = trial;
            Ok(())
        }
        InstructionAst::AnimateOrSetBasePT {
            subject,
            power,
            toughness,
            description,
            keywords,
            duration,
        } => lower_animate_or_set_pt_extended(
            subject,
            power,
            toughness,
            description.as_deref(),
            keywords,
            duration,
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::BecomeChosenCharacteristic {
            subject,
            kind,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let kind_low = kind.trim().to_ascii_lowercase();
            let add = kind_low.contains("in addition");
            let mut trial = builder.clone();
            let part = if kind_low.contains("creature type") {
                let slot = ids.creature_type_slot();
                trial
                    .instructions
                    .push(super::super::toolbox_emit::choose_creature_type(
                        slot,
                        BTreeSet::new(),
                    ));
                ResolvedPartSpec::Type(TypeChangeSpec {
                    land_subtypes: SubtypeOperation::Keep,
                    supertypes: TypeOperation::Keep,
                    types: TypeOperation::Keep,
                    subtypes: if add {
                        SubtypeOperation::Add(InstructionSubtypeOperand::Chosen(slot))
                    } else {
                        SubtypeOperation::Set(InstructionSubtypeOperand::Chosen(slot))
                    },
                })
            } else if kind_low.contains("basic land type") || kind_low.contains("basic_land_type") {
                trial.instructions.push(Instruction::ChooseValue {
                    chooser: ResolvingPlayer::Controller,
                    domain: ResolutionChoiceDomain::BasicLandType,
                });
                ResolvedPartSpec::ChosenBasicLandType { add }
            } else if kind_low == "color"
                || kind_low == "a color"
                || kind_low == "the color"
                || (kind_low.contains("color")
                    && !kind_low.contains("colors")
                    && !kind_low.contains("colorless"))
            {
                trial.instructions.push(Instruction::ChooseValue {
                    chooser: ResolvingPlayer::Controller,
                    domain: ResolutionChoiceDomain::Color,
                });
                ResolvedPartSpec::ChosenColor { add }
            } else {
                return Err("oracle:unsupported-become-chosen");
            };
            emit_resolved_parts_for_subject(
                subject,
                ResolvedPartsSpec(vec![part]),
                dur,
                &mut trial,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )
            .map_err(|_| "oracle:unsupported-become-chosen")?;
            *builder = trial;
            Ok(())
        }
        InstructionAst::KeepExistingTypes(_) => {
            let Some(last) = builder.instructions.last_mut() else {
                return Err("oracle:unsupported-keep-existing-types");
            };
            let parts_mut = match last {
                Instruction::ApplyContinuous(spec) => match &mut spec.payload {
                    spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(parts)
                    | spellwake_engine::toolbox::ContinuousPayload::ModifyAnyNumber { parts } => {
                        Some(&mut parts.0)
                    }
                    _ => None,
                },
                _ => None,
            };
            let Some(parts) = parts_mut else {
                return Err("oracle:unsupported-keep-existing-types");
            };
            let mut updated = false;
            for part in parts.iter_mut() {
                match part {
                    ResolvedPartSpec::ChosenBasicLandType { add }
                    | ResolvedPartSpec::ChosenColor { add } => {
                        *add = true;
                        updated = true;
                    }
                    ResolvedPartSpec::Type(tc) => {
                        if let SubtypeOperation::Set(op) = tc.subtypes.clone() {
                            tc.subtypes = SubtypeOperation::Add(op);
                            updated = true;
                        }
                    }
                    _ => {}
                }
            }
            if updated {
                Ok(())
            } else {
                Err("oracle:unsupported-keep-existing-types")
            }
        }
        InstructionAst::DistributeCounters {
            counter_kind,
            count,
            targets,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:distribute-counter-kind")?;
            if ck.is_player_counter() {
                return Err("oracle:distribute-counter-kind");
            }
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let ValueExpr::Literal(n) = &amt else {
                return Err("oracle:distribute-non-const");
            };
            let Some(total) = n
                .to_usize()
                .and_then(|v| u16::try_from(v).ok())
                .filter(|&v| v > 0)
            else {
                return Err("oracle:distribute-non-const");
            };
            let filter = match targets.as_ref() {
                SubjectRefAst::TargetObject { filter, .. } if filter.zone.is_none() => filter,
                _ => return Err("oracle:distribute-unsupported-target"),
            };
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::distribute_counters(ck, total),
                )));
            Ok(())
        }
        InstructionAst::Monstrosity { count } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            builder
                .instructions
                .push(Instruction::MutateObject(Box::new(
                    spellwake_engine::toolbox::MutateObjectSpec::monstrosity(amt),
                )));
            Ok(())
        }
        InstructionAst::GrantKeywordChoice {
            subject,
            choices,
            duration,
        } if choices.len() >= 2 => {
            let mut trial = builder.clone();
            let branch_subject = match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target_in_context(
                        filter, &trial, in_trigger, vocabulary,
                    )?;
                    trial.bind_target(bt)?;
                    SubjectRefAst::Anaphora("it".to_owned())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, &trial, in_trigger, vocabulary,
                    )?;
                    trial.bind_target(bt)?;
                    SubjectRefAst::Anaphora("it".to_owned())
                }
                other => other.clone(),
            };
            let mut options = Vec::with_capacity(choices.len());
            for choice in choices {
                let mut sub = BodyBuilder::with_trigger_hint(trial.trigger_hint);
                sub.target = trial.target;
                sub.has_source = trial.has_source || in_trigger || self_pronoun_valid;
                sub.last_group_filter = trial.last_group_filter;
                lower_instruction(
                    &InstructionAst::GrantKeywords {
                        subject: Box::new(branch_subject.clone()),
                        keywords: vec![choice.clone()],
                        duration: duration.clone(),
                    },
                    &mut sub,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                if sub.target != trial.target || sub.instructions.is_empty() {
                    return Err("oracle:unsupported-grant-kw-choice");
                }
                options.push(ChooseOptionBranch {
                    label: choice.trim().to_owned(),
                    body: ExecutableBody::new(BodyTarget::None, sub.instructions),
                });
            }
            trial.instructions.push(Instruction::ChooseOption {
                chooser: ResolvingPlayer::Controller,
                options,
            });
            *builder = trial;
            Ok(())
        }
        InstructionAst::ModifyPTChoice {
            subject,
            choices,
            duration,
        } if choices.len() >= 2 => {
            let mut trial = builder.clone();
            let branch_subject = match subject.as_ref() {
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target_in_context(
                        filter, &trial, in_trigger, vocabulary,
                    )?;
                    trial.bind_target(bt)?;
                    SubjectRefAst::Anaphora("it".to_owned())
                }
                SubjectRefAst::TargetOneOf {
                    options,
                    count: TargetCountAst::Exact(1),
                } => {
                    let bt = lower_target_one_of_permanent_body_target(
                        options, &trial, in_trigger, vocabulary,
                    )?;
                    trial.bind_target(bt)?;
                    SubjectRefAst::Anaphora("it".to_owned())
                }
                other => other.clone(),
            };
            let mut options = Vec::with_capacity(choices.len());
            for (dx, dy) in choices {
                let mut sub = BodyBuilder::with_trigger_hint(trial.trigger_hint);
                sub.target = trial.target;
                sub.has_source = trial.has_source || in_trigger || self_pronoun_valid;
                sub.last_group_filter = trial.last_group_filter;
                lower_instruction(
                    &InstructionAst::ModifyPT {
                        subject: Box::new(branch_subject.clone()),
                        dx: dx.clone(),
                        dy: dy.clone(),
                        duration: duration.clone(),
                        scale: None,
                    },
                    &mut sub,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                if sub.target != trial.target || sub.instructions.is_empty() {
                    return Err("oracle:unsupported-modify-pt-choice");
                }
                options.push(ChooseOptionBranch {
                    label: format!("{dx}/{dy}"),
                    body: ExecutableBody::new(BodyTarget::None, sub.instructions),
                });
            }
            trial.instructions.push(Instruction::ChooseOption {
                chooser: ResolvingPlayer::Controller,
                options,
            });
            *builder = trial;
            Ok(())
        }
        InstructionAst::MoveCounter {
            counter_kind,
            from,
            to,
        } => lower_instruction(
            &InstructionAst::MoveCounters {
                from: from.clone(),
                to: to.clone(),
                counter_kind: counter_kind.clone().unwrap_or_default(),
                all: false,
                any_number: false,
                count: Some(Box::new(ValueExprAst::Const(1))),
            },
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        ),
        InstructionAst::MoveCounters {
            from,
            to,
            counter_kind,
            all,
            any_number,
            count,
        } => {
            let kind_clean = counter_kind
                .trim()
                .trim_end_matches(" counters")
                .trim_end_matches(" counter")
                .trim()
                .to_ascii_lowercase();
            let count_is_default_one = count
                .as_deref()
                .is_none_or(|c| matches!(c, ValueExprAst::Const(1)));
            let effective_all = *all
                || (!*any_number
                    && count_is_default_one
                    && matches!(kind_clean.as_str(), "all" | "all of the" | "all the"));
            let effective_any_number = *any_number
                || (!effective_all
                    && count_is_default_one
                    && matches!(kind_clean.as_str(), "any number of" | "any number"));
            let kind = if kind_clean.is_empty()
                || matches!(
                    kind_clean.as_str(),
                    "any"
                        | "all"
                        | "all of the"
                        | "all the"
                        | "a"
                        | "an"
                        | "one"
                        | "one or more"
                        | "any kind"
                        | "of any kind"
                        | "any number of"
                        | "any number"
                ) {
                None
            } else {
                let ck = parse_counter_kind(&kind_clean)
                    .filter(|k| k.is_permanent())
                    .ok_or("oracle:unsupported-move-counters")?;
                Some(ck)
            };
            let unwrap_bf = |s: &SubjectRefAst| -> SubjectRefAst {
                if let SubjectRefAst::InZone {
                    subject,
                    zone: ZoneFilter::Battlefield,
                } = s
                {
                    (**subject).clone()
                } else {
                    s.clone()
                }
            };
            let eff_from = unwrap_bf(from);
            let eff_to = unwrap_bf(to);
            let parse_bf_target = |s: &SubjectRefAst,
                                   b: &BodyBuilder,
                                   allow_first_target_controller: bool|
             -> Option<Result<BodyTarget, &'static str>> {
                match s {
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let mut same_ctrl_as_first = false;
                        if allow_first_target_controller
                                && f_clean.controller.is_none()
                                && f_clean.predicates.iter().any(|p| {
                                    matches!(
                                        p,
                                        spellwake_oracle_compiler::ast::FilterPredicateAst::WithSameControllerAs(sub)
                                            if matches!(sub.as_ref(), SubjectRefAst::Anaphora(_))
                                    )
                                })
                            {
                                f_clean.predicates.retain(|p| {
                                    !matches!(
                                        p,
                                        spellwake_oracle_compiler::ast::FilterPredicateAst::WithSameControllerAs(sub)
                                            if matches!(sub.as_ref(), SubjectRefAst::Anaphora(_))
                                    )
                                });
                                same_ctrl_as_first = true;
                            }
                        if same_ctrl_as_first {
                            let res =
                                lower_object_filter_in_context(&f_clean, b, in_trigger, vocabulary)
                                    .map(|obj| {
                                        let with_ctrl = match obj {
                                            ObjectFilter::Clause(mut c) => {
                                                c.controller =
                                                    ControllerFilter::FirstTargetController;
                                                ObjectFilter::Clause(c)
                                            }
                                            ObjectFilter::Either(mut a, mut b) => {
                                                a.controller =
                                                    ControllerFilter::FirstTargetController;
                                                b.controller =
                                                    ControllerFilter::FirstTargetController;
                                                ObjectFilter::Either(a, b)
                                            }
                                        };
                                        BodyTarget::FilteredPermanent(with_ctrl)
                                    });
                            Some(res)
                        } else {
                            Some(lower_battlefield_body_target_in_context(
                                &f_clean, b, in_trigger, vocabulary,
                            ))
                        }
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::Exact(1),
                    } => Some(lower_target_one_of_permanent_body_target(
                        options, b, in_trigger, vocabulary,
                    )),
                    _ => None,
                }
            };
            let mut trial = builder.clone();
            let amount = if effective_all || effective_any_number {
                None
            } else {
                let default_one = ValueExprAst::Const(1);
                let count_ast = count.as_deref().unwrap_or(&default_one);
                Some(lower_value_expr_in_context(
                    resolve_x(count_ast, bound_x),
                    &trial,
                    in_trigger,
                    vocabulary,
                )?)
            };
            if trial.target == BodyTarget::None
                && let (Some(from_res), Some(to_res)) = (
                    parse_bf_target(&eff_from, &trial, false),
                    parse_bf_target(&eff_to, &trial, true),
                )
            {
                let from_bt = from_res?;
                let to_bt = to_res?;
                if !is_permanent_body_target(from_bt) || !is_permanent_body_target(to_bt) {
                    return Err("oracle:unsupported-move-counters");
                }
                trial
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(
                            from_bt,
                            vec![Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(
                            ),
                        ))],
                        ),
                    )));
                trial
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(
                            to_bt,
                            vec![Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::remember_permanent_target(
                            ),
                        ))],
                        ),
                    )));
                trial.instructions.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::move_counters(
                        spellwake_engine::toolbox::ObjectQuery::LinkedTarget { index: 0 },
                        spellwake_engine::toolbox::ObjectQuery::LinkedTarget { index: 1 },
                        kind,
                        if effective_all { None } else { amount },
                    ),
                );
                *builder = trial;
                return Ok(());
            }
            let has_source = in_trigger || self_pronoun_valid;
            let resolve_side = |s: &SubjectRefAst,
                                b: &mut BodyBuilder|
             -> Result<MoveSubjectRef, &'static str> {
                if has_source && is_self_pronoun(s, b, self_pronoun_valid) {
                    return Ok(MoveSubjectRef::AbilitySource);
                }
                match s {
                    SubjectRefAst::SelfObject if has_source => Ok(MoveSubjectRef::AbilitySource),
                    SubjectRefAst::AttachedHost(_) if has_source => {
                        Ok(MoveSubjectRef::AttachedHost)
                    }
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Ok(MoveSubjectRef::TriggeringObject)
                    }
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                        let mut f_clean = filter.as_ref().clone();
                        f_clean.zone = None;
                        let bt = lower_battlefield_body_target_in_context(
                            &f_clean, b, in_trigger, vocabulary,
                        )?;
                        if !is_permanent_body_target(bt) {
                            return Err("oracle:unsupported-move-counters");
                        }
                        b.bind_target(bt)?;
                        Ok(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::TargetOneOf {
                        options,
                        count: TargetCountAst::Exact(1),
                    } => {
                        let bt = lower_target_one_of_permanent_body_target(
                            options, b, in_trigger, vocabulary,
                        )?;
                        if !is_permanent_body_target(bt) {
                            return Err("oracle:unsupported-move-counters");
                        }
                        b.bind_target(bt)?;
                        Ok(MoveSubjectRef::TargetObject)
                    }
                    SubjectRefAst::Anaphora(raw) => {
                        if is_permanent_body_target(b.target) {
                            Ok(MoveSubjectRef::TargetObject)
                        } else if last_instruction_created_tokens(&b.instructions) {
                            Ok(MoveSubjectRef::CreatedTokens)
                        } else if last_instruction_moved_object(&b.instructions) {
                            Ok(MoveSubjectRef::LastMoved)
                        } else if in_trigger
                            && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        {
                            Ok(MoveSubjectRef::TriggeringObject)
                        } else {
                            Err("oracle:unsupported-move-counters")
                        }
                    }
                    _ => Err("oracle:unsupported-move-counters"),
                }
            };
            let from_ref = resolve_side(&eff_from, &mut trial)?;
            let to_ref = resolve_side(&eff_to, &mut trial)?;
            trial.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::move_counters(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject_to_query(
                        from_ref,
                    ),
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::move_subject_to_query(
                        to_ref,
                    ),
                    kind,
                    if effective_all { None } else { amount },
                ),
            );
            *builder = trial;
            Ok(())
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

pub(super) fn parse_scaled_pt_delta_term(raw: &str) -> Option<ScaledPtDeltaTerm> {
    let cleaned = raw.trim().replace('\u{2212}', "-");
    let low = cleaned.to_ascii_lowercase();
    if let Some(n) = parse_signed_pt_delta(&low) {
        return Some(ScaledPtDeltaTerm::Literal(n));
    }
    let body = low.strip_suffix('x')?.trim();
    match body {
        "" | "+" => Some(ScaledPtDeltaTerm::ScaledX(1)),
        "-" => Some(ScaledPtDeltaTerm::ScaledX(-1)),
        other => {
            let coeff = parse_signed_pt_delta(other)?;
            Some(ScaledPtDeltaTerm::ScaledX(coeff))
        }
    }
}

pub(super) fn emit_resolved_parts_for_subject(
    subject: &SubjectRefAst,
    parts: ResolvedPartsSpec,
    dur: ResolvedDuration,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    match subject {
        SubjectRefAst::InZone {
            subject: inner,
            zone: ZoneFilter::Battlefield,
        } => emit_resolved_parts_for_subject(
            inner,
            parts,
            dur,
            builder,
            vocabulary,
            in_trigger,
            self_pronoun_valid,
        ),
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            for it in items {
                emit_resolved_parts_for_subject(
                    it,
                    parts.clone(),
                    dur,
                    builder,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )?;
            }
            Ok(())
        }
        s if (self_pronoun_valid || in_trigger)
            && is_self_pronoun(s, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Source,
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
            let mut f_clean = filter.as_ref().clone();
            f_clean.zone = None;
            let bt = lower_battlefield_body_target_in_context(
                &f_clean, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::Exact(1),
        } => {
            let bt = lower_target_one_of_permanent_body_target(
                options, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::UpTo(1),
        } => {
            let bt = lower_target_one_of_permanent_body_target(
                options, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder.push_bounded_target_op(
                0,
                1,
                super::super::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(n),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
            let mut f_clean = filter.as_ref().clone();
            f_clean.zone = None;
            let bt = lower_battlefield_body_target_in_context(
                &f_clean, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder.push_bounded_target_op(
                *n,
                *n,
                super::super::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::UpTo(max),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *max > 0 => {
            let mut f_clean = filter.as_ref().clone();
            f_clean.zone = None;
            let bt = lower_battlefield_body_target_in_context(
                &f_clean, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            builder.push_bounded_target_op(
                0,
                *max,
                super::super::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && !in_trigger => {
            let mut f_clean = filter.as_ref().clone();
            f_clean.zone = None;
            let bt = lower_battlefield_body_target_in_context(
                &f_clean, builder, in_trigger, vocabulary,
            )?;
            builder.bind_target(bt)?;
            if *tc == TargetCountAst::UpToX {
                builder.up_to_x_target_count = true;
            } else {
                builder.x_target_count = true;
            }
            builder.push_bounded_target_op(
                0,
                u16::MAX,
                super::super::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::AnyNumber,
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
            let mut f_clean = filter.as_ref().clone();
            f_clean.zone = None;
            let obj = lower_object_filter_in_context(&f_clean, builder, in_trigger, vocabulary)?;
            builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
            builder
                .instructions
                .push(Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::modify_any_number(parts, dur),
                )));
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::Matching(ObjectFilter::Clause(
                            ObjectClause::any().attached_to_self(),
                        )),
                    )),
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
            builder
                .instructions
                .push(super::super::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    parts,
                    dur,
                ));
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if last_instruction_created_tokens(&builder.instructions) => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::CreatedTokens,
                    )),
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if last_instruction_moved_object(&builder.instructions) => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::LastMoved,
                    )),
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None
                && let Some(group_obj) = builder.last_group_filter =>
        {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::Matching(group_obj),
                    )),
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if !is_permanent_body_target(builder.target)
                && has_prior_moved_object(&builder.instructions) =>
        {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::LastMoved,
                    )),
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::TriggerSubject,
                    )),
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::Anaphora(raw)
            if builder.target == BodyTarget::None
                && in_trigger
                && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
        {
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::TriggerSubject,
                    )),
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
        {
            let mut f_clean = f.as_ref().clone();
            f_clean.zone = None;
            let obj = lower_object_filter_in_context(&f_clean, builder, in_trigger, vocabulary)?;
            bind_filter_player_target(builder, &obj)?;
            ensure_target_power_consumed(builder, &obj);
            builder.last_group_filter = Some(obj);
            builder.instructions.push(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                    spellwake_engine::toolbox::ObjectQuery::EffectSubject(Box::new(
                        EffectSubject::Matching(obj),
                    )),
                    parts,
                    dur,
                ),
            );
            Ok(())
        }
        _ => Err("oracle:unsupported-temp-static"),
    }
}

pub(super) fn parse_type_removal_part(
    desc: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ResolvedPartSpec> {
    let low = desc.trim().trim_end_matches('.').to_ascii_lowercase();
    let rest = [
        "isn't a ",
        "isn't an ",
        "isn't ",
        "aren't ",
        "is no longer a ",
        "is no longer an ",
        "is no longer ",
        "are no longer ",
        "no longer a ",
        "no longer an ",
        "no longer ",
    ]
    .iter()
    .find_map(|prefix| low.strip_prefix(prefix))?
    .trim();
    if rest.is_empty() {
        return None;
    }
    let mut supertypes = BTreeSet::new();
    let mut types = BTreeSet::new();
    let mut land_subtypes = BTreeSet::new();
    let mut creature_subtypes = BTreeSet::new();
    for seg in rest
        .replace(", or ", ",")
        .replace(", and ", ",")
        .replace(" or ", ",")
        .replace(" and ", ",")
        .split(',')
    {
        let tok = seg
            .trim()
            .strip_prefix("a ")
            .or_else(|| seg.trim().strip_prefix("an "))
            .unwrap_or(seg.trim())
            .trim();
        if tok.is_empty() {
            continue;
        }
        match tok {
            "snow" => {
                supertypes.insert(Supertype::Snow);
            }
            "legendary" => {
                supertypes.insert(Supertype::Legendary);
            }
            "basic" => {
                supertypes.insert(Supertype::Basic);
            }
            "plains" => {
                land_subtypes.insert(LandType::Plains);
            }
            "island" | "islands" => {
                land_subtypes.insert(LandType::Island);
            }
            "swamp" | "swamps" => {
                land_subtypes.insert(LandType::Swamp);
            }
            "mountain" | "mountains" => {
                land_subtypes.insert(LandType::Mountain);
            }
            "forest" | "forests" => {
                land_subtypes.insert(LandType::Forest);
            }
            other => {
                if let Some(ct) = parse_card_type_word(other) {
                    types.insert(ct);
                } else {
                    let sub = resolve_protection_subtype(other, vocabulary)?;
                    creature_subtypes.insert(sub);
                }
            }
        }
    }
    if supertypes.is_empty()
        && types.is_empty()
        && land_subtypes.is_empty()
        && creature_subtypes.is_empty()
    {
        return None;
    }
    Some(ResolvedPartSpec::Type(TypeChangeSpec {
        land_subtypes: if land_subtypes.is_empty() {
            SubtypeOperation::Keep
        } else {
            SubtypeOperation::Remove(land_subtypes)
        },
        supertypes: if supertypes.is_empty() {
            TypeOperation::Keep
        } else {
            TypeOperation::Remove(supertypes)
        },
        types: if types.is_empty() {
            TypeOperation::Keep
        } else {
            TypeOperation::Remove(types)
        },
        subtypes: if creature_subtypes.is_empty() {
            SubtypeOperation::Keep
        } else {
            SubtypeOperation::Remove(creature_subtypes)
        },
    }))
}

pub(super) fn convert_static_type_change_to_spec(
    stc: StaticTypeChange,
) -> Result<TypeChangeSpec, &'static str> {
    let land_subtypes = match stc.land_subtypes {
        SubtypeOperation::Keep => SubtypeOperation::Keep,
        SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(s)) => SubtypeOperation::Add(s),
        SubtypeOperation::Set(StaticLandSubtypeOperand::Literal(s)) => SubtypeOperation::Set(s),
        SubtypeOperation::Remove(s) => SubtypeOperation::Remove(s),
        _ => return Err("oracle:unsupported-layer-type"),
    };
    let subtypes = match stc.subtypes {
        SubtypeOperation::Keep => SubtypeOperation::Keep,
        SubtypeOperation::Add(StaticSubtypeOperand::Literal(s)) => {
            SubtypeOperation::Add(InstructionSubtypeOperand::Literal(s))
        }
        SubtypeOperation::Set(StaticSubtypeOperand::Literal(s)) => {
            SubtypeOperation::Set(InstructionSubtypeOperand::Literal(s))
        }
        SubtypeOperation::Remove(s) => SubtypeOperation::Remove(s),
        _ => return Err("oracle:unsupported-layer-type"),
    };
    Ok(TypeChangeSpec {
        land_subtypes,
        supertypes: stc.supertypes,
        types: stc.types,
        subtypes,
    })
}

// Mirrors the StaticAbilityAst::ContinuousLayerEffect payload fields for temporary lowering.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_continuous_layer_effect_parts(
    set_types: Option<&str>,
    additive_types: bool,
    base_pt: Option<&(String, String)>,
    set_colors: Option<&str>,
    grant_keywords: &[String],
    lose_keywords: &[String],
    lose_all_abilities: bool,
    vocabulary: &CreatureTypeVocabulary,
    bound_x: Option<&ValueExprAst>,
    builder: &BodyBuilder,
    in_trigger: bool,
) -> Result<Vec<ResolvedPartSpec>, &'static str> {
    let mut parts = Vec::new();
    let mut has_color_part = false;
    if let Some(desc) = set_types {
        if let Some(removal_part) = parse_type_removal_part(desc, vocabulary) {
            parts.push(removal_part);
        } else {
            let static_part = parse_static_type_part(desc, additive_types, vocabulary)?;
            match static_part {
                StaticPart::Type(stc) => {
                    parts.push(ResolvedPartSpec::Type(convert_static_type_change_to_spec(
                        stc,
                    )?));
                }
                StaticPart::TypeWithSubtypes {
                    base,
                    artifact_subtypes,
                    enchantment_subtypes,
                } => {
                    parts.push(ResolvedPartSpec::TypeWithSubtypes {
                        base: convert_static_type_change_to_spec(base)?,
                        artifact_subtypes,
                        enchantment_subtypes,
                    });
                }
                StaticPart::Color(ColorOperation::Set(StaticColorOperand::Literal(c))) => {
                    has_color_part = true;
                    parts.push(ResolvedPartSpec::Color(ColorOperation::Set(c)));
                }
                StaticPart::Color(ColorOperation::Add(StaticColorOperand::Literal(c))) => {
                    has_color_part = true;
                    parts.push(ResolvedPartSpec::Color(ColorOperation::Add(c)));
                }
                _ => return Err("oracle:unsupported-layer-type"),
            }
        }
    }
    if let Some(color_str) = set_colors {
        if has_color_part {
            return Err("oracle:unsupported-layer-color");
        }
        let low_cs = color_str.trim().trim_end_matches('.').to_ascii_lowercase();
        let additive_colors = low_cs.contains("in addition to its other colors")
            || low_cs.contains("in addition to their other colors");
        let core = low_cs
            .replace("in addition to its other colors", " ")
            .replace("in addition to their other colors", " ");
        let mut colors = BTreeSet::new();
        let mut saw_colorless = false;
        for raw_tok in core
            .replace(" and ", " ")
            .replace(',', " ")
            .split_whitespace()
        {
            match raw_tok {
                "white" => {
                    colors.insert(Color::White);
                }
                "blue" => {
                    colors.insert(Color::Blue);
                }
                "black" => {
                    colors.insert(Color::Black);
                }
                "red" => {
                    colors.insert(Color::Red);
                }
                "green" => {
                    colors.insert(Color::Green);
                }
                "colorless" => {
                    saw_colorless = true;
                }
                "a" | "an" | "is" | "are" | "becomes" | "become" => {}
                _ => return Err("oracle:unsupported-layer-color"),
            }
        }
        if colors.is_empty() && !saw_colorless {
            return Err("oracle:unsupported-layer-color");
        }
        parts.push(ResolvedPartSpec::Color(if additive_colors {
            ColorOperation::Add(colors)
        } else {
            ColorOperation::Set(colors)
        }));
    }
    if lose_all_abilities || !grant_keywords.is_empty() || !lose_keywords.is_empty() {
        let mut change = if !grant_keywords.is_empty() {
            lower_ability_change(grant_keywords, vocabulary)?
        } else {
            AbilityChange::default()
        };
        if lose_all_abilities {
            change.removal = Some(AbilityRemoval::All);
        }
        if !lose_keywords.is_empty() {
            change
                .suppressed_keywords
                .extend(lower_keyword_list(lose_keywords)?);
        }
        parts.push(ResolvedPartSpec::Abilities(change));
    }
    if let Some((p_raw, t_raw)) = base_pt {
        let p_t = p_raw.trim();
        let t_t = t_raw.trim();
        if p_t.eq_ignore_ascii_case("keep") || t_t.eq_ignore_ascii_case("keep") {
            return Err("oracle:unsupported-layer-pt");
        }
        match (p_t.parse::<i64>(), t_t.parse::<i64>()) {
            (Ok(p), Ok(t)) => {
                parts.push(ResolvedPartSpec::SetPT(PowerToughness {
                    power: p.into(),
                    toughness: t.into(),
                }));
            }
            _ => {
                let parse_side = |s: &str| -> Result<ValueExpr, &'static str> {
                    if let Ok(n) = s.parse::<i64>() {
                        return Ok(ValueExpr::literal(n));
                    }
                    if s.eq_ignore_ascii_case("x") {
                        return if let Some(bx) = bound_x {
                            lower_value_expr_in_context(bx, builder, in_trigger, vocabulary)
                        } else {
                            Ok(ValueExpr::chosen_x())
                        };
                    }
                    let ast_e = spellwake_oracle_compiler::filter::parse_value_expr(
                        s,
                        &cached_compiler_vocab(vocabulary),
                    )
                    .map_err(|_| "oracle:unsupported-layer-pt")?;
                    lower_value_expr_in_context(
                        resolve_x(&ast_e, bound_x),
                        builder,
                        in_trigger,
                        vocabulary,
                    )
                };
                parts.push(ResolvedPartSpec::SetPTExpr {
                    power: Box::new(parse_side(p_t)?),
                    toughness: Box::new(parse_side(t_t)?),
                });
            }
        }
    }
    if parts.is_empty() {
        return Err("oracle:unsupported-layer-effect");
    }
    Ok(parts)
}

pub(super) fn parse_cant_be_target_keywords(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<BTreeSet<KeywordAbility>> {
    let ts = raw.trim().trim_end_matches('.').trim().to_ascii_lowercase();
    if ts == "spells or abilities" || ts == "any spells or abilities" {
        return Some(BTreeSet::from([KeywordAbility::Shroud]));
    }
    if ts == "spells or abilities your opponents control"
        || ts == "spells or abilities opponents control"
        || ts == "spells or abilities an opponent controls"
    {
        return Some(BTreeSet::from([KeywordAbility::Hexproof]));
    }
    if let Some(qual_raw) = ts
        .strip_suffix(" spells or abilities your opponents control")
        .or_else(|| ts.strip_suffix(" spells or abilities opponents control"))
        .or_else(|| ts.strip_suffix(" spells or abilities an opponent controls"))
    {
        return parse_hexproof_from_qualities(qual_raw.trim(), vocabulary).map(|quals| {
            quals
                .into_iter()
                .map(KeywordAbility::HexproofFrom)
                .collect()
        });
    }
    None
}

pub(super) fn partition_temp_keyword_grants(
    keywords: &[String],
) -> (Vec<ResolvedPartSpec>, Vec<String>) {
    let mut special_parts = Vec::new();
    let mut remaining = Vec::new();
    for kw in keywords {
        let low = kw.trim().trim_end_matches('.').trim().to_ascii_lowercase();
        if lower_keyword_ability_name(&low).is_some() {
            remaining.push(kw.clone());
            continue;
        }
        if low == "soulbond" {
            for entrant_only in [false, true] {
                special_parts.push(ResolvedPartSpec::GrantTrigger(Box::new(TriggeredAbility {
                    id: TriggeredAbilityId(0),
                    condition: TriggerSpec::keyword(TriggerKeywordEvent::Soulbond { entrant_only }),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::MutateObject(Box::new(
                            spellwake_engine::toolbox::MutateObjectSpec::soulbond_pair(
                                entrant_only,
                            ),
                        ))],
                    ),
                })));
            }
            continue;
        }
        if low == "vanishing" {
            special_parts.push(ResolvedPartSpec::GrantTrigger(Box::new(
                CounterKeyword::Vanishing(None).ability(TriggeredAbilityId(0)),
            )));
            special_parts.push(ResolvedPartSpec::GrantTrigger(Box::new(
                CounterKeyword::VanishingLastCounter.ability(TriggeredAbilityId(0)),
            )));
            continue;
        }
        if let Some(rest) = low.strip_prefix("equip ") {
            let cost_raw = rest
                .trim()
                .trim_start_matches(['-', ':'])
                .trim()
                .trim_end_matches('.')
                .trim();
            if let Some(mana_cost) = parse_fixed_mana_cost(&cost_raw.to_ascii_uppercase()) {
                special_parts.push(ResolvedPartSpec::GrantActivated(Box::new(
                    ActivatedAbility {
                        id: ActivatedAbilityId(0),
                        mana_cost,
                        source_cost: SourceCost::None,
                        body: ExecutableBody::new(
                            BodyTarget::SingleCreature,
                            vec![Instruction::MutateObject(Box::new(
                                spellwake_engine::toolbox::MutateObjectSpec::attach_ability(
                                    AttachAbility::Equip,
                                ),
                            ))],
                        ),
                    },
                )));
                continue;
            }
        }
        if let Some(rest) = low.strip_prefix("crew ")
            && let Ok(n) = rest.trim().trim_end_matches('.').trim().parse::<u64>()
        {
            special_parts.push(ResolvedPartSpec::GrantActivated(Box::new(
                ActivatedAbility::tap_for_power(ActivatedAbilityId(0), PowerAbility::Crew, n),
            )));
            continue;
        }
        if let Some(rest) = low.strip_prefix("outlast ") {
            let cost_raw = rest
                .trim()
                .trim_start_matches(['-', ':'])
                .trim()
                .trim_end_matches('.')
                .trim();
            if let Some(mana_cost) = parse_fixed_mana_cost(&cost_raw.to_ascii_uppercase()) {
                special_parts.push(ResolvedPartSpec::GrantActivatedRestricted {
                    ability: Box::new(ActivatedAbility {
                        id: ActivatedAbilityId(0),
                        mana_cost,
                        source_cost: SourceCost::Tap,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(
                                    CounterKind::PlusOne,
                                    ValueExpr::literal(1),
                                ),
                            ],
                        ),
                    }),
                    restriction: Box::new(ActivationRestriction::Sorcery),
                });
                continue;
            }
        }
        if let Some(br) = lower_extra_combat_mod_blocking_restriction(&low) {
            special_parts.push(ResolvedPartSpec::BlockingRestriction(br));
            continue;
        }
        let combat_br = match low.as_str() {
            "can't attack" | "cannot attack" => Some(BlockingRestriction::CannotAttack),
            "can't block" | "cannot block" => Some(BlockingRestriction::CannotBlock),
            "can't attack or block" | "cannot attack or block" => {
                Some(BlockingRestriction::CannotAttackOrBlock)
            }
            "blocks each combat if able" | "blocks this turn if able" => {
                Some(BlockingRestriction::MustBlockEachCombatIfAble)
            }
            "must be blocked if able" | "must be blocked this turn if able" => {
                Some(BlockingRestriction::MustBeBlocked)
            }
            "can block an additional creature" | "can block an additional creature each combat" => {
                Some(BlockingRestriction::AdditionalBlock)
            }
            "can block any number of creatures" => Some(BlockingRestriction::CanBlockAnyNumber),
            "can attack as though it didn't have defender" => {
                Some(BlockingRestriction::CanAttackAsThoughNoDefender)
            }
            _ => None,
        };
        if let Some(br) = combat_br {
            special_parts.push(ResolvedPartSpec::BlockingRestriction(br));
        } else {
            remaining.push(kw.clone());
        }
    }
    (special_parts, remaining)
}

#[allow(clippy::too_many_arguments)]
fn lower_animate_or_set_pt_extended(
    subject: &SubjectRefAst,
    power: &str,
    toughness: &str,
    description: Option<&str>,
    keywords: &[String],
    duration: &DurationAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    let p_trim = power.trim();
    let t_trim = toughness.trim();
    let omit_pt = (p_trim.is_empty() || p_trim.eq_ignore_ascii_case("keep"))
        && (t_trim.is_empty() || t_trim.eq_ignore_ascii_case("keep"));

    if let Some(desc) = description
        && omit_pt
        && keywords.is_empty()
    {
        let d_low = desc
            .trim()
            .trim_end_matches('.')
            .trim()
            .to_ascii_lowercase();
        let dur = lower_duration(duration)?;

        // Chosen characteristic references (Mistform Wakecaster, Elsewhere Flask, Terraformer, Puca's Eye)
        if matches!(
            d_low.as_str(),
            "the creature type of your choice" | "a creature type of your choice"
        ) {
            let mut trial = builder.clone();
            let slot = ids.creature_type_slot();
            trial
                .instructions
                .push(super::super::toolbox_emit::choose_creature_type(
                    slot,
                    BTreeSet::new(),
                ));
            let part = ResolvedPartSpec::Type(TypeChangeSpec {
                land_subtypes: SubtypeOperation::Keep,
                supertypes: TypeOperation::Keep,
                types: TypeOperation::Keep,
                subtypes: SubtypeOperation::Set(InstructionSubtypeOperand::Chosen(slot)),
            });
            emit_resolved_parts_for_subject(
                subject,
                ResolvedPartsSpec(vec![part]),
                dur,
                &mut trial,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            *builder = trial;
            return Ok(());
        }
        if matches!(
            d_low.as_str(),
            "that type" | "the chosen type" | "that basic land type" | "the chosen basic land type"
        ) {
            let prior_part = builder.instructions.iter().rev().find_map(|op| match op {
                Instruction::ChooseValue {
                    domain: ResolutionChoiceDomain::BasicLandType,
                    ..
                } => Some(ResolvedPartSpec::ChosenBasicLandType { add: false }),
                Instruction::PromptChoice(spec) => match &spec.domain {
                    spellwake_engine::toolbox::PromptDomain::Subtype {
                        legacy_slot: Some(into),
                        ..
                    } => Some(ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Set(InstructionSubtypeOperand::Chosen(*into)),
                    })),
                    _ => None,
                },
                _ => None,
            });
            if let Some(part) = prior_part {
                let mut trial = builder.clone();
                emit_resolved_parts_for_subject(
                    subject,
                    ResolvedPartsSpec(vec![part]),
                    dur,
                    &mut trial,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                )?;
                *builder = trial;
                return Ok(());
            }
        }
        if matches!(d_low.as_str(), "the chosen color" | "that color")
            && builder.instructions.iter().rev().any(|op| {
                matches!(
                    op,
                    Instruction::ChooseValue {
                        domain: ResolutionChoiceDomain::Color,
                        ..
                    }
                )
            })
        {
            let mut trial = builder.clone();
            emit_resolved_parts_for_subject(
                subject,
                ResolvedPartsSpec(vec![ResolvedPartSpec::ChosenColor { add: false }]),
                dur,
                &mut trial,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
            )?;
            *builder = trial;
            return Ok(());
        }

        // Basic land type choice ("a Plains or an Island", e.g. Tundra Kavu)
        if d_low.contains(" or ") {
            let parse_land_opt = |seg: &str| -> Option<(LandType, &'static str)> {
                let cleaned = seg
                    .trim()
                    .trim_start_matches("an ")
                    .trim_start_matches("a ")
                    .trim();
                match cleaned {
                    "plains" => Some((LandType::Plains, "Plains")),
                    "island" => Some((LandType::Island, "Island")),
                    "swamp" => Some((LandType::Swamp, "Swamp")),
                    "mountain" => Some((LandType::Mountain, "Mountain")),
                    "forest" => Some((LandType::Forest, "Forest")),
                    _ => None,
                }
            };
            let land_opts: Vec<(LandType, &'static str)> =
                d_low.split(" or ").filter_map(parse_land_opt).collect();
            let raw_count = d_low.split(" or ").count();
            if raw_count >= 2 && land_opts.len() == raw_count {
                let mut trial = builder.clone();
                let is_target_one = matches!(
                    subject,
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                );
                let is_self = matches!(subject, SubjectRefAst::SelfObject)
                    || is_self_pronoun(subject, &trial, self_pronoun_valid);
                if is_target_one || is_self {
                    if let SubjectRefAst::TargetObject { filter, .. } = subject {
                        let mut inner = filter.as_ref().clone();
                        inner.zone = None;
                        let bt = lower_battlefield_body_target(&inner, vocabulary)?;
                        trial.bind_target(bt)?;
                    }
                    let branches: Vec<ChooseOptionBranch> = land_opts
                        .into_iter()
                        .map(|(lt, label)| {
                            let parts =
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                                    land_subtypes: SubtypeOperation::Set(BTreeSet::from([lt])),
                                    supertypes: TypeOperation::Keep,
                                    types: TypeOperation::Keep,
                                    subtypes: SubtypeOperation::Keep,
                                })]);
                            let op = if is_self {
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                    spellwake_engine::toolbox::ObjectQuery::Source,
                                    parts,
                                    dur,
                                )
                            } else {
                                crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(spellwake_engine::toolbox::ObjectQuery::Targets, parts, dur)
                            };
                            ChooseOptionBranch {
                                label: label.to_string(),
                                body: ExecutableBody::new(BodyTarget::None, vec![op]),
                            }
                        })
                        .collect();
                    trial.instructions.push(Instruction::ChooseOption {
                        chooser: ResolvingPlayer::Controller,
                        options: branches,
                    });
                    *builder = trial;
                    return Ok(());
                }
            }
        }
    }

    let mut eff_power = power.to_string();
    let mut eff_toughness = toughness.to_string();
    let mut eff_keywords: Vec<String> = keywords.to_vec();
    let mut extra_unblockable = false;
    let mut extra_suppressed_kws: BTreeSet<KeywordAbility> = BTreeSet::new();
    let mut remove_equipment = false;
    let mut normalized_desc: Option<String> = None;

    if let Some(desc) = description {
        let mut buf = desc.trim().to_string();
        let replace_ci = |s: &mut String, needle: &str, with: &str| -> bool {
            let mut found = false;
            while let Some(idx) = s.to_ascii_lowercase().find(needle) {
                s.replace_range(idx..idx + needle.len(), with);
                found = true;
            }
            found
        };

        replace_ci(&mut buf, "has no abilities", "loses all abilities");
        replace_ci(&mut buf, "have no abilities", "loses all abilities");
        replace_ci(
            &mut buf,
            "in addition to his other types",
            "in addition to its other types",
        );
        replace_ci(
            &mut buf,
            "in addition to her other types",
            "in addition to its other types",
        );
        replace_ci(&mut buf, "that are still lands", "still lands");
        replace_ci(&mut buf, "that's still a land", "still a land");
        replace_ci(&mut buf, "that is still a land", "still a land");
        replace_ci(&mut buf, "that are still artifacts", "still artifacts");
        replace_ci(&mut buf, "that's still an artifact", "still an artifact");
        replace_ci(&mut buf, "that is still an artifact", "still an artifact");

        if replace_ci(&mut buf, "that's no longer an equipment", "")
            || replace_ci(&mut buf, "that is no longer an equipment", "")
        {
            remove_equipment = true;
        }
        if replace_ci(&mut buf, "and can't be blocked this turn", "")
            || replace_ci(&mut buf, "and can't be blocked", "")
        {
            extra_unblockable = true;
        }

        // Trailing "and loses <keyword>" (Chimeric Sphere)
        for lose_pat in [" and loses ", " and lose "] {
            let buf_low = buf.to_ascii_lowercase();
            if let Some(idx) = buf_low.find(lose_pat) {
                let tail = buf[idx + lose_pat.len()..]
                    .trim()
                    .trim_end_matches('.')
                    .trim();
                if let Some(kw) = lower_keyword_ability_name(&tail.to_ascii_lowercase()) {
                    extra_suppressed_kws.insert(kw);
                    buf.truncate(idx);
                    break;
                }
            }
        }

        // Trailing "and gains <kw1> [and <kw2>]" (Majestic Metamorphosis)
        for gain_pat in [" and gains ", " and gain "] {
            let buf_low = buf.to_ascii_lowercase();
            if let Some(idx) = buf_low.find(gain_pat) {
                let tail = buf[idx + gain_pat.len()..]
                    .trim()
                    .trim_end_matches('.')
                    .trim();
                let tail_parts: Vec<&str> = tail
                    .split(',')
                    .flat_map(|seg| seg.split(" and "))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                if !tail_parts.is_empty()
                    && tail_parts
                        .iter()
                        .all(|k| lower_keyword_ability_name(&k.to_ascii_lowercase()).is_some())
                {
                    for k in tail_parts {
                        eff_keywords.push(k.to_string());
                    }
                    buf.truncate(idx);
                    break;
                }
            }
        }

        // Inline "with base power and toughness P/T" when outer power/toughness omitted (The Master, Transcendent)
        if omit_pt {
            let buf_low = buf.to_ascii_lowercase();
            for pt_pat in [
                " with base power and toughness ",
                " with power and toughness ",
            ] {
                if let Some(idx) = buf_low.find(pt_pat) {
                    let tail = buf[idx + pt_pat.len()..]
                        .trim()
                        .trim_end_matches('.')
                        .trim();
                    if let Some((ps, ts)) = tail.split_once('/')
                        && ps.trim().parse::<i64>().is_ok()
                        && ts.trim().parse::<i64>().is_ok()
                    {
                        eff_power = ps.trim().to_string();
                        eff_toughness = ts.trim().to_string();
                        buf.truncate(idx);
                        break;
                    }
                }
            }
        }

        normalized_desc = Some(buf.trim().to_string());
    }

    let mut trial = builder.clone();
    let before_len = trial.instructions.len();
    lower_animate_or_set_pt(
        subject,
        &eff_power,
        &eff_toughness,
        normalized_desc.as_deref(),
        &eff_keywords,
        duration,
        false,
        &mut trial,
        vocabulary,
        in_trigger,
        self_pronoun_valid,
        bound_x,
    )?;

    if extra_unblockable || !extra_suppressed_kws.is_empty() || remove_equipment {
        if trial.instructions.len() != before_len + 1 {
            return Err("oracle:unsupported-animate-desc");
        }
        let Some(last) = trial.instructions.last_mut() else {
            return Err("oracle:unsupported-animate-desc");
        };
        let parts_mut = match last {
            Instruction::ApplyContinuous(spec) => match &mut spec.payload {
                spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(parts)
                | spellwake_engine::toolbox::ContinuousPayload::ModifyAnyNumber { parts } => {
                    &mut parts.0
                }
                _ => return Err("oracle:unsupported-animate-desc"),
            },
            _ => return Err("oracle:unsupported-animate-desc"),
        };
        if extra_unblockable || !extra_suppressed_kws.is_empty() {
            let mut merged_ab = false;
            for part in parts_mut.iter_mut() {
                if let ResolvedPartSpec::Abilities(ab) = part {
                    if extra_unblockable {
                        ab.grants_unblockable = true;
                    }
                    ab.suppressed_keywords
                        .extend(extra_suppressed_kws.iter().copied());
                    merged_ab = true;
                    break;
                }
            }
            if !merged_ab {
                let ab = AbilityChange {
                    grants_unblockable: extra_unblockable,
                    suppressed_keywords: extra_suppressed_kws,
                    ..AbilityChange::default()
                };
                parts_mut.push(ResolvedPartSpec::Abilities(ab));
            }
        }
        if remove_equipment {
            let mut patched = false;
            for part in parts_mut.iter_mut() {
                if let ResolvedPartSpec::Type(base) = part {
                    let base_clone = base.clone();
                    *part = ResolvedPartSpec::TypeWithSubtypes {
                        base: base_clone,
                        artifact_subtypes: SubtypeOperation::Remove(BTreeSet::from([
                            ArtifactSubtype::Equipment,
                        ])),
                        enchantment_subtypes: SubtypeOperation::Keep,
                    };
                    patched = true;
                    break;
                }
                if let ResolvedPartSpec::TypeWithSubtypes {
                    artifact_subtypes, ..
                } = part
                    && matches!(artifact_subtypes, SubtypeOperation::Keep)
                {
                    *artifact_subtypes =
                        SubtypeOperation::Remove(BTreeSet::from([ArtifactSubtype::Equipment]));
                    patched = true;
                    break;
                }
            }
            if !patched {
                return Err("oracle:unsupported-animate-desc");
            }
        }
    }

    *builder = trial;
    Ok(())
}
