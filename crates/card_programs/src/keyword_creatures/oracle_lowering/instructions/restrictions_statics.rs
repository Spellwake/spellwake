use super::*;

pub(super) fn lower_restrictions_statics_instruction(
    ins: &InstructionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    match ins {
        InstructionAst::ExtraTurn {
            player: PlayerRefAst::You,
        } => {
            builder.instructions.push(Instruction::TakeExtraTurn);
            Ok(())
        }
        InstructionAst::EndTurn => {
            builder.instructions.push(Instruction::EndControllerTurn);
            Ok(())
        }
        InstructionAst::DelayedTrigger { step, scope, body } => {
            let own_step_only = match scope {
                DelayedStepScope::Next => false,
                DelayedStepScope::YourNext => true,
                DelayedStepScope::ThatPlayersNext
                | DelayedStepScope::Each
                | DelayedStepScope::EachPlayers
                | DelayedStepScope::EachOpponents
                | DelayedStepScope::EachOfYour => {
                    return Err("oracle:unsupported-delayed-scope");
                }
            };
            let eng_step = match step {
                TurnStepAst::Upkeep => Step::Upkeep,
                TurnStepAst::DrawStep => Step::Draw,
                TurnStepAst::PrecombatMain | TurnStepAst::MainPhase => Step::PrecombatMain,
                TurnStepAst::PostcombatMain => Step::PostcombatMain,
                TurnStepAst::BeginningOfCombat => Step::BeginningCombat,
                TurnStepAst::EndOfCombat => Step::EndCombat,
                TurnStepAst::EndStep => Step::End,
                TurnStepAst::UntapStep | TurnStepAst::CleanupStep => {
                    return Err("oracle:unsupported-delayed-step");
                }
            };
            let delayed_body = lower_effect_body(body, vocabulary, ids, true, self_pronoun_valid)?;
            if let Some(x_ast) = bound_x
                && body_uses_chosen_x(&delayed_body)
            {
                let x_val = lower_value_expr_in_context(x_ast, builder, in_trigger, vocabulary)?;
                builder
                    .instructions
                    .push(Instruction::BindResolutionX(x_val));
            }
            builder
                .instructions
                .push(Instruction::ScheduleDelayedStepTrigger {
                    step: eng_step,
                    own_step_only,
                    body: Box::new(delayed_body),
                });
            Ok(())
        }
        InstructionAst::ReflexiveTrigger(body) => {
            let reflexive_body =
                lower_effect_body(body, vocabulary, ids, true, self_pronoun_valid)?;
            builder
                .instructions
                .push(Instruction::EmitReflexiveTrigger {
                    body: Box::new(reflexive_body),
                });
            Ok(())
        }
        InstructionAst::TemporaryStatic {
            static_ability,
            duration,
        } => {
            match static_ability.as_ref() {
                StaticAbilityAst::NextSpellModifier { filter, effect } => {
                    let dur = lower_duration(duration)?;
                    let mut inner_f = filter.as_ref().clone();
                    inner_f.any_spell = false;
                    let flt_opt = if is_unrestricted_filter(&inner_f) {
                        None
                    } else {
                        Some(lower_object_filter(&inner_f, vocabulary)?)
                    };
                    let flt_obj = flt_opt.unwrap_or_else(ObjectFilter::any);
                    match effect.as_ref() {
                        StaticAbilityAst::GrantKeywords {
                            subject,
                            keywords,
                            condition: None,
                            paired_rule: None,
                        } if matches!(subject.as_ref(), SubjectRefAst::Group(inner) if inner == filter)
                            && !keywords.is_empty() =>
                        {
                            let mut rules = Vec::with_capacity(keywords.len());
                            for kw in keywords {
                                let rule = match kw.trim().to_ascii_lowercase().as_str() {
                                    "flash" => TemporaryRuleKind::FilteredFlash {
                                        player: ResolvingPlayer::Controller,
                                        filter: flt_obj,
                                        next_only: true,
                                    },
                                    "convoke" => TemporaryRuleKind::NextSpellKeyword {
                                        filter: flt_obj,
                                        keyword: KeywordAbility::Convoke,
                                    },
                                    "improvise" => TemporaryRuleKind::NextSpellKeyword {
                                        filter: flt_obj,
                                        keyword: KeywordAbility::Improvise,
                                    },
                                    "delve" => TemporaryRuleKind::NextSpellKeyword {
                                        filter: flt_obj,
                                        keyword: KeywordAbility::Delve,
                                    },
                                    "bargain" => TemporaryRuleKind::NextSpellKeyword {
                                        filter: flt_obj,
                                        keyword: KeywordAbility::Bargain,
                                    },
                                    "retrace" => TemporaryRuleKind::NextSpellKeyword {
                                        filter: flt_obj,
                                        keyword: KeywordAbility::Retrace,
                                    },
                                    "jump-start" | "jumpstart" => {
                                        TemporaryRuleKind::NextSpellKeyword {
                                            filter: flt_obj,
                                            keyword: KeywordAbility::JumpStart,
                                        }
                                    }
                                    "rebound" => TemporaryRuleKind::NextSpellKeyword {
                                        filter: flt_obj,
                                        keyword: KeywordAbility::Rebound,
                                    },
                                    _ => return Err("oracle:unsupported-temp-static"),
                                };
                                rules.push(rule);
                            }
                            for rule in rules {
                                builder.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(rule),
                                    duration: dur,
                                });
                            }
                            return Ok(());
                        }
                        StaticAbilityAst::PlayOrCastPermission {
                            permission,
                            condition: None,
                            ..
                        } => {
                            let p_low =
                                permission.trim().trim_end_matches('.').to_ascii_lowercase();
                            if matches!(
                                p_low.as_str(),
                                "can't be countered"
                                    | "cant_be_countered"
                                    | "next_spell_cant_be_countered"
                            ) {
                                builder.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::CantBeCountered {
                                        filter: flt_opt,
                                        controller_only: true,
                                        next_only: true,
                                    }),
                                    duration: dur,
                                });
                                return Ok(());
                            }
                            if matches!(
                                p_low.as_str(),
                                "flash"
                                    | "as though it had flash"
                                    | "as though they had flash"
                                    | "can be cast as though it had flash"
                                    | "cast_as_though_flash"
                            ) {
                                builder.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::FilteredFlash {
                                        player: ResolvingPlayer::Controller,
                                        filter: flt_obj,
                                        next_only: true,
                                    }),
                                    duration: dur,
                                });
                                return Ok(());
                            }
                            if matches!(
                                p_low.as_str(),
                                "without paying its mana cost"
                                    | "can be cast without paying its mana cost"
                            ) {
                                builder.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::NextSpellFree {
                                        filter: flt_opt,
                                    }),
                                    duration: dur,
                                });
                                return Ok(());
                            }
                        }
                        StaticAbilityAst::RuleOrCombatRestriction {
                            rule,
                            filter: None,
                            condition: None,
                            ..
                        } => {
                            let r_low = rule.trim().trim_end_matches('.').to_ascii_lowercase();
                            if matches!(
                                r_low.as_str(),
                                "can't be countered"
                                    | "cant_be_countered"
                                    | "next_spell_cant_be_countered"
                            ) {
                                builder.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::CantBeCountered {
                                        filter: flt_opt,
                                        controller_only: true,
                                        next_only: true,
                                    }),
                                    duration: dur,
                                });
                                return Ok(());
                            }
                        }
                        StaticAbilityAst::CostModifier {
                            delta,
                            less: true,
                            per,
                            scale_expr,
                            condition: None,
                            ..
                        } => {
                            let d_clean = delta.trim();
                            if per.is_none()
                                && scale_expr.is_none()
                                && let Some(generic) = parse_generic_cost_reduction_delta(d_clean)
                            {
                                builder.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::SpellCostReduction {
                                        filter: flt_obj,
                                        generic,
                                        next_only: true,
                                    }),
                                    duration: dur,
                                });
                                return Ok(());
                            }
                            if per.is_none()
                                && let Some(se) = scale_expr
                            {
                                let unit_opt = if matches!(d_clean, "{1}" | "1" | "{X}" | "X" | "x")
                                {
                                    Some(1i64)
                                } else {
                                    parse_generic_cost_reduction_delta(d_clean).map(i64::from)
                                };
                                if let Some(unit) = unit_opt {
                                    let base = lower_value_expr_in_context(
                                        resolve_x(se, bound_x),
                                        builder,
                                        in_trigger,
                                        vocabulary,
                                    )?;
                                    let amt = scale_value_expr(base, unit);
                                    builder.instructions.push(Instruction::CreateTemporaryRule {
                                        rule: Box::new(TemporaryRuleKind::SpellCostReductionExpr {
                                            filter: flt_obj,
                                            amount: Box::new(amt),
                                            next_only: true,
                                        }),
                                        duration: dur,
                                    });
                                    return Ok(());
                                }
                            }
                            if scale_expr.is_none()
                                && let Some(per_filter) = per
                                && let Some(unit) =
                                    parse_generic_cost_reduction_delta(d_clean).map(i64::from)
                                && let Ok(base_val) = (match per_filter.zone {
                                    None | Some(ZoneFilter::Battlefield) => {
                                        let mut pf = per_filter.as_ref().clone();
                                        pf.zone = None;
                                        lower_object_filter(&pf, vocabulary)
                                            .map(ValueExpr::count_battlefield)
                                    }
                                    Some(ZoneFilter::YourGraveyard) => {
                                        let mut pf = per_filter.as_ref().clone();
                                        pf.zone = None;
                                        lower_object_filter(&pf, vocabulary).map(|of| {
                                            ValueExpr::count_graveyard(
                                                ResolvingPlayer::Controller,
                                                of,
                                            )
                                        })
                                    }
                                    _ => Err("oracle:unsupported-per-zone"),
                                })
                            {
                                let amt = scale_value_expr(base_val, unit);
                                builder.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::SpellCostReductionExpr {
                                        filter: flt_obj,
                                        amount: Box::new(amt),
                                        next_only: true,
                                    }),
                                    duration: dur,
                                });
                                return Ok(());
                            }
                            if per.is_none()
                                && scale_expr.is_none()
                                && (matches!(d_clean, "{X}" | "X" | "x")
                                    || parse_mana_cost_with_x(d_clean).is_some_and(
                                        |(fixed, x_cnt)| x_cnt > 0 && fixed.required.is_empty(),
                                    ))
                            {
                                let (fixed_generic, x_cnt) =
                                    if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(d_clean) {
                                        (fixed.generic, x_cnt)
                                    } else {
                                        (0.into(), 1u8)
                                    };
                                let x_val = match bound_x {
                                    Some(expr) => lower_value_expr_in_context(
                                        expr, builder, in_trigger, vocabulary,
                                    )?,
                                    None => ValueExpr::chosen_x(),
                                };
                                let mut amt = scale_value_expr(x_val, i64::from(x_cnt));
                                if fixed_generic.is_positive() {
                                    amt = ValueExpr::Add(
                                        Box::new(amt),
                                        Box::new(ValueExpr::Literal(fixed_generic)),
                                    );
                                }
                                builder.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::SpellCostReductionExpr {
                                        filter: flt_obj,
                                        amount: Box::new(amt),
                                        next_only: true,
                                    }),
                                    duration: dur,
                                });
                                return Ok(());
                            }
                        }
                        StaticAbilityAst::AlternativeCastCost {
                            cost,
                            condition: None,
                            ..
                        } if cost.is_empty()
                            || matches!(
                                cost.as_slice(),
                                [CostAtomAst::PayMana(raw)] if raw.trim() == "{0}" || raw.trim() == "0"
                            ) =>
                        {
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::NextSpellFree {
                                    filter: flt_opt,
                                }),
                                duration: dur,
                            });
                            return Ok(());
                        }
                        _ => {}
                    }
                    return Err("oracle:unsupported-temp-static");
                }
                StaticAbilityAst::Compound(items) if !items.is_empty() => {
                    for item in items {
                        lower_instruction(
                            &InstructionAst::TemporaryStatic {
                                static_ability: Box::new(item.clone()),
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
                StaticAbilityAst::ContinuousModifyPT {
                    subject,
                    dx,
                    dy,
                    grant_keywords,
                    grant_ability_text,
                    scale,
                    condition: None,
                    paired_rule,
                } => {
                    let mut seq = vec![InstructionAst::ModifyPT {
                        subject: subject.clone(),
                        dx: dx.clone(),
                        dy: dy.clone(),
                        duration: duration.clone(),
                        scale: scale.clone(),
                    }];
                    if !grant_keywords.is_empty() {
                        seq.push(InstructionAst::GrantKeywords {
                            subject: subject.clone(),
                            keywords: grant_keywords.clone(),
                            duration: duration.clone(),
                        });
                    }
                    if let Some(raw_text) = grant_ability_text {
                        let cleaned = raw_text.trim().trim_matches('"').trim();
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        let synthetic = compile_card(
                            &CardInput {
                                oracle_id: "",
                                name: "Synthetic",
                                layout: "normal",
                                faces: vec![FaceInput {
                                    name: "Synthetic",
                                    type_line: "Creature",
                                    oracle_text: cleaned,
                                }],
                            },
                            &compiler_vocab,
                        );
                        let compiled = if synthetic.strictly_compiled && synthetic.faces.len() == 1
                        {
                            Some(Box::new(synthetic.faces[0].clone()))
                        } else {
                            None
                        };
                        seq.push(InstructionAst::GrantAbilityText {
                            subject: subject.clone(),
                            ability_text: raw_text.clone(),
                            compiled,
                            duration: duration.clone(),
                        });
                    }
                    if let Some(paired) = paired_rule {
                        seq.push(InstructionAst::TemporaryStatic {
                            static_ability: paired.clone(),
                            duration: duration.clone(),
                        });
                    }
                    return lower_instruction_seq(
                        &seq,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
                StaticAbilityAst::GrantKeywords {
                    subject,
                    keywords,
                    condition: None,
                    paired_rule,
                } => {
                    if paired_rule.is_none()
                        && keywords.len() == 1
                        && keywords[0].trim().eq_ignore_ascii_case("flash")
                        && let SubjectRefAst::Group(f) = subject.as_ref()
                        && f.zone.is_none()
                        && (f.any_spell
                            || (f.any_types.0
                                & (AstCardTypeMask::INSTANT.0 | AstCardTypeMask::SORCERY.0))
                                != 0)
                    {
                        let dur = lower_duration(duration)?;
                        let mut inner = f.as_ref().clone();
                        inner.any_spell = false;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::FilteredFlash {
                                player: ResolvingPlayer::Controller,
                                filter: obj,
                                next_only: true,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if paired_rule.is_none()
                        && !keywords.is_empty()
                        && matches!(
                            subject.as_ref(),
                            SubjectRefAst::Player(_)
                                | SubjectRefAst::TargetPlayer {
                                    count: TargetCountAst::Exact(1),
                                    ..
                                }
                        )
                        && let Ok(ab) = lower_ability_change(keywords, vocabulary)
                        && ab.removal.is_none()
                        && ab.suppressed_keywords.is_empty()
                        && ab.suppressed_activations.is_none()
                        && !ab.doesnt_untap
                        && ab.granted_exalted == 0
                        && !ab.grants_unblockable
                        && ab.granted_triggers.is_empty()
                        && ab.granted_toxic.is_none()
                        && ab.granted_combat_keywords.is_empty()
                        && !ab.granted_keywords.is_empty()
                        && ab.granted_keywords.iter().all(|kw| {
                            matches!(
                                kw,
                                KeywordAbility::Hexproof
                                    | KeywordAbility::Shroud
                                    | KeywordAbility::HexproofFrom(_)
                                    | KeywordAbility::ProtectionFromWhite
                                    | KeywordAbility::ProtectionFromBlue
                                    | KeywordAbility::ProtectionFromBlack
                                    | KeywordAbility::ProtectionFromRed
                                    | KeywordAbility::ProtectionFromGreen
                                    | KeywordAbility::ProtectionFromMulticolored
                                    | KeywordAbility::ProtectionFromSubtype(_)
                                    | KeywordAbility::ProtectionFrom(_)
                            )
                        })
                    {
                        let dur = lower_duration(duration)?;
                        let players = match subject.as_ref() {
                            SubjectRefAst::Player(p) => {
                                lower_affected_players_in_context(*p, builder, in_trigger)?
                            }
                            SubjectRefAst::TargetPlayer {
                                opponent_only,
                                count: TargetCountAst::Exact(1),
                            } => {
                                builder.bind_target(if *opponent_only {
                                    BodyTarget::SingleOpponent
                                } else {
                                    BodyTarget::SinglePlayer
                                })?;
                                AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                            }
                            _ => unreachable!(),
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::PlayerGrantKeyword {
                                players,
                                keywords: ab.granted_keywords,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    let mut seq = vec![InstructionAst::GrantKeywords {
                        subject: subject.clone(),
                        keywords: keywords.clone(),
                        duration: duration.clone(),
                    }];
                    if let Some(paired) = paired_rule {
                        seq.push(InstructionAst::TemporaryStatic {
                            static_ability: paired.clone(),
                            duration: duration.clone(),
                        });
                    }
                    let mut trial = builder.clone();
                    if lower_instruction_seq(
                        &seq,
                        &mut trial,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )
                    .is_ok()
                    {
                        *builder = trial;
                        return Ok(());
                    }
                }
                StaticAbilityAst::ContinuousLayerEffect {
                    subject,
                    set_types,
                    additive_types,
                    base_pt,
                    set_colors,
                    grant_keywords,
                    lose_keywords,
                    lose_all_abilities,
                    condition: None,
                } => {
                    let dur = lower_duration(duration)?;
                    let parts = if set_types.as_deref() == Some("lose_all_creature_types")
                        && !*additive_types
                    {
                        let mut out = vec![ResolvedPartSpec::Type(TypeChangeSpec {
                            supertypes: TypeOperation::Keep,
                            types: TypeOperation::Keep,
                            subtypes: SubtypeOperation::Set(InstructionSubtypeOperand::Literal(
                                BTreeSet::new(),
                            )),
                            land_subtypes: SubtypeOperation::Keep,
                        })];
                        if base_pt.is_some()
                            || set_colors.is_some()
                            || !grant_keywords.is_empty()
                            || !lose_keywords.is_empty()
                            || *lose_all_abilities
                        {
                            out.extend(lower_continuous_layer_effect_parts(
                                None,
                                false,
                                base_pt.as_ref(),
                                set_colors.as_deref(),
                                grant_keywords,
                                lose_keywords,
                                *lose_all_abilities,
                                vocabulary,
                                bound_x,
                                builder,
                                in_trigger,
                            )?);
                        }
                        out
                    } else {
                        lower_continuous_layer_effect_parts(
                            set_types.as_deref(),
                            *additive_types,
                            base_pt.as_ref(),
                            set_colors.as_deref(),
                            grant_keywords,
                            lose_keywords,
                            *lose_all_abilities,
                            vocabulary,
                            bound_x,
                            builder,
                            in_trigger,
                        )?
                    };
                    return emit_resolved_parts_for_subject(
                        subject,
                        ResolvedPartsSpec(parts),
                        dur,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    );
                }
                StaticAbilityAst::CantBeBlockedExceptByAny {
                    subject,
                    allowed_filters,
                    condition: None,
                } if !allowed_filters.is_empty() => {
                    let dur = lower_duration(duration)?;
                    let mut clauses = Vec::with_capacity(allowed_filters.len());
                    let mut ok = true;
                    for flt in allowed_filters {
                        if let Ok(obj) = lower_object_filter(flt, vocabulary)
                            && !obj.has_target_player()
                        {
                            match obj {
                                ObjectFilter::Clause(c) => clauses.push(c),
                                ObjectFilter::Either(a, b) => {
                                    clauses.push(a);
                                    clauses.push(b);
                                }
                            }
                        } else {
                            ok = false;
                            break;
                        }
                    }
                    if ok {
                        let combined = match clauses.as_slice() {
                            [only] => Some(ObjectFilter::Clause(*only)),
                            [a, b] => Some(ObjectFilter::Either(*a, *b)),
                            _ => None,
                        };
                        if let Some(obj_flt) = combined {
                            let parts =
                                ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(
                                    BlockingRestriction::OnlyBlockedByMatching(obj_flt),
                                )]);
                            let mut trial = builder.clone();
                            if emit_resolved_parts_for_subject(
                                subject,
                                parts,
                                dur,
                                &mut trial,
                                vocabulary,
                                in_trigger,
                                self_pronoun_valid,
                            )
                            .is_ok()
                            {
                                *builder = trial;
                                return Ok(());
                            }
                        }
                    }
                }
                StaticAbilityAst::SetOrAddType {
                    subject,
                    type_or_subtype,
                    additive,
                    condition: None,
                } => {
                    return lower_animate_or_set_pt(
                        subject,
                        "",
                        "",
                        Some(type_or_subtype),
                        &[],
                        duration,
                        *additive,
                        builder,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
                StaticAbilityAst::PreventAllDamageStatic {
                    combat_only: true,
                    noncombat_only: false,
                    ..
                } => {
                    let dur = lower_duration(duration)?;
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::PreventAllCombatDamage),
                        duration: dur,
                    });
                    return Ok(());
                }
                StaticAbilityAst::PlayOrCastPermission {
                    permission,
                    filter: _,
                    cast_only: false,
                    condition: None,
                } if {
                    let p = permission.trim().trim_end_matches('.').to_ascii_lowercase();
                    p.contains("additional land") && p.contains("this turn")
                } =>
                {
                    let dur = lower_duration(duration)?;
                    let p = permission.trim().to_ascii_lowercase();
                    let count = if p.contains("two additional") {
                        2
                    } else if p.contains("three additional") {
                        3
                    } else {
                        1
                    };
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::AdditionalLandPlay {
                            player: ResolvingPlayer::Controller,
                            count,
                        }),
                        duration: dur,
                    });
                    return Ok(());
                }
                StaticAbilityAst::GrantAbilityText {
                    subject,
                    ability_text,
                    compiled,
                    condition: None,
                } => {
                    return lower_instruction(
                        &InstructionAst::GrantAbilityText {
                            subject: subject.clone(),
                            ability_text: ability_text.clone(),
                            compiled: compiled.clone(),
                            duration: duration.clone(),
                        },
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
                StaticAbilityAst::FlashbackCostModifier { cost, delta }
                    if cost == "mana_cost" && matches!(delta.trim(), "{0}" | "0" | "") =>
                {
                    let has_flashback_grant = builder.instructions.iter().any(|op| {
                        matches!(op, Instruction::GrantFlashback(_))
                            || matches!(
                                op,
                                Instruction::CreateTemporaryRule { rule, .. }
                                    if matches!(
                                        rule.as_ref(),
                                        TemporaryRuleKind::ZoneCastPermission {
                                            zone: SourceZoneFilter::Graveyard,
                                            free: false,
                                            ..
                                        }
                                    )
                            )
                    });
                    if has_flashback_grant {
                        return Ok(());
                    }
                }
                StaticAbilityAst::CostModifier {
                    subject,
                    delta,
                    less: false,
                    next_only: false,
                    per: None,
                    scale_expr: None,
                    condition: None,
                } => {
                    let mut dur = lower_duration(duration)?;
                    let filter_opt = match subject.as_ref() {
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f) => {
                            let players = match f.controller {
                                Some(
                                    ControllerRelation::Opponent | ControllerRelation::Opponents,
                                ) => AffectedPlayers::EachOpponent,
                                Some(ControllerRelation::You) => {
                                    AffectedPlayers::One(ResolvingPlayer::Controller)
                                }
                                Some(ControllerRelation::ThatPlayer) => {
                                    lower_affected_players_in_context(
                                        PlayerRefAst::ThatPlayer,
                                        builder,
                                        in_trigger,
                                    )?
                                }
                                Some(ControllerRelation::TargetPlayer) => {
                                    builder.bind_target(BodyTarget::SinglePlayer)?;
                                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                                }
                                Some(ControllerRelation::TargetOpponent) => {
                                    builder.bind_target(BodyTarget::SingleOpponent)?;
                                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                                }
                                None => AffectedPlayers::EachPlayer,
                                _ => return Err("oracle:unsupported-temp-static"),
                            };
                            let mut adj = f.as_ref().clone();
                            adj.controller = None;
                            Some((players, lower_object_filter(&adj, vocabulary)?))
                        }
                        SubjectRefAst::Anaphora(raw) => {
                            let raw_low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
                            let parsed_scope = raw_low
                                .strip_suffix(" they cast this turn")
                                .map(|s| (s.trim(), PlayerRefAst::ThatPlayer, true))
                                .or_else(|| {
                                    raw_low
                                        .strip_suffix(" that player casts this turn")
                                        .map(|s| (s.trim(), PlayerRefAst::ThatPlayer, true))
                                })
                                .or_else(|| {
                                    raw_low
                                        .strip_suffix(" you cast this turn")
                                        .map(|s| (s.trim(), PlayerRefAst::You, true))
                                })
                                .or_else(|| {
                                    raw_low
                                        .strip_suffix(" your opponents cast this turn")
                                        .map(|s| (s.trim(), PlayerRefAst::EachOpponent, true))
                                })
                                .or_else(|| {
                                    raw_low
                                        .strip_suffix(" opponents cast this turn")
                                        .map(|s| (s.trim(), PlayerRefAst::EachOpponent, true))
                                })
                                .or_else(|| {
                                    (dur == ResolvedDuration::UntilEndOfTurn)
                                        .then(|| {
                                            raw_low
                                                .strip_suffix(" they cast")
                                                .map(|s| {
                                                    (s.trim(), PlayerRefAst::ThatPlayer, false)
                                                })
                                                .or_else(|| {
                                                    raw_low.strip_suffix(" that player casts").map(
                                                        |s| {
                                                            (
                                                                s.trim(),
                                                                PlayerRefAst::ThatPlayer,
                                                                false,
                                                            )
                                                        },
                                                    )
                                                })
                                                .or_else(|| {
                                                    raw_low.strip_suffix(" you cast").map(|s| {
                                                        (s.trim(), PlayerRefAst::You, false)
                                                    })
                                                })
                                        })
                                        .flatten()
                                });
                            if let Some((stem, p_ref, had_this_turn)) = parsed_scope
                                && matches!(
                                    dur,
                                    ResolvedDuration::NoStatedDuration
                                        | ResolvedDuration::UntilEndOfTurn
                                )
                                && !stem.is_empty()
                                && !stem.starts_with("the next ")
                                && !stem.starts_with("the first ")
                                && !stem.starts_with("other ")
                                && !stem.contains(" from ")
                                && !stem.contains(" during ")
                                && !stem.contains(" while ")
                            {
                                let obj_opt = if matches!(stem, "spells" | "spell") {
                                    Some(ObjectFilter::any())
                                } else {
                                    let vocab_ref = cached_compiler_vocab(vocabulary);
                                    let parsed_ast =
                                        spellwake_oracle_compiler::filter::parse_object_filter(
                                            stem, &vocab_ref,
                                        )
                                        .or_else(|_| {
                                            let no_spells = stem
                                                .strip_suffix(" spells")
                                                .or_else(|| stem.strip_suffix(" spell"))
                                                .unwrap_or(stem)
                                                .trim();
                                            spellwake_oracle_compiler::filter::parse_object_filter(
                                                no_spells, &vocab_ref,
                                            )
                                        });
                                    if let Ok(mut ast_f) = parsed_ast
                                        && ast_f.zone.is_none()
                                        && ast_f.predicates.is_empty()
                                        && !ast_f.is_ability
                                        && ast_f.named.is_none()
                                        && ast_f.controller.is_none()
                                    {
                                        ast_f.any_spell = false;
                                        lower_object_filter(&ast_f, vocabulary)
                                            .ok()
                                            .filter(|o| !o.has_target_player())
                                    } else {
                                        None
                                    }
                                };
                                if let Some(obj) = obj_opt {
                                    let players = lower_affected_players_in_context(
                                        p_ref, builder, in_trigger,
                                    )?;
                                    if had_this_turn && dur == ResolvedDuration::NoStatedDuration {
                                        dur = ResolvedDuration::UntilEndOfTurn;
                                    }
                                    Some((players, obj))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some((players, obj)) = filter_opt
                        && let Some((mc, 0, hybrids)) = parse_mana_cost_full(delta)
                        && hybrids.is_empty()
                        && mc.required.is_empty()
                        && let Some(generic) =
                            mc.generic.to_usize().and_then(|v| u16::try_from(v).ok())
                        && generic > 0
                    {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::SpellCostIncrease {
                                players,
                                filter: obj,
                                generic,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                }
                StaticAbilityAst::CostModifier {
                    subject,
                    delta,
                    less: true,
                    next_only,
                    per,
                    scale_expr,
                    condition: None,
                } => {
                    let mut dur = lower_duration(duration)?;
                    let filter_opt = match subject.as_ref() {
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f) => Some(lower_object_filter(f, vocabulary)?),
                        SubjectRefAst::Anaphora(raw) => {
                            let raw_low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
                            let color_clause = raw_low
                                .strip_prefix("spells you cast this turn that are ")
                                .map(|s| (s.trim(), true))
                                .or_else(|| {
                                    (dur == ResolvedDuration::UntilEndOfTurn)
                                        .then(|| raw_low.strip_prefix("spells you cast that are "))
                                        .flatten()
                                        .map(|s| (s.trim(), false))
                                });
                            if let Some((colors_raw, had_this_turn)) = color_clause
                                && matches!(
                                    dur,
                                    ResolvedDuration::NoStatedDuration
                                        | ResolvedDuration::UntilEndOfTurn
                                )
                            {
                                let mut mask = ColorMask::default();
                                let mut all_colors_ok = true;
                                for tok in colors_raw
                                    .replace(" and/or ", " ")
                                    .replace(" or ", " ")
                                    .replace(',', " ")
                                    .split_whitespace()
                                {
                                    match tok {
                                        "white" => mask = mask.union(ColorMask::WHITE),
                                        "blue" => mask = mask.union(ColorMask::BLUE),
                                        "black" => mask = mask.union(ColorMask::BLACK),
                                        "red" => mask = mask.union(ColorMask::RED),
                                        "green" => mask = mask.union(ColorMask::GREEN),
                                        _ => {
                                            all_colors_ok = false;
                                            break;
                                        }
                                    }
                                }
                                if all_colors_ok && !mask.is_empty() {
                                    if had_this_turn && dur == ResolvedDuration::NoStatedDuration {
                                        dur = ResolvedDuration::UntilEndOfTurn;
                                    }
                                    Some(ObjectFilter::Clause(ObjectClause {
                                        color: ObjectColorFilter::any_of(mask),
                                        ..ObjectClause::any()
                                    }))
                                } else {
                                    None
                                }
                            } else {
                                let stripped = raw_low
                                    .strip_suffix(" you cast this turn")
                                    .map(|s| (s.trim(), true))
                                    .or_else(|| {
                                        (dur == ResolvedDuration::UntilEndOfTurn)
                                            .then(|| raw_low.strip_suffix(" you cast"))
                                            .flatten()
                                            .map(|s| (s.trim(), false))
                                    });
                                if let Some((stem, had_this_turn)) = stripped
                                    && matches!(
                                        dur,
                                        ResolvedDuration::NoStatedDuration
                                            | ResolvedDuration::UntilEndOfTurn
                                    )
                                    && !stem.is_empty()
                                    && !stem.starts_with("the next ")
                                    && !stem.starts_with("the first ")
                                    && !stem.starts_with("other ")
                                    && !stem.contains(" from ")
                                    && !stem.contains(" during ")
                                    && !stem.contains(" while ")
                                    && !stem.contains(" cast ")
                                {
                                    let vocab_ref = cached_compiler_vocab(vocabulary);
                                    let parsed_ast =
                                        spellwake_oracle_compiler::filter::parse_object_filter(
                                            stem, &vocab_ref,
                                        )
                                        .or_else(|_| {
                                            let no_spells = stem
                                                .strip_suffix(" spells")
                                                .or_else(|| stem.strip_suffix(" spell"))
                                                .unwrap_or(stem)
                                                .trim();
                                            spellwake_oracle_compiler::filter::parse_object_filter(
                                                no_spells, &vocab_ref,
                                            )
                                        });
                                    if let Ok(mut ast_f) = parsed_ast
                                        && ast_f.zone.is_none()
                                        && ast_f.predicates.is_empty()
                                        && !ast_f.is_ability
                                        && ast_f.named.is_none()
                                        && matches!(
                                            ast_f.controller,
                                            None | Some(ControllerRelation::You)
                                        )
                                    {
                                        ast_f.controller = None;
                                        if let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
                                            && !obj.has_target_player()
                                        {
                                            if had_this_turn
                                                && dur == ResolvedDuration::NoStatedDuration
                                            {
                                                dur = ResolvedDuration::UntilEndOfTurn;
                                            }
                                            Some(obj)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            }
                        }
                        _ => None,
                    };
                    let next_only = *next_only;
                    if let Some(obj) = filter_opt {
                        if per.is_none()
                            && scale_expr.is_none()
                            && let Some((mc, 0, hybrids)) = parse_mana_cost_full(delta)
                            && hybrids.is_empty()
                            && mc.required.is_empty()
                            && let Some(generic) =
                                mc.generic.to_usize().and_then(|v| u16::try_from(v).ok())
                            && generic > 0
                        {
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::SpellCostReduction {
                                    filter: obj,
                                    generic,
                                    next_only,
                                }),
                                duration: dur,
                            });
                            return Ok(());
                        }
                        if let Some(scale) = scale_expr {
                            let d_clean = delta.trim();
                            let unit_opt = if matches!(d_clean, "{1}" | "1" | "{X}" | "X" | "x") {
                                Some(1i64)
                            } else if let Some((mc, 0, hybrids)) = parse_mana_cost_full(delta)
                                && hybrids.is_empty()
                                && mc.required.is_empty()
                            {
                                mc.generic
                                    .to_usize()
                                    .and_then(|v| i64::try_from(v).ok())
                                    .filter(|&u| u > 0)
                            } else {
                                None
                            };
                            if let Some(unit) = unit_opt {
                                let base_val = lower_value_expr_in_context(
                                    resolve_x(scale, bound_x),
                                    builder,
                                    in_trigger,
                                    vocabulary,
                                )?;
                                let amount = scale_value_expr(base_val, unit);
                                builder.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::SpellCostReductionExpr {
                                        filter: obj,
                                        amount: Box::new(amount),
                                        next_only,
                                    }),
                                    duration: dur,
                                });
                                return Ok(());
                            }
                        }
                        if let Some(per_filter) = per
                            && let Some((mc, 0, hybrids)) = parse_mana_cost_full(delta)
                            && hybrids.is_empty()
                            && mc.required.is_empty()
                            && let Some(unit) =
                                mc.generic.to_usize().and_then(|v| i64::try_from(v).ok())
                            && unit > 0
                            && let Ok(base_val) = (match per_filter.zone {
                                None | Some(ZoneFilter::Battlefield) => {
                                    let mut pf = per_filter.as_ref().clone();
                                    pf.zone = None;
                                    lower_object_filter(&pf, vocabulary)
                                        .map(ValueExpr::count_battlefield)
                                }
                                Some(ZoneFilter::YourGraveyard) => {
                                    let mut pf = per_filter.as_ref().clone();
                                    pf.zone = None;
                                    lower_object_filter(&pf, vocabulary).map(|of| {
                                        ValueExpr::count_graveyard(ResolvingPlayer::Controller, of)
                                    })
                                }
                                _ => Err("oracle:unsupported-per-zone"),
                            })
                        {
                            let amount = scale_value_expr(base_val, unit);
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::SpellCostReductionExpr {
                                    filter: obj,
                                    amount: Box::new(amount),
                                    next_only,
                                }),
                                duration: dur,
                            });
                            return Ok(());
                        }
                        if per.is_none()
                            && scale_expr.is_none()
                            && let Some((fixed, x_cnt)) = parse_mana_cost_with_x(delta)
                            && x_cnt > 0
                            && fixed.required.is_empty()
                        {
                            let x_val = match bound_x {
                                Some(expr) => lower_value_expr_in_context(
                                    expr, builder, in_trigger, vocabulary,
                                )?,
                                None => ValueExpr::chosen_x(),
                            };
                            let mut amount = scale_value_expr(x_val, i64::from(x_cnt));
                            if fixed.generic.is_positive() {
                                amount = ValueExpr::Add(
                                    Box::new(amount),
                                    Box::new(ValueExpr::Literal(fixed.generic)),
                                );
                            }
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::SpellCostReductionExpr {
                                    filter: obj,
                                    amount: Box::new(amount),
                                    next_only,
                                }),
                                duration: dur,
                            });
                            return Ok(());
                        }
                    }
                }
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject,
                    rule,
                    filter,
                    condition: None,
                } => {
                    let dur = lower_duration(duration)?;
                    let rule_low = rule.trim().to_ascii_lowercase();
                    if let Some((is_your_spells, by_raw)) = rule_low
                        .strip_prefix("spells you control can't be countered by ")
                        .map(|r| (true, r))
                        .or_else(|| {
                            rule_low
                                .strip_prefix("can't be countered by ")
                                .map(|r| (false, r))
                        })
                    {
                        let (by_body, eff_dur) = strip_trailing_temp_duration(by_raw, dur)
                            .unwrap_or((by_raw.trim().trim_end_matches('.').to_owned(), dur));
                        if let Some((colors, true)) = parse_cant_be_target_colors(&by_body) {
                            let (flt, controller_only) = if is_your_spells {
                                (None, true)
                            } else if let Some(f) = filter.as_deref() {
                                let mut inner = f.clone();
                                let ctrl_only = match inner.controller {
                                    Some(ControllerRelation::You) => {
                                        inner.controller = None;
                                        true
                                    }
                                    None => false,
                                    _ => return Err("oracle:unsupported-temp-static"),
                                };
                                inner.any_spell = false;
                                let flt_opt = if is_unrestricted_filter(&inner) {
                                    None
                                } else {
                                    Some(lower_object_filter(&inner, vocabulary)?)
                                };
                                (flt_opt, ctrl_only)
                            } else if let Some(
                                SubjectRefAst::All(f)
                                | SubjectRefAst::Each(f)
                                | SubjectRefAst::Group(f),
                            ) = subject.as_deref()
                                && f.zone.is_none()
                            {
                                let mut inner = f.as_ref().clone();
                                let ctrl_only = match inner.controller {
                                    Some(ControllerRelation::You) => {
                                        inner.controller = None;
                                        true
                                    }
                                    None => false,
                                    _ => return Err("oracle:unsupported-temp-static"),
                                };
                                inner.any_spell = false;
                                let flt_opt = if is_unrestricted_filter(&inner) {
                                    None
                                } else {
                                    Some(lower_object_filter(&inner, vocabulary)?)
                                };
                                (flt_opt, ctrl_only)
                            } else {
                                return Err("oracle:unsupported-temp-static");
                            };
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::CantBeCounteredByColors {
                                    colors,
                                    filter: flt,
                                    controller_only,
                                    next_only: false,
                                }),
                                duration: eff_dur,
                            });
                            return Ok(());
                        }
                    }
                    if (rule_low.contains("can't be countered") && !rule_low.contains(" by "))
                        || rule_low == "cant_be_countered"
                    {
                        let (flt, controller_only) = if let Some(f) = filter.as_deref() {
                            let mut inner = f.clone();
                            inner.any_spell = false;
                            let flt_opt = if is_unrestricted_filter(&inner) {
                                None
                            } else {
                                Some(lower_object_filter(&inner, vocabulary)?)
                            };
                            (flt_opt, true)
                        } else if let Some(
                            SubjectRefAst::All(f)
                            | SubjectRefAst::Each(f)
                            | SubjectRefAst::Group(f),
                        ) = subject.as_deref()
                            && f.zone.is_none()
                        {
                            let mut inner = f.as_ref().clone();
                            let ctrl_only = match inner.controller {
                                Some(ControllerRelation::You) => {
                                    inner.controller = None;
                                    true
                                }
                                None => false,
                                _ => return Err("oracle:unsupported-temp-static"),
                            };
                            inner.any_spell = false;
                            let flt_opt = if is_unrestricted_filter(&inner) {
                                None
                            } else {
                                Some(lower_object_filter(&inner, vocabulary)?)
                            };
                            (flt_opt, ctrl_only)
                        } else {
                            (None, true)
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::CantBeCountered {
                                filter: flt,
                                controller_only,
                                next_only: rule_low.contains("next "),
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if matches!(
                        rule_low.as_str(),
                        "cant_play_lands" | "can't play lands" | "cantplaylands"
                    ) {
                        let players = match subject.as_deref() {
                            Some(SubjectRefAst::Player(p)) => {
                                lower_affected_players_in_context(*p, builder, in_trigger)?
                            }
                            Some(SubjectRefAst::TargetPlayer {
                                opponent_only,
                                count: TargetCountAst::Exact(1),
                            }) => {
                                builder.bind_target(if *opponent_only {
                                    BodyTarget::SingleOpponent
                                } else {
                                    BodyTarget::SinglePlayer
                                })?;
                                AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                            }
                            None => AffectedPlayers::EachPlayer,
                            _ => return Err("oracle:unsupported-temp-static"),
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::CantPlayLands { players }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if let Some((rule_body, eff_dur)) = strip_trailing_temp_duration(&rule_low, dur)
                    {
                        if let Some(nonmana_only) = parse_player_cant_activate_rule(&rule_body)
                            .or_else(|| {
                                matches!(
                                    rule_body.as_str(),
                                    "cant_activate_non:mana abilities" | "cant_activate_non:mana"
                                )
                                .then_some(true)
                            })
                            && matches!(
                                subject.as_deref(),
                                Some(
                                    SubjectRefAst::Player(_)
                                        | SubjectRefAst::TargetPlayer {
                                            count: TargetCountAst::Exact(1),
                                            ..
                                        }
                                )
                            )
                        {
                            let players = match subject.as_deref() {
                                Some(SubjectRefAst::Player(p)) => {
                                    lower_affected_players_in_context(*p, builder, in_trigger)?
                                }
                                Some(SubjectRefAst::TargetPlayer {
                                    opponent_only,
                                    count: TargetCountAst::Exact(1),
                                }) => {
                                    builder.bind_target(if *opponent_only {
                                        BodyTarget::SingleOpponent
                                    } else {
                                        BodyTarget::SinglePlayer
                                    })?;
                                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                                }
                                _ => return Err("oracle:unsupported-temp-static"),
                            };
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::CantActivateAbilities {
                                    players,
                                    nonmana_only,
                                }),
                                duration: eff_dur,
                            });
                            return Ok(());
                        }
                        if filter.is_none()
                            && let Some(subj) = subject.as_deref()
                            && let Some(lock_parts) = parse_temp_permanent_lock_parts(&rule_body)
                        {
                            if eff_dur.requires_ability_source()
                                && !(in_trigger || self_pronoun_valid)
                            {
                                return Err("oracle:unsupported-temp-static");
                            }
                            return emit_resolved_parts_for_subject(
                                subj,
                                ResolvedPartsSpec(lock_parts),
                                eff_dur,
                                builder,
                                vocabulary,
                                in_trigger,
                                self_pronoun_valid,
                            );
                        }
                    }
                    if rule_low.contains("can't gain life") || rule_low == "cant_gain_life" {
                        let players = match subject.as_deref() {
                            Some(SubjectRefAst::Player(p)) => {
                                lower_affected_players_in_context(*p, builder, in_trigger)?
                            }
                            Some(SubjectRefAst::TargetPlayer {
                                opponent_only,
                                count: TargetCountAst::Exact(1),
                            }) => {
                                builder.bind_target(if *opponent_only {
                                    BodyTarget::SingleOpponent
                                } else {
                                    BodyTarget::SinglePlayer
                                })?;
                                AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                            }
                            None => AffectedPlayers::EachPlayer,
                            _ => return Err("oracle:unsupported-temp-static"),
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::PlayerCantGainLife { players }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if matches!(
                        rule_low.as_str(),
                        "players can't lose life"
                            | "players can't lose life this turn"
                            | "cant_lose_life"
                            | "can't lose life"
                            | "you can't lose life"
                            | "you can't lose life this turn"
                    ) {
                        let players = if rule_low.starts_with("players ") {
                            AffectedPlayers::EachPlayer
                        } else if rule_low.starts_with("you ") {
                            AffectedPlayers::One(ResolvingPlayer::Controller)
                        } else {
                            match subject.as_deref() {
                                Some(SubjectRefAst::Player(p)) => {
                                    lower_affected_players_in_context(*p, builder, in_trigger)?
                                }
                                Some(SubjectRefAst::TargetPlayer {
                                    opponent_only,
                                    count: TargetCountAst::Exact(1),
                                }) => {
                                    builder.bind_target(if *opponent_only {
                                        BodyTarget::SingleOpponent
                                    } else {
                                        BodyTarget::SinglePlayer
                                    })?;
                                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                                }
                                None => AffectedPlayers::EachPlayer,
                                _ => return Err("oracle:unsupported-temp-static"),
                            }
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                                StaticContinuousAbility::EventReplacement {
                                    id: ids.static_id(),
                                    event: ReplaceableEvent::LifeLoss { player: players },
                                    condition: None,
                                    action: StaticReplacementAction::Skip,
                                },
                            ))),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if matches!(
                        rule_low.as_str(),
                        "you can't lose the game and your opponents can't win the game"
                            | "you can't lose the game this turn and your opponents can't win the game this turn"
                    ) {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                                StaticContinuousAbility::Restriction {
                                    id: ids.static_id(),
                                    rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                                },
                            ))),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    let rule_no_dot = rule_low.trim_end_matches('.').trim();
                    if filter.is_none()
                        && ((matches!(
                            rule_no_dot,
                            "you can't lose the game"
                                | "you can't lose the game this turn"
                                | "you can't lose the game for the rest of the game"
                        ) && matches!(
                            subject.as_deref(),
                            None | Some(SubjectRefAst::Player(PlayerRefAst::You))
                        )) || (matches!(
                            rule_no_dot,
                            "cant_lose_game"
                                | "can't lose the game"
                                | "can't lose the game this turn"
                                | "can't lose the game for the rest of the game"
                        ) && matches!(
                            subject.as_deref(),
                            Some(SubjectRefAst::Player(PlayerRefAst::You))
                        )))
                    {
                        let eff_dur = if dur == ResolvedDuration::NoStatedDuration {
                            if rule_no_dot.ends_with(" this turn") {
                                ResolvedDuration::UntilEndOfTurn
                            } else if rule_no_dot.ends_with(" for the rest of the game") {
                                ResolvedDuration::NoStatedDuration
                            } else {
                                dur
                            }
                        } else {
                            dur
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                                StaticContinuousAbility::Restriction {
                                    id: ids.static_id(),
                                    rule: StaticRestriction::ControllerCantLoseGame,
                                },
                            ))),
                            duration: eff_dur,
                        });
                        return Ok(());
                    }
                    if matches!(
                        rule_low.as_str(),
                        "players can't lose the game or win the game"
                            | "players can't win the game or lose the game"
                            | "players can't lose the game or win the game this turn"
                            | "players can't win the game or lose the game this turn"
                    ) {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                                StaticContinuousAbility::Restriction {
                                    id: ids.static_id(),
                                    rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                                },
                            ))),
                            duration: dur,
                        });
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                                StaticContinuousAbility::Restriction {
                                    id: ids.static_id(),
                                    rule: StaticRestriction::ControllerCantWinOpponentsCantLose,
                                },
                            ))),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if matches!(
                        rule_low.as_str(),
                        "players can't draw cards"
                            | "players can't draw"
                            | "opponents can't draw cards"
                            | "your opponents can't draw cards"
                            | "opponents can't draw"
                            | "your opponents can't draw"
                    ) {
                        let draw_rule = if rule_low.contains("opponent") {
                            StaticDrawRule::OpponentsCannotDraw
                        } else {
                            StaticDrawRule::PlayersCannotDraw
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                                StaticContinuousAbility::DrawRule {
                                    id: ids.static_id(),
                                    rule: draw_rule,
                                },
                            ))),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if rule_low == "cant_cast_additional_spells" {
                        let players = match subject.as_deref() {
                            Some(SubjectRefAst::Player(p)) => {
                                lower_affected_players_in_context(*p, builder, in_trigger)?
                            }
                            Some(SubjectRefAst::TargetPlayer {
                                opponent_only,
                                count: TargetCountAst::Exact(1),
                            }) => {
                                builder.bind_target(if *opponent_only {
                                    BodyTarget::SingleOpponent
                                } else {
                                    BodyTarget::SinglePlayer
                                })?;
                                AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                            }
                            _ => return Err("oracle:unsupported-temp-static"),
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::CantCastSpells {
                                players,
                                filter: None,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if rule_low.contains("damage can't be prevented")
                        || rule_low == "damage_cant_be_prevented"
                    {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::DamageCantBePrevented),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if matches!(
                        rule_no_dot,
                        "cant_cast_spells_or_activate_abilities"
                            | "can't cast spells or activate abilities"
                            | "can't cast spells or activate abilities this turn"
                    ) {
                        let players = match subject.as_deref() {
                            Some(SubjectRefAst::Player(p)) => {
                                lower_affected_players_in_context(*p, builder, in_trigger)?
                            }
                            Some(SubjectRefAst::TargetPlayer {
                                opponent_only,
                                count: TargetCountAst::Exact(1),
                            }) => {
                                builder.bind_target(if *opponent_only {
                                    BodyTarget::SingleOpponent
                                } else {
                                    BodyTarget::SinglePlayer
                                })?;
                                AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                            }
                            None => AffectedPlayers::EachOpponent,
                            _ => return Err("oracle:unsupported-temp-static"),
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::CantCastSpells {
                                players,
                                filter: None,
                            }),
                            duration: dur,
                        });
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::CantActivateAbilities {
                                players,
                                nonmana_only: false,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if let Some(res) = try_lower_cant_cast_from_zone(
                        subject.as_deref(),
                        &rule_low,
                        filter.as_deref(),
                        dur,
                        builder,
                        vocabulary,
                        in_trigger,
                    ) {
                        return res;
                    }
                    let is_cant_cast_nonland = rule == "CantCastOrPlay"
                        && matches!(subject.as_deref(), None | Some(SubjectRefAst::Player(_)))
                        && filter.as_deref().is_some_and(|f| {
                            f.zone.is_none()
                                && !f.any_card
                                && (f.any_types.0 & AstCardTypeMask::LAND.0) == 0
                                && (f.any_spell
                                    || (f.forbidden_types.0 & AstCardTypeMask::LAND.0) != 0)
                        });
                    let has_unhandled_cast_clause = rule_low.contains(" from ")
                        || rule_low.contains(" during ")
                        || rule_low.contains(" more than ")
                        || rule_low.contains(" unless ")
                        || rule_low.contains(" other than ")
                        || rule_low.contains(" without ")
                        || rule_low.contains(" as though ");
                    if !has_unhandled_cast_clause
                        && (rule_low.contains("can't cast")
                            || rule_low == "cant_cast_spells"
                            || rule_low == "cantcast:filter"
                            || is_cant_cast_nonland)
                    {
                        let players = match subject.as_deref() {
                            Some(SubjectRefAst::Player(p)) => {
                                lower_affected_players_in_context(*p, builder, in_trigger)?
                            }
                            Some(SubjectRefAst::TargetPlayer {
                                opponent_only,
                                count: TargetCountAst::Exact(1),
                            }) => {
                                builder.bind_target(if *opponent_only {
                                    BodyTarget::SingleOpponent
                                } else {
                                    BodyTarget::SinglePlayer
                                })?;
                                AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
                            }
                            None => {
                                if rule_low.starts_with("players ")
                                    || rule_low.starts_with("each player ")
                                {
                                    AffectedPlayers::EachPlayer
                                } else if rule_low.starts_with("you ") {
                                    AffectedPlayers::One(ResolvingPlayer::Controller)
                                } else if rule_low.starts_with("your opponents ")
                                    || rule_low.starts_with("opponents ")
                                    || rule_low.starts_with("each opponent ")
                                    || rule_low == "cant_cast_spells"
                                {
                                    AffectedPlayers::EachOpponent
                                } else {
                                    return Err("oracle:unsupported-temp-static");
                                }
                            }
                            _ => return Err("oracle:unsupported-temp-static"),
                        };
                        let flt = filter
                            .as_deref()
                            .map(|f| {
                                let mut inner = f.clone();
                                inner.any_spell = false;
                                if is_unrestricted_filter(&inner) {
                                    Ok(None)
                                } else {
                                    lower_object_filter(&inner, vocabulary).map(Some)
                                }
                            })
                            .transpose()?
                            .flatten();
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::CantCastSpells {
                                players,
                                filter: flt,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if let Some(subj) = subject.as_deref()
                        && let Some(type_part) = parse_type_removal_part(&rule_low, vocabulary)
                    {
                        return emit_resolved_parts_for_subject(
                            subj,
                            ResolvedPartsSpec(vec![type_part]),
                            dur,
                            builder,
                            vocabulary,
                            in_trigger,
                            self_pronoun_valid,
                        );
                    }
                    if let Some(src_raw) = rule_low
                        .strip_prefix("cantbetargetof:")
                        .or_else(|| rule_low.strip_prefix("can't be the target of "))
                        .or_else(|| rule_low.strip_prefix("can't be the targets of "))
                    {
                        let (src_body, eff_dur) = strip_trailing_temp_duration(src_raw, dur)
                            .unwrap_or((src_raw.to_owned(), dur));
                        let opt_parts = if let Some((mask, spells_only)) =
                            parse_cant_be_target_colors(&src_body)
                        {
                            Some(ResolvedPartsSpec(vec![if spells_only {
                                ResolvedPartSpec::CannotBeTargetedBySpellsOfColors(mask)
                            } else {
                                ResolvedPartSpec::CannotBeTargetedByColors(mask)
                            }]))
                        } else {
                            parse_cant_be_target_keywords(&src_body, vocabulary).map(|kws| {
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(
                                    AbilityChange::grant_keywords(kws),
                                )])
                            })
                        };
                        if let Some(parts) = opt_parts {
                            match subject.as_deref() {
                                None if self_pronoun_valid => {
                                    builder.instructions.push(Instruction::ChangeAbilitySource {
                                        parts,
                                        duration: eff_dur,
                                    });
                                    return Ok(());
                                }
                                Some(subj) => {
                                    return emit_resolved_parts_for_subject(
                                        subj,
                                        parts,
                                        eff_dur,
                                        builder,
                                        vocabulary,
                                        in_trigger,
                                        self_pronoun_valid,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    if is_temp_menace_blocking_rule(rule, &rule_low, filter.as_deref()) {
                        let mut ab = AbilityChange::default();
                        ab.granted_keywords.insert(KeywordAbility::Menace);
                        let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::Abilities(ab)]);
                        match subject.as_deref() {
                            None if self_pronoun_valid => {
                                builder.instructions.push(Instruction::ChangeAbilitySource {
                                    parts,
                                    duration: dur,
                                });
                                return Ok(());
                            }
                            Some(subj) => {
                                return emit_resolved_parts_for_subject(
                                    subj,
                                    parts,
                                    dur,
                                    builder,
                                    vocabulary,
                                    in_trigger,
                                    self_pronoun_valid,
                                );
                            }
                            _ => {}
                        }
                    }
                }
                StaticAbilityAst::RuleOrCombatRestriction {
                    subject,
                    rule,
                    filter,
                    condition: Some(cond),
                } if matches!(duration, DurationAst::ForAsLongAs(dur_cond) if dur_cond.as_ref() == cond.as_ref()) =>
                {
                    let mut trial = builder.clone();
                    if lower_instruction(
                        &InstructionAst::TemporaryStatic {
                            static_ability: Box::new(StaticAbilityAst::RuleOrCombatRestriction {
                                subject: subject.clone(),
                                rule: rule.clone(),
                                filter: filter.clone(),
                                condition: None,
                            }),
                            duration: duration.clone(),
                        },
                        &mut trial,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )
                    .is_ok()
                    {
                        *builder = trial;
                        return Ok(());
                    }
                }
                StaticAbilityAst::PlayOrCastPermission {
                    permission,
                    filter,
                    cast_only,
                    condition: None,
                } => {
                    let p_low = permission.trim().trim_end_matches('.').to_ascii_lowercase();
                    if p_low == "can be cast as though it had flash" {
                        let dur = lower_duration(duration)?;
                        let mut inner = filter.as_ref().clone();
                        inner.any_spell = false;
                        let obj = if is_unrestricted_filter(&inner) {
                            ObjectFilter::any()
                        } else {
                            lower_object_filter(&inner, vocabulary)?
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::FilteredFlash {
                                player: ResolvingPlayer::Controller,
                                filter: obj,
                                next_only: true,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if p_low == "can be cast without paying its mana cost" {
                        let dur = lower_duration(duration)?;
                        let mut inner = filter.as_ref().clone();
                        inner.any_spell = false;
                        let flt = if is_unrestricted_filter(&inner) {
                            None
                        } else {
                            Some(lower_object_filter(&inner, vocabulary)?)
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::NextSpellFree { filter: flt }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if p_low == "can't be countered" {
                        let dur = lower_duration(duration)?;
                        let mut inner = filter.as_ref().clone();
                        inner.any_spell = false;
                        let flt = if is_unrestricted_filter(&inner) {
                            None
                        } else {
                            Some(lower_object_filter(&inner, vocabulary)?)
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::CantBeCountered {
                                filter: flt,
                                controller_only: true,
                                next_only: true,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    if matches!(
                        p_low.as_str(),
                        "flash" | "as though they had flash" | "cast_as_though_flash"
                    ) {
                        if matches!(duration, DurationAst::UntilEndOfTurn)
                            && is_unrestricted_filter(filter)
                        {
                            builder
                                .instructions
                                .push(Instruction::GrantControllerFlashUntilCleanup);
                            return Ok(());
                        }
                        let dur = lower_duration(duration)?;
                        let obj = lower_object_filter(filter, vocabulary)?;
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::FilteredFlash {
                                player: ResolvingPlayer::Controller,
                                filter: obj,
                                next_only: false,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                    let zone_opt = if p_low.contains("from your hand or graveyard")
                        || p_low.contains("from your hand and/or graveyard")
                    {
                        Some(SourceZoneFilter::HandOrGraveyard)
                    } else if p_low.contains("from your graveyard")
                        || p_low == "play_from_graveyard"
                        || p_low == "cast_from_graveyard"
                    {
                        Some(SourceZoneFilter::Graveyard)
                    } else if p_low.contains("from a graveyard")
                        || p_low.contains("from any graveyard")
                        || p_low.contains("from graveyards")
                    {
                        Some(SourceZoneFilter::AnyGraveyard)
                    } else if p_low.contains("from an opponent's graveyard")
                        || p_low.contains("from opponents' graveyards")
                    {
                        Some(SourceZoneFilter::OpponentGraveyard)
                    } else if p_low.contains("from exile") || p_low == "play_from_exile" {
                        Some(SourceZoneFilter::Exile)
                    } else if p_low.contains("from your hand") {
                        Some(SourceZoneFilter::Hand)
                    } else {
                        None
                    };
                    if let Some(zone) = zone_opt {
                        let dur = lower_duration(duration)?;
                        let flt = if is_unrestricted_filter(filter) {
                            None
                        } else {
                            Some(lower_object_filter(filter, vocabulary)?)
                        };
                        let free = p_low.contains("without paying its mana cost")
                            || p_low.contains("without paying their mana costs");
                        let any_color = p_low.contains("mana of any color")
                            || p_low.contains("mana of any type");
                        let once = p_low.contains("once")
                            || p_low.contains("a spell")
                            || p_low.contains("one ");
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::ZoneCastPermission {
                                player: ResolvingPlayer::Controller,
                                zone,
                                filter: flt,
                                cast_only: *cast_only,
                                free,
                                any_color,
                                once,
                                sorcery_only: false,
                            }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                }
                _ => {}
            }
            let dur = lower_duration(duration)?;
            if let StaticAbilityAst::RuleOrCombatRestriction {
                subject,
                rule,
                filter,
                condition: None,
            } = static_ability.as_ref()
            {
                let rule_norm_owned = rule
                    .trim()
                    .trim_end_matches('.')
                    .to_ascii_lowercase()
                    .replace(
                        "can't be blocked this turn except by ",
                        "can't be blocked except by ",
                    )
                    .replace(
                        "can't be blocked this combat except by ",
                        "can't be blocked except by ",
                    )
                    .replace("can't be blocked this turn by ", "can't be blocked by ")
                    .replace("can't be blocked this combat by ", "can't be blocked by ");
                let eff_br_dur = if rule.to_ascii_lowercase().contains(" this combat ")
                    && matches!(
                        dur,
                        ResolvedDuration::NoStatedDuration | ResolvedDuration::UntilEndOfTurn
                    ) {
                    ResolvedDuration::UntilEndOfCombat
                } else {
                    dur
                };
                if let Some(br) =
                    lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary)
                        .or_else(|| {
                            lower_rule_blocking_restriction(
                                &rule_norm_owned,
                                filter.as_deref(),
                                vocabulary,
                            )
                        })
                        .or_else(|| {
                            filter
                                .is_none()
                                .then(|| {
                                    lower_combat_mod_blocking_restriction(
                                        &rule_norm_owned,
                                        vocabulary,
                                    )
                                })
                                .flatten()
                        })
                        .or_else(|| {
                            (filter.is_none()
                                && matches!(
                                    rule_norm_owned.as_str(),
                                    "can_block_any_number" | "can block any number of creatures"
                                ))
                            .then_some(BlockingRestriction::CanBlockAnyNumber)
                        })
                        .filter(|br| {
                            !matches!(
                                br,
                                BlockingRestriction::CannotBlockSource
                                    | BlockingRestriction::MustBlockSource
                                    | BlockingRestriction::CannotBeBlockedBySource
                            ) || self_pronoun_valid
                        })
                {
                    let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]);
                    match subject.as_deref() {
                        None if self_pronoun_valid => {
                            builder.instructions.push(Instruction::ChangeAbilitySource {
                                parts,
                                duration: eff_br_dur,
                            });
                            return Ok(());
                        }
                        Some(subj) => {
                            if matches!(subj, SubjectRefAst::Anaphora(_))
                                && is_permanent_body_target(builder.target)
                                && let Some((minimum, maximum)) =
                                    builder.instructions.iter().rev().find_map(|op| {
                                        if let Instruction::AffectPermanentTargets {
                                            minimum,
                                            maximum,
                                            ..
                                        } = op
                                        {
                                            Some((*minimum, *maximum))
                                        } else {
                                            None
                                        }
                                    })
                            {
                                builder
                                    .instructions
                                    .push(Instruction::AffectPermanentTargets {
                                        minimum,
                                        maximum,
                                        action: PermanentTargetAction::Modify {
                                            parts,
                                            duration: eff_br_dur,
                                        },
                                    });
                                return Ok(());
                            }
                            let mut trial = builder.clone();
                            if emit_resolved_parts_for_subject(
                                subj,
                                parts,
                                eff_br_dur,
                                &mut trial,
                                vocabulary,
                                in_trigger,
                                self_pronoun_valid,
                            )
                            .is_ok()
                            {
                                *builder = trial;
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                } else if filter.is_none()
                    && let Some(after_by) = rule_norm_owned.strip_prefix("can't be blocked by ")
                    && after_by.contains(" and/or ")
                    && let Some(conj_parts) = {
                        let mut parts_vec = Vec::new();
                        let mut all_valid = true;
                        for seg in after_by.split(" and/or ") {
                            let seg_clean = seg.trim().trim_end_matches('.');
                            if seg_clean.is_empty() {
                                all_valid = false;
                                break;
                            }
                            let probe = format!("can't be blocked by {seg_clean}");
                            if let Some(br) = lower_combat_mod_blocking_restriction(
                                &probe, vocabulary,
                            )
                            .filter(|br| {
                                !matches!(
                                    br,
                                    BlockingRestriction::CannotBlockSource
                                        | BlockingRestriction::MustBlockSource
                                        | BlockingRestriction::CannotBeBlockedBySource
                                ) || self_pronoun_valid
                            }) {
                                parts_vec.push(ResolvedPartSpec::BlockingRestriction(br));
                            } else {
                                all_valid = false;
                                break;
                            }
                        }
                        (all_valid && parts_vec.len() >= 2).then_some(ResolvedPartsSpec(parts_vec))
                    }
                {
                    match subject.as_deref() {
                        None if self_pronoun_valid => {
                            builder.instructions.push(Instruction::ChangeAbilitySource {
                                parts: conj_parts,
                                duration: eff_br_dur,
                            });
                            return Ok(());
                        }
                        Some(subj) => {
                            let mut trial = builder.clone();
                            if emit_resolved_parts_for_subject(
                                subj,
                                conj_parts,
                                eff_br_dur,
                                &mut trial,
                                vocabulary,
                                in_trigger,
                                self_pronoun_valid,
                            )
                            .is_ok()
                            {
                                *builder = trial;
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                } else if subject.is_none()
                    && let Some(flt) = filter.as_deref()
                    && let Some(br) = lower_rule_blocking_restriction(rule, None, vocabulary)
                {
                    let parts = ResolvedPartsSpec(vec![ResolvedPartSpec::BlockingRestriction(br)]);
                    let flt_subj = SubjectRefAst::All(Box::new(flt.clone()));
                    let mut trial = builder.clone();
                    if emit_resolved_parts_for_subject(
                        &flt_subj,
                        parts,
                        eff_br_dur,
                        &mut trial,
                        vocabulary,
                        in_trigger,
                        self_pronoun_valid,
                    )
                    .is_ok()
                    {
                        *builder = trial;
                        return Ok(());
                    }
                }
            }
            if let StaticAbilityAst::RuleOrCombatRestriction {
                subject: Some(subj),
                rule,
                filter: None,
                condition: None,
            } = static_ability.as_ref()
            {
                let mut trial = builder.clone();
                if lower_instruction(
                    &InstructionAst::CombatRuleMod {
                        subject: subj.clone(),
                        rule: rule.clone(),
                        duration: duration.clone(),
                    },
                    &mut trial,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                {
                    *builder = trial;
                    return Ok(());
                }
            }
            {
                let mut scratch = Program::new(
                    BTreeSet::from([CardType::Enchantment]),
                    BTreeSet::new(),
                    BTreeSet::new(),
                    ProgramKind::Permanent(PermanentDefinition {
                        entry_tap: None,
                        attachment: None,
                        printed_pt: None,
                        casting_cost: Some(ManaCost::free()),
                    }),
                );
                let mut scratch_ids = LoweringIds::default();
                if lower_static(
                    static_ability,
                    &mut scratch,
                    vocabulary,
                    &mut scratch_ids,
                    false,
                )
                .is_ok()
                    && scratch.triggered.is_empty()
                    && scratch.activated.is_empty()
                    && scratch.mana.is_empty()
                    && scratch.entry_counter_abilities.is_empty()
                    && scratch.entry_tap_unless.is_none()
                    && scratch.entry_copy.is_none()
                    && scratch.conditional_cost.is_none()
                    && scratch.attack_requirement.is_none()
                    && scratch.additional_attack_requirements.is_empty()
                    && !scratch.enters_prepared
                    && !scratch.cannot_be_countered
                    && !scratch.opening_hand_battlefield
                    && matches!(
                        scratch.kind,
                        ProgramKind::Permanent(PermanentDefinition {
                            entry_tap: None,
                            attachment: None,
                            printed_pt: None,
                            ..
                        })
                    )
                {
                    let has_source = in_trigger || self_pronoun_valid;
                    let zone_repls_valid = scratch.zone_replacements.is_empty()
                        || (scratch.static_conditions.is_empty()
                            && scratch.validate(vocabulary).is_ok());
                    let combined_cond = scratch.static_conditions.into_values().reduce(|a, b| {
                        spellwake_engine::characteristics::StaticSourceCondition::And(
                            Box::new(a),
                            Box::new(b),
                        )
                    });
                    let cond_ok = combined_cond.as_ref().is_none_or(|c| {
                        has_source || static_source_condition_is_source_independent(c)
                    });
                    if zone_repls_valid && cond_ok {
                        let mut abilities: Vec<StaticContinuousAbility> = scratch
                            .continuous
                            .into_iter()
                            .chain(scratch.additional_battlefield_abilities)
                            .collect();
                        let zone_repls = scratch.zone_replacements;
                        let mut emitted = Vec::new();
                        let mut all_ok = true;
                        if let Some(ref cond) = combined_cond {
                            for ab in &mut abilities {
                                if let StaticContinuousAbility::Battlefield { selector, .. } = ab {
                                    selector.source_condition = match selector.source_condition.take()
                                    {
                                        Some(existing) => Some(
                                            spellwake_engine::characteristics::StaticSourceCondition::And(
                                                Box::new(existing),
                                                Box::new(cond.clone()),
                                            ),
                                        ),
                                        None => Some(cond.clone()),
                                    };
                                }
                            }
                            for ab in abilities {
                                let is_bf =
                                    matches!(ab, StaticContinuousAbility::Battlefield { .. });
                                if let Some(mut ops) = lower_temp_static_abilities_to_ops(
                                    vec![ab],
                                    dur,
                                    builder,
                                    in_trigger,
                                    self_pronoun_valid,
                                ) {
                                    if !is_bf {
                                        for op in &mut ops {
                                            if let Instruction::CreateTemporaryRule { rule, .. } =
                                                op
                                                && let TemporaryRuleKind::TemporaryStatic(inner) =
                                                    rule.as_ref()
                                            {
                                                **rule =
                                                    TemporaryRuleKind::ConditionalTemporaryStatic {
                                                        condition: Box::new(cond.clone()),
                                                        ability: inner.clone(),
                                                    };
                                            } else {
                                                all_ok = false;
                                                break;
                                            }
                                        }
                                    }
                                    if !all_ok {
                                        break;
                                    }
                                    emitted.extend(ops);
                                } else {
                                    all_ok = false;
                                    break;
                                }
                            }
                        } else if !abilities.is_empty() {
                            if let Some(lowered_ops) = lower_temp_static_abilities_to_ops(
                                abilities,
                                dur,
                                builder,
                                in_trigger,
                                self_pronoun_valid,
                            ) {
                                emitted.extend(lowered_ops);
                            } else {
                                all_ok = false;
                            }
                        }
                        if all_ok {
                            for zr in zone_repls {
                                emitted.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::TemporaryZoneReplacement(
                                        Box::new(zr),
                                    )),
                                    duration: dur,
                                });
                            }
                            if !emitted.is_empty() {
                                builder.instructions.extend(emitted);
                                return Ok(());
                            }
                        }
                    }
                }
            }
            // Per CR 611.2b/c, continuous "As long as <cond>, ... until end of turn" effects
            // must evaluate <cond> continuously via StaticSourceCondition, not snapshot it once
            // at resolution via BranchIfFalse. Any conditional TemporaryStatic not handled by
            // lower_static into StaticSourceCondition fails closed here.
            Err("oracle:unsupported-temp-static")
        }
        InstructionAst::TemporaryReplacement {
            replacement,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            if let ReplacementEffectAst::EventInstead {
                subject_or_event,
                would,
                instead,
                instead_body,
            } = replacement.as_ref()
            {
                let subj_low = subject_or_event.trim().to_ascii_lowercase();
                let would_low = would
                    .trim()
                    .trim_end_matches(',')
                    .trim()
                    .to_ascii_lowercase();
                let ev_low = would_low
                    .strip_prefix("would ")
                    .unwrap_or(&would_low)
                    .trim()
                    .to_owned();
                let rep_low = instead.trim().to_ascii_lowercase();
                let ev_no_turn = ev_low.trim_end_matches('.').replace(" this turn", "");
                let rep_no_instead = rep_low
                    .trim_end_matches('.')
                    .trim()
                    .strip_suffix(" instead")
                    .unwrap_or(rep_low.trim_end_matches('.').trim())
                    .trim();
                if ev_low == "be_put_into_graveyard_as_it_resolves"
                    && rep_low == "exile_on_resolve"
                    && instead_body.as_deref().is_none_or(|b| {
                        matches!(
                            b,
                            [InstructionAst::Exile {
                                face_down: false,
                                into_pile: false,
                                ..
                            }]
                        )
                    })
                {
                    let prior_cast_player =
                        builder.instructions.iter().rev().find_map(|op| match op {
                            Instruction::GrantCardPlayPermission { player, .. } => Some(*player),
                            Instruction::CreateTemporaryRule { rule, .. } => match rule.as_ref() {
                                TemporaryRuleKind::ZoneCastPermission { player, .. } => {
                                    Some(*player)
                                }
                                _ => None,
                            },
                            _ => None,
                        });
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::ExileSpellOnResolve {
                            filter: None,
                            players: AffectedPlayers::One(
                                prior_cast_player.unwrap_or(ResolvingPlayer::Controller),
                            ),
                        }),
                        duration: dur,
                    });
                    return Ok(());
                }
                if try_lower_temp_exile_replacement(
                    &subj_low,
                    &ev_low,
                    &rep_low,
                    instead_body.as_deref(),
                    dur,
                    builder,
                    in_trigger,
                    self_pronoun_valid,
                ) {
                    return Ok(());
                }
                if ev_no_turn.trim() == "gain life"
                    && !rep_no_instead.contains(" and ")
                    && !rep_no_instead.contains(',')
                    && (rep_no_instead.ends_with(" gains no life")
                        || rep_no_instead.ends_with(" gain no life")
                        || rep_no_instead.ends_with(" can't gain life"))
                {
                    let players_opt = match subj_low.as_str() {
                        "that player or that planeswalker's controller"
                        | "that player or planeswalker's controller" => {
                            lower_resolving_player_in_context(
                                PlayerRefAst::ThatPlayerOrPlaneswalkerController,
                                builder,
                                in_trigger,
                            )
                            .ok()
                            .map(AffectedPlayers::One)
                        }
                        "that player" | "they" | "that opponent" => {
                            lower_resolving_player_in_context(
                                PlayerRefAst::ThatPlayer,
                                builder,
                                in_trigger,
                            )
                            .ok()
                            .map(AffectedPlayers::One)
                        }
                        "you" => Some(AffectedPlayers::One(ResolvingPlayer::Controller)),
                        "an opponent" | "opponents" | "your opponents" => {
                            Some(AffectedPlayers::EachOpponent)
                        }
                        "a player" | "any player" | "players" => Some(AffectedPlayers::EachPlayer),
                        _ => None,
                    };
                    if let Some(players) = players_opt {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::PlayerCantGainLife { players }),
                            duration: dur,
                        });
                        return Ok(());
                    }
                }
                let mut trial = builder.clone();
                let before_len = trial.instructions.len();
                if let Some(res) = try_lower_redirect_next_damage(
                    &subj_low,
                    &ev_low,
                    &rep_low,
                    &mut trial,
                    in_trigger,
                    in_trigger || self_pronoun_valid,
                    self_pronoun_valid,
                    bound_x,
                    vocabulary,
                ) {
                    res?;
                    if trial.instructions.len() == before_len + 1
                        && let Some(Instruction::CreateTemporaryRule {
                            rule,
                            duration: rule_dur,
                        }) = trial.instructions.last_mut()
                        && matches!(
                            rule.as_ref(),
                            TemporaryRuleKind::RedirectNextDamage { .. }
                                | TemporaryRuleKind::RedirectNextDamageExpr { .. }
                        )
                    {
                        *rule_dur = dur;
                        *builder = trial;
                        return Ok(());
                    }
                    return Err("oracle:unsupported-temp-replacement");
                }
            }
            let mut scratch = Program::new(
                BTreeSet::from([CardType::Enchantment]),
                BTreeSet::new(),
                BTreeSet::new(),
                ProgramKind::Permanent(PermanentDefinition {
                    entry_tap: None,
                    attachment: None,
                    printed_pt: None,
                    casting_cost: Some(ManaCost::free()),
                }),
            );
            let mut scratch_ids = LoweringIds::default();
            let lowered_temp_rep =
                lower_replacement(replacement, &mut scratch, vocabulary, &mut scratch_ids).is_ok()
                    || (dur == ResolvedDuration::UntilEndOfTurn
                        && match replacement.as_ref() {
                            ReplacementEffectAst::EventInstead {
                                subject_or_event,
                                would,
                                instead,
                                instead_body,
                            } if would.to_ascii_lowercase().contains(" this turn")
                                || (would.eq_ignore_ascii_case("would_event")
                                    && instead.to_ascii_lowercase().contains(" this turn")) =>
                            {
                                let synth = ReplacementEffectAst::EventInstead {
                                    subject_or_event: subject_or_event.clone(),
                                    would: collapse_ws(
                                        &would
                                            .trim()
                                            .to_ascii_lowercase()
                                            .replace(" this turn", ""),
                                    ),
                                    instead: if would.eq_ignore_ascii_case("would_event") {
                                        collapse_ws(
                                            &instead
                                                .trim()
                                                .to_ascii_lowercase()
                                                .replace(" this turn", ""),
                                        )
                                    } else {
                                        instead.clone()
                                    },
                                    instead_body: instead_body.clone(),
                                };
                                lower_replacement(
                                    &synth,
                                    &mut scratch,
                                    vocabulary,
                                    &mut scratch_ids,
                                )
                                .is_ok()
                            }
                            _ => false,
                        });
            if lowered_temp_rep
                && scratch.triggered.is_empty()
                && scratch.activated.is_empty()
                && scratch.mana.is_empty()
                && scratch.entry_counter_abilities.is_empty()
                && scratch.entry_tap_unless.is_none()
                && scratch.entry_copy.is_none()
                && scratch.conditional_cost.is_none()
                && scratch.static_conditions.is_empty()
                && scratch.attack_requirement.is_none()
                && scratch.additional_attack_requirements.is_empty()
                && !scratch.enters_prepared
                && !scratch.cannot_be_countered
                && !scratch.opening_hand_battlefield
                && matches!(
                    scratch.kind,
                    ProgramKind::Permanent(PermanentDefinition {
                        entry_tap: None,
                        attachment: None,
                        printed_pt: None,
                        ..
                    })
                )
            {
                let zone_repls_valid =
                    scratch.zone_replacements.is_empty() || scratch.validate(vocabulary).is_ok();
                if zone_repls_valid {
                    let abilities: Vec<StaticContinuousAbility> = scratch
                        .continuous
                        .into_iter()
                        .chain(scratch.additional_battlefield_abilities)
                        .collect();
                    let zone_repls = scratch.zone_replacements;
                    let mut emitted = Vec::new();
                    let abilities_ok = if abilities.is_empty() {
                        true
                    } else if let Some(lowered_ops) = lower_temp_static_abilities_to_ops(
                        abilities,
                        dur,
                        builder,
                        in_trigger,
                        self_pronoun_valid,
                    ) {
                        emitted.extend(lowered_ops);
                        true
                    } else {
                        false
                    };
                    if abilities_ok {
                        for zr in zone_repls {
                            emitted.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::TemporaryZoneReplacement(
                                    Box::new(zr),
                                )),
                                duration: dur,
                            });
                        }
                        if !emitted.is_empty() {
                            builder.instructions.extend(emitted);
                            return Ok(());
                        }
                    }
                }
            }
            if dur == ResolvedDuration::UntilEndOfTurn
                && let ReplacementEffectAst::EventInstead {
                    subject_or_event,
                    would,
                    instead,
                    ..
                } = replacement.as_ref()
            {
                let subj_low = subject_or_event.trim().to_ascii_lowercase();
                let would_low = would
                    .trim()
                    .trim_end_matches(',')
                    .trim()
                    .to_ascii_lowercase();
                let ev_no_would = would_low
                    .strip_prefix("would ")
                    .unwrap_or(&would_low)
                    .trim();
                let ev_norm_owned = ev_no_would.trim_end_matches('.').replace(" this turn", "");
                let ev_norm = ev_norm_owned.trim();
                let rep_low = instead.trim().to_ascii_lowercase();
                let rep_no_dot = rep_low.trim_end_matches('.').trim();
                let rep_no_instead = rep_no_dot
                    .strip_suffix(" instead")
                    .unwrap_or(rep_no_dot)
                    .trim();
                let rep_norm_owned = rep_no_instead.replace(" this turn", "");
                let rep_norm = rep_norm_owned.trim();
                let parsed_dmg_rule = if let Some(rule) =
                    parse_modify_damage_event(&subj_low, ev_norm, rep_norm, vocabulary)
                {
                    Some(rule)
                } else if rep_norm.contains("that much damage plus x") {
                    let probe_rep =
                        rep_norm.replacen("that much damage plus x", "that much damage plus 1", 1);
                    if let Some(StaticDamageRule::ModifyDamage {
                        source,
                        source_controller,
                        recipient,
                        kind,
                        op: StaticDamageOp::Add(1),
                    }) = parse_modify_damage_event(&subj_low, ev_norm, &probe_rep, vocabulary)
                    {
                        let x_val = lower_value_expr_in_context(
                            resolve_x(&ValueExprAst::ChosenX, bound_x),
                            builder,
                            in_trigger,
                            vocabulary,
                        )?;
                        Some(StaticDamageRule::ModifyDamage {
                            source,
                            source_controller,
                            recipient,
                            kind,
                            op: StaticDamageOp::AddExpr(Box::new(x_val)),
                        })
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(rule) = parsed_dmg_rule {
                    let recipient_ok = (in_trigger || self_pronoun_valid)
                        || !matches!(
                            rule,
                            StaticDamageRule::ModifyDamage {
                                recipient: StaticDamageRecipient::SelfObject
                                    | StaticDamageRecipient::AttachedHost,
                                ..
                            }
                        );
                    if recipient_ok {
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                                StaticContinuousAbility::DamageRule {
                                    id: ids.static_id(),
                                    rule,
                                },
                            ))),
                            duration: dur,
                        });
                        return Ok(());
                    }
                }
            }
            Err("oracle:unsupported-temp-replacement")
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if matches!(
            restriction.as_str(),
            "DamageCantBePreventedThisTurn" | "DamageCantBePrevented"
        ) =>
        {
            builder
                .instructions
                .push(Instruction::DisallowDamagePreventionThisTurn);
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("proliferate again") => {
            builder.instructions.push(Instruction::Proliferate);
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("choose new targets for it")
            || restriction.eq_ignore_ascii_case("choose a new target for it")
            || restriction.eq_ignore_ascii_case("you may choose new targets for it")
            || restriction.eq_ignore_ascii_case("you may choose new targets for the copy") =>
        {
            builder.instructions.push(Instruction::ChooseNewTargets);
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("players can't gain life this turn")
            || restriction.eq_ignore_ascii_case("players can't gain life") =>
        {
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::PlayerCantGainLife {
                    players: AffectedPlayers::EachPlayer,
                }),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "players can't lose life" | "players can't lose life this turn"
            )
        } =>
        {
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                    StaticContinuousAbility::EventReplacement {
                        id: ids.static_id(),
                        event: ReplaceableEvent::LifeLoss {
                            player: AffectedPlayers::EachPlayer,
                        },
                        condition: None,
                        action: StaticReplacementAction::Skip,
                    },
                ))),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "players can't lose the game or win the game"
                    | "players can't win the game or lose the game"
                    | "players can't lose the game or win the game this turn"
                    | "players can't win the game or lose the game this turn"
            )
        } =>
        {
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                    },
                ))),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerCantWinOpponentsCantLose,
                    },
                ))),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "you can't lose the game and your opponents can't win the game"
                    | "you can't lose the game this turn and your opponents can't win the game this turn"
                    | "you can't lose the game and your opponents can't win the game this turn"
                    | "you can't lose the game and your opponents can't win the game for the rest of the game"
            )
        } =>
        {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            let dur = if r_low.ends_with(" for the rest of the game") {
                ResolvedDuration::NoStatedDuration
            } else {
                ResolvedDuration::UntilEndOfTurn
            };
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                    },
                ))),
                duration: dur,
            });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "that player can't gain life for the rest of the game"
                    | "that player can't gain life this turn"
            )
        } =>
        {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            let dur = if r_low.ends_with(" for the rest of the game") {
                ResolvedDuration::NoStatedDuration
            } else {
                ResolvedDuration::UntilEndOfTurn
            };
            let rp =
                lower_resolving_player_in_context(PlayerRefAst::ThatPlayer, builder, in_trigger)?;
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::PlayerCantGainLife {
                    players: AffectedPlayers::One(rp),
                }),
                duration: dur,
            });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case("permanents enter tapped this turn") =>
        {
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OthersEnterTapped(Box::default()),
                    },
                ))),
                duration: ResolvedDuration::UntilEndOfTurn,
            });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case("investigate an additional time") =>
        {
            lower_instruction(
                &InstructionAst::Investigate {
                    player: PlayerRefAst::You,
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
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case("skip the untap step of that turn") =>
        {
            if builder
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::TakeExtraTurn))
            {
                builder.instructions.push(Instruction::SkipNextStep {
                    player: ResolvingPlayer::Controller,
                    step: Step::Untap,
                });
                Ok(())
            } else {
                Err("oracle:unsupported-instruction")
            }
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "counter all abilities"
                    | "counter all activated and triggered abilities"
                    | "counter all activated or triggered abilities"
                    | "counter all abilities your opponents control"
                    | "counter all abilities opponents control"
                    | "counter all activated and triggered abilities your opponents control"
                    | "counter all activated or triggered abilities your opponents control"
                    | "counter all spells"
                    | "counter all other spells"
                    | "counter all spells your opponents control"
                    | "counter all spells opponents control"
                    | "counter all other spells your opponents control"
                    | "counter all spells and abilities"
                    | "counter all other spells and abilities"
                    | "counter all spells and all abilities"
                    | "counter all other spells and all abilities"
                    | "counter all spells and abilities your opponents control"
                    | "counter all spells your opponents control and all abilities your opponents control"
                    | "counter all other spells and abilities your opponents control"
                    | "counter all spells and abilities opponents control"
            )
        } =>
        {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            let include_spells = r_low.contains("spell");
            let include_abilities = r_low.contains("abilit");
            let opponents_only = r_low.contains("opponent");
            builder
                .instructions
                .push(Instruction::CounterMatchingStackObjects {
                    filter: None,
                    include_spells,
                    include_abilities,
                    opponents_only,
                });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case("it enters tapped") =>
        {
            match builder.instructions.last_mut() {
                Some(Instruction::MoveSubject {
                    destination: SubjectDestination::Battlefield { tapped, .. },
                    ..
                })
                | Some(Instruction::SelectZoneCardsToZone {
                    destination: SubjectDestination::Battlefield { tapped, .. },
                    ..
                })
                | Some(Instruction::MoveLastMovedFiltered {
                    destination: SubjectDestination::Battlefield { tapped, .. },
                    ..
                })
                | Some(Instruction::ReturnLinkedExile {
                    destination: SubjectDestination::Battlefield { tapped, .. },
                    ..
                })
                | Some(Instruction::ReturnOwnGraveyardPermanentToBattlefield { tapped })
                | Some(Instruction::ReturnSourceGraveyardToBattlefield { tapped }) => {
                    *tapped = true;
                    Ok(())
                }
                Some(Instruction::CreateTokens { creations }) if creations.len() == 1 => {
                    creations[0].enters_tapped = true;
                    Ok(())
                }
                Some(Instruction::CreateTokenCopy { tapped, .. }) => {
                    *tapped = true;
                    Ok(())
                }
                Some(Instruction::CreateTokenCopyExt { enters_tapped, .. }) => {
                    *enters_tapped = true;
                    Ok(())
                }
                _ => Err("oracle:unsupported-instruction"),
            }
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "you can't lose the game this turn"
                    | "you can't lose the game for the rest of the game"
            )
        } =>
        {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            let dur = if r_low.ends_with(" for the rest of the game") {
                ResolvedDuration::NoStatedDuration
            } else {
                ResolvedDuration::UntilEndOfTurn
            };
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerCantLoseGame,
                    },
                ))),
                duration: dur,
            });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("NoMaxHandSizeRestOfGame") => {
            builder
                .instructions
                .push(Instruction::GrantNoMaxHandSizeRestOfGame);
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("FlashbackEqualManaCost") => {
            let has_flashback_grant = builder.instructions.iter().any(|op| {
                matches!(op, Instruction::GrantFlashback(_))
                    || matches!(
                        op,
                        Instruction::CreateTemporaryRule { rule, .. }
                            if matches!(
                                rule.as_ref(),
                                TemporaryRuleKind::ZoneCastPermission {
                                    zone: SourceZoneFilter::Graveyard,
                                    free: false,
                                    ..
                                }
                            )
                    )
            });
            if has_flashback_grant {
                Ok(())
            } else {
                Err("oracle:unsupported-instruction")
            }
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("XCantBeZero") => {
            if let Some(x_ast) = bound_x {
                let x_val = lower_value_expr_in_context(x_ast, builder, in_trigger, vocabulary)?;
                if matches!(&x_val, ValueExpr::Literal(n) if n.is_positive()) {
                    return Ok(());
                }
                let has_whole_body_target_op = builder.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::AffectPermanentTargets { .. }
                            | Instruction::DoubleCountersOnTargets
                            | Instruction::ModifyAnyNumberOfPermanents { .. }
                    )
                });
                if !builder.instructions.is_empty() && !has_whole_body_target_op {
                    let end_dest = builder.instructions.len() + 1;
                    prepend_instruction_at(
                        &mut builder.instructions,
                        0,
                        Instruction::BranchIfFalse {
                            condition: ResolutionCondition {
                                left: x_val,
                                comparison: Comparison::Greater,
                                right: ValueExpr::literal(0),
                            },
                            destination: end_dest,
                        },
                    );
                    return Ok(());
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case("the game is a draw") =>
        {
            builder.instructions.push(Instruction::DrawGame);
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            r_low.starts_with("roll ")
                && (r_low.ends_with("-sided die") || r_low.ends_with("-sided dice"))
        } =>
        {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            let rest = r_low
                .strip_prefix("roll ")
                .and_then(|s| {
                    s.strip_suffix("-sided dice")
                        .or_else(|| s.strip_suffix("-sided die"))
                })
                .ok_or("oracle:unsupported-instruction")?
                .trim();
            let mut parts = rest.split_whitespace();
            let count_tok = parts.next().ok_or("oracle:unsupported-instruction")?;
            let sides_tok = parts.next().ok_or("oracle:unsupported-instruction")?;
            if parts.next().is_some() {
                return Err("oracle:unsupported-instruction");
            }
            let sides_num: u16 = match sides_tok {
                "four" | "4" => 4,
                "six" | "6" => 6,
                "eight" | "8" => 8,
                "ten" | "10" => 10,
                "twelve" | "12" => 12,
                "twenty" | "20" => 20,
                _ => return Err("oracle:unsupported-instruction"),
            };
            let count = if count_tok == "x" {
                lower_value_expr_in_context(
                    resolve_x(&ValueExprAst::ChosenX, bound_x),
                    builder,
                    in_trigger,
                    vocabulary,
                )?
            } else if matches!(count_tok, "a" | "an") {
                ValueExpr::literal(1)
            } else if let Some(n) = parse_small_number_word(count_tok)
                && n > 0
            {
                ValueExpr::literal(i64::from(n))
            } else {
                return Err("oracle:unsupported-instruction");
            };
            let sides = std::num::NonZeroU16::new(sides_num).ok_or("oracle:zero-die-sides")?;
            builder.instructions.push(Instruction::RollDie {
                player: ResolvingPlayer::Controller,
                sides,
                count,
                mode: spellwake_engine::program::DieRollMode::Sum,
                bands: Vec::new(),
            });
            Ok(())
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "the token enters tapped and attacking"
                    | "those tokens enter tapped and attacking"
                    | "it enters tapped and attacking"
                    | "the token enters tapped and attacking that player"
                    | "those tokens enter tapped and attacking that player"
                    | "it enters tapped and attacking that player"
                    | "the token enters tapped and attacking defending player"
                    | "those tokens enter tapped and attacking defending player"
                    | "it enters tapped and attacking defending player"
            )
        } =>
        {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            let defender_scope = if r_low.ends_with(" that player") {
                let rp = lower_resolving_player_in_context(
                    PlayerRefAst::ThatPlayer,
                    builder,
                    in_trigger,
                )?;
                spellwake_engine::token::AttackingDefenderScope::Player(rp)
            } else if r_low.ends_with(" defending player") {
                spellwake_engine::token::AttackingDefenderScope::Player(
                    ResolvingPlayer::DefendingPlayer,
                )
            } else {
                spellwake_engine::token::AttackingDefenderScope::AnyLegal
            };
            let can_upgrade = matches!(
                builder.instructions.last(),
                Some(Instruction::CreateTokens { creations })
                    if creations.len() == 1
                        && creations[0].recipients == TokenRecipients::Controller
                        && creations[0].definition.program().has_type(CardType::Creature)
            );
            if can_upgrade
                && let Some(Instruction::CreateTokens { mut creations }) =
                    builder.instructions.pop()
            {
                creations[0].enters_tapped = true;
                builder
                    .instructions
                    .push(Instruction::CreateAttackingTokens {
                        creations,
                        defender_scope,
                    });
                return Ok(());
            }
            if let Some(last) = builder.instructions.last_mut() {
                match last {
                    Instruction::CreateTokenCopyExt {
                        recipients,
                        enters_tapped,
                        enters_attacking,
                        defender_scope: ds,
                        ..
                    } if *recipients == TokenRecipients::Controller => {
                        *enters_tapped = true;
                        *enters_attacking = true;
                        *ds = defender_scope;
                        return Ok(());
                    }
                    Instruction::CreateTokenCopy {
                        recipients,
                        tapped,
                        attacking,
                        ..
                    } if *recipients == TokenRecipients::Controller
                        && defender_scope
                            == spellwake_engine::token::AttackingDefenderScope::AnyLegal =>
                    {
                        *tapped = true;
                        *attacking = true;
                        return Ok(());
                    }
                    Instruction::CreateTokenCopy {
                        source,
                        count,
                        sacrifice_at_end_step,
                        exile_at_end_step,
                        grant_keywords,
                        nonlegendary,
                        override_pt,
                        override_colors: None,
                        add_subtypes,
                        add_card_types,
                        strip_mana_cost: false,
                        recipients,
                        ..
                    } if *recipients == TokenRecipients::Controller => {
                        let mut extra_keywords = std::mem::take(grant_keywords);
                        let haste = extra_keywords.remove(&KeywordAbility::Haste);
                        let ext_pt =
                            override_pt.map(|(p, t)| (i64::from(p).into(), i64::from(t).into()));
                        *last = Instruction::CreateTokenCopyExt {
                            source: *source,
                            count: count.clone(),
                            recipients: *recipients,
                            enters_tapped: true,
                            enters_attacking: true,
                            haste,
                            sacrifice_at_end_step: *sacrifice_at_end_step,
                            exile_at_end_step: *exile_at_end_step,
                            nonlegendary: *nonlegendary,
                            extra_types: std::mem::take(add_card_types),
                            extra_subtypes: std::mem::take(add_subtypes),
                            extra_keywords,
                            override_pt: ext_pt,
                            extra_colors: BTreeSet::new(),
                            defender_scope,
                        };
                        return Ok(());
                    }
                    _ => {}
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if {
            let r_low = restriction
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "it's still a land"
                    | "they're still lands"
                    | "it's still a creature"
                    | "they're still creatures"
                    | "it's still an artifact"
                    | "they're still artifacts"
                    | "it's still an enchantment"
                    | "they're still enchantments"
            )
        } =>
        {
            if let Some(op) = builder.instructions.last_mut() {
                let parts = match op {
                    Instruction::ChangeAbilitySource { parts, .. }
                    | Instruction::CreateContinuousEffect { parts, .. }
                    | Instruction::ChangeGroup { parts, .. } => Some(parts),
                    Instruction::AffectPermanentTargets {
                        action: PermanentTargetAction::Modify { parts, .. },
                        ..
                    } => Some(parts),
                    _ => None,
                };
                if let Some(p) = parts {
                    let mut touched = false;
                    for part in &mut p.0 {
                        if let ResolvedPartSpec::Type(tc)
                        | ResolvedPartSpec::TypeWithSubtypes { base: tc, .. } = part
                        {
                            match &mut tc.types {
                                TypeOperation::Add(_) => touched = true,
                                TypeOperation::Set(types) => {
                                    let existing = std::mem::take(types);
                                    tc.types = TypeOperation::Add(existing);
                                    touched = true;
                                }
                                TypeOperation::Keep | TypeOperation::Remove(_) => {}
                            }
                        }
                    }
                    if touched {
                        return Ok(());
                    }
                }
            }
            Err("oracle:unsupported-instruction")
        }
        InstructionAst::GrantAbilityText {
            subject,
            ability_text,
            compiled: Some(compiled_face),
            duration,
        } => {
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
                for it in items {
                    lower_instruction(
                        &InstructionAst::GrantAbilityText {
                            subject: Box::new(it.clone()),
                            ability_text: ability_text.clone(),
                            compiled: Some(compiled_face.clone()),
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
            let dur = lower_duration(duration)?;
            let parts = lower_granted_ability_specs(compiled_face, vocabulary, ids)?;
            if parts.is_empty() {
                return Err("oracle:unsupported-grant-ability-text");
            }
            match eff_subject {
                s if (self_pronoun_valid || in_trigger)
                    && is_self_pronoun(s, builder, self_pronoun_valid) =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeAbilitySource {
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
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
                    for part in parts {
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![part]),
                                duration: dur,
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
                    builder
                        .instructions
                        .push(Instruction::AffectPermanentTargets {
                            minimum: *n,
                            maximum: *n,
                            action: PermanentTargetAction::Modify {
                                parts: ResolvedPartsSpec(parts),
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
                                parts: ResolvedPartsSpec(parts),
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
                    for part in parts {
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![part]),
                                duration: dur,
                            });
                    }
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
                                parts: ResolvedPartsSpec(parts),
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
                                parts: ResolvedPartsSpec(parts),
                                duration: dur,
                            },
                        });
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
                            parts: ResolvedPartsSpec(parts),
                            duration: dur,
                        });
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) | SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if is_permanent_body_target(builder.target) =>
                {
                    for part in parts {
                        builder
                            .instructions
                            .push(Instruction::CreateContinuousEffect {
                                parts: ResolvedPartsSpec(vec![part]),
                                duration: dur,
                            });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) | SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if builder.target == BodyTarget::None
                        && last_instruction_moved_object(&builder.instructions) =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::LastMoved),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) | SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if builder.target == BodyTarget::None
                        && has_prior_token_creation(&builder.instructions) =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::CreatedTokens),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_) | SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if builder.target == BodyTarget::None
                        && let Some(group_obj) = builder.last_group_filter =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(group_obj)),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(_)
                    if try_append_fixed_first_target_parts(
                        builder,
                        ResolvedPartsSpec(parts.clone()),
                        dur,
                    ) =>
                {
                    Ok(())
                }
                SubjectRefAst::AttachedHost(_) => {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                ObjectClause::any().attached_to_self(),
                            ))),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Anaphora(raw)
                    if builder.target == BodyTarget::None
                        && in_trigger
                        && (!self_pronoun_valid || !is_self_anaphora_label(raw)) =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
                    if builder.target == BodyTarget::None && in_trigger =>
                {
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::TriggerSubject),
                            parts: ResolvedPartsSpec(vec![part]),
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
                    for part in parts {
                        builder.instructions.push(Instruction::ChangeGroup {
                            group: Box::new(EffectSubject::Matching(obj)),
                            parts: ResolvedPartsSpec(vec![part]),
                            duration: dur,
                        });
                    }
                    Ok(())
                }
                _ => Err("oracle:unsupported-grant-ability-text"),
            }
        }
        InstructionAst::AdditionalLandPlay { count, duration } if *count > 0 => {
            let dur = lower_duration(duration)?;
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::AdditionalLandPlay {
                    player: ResolvingPlayer::Controller,
                    count: *count,
                }),
                duration: dur,
            });
            Ok(())
        }
        InstructionAst::OneShotReplacement {
            subject,
            event,
            replace_with,
            instead_body,
        } => {
            let ev_low = event.trim().to_ascii_lowercase();
            let rep_low = replace_with.trim().to_ascii_lowercase();
            let subj_low = subject.trim().to_ascii_lowercase();
            let body_exiles = instead_body.as_ref().is_some_and(|b| {
                b.iter()
                    .any(|op| matches!(op, InstructionAst::Exile { .. }))
            });
            let is_exile = rep_low.contains("exile") || body_exiles;
            if is_exile
                && matches!(builder.target, BodyTarget::SingleSpell(_))
                && (ev_low.contains("counter")
                    || ev_low.contains("graveyard")
                    || subj_low.contains("spell"))
                && let Some(last) = builder.instructions.last_mut()
                && *last == Instruction::CounterSpell
            {
                *last = Instruction::CounterSpellToExile;
                return Ok(());
            }
            let to_top_of_library =
                rep_low == "top_of_library" || rep_low.contains("top of its owner's library");
            let to_bottom_of_library =
                rep_low == "bottom_of_library" || rep_low.contains("bottom of its owner's library");
            if (to_top_of_library || to_bottom_of_library)
                && matches!(builder.target, BodyTarget::SingleSpell(_))
                && (ev_low.contains("counter")
                    || ev_low.contains("graveyard")
                    || subj_low.contains("spell"))
                && let Some(last) = builder.instructions.last_mut()
                && *last == Instruction::CounterSpell
            {
                *last = Instruction::CounterSpellToLibrary {
                    top: to_top_of_library,
                };
                return Ok(());
            }
            if is_exile
                && (ev_low.contains("die") || ev_low.contains("graveyard"))
                && (subj_low.contains("dealt damage this way")
                    || ev_low.contains("dealt damage this way")
                    || subj_low.contains("dealt damage by ~")
                    || subj_low.contains("dealt damage by this spell")
                    || ((self_pronoun_valid || in_trigger)
                        && (subj_low.contains("dealt damage by this creature")
                            || subj_low.contains("dealt damage by this permanent"))))
            {
                if (subj_low.contains("dealt damage this way")
                    || ev_low.contains("dealt damage this way"))
                    && let Some(last) = builder.instructions.last_mut()
                {
                    match last.clone() {
                        Instruction::Damage { amount } => {
                            *last = Instruction::DamageAmountThenExileIfDies(ValueExpr::Literal(
                                amount,
                            ));
                            return Ok(());
                        }
                        Instruction::DamageAmount(expr) => {
                            *last = Instruction::DamageAmountThenExileIfDies(expr);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                let has_damage_op = ((self_pronoun_valid || in_trigger)
                    && (subj_low.contains("dealt damage by ~")
                        || subj_low.contains("dealt damage by this creature")
                        || subj_low.contains("dealt damage by this permanent")))
                    || builder.instructions.iter().any(|op| {
                        matches!(
                            op,
                            Instruction::Damage { .. }
                                | Instruction::DamageAmount(_)
                                | Instruction::DamageAmountThenExileIfDies(_)
                                | Instruction::DamageGroup { .. }
                                | Instruction::DamageGroupAmount { .. }
                                | Instruction::DamageOpponentsAmount(_)
                                | Instruction::DamageRelatedPlayerAmount(_)
                                | Instruction::DamageOrderedTargets { .. }
                                | Instruction::DamageFromPower(_)
                                | Instruction::DamageFromGrantingPermanent { .. }
                                | Instruction::DamageDamagedPlayersCreatures
                                | Instruction::DamageTargetControllersCreatures { .. }
                                | Instruction::DamageCapturedBlocker(_)
                                | Instruction::DamageCreatureAtSpellMastery { .. }
                                | Instruction::DealDamageFrom { .. }
                                | Instruction::DealDamageToAnnouncedTargets { .. }
                                | Instruction::AffectPermanentTargets {
                                    action: PermanentTargetAction::Damage(_),
                                    ..
                                }
                                | Instruction::DivideDamageAmongTargets { .. }
                                | Instruction::DivideDamageExprAmongTargets { .. }
                                | Instruction::CreatureDealsPowerDamageToOther { .. }
                                | Instruction::Fight { .. }
                                | Instruction::FightSourceWithTarget
                                | Instruction::FightFixedTargets
                                | Instruction::FightPairedCreatures(_)
                        )
                    });
                if has_damage_op {
                    let any_types = if subj_low.contains("creature or planeswalker") {
                        Some(CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER))
                    } else if subj_low.contains("planeswalker") {
                        Some(CardTypeMask::PLANESWALKER)
                    } else if subj_low.contains("creature") {
                        Some(CardTypeMask::CREATURE)
                    } else if subj_low.contains("permanent") {
                        Some(CardTypeMask::PERMANENT)
                    } else {
                        None
                    };
                    if let Some(any_types) = any_types {
                        let dmg_flag = if self_pronoun_valid
                            || in_trigger
                            || subj_low.contains("dealt damage by ~")
                            || subj_low.contains("dealt damage by this spell")
                        {
                            ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN
                        } else {
                            ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN
                        };
                        let clause = ObjectClause {
                            any_types,
                            flags: dmg_flag,
                            ..ObjectClause::any()
                        };
                        builder.instructions.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::ExileIfDies {
                                subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                                    clause,
                                ))),
                            }),
                            duration: ResolvedDuration::UntilEndOfTurn,
                        });
                        return Ok(());
                    }
                }
            }
            if ev_low == "next_time_this_turn" {
                let rep_trim = rep_low.trim().trim_end_matches('.').trim();
                let is_prevent_rep = matches!(
                    rep_trim,
                    "prevent that damage"
                        | "prevent all of that damage"
                        | "prevent all damage that would be dealt this way"
                );
                let half_round_down = matches!(
                    rep_trim,
                    "prevent half that damage, rounded down"
                        | "prevent half of that damage, rounded down"
                        | "prevent half of the damage, rounded down"
                );
                let is_deflecting_palm_rep = rep_trim.starts_with("prevent that damage")
                    && (rep_trim.contains("if damage is prevented this way, ~ deals that much damage to that source's controller")
                        || rep_trim.contains("if damage is prevented this way, ~ deals that much damage to its controller"));
                if (is_prevent_rep || half_round_down)
                    && !is_deflecting_palm_rep
                    && let Some((combat_only, after_would)) = subj_low
                        .strip_prefix("combat damage would ")
                        .map(|r| (true, r))
                        .or_else(|| subj_low.strip_prefix("damage would ").map(|r| (false, r)))
                {
                    let after_norm = collapse_ws(
                        &after_would
                            .replace(" this turn", "")
                            .replace(" this combat", ""),
                    );
                    if let Some((from_part, s_part_opt)) = split_dealt_from_and_source(&after_norm)
                    {
                        let mut trial = builder.clone();
                        let has_source = in_trigger || self_pronoun_valid;
                        let src_parsed = match s_part_opt {
                            Some(s_part) => parse_next_time_source_with_target(
                                s_part, &mut trial, has_source, in_trigger, vocabulary,
                            ),
                            None => Some(Ok((None, false, false, false))),
                        };
                        if let Some(Ok((
                            source,
                            source_chosen_color,
                            source_chosen_type,
                            source_is_target,
                        ))) = src_parsed
                            && let Some(Ok(recipient)) = parse_damage_recipient_phrase_with_target(
                                from_part, &mut trial, has_source, in_trigger, vocabulary,
                            )
                            && !(source_is_target
                                && matches!(
                                    recipient,
                                    StaticDamageRecipient::TargetObject
                                        | StaticDamageRecipient::TargetAny
                                ))
                        {
                            trial.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::PreventNextTimeDamage {
                                    source,
                                    source_chosen_color,
                                    source_chosen_type,
                                    source_is_target,
                                    half_round_down,
                                    recipient,
                                    combat_only,
                                }),
                                duration: ResolvedDuration::UntilEndOfTurn,
                            });
                            *builder = trial;
                            return Ok(());
                        }
                    }
                }
                if (is_prevent_rep || half_round_down || is_deflecting_palm_rep)
                    && let Some((src_raw, dst_raw)) = subj_low.split_once(" would deal ")
                    && dst_raw.contains("damage")
                {
                    let combat_only = dst_raw.contains("combat damage");
                    let after_dmg = dst_raw
                        .split_once("combat damage")
                        .or_else(|| dst_raw.split_once("damage"))
                        .map(|(_, r)| r.trim())
                        .unwrap_or("");
                    let mut trial = builder.clone();
                    let has_source = in_trigger || self_pronoun_valid;
                    let src_parsed = parse_next_time_source_with_target(
                        src_raw, &mut trial, has_source, in_trigger, vocabulary,
                    );
                    if let Some(Ok((
                        source,
                        source_chosen_color,
                        source_chosen_type,
                        source_is_target,
                    ))) = src_parsed
                    {
                        let rec_parsed = if after_dmg.is_empty() {
                            Some(Ok(StaticDamageRecipient::Any))
                        } else {
                            parse_damage_recipient_phrase_with_target(
                                after_dmg, &mut trial, has_source, in_trigger, vocabulary,
                            )
                        };
                        if let Some(Ok(recipient)) = rec_parsed
                            && !(source_is_target
                                && matches!(
                                    recipient,
                                    StaticDamageRecipient::TargetObject
                                        | StaticDamageRecipient::TargetAny
                                ))
                        {
                            if is_deflecting_palm_rep
                                && !source_chosen_color
                                && !source_chosen_type
                                && !source_is_target
                            {
                                trial.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                                        StaticContinuousAbility::DamageRule {
                                            id: ids.static_id(),
                                            rule: StaticDamageRule::ModifyDamage {
                                                source,
                                                source_controller: PlayerRelation::AnyParticipant,
                                                recipient,
                                                kind: if combat_only {
                                                    StaticDamageKind::CombatOnly
                                                } else {
                                                    StaticDamageKind::Any
                                                },
                                                op: StaticDamageOp::PreventAndFollowup {
                                                    mode: DamagePreventMode::All,
                                                    followup: DamagePreventFollowup::DealPreventedToSourceController,
                                                },
                                            },
                                        },
                                    ))),
                                    duration: ResolvedDuration::UntilEndOfTurn,
                                });
                            } else if !is_deflecting_palm_rep {
                                trial.instructions.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::PreventNextTimeDamage {
                                        source,
                                        source_chosen_color,
                                        source_chosen_type,
                                        source_is_target,
                                        half_round_down,
                                        recipient,
                                        combat_only,
                                    }),
                                    duration: ResolvedDuration::UntilEndOfTurn,
                                });
                            } else {
                                return Err("oracle:unsupported-oneshot-replacement");
                            }
                            *builder = trial;
                            return Ok(());
                        }
                    }
                }
                if let Some((actor_part, would_part_raw)) = subj_low.split_once(" would ") {
                    let actor_low = actor_part.trim();
                    let would_clean_owned = collapse_ws(
                        &would_part_raw
                            .trim()
                            .trim_end_matches(',')
                            .replace(" this turn", ""),
                    );
                    let rep_clean_raw = rep_low.trim().trim_end_matches('.').trim();
                    let rep_clean_norm = rep_clean_raw
                        .strip_prefix("you may instead ")
                        .or_else(|| rep_clean_raw.strip_prefix("instead, "))
                        .or_else(|| rep_clean_raw.strip_prefix("instead "))
                        .unwrap_or(rep_clean_raw)
                        .trim_end_matches(" instead")
                        .trim();
                    let has_source = in_trigger || self_pronoun_valid;
                    if let Some(mut ab) = try_lower_event_replacement_ability(
                        EventReplacementInput {
                            subj_low: actor_low,
                            would_norm: &would_clean_owned,
                            instead_raw: rep_clean_raw,
                            instead_norm: rep_clean_norm,
                            instead_body: instead_body.as_deref(),
                        },
                        vocabulary,
                        ids,
                    ) && (has_source || !event_replacement_uses_source(&ab))
                    {
                        // OneShotEventReplacement is consumed in try_replace_player_loss (LoseGame)
                        // and execute_inline_replacement_body (ReplaceWithBody for Draw/Life/Mill/Scry).
                        // Convert non-LoseGame Skip into an inline no-op ReplaceWithBody so first use consumes it;
                        // fail closed for ModifyAmount or event kinds without one-shot consumption.
                        let one_shot_ready = match &mut ab {
                            StaticContinuousAbility::EventReplacement {
                                event: ReplaceableEvent::LoseGame { .. },
                                action:
                                    StaticReplacementAction::Skip
                                    | StaticReplacementAction::ReplaceWithBody {
                                        optional: false, ..
                                    },
                                ..
                            } => true,
                            StaticContinuousAbility::EventReplacement {
                                event:
                                    ReplaceableEvent::Draw { .. }
                                    | ReplaceableEvent::LifeGain { .. }
                                    | ReplaceableEvent::LifeLoss { .. }
                                    | ReplaceableEvent::Mill { .. }
                                    | ReplaceableEvent::Scry { .. },
                                action,
                                ..
                            } => match action {
                                StaticReplacementAction::ReplaceWithBody { .. } => true,
                                StaticReplacementAction::Skip => {
                                    *action = StaticReplacementAction::ReplaceWithBody {
                                        optional: false,
                                        body: Box::new(ExecutableBody::new(
                                            BodyTarget::None,
                                            vec![Instruction::Jump { destination: 1 }],
                                        )),
                                    };
                                    true
                                }
                                StaticReplacementAction::ModifyAmount { .. } => false,
                            },
                            _ => false,
                        };
                        if one_shot_ready && ab.validate(vocabulary).is_ok() {
                            builder.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::OneShotEventReplacement(
                                    Box::new(ab),
                                )),
                                duration: ResolvedDuration::UntilEndOfTurn,
                            });
                            return Ok(());
                        }
                    }
                }
            }
            if ev_low.contains("combat damage") && rep_low.contains("prevent") {
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::PreventAllCombatDamage),
                    duration: ResolvedDuration::UntilEndOfTurn,
                });
                return Ok(());
            }
            if is_exile
                && (ev_low.contains("leave the battlefield")
                    || ev_low.contains("leaves the battlefield"))
            {
                let eff_subj = if matches!(
                    subj_low.as_str(),
                    "it" | "them"
                        | "that creature"
                        | "those creatures"
                        | "that permanent"
                        | "those permanents"
                        | "that token"
                        | "those tokens"
                ) {
                    if last_instruction_created_tokens(&builder.instructions) {
                        Some(EffectSubject::CreatedTokens)
                    } else if is_permanent_body_target(builder.target) {
                        Some(EffectSubject::TargetObject)
                    } else if last_instruction_moved_object(&builder.instructions) {
                        Some(EffectSubject::LastMoved)
                    } else if self_pronoun_valid {
                        Some(EffectSubject::AbilitySource)
                    } else if in_trigger {
                        Some(EffectSubject::TriggerSubject)
                    } else {
                        builder.last_group_filter.map(EffectSubject::Matching)
                    }
                } else if matches!(subj_low.as_str(), "~" | "this creature" | "this permanent") {
                    self_pronoun_valid.then_some(EffectSubject::AbilitySource)
                } else if subj_low.contains("a creature") || subj_low.contains("any creature") {
                    Some(EffectSubject::Matching(ObjectFilter::creature()))
                } else {
                    None
                };
                if let Some(eff_subj) = eff_subj {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::ExileIfLeavesBattlefield {
                            subject: Box::new(eff_subj),
                        }),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    });
                    return Ok(());
                }
            }
            if is_exile
                && (ev_low == "resolve_to_graveyard"
                    || (ev_low.contains("graveyard")
                        && (subj_low.contains("spell")
                            || ev_low.contains("spell")
                            || subj_low.contains("cast this way"))))
            {
                let prior_cast_player = builder.instructions.iter().rev().find_map(|op| match op {
                    Instruction::GrantCardPlayPermission { player, .. } => Some(*player),
                    Instruction::CreateTemporaryRule { rule, .. } => match rule.as_ref() {
                        TemporaryRuleKind::ZoneCastPermission { player, .. } => Some(*player),
                        _ => None,
                    },
                    _ => None,
                });
                let players = if subj_low.contains("you cast")
                    || subj_low.contains("your")
                    || subj_low.contains("cast this way")
                    || ev_low.contains("your graveyard")
                    || ev_low == "resolve_to_graveyard"
                {
                    AffectedPlayers::One(prior_cast_player.unwrap_or(ResolvingPlayer::Controller))
                } else if subj_low.contains("opponent") {
                    AffectedPlayers::EachOpponent
                } else {
                    AffectedPlayers::EachPlayer
                };
                let filter = if subj_low.contains("instant or sorcery")
                    || subj_low.contains("instant and sorcery")
                {
                    Some(ObjectFilter::Clause(ObjectClause::any().any_types(
                        CardTypeMask::INSTANT.union(CardTypeMask::SORCERY),
                    )))
                } else if subj_low.contains("creature") {
                    Some(ObjectFilter::creature())
                } else {
                    None
                };
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::ExileSpellOnResolve { filter, players }),
                    duration: ResolvedDuration::UntilEndOfTurn,
                });
                return Ok(());
            }
            if rep_low.starts_with("prevent the next ") || ev_low.starts_with("the next ") {
                let text = if rep_low.starts_with("prevent the next ") {
                    &rep_low
                } else {
                    &ev_low
                };
                if let Some(after_next) = text
                    .strip_prefix("prevent the next ")
                    .or_else(|| text.strip_prefix("the next "))
                {
                    let mut parts = after_next.split_whitespace();
                    let amt_tok = parts.next().unwrap_or("");
                    let combat_only = after_next.contains("combat damage");
                    let mut trial = builder.clone();
                    let has_source = in_trigger || self_pronoun_valid;
                    let after_dealt_owned = after_next.replace(" this turn", "");
                    let rec_phrase = after_dealt_owned
                        .split_once(" dealt to ")
                        .or_else(|| after_dealt_owned.split_once(" to "))
                        .map(|(_, r)| r.trim())
                        .unwrap_or("");
                    let recipient_opt = if !rec_phrase.is_empty() {
                        match parse_damage_recipient_phrase_with_target(
                            rec_phrase, &mut trial, has_source, in_trigger, vocabulary,
                        ) {
                            Some(Ok(r)) => Some(r),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if let Some(recipient) = recipient_opt {
                        if amt_tok == "x" {
                            let x_expr = match bound_x {
                                Some(expr) => lower_value_expr_in_context(
                                    expr, &trial, in_trigger, vocabulary,
                                )?,
                                None => ValueExpr::chosen_x(),
                            };
                            trial.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::PreventNextDamageExpr {
                                    amount: Box::new(x_expr),
                                    source: None,
                                    recipient,
                                    combat_only,
                                }),
                                duration: ResolvedDuration::UntilEndOfTurn,
                            });
                            *builder = trial;
                            return Ok(());
                        } else if let Some(n) = parse_small_number_word(amt_tok)
                            && n > 0
                        {
                            trial.instructions.push(Instruction::CreateTemporaryRule {
                                rule: Box::new(TemporaryRuleKind::PreventNextDamage {
                                    amount: i64::from(n).into(),
                                    source: None,
                                    recipient,
                                    combat_only,
                                }),
                                duration: ResolvedDuration::UntilEndOfTurn,
                            });
                            *builder = trial;
                            return Ok(());
                        }
                    }
                }
            }
            if is_exile
                && !rep_low.contains(" and ")
                && !rep_low.contains(" with ")
                && matches!(
                    subj_low.as_str(),
                    "a card" | "card" | "a card or token" | "a card or a token"
                )
                && (ev_low.contains("put into your graveyard from anywhere")
                    || ev_low.contains("put into an opponent's graveyard from anywhere")
                    || ev_low.contains("put into a graveyard from anywhere"))
            {
                let objects = if subj_low.contains("token") {
                    ZoneObjectKind::Either
                } else {
                    ZoneObjectKind::Card
                };
                let zr_subject = if ev_low.contains("your graveyard") {
                    ZoneReplacementSubject::Matching {
                        filter: Box::new(ObjectFilter::any()),
                        owner_relation: Some(PlayerRelation::Controller),
                    }
                } else if ev_low.contains("opponent's graveyard") {
                    ZoneReplacementSubject::Matching {
                        filter: Box::new(ObjectFilter::any()),
                        owner_relation: Some(PlayerRelation::Opponent),
                    }
                } else {
                    ZoneReplacementSubject::AnyObject
                };
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::TemporaryZoneReplacement(Box::new(
                        ZoneReplacementAbility {
                            id: ZoneReplacementAbilityId(ids.static_id().0),
                            actions: ZoneActionPredicate::Any,
                            objects,
                            subject: zr_subject,
                            destination: PublicZoneDestination::Graveyard,
                            replace_with: PublicZoneDestination::Exile,
                            choice: ZoneReplacementChoice::Mandatory,
                        },
                    ))),
                    duration: ResolvedDuration::UntilEndOfTurn,
                });
                return Ok(());
            }
            if (ev_low.contains("die") || ev_low.contains("graveyard")) && is_exile {
                let eff_subj = if matches!(
                    subj_low.as_str(),
                    "it" | "them"
                        | "that creature"
                        | "those creatures"
                        | "that permanent"
                        | "those permanents"
                        | "that creature or planeswalker"
                        | "that planeswalker"
                        | "that token"
                        | "those tokens"
                ) {
                    if last_instruction_created_tokens(&builder.instructions) {
                        Some(EffectSubject::CreatedTokens)
                    } else if is_permanent_body_target(builder.target) {
                        Some(EffectSubject::TargetObject)
                    } else if last_instruction_moved_object(&builder.instructions) {
                        Some(EffectSubject::LastMoved)
                    } else if self_pronoun_valid {
                        Some(EffectSubject::AbilitySource)
                    } else if in_trigger {
                        Some(EffectSubject::TriggerSubject)
                    } else {
                        builder.last_group_filter.map(EffectSubject::Matching)
                    }
                } else if matches!(subj_low.as_str(), "~" | "this creature" | "this permanent") {
                    self_pronoun_valid.then_some(EffectSubject::AbilitySource)
                } else if subj_low.contains("creature you control") {
                    Some(EffectSubject::Matching(
                        ObjectFilter::creature().controlled(),
                    ))
                } else if subj_low.contains("creature an opponent controls") {
                    Some(EffectSubject::Matching(ObjectFilter::creature().opposing()))
                } else if subj_low.contains("nontoken creature") {
                    Some(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause {
                            token: Some(false),
                            ..ObjectClause::creature()
                        },
                    )))
                } else if subj_low.contains("creature or planeswalker") {
                    Some(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause::any()
                            .any_types(CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER)),
                    )))
                } else if subj_low.contains("planeswalker") {
                    Some(EffectSubject::Matching(ObjectFilter::planeswalker()))
                } else if subj_low.is_empty()
                    || subj_low.contains("a creature")
                    || subj_low.contains("any creature")
                {
                    Some(EffectSubject::Matching(ObjectFilter::creature()))
                } else {
                    None
                };
                if let Some(eff_subj) = eff_subj {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::ExileIfDies {
                            subject: Box::new(eff_subj),
                        }),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    });
                    return Ok(());
                }
            }
            if let Some(res) = try_lower_redirect_next_damage(
                &subj_low,
                &ev_low,
                &rep_low,
                builder,
                in_trigger,
                in_trigger || self_pronoun_valid,
                self_pronoun_valid,
                bound_x,
                vocabulary,
            ) {
                return res;
            }
            let ev_norm_owned = ev_low.trim_end_matches('.').replace(" this turn", "");
            let ev_norm = ev_norm_owned.trim();
            let rep_no_dot = rep_low.trim_end_matches('.').trim();
            let rep_no_instead = rep_no_dot
                .strip_suffix(" instead")
                .unwrap_or(rep_no_dot)
                .trim();
            let rep_norm_owned = rep_no_instead.replace(" this turn", "");
            let rep_norm = rep_norm_owned.trim();
            let parsed_dmg_rule = if let Some(rule) =
                parse_modify_damage_event(&subj_low, ev_norm, rep_norm, vocabulary)
            {
                Some(rule)
            } else if rep_norm.contains("that much damage plus x") {
                let probe_rep =
                    rep_norm.replacen("that much damage plus x", "that much damage plus 1", 1);
                if let Some(StaticDamageRule::ModifyDamage {
                    source,
                    source_controller,
                    recipient,
                    kind,
                    op: StaticDamageOp::Add(1),
                }) = parse_modify_damage_event(&subj_low, ev_norm, &probe_rep, vocabulary)
                {
                    let x_val = lower_value_expr_in_context(
                        resolve_x(&ValueExprAst::ChosenX, bound_x),
                        builder,
                        in_trigger,
                        vocabulary,
                    )?;
                    Some(StaticDamageRule::ModifyDamage {
                        source,
                        source_controller,
                        recipient,
                        kind,
                        op: StaticDamageOp::AddExpr(Box::new(x_val)),
                    })
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(rule) = parsed_dmg_rule {
                let recipient_ok = (in_trigger || self_pronoun_valid)
                    || !matches!(
                        rule,
                        StaticDamageRule::ModifyDamage {
                            recipient: StaticDamageRecipient::SelfObject
                                | StaticDamageRecipient::AttachedHost,
                            ..
                        }
                    );
                if recipient_ok {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                            StaticContinuousAbility::DamageRule {
                                id: ids.static_id(),
                                rule,
                            },
                        ))),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    });
                    return Ok(());
                }
            }
            {
                let mut scratch = Program::new(
                    BTreeSet::from([CardType::Enchantment]),
                    BTreeSet::new(),
                    BTreeSet::new(),
                    ProgramKind::Permanent(PermanentDefinition {
                        entry_tap: None,
                        attachment: None,
                        printed_pt: None,
                        casting_cost: Some(ManaCost::free()),
                    }),
                );
                let mut scratch_ids = LoweringIds::default();
                let synth_instead = if rep_norm == "exile" || (rep_norm.is_empty() && body_exiles) {
                    "exile it".to_owned()
                } else {
                    rep_norm.to_owned()
                };
                let synth_repl = ReplacementEffectAst::EventInstead {
                    subject_or_event: subject.clone(),
                    would: ev_norm.to_owned(),
                    instead: synth_instead.clone(),
                    instead_body: instead_body.clone(),
                };
                let lowered_ok =
                    lower_replacement(&synth_repl, &mut scratch, vocabulary, &mut scratch_ids)
                        .is_ok()
                        || (ev_low.contains(" this turn") && {
                            let synth_with_turn = ReplacementEffectAst::EventInstead {
                                subject_or_event: subject.clone(),
                                would: ev_low.trim_end_matches('.').to_owned(),
                                instead: synth_instead,
                                instead_body: instead_body.clone(),
                            };
                            lower_replacement(
                                &synth_with_turn,
                                &mut scratch,
                                vocabulary,
                                &mut scratch_ids,
                            )
                            .is_ok()
                        });
                if lowered_ok
                    && scratch.triggered.is_empty()
                    && scratch.activated.is_empty()
                    && scratch.mana.is_empty()
                    && scratch.entry_counter_abilities.is_empty()
                    && scratch.entry_tap_unless.is_none()
                    && scratch.entry_copy.is_none()
                    && scratch.conditional_cost.is_none()
                    && scratch.static_conditions.is_empty()
                    && scratch.attack_requirement.is_none()
                    && scratch.additional_attack_requirements.is_empty()
                    && !scratch.enters_prepared
                    && !scratch.cannot_be_countered
                    && !scratch.opening_hand_battlefield
                    && matches!(
                        scratch.kind,
                        ProgramKind::Permanent(PermanentDefinition {
                            entry_tap: None,
                            attachment: None,
                            printed_pt: None,
                            ..
                        })
                    )
                {
                    let zone_repls_valid = scratch.zone_replacements.is_empty()
                        || scratch.validate(vocabulary).is_ok();
                    if zone_repls_valid {
                        let abilities: Vec<StaticContinuousAbility> = scratch
                            .continuous
                            .into_iter()
                            .chain(scratch.additional_battlefield_abilities)
                            .collect();
                        let zone_repls = scratch.zone_replacements;
                        let mut emitted = Vec::new();
                        let abilities_ok = if abilities.is_empty() {
                            true
                        } else if let Some(lowered_ops) = lower_temp_static_abilities_to_ops(
                            abilities,
                            ResolvedDuration::UntilEndOfTurn,
                            builder,
                            in_trigger,
                            self_pronoun_valid,
                        ) {
                            emitted.extend(lowered_ops);
                            true
                        } else {
                            false
                        };
                        if abilities_ok {
                            for zr in zone_repls {
                                emitted.push(Instruction::CreateTemporaryRule {
                                    rule: Box::new(TemporaryRuleKind::TemporaryZoneReplacement(
                                        Box::new(zr),
                                    )),
                                    duration: ResolvedDuration::UntilEndOfTurn,
                                });
                            }
                            if !emitted.is_empty() {
                                builder.instructions.extend(emitted);
                                return Ok(());
                            }
                        }
                    }
                }
            }
            Err("oracle:unsupported-oneshot-replacement")
        }
        InstructionAst::DelayedEventTrigger { trigger, body } => {
            let (mut spec, mut grouping, guard) = lower_trigger_spec(trigger, vocabulary)?;
            let stripped = strip_triggered_restrictions(body, &mut spec, &mut grouping, vocabulary);
            let self_pron = trigger_subject_is_self(trigger);
            let mut exec_body =
                lower_trigger_effect_body(&stripped, vocabulary, ids, &spec, self_pron)?;
            if let Some(cond) = guard {
                prepend_instruction_at(
                    &mut exec_body.instructions,
                    0,
                    Instruction::RequireCondition(cond),
                );
            }
            apply_trigger_body_rewrites(&mut exec_body, &spec, grouping, self_pron);
            let once = !trigger.events.is_empty()
                && trigger.events.iter().all(|ev| {
                    matches!(
                        ev,
                        TriggerEventAst::ZoneChange { subject, .. }
                            if matches!(
                                subject.as_ref(),
                                SubjectRefAst::SelfObject
                                    | SubjectRefAst::Anaphora(_)
                                    | SubjectRefAst::TargetObject { .. }
                            )
                    )
                });
            builder
                .instructions
                .push(Instruction::ScheduleDelayedEventTrigger {
                    trigger: Box::new(spec),
                    body: Box::new(exec_body),
                    expires: ResolvedDuration::UntilEndOfTurn,
                    once,
                });
            Ok(())
        }
        InstructionAst::WinGame { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::WinGame(ResolvingPlayer::LoopPlayer)],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::WinGame(rp));
            Ok(())
        }
        InstructionAst::LoseGame { player } => {
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::LoseGame(players));
            Ok(())
        }
        InstructionAst::EntersTappedRule {
            subject,
            tapped: true,
            attacking: None,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let sel = lower_subject_to_permanent_selector(subject, vocabulary)?;
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OthersEnterTapped(Box::new(sel)),
                    },
                ))),
                duration: dur,
            });
            Ok(())
        }
        InstructionAst::ForAsLongAs { condition, body } => {
            let is_remains_exiled = matches!(
                condition.as_ref(),
                ConditionAst::Marker(
                    spellwake_oracle_compiler::ast::GameMarker::RemainsExiled
                        | spellwake_oracle_compiler::ast::GameMarker::SubjectRemainsExiled { .. }
                )
            ) || matches!(
                condition.as_ref(),
                ConditionAst::GameStateFlag(flag)
                    if matches!(
                        flag.trim().trim_end_matches('.').to_ascii_lowercase().as_str(),
                        "it remains exiled"
                            | "those cards remain exiled"
                            | "they remain exiled"
                    )
            );
            if is_remains_exiled {
                match body.as_slice() {
                    [
                        InstructionAst::GrantCastPermission {
                            player,
                            cast_only,
                            subject,
                            free,
                            duration: _,
                            count_limit,
                            alt_cost,
                        },
                    ] => {
                        return lower_instruction(
                            &InstructionAst::GrantCastPermission {
                                player: *player,
                                cast_only: *cast_only,
                                subject: subject.clone(),
                                free: *free,
                                duration: "while_exiled".to_string(),
                                count_limit: count_limit.clone(),
                                alt_cost: alt_cost.clone(),
                            },
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        );
                    }
                    [InstructionAst::GrantModifiedCastPermission(perm)] => {
                        let mut cloned = perm.as_ref().clone();
                        cloned.duration = "while_exiled".to_string();
                        return lower_instruction(
                            &InstructionAst::GrantModifiedCastPermission(Box::new(cloned)),
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        );
                    }
                    _ => {}
                }
            }
            if (self_pronoun_valid || in_trigger)
                && !body.is_empty()
                && let Some(as_long_dur) = condition_to_for_as_long_as_duration(condition)
            {
                let mut rewritten = Vec::with_capacity(body.len());
                let mut all_ok = true;
                for ins in body {
                    if let Some(r) = rewrite_instruction_duration(ins, &as_long_dur) {
                        rewritten.push(r);
                    } else {
                        all_ok = false;
                        break;
                    }
                }
                if all_ok {
                    return lower_instruction_seq(
                        &rewritten,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
            }
            Err("oracle:unsupported-for-as-long-as")
        }
        InstructionAst::TemporaryTrigger {
            trigger,
            body,
            duration,
        } => {
            let expires = lower_duration(duration)?;
            if trigger_spec_has_unbound_target_or_anaphora(
                trigger,
                self_pronoun_valid || in_trigger,
            ) {
                return Err("oracle:unsupported-temp-trigger");
            }
            if builder.target != BodyTarget::None
                && body.iter().any(instruction_references_anaphora)
            {
                return Err("oracle:unsupported-temp-trigger");
            }
            let (mut spec, mut grouping, guard) = lower_trigger_spec(trigger, vocabulary)?;
            let stripped = strip_triggered_restrictions(body, &mut spec, &mut grouping, vocabulary);
            let self_pron = trigger_subject_is_self(trigger);
            let mut exec_body =
                lower_trigger_effect_body(&stripped, vocabulary, ids, &spec, self_pron)?;
            if let Some(cond) = guard {
                prepend_instruction_at(
                    &mut exec_body.instructions,
                    0,
                    Instruction::RequireCondition(cond),
                );
            }
            apply_trigger_body_rewrites(&mut exec_body, &spec, grouping, self_pron);
            builder
                .instructions
                .push(Instruction::ScheduleDelayedEventTrigger {
                    trigger: Box::new(spec),
                    body: Box::new(exec_body),
                    expires,
                    once: false,
                });
            Ok(())
        }
        InstructionAst::WhenYouNext {
            event,
            body,
            duration,
        } if !body.is_empty() => {
            let ev_trimmed = event.trim().trim_end_matches('.').to_ascii_lowercase();
            let (ev_norm, had_this_turn) = match ev_trimmed.strip_suffix(" this turn") {
                Some(prefix) => (prefix.trim(), true),
                None => (ev_trimmed.as_str(), false),
            };
            let expires = if had_this_turn && matches!(duration, DurationAst::Permanent) {
                ResolvedDuration::UntilEndOfTurn
            } else {
                lower_duration(duration)?
            };
            let is_copy_body = |expected_count: i32, allow_ability: bool| -> bool {
                match body.as_slice() {
                    [
                        InstructionAst::Copy {
                            subject,
                            exception: None,
                            count,
                        },
                    ]
                    | [
                        InstructionAst::Copy {
                            subject,
                            exception: None,
                            count,
                        },
                        InstructionAst::MayChooseNewTargetsForCopy,
                    ] => {
                        let cnt_ok = match count.as_deref() {
                            None => expected_count == 1,
                            Some(ValueExprAst::Const(n)) => *n == expected_count,
                            _ => false,
                        };
                        if !cnt_ok {
                            return false;
                        }
                        matches!(subject.as_ref(), SubjectRefAst::Anaphora(raw) if {
                            let low = raw.trim().to_ascii_lowercase();
                            matches!(low.as_str(), "that spell" | "it")
                                || (allow_ability
                                    && matches!(
                                        low.as_str(),
                                        "that ability" | "that spell or ability"
                                    ))
                        })
                    }
                    _ => false,
                }
            };
            if matches!(
                ev_norm,
                "cast an instant spell, cast a sorcery spell, or activate a loyalty ability"
                    | "cast an instant or sorcery spell or activate a loyalty ability"
                    | "cast an instant or sorcery spell, or activate a loyalty ability"
            ) {
                if expires == ResolvedDuration::UntilEndOfTurn
                    && builder.target == BodyTarget::None
                    && !in_trigger
                    && is_copy_body(2, true)
                {
                    builder
                        .instructions
                        .push(Instruction::WatchNextSpellOrLoyaltyTwice);
                    return Ok(());
                }
                return Err("oracle:unsupported-when-you-next");
            }
            if matches!(
                ev_norm,
                "activate_loyalty_ability"
                    | "activate a loyalty ability"
                    | "activate a loyalty ability of a planeswalker you control"
                    | "activate a loyalty ability of a planeswalker"
            ) {
                if expires == ResolvedDuration::UntilEndOfTurn
                    && builder.target == BodyTarget::None
                    && in_trigger
                    && is_copy_body(1, true)
                {
                    builder.instructions.push(Instruction::WatchNextLoyaltyCopy);
                    return Ok(());
                }
                return Err("oracle:unsupported-when-you-next");
            }
            if let Some(after_cast) = ev_norm.strip_prefix("cast ")
                && !after_cast.contains(',')
                && !after_cast.contains(" from ")
                && !after_cast.contains(" with ")
                && !after_cast.contains(" without ")
                && !after_cast.contains(" targeting ")
                && !after_cast.contains(" that ")
                && !after_cast.contains(" where ")
                && !after_cast.contains(" during ")
                && !after_cast.contains(" if ")
                && !after_cast.contains(" other ")
                && !after_cast.contains(" another ")
                && !after_cast.contains(" unless ")
            {
                let phrase = after_cast
                    .strip_prefix("your next ")
                    .or_else(|| after_cast.strip_prefix("the next "))
                    .unwrap_or(after_cast)
                    .trim();
                let spell_filter = if matches!(phrase, "a spell" | "spell") {
                    Some(None)
                } else if phrase.ends_with(" spell")
                    && let Ok(mut parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                        phrase,
                        &cached_compiler_vocab(vocabulary),
                    )
                    && parsed.zone.is_none()
                    && parsed.extra_zones.is_empty()
                    && parsed.predicates.is_empty()
                    && !parsed.is_ability
                    && parsed.named.is_none()
                    && parsed.controller.is_none()
                    && parsed.owner.is_none()
                {
                    parsed.any_spell = false;
                    Some(Some(Box::new(parsed)))
                } else {
                    None
                };
                if let Some(spell) = spell_filter {
                    if builder.target != BodyTarget::None
                        && body.iter().any(instruction_references_anaphora)
                    {
                        return Err("oracle:unsupported-when-you-next");
                    }
                    let trigger_ast = TriggerSpecAst {
                        events: vec![TriggerEventAst::SpellCast {
                            caster: PlayerRefAst::You,
                            spell,
                            self_spell: false,
                            targets: None,
                            nth_each_turn: None,
                            include_copy: false,
                        }],
                        intervening_if: None,
                    };
                    let (mut spec, mut grouping, guard) =
                        lower_trigger_spec(&trigger_ast, vocabulary)?;
                    let stripped =
                        strip_triggered_restrictions(body, &mut spec, &mut grouping, vocabulary);
                    let mut exec_body =
                        lower_trigger_effect_body(&stripped, vocabulary, ids, &spec, false)?;
                    if let Some(cond) = guard {
                        prepend_instruction_at(
                            &mut exec_body.instructions,
                            0,
                            Instruction::RequireCondition(cond),
                        );
                    }
                    apply_trigger_body_rewrites(&mut exec_body, &spec, grouping, false);
                    builder
                        .instructions
                        .push(Instruction::ScheduleDelayedEventTrigger {
                            trigger: Box::new(spec),
                            body: Box::new(exec_body),
                            expires,
                            once: true,
                        });
                    return Ok(());
                }
            }
            Err("oracle:unsupported-when-you-next")
        }
        InstructionAst::ScopedDuration {
            duration: DurationAst::UntilEndOfTurn,
            body,
        } if matches!(body.as_slice(), [InstructionAst::RetainManaUntilEndOfTurn]) => {
            // This clause refers to the mana just produced, not all mana in the player's pool.
            // Reuse the sequence folder, but reject its global-retention fallback without a producer.
            let mut trial = builder.clone();
            let mut trial_ids = LoweringIds::default();
            lower_instruction_seq(
                body,
                &mut trial,
                vocabulary,
                &mut trial_ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if trial.instructions.len() != builder.instructions.len() {
                return Err("oracle:unsupported-scoped-duration");
            }
            *builder = trial;
            Ok(())
        }
        InstructionAst::ScopedDuration { duration, body } if !body.is_empty() => {
            let mut rewritten = Vec::with_capacity(body.len());
            for ins in body {
                let r = rewrite_instruction_duration(ins, duration)
                    .ok_or("oracle:unsupported-scoped-duration")?;
                rewritten.push(r);
            }
            lower_instruction_seq(
                &rewritten,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
        }
        InstructionAst::ReplaceEffectWith(rew) if !rew.only => {
            let instead_ok = rew.instead_of.as_deref().is_none_or(|s| {
                let low = s.trim().to_ascii_lowercase();
                low.is_empty() || low.contains("graveyard")
            });
            if instead_ok
                && matches!(builder.target, BodyTarget::SingleSpell(_))
                && builder.instructions.last() == Some(&Instruction::CounterSpell)
            {
                let extract_lib_top_choice = |ins: &InstructionAst| -> Option<Option<bool>> {
                    let (subj, dest_raw) = match ins {
                        InstructionAst::MoveZone { subject, to } => (subject.as_ref(), to.as_str()),
                        InstructionAst::ReturnZone {
                            subject,
                            to,
                            tapped: false,
                            transformed: false,
                            with_counter: None,
                            attached_to: None,
                            random: false,
                            except: None,
                            count: None,
                            up_to: false,
                            any_number: false,
                            ..
                        } => (subject.as_ref(), to.as_str()),
                        _ => return None,
                    };
                    if !matches!(subj, SubjectRefAst::Anaphora(_)) {
                        return None;
                    }
                    let d = dest_raw.trim().to_ascii_lowercase();
                    if (d.contains("top or bottom") || d.contains("top or the bottom"))
                        && (d.contains("library") || d.contains("owner"))
                    {
                        return Some(None);
                    }
                    if d == "top_of_library"
                        || d.contains("top of its owner's library")
                        || d.contains("top of that player's library")
                    {
                        return Some(Some(true));
                    }
                    if d == "bottom_of_library"
                        || d.contains("bottom of its owner's library")
                        || d.contains("bottom of that player's library")
                    {
                        return Some(Some(false));
                    }
                    None
                };
                match rew.body.as_slice() {
                    [
                        InstructionAst::Exile {
                            actor: None | Some(PlayerRefAst::You),
                            subject,
                            random: false,
                            face_down: false,
                            into_pile: false,
                        },
                    ] if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                        if let Some(last) = builder.instructions.last_mut() {
                            *last = Instruction::CounterSpellToExile;
                            return Ok(());
                        }
                    }
                    [single] if extract_lib_top_choice(single).is_some() => {
                        match extract_lib_top_choice(single) {
                            Some(Some(top)) => {
                                if let Some(last) = builder.instructions.last_mut() {
                                    *last = Instruction::CounterSpellToLibrary { top };
                                    return Ok(());
                                }
                            }
                            Some(None) => {
                                builder.instructions.pop();
                                builder.instructions.push(Instruction::ChooseOption {
                                    chooser: ResolvingPlayer::Controller,
                                    options: vec![
                                        ChooseOptionBranch {
                                            label: "Top of owner's library".to_owned(),
                                            body: ExecutableBody::new(
                                                BodyTarget::None,
                                                vec![Instruction::CounterSpellToLibrary {
                                                    top: true,
                                                }],
                                            ),
                                        },
                                        ChooseOptionBranch {
                                            label: "Bottom of owner's library".to_owned(),
                                            body: ExecutableBody::new(
                                                BodyTarget::None,
                                                vec![Instruction::CounterSpellToLibrary {
                                                    top: false,
                                                }],
                                            ),
                                        },
                                    ],
                                });
                                return Ok(());
                            }
                            None => {}
                        }
                    }
                    [InstructionAst::Choice(branches)] if branches.len() == 2 => {
                        if let (Some(Some(a)), Some(Some(b))) = (
                            branches[0]
                                .first()
                                .filter(|_| branches[0].len() == 1)
                                .and_then(extract_lib_top_choice),
                            branches[1]
                                .first()
                                .filter(|_| branches[1].len() == 1)
                                .and_then(extract_lib_top_choice),
                        ) && a != b
                        {
                            builder.instructions.pop();
                            builder.instructions.push(Instruction::ChooseOption {
                                chooser: ResolvingPlayer::Controller,
                                options: vec![
                                    ChooseOptionBranch {
                                        label: "Top of owner's library".to_owned(),
                                        body: ExecutableBody::new(
                                            BodyTarget::None,
                                            vec![Instruction::CounterSpellToLibrary { top: true }],
                                        ),
                                    },
                                    ChooseOptionBranch {
                                        label: "Bottom of owner's library".to_owned(),
                                        body: ExecutableBody::new(
                                            BodyTarget::None,
                                            vec![Instruction::CounterSpellToLibrary { top: false }],
                                        ),
                                    },
                                ],
                            });
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }
            Err("oracle:unsupported-replace-effect-with")
        }
        InstructionAst::SkipNextTurn { player } => {
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::SkipNextTurns {
                        player: ResolvingPlayer::LoopPlayer,
                        count: 1,
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::SkipNextTurns {
                player: rp,
                count: 1,
            });
            Ok(())
        }
        InstructionAst::SkipNextStep { player, step } => {
            use spellwake_engine::turn::Step;
            use spellwake_oracle_compiler::ast::TurnStepAst;
            let engine_step = match step {
                TurnStepAst::UntapStep => Step::Untap,
                TurnStepAst::Upkeep => Step::Upkeep,
                TurnStepAst::DrawStep => Step::Draw,
                TurnStepAst::PrecombatMain => Step::PrecombatMain,
                TurnStepAst::PostcombatMain => Step::PostcombatMain,
                TurnStepAst::BeginningOfCombat => Step::BeginningCombat,
                TurnStepAst::EndStep => Step::End,
                TurnStepAst::MainPhase | TurnStepAst::EndOfCombat | TurnStepAst::CleanupStep => {
                    return Err("oracle:unsupported-instruction");
                }
            };
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::SkipNextStep {
                        player: ResolvingPlayer::LoopPlayer,
                        step: engine_step,
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::SkipNextStep {
                player: rp,
                step: engine_step,
            });
            Ok(())
        }
        InstructionAst::SkipNextTurns { player, count } => {
            if *count == 0 {
                return Err("oracle:unsupported-skip-next-turns");
            }
            if let Some(players) = try_group_affected_players(*player) {
                push_for_each_player(
                    players,
                    vec![Instruction::SkipNextTurns {
                        player: ResolvingPlayer::LoopPlayer,
                        count: *count,
                    }],
                    builder,
                );
                return Ok(());
            }
            let rp = lower_resolving_player_in_context(*player, builder, in_trigger)?;
            builder.instructions.push(Instruction::SkipNextTurns {
                player: rp,
                count: *count,
            });
            Ok(())
        }
        InstructionAst::ScopedDurationText {
            duration_text,
            body,
        } if !body.is_empty() => {
            let has_source = in_trigger || self_pronoun_valid || builder.has_source;
            let norm = duration_text
                .trim()
                .trim_end_matches(',')
                .to_ascii_lowercase();
            let step_cast_slug = match norm.as_str() {
                "until your next end step" | "until the beginning of your next end step" => {
                    Some("until_your_next_end_step")
                }
                "until that player's next end step" | "until their next end step" => {
                    Some("until_their_next_end_step")
                }
                "until the end of your next end step" => {
                    Some("until the end of your next end step")
                }
                "until the end of that player's next end step"
                | "until the end of their next end step" => {
                    Some("until the end of their next end step")
                }
                "until your next upkeep" | "until the beginning of your next upkeep" => {
                    Some("until_your_next_upkeep")
                }
                "until that player's next upkeep" | "until their next upkeep" => {
                    Some("until_their_next_upkeep")
                }
                _ => None,
            };
            if let Some(slug) = step_cast_slug {
                let mut rewritten = Vec::with_capacity(body.len());
                let mut all_ok = true;
                for ins in body {
                    if let Some(r) = rewrite_grant_cast_duration_slug(ins, slug) {
                        rewritten.push(r);
                    } else {
                        all_ok = false;
                        break;
                    }
                }
                if all_ok {
                    return lower_instruction_seq(
                        &rewritten,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    );
                }
            }
            let duration = match norm.as_str() {
                "until end of turn" | "this turn" => DurationAst::UntilEndOfTurn,
                "until your next turn" => DurationAst::UntilYourNextTurn,
                "until end of combat" => DurationAst::UntilEndOfCombat,
                "until the end of your next turn" => DurationAst::UntilEndOfNextTurn,
                "until the end of that player's next turn" | "until the end of their next turn" => {
                    DurationAst::UntilEndOfPlayersNextTurn(PlayerRefAst::ThatPlayer)
                }
                "for as long as ~ remains on the battlefield"
                | "while ~ remains on the battlefield"
                    if has_source =>
                {
                    DurationAst::ForAsLongAs(Box::new(ConditionAst::Marker(
                        GameMarker::RemainsOnBattlefield,
                    )))
                }
                "as long as you control ~" | "for as long as you control ~" if has_source => {
                    DurationAst::ForAsLongAs(Box::new(ConditionAst::Marker(
                        GameMarker::ControlsSelf,
                    )))
                }
                _ => return Err("oracle:unsupported-scoped-duration"),
            };
            lower_instruction(
                &InstructionAst::ScopedDuration {
                    duration,
                    body: body.clone(),
                },
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
        }
        InstructionAst::TemporaryPlayerRestriction {
            player,
            subject,
            rule,
            duration,
        } => {
            let dur = lower_duration(duration)?;
            let rule_low = rule.trim().to_ascii_lowercase();
            if rule_low == "cant_sacrifice"
                && let Some(subj) = subject.as_deref()
            {
                let mut sel = lower_permanent_selector(subj, vocabulary)
                    .map_err(|_| "oracle:unsupported-temp-player-restriction")?;
                let ctrl_ok = match (*player, sel.controller) {
                    (PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer, _) => true,
                    (PlayerRefAst::You, ControllerSelector::SourceController) => true,
                    (PlayerRefAst::You, ControllerSelector::Any) => {
                        sel.controller = ControllerSelector::SourceController;
                        true
                    }
                    (
                        PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent,
                        ControllerSelector::Opponents,
                    ) => true,
                    (
                        PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent,
                        ControllerSelector::Any,
                    ) => {
                        sel.controller = ControllerSelector::Opponents;
                        true
                    }
                    _ => false,
                };
                if ctrl_ok {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::CannotBeSacrificed(Box::new(sel)),
                            },
                        ))),
                        duration: dur,
                    });
                    return Ok(());
                }
                return Err("oracle:unsupported-temp-player-restriction");
            }
            if subject.is_some() {
                return Err("oracle:unsupported-temp-player-restriction");
            }
            if rule_low == "play_with_hand_revealed" {
                let perm_opt = match *player {
                    PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent => {
                        Some(CastPermissionRule::RevealHands {
                            opponents_only: true,
                        })
                    }
                    PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer => {
                        Some(CastPermissionRule::RevealHands {
                            opponents_only: false,
                        })
                    }
                    _ => None,
                };
                if let Some(permission) = perm_opt {
                    builder.instructions.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(
                            StaticContinuousAbility::CastPermission {
                                id: ids.static_id(),
                                permission,
                            },
                        ))),
                        duration: dur,
                    });
                    return Ok(());
                }
                return Err("oracle:unsupported-temp-player-restriction");
            }
            let players = lower_affected_players_in_context(*player, builder, in_trigger)?;
            let kind = match rule_low.as_str() {
                "cant_gain_life" => TemporaryRuleKind::PlayerCantGainLife { players },
                "cant_play_lands" => TemporaryRuleKind::CantPlayLands { players },
                "cant_cast_spells" => TemporaryRuleKind::CantCastSpells {
                    players,
                    filter: None,
                },
                "cant_activate_abilities" => TemporaryRuleKind::CantActivateAbilities {
                    players,
                    nonmana_only: false,
                },
                "cant_activate_nonmana_abilities" => TemporaryRuleKind::CantActivateAbilities {
                    players,
                    nonmana_only: true,
                },
                _ => return Err("oracle:unsupported-temp-player-restriction"),
            };
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(kind),
                duration: dur,
            });
            Ok(())
        }
        _ => Err("oracle:unsupported-instruction"),
    }
}

