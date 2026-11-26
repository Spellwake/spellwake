use super::*;

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
        InstructionAst::Transform { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(Instruction::TransformSource);
            Ok(())
        }
        InstructionAst::BecomePrepared { subject }
            if is_self_pronoun(subject, builder, self_pronoun_valid) =>
        {
            builder.instructions.push(Instruction::BecomePreparedSource);
            Ok(())
        }
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            duration,
            scale: Some(scale_expr),
        } => {
            let clean_subject = strip_battlefield_zone_from_subject(subject);
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
                            Some(EffectSubject::Matching(obj))
                        }
                        _ => None,
                    };
                    if let Some(group) = group {
                        builder.instructions.push(Instruction::ModifyGroupByAmount {
                            group: Box::new(group),
                            scale: PowerToughness {
                                power: dp.into(),
                                toughness: dt.into(),
                            },
                            amount: amt,
                            keywords: BTreeSet::new(),
                        });
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
                    builder.instructions.push(Instruction::ChangeGroup {
                        group: Box::new(EffectSubject::CreatedTokens),
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    });
                    return Ok(());
                }
                if matches!(subject, SubjectRefAst::Anaphora(_))
                    && !is_permanent_body_target(builder.target)
                    && anaphora_moved
                {
                    builder.instructions.push(Instruction::ChangeGroup {
                        group: Box::new(EffectSubject::LastMoved),
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    });
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
                builder.instructions.push(Instruction::ChangeGroup {
                    group: Box::new(EffectSubject::CreatedTokens),
                    parts: ResolvedPartsSpec(vec![part]),
                    duration: dur,
                });
                return Ok(());
            }
            if matches!(subject, SubjectRefAst::Anaphora(_))
                && !is_permanent_body_target(builder.target)
                && anaphora_moved
            {
                builder.instructions.push(Instruction::ChangeGroup {
                    group: Box::new(EffectSubject::LastMoved),
                    parts: ResolvedPartsSpec(vec![part]),
                    duration: dur,
                });
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
            let clean_subject = strip_battlefield_zone_from_subject(subject);
            let subject = &clean_subject;
            let dur = lower_duration(duration)?;
            let dp = parse_signed_pt_delta(dx);
            let dt = parse_signed_pt_delta(dy);
            let anaphora_created = last_instruction_created_tokens(&builder.instructions)
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
                                .push(Instruction::ModifyGroupUntilEndOfTurn {
                                    group: Box::new(EffectSubject::CreatedTokens),
                                    power_toughness: Some(pt),
                                    keywords: BTreeSet::new(),
                                });
                        } else {
                            builder.instructions.push(Instruction::ChangeGroup {
                                group: Box::new(EffectSubject::CreatedTokens),
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                                duration: dur,
                            });
                        }
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && !is_permanent_body_target(builder.target)
                        && anaphora_moved
                    {
                        if dur == ResolvedDuration::UntilEndOfTurn {
                            builder
                                .instructions
                                .push(Instruction::ModifyGroupUntilEndOfTurn {
                                    group: Box::new(EffectSubject::LastMoved),
                                    power_toughness: Some(pt),
                                    keywords: BTreeSet::new(),
                                });
                        } else {
                            builder.instructions.push(Instruction::ChangeGroup {
                                group: Box::new(EffectSubject::LastMoved),
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(pt)]),
                                duration: dur,
                            });
                        }
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
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::CreatedTokens),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if matches!(subject, SubjectRefAst::Anaphora(_))
                        && !is_permanent_body_target(builder.target)
                        && anaphora_moved
                    {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::LastMoved),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
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
                        builder.instructions.push(Instruction::GrantFlashback(
                            spellwake_engine::program::FlashbackSubject::Target,
                        ));
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
                        builder.instructions.push(Instruction::GrantFlashback(
                            spellwake_engine::program::FlashbackSubject::ControllerGraveyard,
                        ));
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
                        if dur == ResolvedDuration::UntilEndOfTurn {
                            ops.push(Instruction::GrantProtectionChosenColor(recipients));
                        } else {
                            ops.push(Instruction::GrantChosenProtection {
                                recipients,
                                duration: dur,
                            });
                        }
                    };
                let mut handled_prot = false;
                if matches!(chosen_kind, ChosenQualityGrant::ProtectionColor) {
                    handled_prot = match eff_subject {
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
                            count: TargetCountAst::Exact(1),
                        } => {
                            let bt = lower_target_one_of_permanent_body_target(
                                options, builder, in_trigger, vocabulary,
                            )?;
                            builder.bind_target(bt)?;
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
                        TargetUpTo(u16),
                        AbilitySource,
                        Group(Box<EffectSubject>),
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
                        let mut branches = Vec::with_capacity(kw_options.len());
                        for (label, kw) in kw_options {
                            let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                AbilityChange::grant_keywords(BTreeSet::from([kw])),
                            )]);
                            let ops = match &tk {
                                ChoiceTargetKind::TargetExact => {
                                    vec![Instruction::CreateContinuousEffect {
                                        parts,
                                        duration: dur,
                                    }]
                                }
                                ChoiceTargetKind::TargetUpTo(max) => {
                                    vec![Instruction::AffectPermanentTargets {
                                        minimum: 0,
                                        maximum: *max,
                                        action: PermanentTargetAction::Modify {
                                            parts,
                                            duration: dur,
                                        },
                                    }]
                                }
                                ChoiceTargetKind::AbilitySource => {
                                    vec![Instruction::ChangeAbilitySource {
                                        parts,
                                        duration: dur,
                                    }]
                                }
                                ChoiceTargetKind::Group(subj) => {
                                    vec![Instruction::ChangeGroup {
                                        group: subj.clone(),
                                        parts,
                                        duration: dur,
                                    }]
                                }
                                ChoiceTargetKind::ControllerAndPermanents => vec![
                                    Instruction::CreateTemporaryRule {
                                        rule: Box::new(TemporaryRuleKind::PlayerGrantKeyword {
                                            players: AffectedPlayers::One(
                                                ResolvingPlayer::Controller,
                                            ),
                                            keywords: BTreeSet::from([kw]),
                                        }),
                                        duration: dur,
                                    },
                                    Instruction::ChangeGroup {
                                        group: Box::new(EffectSubject::Matching(
                                            ObjectFilter::Clause(ObjectClause::any().controlled()),
                                        )),
                                        parts,
                                        duration: dur,
                                    },
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
            for kw in &remaining_keywords {
                let low = kw.trim().trim_end_matches('.').to_ascii_lowercase();
                if let Some(rest) = low.strip_prefix("protection from ")
                    && rest.contains(',')
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
                            parts.push(format!("protection from {item}"));
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
                expanded_keywords.push(kw.clone());
            }
            let (mut extra_parts, kw_remaining) = partition_temp_keyword_grants(&expanded_keywords);
            let anaphora_after_chosen = SubjectRefAst::Anaphora("it".to_string());
            let grant_subject = if bound_subject_already {
                &anaphora_after_chosen
            } else {
                eff_subject
            };
            if !extra_parts.is_empty() {
                if !kw_remaining.is_empty() {
                    let ab_change = lower_ability_change(&kw_remaining, vocabulary)?;
                    extra_parts.push(ResolvedPartSpec::Abilities(ab_change));
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
                        if let Some(Instruction::BoostSource {
                            keywords: prev_kws, ..
                        }) = builder.instructions.last_mut()
                        {
                            prev_kws.extend(kws);
                            return Ok(());
                        }
                        if let Some(Instruction::ModifyGroupUntilEndOfTurn {
                            group,
                            keywords: prev_kws,
                            ..
                        }) = builder.instructions.last_mut()
                            && matches!(group.as_ref(), EffectSubject::AbilitySource)
                        {
                            prev_kws.extend(kws);
                            return Ok(());
                        }
                        if in_trigger {
                            builder
                                .instructions
                                .push(Instruction::ModifyGroupUntilEndOfTurn {
                                    group: Box::new(EffectSubject::AbilitySource),
                                    power_toughness: None,
                                    keywords: kws,
                                });
                        } else {
                            builder.instructions.push(Instruction::BoostSource {
                                power_toughness: None,
                                keywords: kws,
                            });
                        }
                        return Ok(());
                    }
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                        duration: dur,
                    });
                    Ok(())
                }
                s if (self_pronoun_valid || in_trigger)
                    && is_self_pronoun(s, builder, self_pronoun_valid) =>
                {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                        duration: dur,
                    });
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
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: 1,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                    if let Some(Instruction::CreateContinuousEffect { parts, duration: d }) =
                        builder.instructions.last_mut()
                        && *d == dur
                        && !parts
                            .0
                            .iter()
                            .any(|p| matches!(p, ResolvedPartSpec::Abilities(_)))
                    {
                        parts.0.push(ResolvedPartSpec::Abilities(ab_change));
                        return Ok(());
                    }
                    if let Some(kws) = pure_kws
                        && let Some(Instruction::BoostTargetPower {
                            keywords: prev_kws, ..
                        }) = builder.instructions.last_mut()
                        && dur == ResolvedDuration::UntilEndOfTurn
                    {
                        prev_kws.extend(kws);
                        return Ok(());
                    }
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: *n,
                            maximum: *n,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
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
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: *max,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
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
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: u16::MAX,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
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
                    builder
                        .instructions
                        .push(Instruction::ModifyAnyNumberOfPermanents {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::PlayerGrantKeyword {
                            players,
                            keywords: kws,
                        }),
                        duration: dur,
                    });
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
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::PlayerGrantKeyword {
                            players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                            keywords: kws,
                        }),
                        duration: dur,
                    });
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if is_permanent_body_target(builder.target) =>
                {
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) | SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if !is_permanent_body_target(builder.target)
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    let group = Box::new(EffectSubject::LastMoved);
                    if dur == ResolvedDuration::UntilEndOfTurn
                        && let Some(kws) = pure_kws
                    {
                        builder
                            .instructions
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group: Box::new(EffectSubject::Matching(group_obj)),
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(group_obj)),
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                            .push(Instruction::ModifyGroupUntilEndOfTurn {
                                group,
                                power_toughness: None,
                                keywords: kws,
                            });
                    } else {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group,
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: 1,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                                duration: dur,
                            },
                        });
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
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                            duration: dur,
                        });
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
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: 1,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                                duration: dur,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::SelfObject => {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::SwitchPT]),
                        duration: dur,
                    });
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
            if matches!(count.as_ref(), ValueExprAst::Const(1))
                && matches!(
                    kind_low_full.as_str(),
                    "each_kind_already_on"
                        | "of each kind already on it"
                        | "each kind already on it"
                        | "another counter of each kind already on it"
                        | "a counter of each kind already on it"
                        | "of each kind already there"
                        | "each kind already there"
                        | "of each kind already on that permanent"
                        | "of each kind already on that creature"
                        | "each kind of counter already on it"
                        | "of each kind on it"
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
                    SubjectRefAst::TriggerSubject if in_trigger => {
                        Some(EffectSubject::TriggerSubject)
                    }
                    SubjectRefAst::Anaphora(_) if is_permanent_body_target(trial.target) => {
                        Some(EffectSubject::TargetObject)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_created_tokens(&trial.instructions) =>
                    {
                        Some(EffectSubject::CreatedTokens)
                    }
                    SubjectRefAst::Anaphora(_)
                        if last_instruction_moved_object(&trial.instructions) =>
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
                        .push(Instruction::AddOneOfEachCounterKindOnSubject {
                            subject: Box::new(eff_subj),
                        });
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
                        .or_else(|| kind_low_full.strip_prefix("your choice of a "))
                        .and_then(|rest| {
                            if rest.starts_with("counter from among ") {
                                rest.strip_prefix("counter from among ")
                            } else if kind_low_full
                                .starts_with("your choice of a counter from among ")
                            {
                                Some(rest)
                            } else {
                                None
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
                            count: TargetCountAst::Exact(1),
                        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
                            let mut f_clean = filter.as_ref().clone();
                            f_clean.zone = None;
                            let bt = lower_battlefield_body_target_in_context(
                                &f_clean, &trial, in_trigger, vocabulary,
                            )?;
                            trial.bind_target(bt)?;
                            SubjectRefAst::Anaphora("it".to_string())
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
                    let prior_had_moved = last_instruction_moved_object(&trial.instructions);
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
                                    vec![Instruction::PutCountersOnSubject {
                                        subject: Box::new(*direct_subj),
                                        kind: *opt_ck,
                                        amount: amt_expr,
                                    }],
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
                                count: TargetCountAst::Exact(1),
                                ..
                            }
                            | SubjectRefAst::TargetOneOf {
                                count: TargetCountAst::Exact(1),
                                ..
                            } => Box::new(SubjectRefAst::Anaphora("it".to_string())),
                            SubjectRefAst::TargetPlayer {
                                count: TargetCountAst::Exact(1),
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
            let ck = parse_counter_kind(kind_clean).ok_or("oracle:put-counter-kind")?;
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            if ck.is_player_counter() {
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
                    builder.instructions.push(Instruction::AddPlayerCounter {
                        players,
                        kind: ck,
                        amount: amt,
                    });
                    return Ok(());
                }
                if let SubjectRefAst::Anaphora(raw) = subject.as_ref()
                    && is_player_anaphora_label(raw)
                {
                    let players = lower_affected_players_in_context(
                        PlayerRefAst::ThatPlayer,
                        builder,
                        in_trigger,
                    )?;
                    builder.instructions.push(Instruction::AddPlayerCounter {
                        players,
                        kind: ck,
                        amount: amt,
                    });
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
                    builder.instructions.push(Instruction::AddPlayerCounter {
                        players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                        kind: ck,
                        amount: amt,
                    });
                    return Ok(());
                }
                return Err("oracle:put-counter-kind");
            }
            let const_n = match &amt {
                ValueExpr::Literal(n) => n.to_usize().filter(|&v| v > 0),
                _ => None,
            };
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
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    if let (ValueExpr::Literal(n), Some(n_usize)) = (&amt, const_n) {
                        if ck == CounterKind::PlusOne {
                            builder
                                .instructions
                                .push(Instruction::PutCountersOnSource { amount: n.clone() });
                            return Ok(());
                        }
                        if ck == CounterKind::Charge && n_usize == 1 {
                            builder
                                .instructions
                                .push(Instruction::PutChargeCounterOnSource);
                            return Ok(());
                        }
                        if ck != CounterKind::MinusOne
                            && let Ok(u) = u16::try_from(n_usize)
                        {
                            builder
                                .instructions
                                .push(Instruction::PutNamedCounterOnSource {
                                    kind: ck,
                                    amount: u,
                                });
                            return Ok(());
                        }
                    }
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::AbilitySource),
                            kind: ck,
                            amount: amt,
                        });
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
                    if ck == CounterKind::Indestructible && const_n == Some(1) {
                        builder
                            .instructions
                            .push(Instruction::PutIndestructibleCounter);
                    } else if ck == CounterKind::Loyalty && const_n == Some(1) {
                        builder.instructions.push(Instruction::PutLoyaltyCounter);
                    } else if let (ValueExpr::Literal(n), Some(_)) = (&amt, const_n)
                        && matches!(ck, CounterKind::PlusOne | CounterKind::MinusOne)
                    {
                        builder
                            .instructions
                            .push(Instruction::PutPowerToughnessCounters {
                                counters: if ck == CounterKind::PlusOne {
                                    PowerToughnessCounters {
                                        plus_one: n.clone(),
                                        minus_one: 0.into(),
                                    }
                                } else {
                                    PowerToughnessCounters {
                                        plus_one: 0.into(),
                                        minus_one: n.clone(),
                                    }
                                },
                            });
                    } else {
                        builder
                            .instructions
                            .push(Instruction::PutNamedCounterOnTarget {
                                kind: ck,
                                amount: amt,
                            });
                    }
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
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: *n,
                            maximum: *n,
                            action,
                        });
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
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: *max,
                            action,
                        });
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
                    if ck == CounterKind::Indestructible && const_n == Some(1) {
                        builder
                            .instructions
                            .push(Instruction::PutIndestructibleCounter);
                    } else if ck == CounterKind::Loyalty && const_n == Some(1) {
                        builder.instructions.push(Instruction::PutLoyaltyCounter);
                    } else if let (ValueExpr::Literal(n), Some(_)) = (&amt, const_n)
                        && matches!(ck, CounterKind::PlusOne | CounterKind::MinusOne)
                    {
                        builder
                            .instructions
                            .push(Instruction::PutPowerToughnessCounters {
                                counters: if ck == CounterKind::PlusOne {
                                    PowerToughnessCounters {
                                        plus_one: n.clone(),
                                        minus_one: 0.into(),
                                    }
                                } else {
                                    PowerToughnessCounters {
                                        plus_one: 0.into(),
                                        minus_one: n.clone(),
                                    }
                                },
                            });
                    } else {
                        builder
                            .instructions
                            .push(Instruction::PutNamedCounterOnTarget {
                                kind: ck,
                                amount: amt,
                            });
                    }
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
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: *n,
                            maximum: *n,
                            action,
                        });
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
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: *max,
                            action,
                        });
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
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: u16::MAX,
                            action,
                        });
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
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: u16::MAX,
                            action,
                        });
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
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: u16::MAX,
                            action: PermanentTargetAction::PutCounters {
                                kind: ck,
                                amount: amt,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions)
                        || (has_prior_token_creation(&builder.instructions)
                            && !is_permanent_body_target(builder.target)
                            && !has_prior_moved_object(&builder.instructions)) =>
                {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::CreatedTokens),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::LastMoved),
                            kind: ck,
                            amount: amt,
                        });
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
                    if ck == CounterKind::Indestructible && const_n == Some(1) {
                        builder
                            .instructions
                            .push(Instruction::PutIndestructibleCounter);
                    } else if ck == CounterKind::Loyalty && const_n == Some(1) {
                        builder.instructions.push(Instruction::PutLoyaltyCounter);
                    } else {
                        builder
                            .instructions
                            .push(Instruction::PutNamedCounterOnTarget {
                                kind: ck,
                                amount: amt,
                            });
                    }
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject => {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) =>
                {
                    let mut f_clean = f.as_ref().clone();
                    f_clean.zone = None;
                    let obj = lower_object_filter(&f_clean, vocabulary)?;
                    bind_filter_player_target(builder, &obj)?;
                    builder.last_group_filter = Some(obj);
                    if ck == CounterKind::PlusOne
                        && const_n == Some(1)
                        && f_clean.controller == Some(ControllerRelation::You)
                        && filter_only_has(
                            &f_clean,
                            FilterAllow {
                                types: true,
                                subtypes: true,
                                controller: true,
                                flags: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        if f_clean.subtypes.len() == 1
                            && f_clean.flags.is_empty()
                            && (f_clean.any_types.is_empty()
                                || f_clean.any_types == AstCardTypeMask::CREATURE)
                            && let Some(ct) = vocabulary.resolve_label(&f_clean.subtypes[0])
                        {
                            builder
                                .instructions
                                .push(Instruction::PutCountersOnControlledSubtype(ct));
                            return Ok(());
                        }
                        if f_clean.any_types == AstCardTypeMask::CREATURE
                            && f_clean.subtypes.is_empty()
                        {
                            if f_clean.flags == AstFilterFlags::EXCLUDE_SELF && self_pronoun_valid {
                                builder
                                    .instructions
                                    .push(Instruction::PutCountersOnOtherCreatures);
                                return Ok(());
                            }
                            if f_clean.flags.is_empty() {
                                builder
                                    .instructions
                                    .push(Instruction::PutCountersOnControlledCreatures);
                                return Ok(());
                            }
                        }
                    }
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::Matching(obj)),
                            kind: ck,
                            amount: amt,
                        });
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
                    let action = if ck == CounterKind::PlusOne && const_n == Some(1) {
                        PermanentTargetAction::PutPlusOneCounter
                    } else {
                        PermanentTargetAction::PutCounters {
                            kind: ck,
                            amount: amt,
                        }
                    };
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: u16::MAX,
                            action,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::Matching(group_obj)),
                            kind: ck,
                            amount: amt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if !is_permanent_body_target(builder.target)
                        && has_prior_moved_object(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::PutCountersOnSubject {
                            subject: Box::new(EffectSubject::LastMoved),
                            kind: ck,
                            amount: amt,
                        });
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
            builder.instructions.push(Instruction::Proliferate);
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
                    let amt = match amt_opt {
                        Some(v) => v,
                        None => ValueExpr::CounterCount {
                            subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                            kind,
                        },
                    };
                    builder
                        .instructions
                        .push(Instruction::RemoveCountersFromPermanent {
                            target_self: true,
                            kind,
                            amount: amt,
                        });
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
                    let amt = match amt_opt {
                        Some(v) => v,
                        None => ValueExpr::CounterCount {
                            subject: spellwake_engine::program::CounterSubjectRef::TargetObject,
                            kind,
                        },
                    };
                    builder
                        .instructions
                        .push(Instruction::RemoveCountersFromPermanent {
                            target_self: false,
                            kind,
                            amount: amt,
                        });
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
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: min_t,
                            maximum: max_t,
                            action: PermanentTargetAction::RemoveCounters {
                                kind,
                                amount: amt_opt,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                    builder
                        .instructions
                        .push(Instruction::RemoveCountersFromSubject {
                            subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            kind,
                            amount: amt_opt,
                        });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder
                        .instructions
                        .push(Instruction::RemoveCountersFromSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            kind,
                            amount: amt_opt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::RemoveCountersFromSubject {
                            subject: Box::new(EffectSubject::CreatedTokens),
                            kind,
                            amount: amt_opt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
                    builder
                        .instructions
                        .push(Instruction::RemoveCountersFromSubject {
                            subject: Box::new(EffectSubject::TargetObject),
                            kind,
                            amount: amt_opt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if !is_permanent_body_target(builder.target)
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::RemoveCountersFromSubject {
                            subject: Box::new(EffectSubject::LastMoved),
                            kind,
                            amount: amt_opt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    builder
                        .instructions
                        .push(Instruction::RemoveCountersFromSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            kind,
                            amount: amt_opt,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    builder
                        .instructions
                        .push(Instruction::RemoveCountersFromSubject {
                            subject: Box::new(EffectSubject::Matching(group_obj)),
                            kind,
                            amount: amt_opt,
                        });
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
                        .push(Instruction::RemoveCountersFromSubject {
                            subject: Box::new(EffectSubject::Matching(obj)),
                            kind,
                            amount: amt_opt,
                        });
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
            builder.instructions.push(Instruction::Bolster(u));
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
            builder.instructions.push(Instruction::Adapt(u));
            Ok(())
        }
        InstructionAst::Explore { subject } => {
            if is_self_pronoun(subject, builder, self_pronoun_valid) {
                builder.instructions.push(Instruction::ExploreSource);
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
                        .push(Instruction::ExploreSubject(MoveSubjectRef::TargetObject));
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    builder
                        .instructions
                        .push(Instruction::ExploreSubject(MoveSubjectRef::TargetObject));
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder.instructions.push(Instruction::ExploreSubject(
                        MoveSubjectRef::TriggeringObject,
                    ));
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                    builder
                        .instructions
                        .push(Instruction::ExploreSubject(MoveSubjectRef::AttachedHost));
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
        InstructionAst::Empower {
            planeswalker,
            count,
        } => {
            if !planeswalker.eq_ignore_ascii_case("jace") {
                return Err("oracle:unsupported-empower-planeswalker");
            }
            let resolved_count = resolve_x(count, bound_x);
            let expr =
                lower_value_expr_in_context(resolved_count, builder, in_trigger, vocabulary)?;
            builder
                .instructions
                .push(Instruction::EmpowerJace { count: expr });
            Ok(())
        }
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:player-counter-kind")?;
            if !ck.is_player_counter() {
                return Err("oracle:player-counter-kind");
            }
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            if matches!(player, PlayerRefAst::You)
                && ck == CounterKind::Experience
                && amt == ValueExpr::literal(1)
            {
                builder.instructions.push(Instruction::AddExperienceCounter);
                return Ok(());
            }
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::AddPlayerCounter {
                players,
                kind: ck,
                amount: amt,
            });
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
                    vec![Instruction::Blight {
                        player: ResolvingPlayer::LoopPlayer,
                        amount: amt,
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::Blight {
                player: rp,
                amount: amt,
            });
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
            builder.instructions.push(Instruction::Connive {
                subject: subj,
                count: amt,
            });
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
            if in_trigger && self_pronoun_valid {
                builder.bind_target(BodyTarget::SingleCreature)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: max,
                        action: PermanentTargetAction::PutPlusOneCounterOnOthers,
                    });
            } else if self_pronoun_valid {
                builder.bind_target(BodyTarget::FilteredPermanent(ObjectFilter::Clause(
                    ObjectClause {
                        source_relation: SourceRelation::OtherThanAbilitySource,
                        ..ObjectClause::creature()
                    },
                )))?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: max,
                        action: PermanentTargetAction::PutPlusOneCounter,
                    });
            } else {
                builder.bind_target(BodyTarget::SingleCreature)?;
                builder
                    .instructions
                    .push(Instruction::AffectPermanentTargets {
                        minimum: 0,
                        maximum: max,
                        action: PermanentTargetAction::PutPlusOneCounter,
                    });
            }
            Ok(())
        }
        InstructionAst::Endure { count } => {
            let amt = lower_value_expr_in_context(
                resolve_x(count, bound_x),
                builder,
                in_trigger,
                vocabulary,
            )?;
            let ValueExpr::Literal(lit_amt) = &amt else {
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
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::PutCountersOnSource {
                        amount: lit_amt.clone(),
                    }],
                ),
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
                if rp == ResolvingPlayer::Controller {
                    builder.instructions.push(Instruction::GainControllerLife {
                        amount: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    });
                } else {
                    builder.instructions.push(Instruction::AdjustPlayersLife {
                        players: AffectedPlayers::One(rp),
                        gain: true,
                        amount: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(rp)),
                    });
                }
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
                        .push(Instruction::DoubleCountersOnTargets);
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
                        .push(Instruction::DoubleCountersOnTargets);
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
                        .push(Instruction::DoubleCountersOnTargets);
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
                    SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
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
                    .push(Instruction::DoubleCountersOnSubject {
                        subject: Box::new(eff_subj),
                        kind: ck_filter,
                    });
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
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughness {
                            target_self: true,
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughness {
                            target_self: false,
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_created_tokens(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::CreatedTokens),
                            power,
                            toughness,
                            duration: dur,
                        });
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
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughness {
                            target_self: false,
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if last_instruction_moved_object(&builder.instructions) =>
                {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::LastMoved),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw))
                        && builder.target == BodyTarget::None =>
                {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::TriggerSubject),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) if self_pronoun_valid => {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::Matching(group_obj)),
                            power,
                            toughness,
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
                    if f.zone.is_none() =>
                {
                    let obj = lower_object_filter(f, vocabulary)?;
                    builder.last_group_filter = Some(obj);
                    builder
                        .instructions
                        .push(Instruction::DoublePowerToughnessSubject {
                            subject: Box::new(EffectSubject::Matching(obj)),
                            power,
                            toughness,
                            duration: dur,
                        });
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
            match subject.as_ref() {
                s if is_self_pronoun(s, builder, self_pronoun_valid) => {
                    builder.instructions.push(Instruction::ChangeAbilitySource {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                        duration: dur,
                    });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::UpTo(1),
                } if filter.zone.is_none() => {
                    let bt = lower_battlefield_body_target(filter, vocabulary)?;
                    builder.bind_target(bt)?;
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: 1,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
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
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
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
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: 0,
                            maximum: 1,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    ab_change,
                                )]),
                                duration: dur,
                            },
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    builder
                        .instructions
                        .push(Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab_change)]),
                            duration: dur,
                        });
                    Ok(())
                }
                _ => Err("oracle:unsupported-remove-keywords"),
            }
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
                trial.instructions.push(Instruction::ChooseCreatureType {
                    into: slot,
                    excluding: BTreeSet::new(),
                });
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
                Instruction::CreateContinuousEffect { parts, .. }
                | Instruction::ChangeAbilitySource { parts, .. }
                | Instruction::ChangeGroup { parts, .. }
                | Instruction::ModifyAnyNumberOfPermanents { parts, .. }
                | Instruction::AffectPermanentTargets {
                    action: PermanentTargetAction::Modify { parts, .. },
                    ..
                } => Some(&mut parts.0),
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
                .push(Instruction::DistributeCountersAmongTargets { kind: ck, total });
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
                .push(Instruction::Monstrosity { amount: amt });
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
            let kind = if kind_clean.is_empty()
                || matches!(
                    kind_clean.as_str(),
                    "any"
                        | "a"
                        | "an"
                        | "one"
                        | "one or more"
                        | "any kind"
                        | "of any kind"
                        | "any number of"
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
            let amount = if *all || *any_number {
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
                        ExecutableBody::new(from_bt, vec![Instruction::RememberPermanentTarget]),
                    )));
                trial
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(to_bt, vec![Instruction::RememberPermanentTarget]),
                    )));
                trial.instructions.push(Instruction::MoveCounters {
                    from: MoveSubjectRef::RememberedFirst,
                    to: MoveSubjectRef::RememberedSecond,
                    kind,
                    all: *all,
                    any_number: *any_number,
                    amount,
                });
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
            trial.instructions.push(Instruction::MoveCounters {
                from: from_ref,
                to: to_ref,
                kind,
                all: *all,
                any_number: *any_number,
                amount,
            });
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
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts,
                duration: dur,
            });
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
                .push(Instruction::CreateContinuousEffect {
                    parts,
                    duration: dur,
                });
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
                .push(Instruction::CreateContinuousEffect {
                    parts,
                    duration: dur,
                });
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
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: 1,
                    action: PermanentTargetAction::Modify {
                        parts,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(n),
        } if filter.zone.is_none() && *n >= 2 => {
            let bt =
                lower_battlefield_body_target_in_context(filter, builder, in_trigger, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: *n,
                    maximum: *n,
                    action: PermanentTargetAction::Modify {
                        parts,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::UpTo(max),
        } if filter.zone.is_none() && *max > 0 => {
            let bt =
                lower_battlefield_body_target_in_context(filter, builder, in_trigger, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: *max,
                    action: PermanentTargetAction::Modify {
                        parts,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: tc @ (TargetCountAst::ChosenX | TargetCountAst::UpToX),
        } if filter.zone.is_none() && !in_trigger => {
            let bt =
                lower_battlefield_body_target_in_context(filter, builder, in_trigger, vocabulary)?;
            builder.bind_target(bt)?;
            if *tc == TargetCountAst::UpToX {
                builder.up_to_x_target_count = true;
            } else {
                builder.x_target_count = true;
            }
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: u16::MAX,
                    action: PermanentTargetAction::Modify {
                        parts,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::AnyNumber,
        } if filter.zone.is_none() => {
            let obj = lower_object_filter_in_context(filter, builder, in_trigger, vocabulary)?;
            builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
            builder
                .instructions
                .push(Instruction::ModifyAnyNumberOfPermanents {
                    parts,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                    ObjectClause::any().attached_to_self(),
                ))),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if is_permanent_body_target(builder.target) => {
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if last_instruction_created_tokens(&builder.instructions) => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::CreatedTokens),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if last_instruction_moved_object(&builder.instructions) => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::LastMoved),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None
                && let Some(group_obj) = builder.last_group_filter =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(group_obj)),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(raw)
            if builder.target == BodyTarget::None
                && in_trigger
                && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts,
                duration: dur,
            });
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
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(obj)),
                parts,
                duration: dur,
            });
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
                        vec![Instruction::PairWithCreature { entrant_only }],
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
                            vec![Instruction::Attach(AttachAbility::Equip)],
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
                            vec![Instruction::PutCountersOnSource { amount: 1.into() }],
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
            trial.instructions.push(Instruction::ChooseCreatureType {
                into: slot,
                excluding: BTreeSet::new(),
            });
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
                Instruction::ChooseCreatureType { into, .. } => {
                    Some(ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Keep,
                        subtypes: SubtypeOperation::Set(InstructionSubtypeOperand::Chosen(*into)),
                    }))
                }
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
                                Instruction::ChangeAbilitySource {
                                    parts,
                                    duration: dur,
                                }
                            } else {
                                Instruction::CreateContinuousEffect {
                                    parts,
                                    duration: dur,
                                }
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
            Instruction::CreateContinuousEffect { parts, .. }
            | Instruction::ChangeAbilitySource { parts, .. }
            | Instruction::ChangeGroup { parts, .. }
            | Instruction::ModifyAnyNumberOfPermanents { parts, .. }
            | Instruction::AffectPermanentTargets {
                action: PermanentTargetAction::Modify { parts, .. },
                ..
            } => &mut parts.0,
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