#[allow(clippy::too_many_arguments)]
fn try_lower_temp_exile_replacement(
    subj_low: &str,
    ev_low: &str,
    rep_low: &str,
    instead_body: Option<&[InstructionAst]>,
    dur: ResolvedDuration,
    builder: &mut BodyBuilder,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> bool {
    let body_exiles = instead_body.is_some_and(|b| {
        b.iter()
            .any(|op| matches!(op, InstructionAst::Exile { .. }))
    });
    let is_exile = rep_low.contains("exile") || body_exiles;
    if !is_exile {
        return false;
    }
    let has_extra_rep = rep_low.contains(" and ")
        || rep_low.contains(" with ")
        || (rep_low.contains(" instead of putting it into")
            && !rep_low.contains("graveyard")
            && !rep_low.contains("anywhere else"));
    if has_extra_rep {
        return false;
    }
    let is_die_or_gy = ev_low.contains("die") || ev_low.contains("graveyard");
    let is_ltb =
        ev_low.contains("leave the battlefield") || ev_low.contains("leaves the battlefield");
    if is_die_or_gy
        && (subj_low.contains("dealt damage this way")
            || ev_low.contains("dealt damage this way")
            || subj_low.contains("dealt damage by ~")
            || subj_low.contains("dealt damage by this spell")
            || ((self_pronoun_valid || in_trigger)
                && (subj_low.contains("dealt damage by this creature")
                    || subj_low.contains("dealt damage by this permanent"))))
    {
        if dur == ResolvedDuration::UntilEndOfTurn
            && (subj_low.contains("dealt damage this way")
                || ev_low.contains("dealt damage this way"))
            && let Some(last) = builder.instructions.last_mut()
        {
            match last.clone() {
                Instruction::Damage { amount } => {
                    *last = Instruction::DamageAmountThenExileIfDies(ValueExpr::Literal(amount));
                    return true;
                }
                Instruction::DamageAmount(expr) => {
                    *last = Instruction::DamageAmountThenExileIfDies(expr);
                    return true;
                }
                _ => {}
            }
        }
        let has_damage_op = ((self_pronoun_valid || in_trigger)
            && (subj_low.contains("dealt damage by ~")
                || subj_low.contains("dealt damage by this creature")
                || subj_low.contains("dealt damage by this permanent")))
            || builder.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::Damage { .. }
                        | Instruction::DamageAmount(_)
                        | Instruction::DamageAmountThenExileIfDies(_)
                        | Instruction::DamageGroup { .. }
                        | Instruction::DamageGroupAmount { .. }
                        | Instruction::DamageOpponentsAmount(_)
                        | Instruction::DamageRelatedPlayerAmount(_)
                        | Instruction::DamageOrderedTargets { .. }
                        | Instruction::DamageFromPower(_)
                        | Instruction::DamageFromGrantingPermanent { .. }
                        | Instruction::DamageDamagedPlayersCreatures
                        | Instruction::DamageTargetControllersCreatures { .. }
                        | Instruction::DamageCapturedBlocker(_)
                        | Instruction::DamageCreatureAtSpellMastery { .. }
                        | Instruction::DealDamageFrom { .. }
                        | Instruction::DealDamageToAnnouncedTargets { .. }
                        | Instruction::AffectPermanentTargets {
                            action: PermanentTargetAction::Damage(_),
                            ..
                        }
                        | Instruction::DivideDamageAmongTargets { .. }
                        | Instruction::DivideDamageExprAmongTargets { .. }
                        | Instruction::CreatureDealsPowerDamageToOther { .. }
                        | Instruction::Fight { .. }
                        | Instruction::FightSourceWithTarget
                        | Instruction::FightFixedTargets
                        | Instruction::FightPairedCreatures(_)
                )
            });
        if has_damage_op {
            let any_types = if subj_low.contains("creature or planeswalker") {
                Some(CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER))
            } else if subj_low.contains("planeswalker") {
                Some(CardTypeMask::PLANESWALKER)
            } else if subj_low.contains("creature") {
                Some(CardTypeMask::CREATURE)
            } else if subj_low.contains("permanent") {
                Some(CardTypeMask::PERMANENT)
            } else {
                None
            };
            if let Some(any_types) = any_types {
                let dmg_flag = if self_pronoun_valid
                    || in_trigger
                    || subj_low.contains("dealt damage by ~")
                    || subj_low.contains("dealt damage by this spell")
                {
                    ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN
                } else {
                    ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN
                };
                let clause = ObjectClause {
                    any_types,
                    flags: dmg_flag,
                    ..ObjectClause::any()
                };
                builder.instructions.push(Instruction::CreateTemporaryRule {
                    rule: Box::new(TemporaryRuleKind::ExileIfDies {
                        subject: Box::new(EffectSubject::Matching(ObjectFilter::Clause(clause))),
                    }),
                    duration: dur,
                });
                return true;
            }
        }
    }
    if is_ltb || is_die_or_gy {
        let eff_subj = if matches!(
            subj_low,
            "it" | "them"
                | "that creature"
                | "those creatures"
                | "that permanent"
                | "those permanents"
                | "that creature or planeswalker"
                | "that planeswalker"
                | "that token"
                | "those tokens"
        ) {
            if last_instruction_created_tokens(&builder.instructions) {
                Some(EffectSubject::CreatedTokens)
            } else if is_permanent_body_target(builder.target) {
                Some(EffectSubject::TargetObject)
            } else if last_instruction_moved_object(&builder.instructions) {
                Some(EffectSubject::LastMoved)
            } else if self_pronoun_valid {
                Some(EffectSubject::AbilitySource)
            } else if in_trigger {
                Some(EffectSubject::TriggerSubject)
            } else {
                builder.last_group_filter.map(EffectSubject::Matching)
            }
        } else if matches!(subj_low, "~" | "this creature" | "this permanent") {
            self_pronoun_valid.then_some(EffectSubject::AbilitySource)
        } else if matches!(
            subj_low,
            "a creature you control" | "creatures you control" | "the creature you control"
        ) {
            Some(EffectSubject::Matching(
                ObjectFilter::creature().controlled(),
            ))
        } else if matches!(
            subj_low,
            "a creature an opponent controls"
                | "creatures an opponent controls"
                | "the creature an opponent controls"
                | "a creature you don't control"
                | "creatures you don't control"
        ) {
            Some(EffectSubject::Matching(ObjectFilter::creature().opposing()))
        } else if matches!(subj_low, "a nontoken creature" | "nontoken creatures") {
            Some(EffectSubject::Matching(ObjectFilter::Clause(
                ObjectClause {
                    token: Some(false),
                    ..ObjectClause::creature()
                },
            )))
        } else if matches!(
            subj_low,
            "a creature or planeswalker" | "any creature or planeswalker"
        ) {
            Some(EffectSubject::Matching(ObjectFilter::Clause(
                ObjectClause::any()
                    .any_types(CardTypeMask::CREATURE.union(CardTypeMask::PLANESWALKER)),
            )))
        } else if matches!(subj_low, "a planeswalker" | "any planeswalker") {
            Some(EffectSubject::Matching(ObjectFilter::planeswalker()))
        } else if matches!(subj_low, "a creature" | "any creature") {
            Some(EffectSubject::Matching(ObjectFilter::creature()))
        } else {
            None
        };
        if let Some(eff_subj) = eff_subj {
            let rule = if is_ltb {
                TemporaryRuleKind::ExileIfLeavesBattlefield {
                    subject: Box::new(eff_subj),
                }
            } else {
                TemporaryRuleKind::ExileIfDies {
                    subject: Box::new(eff_subj),
                }
            };
            builder.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(rule),
                duration: dur,
            });
            return true;
        }
    }
    false
}

pub(super) fn has_unconditional_mill_one(ops: &[Instruction]) -> bool {
    let mut in_conditional = false;
    for op in ops {
        match op {
            Instruction::BranchIfFalse { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OptionalSequence { .. } => {
                in_conditional = true;
            }
            Instruction::Mill { amount, .. }
                if !in_conditional && *amount == ValueExpr::literal(1) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

pub(super) fn rewrite_milled_card_mana_value(expr: ValueExpr, builder: &BodyBuilder) -> ValueExpr {
    if !matches!(
        builder.target,
        BodyTarget::None | BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
    ) || !has_unconditional_mill_one(&builder.instructions)
    {
        return expr;
    }
    fn walk(e: ValueExpr) -> ValueExpr {
        match e {
            ValueExpr::ObjectStat {
                subject: ValueObjectRef::TargetObject,
                stat: ObjectStatKind::ManaValue,
            } => ValueExpr::EventValue(EventValueKind::MilledCardsManaValueSum),
            ValueExpr::Add(a, b) => ValueExpr::Add(Box::new(walk(*a)), Box::new(walk(*b))),
            ValueExpr::Sub(a, b) => ValueExpr::Sub(Box::new(walk(*a)), Box::new(walk(*b))),
            ValueExpr::Scale { expr, factor } => ValueExpr::Scale {
                expr: Box::new(walk(*expr)),
                factor,
            },
            ValueExpr::DivConst {
                expr,
                divisor,
                round_up,
            } => ValueExpr::DivConst {
                expr: Box::new(walk(*expr)),
                divisor,
                round_up,
            },
            other => other,
        }
    }
    walk(expr)
}

pub(super) fn parse_same_is_true_for_keywords(
    raw: &str,
) -> Result<Vec<KeywordAbility>, &'static str> {
    let cleaned = raw.trim().trim_end_matches('.').trim();
    if cleaned.is_empty() {
        return Err("oracle:unsupported-combat-mod");
    }
    let mut out = Vec::new();
    for seg in cleaned.split(',') {
        let seg = seg.trim();
        let seg = seg.strip_prefix("and ").unwrap_or(seg).trim();
        if seg.is_empty() {
            continue;
        }
        for part in seg.split(" and ") {
            let tok = part.trim();
            if tok.is_empty() {
                return Err("oracle:unsupported-combat-mod");
            }
            let kw = lower_keyword_ability_name(tok).ok_or("oracle:unsupported-combat-mod")?;
            out.push(kw);
        }
    }
    if out.is_empty() {
        return Err("oracle:unsupported-combat-mod");
    }
    Ok(out)
}

pub(super) fn lower_temp_static_abilities_to_ops(
    abilities: Vec<StaticContinuousAbility>,
    dur: ResolvedDuration,
    builder: &BodyBuilder,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Option<Vec<Instruction>> {
    use spellwake_engine::characteristics::{
        CastPermissionRule, LibraryTopPermission, SpellCostModifier, SpellDiscount,
        StaticContinuousAbility, StaticDamageRecipient, StaticDamageRule, StaticDrawRule,
        StaticModifierRule, StaticRestriction, StaticUntapRule,
    };
    if abilities.is_empty() {
        return None;
    }
    let has_source = in_trigger || self_pronoun_valid;
    let selector_allowed = |selector: &PermanentSelector| {
        has_source
            || (!selector.attached_to_source
                && !selector.exclude_source
                && !matches!(
                    selector.controller,
                    ControllerSelector::SourceItself
                        | ControllerSelector::SelfAtMaxSpeed
                        | ControllerSelector::SourcePair
                )
                && selector
                    .source_condition
                    .as_ref()
                    .is_none_or(static_source_condition_is_source_independent))
    };
    let mut lowered_ops: Vec<Instruction> = Vec::with_capacity(abilities.len());
    for ab in abilities {
        match &ab {
            StaticContinuousAbility::Battlefield {
                selector, parts, ..
            } => {
                if let Some(op) =
                    try_lower_temp_battlefield_static(selector, parts, dur, self_pronoun_valid)
                {
                    lowered_ops.push(op);
                } else {
                    if selector_allowed(selector) {
                        lowered_ops.push(Instruction::CreateTemporaryRule {
                            rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(ab))),
                            duration: dur,
                        });
                    } else {
                        return None;
                    }
                }
            }
            StaticContinuousAbility::DamageRule {
                rule:
                    StaticDamageRule::RedirectDamage {
                        source,
                        from,
                        to,
                        combat_only,
                    },
                ..
            } => {
                let ok = ((in_trigger || self_pronoun_valid)
                    || (!matches!(
                        from,
                        StaticDamageRecipient::SelfObject | StaticDamageRecipient::AttachedHost
                    ) && !matches!(
                        to,
                        DamageRedirectTarget::AbilitySource | DamageRedirectTarget::AttachedHost
                    )))
                    && (*to != DamageRedirectTarget::TargetObject
                        || is_permanent_body_target(builder.target));
                if ok {
                    lowered_ops.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::RedirectNextDamage {
                            source: *source,
                            source_is_target: false,
                            from: *from,
                            to: *to,
                            combat_only: *combat_only,
                            amount: None,
                            one_shot: false,
                        }),
                        duration: dur,
                    });
                } else {
                    return None;
                }
            }
            StaticContinuousAbility::CharacteristicDefining { .. }
            | StaticContinuousAbility::CommandZoneActive { .. } => {
                return None;
            }
            other => {
                let ok = match other {
                    StaticContinuousAbility::DamageRule { rule, .. } => match rule {
                        StaticDamageRule::PreventNoncombat
                        | StaticDamageRule::PreventHalfIncoming
                        | StaticDamageRule::DoubleToOpponents
                        | StaticDamageRule::ReplaceSubtypeCombatWithMill(_)
                        | StaticDamageRule::LethalUsesPower
                        | StaticDamageRule::DamageCannotBePrevented
                        | StaticDamageRule::MinimumLifeTotal { .. } => true,
                        StaticDamageRule::ModifyDamage { recipient, .. }
                        | StaticDamageRule::ConditionalModifyDamage { recipient, .. } => {
                            (in_trigger || self_pronoun_valid)
                                || !matches!(
                                    recipient,
                                    StaticDamageRecipient::SelfObject
                                        | StaticDamageRecipient::AttachedHost
                                )
                        }
                        _ => false,
                    },
                    StaticContinuousAbility::DrawRule { rule, .. } => {
                        !matches!(rule, StaticDrawRule::EmptyDrawPutCounters { .. })
                    }
                    StaticContinuousAbility::UntapRule { rule, .. } => match rule {
                        StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(selector) => {
                            selector_allowed(selector)
                        }
                        _ => false,
                    },
                    StaticContinuousAbility::Restriction { rule, .. } => !matches!(
                        rule,
                        StaticRestriction::EnchantedPlayerCastLimit
                            | StaticRestriction::FlashSacrificeAtCleanup
                    ),
                    StaticContinuousAbility::Modifier { rule, .. } => matches!(
                        rule,
                        StaticModifierRule::DoubleCreatedTokens
                            | StaticModifierRule::DoublePlacedCounters
                            | StaticModifierRule::OthersEnterWithCounters { .. }
                            | StaticModifierRule::OthersEnterWithCountersExpr { .. }
                            | StaticModifierRule::ModifyCounters { .. }
                            | StaticModifierRule::ModifyTokens { .. }
                            | StaticModifierRule::ModifyTokenSubtype { .. }
                            | StaticModifierRule::ModifyTokensFiltered { .. }
                            | StaticModifierRule::StealCreatedTokens { .. }
                            | StaticModifierRule::CrewAndSaddlePowerBonus { .. }
                            | StaticModifierRule::ModifyMaxHandSize { .. }
                            | StaticModifierRule::ConvertUnspentMana { .. }
                            | StaticModifierRule::BonusTokenOnCreate { .. }
                            | StaticModifierRule::HalveOpponentCounters
                            | StaticModifierRule::ModifyLifeGain { .. }
                            | StaticModifierRule::ConditionalModifyLifeGain { .. }
                            | StaticModifierRule::IgnoreLegendRule
                            | StaticModifierRule::IgnoreLegendRuleForController
                            | StaticModifierRule::IgnoreLegendForSubtype(_)
                            | StaticModifierRule::RetainMana(_)
                            | StaticModifierRule::TwiceLoyaltyPerTurn
                            | StaticModifierRule::AdditionalCreatureDeathTrigger
                    ),
                    StaticContinuousAbility::CostModifier { modifier, .. } => matches!(
                        modifier,
                        SpellCostModifier::Discount {
                            amount: SpellDiscount::Fixed(_)
                                | SpellDiscount::PerExperienceCounter(_)
                                | SpellDiscount::ControlledCreaturesWithPlusOneCounters
                                | SpellDiscount::Dynamic(_),
                            ..
                        } | SpellCostModifier::IncreaseExceptType { .. }
                            | SpellCostModifier::Increase { .. }
                            | SpellCostModifier::SubtypeDiscountFromCommandOrBattlefield { .. }
                            | SpellCostModifier::FreeSubtypeCreatureSpells(_)
                            | SpellCostModifier::FreeColorlessHandSpellOnce
                            | SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce
                            | SpellCostModifier::NonartifactSpellsHaveImprovise
                            | SpellCostModifier::HighManaValueProtected { .. }
                            | SpellCostModifier::FirstSpellRule { .. }
                            | SpellCostModifier::AlternativeForMatching { .. }
                    ),
                    StaticContinuousAbility::CastPermission { permission, .. } => !matches!(
                        permission,
                        CastPermissionRule::PlayLibraryTop(
                            LibraryTopPermission::CreatureOfSourceEntryType
                        )
                    ),
                    StaticContinuousAbility::EventReplacement { .. } => {
                        has_source || !event_replacement_uses_source(other)
                    }
                    StaticContinuousAbility::Battlefield { .. }
                    | StaticContinuousAbility::CharacteristicDefining { .. }
                    | StaticContinuousAbility::CommandZoneActive { .. } => false,
                };
                if ok {
                    lowered_ops.push(Instruction::CreateTemporaryRule {
                        rule: Box::new(TemporaryRuleKind::TemporaryStatic(Box::new(ab))),
                        duration: dur,
                    });
                } else {
                    return None;
                }
            }
        }
    }
    Some(lowered_ops)
}

fn event_replacement_uses_source(ability: &StaticContinuousAbility) -> bool {
    let StaticContinuousAbility::EventReplacement {
        condition, action, ..
    } = ability
    else {
        return false;
    };
    if condition
        .as_deref()
        .is_some_and(|c| !static_source_condition_is_source_independent(c))
    {
        return true;
    }
    let StaticReplacementAction::ReplaceWithBody { body, .. } = action else {
        return false;
    };
    body.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::PutCountersOnSource { .. }
                | Instruction::PutChargeCounterOnSource
                | Instruction::PutBlessingCounterOnSource
                | Instruction::PutQuestCounterOnSource
                | Instruction::PutStorageCounterOnSource
                | Instruction::PutNamedCounterOnSource { .. }
                | Instruction::RemoveCountersFromPermanent {
                    target_self: true,
                    ..
                }
                | Instruction::MoveSubject {
                    subject: MoveSubjectRef::AbilitySource,
                    ..
                }
        ) || matches!(
            op,
            Instruction::PutCountersOnSubject { subject, .. }
                | Instruction::RemoveCountersFromSubject { subject, .. }
                if matches!(subject.as_ref(), EffectSubject::AbilitySource)
        )
    })
}

pub(super) fn temp_static_selector_to_clause(
    selector: &PermanentSelector,
    self_pronoun_valid: bool,
) -> Option<ObjectClause> {
    use spellwake_engine::characteristics::{CombatSelector, NameFilter};
    let PermanentSelector {
        combat_role,
        requires_untapped,
        requires_tapped,
        sole_controlled_creature,
        token,
        required_supertypes,
        forbidden_supertypes,
        required_types,
        any_types,
        forbidden_types,
        creature_subtype,
        any_creature_subtypes,
        forbidden_creature_subtype,
        land_subtype,
        any_land_subtypes,
        color,
        name,
        controller,
        attached_to_source,
        while_controlling_own_commander,
        plus_one_counter,
        minus_one_counter,
        minimum_plus_one_counters,
        any_counter,
        source_has_imprinted_creature,
        controlled_creatures_and_typed_tokens,
        exclude_auras,
        artifact_subtype,
        planeswalker_subtype,
        matches_source_entry_type,
        controller_type_threshold,
        source_condition,
        requires_flying,
        required_keyword,
        extra_required_keyword,
        requires_toxic,
        required_counter_kind,
        forbidden_keyword,
        forbidden_keywords,
        equipped_by_at_least,
        requires_enchanted,
        min_power,
        max_power,
        min_toughness,
        max_toughness,
        min_mana_value,
        max_mana_value,
        devotion_below,
        station_at,
        exclude_source,
        required_subtypes,
        any_subtypes,
        forbidden_subtypes,
        owner,
        commander,
        requires_modified,
        face_down,
        entered_this_turn,
        matches_source_entry_color,
        without_counters,
        attached_to_source_permanent,
        attached_to_source_host,
        any_required_keywords,
        historic,
        attacked_this_turn,
        dealt_damage_this_turn,
        kicked,
        renowned,
        saddled,
        monstrous,
        suspected,
        solved,
        exerted,
        toughness_greater_than_power,
    } = selector;
    if !required_subtypes.is_empty()
        || *toughness_greater_than_power
        || !any_subtypes.is_empty()
        || !forbidden_subtypes.is_empty()
        || owner.is_some()
        || commander.is_some()
        || *requires_modified
        || face_down.is_some()
        || *entered_this_turn
        || *matches_source_entry_color
        || *without_counters
        || *attached_to_source_permanent
        || *attached_to_source_host
        || !any_required_keywords.is_empty()
        || historic.is_some()
        || attacked_this_turn.is_some()
        || *dealt_damage_this_turn
        || *kicked
        || *renowned
        || *saddled
        || *monstrous
        || *suspected
        || *solved
        || *exerted
        || *sole_controlled_creature
        || *while_controlling_own_commander
        || *source_has_imprinted_creature
        || controlled_creatures_and_typed_tokens.is_some()
        || *matches_source_entry_type
        || controller_type_threshold.is_some()
        || source_condition.is_some()
        || devotion_below.is_some()
        || station_at.is_some()
        || *name != NameFilter::Any
        || !any_creature_subtypes.is_empty()
        || !any_land_subtypes.is_empty()
        || *minimum_plus_one_counters > 0
        || *equipped_by_at_least > 0
        || *requires_enchanted
        || !forbidden_keywords.is_empty()
        || extra_required_keyword.is_some()
        || *requires_toxic
        || required_counter_kind.is_some()
        || (*requires_flying && required_keyword.is_some())
        || (*requires_tapped && *requires_untapped)
        || *plus_one_counter
        || *minus_one_counter
        || *any_counter
        || min_power.is_some()
        || max_power.is_some()
        || min_toughness.is_some()
        || max_toughness.is_some()
        || min_mana_value.is_some()
        || max_mana_value.is_some()
        || *exclude_auras
    {
        return None;
    }
    let mut clause = ObjectClause::any();
    clause.controller = match controller {
        ControllerSelector::Any => ControllerFilter::Any,
        ControllerSelector::SourceController => ControllerFilter::You,
        ControllerSelector::Opponents => ControllerFilter::Opponent,
        _ => return None,
    };
    if *attached_to_source {
        if !self_pronoun_valid || *exclude_source {
            return None;
        }
        clause = clause.attached_to_self();
    } else if *exclude_source {
        if !self_pronoun_valid {
            return None;
        }
        clause.source_relation = SourceRelation::OtherThanAbilitySource;
    }
    for t in required_types {
        clause = clause.required_type(*t);
    }
    for t in any_types {
        clause = clause.any_types(CardTypeMask::from_card_type(*t));
    }
    for t in forbidden_types {
        clause = clause.forbid_types(CardTypeMask::from_card_type(*t));
    }
    for st in required_supertypes {
        clause.required_supertypes = clause
            .required_supertypes
            .union(SupertypeMask::from_supertype(*st));
    }
    for st in forbidden_supertypes {
        clause.forbidden_supertypes = clause
            .forbidden_supertypes
            .union(SupertypeMask::from_supertype(*st));
    }
    let mut sub_count = 0u8;
    if let Some(ct) = creature_subtype {
        clause.subtypes = SubtypeFilter::Has(*ct);
        sub_count += 1;
    }
    if let Some(ct) = forbidden_creature_subtype {
        clause.subtypes = SubtypeFilter::Lacks(*ct);
        sub_count += 1;
    }
    if let Some(lt) = land_subtype {
        clause.subtypes = SubtypeFilter::Land(*lt);
        sub_count += 1;
    }
    if let Some(at) = artifact_subtype {
        clause.subtypes = SubtypeFilter::Artifact(*at);
        sub_count += 1;
    }
    if let Some(pw) = planeswalker_subtype {
        clause.subtypes = SubtypeFilter::Planeswalker(*pw);
        sub_count += 1;
    }
    if sub_count > 1 {
        return None;
    }
    clause.color = match color {
        ColorFilter::Any => ObjectColorFilter::Any,
        ColorFilter::Colorless => ObjectColorFilter::Colorless,
        ColorFilter::Colored => ObjectColorFilter::Colored,
        ColorFilter::Monocolored => ObjectColorFilter::Monocolored,
        ColorFilter::Multicolored => ObjectColorFilter::Multicolored,
        ColorFilter::Has(c) => ObjectColorFilter::has(*c),
        ColorFilter::Lacks(c) => ObjectColorFilter::lacks(*c),
        _ => return None,
    };
    clause.combat = match combat_role {
        None => CombatFilter::Any,
        Some(CombatSelector::Attacking) => CombatFilter::Attacking,
        Some(CombatSelector::Blocking) => CombatFilter::Blocking,
        Some(CombatSelector::AttackingOrBlocking) => CombatFilter::AttackingOrBlocking,
        Some(CombatSelector::Blocked) => CombatFilter::Blocked,
        Some(CombatSelector::Unblocked) => CombatFilter::Unblocked,
        Some(CombatSelector::BlockingSource) => CombatFilter::BlockingSource,
        Some(CombatSelector::BlockedBySource) => CombatFilter::BlockedBySource,
        Some(CombatSelector::BlockingOrBlockedBySource) => CombatFilter::BlockingOrBlockedBySource,
        Some(CombatSelector::NotAttacking) => CombatFilter::NotAttacking,
        Some(CombatSelector::OutsideCombat) => return None,
    };
    if *requires_tapped {
        clause.tapped = Some(true);
    } else if *requires_untapped {
        clause.tapped = Some(false);
    }
    clause.token = *token;
    if *requires_flying {
        clause.keyword = Some(KeywordAbility::Flying);
    } else if let Some(kw) = required_keyword {
        clause.keyword = Some(*kw);
    }
    clause.forbidden_keyword = *forbidden_keyword;
    Some(clause)
}

pub(super) fn numeric_expr_to_value_expr_temp(
    expr: NumericExpression,
    self_pronoun_valid: bool,
) -> Option<ValueExpr> {
    match expr {
        NumericExpression::Literal(v) => Some(ValueExpr::Literal(v)),
        NumericExpression::Eval(v) => Some(*v),
        NumericExpression::Count {
            selector,
            multiplier,
            offset,
        } => {
            let clause = temp_static_selector_to_clause(&selector, self_pronoun_valid)?;
            let mut ve = ValueExpr::Count {
                zone: ValueZone::Battlefield,
                filter: Box::new(ObjectFilter::Clause(clause)),
                controller_scope: None,
            };
            if multiplier != 1.into() {
                ve = ValueExpr::Scale {
                    expr: Box::new(ve),
                    factor: multiplier,
                };
            }
            let zero: RulesInteger = 0.into();
            if offset > zero {
                ve = ValueExpr::Add(Box::new(ve), Box::new(ValueExpr::Literal(offset)));
            } else if offset < zero {
                ve = ValueExpr::Sub(
                    Box::new(ve),
                    Box::new(ValueExpr::Literal(zero.minus(&offset))),
                );
            }
            Some(ve)
        }
        NumericExpression::Counters { source: None, kind }
            if self_pronoun_valid && kind.is_permanent() =>
        {
            Some(ValueExpr::CounterCount {
                subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                kind: Some(kind),
            })
        }
        NumericExpression::PlayerStat(StaticPlayerStat::Devotion(color)) => {
            Some(ValueExpr::Devotion(ColorMask::from_color(color)))
        }
        _ => None,
    }
}

pub(super) fn try_lower_temp_battlefield_static(
    selector: &PermanentSelector,
    parts: &StaticParts,
    duration: ResolvedDuration,
    self_pronoun_valid: bool,
) -> Option<Instruction> {
    use spellwake_engine::characteristics::{ControlRecipient, NameFilter};
    if parts.0.is_empty() {
        return None;
    }
    let mut resolved = Vec::with_capacity(parts.0.len());
    for part in &parts.0 {
        let rp = match part {
            StaticPart::ControlSourceController => {
                ResolvedPartSpec::Control(ControlRecipient::ResolvingController)
            }
            StaticPart::Type(stc) => {
                ResolvedPartSpec::Type(static_type_change_to_spec_temp(stc.clone())?)
            }
            StaticPart::TypeWithSubtypes {
                base,
                artifact_subtypes,
                enchantment_subtypes,
            } => ResolvedPartSpec::TypeWithSubtypes {
                base: static_type_change_to_spec_temp(base.clone())?,
                artifact_subtypes: artifact_subtypes.clone(),
                enchantment_subtypes: enchantment_subtypes.clone(),
            },
            StaticPart::Abilities(ab) => ResolvedPartSpec::Abilities(ab.clone()),
            StaticPart::BlockingRestriction(br) => {
                ResolvedPartSpec::BlockingRestriction(br.clone())
            }
            StaticPart::ModifyPT(NumericPair {
                power: NumericExpression::Literal(p),
                toughness: NumericExpression::Literal(t),
            }) => ResolvedPartSpec::ModifyPT(PowerToughness {
                power: p.clone(),
                toughness: t.clone(),
            }),
            StaticPart::ModifyPT(pair) => ResolvedPartSpec::ModifyPTExpr {
                power: Box::new(numeric_expr_to_value_expr_temp(
                    pair.power.clone(),
                    self_pronoun_valid,
                )?),
                toughness: Box::new(numeric_expr_to_value_expr_temp(
                    pair.toughness.clone(),
                    self_pronoun_valid,
                )?),
            },
            StaticPart::SetPT(NumericPair {
                power: NumericExpression::Literal(p),
                toughness: NumericExpression::Literal(t),
            }) => ResolvedPartSpec::SetPT(PowerToughness {
                power: p.clone(),
                toughness: t.clone(),
            }),
            StaticPart::SetPT(pair) => ResolvedPartSpec::SetPTExpr {
                power: Box::new(numeric_expr_to_value_expr_temp(
                    pair.power.clone(),
                    self_pronoun_valid,
                )?),
                toughness: Box::new(numeric_expr_to_value_expr_temp(
                    pair.toughness.clone(),
                    self_pronoun_valid,
                )?),
            },
            StaticPart::SetPTToOwnManaValue
                if selector.controller == ControllerSelector::SourceItself
                    && self_pronoun_valid =>
            {
                let mv = ValueExpr::ObjectStat {
                    subject: ValueObjectRef::AbilitySource,
                    stat: ObjectStatKind::ManaValue,
                };
                ResolvedPartSpec::SetPTExpr {
                    power: Box::new(mv.clone()),
                    toughness: Box::new(mv),
                }
            }
            StaticPart::SwitchPT => ResolvedPartSpec::SwitchPT,
            StaticPart::GrantTrigger(tr) => ResolvedPartSpec::GrantTrigger(tr.clone()),
            StaticPart::GrantActivated(act) => ResolvedPartSpec::GrantActivated(act.clone()),
            StaticPart::GrantActivatedRestricted {
                ability,
                restriction,
            } => ResolvedPartSpec::GrantActivatedRestricted {
                ability: ability.clone(),
                restriction: restriction.clone(),
            },
            StaticPart::GrantAnyColorMana => ResolvedPartSpec::GrantAnyColorMana,
            StaticPart::GrantFixedColorMana(mt) => ResolvedPartSpec::GrantFixedColorMana(*mt),
            StaticPart::GrantMana(m) => ResolvedPartSpec::GrantMana(m.clone()),
            StaticPart::Color(op) => match op {
                ColorOperation::Set(StaticColorOperand::Literal(c)) => {
                    ResolvedPartSpec::Color(ColorOperation::Set(c.clone()))
                }
                ColorOperation::Add(StaticColorOperand::Literal(c)) => {
                    ResolvedPartSpec::Color(ColorOperation::Add(c.clone()))
                }
                ColorOperation::Remove(StaticColorOperand::Literal(c)) => {
                    ResolvedPartSpec::Color(ColorOperation::Remove(c.clone()))
                }
                _ => return None,
            },
            _ => return None,
        };
        resolved.push(rp);
    }
    if selector.controller == ControllerSelector::SourceItself {
        let PermanentSelector {
            combat_role,
            requires_untapped,
            requires_tapped,
            sole_controlled_creature,
            token,
            required_supertypes,
            forbidden_supertypes,
            required_types,
            any_types,
            forbidden_types,
            creature_subtype,
            any_creature_subtypes,
            forbidden_creature_subtype,
            land_subtype,
            any_land_subtypes,
            color,
            name,
            controller: _,
            attached_to_source,
            while_controlling_own_commander,
            plus_one_counter,
            minus_one_counter,
            minimum_plus_one_counters,
            any_counter,
            source_has_imprinted_creature,
            controlled_creatures_and_typed_tokens,
            exclude_auras,
            artifact_subtype,
            planeswalker_subtype,
            matches_source_entry_type,
            controller_type_threshold,
            source_condition,
            requires_flying,
            required_keyword,
            extra_required_keyword,
            requires_toxic,
            required_counter_kind,
            forbidden_keyword,
            forbidden_keywords,
            equipped_by_at_least,
            requires_enchanted,
            min_power,
            max_power,
            min_toughness,
            max_toughness,
            min_mana_value,
            max_mana_value,
            devotion_below,
            station_at,
            exclude_source,
            required_subtypes,
            any_subtypes,
            forbidden_subtypes,
            owner,
            commander,
            requires_modified,
            face_down,
            entered_this_turn,
            matches_source_entry_color,
            without_counters,
            attached_to_source_permanent,
            attached_to_source_host,
            any_required_keywords,
            historic,
            attacked_this_turn,
            dealt_damage_this_turn,
            kicked,
            renowned,
            saddled,
            monstrous,
            suspected,
            solved,
            exerted,
            toughness_greater_than_power,
        } = selector;
        if !self_pronoun_valid
            || *toughness_greater_than_power
            || !required_subtypes.is_empty()
            || !any_subtypes.is_empty()
            || !forbidden_subtypes.is_empty()
            || owner.is_some()
            || commander.is_some()
            || *requires_modified
            || face_down.is_some()
            || *entered_this_turn
            || *matches_source_entry_color
            || *without_counters
            || *attached_to_source_permanent
            || *attached_to_source_host
            || !any_required_keywords.is_empty()
            || historic.is_some()
            || attacked_this_turn.is_some()
            || *dealt_damage_this_turn
            || *kicked
            || *renowned
            || *saddled
            || *monstrous
            || *suspected
            || *solved
            || *exerted
            || *sole_controlled_creature
            || *while_controlling_own_commander
            || *source_has_imprinted_creature
            || controlled_creatures_and_typed_tokens.is_some()
            || *matches_source_entry_type
            || controller_type_threshold.is_some()
            || source_condition.is_some()
            || devotion_below.is_some()
            || station_at.is_some()
            || *name != NameFilter::Any
            || !any_creature_subtypes.is_empty()
            || !any_land_subtypes.is_empty()
            || *minimum_plus_one_counters > 0
            || *equipped_by_at_least > 0
            || *requires_enchanted
            || !forbidden_keywords.is_empty()
            || extra_required_keyword.is_some()
            || *requires_toxic
            || required_counter_kind.is_some()
            || *plus_one_counter
            || *minus_one_counter
            || *any_counter
            || min_power.is_some()
            || max_power.is_some()
            || min_toughness.is_some()
            || max_toughness.is_some()
            || min_mana_value.is_some()
            || max_mana_value.is_some()
            || combat_role.is_some()
            || *requires_untapped
            || *requires_tapped
            || token.is_some()
            || !required_supertypes.is_empty()
            || !forbidden_supertypes.is_empty()
            || !required_types.is_empty()
            || !any_types.is_empty()
            || !forbidden_types.is_empty()
            || creature_subtype.is_some()
            || forbidden_creature_subtype.is_some()
            || land_subtype.is_some()
            || *color != ColorFilter::Any
            || *attached_to_source
            || *exclude_auras
            || artifact_subtype.is_some()
            || planeswalker_subtype.is_some()
            || *requires_flying
            || required_keyword.is_some()
            || forbidden_keyword.is_some()
            || *exclude_source
        {
            return None;
        }
        return Some(Instruction::ChangeAbilitySource {
            parts: ResolvedPartsSpec(resolved),
            duration,
        });
    }
    let clause = temp_static_selector_to_clause(selector, self_pronoun_valid)?;
    Some(Instruction::ChangeGroup {
        group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(clause))),
        parts: ResolvedPartsSpec(resolved),
        duration,
    })
}

pub(super) fn parse_temp_permanent_lock_parts(rule_body: &str) -> Option<Vec<ResolvedPartSpec>> {
    let cleaned = rule_body
        .strip_prefix("abilitytimingorlock:")
        .unwrap_or(rule_body)
        .trim();
    let (doesnt_untap, suppressed, cant_attack_or_block) = match cleaned {
        "doesnt_untap"
        | "doesntuntap"
        | "doesn't untap during its controller's untap step"
        | "don't untap during their controllers' untap steps"
        | "doesn't untap during your untap step" => (true, None, false),
        "activatedabilitiescantbeactivated"
        | "cantactivateabilities"
        | "activated_abilities_cant_be_activated"
        | "cant_activate_abilities"
        | "its activated abilities can't be activated"
        | "their activated abilities can't be activated" => {
            (false, Some(ActivationSuppression::All), false)
        }
        "nonmanaactivatedabilitiescantbeactivated"
        | "cant_activate_nonmana_abilities"
        | "its activated abilities can't be activated unless they're mana abilities"
        | "their activated abilities can't be activated unless they're mana abilities" => {
            (false, Some(ActivationSuppression::NonMana), false)
        }
        "detain"
        | "detained"
        | "can't attack or block, and its activated abilities can't be activated"
        | "can't attack or block and its activated abilities can't be activated"
        | "can't attack or block, and their activated abilities can't be activated"
        | "can't attack or block and their activated abilities can't be activated" => {
            (false, Some(ActivationSuppression::All), true)
        }
        "can't attack or block, and its activated abilities can't be activated unless they're mana abilities"
        | "can't attack or block and its activated abilities can't be activated unless they're mana abilities"
        | "can't attack or block, and their activated abilities can't be activated unless they're mana abilities"
        | "can't attack or block and their activated abilities can't be activated unless they're mana abilities" => {
            (false, Some(ActivationSuppression::NonMana), true)
        }
        "doesn't untap during its controller's untap step, and its activated abilities can't be activated"
        | "doesn't untap during its controller's untap step and its activated abilities can't be activated"
        | "don't untap during their controllers' untap steps, and their activated abilities can't be activated"
        | "don't untap during their controllers' untap steps and their activated abilities can't be activated" => {
            (true, Some(ActivationSuppression::All), false)
        }
        "doesn't untap during its controller's untap step, and its activated abilities can't be activated unless they're mana abilities"
        | "doesn't untap during its controller's untap step and its activated abilities can't be activated unless they're mana abilities"
        | "don't untap during their controllers' untap steps, and their activated abilities can't be activated unless they're mana abilities"
        | "don't untap during their controllers' untap steps and their activated abilities can't be activated unless they're mana abilities" => {
            (true, Some(ActivationSuppression::NonMana), false)
        }
        _ => return None,
    };
    let mut parts = Vec::new();
    if cant_attack_or_block {
        parts.push(ResolvedPartSpec::BlockingRestriction(
            BlockingRestriction::CannotAttackOrBlock,
        ));
    }
    parts.push(ResolvedPartSpec::Abilities(AbilityChange {
        suppressed_activations: suppressed,
        doesnt_untap,
        ..AbilityChange::default()
    }));
    Some(parts)
}

pub(super) fn condition_to_for_as_long_as_duration(
    condition: &ConditionAst,
) -> Option<DurationAst> {
    let as_marker = |c: &ConditionAst| -> Option<GameMarker> {
        match c {
            ConditionAst::Marker(
                m @ (GameMarker::ControlsSelf
                | GameMarker::RemainsOnBattlefield
                | GameMarker::RemainsTapped),
            ) => Some(m.clone()),
            ConditionAst::GameStateFlag(flag) => {
                match flag
                    .trim()
                    .trim_end_matches('.')
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "you control ~"
                    | "you control this creature"
                    | "you control this permanent"
                    | "you control this artifact"
                    | "you control this enchantment"
                    | "you control this saga"
                    | "you control it" => Some(GameMarker::ControlsSelf),
                    "~ remains on the battlefield"
                    | "this creature remains on the battlefield"
                    | "this permanent remains on the battlefield"
                    | "this artifact remains on the battlefield"
                    | "this enchantment remains on the battlefield"
                    | "this saga remains on the battlefield"
                    | "it remains on the battlefield" => Some(GameMarker::RemainsOnBattlefield),
                    "~ remains tapped"
                    | "this creature remains tapped"
                    | "this permanent remains tapped"
                    | "this artifact remains tapped"
                    | "this enchantment remains tapped"
                    | "it remains tapped" => Some(GameMarker::RemainsTapped),
                    _ => None,
                }
            }
            _ => None,
        }
    };
    if let Some(m) = as_marker(condition) {
        return Some(DurationAst::ForAsLongAs(Box::new(ConditionAst::Marker(m))));
    }
    if let ConditionAst::And(a, b) = condition
        && let (Some(ma), Some(mb)) = (as_marker(a), as_marker(b))
        && matches!(
            (ma, mb),
            (GameMarker::ControlsSelf, GameMarker::RemainsTapped)
                | (GameMarker::RemainsTapped, GameMarker::ControlsSelf)
        )
    {
        return Some(DurationAst::ForAsLongAs(Box::new(ConditionAst::And(
            Box::new(ConditionAst::Marker(GameMarker::ControlsSelf)),
            Box::new(ConditionAst::Marker(GameMarker::RemainsTapped)),
        ))));
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScaledPtDeltaTerm {
    Literal(i64),
    ScaledX(i64),
}

pub(super) fn value_expr_uses_ability_source(expr: &ValueExpr) -> bool {
    match expr {
        ValueExpr::ObjectStat {
            subject: ValueObjectRef::AbilitySource,
            ..
        }
        | ValueExpr::CounterCount {
            subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
            ..
        }
        | ValueExpr::ObjectMatches {
            subject: spellwake_engine::program::MatchObjectRef::AbilitySource,
            ..
        } => true,
        ValueExpr::Count { filter, .. }
        | ValueExpr::Aggregate { filter, .. }
        | ValueExpr::ObjectMatches { filter, .. } => {
            let clause_uses_src = |c: &ObjectClause| {
                matches!(
                    c.source_relation,
                    SourceRelation::SelfObject
                        | SourceRelation::OtherThanAbilitySource
                        | SourceRelation::AttachedToSelf
                        | SourceRelation::SharingNameWithAbilitySource
                        | SourceRelation::OtherThanAbilitySourceSharingName
                )
            };
            match filter.as_ref() {
                ObjectFilter::Clause(c) => clause_uses_src(c),
                ObjectFilter::Either(a, b) => clause_uses_src(a) || clause_uses_src(b),
            }
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            value_expr_uses_ability_source(a) || value_expr_uses_ability_source(b)
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            value_expr_uses_ability_source(inner)
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            value_expr_uses_ability_source(left)
                || value_expr_uses_ability_source(right)
                || value_expr_uses_ability_source(then_value)
                || value_expr_uses_ability_source(else_value)
        }
        _ => false,
    }
}

pub(super) fn static_selector_is_source_independent(selector: &PermanentSelector) -> bool {
    !selector.attached_to_source
        && !selector.exclude_source
        && !selector.matches_source_entry_type
        && !selector.source_has_imprinted_creature
        && !matches!(
            selector.controller,
            ControllerSelector::SourceItself
                | ControllerSelector::SelfAtMaxSpeed
                | ControllerSelector::SourcePair
        )
        && selector
            .source_condition
            .as_ref()
            .is_none_or(static_source_condition_is_source_independent)
}

pub(super) fn static_source_condition_is_source_independent(
    cond: &spellwake_engine::characteristics::StaticSourceCondition,
) -> bool {
    use spellwake_engine::characteristics::StaticSourceCondition;
    match cond {
        StaticSourceCondition::HasKeyword(_)
        | StaticSourceCondition::SourceCounterAtLeast { .. }
        | StaticSourceCondition::SourceAnyCounterAtLeast(_)
        | StaticSourceCondition::SourceIsEquipped
        | StaticSourceCondition::SourceIsEnchanted
        | StaticSourceCondition::AttachedHostMatches(_)
        | StaticSourceCondition::SourceIsTapped
        | StaticSourceCondition::SourceIsUntapped
        | StaticSourceCondition::SourceClassLevelAtLeast(_)
        | StaticSourceCondition::SourceIsSolved
        | StaticSourceCondition::SourceEnteredThisTurn
        | StaticSourceCondition::SourcePaired
        | StaticSourceCondition::SourceIsPrepared
        | StaticSourceCondition::EntryOptionChosen(_)
        | StaticSourceCondition::RoomDoorUnlocked(_)
        | StaticSourceCondition::RoomFullyUnlocked => false,
        StaticSourceCondition::ControlsExactCount { selector, .. }
        | StaticSourceCondition::ControlsMatching { selector, .. }
        | StaticSourceCondition::OpponentControlsMatching { selector, .. } => {
            static_selector_is_source_independent(selector)
        }
        StaticSourceCondition::Eval(rc) => {
            !value_expr_uses_ability_source(&rc.left) && !value_expr_uses_ability_source(&rc.right)
        }
        StaticSourceCondition::Not(inner) => static_source_condition_is_source_independent(inner),
        StaticSourceCondition::And(a, b) | StaticSourceCondition::Or(a, b) => {
            static_source_condition_is_source_independent(a)
                && static_source_condition_is_source_independent(b)
        }
        StaticSourceCondition::ControllerLifeAtMost(_)
        | StaticSourceCondition::ControllerLifeAtLeast(_)
        | StaticSourceCondition::ControllerControlsSubtype(_)
        | StaticSourceCondition::OpponentControlsSubtype(_)
        | StaticSourceCondition::OpponentsTurn
        | StaticSourceCondition::ControllersTurn
        | StaticSourceCondition::Delirium
        | StaticSourceCondition::Metalcraft
        | StaticSourceCondition::Threshold
        | StaticSourceCondition::ControlsCardType { .. }
        | StaticSourceCondition::GraveyardCountAtLeast { .. }
        | StaticSourceCondition::HandCountAtMost(_)
        | StaticSourceCondition::HandCountAtLeast(_)
        | StaticSourceCondition::NonlandPermanentsEnteredThisTurnAtLeast(_)
        | StaticSourceCondition::AnyLibraryAtMost(_)
        | StaticSourceCondition::AnyPlayerHandCountAtMost(_)
        | StaticSourceCondition::MostCommonPermanentColor { .. }
        | StaticSourceCondition::ControllerHasCityBlessing
        | StaticSourceCondition::ControllerGainedLifeThisTurn
        | StaticSourceCondition::ControllerLostLifeThisTurn
        | StaticSourceCondition::OpponentLostLifeThisTurn
        | StaticSourceCondition::ControllerAttackedThisTurn
        | StaticSourceCondition::ControllerCastSpellsThisTurn(_)
        | StaticSourceCondition::ControllerDiscardedThisTurn
        | StaticSourceCondition::ControllerSacrificedThisTurn
        | StaticSourceCondition::PermanentLeftBattlefieldThisTurn
        | StaticSourceCondition::CreatureEnteredUnderControlThisTurn
        | StaticSourceCondition::DescendedThisTurn
        | StaticSourceCondition::CommittedCrimeThisTurn
        | StaticSourceCondition::ControllerDrewCardsThisTurn(_) => true,
    }
}

fn rewrite_grant_cast_duration_slug(ins: &InstructionAst, slug: &str) -> Option<InstructionAst> {
    match ins {
        InstructionAst::GrantCastPermission {
            player,
            cast_only,
            subject,
            free,
            count_limit,
            alt_cost,
            ..
        } => Some(InstructionAst::GrantCastPermission {
            player: *player,
            cast_only: *cast_only,
            subject: subject.clone(),
            free: *free,
            duration: slug.to_string(),
            count_limit: count_limit.clone(),
            alt_cost: alt_cost.clone(),
        }),
        InstructionAst::GrantModifiedCastPermission(perm) => {
            let mut cloned = perm.as_ref().clone();
            cloned.duration = slug.to_string();
            Some(InstructionAst::GrantModifiedCastPermission(Box::new(
                cloned,
            )))
        }
        InstructionAst::Optional(inner) if !inner.is_empty() => {
            let mut out = Vec::with_capacity(inner.len());
            for sub in inner {
                out.push(rewrite_grant_cast_duration_slug(sub, slug)?);
            }
            Some(InstructionAst::Optional(out))
        }
        InstructionAst::OptionalPlayer { player, body } if !body.is_empty() => {
            let mut out = Vec::with_capacity(body.len());
            for sub in body {
                out.push(rewrite_grant_cast_duration_slug(sub, slug)?);
            }
            Some(InstructionAst::OptionalPlayer {
                player: *player,
                body: out,
            })
        }
        InstructionAst::ManaSpendRestriction(_) => Some(ins.clone()),
        _ => None,
    }
}

pub(super) fn duration_ast_to_grant_cast_str(dur: &DurationAst) -> Option<String> {
    match dur {
        DurationAst::UntilEndOfTurn => Some("this_turn".to_string()),
        DurationAst::UntilYourNextTurn => Some("until_your_next_turn".to_string()),
        DurationAst::UntilEndOfNextTurn => Some("until_end_of_next_turn".to_string()),
        DurationAst::UntilEndOfPlayersNextTurn(_) => {
            Some("until_end_of_players_next_turn".to_string())
        }
        DurationAst::Permanent | DurationAst::Instant => Some(String::new()),
        DurationAst::ForAsLongAs(cond) => match cond.as_ref() {
            ConditionAst::Marker(GameMarker::UntilStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::EndStep,
            }) => Some("until_your_next_end_step".to_string()),
            ConditionAst::Marker(GameMarker::UntilStep {
                player: PlayerRefAst::You,
                step: TurnStepAst::Upkeep,
            }) => Some("until_your_next_upkeep".to_string()),
            ConditionAst::Marker(GameMarker::UntilStep {
                player: PlayerRefAst::ThatPlayer,
                step: TurnStepAst::EndStep,
            }) => Some("until_their_next_end_step".to_string()),
            ConditionAst::Marker(GameMarker::UntilStep {
                player: PlayerRefAst::ThatPlayer,
                step: TurnStepAst::Upkeep,
            }) => Some("until_their_next_upkeep".to_string()),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn rewrite_instruction_seq_duration(
    seq: &[InstructionAst],
    dur: &DurationAst,
) -> Option<Vec<InstructionAst>> {
    if seq.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(seq.len());
    for ins in seq {
        out.push(rewrite_instruction_duration(ins, dur)?);
    }
    Some(out)
}

pub(super) fn rewrite_instruction_duration(
    ins: &InstructionAst,
    dur: &DurationAst,
) -> Option<InstructionAst> {
    match ins {
        InstructionAst::DelayedEventTrigger { trigger, body } => {
            if matches!(dur, DurationAst::UntilEndOfTurn) {
                Some(ins.clone())
            } else {
                Some(InstructionAst::TemporaryTrigger {
                    trigger: trigger.clone(),
                    body: body.clone(),
                    duration: dur.clone(),
                })
            }
        }
        InstructionAst::SetBasicLandType {
            subject, land_type, ..
        } => Some(InstructionAst::SetBasicLandType {
            subject: subject.clone(),
            land_type: land_type.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::PreventAllDamageFrom {
            combat_only,
            source,
            recipient,
            ..
        } => Some(InstructionAst::PreventAllDamageFrom {
            combat_only: *combat_only,
            source: source.clone(),
            recipient: recipient.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::ManaSpendRestriction(_)
            if matches!(
                dur,
                DurationAst::UntilEndOfTurn
                    | DurationAst::UntilYourNextTurn
                    | DurationAst::UntilEndOfNextTurn
                    | DurationAst::UntilEndOfPlayersNextTurn(_)
            ) =>
        {
            Some(ins.clone())
        }
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } if matches!(
            subject.as_ref(),
            SubjectRefAst::All(_) | SubjectRefAst::Each(_) | SubjectRefAst::Group(_)
        ) =>
        {
            Some(InstructionAst::TemporaryReplacement {
                replacement: Box::new(ReplacementEffectAst::OthersEnterWithCounters {
                    subject: subject.clone(),
                    counter_kind: counter_kind.clone(),
                    count: count.clone(),
                    additional: true,
                    per: None,
                }),
                duration: dur.clone(),
            })
        }
        InstructionAst::GainControl {
            controller,
            subject,
            ..
        } => Some(InstructionAst::GainControl {
            controller: *controller,
            subject: subject.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::CombatRuleMod { subject, rule, .. } => {
            Some(InstructionAst::CombatRuleMod {
                subject: subject.clone(),
                rule: rule.clone(),
                duration: dur.clone(),
            })
        }
        InstructionAst::TemporaryStatic { static_ability, .. } => {
            Some(InstructionAst::TemporaryStatic {
                static_ability: static_ability.clone(),
                duration: dur.clone(),
            })
        }
        InstructionAst::TemporaryPlayerRestriction {
            player,
            subject,
            rule,
            ..
        } => Some(InstructionAst::TemporaryPlayerRestriction {
            player: *player,
            subject: subject.clone(),
            rule: rule.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::GrantKeywords {
            subject, keywords, ..
        } => Some(InstructionAst::GrantKeywords {
            subject: subject.clone(),
            keywords: keywords.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::RemoveKeywords {
            subject, keywords, ..
        } => Some(InstructionAst::RemoveKeywords {
            subject: subject.clone(),
            keywords: keywords.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::ModifyPT {
            subject,
            dx,
            dy,
            scale,
            ..
        } => Some(InstructionAst::ModifyPT {
            subject: subject.clone(),
            dx: dx.clone(),
            dy: dy.clone(),
            duration: dur.clone(),
            scale: scale.clone(),
        }),
        InstructionAst::ModifyPTChoice {
            subject, choices, ..
        } => Some(InstructionAst::ModifyPTChoice {
            subject: subject.clone(),
            choices: choices.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::GrantKeywordChoice {
            subject, choices, ..
        } => Some(InstructionAst::GrantKeywordChoice {
            subject: subject.clone(),
            choices: choices.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::RemoveAllAbilities { subject, .. } => {
            Some(InstructionAst::RemoveAllAbilities {
                subject: subject.clone(),
                duration: dur.clone(),
            })
        }
        InstructionAst::GrantAbilityText {
            subject,
            ability_text,
            compiled,
            ..
        } => Some(InstructionAst::GrantAbilityText {
            subject: subject.clone(),
            ability_text: ability_text.clone(),
            compiled: compiled.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::SwitchPowerToughness { subject, .. } => {
            Some(InstructionAst::SwitchPowerToughness {
                subject: subject.clone(),
                duration: dur.clone(),
            })
        }
        InstructionAst::AnimateOrSetBasePT {
            subject,
            power,
            toughness,
            description,
            keywords,
            ..
        } => Some(InstructionAst::AnimateOrSetBasePT {
            subject: subject.clone(),
            power: power.clone(),
            toughness: toughness.clone(),
            description: description.clone(),
            keywords: keywords.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::SetDynamicPT {
            subject,
            power,
            toughness,
            ..
        } => Some(InstructionAst::SetDynamicPT {
            subject: subject.clone(),
            power: power.clone(),
            toughness: toughness.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::BecomeCopy {
            subject,
            source,
            exception,
            ..
        } => Some(InstructionAst::BecomeCopy {
            subject: subject.clone(),
            source: source.clone(),
            duration: dur.clone(),
            exception: exception.clone(),
        }),
        InstructionAst::BecomeChosenCharacteristic { subject, kind, .. } => {
            Some(InstructionAst::BecomeChosenCharacteristic {
                subject: subject.clone(),
                kind: kind.clone(),
                duration: dur.clone(),
            })
        }
        InstructionAst::DoubleStatOrCounters { subject, kind, .. } => {
            Some(InstructionAst::DoubleStatOrCounters {
                subject: subject.clone(),
                kind: kind.clone(),
                duration: dur.clone(),
            })
        }
        InstructionAst::AdditionalLandPlay { count, .. } => {
            Some(InstructionAst::AdditionalLandPlay {
                count: *count,
                duration: dur.clone(),
            })
        }
        InstructionAst::EntersTappedRule {
            subject,
            tapped,
            attacking,
            ..
        } => Some(InstructionAst::EntersTappedRule {
            subject: subject.clone(),
            tapped: *tapped,
            attacking: attacking.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::PreventDamage { amount, target, .. } => {
            Some(InstructionAst::PreventDamage {
                amount: amount.clone(),
                target: target.clone(),
                duration: dur.clone(),
            })
        }
        InstructionAst::PreventAllDamage {
            combat_only,
            target,
            ..
        } => Some(InstructionAst::PreventAllDamage {
            combat_only: *combat_only,
            target: target.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::TemporaryReplacement { replacement, .. } => {
            Some(InstructionAst::TemporaryReplacement {
                replacement: replacement.clone(),
                duration: dur.clone(),
            })
        }
        InstructionAst::TemporaryTrigger { trigger, body, .. } => {
            Some(InstructionAst::TemporaryTrigger {
                trigger: trigger.clone(),
                body: body.clone(),
                duration: dur.clone(),
            })
        }
        InstructionAst::WhenYouNext { event, body, .. } => Some(InstructionAst::WhenYouNext {
            event: event.clone(),
            body: body.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::GrantCastPermission {
            player,
            cast_only,
            subject,
            free,
            count_limit,
            alt_cost,
            ..
        } => Some(InstructionAst::GrantCastPermission {
            player: *player,
            cast_only: *cast_only,
            subject: subject.clone(),
            free: *free,
            duration: duration_ast_to_grant_cast_str(dur)?,
            count_limit: count_limit.clone(),
            alt_cost: alt_cost.clone(),
        }),
        InstructionAst::GrantModifiedCastPermission(perm) => {
            let mut cloned = perm.as_ref().clone();
            cloned.duration = duration_ast_to_grant_cast_str(dur)?;
            Some(InstructionAst::GrantModifiedCastPermission(Box::new(
                cloned,
            )))
        }
        InstructionAst::RedirectDamage {
            amount,
            from,
            to,
            source_choice,
            ..
        } => Some(InstructionAst::RedirectDamage {
            amount: amount.clone(),
            from: from.clone(),
            to: to.clone(),
            source_choice: source_choice.clone(),
            duration: Some(dur.clone()),
        }),
        InstructionAst::TemporarySpecialAction { timing, body, .. } => {
            Some(InstructionAst::TemporarySpecialAction {
                timing: timing.clone(),
                duration: dur.clone(),
                body: body.clone(),
            })
        }
        InstructionAst::ForcedBlockByFilter {
            blockers, target, ..
        } => Some(InstructionAst::ForcedBlockByFilter {
            blockers: blockers.clone(),
            target: target.clone(),
            duration: dur.clone(),
        }),
        InstructionAst::OneShotReplacement {
            subject,
            event,
            replace_with,
            instead_body,
        } => {
            if matches!(dur, DurationAst::UntilEndOfTurn | DurationAst::Instant) {
                Some(ins.clone())
            } else if event != "next_time_this_turn" {
                Some(InstructionAst::TemporaryReplacement {
                    replacement: Box::new(ReplacementEffectAst::EventInstead {
                        subject_or_event: subject.clone(),
                        would: event.clone(),
                        instead: replace_with.clone(),
                        instead_body: instead_body.clone(),
                    }),
                    duration: dur.clone(),
                })
            } else {
                None
            }
        }
        InstructionAst::BindX { x_expr, body } => Some(InstructionAst::BindX {
            x_expr: x_expr.clone(),
            body: rewrite_instruction_seq_duration(body, dur)?,
        }),
        InstructionAst::BindXY {
            x_expr,
            y_expr,
            body,
        } => Some(InstructionAst::BindXY {
            x_expr: x_expr.clone(),
            y_expr: y_expr.clone(),
            body: rewrite_instruction_seq_duration(body, dur)?,
        }),
        InstructionAst::IfCondition {
            condition,
            then_body,
        } => Some(InstructionAst::IfCondition {
            condition: condition.clone(),
            then_body: rewrite_instruction_seq_duration(then_body, dur)?,
        }),
        InstructionAst::Optional(inner) => Some(InstructionAst::Optional(
            rewrite_instruction_seq_duration(inner, dur)?,
        )),
        InstructionAst::OptionalPlayer { player, body } => Some(InstructionAst::OptionalPlayer {
            player: *player,
            body: rewrite_instruction_seq_duration(body, dur)?,
        }),
        InstructionAst::ForEachPlayer { player, body } => Some(InstructionAst::ForEachPlayer {
            player: *player,
            body: rewrite_instruction_seq_duration(body, dur)?,
        }),
        InstructionAst::ForEachObject { filter, body } => Some(InstructionAst::ForEachObject {
            filter: filter.clone(),
            body: rewrite_instruction_seq_duration(body, dur)?,
        }),
        InstructionAst::ForEachCount { count, body } => Some(InstructionAst::ForEachCount {
            count: count.clone(),
            body: rewrite_instruction_seq_duration(body, dur)?,
        }),
        InstructionAst::Choice(branches) if !branches.is_empty() => {
            let mut out = Vec::with_capacity(branches.len());
            for branch in branches {
                out.push(rewrite_instruction_seq_duration(branch, dur)?);
            }
            Some(InstructionAst::Choice(out))
        }
        InstructionAst::Unless {
            body,
            unless_player,
            unless_cost,
            unless_action,
            unless_condition,
        } => Some(InstructionAst::Unless {
            body: rewrite_instruction_seq_duration(body, dur)?,
            unless_player: *unless_player,
            unless_cost: unless_cost.clone(),
            unless_action: unless_action.clone(),
            unless_condition: unless_condition.clone(),
        }),
        InstructionAst::Simultaneous(inner) => Some(InstructionAst::Simultaneous(
            rewrite_instruction_seq_duration(inner, dur)?,
        )),
        InstructionAst::IfPreviousSucceeded(inner) => Some(InstructionAst::IfPreviousSucceeded(
            rewrite_instruction_seq_duration(inner, dur)?,
        )),
        InstructionAst::IfPreviousFailed(inner) => Some(InstructionAst::IfPreviousFailed(
            rewrite_instruction_seq_duration(inner, dur)?,
        )),
        InstructionAst::Otherwise(inner) => Some(InstructionAst::Otherwise(
            rewrite_instruction_seq_duration(inner, dur)?,
        )),
        InstructionAst::ScopedDuration {
            duration: inner_dur,
            body,
        } if matches!(inner_dur, DurationAst::Instant) || inner_dur == dur => {
            Some(InstructionAst::ScopedDuration {
                duration: dur.clone(),
                body: rewrite_instruction_seq_duration(body, dur)?,
            })
        }
        _ => None,
    }
}

pub(super) fn lower_subject_to_permanent_selector(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    let f = match subject {
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if f.zone.is_none() =>
        {
            f
        }
        _ => return Err("oracle:unsupported-enters-tapped-rule"),
    };
    if !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || f.combat.is_some()
        || f.tapped.is_some()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || f.counter_kind.is_some()
        || f.named.is_some()
        || !f.predicates.is_empty()
        || f.any_card
        || f.any_spell
        || f.is_ability
    {
        return Err("oracle:unsupported-enters-tapped-rule");
    }
    let allowed_flags = AstFilterFlags::EXCLUDE_SELF;
    if (f.flags.0 & !allowed_flags.0) != 0 {
        return Err("oracle:unsupported-enters-tapped-rule");
    }
    let mut sel = PermanentSelector {
        controller: match f.controller {
            None => ControllerSelector::Any,
            Some(ControllerRelation::You) => ControllerSelector::SourceController,
            Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                ControllerSelector::Opponents
            }
            _ => return Err("oracle:unsupported-enters-tapped-rule"),
        },
        token: f.token,
        exclude_source: (f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0) != 0,
        ..PermanentSelector::default()
    };
    if !f.required_supertypes.is_empty() {
        sel.required_supertypes = supertype_mask_to_set(f.required_supertypes)?;
    }
    if !f.forbidden_supertypes.is_empty() {
        sel.forbidden_supertypes = supertype_mask_to_set(f.forbidden_supertypes)?;
    }
    if !f.any_types.is_empty() {
        let types = card_type_mask_to_set(f.any_types)?;
        if types.len() == 1 {
            sel.required_types = types;
        } else {
            sel.any_types = types;
        }
    }
    if !f.forbidden_types.is_empty() {
        sel.forbidden_types = card_type_mask_to_set(f.forbidden_types)?;
    }
    if f.subtypes.len() == 1 {
        let st = &f.subtypes[0];
        if let Some(ct) = resolve_protection_subtype(st, vocabulary) {
            sel.creature_subtype = Some(ct);
        } else {
            return Err("oracle:unsupported-enters-tapped-rule");
        }
    } else if f.subtypes.len() > 1 {
        for st in &f.subtypes {
            let ct = resolve_protection_subtype(st, vocabulary)
                .ok_or("oracle:unsupported-enters-tapped-rule")?;
            sel.any_creature_subtypes.insert(ct);
        }
    }
    if f.forbidden_subtypes.len() == 1 {
        let ct = resolve_protection_subtype(&f.forbidden_subtypes[0], vocabulary)
            .ok_or("oracle:unsupported-enters-tapped-rule")?;
        sel.forbidden_creature_subtype = Some(ct);
    } else if f.forbidden_subtypes.len() > 1 {
        return Err("oracle:unsupported-enters-tapped-rule");
    }
    Ok(sel)
}

pub(super) fn collect_combinations(
    n: usize,
    size: usize,
    start: usize,
    cur: &mut Vec<usize>,
    out: &mut Vec<Vec<usize>>,
    cap: usize,
) -> bool {
    if out.len() > cap {
        return false;
    }
    if cur.len() == size {
        out.push(cur.clone());
        return out.len() <= cap;
    }
    let remaining_needed = size - cur.len();
    for i in start..=n.saturating_sub(remaining_needed) {
        cur.push(i);
        if !collect_combinations(n, size, i + 1, cur, out, cap) {
            return false;
        }
        cur.pop();
    }
    true
}

pub(super) fn str_mentions_y_var(s: &str) -> bool {
    let t = s.trim();
    t.eq_ignore_ascii_case("y") || t.eq_ignore_ascii_case("+y") || t.eq_ignore_ascii_case("-y")
}

pub(super) fn body_mentions_y_var(body: &[InstructionAst]) -> bool {
    body.iter().any(|ins| match ins {
        InstructionAst::ModifyPT { dx, dy, .. } => str_mentions_y_var(dx) || str_mentions_y_var(dy),
        InstructionAst::AnimateOrSetBasePT {
            power, toughness, ..
        } => str_mentions_y_var(power) || str_mentions_y_var(toughness),
        InstructionAst::TemporaryStatic { static_ability, .. } => match static_ability.as_ref() {
            StaticAbilityAst::ContinuousModifyPT { dx, dy, .. } => {
                str_mentions_y_var(dx) || str_mentions_y_var(dy)
            }
            StaticAbilityAst::ContinuousLayerEffect {
                base_pt: Some((p, t)),
                ..
            } => str_mentions_y_var(p) || str_mentions_y_var(t),
            _ => false,
        },
        InstructionAst::Optional(inner)
        | InstructionAst::OptionalPlayer { body: inner, .. }
        | InstructionAst::ForEachPlayer { body: inner, .. }
        | InstructionAst::ForEachSubjectPlayer { body: inner, .. }
        | InstructionAst::ForAsLongAs { body: inner, .. }
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::BindXY { body: inner, .. }
        | InstructionAst::DelayedTrigger { body: inner, .. }
        | InstructionAst::IfCondition {
            then_body: inner, ..
        }
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::Otherwise(inner) => body_mentions_y_var(inner),
        _ => false,
    })
}

pub(super) fn is_player_anaphora_label(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "them"
            | "they"
            | "that player"
            | "that opponent"
            | "him or her"
            | "its controller"
            | "that spell's controller"
            | "that creature's controller"
            | "that permanent's controller"
            | "their controller"
    )
}

pub(super) fn parse_cant_be_target_colors(raw: &str) -> Option<(ColorMask, bool)> {
    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if low.contains("opponent")
        || low.contains("you control")
        || low.contains("they control")
        || low.contains("creature")
        || low.contains("artifact")
        || low.contains("enchantment")
        || low.contains("instant")
        || low.contains("sorcery")
        || low.contains("aura")
        || low.contains("equipment")
        || low.contains("non")
    {
        return None;
    }
    let has_spells = low.contains("spell");
    let has_abilities = low.contains("abilit");
    if !has_spells {
        return None;
    }
    let (prefix, suffix) = low
        .split_once(" spells")
        .or_else(|| low.split_once(" spell"))?;
    let suffix_trim = suffix.trim();
    let spells_only = if !has_abilities {
        if !suffix_trim.is_empty() {
            return None;
        }
        true
    } else {
        if suffix_trim.contains("from ") || suffix_trim.contains("with ") {
            return None;
        }
        false
    };
    let mut mask = ColorMask::default();
    for tok in prefix
        .trim()
        .replace(',', " ")
        .replace(" or ", " ")
        .replace(" and ", " ")
        .split_whitespace()
    {
        match tok {
            "white" => mask = mask.union(ColorMask::WHITE),
            "blue" => mask = mask.union(ColorMask::BLUE),
            "black" => mask = mask.union(ColorMask::BLACK),
            "red" => mask = mask.union(ColorMask::RED),
            "green" => mask = mask.union(ColorMask::GREEN),
            _ => return None,
        }
    }
    if mask.is_empty() {
        None
    } else {
        Some((mask, spells_only))
    }
}

pub(super) fn try_lower_cant_cast_from_zone(
    subject: Option<&SubjectRefAst>,
    rule_low: &str,
    filter: Option<&ObjectFilterAst>,
    dur: ResolvedDuration,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
) -> Option<Result<(), &'static str>> {
    let resolve_players = |subj: Option<&SubjectRefAst>,
                           who_opt: Option<&str>,
                           b: &mut BodyBuilder|
     -> Result<AffectedPlayers, &'static str> {
        match subj {
            Some(SubjectRefAst::Player(p)) => lower_affected_players_in_context(*p, b, in_trigger),
            Some(SubjectRefAst::TargetPlayer {
                opponent_only,
                count: TargetCountAst::Exact(1),
            }) => {
                b.bind_target(if *opponent_only {
                    BodyTarget::SingleOpponent
                } else {
                    BodyTarget::SinglePlayer
                })?;
                Ok(AffectedPlayers::One(ResolvingPlayer::TargetPlayer))
            }
            Some(_) => Err("oracle:unsupported-temp-static"),
            None => match who_opt.map(str::trim) {
                Some("players" | "each player") => Ok(AffectedPlayers::EachPlayer),
                Some("your opponents" | "opponents" | "each opponent") => {
                    Ok(AffectedPlayers::EachOpponent)
                }
                Some("you") => Ok(AffectedPlayers::One(ResolvingPlayer::Controller)),
                Some("that player" | "they") => {
                    lower_affected_players_in_context(PlayerRefAst::ThatPlayer, b, in_trigger)
                }
                Some("target player") => {
                    b.bind_target(BodyTarget::SinglePlayer)?;
                    Ok(AffectedPlayers::One(ResolvingPlayer::TargetPlayer))
                }
                Some("target opponent") => {
                    b.bind_target(BodyTarget::SingleOpponent)?;
                    Ok(AffectedPlayers::One(ResolvingPlayer::TargetPlayer))
                }
                _ => Err("oracle:unsupported-temp-static"),
            },
        }
    };

    if let Some(f) = filter
        && let Some(zf) = f.zone
        && (rule_low == "cantcast:filter"
            || rule_low == "cant_cast_spells"
            || rule_low.contains("can't cast"))
    {
        if !f.extra_zones.is_empty() {
            return Some(Err("oracle:unsupported-temp-static"));
        }
        let mut trial = builder.clone();
        let players = match resolve_players(subject, None, &mut trial) {
            Ok(p) => p,
            Err(e) => return Some(Err(e)),
        };
        let is_controller_only = players == AffectedPlayers::One(ResolvingPlayer::Controller);
        let zones: Vec<SourceZoneFilter> = match zf {
            ZoneFilter::YourGraveyard if is_controller_only => vec![SourceZoneFilter::Graveyard],
            ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards => {
                vec![SourceZoneFilter::AnyGraveyard]
            }
            ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards
                if is_controller_only =>
            {
                vec![SourceZoneFilter::OpponentGraveyard]
            }
            ZoneFilter::Exile => vec![SourceZoneFilter::Exile],
            ZoneFilter::YourHand if is_controller_only => vec![SourceZoneFilter::Hand],
            ZoneFilter::AnyHand | ZoneFilter::EachPlayerHand => vec![SourceZoneFilter::Hand],
            ZoneFilter::YourLibrary if is_controller_only => vec![SourceZoneFilter::YourLibrary],
            ZoneFilter::AnyLibrary | ZoneFilter::EachPlayerLibrary => {
                vec![SourceZoneFilter::YourLibrary]
            }
            ZoneFilter::YourHandOrGraveyard if is_controller_only => {
                vec![SourceZoneFilter::HandOrGraveyard]
            }
            ZoneFilter::AnyGraveyardOrExile => {
                vec![SourceZoneFilter::AnyGraveyard, SourceZoneFilter::Exile]
            }
            ZoneFilter::YourGraveyardOrExile if is_controller_only => {
                vec![SourceZoneFilter::Graveyard, SourceZoneFilter::Exile]
            }
            ZoneFilter::YourLibraryOrGraveyard if is_controller_only => {
                vec![SourceZoneFilter::YourLibrary, SourceZoneFilter::Graveyard]
            }
            _ => return Some(Err("oracle:unsupported-temp-static")),
        };
        let mut inner = f.clone();
        inner.zone = None;
        inner.any_spell = false;
        let flt = if is_unrestricted_filter(&inner) {
            None
        } else {
            match lower_object_filter(&inner, vocabulary) {
                Ok(obj) => Some(obj),
                Err(e) => return Some(Err(e)),
            }
        };
        for zone in zones {
            trial.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::CantCastFromZone {
                    players,
                    zone,
                    filter: flt,
                }),
                duration: dur,
            });
        }
        *builder = trial;
        return Some(Ok(()));
    }

    if filter.is_none() && rule_low.contains("can't cast ") && rule_low.contains(" from ") {
        let (body, eff_dur) = strip_trailing_temp_duration(rule_low, dur)
            .unwrap_or_else(|| (rule_low.trim().trim_end_matches('.').trim().to_owned(), dur));
        let (who_opt, after_cast) = if let Some((who, rest)) = body.split_once(" can't cast ") {
            (Some(who.trim()), rest.trim())
        } else {
            let rest = body.strip_prefix("can't cast ")?;
            (None, rest.trim())
        };
        let (what_raw, zone_raw) = after_cast.split_once(" from ")?;
        let zone_clean = zone_raw.trim().trim_end_matches('.').trim();
        if zone_clean.contains(" unless ")
            || zone_clean.contains(" during ")
            || zone_clean.contains(" other than ")
            || zone_clean.contains(" except ")
            || zone_clean.contains(" without ")
        {
            return Some(Err("oracle:unsupported-temp-static"));
        }
        let mut trial = builder.clone();
        let players = match resolve_players(subject, who_opt, &mut trial) {
            Ok(p) => p,
            Err(e) => return Some(Err(e)),
        };
        let is_controller_only = players == AffectedPlayers::One(ResolvingPlayer::Controller);
        let zones: Vec<SourceZoneFilter> = match zone_clean {
            "graveyards" | "a graveyard" | "any graveyard" => {
                vec![SourceZoneFilter::AnyGraveyard]
            }
            "their graveyard" | "their graveyards" => vec![SourceZoneFilter::Graveyard],
            "your graveyard" if is_controller_only => vec![SourceZoneFilter::Graveyard],
            "an opponent's graveyard" | "opponents' graveyards" if is_controller_only => {
                vec![SourceZoneFilter::OpponentGraveyard]
            }
            "exile" => vec![SourceZoneFilter::Exile],
            "libraries" | "a library" | "their library" | "their libraries" => {
                vec![SourceZoneFilter::YourLibrary]
            }
            "your library" if is_controller_only => vec![SourceZoneFilter::YourLibrary],
            "their hand" | "their hands" | "a hand" | "hands" => vec![SourceZoneFilter::Hand],
            "your hand" if is_controller_only => vec![SourceZoneFilter::Hand],
            "graveyards or libraries"
            | "graveyards or from libraries"
            | "a graveyard or a library"
            | "libraries or graveyards"
            | "libraries or from graveyards"
            | "graveyards and libraries" => {
                vec![
                    SourceZoneFilter::AnyGraveyard,
                    SourceZoneFilter::YourLibrary,
                ]
            }
            "graveyards or exile"
            | "graveyards or from exile"
            | "a graveyard or exile"
            | "exile or graveyards"
            | "exile or from graveyards"
            | "graveyards and exile" => {
                vec![SourceZoneFilter::AnyGraveyard, SourceZoneFilter::Exile]
            }
            "exile or libraries"
            | "exile or from libraries"
            | "libraries or exile"
            | "libraries or from exile" => {
                vec![SourceZoneFilter::Exile, SourceZoneFilter::YourLibrary]
            }
            "your graveyard or exile" | "your graveyard or from exile" if is_controller_only => {
                vec![SourceZoneFilter::Graveyard, SourceZoneFilter::Exile]
            }
            "their hand or graveyard" => vec![SourceZoneFilter::HandOrGraveyard],
            "your hand or graveyard" if is_controller_only => {
                vec![SourceZoneFilter::HandOrGraveyard]
            }
            _ => return Some(Err("oracle:unsupported-temp-static")),
        };
        let what_clean = what_raw.trim();
        let flt = if matches!(what_clean, "spells" | "a spell" | "any spells") {
            None
        } else {
            let vocab_ref = cached_compiler_vocab(vocabulary);
            let parsed =
                spellwake_oracle_compiler::filter::parse_object_filter(what_clean, &vocab_ref)
                    .or_else(|_| {
                        let stem = what_clean
                            .strip_suffix(" spells")
                            .or_else(|| what_clean.strip_suffix(" spell"))
                            .unwrap_or(what_clean)
                            .trim();
                        spellwake_oracle_compiler::filter::parse_object_filter(stem, &vocab_ref)
                    });
            let mut ast_f = match parsed {
                Ok(f) => f,
                Err(_) => return Some(Err("oracle:unsupported-temp-static")),
            };
            if ast_f.zone.is_some()
                || !ast_f.extra_zones.is_empty()
                || !ast_f.predicates.is_empty()
                || ast_f.is_ability
                || ast_f.named.is_some()
            {
                return Some(Err("oracle:unsupported-temp-static"));
            }
            ast_f.any_spell = false;
            if is_unrestricted_filter(&ast_f) {
                None
            } else {
                match lower_object_filter(&ast_f, vocabulary) {
                    Ok(obj) => Some(obj),
                    Err(e) => return Some(Err(e)),
                }
            }
        };
        for zone in zones {
            trial.instructions.push(Instruction::CreateTemporaryRule {
                rule: Box::new(TemporaryRuleKind::CantCastFromZone {
                    players,
                    zone,
                    filter: flt,
                }),
                duration: eff_dur,
            });
        }
        *builder = trial;
        return Some(Ok(()));
    }

    None
}

#[cfg(test)]
mod wave10_static_tests {
    use super::*;

    fn vocabulary() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "wave10-static-tests".into(),
            [0; 32],
            vec!["Giant".into(), "Human".into()],
        )
        .expect("test vocabulary")
    }

    fn lower(ability: StaticAbilityAst) -> Result<BodyBuilder, &'static str> {
        let mut builder = BodyBuilder::new();
        lower_restrictions_statics_instruction(
            &InstructionAst::TemporaryStatic {
                static_ability: Box::new(ability),
                duration: DurationAst::UntilEndOfTurn,
            },
            &mut builder,
            &vocabulary(),
            &mut LoweringIds::default(),
            false,
            false,
            None,
        )?;
        Ok(builder)
    }

    fn casting_lock(subject: SubjectRefAst, filter: ObjectFilterAst) -> StaticAbilityAst {
        StaticAbilityAst::RuleOrCombatRestriction {
            subject: Some(Box::new(subject)),
            rule: "CantCast:filter".into(),
            filter: Some(Box::new(filter)),
            condition: None,
        }
    }

    #[test]
    fn wave10_cast_lock_keeps_target_player_and_spell_filter() {
        let filter = ObjectFilterAst {
            any_spell: true,
            any_types: AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY),
            ..Default::default()
        };
        let builder = lower(casting_lock(
            SubjectRefAst::Player(PlayerRefAst::TargetPlayer),
            filter,
        ))
        .expect("filtered casting restriction");
        assert_eq!(builder.target, BodyTarget::SinglePlayer);
        let [Instruction::CreateTemporaryRule { rule, duration }] = builder.instructions.as_slice()
        else {
            panic!("expected one temporary rule");
        };
        assert_eq!(*duration, ResolvedDuration::UntilEndOfTurn);
        assert_eq!(
            rule.as_ref(),
            &TemporaryRuleKind::CantCastSpells {
                players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                filter: Some(ObjectFilter::Clause(
                    ObjectClause::any()
                        .any_types(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY))
                )),
            }
        );
    }

    #[test]
    fn wave10_cast_lock_does_not_widen_unknown_subject_to_all_players() {
        assert!(
            lower(casting_lock(
                SubjectRefAst::SelfObject,
                ObjectFilterAst::default(),
            ))
            .is_err()
        );
    }

    #[test]
    fn wave10_cast_lock_preserves_explicit_opponent_target() {
        let builder = lower(casting_lock(
            SubjectRefAst::TargetPlayer {
                opponent_only: true,
                count: TargetCountAst::Exact(1),
            },
            ObjectFilterAst {
                any_spell: true,
                ..Default::default()
            },
        ))
        .expect("opponent casting restriction");
        assert_eq!(builder.target, BodyTarget::SingleOpponent);
        assert!(matches!(builder.instructions.as_slice(),
            [Instruction::CreateTemporaryRule { rule, .. }]
                if matches!(rule.as_ref(), TemporaryRuleKind::CantCastSpells {
                    players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    filter: None,
                })
        ));
    }

    fn next_flash(filter: ObjectFilterAst, effect_filter: ObjectFilterAst) -> StaticAbilityAst {
        StaticAbilityAst::NextSpellModifier {
            filter: Box::new(filter),
            effect: Box::new(StaticAbilityAst::GrantKeywords {
                subject: Box::new(SubjectRefAst::Group(Box::new(effect_filter))),
                keywords: vec!["flash".into()],
                condition: None,
                paired_rule: None,
            }),
        }
    }

    #[test]
    fn wave10_next_flash_preserves_creature_filter_and_consumption() {
        let filter = ObjectFilterAst {
            any_types: AstCardTypeMask::CREATURE,
            ..Default::default()
        };
        let builder = lower(next_flash(filter.clone(), filter)).expect("next creature flash");
        assert!(matches!(builder.instructions.as_slice(),
            [Instruction::CreateTemporaryRule { rule, .. }]
                if matches!(rule.as_ref(), TemporaryRuleKind::FilteredFlash {
                    player: ResolvingPlayer::Controller,
                    filter,
                    next_only: true,
                } if *filter == ObjectFilter::creature())
        ));
        assert!(
            lower(next_flash(
                ObjectFilterAst::default(),
                ObjectFilterAst {
                    any_types: AstCardTypeMask::CREATURE,
                    ..Default::default()
                }
            ))
            .is_err()
        );
    }
    #[test]
    fn wave10_scouts_warning_lowers_flash_and_draw_together() {
        let card = InventoryCard {
            scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
            oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
            name: "Scout's Warning".into(),
            layout: "normal".into(),
            faces: vec![spellwake_content::CatalogFace {
                name: "Scout's Warning".into(),
                type_line: Some("Instant".into()),
                oracle_text: Some("The next creature spell you cast this turn can be cast as though it had flash.\nDraw a card.".into()),
                mana_cost: Some("{W}".into()),
                power: None,
                toughness: None,
            }],
            legalities: BTreeMap::new(),
            eligible_formats: Vec::new(),
            required_by_base_formats: true,
            program_evidence: "test",
        };
        compile_with_reason(&card, &vocabulary()).expect("Scout's Warning whole-card lowering");
    }
    #[test]
    fn retained_restricted_mana_preserves_both_clause_orders() {
        let producer = InstructionAst::AddMana {
            player: PlayerRefAst::You,
            mana: ManaSpecAst::Fixed("{W}{W}{W}".into()),
        };
        let restriction = InstructionAst::ManaSpendRestriction(
            "Spend this mana only to cast creature spells".into(),
        );
        let retention = InstructionAst::ScopedDuration {
            duration: DurationAst::UntilEndOfTurn,
            body: vec![InstructionAst::RetainManaUntilEndOfTurn],
        };
        let expected = vec![Instruction::AddMana(EffectManaYield::RetainedUntilCleanup(
            Box::new(EffectManaYield::RestrictedFixed {
                bundle: BTreeMap::from([(ManaType::White, 3.into())]),
                restriction: ManaRestriction::CreatureSpell,
            }),
        ))];
        for clauses in [
            [restriction.clone(), retention.clone()],
            [retention, restriction],
        ] {
            let sequence = [producer.clone(), clauses[0].clone(), clauses[1].clone()];
            let mut builder = BodyBuilder::new();
            lower_instruction_seq(
                &sequence,
                &mut builder,
                &vocabulary(),
                &mut LoweringIds::default(),
                false,
                false,
                None,
            )
            .expect("restriction and retention compose in either order");
            assert_eq!(builder.instructions, expected);
            assert_eq!(builder.target, BodyTarget::None);

            let mut no_producer = BodyBuilder::new();
            assert!(
                lower_instruction_seq(
                    &clauses,
                    &mut no_producer,
                    &vocabulary(),
                    &mut LoweringIds::default(),
                    false,
                    false,
                    None,
                )
                .is_err()
            );
            assert!(no_producer.instructions.is_empty());
        }
    }
    #[test]
    fn wave10_scoped_retention_folds_only_prior_mana_production() {
        let instruction = InstructionAst::ScopedDuration {
            duration: DurationAst::UntilEndOfTurn,
            body: vec![InstructionAst::RetainManaUntilEndOfTurn],
        };
        let mut builder = BodyBuilder::new();
        builder
            .instructions
            .push(Instruction::AddMana(EffectManaYield::Fixed(
                BTreeMap::from([(ManaType::White, 3.into())]),
            )));
        lower_restrictions_statics_instruction(
            &instruction,
            &mut builder,
            &vocabulary(),
            &mut LoweringIds::default(),
            true,
            true,
            None,
        )
        .expect("scoped producer retention");
        assert!(matches!(builder.instructions.as_slice(),
            [Instruction::AddMana(EffectManaYield::RetainedUntilCleanup(inner))]
                if matches!(inner.as_ref(), EffectManaYield::Fixed(amount)
                    if amount == &BTreeMap::from([(ManaType::White, 3.into())]))
        ));
        let mut no_producer = BodyBuilder::new();
        assert!(
            lower_restrictions_statics_instruction(
                &instruction,
                &mut no_producer,
                &vocabulary(),
                &mut LoweringIds::default(),
                true,
                true,
                None
            )
            .is_err()
        );
        assert!(no_producer.instructions.is_empty());
        let mut extended = instruction;
        if let InstructionAst::ScopedDuration { duration, .. } = &mut extended {
            *duration = DurationAst::UntilYourNextTurn;
        }
        assert!(
            lower_restrictions_statics_instruction(
                &extended,
                &mut builder,
                &vocabulary(),
                &mut LoweringIds::default(),
                true,
                true,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn wave10_colossal_plow_retains_its_three_white_mana() {
        let card = InventoryCard {
            scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
            oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
            name: "Colossal Plow".into(),
            layout: "normal".into(),
            faces: vec![spellwake_content::CatalogFace {
                name: "Colossal Plow".into(),
                type_line: Some("Artifact \u{2014} Vehicle".into()),
                oracle_text: Some("Whenever this Vehicle attacks, add {W}{W}{W} and you gain 3 life. Until end of turn, you don't lose this mana as steps and phases end.\nCrew 6".into()),
                mana_cost: Some("{2}".into()),
                power: Some("6".into()),
                toughness: Some("3".into()),
            }],
            legalities: BTreeMap::new(),
            eligible_formats: Vec::new(),
            required_by_base_formats: true,
            program_evidence: "test",
        };
        let program =
            compile_with_reason(&card, &vocabulary()).expect("Colossal Plow whole-card lowering");
        assert!(
            program.triggered[0]
                .body
                .instructions
                .iter()
                .any(|instruction| matches!(
                    instruction,
                    Instruction::AddMana(EffectManaYield::RetainedUntilCleanup(_))
                ))
        );
    }
    #[test]
    fn wave10_mountain_titan_delayed_trigger_keeps_end_of_turn_expiry() {
        let card = InventoryCard {
            scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
            oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
            name: "Mountain Titan".into(),
            layout: "normal".into(),
            faces: vec![spellwake_content::CatalogFace {
                name: "Mountain Titan".into(),
                type_line: Some("Creature \u{2014} Giant".into()),
                oracle_text: Some("{1}{R}{R}: Until end of turn, whenever you cast a black spell, put a +1/+1 counter on this creature.".into()),
                mana_cost: Some("{2}{B}{R}".into()),
                power: Some("2".into()),
                toughness: Some("2".into()),
            }],
            legalities: BTreeMap::new(),
            eligible_formats: Vec::new(),
            required_by_base_formats: true,
            program_evidence: "test",
        };
        let program =
            compile_with_reason(&card, &vocabulary()).expect("Mountain Titan whole-card lowering");
        assert!(matches!(
            program.activated[0].body.instructions.as_slice(),
            [Instruction::ScheduleDelayedEventTrigger {
                expires: ResolvedDuration::UntilEndOfTurn,
                once: false,
                ..
            }]
        ));
    }
    #[test]
    fn wave10_named_next_spell_payment_keywords_lower_complete_cards() {
        let vocabulary = CreatureTypeVocabulary::new(
            "wave10-payment-keywords".into(),
            [0; 32],
            vec!["Spirit".into()],
        )
        .expect("test vocabulary");
        for (name, kind, cost, pt, text, keyword) in [
            (
                "Wand of the Worldsoul",
                "Artifact",
                Some("{2}{W}"),
                None,
                "This artifact enters tapped.\n{T}: Add {W}.\n{T}: The next spell you cast this turn has convoke.",
                KeywordAbility::Convoke,
            ),
            (
                "Flockchaser Phantom",
                "Creature \u{2014} Spirit",
                Some("{4}{W}{U}"),
                Some(("5", "5")),
                "Convoke\nFlying, vigilance\nWhenever this creature attacks, the next spell you cast this turn has convoke.",
                KeywordAbility::Convoke,
            ),
            (
                "Archway of Innovation",
                "Land",
                None,
                None,
                "This land enters tapped unless you control an Island.\n{T}: Add {U}.\n{U}, {T}: The next spell you cast this turn has improvise.",
                KeywordAbility::Improvise,
            ),
        ] {
            let card = InventoryCard {
                scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
                oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
                name: name.into(),
                layout: "normal".into(),
                faces: vec![spellwake_content::CatalogFace {
                    name: name.into(),
                    type_line: Some(kind.into()),
                    oracle_text: Some(text.into()),
                    mana_cost: cost.map(str::to_owned),
                    power: pt.map(|(power, _)| power.into()),
                    toughness: pt.map(|(_, toughness)| toughness.into()),
                }],
                legalities: BTreeMap::new(),
                eligible_formats: Vec::new(),
                required_by_base_formats: true,
                program_evidence: "test",
            };
            let program = compile_with_reason(&card, &vocabulary)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let bodies = program
                .activated
                .iter()
                .map(|ability| &ability.body)
                .chain(program.triggered.iter().map(|ability| &ability.body));
            assert!(bodies.flat_map(|body| &body.instructions).any(|instruction|
                matches!(instruction, Instruction::CreateTemporaryRule { rule, duration: ResolvedDuration::UntilEndOfTurn }
                    if matches!(rule.as_ref(), TemporaryRuleKind::NextSpellKeyword { filter, keyword: actual }
                        if *actual == keyword && *filter == ObjectFilter::any()))), "{name}");
        }
    }
}
