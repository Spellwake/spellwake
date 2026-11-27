use super::*;
use spellwake_engine::characteristics::{
    ActivatedCostScope, ActivationSuppression, CastKeywordCostKind, TriggerMultiplierEvent,
    TriggerMultiplierRule,
};
use spellwake_engine::program::{AttachedCountKind, DynamicStatAnchor};

pub(super) fn lower_static(
    st: &StaticAbilityAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    is_spell: bool,
) -> Result<(), &'static str> {
    match st {
        StaticAbilityAst::AttachmentRestriction { .. } => {
            // Already consumed when creating Aura attachment restriction
            if matches!(
                program.permanent().and_then(|p| p.attachment),
                Some(AttachmentKind::Aura(_))
            ) {
                Ok(())
            } else {
                Err("oracle:enchant-on-non-aura")
            }
        }
        StaticAbilityAst::ContinuousModifyPT {
            subject,
            dx,
            dy,
            grant_keywords,
            grant_ability_text,
            scale,
            condition,
            paired_rule,
        } => {
            let mut selectors = lower_static_permanent_selectors_with_context(
                subject,
                condition.as_deref(),
                vocabulary,
            )?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let expr_ctx = StaticExprContext::from_subject_and_program(subject.as_ref(), program);
            let pt_part = lower_modify_pt_part(dx, dy, scale.as_deref(), expr_ctx, vocabulary)?;
            let (extra_prot_parts, remaining_grant_kws) =
                partition_static_keyword_grants(grant_keywords, vocabulary, ids);
            let ab_change = if !remaining_grant_kws.is_empty() {
                Some(lower_ability_change_with_ids(
                    &remaining_grant_kws,
                    vocabulary,
                    ids,
                )?)
            } else {
                None
            };
            let mut granted_face_fallback: Option<FaceAst> = None;
            let granted_parts = if let Some(raw_text) = grant_ability_text {
                let cleaned = raw_text
                    .trim()
                    .trim_matches('"')
                    .trim()
                    .trim_end_matches(',')
                    .trim();
                if cleaned.is_empty() {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let synthetic = compile_card(
                    &CardInput {
                        oracle_id: "",
                        name: "~",
                        layout: "normal",
                        faces: vec![FaceInput {
                            name: "~",
                            type_line: if looks_like_loyalty_ability(cleaned) {
                                "Planeswalker"
                            } else {
                                "Creature"
                            },
                            oracle_text: cleaned,
                        }],
                    },
                    &compiler_vocab,
                );
                if !synthetic.strictly_compiled || synthetic.faces.len() != 1 {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                if let Ok(parts) =
                    lower_granted_ability_face_parts(&synthetic.faces[0], vocabulary, ids)
                {
                    parts
                } else {
                    granted_face_fallback = synthetic.faces.into_iter().next();
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            for selector in &selectors {
                let mut parts = vec![pt_part.clone()];
                if let Some(ab) = &ab_change {
                    parts.push(StaticPart::Abilities(ab.clone()));
                }
                for pp in &extra_prot_parts {
                    parts.push(pp.clone());
                }
                if let [gp] = granted_parts.as_slice()
                    && ab_change.is_none()
                    && extra_prot_parts.is_empty()
                {
                    parts.push(gp.clone());
                } else {
                    for gp in &granted_parts {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::Battlefield {
                                id: ids.static_id(),
                                selector: selector.clone(),
                                parts: StaticParts(vec![gp.clone()]),
                            },
                        )?;
                    }
                }
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(parts),
                    },
                )?;
            }
            if let Some(face) = granted_face_fallback.as_ref() {
                emit_granted_ability_face_for_selectors(
                    face, &selectors, program, vocabulary, ids,
                )?;
            }
            if let Some(paired) = paired_rule {
                lower_paired_static(paired, &selectors, program, vocabulary, ids)?;
            }
            Ok(())
        }
        StaticAbilityAst::GrantKeywords {
            subject,
            keywords,
            condition,
            paired_rule,
        } => {
            if matches!(subject.as_ref(), SubjectRefAst::Player(PlayerRefAst::You)) {
                if paired_rule.is_none() && keywords.len() == 1 {
                    let kw_low = keywords[0].trim().to_ascii_lowercase();
                    let rule = match kw_low.as_str() {
                        "hexproof" => Some(StaticRestriction::ControllerHasHexproof),
                        "shroud" => Some(StaticRestriction::ControllerHasShroud),
                        _ => None,
                    };
                    if let Some(rule) = rule {
                        return push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule,
                            },
                            condition.as_deref(),
                            vocabulary,
                        );
                    }
                }
                return Err("oracle:unsupported-player-grant-keywords");
            }
            if let SubjectRefAst::Compound(items) = subject.as_ref()
                && items
                    .iter()
                    .any(|it| matches!(it, SubjectRefAst::Player(_)))
            {
                let only_you_player = items
                    .iter()
                    .filter(|it| matches!(it, SubjectRefAst::Player(_)))
                    .all(|it| matches!(it, SubjectRefAst::Player(PlayerRefAst::You)));
                let player_rule = if keywords.len() == 1 {
                    match keywords[0].trim().to_ascii_lowercase().as_str() {
                        "hexproof" => Some(StaticRestriction::ControllerHasHexproof),
                        "shroud" => Some(StaticRestriction::ControllerHasShroud),
                        _ => None,
                    }
                } else {
                    None
                };
                if !only_you_player || paired_rule.is_some() || player_rule.is_none() {
                    return Err("oracle:unsupported-player-grant-keywords");
                }
                let player_rule = player_rule.ok_or("oracle:unsupported-player-grant-keywords")?;
                let mut perm_selectors = Vec::new();
                for item in items {
                    if !matches!(item, SubjectRefAst::Player(_)) {
                        perm_selectors.extend(lower_static_permanent_selectors(item, vocabulary)?);
                    }
                }
                for sel in &mut perm_selectors {
                    if let Some(cond) = condition {
                        apply_static_selector_condition(sel, cond, vocabulary)?;
                    }
                }
                push_conditional_static(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: player_rule,
                    },
                    condition.as_deref(),
                    vocabulary,
                )?;
                let ab_change = lower_ability_change_with_ids(keywords, vocabulary, ids)?;
                for selector in perm_selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![StaticPart::Abilities(ab_change.clone())]),
                        },
                    )?;
                }
                return Ok(());
            }
            if !is_spell
                && paired_rule.is_none()
                && let SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) =
                    subject.as_ref()
                && is_spell_keyword_grant_filter(f)
            {
                return lower_spell_keyword_grant(
                    f,
                    keywords,
                    condition.as_deref(),
                    program,
                    vocabulary,
                    ids,
                );
            }
            let mut selectors = lower_static_permanent_selectors_with_context(
                subject,
                condition.as_deref(),
                vocabulary,
            )?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let (mut parts_vec, remaining_kws) =
                partition_static_keyword_grants(keywords, vocabulary, ids);
            if !remaining_kws.is_empty() {
                let ab_change = lower_ability_change_with_ids(&remaining_kws, vocabulary, ids)?;
                parts_vec.push(StaticPart::Abilities(ab_change));
            }
            if parts_vec.is_empty() {
                return Err("oracle:empty-grant-keywords");
            }
            for selector in &selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(parts_vec.clone()),
                    },
                )?;
            }
            if let Some(paired) = paired_rule {
                lower_paired_static(paired, &selectors, program, vocabulary, ids)?;
            }
            Ok(())
        }
        StaticAbilityAst::GrantAbilityText {
            subject,
            ability_text,
            compiled,
            condition,
        } => {
            let mut selectors = lower_static_permanent_selectors_with_context(
                subject,
                condition.as_deref(),
                vocabulary,
            )?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let owned_face;
            let face: &FaceAst = if let Some(compiled_face) = compiled.as_deref() {
                compiled_face
            } else {
                let cleaned = ability_text
                    .trim()
                    .trim_matches('"')
                    .trim()
                    .trim_end_matches(',')
                    .trim();
                if cleaned.is_empty() {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let synthetic = compile_card(
                    &CardInput {
                        oracle_id: "",
                        name: "~",
                        layout: "normal",
                        faces: vec![spellwake_oracle_compiler::FaceInput {
                            name: "~",
                            type_line: if looks_like_loyalty_ability(cleaned) {
                                "Planeswalker"
                            } else {
                                "Creature"
                            },
                            oracle_text: cleaned,
                        }],
                    },
                    &compiler_vocab,
                );
                if !synthetic.strictly_compiled || synthetic.faces.len() != 1 {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                owned_face = synthetic
                    .faces
                    .into_iter()
                    .next()
                    .ok_or("oracle:unsupported-grant-ability-text")?;
                &owned_face
            };
            emit_granted_ability_face_for_selectors(face, &selectors, program, vocabulary, ids)
        }
        StaticAbilityAst::SetOrAddType {
            subject,
            type_or_subtype,
            additive,
            condition,
        } => {
            let mut selectors = lower_static_permanent_selectors_with_context(
                subject,
                condition.as_deref(),
                vocabulary,
            )?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let entry_tap = program.permanent().and_then(|p| p.entry_tap);
            let type_parts = parse_static_type_and_color_parts_with_entry(
                type_or_subtype,
                *additive,
                entry_tap,
                vocabulary,
            )?;
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(type_parts.clone()),
                    },
                )?;
            }
            Ok(())
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
            condition,
        } => {
            let mut selectors = lower_static_permanent_selectors_with_context(
                subject,
                condition.as_deref(),
                vocabulary,
            )?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let entry_tap = program.permanent().and_then(|p| p.entry_tap);
            let mut parts = Vec::new();
            let mut has_color_part = false;
            let mut effective_grant_keywords: Vec<String> = grant_keywords.clone();
            if let Some(desc) = set_types {
                let mut desc_type_slice: &str = desc.as_str();
                if effective_grant_keywords.is_empty()
                    && let Some((lhs_ty, rhs_kw)) = desc_type_slice
                        .split_once(" and has ")
                        .or_else(|| desc_type_slice.split_once(" and have "))
                {
                    let compiler_vocab = cached_compiler_vocab(vocabulary);
                    if let Some(parsed_kws) = spellwake_oracle_compiler::filter::parse_keyword_list(
                        rhs_kw.trim(),
                        &compiler_vocab,
                    ) && !parsed_kws.is_empty()
                    {
                        desc_type_slice = lhs_ty.trim();
                        effective_grant_keywords = parsed_kws;
                    }
                }
                let type_parts = parse_static_type_and_color_parts_with_entry(
                    desc_type_slice,
                    *additive_types,
                    entry_tap,
                    vocabulary,
                )?;
                if type_parts
                    .iter()
                    .any(|part| matches!(part, StaticPart::Color(_)))
                {
                    has_color_part = true;
                }
                parts.extend(type_parts);
            }
            if let Some(color_str) = set_colors {
                if has_color_part {
                    return Err("oracle:unsupported-layer-color");
                }
                let color_clean = color_str.trim().trim_end_matches('.').trim();
                let colors = if color_clean.eq_ignore_ascii_case("all colors") {
                    BTreeSet::from([
                        Color::White,
                        Color::Blue,
                        Color::Black,
                        Color::Red,
                        Color::Green,
                    ])
                } else {
                    let mut parsed_colors = BTreeSet::new();
                    let mut saw_colorless = false;
                    for raw_tok in color_clean
                        .replace(" and ", " ")
                        .replace(',', " ")
                        .split_whitespace()
                    {
                        match raw_tok.to_ascii_lowercase().as_str() {
                            "white" => {
                                parsed_colors.insert(Color::White);
                            }
                            "blue" => {
                                parsed_colors.insert(Color::Blue);
                            }
                            "black" => {
                                parsed_colors.insert(Color::Black);
                            }
                            "red" => {
                                parsed_colors.insert(Color::Red);
                            }
                            "green" => {
                                parsed_colors.insert(Color::Green);
                            }
                            "colorless" => {
                                saw_colorless = true;
                            }
                            "a" | "an" => {}
                            _ => return Err("oracle:unsupported-layer-color"),
                        }
                    }
                    if parsed_colors.is_empty() && !saw_colorless {
                        return Err("oracle:unsupported-layer-color");
                    }
                    parsed_colors
                };
                parts.push(StaticPart::Color(ColorOperation::Set(
                    StaticColorOperand::Literal(colors),
                )));
            }
            if *lose_all_abilities
                || !effective_grant_keywords.is_empty()
                || !lose_keywords.is_empty()
            {
                let (special_parts, rem_kws) =
                    partition_static_keyword_grants(&effective_grant_keywords, vocabulary, ids);
                parts.extend(special_parts);
                if *lose_all_abilities || !rem_kws.is_empty() || !lose_keywords.is_empty() {
                    let mut change = if !rem_kws.is_empty() {
                        lower_ability_change_with_ids(&rem_kws, vocabulary, ids)?
                    } else {
                        AbilityChange::default()
                    };
                    if *lose_all_abilities {
                        change.removal = Some(AbilityRemoval::All);
                    }
                    if !lose_keywords.is_empty() {
                        change
                            .suppressed_keywords
                            .extend(lower_keyword_list_with_vocab(lose_keywords, vocabulary)?);
                    }
                    parts.push(StaticPart::Abilities(change));
                }
            }
            if let Some((p_raw, t_raw)) = base_pt {
                parts.push(lower_static_base_pt_part(
                    p_raw,
                    t_raw,
                    vocabulary,
                    "oracle:unsupported-layer-pt",
                )?);
            }
            if parts.is_empty() {
                return Err("oracle:empty-layer-effect");
            }
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(parts.clone()),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::FollowUp(ins) => {
            if lower_followup_instructions(ins, program, vocabulary, ids).is_ok() {
                return Ok(());
            }
            lower_exert_attack_followup(ins, program, vocabulary, ids, is_spell)
        }
        StaticAbilityAst::Compound(items) => {
            let mut idx = 0usize;
            let mut prev_subject: Option<SubjectRefAst> = None;
            let mut prev_looked_exiled = false;
            while idx < items.len() {
                if !is_spell
                    && let StaticAbilityAst::FollowUp(first_ins) = &items[idx]
                    && let Some(StaticAbilityAst::FollowUp(next_ins)) = items.get(idx + 1)
                    && let Some(res) = try_lower_compound_exile_play_replacement(
                        first_ins, next_ins, program, vocabulary, ids,
                    )
                {
                    res?;
                    prev_looked_exiled = false;
                    idx += 2;
                    continue;
                }
                if let StaticAbilityAst::RuleOrCombatRestriction {
                    subject: None,
                    rule,
                    filter: None,
                    condition: None,
                } = &items[idx]
                    && rule
                        .trim()
                        .trim_end_matches('.')
                        .eq_ignore_ascii_case("you may exert ~ as it attacks")
                    && let Some(StaticAbilityAst::FollowUp(ins)) = items.get(idx + 1)
                {
                    lower_exert_attack_followup(ins, program, vocabulary, ids, is_spell)?;
                    prev_looked_exiled = false;
                    idx += 2;
                    continue;
                }
                if !is_spell
                    && let StaticAbilityAst::PlayOrCastPermission {
                        cast_only,
                        filter,
                        permission,
                        condition,
                    } = &items[idx]
                    && let Some(StaticAbilityAst::FollowUp(ins)) = items.get(idx + 1)
                    && let Some(pay_life_mv) = extract_cast_this_way_alt_cost_followup(ins)
                    && let Some(res) = try_lower_play_or_cast_with_alt_cost(
                        permission,
                        *cast_only,
                        filter,
                        condition.as_deref(),
                        false,
                        pay_life_mv,
                        program,
                        vocabulary,
                        ids,
                    )
                {
                    res?;
                    prev_looked_exiled = false;
                    idx += 2;
                    continue;
                }
                if !is_spell
                    && let StaticAbilityAst::PlayOrCastPermission {
                        cast_only,
                        filter,
                        permission,
                        condition,
                    } = &items[idx]
                    && let Some(StaticAbilityAst::FollowUp(ins)) = items.get(idx + 1)
                    && extract_cast_this_way_flash_followup(ins)
                    && let Some(res) = try_lower_play_or_cast_with_flash(
                        permission,
                        *cast_only,
                        filter,
                        condition.as_deref(),
                        false,
                        program,
                        vocabulary,
                        ids,
                    )
                {
                    res?;
                    prev_looked_exiled = false;
                    idx += 2;
                    continue;
                }
                let mut cur = items[idx].clone();
                if prev_looked_exiled
                    && let StaticAbilityAst::PlayOrCastPermission {
                        ref mut permission, ..
                    } = cur
                {
                    let p_low = permission.to_ascii_lowercase();
                    if p_low.contains("from among those cards") {
                        *permission = p_low
                            .replace("from among those cards", "from among cards exiled with ~");
                    }
                }
                if let Some(prev) = prev_subject.as_ref() {
                    resolve_static_anaphora_subject(&mut cur, prev);
                }
                if matches!(cur, StaticAbilityAst::AttachmentRestriction { .. }) {
                    prev_subject = Some(SubjectRefAst::AttachedHost(
                        "enchanted permanent".to_string(),
                    ));
                } else if let Some(subj) = static_ast_subject(&cur)
                    && !matches!(subj, SubjectRefAst::Anaphora(_))
                {
                    prev_subject = Some(subj.clone());
                }
                if let StaticAbilityAst::FollowUp(ins) = &cur {
                    // Inside a Compound static not preceded by an exert lead-in,
                    // keep FollowUp strictly limited to replacement follow-ups.
                    lower_followup_instructions(ins, program, vocabulary, ids)?;
                    prev_looked_exiled = false;
                    idx += 1;
                    continue;
                }
                if let Some(StaticAbilityAst::Otherwise(else_static)) = items.get(idx + 1)
                    && let Some(cond) = static_ast_condition(&cur)
                {
                    lower_static(&cur, program, vocabulary, ids, is_spell)?;
                    let neg = ConditionAst::Not(Box::new(cond.clone()));
                    let mut wrapped = else_static.as_ref().clone();
                    if let Some(prev) = prev_subject.as_ref() {
                        resolve_static_anaphora_subject(&mut wrapped, prev);
                    }
                    attach_ast_condition(&mut wrapped, neg);
                    lower_static(&wrapped, program, vocabulary, ids, is_spell)?;
                    prev_looked_exiled = false;
                    idx += 2;
                    continue;
                }
                let cur_is_look_exiled = is_look_at_source_exiled_static(&cur);
                lower_static(&cur, program, vocabulary, ids, is_spell)?;
                prev_looked_exiled = cur_is_look_exiled;
                idx += 1;
            }
            Ok(())
        }
        StaticAbilityAst::PreventAllDamageStatic {
            combat_only,
            noncombat_only,
            both_ways,
            subject,
            by,
        } => {
            let kind = if *combat_only {
                StaticDamageKind::CombatOnly
            } else if *noncombat_only {
                StaticDamageKind::NoncombatOnly
            } else {
                StaticDamageKind::Any
            };
            if *both_ways {
                if matches!(subject.as_ref(), SubjectRefAst::SelfObject) && *combat_only {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::DamageRule {
                            id: ids.static_id(),
                            rule: StaticDamageRule::PreventSourceCombat,
                        },
                    );
                }
                if matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_)) && !*noncombat_only {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::DamageRule {
                            id: ids.static_id(),
                            rule: StaticDamageRule::PreventEnchantedCreature {
                                combat_only: *combat_only,
                            },
                        },
                    );
                }
                let recipients = lower_damage_recipients(subject, vocabulary)?;
                let (src_filter, src_ctrl) = lower_damage_source_filter(subject, vocabulary)?;
                for recipient in recipients {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::DamageRule {
                            id: ids.static_id(),
                            rule: StaticDamageRule::ModifyDamage {
                                source: None,
                                source_controller: PlayerRelation::AnyParticipant,
                                recipient,
                                kind,
                                op: StaticDamageOp::PreventAll,
                            },
                        },
                    )?;
                }
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DamageRule {
                        id: ids.static_id(),
                        rule: StaticDamageRule::ModifyDamage {
                            source: src_filter,
                            source_controller: src_ctrl,
                            recipient: StaticDamageRecipient::Any,
                            kind,
                            op: StaticDamageOp::PreventAll,
                        },
                    },
                );
            }
            if by.is_none() && matches!(subject.as_ref(), SubjectRefAst::SelfObject) && *combat_only
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DamageRule {
                        id: ids.static_id(),
                        rule: StaticDamageRule::PreventIncomingCombatToSource,
                    },
                );
            }
            let (source, source_controller) = match by.as_deref() {
                None => (None, PlayerRelation::AnyParticipant),
                Some(by_subj) => lower_damage_source_filter(by_subj, vocabulary)?,
            };
            let recipients = lower_damage_recipients(subject, vocabulary)?;
            for recipient in recipients {
                push_static_ability(
                    program,
                    StaticContinuousAbility::DamageRule {
                        id: ids.static_id(),
                        rule: StaticDamageRule::ModifyDamage {
                            source,
                            source_controller,
                            recipient,
                            kind,
                            op: StaticDamageOp::PreventAll,
                        },
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::ControlAttachedHost => push_static_ability(
            program,
            StaticContinuousAbility::Battlefield {
                id: ids.static_id(),
                selector: PermanentSelector {
                    attached_to_source: true,
                    ..PermanentSelector::default()
                },
                parts: StaticParts(vec![StaticPart::ControlSourceController]),
            },
        ),
        StaticAbilityAst::CdaPowerToughness { power, toughness } => {
            let expr_ctx = StaticExprContext::for_source(program);
            let p = match power {
                Some(v) => Some(Box::new(lower_numeric_expr_with_context(
                    v, expr_ctx, vocabulary,
                )?)),
                None => None,
            };
            let t = match toughness {
                Some(v) => Some(Box::new(lower_numeric_expr_with_context(
                    v, expr_ctx, vocabulary,
                )?)),
                None => None,
            };
            if p.is_none() && t.is_none() {
                return Err("oracle:empty-cda-pt");
            }
            push_static_ability(
                program,
                StaticContinuousAbility::CharacteristicDefining {
                    id: ids.static_id(),
                    parts: DefiningParts(vec![DefiningPart::PowerToughness {
                        power: p,
                        toughness: t,
                    }]),
                },
            )
        }
        StaticAbilityAst::CostModifier {
            subject,
            delta,
            less: true,
            next_only: false,
            per,
            scale_expr,
            condition,
        } => {
            // Self spell cost reduction: "This spell costs {N} less to cast [for each ...] [if ...]"
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                let expr_ctx = StaticExprContext::for_source(program);
                let wrap_optional_condition = |expr: ValueExpr| -> Result<ValueExpr, &'static str> {
                    let Some(cond) = condition.as_deref() else {
                        return Ok(expr);
                    };
                    let mut tmp_builder = BodyBuilder::new();
                    let rc =
                        lower_resolution_condition(cond, &mut tmp_builder, vocabulary, false, None)
                            .map_err(|_| "oracle:unsupported-self-discount")?;
                    if tmp_builder.target != BodyTarget::None {
                        return Err("oracle:unsupported-self-discount");
                    }
                    Ok(ValueExpr::conditional(
                        rc.left,
                        rc.comparison,
                        rc.right,
                        expr,
                        ValueExpr::literal(0_i64),
                    ))
                };
                if let Some(se) = scale_expr
                    && per.is_none()
                {
                    let generic_n = if delta.trim().eq_ignore_ascii_case("{x}") {
                        1
                    } else {
                        parse_generic_mana_amount(delta)
                            .ok_or("oracle:unsupported-self-discount-delta")?
                    };
                    if generic_n == 0 {
                        return Err("oracle:unsupported-self-discount-delta");
                    }
                    let is_life_below_starting = matches!(
                        condition.as_deref(),
                        Some(ConditionAst::Compare {
                            lhs,
                            op: CompareOp::Lt,
                            rhs,
                        }) if matches!(
                            lhs.as_ref(),
                            ValueExprAst::PlayerStat {
                                player: PlayerRefAst::You,
                                stat: PlayerStatKind::LifeTotal,
                            }
                        ) && matches!(
                            rhs.as_ref(),
                            ValueExprAst::StartingLifeTotal(PlayerRefAst::You)
                        )
                    );
                    let base_expr = if matches!(se.as_ref(), ValueExprAst::PriorResult)
                        && is_life_below_starting
                    {
                        ValueExpr::Sub(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::ControllerStartingLife,
                            )),
                            Box::new(ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife)),
                        )
                    } else {
                        lower_static_value_expr(se, expr_ctx, vocabulary)?
                    };
                    let scaled = if generic_n == 1 {
                        base_expr
                    } else {
                        scale_value_expr(base_expr, i64::from(generic_n))
                    };
                    let final_expr = wrap_optional_condition(scaled)?;
                    if final_expr.is_world_evaluable() {
                        program
                            .cast_reductions
                            .push(CastReduction::Dynamic(Box::new(final_expr)));
                        return Ok(());
                    }
                    return Err("oracle:unsupported-self-discount");
                }
                if scale_expr.is_some() {
                    return Err("oracle:unsupported-self-discount");
                }
                let generic_n = if delta.trim().eq_ignore_ascii_case("{x}") && per.is_some() {
                    1
                } else {
                    parse_generic_mana_amount(delta)
                        .ok_or("oracle:unsupported-self-discount-delta")?
                };
                if generic_n == 0 {
                    return Err("oracle:unsupported-self-discount-delta");
                }
                if let Some(per_filter) = per {
                    if condition.is_none() {
                        if generic_n == 1
                            && let Ok(count) = lower_cast_cost_count(per_filter)
                        {
                            program
                                .cast_reductions
                                .push(CastReduction::PerObject(count));
                            return Ok(());
                        }
                        if per_filter.zone.is_none()
                            && per_filter.controller == Some(ControllerRelation::You)
                            && let Ok(mut sel) =
                                lower_static_filter_to_permanent_selector(per_filter, vocabulary)
                        {
                            sel.controller = ControllerSelector::SourceController;
                            for _ in 0..generic_n {
                                program
                                    .cast_reductions
                                    .push(CastReduction::Affinity(Box::new(sel.clone())));
                            }
                            return Ok(());
                        }
                    }
                    if let Ok(base_expr) = lower_static_value_expr(
                        &ValueExprAst::Count(per_filter.clone()),
                        expr_ctx,
                        vocabulary,
                    ) {
                        let scaled = if generic_n == 1 {
                            base_expr
                        } else {
                            scale_value_expr(base_expr, i64::from(generic_n))
                        };
                        if let Ok(final_expr) = wrap_optional_condition(scaled)
                            && final_expr.is_world_evaluable()
                        {
                            program
                                .cast_reductions
                                .push(CastReduction::Dynamic(Box::new(final_expr)));
                            return Ok(());
                        }
                    }
                    if condition.is_some() {
                        return Err("oracle:unsupported-self-discount");
                    }
                    let mut inner = per_filter.as_ref().clone();
                    let zone = match inner.zone.take() {
                        None => spellwake_engine::program::ValueZone::Battlefield,
                        Some(ZoneFilter::YourGraveyard) => {
                            spellwake_engine::program::ValueZone::Graveyard(
                                ResolvingPlayer::Controller,
                            )
                        }
                        Some(ZoneFilter::YourHand) => {
                            spellwake_engine::program::ValueZone::Hand(ResolvingPlayer::Controller)
                        }
                        Some(ZoneFilter::AllGraveyards | ZoneFilter::AnyGraveyard) => {
                            spellwake_engine::program::ValueZone::AllGraveyards
                        }
                        Some(ZoneFilter::AnyHand | ZoneFilter::EachPlayerHand) => {
                            spellwake_engine::program::ValueZone::AllHands
                        }
                        Some(ZoneFilter::OpponentsHands) => {
                            spellwake_engine::program::ValueZone::OpponentsHands
                        }
                        _ => return Err("oracle:unsupported-self-discount"),
                    };
                    let obj = if is_unrestricted_filter(&inner) {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    program.cast_reductions.push(CastReduction::CountZone {
                        zone,
                        filter: Box::new(obj),
                        per_card: generic_n,
                    });
                    return Ok(());
                }
                if per.is_none()
                    && let Some(cond) = condition
                {
                    if let ConditionAst::TargetsFilter(f) = &**cond {
                        let obj = lower_object_filter(f, vocabulary)?;
                        program.cast_reductions.push(CastReduction::IfTargets {
                            filter: Box::new(obj),
                            discount: generic_n,
                        });
                        return Ok(());
                    }
                    let mut tmp_builder = BodyBuilder::new();
                    let rc = lower_resolution_condition(
                        cond,
                        &mut tmp_builder,
                        vocabulary,
                        false,
                        None,
                    )?;
                    if tmp_builder.target != BodyTarget::None {
                        return Err("oracle:unsupported-self-discount");
                    }
                    program.cast_reductions.push(CastReduction::Conditional {
                        condition: Box::new(rc),
                        discount: generic_n,
                    });
                    return Ok(());
                }
                return Err("oracle:unsupported-self-discount");
            }
            // Battlefield permanent cost modifier on anaphora subjects
            // (activated ability costs, self-ability target/world discounts, keyword cast costs, scoped spells).
            if !is_spell && let SubjectRefAst::Anaphora(raw) = subject.as_ref() {
                return lower_anaphora_cost_modifier(
                    AnaphoraCostModifier {
                        raw,
                        delta,
                        less: true,
                        per: per.as_deref(),
                        scale_expr: scale_expr.as_deref(),
                        condition: condition.as_deref(),
                        min_mana_floor: None,
                    },
                    program,
                    vocabulary,
                    ids,
                );
            }
            // Battlefield permanent discounting spells
            if is_spell {
                return Err("oracle:unsupported-cost-modifier");
            }
            let generic_n = if delta.trim().eq_ignore_ascii_case("{x}")
                && scale_expr.is_some()
                && per.is_none()
            {
                1
            } else {
                parse_generic_mana_amount(delta).ok_or("oracle:non-generic-discount")?
            };
            let unit = NonZeroU8::new(generic_n).ok_or("oracle:zero-discount")?;
            let (any_player, matching) = lower_spell_subject_filter(subject, vocabulary)?;
            let amount = match lower_static_spell_discount_amount(
                unit,
                per.as_deref(),
                scale_expr.as_deref(),
            ) {
                Ok(a) => a,
                Err(_) => lower_dynamic_static_spell_discount(
                    unit,
                    per.as_deref(),
                    scale_expr.as_deref(),
                    any_player,
                    program,
                    vocabulary,
                )?,
            };
            push_conditional_static(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ids.static_id(),
                    modifier: SpellCostModifier::Discount {
                        any_player,
                        matching,
                        amount,
                    },
                },
                condition.as_deref(),
                vocabulary,
            )
        }
        StaticAbilityAst::CostModifier {
            subject,
            delta,
            less: false,
            next_only: false,
            per: None,
            scale_expr,
            condition,
        } if !is_spell => {
            if let SubjectRefAst::Anaphora(raw) = subject.as_ref() {
                return lower_anaphora_cost_modifier(
                    AnaphoraCostModifier {
                        raw,
                        delta,
                        less: false,
                        per: None,
                        scale_expr: scale_expr.as_deref(),
                        condition: condition.as_deref(),
                        min_mana_floor: None,
                    },
                    program,
                    vocabulary,
                    ids,
                );
            }
            if scale_expr.is_some() {
                return Err("oracle:unsupported-cost-tax");
            }
            let generic_n = parse_generic_mana_amount(delta).ok_or("oracle:non-generic-tax")?;
            let amount = NonZeroU8::new(generic_n).ok_or("oracle:zero-tax")?;
            let f = match subject.as_ref() {
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => f,
                _ => return Err("oracle:unsupported-cost-tax"),
            };
            if f.any_types.is_empty()
                && f.controller.is_none()
                && f.subtypes.is_empty()
                && f.any_colors.is_empty()
                && f.forbidden_colors.is_empty()
                && f.color_arity.is_none()
                && f.stat_bounds.is_empty()
                && f.flags.is_empty()
                && f.zone.is_none()
                && f.predicates.is_empty()
            {
                let mut forb = card_type_mask_to_set(f.forbidden_types)?;
                if forb.len() == 1
                    && let Some(exempt) = forb.pop_first()
                {
                    return push_conditional_static(
                        program,
                        StaticContinuousAbility::CostModifier {
                            id: ids.static_id(),
                            modifier: SpellCostModifier::IncreaseExceptType { exempt, amount },
                        },
                        condition.as_deref(),
                        vocabulary,
                    );
                }
            }
            let (any_player, opponents_only) = match f.controller {
                None => (true, false),
                Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => (false, true),
                Some(ControllerRelation::You) => (false, false),
                _ => return Err("oracle:unsupported-cost-modifier"),
            };
            let mut stripped_f = f.as_ref().clone();
            stripped_f.controller = None;
            let matching = lower_spell_filter(&stripped_f, vocabulary)?;
            push_conditional_static(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ids.static_id(),
                    modifier: SpellCostModifier::Increase {
                        any_player,
                        opponents_only,
                        matching,
                        amount,
                    },
                },
                condition.as_deref(),
                vocabulary,
            )
        }
        StaticAbilityAst::CostModifier {
            subject,
            delta,
            less: true,
            next_only: true,
            per: None,
            scale_expr: None,
            condition,
        } if !is_spell => lower_first_spell_discount_modifier(
            subject,
            delta,
            condition.as_deref(),
            program,
            vocabulary,
            ids,
        ),
        StaticAbilityAst::CostModifierWithFloor {
            subject,
            delta,
            less: true,
            next_only: false,
            per: None,
            scale_expr,
            min_mana,
            condition,
        } if !is_spell => {
            let SubjectRefAst::Anaphora(raw) = subject.as_ref() else {
                return Err("oracle:unsupported-cost-modifier");
            };
            lower_anaphora_cost_modifier(
                AnaphoraCostModifier {
                    raw,
                    delta,
                    less: true,
                    per: None,
                    scale_expr: scale_expr.as_deref(),
                    condition: condition.as_deref(),
                    min_mana_floor: Some(*min_mana),
                },
                program,
                vocabulary,
                ids,
            )
        }
        StaticAbilityAst::PlayOrCastPermission {
            cast_only,
            filter,
            permission,
            condition,
        } => {
            let p_low = permission.trim().trim_end_matches('.').to_ascii_lowercase();
            if p_low == "as though it had flash"
                && condition.is_none()
                && (filter.any_card || is_unrestricted_filter(filter))
            {
                program.printed_keywords.insert(KeywordAbility::Flash);
                return Ok(());
            }
            if !is_spell
                && *cast_only
                && p_low.starts_with("the first ")
                && let Some(res) = try_lower_first_spell_permission(
                    &p_low,
                    filter,
                    condition.as_deref(),
                    program,
                    vocabulary,
                    ids,
                )
            {
                return res;
            }
            if !is_spell
                && *cast_only
                && let Some(pay_life_mv) = classify_direct_alt_cast_permission(&p_low)
                && let Some((modifier, rem_cond)) = try_lower_alt_matching_permission(
                    &p_low,
                    filter,
                    condition.as_deref(),
                    false,
                    pay_life_mv,
                    vocabulary,
                )
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CostModifier {
                        id: ids.static_id(),
                        modifier,
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            if !is_spell
                && let Some((base_perm, pay_life_mv)) = strip_cast_this_way_alt_cost_suffix(&p_low)
                && let Some(res) = try_lower_play_or_cast_with_alt_cost(
                    base_perm,
                    *cast_only,
                    filter,
                    condition.as_deref(),
                    false,
                    pay_life_mv,
                    program,
                    vocabulary,
                    ids,
                )
            {
                return res;
            }
            if !is_spell
                && let Some(base_perm) = strip_cast_this_way_flash_suffix(&p_low)
                && let Some(res) = try_lower_play_or_cast_with_flash(
                    base_perm,
                    *cast_only,
                    filter,
                    condition.as_deref(),
                    false,
                    program,
                    vocabulary,
                    ids,
                )
            {
                return res;
            }
            if !is_spell
                && *cast_only
                && p_low == "any_player_free_flash"
                && filter.controller.is_none()
            {
                let matching = lower_spell_filter(filter, vocabulary)?;
                if !spell_filter_supported_by_cast_alt_cost(&matching) {
                    return Err("oracle:unsupported-cast-permission");
                }
                let mut probe = program.clone();
                push_conditional_static(
                    &mut probe,
                    StaticContinuousAbility::CostModifier {
                        id: ids.static_id(),
                        modifier: SpellCostModifier::AlternativeForMatching {
                            filter: matching,
                            mana: ManaCost::free(),
                            once_each_turn: false,
                            own_turn_only: false,
                            pay_life_mv: false,
                            each_player: true,
                        },
                    },
                    condition.as_deref(),
                    vocabulary,
                )?;
                push_conditional_static(
                    &mut probe,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::Flash {
                            any_player: true,
                            matching,
                        },
                    },
                    condition.as_deref(),
                    vocabulary,
                )?;
                *program = probe;
                return Ok(());
            }
            if !is_spell
                && *cast_only
                && p_low == "first_each_turn_flash"
                && matches!(filter.controller, None | Some(ControllerRelation::You))
            {
                let mut stripped_f = filter.as_ref().clone();
                stripped_f.controller = None;
                let matching = lower_spell_filter(&stripped_f, vocabulary)?;
                if !spell_filter_supported_by_ordinal_this_turn(&matching) {
                    return Err("oracle:unsupported-cast-permission");
                }
                let matching = matching.ordinal_this_turn(NonZeroU32::MIN);
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::Flash {
                            any_player: false,
                            matching,
                        },
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if !is_spell
                && *cast_only
                && let Some((timing_cond, inner_perm)) = split_leading_flash_timing_prefix(&p_low)
                && let Some((any_player, matching)) =
                    parse_flash_permission_clause(inner_perm, filter, vocabulary)
            {
                let combined_cond = match condition.as_deref() {
                    Some(c) => ConditionAst::And(Box::new(timing_cond), Box::new(c.clone())),
                    None => timing_cond,
                };
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::Flash {
                            any_player,
                            matching,
                        },
                    },
                    Some(&combined_cond),
                    vocabulary,
                );
            }
            if p_low == "as though they had flash"
                || p_low == "as though it had flash"
                || p_low == "any_player_flash"
                || p_low == "you may cast spells as though they had flash"
                || p_low == "any player may cast spells as though they had flash"
                || (p_low.starts_with("you may cast ")
                    && p_low.ends_with(" as though they had flash"))
                || (p_low.starts_with("any player may cast ")
                    && p_low.ends_with(" as though they had flash"))
            {
                let any_player = p_low == "any_player_flash"
                    || (filter.controller.is_none() && p_low.starts_with("any player may cast "));
                let mut stripped_f = filter.as_ref().clone();
                if !any_player && stripped_f.controller == Some(ControllerRelation::You) {
                    stripped_f.controller = None;
                }
                let matching = lower_spell_filter(&stripped_f, vocabulary)?;
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::Flash {
                            any_player,
                            matching,
                        },
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            let extra_lands: Option<u8> = match p_low.as_str() {
                "you may play an additional land on each of your turns" => Some(1),
                "you may play two additional lands on each of your turns" => Some(2),
                "you may play three additional lands on each of your turns" => Some(3),
                _ => None,
            };
            if let Some(n) = extra_lands {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::AdditionalLandPlays(
                            NonZeroU8::new(n).ok_or("oracle:unreachable")?,
                        ),
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if (filter.any_card
                || is_unrestricted_filter(filter)
                || (filter.any_types == AstCardTypeMask::LAND
                    && filter_only_has(
                        filter,
                        FilterAllow {
                            types: true,
                            ..FilterAllow::NONE
                        },
                    )))
                && let Some(n) = parse_all_players_additional_land_plays(&p_low)
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::AllPlayersAdditionalLandPlays(n),
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if matches!(
                p_low.as_str(),
                "you may look at the top card of your library any time"
                    | "look_top_library_any_time"
            ) {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::SeeLibraryTop { public: false },
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if matches!(
                p_low.as_str(),
                "play with the top card of your library revealed"
            ) {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::SeeLibraryTop { public: true },
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if !is_spell
                && let Some((permission, rem_cond)) = try_lower_library_top_filtered_permission(
                    &p_low,
                    *cast_only,
                    filter,
                    false,
                    condition.as_deref(),
                    vocabulary,
                )
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission,
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            if matches!(
                p_low.as_str(),
                "from the top of your library"
                    | "from the top of your library at any time"
                    | "you may play lands from the top of your library"
                    | "you may play land cards from the top of your library"
                    | "you may play lands and cast spells from the top of your library"
                    | "you may cast instant and sorcery spells from the top of your library"
            ) && matches!(filter.zone, None | Some(ZoneFilter::YourLibrary))
            {
                let top_perm = if filter.any_card || is_unrestricted_filter(filter) {
                    Some(CastPermissionRule::PlayLibraryTop(if *cast_only {
                        LibraryTopPermission::Nonland
                    } else {
                        LibraryTopPermission::All
                    }))
                } else if filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                ) {
                    if !*cast_only && filter.any_types == AstCardTypeMask::LAND {
                        Some(CastPermissionRule::PlayLibraryTop(
                            LibraryTopPermission::Land,
                        ))
                    } else if filter.any_types == AstCardTypeMask::CREATURE {
                        Some(CastPermissionRule::PlayLibraryTop(
                            LibraryTopPermission::Creature,
                        ))
                    } else if filter.any_types == AstCardTypeMask::ARTIFACT {
                        Some(CastPermissionRule::PlayLibraryTop(
                            LibraryTopPermission::Artifact,
                        ))
                    } else if filter.any_types
                        == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY)
                    {
                        Some(CastPermissionRule::PlayLibraryTop(
                            LibraryTopPermission::InstantOrSorcery,
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(permission) = top_perm {
                    return push_conditional_static(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ids.static_id(),
                            permission,
                        },
                        condition.as_deref(),
                        vocabulary,
                    );
                }
                let mut inner = filter.as_ref().clone();
                inner.zone = None;
                if let Ok(obj) = lower_object_filter(&inner, vocabulary) {
                    return push_conditional_static(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ids.static_id(),
                            permission: CastPermissionRule::PlayLibraryTopMatching {
                                lands_allowed: !*cast_only,
                                filter: Box::new(obj),
                            },
                        },
                        condition.as_deref(),
                        vocabulary,
                    );
                }
            }
            if !*cast_only
                && matches!(filter.zone, None | Some(ZoneFilter::YourLibrary))
                && (filter.any_card || is_unrestricted_filter(filter))
                && let Some(flt_raw) = p_low
                    .strip_prefix("you may play lands and cast ")
                    .and_then(|rest| rest.strip_suffix(" from the top of your library"))
                && let Some(obj) = parse_library_top_cast_filter(flt_raw.trim(), vocabulary)
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::PlayLibraryTopMatching {
                            lands_allowed: true,
                            filter: Box::new(obj),
                        },
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if !is_spell
                && matches!(filter.zone, None | Some(ZoneFilter::YourLibrary))
                && (filter.any_card || is_unrestricted_filter(filter))
                && let Some(head) = p_low
                    .strip_suffix(" from the top of your library")
                    .or_else(|| p_low.strip_suffix(" from the top of your library at any time"))
                && !head.contains("without paying")
                && !head.contains(" by ")
                && let Some((lands_allowed, obj, false)) =
                    parse_play_or_cast_clause(head, vocabulary)
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::PlayLibraryTopMatching {
                            lands_allowed,
                            filter: Box::new(obj),
                        },
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if matches!(
                p_low.as_str(),
                "from your graveyard"
                    | "you may play lands from your graveyard"
                    | "you may play land cards from your graveyard"
            ) && !*cast_only
                && filter.any_types == AstCardTypeMask::LAND
                && filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
                && matches!(filter.zone, None | Some(ZoneFilter::YourGraveyard))
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::GraveyardLandPlay,
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if matches!(
                p_low.as_str(),
                "from your graveyard" | "you may cast creature spells from your graveyard"
            ) && filter.any_types == AstCardTypeMask::CREATURE
                && filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
                && matches!(filter.zone, None | Some(ZoneFilter::YourGraveyard))
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::GraveyardCreatureCast,
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if matches!(
                p_low.as_str(),
                "from your graveyard or from exile"
                    | "from your graveyard or exile"
                    | "you may cast ~ from your graveyard or from exile"
            ) && *cast_only
                && condition.is_none()
                && filter.any_card
                && !filter.any_spell
                && filter.zone.is_none()
                && is_unrestricted_filter(filter)
                && program.graveyard_self_cast.is_none()
                && !program.has_type(CardType::Land)
                && program.printed_mana_cost().is_some()
            {
                program.graveyard_self_cast =
                    Some(spellwake_engine::program::GraveyardSelfCast::GraveyardOrExile);
                return Ok(());
            }
            let self_gy_filter_ok = *cast_only
                && filter.any_card
                && !filter.any_spell
                && !filter.any_permanent
                && matches!(filter.zone, None | Some(ZoneFilter::YourGraveyard))
                && filter_only_has(
                    filter,
                    FilterAllow {
                        zone: true,
                        ..FilterAllow::NONE
                    },
                );
            if self_gy_filter_ok
                && condition.is_none()
                && let Some(after_by) = p_low
                    .strip_prefix("from your graveyard by ")
                    .or_else(|| p_low.strip_prefix("you may cast ~ from your graveyard by "))
                    .or_else(|| {
                        p_low.strip_prefix("you may cast this card from your graveyard by ")
                    })
                && let Some(cost_raw) = after_by
                    .strip_suffix(" in addition to paying its other costs")
                    .or_else(|| after_by.strip_suffix(" in addition to paying its other cost"))
                && !cost_raw.contains("rather than")
                && program.graveyard_self_cast.is_none()
                && !program.has_type(CardType::Land)
                && program.printed_mana_cost().is_some()
                && let Some(extra) = parse_gerund_extra_cast_costs(cost_raw, vocabulary)
            {
                program.graveyard_self_cast = Some(
                    spellwake_engine::program::GraveyardSelfCast::WithExtraCost(extra),
                );
                return Ok(());
            }
            if self_gy_filter_ok
                && condition.is_none()
                && let Some(after_by) = p_low
                    .strip_prefix("from your graveyard by ")
                    .or_else(|| p_low.strip_prefix("you may cast ~ from your graveyard by "))
                    .or_else(|| {
                        p_low.strip_prefix("you may cast this card from your graveyard by ")
                    })
                && let Some(cost_raw) = after_by
                    .strip_suffix(" rather than paying its mana cost")
                    .or_else(|| after_by.strip_suffix(" rather than paying ~'s mana cost"))
                && program.graveyard_self_cast.is_none()
                && !program.has_type(CardType::Land)
                && let Some((mana, extra)) =
                    parse_gerund_alternative_cast_cost(cost_raw.trim(), vocabulary)
            {
                program.graveyard_self_cast = Some(
                    spellwake_engine::program::GraveyardSelfCast::WithAlternativeCost {
                        mana,
                        extra,
                    },
                );
                return Ok(());
            }
            let self_gy_inline_cond_raw: Option<&str> = p_low
                .strip_prefix("you may cast ~ from your graveyard ")
                .or_else(|| p_low.strip_prefix("you may cast this card from your graveyard "))
                .or_else(|| {
                    self_gy_filter_ok
                        .then(|| p_low.strip_prefix("from your graveyard "))
                        .flatten()
                })
                .and_then(|tail| {
                    tail.strip_prefix("if ")
                        .or_else(|| tail.strip_prefix("as long as "))
                })
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let is_self_gy_cast = matches!(
                p_low.as_str(),
                "cast_self_from_graveyard"
                    | "you may cast ~ from your graveyard"
                    | "you may cast this card from your graveyard"
            ) || (p_low == "from your graveyard" && self_gy_filter_ok)
                || self_gy_inline_cond_raw.is_some();
            if is_self_gy_cast {
                let inline_cond = match self_gy_inline_cond_raw {
                    Some(raw) => {
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        match spellwake_oracle_compiler::filter::parse_condition(
                            raw,
                            &compiler_vocab,
                        ) {
                            Ok(parsed) => Some(parsed),
                            Err(_) => return Err("oracle:unsupported-cast-permission"),
                        }
                    }
                    None => None,
                };
                let combined_cond = match (condition.as_deref(), inline_cond) {
                    (Some(outer), Some(inner)) => {
                        Some(ConditionAst::And(Box::new(outer.clone()), Box::new(inner)))
                    }
                    (Some(outer), None) => Some(outer.clone()),
                    (None, Some(inner)) => Some(inner),
                    (None, None) => None,
                };
                if let Some(cond) = combined_cond {
                    let mut tmp_builder = BodyBuilder::new();
                    if let Ok(rc) =
                        lower_resolution_condition(&cond, &mut tmp_builder, vocabulary, false, None)
                        && tmp_builder.target == BodyTarget::None
                        && rc.is_world_evaluable()
                        && rc.validate_world(vocabulary).is_ok()
                    {
                        return push_static_ability(
                            program,
                            StaticContinuousAbility::CastPermission {
                                id: ids.static_id(),
                                permission: CastPermissionRule::GraveyardSelfConditional {
                                    condition: Box::new(rc),
                                },
                            },
                        );
                    }
                } else if program.graveyard_self_cast.is_none()
                    && !program.has_type(CardType::Land)
                    && program.printed_mana_cost().is_some()
                {
                    program.graveyard_self_cast =
                        Some(spellwake_engine::program::GraveyardSelfCast::Unconditional);
                    return Ok(());
                }
            }
            if let Some((permission, rem_cond)) = try_lower_graveyard_matching_permission_ext(
                &p_low,
                *cast_only,
                filter,
                false,
                condition.as_deref(),
                vocabulary,
            ) {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission,
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            if let Some((permission, rem_cond)) = try_lower_exile_matching_permission_ext(
                &p_low,
                *cast_only,
                filter,
                false,
                condition.as_deref(),
                vocabulary,
            ) {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission,
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            Err("oracle:unsupported-cast-permission")
        }
        StaticAbilityAst::StrivePerExtraTarget { delta } => {
            if program.has_type(CardType::Land) {
                return Err("oracle:unsupported-strive-cost");
            }
            let extra_cost =
                parse_fixed_mana_cost(delta.trim()).ok_or("oracle:unsupported-strive-cost")?;
            if !extra_cost.total().is_positive() {
                return Err("oracle:unsupported-strive-cost");
            }
            if program.per_extra_target_cost.replace(extra_cost).is_some() {
                return Err("oracle:duplicate-additional-cost");
            }
            Ok(())
        }
        StaticAbilityAst::AdditionalCastCost { cost } => match cost {
            CostAtomAst::Sacrifice {
                count,
                filter,
                up_to: false,
                min_one: false,
            } if matches!(count.as_ref(), ValueExprAst::Const(1)) => {
                if program.sacrifice_creature_cast_cost {
                    let extra = lower_extra_cost_atom(cost, vocabulary)
                        .map_err(|_| "oracle:duplicate-additional-cost")?;
                    program.extra_cast_costs.push(extra);
                    return Ok(());
                }
                let is_plain_creature = filter.any_types == AstCardTypeMask::CREATURE
                    && filter_only_has(
                        filter,
                        FilterAllow {
                            types: true,
                            controller: true,
                            ..FilterAllow::NONE
                        },
                    )
                    && matches!(filter.controller, None | Some(ControllerRelation::You));
                if !is_plain_creature {
                    let obj_filter = lower_object_filter(filter, vocabulary)?;
                    program.sacrifice_cast_cost_filter = Some(obj_filter);
                }
                program.sacrifice_creature_cast_cost = true;
                Ok(())
            }
            CostAtomAst::Discard {
                count,
                filter,
                random: false,
                up_to: false,
                any_number: false,
            } if matches!(count.as_ref(), ValueExprAst::Const(1)) => {
                if program.discard_cast_cost {
                    let extra = lower_extra_cost_atom(cost, vocabulary)
                        .map_err(|_| "oracle:duplicate-additional-cost")?;
                    program.extra_cast_costs.push(extra);
                    return Ok(());
                }
                if !is_unrestricted_filter(filter) {
                    let obj_filter = lower_object_filter(filter, vocabulary)?;
                    program.discard_cast_cost_filter = Some(obj_filter);
                }
                program.discard_cast_cost = true;
                Ok(())
            }
            CostAtomAst::PayLife(amount) => {
                let expr = lower_value_expr(amount, vocabulary)?;
                if program.additional_life_cost.replace(expr).is_some() {
                    return Err("oracle:duplicate-additional-cost");
                }
                Ok(())
            }
            CostAtomAst::Exile {
                count,
                filter,
                face_down: false,
                face_up: false,
                single_graveyard: false,
                any_number: false,
                min_one: false,
                total_mv: None,
            } if filter.zone == Some(ZoneFilter::YourGraveyard) => match count.as_ref() {
                ValueExprAst::Const(n)
                    if *n > 0
                        && filter.any_types == AstCardTypeMask::CREATURE
                        && filter_only_has(
                            filter,
                            FilterAllow {
                                types: true,
                                zone: true,
                                ..FilterAllow::NONE
                            },
                        ) =>
                {
                    let cnt =
                        u8::try_from(*n).map_err(|_| "oracle:unsupported-additional-cast-cost")?;
                    if program.graveyard_creature_exile_cost != 0 {
                        return Err("oracle:duplicate-additional-cost");
                    }
                    program.graveyard_creature_exile_cost = cnt;
                    Ok(())
                }
                ValueExprAst::ChosenX => {
                    let mut inner = filter.as_ref().clone();
                    inner.zone = None;
                    if !is_unrestricted_filter(&inner) {
                        let extra = lower_extra_cost_atom(cost, vocabulary)
                            .map_err(|_| "oracle:unsupported-additional-cast-cost")?;
                        if !is_valid_cast_extra_cost(&extra) {
                            return Err("oracle:unsupported-additional-cast-cost");
                        }
                        program.extra_cast_costs.push(extra);
                        return Ok(());
                    }
                    if program.graveyard_exile_x_cost {
                        return Err("oracle:duplicate-additional-cost");
                    }
                    program.graveyard_exile_x_cost = true;
                    Ok(())
                }
                _ => {
                    let extra = lower_extra_cost_atom(cost, vocabulary)
                        .map_err(|_| "oracle:unsupported-additional-cast-cost")?;
                    if !is_valid_cast_extra_cost(&extra) {
                        return Err("oracle:unsupported-additional-cast-cost");
                    }
                    program.extra_cast_costs.push(extra);
                    Ok(())
                }
            },
            CostAtomAst::TapPermanent {
                count: _,
                filter,
                any_number: true,
            } if is_spell
                && !program.tap_creatures_cast_cost
                && !program.sacrifice_creature_cast_cost
                && !program.reveal_colorless_creature_cast_cost
                && filter.tapped == Some(false)
                && filter.any_types == AstCardTypeMask::CREATURE
                && filter.controller == Some(ControllerRelation::You) =>
            {
                let mut cleaned = filter.as_ref().clone();
                cleaned.tapped = None;
                if filter_only_has(
                    &cleaned,
                    FilterAllow {
                        types: true,
                        controller: true,
                        ..FilterAllow::NONE
                    },
                ) {
                    program.tap_creatures_cast_cost = true;
                    Ok(())
                } else {
                    Err("oracle:unsupported-additional-cast-cost")
                }
            }
            _ => {
                let extra = lower_extra_cost_atom(cost, vocabulary)
                    .map_err(|_| "oracle:unsupported-additional-cast-cost")?;
                if !is_valid_cast_extra_cost(&extra) {
                    return Err("oracle:unsupported-additional-cast-cost");
                }
                program.extra_cast_costs.push(extra);
                Ok(())
            }
        },
        StaticAbilityAst::PreventCounters {
            subject,
            counter_kind,
            condition,
        } => {
            let kind = match counter_kind.as_deref() {
                None => None,
                Some(raw) => {
                    let clean = raw
                        .trim()
                        .trim_end_matches(" counters")
                        .trim_end_matches(" counter")
                        .trim();
                    let k = parse_counter_kind(clean).ok_or("oracle:unsupported-counter-kind")?;
                    Some(k)
                }
            };
            match subject.as_ref() {
                SubjectRefAst::Player(p) => {
                    let rel = match p {
                        PlayerRefAst::You => PlayerRelation::Controller,
                        PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent => {
                            PlayerRelation::Opponent
                        }
                        PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer => {
                            PlayerRelation::AnyParticipant
                        }
                        _ => return Err("oracle:unsupported-static"),
                    };
                    push_conditional_static(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::PreventCounters {
                                selector: None,
                                players: Some(rel),
                                counter_kind: kind,
                            },
                        },
                        condition.as_deref(),
                        vocabulary,
                    )
                }
                _ => {
                    let selectors = lower_static_permanent_selectors(subject, vocabulary)?;
                    for sel in selectors {
                        push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::PreventCounters {
                                    selector: Some(Box::new(sel)),
                                    players: None,
                                    counter_kind: kind,
                                },
                            },
                            condition.as_deref(),
                            vocabulary,
                        )?;
                    }
                    Ok(())
                }
            }
        }
        StaticAbilityAst::RuleOrCombatRestriction {
            subject,
            rule,
            filter,
            condition,
        } => {
            if !is_spell
                && subject.is_none()
                && filter.is_none()
                && rule
                    .trim()
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case("all lands lose all abilities except mana abilities")
            {
                let mut sel = PermanentSelector {
                    required_types: BTreeSet::from([CardType::Land]),
                    controller: ControllerSelector::Any,
                    ..PermanentSelector::default()
                };
                if let Some(cond) = condition {
                    apply_static_selector_condition(&mut sel, cond, vocabulary)?;
                }
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: sel,
                        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::remove(
                            AbilityRemoval::NonMana,
                        ))]),
                    },
                );
            }
            if rule == "CaseToSolve" {
                let cond = condition
                    .as_deref()
                    .ok_or("oracle:case-missing-to-solve-condition")?;
                let tc = lower_trigger_condition(cond, vocabulary)?;
                let full_cond = TriggerCondition::And(
                    Box::new(TriggerCondition::SourceIsUnsolved),
                    Box::new(tc.clone()),
                );
                let guard = InterveningCondition::from_trigger_condition(&tc);
                let mut body = ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::MutateObject(Box::new(
                        spellwake_engine::toolbox::MutateObjectSpec::solve_case(),
                    ))],
                );
                if let Some(g) = guard {
                    prepend_instruction_at(
                        &mut body.instructions,
                        0,
                        Instruction::MutatePlayer(Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::require_condition(g),
                        )),
                    );
                }
                program.triggered.push(TriggeredAbility {
                    id: ids.triggered(),
                    grouping: TriggerGrouping::EachOccurrence,
                    condition: TriggerSpec::step_began(Step::End, PlayerRelation::Controller)
                        .only_if(full_cond),
                    body,
                });
                return Ok(());
            }
            if rule.eq_ignore_ascii_case("cantbecountered")
                || rule.eq_ignore_ascii_case("can't be countered")
                || rule.eq_ignore_ascii_case("can't be countered by spells or abilities")
            {
                let spell_filter = filter.as_deref().or(match subject.as_deref() {
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f),
                    ) => Some(f),
                    _ => None,
                });
                let is_self_spell = match subject.as_deref() {
                    None | Some(SubjectRefAst::SelfObject) => true,
                    Some(SubjectRefAst::Anaphora(raw)) => {
                        raw.trim().eq_ignore_ascii_case("this spell")
                    }
                    _ => false,
                };
                match (subject.as_deref(), spell_filter) {
                    (_, None) if is_self_spell && condition.is_none() => {
                        program.cannot_be_countered = true;
                        return Ok(());
                    }
                    (_, Some(f))
                        if f.controller == Some(ControllerRelation::You)
                            && f.subtypes.is_empty()
                            && f.any_colors.is_empty()
                            && f.forbidden_colors.is_empty()
                            && f.color_arity.is_none()
                            && f.stat_bounds.is_empty()
                            && f.flags.is_empty()
                            && f.predicates.is_empty()
                            && f.zone.is_none() =>
                    {
                        let mut types = if f.any_types.is_empty() {
                            BTreeSet::from([
                                CardType::Creature,
                                CardType::Instant,
                                CardType::Sorcery,
                                CardType::Artifact,
                                CardType::Enchantment,
                                CardType::Planeswalker,
                                CardType::Battle,
                            ])
                        } else {
                            card_type_mask_to_set(f.any_types)?
                        };
                        if !f.forbidden_types.is_empty() {
                            let forbidden = card_type_mask_to_set(f.forbidden_types)?;
                            for ft in forbidden {
                                types.remove(&ft);
                            }
                        }
                        if !types.is_empty() {
                            return push_conditional_static(
                                program,
                                StaticContinuousAbility::CastPermission {
                                    id: ids.static_id(),
                                    permission:
                                        CastPermissionRule::ControllerSpellsCannotBeCountered(types),
                                },
                                condition.as_deref(),
                                vocabulary,
                            );
                        }
                    }
                    (_, Some(f)) if f.controller == Some(ControllerRelation::You) => {
                        let mut stripped = f.clone();
                        stripped.controller = None;
                        if let Ok(sf) = lower_spell_filter(&stripped, vocabulary) {
                            return push_conditional_static(
                                program,
                                StaticContinuousAbility::CastPermission {
                                    id: ids.static_id(),
                                    permission:
                                        CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(
                                            Box::new(sf),
                                        ),
                                },
                                condition.as_deref(),
                                vocabulary,
                            );
                        }
                    }
                    (_, Some(f))
                        if !is_spell
                            && f.controller.is_none()
                            && f.owner.is_none()
                            && f.zone.is_none()
                            && (f.any_spell || !f.any_permanent) =>
                    {
                        if let Ok(sf) = lower_spell_filter(f, vocabulary)
                            && spell_filter_supported_by_matches_characteristics(&sf)
                        {
                            return push_conditional_static(
                                program,
                                StaticContinuousAbility::CastPermission {
                                    id: ids.static_id(),
                                    permission:
                                        CastPermissionRule::AllSpellsCannotBeCounteredFiltered(
                                            Box::new(sf),
                                        ),
                                },
                                condition.as_deref(),
                                vocabulary,
                            );
                        }
                    }
                    _ => {}
                }
            }
            if condition.is_none() && rule == "look_top_library_any_time" && filter.is_none() {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::SeeLibraryTop { public: false },
                    },
                );
            }
            if condition.is_none() && rule == "EntersTappedAndDoesntUntap" && filter.is_none() {
                let ProgramKind::Permanent(def) = &mut program.kind else {
                    return Err("oracle:unsupported-rule-restriction");
                };
                def.entry_tap = Some(EntryTap::Always);
                return push_static_ability(
                    program,
                    StaticContinuousAbility::UntapRule {
                        id: ids.static_id(),
                        rule: StaticUntapRule::SourceSkipsUntap,
                    },
                );
            }
            if condition.is_none() && rule == "OpeningHandBeginOnBattlefield" && filter.is_none() {
                if !matches!(program.kind, ProgramKind::Permanent(_)) {
                    return Err("oracle:unsupported-rule-restriction");
                }
                program.opening_hand_battlefield = true;
                return Ok(());
            }
            if condition.is_none() && rule == "FlashSacrificeAtCleanup" && filter.is_none() {
                if !matches!(program.kind, ProgramKind::Permanent(_)) {
                    return Err("oracle:unsupported-rule-restriction");
                }
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::FlashSacrificeAtCleanup,
                    },
                );
            }
            if condition.is_none()
                && matches!(
                    rule.trim().to_ascii_lowercase().as_str(),
                    "entersuntapped"
                        | "enters_untapped"
                        | "enters untapped"
                        | "enter untapped"
                        | "enter the battlefield untapped"
                )
                && filter.is_none()
                && let Some(subj) = subject.as_deref()
                && let Ok(selectors) = lower_static_permanent_selectors(subj, vocabulary)
                && !selectors.is_empty()
            {
                for sel in selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::EnterUntapped(Box::new(sel)),
                        },
                    )?;
                }
                return Ok(());
            }
            if condition.is_none()
                && rule == "CantEnterBattlefield"
                && filter.is_none()
                && let Some(subj) = subject.as_deref()
                && let Ok(selectors) = lower_static_permanent_selectors(subj, vocabulary)
                && !selectors.is_empty()
            {
                for sel in selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ProhibitEntering(Box::new(sel)),
                        },
                    )?;
                }
                return Ok(());
            }
            if condition.is_none()
                && filter.is_none()
                && rule
                    .to_ascii_lowercase()
                    .starts_with("you don't lose unspent ")
                && rule
                    .to_ascii_lowercase()
                    .contains(" mana as steps and phases end")
            {
                let low = rule.to_ascii_lowercase();
                let mt = if low.contains("white") {
                    Some(ManaType::White)
                } else if low.contains("blue") {
                    Some(ManaType::Blue)
                } else if low.contains("black") {
                    Some(ManaType::Black)
                } else if low.contains("red") {
                    Some(ManaType::Red)
                } else if low.contains("green") {
                    Some(ManaType::Green)
                } else if low.contains("colorless") {
                    Some(ManaType::Colourless)
                } else {
                    None
                };
                if let Some(mt) = mt {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::Modifier {
                            id: ids.static_id(),
                            rule: StaticModifierRule::RetainMana(mt),
                        },
                    );
                }
            }
            if condition.is_none()
                && rule.eq_ignore_ascii_case("you may choose not to untap ~ during your untap step")
                && filter.is_none()
                && matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject))
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::UntapRule {
                        id: ids.static_id(),
                        rule: StaticUntapRule::MayChooseNotToUntapSource,
                    },
                );
            }
            if filter.is_none()
                && let Some(lock) = parse_static_ability_lock(rule, condition.is_some())
            {
                let mut selectors = match subject.as_deref() {
                    Some(SubjectRefAst::Anaphora(pron))
                        if pron.eq_ignore_ascii_case("it")
                            && lock.singular_attached_fallback
                            && is_permanent_attachment_program(program) =>
                    {
                        vec![PermanentSelector {
                            attached_to_source: true,
                            ..PermanentSelector::default()
                        }]
                    }
                    None if lock.singular_attached_fallback
                        && is_permanent_attachment_program(program) =>
                    {
                        vec![PermanentSelector {
                            attached_to_source: true,
                            ..PermanentSelector::default()
                        }]
                    }
                    None if lock.allow_none_as_self => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                    Some(subj) => lower_static_permanent_selectors_with_context(
                        subj,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    _ => return Err("oracle:unsupported-rule-restriction"),
                };
                if lock.requires_source_controller
                    && !selectors
                        .iter()
                        .all(|sel| selector_guaranteed_source_controller(sel, program))
                {
                    return Err("oracle:unsupported-untap-rule");
                }
                for sel in &mut selectors {
                    if let Some(cond) = condition {
                        apply_static_selector_condition(sel, cond, vocabulary)?;
                    }
                }
                let mut parts = Vec::new();
                if lock.cannot_attack_or_block {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::CannotAttackOrBlock,
                    ));
                }
                if lock.cannot_crew_vehicles {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::CannotCrewVehicles,
                    ));
                }
                if lock.suppressed_activations.is_some() || lock.doesnt_untap {
                    parts.push(StaticPart::Abilities(AbilityChange {
                        suppressed_activations: lock.suppressed_activations,
                        doesnt_untap: lock.doesnt_untap,
                        ..AbilityChange::default()
                    }));
                }
                for selector in selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(parts.clone()),
                        },
                    )?;
                }
                return Ok(());
            }
            if condition.is_none() && rule == "UntapEachOtherUntapStep" && filter.is_none() {
                match subject.as_deref() {
                    None | Some(SubjectRefAst::SelfObject) => {
                        return push_static_ability(
                            program,
                            StaticContinuousAbility::UntapRule {
                                id: ids.static_id(),
                                rule: StaticUntapRule::UntapDuringOtherPlayersUntap,
                            },
                        );
                    }
                    Some(subj) => {
                        let selectors = lower_static_permanent_selectors(subj, vocabulary)?;
                        for sel in selectors {
                            push_static_ability(
                                program,
                                StaticContinuousAbility::UntapRule {
                                    id: ids.static_id(),
                                    rule: StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(
                                        Box::new(sel),
                                    ),
                                },
                            )?;
                        }
                        return Ok(());
                    }
                }
            }
            if condition.is_none()
                && rule == "CantAttackYouOrBlockYourCreatures"
                && filter.is_none()
            {
                push_attack_requirement(
                    program,
                    AttackRequirement::EnchantedCannotAttackOrBlockController,
                );
                return Ok(());
            }
            if rule.eq_ignore_ascii_case("can't block and can't be blocked") && filter.is_none() {
                let mut selector = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selector(s, vocabulary)?,
                    None => PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    },
                };
                if let Some(cond) = condition {
                    apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                }
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotBlock,
                        )]),
                    },
                )?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotBeBlocked,
                        )]),
                    },
                );
            }
            if rule.eq_ignore_ascii_case("can't attack or block alone") && filter.is_none() {
                let mut selector = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selector(s, vocabulary)?,
                    None => PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    },
                };
                if let Some(cond) = condition {
                    apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                }
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotAttackAlone,
                        )]),
                    },
                )?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotBlockAlone,
                        )]),
                    },
                );
            }
            if filter.is_none()
                && matches!(
                    rule.as_str(),
                    "CantAttackUnless" | "CantAttackOrBlockUnless" | "CantBlockUnless"
                )
                && let Some(cond) = condition.as_deref()
            {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                if rule == "CantAttackUnless"
                    && let ConditionAst::ControlsCount {
                        player: PlayerRefAst::DefendingPlayer,
                        filter: def_filter,
                        op: CompareOp::Ge,
                        value: 1,
                    } = cond
                {
                    let mut clean_f = def_filter.as_ref().clone();
                    clean_f.controller = None;
                    let obj_filter = lower_object_filter(&clean_f, vocabulary)?;
                    for selector in selectors {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::Battlefield {
                                id: ids.static_id(),
                                selector,
                                parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                    BlockingRestriction::CannotAttackUnlessDefenderControls(
                                        obj_filter,
                                    ),
                                )]),
                            },
                        )?;
                    }
                    return Ok(());
                }
                let neg_cond = ConditionAst::Not(Box::new(cond.clone()));
                let br = match rule.as_str() {
                    "CantAttackUnless" => BlockingRestriction::CannotAttack,
                    "CantAttackOrBlockUnless" => BlockingRestriction::CannotAttackOrBlock,
                    "CantBlockUnless" => BlockingRestriction::CannotBlock,
                    _ => unreachable!(),
                };
                for mut selector in selectors {
                    apply_static_selector_condition(&mut selector, &neg_cond, vocabulary)?;
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![StaticPart::BlockingRestriction(br.clone())]),
                        },
                    )?;
                }
                return Ok(());
            }
            if let Some(obj) =
                parse_cant_block_or_be_blocked_by_filter(rule, filter.as_deref(), vocabulary)
            {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for mut selector in selectors {
                    if let Some(cond) = condition {
                        apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![
                                StaticPart::BlockingRestriction(
                                    BlockingRestriction::CannotBlockMatching(obj),
                                ),
                                StaticPart::BlockingRestriction(
                                    BlockingRestriction::CannotBeBlockedByMatching(obj),
                                ),
                            ]),
                        },
                    )?;
                }
                return Ok(());
            }
            if filter.is_none() && is_menace_blocking_rule(rule) {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                let menace_change = AbilityChange {
                    granted_keywords: BTreeSet::from([KeywordAbility::Menace]),
                    ..AbilityChange::default()
                };
                for mut selector in selectors {
                    if let Some(cond) = condition {
                        apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![StaticPart::Abilities(menace_change.clone())]),
                        },
                    )?;
                }
                return Ok(());
            }
            if filter.is_none()
                && let Some(extra_blocks) = parse_block_additional_count(rule)
                && extra_blocks >= 2
            {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                let parts_vec =
                    vec![
                        StaticPart::BlockingRestriction(BlockingRestriction::AdditionalBlock);
                        usize::from(extra_blocks)
                    ];
                for mut selector in selectors {
                    if let Some(cond) = condition {
                        apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(parts_vec.clone()),
                        },
                    )?;
                }
                return Ok(());
            }
            if let Some(br) = lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary) {
                let selectors = match subject.as_deref() {
                    Some(SubjectRefAst::Anaphora(pron))
                        if pron.eq_ignore_ascii_case("it")
                            && matches!(
                                br,
                                BlockingRestriction::BlockerPowerAtLeastSource
                                    | BlockingRestriction::CannotBeBlockedByGreaterPower
                            ) =>
                    {
                        vec![PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            ..PermanentSelector::default()
                        }]
                    }
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for mut selector in selectors {
                    if let Some(cond) = condition {
                        if br == BlockingRestriction::CannotAttack
                            && let ConditionAst::Not(inner) = cond.as_ref()
                            && let ConditionAst::ControlsCount {
                                player: PlayerRefAst::DefendingPlayer,
                                filter: def_filter,
                                op: CompareOp::Ge,
                                value: 1,
                            } = inner.as_ref()
                        {
                            let mut clean_f = def_filter.as_ref().clone();
                            clean_f.controller = None;
                            let obj_filter = lower_object_filter(&clean_f, vocabulary)?;
                            push_static_ability(
                                program,
                                StaticContinuousAbility::Battlefield {
                                    id: ids.static_id(),
                                    selector,
                                    parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                        BlockingRestriction::CannotAttackUnlessDefenderControls(
                                            obj_filter,
                                        ),
                                    )]),
                                },
                            )?;
                            continue;
                        }
                        apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![StaticPart::BlockingRestriction(br.clone())]),
                        },
                    )?;
                }
                return Ok(());
            }
            if rule == "ActivatedAbilitiesCantBeActivated"
                && (subject.is_none() || filter.is_none())
            {
                let selectors = if let Some(flt) = filter.as_deref() {
                    vec![lower_static_filter_to_permanent_selector(flt, vocabulary)?]
                } else if let Some(subj) = subject.as_deref() {
                    lower_static_permanent_selectors(subj, vocabulary)?
                } else {
                    Vec::new()
                };
                if !selectors.is_empty() {
                    for selector in selectors {
                        push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::PreventMatchingActivations(Box::new(
                                    selector,
                                )),
                            },
                            condition.as_deref(),
                            vocabulary,
                        )?;
                    }
                    return Ok(());
                }
            }
            if rule == "NonManaActivatedAbilitiesCantBeActivated"
                && (subject.is_none() || filter.is_none())
            {
                let selectors = if let Some(flt) = filter.as_deref() {
                    vec![lower_static_filter_to_permanent_selector(flt, vocabulary)?]
                } else if let Some(subj) = subject.as_deref() {
                    lower_static_permanent_selectors(subj, vocabulary)?
                } else {
                    Vec::new()
                };
                if !selectors.is_empty() {
                    for selector in selectors {
                        push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::PreventMatchingNonManaActivations(
                                    Box::new(selector),
                                ),
                            },
                            condition.as_deref(),
                            vocabulary,
                        )?;
                    }
                    return Ok(());
                }
            }
            if rule == "ActivateAbilitiesAsThoughHaste"
                && (subject.is_none() || filter.is_none())
                && let Some(flt) = filter.as_deref().or(match subject.as_deref() {
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f),
                    ) => Some(f),
                    _ => None,
                })
                && flt.controller == Some(ControllerRelation::You)
            {
                let selector = lower_static_filter_to_permanent_selector(flt, vocabulary)?;
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ActivateAbilitiesAsThoughHaste(Box::new(selector)),
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if rule.eq_ignore_ascii_case("entertriggerssuppressed") {
                let creature_only_or_none = filter.as_deref().is_none_or(|f| {
                    f.any_types == AstCardTypeMask::CREATURE
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                ..FilterAllow::NONE
                            },
                        )
                });
                if creature_only_or_none {
                    return push_conditional_static(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::SuppressEntersTriggers,
                        },
                        condition.as_deref(),
                        vocabulary,
                    );
                }
            }
            if rule.eq_ignore_ascii_case("legendruleexempt") {
                let legend_filter = filter.as_deref().or(match subject.as_deref() {
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Group(f) | SubjectRefAst::Each(f),
                    ) => Some(f.as_ref()),
                    _ => None,
                });
                let rule_opt = match (subject.as_deref(), legend_filter) {
                    (None, None) => Some(StaticModifierRule::IgnoreLegendRule),
                    (Some(SubjectRefAst::Player(PlayerRefAst::You)), None) => {
                        Some(StaticModifierRule::IgnoreLegendRuleForController)
                    }
                    (
                        Some(SubjectRefAst::Player(
                            PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer,
                        )),
                        None,
                    ) => Some(StaticModifierRule::IgnoreLegendRule),
                    (_, Some(f))
                        if f.controller == Some(ControllerRelation::You)
                            && f.subtypes.is_empty()
                            && f.forbidden_types.is_empty()
                            && f.any_types.is_empty()
                            && f.any_colors.is_empty()
                            && f.forbidden_colors.is_empty()
                            && f.stat_bounds.is_empty()
                            && f.flags.is_empty()
                            && f.token.is_none() =>
                    {
                        Some(StaticModifierRule::IgnoreLegendRuleForController)
                    }
                    (_, Some(f))
                        if f.controller.is_none()
                            && f.subtypes.is_empty()
                            && f.forbidden_types.is_empty()
                            && f.any_types.is_empty()
                            && f.any_colors.is_empty()
                            && f.forbidden_colors.is_empty()
                            && f.stat_bounds.is_empty()
                            && f.flags.is_empty()
                            && f.token.is_none() =>
                    {
                        Some(StaticModifierRule::IgnoreLegendRule)
                    }
                    (_, Some(f))
                        if f.subtypes.len() == 1
                            && f.controller.is_none()
                            && f.forbidden_types.is_empty()
                            && f.any_colors.is_empty()
                            && f.forbidden_colors.is_empty()
                            && f.stat_bounds.is_empty()
                            && f.flags.is_empty()
                            && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0]) =>
                    {
                        Some(StaticModifierRule::IgnoreLegendForSubtype(ct))
                    }
                    _ => None,
                };
                if let Some(mod_rule) = rule_opt {
                    return push_conditional_static(
                        program,
                        StaticContinuousAbility::Modifier {
                            id: ids.static_id(),
                            rule: mod_rule,
                        },
                        condition.as_deref(),
                        vocabulary,
                    );
                }
            }
            if rule != "CantCast:~"
                && (rule.starts_with("CantCast:")
                    || rule == "CantPlay"
                    || rule == "CantCastOrPlay"
                    || rule == "CantBeCast")
                && let Some(flt) = filter.as_deref()
            {
                let player_rel = if rule == "CantBeCast" && subject.is_none() {
                    Some(PlayerRelation::AnyParticipant)
                } else if let Some(SubjectRefAst::Player(p)) = subject.as_deref() {
                    lower_player_relation(*p).ok()
                } else {
                    None
                };
                if let Some(player) = player_rel {
                    let includes_lands_rule =
                        matches!(rule.as_str(), "CantPlay" | "CantCastOrPlay");
                    if flt.any_card && is_unrestricted_filter(flt) {
                        if includes_lands_rule {
                            push_conditional_static(
                                program,
                                StaticContinuousAbility::Restriction {
                                    id: ids.static_id(),
                                    rule: StaticRestriction::ProhibitCasting {
                                        player,
                                        filter: Box::new(SpellFilter::any()),
                                    },
                                },
                                condition.as_deref(),
                                vocabulary,
                            )?;
                            return push_conditional_static(
                                program,
                                StaticContinuousAbility::Restriction {
                                    id: ids.static_id(),
                                    rule: StaticRestriction::CantPlayLands(player),
                                },
                                condition.as_deref(),
                                vocabulary,
                            );
                        }
                        return push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::ProhibitCasting {
                                    player,
                                    filter: Box::new(SpellFilter::any()),
                                },
                            },
                            condition.as_deref(),
                            vocabulary,
                        );
                    }
                    if includes_lands_rule
                        && !flt.any_card
                        && flt.any_types == AstCardTypeMask::LAND
                        && filter_only_has(
                            flt,
                            FilterAllow {
                                types: true,
                                ..FilterAllow::NONE
                            },
                        )
                    {
                        return push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::CantPlayLands(player),
                            },
                            condition.as_deref(),
                            vocabulary,
                        );
                    }
                    let nonland_spell_ok = (flt.any_types.0 & AstCardTypeMask::LAND.0) == 0
                        && (!includes_lands_rule
                            || (!flt.any_card
                                && (flt.any_spell
                                    || !flt.any_types.is_empty()
                                    || (flt.forbidden_types.0 & AstCardTypeMask::LAND.0) != 0)));
                    if nonland_spell_ok
                        && let Ok(sf) = lower_spell_filter(flt, vocabulary)
                        && spell_filter_supported_by_prohibit_casting(&sf)
                    {
                        return push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::ProhibitCasting {
                                    player,
                                    filter: Box::new(sf),
                                },
                            },
                            condition.as_deref(),
                            vocabulary,
                        );
                    }
                }
            }
            let remove_type_opt = match rule.trim().to_ascii_lowercase().as_str() {
                "isn't a creature" | "aren't creatures" => Some(CardType::Creature),
                "isn't an artifact" | "aren't artifacts" => Some(CardType::Artifact),
                "isn't an enchantment" | "aren't enchantments" => Some(CardType::Enchantment),
                "isn't a land" | "aren't lands" => Some(CardType::Land),
                _ => None,
            };
            if let Some(rem_ty) = remove_type_opt
                && filter.is_none()
            {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for mut selector in selectors {
                    if let Some(cond) = condition {
                        apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                                types: TypeOperation::Remove(BTreeSet::from([rem_ty])),
                                supertypes: TypeOperation::Keep,
                                subtypes: SubtypeOperation::Keep,
                                land_subtypes: SubtypeOperation::Keep,
                            })]),
                        },
                    )?;
                }
                return Ok(());
            }
            let rule_low_trimmed = rule.trim().to_ascii_lowercase();
            if let Some(target_src) = rule_low_trimmed.strip_prefix("cantbetargetof:") {
                let ts = target_src.trim().trim_end_matches('.').trim();
                let ts_nodur = ts.strip_suffix(" this turn").unwrap_or(ts).trim();
                if ts == "spells or abilities"
                    && let Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f),
                    ) = subject.as_deref()
                    && matches!(
                        f.zone,
                        Some(ZoneFilter::AllGraveyards | ZoneFilter::AnyGraveyard)
                    )
                    && !f.any_permanent
                    && !f.any_spell
                    && filter_only_has(
                        f,
                        FilterAllow {
                            zone: true,
                            ..FilterAllow::NONE
                        },
                    )
                {
                    return push_conditional_static(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::PreventGraveyardTargeting,
                        },
                        condition.as_deref(),
                        vocabulary,
                    );
                }
                if matches!(
                    subject.as_deref(),
                    Some(SubjectRefAst::Player(PlayerRefAst::You))
                ) {
                    if ts == "spells or abilities your opponents control" {
                        return push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::ControllerHasHexproof,
                            },
                            condition.as_deref(),
                            vocabulary,
                        );
                    }
                    if ts == "spells or abilities" {
                        return push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::ControllerHasShroud,
                            },
                            condition.as_deref(),
                            vocabulary,
                        );
                    }
                }
                if ts == "aura spells" {
                    let selectors = match subject.as_deref() {
                        Some(s) => lower_static_permanent_selectors(s, vocabulary)?,
                        None => vec![PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            ..PermanentSelector::default()
                        }],
                    };
                    for selector in selectors {
                        push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::CantBeTargetedBy {
                                    selector: Box::new(selector),
                                    auras_only: true,
                                    colors: ColorMask::EMPTY,
                                },
                            },
                            condition.as_deref(),
                            vocabulary,
                        )?;
                    }
                    return Ok(());
                }
                if let Some(colors) = parse_cant_be_targeted_by_color_mask(ts) {
                    let selectors = match subject.as_deref() {
                        Some(s) => lower_static_permanent_selectors(s, vocabulary)?,
                        None => vec![PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            ..PermanentSelector::default()
                        }],
                    };
                    for selector in selectors {
                        push_conditional_static(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::CantBeTargetedBy {
                                    selector: Box::new(selector),
                                    auras_only: false,
                                    colors,
                                },
                            },
                            condition.as_deref(),
                            vocabulary,
                        )?;
                    }
                    return Ok(());
                }
                let (granted_kws, opponents_anchored) = if ts_nodur == "spells or abilities" {
                    (Some(BTreeSet::from([KeywordAbility::Shroud])), false)
                } else if ts_nodur == "spells or abilities your opponents control" {
                    (Some(BTreeSet::from([KeywordAbility::Hexproof])), true)
                } else if let Some(qual_raw) =
                    ts_nodur.strip_suffix(" spells or abilities your opponents control")
                {
                    (
                        parse_hexproof_from_qualities(qual_raw.trim(), vocabulary).map(|quals| {
                            quals
                                .into_iter()
                                .map(KeywordAbility::HexproofFrom)
                                .collect()
                        }),
                        true,
                    )
                } else {
                    (None, false)
                };
                if let Some(kws) = granted_kws {
                    let selectors = match subject.as_deref() {
                        Some(s) => lower_static_permanent_selectors_with_context(
                            s,
                            condition.as_deref(),
                            vocabulary,
                        )?,
                        None => vec![PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            ..PermanentSelector::default()
                        }],
                    };
                    let all_source_controlled = !opponents_anchored
                        || selectors
                            .iter()
                            .all(|sel| selector_guaranteed_source_controller(sel, program));
                    if all_source_controlled {
                        for mut selector in selectors {
                            if let Some(cond) = condition {
                                apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                            }
                            push_static_ability(
                                program,
                                StaticContinuousAbility::Battlefield {
                                    id: ids.static_id(),
                                    selector,
                                    parts: StaticParts(vec![StaticPart::Abilities(
                                        AbilityChange {
                                            granted_keywords: kws.clone(),
                                            ..AbilityChange::default()
                                        },
                                    )]),
                                },
                            )?;
                        }
                        return Ok(());
                    }
                }
            }
            if filter.is_some() {
                return Err("oracle:unsupported-rule-restriction");
            }
            if matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject))
                && (matches!(rule_low_trimmed.as_str(), "castonlyif" | "cast_only_if")
                    || rule_low_trimmed.starts_with("cast ~ only ")
                    || rule_low_trimmed.starts_with("cast this spell only "))
            {
                let timing = if matches!(rule_low_trimmed.as_str(), "castonlyif" | "cast_only_if") {
                    let cond = condition
                        .as_deref()
                        .ok_or("oracle:unsupported-rule-restriction")?;
                    lower_cast_timing_condition(cond, vocabulary)
                        .ok_or("oracle:unsupported-rule-restriction")?
                } else {
                    let base = parse_cast_timing_restriction(rule, None, vocabulary);
                    match (base, condition.as_deref()) {
                        (Some(b), Some(cond)) => {
                            let c = lower_cast_timing_condition(cond, vocabulary)
                                .ok_or("oracle:unsupported-rule-restriction")?;
                            merge_cast_timing_restrictions(b, c)
                        }
                        (Some(b), None) => b,
                        (None, Some(cond)) => lower_cast_timing_condition(cond, vocabulary)
                            .ok_or("oracle:unsupported-rule-restriction")?,
                        (None, None) => return Err("oracle:unsupported-rule-restriction"),
                    }
                };
                program.cast_timing = Some(match program.cast_timing.take() {
                    Some(prev) => merge_cast_timing_restrictions(prev, timing),
                    None => timing,
                });
                return Ok(());
            }
            if matches!(
                rule_low_trimmed.as_str(),
                "loses all other abilities" | "lose all other abilities"
            ) {
                return lower_loses_all_other_abilities(
                    subject.as_deref(),
                    condition.as_deref(),
                    program,
                    vocabulary,
                    ids,
                );
            }
            if rule == "AttacksEachCombatIfAble"
                || rule_low_trimmed == "attacks each combat if able"
                || rule_low_trimmed == "attacks each turn if able"
            {
                if condition.is_none() {
                    let bare_req = match subject.as_deref() {
                        Some(SubjectRefAst::SelfObject) | None => {
                            Some(AttackRequirement::SourceAttacks)
                        }
                        Some(SubjectRefAst::AttachedHost(_)) => {
                            Some(AttackRequirement::AttachedCreatureAttacks)
                        }
                        Some(
                            SubjectRefAst::All(f)
                            | SubjectRefAst::Each(f)
                            | SubjectRefAst::Group(f),
                        ) if f.any_types == AstCardTypeMask::CREATURE
                            && filter_only_has(
                                f,
                                FilterAllow {
                                    types: true,
                                    ..FilterAllow::NONE
                                },
                            ) =>
                        {
                            Some(AttackRequirement::AllCreaturesAttack)
                        }
                        _ => None,
                    };
                    if let Some(req) = bare_req {
                        push_attack_requirement(program, req);
                        return Ok(());
                    }
                }
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for mut selector in selectors {
                    let mut cond_for_static = condition.as_deref();
                    if let Some(cond) = condition.as_deref()
                        && lower_static_source_condition(cond, vocabulary).is_none()
                    {
                        apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                        cond_for_static = None;
                    }
                    push_conditional_static(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                BlockingRestriction::MustAttackEachCombatIfAble,
                            )]),
                        },
                        cond_for_static,
                        vocabulary,
                    )?;
                }
                return Ok(());
            }
            if rule == "AttacksOrBlocksEachCombatIfAble"
                || rule_low_trimmed == "attacks or blocks each combat if able"
                || rule_low_trimmed == "attack or block each combat if able"
            {
                let bare_req = if condition.is_none() {
                    match subject.as_deref() {
                        Some(SubjectRefAst::SelfObject) | None => {
                            Some(AttackRequirement::SourceAttacks)
                        }
                        Some(SubjectRefAst::AttachedHost(_)) => {
                            Some(AttackRequirement::AttachedCreatureAttacks)
                        }
                        Some(
                            SubjectRefAst::All(f)
                            | SubjectRefAst::Each(f)
                            | SubjectRefAst::Group(f),
                        ) if f.any_types == AstCardTypeMask::CREATURE
                            && filter_only_has(
                                f,
                                FilterAllow {
                                    types: true,
                                    ..FilterAllow::NONE
                                },
                            ) =>
                        {
                            Some(AttackRequirement::AllCreaturesAttack)
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                if let Some(req) = bare_req {
                    push_attack_requirement(program, req);
                    for selector in selectors {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::Battlefield {
                                id: ids.static_id(),
                                selector,
                                parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                    BlockingRestriction::MustBlockEachCombatIfAble,
                                )]),
                            },
                        )?;
                    }
                    return Ok(());
                }
                for mut selector in selectors {
                    let mut cond_for_static = condition.as_deref();
                    if let Some(cond) = condition.as_deref()
                        && lower_static_source_condition(cond, vocabulary).is_none()
                    {
                        apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                        cond_for_static = None;
                    }
                    push_conditional_static(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![
                                StaticPart::BlockingRestriction(
                                    BlockingRestriction::MustAttackEachCombatIfAble,
                                ),
                                StaticPart::BlockingRestriction(
                                    BlockingRestriction::MustBlockEachCombatIfAble,
                                ),
                            ]),
                        },
                        cond_for_static,
                        vocabulary,
                    )?;
                }
                return Ok(());
            }
            let cond_sc = match condition.as_deref() {
                Some(cond) => Some(
                    lower_static_source_condition(cond, vocabulary)
                        .ok_or("oracle:unsupported-static-cond")?,
                ),
                None => None,
            };
            let prev_cont_id = program.continuous.as_ref().map(StaticContinuousAbility::id);
            let prev_add_len = program.additional_battlefield_abilities.len();
            let prev_cannot_be_countered = program.cannot_be_countered;
            let prev_attack_req = program.attack_requirement.is_some();
            let prev_add_attack_len = program.additional_attack_requirements.len();
            let r_low = rule.to_ascii_lowercase();
            let apply_cond_if_pushed =
                |program: &mut Program,
                 cond_sc: Option<spellwake_engine::characteristics::StaticSourceCondition>|
                 -> Result<(), &'static str> {
                    if let Some(sc) = cond_sc {
                        if program.cannot_be_countered != prev_cannot_be_countered
                            || program.attack_requirement.is_some() != prev_attack_req
                            || program.additional_attack_requirements.len() != prev_add_attack_len
                        {
                            return Err("oracle:unsupported-static");
                        }
                        let mut pushed_any = false;
                        if let Some(cur) = program.continuous.as_ref()
                            && Some(cur.id()) != prev_cont_id
                        {
                            program.static_conditions.insert(cur.id(), sc.clone());
                            pushed_any = true;
                        }
                        for ab in &program.additional_battlefield_abilities[prev_add_len..] {
                            program.static_conditions.insert(ab.id(), sc.clone());
                            pushed_any = true;
                        }
                        if !pushed_any {
                            return Err("oracle:unsupported-static");
                        }
                    }
                    Ok(())
                };
            if cond_sc.is_some() {
                let mut probe = program.clone();
                let inner = StaticAbilityAst::RuleOrCombatRestriction {
                    subject: subject.clone(),
                    rule: rule.clone(),
                    filter: None,
                    condition: None,
                };
                lower_static(&inner, &mut probe, vocabulary, ids, is_spell)?;
                apply_cond_if_pushed(&mut probe, cond_sc)?;
                *program = probe;
                return Ok(());
            }
            if r_low == "~ can be your commander"
                || r_low.ends_with(" can be your commander")
                || r_low.contains("a deck can have any number of cards named")
                || r_low.contains("a deck can have up to ")
            {
                return Ok(());
            }
            if r_low
                == "if ~ is your commander, choose a color before the game begins. ~ is the chosen color"
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CharacteristicDefining {
                        id: ids.static_id(),
                        parts: DefiningParts(vec![DefiningPart::CommanderChosenColor(None)]),
                    },
                );
            }
            if r_low == "becomesdayonenterifneither" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ids.static_id(),
                        rule: StaticModifierRule::BecomesDayIfNeither,
                    },
                );
            }
            if r_low == "blockseachcombatifable" || r_low == "blocks each combat if able" {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for mut selector in selectors {
                    if let Some(cond) = condition {
                        apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                BlockingRestriction::MustBlockEachCombatIfAble,
                            )]),
                        },
                    )?;
                }
                return Ok(());
            }
            if matches!(
                r_low.as_str(),
                "cantattackyou"
                    | "cant_attack:you"
                    | "cant_attack:you or planeswalkers you control"
                    | "cant_attack:you or a planeswalker you control"
                    | "cant_attack:you or planeswalkers you control."
                    | "can't attack you or planeswalkers you control"
                    | "can't attack you or a planeswalker you control"
            ) {
                let req = match subject.as_deref() {
                    Some(SubjectRefAst::AttachedHost(_)) => {
                        Some(AttackRequirement::AttachedCannotAttackController)
                    }
                    None | Some(SubjectRefAst::SelfObject) => {
                        Some(AttackRequirement::SourceCannotAttackController)
                    }
                    Some(SubjectRefAst::All(f) | SubjectRefAst::Each(f))
                        if f.any_types == AstCardTypeMask::CREATURE
                            && f.required_keywords.len() == 1
                            && f.required_keywords[0].eq_ignore_ascii_case("flying")
                            && filter_only_has(
                                f,
                                FilterAllow {
                                    types: true,
                                    req_keywords: true,
                                    ..FilterAllow::NONE
                                },
                            ) =>
                    {
                        Some(AttackRequirement::FlyingCannotAttackController)
                    }
                    _ => None,
                };
                if let Some(req) = req
                    && condition.is_none()
                {
                    push_attack_requirement(program, req);
                    return Ok(());
                }
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors_with_context(
                        s,
                        condition.as_deref(),
                        vocabulary,
                    )?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for mut selector in selectors {
                    if let Some(cond) = condition {
                        apply_static_selector_condition(&mut selector, cond, vocabulary)?;
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector,
                            parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                BlockingRestriction::CannotAttackSourceController,
                            )]),
                        },
                    )?;
                }
                return Ok(());
            }
            if let Some(tax_text) = r_low.strip_prefix("attacktax:").or_else(|| {
                r_low
                    .starts_with("creatures can't attack you unless their controller pays {")
                    .then_some(r_low.as_str())
            }) && tax_text.starts_with("creatures can't attack you")
                && let Some(after_pay) = tax_text.split("unless their controller pays ").nth(1)
                && let Some(mana_tok) = after_pay.split_whitespace().next()
                && after_pay.contains("for each creature they control that's attacking you")
            {
                let symbol = if mana_tok == "{w/p}" {
                    Some(spellwake_engine::program::AttackCostSymbol::WhitePhyrexian)
                } else {
                    parse_generic_mana_amount(mana_tok)
                        .filter(|&n| n > 0)
                        .map(|n| {
                            if n == 1 {
                                spellwake_engine::program::AttackCostSymbol::OneGeneric
                            } else {
                                spellwake_engine::program::AttackCostSymbol::Generic(n)
                            }
                        })
                };
                if let Some(symbol) = symbol {
                    let protect_player = true;
                    push_attack_requirement(
                        program,
                        AttackRequirement::PayPerAttacker {
                            protect_player,
                            symbol,
                        },
                    );
                    return Ok(());
                }
            }
            if let Some(dest_raw) = r_low.strip_prefix("redirectalldamageto:") {
                let dest = dest_raw.trim_end_matches(" instead").trim();
                let from_you = matches!(
                    subject.as_deref(),
                    Some(SubjectRefAst::Player(PlayerRefAst::You))
                );
                if from_you {
                    let dmg_rule = if dest == "~" {
                        Some(StaticDamageRule::RedirectPlayerDamageToSource {
                            include_other_permanents: false,
                        })
                    } else if dest == "enchanted creature" || dest == "equipped creature" {
                        Some(StaticDamageRule::RedirectPlayerDamageToEnchanted)
                    } else {
                        None
                    };
                    if let Some(rule) = dmg_rule {
                        return push_static_ability(
                            program,
                            StaticContinuousAbility::DamageRule {
                                id: ids.static_id(),
                                rule,
                            },
                        );
                    }
                }
            }
            if let Some(draw_raw) = r_low.strip_prefix("cantdraw:") {
                let draw_rule = if draw_raw == "players can't draw cards" {
                    Some(StaticDrawRule::PlayersCannotDraw)
                } else if matches!(
                    draw_raw,
                    "your opponents can't draw cards"
                        | "each opponent can't draw cards"
                        | "opponents can't draw cards"
                ) {
                    Some(StaticDrawRule::OpponentsCannotDraw)
                } else {
                    None
                };
                if let Some(rule) = draw_rule {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::DrawRule {
                            id: ids.static_id(),
                            rule,
                        },
                    );
                }
            }
            if r_low == "looktopanytime:you may look at the top card of your library any time" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::SeeLibraryTop { public: false },
                    },
                );
            }
            if r_low == "players have no maximum hand size" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DrawRule {
                        id: ids.static_id(),
                        rule: StaticDrawRule::AllPlayersNoMaximumHandSize,
                    },
                );
            }
            if r_low == "you may play lands and cast spells from the top of your library" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::PlayLibraryTop(LibraryTopPermission::All),
                    },
                );
            }
            if (r_low.contains("can't be countered") || r_low == "cantbecountered")
                && matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject))
            {
                program.cannot_be_countered = true;
                return Ok(());
            }
            if let Some(as_though) = r_low.strip_prefix("as_though:") {
                let at = as_though.trim().trim_end_matches('.').trim();
                if matches!(
                    at,
                    "spells and abilities you control as though they didn't have hexproof"
                        | "spells and abilities you control as though it didn't have hexproof"
                ) && let Some(subj) = subject.as_deref()
                {
                    let selectors = lower_static_permanent_selectors(subj, vocabulary)?;
                    for selector in selectors {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::Restriction {
                                id: ids.static_id(),
                                rule: StaticRestriction::IgnoreOpponentsHexproof(Box::new(
                                    selector,
                                )),
                            },
                        )?;
                    }
                    return Ok(());
                }
            }
            if r_low == "can't become untapped" {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors(s, vocabulary)?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for selector in selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::CannotBeUntapped(Box::new(selector)),
                        },
                    )?;
                }
                return Ok(());
            }
            if matches!(
                r_low.as_str(),
                "can't be sacrificed" | "cant_be_sacrificed" | "cantbesacrificed"
            ) {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors(s, vocabulary)?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for selector in selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::CannotBeSacrificed(Box::new(selector)),
                        },
                    )?;
                }
                return Ok(());
            }
            if matches!(
                r_low.as_str(),
                "can't have counters put on it"
                    | "can't have counters placed on it"
                    | "can't have counters put on them"
                    | "can't have counters placed on them"
            ) {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_static_permanent_selectors(s, vocabulary)?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for selector in selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::PreventCounters {
                                selector: Some(Box::new(selector)),
                                players: None,
                                counter_kind: None,
                            },
                        },
                    )?;
                }
                return Ok(());
            }
            if matches!(
                r_low.as_str(),
                "is goaded" | "is also goaded" | "permanents_are_goaded"
            ) && matches!(subject.as_deref(), Some(SubjectRefAst::AttachedHost(_)))
                && is_permanent_attachment_program(program)
            {
                push_attack_requirement(program, AttackRequirement::EnchantedCreatureIsGoaded);
                return Ok(());
            }
            if r_low == "you may look at the top card of your library any time"
                || r_low == "look_top_library_any_time"
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::SeeLibraryTop { public: false },
                    },
                );
            }
            if r_low == "play with the top card of your library revealed" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::SeeLibraryTop { public: true },
                    },
                );
            }
            if r_low == "players play with the top card of their libraries revealed"
                && subject.is_none()
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::AllPlayersRevealLibraryTop,
                    },
                );
            }
            if r_low == "players play with their hands revealed" && subject.is_none() {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::RevealHands {
                            opponents_only: false,
                        },
                    },
                );
            }
            if r_low == "your opponents play with their hands revealed" && subject.is_none() {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::RevealHands {
                            opponents_only: true,
                        },
                    },
                );
            }
            if matches!(
                r_low.as_str(),
                "you may look at cards exiled with ~"
                    | "you may look at face-down cards exiled with ~"
                    | "you may look at the cards exiled with ~"
                    | "you may look at the face-down cards exiled with ~"
            ) && subject.is_none()
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::LookAtSourceExiledCards,
                    },
                );
            }
            if !is_spell
                && subject.is_none()
                && r_low
                    == "you may play cards exiled with ~, and you may spend mana as though it were mana of any color to cast those spells"
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::ExileMatchingExt {
                            filter: Box::new(ObjectFilter::any()),
                            linked_to_source: true,
                            free: false,
                            any_color: true,
                            lands_allowed: true,
                            once_each_turn: false,
                            own_turn_only: false,
                        },
                    },
                );
            }
            if !is_spell
                && subject.is_none()
                && r_low
                    == "you may cast the exiled card, and mana of any type can be spent to cast that spell"
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::ExileMatchingExt {
                            filter: Box::new(ObjectFilter::any()),
                            linked_to_source: true,
                            free: false,
                            any_color: true,
                            lands_allowed: false,
                            once_each_turn: false,
                            own_turn_only: false,
                        },
                    },
                );
            }
            if !is_spell
                && subject.is_none()
                && r_low == "you may exert ~ as it attacks"
                && matches!(program.kind, ProgramKind::Permanent(_))
                && program.has_type(CardType::Creature)
            {
                let spec = TriggerSpec::attacks(
                    ObjectFilter::self_object(),
                    AttackTriggerMode::Individual {
                        capture_defender: false,
                    },
                );
                let body = ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        },
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::exert_source(),
                    ],
                );
                program.triggered.push(TriggeredAbility {
                    id: ids.triggered(),
                    grouping: TriggerGrouping::EachOccurrence,
                    condition: spec,
                    body,
                });
                return Ok(());
            }
            if !is_spell
                && r_low
                    == "once during each of your turns, you may cast an enchantment spell by paying life equal to its mana value rather than paying its mana cost"
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CostModifier {
                        id: ids.static_id(),
                        modifier: SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce,
                    },
                );
            }
            if r_low == "you may play an additional land on each of your turns" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::AdditionalLandPlays(
                            NonZeroU8::new(1).ok_or("oracle:unreachable")?,
                        ),
                    },
                );
            }
            if subject.is_none()
                && let Some(n) = parse_all_players_additional_land_plays(&r_low)
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: CastPermissionRule::AllPlayersAdditionalLandPlays(n),
                    },
                );
            }
            if r_low == "you have no maximum hand size" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DrawRule {
                        id: ids.static_id(),
                        rule: StaticDrawRule::NoMaximumHandSize,
                    },
                );
            }
            if subject.is_none()
                && let Some(hand_raw) = r_low.strip_prefix("maxhandsize:")
            {
                let cleaned = hand_raw.trim().trim_end_matches('.').trim();
                if let Some(num_raw) = cleaned.strip_prefix("your maximum hand size is ")
                    && let Some(limit) = parse_nonneg_u8_word(num_raw.trim())
                {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::DrawRule {
                            id: ids.static_id(),
                            rule: StaticDrawRule::MaxHandSize(limit),
                        },
                    );
                }
                if !is_spell && let Some((player, delta)) = parse_relative_max_hand_size(cleaned) {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::Modifier {
                            id: ids.static_id(),
                            rule: StaticModifierRule::ModifyMaxHandSize { player, delta },
                        },
                    );
                }
            }
            if matches!(
                r_low.as_str(),
                "each player can't cast more than one spell each turn"
                    | "players can't cast more than one spell each turn"
                    | "each player can't cast more than 1 spell each turn"
                    | "players can't cast more than 1 spell each turn"
            ) && matches!(
                subject.as_deref(),
                None | Some(SubjectRefAst::Player(
                    PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer
                ))
            ) {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OneSpellPerTurn,
                    },
                );
            }
            if matches!(
                r_low.as_str(),
                "you can't cast more than one spell each turn"
                    | "you can't cast more than 1 spell each turn"
            ) && matches!(
                subject.as_deref(),
                None | Some(SubjectRefAst::Player(PlayerRefAst::You))
            ) {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerOneSpellPerTurn,
                    },
                );
            }
            if r_low == "your opponents can't cast spells during combat" && subject.is_none() {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OpponentsCantCastDuringCombat,
                    },
                );
            }
            if matches!(
                r_low.as_str(),
                "each opponent can cast spells only any time they could cast a sorcery"
                    | "your opponents can cast spells only any time they could cast a sorcery"
                    | "opponents can cast spells only any time they could cast a sorcery"
            ) && subject.is_none()
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OpponentsSorceryOnly,
                    },
                );
            }
            if r_low == "entertriggerssuppressed" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::SuppressEntersTriggers,
                    },
                );
            }
            if r_low == "legendruleexempt" {
                let rule = match subject.as_deref() {
                    None => StaticModifierRule::IgnoreLegendRule,
                    Some(SubjectRefAst::All(f) | SubjectRefAst::Group(f))
                        if f.controller == Some(ControllerRelation::You)
                            && filter_only_has(
                                f,
                                FilterAllow {
                                    controller: true,
                                    ..FilterAllow::NONE
                                },
                            ) =>
                    {
                        StaticModifierRule::IgnoreLegendRuleForController
                    }
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Group(f) | SubjectRefAst::Each(f),
                    ) if f.subtypes.len() == 1
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                subtypes: true,
                                controller: true,
                                ..FilterAllow::NONE
                            },
                        )
                        && (f.any_types.is_empty() || f.any_types == AstCardTypeMask::CREATURE)
                        && f.controller.is_none()
                        && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0]) =>
                    {
                        StaticModifierRule::IgnoreLegendForSubtype(ct)
                    }
                    _ => return Err("oracle:unsupported-rule-restriction"),
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ids.static_id(),
                        rule,
                    },
                );
            }
            if r_low == "players can't gain life" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PlayersCannotGainLife,
                    },
                );
            }
            if r_low == "your opponents can't gain life" || r_low == "opponents can't gain life" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OpponentsCannotGainLife,
                    },
                );
            }
            if r_low == "you have hexproof" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerHasHexproof,
                    },
                );
            }
            if r_low == "you have shroud" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerHasShroud,
                    },
                );
            }
            if r_low == "your life total can't change"
                && matches!(
                    subject.as_deref(),
                    None | Some(SubjectRefAst::Player(PlayerRefAst::You))
                )
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerLifeTotalCantChange,
                    },
                );
            }
            if r_low == "you don't lose the game for having 0 or less life" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerCantLoseForZeroLife,
                    },
                );
            }
            if r_low == "you can't lose the game and your opponents can't win the game" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                    },
                );
            }
            if r_low == "you can't win the game and your opponents can't lose the game" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerCantWinOpponentsCantLose,
                    },
                );
            }
            if r_low == "players can't lose the game or win the game" {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerCantLoseOpponentsCantWin,
                    },
                )?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ControllerCantWinOpponentsCantLose,
                    },
                );
            }
            if matches!(
                r_low.as_str(),
                "players can't play lands" | "players_cant_play_lands" | "playerscantplaylands"
            ) {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::CantPlayLands(PlayerRelation::AnyParticipant),
                    },
                );
            }
            if matches!(
                r_low.as_str(),
                "your opponents can't play lands"
                    | "opponents can't play lands"
                    | "your_opponents_cant_play_lands"
                    | "opponents_cant_play_lands"
            ) {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::CantPlayLands(PlayerRelation::Opponent),
                    },
                );
            }
            if matches!(
                r_low.as_str(),
                "you can't play lands" | "you_cant_play_lands"
            ) {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::CantPlayLands(PlayerRelation::Controller),
                    },
                );
            }
            if matches!(
                r_low.as_str(),
                "players can't search libraries"
                    | "each player can't search libraries"
                    | "players_cant_search_libraries"
                    | "playerscantsearchlibraries"
            ) && matches!(
                subject.as_deref(),
                None | Some(SubjectRefAst::Player(
                    PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer
                ))
            ) {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PlayersCantSearchLibraries(
                            PlayerRelation::AnyParticipant,
                        ),
                    },
                );
            }
            if matches!(
                r_low.as_str(),
                "your opponents can't search libraries"
                    | "opponents can't search libraries"
                    | "each opponent can't search libraries"
            ) && matches!(
                subject.as_deref(),
                None | Some(SubjectRefAst::Player(
                    PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent
                ))
            ) {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PlayersCantSearchLibraries(
                            PlayerRelation::Opponent,
                        ),
                    },
                );
            }
            if r_low == "you can't search libraries"
                && matches!(
                    subject.as_deref(),
                    None | Some(SubjectRefAst::Player(PlayerRefAst::You))
                )
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PlayersCantSearchLibraries(
                            PlayerRelation::Controller,
                        ),
                    },
                );
            }
            if r_low == "players can't cast spells from graveyards or libraries" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::RestrictGraveyardAndLibrary,
                    },
                );
            }
            if r_low
                == "players can't pay life or sacrifice creatures to cast spells or activate abilities"
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ProhibitLifeAndCreatureCosts,
                    },
                );
            }
            if r_low
                == "spells and abilities your opponents control can't cause you to sacrifice permanents"
            {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::ProhibitOpponentsCausingSacrifice,
                    },
                );
            }
            if r_low == "activated abilities of artifacts can't be activated" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventArtifactActivations,
                    },
                );
            }
            if r_low == "activated abilities of planeswalkers can't be activated" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventLoyaltyActivations,
                    },
                );
            }
            if r_low == "your opponents can't cast spells during your turn" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::RestrictOpponentsDuringOwnTurn,
                    },
                );
            }
            if r_low == "each opponent can't draw more than one card each turn" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DrawRule {
                        id: ids.static_id(),
                        rule: StaticDrawRule::OpponentsAtMostOne,
                    },
                );
            }
            if r_low == "skip your draw step" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DrawRule {
                        id: ids.static_id(),
                        rule: StaticDrawRule::SkipControllerDrawStep,
                    },
                );
            }
            if !is_spell && subject.is_none() {
                if r_low == "skip your upkeep step if you have no cards in hand" {
                    let id = ids.static_id();
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Restriction {
                            id,
                            rule: StaticRestriction::SkipStep {
                                player: PlayerRelation::Controller,
                                step: Step::Upkeep,
                            },
                        },
                    )?;
                    program
                        .static_conditions
                        .insert(id, StaticSourceCondition::HandCountAtMost(0));
                    return Ok(());
                }
                let skip_step_rule = match r_low.as_str() {
                    "skip your untap step" => Some((PlayerRelation::Controller, Step::Untap)),
                    "skip your upkeep step" => Some((PlayerRelation::Controller, Step::Upkeep)),
                    "skip your end step" => Some((PlayerRelation::Controller, Step::End)),
                    "players skip their untap steps" | "each player skips their untap step" => {
                        Some((PlayerRelation::AnyParticipant, Step::Untap))
                    }
                    "players skip their upkeep steps" | "each player skips their upkeep step" => {
                        Some((PlayerRelation::AnyParticipant, Step::Upkeep))
                    }
                    "players skip their draw steps" | "each player skips their draw step" => {
                        Some((PlayerRelation::AnyParticipant, Step::Draw))
                    }
                    "players skip their end steps" | "each player skips their end step" => {
                        Some((PlayerRelation::AnyParticipant, Step::End))
                    }
                    "your opponents skip their untap steps"
                    | "each opponent skips their untap step" => {
                        Some((PlayerRelation::Opponent, Step::Untap))
                    }
                    "your opponents skip their upkeep steps"
                    | "each opponent skips their upkeep step" => {
                        Some((PlayerRelation::Opponent, Step::Upkeep))
                    }
                    "your opponents skip their draw steps"
                    | "each opponent skips their draw step" => {
                        Some((PlayerRelation::Opponent, Step::Draw))
                    }
                    _ => None,
                };
                if let Some((player, step)) = skip_step_rule {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::SkipStep { player, step },
                        },
                    );
                }
            }
            if !is_spell && subject.is_none() && condition.is_none() {
                if r_low == "no more than one creature can attack each combat" {
                    program.combat_limits.one_attacker = true;
                    return Ok(());
                }
                if r_low == "no more than one creature can block each combat" {
                    program.combat_limits.one_blocker = true;
                    return Ok(());
                }
            }
            if !is_spell
                && subject.is_none()
                && r_low == "enchanted land is the chosen type"
                && program.permanent().and_then(|p| p.entry_tap)
                    == Some(EntryTap::ChooseBasicLandType)
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: PermanentSelector {
                            attached_to_source: true,
                            ..PermanentSelector::default()
                        },
                        parts: StaticParts(vec![StaticPart::Type(StaticTypeChange {
                            supertypes: TypeOperation::Keep,
                            types: TypeOperation::Keep,
                            subtypes: SubtypeOperation::Keep,
                            land_subtypes: SubtypeOperation::Set(
                                StaticLandSubtypeOperand::SourceEntryBasicLandType,
                            ),
                        })]),
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if r_low == "damage that would be dealt by ~ can't be prevented"
                || (r_low == "damage can't be prevented"
                    && matches!(subject.as_deref(), Some(SubjectRefAst::SelfObject)))
            {
                program
                    .printed_keywords
                    .insert(KeywordAbility::DamageCannotBePrevented);
                return Ok(());
            }
            if r_low == "damage can't be prevented" && subject.is_none() {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DamageRule {
                        id: ids.static_id(),
                        rule: StaticDamageRule::DamageCannotBePrevented,
                    },
                );
            }
            Err("oracle:unsupported-rule-restriction")
        }
        StaticAbilityAst::TriggerMultiplier {
            event_desc,
            multiplier,
            condition,
        } => {
            let event = lower_trigger_multiplier_event(event_desc);
            let exclude_sacrifice_triggers = matches!(event, TriggerMultiplierEvent::Dies(_));
            let additional = multiplier.saturating_sub(1).max(1);
            push_conditional_static(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::AdditionalTrigger(Box::new(TriggerMultiplierRule {
                        additional,
                        event,
                        triggering_source: None,
                        exclude_self: true,
                        exclude_sacrifice_triggers,
                    })),
                },
                condition.as_deref(),
                vocabulary,
            )
        }
        StaticAbilityAst::ScopedTriggerMultiplier {
            event_desc,
            source_filter,
            additional_triggers,
            condition,
        } => {
            if *additional_triggers == 0 {
                return Err("oracle:unsupported-trigger-multiplier");
            }
            let selector = lower_static_filter_to_permanent_selector(source_filter, vocabulary)?;
            let event = lower_trigger_multiplier_event(event_desc);
            let exclude_sacrifice_triggers = matches!(event, TriggerMultiplierEvent::Dies(_));
            push_conditional_static(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::AdditionalTrigger(Box::new(TriggerMultiplierRule {
                        additional: *additional_triggers,
                        event,
                        triggering_source: Some(selector),
                        exclude_self: false,
                        exclude_sacrifice_triggers,
                    })),
                },
                condition.as_deref(),
                vocabulary,
            )
        }
        StaticAbilityAst::ConspiracyAgenda {
            hidden,
            named_slots,
        } => push_static_ability(
            program,
            StaticContinuousAbility::CommandZoneActive {
                id: ids.static_id(),
                agenda: spellwake_engine::characteristics::AgendaSpec {
                    hidden: *hidden,
                    named_slots: *named_slots,
                },
            },
        ),
        StaticAbilityAst::OncePerTurn(inner) => {
            lower_once_per_turn_static(inner, false, program, vocabulary, ids, is_spell)
        }
        StaticAbilityAst::OnceDuringYourTurn(inner) => {
            lower_once_per_turn_static(inner, true, program, vocabulary, ids, is_spell)
        }
        StaticAbilityAst::AlternativeCastCost {
            cost,
            condition,
            for_spells,
            next_only: false,
        } => lower_alternative_cast_cost(
            cost,
            condition.as_deref(),
            for_spells.as_deref(),
            program,
            vocabulary,
            ids,
            is_spell,
        ),
        StaticAbilityAst::PairedTrigger { trigger, body } => lower_ability(
            &AbilityAst::Triggered {
                ability_word: None,
                trigger: trigger.clone(),
                body: body.clone(),
            },
            program,
            vocabulary,
            ids,
        ),
        StaticAbilityAst::DoubleAllDamage { source, condition } if !is_spell => {
            let (src_filter, source_controller) = lower_damage_source_filter(source, vocabulary)
                .map_err(|_| "oracle:unsupported-static")?;
            push_damage_modify_rules(
                DamageModifySpec {
                    source: src_filter,
                    source_controller,
                    kind: StaticDamageKind::Any,
                    op: StaticDamageOp::Multiply(2),
                },
                &[StaticDamageRecipient::Any],
                condition.as_deref(),
                program,
                vocabulary,
                ids,
            )
        }
        StaticAbilityAst::PreventFractionDamage {
            source,
            target,
            numerator,
            denom,
            rounding,
            condition,
        } if !is_spell => {
            if *numerator != 1 || *denom != 2 {
                return Err("oracle:unsupported-static");
            }
            let (src_filter, source_controller) = match source.as_deref() {
                Some(s) => lower_damage_source_filter(s, vocabulary)
                    .map_err(|_| "oracle:unsupported-static")?,
                None => (None, PlayerRelation::AnyParticipant),
            };
            let recipients = lower_damage_recipients(target, vocabulary)
                .map_err(|_| "oracle:unsupported-static")?;
            push_damage_modify_rules(
                DamageModifySpec {
                    source: src_filter,
                    source_controller,
                    kind: StaticDamageKind::Any,
                    op: StaticDamageOp::PreventHalf {
                        round_up: matches!(rounding, Rounding::Up),
                    },
                },
                &recipients,
                condition.as_deref(),
                program,
                vocabulary,
                ids,
            )
        }
        StaticAbilityAst::PreventThresholdDamage {
            source,
            target,
            max_damage,
            condition,
        } if !is_spell => {
            let ValueExprAst::Const(n) = max_damage.as_ref() else {
                return Err("oracle:unsupported-static");
            };
            if *n <= 0 {
                return Err("oracle:unsupported-static");
            }
            let max_u8 = u8::try_from(*n).map_err(|_| "oracle:unsupported-static")?;
            let (src_filter, source_controller) = match source.as_deref() {
                Some(s) => lower_damage_source_filter(s, vocabulary)
                    .map_err(|_| "oracle:unsupported-static")?,
                None => (None, PlayerRelation::AnyParticipant),
            };
            let recipients = lower_damage_recipients(target, vocabulary)
                .map_err(|_| "oracle:unsupported-static")?;
            push_damage_modify_rules(
                DamageModifySpec {
                    source: src_filter,
                    source_controller,
                    kind: StaticDamageKind::Any,
                    op: StaticDamageOp::PreventAtMost(max_u8),
                },
                &recipients,
                condition.as_deref(),
                program,
                vocabulary,
                ids,
            )
        }
        StaticAbilityAst::CounterModificationReplacement {
            player,
            modifier,
            rounding,
            condition: None,
        } if !is_spell => {
            let mod_low = modifier.trim().to_ascii_lowercase();
            let rule = match (mod_low.as_str(), player, rounding) {
                (
                    "half",
                    PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent,
                    None | Some(Rounding::Down),
                ) => StaticModifierRule::HalveOpponentCounters,
                ("double" | "twice", PlayerRefAst::You, None) => {
                    StaticModifierRule::DoublePlacedCounters
                }
                _ => return Err("oracle:unsupported-static"),
            };
            push_static_ability(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule,
                },
            )
        }
        StaticAbilityAst::LookOrRevealRule {
            rule,
            filter,
            follow_up,
            condition,
        } if !is_spell && follow_up.is_empty() => {
            let r_low = rule
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_ascii_lowercase();
            let is_your_library_top = |flt: &ObjectFilterAst| {
                flt.zone == Some(ZoneFilter::YourLibrary)
                    && !flt.any_permanent
                    && filter_only_has(
                        flt,
                        FilterAllow {
                            zone: true,
                            ..FilterAllow::NONE
                        },
                    )
            };
            let is_exiled_with_self = |flt: &ObjectFilterAst| {
                matches!(flt.zone, None | Some(ZoneFilter::Exile))
                    && !flt.any_permanent
                    && !flt.any_spell
                    && (flt.flags == AstFilterFlags::EXILED_WITH_SELF
                        || flt.flags
                            == AstFilterFlags(
                                AstFilterFlags::EXILED_WITH_SELF.0 | AstFilterFlags::FACE_DOWN.0,
                            ))
                    && filter_only_has(
                        flt,
                        FilterAllow {
                            zone: true,
                            flags: true,
                            ..FilterAllow::NONE
                        },
                    )
            };
            let permission = match (r_low.as_str(), filter.as_deref()) {
                ("look_any_time", Some(flt)) if is_your_library_top(flt) => {
                    CastPermissionRule::SeeLibraryTop { public: false }
                }
                (
                    "play_with_top_revealed" | "play with the top card of your library revealed",
                    flt,
                ) if flt.is_none_or(is_your_library_top) => {
                    CastPermissionRule::SeeLibraryTop { public: true }
                }
                (
                    "players play with the top card of their libraries revealed"
                    | "all_players_reveal_library_top",
                    None,
                ) => CastPermissionRule::AllPlayersRevealLibraryTop,
                (
                    "players play with their hands revealed"
                    | "play_with_hand_revealed:each player"
                    | "play_with_hand_revealed:players",
                    None,
                ) => CastPermissionRule::RevealHands {
                    opponents_only: false,
                },
                (
                    "your opponents play with their hands revealed"
                    | "opponents play with their hands revealed"
                    | "play_with_hand_revealed:your opponents"
                    | "play_with_hand_revealed:opponents"
                    | "play_with_hand_revealed:each opponent",
                    None,
                ) => CastPermissionRule::RevealHands {
                    opponents_only: true,
                },
                (
                    "you may look at cards exiled with ~"
                    | "you may look at face-down cards exiled with ~"
                    | "you may look at the cards exiled with ~"
                    | "you may look at the face-down cards exiled with ~",
                    None,
                ) => CastPermissionRule::LookAtSourceExiledCards,
                ("look_any_time", Some(flt)) if is_exiled_with_self(flt) => {
                    CastPermissionRule::LookAtSourceExiledCards
                }
                _ => return Err("oracle:unsupported-static"),
            };
            push_conditional_static(
                program,
                StaticContinuousAbility::CastPermission {
                    id: ids.static_id(),
                    permission,
                },
                condition.as_deref(),
                vocabulary,
            )
        }
        StaticAbilityAst::PreventAndReplaceDamage {
            target,
            follow_up,
            condition,
        } if !is_spell => lower_prevent_and_replace_damage(
            target,
            follow_up,
            condition.as_deref(),
            program,
            vocabulary,
            ids,
        ),
        StaticAbilityAst::ActivationLimitOverride {
            ability_kind,
            subject,
            extra_times,
            instant_speed,
            condition,
        } if !is_spell => {
            if !ability_kind.trim().eq_ignore_ascii_case("loyalty")
                || *instant_speed
                || *extra_times != 1
            {
                return Err("oracle:unsupported-static");
            }
            let matches_own_planeswalkers = match subject.as_ref() {
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
                    f.any_types == AstCardTypeMask::PLANESWALKER
                        && f.controller == Some(ControllerRelation::You)
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                controller: true,
                                ..FilterAllow::NONE
                            },
                        )
                }
                _ => false,
            };
            if !matches_own_planeswalkers {
                return Err("oracle:unsupported-static");
            }
            push_conditional_static(
                program,
                StaticContinuousAbility::Modifier {
                    id: ids.static_id(),
                    rule: StaticModifierRule::TwiceLoyaltyPerTurn,
                },
                condition.as_deref(),
                vocabulary,
            )
        }
        StaticAbilityAst::CombatDamageAssignmentRule {
            subject,
            rule,
            condition,
        } => {
            let r_low = rule
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_ascii_lowercase();
            match r_low.as_str() {
                "toughness_instead_of_power"
                | "assigns combat damage equal to its toughness rather than its power"
                | "assign combat damage equal to their toughness rather than their power" => {
                    let mut selectors = lower_static_permanent_selectors_with_context(
                        subject,
                        condition.as_deref(),
                        vocabulary,
                    )?;
                    for sel in &mut selectors {
                        if let Some(cond) = condition {
                            apply_static_selector_condition(sel, cond, vocabulary)?;
                        }
                    }
                    for selector in selectors {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::Battlefield {
                                id: ids.static_id(),
                                selector,
                                parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                    BlockingRestriction::AssignDamageByToughness,
                                )]),
                            },
                        )?;
                    }
                    Ok(())
                }
                "assign_combat_damage_unblocked" | "assigndamageasthoughunblocked" => {
                    let mut selectors = lower_static_permanent_selectors_with_context(
                        subject,
                        condition.as_deref(),
                        vocabulary,
                    )?;
                    for sel in &mut selectors {
                        if let Some(cond) = condition {
                            apply_static_selector_condition(sel, cond, vocabulary)?;
                        }
                    }
                    for selector in selectors {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::Battlefield {
                                id: ids.static_id(),
                                selector,
                                parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                    BlockingRestriction::AssignDamageAsThoughUnblocked,
                                )]),
                            },
                        )?;
                    }
                    Ok(())
                }
                "lethal_damage_by_power" if !is_spell => {
                    let matches_own_creatures = match subject.as_ref() {
                        SubjectRefAst::All(f)
                        | SubjectRefAst::Each(f)
                        | SubjectRefAst::Group(f) => {
                            f.any_types == AstCardTypeMask::CREATURE
                                && f.controller == Some(ControllerRelation::You)
                                && filter_only_has(
                                    f,
                                    FilterAllow {
                                        types: true,
                                        controller: true,
                                        ..FilterAllow::NONE
                                    },
                                )
                        }
                        _ => false,
                    };
                    if !matches_own_creatures {
                        return Err("oracle:unsupported-static");
                    }
                    push_conditional_static(
                        program,
                        StaticContinuousAbility::DamageRule {
                            id: ids.static_id(),
                            rule: StaticDamageRule::LethalUsesPower,
                        },
                        condition.as_deref(),
                        vocabulary,
                    )
                }
                _ => Err("oracle:unsupported-static"),
            }
        }
        StaticAbilityAst::DynamicBasePT {
            subject,
            power,
            toughness,
            condition,
        } => lower_dynamic_base_pt(
            subject,
            power,
            toughness,
            condition.as_deref(),
            program,
            vocabulary,
            ids,
        ),
        StaticAbilityAst::ExertOnAttackAction { body } => {
            lower_exert_attack_followup(body, program, vocabulary, ids, is_spell)
        }
        StaticAbilityAst::CantBeBlockedExceptByAny {
            subject,
            allowed_filters,
            condition,
        } => {
            let br = lower_cant_be_blocked_except_by_any(allowed_filters, vocabulary)
                .ok_or("oracle:unsupported-blocking-restriction")?;
            let mut selectors = lower_static_permanent_selectors_with_context(
                subject,
                condition.as_deref(),
                vocabulary,
            )?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(br.clone())]),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::CrewOrSaddlePowerBonus {
            subject,
            crews_vehicles,
            saddles_mounts,
            bonus,
        } if !is_spell => {
            if !*crews_vehicles && !*saddles_mounts {
                return Err("oracle:unsupported-static");
            }
            let ValueExprAst::Const(n) = bonus.as_ref() else {
                return Err("oracle:unsupported-static");
            };
            if *n <= 0 {
                return Err("oracle:unsupported-static");
            }
            let bonus_u8 = u8::try_from(*n).map_err(|_| "oracle:unsupported-static")?;
            let selectors = lower_static_permanent_selectors(subject, vocabulary)?;
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ids.static_id(),
                        rule: StaticModifierRule::CrewAndSaddlePowerBonus {
                            selector: Box::new(selector),
                            crews: *crews_vehicles,
                            saddles: *saddles_mounts,
                            bonus: bonus_u8,
                        },
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::SkipStep {
            player,
            step,
            condition,
        } if !is_spell => {
            let player_rel =
                lower_player_relation(*player).map_err(|_| "oracle:unsupported-static")?;
            let engine_step = lower_turn_step_for_skip(*step)?;
            push_conditional_static(
                program,
                StaticContinuousAbility::Restriction {
                    id: ids.static_id(),
                    rule: StaticRestriction::SkipStep {
                        player: player_rel,
                        step: engine_step,
                    },
                },
                condition.as_deref(),
                vocabulary,
            )
        }
        StaticAbilityAst::ModifyMaximumHandSize {
            player,
            size,
            condition,
        } if !is_spell => {
            if matches!(player, PlayerRefAst::You)
                && let ValueExprAst::Const(n) = size.as_ref()
                && *n >= 0
                && let Ok(limit) = u8::try_from(*n)
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::DrawRule {
                        id: ids.static_id(),
                        rule: StaticDrawRule::MaxHandSize(limit),
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if let ValueExprAst::Const(n) = size.as_ref()
                && *n < 0
                && let Ok(delta) = i8::try_from(*n)
                && let Ok(player_rel) = lower_player_relation(*player)
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::Modifier {
                        id: ids.static_id(),
                        rule: StaticModifierRule::ModifyMaxHandSize {
                            player: player_rel,
                            delta,
                        },
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            Err("oracle:unsupported-static")
        }
        StaticAbilityAst::DynamicModifyPT {
            subject,
            power_delta,
            toughness_delta,
            condition,
        } => {
            let mut selectors = lower_static_permanent_selectors_with_context(
                subject,
                condition.as_deref(),
                vocabulary,
            )?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let expr_ctx = StaticExprContext::from_subject_and_program(subject.as_ref(), program);
            let p_expr = lower_numeric_expr_with_context(power_delta, expr_ctx, vocabulary)?;
            let t_expr = lower_numeric_expr_with_context(toughness_delta, expr_ctx, vocabulary)?;
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                            power: p_expr.clone(),
                            toughness: t_expr.clone(),
                        })]),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::GrantKeywordWithCost {
            subject,
            keyword,
            cost_rule,
            condition,
        } => lower_grant_keyword_with_cost(
            subject,
            keyword,
            cost_rule,
            condition.as_deref(),
            program,
            vocabulary,
            ids,
        ),
        StaticAbilityAst::InheritAbilities {
            subject,
            source_filter,
            activated_only,
            condition,
        } if !is_spell => {
            if !*activated_only || !source_filter.extra_zones.is_empty() {
                return Err("oracle:unsupported-static");
            }
            let effective_cond = match condition.as_deref() {
                Some(ConditionAst::SubjectInZone {
                    subject: cond_subj,
                    zone: ZoneFilter::Battlefield,
                }) if matches!(cond_subj.as_ref(), SubjectRefAst::SelfObject) => None,
                other => other,
            };
            let (zone, inner_filter, loyalty_only) = if (source_filter.flags.0
                & AstFilterFlags::EXILED_WITH_SELF.0)
                != 0
                && matches!(source_filter.zone, None | Some(ZoneFilter::Exile))
                && source_filter.owner.is_none()
                && source_filter.controller.is_none()
            {
                let mut inner = source_filter.as_ref().clone();
                inner.zone = None;
                inner.flags = AstFilterFlags(inner.flags.0 & !AstFilterFlags::EXILED_WITH_SELF.0);
                (StaticZone::LinkedExile, inner, false)
            } else if source_filter.zone == Some(ZoneFilter::YourGraveyard)
                && source_filter.controller.is_none()
                && matches!(
                    source_filter.owner,
                    None | Some(ControllerRelation::You | ControllerRelation::OwnerYou)
                )
            {
                let mut inner = source_filter.as_ref().clone();
                inner.zone = None;
                inner.owner = None;
                (StaticZone::OwnGraveyard, inner, false)
            } else if matches!(
                source_filter.zone,
                Some(ZoneFilter::AllGraveyards | ZoneFilter::AnyGraveyard)
            ) && source_filter.controller.is_none()
                && source_filter.owner.is_none()
            {
                let mut inner = source_filter.as_ref().clone();
                inner.zone = None;
                (StaticZone::AllGraveyards, inner, false)
            } else if source_filter.zone == Some(ZoneFilter::YourHand)
                && source_filter.controller.is_none()
                && matches!(
                    source_filter.owner,
                    None | Some(ControllerRelation::You | ControllerRelation::OwnerYou)
                )
            {
                let mut inner = source_filter.as_ref().clone();
                inner.zone = None;
                inner.owner = None;
                (StaticZone::OwnHand, inner, false)
            } else if source_filter.zone == Some(ZoneFilter::Exile)
                && matches!(
                    source_filter.owner,
                    Some(ControllerRelation::You | ControllerRelation::OwnerYou)
                )
                && source_filter.controller.is_none()
            {
                let mut inner = source_filter.as_ref().clone();
                inner.zone = None;
                inner.owner = None;
                (StaticZone::OwnExile, inner, false)
            } else if matches!(source_filter.zone, None | Some(ZoneFilter::Battlefield))
                && source_filter.controller == Some(ControllerRelation::You)
                && source_filter.owner.is_none()
                && !source_filter.any_card
                && !source_filter.any_spell
            {
                let mut inner = source_filter.as_ref().clone();
                inner.zone = None;
                inner.controller = None;
                let loyalty_only = inner.any_types == AstCardTypeMask::PLANESWALKER;
                (StaticZone::ControlledBattlefield, inner, loyalty_only)
            } else if matches!(source_filter.zone, None | Some(ZoneFilter::Battlefield))
                && source_filter.owner.is_none()
                && !source_filter.any_card
                && !source_filter.any_spell
                && matches!(
                    source_filter.controller,
                    None | Some(ControllerRelation::Opponent | ControllerRelation::Opponents)
                )
                && (!source_filter.any_types.is_empty() || !source_filter.subtypes.is_empty())
            {
                let mut inner = source_filter.as_ref().clone();
                inner.zone = None;
                let loyalty_only = inner.any_types == AstCardTypeMask::PLANESWALKER;
                (StaticZone::AnyBattlefield, inner, loyalty_only)
            } else {
                return Err("oracle:unsupported-static");
            };
            let obj_filter = if is_unrestricted_filter(&inner_filter) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner_filter, vocabulary)
                    .map_err(|_| "oracle:unsupported-static")?
            };
            if ValueExpr::count_battlefield(obj_filter)
                .validate_world_expr(vocabulary)
                .is_err()
            {
                return Err("oracle:unsupported-static");
            }
            let mut selectors =
                lower_static_permanent_selectors_with_context(subject, effective_cond, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = effective_cond {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::InheritZoneAbilities {
                            zone,
                            filter: Box::new(obj_filter),
                            loyalty_only,
                        }]),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::DeckConstructionRule(text) => {
            let low = text.trim().to_ascii_lowercase();
            if low.contains("a deck can have any number of cards named")
                || low.contains("a deck can have up to ")
                || low.ends_with("can be your commander.")
                || low.ends_with("can be your commander")
            {
                // CR 113.6a / 903.3: deck-construction rules have no in-game runtime effect.
                Ok(())
            } else {
                Err("oracle:unsupported-static")
            }
        }
        StaticAbilityAst::CompiledStickerOption {
            ticket_cost,
            payload,
            power,
            toughness,
            compiled,
        } if !is_spell => {
            let mut resolved_parts = Vec::new();
            if let (Some(p_str), Some(t_str)) = (power.as_deref(), toughness.as_deref()) {
                let p = p_str
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| "oracle:unsupported-static")?;
                let t = t_str
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| "oracle:unsupported-static")?;
                resolved_parts.push(ResolvedPartSpec::SetPT(PowerToughness {
                    power: p.into(),
                    toughness: t.into(),
                }));
            } else if let Some(sub_face) = compiled.as_deref() {
                let static_parts = lower_granted_ability_face_parts(sub_face, vocabulary, ids)
                    .map_err(|_| "oracle:unsupported-static")?;
                for part in static_parts {
                    resolved_parts.push(
                        static_part_to_resolved_part_spec(part)
                            .map_err(|_| "oracle:unsupported-static")?,
                    );
                }
            } else {
                return Err("oracle:unsupported-static");
            }
            if resolved_parts.is_empty() {
                return Err("oracle:unsupported-static");
            }
            let empty_slots = std::collections::BTreeSet::new();
            let mut ops = Vec::new();
            if *ticket_cost > 0 {
                let ticket_kind =
                    CounterKind::from_oracle_label("ticket").ok_or("oracle:unsupported-static")?;
                ops.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::pay_player_counter(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::Controller,
                        ),
                        ticket_kind,
                        ValueExpr::literal(i64::from(*ticket_cost)),
                    ),
                );
            }
            ops.push(Instruction::SetObjectTag {
                query: Box::new(spellwake_engine::toolbox::ObjectQuery::Targets),
                key: spellwake_engine::toolbox::TagKey::Label("sticker".to_string()),
                value: Some(spellwake_engine::toolbox::TagValue::Text(payload.clone())),
            });
            for part in resolved_parts {
                let spec = ResolvedPartsSpec(vec![part]);
                spec.validate(vocabulary, &empty_slots)
                    .map_err(|_| "oracle:unsupported-static")?;
                ops.push(
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spec,
                        ResolvedDuration::NoStatedDuration,
                    ),
                );
            }
            let nonland_controlled =
                ObjectFilter::from_clause(ObjectClause::any().controlled().nonland());
            program.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: ExecutableBody::new(BodyTarget::FilteredPermanent(nonland_controlled), ops),
            });
            Ok(())
        }
        _ => Err("oracle:unsupported-static"),
    }
}

fn merge_cast_timing_restrictions(
    first: CastTimingRestriction,
    second: CastTimingRestriction,
) -> CastTimingRestriction {
    let mut flat = Vec::new();
    push_cast_timing_flat(&mut flat, first);
    push_cast_timing_flat(&mut flat, second);
    if flat.len() == 1
        && let Some(single) = flat.pop()
    {
        single
    } else {
        CastTimingRestriction::All(flat)
    }
}

fn lower_cast_timing_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<CastTimingRestriction> {
    match cond {
        ConditionAst::Marker(GameMarker::DuringStep { player, step }) => {
            let engine_step = lower_turn_step_for_skip(*step).ok()?;
            match player {
                PlayerRefAst::You => Some(CastTimingRestriction::DuringOwnStep(engine_step)),
                PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent => {
                    Some(CastTimingRestriction::DuringOpponentsStep(engine_step))
                }
                PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer => {
                    Some(CastTimingRestriction::DuringStep(engine_step))
                }
                _ => None,
            }
        }
        ConditionAst::IsYourTurn => Some(CastTimingRestriction::DuringYourTurn),
        ConditionAst::Not(inner) if matches!(inner.as_ref(), ConditionAst::IsYourTurn) => {
            Some(CastTimingRestriction::DuringOpponentsTurn)
        }
        ConditionAst::And(lhs, rhs) => {
            let a = lower_cast_timing_condition(lhs, vocabulary)?;
            let b = lower_cast_timing_condition(rhs, vocabulary)?;
            Some(merge_cast_timing_restrictions(a, b))
        }
        other if is_x_nonzero_condition_ast(other) => Some(CastTimingRestriction::XNonZero),
        other => {
            let mut dummy = BodyBuilder::new();
            let rc = lower_resolution_condition(other, &mut dummy, vocabulary, false, None).ok()?;
            if dummy.target == BodyTarget::None
                && rc.is_world_evaluable()
                && rc.validate_world(vocabulary).is_ok()
            {
                Some(CastTimingRestriction::Condition(Box::new(rc)))
            } else {
                None
            }
        }
    }
}

fn lower_once_per_turn_static(
    inner: &StaticAbilityAst,
    force_own_turn_only: bool,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    is_spell: bool,
) -> Result<(), &'static str> {
    match inner {
        StaticAbilityAst::PlayOrCastPermission {
            cast_only,
            filter,
            permission,
            condition,
        } => {
            let p_low = permission.trim().trim_end_matches('.').to_ascii_lowercase();
            if matches!(
                p_low.as_str(),
                "from your graveyard" | "you may cast creature spells from your graveyard"
            ) && filter.any_types == AstCardTypeMask::CREATURE
                && filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
                && matches!(filter.zone, None | Some(ZoneFilter::YourGraveyard))
            {
                let (cond_own_turn, rem_cond) = split_is_your_turn_condition(condition.as_deref());
                let perm = if force_own_turn_only || cond_own_turn {
                    CastPermissionRule::GraveyardMatchingOwnTurn {
                        lands_allowed: false,
                        filter: Box::new(ObjectFilter::creature()),
                        once_each_turn: true,
                    }
                } else {
                    CastPermissionRule::GraveyardCreatureCast
                };
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: perm,
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            if !is_spell
                && !force_own_turn_only
                && *cast_only
                && (p_low == "from your hand without paying its mana cost"
                    || p_low == "from your hand without paying their mana costs"
                    || (matches!(
                        p_low.as_str(),
                        "without paying its mana cost" | "without paying their mana costs"
                    ) && filter.zone == Some(ZoneFilter::YourHand)))
                && filter.color_arity == Some(ColorArity::Colorless)
                && matches!(filter.zone, None | Some(ZoneFilter::YourHand))
                && filter.any_types.is_empty()
                && filter_only_has(
                    filter,
                    FilterAllow {
                        color_arity: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CostModifier {
                        id: ids.static_id(),
                        modifier: SpellCostModifier::FreeColorlessHandSpellOnce,
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if !is_spell
                && *cast_only
                && let Some(pay_life_mv) = classify_direct_alt_cast_permission(&p_low)
                && let Some((mut modifier, rem_cond)) = try_lower_alt_matching_permission(
                    &p_low,
                    filter,
                    condition.as_deref(),
                    true,
                    pay_life_mv,
                    vocabulary,
                )
            {
                if force_own_turn_only
                    && let SpellCostModifier::AlternativeForMatching {
                        ref mut own_turn_only,
                        ..
                    } = modifier
                {
                    *own_turn_only = true;
                }
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CostModifier {
                        id: ids.static_id(),
                        modifier,
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            if !is_spell
                && let Some((base_perm, pay_life_mv)) = strip_cast_this_way_alt_cost_suffix(&p_low)
            {
                let own_turn_cond;
                let effective_cond = if force_own_turn_only {
                    own_turn_cond = match condition.as_deref() {
                        Some(c) => ConditionAst::And(
                            Box::new(ConditionAst::IsYourTurn),
                            Box::new(c.clone()),
                        ),
                        None => ConditionAst::IsYourTurn,
                    };
                    Some(&own_turn_cond)
                } else {
                    condition.as_deref()
                };
                if let Some(res) = try_lower_play_or_cast_with_alt_cost(
                    base_perm,
                    *cast_only,
                    filter,
                    effective_cond,
                    true,
                    pay_life_mv,
                    program,
                    vocabulary,
                    ids,
                ) {
                    return res;
                }
            }
            if !is_spell
                && let Some((mut perm, rem_cond)) = try_lower_library_top_filtered_permission(
                    &p_low,
                    *cast_only,
                    filter,
                    true,
                    condition.as_deref(),
                    vocabulary,
                )
            {
                if force_own_turn_only
                    && let CastPermissionRule::PlayLibraryTopFiltered {
                        ref mut own_turn_only,
                        ..
                    } = perm
                {
                    *own_turn_only = true;
                }
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: perm,
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            if let Some((perm, rem_cond)) = try_lower_graveyard_matching_permission_ext(
                &p_low,
                *cast_only,
                filter,
                true,
                condition.as_deref(),
                vocabulary,
            ) {
                let perm = if force_own_turn_only {
                    match perm {
                        CastPermissionRule::GraveyardMatching {
                            lands_allowed,
                            filter,
                            once_each_turn,
                        } => CastPermissionRule::GraveyardMatchingOwnTurn {
                            lands_allowed,
                            filter,
                            once_each_turn,
                        },
                        other => other,
                    }
                } else {
                    perm
                };
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: perm,
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            if let Some((perm, rem_cond)) = try_lower_exile_matching_permission_ext(
                &p_low,
                *cast_only,
                filter,
                true,
                condition.as_deref(),
                vocabulary,
            ) {
                let perm = if force_own_turn_only {
                    match perm {
                        CastPermissionRule::ExileMatchingExt {
                            filter,
                            linked_to_source,
                            free,
                            any_color,
                            lands_allowed,
                            once_each_turn,
                            ..
                        } => CastPermissionRule::ExileMatchingExt {
                            filter,
                            linked_to_source,
                            free,
                            any_color,
                            lands_allowed,
                            once_each_turn,
                            own_turn_only: true,
                        },
                        CastPermissionRule::ExileMatching {
                            filter,
                            linked_to_source,
                            free,
                            any_color,
                        } => CastPermissionRule::ExileMatchingExt {
                            filter,
                            linked_to_source,
                            free,
                            any_color,
                            lands_allowed: false,
                            once_each_turn: true,
                            own_turn_only: true,
                        },
                        other => other,
                    }
                } else {
                    perm
                };
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CastPermission {
                        id: ids.static_id(),
                        permission: perm,
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            Err("oracle:unsupported-static")
        }
        StaticAbilityAst::AlternativeCastCost {
            cost,
            condition,
            for_spells: Some(spell_filter),
            next_only: false,
        } if !is_spell => {
            let is_free_cost = cost.is_empty()
                || matches!(
                    cost.as_slice(),
                    [CostAtomAst::PayMana(raw)]
                        if parse_fixed_mana_cost(&raw.to_ascii_uppercase())
                            == Some(ManaCost::free())
                );
            if is_free_cost
                && !force_own_turn_only
                && spell_filter.color_arity == Some(ColorArity::Colorless)
                && spell_filter.zone == Some(ZoneFilter::YourHand)
                && spell_filter.any_types.is_empty()
                && matches!(
                    spell_filter.controller,
                    None | Some(ControllerRelation::You)
                )
                && filter_only_has(
                    spell_filter,
                    FilterAllow {
                        color_arity: true,
                        zone: true,
                        controller: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CostModifier {
                        id: ids.static_id(),
                        modifier: SpellCostModifier::FreeColorlessHandSpellOnce,
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if let [CostAtomAst::PayLife(val)] = cost.as_slice()
                && matches!(
                    val.as_ref(),
                    ValueExprAst::ObjectStat {
                        stat: StatKind::ManaValue,
                        ..
                    }
                )
                && matches!(
                    spell_filter.controller,
                    None | Some(ControllerRelation::You)
                )
            {
                if spell_filter.any_types == AstCardTypeMask::ENCHANTMENT
                    && filter_only_has(
                        spell_filter,
                        FilterAllow {
                            types: true,
                            controller: true,
                            ..FilterAllow::NONE
                        },
                    )
                {
                    let (_, rem_cond) = split_is_your_turn_condition(condition.as_deref());
                    return push_conditional_static(
                        program,
                        StaticContinuousAbility::CostModifier {
                            id: ids.static_id(),
                            modifier: SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce,
                        },
                        if force_own_turn_only {
                            rem_cond
                        } else {
                            condition.as_deref()
                        },
                        vocabulary,
                    );
                }
                let mut stripped = spell_filter.clone();
                stripped.controller = None;
                let sf = lower_spell_filter(&stripped, vocabulary)?;
                if spell_filter_supported_by_cast_alt_cost(&sf) {
                    let (cond_own_turn, rem_cond) =
                        split_is_your_turn_condition(condition.as_deref());
                    return push_conditional_static(
                        program,
                        StaticContinuousAbility::CostModifier {
                            id: ids.static_id(),
                            modifier: SpellCostModifier::AlternativeForMatching {
                                filter: sf,
                                mana: ManaCost::free(),
                                once_each_turn: true,
                                own_turn_only: force_own_turn_only || cond_own_turn,
                                pay_life_mv: true,
                                each_player: false,
                            },
                        },
                        rem_cond,
                        vocabulary,
                    );
                }
            }
            let alt_mana = if is_free_cost {
                Some(ManaCost::free())
            } else {
                extract_fixed_alternative_mana_cost(cost)
            };
            if let Some(mana) = alt_mana
                && matches!(
                    spell_filter.controller,
                    None | Some(ControllerRelation::You)
                )
            {
                let mut stripped = spell_filter.as_ref().clone();
                stripped.controller = None;
                let sf = lower_spell_filter(&stripped, vocabulary)?;
                let (own_turn_only, rem_cond) = split_is_your_turn_condition(condition.as_deref());
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CostModifier {
                        id: ids.static_id(),
                        modifier: SpellCostModifier::AlternativeForMatching {
                            filter: sf,
                            mana,
                            once_each_turn: true,
                            own_turn_only: own_turn_only || force_own_turn_only,
                            pay_life_mv: false,
                            each_player: false,
                        },
                    },
                    rem_cond,
                    vocabulary,
                );
            }
            Err("oracle:unsupported-static")
        }
        _ => Err("oracle:unsupported-static"),
    }
}

struct DamageModifySpec {
    source: Option<ObjectFilter>,
    source_controller: PlayerRelation,
    kind: StaticDamageKind,
    op: StaticDamageOp,
}

fn push_damage_modify_rules(
    spec: DamageModifySpec,
    recipients: &[StaticDamageRecipient],
    condition: Option<&ConditionAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if recipients.is_empty() {
        return Err("oracle:unsupported-static");
    }
    let cond_sc = match condition {
        Some(cond) => Some(
            lower_static_source_condition_ext(cond, vocabulary)
                .ok_or("oracle:unsupported-static-cond")?,
        ),
        None => None,
    };
    for recipient in recipients {
        let rule = match &cond_sc {
            Some(sc) => StaticDamageRule::ConditionalModifyDamage {
                condition: Box::new(sc.clone()),
                source: spec.source,
                source_controller: spec.source_controller,
                recipient: *recipient,
                kind: spec.kind,
                op: spec.op.clone(),
            },
            None => StaticDamageRule::ModifyDamage {
                source: spec.source,
                source_controller: spec.source_controller,
                recipient: *recipient,
                kind: spec.kind,
                op: spec.op.clone(),
            },
        };
        push_static_ability(
            program,
            StaticContinuousAbility::DamageRule {
                id: ids.static_id(),
                rule,
            },
        )?;
    }
    Ok(())
}

fn validate_inline_damage_followup_body(
    body: &ExecutableBody,
    vocabulary: &CreatureTypeVocabulary,
) -> bool {
    StaticContinuousAbility::DamageRule {
        id: StaticAbilityId(0),
        rule: StaticDamageRule::ModifyDamage {
            source: None,
            source_controller: PlayerRelation::AnyParticipant,
            recipient: StaticDamageRecipient::SelfObject,
            kind: StaticDamageKind::Any,
            op: StaticDamageOp::PreventAndFollowup {
                mode: DamagePreventMode::All,
                followup: DamagePreventFollowup::InlineBody(Box::new(body.clone())),
            },
        },
    }
    .validate(vocabulary)
    .is_ok()
}

fn lower_prevent_and_replace_damage(
    target: &SubjectRefAst,
    follow_up: &[InstructionAst],
    condition: Option<&ConditionAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if follow_up.is_empty() {
        return Err("oracle:unsupported-static");
    }
    let is_self_subj = |s: &SubjectRefAst| match s {
        SubjectRefAst::SelfObject => true,
        SubjectRefAst::Anaphora(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "~" | "it" | "this creature" | "this permanent"
        ),
        _ => false,
    };

    if let [
        InstructionAst::RemoveCounters {
            subject,
            counter_kind,
            count,
            all: false,
        },
    ] = follow_up
        && is_self_subj(subject.as_ref())
        && let Some(kind) = parse_counter_kind(counter_kind).filter(|k| k.is_permanent())
    {
        if matches!(target, SubjectRefAst::SelfObject)
            && matches!(count.as_deref(), None | Some(ValueExprAst::Const(1)))
            && let Some(ConditionAst::CounterCompare {
                subject: cond_subj,
                counter_kind: cond_ck,
                op: CompareOp::Ge,
                value: 1,
            }) = condition
            && matches!(cond_subj.as_ref(), SubjectRefAst::SelfObject)
            && cond_ck.trim().eq_ignore_ascii_case(counter_kind.trim())
        {
            return push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule: StaticDamageRule::ModifyDamage {
                        source: None,
                        source_controller: PlayerRelation::AnyParticipant,
                        recipient: StaticDamageRecipient::SelfObject,
                        kind: StaticDamageKind::Any,
                        op: StaticDamageOp::PreventAndFollowup {
                            mode: DamagePreventMode::UpToSourceCounters(kind),
                            followup: DamagePreventFollowup::RemoveCountersFromSource {
                                kind,
                                per_prevented: true,
                            },
                        },
                    },
                },
            );
        }
        let per_prevented = match count.as_deref() {
            None | Some(ValueExprAst::Const(1)) => false,
            Some(ValueExprAst::PriorResult) => true,
            _ => return Err("oracle:unsupported-static"),
        };
        if matches!(target, SubjectRefAst::SelfObject)
            && kind == CounterKind::PlusOne
            && condition.is_none()
        {
            let rule = if per_prevented {
                StaticDamageRule::PreventIncomingRemoveNPlusOne
            } else {
                StaticDamageRule::PreventIncomingRemovePlusOne
            };
            return push_static_ability(
                program,
                StaticContinuousAbility::DamageRule {
                    id: ids.static_id(),
                    rule,
                },
            );
        }
        let recipients =
            lower_damage_recipients(target, vocabulary).map_err(|_| "oracle:unsupported-static")?;
        return push_damage_modify_rules(
            DamageModifySpec {
                source: None,
                source_controller: PlayerRelation::AnyParticipant,
                kind: StaticDamageKind::Any,
                op: StaticDamageOp::PreventAndFollowup {
                    mode: DamagePreventMode::All,
                    followup: DamagePreventFollowup::RemoveCountersFromSource {
                        kind,
                        per_prevented,
                    },
                },
            },
            &recipients,
            condition,
            program,
            vocabulary,
            ids,
        );
    }

    if let [
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        },
    ] = follow_up
        && let Some(kind) = parse_counter_kind(counter_kind).filter(|k| k.is_permanent())
    {
        let refers_to_recipient = subject.as_ref() == target
            || matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "it" | "that creature"
                            | "that permanent"
                            | "them"
                            | "~"
                            | "this creature"
                            | "this permanent"
                    )
            );
        if refers_to_recipient {
            let per_prevented = match count.as_ref() {
                ValueExprAst::Const(1) => false,
                ValueExprAst::PriorResult => true,
                _ => return Err("oracle:unsupported-static"),
            };
            let recipients = lower_damage_recipients(target, vocabulary)
                .map_err(|_| "oracle:unsupported-static")?;
            let all_permanent_recipients = recipients.iter().all(|rec| {
                matches!(
                    rec,
                    StaticDamageRecipient::SelfObject
                        | StaticDamageRecipient::AttachedHost
                        | StaticDamageRecipient::Creatures
                        | StaticDamageRecipient::ControlledCreatures
                        | StaticDamageRecipient::OpponentCreatures
                        | StaticDamageRecipient::MatchingPermanent(_)
                )
            });
            if all_permanent_recipients {
                return push_damage_modify_rules(
                    DamageModifySpec {
                        source: None,
                        source_controller: PlayerRelation::AnyParticipant,
                        kind: StaticDamageKind::Any,
                        op: StaticDamageOp::PreventAndFollowup {
                            mode: DamagePreventMode::All,
                            followup: DamagePreventFollowup::PutCountersOnRecipient {
                                kind,
                                per_prevented,
                            },
                        },
                    },
                    &recipients,
                    condition,
                    program,
                    vocabulary,
                    ids,
                );
            }
        }
    }

    let recipients =
        lower_damage_recipients(target, vocabulary).map_err(|_| "oracle:unsupported-static")?;
    let mut body = lower_effect_body(follow_up, vocabulary, ids, false, true)
        .map_err(|_| "oracle:unsupported-static")?;
    rewrite_body_prior_result(&mut body, EventValueKind::DamagePreventedThisWay);
    if !validate_inline_damage_followup_body(&body, vocabulary) {
        return Err("oracle:unsupported-static");
    }
    push_damage_modify_rules(
        DamageModifySpec {
            source: None,
            source_controller: PlayerRelation::AnyParticipant,
            kind: StaticDamageKind::Any,
            op: StaticDamageOp::PreventAndFollowup {
                mode: DamagePreventMode::All,
                followup: DamagePreventFollowup::InlineBody(Box::new(body)),
            },
        },
        &recipients,
        condition,
        program,
        vocabulary,
        ids,
    )
}

fn ast_filter_has_pt_dependency(flt: &ObjectFilterAst) -> bool {
    flt.stat_bounds
        .iter()
        .any(|sb| matches!(sb.stat, StatKind::Power | StatKind::Toughness))
}

fn ast_expr_has_pt_dependency(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ObjectStat {
            stat: StatKind::Power | StatKind::Toughness,
            ..
        }
        | ValueExprAst::AggregateStat {
            stat: StatKind::Power | StatKind::Toughness,
            ..
        } => true,
        ValueExprAst::Count(flt)
        | ValueExprAst::AggregateStat { filter: flt, .. }
        | ValueExprAst::ColorsAmong(flt)
        | ValueExprAst::CardTypesAmong(flt)
        | ValueExprAst::SubtypesAmong(flt) => ast_filter_has_pt_dependency(flt),
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::Pow { base: a, exp: b }
        | ValueExprAst::Shortfall {
            threshold: a,
            actual: b,
        } => ast_expr_has_pt_dependency(a) || ast_expr_has_pt_dependency(b),
        ValueExprAst::DivConst { expr: inner, .. }
        | ValueExprAst::AggregateOverPlayers { metric: inner, .. } => {
            ast_expr_has_pt_dependency(inner)
        }
        _ => false,
    }
}

fn object_clause_has_pt_dependency(c: &ObjectClause) -> bool {
    c.power != PowerFilter::Any || c.toughness != ToughnessFilter::Any
}

fn object_filter_has_pt_dependency(f: &ObjectFilter) -> bool {
    match f {
        ObjectFilter::Clause(c) => object_clause_has_pt_dependency(c),
        ObjectFilter::Either(a, b) => {
            object_clause_has_pt_dependency(a) || object_clause_has_pt_dependency(b)
        }
    }
}

fn value_expr_has_pt_dependency(ve: &ValueExpr) -> bool {
    match ve {
        ValueExpr::ObjectStat {
            stat: ObjectStatKind::Power | ObjectStatKind::Toughness,
            ..
        }
        | ValueExpr::Aggregate {
            stat: ObjectStatKind::Power | ObjectStatKind::Toughness,
            ..
        }
        | ValueExpr::PlayerStat(PlayerStatExpr::ControlsGreatestStat { .. }) => true,
        ValueExpr::Count { filter, .. }
        | ValueExpr::OpponentMaxCount { filter, .. }
        | ValueExpr::OpponentMinCount { filter, .. }
        | ValueExpr::DistinctColors { filter }
        | ValueExpr::LastActionMatchingCount(filter)
        | ValueExpr::Aggregate { filter, .. }
        | ValueExpr::ObjectMatches { filter, .. } => object_filter_has_pt_dependency(filter),
        ValueExpr::Scale { expr, .. } | ValueExpr::DivConst { expr, .. } => {
            value_expr_has_pt_dependency(expr)
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            value_expr_has_pt_dependency(a) || value_expr_has_pt_dependency(b)
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            value_expr_has_pt_dependency(left)
                || value_expr_has_pt_dependency(right)
                || value_expr_has_pt_dependency(then_value)
                || value_expr_has_pt_dependency(else_value)
        }
        _ => false,
    }
}

fn numeric_expr_has_pt_dependency(expr: &NumericExpression) -> bool {
    match expr {
        NumericExpression::ImprintedStat {
            stat: ObjectStatKind::Power | ObjectStatKind::Toughness,
            ..
        }
        | NumericExpression::PlayerStat(StaticPlayerStat::OwnGraveyardGreatestCreaturePower) => {
            true
        }
        NumericExpression::Count { selector, .. } => {
            selector.min_power.is_some()
                || selector.max_power.is_some()
                || selector.min_toughness.is_some()
                || selector.max_toughness.is_some()
        }
        NumericExpression::Eval(ve) => value_expr_has_pt_dependency(ve),
        NumericExpression::Add(lhs, rhs) => {
            numeric_expr_has_pt_dependency(lhs) || numeric_expr_has_pt_dependency(rhs)
        }
        NumericExpression::Scale { expr, .. } | NumericExpression::Cap { expr, .. } => {
            numeric_expr_has_pt_dependency(expr)
        }
        _ => false,
    }
}

fn selector_guarantees_creature(sel: &PermanentSelector, program: &Program) -> bool {
    if sel.required_types.contains(&CardType::Creature)
        || (sel.any_types.len() == 1 && sel.any_types.contains(&CardType::Creature))
    {
        return true;
    }
    if sel.controller == ControllerSelector::SourceItself && program.has_type(CardType::Creature) {
        return true;
    }
    if sel.attached_to_source {
        return match program.permanent().and_then(|p| p.attachment) {
            Some(
                AttachmentKind::Equipment
                | AttachmentKind::Aura(
                    EnchantRestriction::Creature
                    | EnchantRestriction::NonblackCreature
                    | EnchantRestriction::CreatureYouControl
                    | EnchantRestriction::CreatureAnOpponentControls,
                ),
            ) => true,
            Some(AttachmentKind::Aura(EnchantRestriction::Matching(filter))) => {
                filter.guarantees_creature()
            }
            _ => false,
        };
    }
    false
}

fn program_has_creature_animation_static(program: &Program) -> bool {
    program
        .continuous
        .iter()
        .chain(&program.additional_battlefield_abilities)
        .any(|ability| {
            let StaticContinuousAbility::Battlefield { parts, .. } = ability else {
                return false;
            };
            parts.0.iter().any(|part| match part {
                StaticPart::Type(stc) | StaticPart::TypeWithSubtypes { base: stc, .. } => {
                    matches!(
                        &stc.types,
                        TypeOperation::Add(set) | TypeOperation::Set(set)
                            if set.contains(&CardType::Creature)
                    )
                }
                _ => false,
            })
        })
}

fn lower_dynamic_base_pt(
    subject: &SubjectRefAst,
    power: &ValueExprAst,
    toughness: &ValueExprAst,
    condition: Option<&ConditionAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    let is_own_mana_value = |expr: &ValueExprAst| -> bool {
        let ValueExprAst::ObjectStat {
            subject: stat_subj,
            stat: StatKind::ManaValue,
        } = expr
        else {
            return false;
        };
        match stat_subj.as_ref() {
            SubjectRefAst::SelfObject => matches!(subject, SubjectRefAst::SelfObject),
            SubjectRefAst::Anaphora(raw) => matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "it" | "its"
                    | "each"
                    | "that creature"
                    | "that permanent"
                    | "that artifact"
                    | "that enchantment"
                    | "that land"
                    | "~"
            ),
            _ => false,
        }
    };

    let mut selectors =
        lower_static_permanent_selectors_with_context(subject, condition, vocabulary)?;
    for sel in &mut selectors {
        if let Some(cond) = condition {
            apply_static_selector_condition(sel, cond, vocabulary)?;
        }
    }

    if is_own_mana_value(power) && is_own_mana_value(toughness) {
        // Stay fail-closed on lossy Stage 1 ASTs (such as Opalescence / Starfield of Nyx)
        // where "is a creature in addition to its other types" was dropped from a non-creature subject.
        let targets_creatures = selectors
            .iter()
            .all(|sel| selector_guarantees_creature(sel, program))
            || program_has_creature_animation_static(program);
        if !targets_creatures {
            return Err("oracle:unsupported-static");
        }
        for selector in selectors {
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ids.static_id(),
                    selector,
                    parts: StaticParts(vec![StaticPart::SetPTToOwnManaValue]),
                },
            )?;
        }
        return Ok(());
    }

    // CR 613.8: keep Layer 7b base P/T dependencies on Layer 7 power/toughness fail-closed.
    if ast_expr_has_pt_dependency(power) || ast_expr_has_pt_dependency(toughness) {
        return Err("oracle:unsupported-static");
    }
    let expr_ctx = StaticExprContext::from_subject_and_program(subject, program);
    let p_expr = lower_numeric_expr_with_context(power, expr_ctx, vocabulary)?;
    let t_expr = lower_numeric_expr_with_context(toughness, expr_ctx, vocabulary)?;
    if numeric_expr_has_pt_dependency(&p_expr) || numeric_expr_has_pt_dependency(&t_expr) {
        return Err("oracle:unsupported-static");
    }
    for selector in selectors {
        push_static_ability(
            program,
            StaticContinuousAbility::Battlefield {
                id: ids.static_id(),
                selector,
                parts: StaticParts(vec![StaticPart::SetPT(NumericPair {
                    power: p_expr.clone(),
                    toughness: t_expr.clone(),
                })]),
            },
        )?;
    }
    Ok(())
}

fn lower_alternative_cast_cost(
    cost: &[CostAtomAst],
    condition: Option<&ConditionAst>,
    for_spells: Option<&ObjectFilterAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    is_spell: bool,
) -> Result<(), &'static str> {
    // Case 1: alternative cost for matching spells you cast (e.g. Rooftop Storm).
    if let Some(spell_filter) = for_spells {
        if is_spell {
            return Err("oracle:unsupported-alternative-cast-cost");
        }
        let is_free_cost = cost.is_empty()
            || matches!(
                cost,
                [CostAtomAst::PayMana(raw)]
                    if parse_fixed_mana_cost(&raw.to_ascii_uppercase())
                        == Some(ManaCost::free())
            );
        if is_free_cost
            && spell_filter.any_types == AstCardTypeMask::CREATURE
            && spell_filter.subtypes.len() == 1
            && spell_filter.controller == Some(ControllerRelation::You)
            && filter_only_has(
                spell_filter,
                FilterAllow {
                    types: true,
                    subtypes: true,
                    controller: true,
                    ..FilterAllow::NONE
                },
            )
            && let Some(ct) = vocabulary.resolve_label(&spell_filter.subtypes[0])
        {
            return push_conditional_static(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ids.static_id(),
                    modifier: SpellCostModifier::FreeSubtypeCreatureSpells(ct),
                },
                condition,
                vocabulary,
            );
        }
        if let [CostAtomAst::PayLife(val)] = cost
            && matches!(
                val.as_ref(),
                ValueExprAst::ObjectStat {
                    stat: StatKind::ManaValue,
                    ..
                }
            )
            && matches!(
                spell_filter.controller,
                None | Some(ControllerRelation::You)
            )
        {
            let mut stripped = spell_filter.clone();
            stripped.controller = None;
            let sf = lower_spell_filter(&stripped, vocabulary)?;
            if spell_filter_supported_by_cast_alt_cost(&sf) {
                let (own_turn_only, rem_cond) = split_is_your_turn_condition(condition);
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::CostModifier {
                        id: ids.static_id(),
                        modifier: SpellCostModifier::AlternativeForMatching {
                            filter: sf,
                            mana: ManaCost::free(),
                            once_each_turn: false,
                            own_turn_only,
                            pay_life_mv: true,
                            each_player: false,
                        },
                    },
                    rem_cond,
                    vocabulary,
                );
            }
        }
        if let Some(mana) = extract_fixed_alternative_mana_cost(cost)
            && matches!(
                spell_filter.controller,
                None | Some(ControllerRelation::You)
            )
        {
            let mut stripped = spell_filter.clone();
            stripped.controller = None;
            let sf = lower_spell_filter(&stripped, vocabulary)?;
            let (own_turn_only, rem_cond) = split_is_your_turn_condition(condition);
            return push_conditional_static(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ids.static_id(),
                    modifier: SpellCostModifier::AlternativeForMatching {
                        filter: sf,
                        mana,
                        once_each_turn: false,
                        own_turn_only,
                        pay_life_mv: false,
                        each_player: false,
                    },
                },
                rem_cond,
                vocabulary,
            );
        }
        return Err("oracle:unsupported-alternative-cast-cost");
    }

    if program.conditional_cost.is_some() || program.has_type(CardType::Land) {
        return Err("oracle:unsupported-alternative-cast-cost");
    }

    // Case 2: self alternative cast cost gated by a condition.
    if let Some(cond) = condition {
        let pure_mana = match cost {
            [] => Some(ManaCost::free()),
            [CostAtomAst::PayMana(raw)] => parse_fixed_mana_cost(&raw.to_ascii_uppercase()),
            _ => None,
        };
        if let Some(mana) = pure_mana {
            if matches!(cond, ConditionAst::ControlsCommander) {
                program.conditional_cost = Some(ConditionalCastCost::ControlsCommander(mana));
                return Ok(());
            }
            let mut dummy = BodyBuilder::new();
            let rc = lower_resolution_condition(cond, &mut dummy, vocabulary, false, None)
                .map_err(|_| "oracle:unsupported-alternative-cast-cost")?;
            if dummy.target != BodyTarget::None
                || !rc.is_world_evaluable()
                || rc.validate_world(vocabulary).is_err()
            {
                return Err("oracle:unsupported-alternative-cast-cost");
            }
            program.conditional_cost = Some(ConditionalCastCost::Condition {
                condition: Box::new(rc),
                mana,
            });
            return Ok(());
        }
        return lower_general_self_alternative_cast_cost(cost, Some(cond), program, vocabulary);
    }

    // Case 3a: unconditional fixed mana alternative cast cost.
    if let [CostAtomAst::PayMana(raw)] = cost {
        let mana = parse_fixed_mana_cost(&raw.to_ascii_uppercase())
            .ok_or("oracle:unsupported-alternative-cast-cost")?;
        program.conditional_cost = Some(ConditionalCastCost::Condition {
            condition: Box::new(ResolutionCondition {
                left: ValueExpr::literal(1),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            }),
            mana,
        });
        return Ok(());
    }

    // Case 3b: unconditional pitch / exile cards from hand (+ optional fixed mana).
    let exile_opt = match cost {
        [
            CostAtomAst::Exile {
                count,
                filter,
                face_down: false,
                face_up: false,
                single_graveyard: false,
                any_number: false,
                min_one: false,
                total_mv: None,
            },
        ] => Some((ManaCost::free(), count.as_ref(), filter.as_ref())),
        [
            CostAtomAst::PayMana(raw),
            CostAtomAst::Exile {
                count,
                filter,
                face_down: false,
                face_up: false,
                single_graveyard: false,
                any_number: false,
                min_one: false,
                total_mv: None,
            },
        ]
        | [
            CostAtomAst::Exile {
                count,
                filter,
                face_down: false,
                face_up: false,
                single_graveyard: false,
                any_number: false,
                min_one: false,
                total_mv: None,
            },
            CostAtomAst::PayMana(raw),
        ] => parse_fixed_mana_cost(&raw.to_ascii_uppercase())
            .map(|mana| (mana, count.as_ref(), filter.as_ref())),
        _ => None,
    };
    if let Some((mana, count_expr, exile_filter)) = exile_opt
        && let ValueExprAst::Const(n) = count_expr
        && *n > 0
        && let Ok(count) = u8::try_from(*n)
    {
        let single_color = match exile_filter.any_colors {
            m if m == AstColorMask::WHITE => Some(Color::White),
            m if m == AstColorMask::BLUE => Some(Color::Blue),
            m if m == AstColorMask::BLACK => Some(Color::Black),
            m if m == AstColorMask::RED => Some(Color::Red),
            m if m == AstColorMask::GREEN => Some(Color::Green),
            _ => None,
        };
        let allowed_flags = AstFilterFlags::EXCLUDE_SELF.0 | AstFilterFlags::LKI_PAST_TENSE.0;
        if exile_filter.zone == Some(ZoneFilter::YourHand)
            && !exile_filter.any_permanent
            && (exile_filter.flags.0 & !allowed_flags) == 0
            && filter_only_has(
                exile_filter,
                FilterAllow {
                    colors: true,
                    zone: true,
                    flags: true,
                    ..FilterAllow::NONE
                },
            )
            && let Some(color) = single_color
        {
            program.conditional_cost =
                Some(ConditionalCastCost::ExileHandCards { mana, count, color });
            return Ok(());
        }
    }

    // Case 3c: unconditional TapPermanents with exact CastTapQuality (Zahid, Sephara).
    let tap_opt = match cost {
        [
            CostAtomAst::TapPermanent {
                count,
                filter,
                any_number: false,
            },
        ] => Some((ManaCost::free(), count.as_ref(), filter.as_ref())),
        [
            CostAtomAst::PayMana(raw),
            CostAtomAst::TapPermanent {
                count,
                filter,
                any_number: false,
            },
        ]
        | [
            CostAtomAst::TapPermanent {
                count,
                filter,
                any_number: false,
            },
            CostAtomAst::PayMana(raw),
        ] => parse_fixed_mana_cost(&raw.to_ascii_uppercase())
            .map(|mana| (mana, count.as_ref(), filter.as_ref())),
        _ => None,
    };
    if let Some((mana, tap_count, tap_filter)) = tap_opt
        && let ValueExprAst::Const(n) = tap_count
        && *n > 0
        && let Ok(count) = u8::try_from(*n)
        && tap_filter.tapped == Some(false)
        && matches!(tap_filter.controller, None | Some(ControllerRelation::You))
    {
        let mut cleaned = tap_filter.clone();
        cleaned.tapped = None;
        let quality = if cleaned.any_types == AstCardTypeMask::ARTIFACT
            && filter_only_has(
                &cleaned,
                FilterAllow {
                    types: true,
                    controller: true,
                    ..FilterAllow::NONE
                },
            ) {
            Some(spellwake_engine::program::CastTapQuality::Artifact)
        } else if cleaned.any_types == AstCardTypeMask::CREATURE
            && cleaned.required_keywords.len() == 1
            && cleaned.required_keywords[0].eq_ignore_ascii_case("flying")
            && filter_only_has(
                &cleaned,
                FilterAllow {
                    types: true,
                    controller: true,
                    req_keywords: true,
                    ..FilterAllow::NONE
                },
            )
        {
            Some(spellwake_engine::program::CastTapQuality::FlyingCreature)
        } else {
            None
        };
        if let Some(quality) = quality {
            program.conditional_cost = Some(ConditionalCastCost::TapPermanents {
                mana,
                count,
                quality,
            });
            return Ok(());
        }
    }

    // Case 3d: generalized alternative cast cost (optional mana + extra activation costs).
    lower_general_self_alternative_cast_cost(cost, None, program, vocabulary)
}

fn is_look_at_source_exiled_static(st: &StaticAbilityAst) -> bool {
    match st {
        StaticAbilityAst::LookOrRevealRule {
            rule,
            filter,
            follow_up,
            ..
        } if follow_up.is_empty() => {
            let r_low = rule
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "you may look at cards exiled with ~"
                    | "you may look at face-down cards exiled with ~"
                    | "you may look at the cards exiled with ~"
                    | "you may look at the face-down cards exiled with ~"
            ) || (r_low == "look_any_time"
                && filter.as_deref().is_some_and(|flt| {
                    matches!(flt.zone, None | Some(ZoneFilter::Exile))
                        && (flt.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                }))
        }
        StaticAbilityAst::RuleOrCombatRestriction {
            subject: None,
            rule,
            filter: None,
            ..
        } => {
            let r_low = rule
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_ascii_lowercase();
            matches!(
                r_low.as_str(),
                "you may look at cards exiled with ~"
                    | "you may look at face-down cards exiled with ~"
                    | "you may look at the cards exiled with ~"
                    | "you may look at the face-down cards exiled with ~"
            )
        }
        _ => false,
    }
}

fn try_lower_compound_exile_play_replacement(
    first_ins: &[InstructionAst],
    next_ins: &[InstructionAst],
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<Result<(), &'static str>> {
    use spellwake_engine::characteristics::{ReplaceableEvent, StaticReplacementAction};
    use spellwake_engine::program::ImpulsePlayDuration;

    let [
        InstructionAst::TemporaryReplacement {
            duration: DurationAst::Instant | DurationAst::Permanent,
            replacement,
        },
    ] = first_ins
    else {
        return None;
    };
    let ReplacementEffectAst::EventInstead {
        subject_or_event,
        would,
        instead,
        instead_body,
    } = replacement.as_ref()
    else {
        return None;
    };
    if !subject_or_event.trim().eq_ignore_ascii_case("you") {
        return None;
    }
    let would_raw = would
        .trim()
        .trim_end_matches(',')
        .trim()
        .to_ascii_lowercase();
    let would_norm = would_raw
        .strip_prefix("would ")
        .unwrap_or(&would_raw)
        .trim();
    if would_norm != "draw a card" {
        return None;
    }

    let grant_ast = match next_ins {
        [g @ InstructionAst::GrantCastPermission { .. }] => g,
        [InstructionAst::Optional(inner)] if inner.len() == 1 => &inner[0],
        _ => return None,
    };
    let InstructionAst::GrantCastPermission {
        player: PlayerRefAst::You,
        cast_only,
        subject,
        free,
        duration,
        count_limit: None,
        alt_cost: None,
    } = grant_ast
    else {
        return None;
    };
    let SubjectRefAst::Anaphora(anaph) = subject.as_ref() else {
        return None;
    };
    if !matches!(
        anaph.trim().to_ascii_lowercase().as_str(),
        "it" | "them"
            | "that card"
            | "those cards"
            | "the exiled card"
            | "the exiled cards"
            | "cards exiled this way"
            | "a card exiled this way"
    ) {
        return None;
    }
    let dur_low = duration.trim().to_ascii_lowercase();
    let impulse_dur = if dur_low == "until_end_of_next_turn"
        || dur_low.contains("end of your next turn")
        || dur_low.contains("untilendofnextturn")
        || dur_low.contains("untilyournextturn")
    {
        ImpulsePlayDuration::UntilEndOfNextTurn
    } else if dur_low == "this_turn"
        || dur_low == "until_end_of_turn"
        || dur_low.contains("this turn")
        || dur_low.contains("end of turn")
        || dur_low.contains("untilendofturn")
    {
        ImpulsePlayDuration::ThisTurn
    } else if dur_low == "while_exiled"
        || dur_low.contains("remains exiled")
        || dur_low.contains("remain exiled")
        || dur_low == "permanent"
    {
        ImpulsePlayDuration::WhileExiled
    } else {
        return None;
    };

    let parsed_instead_owned;
    let exile_slice: &[InstructionAst] = if let Some(body_ins) = instead_body.as_deref() {
        match body_ins {
            [InstructionAst::ReplaceEffect(inner)] => inner.as_slice(),
            other => other,
        }
    } else {
        let cleaned = instead
            .trim()
            .trim_end_matches('.')
            .trim()
            .to_ascii_lowercase();
        let cleaned = cleaned
            .strip_prefix("instead, ")
            .or_else(|| cleaned.strip_prefix("instead "))
            .unwrap_or(&cleaned)
            .trim_end_matches(" instead")
            .trim();
        let (ins, fails) = spellwake_oracle_compiler::clauses::parse_effect_body(
            cleaned,
            &cached_compiler_vocab(vocabulary),
        );
        if !fails.is_empty() || ins.is_empty() {
            return None;
        }
        parsed_instead_owned = ins;
        &parsed_instead_owned
    };
    let [
        InstructionAst::ExileTopLibrary {
            actor: None | Some(PlayerRefAst::You),
            whose,
            count,
            face_down,
        },
    ] = exile_slice
    else {
        return None;
    };
    if !matches!(
        whose.trim().to_ascii_lowercase().as_str(),
        "your" | "own" | "" | "your library"
    ) {
        return None;
    }
    let cnt = lower_value_expr(count, vocabulary).ok()?;
    if !cnt.is_world_evaluable() {
        return None;
    }
    let body = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::ExileTopPlay {
            count: cnt,
            library_owner: AffectedPlayers::One(ResolvingPlayer::Controller),
            duration: impulse_dur,
            cast_only: *cast_only,
            free: *free,
            any_color: false,
            face_down: *face_down,
            grant_play: true,
        }],
    );
    let ability = StaticContinuousAbility::EventReplacement {
        id: ids.static_id(),
        event: ReplaceableEvent::Draw {
            player: AffectedPlayers::One(ResolvingPlayer::Controller),
        },
        condition: None,
        action: StaticReplacementAction::ReplaceWithBody {
            optional: false,
            body: Box::new(body),
        },
    };
    if ability.validate(vocabulary).is_err() {
        return None;
    }
    Some(push_static_ability(program, ability))
}

fn lower_followup_instructions(
    ins: &[InstructionAst],
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if ins.is_empty() {
        return Err("oracle:unsupported-static");
    }
    let mut probe = program.clone();
    for op in ins {
        match op {
            InstructionAst::TemporaryReplacement {
                duration: DurationAst::Permanent | DurationAst::Instant,
                replacement,
            } => {
                lower_replacement(replacement, &mut probe, vocabulary, ids)?;
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } if !then_body.is_empty()
                && then_body.iter().all(|inner| {
                    matches!(
                        inner,
                        InstructionAst::TemporaryReplacement {
                            duration: DurationAst::Permanent | DurationAst::Instant,
                            ..
                        }
                    )
                }) =>
            {
                for inner in then_body {
                    if let InstructionAst::TemporaryReplacement { replacement, .. } = inner {
                        lower_conditional_replacement(
                            condition,
                            replacement,
                            &mut probe,
                            vocabulary,
                            ids,
                        )?;
                    }
                }
            }
            _ => return Err("oracle:unsupported-static"),
        }
    }
    *program = probe;
    Ok(())
}

fn lower_conditional_replacement(
    condition: &ConditionAst,
    replacement: &ReplacementEffectAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    match replacement {
        ReplacementEffectAst::EntersWithCounters {
            counter_kind,
            count,
            per,
            x_expr,
            condition: existing,
        } => {
            let merged = match existing {
                Some(prev) => ConditionAst::And(prev.clone(), Box::new(condition.clone())),
                None => condition.clone(),
            };
            let conditioned = ReplacementEffectAst::EntersWithCounters {
                counter_kind: counter_kind.clone(),
                count: count.clone(),
                per: per.clone(),
                x_expr: x_expr.clone(),
                condition: Some(Box::new(merged)),
            };
            lower_replacement(&conditioned, program, vocabulary, ids)
        }
        ReplacementEffectAst::EntersTapped { unless: None } => {
            let conditioned = ReplacementEffectAst::EntersTapped {
                unless: Some(Box::new(ConditionAst::Not(Box::new(condition.clone())))),
            };
            lower_replacement(&conditioned, program, vocabulary, ids)
        }
        _ => {
            let sc = lower_static_source_condition_ext(condition, vocabulary)
                .ok_or("oracle:unsupported-static")?;
            let prev_cont_id = program.continuous.as_ref().map(StaticContinuousAbility::id);
            let prev_add_len = program.additional_battlefield_abilities.len();
            let mut probe = program.clone();
            lower_replacement(replacement, &mut probe, vocabulary, ids)?;
            let mut baseline = probe.clone();
            baseline.continuous = program.continuous.clone();
            baseline.additional_battlefield_abilities =
                program.additional_battlefield_abilities.clone();
            if baseline != *program {
                return Err("oracle:unsupported-static");
            }
            let mut pushed_any = false;
            if let Some(cur) = probe.continuous.as_ref()
                && Some(cur.id()) != prev_cont_id
            {
                probe.static_conditions.insert(cur.id(), sc.clone());
                pushed_any = true;
            }
            for ab in &probe.additional_battlefield_abilities[prev_add_len..] {
                probe.static_conditions.insert(ab.id(), sc.clone());
                pushed_any = true;
            }
            if !pushed_any {
                return Err("oracle:unsupported-static");
            }
            *program = probe;
            Ok(())
        }
    }
}

fn push_attack_requirement(program: &mut Program, req: AttackRequirement) {
    if program.attack_requirement.is_none() {
        program.attack_requirement = Some(req);
    } else if program.attack_requirement != Some(req)
        && !program.additional_attack_requirements.contains(&req)
    {
        program.additional_attack_requirements.push(req);
    }
}

fn lower_static_source_condition_ext(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticSourceCondition> {
    if let Some(sc) = lower_static_source_condition(cond, vocabulary) {
        return Some(sc);
    }
    if let Some(you_control_host) = attached_host_control_polarity(cond) {
        let host_you = StaticSourceCondition::AttachedHostMatches(Box::new(PermanentSelector {
            controller: ControllerSelector::SourceController,
            ..PermanentSelector::default()
        }));
        return Some(if you_control_host {
            host_you
        } else {
            StaticSourceCondition::Not(Box::new(host_you))
        });
    }
    match cond {
        ConditionAst::Not(inner) => Some(StaticSourceCondition::Not(Box::new(
            lower_static_source_condition_ext(inner, vocabulary)?,
        ))),
        ConditionAst::And(a, b) => Some(StaticSourceCondition::And(
            Box::new(lower_static_source_condition_ext(a, vocabulary)?),
            Box::new(lower_static_source_condition_ext(b, vocabulary)?),
        )),
        ConditionAst::Or(a, b) => Some(StaticSourceCondition::Or(
            Box::new(lower_static_source_condition_ext(a, vocabulary)?),
            Box::new(lower_static_source_condition_ext(b, vocabulary)?),
        )),
        ConditionAst::IsPhaseOrStep {
            player:
                Some(
                    p @ (PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::EachOtherPlayer
                    | PlayerRefAst::AnyPlayer
                    | PlayerRefAst::EachPlayer),
                ),
            phase,
            first_only: false,
        } => {
            use spellwake_engine::program::PhaseCheck;
            let ph = phase
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .replace('_', " ");
            let stat = match ph.as_str() {
                "untap" | "untap step" => Some(PlayerStatExpr::IsInStep {
                    step: Step::Untap,
                    player: None,
                }),
                "upkeep" | "upkeep step" => Some(PlayerStatExpr::IsInStep {
                    step: Step::Upkeep,
                    player: None,
                }),
                "draw" | "draw step" => Some(PlayerStatExpr::IsInStep {
                    step: Step::Draw,
                    player: None,
                }),
                "precombat main" | "precombat main phase" | "first main phase" => {
                    Some(PlayerStatExpr::IsInStep {
                        step: Step::PrecombatMain,
                        player: None,
                    })
                }
                "beginning of combat" | "beginning of combat step" => {
                    Some(PlayerStatExpr::IsInStep {
                        step: Step::BeginningCombat,
                        player: None,
                    })
                }
                "declare attackers" | "declare attackers step" => Some(PlayerStatExpr::IsInStep {
                    step: Step::DeclareAttackers,
                    player: None,
                }),
                "declare blockers" | "declare blockers step" => Some(PlayerStatExpr::IsInStep {
                    step: Step::DeclareBlockers,
                    player: None,
                }),
                "combat damage" | "combat damage step" => Some(PlayerStatExpr::IsInStep {
                    step: Step::CombatDamage,
                    player: None,
                }),
                "end of combat" | "end of combat step" => Some(PlayerStatExpr::IsInStep {
                    step: Step::EndCombat,
                    player: None,
                }),
                "postcombat main" | "postcombat main phase" | "second main phase" => {
                    Some(PlayerStatExpr::IsInStep {
                        step: Step::PostcombatMain,
                        player: None,
                    })
                }
                "end" | "end step" => Some(PlayerStatExpr::IsInStep {
                    step: Step::End,
                    player: None,
                }),
                "cleanup" | "cleanup step" => Some(PlayerStatExpr::IsInStep {
                    step: Step::Cleanup,
                    player: None,
                }),
                "beginning" | "beginning phase" => Some(PlayerStatExpr::IsInPhase {
                    phase: PhaseCheck::Beginning,
                    player: None,
                }),
                "main" | "main phase" => Some(PlayerStatExpr::IsInPhase {
                    phase: PhaseCheck::Main,
                    player: None,
                }),
                "combat" | "combat phase" => Some(PlayerStatExpr::IsInPhase {
                    phase: PhaseCheck::Combat,
                    player: None,
                }),
                "ending" | "ending phase" => Some(PlayerStatExpr::IsInPhase {
                    phase: PhaseCheck::Ending,
                    player: None,
                }),
                _ => None,
            }?;
            let rc = ResolutionCondition {
                left: ValueExpr::PlayerStat(stat),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            };
            if !rc.is_world_evaluable() || rc.validate_world(vocabulary).is_err() {
                return None;
            }
            let step_cond = StaticSourceCondition::Eval(Box::new(rc));
            match p {
                PlayerRefAst::AnyOpponent
                | PlayerRefAst::EachOpponent
                | PlayerRefAst::EachOtherPlayer => Some(StaticSourceCondition::And(
                    Box::new(StaticSourceCondition::OpponentsTurn),
                    Box::new(step_cond),
                )),
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => Some(step_cond),
                _ => None,
            }
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value >= 1
            && matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            && filter.extra_zones.is_empty()
            && matches!(filter.controller, None | Some(ControllerRelation::You)) =>
        {
            let mut f_sel = lower_static_filter_to_permanent_selector(filter, vocabulary).ok()?;
            f_sel.controller = ControllerSelector::SourceController;
            Some(StaticSourceCondition::ControlsMatching {
                selector: Box::new(f_sel),
                minimum: *value,
            })
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            && filter.extra_zones.is_empty()
            && filter.controller.is_none() =>
        {
            let mut f_sel = lower_static_filter_to_permanent_selector(filter, vocabulary).ok()?;
            f_sel.controller = ControllerSelector::Opponents;
            Some(StaticSourceCondition::OpponentControlsMatching {
                selector: Box::new(f_sel),
                minimum: 1,
            })
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op: CompareOp::Ge,
            value: 1,
        } if matches!(
            subject.as_ref(),
            SubjectRefAst::All(_) | SubjectRefAst::Each(_) | SubjectRefAst::Group(_)
        ) =>
        {
            let f = match subject.as_ref() {
                SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => f,
                _ => return None,
            };
            if !matches!(f.zone, None | Some(ZoneFilter::Battlefield)) || !f.extra_zones.is_empty()
            {
                return None;
            }
            let ck_low = counter_kind.trim().to_ascii_lowercase();
            let flag = match ck_low.as_str() {
                "-1/-1" => ObjectFilterFlags::WITH_MINUS_ONE_COUNTER,
                "+1/+1" => ObjectFilterFlags::WITH_PLUS_ONE_COUNTER,
                "" | "any" | "a" | "one or more" => ObjectFilterFlags::WITH_ANY_COUNTER,
                _ => return None,
            };
            let mut inner = f.as_ref().clone();
            inner.zone = None;
            let base = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary).ok()?
            };
            let rc = ResolutionCondition {
                left: ValueExpr::count_battlefield(base.with_flags(flag)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1_i64),
            };
            if rc.is_world_evaluable() && rc.validate_world(vocabulary).is_ok() {
                Some(StaticSourceCondition::Eval(Box::new(rc)))
            } else {
                None
            }
        }
        ConditionAst::ZoneObjectCount { filter, op, value }
            if filter.zone == Some(ZoneFilter::Exile)
                && filter.extra_zones.is_empty()
                && filter.controller.is_none()
                && matches!(
                    filter.owner,
                    None | Some(ControllerRelation::You | ControllerRelation::OwnerYou)
                ) =>
        {
            let linked_exile = (filter.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
            let owner = if filter.owner.is_some() {
                Some(ResolvingPlayer::Controller)
            } else {
                None
            };
            let mut cleaned = filter.as_ref().clone();
            cleaned.zone = None;
            cleaned.owner = None;
            if linked_exile {
                cleaned.flags =
                    AstFilterFlags(cleaned.flags.0 & !AstFilterFlags::EXILED_WITH_SELF.0);
            }
            for pred in &mut cleaned.predicates {
                if let spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(q) =
                    pred
                    && q.trim().eq_ignore_ascii_case("has_adventure")
                {
                    *pred = spellwake_oracle_compiler::ast::FilterPredicateAst::HasAbilityKind {
                        kind: "adventure".to_string(),
                        negated: false,
                    };
                }
            }
            let obj = if is_unrestricted_filter(&cleaned) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&cleaned, vocabulary).ok()?
            };
            let zone = if linked_exile {
                ValueZone::LinkedExile
            } else {
                ValueZone::Exile(owner)
            };
            let rc = ResolutionCondition {
                left: ValueExpr::Count {
                    zone,
                    filter: Box::new(obj),
                    controller_scope: None,
                },
                comparison: static_lower_compare_op(*op),
                right: ValueExpr::literal(i64::from(*value)),
            };
            if rc.is_world_evaluable() && rc.validate_world(vocabulary).is_ok() {
                Some(StaticSourceCondition::Eval(Box::new(rc)))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn push_conditional_static(
    program: &mut Program,
    ability: StaticContinuousAbility,
    condition: Option<&ConditionAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
    let sc = match condition {
        Some(cond) => Some(
            lower_static_source_condition_ext(cond, vocabulary)
                .ok_or("oracle:unsupported-static-cond")?,
        ),
        None => None,
    };
    let id = ability.id();
    push_static_ability(program, ability)?;
    if let Some(sc) = sc {
        program.static_conditions.insert(id, sc);
    }
    Ok(())
}

fn static_condition_primary_subject(cond: &ConditionAst) -> Option<&SubjectRefAst> {
    match cond {
        ConditionAst::CounterCompare { subject, .. }
        | ConditionAst::HasStatus { subject, .. }
        | ConditionAst::HasKeyword { subject, .. }
        | ConditionAst::MatchesFilter { subject, .. }
        | ConditionAst::StatCompare { subject, .. } => Some(subject.as_ref()),
        ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(subject)) => Some(subject.as_ref()),
        ConditionAst::Compare { lhs, rhs, .. } => {
            let is_possessive_or_anaphora = |s: &SubjectRefAst| {
                matches!(
                    s,
                    SubjectRefAst::Anaphora(raw)
                        if matches!(
                            raw.trim().to_ascii_lowercase().as_str(),
                            "it" | "its" | "his" | "her" | "their"
                        )
                )
            };
            match (lhs.as_ref(), rhs.as_ref()) {
                (
                    ValueExprAst::ObjectStat { subject: s1, .. },
                    ValueExprAst::ObjectStat { subject: s2, .. },
                ) if !matches!(s1.as_ref(), SubjectRefAst::Anaphora(_))
                    && (s1 == s2 || is_possessive_or_anaphora(s2)) =>
                {
                    Some(s1.as_ref())
                }
                (
                    ValueExprAst::ObjectStat { subject: s1, .. },
                    ValueExprAst::ObjectStat { subject: s2, .. },
                ) if !matches!(s2.as_ref(), SubjectRefAst::Anaphora(_))
                    && is_possessive_or_anaphora(s1) =>
                {
                    Some(s2.as_ref())
                }
                _ => None,
            }
        }
        ConditionAst::Not(inner) => static_condition_primary_subject(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            let sa = static_condition_primary_subject(a);
            let sb = static_condition_primary_subject(b);
            match (sa, sb) {
                (Some(x), Some(y)) if x == y => Some(x),
                (Some(x), None) => Some(x),
                (None, Some(y)) => Some(y),
                _ => None,
            }
        }
        _ => None,
    }
}

fn resolve_selector_keyword(
    kw_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<KeywordAbility> {
    lower_keyword_ability_name(kw_raw).or_else(|| {
        let low = kw_raw.trim().to_ascii_lowercase();
        if let Some(rest) = low.strip_prefix("protection from ") {
            let quals = parse_protection_keywords(rest, vocabulary)?;
            if quals.len() == 1 {
                return Some(quals[0]);
            }
        }
        if let Some(rest) = low.strip_prefix("hexproof from ") {
            let quals = parse_hexproof_from_qualities(rest, vocabulary)?;
            if quals.len() == 1 {
                return Some(KeywordAbility::HexproofFrom(quals[0]));
            }
        }
        None
    })
}

fn lower_static_filter_to_permanent_selector(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    lower_static_filter_to_permanent_selector_with_host(f, false, vocabulary)
}

fn lower_static_filter_to_permanent_selector_with_host(
    f: &ObjectFilterAst,
    subject_is_attached_host: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    let mut requires_flying = false;
    let mut required_keyword: Option<KeywordAbility> = None;
    let mut extra_required_keyword: Option<KeywordAbility> = None;
    let mut requires_toxic = false;
    let mut required_counter_kind: Option<CounterKind> = None;
    let mut minimum_plus_one_counters: u16 = 0;
    let mut toughness_greater_than_power = false;
    let mut needs_clone = false;

    if !f.required_keywords.is_empty() {
        for kw_raw in &f.required_keywords {
            let low = kw_raw.trim().to_ascii_lowercase();
            if low == "toxic" {
                requires_toxic = true;
                needs_clone = true;
                continue;
            }
            let kw = resolve_selector_keyword(kw_raw, vocabulary)
                .ok_or("oracle:unsupported-selector-predicate")?;
            if kw == KeywordAbility::Flying {
                requires_flying = true;
            } else if required_keyword.is_none() || required_keyword == Some(kw) {
                required_keyword = Some(kw);
            } else if extra_required_keyword.is_none() || extra_required_keyword == Some(kw) {
                extra_required_keyword = Some(kw);
            } else {
                return Err("oracle:unsupported-selector-predicate");
            }
            if f.required_keywords.len() > 1 {
                needs_clone = true;
            }
        }
    }

    if (f.flags.0 & AstFilterFlags::WITHOUT_COUNTERS.0) == 0
        && let Some(ck_raw) = f.counter_kind.as_deref()
    {
        let ck_low = ck_raw.trim().to_ascii_lowercase();
        let ck_clean = ck_low
            .trim_end_matches(" counters")
            .trim_end_matches(" counter")
            .trim();
        if let Some(min_p1) = parse_minimum_plus_one_counter_kind(ck_clean) {
            minimum_plus_one_counters = minimum_plus_one_counters.max(min_p1);
            needs_clone = true;
        } else if !matches!(ck_clean, "+1/+1" | "-1/-1" | "any" | "a" | "one or more")
            && let Some(kind) = parse_counter_kind(ck_clean).filter(|k| k.is_permanent())
        {
            required_counter_kind = Some(kind);
            needs_clone = true;
        }
    }

    if f.predicates.iter().any(is_toughness_gt_power_predicate)
        || f.stat_bounds.iter().any(is_toughness_gt_power_stat_bound)
    {
        toughness_greater_than_power = true;
        needs_clone = true;
    }

    if !needs_clone {
        return super::lower_filter_to_permanent_selector_with_host_context(
            f,
            subject_is_attached_host,
            vocabulary,
        );
    }

    let mut cloned = f.clone();
    cloned.required_keywords.clear();
    if required_counter_kind.is_some() || minimum_plus_one_counters > 0 {
        cloned.counter_kind = None;
        cloned.flags = AstFilterFlags(cloned.flags.0 & !AstFilterFlags::WITH_ANY_COUNTER.0);
    }
    if toughness_greater_than_power {
        cloned
            .predicates
            .retain(|p| !is_toughness_gt_power_predicate(p));
        cloned
            .stat_bounds
            .retain(|sb| !is_toughness_gt_power_stat_bound(sb));
    }
    let mut sel = super::lower_filter_to_permanent_selector_with_host_context(
        &cloned,
        subject_is_attached_host,
        vocabulary,
    )?;
    sel.requires_flying |= requires_flying;
    if let Some(kw) = required_keyword {
        sel.required_keyword = Some(kw);
    }
    if let Some(kw) = extra_required_keyword {
        sel.extra_required_keyword = Some(kw);
    }
    sel.requires_toxic |= requires_toxic;
    if let Some(ck) = required_counter_kind {
        sel.required_counter_kind = Some(ck);
    }
    if minimum_plus_one_counters > 0 {
        sel.minimum_plus_one_counters =
            sel.minimum_plus_one_counters.max(minimum_plus_one_counters);
    }
    sel.toughness_greater_than_power |= toughness_greater_than_power;
    Ok(sel)
}

fn parse_minimum_plus_one_counter_kind(ck_clean: &str) -> Option<u16> {
    let num_raw = ck_clean
        .strip_suffix(" or more +1/+1")
        .or_else(|| {
            ck_clean
                .strip_prefix("at least ")
                .and_then(|r| r.strip_suffix(" +1/+1"))
        })?
        .trim();
    let n: u16 = match num_raw {
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        other => other.parse().ok()?,
    };
    (n >= 1).then_some(n)
}

fn is_toughness_gt_power_stat_bound(sb: &spellwake_oracle_compiler::ast::StatBoundAst) -> bool {
    let is_self_possessive = |subj: &SubjectRefAst| {
        matches!(
            subj,
            SubjectRefAst::Anaphora(raw)
                if matches!(
                    raw.trim().to_ascii_lowercase().as_str(),
                    "its" | "their" | "it" | "them" | "his" | "her"
                )
        )
    };
    match (sb.stat, sb.op, sb.value.as_ref()) {
        (
            StatKind::Toughness,
            CompareOp::Gt,
            ValueExprAst::ObjectStat {
                subject,
                stat: StatKind::Power,
            },
        )
        | (
            StatKind::Power,
            CompareOp::Lt,
            ValueExprAst::ObjectStat {
                subject,
                stat: StatKind::Toughness,
            },
        ) => is_self_possessive(subject),
        _ => false,
    }
}

fn is_toughness_gt_power_predicate(
    pred: &spellwake_oracle_compiler::ast::FilterPredicateAst,
) -> bool {
    matches!(
        pred,
        spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(q)
            if matches!(
                q.trim().to_ascii_lowercase().as_str(),
                "with toughness greater than their power"
                    | "with toughness greater than its power"
                    | "toughness greater than their power"
                    | "toughness greater than its power"
                    | "toughness_greater_than_power"
            )
    )
}

fn static_lower_compare_op(op: CompareOp) -> Comparison {
    match op {
        CompareOp::Lt => Comparison::Less,
        CompareOp::Le => Comparison::LessOrEqual,
        CompareOp::Eq => Comparison::Equal,
        CompareOp::Ge => Comparison::GreaterOrEqual,
        CompareOp::Gt => Comparison::Greater,
    }
}

fn static_lower_stat_kind(stat: StatKind) -> spellwake_engine::program::ObjectStatKind {
    match stat {
        StatKind::Power => spellwake_engine::program::ObjectStatKind::Power,
        StatKind::Toughness => spellwake_engine::program::ObjectStatKind::Toughness,
        StatKind::ManaValue => spellwake_engine::program::ObjectStatKind::ManaValue,
        StatKind::Loyalty => spellwake_engine::program::ObjectStatKind::Loyalty,
    }
}

fn condition_contains_anaphora(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Not(inner) => condition_contains_anaphora(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            condition_contains_anaphora(a) || condition_contains_anaphora(b)
        }
        ConditionAst::HasStatus { subject, .. }
        | ConditionAst::HasKeyword { subject, .. }
        | ConditionAst::CounterCompare { subject, .. }
        | ConditionAst::StatCompare { subject, .. }
        | ConditionAst::MatchesFilter { subject, .. } => {
            matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
        }
        ConditionAst::Marker(
            GameMarker::SubjectEnteredThisTurn(subject) | GameMarker::AttackedThisTurn { subject },
        ) => matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)),
        ConditionAst::Compare { lhs, rhs, .. } => {
            let expr_has = |e: &ValueExprAst| {
                matches!(
                    e,
                    ValueExprAst::ObjectStat { subject, .. }
                        | ValueExprAst::CounterCount { subject, .. }
                        if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                )
            };
            expr_has(lhs) || expr_has(rhs)
        }
        _ => false,
    }
}

fn and_selector_source_condition(
    sel: &mut PermanentSelector,
    extra: spellwake_engine::characteristics::StaticSourceCondition,
) {
    sel.source_condition = Some(match sel.source_condition.take() {
        Some(prev) => spellwake_engine::characteristics::StaticSourceCondition::And(
            Box::new(prev),
            Box::new(extra),
        ),
        None => extra,
    });
}

fn selector_subject_is_recipient_anchor(sel: &PermanentSelector, subj: &SubjectRefAst) -> bool {
    match subj {
        SubjectRefAst::SelfObject => sel.controller == ControllerSelector::SourceItself,
        SubjectRefAst::AttachedHost(_) => sel.attached_to_source,
        SubjectRefAst::Anaphora(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "it" | "they"
                | "this creature"
                | "this permanent"
                | "this artifact"
                | "this enchantment"
                | "this land"
                | "this planeswalker"
                | "this vehicle"
                | "this equipment"
                | "this aura"
                | "he"
                | "she"
                | "him"
                | "her"
                | "that creature"
                | "that permanent"
                | "those creatures"
                | "those permanents"
        ),
        _ => false,
    }
}

fn try_apply_extended_selector_status(
    sel: &mut PermanentSelector,
    subj: &SubjectRefAst,
    status: &str,
    negated: bool,
) -> bool {
    use spellwake_engine::characteristics::{CombatSelector, StaticSourceCondition};
    use spellwake_engine::program::SourceStateExpr;

    if !selector_subject_is_recipient_anchor(sel, subj) {
        return false;
    }
    let st = status.trim().to_ascii_lowercase();
    let is_self = sel.controller == ControllerSelector::SourceItself;
    if negated {
        match st.as_str() {
            "tapped" => {
                sel.requires_untapped = true;
                return true;
            }
            "untapped" => {
                sel.requires_tapped = true;
                return true;
            }
            "attacking" => {
                sel.combat_role = Some(CombatSelector::NotAttacking);
                return true;
            }
            "token" => {
                sel.token = Some(false);
                return true;
            }
            "nontoken" => {
                sel.token = Some(true);
                return true;
            }
            "legendary" => {
                sel.forbidden_supertypes.insert(Supertype::Legendary);
                return true;
            }
            "snow" => {
                sel.forbidden_supertypes.insert(Supertype::Snow);
                return true;
            }
            "basic" => {
                sel.forbidden_supertypes.insert(Supertype::Basic);
                return true;
            }
            "face-down" | "face down" => {
                sel.face_down = Some(false);
                return true;
            }
            "face-up" | "face up" => {
                sel.face_down = Some(true);
                return true;
            }
            "historic" => {
                sel.historic = Some(false);
                return true;
            }
            "commander" => {
                sel.commander = Some(false);
                return true;
            }
            "equipped" if is_self => {
                and_selector_source_condition(
                    sel,
                    StaticSourceCondition::Not(Box::new(StaticSourceCondition::SourceIsEquipped)),
                );
                return true;
            }
            "enchanted" if is_self => {
                and_selector_source_condition(
                    sel,
                    StaticSourceCondition::Not(Box::new(StaticSourceCondition::SourceIsEnchanted)),
                );
                return true;
            }
            "solved" if is_self => {
                and_selector_source_condition(
                    sel,
                    StaticSourceCondition::Not(Box::new(StaticSourceCondition::SourceIsSolved)),
                );
                return true;
            }
            "paired" if is_self => {
                and_selector_source_condition(
                    sel,
                    StaticSourceCondition::Not(Box::new(StaticSourceCondition::SourcePaired)),
                );
                return true;
            }
            "prepared" if is_self => {
                and_selector_source_condition(
                    sel,
                    StaticSourceCondition::Not(Box::new(StaticSourceCondition::SourceIsPrepared)),
                );
                return true;
            }
            "monstrous" | "renowned" | "saddled" | "suspected" | "goaded" | "exerted"
                if is_self =>
            {
                let expr = match st.as_str() {
                    "monstrous" => SourceStateExpr::Monstrous,
                    "renowned" => SourceStateExpr::Renowned,
                    "saddled" => SourceStateExpr::Saddled,
                    "suspected" => SourceStateExpr::Suspected,
                    "goaded" => SourceStateExpr::Goaded,
                    _ => SourceStateExpr::ExertedThisTurn,
                };
                and_selector_source_condition(
                    sel,
                    StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::SourceState(expr),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0_i64),
                    })),
                );
                return true;
            }
            _ => return false,
        }
    }
    match st.as_str() {
        "modified" => {
            sel.requires_modified = true;
            true
        }
        "face-down" | "face down" => {
            sel.face_down = Some(true);
            true
        }
        "face-up" | "face up" => {
            sel.face_down = Some(false);
            true
        }
        "monstrous" => {
            sel.monstrous = true;
            true
        }
        "renowned" => {
            sel.renowned = true;
            true
        }
        "saddled" => {
            sel.saddled = true;
            true
        }
        "suspected" => {
            sel.suspected = true;
            true
        }
        "solved" => {
            sel.solved = true;
            true
        }
        "kicked" => {
            sel.kicked = true;
            true
        }
        "exerted" => {
            sel.exerted = true;
            true
        }
        "historic" => {
            sel.historic = Some(true);
            true
        }
        "nonhistoric" | "non-historic" => {
            sel.historic = Some(false);
            true
        }
        "commander" => {
            sel.commander = Some(true);
            true
        }
        "legendary" => {
            sel.required_supertypes.insert(Supertype::Legendary);
            true
        }
        "snow" => {
            sel.required_supertypes.insert(Supertype::Snow);
            true
        }
        "basic" => {
            sel.required_supertypes.insert(Supertype::Basic);
            true
        }
        "attacking" => {
            if sel.combat_role.is_some() && sel.combat_role != Some(CombatSelector::Attacking) {
                return false;
            }
            sel.combat_role = Some(CombatSelector::Attacking);
            true
        }
        "blocking" => {
            if sel.combat_role.is_some() && sel.combat_role != Some(CombatSelector::Blocking) {
                return false;
            }
            sel.combat_role = Some(CombatSelector::Blocking);
            true
        }
        "blocked" => {
            if sel.combat_role.is_some() && sel.combat_role != Some(CombatSelector::Blocked) {
                return false;
            }
            sel.combat_role = Some(CombatSelector::Blocked);
            true
        }
        "unblocked" => {
            if sel.combat_role.is_some() && sel.combat_role != Some(CombatSelector::Unblocked) {
                return false;
            }
            sel.combat_role = Some(CombatSelector::Unblocked);
            true
        }
        "tapped" => {
            if sel.requires_untapped {
                return false;
            }
            sel.requires_tapped = true;
            true
        }
        "untapped" => {
            if sel.requires_tapped {
                return false;
            }
            sel.requires_untapped = true;
            true
        }
        "not attacking" => {
            if sel.combat_role.is_some() && sel.combat_role != Some(CombatSelector::NotAttacking) {
                return false;
            }
            sel.combat_role = Some(CombatSelector::NotAttacking);
            true
        }
        "attacking or blocking" => {
            if sel.combat_role.is_some()
                && sel.combat_role != Some(CombatSelector::AttackingOrBlocking)
            {
                return false;
            }
            sel.combat_role = Some(CombatSelector::AttackingOrBlocking);
            true
        }
        "equipped" => {
            sel.equipped_by_at_least = sel.equipped_by_at_least.max(1);
            true
        }
        "enchanted" => {
            sel.requires_enchanted = true;
            true
        }
        "paired" if is_self => {
            and_selector_source_condition(sel, StaticSourceCondition::SourcePaired);
            true
        }
        "prepared" if is_self => {
            and_selector_source_condition(sel, StaticSourceCondition::SourceIsPrepared);
            true
        }
        "goaded" if is_self => {
            and_selector_source_condition(
                sel,
                StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::Goaded),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1_i64),
                })),
            );
            true
        }
        "attacking alone" if is_self => {
            and_selector_source_condition(
                sel,
                StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::AttackingAlone),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1_i64),
                })),
            );
            true
        }
        "tribute paid" if is_self => {
            and_selector_source_condition(
                sel,
                StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::TributePaid),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1_i64),
                })),
            );
            true
        }
        _ => false,
    }
}

fn try_apply_negated_atomic_selector_filter(
    sel: &mut PermanentSelector,
    filter: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> bool {
    use spellwake_engine::characteristics::CombatSelector;

    if filter.any_permanent || filter.any_spell || filter.any_card || filter.is_ability {
        return false;
    }
    if filter.combat == Some(CombatState::Attacking)
        && filter_only_has(
            filter,
            FilterAllow {
                combat: true,
                ..FilterAllow::NONE
            },
        )
    {
        if sel.combat_role.is_none() || sel.combat_role == Some(CombatSelector::NotAttacking) {
            sel.combat_role = Some(CombatSelector::NotAttacking);
            return true;
        }
        return false;
    }
    if !filter.required_supertypes.is_empty()
        && filter.forbidden_supertypes.is_empty()
        && filter_only_has(
            filter,
            FilterAllow {
                req_supertypes: true,
                ..FilterAllow::NONE
            },
        )
    {
        let single = if filter.required_supertypes == AstSupertypeMask::LEGENDARY {
            Some(Supertype::Legendary)
        } else if filter.required_supertypes == AstSupertypeMask::SNOW {
            Some(Supertype::Snow)
        } else if filter.required_supertypes == AstSupertypeMask::BASIC {
            Some(Supertype::Basic)
        } else {
            None
        };
        if let Some(st) = single {
            sel.forbidden_supertypes.insert(st);
            return true;
        }
    }
    if !filter.any_types.is_empty()
        && filter.forbidden_types.is_empty()
        && filter_only_has(
            filter,
            FilterAllow {
                types: true,
                ..FilterAllow::NONE
            },
        )
    {
        let single_ty = if filter.any_types == AstCardTypeMask::CREATURE {
            Some(CardType::Creature)
        } else if filter.any_types == AstCardTypeMask::ARTIFACT {
            Some(CardType::Artifact)
        } else if filter.any_types == AstCardTypeMask::ENCHANTMENT {
            Some(CardType::Enchantment)
        } else if filter.any_types == AstCardTypeMask::LAND {
            Some(CardType::Land)
        } else if filter.any_types == AstCardTypeMask::PLANESWALKER {
            Some(CardType::Planeswalker)
        } else if filter.any_types == AstCardTypeMask::BATTLE {
            Some(CardType::Battle)
        } else {
            None
        };
        if let Some(ct) = single_ty {
            sel.forbidden_types.insert(ct);
            return true;
        }
    }
    if filter.subtypes.len() == 1
        && filter_only_has(
            filter,
            FilterAllow {
                subtypes: true,
                ..FilterAllow::NONE
            },
        )
        && let Some(ct) = vocabulary.resolve_label(&filter.subtypes[0])
    {
        sel.forbidden_subtypes
            .insert(spellwake_engine::characteristics::SelectorSubtype::Creature(ct));
        return true;
    }
    false
}

fn apply_static_selector_condition(
    sel: &mut PermanentSelector,
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
    use spellwake_engine::characteristics::StaticSourceCondition;
    use spellwake_engine::program::{CounterSubjectRef, ValueObjectRef};

    if let ConditionAst::And(a, b) = cond {
        let mut trial = sel.clone();
        apply_static_selector_condition(&mut trial, a, vocabulary)?;
        let saved_sc = trial.source_condition.take();
        apply_static_selector_condition(&mut trial, b, vocabulary)?;
        if let Some(prev) = saved_sc {
            trial.source_condition = Some(match trial.source_condition.take() {
                Some(next) => StaticSourceCondition::And(Box::new(prev), Box::new(next)),
                None => prev,
            });
        }
        *sel = trial;
        return Ok(());
    }
    if let ConditionAst::HasStatus { subject, status } = cond
        && try_apply_extended_selector_status(sel, subject, status, false)
    {
        return Ok(());
    }
    if let ConditionAst::PlayerActionThisTurn {
        player: PlayerRefAst::You,
        action,
    } = cond
        && sel.attached_to_source
    {
        let act = action.trim().to_ascii_lowercase();
        if matches!(
            act.as_str(),
            "enchanted creature is attacking alone" | "equipped creature is attacking alone"
        ) && (sel.combat_role.is_none()
            || sel.combat_role
                == Some(spellwake_engine::characteristics::CombatSelector::Attacking))
        {
            sel.combat_role = Some(spellwake_engine::characteristics::CombatSelector::Attacking);
            and_selector_source_condition(
                sel,
                StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_battlefield(ObjectFilter::Clause(
                        ObjectClause::creature().combat(CombatFilter::Attacking),
                    )),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1_i64),
                })),
            );
            return Ok(());
        }
    }
    if let Some(you_control_host) = attached_host_control_polarity(cond) {
        if sel.attached_to_source
            && you_control_host
            && matches!(
                sel.controller,
                ControllerSelector::Any | ControllerSelector::SourceController
            )
        {
            sel.controller = ControllerSelector::SourceController;
            return Ok(());
        }
        let host_you = StaticSourceCondition::AttachedHostMatches(Box::new(PermanentSelector {
            controller: ControllerSelector::SourceController,
            ..PermanentSelector::default()
        }));
        and_selector_source_condition(
            sel,
            if you_control_host {
                host_you
            } else {
                StaticSourceCondition::Not(Box::new(host_you))
            },
        );
        return Ok(());
    }
    if let ConditionAst::Not(inner) = cond {
        if let Some(you_control_host) = attached_host_control_polarity(inner) {
            let host_you =
                StaticSourceCondition::AttachedHostMatches(Box::new(PermanentSelector {
                    controller: ControllerSelector::SourceController,
                    ..PermanentSelector::default()
                }));
            and_selector_source_condition(
                sel,
                if you_control_host {
                    StaticSourceCondition::Not(Box::new(host_you))
                } else {
                    host_you
                },
            );
            return Ok(());
        }
        if let ConditionAst::HasStatus { subject, status } = inner.as_ref()
            && try_apply_extended_selector_status(sel, subject, status, true)
        {
            return Ok(());
        }
        if let ConditionAst::HasKeyword { subject, keyword } = inner.as_ref()
            && selector_subject_is_recipient_anchor(sel, subject)
            && let Some(ka) = lower_keyword_ability_name(keyword)
        {
            sel.forbidden_keywords.insert(ka);
            return Ok(());
        }
        if let ConditionAst::MatchesFilter { subject, filter } = inner.as_ref()
            && selector_subject_is_recipient_anchor(sel, subject)
        {
            if try_apply_negated_atomic_selector_filter(sel, filter, vocabulary) {
                return Ok(());
            }
            if sel.attached_to_source
                && let Ok(host_sel) = lower_static_filter_to_permanent_selector(filter, vocabulary)
            {
                and_selector_source_condition(
                    sel,
                    StaticSourceCondition::Not(Box::new(
                        StaticSourceCondition::AttachedHostMatches(Box::new(host_sel)),
                    )),
                );
                return Ok(());
            }
        }
    }
    if let ConditionAst::HasKeyword { subject, keyword } = cond
        && selector_subject_is_recipient_anchor(sel, subject)
        && let Some(ka) = lower_keyword_ability_name(keyword)
    {
        if sel.required_keyword.is_none() || sel.required_keyword == Some(ka) {
            sel.required_keyword = Some(ka);
            return Ok(());
        }
        if sel.extra_required_keyword.is_none() || sel.extra_required_keyword == Some(ka) {
            sel.extra_required_keyword = Some(ka);
            return Ok(());
        }
    }
    if let ConditionAst::StatCompare {
        subject,
        stat,
        op,
        value,
    } = cond
        && selector_subject_is_recipient_anchor(sel, subject)
    {
        if sel.controller == ControllerSelector::SourceItself
            && matches!(subject.as_ref(), SubjectRefAst::SelfObject)
        {
            and_selector_source_condition(
                sel,
                StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::object_stat(
                        ValueObjectRef::AbilitySource,
                        static_lower_stat_kind(*stat),
                    ),
                    comparison: static_lower_compare_op(*op),
                    right: ValueExpr::literal(i64::from(*value)),
                })),
            );
            return Ok(());
        }
        if sel.attached_to_source && matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_)) {
            and_selector_source_condition(
                sel,
                StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::object_stat(
                        ValueObjectRef::AttachedHost,
                        static_lower_stat_kind(*stat),
                    ),
                    comparison: static_lower_compare_op(*op),
                    right: ValueExpr::literal(i64::from(*value)),
                })),
            );
            return Ok(());
        }
        if *stat == StatKind::ManaValue
            && let Ok(v) = u32::try_from(*value)
        {
            match op {
                CompareOp::Ge => {
                    sel.min_mana_value = Some(sel.min_mana_value.map_or(v, |prev| prev.max(v)));
                    return Ok(());
                }
                CompareOp::Gt => {
                    let min_v = v.saturating_add(1);
                    sel.min_mana_value =
                        Some(sel.min_mana_value.map_or(min_v, |prev| prev.max(min_v)));
                    return Ok(());
                }
                CompareOp::Le => {
                    sel.max_mana_value = Some(sel.max_mana_value.map_or(v, |prev| prev.min(v)));
                    return Ok(());
                }
                CompareOp::Lt if v > 0 => {
                    let max_v = v - 1;
                    sel.max_mana_value =
                        Some(sel.max_mana_value.map_or(max_v, |prev| prev.min(max_v)));
                    return Ok(());
                }
                CompareOp::Eq => {
                    sel.min_mana_value = Some(sel.min_mana_value.map_or(v, |prev| prev.max(v)));
                    sel.max_mana_value = Some(sel.max_mana_value.map_or(v, |prev| prev.min(v)));
                    return Ok(());
                }
                _ => {}
            }
        }
    }
    if let ConditionAst::CounterCompare {
        subject,
        counter_kind,
        op,
        value,
    } = cond
        && selector_subject_is_recipient_anchor(sel, subject)
    {
        let ck_low = counter_kind.trim().to_ascii_lowercase();
        let kind_opt = if matches!(ck_low.as_str(), "" | "any" | "a" | "one or more") {
            Some(None)
        } else {
            parse_counter_kind(&ck_low)
                .filter(|k| k.is_permanent())
                .map(Some)
        };
        if let Some(kind) = kind_opt {
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && sel.controller == ControllerSelector::SourceItself
            {
                and_selector_source_condition(
                    sel,
                    StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::counters(CounterSubjectRef::AbilitySource, kind),
                        comparison: static_lower_compare_op(*op),
                        right: ValueExpr::literal(i64::from(*value)),
                    })),
                );
                return Ok(());
            }
            if matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_)) && sel.attached_to_source
            {
                and_selector_source_condition(
                    sel,
                    StaticSourceCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::counters(CounterSubjectRef::AttachedHost, kind),
                        comparison: static_lower_compare_op(*op),
                        right: ValueExpr::literal(i64::from(*value)),
                    })),
                );
                return Ok(());
            }
            if kind.is_none() && *op == CompareOp::Eq && *value == 0 {
                sel.without_counters = true;
                return Ok(());
            }
        }
    }
    if let ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(subj)) = cond
        && selector_subject_is_recipient_anchor(sel, subj)
    {
        sel.entered_this_turn = true;
        return Ok(());
    }
    if let ConditionAst::Marker(GameMarker::AttackedThisTurn { subject }) = cond
        && selector_subject_is_recipient_anchor(sel, subject)
    {
        sel.attacked_this_turn = Some(true);
        return Ok(());
    }
    if let ConditionAst::Compare { lhs, op, rhs } = cond {
        let is_possessive = |subj: &SubjectRefAst| -> bool {
            matches!(
                subj,
                SubjectRefAst::Anaphora(raw)
                    if matches!(
                        raw.trim().to_ascii_lowercase().as_str(),
                        "its" | "his" | "her" | "their"
                    )
            )
        };
        let matches_anchor = |subj: &SubjectRefAst| -> bool {
            selector_subject_is_recipient_anchor(sel, subj) || is_possessive(subj)
        };
        let is_t_gt_p = match (lhs.as_ref(), op, rhs.as_ref()) {
            (
                ValueExprAst::ObjectStat {
                    subject: s1,
                    stat: StatKind::Toughness,
                },
                CompareOp::Gt,
                ValueExprAst::ObjectStat {
                    subject: s2,
                    stat: StatKind::Power,
                },
            )
            | (
                ValueExprAst::ObjectStat {
                    subject: s1,
                    stat: StatKind::Power,
                },
                CompareOp::Lt,
                ValueExprAst::ObjectStat {
                    subject: s2,
                    stat: StatKind::Toughness,
                },
            ) => {
                matches_anchor(s1)
                    && matches_anchor(s2)
                    && (selector_subject_is_recipient_anchor(sel, s1)
                        || selector_subject_is_recipient_anchor(sel, s2))
            }
            _ => false,
        };
        if is_t_gt_p {
            sel.toughness_greater_than_power = true;
            return Ok(());
        }
    }
    let saved_sc = sel.source_condition.take();
    let res = apply_selector_condition(sel, cond, vocabulary);
    if res.is_ok() {
        if let Some(prev) = saved_sc {
            sel.source_condition = Some(match sel.source_condition.take() {
                Some(next) => StaticSourceCondition::And(Box::new(prev), Box::new(next)),
                None => prev,
            });
        }
        return Ok(());
    }
    sel.source_condition = saved_sc;
    if sel.controller == ControllerSelector::SourceItself || !condition_contains_anaphora(cond) {
        if let Some(sc) = lower_static_source_condition_ext(cond, vocabulary) {
            and_selector_source_condition(sel, sc);
            return Ok(());
        }
        let mut tmp_builder = BodyBuilder::new();
        if let Ok(rc) = lower_resolution_condition(cond, &mut tmp_builder, vocabulary, false, None)
            && tmp_builder.target == BodyTarget::None
            && rc.is_world_evaluable()
            && rc.validate_world(vocabulary).is_ok()
        {
            and_selector_source_condition(sel, StaticSourceCondition::Eval(Box::new(rc)));
            return Ok(());
        }
    }
    res
}

fn lower_static_permanent_selector(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentSelector, &'static str> {
    match subject {
        SubjectRefAst::SelfObject => Ok(PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        }),
        SubjectRefAst::AttachedHost(_) => Ok(PermanentSelector {
            attached_to_source: true,
            ..PermanentSelector::default()
        }),
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
            lower_static_filter_to_permanent_selector(f, vocabulary)
        }
        _ => Err("oracle:unsupported-permanent-selector"),
    }
}

fn lower_static_permanent_selectors(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Vec<PermanentSelector>, &'static str> {
    match subject {
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.extend(lower_static_permanent_selectors(item, vocabulary)?);
            }
            Ok(out)
        }
        other => Ok(vec![lower_static_permanent_selector(other, vocabulary)?]),
    }
}

/// Returns `Some(true)` if `cond` asserts that you control the attached host, or
/// `Some(false)` if `cond` asserts that another player controls the attached host.
fn attached_host_control_polarity(cond: &ConditionAst) -> Option<bool> {
    match cond {
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } => {
            let act = action.trim().to_ascii_lowercase();
            matches!(
                act.as_str(),
                "you control enchanted creature"
                    | "you control equipped creature"
                    | "you control enchanted permanent"
                    | "you control equipped permanent"
            )
            .then_some(true)
        }
        ConditionAst::ControlsCount {
            player,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if (filter.flags.0 & AstFilterFlags::ATTACHED_TO_SELF.0) != 0
            && matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            && filter.controller.is_none()
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    flags: true,
                    zone: true,
                    ..FilterAllow::NONE
                },
            ) =>
        {
            match player {
                PlayerRefAst::You => Some(true),
                PlayerRefAst::EachOtherPlayer => Some(false),
                _ => None,
            }
        }
        _ => None,
    }
}

fn lower_static_permanent_selectors_with_context(
    subject: &SubjectRefAst,
    condition: Option<&ConditionAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Vec<PermanentSelector>, &'static str> {
    match subject {
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.extend(lower_static_permanent_selectors_with_context(
                    item, condition, vocabulary,
                )?);
            }
            Ok(out)
        }
        SubjectRefAst::Anaphora(raw) => {
            let low = raw.trim().to_ascii_lowercase();
            if let Some(cond) = condition {
                if matches!(
                    low.as_str(),
                    "that creature"
                        | "that permanent"
                        | "that artifact"
                        | "that land"
                        | "that enchantment"
                ) && let ConditionAst::ControlsCount {
                    player: PlayerRefAst::You,
                    filter,
                    op: CompareOp::Eq,
                    value: 1,
                } = cond
                {
                    let mut sel = lower_static_filter_to_permanent_selector(filter, vocabulary)?;
                    sel.controller = ControllerSelector::SourceController;
                    return Ok(vec![sel]);
                }
                if matches!(
                    low.as_str(),
                    "it" | "he"
                        | "she"
                        | "him"
                        | "her"
                        | "that creature"
                        | "that permanent"
                        | "that artifact"
                        | "that land"
                        | "that enchantment"
                ) {
                    let is_attached_host_cond = match cond {
                        ConditionAst::Not(inner) => attached_host_control_polarity(inner).is_some(),
                        other => attached_host_control_polarity(other).is_some(),
                    };
                    if is_attached_host_cond {
                        return Ok(vec![PermanentSelector {
                            attached_to_source: true,
                            ..PermanentSelector::default()
                        }]);
                    }
                    let attached_action = match cond {
                        ConditionAst::PlayerActionThisTurn {
                            player: PlayerRefAst::You,
                            action,
                        } => Some(action.as_str()),
                        ConditionAst::Not(inner) => match inner.as_ref() {
                            ConditionAst::PlayerActionThisTurn {
                                player: PlayerRefAst::You,
                                action,
                            } => Some(action.as_str()),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(act) = attached_action
                        && matches!(
                            act.trim().to_ascii_lowercase().as_str(),
                            "enchanted creature is attacking alone"
                                | "equipped creature is attacking alone"
                                | "you control enchanted creature"
                                | "you control equipped creature"
                        )
                    {
                        return Ok(vec![PermanentSelector {
                            attached_to_source: true,
                            ..PermanentSelector::default()
                        }]);
                    }
                    if let Some(primary) = static_condition_primary_subject(cond) {
                        return lower_static_permanent_selectors(primary, vocabulary);
                    }
                }
            }
            Err("oracle:unsupported-permanent-selector")
        }
        other => lower_static_permanent_selectors(other, vocabulary),
    }
}

fn is_spell_keyword_grant_filter(f: &ObjectFilterAst) -> bool {
    matches!(
        f.zone,
        Some(
            ZoneFilter::Stack
                | ZoneFilter::YourHand
                | ZoneFilter::YourGraveyard
                | ZoneFilter::Exile
                | ZoneFilter::YourLibrary
        )
    ) || (!f.any_permanent
        && (f.any_spell
            || (f.any_types.0 & (AstCardTypeMask::INSTANT.0 | AstCardTypeMask::SORCERY.0)) != 0))
}

fn lower_spell_keyword_grant(
    f: &ObjectFilterAst,
    keywords: &[String],
    condition: Option<&ConditionAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if keywords.is_empty() || f.any_types == AstCardTypeMask::LAND {
        return Err("oracle:unsupported-spell-keyword-grant");
    }
    let own_zone = matches!(
        f.zone,
        Some(ZoneFilter::YourGraveyard | ZoneFilter::YourHand | ZoneFilter::YourLibrary)
    );
    let any_player = if f.controller == Some(ControllerRelation::You)
        || (own_zone && matches!(f.controller, None | Some(ControllerRelation::OwnerYou)))
    {
        false
    } else if f.controller.is_none()
        && matches!(
            f.zone,
            None | Some(
                ZoneFilter::Stack
                    | ZoneFilter::Exile
                    | ZoneFilter::AllGraveyards
                    | ZoneFilter::AnyGraveyard
                    | ZoneFilter::AnyHand
            )
        )
    {
        true
    } else {
        return Err("oracle:unsupported-spell-keyword-grant");
    };

    let mut stripped = f.clone();
    stripped.controller = None;
    if own_zone
        && matches!(
            stripped.owner,
            Some(ControllerRelation::You | ControllerRelation::OwnerYou)
        )
    {
        stripped.owner = None;
    }
    if stripped.any_permanent {
        if stripped.any_types.is_empty() {
            stripped.any_types = AstCardTypeMask::PERMANENT;
        }
        stripped.any_permanent = false;
    }

    if !any_player
        && keywords
            .iter()
            .all(|k| k.trim().eq_ignore_ascii_case("cascade"))
        && f.zone == Some(ZoneFilter::YourHand)
        && f.color_arity == Some(ColorArity::Colorless)
        && f.any_types.is_empty()
        && f.forbidden_types.is_empty()
        && filter_only_has(
            &stripped,
            FilterAllow {
                color_arity: true,
                zone: true,
                stat_bounds: true,
                ..FilterAllow::NONE
            },
        )
        && let [sb] = stripped.stat_bounds.as_slice()
        && sb.stat == StatKind::ManaValue
        && let ValueExprAst::Const(v) = sb.value.as_ref()
        && *v >= 0
        && let Ok(v_u16) = u16::try_from(*v)
        && let Some(min_mv) = match sb.op {
            CompareOp::Ge => Some(v_u16),
            CompareOp::Gt => v_u16.checked_add(1),
            _ => None,
        }
        && let Ok(cnt_u8) = u8::try_from(keywords.len())
        && let Some(count) = NonZeroU8::new(cnt_u8)
    {
        return push_conditional_static(
            program,
            StaticContinuousAbility::CastPermission {
                id: ids.static_id(),
                permission: CastPermissionRule::ColorlessHandSpellsHaveCascade {
                    minimum_mana_value: min_mv,
                    count,
                },
            },
            condition,
            vocabulary,
        );
    }

    let matching = lower_spell_filter(&stripped, vocabulary)?;
    let mut cascade_count: u8 = 0;
    let mut affinity_discounts: Vec<ValueExpr> = Vec::new();
    let mut cant_be_countered = false;
    let mut resolved = Vec::with_capacity(keywords.len());
    for kw in keywords {
        let kw_low = kw.trim().trim_end_matches('.').trim().to_ascii_lowercase();
        if kw_low == "cascade" {
            cascade_count = cascade_count
                .checked_add(1)
                .ok_or("oracle:unsupported-spell-keyword-grant")?;
        } else if matches!(
            kw_low.as_str(),
            "can't be countered"
                | "cannot be countered"
                | "can't be countered by spells or abilities"
        ) && (!any_player || spell_filter_supported_by_matches_characteristics(&matching))
        {
            cant_be_countered = true;
        } else if !any_player && let Some(target_raw) = kw_low.strip_prefix("affinity for ") {
            let discount_expr = lower_spell_affinity_discount(target_raw, vocabulary)
                .ok_or("oracle:unsupported-spell-keyword-grant")?;
            affinity_discounts.push(discount_expr);
        } else {
            let ka = if kw_low == "jumpstart" {
                KeywordAbility::JumpStart
            } else {
                lower_keyword_ability_name(&kw_low)
                    .ok_or("oracle:unsupported-spell-keyword-grant")?
            };
            resolved.push(ka);
        }
    }

    if cant_be_countered {
        let permission = if any_player {
            CastPermissionRule::AllSpellsCannotBeCounteredFiltered(Box::new(matching))
        } else {
            CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(Box::new(matching))
        };
        push_conditional_static(
            program,
            StaticContinuousAbility::CastPermission {
                id: ids.static_id(),
                permission,
            },
            condition,
            vocabulary,
        )?;
    }

    if let Some(count) = NonZeroU8::new(cascade_count) {
        push_conditional_static(
            program,
            StaticContinuousAbility::CastPermission {
                id: ids.static_id(),
                permission: CastPermissionRule::GrantSpellCascade {
                    any_player,
                    matching,
                    count,
                },
            },
            condition,
            vocabulary,
        )?;
    }

    for discount_expr in affinity_discounts {
        push_conditional_static(
            program,
            StaticContinuousAbility::CostModifier {
                id: ids.static_id(),
                modifier: SpellCostModifier::Discount {
                    any_player: false,
                    matching,
                    amount: SpellDiscount::Dynamic(Box::new(discount_expr)),
                },
            },
            condition,
            vocabulary,
        )?;
    }

    for kw in resolved {
        let permission = if kw == KeywordAbility::Flash {
            CastPermissionRule::Flash {
                any_player,
                matching,
            }
        } else {
            CastPermissionRule::GrantSpellKeyword {
                any_player,
                matching,
                keyword: kw,
            }
        };
        push_conditional_static(
            program,
            StaticContinuousAbility::CastPermission {
                id: ids.static_id(),
                permission,
            },
            condition,
            vocabulary,
        )?;
    }
    Ok(())
}

fn lower_spell_affinity_discount(
    target_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ValueExpr> {
    let norm = target_raw
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    let obj = match norm.as_str() {
        "artifacts" => ObjectFilter::Clause(ObjectClause::artifact().controlled()),
        "creatures" => ObjectFilter::Clause(ObjectClause::creature().controlled()),
        "enchantments" => ObjectFilter::Clause(ObjectClause::enchantment().controlled()),
        "lands" => ObjectFilter::Clause(ObjectClause::land().controlled()),
        "planeswalkers" => ObjectFilter::Clause(ObjectClause::planeswalker().controlled()),
        _ => {
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            let mut parsed =
                spellwake_oracle_compiler::filter::parse_object_filter(&norm, &compiler_vocab)
                    .ok()?;
            if parsed.zone.is_some()
                || parsed.controller.is_some()
                || parsed.owner.is_some()
                || parsed.any_spell
                || parsed.any_card
                || (parsed.any_types.is_empty()
                    && parsed.subtypes.is_empty()
                    && parsed.token.is_none()
                    && parsed.required_supertypes.is_empty()
                    && !parsed.any_permanent)
            {
                return None;
            }
            parsed.controller = Some(ControllerRelation::You);
            lower_object_filter(&parsed, vocabulary).ok()?
        }
    };
    let expr = ValueExpr::count_battlefield(obj);
    expr.validate_world_expr(vocabulary).ok()?;
    Some(expr)
}

fn normalize_self_pronoun_in_value_expr(ast: &mut ValueExprAst) {
    let rewrite_subj = |s: &mut SubjectRefAst| {
        if let SubjectRefAst::Anaphora(raw) = s
            && matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "it" | "its" | "this creature" | "this permanent" | "his" | "her" | "their"
            )
        {
            *s = SubjectRefAst::SelfObject;
        }
    };
    match ast {
        ValueExprAst::ObjectStat { subject, .. } | ValueExprAst::CounterCount { subject, .. } => {
            rewrite_subj(subject)
        }
        ValueExprAst::Add(a, b) | ValueExprAst::Sub(a, b) | ValueExprAst::Mul(a, b) => {
            normalize_self_pronoun_in_value_expr(a);
            normalize_self_pronoun_in_value_expr(b);
        }
        ValueExprAst::DivConst { expr, .. } => normalize_self_pronoun_in_value_expr(expr),
        _ => {}
    }
}

fn lower_dynamic_keyword_x_value_expr(
    cost_rule: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ValueExpr> {
    let trimmed = cost_rule.trim().trim_end_matches('.').trim();
    let lower = trimmed.to_ascii_lowercase();
    let expr_raw = if let Some(rest) = lower.strip_prefix("x=") {
        rest.trim()
    } else if let Some(rest) = lower.strip_prefix("where x is ") {
        rest.trim()
    } else {
        lower.as_str()
    };
    if expr_raw.is_empty() || expr_raw.contains('|') {
        return None;
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let mut val_ast =
        spellwake_oracle_compiler::filter::parse_value_expr(expr_raw, &compiler_vocab).ok()?;
    normalize_self_pronoun_in_value_expr(&mut val_ast);
    let expr_ctx = StaticExprContext {
        anchor: StaticSelfAnchor::Source,
        has_chosen_player_entry: false,
    };
    let val = lower_static_value_expr(&val_ast, expr_ctx, vocabulary).ok()?;
    if !val.is_world_evaluable() || val.validate_world_expr(vocabulary).is_err() {
        return None;
    }
    Some(val)
}

fn lower_dynamic_ward_trigger(
    cost_rule: &str,
    vocabulary: &CreatureTypeVocabulary,
    id: TriggeredAbilityId,
) -> Option<TriggeredAbility> {
    let extra_generic = lower_dynamic_keyword_x_value_expr(cost_rule, vocabulary)?;
    Some(TriggeredAbility {
        id,
        condition: TriggerSpec::new(TriggerEvent::Targeted {
            subject: ObjectFilter::self_object(),
            by_opponent: true,
            spell_only: false,
            instant_or_sorcery_only: false,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OfferDynamicManaPayment {
                    payer: ResolvingPlayer::UniqueRelatedPlayer,
                    fixed: ManaCost::free(),
                    extra_generic,
                    paid_destination: 2,
                    declined_destination: 1,
                },
                Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::counter_ward_source(),
                )),
            ],
        ),
    })
}

fn lower_dynamic_mobilize_trigger(
    cost_rule: &str,
    vocabulary: &CreatureTypeVocabulary,
    id: TriggeredAbilityId,
) -> Option<TriggeredAbility> {
    let amount = lower_dynamic_keyword_x_value_expr(cost_rule, vocabulary)?;
    let warrior = vocabulary.resolve_label("Warrior")?;
    let mut token_prog = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([warrior]));
    token_prog.effect_defined_colors = Some(BTreeSet::from([Color::Red]));
    let warrior_def = TokenDefinition::new(
        token_prog,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(warrior)],
        },
        vocabulary,
    )
    .ok()?;
    Some(TriggeredAbility {
        id,
        condition: TriggerSpec::self_attacks(),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::CreateTokens {
                    creations: vec![TokenCreation {
                        enters_tapped: true,
                        definition: warrior_def,
                        amount,
                        base_size: None,
                        recipients: TokenRecipients::Controller,
                        attacking_defender: Some(
                            spellwake_engine::token::AttackingDefenderScope::AnyLegal,
                        ),
                    }],
                },
                Instruction::ScheduleDelayedStepTrigger {
                    step: Step::End,
                    own_step_only: false,
                    body: Box::new(ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::ZoneOp(Box::new(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics:
                                        spellwake_engine::toolbox::GraveyardSemantics::Sacrifice,
                                },
                            ),
                        ))],
                    )),
                },
            ],
        ),
    })
}

fn lower_grant_keyword_with_cost(
    subject: &SubjectRefAst,
    keyword: &str,
    cost_rule: &str,
    condition: Option<&ConditionAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    let is_spell = matches!(program.kind, ProgramKind::Spell(_));
    let kw_low = keyword.trim().to_ascii_lowercase();
    let cost_trimmed = cost_rule.trim().trim_end_matches('.').trim();
    let cost_low = cost_trimmed.to_ascii_lowercase();
    if kw_low.is_empty() || cost_low.is_empty() {
        return Err("oracle:unsupported-static");
    }
    if !is_spell && kw_low == "ward" {
        if matches!(subject, SubjectRefAst::SelfObject)
            && condition.is_none()
            && let Some(trigger) =
                lower_dynamic_ward_trigger(cost_trimmed, vocabulary, ids.triggered())
        {
            program.triggered.push(trigger);
            return Ok(());
        }
        if let Some(trigger) =
            lower_dynamic_ward_trigger(cost_trimmed, vocabulary, TriggeredAbilityId(0))
            && let Ok(mut selectors) =
                lower_static_permanent_selectors_with_context(subject, condition, vocabulary)
        {
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::GrantTrigger(Box::new(
                            trigger.clone(),
                        ))]),
                    },
                )?;
            }
            return Ok(());
        }
    }
    if !is_spell && kw_low == "mobilize" {
        // Fail closed if an attachment permanent receives a SelfObject mobilize grant
        // (prevents miscompiling granted Equipment/Aura mobilize onto the attachment itself).
        if matches!(subject, SubjectRefAst::SelfObject)
            && is_permanent_attachment_program(program)
            && !program.has_type(CardType::Creature)
        {
            return Err("oracle:unsupported-static");
        }
        if matches!(subject, SubjectRefAst::SelfObject)
            && condition.is_none()
            && let Some(trigger) =
                lower_dynamic_mobilize_trigger(cost_trimmed, vocabulary, ids.triggered())
        {
            program.triggered.push(trigger);
            return Ok(());
        }
        if let Some(trigger) =
            lower_dynamic_mobilize_trigger(cost_trimmed, vocabulary, TriggeredAbilityId(0))
            && let Ok(mut selectors) =
                lower_static_permanent_selectors_with_context(subject, condition, vocabulary)
        {
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![StaticPart::GrantTrigger(Box::new(
                            trigger.clone(),
                        ))]),
                    },
                )?;
            }
            return Ok(());
        }
    }
    if !is_spell
        && kw_low == "encore"
        && cost_low == "its mana cost"
        && let SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) = subject
        && f.zone == Some(ZoneFilter::YourGraveyard)
        && (f.any_types.is_empty() || f.any_types == AstCardTypeMask::CREATURE)
        && f.subtypes.len() == 1
        && matches!(
            f.controller,
            None | Some(ControllerRelation::You | ControllerRelation::OwnerYou)
        )
        && filter_only_has(
            f,
            FilterAllow {
                types: true,
                subtypes: true,
                zone: true,
                controller: true,
                ..FilterAllow::NONE
            },
        )
        && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0])
    {
        return push_conditional_static(
            program,
            StaticContinuousAbility::CastPermission {
                id: ids.static_id(),
                permission: CastPermissionRule::GraveyardSubtypeEncore(ct),
            },
            condition,
            vocabulary,
        );
    }
    if !is_spell
        && kw_low == "replicate"
        && cost_low == "its mana cost"
        && let SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) = subject
        && matches!(f.zone, None | Some(ZoneFilter::Stack))
        && (f.any_types.is_empty() || f.any_types == AstCardTypeMask::CREATURE)
        && f.subtypes.len() == 1
        && f.controller == Some(ControllerRelation::You)
        && filter_only_has(
            f,
            FilterAllow {
                types: true,
                subtypes: true,
                zone: true,
                controller: true,
                ..FilterAllow::NONE
            },
        )
        && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0])
    {
        return push_conditional_static(
            program,
            StaticContinuousAbility::CastPermission {
                id: ids.static_id(),
                permission: CastPermissionRule::SubtypeSpellsHaveReplicate(ct),
            },
            condition,
            vocabulary,
        );
    }
    if cost_low.starts_with("x=")
        || cost_low.contains("where x is")
        || cost_low.contains('|')
        || matches!(
            kw_low.as_str(),
            "flashback"
                | "escape"
                | "unearth"
                | "offspring"
                | "evoke"
                | "craft"
                | "encore"
                | "replicate"
                | "cascade"
        )
    {
        return Err("oracle:unsupported-static");
    }
    let combined = format!("{kw_low} {cost_trimmed}");
    let mut selectors =
        lower_static_permanent_selectors_with_context(subject, condition, vocabulary)?;
    for sel in &mut selectors {
        if let Some(cond) = condition {
            apply_static_selector_condition(sel, cond, vocabulary)?;
        }
    }
    let (mut parts_vec, rem_kws) = partition_static_keyword_grants(&[combined], vocabulary, ids);
    if !rem_kws.is_empty() {
        let ab_change = lower_ability_change_with_ids(&rem_kws, vocabulary, ids)?;
        parts_vec.push(StaticPart::Abilities(ab_change));
    }
    if parts_vec.is_empty() {
        return Err("oracle:unsupported-static");
    }
    for selector in selectors {
        push_static_ability(
            program,
            StaticContinuousAbility::Battlefield {
                id: ids.static_id(),
                selector,
                parts: StaticParts(parts_vec.clone()),
            },
        )?;
    }
    Ok(())
}

fn is_safe_exert_followup_ast(ins: &[InstructionAst]) -> bool {
    if ins.is_empty() {
        return false;
    }
    for op in ins {
        match op {
            InstructionAst::DelayedTrigger { .. }
            | InstructionAst::DelayedEventTrigger { .. }
            | InstructionAst::TemporaryTrigger { .. }
            | InstructionAst::TemporarySpecialAction { .. }
            | InstructionAst::ChooseOption(_)
            | InstructionAst::TemporaryStatic { .. }
            | InstructionAst::TemporaryReplacement { .. }
            | InstructionAst::Attach { .. } => return false,
            InstructionAst::PutCounters { counter_kind, .. }
            | InstructionAst::RemoveCounters { counter_kind, .. }
                if counter_kind.trim().eq_ignore_ascii_case("loyalty") =>
            {
                return false;
            }
            InstructionAst::Optional(inner)
            | InstructionAst::IfPreviousSucceeded(inner)
            | InstructionAst::IfPreviousFailed(inner) => {
                if !is_safe_exert_followup_ast(inner) {
                    return false;
                }
            }
            InstructionAst::IfCondition { then_body, .. }
                if !is_safe_exert_followup_ast(then_body) =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

fn lower_exert_attack_followup(
    ins: &[InstructionAst],
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    is_spell: bool,
) -> Result<(), &'static str> {
    if is_spell
        || !matches!(program.kind, ProgramKind::Permanent(_))
        || !program.has_type(CardType::Creature)
        || !program.face_up_abilities.is_empty()
        || !is_safe_exert_followup_ast(ins)
    {
        return Err("oracle:unsupported-static");
    }
    let mut body = lower_effect_body(ins, vocabulary, ids, true, true)?;
    if body.instructions.is_empty() {
        return Err("oracle:unsupported-static");
    }
    let spec = TriggerSpec::attacks(
        ObjectFilter::self_object(),
        AttackTriggerMode::Individual {
            capture_defender: false,
        },
    );
    apply_trigger_body_rewrites(&mut body, &spec, TriggerGrouping::EachOccurrence, true);
    prepend_instruction_at(
        &mut body.instructions,
        0,
        crate::keyword_creatures::oracle_lowering::toolbox_emit::exert_source(),
    );
    let inner_len = body.instructions.len();
    prepend_instruction_at(
        &mut body.instructions,
        0,
        Instruction::OptionalSequence {
            chooser: ResolvingPlayer::Controller,
            instructions: inner_len,
        },
    );
    program.triggered.push(TriggeredAbility {
        id: ids.triggered(),
        grouping: TriggerGrouping::EachOccurrence,
        condition: spec,
        body,
    });
    Ok(())
}

fn split_is_your_turn_condition(condition: Option<&ConditionAst>) -> (bool, Option<&ConditionAst>) {
    match condition {
        Some(ConditionAst::IsYourTurn) => (true, None),
        other => (false, other),
    }
}

fn extract_fixed_alternative_mana_cost(cost: &[CostAtomAst]) -> Option<ManaCost> {
    match cost {
        [] => Some(ManaCost::free()),
        [CostAtomAst::PayMana(raw)] => parse_fixed_mana_cost(&raw.to_ascii_uppercase()),
        _ => None,
    }
}

fn lower_first_spell_discount_modifier(
    subject: &SubjectRefAst,
    delta: &str,
    condition: Option<&ConditionAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    let discount = parse_generic_mana_amount(delta).ok_or("oracle:non-generic-discount")?;
    if discount == 0 {
        return Err("oracle:zero-discount");
    }
    let (filter, subj_own_turn, each_player) = match subject {
        SubjectRefAst::Anaphora(raw) => {
            let low = raw.trim().to_ascii_lowercase();
            let after_first = low
                .strip_prefix("the first ")
                .ok_or("oracle:unsupported-cost-modifier")?;
            let (spell_part, own_turn, each_player) = if let Some(sp) = after_first
                .strip_suffix(" you cast during each of your turns")
                .or_else(|| after_first.strip_suffix(" you cast on each of your turns"))
                .or_else(|| after_first.strip_suffix(" you cast during your turn"))
                .or_else(|| after_first.strip_suffix(" you cast on your turn"))
            {
                (sp.trim(), true, false)
            } else if let Some(sp) = after_first
                .strip_suffix(" you cast each turn")
                .or_else(|| after_first.strip_suffix(" you cast"))
            {
                (sp.trim(), false, false)
            } else if let Some(sp) = after_first
                .strip_suffix(" each player casts during each of their turns")
                .or_else(|| after_first.strip_suffix(" each player casts on each of their turns"))
                .or_else(|| after_first.strip_suffix(" each player casts during their turn"))
                .or_else(|| after_first.strip_suffix(" a player casts during each of their turns"))
                .or_else(|| after_first.strip_suffix(" a player casts on each of their turns"))
                .or_else(|| after_first.strip_suffix(" a player casts during their turn"))
            {
                (sp.trim(), true, true)
            } else if let Some(sp) = after_first
                .strip_suffix(" each player casts each turn")
                .or_else(|| after_first.strip_suffix(" each player casts"))
                .or_else(|| after_first.strip_suffix(" a player casts each turn"))
                .or_else(|| after_first.strip_suffix(" a player casts"))
            {
                (sp.trim(), false, true)
            } else {
                return Err("oracle:unsupported-cost-modifier");
            };
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            let mut parsed =
                spellwake_oracle_compiler::filter::parse_object_filter(spell_part, &compiler_vocab)
                    .map_err(|_| "oracle:unsupported-cost-modifier")?;
            if each_player {
                if parsed.controller.is_some() {
                    return Err("oracle:unsupported-cost-modifier");
                }
            } else if !matches!(parsed.controller, None | Some(ControllerRelation::You)) {
                return Err("oracle:unsupported-cost-modifier");
            }
            parsed.controller = None;
            (
                lower_spell_filter(&parsed, vocabulary)?,
                own_turn,
                each_player,
            )
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
            if f.controller != Some(ControllerRelation::You) {
                return Err("oracle:unsupported-cost-modifier");
            }
            let mut stripped = f.as_ref().clone();
            stripped.controller = None;
            (lower_spell_filter(&stripped, vocabulary)?, false, false)
        }
        _ => return Err("oracle:unsupported-cost-modifier"),
    };
    let (cond_own_turn, rem_cond) = if each_player {
        (false, condition)
    } else {
        split_is_your_turn_condition(condition)
    };
    push_conditional_static(
        program,
        StaticContinuousAbility::CostModifier {
            id: ids.static_id(),
            modifier: SpellCostModifier::FirstSpellRule {
                filter,
                own_turn_only: cond_own_turn || subj_own_turn,
                each_player,
                free: false,
                pay_life_mv: false,
                discount,
            },
        },
        rem_cond,
        vocabulary,
    )
}

fn try_lower_first_spell_permission(
    p_low: &str,
    filter: &ObjectFilterAst,
    condition: Option<&ConditionAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<Result<(), &'static str>> {
    let body = p_low.strip_prefix("the first ")?;
    let (before_alt, free, pay_life_mv, discount, also_flash) = if let Some(b) =
        body.strip_suffix(" may be cast without paying its mana cost")
    {
        (b, true, false, 0u8, false)
    } else if let Some(b) = body
        .strip_suffix(
            " may be cast by paying life equal to its mana value rather than paying its mana cost",
        )
        .or_else(|| {
            body.strip_suffix(
                " may be cast by paying life equal to its mana value rather than pay its mana cost",
            )
        })
    {
        (b, false, true, 0u8, false)
    } else {
        let b = body.strip_suffix(" can be cast as though it had flash")?;
        if let Some((head, cost_tail)) = b.rsplit_once(" costs ")
            && let Some(delta_raw) = cost_tail.strip_suffix(" less to cast and")
            && let Some(n) = parse_generic_mana_amount(delta_raw.trim()).filter(|&n| n > 0)
        {
            (head, false, false, n, true)
        } else {
            (b, false, false, 0u8, true)
        }
    };
    let (each_player, subj_own_turn) = if let Some((_, tail)) = before_alt.split_once(" you cast") {
        let own_turn = match tail.trim() {
            "" | "each turn" => false,
            "during each of your turns" | "on each of your turns" | "during your turn" => true,
            _ => return None,
        };
        if !matches!(filter.controller, None | Some(ControllerRelation::You)) {
            return None;
        }
        (false, own_turn)
    } else if let Some((_, tail)) = before_alt
        .split_once(" each player casts")
        .or_else(|| before_alt.split_once(" a player casts"))
    {
        let own_turn = match tail.trim() {
            "" | "each turn" => false,
            "during each of their turns" | "on each of their turns" | "during their turn" => true,
            _ => return None,
        };
        if filter.controller.is_some() {
            return None;
        }
        (true, own_turn)
    } else {
        return None;
    };
    let mut stripped = filter.clone();
    stripped.controller = None;
    let sf = lower_spell_filter(&stripped, vocabulary).ok()?;
    if !spell_filter_supported_by_cast_alt_cost(&sf) {
        return None;
    }
    if also_flash
        && (!spell_filter_supported_by_ordinal_this_turn(&sf) || (each_player && subj_own_turn))
    {
        return None;
    }
    let (cond_own_turn, rem_cond) = if each_player {
        (false, condition)
    } else {
        split_is_your_turn_condition(condition)
    };
    let mut probe = program.clone();
    if (free || pay_life_mv || discount > 0)
        && let Err(err) = push_conditional_static(
            &mut probe,
            StaticContinuousAbility::CostModifier {
                id: ids.static_id(),
                modifier: SpellCostModifier::FirstSpellRule {
                    filter: sf,
                    own_turn_only: cond_own_turn || subj_own_turn,
                    each_player,
                    free,
                    pay_life_mv,
                    discount,
                },
            },
            rem_cond,
            vocabulary,
        )
    {
        return Some(Err(err));
    }
    if also_flash {
        let flash_cond_owned;
        let flash_cond = if cond_own_turn || subj_own_turn {
            flash_cond_owned = match rem_cond {
                Some(c) => {
                    ConditionAst::And(Box::new(ConditionAst::IsYourTurn), Box::new(c.clone()))
                }
                None => ConditionAst::IsYourTurn,
            };
            Some(&flash_cond_owned)
        } else {
            rem_cond
        };
        if let Err(err) = push_conditional_static(
            &mut probe,
            StaticContinuousAbility::CastPermission {
                id: ids.static_id(),
                permission: CastPermissionRule::Flash {
                    any_player: each_player,
                    matching: sf.ordinal_this_turn(NonZeroU32::MIN),
                },
            },
            flash_cond,
            vocabulary,
        ) {
            return Some(Err(err));
        }
    }
    *program = probe;
    Some(Ok(()))
}

fn spell_filter_supported_by_cast_alt_cost(sf: &SpellFilter) -> bool {
    use spellwake_engine::program::{SpellSpecialFilter, SpellTargetShape};
    const ALLOWED_FLAGS: ObjectFilterFlags = ObjectFilterFlags::HISTORIC
        .union(ObjectFilterFlags::NON_HISTORIC)
        .union(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
        .union(ObjectFilterFlags::SADDLED)
        .union(ObjectFilterFlags::HAS_ADVENTURE)
        .union(ObjectFilterFlags::HAS_X_IN_COST)
        .union(ObjectFilterFlags::HAS_TOXIC)
        .union(ObjectFilterFlags::NO_ABILITIES)
        .union(ObjectFilterFlags::COMMANDER_ONLY)
        .union(ObjectFilterFlags::NOT_COMMANDER)
        .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_TYPE);
    if sf.target_shape != SpellTargetShape::Any || sf.ordinal_this_turn.is_some() {
        return false;
    }
    match sf.special {
        SpellSpecialFilter::AuraOrEquipment | SpellSpecialFilter::ChosenEntryCreatureType => {
            return sf.object == ObjectFilter::any();
        }
        SpellSpecialFilter::None => {}
        SpellSpecialFilter::AboveExperience | SpellSpecialFilter::CopyableInstruction => {
            return false;
        }
    }
    let clause_ok = |c: ObjectClause| {
        matches!(
            c.subtypes,
            SubtypeFilter::Any
                | SubtypeFilter::Has(_)
                | SubtypeFilter::Lacks(_)
                | SubtypeFilter::AnyOf(_)
                | SubtypeFilter::NoneOf(_)
                | SubtypeFilter::AnyOfList(_)
                | SubtypeFilter::AllOfList(_)
                | SubtypeFilter::NoneOfList(_)
                | SubtypeFilter::Land(_)
                | SubtypeFilter::NotLand(_)
                | SubtypeFilter::Enchantment(_)
                | SubtypeFilter::Spell(_)
                | SubtypeFilter::Artifact(_)
                | SubtypeFilter::Planeswalker(_)
                | SubtypeFilter::Aura
                | SubtypeFilter::AnyLandMask(_)
                | SubtypeFilter::NotLandMask(_)
        ) && c.controller == ControllerFilter::Any
            && c.owner == ControllerFilter::Any
            && c.source_relation == SourceRelation::Any
            && c.combat == CombatFilter::Any
            && matches!(c.power, PowerFilter::Any | PowerFilter::AtLeast(_))
            && c.toughness == ToughnessFilter::Any
            && matches!(
                c.mana_value,
                ManaValueFilter::Any
                    | ManaValueFilter::AtMost(_)
                    | ManaValueFilter::AtLeast(_)
                    | ManaValueFilter::Equal(_)
                    | ManaValueFilter::Even
                    | ManaValueFilter::Odd
            )
            && c.keyword.is_none()
            && c.forbidden_keyword.is_none()
            && c.tapped.is_none()
            && c.token.is_none()
            && c.required_counter_kind.is_none()
            && c.forbidden_counter_kind.is_none()
            && ALLOWED_FLAGS.union(c.flags) == ALLOWED_FLAGS
    };
    match sf.object {
        ObjectFilter::Clause(c) => clause_ok(c),
        ObjectFilter::Either(a, b) => clause_ok(a) && clause_ok(b),
    }
}

fn classify_direct_alt_cast_permission(p_low: &str) -> Option<bool> {
    match p_low {
        "from your hand without paying their mana costs"
        | "from your hand without paying its mana cost"
        | "without paying their mana costs"
        | "without paying its mana cost" => Some(false),
        "from your hand by paying life equal to its mana value rather than paying its mana cost"
        | "from your hand by paying life equal to its mana value rather than pay its mana cost"
        | "from your hand by paying life equal to their mana values rather than paying their mana costs"
        | "from your hand by paying life equal to their mana values rather than pay their mana costs"
        | "by paying life equal to its mana value rather than paying its mana cost"
        | "by paying life equal to its mana value rather than pay its mana cost"
        | "by paying life equal to their mana values rather than paying their mana costs"
        | "by paying life equal to their mana values rather than pay their mana costs" => {
            Some(true)
        }
        _ => None,
    }
}

fn strip_cast_this_way_alt_cost_suffix(p_low: &str) -> Option<(&str, bool)> {
    for suffix in [
        ". if you cast a spell this way, pay life equal to its mana value rather than pay its mana cost",
        ". if you cast a spell this way, pay life equal to its mana value rather than paying its mana cost",
        ". if you cast a spell this way, you pay life equal to its mana value rather than pay its mana cost",
        ". if you cast a spell this way, you pay life equal to its mana value rather than paying its mana cost",
        ". if you cast a spell this way, pay life equal to that spell's mana value rather than pay its mana cost",
        ". if you cast a spell this way, pay life equal to that spell's mana value rather than paying its mana cost",
        ". if you cast a spell this way, you pay life equal to that spell's mana value rather than pay its mana cost",
        ". if you cast a spell this way, you pay life equal to that spell's mana value rather than paying its mana cost",
    ] {
        if let Some(prefix) = p_low.strip_suffix(suffix) {
            return Some((prefix.trim(), true));
        }
    }
    for suffix in [
        ". if you cast a spell this way, you cast it without paying its mana cost",
        ". if you cast a spell this way, you may cast it without paying its mana cost",
    ] {
        if let Some(prefix) = p_low.strip_suffix(suffix) {
            return Some((prefix.trim(), false));
        }
    }
    None
}

fn extract_cast_this_way_alt_cost_followup(ins: &[InstructionAst]) -> Option<bool> {
    let [
        InstructionAst::GrantCastPermission {
            player: PlayerRefAst::You,
            cast_only: true,
            subject,
            free,
            duration,
            count_limit: None,
            alt_cost,
        },
    ] = ins
    else {
        return None;
    };
    if !duration.eq_ignore_ascii_case("now") {
        return None;
    }
    let SubjectRefAst::Anaphora(raw) = subject.as_ref() else {
        return None;
    };
    let subj_low = raw.trim().to_ascii_lowercase();
    match (*free, alt_cost) {
        (true, None) if matches!(subj_low.as_str(), "that spell" | "it") => Some(false),
        (false, Some(spellwake_oracle_compiler::ast::AltCastCostAst::PayLifeEqualToManaValue))
            if matches!(subj_low.as_str(), "that spell" | "it") =>
        {
            Some(true)
        }
        // clauses.rs emits free=false, alt_cost=None, subject="that spell" solely for
        // "if you cast a spell this way, pay life equal to its mana value rather than pay its mana cost".
        (false, None) if subj_low == "that spell" => Some(true),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn try_lower_play_or_cast_with_alt_cost(
    base_perm_raw: &str,
    cast_only: bool,
    filter: &ObjectFilterAst,
    condition: Option<&ConditionAst>,
    once_each_turn: bool,
    pay_life_mv: bool,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<Result<(), &'static str>> {
    let base_low = base_perm_raw
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();

    let (perm_rule, rem_cond, alt_filter, alt_once, alt_own_turn) = if let Some((perm, rem_c)) =
        try_lower_library_top_filtered_permission(
            &base_low,
            cast_only,
            filter,
            once_each_turn,
            condition,
            vocabulary,
        ) {
        let (sf, once, own_turn) = match &perm {
            CastPermissionRule::PlayLibraryTopFiltered {
                filter: obj,
                once_each_turn: o,
                own_turn_only: own,
                ..
            } => (SpellFilter::from_object(**obj).from_own_library(), *o, *own),
            _ => return None,
        };
        (perm, rem_c, sf, once, own_turn)
    } else if !once_each_turn
        && matches!(
            base_low.as_str(),
            "from the top of your library"
                | "from the top of your library at any time"
                | "you may play lands and cast spells from the top of your library"
                | "you may cast instant and sorcery spells from the top of your library"
        )
        && matches!(filter.zone, None | Some(ZoneFilter::YourLibrary))
    {
        let (own_turn, rem_c) = split_is_your_turn_condition(condition);
        if filter.any_card || is_unrestricted_filter(filter) {
            let perm = CastPermissionRule::PlayLibraryTop(if cast_only {
                LibraryTopPermission::Nonland
            } else {
                LibraryTopPermission::All
            });
            (
                perm,
                rem_c,
                SpellFilter::any().from_own_library(),
                false,
                own_turn,
            )
        } else if filter_only_has(
            filter,
            FilterAllow {
                types: true,
                zone: true,
                ..FilterAllow::NONE
            },
        ) && filter.any_types != AstCardTypeMask::LAND
        {
            let (perm, sf) = if filter.any_types == AstCardTypeMask::CREATURE {
                (
                    CastPermissionRule::PlayLibraryTop(LibraryTopPermission::Creature),
                    SpellFilter::creature().from_own_library(),
                )
            } else if filter.any_types == AstCardTypeMask::ARTIFACT {
                (
                    CastPermissionRule::PlayLibraryTop(LibraryTopPermission::Artifact),
                    SpellFilter::of_type(CardType::Artifact).from_own_library(),
                )
            } else if filter.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY) {
                (
                    CastPermissionRule::PlayLibraryTop(LibraryTopPermission::InstantOrSorcery),
                    SpellFilter::instant_or_sorcery().from_own_library(),
                )
            } else {
                let mut inner = filter.clone();
                inner.zone = None;
                let obj = lower_object_filter(&inner, vocabulary).ok()?;
                (
                    CastPermissionRule::PlayLibraryTopMatching {
                        lands_allowed: !cast_only,
                        filter: Box::new(obj),
                    },
                    SpellFilter::from_object(obj).from_own_library(),
                )
            };
            (perm, rem_c, sf, false, own_turn)
        } else {
            let mut inner = filter.clone();
            inner.zone = None;
            let obj = lower_object_filter(&inner, vocabulary).ok()?;
            (
                CastPermissionRule::PlayLibraryTopMatching {
                    lands_allowed: !cast_only,
                    filter: Box::new(obj),
                },
                rem_c,
                SpellFilter::from_object(obj).from_own_library(),
                false,
                own_turn,
            )
        }
    } else if !once_each_turn
        && !cast_only
        && matches!(filter.zone, None | Some(ZoneFilter::YourLibrary))
        && (filter.any_card || is_unrestricted_filter(filter))
        && let Some(flt_raw) = base_low
            .strip_prefix("you may play lands and cast ")
            .and_then(|rest| rest.strip_suffix(" from the top of your library"))
        && let Some(obj) = parse_library_top_cast_filter(flt_raw.trim(), vocabulary)
    {
        let (own_turn, rem_c) = split_is_your_turn_condition(condition);
        (
            CastPermissionRule::PlayLibraryTopMatching {
                lands_allowed: true,
                filter: Box::new(obj),
            },
            rem_c,
            SpellFilter::from_object(obj).from_own_library(),
            false,
            own_turn,
        )
    } else if !once_each_turn
        && matches!(filter.zone, None | Some(ZoneFilter::YourLibrary))
        && (filter.any_card || is_unrestricted_filter(filter))
        && let Some(head) = base_low
            .strip_suffix(" from the top of your library")
            .or_else(|| base_low.strip_suffix(" from the top of your library at any time"))
        && !head.contains("without paying")
        && !head.contains(" by ")
        && let Some((lands_allowed, obj, false)) = parse_play_or_cast_clause(head, vocabulary)
    {
        let (own_turn, rem_c) = split_is_your_turn_condition(condition);
        (
            CastPermissionRule::PlayLibraryTopMatching {
                lands_allowed,
                filter: Box::new(obj),
            },
            rem_c,
            SpellFilter::from_object(obj).from_own_library(),
            false,
            own_turn,
        )
    } else if let Some((perm, rem_c)) = try_lower_graveyard_matching_permission_ext(
        &base_low,
        cast_only,
        filter,
        once_each_turn,
        condition,
        vocabulary,
    ) {
        let (sf, once, own_turn) = match &perm {
            CastPermissionRule::GraveyardMatching {
                filter: obj,
                once_each_turn: o,
                ..
            } => (SpellFilter::from_object(**obj).from_graveyard(), *o, false),
            CastPermissionRule::GraveyardMatchingOwnTurn {
                filter: obj,
                once_each_turn: o,
                ..
            } => (SpellFilter::from_object(**obj).from_graveyard(), *o, true),
            _ => return None,
        };
        (perm, rem_c, sf, once, own_turn)
    } else {
        return None;
    };

    if !spell_filter_supported_by_cast_alt_cost(&alt_filter) {
        return None;
    }

    let mut probe = program.clone();
    let perm_cond = if alt_own_turn
        && matches!(
            perm_rule,
            CastPermissionRule::PlayLibraryTop(_)
                | CastPermissionRule::PlayLibraryTopMatching { .. }
        ) {
        condition
    } else {
        rem_cond
    };
    if let Err(err) = push_conditional_static(
        &mut probe,
        StaticContinuousAbility::CastPermission {
            id: ids.static_id(),
            permission: perm_rule,
        },
        perm_cond,
        vocabulary,
    ) {
        return Some(Err(err));
    }
    if let Err(err) = push_conditional_static(
        &mut probe,
        StaticContinuousAbility::CostModifier {
            id: ids.static_id(),
            modifier: SpellCostModifier::AlternativeForMatching {
                filter: alt_filter,
                mana: ManaCost::free(),
                once_each_turn: alt_once,
                own_turn_only: alt_own_turn,
                pay_life_mv,
                each_player: false,
            },
        },
        rem_cond,
        vocabulary,
    ) {
        return Some(Err(err));
    }
    *program = probe;
    Some(Ok(()))
}

fn try_lower_alt_matching_permission<'a>(
    p_low: &str,
    filter: &ObjectFilterAst,
    condition: Option<&'a ConditionAst>,
    once_each_turn: bool,
    pay_life_mv: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(SpellCostModifier, Option<&'a ConditionAst>)> {
    if !matches!(filter.controller, None | Some(ControllerRelation::You)) {
        return None;
    }
    let from_hand = p_low.starts_with("from your hand ");
    if from_hand {
        if !matches!(filter.zone, None | Some(ZoneFilter::YourHand)) {
            return None;
        }
    } else if !matches!(
        filter.zone,
        None | Some(ZoneFilter::YourHand | ZoneFilter::Stack)
    ) {
        return None;
    }
    let mut stripped = filter.clone();
    stripped.controller = None;
    if from_hand {
        stripped.zone = Some(ZoneFilter::YourHand);
    }
    let sf = lower_spell_filter(&stripped, vocabulary).ok()?;
    if !spell_filter_supported_by_cast_alt_cost(&sf) {
        return None;
    }
    let (own_turn_only, rem_cond) = split_is_your_turn_condition(condition);
    Some((
        SpellCostModifier::AlternativeForMatching {
            filter: sf,
            mana: ManaCost::free(),
            once_each_turn,
            own_turn_only,
            pay_life_mv,
            each_player: false,
        },
        rem_cond,
    ))
}

fn spell_filter_supported_by_ordinal_this_turn(sf: &SpellFilter) -> bool {
    use spellwake_engine::program::{SingleSubtype, SpellSpecialFilter, SpellTargetShape};
    if sf.target_shape != SpellTargetShape::Any || sf.special != SpellSpecialFilter::None {
        return false;
    }
    let clause_ok = |c: ObjectClause| {
        let subtypes_ok = match c.subtypes {
            SubtypeFilter::Any
            | SubtypeFilter::Has(_)
            | SubtypeFilter::Lacks(_)
            | SubtypeFilter::AnyOf(_)
            | SubtypeFilter::NoneOf(_) => true,
            SubtypeFilter::AnyOfList(list)
            | SubtypeFilter::AllOfList(list)
            | SubtypeFilter::NoneOfList(list) => list
                .iter()
                .all(|item| matches!(item, SingleSubtype::Creature(_))),
            _ => false,
        };
        c.exact_name.is_none()
            && subtypes_ok
            && c.controller == ControllerFilter::Any
            && c.owner == ControllerFilter::Any
            && c.source_relation == SourceRelation::Any
            && c.combat == CombatFilter::Any
            && c.power == PowerFilter::Any
            && c.toughness == ToughnessFilter::Any
            && c.mana_value == ManaValueFilter::Any
            && c.keyword.is_none()
            && c.forbidden_keyword.is_none()
            && c.tapped.is_none()
            && c.token.is_none()
            && c.required_counter_kind.is_none()
            && c.forbidden_counter_kind.is_none()
            && ObjectFilterFlags::HISTORIC.union(c.flags) == ObjectFilterFlags::HISTORIC
    };
    match sf.object {
        ObjectFilter::Clause(c) => clause_ok(c),
        ObjectFilter::Either(a, b) => clause_ok(a) && clause_ok(b),
    }
}

fn split_leading_flash_timing_prefix(p_low: &str) -> Option<(ConditionAst, &str)> {
    for (pref, cond) in [
        (
            "during each opponent's end step, ",
            ConditionAst::IsPhaseOrStep {
                player: Some(PlayerRefAst::EachOpponent),
                phase: "end step".to_string(),
                first_only: false,
            },
        ),
        (
            "during an opponent's end step, ",
            ConditionAst::IsPhaseOrStep {
                player: Some(PlayerRefAst::AnyOpponent),
                phase: "end step".to_string(),
                first_only: false,
            },
        ),
        (
            "during opponents' end steps, ",
            ConditionAst::IsPhaseOrStep {
                player: Some(PlayerRefAst::EachOpponent),
                phase: "end step".to_string(),
                first_only: false,
            },
        ),
        (
            "during each opponent's turn, ",
            ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)),
        ),
        (
            "during opponents' turns, ",
            ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)),
        ),
        ("during each of your turns, ", ConditionAst::IsYourTurn),
        ("during your turn, ", ConditionAst::IsYourTurn),
    ] {
        if let Some(rest) = p_low.strip_prefix(pref) {
            return Some((cond, rest.trim()));
        }
    }
    None
}

fn parse_flash_permission_clause(
    inner_perm: &str,
    filter: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(bool, SpellFilter)> {
    let unrestricted_outer = filter.any_card || is_unrestricted_filter(filter);
    if !unrestricted_outer {
        return None;
    }
    if inner_perm == "you may cast spells as though they had flash" {
        return Some((false, SpellFilter::any()));
    }
    if inner_perm == "any player may cast spells as though they had flash" {
        return Some((true, SpellFilter::any()));
    }
    let (any_player, flt_raw) = if let Some(rest) = inner_perm
        .strip_prefix("you may cast ")
        .and_then(|s| s.strip_suffix(" as though they had flash"))
    {
        (false, rest.trim())
    } else {
        let rest = inner_perm
            .strip_prefix("any player may cast ")
            .and_then(|s| s.strip_suffix(" as though they had flash"))?;
        (true, rest.trim())
    };
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let mut parsed =
        spellwake_oracle_compiler::filter::parse_object_filter(flt_raw, &compiler_vocab).ok()?;
    if !any_player && parsed.controller == Some(ControllerRelation::You) {
        parsed.controller = None;
    } else if parsed.controller.is_some() {
        return None;
    }
    let matching = lower_spell_filter(&parsed, vocabulary).ok()?;
    if !spell_filter_supported_by_cast_alt_cost(&matching) {
        return None;
    }
    Some((any_player, matching))
}

fn strip_cast_this_way_flash_suffix(p_low: &str) -> Option<&str> {
    for suffix in [
        ". if you cast a spell this way, you may cast it as though it had flash",
        ". if you cast a spell this way, cast it as though it had flash",
        ". if you cast a spell this way, it may be cast as though it had flash",
    ] {
        if let Some(prefix) = p_low.strip_suffix(suffix) {
            return Some(prefix.trim());
        }
    }
    None
}

fn extract_cast_this_way_flash_followup(ins: &[InstructionAst]) -> bool {
    let [
        InstructionAst::IfCondition {
            condition,
            then_body,
        },
    ] = ins
    else {
        return false;
    };
    let ConditionAst::PriorActionOccurred {
        action,
        subject_filter,
        min_count: 1,
    } = condition.as_ref()
    else {
        return false;
    };
    if action != "cast_this_way" {
        return false;
    }
    if let Some(sf) = subject_filter.as_deref()
        && !(sf.any_spell && is_unrestricted_filter(sf))
    {
        return false;
    }
    let inner = match then_body.as_slice() {
        [InstructionAst::Optional(inner)] => inner.as_slice(),
        other => other,
    };
    matches!(
        inner,
        [InstructionAst::TemporaryStatic {
            static_ability,
            duration: DurationAst::Instant,
        }] if matches!(
            static_ability.as_ref(),
            StaticAbilityAst::GrantKeywords {
                subject,
                keywords,
                condition: None,
                paired_rule: None,
            } if matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(r)
                    if matches!(r.trim().to_ascii_lowercase().as_str(), "it" | "that spell")
            ) && keywords.len() == 1
                && keywords[0].trim().eq_ignore_ascii_case("flash")
        )
    ) || matches!(
        inner,
        [InstructionAst::GrantModifiedCastPermission(spec)]
            if spec.player == PlayerRefAst::You
                && spec.cast_only
                && !spec.free
                && spec.as_though_flash
                && !spec.face_down_2_2
                && !spec.spend_mana_as_any_color
                && spec.additional_cost.is_none()
                && matches!(spec.duration.as_str(), "now" | "")
                && matches!(
                    spec.subject.as_ref(),
                    SubjectRefAst::Anaphora(r)
                        if matches!(r.trim().to_ascii_lowercase().as_str(), "it" | "that spell")
                )
    )
}

#[allow(clippy::too_many_arguments)]
fn try_lower_play_or_cast_with_flash(
    base_perm_raw: &str,
    cast_only: bool,
    filter: &ObjectFilterAst,
    condition: Option<&ConditionAst>,
    once_each_turn: bool,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<Result<(), &'static str>> {
    let base_low = base_perm_raw
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();

    let (perm_rule, rem_cond, flash_filter, flash_own_turn) = if let Some((perm, rem_c)) =
        try_lower_library_top_filtered_permission(
            &base_low,
            cast_only,
            filter,
            once_each_turn,
            condition,
            vocabulary,
        ) {
        let (sf, own_turn) = match &perm {
            CastPermissionRule::PlayLibraryTopFiltered {
                filter: obj,
                own_turn_only: own,
                ..
            } => (SpellFilter::from_object(**obj).from_own_library(), *own),
            _ => return None,
        };
        (perm, rem_c, sf, own_turn)
    } else if !once_each_turn
        && matches!(
            base_low.as_str(),
            "from the top of your library"
                | "from the top of your library at any time"
                | "you may play lands and cast spells from the top of your library"
                | "you may cast instant and sorcery spells from the top of your library"
        )
        && matches!(filter.zone, None | Some(ZoneFilter::YourLibrary))
    {
        let (own_turn, rem_c) = split_is_your_turn_condition(condition);
        if filter.any_card || is_unrestricted_filter(filter) {
            let perm = CastPermissionRule::PlayLibraryTop(if cast_only {
                LibraryTopPermission::Nonland
            } else {
                LibraryTopPermission::All
            });
            (perm, rem_c, SpellFilter::any().from_own_library(), own_turn)
        } else if filter_only_has(
            filter,
            FilterAllow {
                types: true,
                zone: true,
                ..FilterAllow::NONE
            },
        ) && filter.any_types != AstCardTypeMask::LAND
        {
            let (perm, sf) = if filter.any_types == AstCardTypeMask::CREATURE {
                (
                    CastPermissionRule::PlayLibraryTop(LibraryTopPermission::Creature),
                    SpellFilter::creature().from_own_library(),
                )
            } else if filter.any_types == AstCardTypeMask::ARTIFACT {
                (
                    CastPermissionRule::PlayLibraryTop(LibraryTopPermission::Artifact),
                    SpellFilter::of_type(CardType::Artifact).from_own_library(),
                )
            } else if filter.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY) {
                (
                    CastPermissionRule::PlayLibraryTop(LibraryTopPermission::InstantOrSorcery),
                    SpellFilter::instant_or_sorcery().from_own_library(),
                )
            } else {
                let mut inner = filter.clone();
                inner.zone = None;
                let obj = lower_object_filter(&inner, vocabulary).ok()?;
                (
                    CastPermissionRule::PlayLibraryTopMatching {
                        lands_allowed: !cast_only,
                        filter: Box::new(obj),
                    },
                    SpellFilter::from_object(obj).from_own_library(),
                )
            };
            (perm, rem_c, sf, own_turn)
        } else {
            let mut inner = filter.clone();
            inner.zone = None;
            let obj = lower_object_filter(&inner, vocabulary).ok()?;
            (
                CastPermissionRule::PlayLibraryTopMatching {
                    lands_allowed: !cast_only,
                    filter: Box::new(obj),
                },
                rem_c,
                SpellFilter::from_object(obj).from_own_library(),
                own_turn,
            )
        }
    } else if !once_each_turn
        && !cast_only
        && matches!(filter.zone, None | Some(ZoneFilter::YourLibrary))
        && (filter.any_card || is_unrestricted_filter(filter))
        && let Some(flt_raw) = base_low
            .strip_prefix("you may play lands and cast ")
            .and_then(|rest| rest.strip_suffix(" from the top of your library"))
        && let Some(obj) = parse_library_top_cast_filter(flt_raw.trim(), vocabulary)
    {
        let (own_turn, rem_c) = split_is_your_turn_condition(condition);
        (
            CastPermissionRule::PlayLibraryTopMatching {
                lands_allowed: true,
                filter: Box::new(obj),
            },
            rem_c,
            SpellFilter::from_object(obj).from_own_library(),
            own_turn,
        )
    } else if !once_each_turn
        && matches!(filter.zone, None | Some(ZoneFilter::YourLibrary))
        && (filter.any_card || is_unrestricted_filter(filter))
        && let Some(head) = base_low
            .strip_suffix(" from the top of your library")
            .or_else(|| base_low.strip_suffix(" from the top of your library at any time"))
        && !head.contains("without paying")
        && !head.contains(" by ")
        && let Some((lands_allowed, obj, false)) = parse_play_or_cast_clause(head, vocabulary)
    {
        let (own_turn, rem_c) = split_is_your_turn_condition(condition);
        (
            CastPermissionRule::PlayLibraryTopMatching {
                lands_allowed,
                filter: Box::new(obj),
            },
            rem_c,
            SpellFilter::from_object(obj).from_own_library(),
            own_turn,
        )
    } else {
        return None;
    };

    if !spell_filter_supported_by_cast_alt_cost(&flash_filter) {
        return None;
    }

    let mut probe = program.clone();
    let perm_cond = if flash_own_turn
        && matches!(
            perm_rule,
            CastPermissionRule::PlayLibraryTop(_)
                | CastPermissionRule::PlayLibraryTopMatching { .. }
        ) {
        condition
    } else {
        rem_cond
    };
    if let Err(err) = push_conditional_static(
        &mut probe,
        StaticContinuousAbility::CastPermission {
            id: ids.static_id(),
            permission: perm_rule,
        },
        perm_cond,
        vocabulary,
    ) {
        return Some(Err(err));
    }
    let flash_cond_owned;
    let flash_cond = if flash_own_turn {
        flash_cond_owned = match rem_cond {
            Some(c) => ConditionAst::And(Box::new(ConditionAst::IsYourTurn), Box::new(c.clone())),
            None => ConditionAst::IsYourTurn,
        };
        Some(&flash_cond_owned)
    } else {
        rem_cond
    };
    if let Err(err) = push_conditional_static(
        &mut probe,
        StaticContinuousAbility::CastPermission {
            id: ids.static_id(),
            permission: CastPermissionRule::Flash {
                any_player: false,
                matching: flash_filter,
            },
        },
        flash_cond,
        vocabulary,
    ) {
        return Some(Err(err));
    }
    *program = probe;
    Some(Ok(()))
}

fn is_valid_cast_extra_cost(cost: &ExtraActivationCost) -> bool {
    !matches!(
        cost,
        ExtraActivationCost::TapSource
            | ExtraActivationCost::UntapSource
            | ExtraActivationCost::SacrificeSource
            | ExtraActivationCost::DiscardSource
            | ExtraActivationCost::ExertSource
            | ExtraActivationCost::ExileSource {
                from_graveyard: false,
                from_hand: false,
            }
    )
}

fn normalize_gerund_cost_clause(raw: &str) -> String {
    let normalize_verb = |s: &str| -> String {
        let trimmed = s.trim();
        for (gerund, imp) in [
            ("sacrificing ", "sacrifice "),
            ("discarding ", "discard "),
            ("exiling ", "exile "),
            ("paying ", "pay "),
            ("returning ", "return "),
            ("tapping ", "tap "),
            ("untapping ", "untap "),
            ("removing ", "remove "),
            ("milling ", "mill "),
            ("revealing ", "reveal "),
        ] {
            if let Some(rest) = trimmed.strip_prefix(gerund) {
                return format!("{imp}{rest}");
            }
        }
        trimmed.to_string()
    };
    let mut joined = String::new();
    for (idx, chunk) in raw.split(" and ").enumerate() {
        if idx > 0 {
            joined.push_str(" and ");
        }
        let mut sub_joined = String::new();
        for (sub_idx, piece) in chunk.split(", ").enumerate() {
            if sub_idx > 0 {
                sub_joined.push_str(", ");
            }
            sub_joined.push_str(&normalize_verb(piece));
        }
        joined.push_str(&sub_joined);
    }
    joined
}

fn parse_gerund_extra_cast_costs(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<ExtraActivationCost>> {
    let normalized = normalize_gerund_cost_clause(raw);
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let atoms =
        spellwake_oracle_compiler::clauses::parse_cost_list(&normalized, &compiler_vocab).ok()?;
    if atoms.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(atoms.len());
    for atom in &atoms {
        let extra = lower_extra_cost_atom(atom, vocabulary).ok()?;
        if !is_valid_cast_extra_cost(&extra) {
            return None;
        }
        out.push(extra);
    }
    Some(out)
}

fn parse_gerund_alternative_cast_cost(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(ManaCost, Vec<ExtraActivationCost>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(after_pay) = trimmed.strip_prefix("paying ") {
        let after_pay = after_pay.trim();
        if let Some(mana) = parse_fixed_mana_cost(&after_pay.to_ascii_uppercase()) {
            return Some((mana, Vec::new()));
        }
        for sep in [" and ", ", "] {
            if let Some((mana_raw, rest_raw)) = after_pay.split_once(sep)
                && let Some(mana) = parse_fixed_mana_cost(&mana_raw.trim().to_ascii_uppercase())
            {
                let rest = rest_raw.trim();
                let rest = rest.strip_prefix("and ").unwrap_or(rest).trim();
                if let Some(extra) = parse_gerund_extra_cast_costs(rest, vocabulary) {
                    return Some((mana, extra));
                }
            }
        }
    }
    let normalized = normalize_gerund_cost_clause(trimmed);
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let atoms =
        spellwake_oracle_compiler::clauses::parse_cost_list(&normalized, &compiler_vocab).ok()?;
    if atoms.is_empty() {
        return None;
    }
    let mut mana: Option<ManaCost> = None;
    let mut extra = Vec::new();
    for atom in &atoms {
        if let CostAtomAst::PayMana(m) = atom {
            if mana.is_some() {
                return None;
            }
            mana = Some(parse_fixed_mana_cost(&m.to_ascii_uppercase())?);
        } else {
            let lowered = lower_extra_cost_atom(atom, vocabulary).ok()?;
            if !is_valid_cast_extra_cost(&lowered) {
                return None;
            }
            extra.push(lowered);
        }
    }
    if mana.is_none() && extra.is_empty() {
        return None;
    }
    Some((mana.unwrap_or_else(ManaCost::free), extra))
}

fn lower_general_self_alternative_cast_cost(
    cost: &[CostAtomAst],
    condition: Option<&ConditionAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
    if cost.is_empty() && condition.is_none() {
        return Err("oracle:unsupported-alternative-cast-cost");
    }
    let mut mana: Option<ManaCost> = None;
    let mut extra = Vec::new();
    for atom in cost {
        if let CostAtomAst::PayMana(raw) = atom {
            if mana.is_some() {
                return Err("oracle:unsupported-alternative-cast-cost");
            }
            let parsed = parse_fixed_mana_cost(&raw.to_ascii_uppercase())
                .ok_or("oracle:unsupported-alternative-cast-cost")?;
            mana = Some(parsed);
        } else {
            let lowered = lower_extra_cost_atom(atom, vocabulary)
                .map_err(|_| "oracle:unsupported-alternative-cast-cost")?;
            if !is_valid_cast_extra_cost(&lowered) {
                return Err("oracle:unsupported-alternative-cast-cost");
            }
            extra.push(lowered);
        }
    }
    let mana_cost = mana.unwrap_or_else(ManaCost::free);
    let lowered_cond = match condition {
        Some(cond) => {
            let mut dummy = BodyBuilder::new();
            let rc = lower_resolution_condition(cond, &mut dummy, vocabulary, false, None)
                .map_err(|_| "oracle:unsupported-alternative-cast-cost")?;
            if dummy.target != BodyTarget::None
                || !rc.is_world_evaluable()
                || rc.validate_world(vocabulary).is_err()
            {
                return Err("oracle:unsupported-alternative-cast-cost");
            }
            Some(Box::new(rc))
        }
        None => None,
    };
    program.conditional_cost = Some(ConditionalCastCost::Alternative {
        condition: lowered_cond,
        mana: mana_cost,
        extra,
    });
    Ok(())
}

fn lower_turn_step_for_skip(step: TurnStepAst) -> Result<Step, &'static str> {
    match step {
        TurnStepAst::UntapStep => Ok(Step::Untap),
        TurnStepAst::Upkeep => Ok(Step::Upkeep),
        TurnStepAst::DrawStep => Ok(Step::Draw),
        TurnStepAst::PrecombatMain => Ok(Step::PrecombatMain),
        TurnStepAst::PostcombatMain => Ok(Step::PostcombatMain),
        TurnStepAst::BeginningOfCombat => Ok(Step::BeginningCombat),
        TurnStepAst::EndOfCombat => Ok(Step::EndCombat),
        TurnStepAst::EndStep => Ok(Step::End),
        TurnStepAst::CleanupStep => Ok(Step::Cleanup),
        TurnStepAst::MainPhase => Err("oracle:unsupported-static"),
    }
}

fn parse_relative_max_hand_size(cleaned: &str) -> Option<(PlayerRelation, i8)> {
    let (player, after) = if let Some(r) = cleaned.strip_prefix("your maximum hand size is ") {
        (PlayerRelation::Controller, r)
    } else if let Some(r) = cleaned
        .strip_prefix("each opponent's maximum hand size is ")
        .or_else(|| cleaned.strip_prefix("your opponents' maximum hand size is "))
        .or_else(|| cleaned.strip_prefix("opponents' maximum hand size is "))
    {
        (PlayerRelation::Opponent, r)
    } else {
        let r = cleaned
            .strip_prefix("each player's maximum hand size is ")
            .or_else(|| cleaned.strip_prefix("players' maximum hand size is "))?;
        (PlayerRelation::AnyParticipant, r)
    };
    let (sign, num_raw): (i8, &str) = if let Some(n) = after.strip_prefix("reduced by ") {
        (-1, n)
    } else {
        let n = after.strip_prefix("increased by ")?;
        (1, n)
    };
    let mag = parse_nonneg_u8_word(num_raw.trim())?;
    if mag == 0 {
        return None;
    }
    let mag_i8 = i8::try_from(mag).ok()?;
    let delta = mag_i8.checked_mul(sign)?;
    Some((player, delta))
}

fn parse_zone_clause_filter(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let trimmed = raw.trim().trim_end_matches('.').trim();
    if matches!(
        trimmed,
        "cards" | "a card" | "spells" | "a spell" | "the top card" | "top card"
    ) {
        return Some(ObjectFilter::any());
    }
    if matches!(trimmed, "lands" | "land cards" | "a land" | "a land card") {
        return Some(ObjectFilter::land());
    }
    parse_library_top_cast_filter(trimmed, vocabulary)
}

fn parse_play_or_cast_clause(
    clause: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(bool, ObjectFilter, bool)> {
    let c = clause.trim();
    if c == "you may play lands and cast spells" || c == "you may play a land or cast a spell" {
        return Some((true, ObjectFilter::any(), c.starts_with("you may play a ")));
    }
    if c == "you may play historic lands and cast historic spells" {
        return Some((
            true,
            ObjectFilter::Clause(ObjectClause {
                flags: ObjectFilterFlags::HISTORIC,
                ..ObjectClause::any()
            }),
            false,
        ));
    }
    if c == "you may play snow lands and cast snow spells" {
        return Some((
            true,
            ObjectFilter::Clause(ObjectClause {
                required_supertypes: spellwake_engine::program::SupertypeMask::SNOW,
                ..ObjectClause::any()
            }),
            false,
        ));
    }
    if let Some(rest) = c
        .strip_prefix("you may play lands and cast ")
        .or_else(|| c.strip_prefix("you may play land cards and cast "))
    {
        let obj = parse_zone_clause_filter(rest, vocabulary)?;
        return Some((true, obj, false));
    }
    if let Some(rest) = c
        .strip_prefix("you may play a land or cast ")
        .or_else(|| c.strip_prefix("you may play a land and cast "))
    {
        let obj = parse_zone_clause_filter(rest, vocabulary)?;
        let singular = rest.starts_with("a ") || rest.starts_with("an ");
        return Some((true, obj, singular));
    }
    if let Some(rest) = c.strip_prefix("you may play ") {
        if rest.contains(" and cast ") || rest.contains(" or cast ") {
            return None;
        }
        let singular = rest.starts_with("a ") || rest.starts_with("an ");
        let obj = parse_zone_clause_filter(rest, vocabulary)?;
        return Some((true, obj, singular));
    }
    if let Some(rest) = c.strip_prefix("you may cast ") {
        let singular = rest.starts_with("a ") || rest.starts_with("an ");
        let obj = parse_zone_clause_filter(rest, vocabulary)?;
        return Some((false, obj, singular));
    }
    None
}

fn try_lower_library_top_filtered_permission<'a>(
    p_low: &str,
    cast_only: bool,
    filter: &ObjectFilterAst,
    once_each_turn: bool,
    condition: Option<&'a ConditionAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(CastPermissionRule, Option<&'a ConditionAst>)> {
    if !matches!(filter.zone, None | Some(ZoneFilter::YourLibrary)) {
        return None;
    }
    let (cond_own_turn, rem_cond) = split_is_your_turn_condition(condition);
    let (pref_own_turn, inner_p) =
        if let Some(rest) = p_low.strip_prefix("during each of your turns, ") {
            (true, rest.trim())
        } else {
            (false, p_low)
        };
    let own_turn_only = cond_own_turn || pref_own_turn;

    if let Some(head) = inner_p
        .strip_suffix(" from the top of your library")
        .or_else(|| inner_p.strip_suffix(" from the top of your library at any time"))
        && !head.contains("without paying")
        && !head.contains(" by ")
        && let Some((lands_allowed, obj, singular)) = parse_play_or_cast_clause(head, vocabulary)
    {
        let is_legacy_unconditional_lands_and_cast = !own_turn_only
            && !once_each_turn
            && inner_p.starts_with("you may play lands and cast ")
            && !inner_p.starts_with("you may play lands and cast spells");
        if !own_turn_only && !once_each_turn && !is_legacy_unconditional_lands_and_cast {
            let needs_new = inner_p.starts_with("you may play land cards and cast ")
                || inner_p.starts_with("you may play a land or cast ")
                || inner_p.starts_with("you may play a land and cast ");
            if !needs_new {
                return None;
            }
        }
        return Some((
            CastPermissionRule::PlayLibraryTopFiltered {
                lands_allowed,
                filter: Box::new(obj),
                once_each_turn: once_each_turn || (pref_own_turn && singular),
                own_turn_only,
            },
            rem_cond,
        ));
    }

    if (once_each_turn || own_turn_only)
        && matches!(
            inner_p,
            "from the top of your library" | "from the top of your library at any time"
        )
    {
        let mut inner = filter.clone();
        inner.zone = None;
        let obj = if inner.any_card || is_unrestricted_filter(&inner) {
            ObjectFilter::any()
        } else {
            lower_object_filter(&inner, vocabulary).ok()?
        };
        return Some((
            CastPermissionRule::PlayLibraryTopFiltered {
                lands_allowed: !cast_only,
                filter: Box::new(obj),
                once_each_turn,
                own_turn_only,
            },
            rem_cond,
        ));
    }
    None
}

fn try_lower_graveyard_matching_permission_ext<'a>(
    p_low: &str,
    cast_only: bool,
    filter: &ObjectFilterAst,
    once_each_turn: bool,
    condition: Option<&'a ConditionAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(CastPermissionRule, Option<&'a ConditionAst>)> {
    if !matches!(filter.zone, None | Some(ZoneFilter::YourGraveyard)) {
        return None;
    }
    if p_low.contains(" ~ ")
        || p_low.contains("without paying")
        || p_low.contains(" by ")
        || p_low.contains("using its ")
    {
        return None;
    }
    let (cond_own_turn, rem_cond) = split_is_your_turn_condition(condition);
    let (pref_own_turn, inner_p) =
        if let Some(rest) = p_low.strip_prefix("during each of your turns, ") {
            (true, rest.trim())
        } else {
            (false, p_low)
        };
    let own_turn_only = cond_own_turn || pref_own_turn;

    let (lands_allowed, obj, effective_once) = if inner_p == "from your graveyard" {
        let mut inner = filter.clone();
        inner.zone = None;
        let obj = if inner.any_card && is_unrestricted_filter(&inner) {
            ObjectFilter::any()
        } else {
            lower_object_filter(&inner, vocabulary).ok()?
        };
        (!cast_only, obj, once_each_turn)
    } else {
        let head = inner_p.strip_suffix(" from your graveyard")?;
        let (parsed_lands, parsed_obj, singular) = parse_play_or_cast_clause(head, vocabulary)?;
        let effective_once = once_each_turn || (pref_own_turn && singular);
        if filter.any_card && is_unrestricted_filter(filter) {
            (parsed_lands, parsed_obj, effective_once)
        } else {
            let mut inner = filter.clone();
            inner.zone = None;
            let obj = lower_object_filter(&inner, vocabulary).ok()?;
            (parsed_lands || !cast_only, obj, effective_once)
        }
    };

    let rule = if own_turn_only {
        CastPermissionRule::GraveyardMatchingOwnTurn {
            lands_allowed,
            filter: Box::new(obj),
            once_each_turn: effective_once,
        }
    } else {
        CastPermissionRule::GraveyardMatching {
            lands_allowed,
            filter: Box::new(obj),
            once_each_turn: effective_once,
        }
    };
    Some((rule, rem_cond))
}

fn parse_exile_permission_tail(rest: &str) -> Option<(bool, bool)> {
    match rest {
        "" => Some((false, false)),
        "without paying their mana costs" | "without paying its mana cost" => Some((true, false)),
        ", and you may spend mana as though it were mana of any color to cast those spells"
        | ", and you may spend mana as though it were mana of any color to cast it"
        | ", and you may spend mana as though it were mana of any color to cast that spell"
        | ", and you may spend mana as though it were mana of any type to cast those spells"
        | ", and you may spend mana as though it were mana of any type to cast it"
        | ", and you may spend mana as though it were mana of any type to cast that spell"
        | ", and mana of any type can be spent to cast those spells"
        | ", and mana of any type can be spent to cast it"
        | ", and mana of any type can be spent to cast that spell"
        | ", and mana of any color can be spent to cast those spells"
        | ", and mana of any color can be spent to cast it"
        | ", and mana of any color can be spent to cast that spell" => Some((false, true)),
        _ => None,
    }
}

fn try_lower_exile_matching_permission_ext<'a>(
    p_low: &str,
    cast_only: bool,
    filter: &ObjectFilterAst,
    once_each_turn: bool,
    condition: Option<&'a ConditionAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(CastPermissionRule, Option<&'a ConditionAst>)> {
    if !matches!(filter.zone, None | Some(ZoneFilter::Exile)) {
        return None;
    }
    let (cond_own_turn, rem_cond) = split_is_your_turn_condition(condition);
    let (pref_own_turn, inner_p) =
        if let Some(rest) = p_low.strip_prefix("during each of your turns, ") {
            (true, rest.trim())
        } else {
            (false, p_low)
        };
    let own_turn_only = cond_own_turn || pref_own_turn;
    let (inner_p, trail_once) = if let Some(head) = inner_p
        .strip_suffix(". you can't cast more than one spell this way each turn")
        .or_else(|| inner_p.strip_suffix(". you may cast only one spell this way each turn"))
    {
        (head.trim(), true)
    } else {
        (inner_p, false)
    };
    let once_each_turn = once_each_turn || trail_once;

    let (linked_by_perm, rest_opt) = if let Some(rest) = inner_p
        .strip_prefix("from among cards exiled with ~")
        .or_else(|| inner_p.strip_prefix("from among the cards exiled with ~"))
    {
        (true, Some(rest.trim()))
    } else if let Some(rest) = inner_p
        .strip_prefix("from among cards you own exiled with ~")
        .or_else(|| inner_p.strip_prefix("from among the cards you own exiled with ~"))
    {
        (true, Some(rest.trim()))
    } else if let Some(rest) = inner_p.strip_prefix("from exile") {
        (false, Some(rest.trim()))
    } else {
        (false, None)
    };

    if let Some(rest) = rest_opt {
        if trail_once && !cast_only {
            return None;
        }
        let (free, any_color) = parse_exile_permission_tail(rest)?;
        let linked_to_source =
            linked_by_perm || (filter.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
        let mut inner = filter.clone();
        inner.zone = None;
        inner.flags = AstFilterFlags(inner.flags.0 & !AstFilterFlags::EXILED_WITH_SELF.0);
        let obj = if inner.any_card && is_unrestricted_filter(&inner) {
            ObjectFilter::any()
        } else {
            lower_object_filter(&inner, vocabulary).ok()?
        };
        let lands_allowed = !cast_only;
        let rule = if lands_allowed || once_each_turn || own_turn_only {
            CastPermissionRule::ExileMatchingExt {
                filter: Box::new(obj),
                linked_to_source,
                free,
                any_color,
                lands_allowed,
                once_each_turn,
                own_turn_only,
            }
        } else {
            CastPermissionRule::ExileMatching {
                filter: Box::new(obj),
                linked_to_source,
                free,
                any_color,
            }
        };
        return Some((rule, rem_cond));
    }

    for (marker, linked_to_source) in [
        (" from among the cards exiled with ~", true),
        (" from among cards exiled with ~", true),
        (" from among the cards you own exiled with ~", true),
        (" from among cards you own exiled with ~", true),
        (" exiled with ~", true),
        (" from exile", false),
    ] {
        if let Some((head, tail)) = inner_p.split_once(marker)
            && let Some((free, any_color)) = parse_exile_permission_tail(tail.trim())
            && let Some((lands_allowed, obj, singular)) =
                parse_play_or_cast_clause(head, vocabulary)
        {
            if trail_once && lands_allowed {
                return None;
            }
            let effective_once = once_each_turn || (pref_own_turn && singular);
            let rule = if lands_allowed || effective_once || own_turn_only {
                CastPermissionRule::ExileMatchingExt {
                    filter: Box::new(obj),
                    linked_to_source,
                    free,
                    any_color,
                    lands_allowed,
                    once_each_turn: effective_once,
                    own_turn_only,
                }
            } else {
                CastPermissionRule::ExileMatching {
                    filter: Box::new(obj),
                    linked_to_source,
                    free,
                    any_color,
                }
            };
            return Some((rule, rem_cond));
        }
    }
    None
}

pub(super) fn static_ast_subject(st: &StaticAbilityAst) -> Option<&SubjectRefAst> {
    match st {
        StaticAbilityAst::ContinuousModifyPT { subject, .. }
        | StaticAbilityAst::DynamicModifyPT { subject, .. }
        | StaticAbilityAst::DynamicBasePT { subject, .. }
        | StaticAbilityAst::GrantKeywords { subject, .. }
        | StaticAbilityAst::GrantKeywordWithCost { subject, .. }
        | StaticAbilityAst::GrantAbilityText { subject, .. }
        | StaticAbilityAst::SetOrAddType { subject, .. }
        | StaticAbilityAst::ContinuousLayerEffect { subject, .. }
        | StaticAbilityAst::PreventCounters { subject, .. }
        | StaticAbilityAst::CrewOrSaddlePowerBonus { subject, .. }
        | StaticAbilityAst::CantBeBlockedExceptByAny { subject, .. }
        | StaticAbilityAst::CombatDamageAssignmentRule { subject, .. }
        | StaticAbilityAst::PreventAllDamageStatic { subject, .. } => Some(subject.as_ref()),
        StaticAbilityAst::RuleOrCombatRestriction { subject, .. } => subject.as_deref(),
        StaticAbilityAst::OncePerTurn(inner) | StaticAbilityAst::OnceDuringYourTurn(inner) => {
            static_ast_subject(inner)
        }
        StaticAbilityAst::Compound(items) => items.iter().rev().find_map(static_ast_subject),
        _ => None,
    }
}

fn is_cross_static_anaphora_label(low: &str) -> bool {
    matches!(
        low,
        "they"
            | "that creature"
            | "those creatures"
            | "that permanent"
            | "those permanents"
            | "that land"
            | "those lands"
            | "that artifact"
            | "those artifacts"
            | "that enchantment"
            | "those enchantments"
            | "that planeswalker"
            | "those planeswalkers"
            | "that vehicle"
            | "those vehicles"
            | "that equipment"
            | "that aura"
            | "that card"
            | "those cards"
    )
}

fn is_compound_static_anaphora_label(low: &str) -> bool {
    matches!(low, "it" | "he" | "she" | "him" | "her") || is_cross_static_anaphora_label(low)
}

fn rule_accepts_plural_permanent_they(rule: &str) -> bool {
    let low = rule
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    matches!(
        low.as_str(),
        "cant_block"
            | "can't block"
            | "cant_attack"
            | "can't attack"
            | "cant_attack_or_block"
            | "can't attack or block"
            | "cant_be_blocked"
            | "can't be blocked"
            | "can't block and can't be blocked"
            | "can't attack or block alone"
            | "cant_attack_alone"
            | "cant_block_alone"
            | "cantattackyou"
            | "cant_attack:you"
            | "cant_attack:you or planeswalkers you control"
            | "cant_attack:you or a planeswalker you control"
            | "can't attack you or planeswalkers you control"
            | "can't attack you or a planeswalker you control"
            | "doesntuntap"
            | "attackseachcombatifable"
            | "attacks each combat if able"
            | "attacksorblockseachcombatifable"
            | "attacks or blocks each combat if able"
            | "blockseachcombatifable"
            | "blocks each combat if able"
            | "must_be_blocked_if_able"
            | "must be blocked if able"
            | "additional_block"
            | "can_block_any_number"
            | "can_attack_as_though_no_defender"
            | "cant_be_sacrificed"
            | "can't be sacrificed"
            | "can't become untapped"
            | "can't have counters put on them"
            | "can't have counters placed on them"
    )
}

pub(super) fn resolve_static_anaphora_subject_across_statics(
    st: &mut StaticAbilityAst,
    prev: &SubjectRefAst,
) {
    match st {
        StaticAbilityAst::OncePerTurn(inner) | StaticAbilityAst::OnceDuringYourTurn(inner) => {
            resolve_static_anaphora_subject_across_statics(inner, prev);
            return;
        }
        StaticAbilityAst::Compound(items) => {
            if let Some(first) = items.first_mut() {
                resolve_static_anaphora_subject_across_statics(first, prev);
            }
            return;
        }
        _ => {}
    }
    if let StaticAbilityAst::RuleOrCombatRestriction {
        subject: Some(subject),
        rule,
        filter: None,
        ..
    } = st
        && matches!(
            subject.as_ref(),
            SubjectRefAst::Anaphora(raw) if raw.trim().eq_ignore_ascii_case("it")
        )
        && !matches!(prev, SubjectRefAst::Player(_))
    {
        let r_low = rule
            .trim()
            .trim_end_matches('.')
            .trim()
            .to_ascii_lowercase();
        let lock_body = r_low
            .strip_prefix("abilitytimingorlock:")
            .unwrap_or(&r_low)
            .trim();
        if matches!(
            lock_body,
            "its activated abilities can't be activated"
                | "their activated abilities can't be activated"
                | "its activated abilities can't be activated unless they're mana abilities"
                | "their activated abilities can't be activated unless they're mana abilities"
                | "cant_activate_nonmana_abilities"
        ) {
            *subject.as_mut() = prev.clone();
            return;
        }
    }
    let allows_plural_they = !matches!(prev, SubjectRefAst::Player(_))
        && match st {
            StaticAbilityAst::RuleOrCombatRestriction { rule, .. } => {
                rule_accepts_plural_permanent_they(rule)
            }
            StaticAbilityAst::PreventCounters { .. } => false,
            _ => true,
        };
    let slot: Option<&mut SubjectRefAst> = match st {
        StaticAbilityAst::ContinuousModifyPT { subject, .. }
        | StaticAbilityAst::DynamicModifyPT { subject, .. }
        | StaticAbilityAst::DynamicBasePT { subject, .. }
        | StaticAbilityAst::GrantKeywords { subject, .. }
        | StaticAbilityAst::GrantKeywordWithCost { subject, .. }
        | StaticAbilityAst::GrantAbilityText { subject, .. }
        | StaticAbilityAst::SetOrAddType { subject, .. }
        | StaticAbilityAst::ContinuousLayerEffect { subject, .. }
        | StaticAbilityAst::PreventCounters { subject, .. }
        | StaticAbilityAst::CrewOrSaddlePowerBonus { subject, .. }
        | StaticAbilityAst::CantBeBlockedExceptByAny { subject, .. }
        | StaticAbilityAst::CombatDamageAssignmentRule { subject, .. }
        | StaticAbilityAst::PreventAllDamageStatic { subject, .. } => Some(subject.as_mut()),
        StaticAbilityAst::RuleOrCombatRestriction {
            subject: Some(subject),
            ..
        } => Some(subject.as_mut()),
        _ => None,
    };
    if let Some(s) = slot {
        if let SubjectRefAst::Anaphora(raw) = s {
            let low = raw.trim().to_ascii_lowercase();
            if is_cross_static_anaphora_label(&low) {
                *s = prev.clone();
            }
        } else if allows_plural_they && matches!(s, SubjectRefAst::Player(PlayerRefAst::ThatPlayer))
        {
            *s = prev.clone();
        }
    }
}

fn resolve_static_anaphora_subject(st: &mut StaticAbilityAst, prev: &SubjectRefAst) {
    match st {
        StaticAbilityAst::OncePerTurn(inner) | StaticAbilityAst::OnceDuringYourTurn(inner) => {
            resolve_static_anaphora_subject(inner, prev);
            return;
        }
        StaticAbilityAst::Compound(items) => {
            for item in items {
                resolve_static_anaphora_subject(item, prev);
            }
            return;
        }
        _ => {}
    }
    let allows_plural_they = !matches!(prev, SubjectRefAst::Player(_))
        && match st {
            StaticAbilityAst::RuleOrCombatRestriction { rule, .. } => {
                rule_accepts_plural_permanent_they(rule)
            }
            StaticAbilityAst::PreventCounters { .. } => false,
            _ => true,
        };
    let slot: Option<&mut SubjectRefAst> = match st {
        StaticAbilityAst::ContinuousModifyPT { subject, .. }
        | StaticAbilityAst::DynamicModifyPT { subject, .. }
        | StaticAbilityAst::DynamicBasePT { subject, .. }
        | StaticAbilityAst::GrantKeywords { subject, .. }
        | StaticAbilityAst::GrantKeywordWithCost { subject, .. }
        | StaticAbilityAst::GrantAbilityText { subject, .. }
        | StaticAbilityAst::SetOrAddType { subject, .. }
        | StaticAbilityAst::ContinuousLayerEffect { subject, .. }
        | StaticAbilityAst::PreventCounters { subject, .. }
        | StaticAbilityAst::CrewOrSaddlePowerBonus { subject, .. }
        | StaticAbilityAst::CantBeBlockedExceptByAny { subject, .. }
        | StaticAbilityAst::CombatDamageAssignmentRule { subject, .. }
        | StaticAbilityAst::PreventAllDamageStatic { subject, .. } => Some(subject.as_mut()),
        StaticAbilityAst::RuleOrCombatRestriction {
            subject: Some(subject),
            ..
        } => Some(subject.as_mut()),
        _ => None,
    };
    if let Some(s) = slot {
        if let SubjectRefAst::Anaphora(raw) = s {
            let low = raw.trim().to_ascii_lowercase();
            if is_compound_static_anaphora_label(&low) {
                *s = prev.clone();
            }
        } else if allows_plural_they && matches!(s, SubjectRefAst::Player(PlayerRefAst::ThatPlayer))
        {
            *s = prev.clone();
        }
    }
}

fn static_ast_condition(st: &StaticAbilityAst) -> Option<&ConditionAst> {
    match st {
        StaticAbilityAst::ContinuousModifyPT { condition, .. }
        | StaticAbilityAst::DynamicModifyPT { condition, .. }
        | StaticAbilityAst::DynamicBasePT { condition, .. }
        | StaticAbilityAst::GrantKeywords { condition, .. }
        | StaticAbilityAst::GrantKeywordWithCost { condition, .. }
        | StaticAbilityAst::GrantAbilityText { condition, .. }
        | StaticAbilityAst::SetOrAddType { condition, .. }
        | StaticAbilityAst::ContinuousLayerEffect { condition, .. }
        | StaticAbilityAst::CostModifier { condition, .. }
        | StaticAbilityAst::CostModifierWithFloor { condition, .. }
        | StaticAbilityAst::PlayOrCastPermission { condition, .. }
        | StaticAbilityAst::RuleOrCombatRestriction { condition, .. }
        | StaticAbilityAst::SkipStep { condition, .. }
        | StaticAbilityAst::ModifyMaximumHandSize { condition, .. }
        | StaticAbilityAst::CantBeBlockedExceptByAny { condition, .. }
        | StaticAbilityAst::DoubleAllDamage { condition, .. }
        | StaticAbilityAst::PreventFractionDamage { condition, .. }
        | StaticAbilityAst::PreventThresholdDamage { condition, .. }
        | StaticAbilityAst::PreventAndReplaceDamage { condition, .. }
        | StaticAbilityAst::ActivationLimitOverride { condition, .. }
        | StaticAbilityAst::CombatDamageAssignmentRule { condition, .. }
        | StaticAbilityAst::LookOrRevealRule { condition, .. } => condition.as_deref(),
        StaticAbilityAst::OncePerTurn(inner) | StaticAbilityAst::OnceDuringYourTurn(inner) => {
            static_ast_condition(inner)
        }
        StaticAbilityAst::Compound(items) => {
            let mut found: Option<&ConditionAst> = None;
            for item in items {
                let c = static_ast_condition(item)?;
                if let Some(prev) = found {
                    if prev != c {
                        return None;
                    }
                } else {
                    found = Some(c);
                }
            }
            found
        }
        _ => None,
    }
}

fn attach_ast_condition(st: &mut StaticAbilityAst, cond: ConditionAst) {
    match st {
        StaticAbilityAst::OncePerTurn(inner) | StaticAbilityAst::OnceDuringYourTurn(inner) => {
            attach_ast_condition(inner, cond);
            return;
        }
        StaticAbilityAst::Compound(items) => {
            for item in items {
                attach_ast_condition(item, cond.clone());
            }
            return;
        }
        _ => {}
    }
    let slot = match st {
        StaticAbilityAst::ContinuousModifyPT { condition, .. }
        | StaticAbilityAst::DynamicModifyPT { condition, .. }
        | StaticAbilityAst::DynamicBasePT { condition, .. }
        | StaticAbilityAst::GrantKeywords { condition, .. }
        | StaticAbilityAst::GrantKeywordWithCost { condition, .. }
        | StaticAbilityAst::GrantAbilityText { condition, .. }
        | StaticAbilityAst::SetOrAddType { condition, .. }
        | StaticAbilityAst::ContinuousLayerEffect { condition, .. }
        | StaticAbilityAst::CostModifier { condition, .. }
        | StaticAbilityAst::CostModifierWithFloor { condition, .. }
        | StaticAbilityAst::PlayOrCastPermission { condition, .. }
        | StaticAbilityAst::RuleOrCombatRestriction { condition, .. }
        | StaticAbilityAst::SkipStep { condition, .. }
        | StaticAbilityAst::ModifyMaximumHandSize { condition, .. }
        | StaticAbilityAst::CantBeBlockedExceptByAny { condition, .. }
        | StaticAbilityAst::DoubleAllDamage { condition, .. }
        | StaticAbilityAst::PreventFractionDamage { condition, .. }
        | StaticAbilityAst::PreventThresholdDamage { condition, .. }
        | StaticAbilityAst::PreventAndReplaceDamage { condition, .. }
        | StaticAbilityAst::ActivationLimitOverride { condition, .. }
        | StaticAbilityAst::CombatDamageAssignmentRule { condition, .. }
        | StaticAbilityAst::LookOrRevealRule { condition, .. } => condition,
        _ => return,
    };
    *slot = Some(match slot.take() {
        Some(prev) => Box::new(ConditionAst::And(prev, Box::new(cond))),
        None => Box::new(cond),
    });
}

fn lower_paired_static(
    paired: &StaticAbilityAst,
    outer_selectors: &[PermanentSelector],
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    match paired {
        StaticAbilityAst::RuleOrCombatRestriction {
            rule,
            filter,
            condition,
            ..
        } => {
            if filter.is_none() && condition.is_none() && rule == "LosesAllAbilities" {
                for sel in outer_selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel.clone(),
                            parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::remove(
                                AbilityRemoval::All,
                            ))]),
                        },
                    )?;
                }
                return Ok(());
            }
            if filter.is_none()
                && condition.is_none()
                && matches!(
                    rule.trim().to_ascii_lowercase().as_str(),
                    "is goaded" | "is also goaded" | "permanents_are_goaded"
                )
                && outer_selectors.len() == 1
                && outer_selectors[0]
                    == (PermanentSelector {
                        attached_to_source: true,
                        ..PermanentSelector::default()
                    })
                && is_permanent_attachment_program(program)
            {
                push_attack_requirement(program, AttackRequirement::EnchantedCreatureIsGoaded);
                return Ok(());
            }
            if filter.is_none()
                && condition.is_none()
                && outer_selectors.iter().all(|s| s.source_condition.is_none())
                && (rule.eq_ignore_ascii_case("can't become untapped")
                    || rule.eq_ignore_ascii_case("can't be sacrificed")
                    || rule.eq_ignore_ascii_case("cant_be_sacrificed"))
            {
                let is_untap_lock = rule.eq_ignore_ascii_case("can't become untapped");
                for sel in outer_selectors {
                    let restriction = if is_untap_lock {
                        StaticRestriction::CannotBeUntapped(Box::new(sel.clone()))
                    } else {
                        StaticRestriction::CannotBeSacrificed(Box::new(sel.clone()))
                    };
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: restriction,
                        },
                    )?;
                }
                return Ok(());
            }
            if filter.is_none()
                && condition.is_none()
                && let Some(lost_raw) = rule.strip_prefix("Loses:")
            {
                let kws: Vec<String> = lost_raw
                    .replace(" and ", ",")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let suppressed = lower_keyword_list_with_vocab(&kws, vocabulary)?;
                let change = AbilityChange {
                    suppressed_keywords: suppressed,
                    ..AbilityChange::default()
                };
                for sel in outer_selectors {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel.clone(),
                            parts: StaticParts(vec![StaticPart::Abilities(change.clone())]),
                        },
                    )?;
                }
                return Ok(());
            }
            if filter.is_none()
                && (rule.eq_ignore_ascii_case("can't block and can't be blocked")
                    || rule.eq_ignore_ascii_case("can't attack or block alone"))
            {
                let pair = if rule.eq_ignore_ascii_case("can't block and can't be blocked") {
                    [
                        BlockingRestriction::CannotBlock,
                        BlockingRestriction::CannotBeBlocked,
                    ]
                } else {
                    [
                        BlockingRestriction::CannotAttackAlone,
                        BlockingRestriction::CannotBlockAlone,
                    ]
                };
                for sel in outer_selectors {
                    let mut sel = sel.clone();
                    if let Some(cond) = condition {
                        let extra_sc = lower_static_source_condition(cond, vocabulary)
                            .ok_or("oracle:unsupported-static-cond")?;
                        sel.source_condition = Some(match sel.source_condition.take() {
                            Some(prev) => {
                                spellwake_engine::characteristics::StaticSourceCondition::And(
                                    Box::new(prev),
                                    Box::new(extra_sc),
                                )
                            }
                            None => extra_sc,
                        });
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel,
                            parts: StaticParts(vec![
                                StaticPart::BlockingRestriction(pair[0].clone()),
                                StaticPart::BlockingRestriction(pair[1].clone()),
                            ]),
                        },
                    )?;
                }
                return Ok(());
            }
            if let Some(obj) =
                parse_cant_block_or_be_blocked_by_filter(rule, filter.as_deref(), vocabulary)
            {
                for sel in outer_selectors {
                    let mut sel = sel.clone();
                    if let Some(cond) = condition {
                        let extra_sc = lower_static_source_condition(cond, vocabulary)
                            .ok_or("oracle:unsupported-static-cond")?;
                        sel.source_condition = Some(match sel.source_condition.take() {
                            Some(prev) => {
                                spellwake_engine::characteristics::StaticSourceCondition::And(
                                    Box::new(prev),
                                    Box::new(extra_sc),
                                )
                            }
                            None => extra_sc,
                        });
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel,
                            parts: StaticParts(vec![
                                StaticPart::BlockingRestriction(
                                    BlockingRestriction::CannotBlockMatching(obj),
                                ),
                                StaticPart::BlockingRestriction(
                                    BlockingRestriction::CannotBeBlockedByMatching(obj),
                                ),
                            ]),
                        },
                    )?;
                }
                return Ok(());
            }
            if filter.is_none() && is_menace_blocking_rule(rule) {
                let menace_change = AbilityChange {
                    granted_keywords: BTreeSet::from([KeywordAbility::Menace]),
                    ..AbilityChange::default()
                };
                for sel in outer_selectors {
                    let mut sel = sel.clone();
                    if let Some(cond) = condition {
                        let extra_sc = lower_static_source_condition(cond, vocabulary)
                            .ok_or("oracle:unsupported-static-cond")?;
                        sel.source_condition = Some(match sel.source_condition.take() {
                            Some(prev) => {
                                spellwake_engine::characteristics::StaticSourceCondition::And(
                                    Box::new(prev),
                                    Box::new(extra_sc),
                                )
                            }
                            None => extra_sc,
                        });
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel,
                            parts: StaticParts(vec![StaticPart::Abilities(menace_change.clone())]),
                        },
                    )?;
                }
                return Ok(());
            }
            if filter.is_none()
                && let Some(extra_blocks) = parse_block_additional_count(rule)
                && extra_blocks >= 2
            {
                let parts_vec =
                    vec![
                        StaticPart::BlockingRestriction(BlockingRestriction::AdditionalBlock);
                        usize::from(extra_blocks)
                    ];
                for sel in outer_selectors {
                    let mut sel = sel.clone();
                    if let Some(cond) = condition {
                        let extra_sc = lower_static_source_condition(cond, vocabulary)
                            .ok_or("oracle:unsupported-static-cond")?;
                        sel.source_condition = Some(match sel.source_condition.take() {
                            Some(prev) => {
                                spellwake_engine::characteristics::StaticSourceCondition::And(
                                    Box::new(prev),
                                    Box::new(extra_sc),
                                )
                            }
                            None => extra_sc,
                        });
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel,
                            parts: StaticParts(parts_vec.clone()),
                        },
                    )?;
                }
                return Ok(());
            }
            if filter.is_none()
                && let Some(lock) = parse_static_ability_lock(rule, condition.is_some())
            {
                if lock.requires_source_controller
                    && !outer_selectors
                        .iter()
                        .all(|sel| selector_guaranteed_source_controller(sel, program))
                {
                    return Err("oracle:unsupported-paired-static");
                }
                let extra_sc = match condition {
                    Some(cond) => Some(
                        lower_static_source_condition(cond, vocabulary)
                            .ok_or("oracle:unsupported-static-cond")?,
                    ),
                    None => None,
                };
                let mut parts = Vec::new();
                if lock.cannot_attack_or_block {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::CannotAttackOrBlock,
                    ));
                }
                if lock.cannot_crew_vehicles {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::CannotCrewVehicles,
                    ));
                }
                if lock.suppressed_activations.is_some() || lock.doesnt_untap {
                    parts.push(StaticPart::Abilities(AbilityChange {
                        suppressed_activations: lock.suppressed_activations,
                        doesnt_untap: lock.doesnt_untap,
                        ..AbilityChange::default()
                    }));
                }
                for sel in outer_selectors {
                    let mut sel = sel.clone();
                    if let Some(sc) = &extra_sc {
                        sel.source_condition = Some(match sel.source_condition.take() {
                            Some(prev) => {
                                spellwake_engine::characteristics::StaticSourceCondition::And(
                                    Box::new(prev),
                                    Box::new(sc.clone()),
                                )
                            }
                            None => sc.clone(),
                        });
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel,
                            parts: StaticParts(parts.clone()),
                        },
                    )?;
                }
                return Ok(());
            }
            if filter.is_none()
                && (rule == "AttacksEachCombatIfAble"
                    || rule.eq_ignore_ascii_case("attacks each combat if able")
                    || rule.eq_ignore_ascii_case("attacks each turn if able"))
            {
                for sel in outer_selectors {
                    let mut sel = sel.clone();
                    if let Some(cond) = condition {
                        let extra_sc = lower_static_source_condition(cond, vocabulary)
                            .ok_or("oracle:unsupported-static-cond")?;
                        sel.source_condition = Some(match sel.source_condition.take() {
                            Some(prev) => {
                                spellwake_engine::characteristics::StaticSourceCondition::And(
                                    Box::new(prev),
                                    Box::new(extra_sc),
                                )
                            }
                            None => extra_sc,
                        });
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel,
                            parts: StaticParts(vec![StaticPart::BlockingRestriction(
                                BlockingRestriction::MustAttackEachCombatIfAble,
                            )]),
                        },
                    )?;
                }
                return Ok(());
            }
            if let Some(br) = lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary) {
                for sel in outer_selectors {
                    let mut sel = sel.clone();
                    if let Some(cond) = condition {
                        let extra_sc = lower_static_source_condition(cond, vocabulary)
                            .ok_or("oracle:unsupported-static-cond")?;
                        sel.source_condition = Some(match sel.source_condition.take() {
                            Some(prev) => {
                                spellwake_engine::characteristics::StaticSourceCondition::And(
                                    Box::new(prev),
                                    Box::new(extra_sc),
                                )
                            }
                            None => extra_sc,
                        });
                    }
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel,
                            parts: StaticParts(vec![StaticPart::BlockingRestriction(br.clone())]),
                        },
                    )?;
                }
                return Ok(());
            }
            Err("oracle:unsupported-paired-static")
        }
        StaticAbilityAst::SetOrAddType {
            type_or_subtype,
            additive,
            condition,
            ..
        } => {
            let entry_tap = program.permanent().and_then(|p| p.entry_tap);
            let type_parts = parse_static_type_and_color_parts_with_entry(
                type_or_subtype,
                *additive,
                entry_tap,
                vocabulary,
            )?;
            for sel in outer_selectors {
                let mut sel = sel.clone();
                if let Some(cond) = condition {
                    let extra_sc = lower_static_source_condition(cond, vocabulary)
                        .ok_or("oracle:unsupported-static-cond")?;
                    sel.source_condition = Some(match sel.source_condition.take() {
                        Some(prev) => {
                            spellwake_engine::characteristics::StaticSourceCondition::And(
                                Box::new(prev),
                                Box::new(extra_sc),
                            )
                        }
                        None => extra_sc,
                    });
                }
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: sel,
                        parts: StaticParts(type_parts.clone()),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::GrantAbilityText {
            ability_text,
            compiled,
            condition,
            ..
        } => {
            let owned_face;
            let face: &FaceAst = if let Some(compiled_face) = compiled.as_deref() {
                compiled_face
            } else {
                let cleaned = ability_text
                    .trim()
                    .trim_matches('"')
                    .trim()
                    .trim_end_matches(',')
                    .trim();
                if cleaned.is_empty() {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let synthetic = compile_card(
                    &CardInput {
                        oracle_id: "",
                        name: "~",
                        layout: "normal",
                        faces: vec![spellwake_oracle_compiler::FaceInput {
                            name: "~",
                            type_line: if looks_like_loyalty_ability(cleaned) {
                                "Planeswalker"
                            } else {
                                "Creature"
                            },
                            oracle_text: cleaned,
                        }],
                    },
                    &compiler_vocab,
                );
                if !synthetic.strictly_compiled || synthetic.faces.len() != 1 {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                owned_face = synthetic
                    .faces
                    .into_iter()
                    .next()
                    .ok_or("oracle:unsupported-grant-ability-text")?;
                &owned_face
            };
            let mut selectors = outer_selectors.to_vec();
            if let Some(cond) = condition.as_deref() {
                for sel in &mut selectors {
                    apply_static_selector_condition(sel, cond, vocabulary)?;
                }
            }
            emit_granted_ability_face_for_selectors(face, &selectors, program, vocabulary, ids)
        }
        StaticAbilityAst::GrantKeywords {
            keywords,
            condition: None,
            paired_rule,
            ..
        } => {
            let (mut parts_vec, rem_kws) =
                partition_static_keyword_grants(keywords, vocabulary, ids);
            if !rem_kws.is_empty() {
                let ab_change = lower_ability_change_with_ids(&rem_kws, vocabulary, ids)?;
                parts_vec.push(StaticPart::Abilities(ab_change));
            }
            if parts_vec.is_empty() {
                return Err("oracle:empty-grant-keywords");
            }
            for sel in outer_selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: sel.clone(),
                        parts: StaticParts(parts_vec.clone()),
                    },
                )?;
            }
            if let Some(inner) = paired_rule {
                lower_paired_static(inner, outer_selectors, program, vocabulary, ids)?;
            }
            Ok(())
        }
        StaticAbilityAst::FollowUp(ins) => {
            lower_followup_instructions(ins, program, vocabulary, ids)
                .map_err(|_| "oracle:unsupported-paired-static")
        }
        StaticAbilityAst::CantBeBlockedExceptByAny {
            allowed_filters,
            condition,
            ..
        } => {
            let br = lower_cant_be_blocked_except_by_any(allowed_filters, vocabulary)
                .ok_or("oracle:unsupported-paired-static")?;
            let extra_sc = match condition {
                Some(cond) => Some(
                    lower_static_source_condition(cond, vocabulary)
                        .ok_or("oracle:unsupported-static-cond")?,
                ),
                None => None,
            };
            for sel in outer_selectors {
                let mut sel = sel.clone();
                if let Some(sc) = &extra_sc {
                    sel.source_condition = Some(match sel.source_condition.take() {
                        Some(prev) => {
                            spellwake_engine::characteristics::StaticSourceCondition::And(
                                Box::new(prev),
                                Box::new(sc.clone()),
                            )
                        }
                        None => sc.clone(),
                    });
                }
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: sel,
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(br.clone())]),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::ContinuousLayerEffect {
            set_types,
            additive_types,
            base_pt,
            set_colors,
            grant_keywords,
            lose_keywords,
            lose_all_abilities,
            condition,
            ..
        } => {
            let entry_tap = program.permanent().and_then(|p| p.entry_tap);
            let mut parts = Vec::new();
            let mut has_color_part = false;
            let mut effective_grant_keywords: Vec<String> = grant_keywords.clone();
            if let Some(desc) = set_types {
                let mut desc_type_slice: &str = desc.as_str();
                if effective_grant_keywords.is_empty()
                    && let Some((lhs_ty, rhs_kw)) = desc_type_slice
                        .split_once(" and has ")
                        .or_else(|| desc_type_slice.split_once(" and have "))
                {
                    let compiler_vocab = cached_compiler_vocab(vocabulary);
                    if let Some(parsed_kws) = spellwake_oracle_compiler::filter::parse_keyword_list(
                        rhs_kw.trim(),
                        &compiler_vocab,
                    ) && !parsed_kws.is_empty()
                    {
                        desc_type_slice = lhs_ty.trim();
                        effective_grant_keywords = parsed_kws;
                    }
                }
                let type_parts = parse_static_type_and_color_parts_with_entry(
                    desc_type_slice,
                    *additive_types,
                    entry_tap,
                    vocabulary,
                )?;
                if type_parts
                    .iter()
                    .any(|part| matches!(part, StaticPart::Color(_)))
                {
                    has_color_part = true;
                }
                parts.extend(type_parts);
            }
            if let Some(color_str) = set_colors {
                if has_color_part {
                    return Err("oracle:unsupported-layer-color");
                }
                let color_clean = color_str.trim().trim_end_matches('.').trim();
                let colors = if color_clean.eq_ignore_ascii_case("all colors") {
                    BTreeSet::from([
                        Color::White,
                        Color::Blue,
                        Color::Black,
                        Color::Red,
                        Color::Green,
                    ])
                } else {
                    let mut parsed_colors = BTreeSet::new();
                    let mut saw_colorless = false;
                    for raw_tok in color_clean
                        .replace(" and ", " ")
                        .replace(',', " ")
                        .split_whitespace()
                    {
                        match raw_tok.to_ascii_lowercase().as_str() {
                            "white" => {
                                parsed_colors.insert(Color::White);
                            }
                            "blue" => {
                                parsed_colors.insert(Color::Blue);
                            }
                            "black" => {
                                parsed_colors.insert(Color::Black);
                            }
                            "red" => {
                                parsed_colors.insert(Color::Red);
                            }
                            "green" => {
                                parsed_colors.insert(Color::Green);
                            }
                            "colorless" => {
                                saw_colorless = true;
                            }
                            "a" | "an" => {}
                            _ => return Err("oracle:unsupported-layer-color"),
                        }
                    }
                    if parsed_colors.is_empty() && !saw_colorless {
                        return Err("oracle:unsupported-layer-color");
                    }
                    parsed_colors
                };
                parts.push(StaticPart::Color(ColorOperation::Set(
                    StaticColorOperand::Literal(colors),
                )));
            }
            if *lose_all_abilities
                || !effective_grant_keywords.is_empty()
                || !lose_keywords.is_empty()
            {
                let (special_parts, rem_kws) =
                    partition_static_keyword_grants(&effective_grant_keywords, vocabulary, ids);
                parts.extend(special_parts);
                if *lose_all_abilities || !rem_kws.is_empty() || !lose_keywords.is_empty() {
                    let mut change = if !rem_kws.is_empty() {
                        lower_ability_change_with_ids(&rem_kws, vocabulary, ids)?
                    } else {
                        AbilityChange::default()
                    };
                    if *lose_all_abilities {
                        change.removal = Some(AbilityRemoval::All);
                    }
                    if !lose_keywords.is_empty() {
                        change
                            .suppressed_keywords
                            .extend(lower_keyword_list_with_vocab(lose_keywords, vocabulary)?);
                    }
                    parts.push(StaticPart::Abilities(change));
                }
            }
            if let Some((p_raw, t_raw)) = base_pt {
                parts.push(lower_static_base_pt_part(
                    p_raw,
                    t_raw,
                    vocabulary,
                    "oracle:unsupported-layer-pt",
                )?);
            }
            if parts.is_empty() {
                return Err("oracle:empty-layer-effect");
            }
            let extra_sc = match condition {
                Some(cond) => Some(
                    lower_static_source_condition_ext(cond, vocabulary)
                        .ok_or("oracle:unsupported-static-cond")?,
                ),
                None => None,
            };
            for sel in outer_selectors {
                let mut sel = sel.clone();
                if let Some(sc) = &extra_sc {
                    sel.source_condition = Some(match sel.source_condition.take() {
                        Some(prev) => {
                            spellwake_engine::characteristics::StaticSourceCondition::And(
                                Box::new(prev),
                                Box::new(sc.clone()),
                            )
                        }
                        None => sc.clone(),
                    });
                }
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: sel,
                        parts: StaticParts(parts.clone()),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::Compound(items) => {
            for item in items {
                lower_paired_static(item, outer_selectors, program, vocabulary, ids)?;
            }
            Ok(())
        }
        _ => Err("oracle:unsupported-paired-static"),
    }
}

fn lower_damage_source_filter(
    subj: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(Option<ObjectFilter>, PlayerRelation), &'static str> {
    match subj {
        SubjectRefAst::AnyTarget => Ok((None, PlayerRelation::AnyParticipant)),
        SubjectRefAst::SelfObject => Ok((
            Some(ObjectFilter::self_object()),
            PlayerRelation::AnyParticipant,
        )),
        SubjectRefAst::AttachedHost(_) => Ok((
            Some(ObjectFilter::Clause(ObjectClause {
                source_relation: SourceRelation::AttachedToSelf,
                ..ObjectClause::any()
            })),
            PlayerRelation::AnyParticipant,
        )),
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
            if is_unrestricted_filter(f) {
                return Ok((None, PlayerRelation::AnyParticipant));
            }
            if filter_only_has(
                f,
                FilterAllow {
                    controller: true,
                    ..FilterAllow::NONE
                },
            ) && f.any_types.is_empty()
            {
                let rel = match f.controller {
                    Some(ControllerRelation::You) => PlayerRelation::Controller,
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        PlayerRelation::Opponent
                    }
                    None => PlayerRelation::AnyParticipant,
                    _ => return Err("oracle:unsupported-prevent-static"),
                };
                return Ok((None, rel));
            }
            let obj = lower_object_filter(f, vocabulary)
                .map_err(|_| "oracle:unsupported-prevent-static")?;
            Ok((Some(obj), PlayerRelation::AnyParticipant))
        }
        _ => Err("oracle:unsupported-prevent-static"),
    }
}

fn lower_damage_recipients(
    subj: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Vec<StaticDamageRecipient>, &'static str> {
    match subj {
        SubjectRefAst::AnyTarget => Ok(vec![StaticDamageRecipient::Any]),
        SubjectRefAst::SelfObject => Ok(vec![StaticDamageRecipient::SelfObject]),
        SubjectRefAst::AttachedHost(_) => Ok(vec![StaticDamageRecipient::AttachedHost]),
        SubjectRefAst::Player(PlayerRefAst::You) => Ok(vec![StaticDamageRecipient::Controller]),
        SubjectRefAst::Player(PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer) => {
            Ok(vec![StaticDamageRecipient::Players])
        }
        SubjectRefAst::Player(PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent) => {
            Ok(vec![StaticDamageRecipient::Opponents])
        }
        SubjectRefAst::Compound(items) if !items.is_empty() => {
            let mut out = Vec::new();
            for item in items {
                out.extend(lower_damage_recipients(item, vocabulary)?);
            }
            Ok(out)
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
            if f.any_types == AstCardTypeMask::CREATURE
                && filter_only_has(
                    f,
                    FilterAllow {
                        types: true,
                        controller: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                let rec = match f.controller {
                    None => StaticDamageRecipient::Creatures,
                    Some(ControllerRelation::You) => StaticDamageRecipient::ControlledCreatures,
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        StaticDamageRecipient::OpponentCreatures
                    }
                    _ => {
                        let obj = lower_object_filter(f, vocabulary)
                            .map_err(|_| "oracle:unsupported-prevent-static")?;
                        StaticDamageRecipient::MatchingPermanent(obj)
                    }
                };
                return Ok(vec![rec]);
            }
            let obj = lower_object_filter(f, vocabulary)
                .map_err(|_| "oracle:unsupported-prevent-static")?;
            Ok(vec![StaticDamageRecipient::MatchingPermanent(obj)])
        }
        _ => Err("oracle:unsupported-prevent-static"),
    }
}

fn looks_like_loyalty_ability(text: &str) -> bool {
    let trimmed = text.trim();
    let Some((prefix, _)) = trimmed.split_once(':') else {
        return false;
    };
    let core = prefix
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim()
        .replace('\u{2212}', "-");
    if core.is_empty() || core.len() > 6 {
        return false;
    }
    if core == "0" || core.eq_ignore_ascii_case("-x") {
        return true;
    }
    let Some(digits) = core.strip_prefix('+').or_else(|| core.strip_prefix('-')) else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn lower_single_granted_ability(
    ability: &AbilityAst,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<StaticPart, &'static str> {
    match ability {
        AbilityAst::Activated {
            ability_word: _,
            zone,
            cost,
            body,
        } if zone
            .as_deref()
            .is_none_or(|z| z.eq_ignore_ascii_case("battlefield")) =>
        {
            if let Some(mana_out) = try_lower_mana_ability(cost, body, vocabulary) {
                match mana_out {
                    ManaOutput::ChooseColor { amount, colors }
                        if amount == 1.into() && colors == ManaOutput::all_colors() =>
                    {
                        Ok(StaticPart::GrantAnyColorMana)
                    }
                    ManaOutput::Fixed(bundle)
                        if bundle.len() == 1 && bundle.values().next() == Some(&1.into()) =>
                    {
                        let mt = *bundle.keys().next().ok_or("oracle:unreachable")?;
                        Ok(StaticPart::GrantFixedColorMana(mt))
                    }
                    other => Ok(StaticPart::GrantMana(Box::new(other))),
                }
            } else {
                // StaticPart::GrantActivated* wraps ActivatedAbility, which does not carry
                // hybrid_symbols or extra_costs; fail closed if either is non-empty.
                let (mana_cost, source_cost, hybrids, extra_costs) =
                    lower_activation_cost_full(cost, None, vocabulary)?;
                if !hybrids.is_empty() || !extra_costs.is_empty() {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                let (mut exec_body, restriction) = lower_activated_body(body, vocabulary, ids)?;
                let anaphora_ref = match source_cost {
                    SourceCost::SacrificeSelected {
                        filter:
                            SelectedSacrificeFilter::Creature
                            | SelectedSacrificeFilter::OtherCreature
                            | SelectedSacrificeFilter::Subtype(_),
                        count: 1,
                        from_graveyard: false,
                        ..
                    }
                    | SourceCost::PayLife {
                        sacrifice:
                            Some(
                                SelectedSacrificeFilter::Creature
                                | SelectedSacrificeFilter::OtherCreature,
                            ),
                        ..
                    } => ValueObjectRef::SacrificedCreature,
                    SourceCost::SacrificeSelected {
                        filter: SelectedSacrificeFilter::Matching(filter),
                        count: 1,
                        from_graveyard: false,
                        ..
                    } if filter.guarantees_creature() => ValueObjectRef::SacrificedCreature,
                    _ => ValueObjectRef::AbilitySource,
                };
                rewrite_body_anaphora_stats(&mut exec_body, anaphora_ref);
                let ability = Box::new(ActivatedAbility {
                    id: ActivatedAbilityId(0),
                    mana_cost,
                    source_cost,
                    body: exec_body,
                });
                if let Some(restriction) = restriction {
                    Ok(StaticPart::GrantActivatedRestricted {
                        ability,
                        restriction: Box::new(restriction),
                    })
                } else {
                    Ok(StaticPart::GrantActivated(ability))
                }
            }
        }
        AbilityAst::Triggered {
            ability_word: _,
            trigger,
            body,
        } => {
            let (mut spec, mut grouping, guard) = lower_trigger_spec(trigger, vocabulary)?;
            let self_pronoun = trigger_subject_is_self(trigger);
            let stripped_body =
                strip_triggered_restrictions(body, &mut spec, &mut grouping, vocabulary);
            let mut exec_body =
                lower_trigger_effect_body(&stripped_body, vocabulary, ids, &spec, self_pronoun)?;
            if let Some(cond) = guard {
                prepend_instruction_at(
                    &mut exec_body.instructions,
                    0,
                    Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(cond),
                    )),
                );
            }
            apply_trigger_body_rewrites(&mut exec_body, &spec, grouping, self_pronoun);
            Ok(StaticPart::GrantTrigger(Box::new(TriggeredAbility {
                id: ids.triggered(),
                condition: spec,
                grouping,
                body: exec_body,
            })))
        }
        AbilityAst::Loyalty { delta, body } => {
            let trimmed = delta.trim().replace('\u{2212}', "-");
            let (loyalty, variable_loyalty): (Option<i32>, bool) =
                if trimmed.eq_ignore_ascii_case("-x") {
                    (Some(0), true)
                } else {
                    let n: i32 = trimmed
                        .strip_prefix('+')
                        .unwrap_or(&trimmed)
                        .parse()
                        .map_err(|_| "oracle:bad-loyalty-delta")?;
                    (Some(n), false)
                };
            let (exec_body, loyalty_res) = lower_activated_body(body, vocabulary, ids)?;
            if loyalty_res.is_some() {
                return Err("oracle:unsupported-grant-ability-text");
            }
            Ok(StaticPart::GrantLoyalty {
                ability: Box::new(ActivatedAbility {
                    id: ActivatedAbilityId(0),
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::None,
                    body: exec_body,
                }),
                loyalty,
                variable_loyalty,
            })
        }
        _ => Err("oracle:unsupported-grant-ability-text"),
    }
}

fn permanent_selector_to_object_filter(selector: &PermanentSelector) -> Option<ObjectFilter> {
    use spellwake_engine::characteristics::{
        CombatSelector, NameFilter, OwnerSelector, SelectorSubtype,
    };
    use spellwake_engine::program::{ExactNameKey, SingleSubtype, SubtypeList};

    if selector.sole_controlled_creature
        || selector.while_controlling_own_commander
        || selector.source_has_imprinted_creature
        || selector.controlled_creatures_and_typed_tokens.is_some()
        || selector.controller_type_threshold.is_some()
        || selector.source_condition.is_some()
        || selector.extra_required_keyword.is_some()
        || selector.requires_toxic
        || !selector.forbidden_keywords.is_empty()
        || selector.equipped_by_at_least > 1
        || selector.devotion_below.is_some()
        || selector.station_at.is_some()
        || selector.minimum_plus_one_counters > 1
        || selector.attached_to_source_permanent
        || selector.attached_to_source_host
        || !selector.any_required_keywords.is_empty()
        || selector.face_down == Some(false)
    {
        return None;
    }

    let mut clause = ObjectClause::any();
    match &selector.name {
        NameFilter::Any => {}
        NameFilter::Named(name) => {
            clause.exact_name = Some(ExactNameKey::from_name(name.as_str())?);
        }
        NameFilter::SharesWithSource => {
            clause.source_relation = SourceRelation::SharingNameWithAbilitySource;
        }
        NameFilter::DifferentFromSource => return None,
    }
    if selector.matches_source_entry_type {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_TYPE);
    }
    if selector.matches_source_entry_color {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_COLOR);
    }
    if let Some(is_cmd) = selector.commander {
        clause.flags = clause.flags.union(if is_cmd {
            ObjectFilterFlags::COMMANDER_ONLY
        } else {
            ObjectFilterFlags::NOT_COMMANDER
        });
    }
    if selector.requires_modified {
        clause.flags = clause.flags.union(ObjectFilterFlags::MODIFIED);
    }
    if selector.face_down == Some(true) {
        clause.flags = clause.flags.union(ObjectFilterFlags::FACE_DOWN);
    }
    if selector.entered_this_turn {
        clause.flags = clause.flags.union(ObjectFilterFlags::ENTERED_THIS_TURN);
    }
    if selector.without_counters {
        clause.flags = clause.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
    }
    if selector.minus_one_counter {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER);
    }
    if let Some(kind) = selector.required_counter_kind {
        clause.required_counter_kind = Some(kind);
    }
    if let Some(is_hist) = selector.historic {
        clause.flags = clause.flags.union(if is_hist {
            ObjectFilterFlags::HISTORIC
        } else {
            ObjectFilterFlags::NON_HISTORIC
        });
    }
    if let Some(attacked) = selector.attacked_this_turn {
        clause.flags = clause.flags.union(if attacked {
            ObjectFilterFlags::ATTACKED_THIS_TURN
        } else {
            ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN
        });
    }
    if selector.dealt_damage_this_turn {
        clause.flags = clause
            .flags
            .union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN);
    }
    if selector.kicked {
        clause.flags = clause.flags.union(ObjectFilterFlags::KICKED);
    }
    if selector.renowned {
        clause.flags = clause.flags.union(ObjectFilterFlags::RENOWNED);
    }
    if selector.saddled {
        clause.flags = clause.flags.union(ObjectFilterFlags::SADDLED);
    }
    if selector.monstrous {
        clause.flags = clause.flags.union(ObjectFilterFlags::MONSTROUS);
    }
    if selector.suspected {
        clause.flags = clause.flags.union(ObjectFilterFlags::SUSPECTED);
    }
    if selector.solved {
        clause.flags = clause.flags.union(ObjectFilterFlags::SOLVED);
    }
    if selector.exerted {
        clause.flags = clause.flags.union(ObjectFilterFlags::EXERTED);
    }
    if let Some(owner) = selector.owner {
        clause.owner = match owner {
            OwnerSelector::SourceController => ControllerFilter::You,
            OwnerSelector::Opponents => ControllerFilter::Opponent,
        };
    }

    for ty in &selector.required_types {
        clause.required_types = clause.required_types.with(*ty);
    }
    for ty in &selector.any_types {
        clause.any_types = clause.any_types.with(*ty);
    }
    for ty in &selector.forbidden_types {
        clause.forbidden_types = clause.forbidden_types.with(*ty);
    }
    for st in &selector.required_supertypes {
        clause.required_supertypes = clause
            .required_supertypes
            .union(SupertypeMask::from_supertype(*st));
    }
    for st in &selector.forbidden_supertypes {
        clause.forbidden_supertypes = clause
            .forbidden_supertypes
            .union(SupertypeMask::from_supertype(*st));
    }

    let mut subtype_count = 0usize;
    if let Some(ct) = selector.creature_subtype {
        clause.subtypes = SubtypeFilter::Has(ct);
        subtype_count += 1;
    }
    let to_single = |s: SelectorSubtype| -> SingleSubtype {
        match s {
            SelectorSubtype::Creature(ct) => SingleSubtype::Creature(ct),
            SelectorSubtype::Land(lt) => SingleSubtype::Land(lt),
            SelectorSubtype::Artifact(at) => SingleSubtype::Artifact(at),
            SelectorSubtype::Enchantment(et) => SingleSubtype::Enchantment(et),
            SelectorSubtype::Planeswalker(pw) => SingleSubtype::Planeswalker(pw),
            SelectorSubtype::Aura => SingleSubtype::Aura,
        }
    };
    if !selector.any_creature_subtypes.is_empty() {
        subtype_count += 1;
        let items: Vec<SingleSubtype> = selector
            .any_creature_subtypes
            .iter()
            .copied()
            .map(SingleSubtype::Creature)
            .collect();
        clause.subtypes = SubtypeFilter::AnyOfList(SubtypeList::from_slice(&items)?);
    }
    if !selector.any_land_subtypes.is_empty() {
        subtype_count += 1;
        let mask = selector
            .any_land_subtypes
            .iter()
            .copied()
            .fold(0u32, |acc, lt| acc | lt.bit());
        clause.subtypes = SubtypeFilter::AnyLandMask(mask);
    }
    if !selector.required_subtypes.is_empty() {
        subtype_count += 1;
        let items: Vec<SingleSubtype> = selector
            .required_subtypes
            .iter()
            .copied()
            .map(to_single)
            .collect();
        clause.subtypes = SubtypeFilter::AllOfList(SubtypeList::from_slice(&items)?);
    }
    if !selector.any_subtypes.is_empty() {
        subtype_count += 1;
        let items: Vec<SingleSubtype> = selector
            .any_subtypes
            .iter()
            .copied()
            .map(to_single)
            .collect();
        clause.subtypes = SubtypeFilter::AnyOfList(SubtypeList::from_slice(&items)?);
    }
    if !selector.forbidden_subtypes.is_empty() {
        subtype_count += 1;
        let items: Vec<SingleSubtype> = selector
            .forbidden_subtypes
            .iter()
            .copied()
            .map(to_single)
            .collect();
        clause.subtypes = SubtypeFilter::NoneOfList(SubtypeList::from_slice(&items)?);
    }
    if let Some(ct) = selector.forbidden_creature_subtype {
        clause.subtypes = SubtypeFilter::Lacks(ct);
        subtype_count += 1;
    }
    if let Some(lt) = selector.land_subtype {
        clause.subtypes = SubtypeFilter::Land(lt);
        subtype_count += 1;
    }
    if let Some(at) = selector.artifact_subtype {
        clause.subtypes = SubtypeFilter::Artifact(at);
        subtype_count += 1;
    }
    if let Some(pt) = selector.planeswalker_subtype {
        clause.subtypes = SubtypeFilter::Planeswalker(pt);
        subtype_count += 1;
    }
    if subtype_count > 1 {
        return None;
    }

    clause.controller = match selector.controller {
        ControllerSelector::Any => ControllerFilter::Any,
        ControllerSelector::SourceController => ControllerFilter::You,
        ControllerSelector::Opponents => ControllerFilter::Opponent,
        ControllerSelector::OwnerIsSourceController => ControllerFilter::OwnerYou,
        ControllerSelector::OwnerIsOpponent => ControllerFilter::OwnerOpponent,
        ControllerSelector::SourceItself => {
            if selector.attached_to_source
                || selector.exclude_source
                || clause.source_relation != SourceRelation::Any
            {
                return None;
            }
            clause.source_relation = SourceRelation::SelfObject;
            ControllerFilter::Any
        }
        ControllerSelector::SelfAtMaxSpeed
        | ControllerSelector::SourcePair
        | ControllerSelector::EnchantedPlayer => return None,
    };

    if selector.attached_to_source && selector.exclude_source {
        return None;
    }
    if selector.attached_to_source {
        if clause.source_relation != SourceRelation::Any {
            return None;
        }
        clause.source_relation = SourceRelation::AttachedToSelf;
    } else if selector.exclude_source {
        clause.source_relation = match clause.source_relation {
            SourceRelation::Any => SourceRelation::OtherThanAbilitySource,
            SourceRelation::SharingNameWithAbilitySource => {
                SourceRelation::OtherThanAbilitySourceSharingName
            }
            _ => return None,
        };
    }

    clause.combat = match selector.combat_role {
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

    clause.tapped = match (selector.requires_tapped, selector.requires_untapped) {
        (true, true) => return None,
        (true, false) => Some(true),
        (false, true) => Some(false),
        (false, false) => None,
    };
    clause.token = selector.token;

    let bits_to_color_mask = |bits: u8| -> Option<ColorMask> {
        let mut mask = ColorMask::EMPTY;
        for color in Color::ALL {
            if (bits & color.bit()) != 0 {
                mask = mask.with(color);
            }
        }
        (!mask.is_empty()).then_some(mask)
    };
    clause.color = match selector.color {
        ColorFilter::Any => ObjectColorFilter::Any,
        ColorFilter::Colorless => ObjectColorFilter::Colorless,
        ColorFilter::Colored => ObjectColorFilter::Colored,
        ColorFilter::Monocolored => ObjectColorFilter::Monocolored,
        ColorFilter::Multicolored => ObjectColorFilter::Multicolored,
        ColorFilter::Has(c) => ObjectColorFilter::has(c),
        ColorFilter::Lacks(c) => ObjectColorFilter::lacks(c),
        ColorFilter::AnyOf(bits) => ObjectColorFilter::any_of(bits_to_color_mask(bits)?),
        ColorFilter::LacksAll(bits) => ObjectColorFilter::none_of(bits_to_color_mask(bits)?),
    };

    clause.keyword = match (selector.requires_flying, selector.required_keyword) {
        (false, kw) => kw,
        (true, None) => Some(KeywordAbility::Flying),
        (true, Some(KeywordAbility::Flying)) => Some(KeywordAbility::Flying),
        (true, Some(_)) => return None,
    };
    clause.forbidden_keyword = selector.forbidden_keyword;

    if selector.exclude_auras {
        clause.flags = clause.flags.union(ObjectFilterFlags::NON_AURA);
    }
    if selector.any_counter {
        clause.flags = clause.flags.union(ObjectFilterFlags::WITH_ANY_COUNTER);
    }
    if selector.plus_one_counter || selector.minimum_plus_one_counters == 1 {
        clause.flags = clause.flags.union(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER);
    }
    if selector.equipped_by_at_least == 1 {
        clause.flags = clause.flags.union(ObjectFilterFlags::EQUIPPED);
    }
    if selector.requires_enchanted {
        clause.flags = clause.flags.union(ObjectFilterFlags::ENCHANTED);
    }

    clause.power = match (selector.min_power, selector.max_power) {
        (None, None) => PowerFilter::Any,
        (Some(min), None) => PowerFilter::AtLeast(i16::try_from(min).ok()?),
        (None, Some(max)) => PowerFilter::AtMost(u16::try_from(max).ok()?),
        (Some(min), Some(max)) if min == max => PowerFilter::Equal(i16::try_from(min).ok()?),
        _ => return None,
    };
    clause.toughness = match (selector.min_toughness, selector.max_toughness) {
        (None, None) => ToughnessFilter::Any,
        (Some(min), None) => ToughnessFilter::AtLeast(i16::try_from(min).ok()?),
        (None, Some(max)) => ToughnessFilter::AtMost(u16::try_from(max).ok()?),
        (Some(min), Some(max)) if min == max => ToughnessFilter::Equal(i16::try_from(min).ok()?),
        _ => return None,
    };
    clause.mana_value = match (selector.min_mana_value, selector.max_mana_value) {
        (None, None) => ManaValueFilter::Any,
        (Some(min), None) => ManaValueFilter::AtLeast(min),
        (None, Some(max)) => ManaValueFilter::AtMost(max),
        (Some(min), Some(max)) if min == max => ManaValueFilter::Equal(min),
        _ => return None,
    };

    Some(ObjectFilter::Clause(clause))
}

fn static_type_change_to_spec(stc: StaticTypeChange) -> Option<TypeChangeSpec> {
    let subtypes = match stc.subtypes {
        SubtypeOperation::Keep => SubtypeOperation::Keep,
        SubtypeOperation::Add(StaticSubtypeOperand::Literal(s)) => {
            SubtypeOperation::Add(InstructionSubtypeOperand::Literal(s))
        }
        SubtypeOperation::Set(StaticSubtypeOperand::Literal(s)) => {
            SubtypeOperation::Set(InstructionSubtypeOperand::Literal(s))
        }
        SubtypeOperation::Remove(s) => SubtypeOperation::Remove(s),
        _ => return None,
    };
    let land_subtypes = match stc.land_subtypes {
        SubtypeOperation::Keep => SubtypeOperation::Keep,
        SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(s)) => SubtypeOperation::Add(s),
        SubtypeOperation::Set(StaticLandSubtypeOperand::Literal(s)) => SubtypeOperation::Set(s),
        SubtypeOperation::Remove(s) => SubtypeOperation::Remove(s),
        _ => return None,
    };
    Some(TypeChangeSpec {
        land_subtypes,
        supertypes: stc.supertypes,
        types: stc.types,
        subtypes,
    })
}

fn numeric_expr_to_value_expr(expr: NumericExpression) -> Option<ValueExpr> {
    match expr {
        NumericExpression::Literal(v) => Some(ValueExpr::Literal(v)),
        NumericExpression::Eval(v) => Some(*v),
        NumericExpression::Count {
            selector,
            multiplier,
            offset,
        } => {
            let filter = permanent_selector_to_object_filter(&selector)?;
            let mut ve = ValueExpr::Count {
                zone: ValueZone::Battlefield,
                filter: Box::new(filter),
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
        NumericExpression::Counters { source: None, kind } if kind.is_permanent() => {
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

fn static_part_to_resolved_part_spec(part: StaticPart) -> Result<ResolvedPartSpec, &'static str> {
    match part {
        StaticPart::Type(stc) => Ok(ResolvedPartSpec::Type(
            static_type_change_to_spec(stc).ok_or("oracle:unsupported-grant-ability-text")?,
        )),
        StaticPart::TypeWithSubtypes {
            base,
            artifact_subtypes,
            enchantment_subtypes,
        } => Ok(ResolvedPartSpec::TypeWithSubtypes {
            base: static_type_change_to_spec(base)
                .ok_or("oracle:unsupported-grant-ability-text")?,
            artifact_subtypes,
            enchantment_subtypes,
        }),
        StaticPart::Color(op) => {
            let converted = match op {
                ColorOperation::Set(StaticColorOperand::Literal(c)) => ColorOperation::Set(c),
                ColorOperation::Add(StaticColorOperand::Literal(c)) => ColorOperation::Add(c),
                ColorOperation::Remove(StaticColorOperand::Literal(c)) => ColorOperation::Remove(c),
                _ => return Err("oracle:unsupported-grant-ability-text"),
            };
            Ok(ResolvedPartSpec::Color(converted))
        }
        StaticPart::GrantTrigger(ability) => Ok(ResolvedPartSpec::GrantTrigger(ability)),
        StaticPart::GrantActivated(ability) => Ok(ResolvedPartSpec::GrantActivated(ability)),
        StaticPart::GrantActivatedRestricted {
            ability,
            restriction,
        } => Ok(ResolvedPartSpec::GrantActivatedRestricted {
            ability,
            restriction,
        }),
        StaticPart::GrantLoyalty {
            ability,
            loyalty,
            variable_loyalty,
        } => Ok(ResolvedPartSpec::GrantLoyalty {
            ability,
            loyalty,
            variable_loyalty,
        }),
        StaticPart::GrantAnyColorMana => Ok(ResolvedPartSpec::GrantAnyColorMana),
        StaticPart::GrantFixedColorMana(color) => Ok(ResolvedPartSpec::GrantFixedColorMana(color)),
        StaticPart::GrantMana(m) => Ok(ResolvedPartSpec::GrantMana(m)),
        StaticPart::Abilities(change) => Ok(ResolvedPartSpec::Abilities(change)),
        StaticPart::BlockingRestriction(br) => Ok(ResolvedPartSpec::BlockingRestriction(br)),
        StaticPart::ModifyPT(pair) => match (pair.power, pair.toughness) {
            (NumericExpression::Literal(power), NumericExpression::Literal(toughness)) => {
                Ok(ResolvedPartSpec::ModifyPT(PowerToughness {
                    power,
                    toughness,
                }))
            }
            (p, t) => {
                let power =
                    numeric_expr_to_value_expr(p).ok_or("oracle:unsupported-grant-ability-text")?;
                let toughness =
                    numeric_expr_to_value_expr(t).ok_or("oracle:unsupported-grant-ability-text")?;
                Ok(ResolvedPartSpec::ModifyPTExpr {
                    power: Box::new(power),
                    toughness: Box::new(toughness),
                })
            }
        },
        StaticPart::SetPT(pair) => match (pair.power, pair.toughness) {
            (NumericExpression::Literal(power), NumericExpression::Literal(toughness)) => {
                Ok(ResolvedPartSpec::SetPT(PowerToughness { power, toughness }))
            }
            (p, t) => {
                let power =
                    numeric_expr_to_value_expr(p).ok_or("oracle:unsupported-grant-ability-text")?;
                let toughness =
                    numeric_expr_to_value_expr(t).ok_or("oracle:unsupported-grant-ability-text")?;
                Ok(ResolvedPartSpec::SetPTExpr {
                    power: Box::new(power),
                    toughness: Box::new(toughness),
                })
            }
        },
        StaticPart::SetPower(expr) => {
            let power =
                numeric_expr_to_value_expr(expr).ok_or("oracle:unsupported-grant-ability-text")?;
            Ok(ResolvedPartSpec::SetPower(Box::new(power)))
        }
        StaticPart::SetToughness(expr) => {
            let toughness =
                numeric_expr_to_value_expr(expr).ok_or("oracle:unsupported-grant-ability-text")?;
            Ok(ResolvedPartSpec::SetToughness(Box::new(toughness)))
        }
        StaticPart::SwitchPT => Ok(ResolvedPartSpec::SwitchPT),
        _ => Err("oracle:unsupported-grant-ability-text"),
    }
}

fn push_granted_ability_change(
    parts: &mut Vec<StaticPart>,
    change: AbilityChange,
) -> Result<(), &'static str> {
    if let Some(StaticPart::Abilities(existing)) = parts
        .iter_mut()
        .find(|part| matches!(part, StaticPart::Abilities(_)))
    {
        if existing.removal.is_some()
            && change.removal.is_some()
            && existing.removal != change.removal
        {
            return Err("oracle:unsupported-grant-ability-text");
        }
        existing.granted_keywords.extend(change.granted_keywords);
        existing
            .suppressed_keywords
            .extend(change.suppressed_keywords);
        existing.doesnt_untap |= change.doesnt_untap;
        existing.grants_unblockable |= change.grants_unblockable;
        existing.granted_exalted = existing
            .granted_exalted
            .checked_add(change.granted_exalted)
            .ok_or("oracle:exalted-ovf")?;
        if let Some(extra_toxic) = change.granted_toxic {
            existing.granted_toxic = Some(match &existing.granted_toxic {
                Some(prev) => prev.plus(&extra_toxic),
                None => extra_toxic,
            });
        }
        existing
            .granted_combat_keywords
            .extend(change.granted_combat_keywords);
        existing.granted_triggers.extend(change.granted_triggers);
        if existing.removal.is_none() {
            existing.removal = change.removal;
        }
        if let Some(suppression) = change.suppressed_activations {
            existing.suppressed_activations = match (existing.suppressed_activations, suppression) {
                (Some(ActivationSuppression::All), _) | (_, ActivationSuppression::All) => {
                    Some(ActivationSuppression::All)
                }
                _ => Some(ActivationSuppression::NonMana),
            };
        }
        Ok(())
    } else {
        parts.push(StaticPart::Abilities(change));
        Ok(())
    }
}

fn push_granted_keyword_strings(
    parts: &mut Vec<StaticPart>,
    keywords: &[String],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    let (special_parts, remaining) = partition_static_keyword_grants(keywords, vocabulary, ids);
    parts.extend(special_parts);
    if !remaining.is_empty() {
        let ab_change = lower_ability_change_with_ids(&remaining, vocabulary, ids)?;
        push_granted_ability_change(parts, ab_change)?;
    }
    Ok(())
}

fn try_lower_granted_static_restriction(
    st: &StaticAbilityAst,
    vocabulary: &CreatureTypeVocabulary,
    parts: &mut Vec<StaticPart>,
) -> bool {
    let baseline = Program::enchantment(None);
    let mut probe = baseline.clone();
    let mut probe_ids = LoweringIds {
        next_activated: 0,
        next_triggered: 0,
        next_mana: 0,
        next_static: 0,
        next_ct_slot: 0,
    };
    if lower_static(st, &mut probe, vocabulary, &mut probe_ids, false).is_err()
        || !probe.static_conditions.is_empty()
    {
        return false;
    }
    let mut extracted = Vec::new();
    for ab in probe
        .continuous
        .take()
        .into_iter()
        .chain(probe.additional_battlefield_abilities.drain(..))
    {
        match ab {
            StaticContinuousAbility::Restriction { rule, .. } => extracted.push(rule),
            _ => return false,
        }
    }
    if extracted.is_empty() || probe != baseline {
        return false;
    }
    for rule in extracted {
        parts.push(StaticPart::GrantRestriction(Box::new(rule)));
    }
    true
}

pub(super) fn lower_self_cda_pt_static_part(
    power: &ValueExprAst,
    toughness: &ValueExprAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StaticPart, &'static str> {
    let expr_ctx = StaticExprContext {
        anchor: StaticSelfAnchor::Source,
        has_chosen_player_entry: false,
    };
    let p = lower_numeric_expr_with_context(power, expr_ctx, vocabulary)?;
    let t = lower_numeric_expr_with_context(toughness, expr_ctx, vocabulary)?;
    Ok(StaticPart::SetPT(NumericPair {
        power: p,
        toughness: t,
    }))
}

fn lower_granted_static_parts(
    st: &StaticAbilityAst,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    parts: &mut Vec<StaticPart>,
) -> Result<(), &'static str> {
    match st {
        StaticAbilityAst::Compound(items) if !items.is_empty() => {
            for item in items {
                lower_granted_static_parts(item, vocabulary, ids, parts)?;
            }
            Ok(())
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
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            parts.push(lower_modify_pt_part(
                dx,
                dy,
                scale.as_deref(),
                StaticExprContext {
                    anchor: StaticSelfAnchor::Source,
                    has_chosen_player_entry: false,
                },
                vocabulary,
            )?);
            if !grant_keywords.is_empty() {
                push_granted_keyword_strings(parts, grant_keywords, vocabulary, ids)?;
            }
            if let Some(raw_text) = grant_ability_text {
                let cleaned = raw_text
                    .trim()
                    .trim_matches('"')
                    .trim()
                    .trim_end_matches(',')
                    .trim();
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let synthetic = compile_card(
                    &CardInput {
                        oracle_id: "",
                        name: "~",
                        layout: "normal",
                        faces: vec![spellwake_oracle_compiler::FaceInput {
                            name: "~",
                            type_line: if looks_like_loyalty_ability(cleaned) {
                                "Planeswalker"
                            } else {
                                "Creature"
                            },
                            oracle_text: cleaned,
                        }],
                    },
                    &compiler_vocab,
                );
                if !synthetic.strictly_compiled || synthetic.faces.len() != 1 {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                let nested =
                    lower_granted_ability_face_parts(&synthetic.faces[0], vocabulary, ids)?;
                for part in nested {
                    if let StaticPart::Abilities(change) = part {
                        push_granted_ability_change(parts, change)?;
                    } else {
                        parts.push(part);
                    }
                }
            }
            if let Some(paired) = paired_rule {
                lower_granted_static_parts(paired, vocabulary, ids, parts)?;
            }
            Ok(())
        }
        StaticAbilityAst::DynamicModifyPT {
            subject,
            power_delta,
            toughness_delta,
            condition: None,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let expr_ctx = StaticExprContext {
                anchor: StaticSelfAnchor::Source,
                has_chosen_player_entry: false,
            };
            let power = lower_numeric_expr_with_context(power_delta, expr_ctx, vocabulary)?;
            let toughness = lower_numeric_expr_with_context(toughness_delta, expr_ctx, vocabulary)?;
            parts.push(StaticPart::ModifyPT(NumericPair { power, toughness }));
            Ok(())
        }
        StaticAbilityAst::GrantKeywords {
            subject,
            keywords,
            condition: None,
            paired_rule,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) && !keywords.is_empty() => {
            push_granted_keyword_strings(parts, keywords, vocabulary, ids)?;
            if let Some(paired) = paired_rule {
                lower_granted_static_parts(paired, vocabulary, ids, parts)?;
            }
            Ok(())
        }
        StaticAbilityAst::GrantKeywordWithCost {
            subject,
            keyword,
            cost_rule,
            condition: None,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let combined = format!("{} {}", keyword.trim(), cost_rule.trim());
            if let Some(part) = parse_granted_equip_or_crew_part(&combined, vocabulary) {
                parts.push(part);
                return Ok(());
            }
            if keyword.trim().eq_ignore_ascii_case("ward")
                && let Some(tr) = lower_granted_ward_trigger(cost_rule, vocabulary).or_else(|| {
                    lower_dynamic_ward_trigger(cost_rule, vocabulary, TriggeredAbilityId(0))
                })
            {
                let mut change = AbilityChange::default();
                change.granted_triggers.push(tr);
                return push_granted_ability_change(parts, change);
            }
            if keyword.trim().eq_ignore_ascii_case("mobilize")
                && let Some(tr) =
                    lower_dynamic_mobilize_trigger(cost_rule, vocabulary, TriggeredAbilityId(0))
            {
                let mut change = AbilityChange::default();
                change.granted_triggers.push(tr);
                return push_granted_ability_change(parts, change);
            }
            Err("oracle:unsupported-grant-ability-text")
        }
        StaticAbilityAst::SetOrAddType {
            subject,
            type_or_subtype,
            additive,
            condition: None,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let type_parts = parse_static_type_and_color_parts_with_entry(
                type_or_subtype,
                *additive,
                None,
                vocabulary,
            )?;
            parts.extend(type_parts);
            Ok(())
        }
        StaticAbilityAst::CombatDamageAssignmentRule {
            subject,
            rule,
            condition: None,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let r_low = rule
                .trim()
                .trim_end_matches('.')
                .trim()
                .to_ascii_lowercase();
            match r_low.as_str() {
                "toughness_instead_of_power"
                | "assigns combat damage equal to its toughness rather than its power"
                | "assign combat damage equal to their toughness rather than their power" => {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::AssignDamageByToughness,
                    ));
                    Ok(())
                }
                "assign_combat_damage_unblocked" | "assigndamageasthoughunblocked" => {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::AssignDamageAsThoughUnblocked,
                    ));
                    Ok(())
                }
                _ => Err("oracle:unsupported-grant-ability-text"),
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
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let mut has_color_part = false;
            if let Some(desc) = set_types {
                let type_parts = parse_static_type_and_color_parts_with_entry(
                    desc,
                    *additive_types,
                    None,
                    vocabulary,
                )?;
                if type_parts
                    .iter()
                    .any(|part| matches!(part, StaticPart::Color(_)))
                {
                    has_color_part = true;
                }
                parts.extend(type_parts);
            }
            if let Some(color_str) = set_colors {
                if has_color_part {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                let color_clean = color_str.trim().trim_end_matches('.').trim();
                let colors = if color_clean.eq_ignore_ascii_case("all colors") {
                    BTreeSet::from([
                        Color::White,
                        Color::Blue,
                        Color::Black,
                        Color::Red,
                        Color::Green,
                    ])
                } else {
                    let mut parsed_colors = BTreeSet::new();
                    let mut saw_colorless = false;
                    for raw_tok in color_clean
                        .replace(" and ", " ")
                        .replace(',', " ")
                        .split_whitespace()
                    {
                        match raw_tok.to_ascii_lowercase().as_str() {
                            "white" => {
                                parsed_colors.insert(Color::White);
                            }
                            "blue" => {
                                parsed_colors.insert(Color::Blue);
                            }
                            "black" => {
                                parsed_colors.insert(Color::Black);
                            }
                            "red" => {
                                parsed_colors.insert(Color::Red);
                            }
                            "green" => {
                                parsed_colors.insert(Color::Green);
                            }
                            "colorless" => {
                                saw_colorless = true;
                            }
                            "a" | "an" => {}
                            _ => return Err("oracle:unsupported-grant-ability-text"),
                        }
                    }
                    if parsed_colors.is_empty() && !saw_colorless {
                        return Err("oracle:unsupported-grant-ability-text");
                    }
                    parsed_colors
                };
                parts.push(StaticPart::Color(ColorOperation::Set(
                    StaticColorOperand::Literal(colors),
                )));
            }
            if *lose_all_abilities || !grant_keywords.is_empty() || !lose_keywords.is_empty() {
                let (special_parts, rem_kws) =
                    partition_static_keyword_grants(grant_keywords, vocabulary, ids);
                parts.extend(special_parts);
                if *lose_all_abilities || !rem_kws.is_empty() || !lose_keywords.is_empty() {
                    let mut change = if !rem_kws.is_empty() {
                        lower_ability_change_with_ids(&rem_kws, vocabulary, ids)?
                    } else {
                        AbilityChange::default()
                    };
                    if *lose_all_abilities {
                        change.removal = Some(AbilityRemoval::All);
                    }
                    if !lose_keywords.is_empty() {
                        change
                            .suppressed_keywords
                            .extend(lower_keyword_list_with_vocab(lose_keywords, vocabulary)?);
                    }
                    push_granted_ability_change(parts, change)?;
                }
            }
            if let Some((p_raw, t_raw)) = base_pt {
                parts.push(lower_static_base_pt_part(
                    p_raw,
                    t_raw,
                    vocabulary,
                    "oracle:unsupported-grant-ability-text",
                )?);
            }
            Ok(())
        }
        StaticAbilityAst::RuleOrCombatRestriction {
            subject,
            rule,
            filter,
            condition: None,
        } if matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject)) => {
            if filter.is_none()
                && let Some(lock) = parse_static_ability_lock(rule, false)
                && (!lock.singular_attached_fallback
                    || matches!(subject.as_deref(), Some(SubjectRefAst::SelfObject)))
            {
                if lock.cannot_attack_or_block {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::CannotAttackOrBlock,
                    ));
                }
                if lock.cannot_crew_vehicles {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::CannotCrewVehicles,
                    ));
                }
                if lock.suppressed_activations.is_some() || lock.doesnt_untap {
                    return push_granted_ability_change(
                        parts,
                        AbilityChange {
                            suppressed_activations: lock.suppressed_activations,
                            doesnt_untap: lock.doesnt_untap,
                            ..AbilityChange::default()
                        },
                    );
                }
                return Ok(());
            }
            if let Some(obj) =
                parse_cant_block_or_be_blocked_by_filter(rule, filter.as_deref(), vocabulary)
            {
                parts.push(StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotBlockMatching(obj),
                ));
                parts.push(StaticPart::BlockingRestriction(
                    BlockingRestriction::CannotBeBlockedByMatching(obj),
                ));
                return Ok(());
            }
            if filter.is_none() && is_menace_blocking_rule(rule) {
                return push_granted_ability_change(
                    parts,
                    AbilityChange {
                        granted_keywords: BTreeSet::from([KeywordAbility::Menace]),
                        ..AbilityChange::default()
                    },
                );
            }
            if filter.is_none()
                && let Some(brs) = lower_combat_mod_blocking_restrictions(rule, vocabulary)
            {
                for br in brs {
                    parts.push(StaticPart::BlockingRestriction(br));
                }
                return Ok(());
            }
            if let Some(br) = lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary) {
                parts.push(StaticPart::BlockingRestriction(br));
                return Ok(());
            }
            if filter.is_none() {
                let r_low = rule.trim().to_ascii_lowercase();
                if r_low == "blockseachcombatifable" || r_low == "blocks each combat if able" {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::MustBlockEachCombatIfAble,
                    ));
                    return Ok(());
                }
                if r_low == "attackseachcombatifable"
                    || r_low == "attacks each combat if able"
                    || r_low == "attacks each turn if able"
                {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::MustAttackEachCombatIfAble,
                    ));
                    return Ok(());
                }
                if matches!(
                    r_low.as_str(),
                    "cantattackyou"
                        | "can't attack you or planeswalkers you control"
                        | "can't attack you or a planeswalker you control"
                ) {
                    parts.push(StaticPart::BlockingRestriction(
                        BlockingRestriction::CannotAttackSourceController,
                    ));
                    return Ok(());
                }
                if let Some(target_src) = r_low.strip_prefix("cantbetargetof:") {
                    let ts = target_src.trim();
                    let granted_kws: Option<BTreeSet<KeywordAbility>> = if ts
                        == "spells or abilities"
                    {
                        Some(BTreeSet::from([KeywordAbility::Shroud]))
                    } else if ts == "spells or abilities your opponents control" {
                        Some(BTreeSet::from([KeywordAbility::Hexproof]))
                    } else if let Some(qual_raw) =
                        ts.strip_suffix(" spells or abilities your opponents control")
                    {
                        parse_hexproof_from_qualities(qual_raw.trim(), vocabulary).map(|quals| {
                            quals
                                .into_iter()
                                .map(KeywordAbility::HexproofFrom)
                                .collect()
                        })
                    } else {
                        None
                    };
                    if let Some(kws) = granted_kws {
                        return push_granted_ability_change(
                            parts,
                            AbilityChange {
                                granted_keywords: kws,
                                ..AbilityChange::default()
                            },
                        );
                    }
                }
            }
            if try_lower_granted_static_restriction(st, vocabulary, parts) {
                return Ok(());
            }
            Err("oracle:unsupported-grant-ability-text")
        }
        StaticAbilityAst::CantBeBlockedExceptByAny {
            subject,
            allowed_filters,
            condition: None,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let br = lower_cant_be_blocked_except_by_any(allowed_filters, vocabulary)
                .ok_or("oracle:unsupported-grant-ability-text")?;
            parts.push(StaticPart::BlockingRestriction(br));
            Ok(())
        }
        _ => {
            if try_lower_granted_static_restriction(st, vocabulary, parts) {
                return Ok(());
            }
            Err("oracle:unsupported-grant-ability-text")
        }
    }
}

pub(super) fn lower_granted_ability_face(
    compiled_face: &FaceAst,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<StaticPart, &'static str> {
    if !compiled_face.failures.is_empty()
        || !compiled_face.statics.is_empty()
        || !compiled_face.replacements.is_empty()
        || !compiled_face.keywords.is_empty()
        || !compiled_face.spell_effects.is_empty()
        || compiled_face.abilities.len() != 1
    {
        return Err("oracle:unsupported-grant-ability-text");
    }
    lower_single_granted_ability(&compiled_face.abilities[0], vocabulary, ids)
}

pub(super) fn lower_granted_ability_face_parts(
    compiled_face: &FaceAst,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<Vec<StaticPart>, &'static str> {
    if !compiled_face.failures.is_empty()
        || !compiled_face.replacements.is_empty()
        || !compiled_face.spell_effects.is_empty()
    {
        return Err("oracle:unsupported-grant-ability-text");
    }
    if compiled_face.keywords.is_empty()
        && compiled_face.statics.is_empty()
        && compiled_face.abilities.len() == 1
    {
        return Ok(vec![lower_granted_ability_face(
            compiled_face,
            vocabulary,
            ids,
        )?]);
    }
    let mut parts = Vec::new();
    if !compiled_face.keywords.is_empty() {
        let mut kw_names = Vec::with_capacity(compiled_face.keywords.len());
        for kw in &compiled_face.keywords {
            let low_name = kw.name.trim().to_ascii_lowercase();
            let full = match (low_name.as_str(), kw.parameter.as_deref().map(str::trim)) {
                ("protection", Some(p)) if !p.is_empty() => {
                    if p.to_ascii_lowercase().starts_with("from ") {
                        format!("protection {p}")
                    } else {
                        format!("protection from {p}")
                    }
                }
                ("hexproof_from" | "hexproof", Some(p)) if !p.is_empty() => {
                    if p.to_ascii_lowercase().starts_with("from ") {
                        format!("hexproof {p}")
                    } else {
                        format!("hexproof from {p}")
                    }
                }
                ("landwalk", Some(p)) if !p.is_empty() => format!("{p}walk"),
                (_, Some(p)) if !p.is_empty() => format!("{} {p}", kw.name),
                _ => kw.name.clone(),
            };
            kw_names.push(full);
        }
        push_granted_keyword_strings(&mut parts, &kw_names, vocabulary, ids)?;
    }
    for st in &compiled_face.statics {
        lower_granted_static_parts(st, vocabulary, ids, &mut parts)?;
    }
    for ability in &compiled_face.abilities {
        parts.push(lower_single_granted_ability(ability, vocabulary, ids)?);
    }
    if parts.is_empty() {
        return Err("oracle:unsupported-grant-ability-text");
    }
    Ok(parts)
}

fn strip_granted_static_condition(
    st: &StaticAbilityAst,
) -> Option<(StaticAbilityAst, ConditionAst)> {
    let mut cloned = st.clone();
    let cond = match &mut cloned {
        StaticAbilityAst::ContinuousModifyPT { condition, .. }
        | StaticAbilityAst::DynamicModifyPT { condition, .. }
        | StaticAbilityAst::GrantKeywords { condition, .. }
        | StaticAbilityAst::GrantKeywordWithCost { condition, .. }
        | StaticAbilityAst::SetOrAddType { condition, .. }
        | StaticAbilityAst::ContinuousLayerEffect { condition, .. }
        | StaticAbilityAst::RuleOrCombatRestriction { condition, .. }
        | StaticAbilityAst::CantBeBlockedExceptByAny { condition, .. }
        | StaticAbilityAst::CombatDamageAssignmentRule { condition, .. } => condition.take()?,
        _ => return None,
    };
    Some((cloned, *cond))
}

fn rewrite_granted_condition_self_to_anaphora(cond: &ConditionAst) -> ConditionAst {
    let rw_subj = |s: &SubjectRefAst| -> SubjectRefAst {
        if matches!(s, SubjectRefAst::SelfObject) {
            SubjectRefAst::Anaphora("it".to_string())
        } else {
            s.clone()
        }
    };
    match cond {
        ConditionAst::Not(inner) => {
            ConditionAst::Not(Box::new(rewrite_granted_condition_self_to_anaphora(inner)))
        }
        ConditionAst::And(a, b) => ConditionAst::And(
            Box::new(rewrite_granted_condition_self_to_anaphora(a)),
            Box::new(rewrite_granted_condition_self_to_anaphora(b)),
        ),
        ConditionAst::Or(a, b) => ConditionAst::Or(
            Box::new(rewrite_granted_condition_self_to_anaphora(a)),
            Box::new(rewrite_granted_condition_self_to_anaphora(b)),
        ),
        ConditionAst::HasStatus { subject, status } => ConditionAst::HasStatus {
            subject: Box::new(rw_subj(subject)),
            status: status.clone(),
        },
        ConditionAst::HasKeyword { subject, keyword } => ConditionAst::HasKeyword {
            subject: Box::new(rw_subj(subject)),
            keyword: keyword.clone(),
        },
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } => ConditionAst::CounterCompare {
            subject: Box::new(rw_subj(subject)),
            counter_kind: counter_kind.clone(),
            op: *op,
            value: *value,
        },
        ConditionAst::StatCompare {
            subject,
            stat,
            op,
            value,
        } => ConditionAst::StatCompare {
            subject: Box::new(rw_subj(subject)),
            stat: *stat,
            op: *op,
            value: *value,
        },
        ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(subject)) => ConditionAst::Marker(
            GameMarker::SubjectEnteredThisTurn(Box::new(rw_subj(subject))),
        ),
        ConditionAst::Marker(GameMarker::AttackedThisTurn { subject }) => {
            ConditionAst::Marker(GameMarker::AttackedThisTurn {
                subject: Box::new(rw_subj(subject)),
            })
        }
        other => other.clone(),
    }
}

fn emit_granted_ability_face_for_selectors(
    compiled_face: &FaceAst,
    selectors: &[PermanentSelector],
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if !compiled_face.failures.is_empty()
        || !compiled_face.replacements.is_empty()
        || !compiled_face.spell_effects.is_empty()
    {
        return Err("oracle:unsupported-grant-ability-text");
    }
    if let Ok(parts) = lower_granted_ability_face_parts(compiled_face, vocabulary, ids) {
        for selector in selectors {
            for part in &parts {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(vec![part.clone()]),
                    },
                )?;
            }
        }
        return Ok(());
    }
    let mut base_parts = Vec::new();
    if !compiled_face.keywords.is_empty() {
        let mut kw_names = Vec::with_capacity(compiled_face.keywords.len());
        for kw in &compiled_face.keywords {
            let low_name = kw.name.trim().to_ascii_lowercase();
            let full = match (low_name.as_str(), kw.parameter.as_deref().map(str::trim)) {
                ("protection", Some(p)) if !p.is_empty() => {
                    if p.to_ascii_lowercase().starts_with("from ") {
                        format!("protection {p}")
                    } else {
                        format!("protection from {p}")
                    }
                }
                ("hexproof_from" | "hexproof", Some(p)) if !p.is_empty() => {
                    if p.to_ascii_lowercase().starts_with("from ") {
                        format!("hexproof {p}")
                    } else {
                        format!("hexproof from {p}")
                    }
                }
                ("landwalk", Some(p)) if !p.is_empty() => format!("{p}walk"),
                (_, Some(p)) if !p.is_empty() => format!("{} {p}", kw.name),
                _ => kw.name.clone(),
            };
            kw_names.push(full);
        }
        push_granted_keyword_strings(&mut base_parts, &kw_names, vocabulary, ids)?;
    }
    let mut conditional_entries: Vec<(ConditionAst, Vec<StaticPart>)> = Vec::new();
    let mut untap_during_other_players = false;
    for st in &compiled_face.statics {
        if let StaticAbilityAst::RuleOrCombatRestriction {
            subject,
            rule,
            filter: None,
            condition: None,
        } = st
            && rule == "UntapEachOtherUntapStep"
            && matches!(subject.as_deref(), None | Some(SubjectRefAst::SelfObject))
            && selectors
                .iter()
                .all(|sel| selector_guaranteed_source_controller(sel, program))
        {
            untap_during_other_players = true;
            continue;
        }
        if let Some((uncond_st, cond)) = strip_granted_static_condition(st) {
            let mut cond_parts = Vec::new();
            lower_granted_static_parts(&uncond_st, vocabulary, ids, &mut cond_parts)?;
            if cond_parts.is_empty() {
                return Err("oracle:unsupported-grant-ability-text");
            }
            conditional_entries.push((cond, cond_parts));
        } else {
            lower_granted_static_parts(st, vocabulary, ids, &mut base_parts)?;
        }
    }
    for ability in &compiled_face.abilities {
        base_parts.push(lower_single_granted_ability(ability, vocabulary, ids)?);
    }
    if base_parts.is_empty() && conditional_entries.is_empty() && !untap_during_other_players {
        return Err("oracle:unsupported-grant-ability-text");
    }
    for selector in selectors {
        if untap_during_other_players {
            push_static_ability(
                program,
                StaticContinuousAbility::UntapRule {
                    id: ids.static_id(),
                    rule: StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(Box::new(
                        selector.clone(),
                    )),
                },
            )?;
        }
        for part in &base_parts {
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ids.static_id(),
                    selector: selector.clone(),
                    parts: StaticParts(vec![part.clone()]),
                },
            )?;
        }
        let same_controller = selector_guaranteed_source_controller(selector, program);
        for (cond, cond_parts) in &conditional_entries {
            let rewritten = rewrite_granted_condition_self_to_anaphora(cond);
            let mut cond_sel = selector.clone();
            let before_sc = cond_sel.source_condition.clone();
            apply_static_selector_condition(&mut cond_sel, &rewritten, vocabulary)?;
            // If applying the granted static's condition introduced a source_condition,
            // it evaluates against the granting permanent's controller, which is only sound
            // when every matching recipient shares that controller.
            if cond_sel.source_condition != before_sc && !same_controller {
                return Err("oracle:unsupported-grant-ability-text");
            }
            for part in cond_parts {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: cond_sel.clone(),
                        parts: StaticParts(vec![part.clone()]),
                    },
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn lower_granted_ability_specs(
    compiled_face: &FaceAst,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<Vec<ResolvedPartSpec>, &'static str> {
    lower_granted_ability_face_parts(compiled_face, vocabulary, ids)?
        .into_iter()
        .map(static_part_to_resolved_part_spec)
        .collect()
}

pub(super) fn parse_static_type_part(
    raw: &str,
    additive: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StaticPart, &'static str> {
    parse_static_type_part_with_entry(raw, additive, None, vocabulary)
}

pub(super) fn parse_static_type_part_with_entry(
    raw: &str,
    additive: bool,
    entry_tap: Option<EntryTap>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StaticPart, &'static str> {
    let mut parts =
        parse_static_type_and_color_parts_with_entry(raw, additive, entry_tap, vocabulary)?;
    if parts.len() == 1 {
        parts.pop().ok_or("oracle:unsupported-type-change")
    } else {
        Err("oracle:unsupported-type-change")
    }
}

fn title_case_type_token(raw: &str) -> String {
    let mut chars = raw.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn resolve_two_word_creature_type(
    first: &str,
    second: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<CreatureTypeId> {
    let exact = format!("{first} {second}");
    if let Some(ct) = vocabulary.resolve_label(&exact) {
        return Some(ct);
    }
    let first_tc = title_case_type_token(&first.to_ascii_lowercase());
    let second_low = second.to_ascii_lowercase();
    let second_tc = title_case_type_token(&second_low);
    if let Some(ct) = vocabulary.resolve_label(&format!("{first_tc} {second_tc}")) {
        return Some(ct);
    }
    if let Some(stem) = second_low.strip_suffix("es")
        && let Some(ct) =
            vocabulary.resolve_label(&format!("{first_tc} {}", title_case_type_token(stem)))
    {
        return Some(ct);
    }
    if let Some(stem) = second_low.strip_suffix('s')
        && let Some(ct) =
            vocabulary.resolve_label(&format!("{first_tc} {}", title_case_type_token(stem)))
    {
        return Some(ct);
    }
    if let Some(stem) = second_low.strip_suffix("ves")
        && let Some(ct) = vocabulary.resolve_label(&format!(
            "{first_tc} {}",
            title_case_type_token(&format!("{stem}f"))
        ))
    {
        return Some(ct);
    }
    None
}

fn parse_static_type_and_color_parts_with_entry(
    raw: &str,
    additive: bool,
    entry_tap: Option<EntryTap>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Vec<StaticPart>, &'static str> {
    // Reject raw unparsed sentences or prefixed payloads such as "Equipment:...".
    if raw.contains(':') {
        return Err("oracle:unsupported-type-change");
    }
    let raw_low_full = raw.to_ascii_lowercase();
    let color_additive = raw_low_full.contains("in addition to its other colors")
        || raw_low_full.contains("in addition to their other colors");
    let mut additive = additive;
    let mut clean = raw
        .replace(" in addition to its other types", "")
        .replace(" in addition to their other types", "")
        .replace(" in addition to its other creature types", "")
        .replace(" in addition to their other creature types", "")
        .replace(" in addition to its other land types", "")
        .replace(" in addition to their other land types", "")
        .replace(" in addition to its other colors", "")
        .replace(" in addition to their other colors", "");
    let clean_trimmed_low = clean
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    for keep_suffix in [
        " that are still lands",
        " that's still a land",
        " that is still a land",
        ". it's still a land",
        ". it is still a land",
        ". they're still lands",
        " that are still artifacts",
        " that's still an artifact",
        " that is still an artifact",
        ". it's still an artifact",
        ". they're still artifacts",
    ] {
        if let Some(prefix) = clean_trimmed_low.strip_suffix(keep_suffix) {
            clean = prefix.trim().to_string();
            additive = true;
            break;
        }
    }
    let low_entry = clean
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    if low_entry == "no longer snow" {
        return Ok(vec![StaticPart::Type(StaticTypeChange {
            supertypes: TypeOperation::Remove(BTreeSet::from([Supertype::Snow])),
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
            land_subtypes: SubtypeOperation::Keep,
        })]);
    }
    if matches!(
        low_entry.as_str(),
        "lose_all_creature_types" | "loses all creature types" | "lose all creature types"
    ) {
        return Ok(vec![StaticPart::Type(StaticTypeChange {
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Set(StaticSubtypeOperand::Literal(BTreeSet::new())),
            land_subtypes: SubtypeOperation::Keep,
        })]);
    }
    if matches!(
        low_entry.as_str(),
        "the chosen basic land type"
            | "chosen basic land type"
            | "the chosen land type"
            | "chosen land type"
    ) || (matches!(low_entry.as_str(), "the chosen type" | "chosen type")
        && entry_tap == Some(EntryTap::ChooseBasicLandType))
    {
        return Ok(vec![StaticPart::Type(StaticTypeChange {
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
            land_subtypes: if additive {
                SubtypeOperation::Add(StaticLandSubtypeOperand::SourceEntryBasicLandType)
            } else {
                SubtypeOperation::Set(StaticLandSubtypeOperand::SourceEntryBasicLandType)
            },
        })]);
    }
    if matches!(
        low_entry.as_str(),
        "the chosen creature type" | "chosen creature type"
    ) || (matches!(low_entry.as_str(), "the chosen type" | "chosen type")
        && entry_tap == Some(EntryTap::ChooseCreatureType))
    {
        return Ok(vec![StaticPart::Type(StaticTypeChange {
            supertypes: TypeOperation::Keep,
            types: TypeOperation::Keep,
            subtypes: if additive {
                SubtypeOperation::Add(StaticSubtypeOperand::SourceEntryCreatureType)
            } else {
                SubtypeOperation::Set(StaticSubtypeOperand::SourceEntryCreatureType)
            },
            land_subtypes: SubtypeOperation::Keep,
        })]);
    }
    let low_clean = clean.to_ascii_lowercase();
    let all_creature_types =
        low_clean.contains("all creature types") || low_clean.contains("every creature type");
    if all_creature_types {
        clean = low_clean
            .replace("all creature types", "")
            .replace("every creature type", "");
    }
    let low_for_lands = clean.to_ascii_lowercase();
    let all_basic_land_types = low_for_lands.contains("all basic land types")
        || low_for_lands.contains("every basic land type");
    if all_basic_land_types {
        clean = low_for_lands
            .replace("all basic land types", "")
            .replace("every basic land type", "");
    }
    let clean = clean.replace(" and ", " ").replace(',', " ");
    let mut supertypes = BTreeSet::new();
    let mut card_types = BTreeSet::new();
    let mut creature_subtypes = BTreeSet::new();
    let mut land_subtypes = BTreeSet::new();
    if all_basic_land_types {
        land_subtypes.extend([
            LandType::Plains,
            LandType::Island,
            LandType::Swamp,
            LandType::Mountain,
            LandType::Forest,
        ]);
    }
    let mut artifact_subtypes = BTreeSet::new();
    let mut enchantment_subtypes = BTreeSet::new();
    let mut colors = BTreeSet::new();
    let mut saw_colorless = false;
    let mut saw_shard = false;

    let tokens: Vec<&str> = clean
        .split_whitespace()
        .map(|raw_tok| {
            raw_tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'')
        })
        .filter(|tok| !tok.is_empty())
        .collect();
    let mut idx = 0usize;
    while idx < tokens.len() {
        let tok = tokens[idx];
        let tok_low = tok.to_ascii_lowercase();
        if idx + 1 < tokens.len()
            && !matches!(
                tok_low.as_str(),
                "a" | "an"
                    | "the"
                    | "type"
                    | "types"
                    | "is"
                    | "are"
                    | "becomes"
                    | "become"
                    | "also"
                    | "still"
                    | "other"
                    | "white"
                    | "blue"
                    | "black"
                    | "red"
                    | "green"
                    | "colorless"
            )
            && let Some(ct) = resolve_two_word_creature_type(tok, tokens[idx + 1], vocabulary)
        {
            creature_subtypes.insert(ct);
            idx += 2;
            continue;
        }
        match tok_low.as_str() {
            "a" | "an" | "the" | "type" | "types" | "is" | "are" | "becomes" | "become"
            | "also" | "still" | "other" => {}
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
            "legendary" => {
                supertypes.insert(Supertype::Legendary);
            }
            "snow" => {
                supertypes.insert(Supertype::Snow);
            }
            "basic" => {
                supertypes.insert(Supertype::Basic);
            }
            "creature" | "creatures" => {
                card_types.insert(CardType::Creature);
            }
            "artifact" | "artifacts" => {
                card_types.insert(CardType::Artifact);
            }
            "enchantment" | "enchantments" => {
                card_types.insert(CardType::Enchantment);
            }
            "land" | "lands" => {
                card_types.insert(CardType::Land);
            }
            "planeswalker" | "planeswalkers" => {
                card_types.insert(CardType::Planeswalker);
            }
            "battle" | "battles" => {
                card_types.insert(CardType::Battle);
            }
            "attraction" | "attractions" => {
                artifact_subtypes.insert(ArtifactSubtype::Attraction);
            }
            "book" | "books" => {
                artifact_subtypes.insert(ArtifactSubtype::Book);
            }
            "contraption" | "contraptions" => {
                artifact_subtypes.insert(ArtifactSubtype::Contraption);
            }
            "equipment" => {
                artifact_subtypes.insert(ArtifactSubtype::Equipment);
            }
            "treasure" | "treasures" => {
                artifact_subtypes.insert(ArtifactSubtype::Treasure);
            }
            "food" | "foods" => {
                artifact_subtypes.insert(ArtifactSubtype::Food);
            }
            "clue" | "clues" => {
                artifact_subtypes.insert(ArtifactSubtype::Clue);
            }
            "vehicle" | "vehicles" => {
                artifact_subtypes.insert(ArtifactSubtype::Vehicle);
            }
            "blood" => {
                artifact_subtypes.insert(ArtifactSubtype::Blood);
            }
            "gold" => {
                artifact_subtypes.insert(ArtifactSubtype::Gold);
            }
            "map" | "maps" => {
                artifact_subtypes.insert(ArtifactSubtype::Map);
            }
            "mutagen" | "mutagens" => {
                artifact_subtypes.insert(ArtifactSubtype::Mutagen);
            }
            "junk" => {
                artifact_subtypes.insert(ArtifactSubtype::Junk);
            }
            "powerstone" | "powerstones" => {
                artifact_subtypes.insert(ArtifactSubtype::Powerstone);
            }
            "lander" | "landers" => {
                artifact_subtypes.insert(ArtifactSubtype::Lander);
            }
            "spacecraft" => {
                artifact_subtypes.insert(ArtifactSubtype::Spacecraft);
            }
            "incubator" | "incubators" => {
                artifact_subtypes.insert(ArtifactSubtype::Incubator);
            }
            "bobblehead" | "bobbleheads" => {
                artifact_subtypes.insert(ArtifactSubtype::Bobblehead);
            }
            "fortification" | "fortifications" => {
                artifact_subtypes.insert(ArtifactSubtype::Fortification);
            }
            "saga" | "sagas" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Saga);
            }
            "shrine" | "shrines" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Shrine);
            }
            "role" | "roles" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Role);
            }
            "curse" | "curses" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Curse);
            }
            "class" | "classes" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Class);
            }
            "case" | "cases" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Case);
            }
            "background" | "backgrounds" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Background);
            }
            "room" | "rooms" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Room);
            }
            "rune" | "runes" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Rune);
            }
            "cartouche" | "cartouches" => {
                enchantment_subtypes.insert(EnchantmentSubtype::Cartouche);
            }
            "shard" | "shards" => {
                saw_shard = true;
            }
            other => {
                let singular = other.strip_suffix('s').unwrap_or(other);
                if let Some(lt) = parse_land_subtype(tok)
                    .or_else(|| parse_land_subtype(&title_case_type_token(singular)))
                {
                    land_subtypes.insert(lt);
                } else if let Some(ct) = vocabulary
                    .resolve_label(tok)
                    .or_else(|| vocabulary.resolve_label(&title_case_type_token(other)))
                    .or_else(|| vocabulary.resolve_label(&title_case_type_token(singular)))
                    .or_else(|| {
                        other
                            .strip_suffix("es")
                            .and_then(|stem| vocabulary.resolve_label(&title_case_type_token(stem)))
                    })
                    .or_else(|| {
                        other.strip_suffix("ies").and_then(|stem| {
                            vocabulary
                                .resolve_label(&title_case_type_token(&format!("{stem}y")))
                                .or_else(|| {
                                    vocabulary
                                        .resolve_label(&title_case_type_token(&format!("{stem}ie")))
                                })
                        })
                    })
                {
                    creature_subtypes.insert(ct);
                } else {
                    return Err("oracle:unsupported-type-change");
                }
            }
        }
        idx += 1;
    }
    if saw_colorless && !colors.is_empty() {
        return Err("oracle:unsupported-type-change");
    }
    if saw_shard {
        if card_types.contains(&CardType::Artifact) && !card_types.contains(&CardType::Enchantment)
        {
            artifact_subtypes.insert(ArtifactSubtype::Shard);
        } else {
            enchantment_subtypes.insert(EnchantmentSubtype::Shard);
        }
    }
    let has_type_change = all_creature_types
        || !supertypes.is_empty()
        || !card_types.is_empty()
        || !creature_subtypes.is_empty()
        || !land_subtypes.is_empty()
        || !artifact_subtypes.is_empty()
        || !enchantment_subtypes.is_empty();
    let has_color_change = saw_colorless || !colors.is_empty();
    if !has_type_change && !has_color_change {
        return Err("oracle:unsupported-type-change");
    }
    let mut parts = Vec::new();
    if has_type_change {
        let base = StaticTypeChange {
            supertypes: if supertypes.is_empty() {
                TypeOperation::Keep
            } else if additive {
                TypeOperation::Add(supertypes)
            } else {
                TypeOperation::Set(supertypes)
            },
            types: if card_types.is_empty() {
                TypeOperation::Keep
            } else if additive {
                TypeOperation::Add(card_types)
            } else {
                TypeOperation::Set(card_types)
            },
            subtypes: if all_creature_types {
                if additive {
                    SubtypeOperation::Add(StaticSubtypeOperand::AllCreatureTypes)
                } else {
                    SubtypeOperation::Set(StaticSubtypeOperand::AllCreatureTypes)
                }
            } else if creature_subtypes.is_empty() {
                SubtypeOperation::Keep
            } else if additive {
                SubtypeOperation::Add(StaticSubtypeOperand::Literal(creature_subtypes))
            } else {
                SubtypeOperation::Set(StaticSubtypeOperand::Literal(creature_subtypes))
            },
            land_subtypes: if land_subtypes.is_empty() {
                SubtypeOperation::Keep
            } else if additive {
                SubtypeOperation::Add(StaticLandSubtypeOperand::Literal(land_subtypes))
            } else {
                SubtypeOperation::Set(StaticLandSubtypeOperand::Literal(land_subtypes))
            },
        };
        if artifact_subtypes.is_empty() && enchantment_subtypes.is_empty() {
            parts.push(StaticPart::Type(base));
        } else {
            parts.push(StaticPart::TypeWithSubtypes {
                base,
                artifact_subtypes: if artifact_subtypes.is_empty() {
                    SubtypeOperation::Keep
                } else if additive {
                    SubtypeOperation::Add(artifact_subtypes)
                } else {
                    SubtypeOperation::Set(artifact_subtypes)
                },
                enchantment_subtypes: if enchantment_subtypes.is_empty() {
                    SubtypeOperation::Keep
                } else if additive {
                    SubtypeOperation::Add(enchantment_subtypes)
                } else {
                    SubtypeOperation::Set(enchantment_subtypes)
                },
            });
        }
    }
    if has_color_change {
        let color_op = if saw_colorless {
            ColorOperation::Set(StaticColorOperand::Literal(BTreeSet::new()))
        } else if color_additive {
            ColorOperation::Add(StaticColorOperand::Literal(colors))
        } else {
            ColorOperation::Set(StaticColorOperand::Literal(colors))
        };
        parts.push(StaticPart::Color(color_op));
    }
    Ok(parts)
}

pub(super) fn lower_rule_blocking_restriction(
    rule: &str,
    filter: Option<&ObjectFilterAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<BlockingRestriction> {
    if let Some(flt) = filter {
        let norm_rule = strip_combat_duration(rule);
        if (flt.any_types.is_empty() || flt.any_types == AstCardTypeMask::CREATURE)
            && filter_only_has(
                flt,
                FilterAllow {
                    types: true,
                    ..FilterAllow::NONE
                },
            )
            && let Some(BlockingRestriction::CanBlockAnyNumber) =
                lower_blocking_restriction(&norm_rule)
        {
            return Some(BlockingRestriction::CanBlockAnyNumber);
        }
        if rule == "CanBlockOnly"
            && flt.required_keywords == ["flying"]
            && flt.any_types == AstCardTypeMask::CREATURE
            && filter_only_has(
                flt,
                FilterAllow {
                    types: true,
                    req_keywords: true,
                    ..FilterAllow::NONE
                },
            )
        {
            return Some(BlockingRestriction::CanBlockOnlyFlying);
        }
        if rule == "CantBeBlockedBy" {
            let allow_base = FilterAllow {
                types: true,
                ..FilterAllow::NONE
            };
            let types_creature_or_empty =
                flt.any_types.is_empty() || flt.any_types == AstCardTypeMask::CREATURE;
            if types_creature_or_empty {
                if let [
                    spellwake_oracle_compiler::ast::FilterPredicateAst::StateQualification(qual),
                ] = flt.predicates.as_slice()
                {
                    let mut clean = flt.clone();
                    clean.predicates.clear();
                    if filter_only_has(&clean, allow_base) {
                        let qual_low = qual.trim().to_ascii_lowercase();
                        if qual_low == "with greater power" {
                            return Some(BlockingRestriction::CannotBeBlockedByGreaterPower);
                        }
                        if qual_low == "with power less than ~'s" {
                            return Some(BlockingRestriction::BlockerPowerAtLeastSource);
                        }
                    }
                }
                if let [
                    spellwake_oracle_compiler::ast::StatBoundAst {
                        stat: StatKind::Power,
                        op: CompareOp::Lt,
                        value,
                    },
                ] = flt.stat_bounds.as_slice()
                    && matches!(
                        value.as_ref(),
                        ValueExprAst::ObjectStat {
                            subject: stat_subj,
                            stat: StatKind::Power,
                        } if matches!(stat_subj.as_ref(), SubjectRefAst::SelfObject)
                    )
                    && filter_only_has(
                        flt,
                        FilterAllow {
                            stat_bounds: true,
                            ..allow_base
                        },
                    )
                {
                    return Some(BlockingRestriction::BlockerPowerAtLeastSource);
                }
                if flt.required_keywords == ["flying"]
                    && filter_only_has(
                        flt,
                        FilterAllow {
                            req_keywords: true,
                            ..allow_base
                        },
                    )
                {
                    return Some(BlockingRestriction::CannotBeBlockedByFlying);
                }
                if flt.subtypes.len() == 1
                    && filter_only_has(
                        flt,
                        FilterAllow {
                            subtypes: true,
                            ..allow_base
                        },
                    )
                    && let Some(ct) = vocabulary.resolve_label(&flt.subtypes[0])
                {
                    return Some(BlockingRestriction::CannotBeBlockedBySubtype(ct));
                }
                if flt.stat_bounds.len() == 1
                    && filter_only_has(
                        flt,
                        FilterAllow {
                            stat_bounds: true,
                            ..allow_base
                        },
                    )
                {
                    if flt.stat_bounds[0].stat == StatKind::Power
                        && flt.stat_bounds[0].op == CompareOp::Gt
                        && matches!(
                            flt.stat_bounds[0].value.as_ref(),
                            ValueExprAst::ObjectStat {
                                subject,
                                stat: StatKind::Power,
                            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject | SubjectRefAst::AttachedHost(_))
                        )
                    {
                        return Some(BlockingRestriction::CannotBeBlockedByGreaterPower);
                    }
                    if let ValueExprAst::Const(v) = flt.stat_bounds[0].value.as_ref()
                        && let Ok(u) = u16::try_from(*v)
                    {
                        match (flt.stat_bounds[0].stat, flt.stat_bounds[0].op) {
                            (StatKind::Toughness, CompareOp::Ge) if u > 0 => {
                                return Some(
                                    BlockingRestriction::CannotBeBlockedByToughnessAtLeast(u),
                                );
                            }
                            (StatKind::Power, CompareOp::Le) => {
                                return Some(BlockingRestriction::CannotBeBlockedByPowerAtMost(u));
                            }
                            (StatKind::Power, CompareOp::Lt) if u > 0 => {
                                return Some(BlockingRestriction::CannotBeBlockedByPowerAtMost(
                                    u - 1,
                                ));
                            }
                            (StatKind::Power, CompareOp::Ge) => {
                                return Some(BlockingRestriction::CannotBeBlockedByPowerAtLeast(u));
                            }
                            (StatKind::Power, CompareOp::Gt) => {
                                return Some(BlockingRestriction::CannotBeBlockedByPowerAtLeast(
                                    u.saturating_add(1),
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                let single_color = |mask: AstColorMask| -> Option<Color> {
                    match mask {
                        m if m == AstColorMask::WHITE => Some(Color::White),
                        m if m == AstColorMask::BLUE => Some(Color::Blue),
                        m if m == AstColorMask::BLACK => Some(Color::Black),
                        m if m == AstColorMask::RED => Some(Color::Red),
                        m if m == AstColorMask::GREEN => Some(Color::Green),
                        _ => None,
                    }
                };
                if filter_only_has(
                    flt,
                    FilterAllow {
                        colors: true,
                        ..allow_base
                    },
                ) && let Some(c) = single_color(flt.any_colors)
                {
                    return Some(BlockingRestriction::CannotBeBlockedByColor(c));
                }
                if flt.token == Some(true)
                    && filter_only_has(
                        flt,
                        FilterAllow {
                            token: true,
                            ..allow_base
                        },
                    )
                {
                    return Some(BlockingRestriction::CannotBeBlockedByTokens);
                }
                if flt.forbidden_types == AstCardTypeMask::ARTIFACT
                    && filter_only_has(
                        flt,
                        FilterAllow {
                            forbidden_types: true,
                            ..allow_base
                        },
                    )
                {
                    return Some(BlockingRestriction::CannotBeBlockedByNonArtifact);
                }
            }
        }
        if rule == "CantBeBlockedExceptBy"
            && filter_only_has(
                flt,
                FilterAllow {
                    types: true,
                    subtypes: true,
                    colors: true,
                    req_keywords: true,
                    ..FilterAllow::NONE
                },
            )
            && (flt.any_types.is_empty()
                || flt.any_types == AstCardTypeMask::CREATURE
                || flt.any_types == AstCardTypeMask::ARTIFACT
                || flt.any_types == AstCardTypeMask::ARTIFACT.union(AstCardTypeMask::CREATURE))
        {
            let single_color = |mask: AstColorMask| -> Option<Color> {
                match mask {
                    m if m == AstColorMask::WHITE => Some(Color::White),
                    m if m == AstColorMask::BLUE => Some(Color::Blue),
                    m if m == AstColorMask::BLACK => Some(Color::Black),
                    m if m == AstColorMask::RED => Some(Color::Red),
                    m if m == AstColorMask::GREEN => Some(Color::Green),
                    _ => None,
                }
            };
            let has_artifact = (flt.any_types.0 & AstCardTypeMask::ARTIFACT.0) != 0;
            if flt.any_colors.is_empty() {
                if flt.subtypes.len() == 1
                    && !has_artifact
                    && flt.required_keywords.is_empty()
                    && let Some(ct) = vocabulary.resolve_label(&flt.subtypes[0])
                {
                    return Some(BlockingRestriction::OnlyBlockedBySubtype(ct));
                }
                if flt.subtypes.is_empty()
                    && !has_artifact
                    && ((flt.required_keywords.len() == 2
                        && flt.required_keywords.iter().any(|k| k == "flying")
                        && flt.required_keywords.iter().any(|k| k == "reach"))
                        || flt.required_keywords == ["flying or reach"])
                {
                    return Some(BlockingRestriction::OnlyBlockedByFlyingOrReach);
                }
                if flt.subtypes.is_empty() && flt.required_keywords.is_empty() && has_artifact {
                    return Some(BlockingRestriction::OnlyBlockedByArtifact);
                }
            } else if let Some(c) = single_color(flt.any_colors)
                && flt.subtypes.is_empty()
                && flt.required_keywords.is_empty()
            {
                if has_artifact {
                    return Some(BlockingRestriction::OnlyBlockedByColorOrArtifact(c));
                }
                return Some(BlockingRestriction::OnlyBlockedByColor(c));
            }
        }
        if rule == "CantBlock"
            && flt.subtypes.len() == 1
            && (flt.any_types.is_empty() || flt.any_types == AstCardTypeMask::CREATURE)
            && filter_only_has(
                flt,
                FilterAllow {
                    types: true,
                    subtypes: true,
                    ..FilterAllow::NONE
                },
            )
            && let Some(ct) = vocabulary.resolve_label(&flt.subtypes[0])
        {
            return Some(BlockingRestriction::CannotBlockSubtype(ct));
        }
        if let Ok(obj) = lower_object_filter(flt, vocabulary)
            && object_filter_supported_by_blocking_static_characteristics(&obj)
        {
            match rule {
                "CantBeBlockedBy" => {
                    return Some(BlockingRestriction::CannotBeBlockedByMatching(obj));
                }
                "CantBeBlockedExceptBy" | "CanOnlyBeBlockedBy" => {
                    return Some(BlockingRestriction::OnlyBlockedByMatching(obj));
                }
                "CantBlock" | "cant_block" => {
                    return Some(BlockingRestriction::CannotBlockMatching(obj));
                }
                "AllAbleBlock"
                | "MustBeBlockedByAll"
                | "MustBeBlockedByAllMatching"
                | "must_be_blocked_by" => {
                    return Some(BlockingRestriction::MustBeBlockedByAllMatching(obj));
                }
                _ => {}
            }
        }
        return None;
    }
    lower_blocking_restriction(rule)
        .or_else(|| parse_must_be_blocked_by_all_matching(rule, vocabulary))
        .or_else(|| {
            lower_combat_mod_blocking_restriction(rule, vocabulary)
                .filter(|br| *br != BlockingRestriction::CannotAttackSourceController)
        })
}

fn object_filter_supported_by_blocking_static_characteristics(obj: &ObjectFilter) -> bool {
    match obj {
        ObjectFilter::Clause(c) => clause_supported_by_blocking_static_characteristics(c),
        ObjectFilter::Either(a, b) => {
            clause_supported_by_blocking_static_characteristics(a)
                && clause_supported_by_blocking_static_characteristics(b)
        }
    }
}

fn is_menace_blocking_rule(rule: &str) -> bool {
    let norm = strip_combat_duration(rule);
    matches!(
        norm.as_str(),
        "minblockers:two or more creatures"
            | "can't be blocked except by two or more creatures"
            | "cantbeblockedexceptby:two or more creatures"
            | "canonlybeblockedby:two or more creatures"
    )
}

fn parse_disjunctive_blocking_filter(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    if let Some((lhs_raw, rhs_raw)) = trimmed.split_once(" and/or ") {
        let lhs = lhs_raw.trim();
        let rhs = rhs_raw.trim();
        let rhs_obj = spellwake_oracle_compiler::filter::parse_object_filter(rhs, &compiler_vocab)
            .ok()
            .and_then(|ast_f| lower_object_filter(&ast_f, vocabulary).ok())?;
        let lhs_obj = spellwake_oracle_compiler::filter::parse_object_filter(lhs, &compiler_vocab)
            .ok()
            .and_then(|ast_f| lower_object_filter(&ast_f, vocabulary).ok())
            .or_else(|| {
                if rhs.ends_with(" creatures") || rhs.ends_with(" creature") {
                    let expanded = format!("{lhs} creatures");
                    spellwake_oracle_compiler::filter::parse_object_filter(
                        &expanded,
                        &compiler_vocab,
                    )
                    .ok()
                    .and_then(|ast_f| lower_object_filter(&ast_f, vocabulary).ok())
                } else {
                    None
                }
            })?;
        if let (ObjectFilter::Clause(mut a), ObjectFilter::Clause(b)) = (lhs_obj, rhs_obj) {
            if a.any_types.is_empty() && b.any_types == CardTypeMask::CREATURE {
                a.any_types = CardTypeMask::CREATURE;
            }
            return Some(ObjectFilter::Either(a, b));
        }
        return None;
    }
    let ast_f =
        spellwake_oracle_compiler::filter::parse_object_filter(trimmed, &compiler_vocab).ok()?;
    lower_object_filter(&ast_f, vocabulary).ok()
}

fn parse_cant_block_or_be_blocked_by_filter(
    rule: &str,
    filter: Option<&ObjectFilterAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    if let Some(flt) = filter {
        if matches!(
            rule,
            "CantBlockOrBeBlockedBy" | "cant_block_or_be_blocked_by"
        ) {
            let obj = lower_object_filter(flt, vocabulary).ok()?;
            return object_filter_supported_by_blocking_static_characteristics(&obj).then_some(obj);
        }
        return None;
    }
    let norm = strip_combat_duration(rule);
    let rest = norm
        .strip_prefix("can't block or be blocked by ")
        .or_else(|| norm.strip_prefix("cantblockorbeblockedby:"))?;
    let obj = parse_disjunctive_blocking_filter(rest, vocabulary)?;
    object_filter_supported_by_blocking_static_characteristics(&obj).then_some(obj)
}

fn strip_combat_duration(raw: &str) -> String {
    let mut s = raw
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase()
        .replace(" until your next turn ", " ")
        .replace(" until end of turn ", " ")
        .replace(" this turn ", " ")
        .replace(" this combat ", " ");
    loop {
        let next = s
            .strip_suffix(" until your next turn")
            .or_else(|| s.strip_suffix(" until end of turn"))
            .or_else(|| s.strip_suffix(" this turn"))
            .or_else(|| s.strip_suffix(" this combat"));
        match next {
            Some(trimmed) => s = trimmed.trim().to_string(),
            None => return s.trim().to_string(),
        }
    }
}

fn parse_color_word(word: &str) -> Option<Color> {
    match word.trim() {
        "white" => Some(Color::White),
        "blue" => Some(Color::Blue),
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        _ => None,
    }
}

fn parse_blocking_u16(raw: &str) -> Option<u16> {
    let trimmed = raw.trim();
    if let Ok(v) = trimmed.parse::<u16>() {
        return Some(v);
    }
    let word_val = spellwake_oracle_compiler::vocab::parse_number_word(trimmed)?;
    u16::try_from(word_val).ok()
}

fn parse_block_additional_count(rule: &str) -> Option<u8> {
    let norm = strip_combat_duration(rule);
    if matches!(
        norm.as_str(),
        "can block an additional creature"
            | "can block an additional creature each combat"
            | "additional_block"
            | "block_additional:creature each combat"
            | "block_additional:creature"
    ) {
        return Some(1);
    }
    let rest = norm
        .strip_prefix("can block an additional ")
        .or_else(|| norm.strip_prefix("block_additional:"))?
        .trim();
    let rest = rest
        .strip_prefix("an additional ")
        .or_else(|| rest.strip_prefix("additional "))
        .unwrap_or(rest)
        .trim();
    let rest = rest.strip_suffix(" each combat").unwrap_or(rest).trim();
    if rest == "creature" {
        return Some(1);
    }
    let count_raw = rest
        .strip_suffix(" creatures")
        .or_else(|| rest.strip_suffix(" creature"))
        .unwrap_or(rest)
        .trim();
    let count = count_raw.parse::<u8>().ok().or_else(|| {
        spellwake_oracle_compiler::vocab::parse_number_word(count_raw)
            .and_then(|v| u8::try_from(v).ok())
    })?;
    (count > 0).then_some(count)
}

fn parse_blocking_count_more_than(rest: &str) -> Option<u8> {
    let trimmed = rest.trim();
    let count_raw = trimmed
        .strip_suffix(" creatures each combat")
        .or_else(|| trimmed.strip_suffix(" creature each combat"))
        .or_else(|| trimmed.strip_suffix(" creatures"))
        .or_else(|| trimmed.strip_suffix(" creature"))
        .unwrap_or(trimmed)
        .trim();
    let count = count_raw.parse::<u8>().ok().or_else(|| {
        spellwake_oracle_compiler::vocab::parse_number_word(count_raw)
            .and_then(|v| u8::try_from(v).ok())
    })?;
    (count > 0).then_some(count)
}

fn parse_must_be_blocked_by_all_matching(
    rule: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<BlockingRestriction> {
    let norm = strip_combat_duration(rule);
    if let Some(rest) = norm
        .strip_prefix("mustbeblockedbyall:")
        .or_else(|| norm.strip_prefix("must_be_blocked_by:"))
        .or_else(|| norm.strip_prefix("allableblock:"))
    {
        let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
            rest.trim(),
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        let obj = lower_object_filter(&ast_f, vocabulary).ok()?;
        if !object_filter_supported_by_blocking_static_characteristics(&obj) {
            return None;
        }
        return Some(BlockingRestriction::MustBeBlockedByAllMatching(obj));
    }
    let (who_raw, rest) = norm.split_once(" able to block ")?;
    let subj_raw = rest.trim().strip_suffix(" do so")?.trim();
    if !matches!(
        subj_raw,
        "~" | "this creature"
            | "this permanent"
            | "it"
            | "them"
            | "that creature"
            | "those creatures"
            | "target creature"
    ) {
        return None;
    }
    let flt_raw = who_raw
        .trim()
        .strip_prefix("all ")
        .unwrap_or(who_raw.trim())
        .trim();
    if flt_raw.is_empty() {
        return None;
    }
    let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
        flt_raw,
        &cached_compiler_vocab(vocabulary),
    )
    .ok()?;
    let obj = lower_object_filter(&ast_f, vocabulary).ok()?;
    if !object_filter_supported_by_blocking_static_characteristics(&obj) {
        return None;
    }
    Some(BlockingRestriction::MustBeBlockedByAllMatching(obj))
}

pub(super) fn lower_combat_mod_blocking_restriction(
    rule: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<BlockingRestriction> {
    let stripped_owned = strip_combat_duration(rule);
    let stripped = stripped_owned.as_str();
    if let Some(br) = lower_blocking_restriction(stripped) {
        return Some(br);
    }
    if let Some(br) = parse_must_be_blocked_by_all_matching(stripped, vocabulary) {
        return Some(br);
    }
    if stripped == "must be blocked if able" || stripped == "must be blocked" {
        return Some(BlockingRestriction::MustBeBlocked);
    }
    if stripped == "can attack as though it didn't have defender"
        || stripped == "can attack as though they didn't have defender"
        || stripped == "may attack as though it didn't have defender"
        || stripped == "may attack as though they didn't have defender"
    {
        return Some(BlockingRestriction::CanAttackAsThoughNoDefender);
    }
    if matches!(
        stripped,
        "can't block ~"
            | "can't block this creature"
            | "can't block this permanent"
            | "can't block it"
            | "cantblock:~"
            | "cantblock:this creature"
            | "cantblock:this permanent"
            | "cantblock:it"
    ) {
        return Some(BlockingRestriction::CannotBlockSource);
    }
    if matches!(
        stripped,
        "blocks ~ if able"
            | "blocks this creature if able"
            | "blocks it if able"
            | "must block ~ if able"
            | "must block this creature if able"
            | "must block it if able"
            | "must block ~"
            | "must block this creature"
            | "must block it"
            | "must_block:~"
            | "must_block:this creature"
            | "must_block:this permanent"
            | "must_block:it"
    ) {
        return Some(BlockingRestriction::MustBlockSource);
    }
    if matches!(
        stripped,
        "can't be blocked by ~"
            | "can't be blocked by this creature"
            | "can't be blocked by this permanent"
            | "can't be blocked by it"
            | "cantbeblockedby:~"
            | "cantbeblockedby:this creature"
            | "cantbeblockedby:this permanent"
            | "cantbeblockedby:it"
    ) {
        return Some(BlockingRestriction::CannotBeBlockedBySource);
    }
    // Single-restriction callers cannot represent multi-block counts (> 1) without dropping
    // extra blocks, nor "can't attack or block alone" (which requires two BlockingRestrictions).
    // Multi-part callers should use `lower_combat_mod_blocking_restrictions`.
    if let Some(count) = parse_block_additional_count(stripped) {
        return (count == 1).then_some(BlockingRestriction::AdditionalBlock);
    }
    if stripped == "can't attack or block alone" {
        return None;
    }
    if let Some(rest) = stripped
        .strip_prefix("can't be blocked by ")
        .or_else(|| stripped.strip_prefix("cantbeblockedby:"))
    {
        let rest = rest.trim();
        if let Some(after_more) = rest.strip_prefix("more than ")
            && let Some(n) = parse_blocking_count_more_than(after_more)
        {
            return Some(BlockingRestriction::CannotBeBlockedByMoreThan(n));
        }
        if rest == "creatures with flying" {
            return Some(BlockingRestriction::CannotBeBlockedByFlying);
        }
        if rest == "tokens" || rest == "creature tokens" {
            return Some(BlockingRestriction::CannotBeBlockedByTokens);
        }
        if rest == "nonartifact creatures" {
            return Some(BlockingRestriction::CannotBeBlockedByNonArtifact);
        }
        if rest == "artifact creatures" {
            return Some(BlockingRestriction::CannotBeBlockedByMatching(
                ObjectFilter::Clause(ObjectClause {
                    any_types: CardTypeMask::CREATURE,
                    required_types: CardTypeMask::ARTIFACT,
                    ..ObjectClause::any()
                }),
            ));
        }
        if matches!(
            rest,
            "creatures with power greater than ~'s power"
                | "creatures with power greater than its power"
                | "creatures with power greater than this creature's power"
                | "creatures with greater power"
        ) {
            return Some(BlockingRestriction::CannotBeBlockedByGreaterPower);
        }
        if let Some(n_str) = rest
            .strip_prefix("creatures with power ")
            .and_then(|s| s.strip_suffix(" or less"))
            && let Some(n) = parse_blocking_u16(n_str)
        {
            return Some(BlockingRestriction::CannotBeBlockedByPowerAtMost(n));
        }
        if let Some(n_str) = rest.strip_prefix("creatures with power less than ")
            && let Some(n) = parse_blocking_u16(n_str)
            && n > 0
        {
            return Some(BlockingRestriction::CannotBeBlockedByPowerAtMost(n - 1));
        }
        if let Some(n_str) = rest
            .strip_prefix("creatures with power ")
            .and_then(|s| s.strip_suffix(" or greater"))
            && let Some(n) = parse_blocking_u16(n_str)
        {
            return Some(BlockingRestriction::CannotBeBlockedByPowerAtLeast(n));
        }
        if let Some(n_str) = rest.strip_prefix("creatures with power greater than ")
            && let Some(n) = parse_blocking_u16(n_str)
        {
            return Some(BlockingRestriction::CannotBeBlockedByPowerAtLeast(
                n.saturating_add(1),
            ));
        }
        if let Some(n_str) = rest
            .strip_prefix("creatures with toughness ")
            .and_then(|s| s.strip_suffix(" or greater"))
            && let Some(n) = parse_blocking_u16(n_str)
        {
            return Some(BlockingRestriction::CannotBeBlockedByToughnessAtLeast(n));
        }
        if let Some(color_word) = rest.strip_suffix(" creatures") {
            if let Some(c) = parse_color_word(color_word) {
                return Some(BlockingRestriction::CannotBeBlockedByColor(c));
            }
            if let Some(ct) = resolve_protection_subtype(color_word.trim(), vocabulary) {
                return Some(BlockingRestriction::CannotBeBlockedBySubtype(ct));
            }
        }
        if let Some(ct) = resolve_protection_subtype(rest, vocabulary) {
            return Some(BlockingRestriction::CannotBeBlockedBySubtype(ct));
        }
        if let Some(obj) = parse_disjunctive_blocking_filter(rest, vocabulary) {
            return Some(BlockingRestriction::CannotBeBlockedByMatching(obj));
        }
    }
    if let Some(rest) = stripped
        .strip_prefix("can't be blocked except by ")
        .or_else(|| stripped.strip_prefix("cantbeblockedexceptby:"))
        .or_else(|| stripped.strip_prefix("canonlybeblockedby:"))
    {
        let rest = rest.trim();
        if rest == "three or more creatures" {
            return Some(BlockingRestriction::AtLeastThreeBlockers);
        }
        if rest == "two or more creatures" {
            return None;
        }
        if rest == "creatures with flying or reach"
            || rest == "creatures with flying or creatures with reach"
        {
            return Some(BlockingRestriction::OnlyBlockedByFlyingOrReach);
        }
        if rest == "artifact creatures" {
            return Some(BlockingRestriction::OnlyBlockedByArtifact);
        }
        if let Some(color_word) = rest
            .strip_prefix("artifact creatures and/or ")
            .and_then(|s| s.strip_suffix(" creatures"))
            .or_else(|| {
                rest.strip_prefix("artifact or ")
                    .and_then(|s| s.strip_suffix(" creatures"))
            })
            .or_else(|| rest.strip_suffix(" creatures and/or artifact creatures"))
            .or_else(|| rest.strip_suffix(" or artifact creatures"))
            && let Some(c) = parse_color_word(color_word)
        {
            return Some(BlockingRestriction::OnlyBlockedByColorOrArtifact(c));
        }
        if let Some(color_word) = rest.strip_suffix(" creatures") {
            if let Some(c) = parse_color_word(color_word) {
                return Some(BlockingRestriction::OnlyBlockedByColor(c));
            }
            if let Some(ct) = resolve_protection_subtype(color_word.trim(), vocabulary) {
                return Some(BlockingRestriction::OnlyBlockedBySubtype(ct));
            }
        }
        if let Some(ct) = resolve_protection_subtype(rest, vocabulary) {
            return Some(BlockingRestriction::OnlyBlockedBySubtype(ct));
        }
        if let Some(obj) = parse_disjunctive_blocking_filter(rest, vocabulary) {
            return Some(BlockingRestriction::OnlyBlockedByMatching(obj));
        }
    }
    if let Some(rest) = stripped
        .strip_prefix("can't block ")
        .or_else(|| stripped.strip_prefix("cantblock:"))
    {
        let rest = rest.trim();
        if let Some(after_more) = rest.strip_prefix("more than ")
            && let Some(n) = parse_blocking_count_more_than(after_more)
        {
            return Some(BlockingRestriction::CannotBlockMoreThan(n));
        }
        if let Some(n_str) = rest
            .strip_prefix("creatures with power ")
            .and_then(|s| s.strip_suffix(" or less"))
            && let Some(n) = parse_blocking_u16(n_str)
        {
            return Some(BlockingRestriction::CannotBlockMatching(
                ObjectFilter::Clause(ObjectClause {
                    any_types: CardTypeMask::CREATURE,
                    power: PowerFilter::AtMost(n),
                    ..ObjectClause::any()
                }),
            ));
        }
        if let Some(n_str) = rest.strip_prefix("creatures with power less than ")
            && let Some(n) = parse_blocking_u16(n_str)
            && n > 0
        {
            return Some(BlockingRestriction::CannotBlockMatching(
                ObjectFilter::Clause(ObjectClause {
                    any_types: CardTypeMask::CREATURE,
                    power: PowerFilter::AtMost(n - 1),
                    ..ObjectClause::any()
                }),
            ));
        }
        if let Some(n_str) = rest
            .strip_prefix("creatures with power ")
            .and_then(|s| s.strip_suffix(" or greater"))
            && let Some(n) = parse_blocking_u16(n_str)
            && let Ok(n_i16) = i16::try_from(n)
        {
            return Some(BlockingRestriction::CannotBlockMatching(
                ObjectFilter::Clause(ObjectClause {
                    any_types: CardTypeMask::CREATURE,
                    power: PowerFilter::AtLeast(n_i16),
                    ..ObjectClause::any()
                }),
            ));
        }
        if let Some(n_str) = rest.strip_prefix("creatures with power greater than ")
            && let Some(n) = parse_blocking_u16(n_str)
            && let Ok(n_i16) = i16::try_from(n.saturating_add(1))
        {
            return Some(BlockingRestriction::CannotBlockMatching(
                ObjectFilter::Clause(ObjectClause {
                    any_types: CardTypeMask::CREATURE,
                    power: PowerFilter::AtLeast(n_i16),
                    ..ObjectClause::any()
                }),
            ));
        }
        if let Some(sub_word) = rest.strip_suffix(" creatures")
            && let Some(ct) = resolve_protection_subtype(sub_word.trim(), vocabulary)
        {
            return Some(BlockingRestriction::CannotBlockSubtype(ct));
        }
        if let Some(ct) = resolve_protection_subtype(rest, vocabulary) {
            return Some(BlockingRestriction::CannotBlockSubtype(ct));
        }
        if let Some(obj) = parse_disjunctive_blocking_filter(rest, vocabulary) {
            return Some(BlockingRestriction::CannotBlockMatching(obj));
        }
    }
    if let Some(rest) = stripped
        .strip_prefix("can block only ")
        .or_else(|| stripped.strip_prefix("canblockonly:"))
    {
        let rest = rest.trim();
        if let Some(obj) = parse_disjunctive_blocking_filter(rest, vocabulary) {
            return Some(BlockingRestriction::CanBlockOnlyMatching(obj));
        }
    }
    if matches!(
        stripped,
        "can't attack you"
            | "can't attack you or planeswalkers you control"
            | "can't attack you or a planeswalker you control"
            | "can't attack you or permanents you control"
            | "cantattackyou"
            | "cant_attack:you"
    ) {
        return Some(BlockingRestriction::CannotAttackSourceController);
    }
    None
}

pub(super) fn lower_combat_mod_blocking_restrictions(
    rule: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<BlockingRestriction>> {
    let stripped_owned = strip_combat_duration(rule);
    let stripped = stripped_owned.as_str();
    if stripped == "can't attack or block alone" {
        return Some(vec![
            BlockingRestriction::CannotAttackAlone,
            BlockingRestriction::CannotBlockAlone,
        ]);
    }
    if stripped == "can't block and can't be blocked" {
        return Some(vec![
            BlockingRestriction::CannotBlock,
            BlockingRestriction::CannotBeBlocked,
        ]);
    }
    if matches!(
        stripped,
        "can't attack, block, or crew vehicles"
            | "can't attack, block or crew vehicles"
            | "enchanted creature can't attack, block, or crew vehicles"
    ) {
        return Some(vec![
            BlockingRestriction::CannotAttackOrBlock,
            BlockingRestriction::CannotCrewVehicles,
        ]);
    }
    if let Some(obj) = parse_cant_block_or_be_blocked_by_filter(stripped, None, vocabulary) {
        return Some(vec![
            BlockingRestriction::CannotBlockMatching(obj),
            BlockingRestriction::CannotBeBlockedByMatching(obj),
        ]);
    }
    if let Some(count) = parse_block_additional_count(stripped) {
        return Some(vec![
            BlockingRestriction::AdditionalBlock;
            usize::from(count)
        ]);
    }
    lower_combat_mod_blocking_restriction(rule, vocabulary).map(|br| vec![br])
}

pub(super) fn lower_blocking_restriction(rule: &str) -> Option<BlockingRestriction> {
    let norm_owned = strip_combat_duration(rule);
    let norm = norm_owned.as_str();
    match norm {
        "can't block" | "cant_block" | "cantblock" => Some(BlockingRestriction::CannotBlock),
        "can't attack" | "cant_attack" | "cantattack" => Some(BlockingRestriction::CannotAttack),
        "can't attack or block"
        | "cant_attack:or block"
        | "cant_attack:or block,"
        | "cant_attack_or_block"
        | "cantattackorblock"
        | "cantattackorblockif"
        | "cant_attack_or_block_if" => Some(BlockingRestriction::CannotAttackOrBlock),
        "can't attack alone" | "cant_attack_alone" | "cantattackalone" => {
            Some(BlockingRestriction::CannotAttackAlone)
        }
        "assigns no combat damage" | "assign no combat damage" | "assign_no_combat_damage" => {
            Some(BlockingRestriction::AssignNoCombatDamage)
        }
        "can't be regenerated" | "cannot be regenerated" | "cant_be_regenerated" => {
            Some(BlockingRestriction::CannotBeRegenerated)
        }
        "must_block:it" | "must block it" | "must block ~" => {
            Some(BlockingRestriction::MustBlockSource)
        }
        "must_block:each attacking creature"
        | "must_block:all attacking creatures"
        | "must_block:every attacking creature"
        | "blocks each attacking creature if able"
        | "block each attacking creature if able"
        | "blocks all attacking creatures if able"
        | "block all attacking creatures if able"
        | "must block each attacking creature if able"
        | "must block each attacking creature"
        | "must block all attacking creatures if able"
        | "must block all attacking creatures"
        | "must_block_all_attackers"
        | "mustblockallattackers" => Some(BlockingRestriction::MustBlockAllAttackers),
        "can't block alone" | "cant_block_alone" | "cantblockalone" => {
            Some(BlockingRestriction::CannotBlockAlone)
        }
        "assigns combat damage equal to its toughness rather than its power"
        | "assign combat damage equal to their toughness rather than their power" => {
            Some(BlockingRestriction::AssignDamageByToughness)
        }
        "can block any number of creatures"
        | "can block any number of creatures each combat"
        | "can_block:any number of creatures"
        | "can_block:any number of creatures each combat"
        | "it can block any number of creatures"
        | "it can block any number of creatures each combat"
        | "that creature can block any number of creatures"
        | "that creature can block any number of creatures each combat"
        | "can_block_any_number"
        | "canblockanynumber" => Some(BlockingRestriction::CanBlockAnyNumber),
        "can attack as though they didn't have defender"
        | "can attack as though it didn't have defender"
        | "can_attack_as_though_no_defender"
        | "canattackasthoughnodefender" => Some(BlockingRestriction::CanAttackAsThoughNoDefender),
        "you may have ~ assign its combat damage as though it weren't blocked"
        | "you may have ~ assign his combat damage as though he weren't blocked"
        | "you may have ~ assign her combat damage as though she weren't blocked"
        | "you may have it assign its combat damage as though it weren't blocked"
        | "you may have them assign their combat damage as though they weren't blocked"
        | "assign its combat damage as though it weren't blocked"
        | "assign their combat damage as though they weren't blocked"
        | "its combat damage as though it weren't blocked"
        | "their combat damage as though they weren't blocked"
        | "as though it weren't blocked"
        | "as though they weren't blocked"
        | "assigndamageasthoughunblocked"
        | "assign_combat_damage_unblocked" => {
            Some(BlockingRestriction::AssignDamageAsThoughUnblocked)
        }
        "atmostoneblocker" | "at_most_one_blocker" => Some(BlockingRestriction::AtMostOneBlocker),
        "can't be blocked" | "cant_be_blocked" | "cantbeblocked" | "cantbeblockedif"
        | "unblockable" => Some(BlockingRestriction::CannotBeBlocked),
        "can block only creatures with flying" | "only_block_flying" => {
            Some(BlockingRestriction::CanBlockOnlyFlying)
        }
        "must be blocked if able"
        | "it must be blocked if able"
        | "must_be_blocked"
        | "must_be_blocked_if_able"
        | "mustbeblocked" => Some(BlockingRestriction::MustBeBlocked),
        "allableblock" => Some(BlockingRestriction::AllDefendingCreaturesMustBlock),
        "blocks each combat if able"
        | "block each combat if able"
        | "blockseachcombatifable"
        | "blocks_each_combat_if_able" => Some(BlockingRestriction::MustBlockEachCombatIfAble),
        "minblockers:three or more creatures" => Some(BlockingRestriction::AtLeastThreeBlockers),
        "must be blocked by two or more creatures if able"
        | "must be blocked by two or more creatures"
        | "must_be_blocked_by:two or more creatures if able"
        | "must_be_blocked_by:two or more creatures"
        | "minblockers:two or more creatures" => {
            Some(BlockingRestriction::MustBeBlockedByTwoOrMore)
        }
        "can't attack its owner"
        | "cant_attack_owner"
        | "cantattackowner"
        | "can't attack its owner or planeswalkers its owner controls"
        | "can't attack its owner or a planeswalker its owner controls"
        | "can't attack its owner or permanents its owner controls" => {
            Some(BlockingRestriction::CannotAttackOwner)
        }
        "can't crew vehicles" | "cant_crew_vehicles" | "cantcrewvehicles" => {
            Some(BlockingRestriction::CannotCrewVehicles)
        }
        other => {
            if parse_block_additional_count(other) == Some(1) {
                return Some(BlockingRestriction::AdditionalBlock);
            }
            if let Some(rest) = other
                .strip_prefix("can't be blocked by more than ")
                .or_else(|| other.strip_prefix("cantbeblockedby:more than "))
                .or_else(|| other.strip_prefix("cantbeblockedbymorethan:"))
                .or_else(|| other.strip_prefix("cant_be_blocked_by_more_than:"))
                && let Some(n) = parse_blocking_count_more_than(rest)
            {
                return Some(BlockingRestriction::CannotBeBlockedByMoreThan(n));
            }
            if let Some(rest) = other
                .strip_prefix("can't block more than ")
                .or_else(|| other.strip_prefix("cantblock:more than "))
                .or_else(|| other.strip_prefix("cantblockmorethan:"))
                .or_else(|| other.strip_prefix("cant_block_more_than:"))
                && let Some(n) = parse_blocking_count_more_than(rest)
            {
                return Some(BlockingRestriction::CannotBlockMoreThan(n));
            }
            None
        }
    }
}

pub(super) fn parse_generic_mana_amount(delta: &str) -> Option<u8> {
    let inner = delta.trim().strip_prefix('{')?.strip_suffix('}')?;
    inner.parse::<u8>().ok()
}

pub(super) fn lower_cast_cost_count(f: &ObjectFilterAst) -> Result<CastCostCount, &'static str> {
    if f.zone == Some(ZoneFilter::YourGraveyard)
        && filter_only_has(
            f,
            FilterAllow {
                types: true,
                zone: true,
                ..FilterAllow::NONE
            },
        )
    {
        if f.any_types == AstCardTypeMask::CREATURE {
            return Ok(CastCostCount::OwnGraveyardCreatures);
        }
        if f.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY) {
            return Ok(CastCostCount::OwnGraveyardInstantsOrSorceries);
        }
    }
    if f.zone.is_none() && f.any_types == AstCardTypeMask::CREATURE {
        if f.combat == Some(CombatState::Attacking)
            && filter_only_has(
                f,
                FilterAllow {
                    types: true,
                    combat: true,
                    ..FilterAllow::NONE
                },
            )
        {
            return Ok(CastCostCount::AttackingCreatures);
        }
        if f.controller == Some(ControllerRelation::You)
            && f.counter_kind
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case("+1/+1"))
            && filter_only_has(
                f,
                FilterAllow {
                    types: true,
                    controller: true,
                    counter_kind: true,
                    ..FilterAllow::NONE
                },
            )
        {
            return Ok(CastCostCount::ControlledCreaturesWithPlusOneCounters);
        }
        if filter_only_has(
            f,
            FilterAllow {
                types: true,
                ..FilterAllow::NONE
            },
        ) {
            return Ok(CastCostCount::BattlefieldCreatures);
        }
    }
    Err("oracle:unsupported-cast-cost-count")
}

fn parse_landwalk_keyword_param(param: &str) -> Option<KeywordAbility> {
    match param.trim().trim_end_matches('.').trim() {
        "plains" => Some(KeywordAbility::Plainswalk),
        "island" => Some(KeywordAbility::Islandwalk),
        "swamp" => Some(KeywordAbility::Swampwalk),
        "mountain" => Some(KeywordAbility::Mountainwalk),
        "forest" => Some(KeywordAbility::Forestwalk),
        "desert" => Some(KeywordAbility::Desertwalk),
        "nonbasic land" => Some(KeywordAbility::NonbasicLandwalk),
        "legendary land" => Some(KeywordAbility::LegendaryLandwalk),
        "snow land" => Some(KeywordAbility::SnowLandwalk),
        "snow forest" => Some(KeywordAbility::SnowForestwalk),
        "snow swamp" => Some(KeywordAbility::SnowSwampwalk),
        "artifact land" => Some(KeywordAbility::ArtifactLandwalk),
        _ => None,
    }
}

fn parse_protection_keywords_no_vocab(prot: &str) -> Option<Vec<KeywordAbility>> {
    use spellwake_engine::characteristics::ProtectionFromQuality;
    let clean = prot.trim().trim_end_matches('.').trim();
    if matches!(clean, "each color" | "all colors") {
        return Some(vec![
            KeywordAbility::ProtectionFromWhite,
            KeywordAbility::ProtectionFromBlue,
            KeywordAbility::ProtectionFromBlack,
            KeywordAbility::ProtectionFromRed,
            KeywordAbility::ProtectionFromGreen,
        ]);
    }
    if clean == "everything" {
        return Some(vec![KeywordAbility::ProtectionFrom(
            ProtectionFromQuality::Everything,
        )]);
    }
    let mut out = Vec::new();
    for piece in clean.split(" and ") {
        let word = piece
            .trim()
            .strip_prefix("from ")
            .unwrap_or(piece.trim())
            .trim();
        if matches!(word, "each color" | "all colors") {
            out.extend([
                KeywordAbility::ProtectionFromWhite,
                KeywordAbility::ProtectionFromBlue,
                KeywordAbility::ProtectionFromBlack,
                KeywordAbility::ProtectionFromRed,
                KeywordAbility::ProtectionFromGreen,
            ]);
            continue;
        }
        let ka = match word {
            "white" => KeywordAbility::ProtectionFromWhite,
            "blue" => KeywordAbility::ProtectionFromBlue,
            "black" => KeywordAbility::ProtectionFromBlack,
            "red" => KeywordAbility::ProtectionFromRed,
            "green" => KeywordAbility::ProtectionFromGreen,
            "multicolored" => KeywordAbility::ProtectionFromMulticolored,
            "monocolored" => KeywordAbility::ProtectionFrom(ProtectionFromQuality::Monocolored),
            "colorless" => KeywordAbility::ProtectionFrom(ProtectionFromQuality::Colorless),
            "everything" => KeywordAbility::ProtectionFrom(ProtectionFromQuality::Everything),
            "artifact" | "artifacts" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Artifact))
            }
            "creature" | "creatures" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Creature))
            }
            "enchantment" | "enchantments" => KeywordAbility::ProtectionFrom(
                ProtectionFromQuality::CardType(CardType::Enchantment),
            ),
            "instant" | "instants" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Instant))
            }
            "sorcery" | "sorceries" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Sorcery))
            }
            "land" | "lands" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Land))
            }
            "planeswalker" | "planeswalkers" => KeywordAbility::ProtectionFrom(
                ProtectionFromQuality::CardType(CardType::Planeswalker),
            ),
            "battle" | "battles" => {
                KeywordAbility::ProtectionFrom(ProtectionFromQuality::CardType(CardType::Battle))
            }
            _ => return None,
        };
        out.push(ka);
    }
    (!out.is_empty()).then_some(out)
}

fn parse_hexproof_from_qualities_no_vocab(raw: &str) -> Option<Vec<HexproofQuality>> {
    let clean = raw.trim().trim_end_matches('.').trim();
    if matches!(clean, "each color" | "all colors") {
        return Some(vec![
            HexproofQuality::Color(Color::White),
            HexproofQuality::Color(Color::Blue),
            HexproofQuality::Color(Color::Black),
            HexproofQuality::Color(Color::Red),
            HexproofQuality::Color(Color::Green),
        ]);
    }
    if matches!(
        clean,
        "activated and triggered abilities" | "activated or triggered abilities"
    ) {
        return Some(vec![HexproofQuality::ActivatedOrTriggeredAbility]);
    }
    let mut out = Vec::new();
    for comma_part in clean.split(',') {
        let seg = comma_part.trim();
        if seg.is_empty() {
            return None;
        }
        for and_part in seg.split(" and ") {
            for part in and_part.split(" or ") {
                let mut word = part.trim();
                word = word
                    .strip_prefix("and ")
                    .or_else(|| word.strip_prefix("or "))
                    .unwrap_or(word)
                    .trim();
                word = word.strip_prefix("from ").unwrap_or(word).trim();
                if word.is_empty() {
                    return None;
                }
                let q = match word {
                    "white" => HexproofQuality::Color(Color::White),
                    "blue" => HexproofQuality::Color(Color::Blue),
                    "black" => HexproofQuality::Color(Color::Black),
                    "red" => HexproofQuality::Color(Color::Red),
                    "green" => HexproofQuality::Color(Color::Green),
                    "monocolored" => HexproofQuality::Monocolored,
                    "multicolored" => HexproofQuality::Multicolored,
                    "artifact" | "artifacts" => HexproofQuality::CardType(CardType::Artifact),
                    "creature" | "creatures" => HexproofQuality::CardType(CardType::Creature),
                    "enchantment" | "enchantments" => {
                        HexproofQuality::CardType(CardType::Enchantment)
                    }
                    "instant" | "instants" => HexproofQuality::CardType(CardType::Instant),
                    "sorcery" | "sorceries" => HexproofQuality::CardType(CardType::Sorcery),
                    "land" | "lands" => HexproofQuality::CardType(CardType::Land),
                    "planeswalker" | "planeswalkers" => {
                        HexproofQuality::CardType(CardType::Planeswalker)
                    }
                    "battle" | "battles" => HexproofQuality::CardType(CardType::Battle),
                    _ => return None,
                };
                out.push(q);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn lower_keyword_list_inner(
    kws: &[String],
    vocabulary: Option<&CreatureTypeVocabulary>,
) -> Result<BTreeSet<KeywordAbility>, &'static str> {
    let mut out = BTreeSet::new();
    for k in kws {
        let low_owned = k.trim().to_ascii_lowercase();
        let low = strip_protection_doesnt_remove_suffix(&low_owned);
        if let Some(ka) = lower_keyword_ability_name(low) {
            out.insert(ka);
            continue;
        }
        if let Some(rest) = low.strip_prefix("landwalk ")
            && let Some(ka) = parse_landwalk_keyword_param(rest)
        {
            out.insert(ka);
            continue;
        }
        if let Some(prot) = low.strip_prefix("protection from ") {
            let quals = match vocabulary {
                Some(v) => parse_protection_keywords_extended(prot, v),
                None => parse_protection_keywords_no_vocab(prot),
            }
            .ok_or("oracle:unsupported-granted-keyword")?;
            out.extend(quals);
            continue;
        }
        if let Some(hex) = low.strip_prefix("hexproof from ") {
            let quals = match vocabulary {
                Some(v) => parse_hexproof_from_qualities(hex, v),
                None => parse_hexproof_from_qualities_no_vocab(hex),
            }
            .ok_or("oracle:unsupported-granted-keyword")?;
            out.extend(quals.into_iter().map(KeywordAbility::HexproofFrom));
            continue;
        }
        return Err("oracle:unsupported-granted-keyword");
    }
    Ok(out)
}

pub(super) fn lower_keyword_list(kws: &[String]) -> Result<BTreeSet<KeywordAbility>, &'static str> {
    lower_keyword_list_inner(kws, None)
}

fn lower_keyword_list_with_vocab(
    kws: &[String],
    vocabulary: &CreatureTypeVocabulary,
) -> Result<BTreeSet<KeywordAbility>, &'static str> {
    lower_keyword_list_inner(kws, Some(vocabulary))
}

fn strip_protection_doesnt_remove_suffix(low: &str) -> &str {
    let trimmed = low.trim().trim_end_matches('.');
    for suffix in [
        ". this effect doesn't remove ~",
        ". this effect doesn't remove this aura",
        ". this effect doesn't remove this equipment",
        ". this effect doesn't remove this permanent",
        ". this effect doesn't remove it",
        " (this effect doesn't remove ~)",
        " (this effect doesn't remove this aura)",
        " (this effect doesn't remove this equipment)",
        " (this effect doesn't remove this permanent)",
        " (this effect doesn't remove it)",
    ] {
        if let Some(prefix) = trimmed.strip_suffix(suffix) {
            return prefix.trim().trim_end_matches('.');
        }
    }
    trimmed
}

fn parse_granted_equip_or_crew_part(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<StaticPart> {
    let trimmed = raw.trim().trim_end_matches('.').trim();
    let low = trimmed.to_ascii_lowercase();
    if let Some(rest) = low.strip_prefix("equip ") {
        let cost_raw = rest
            .trim()
            .trim_start_matches(['\u{2014}', '-', ':'])
            .trim()
            .trim_end_matches('.')
            .trim();
        if let Some(mana_cost) = parse_fixed_mana_cost(&cost_raw.to_ascii_uppercase()) {
            return Some(StaticPart::GrantActivated(Box::new(ActivatedAbility {
                id: ActivatedAbilityId(0),
                mana_cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Attach(AttachAbility::Equip)],
                ),
            })));
        }
        // Quality-restricted equip, e.g. "equip Knight {0}" or "equip legendary creature {1}" (CR 702.6a).
        if let Some(brace_pos) = cost_raw.find('{') {
            let quality_raw = cost_raw[..brace_pos]
                .trim()
                .trim_end_matches(['\u{2014}', '-', ':'])
                .trim();
            let mana_raw = cost_raw[brace_pos..].trim();
            if !quality_raw.is_empty()
                && let Some(mana_cost) = parse_fixed_mana_cost(&mana_raw.to_ascii_uppercase())
            {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let mut parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                    quality_raw,
                    &compiler_vocab,
                )
                .ok()?;
                if parsed.zone.is_some()
                    || parsed.owner.is_some()
                    || !parsed.extra_zones.is_empty()
                    || parsed
                        .controller
                        .is_some_and(|c| c != ControllerRelation::You)
                    || parsed.any_spell
                    || parsed.any_card
                    || parsed.is_ability
                    || (!parsed.any_types.is_empty()
                        && parsed.any_types != AstCardTypeMask::CREATURE)
                    || is_unrestricted_filter(&parsed)
                {
                    return None;
                }
                parsed.any_types = AstCardTypeMask::CREATURE;
                parsed.controller = Some(ControllerRelation::You);
                let obj = lower_object_filter(&parsed, vocabulary).ok()?;
                return Some(StaticPart::GrantActivated(Box::new(ActivatedAbility {
                    id: ActivatedAbilityId(0),
                    mana_cost,
                    source_cost: SourceCost::None,
                    body: ExecutableBody::new(
                        BodyTarget::FilteredPermanent(obj),
                        vec![Instruction::Attach(AttachAbility::Equip)],
                    ),
                })));
            }
        }
        return None;
    }
    if let Some(rest) = low.strip_prefix("crew ") {
        let n_raw = rest.trim().trim_end_matches('.').trim();
        let n: u64 = n_raw.parse().ok()?;
        return Some(StaticPart::GrantActivated(Box::new(
            ActivatedAbility::tap_for_power(ActivatedAbilityId(0), PowerAbility::Crew, n),
        )));
    }
    if let Some(rest) = low.strip_prefix("saddle ") {
        let n_raw = rest.trim().trim_end_matches('.').trim();
        let n: u64 = n_raw.parse().ok()?;
        return Some(StaticPart::GrantActivated(Box::new(
            ActivatedAbility::tap_for_power(ActivatedAbilityId(0), PowerAbility::Saddle, n),
        )));
    }
    if let Some(rest) = low.strip_prefix("outlast ") {
        let cost_raw = rest
            .trim()
            .trim_start_matches(['\u{2014}', '-', ':'])
            .trim()
            .trim_end_matches('.')
            .trim();
        let mana_cost = parse_fixed_mana_cost(&cost_raw.to_ascii_uppercase())?;
        return Some(StaticPart::GrantActivatedRestricted {
            ability: Box::new(ActivatedAbility {
                id: ActivatedAbilityId(0),
                mana_cost,
                source_cost: SourceCost::Tap,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![crate::keyword_creatures::oracle_lowering::toolbox_emit::put_counters_on_source(CounterKind::PlusOne, ValueExpr::literal(1))],
                ),
            }),
            restriction: Box::new(ActivationRestriction::Sorcery),
        });
    }
    None
}

fn partition_static_keyword_grants(
    keywords: &[String],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> (Vec<StaticPart>, Vec<String>) {
    let mut special_parts = Vec::new();
    let mut remaining = Vec::new();
    for kw in keywords {
        let low = kw.trim().to_ascii_lowercase();
        let clean = strip_protection_doesnt_remove_suffix(&low);
        if matches!(
            clean,
            "protection from the chosen color" | "protection from the color of your choice"
        ) {
            special_parts.push(StaticPart::GrantProtectionChosenColor);
        } else if clean == "protection from the chosen player" {
            special_parts.push(StaticPart::GrantProtectionChosenPlayer);
        } else if clean == "soulbond" {
            for entrant_only in [false, true] {
                special_parts.push(StaticPart::GrantTrigger(Box::new(TriggeredAbility {
                    id: ids.triggered(),
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
        } else if clean == "vanishing" {
            special_parts.push(StaticPart::GrantTrigger(Box::new(
                CounterKeyword::Vanishing(None).ability(ids.triggered()),
            )));
            special_parts.push(StaticPart::GrantTrigger(Box::new(
                CounterKeyword::VanishingLastCounter.ability(ids.triggered()),
            )));
        } else if let Some(part) = parse_granted_equip_or_crew_part(kw, vocabulary) {
            special_parts.push(part);
        } else if let Some(br) = match clean.trim_end_matches('.') {
            "can't attack" => Some(BlockingRestriction::CannotAttack),
            "can't block" => Some(BlockingRestriction::CannotBlock),
            "can't be blocked" => Some(BlockingRestriction::CannotBeBlocked),
            "can't attack or block" => Some(BlockingRestriction::CannotAttackOrBlock),
            "blocks each combat if able" | "block each combat if able" => {
                Some(BlockingRestriction::MustBlockEachCombatIfAble)
            }
            "attacks each combat if able"
            | "attack each combat if able"
            | "attacks each turn if able"
            | "attack each turn if able" => Some(BlockingRestriction::MustAttackEachCombatIfAble),
            "must be blocked if able" => Some(BlockingRestriction::MustBeBlocked),
            "can block an additional creature each combat" | "can block an additional creature" => {
                Some(BlockingRestriction::AdditionalBlock)
            }
            "can block any number of creatures" => Some(BlockingRestriction::CanBlockAnyNumber),
            "can attack as though it didn't have defender"
            | "can attack as though they didn't have defender" => {
                Some(BlockingRestriction::CanAttackAsThoughNoDefender)
            }
            _ => None,
        } {
            special_parts.push(StaticPart::BlockingRestriction(br));
        } else {
            remaining.push(kw.clone());
        }
    }
    (special_parts, remaining)
}

fn lower_granted_ward_trigger(
    raw_suffix: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<TriggeredAbility> {
    let cost_raw = raw_suffix
        .trim()
        .trim_start_matches(['\u{2014}', '\u{2013}', '-', ':'])
        .trim()
        .trim_end_matches('.')
        .trim();
    if cost_raw.is_empty() {
        return None;
    }
    if let Some(cost) = parse_fixed_mana_cost(&cost_raw.to_ascii_uppercase()) {
        return Some(TriggeredAbility::ward(TriggeredAbilityId(0), cost));
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let atoms =
        spellwake_oracle_compiler::clauses::parse_cost_list(cost_raw, &compiler_vocab).ok()?;
    if atoms.is_empty() {
        return None;
    }
    let mut body = lower_ward_cost_body(&atoms, vocabulary).ok()?;
    body.instructions.push(Instruction::StackOp(Box::new(
        spellwake_engine::toolbox::StackOpSpec::counter_ward_source(),
    )));
    let trigger = TriggeredAbility {
        id: TriggeredAbilityId(0),
        condition: TriggerSpec::new(TriggerEvent::Targeted {
            subject: ObjectFilter::self_object(),
            by_opponent: true,
            spell_only: false,
            instant_or_sorcery_only: false,
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body,
    };
    Some(trigger)
}

fn parse_protection_keywords_extended(
    prot: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<KeywordAbility>> {
    if let Some(quals) = parse_protection_keywords(prot, vocabulary) {
        return Some(quals);
    }
    let clean = prot.trim().trim_end_matches('.');
    if !clean.contains(" and ") && !clean.contains(", ") {
        return None;
    }
    let mut raw_pieces: Vec<&str> = Vec::new();
    for seg in clean.split(", ") {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        if let Some(after_and) = seg.strip_prefix("and ") {
            raw_pieces.push(after_and.trim());
        } else if let Some((lhs, rhs)) = seg.split_once(" and ") {
            if !lhs.trim().is_empty() {
                raw_pieces.push(lhs.trim());
            }
            if !rhs.trim().is_empty() {
                raw_pieces.push(rhs.trim());
            }
        } else {
            raw_pieces.push(seg);
        }
    }
    if raw_pieces.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    for piece in raw_pieces {
        let sub = piece
            .trim()
            .strip_prefix("from ")
            .unwrap_or(piece.trim())
            .trim();
        if matches!(sub, "each color" | "all colors") {
            out.extend([
                KeywordAbility::ProtectionFromWhite,
                KeywordAbility::ProtectionFromBlue,
                KeywordAbility::ProtectionFromBlack,
                KeywordAbility::ProtectionFromRed,
                KeywordAbility::ProtectionFromGreen,
            ]);
        } else {
            out.extend(parse_protection_keywords(sub, vocabulary)?);
        }
    }
    (!out.is_empty()).then_some(out)
}

pub(super) fn lower_ability_change(
    kws: &[String],
    vocabulary: &CreatureTypeVocabulary,
) -> Result<AbilityChange, &'static str> {
    if kws.is_empty() {
        return Err("oracle:empty-grant-keywords");
    }
    let mut change = AbilityChange::default();
    for raw in kws {
        let raw_trimmed = raw.trim().trim_end_matches('.').trim();
        let low_owned = raw_trimmed.to_ascii_lowercase();
        let low = strip_protection_doesnt_remove_suffix(&low_owned);
        if let Some(ka) = lower_keyword_ability_name(low) {
            change.granted_keywords.insert(ka);
            continue;
        }
        if low == "damage can't be prevented" {
            change
                .granted_keywords
                .insert(KeywordAbility::DamageCannotBePrevented);
            continue;
        }
        if low == "start your engines" {
            change
                .granted_keywords
                .insert(KeywordAbility::StartYourEngines);
            continue;
        }
        if low == "jumpstart" {
            change.granted_keywords.insert(KeywordAbility::JumpStart);
            continue;
        }
        if let Some(rest) = low.strip_prefix("landwalk ")
            && let Some(ka) = parse_landwalk_keyword_param(rest)
        {
            change.granted_keywords.insert(ka);
            continue;
        }
        if let Some(hex) = low.strip_prefix("hexproof from ")
            && let Some(quals) = parse_hexproof_from_qualities(hex, vocabulary)
        {
            for q in quals {
                change
                    .granted_keywords
                    .insert(KeywordAbility::HexproofFrom(q));
            }
            continue;
        }
        if let Some(prot) = low.strip_prefix("protection from ") {
            let quals = parse_protection_keywords_extended(prot, vocabulary)
                .ok_or("oracle:unsupported-granted-keyword")?;
            change.granted_keywords.extend(quals);
            continue;
        }
        match low {
            "can't be blocked" | "unblockable" => {
                change.grants_unblockable = true;
            }
            "exalted" => {
                change.granted_exalted = change
                    .granted_exalted
                    .checked_add(1)
                    .ok_or("oracle:exalted-ovf")?;
            }
            "prowess" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Prowess),
            "flanking" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Flanking),
            "battle cry" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::BattleCry),
            "melee" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Melee),
            "dethrone" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Dethrone),
            "training" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Training),
            "ingest" => change
                .granted_combat_keywords
                .push(CombatTriggerKeyword::Ingest),
            "decayed" => {
                change.granted_keywords.insert(KeywordAbility::Decayed);
                change
                    .granted_combat_keywords
                    .push(CombatTriggerKeyword::Decayed);
            }
            "cascade" => {
                change
                    .granted_triggers
                    .push(TriggeredAbility::cascade(TriggeredAbilityId(0)));
            }
            "extort" => {
                change
                    .granted_triggers
                    .push(TriggeredAbility::extort(TriggeredAbilityId(0)));
            }
            "exploit" => {
                change.granted_triggers.push(TriggeredAbility {
                    id: TriggeredAbilityId(0),
                    condition: TriggerSpec::self_enters(),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![
                            Instruction::OptionalEffect {
                                chooser: ResolvingPlayer::Controller,
                            },
                            Instruction::Sacrifice {
                                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                                amount: ValueExpr::literal(1),
                                filter: PermanentTypeFilter {
                                    any_types: BTreeSet::new(),
                                    required_types: BTreeSet::from([CardType::Creature]),
                                    forbidden_types: BTreeSet::new(),
                                    object_filter: None,
                                },
                            },
                        ],
                    ),
                });
            }
            "myriad" => {
                change.granted_triggers.push(TriggeredAbility {
                    id: TriggeredAbilityId(0),
                    condition: TriggerSpec::attacks(
                        ObjectFilter::self_object(),
                        AttackTriggerMode::Individual {
                            capture_defender: true,
                        },
                    ),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![
                            Instruction::MutateObject(Box::new(
                                spellwake_engine::toolbox::MutateObjectSpec::myriad(),
                            )),
                            Instruction::ScheduleDelayedStepTrigger {
                                step: Step::EndCombat,
                                own_step_only: false,
                                body: Box::new(ExecutableBody::new(
                                    BodyTarget::None,
                                    vec![Instruction::ZoneOp(Box::new(
                                        spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                            spellwake_engine::toolbox::ObjectQuery::CreatedTokens,
                                            spellwake_engine::toolbox::ZoneDest::exile(),
                                        ),
                                    ))],
                                )),
                            },
                        ],
                    ),
                });
            }
            "evolve" => {
                change
                    .granted_triggers
                    .push(GrowthKeyword::Evolve.ability(TriggeredAbilityId(0)));
            }
            "mentor" => {
                change
                    .granted_triggers
                    .push(GrowthKeyword::Mentor.ability(TriggeredAbilityId(0)));
            }
            "provoke" => {
                change.granted_triggers.push(TriggeredAbility {
                    id: TriggeredAbilityId(0),
                    condition: TriggerSpec::attacks(
                        ObjectFilter::self_object(),
                        AttackTriggerMode::Individual {
                            capture_defender: true,
                        },
                    ),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: ExecutableBody::new(
                        BodyTarget::FilteredPermanent(ObjectFilter::creature().opposing()),
                        vec![
                            Instruction::OptionalEffect {
                                chooser: ResolvingPlayer::Controller,
                            },
                            Instruction::AffectPermanentTargets {
                                minimum: 1,
                                maximum: 1,
                                action: PermanentTargetAction::Untap,
                            },
                            crate::keyword_creatures::oracle_lowering::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![
                                    ResolvedPartSpec::BlockingRestriction(
                                        BlockingRestriction::MustBlockSource,
                                    ),
                                ]),
                                ResolvedDuration::UntilEndOfTurn,
                            ),
                        ],
                    ),
                });
            }
            "undying" => {
                change
                    .granted_triggers
                    .push(CounterKeyword::Undying.ability(TriggeredAbilityId(0)));
            }
            "persist" => {
                change
                    .granted_triggers
                    .push(CounterKeyword::Persist.ability(TriggeredAbilityId(0)));
            }
            _ => {
                if let Some(n_str) = low.strip_prefix("toxic ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n >= 0
                {
                    change.granted_toxic = Some(match &change.granted_toxic {
                        Some(prev) => prev.plus(&n.into()),
                        None => n.into(),
                    });
                } else if let Some(n_str) = low.strip_prefix("afflict ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Afflict(n.into()));
                } else if let Some(n_str) = low.strip_prefix("bushido ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Bushido(n.into()));
                } else if let Some(n_str) = low.strip_prefix("frenzy ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Frenzy(n.into()));
                } else if let Some(n_str) = low.strip_prefix("mobilize ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Mobilize(n.into()));
                } else if let Some(n_str) = low.strip_prefix("poisonous ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Poisonous(n.into()));
                } else if let Some(n_str) = low.strip_prefix("rampage ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Rampage(n.into()));
                } else if let Some(n_str) = low.strip_prefix("renown ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                {
                    change
                        .granted_combat_keywords
                        .push(CombatTriggerKeyword::Renown(n.into()));
                } else if let Some(n_str) = low.strip_prefix("soulshift ")
                    && let Ok(n) = n_str.trim().parse::<u32>()
                {
                    change
                        .granted_triggers
                        .push(TransferKeyword::Soulshift(n).ability(TriggeredAbilityId(0)));
                } else if let Some(n_str) = low.strip_prefix("annihilator ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n > 0
                {
                    change.granted_triggers.push(TriggeredAbility {
                        id: TriggeredAbilityId(0),
                        condition: TriggerSpec::attacks(
                            ObjectFilter::self_object(),
                            AttackTriggerMode::Individual {
                                capture_defender: true,
                            },
                        ),
                        grouping: TriggerGrouping::EachOccurrence,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::Sacrifice {
                                players: AffectedPlayers::One(ResolvingPlayer::DefendingPlayer),
                                amount: ValueExpr::literal(n),
                                filter: PermanentTypeFilter {
                                    any_types: BTreeSet::new(),
                                    required_types: BTreeSet::new(),
                                    forbidden_types: BTreeSet::new(),
                                    object_filter: None,
                                },
                            }],
                        ),
                    });
                } else if let Some(n_str) = low.strip_prefix("afterlife ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n >= 0
                    && let Ok(tr) = spellwake_engine::program::TokenKeyword::Afterlife.ability(
                        TriggeredAbilityId(0),
                        n.into(),
                        vocabulary,
                    )
                {
                    change.granted_triggers.push(tr);
                } else if let Some(n_str) = low.strip_prefix("fabricate ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n >= 0
                    && let Ok(tr) = spellwake_engine::program::TokenKeyword::Fabricate.ability(
                        TriggeredAbilityId(0),
                        n.into(),
                        vocabulary,
                    )
                {
                    change.granted_triggers.push(tr);
                } else if let Some(n_str) = low.strip_prefix("firebending ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n > 0
                {
                    change.granted_triggers.push(TriggeredAbility {
                        id: TriggeredAbilityId(0),
                        condition: TriggerSpec::attacks(
                            ObjectFilter::self_object(),
                            AttackTriggerMode::Individual {
                                capture_defender: false,
                            },
                        ),
                        grouping: TriggerGrouping::EachOccurrence,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::MutatePlayer(Box::new(
                                spellwake_engine::toolbox::MutatePlayerSpec::add_mana(
                                    ResolvingPlayer::Controller,
                                    EffectManaYield::RetainedUntilEndOfCombat {
                                        kind: ManaType::Red,
                                        amount: ValueExpr::literal(n),
                                    },
                                ),
                            ))],
                        ),
                    });
                } else if raw_trimmed
                    .get(..4)
                    .is_some_and(|pref| pref.eq_ignore_ascii_case("ward"))
                    && raw_trimmed[4..].starts_with([' ', '\u{2014}', '\u{2013}', '-', ':'])
                    && let Some(tr) = lower_granted_ward_trigger(&raw_trimmed[4..], vocabulary)
                {
                    change.granted_triggers.push(tr);
                } else {
                    return Err("oracle:unsupported-granted-keyword");
                }
            }
        }
    }
    let base_id = u64::from(change.granted_exalted)
        .saturating_add(u64::try_from(change.granted_combat_keywords.len()).unwrap_or(0));
    for (idx, tr) in change.granted_triggers.iter_mut().enumerate() {
        let offset = u64::try_from(idx).unwrap_or(u64::MAX);
        tr.id = TriggeredAbilityId(base_id.saturating_add(offset));
    }
    Ok(change)
}

fn lower_ability_change_with_ids(
    kws: &[String],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<AbilityChange, &'static str> {
    let mut change = lower_ability_change(kws, vocabulary)?;
    let reserved = u64::from(change.granted_exalted)
        .saturating_add(u64::try_from(change.granted_combat_keywords.len()).unwrap_or(0));
    for tr in &mut change.granted_triggers {
        let mut id = ids.triggered();
        while id.0 < reserved {
            id = ids.triggered();
        }
        tr.id = id;
    }
    Ok(change)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PtDeltaTerm {
    Literal(i64),
    ScaledX(i64),
}

fn parse_pt_delta_term(raw: &str) -> Option<PtDeltaTerm> {
    let norm = raw.trim().replace('\u{2212}', "-");
    let low = norm.to_ascii_lowercase();
    if let Some(coeff_raw) = low.strip_suffix('x') {
        return match coeff_raw {
            "" | "+" => Some(PtDeltaTerm::ScaledX(1)),
            "-" => Some(PtDeltaTerm::ScaledX(-1)),
            _ => {
                let (sign, digits) = if let Some(d) = coeff_raw.strip_prefix('+') {
                    (1i64, d)
                } else if let Some(d) = coeff_raw.strip_prefix('-') {
                    (-1i64, d)
                } else {
                    (1i64, coeff_raw)
                };
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let mag = digits.parse::<i64>().ok()?;
                Some(PtDeltaTerm::ScaledX(sign.checked_mul(mag)?))
            }
        };
    }
    parse_signed_pt_delta(&norm).map(PtDeltaTerm::Literal)
}

pub(super) fn parse_signed_pt_delta(raw: &str) -> Option<i64> {
    let t = raw.trim().replace('\u{2212}', "-");
    let t = t.strip_prefix('+').unwrap_or(&t);
    t.parse::<i64>().ok()
}

fn lower_trigger_multiplier_event(event_desc: &str) -> TriggerMultiplierEvent {
    let ev_low = event_desc.trim().to_ascii_lowercase();
    let obj_filter = if (ev_low.contains("legendary permanent") || ev_low.contains("legendary"))
        && ev_low.contains("artifact")
        && ev_low.contains(" or ")
    {
        ObjectFilter::Either(
            ObjectClause::permanent().legendary(),
            ObjectClause::artifact(),
        )
    } else if ev_low.contains("artifact or creature") || ev_low.contains("creature or artifact") {
        ObjectFilter::Either(ObjectClause::artifact(), ObjectClause::creature())
    } else if ev_low.contains("creature or enchantment")
        || ev_low.contains("enchantment or creature")
    {
        ObjectFilter::Either(ObjectClause::creature(), ObjectClause::enchantment())
    } else if ev_low.contains("artifact or enchantment")
        || ev_low.contains("enchantment or artifact")
    {
        ObjectFilter::Either(ObjectClause::artifact(), ObjectClause::enchantment())
    } else if ev_low.contains("creature") {
        ObjectFilter::creature()
    } else if ev_low.contains("artifact") {
        ObjectFilter::artifact()
    } else if ev_low.contains("enchantment") {
        ObjectFilter::enchantment()
    } else if ev_low.contains("land") && !ev_low.contains("nonland") {
        ObjectFilter::land()
    } else if ev_low.contains("nonland permanent") {
        ObjectFilter::Clause(ObjectClause::permanent().nonland())
    } else if ev_low.contains("legendary permanent") {
        ObjectFilter::Clause(ObjectClause::permanent().legendary())
    } else {
        ObjectFilter::permanent()
    };
    let has_enters = ev_low.contains("enters") || ev_low.contains("entering");
    let has_leaves = ev_low.contains("leaves the battlefield")
        || ev_low.contains("entering or leaving")
        || ev_low.contains("enters or leaves")
        || ev_low.contains("leaving");
    let has_dies = ev_low.contains("dies") || ev_low.contains("dying");
    if has_enters && has_leaves {
        TriggerMultiplierEvent::AnyOf(vec![
            TriggerMultiplierEvent::Enters(obj_filter),
            TriggerMultiplierEvent::LeavesBattlefield(obj_filter),
        ])
    } else if has_enters && has_dies {
        TriggerMultiplierEvent::AnyOf(vec![
            TriggerMultiplierEvent::Enters(obj_filter),
            TriggerMultiplierEvent::Dies(obj_filter),
        ])
    } else if has_enters {
        TriggerMultiplierEvent::Enters(obj_filter)
    } else if has_dies {
        TriggerMultiplierEvent::Dies(obj_filter)
    } else if has_leaves {
        TriggerMultiplierEvent::LeavesBattlefield(obj_filter)
    } else if ev_low.contains("attacks") || ev_low.contains("attacking") {
        TriggerMultiplierEvent::Attacks(obj_filter)
    } else if ev_low.contains("cast") || ev_low.contains("copy") {
        let sf = if ev_low.contains("instant or sorcery") {
            SpellFilter::instant_or_sorcery()
        } else {
            SpellFilter::any()
        };
        TriggerMultiplierEvent::SpellCastOrCopy(sf)
    } else if ev_low.contains("deals damage")
        || ev_low.contains("dealing damage")
        || ev_low.contains("combat damage")
    {
        TriggerMultiplierEvent::DealsDamage {
            combat_only: ev_low.contains("combat"),
            to_player_only: ev_low.contains("to a player"),
        }
    } else {
        TriggerMultiplierEvent::Any
    }
}

fn parse_activated_cost_anaphora(
    raw_low: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<(ActivatedCostScope, PlayerRelation)> {
    let keyword_scope = |prefix: &str| match prefix.trim() {
        "equip" => Some(ActivatedCostScope::Equip),
        "boast" => Some(ActivatedCostScope::Boast),
        "cycling" => Some(ActivatedCostScope::Cycling),
        "ninjutsu" => Some(ActivatedCostScope::Ninjutsu),
        "unearth" => Some(ActivatedCostScope::Unearth),
        "encore" => Some(ActivatedCostScope::Encore),
        "exhaust" => Some(ActivatedCostScope::Exhaust),
        "power-up" => Some(ActivatedCostScope::PowerUp),
        _ => None,
    };
    if let Some(prefix) = raw_low
        .strip_suffix(" abilities you activate")
        .or_else(|| raw_low.strip_suffix(" costs you pay"))
        && let Some(scope) = keyword_scope(prefix)
    {
        return Some((scope, PlayerRelation::Controller));
    }
    if let Some(prefix) = raw_low
        .strip_suffix(" abilities your opponents activate")
        .or_else(|| raw_low.strip_suffix(" abilities opponents activate"))
        .or_else(|| raw_low.strip_suffix(" costs your opponents pay"))
        .or_else(|| raw_low.strip_suffix(" costs opponents pay"))
        && let Some(scope) = keyword_scope(prefix)
    {
        return Some((scope, PlayerRelation::Opponent));
    }
    if let Some(prefix) = raw_low
        .strip_suffix(" abilities players activate")
        .or_else(|| raw_low.strip_suffix(" abilities"))
        .or_else(|| raw_low.strip_suffix(" costs players pay"))
        .or_else(|| raw_low.strip_suffix(" costs"))
        && let Some(scope) = keyword_scope(prefix)
    {
        return Some((scope, PlayerRelation::AnyParticipant));
    }
    let exact = match raw_low {
        "activated abilities of creatures you control" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::Clause(
                ObjectClause::creature().controlled(),
            ))),
            PlayerRelation::Controller,
        )),
        "activated abilities of permanents you control" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::Clause(
                ObjectClause::permanent().controlled(),
            ))),
            PlayerRelation::Controller,
        )),
        "activated abilities of artifacts you control" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::Clause(
                ObjectClause::artifact().controlled(),
            ))),
            PlayerRelation::Controller,
        )),
        "activated abilities of creatures" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::creature())),
            PlayerRelation::AnyParticipant,
        )),
        "activated abilities of permanents" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::permanent())),
            PlayerRelation::AnyParticipant,
        )),
        "activated abilities of enchanted creature"
        | "activated abilities of equipped creature" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::Clause(
                ObjectClause::creature().attached_to_self(),
            ))),
            PlayerRelation::AnyParticipant,
        )),
        "activated abilities of enchanted permanent"
        | "activated abilities of equipped permanent"
        | "enchanted permanent's activated abilities"
        | "equipped permanent's activated abilities" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::Clause(
                ObjectClause::permanent().attached_to_self(),
            ))),
            PlayerRelation::AnyParticipant,
        )),
        "enchanted artifact's activated abilities"
        | "activated abilities of enchanted artifact" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::Clause(
                ObjectClause::artifact().attached_to_self(),
            ))),
            PlayerRelation::AnyParticipant,
        )),
        "enchanted creature's activated abilities" | "equipped creature's activated abilities" => {
            Some((
                ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::Clause(
                    ObjectClause::creature().attached_to_self(),
                ))),
                PlayerRelation::AnyParticipant,
            ))
        }
        "~'s activated abilities" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::self_object())),
            PlayerRelation::AnyParticipant,
        )),
        "activated abilities of ~"
        | "activated abilities of this creature"
        | "activated abilities of this permanent"
        | "activated abilities of this artifact"
        | "activated abilities of this enchantment" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::self_object())),
            PlayerRelation::AnyParticipant,
        )),
        "activated abilities of sources with the chosen name"
        | "activated abilities of sources with the chosen name unless they're mana abilities" => {
            Some((
                ActivatedCostScope::PermanentFilter(Box::new(
                    ObjectFilter::any().with_flags(ObjectFilterFlags::MATCHES_CHOSEN_NAME),
                )),
                PlayerRelation::AnyParticipant,
            ))
        }
        "activated abilities of artifacts" => Some((
            ActivatedCostScope::PermanentFilter(Box::new(ObjectFilter::artifact())),
            PlayerRelation::AnyParticipant,
        )),
        "activated abilities you activate"
        | "abilities you activate"
        | "abilities you activate that aren't mana abilities"
        | "activated abilities you activate that aren't mana abilities" => {
            Some((ActivatedCostScope::Any, PlayerRelation::Controller))
        }
        "activated abilities your opponents activate"
        | "activated abilities opponents activate"
        | "abilities your opponents activate"
        | "abilities opponents activate"
        | "abilities your opponents activate that aren't mana abilities"
        | "activated abilities your opponents activate that aren't mana abilities" => {
            Some((ActivatedCostScope::Any, PlayerRelation::Opponent))
        }
        "activated abilities" | "activated abilities that aren't mana abilities" => {
            Some((ActivatedCostScope::Any, PlayerRelation::AnyParticipant))
        }
        "loyalty abilities of planeswalkers you control" | "loyalty abilities you activate" => {
            Some((ActivatedCostScope::Loyalty, PlayerRelation::Controller))
        }
        "loyalty abilities of planeswalkers your opponents control"
        | "loyalty abilities of planeswalkers opponents control"
        | "loyalty abilities your opponents activate"
        | "loyalty abilities opponents activate" => {
            Some((ActivatedCostScope::Loyalty, PlayerRelation::Opponent))
        }
        "loyalty abilities"
        | "loyalty abilities of planeswalkers"
        | "planeswalkers' loyalty abilities"
        | "loyalty abilities players activate" => {
            Some((ActivatedCostScope::Loyalty, PlayerRelation::AnyParticipant))
        }
        _ => None,
    };
    if exact.is_some() {
        return exact;
    }
    if let Some((actor_part, target_part)) = raw_low
        .split_once(" that target ")
        .or_else(|| raw_low.split_once(" that targets "))
    {
        let parse_target_obj = |raw_tgt: &str| -> Option<ObjectFilter> {
            let target_clean = raw_tgt.trim().trim_end_matches('.').trim();
            match target_clean {
                "~" | "this creature" | "this permanent" | "this artifact" | "this enchantment"
                | "this planeswalker" => Some(ObjectFilter::self_object()),
                "enchanted creature" | "equipped creature" => Some(ObjectFilter::Clause(
                    ObjectClause::creature().attached_to_self(),
                )),
                "enchanted permanent" | "equipped permanent" => Some(ObjectFilter::Clause(
                    ObjectClause::permanent().attached_to_self(),
                )),
                other
                    if !other.is_empty()
                        && !other.contains("card")
                        && !other.contains("graveyard")
                        && !other.contains("hand")
                        && !other.contains("exile")
                        && !other.contains("spell")
                        && !other.contains("player")
                        && !other.starts_with("you")
                        && !other.contains(" or you")
                        && !other.contains("you or ") =>
                {
                    let compiler_vocab = cached_compiler_vocab(vocabulary);
                    spellwake_oracle_compiler::filter::parse_object_filter(other, &compiler_vocab)
                        .ok()
                        .filter(|parsed| {
                            parsed.zone.is_none()
                                && parsed.owner.is_none()
                                && !parsed.any_card
                                && !parsed.any_spell
                                && (!parsed.any_types.is_empty()
                                    || !parsed.subtypes.is_empty()
                                    || parsed.any_permanent
                                    || parsed.token.is_some())
                        })
                        .and_then(|parsed| lower_object_filter(&parsed, vocabulary).ok())
                }
                _ => None,
            }
        };
        let actor_trim = actor_part.trim();
        let equip_controller_opt = match actor_trim {
            "equip abilities you activate" => Some(PlayerRelation::Controller),
            "equip abilities your opponents activate" | "equip abilities opponents activate" => {
                Some(PlayerRelation::Opponent)
            }
            "equip abilities" | "equip abilities players activate" => {
                Some(PlayerRelation::AnyParticipant)
            }
            _ => None,
        };
        if let Some(controller) = equip_controller_opt
            && let Some(obj) = parse_target_obj(target_part)
        {
            return Some((
                ActivatedCostScope::EquipTargeting(Box::new(obj)),
                controller,
            ));
        }
        let controller_opt = match actor_trim {
            "abilities you activate" | "activated abilities you activate" => {
                Some(PlayerRelation::Controller)
            }
            "abilities your opponents activate"
            | "abilities opponents activate"
            | "activated abilities your opponents activate"
            | "activated abilities opponents activate" => Some(PlayerRelation::Opponent),
            "abilities"
            | "activated abilities"
            | "abilities players activate"
            | "activated abilities players activate" => Some(PlayerRelation::AnyParticipant),
            _ => None,
        };
        if let Some(controller) = controller_opt
            && let Some(obj) = parse_target_obj(target_part)
        {
            return Some((ActivatedCostScope::Targeting(Box::new(obj)), controller));
        }
        if let Some(src_raw) = actor_trim.strip_prefix("activated abilities of ") {
            let src_clean = src_raw.trim();
            if !src_clean.is_empty()
                && !src_clean.contains("card")
                && !src_clean.contains("graveyard")
                && !src_clean.contains("hand")
                && !src_clean.contains("exile")
                && !src_clean.contains("source")
                && !src_clean.contains("unless")
                && !src_clean.contains("first")
                && let Some(tgt_obj) = parse_target_obj(target_part)
            {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                if let Ok(parsed) = spellwake_oracle_compiler::filter::parse_object_filter(
                    src_clean,
                    &compiler_vocab,
                ) && parsed.zone.is_none()
                    && parsed.owner.is_none()
                    && !parsed.any_card
                    && !parsed.any_spell
                    && (!parsed.any_types.is_empty()
                        || !parsed.subtypes.is_empty()
                        || parsed.any_permanent
                        || parsed.token.is_some())
                    && let Ok(src_obj) = lower_object_filter(&parsed, vocabulary)
                {
                    let controller = match parsed.controller {
                        Some(ControllerRelation::You) => PlayerRelation::Controller,
                        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                            PlayerRelation::Opponent
                        }
                        None => PlayerRelation::AnyParticipant,
                        _ => return None,
                    };
                    return Some((
                        ActivatedCostScope::PermanentAndTargeting {
                            source: Box::new(src_obj),
                            target: Box::new(tgt_obj),
                        },
                        controller,
                    ));
                }
            }
        }
    }
    if let Some(rest) = raw_low.strip_prefix("activated abilities of ") {
        let rest_trim = rest.trim();
        if let Some(card_part) = rest_trim.strip_suffix(" in your graveyard") {
            let card_clean = card_part.trim();
            let obj_opt = if matches!(card_clean, "cards" | "card") {
                Some(ObjectFilter::any())
            } else if !card_clean.is_empty()
                && !card_clean.contains("source")
                && !card_clean.contains("target")
                && !card_clean.contains("unless")
                && !card_clean.contains("first")
            {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                spellwake_oracle_compiler::filter::parse_object_filter(card_clean, &compiler_vocab)
                    .ok()
                    .filter(|parsed| {
                        parsed.zone.is_none()
                            && parsed.controller.is_none()
                            && parsed.owner.is_none()
                            && !parsed.any_spell
                    })
                    .and_then(|parsed| {
                        if is_unrestricted_filter(&parsed) {
                            Some(ObjectFilter::any())
                        } else {
                            lower_object_filter(&parsed, vocabulary).ok()
                        }
                    })
            } else {
                None
            };
            if let Some(obj) = obj_opt {
                return Some((
                    ActivatedCostScope::GraveyardCardFilter(Box::new(obj)),
                    PlayerRelation::Controller,
                ));
            }
        }
        let rest = rest.trim();
        if !rest.is_empty()
            && !rest.contains("card")
            && !rest.contains("graveyard")
            && !rest.contains("hand")
            && !rest.contains("exile")
            && !rest.contains("source")
            && !rest.contains("target")
            && !rest.contains("unless")
            && !rest.contains("first")
        {
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            if let Ok(parsed) =
                spellwake_oracle_compiler::filter::parse_object_filter(rest, &compiler_vocab)
                && parsed.zone.is_none()
                && parsed.owner.is_none()
                && !parsed.any_card
                && !parsed.any_spell
                && (!parsed.any_types.is_empty()
                    || !parsed.subtypes.is_empty()
                    || parsed.any_permanent
                    || parsed.token.is_some())
                && let Ok(obj) = lower_object_filter(&parsed, vocabulary)
            {
                let controller = match parsed.controller {
                    Some(ControllerRelation::You) => PlayerRelation::Controller,
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        PlayerRelation::Opponent
                    }
                    None => PlayerRelation::AnyParticipant,
                    _ => return None,
                };
                return Some((
                    ActivatedCostScope::PermanentFilter(Box::new(obj)),
                    controller,
                ));
            }
        }
    }
    None
}

fn parse_keyword_cast_cost_anaphora(
    raw_low: &str,
) -> Option<(CastKeywordCostKind, PlayerRelation)> {
    let (kw_part, controller) = if let Some(prefix) = raw_low.strip_suffix(" costs you pay") {
        (prefix.trim(), PlayerRelation::Controller)
    } else if let Some(prefix) = raw_low
        .strip_suffix(" costs your opponents pay")
        .or_else(|| raw_low.strip_suffix(" costs opponents pay"))
    {
        (prefix.trim(), PlayerRelation::Opponent)
    } else {
        let prefix = raw_low
            .strip_suffix(" costs players pay")
            .or_else(|| raw_low.strip_suffix(" costs"))?;
        (prefix.trim(), PlayerRelation::AnyParticipant)
    };
    let keyword = match kw_part {
        "dash" => CastKeywordCostKind::Dash,
        "blitz" => CastKeywordCostKind::Blitz,
        "warp" => CastKeywordCostKind::Warp,
        "flashback" => CastKeywordCostKind::Flashback,
        "buyback" => CastKeywordCostKind::Buyback,
        "bestow" => CastKeywordCostKind::Bestow,
        "evoke" => CastKeywordCostKind::Evoke,
        "mutate" => CastKeywordCostKind::Mutate,
        "escape" => CastKeywordCostKind::Escape,
        "foretell" => CastKeywordCostKind::Foretell,
        "madness" => CastKeywordCostKind::Madness,
        "kicker" => CastKeywordCostKind::Kicker,
        "spectacle" => CastKeywordCostKind::Spectacle,
        "prowl" => CastKeywordCostKind::Prowl,
        "surge" => CastKeywordCostKind::Surge,
        _ => return None,
    };
    Some((keyword, controller))
}

struct AnaphoraCostModifier<'a> {
    raw: &'a str,
    delta: &'a str,
    less: bool,
    per: Option<&'a ObjectFilterAst>,
    scale_expr: Option<&'a ValueExprAst>,
    condition: Option<&'a ConditionAst>,
    min_mana_floor: Option<u8>,
}

fn lower_anaphora_cost_modifier(
    spec: AnaphoraCostModifier<'_>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    let AnaphoraCostModifier {
        raw,
        delta,
        less,
        per,
        scale_expr,
        condition,
        min_mana_floor,
    } = spec;
    let raw_low = raw.trim().to_ascii_lowercase();
    let expr_ctx = StaticExprContext::for_source(program);

    // 1. Self-ability cost reduction attached to a preceding keyword activated ability
    //    (for example Ghostfire Blade's target-filtered equip discount or Plate Armor's scaled equip discount).
    if raw_low == "this ability" || raw_low == "~'s equip ability" {
        if !less || min_mana_floor.is_some() || per.is_some() {
            return Err("oracle:unsupported-ability-cost-modifier");
        }
        let ability_id = if raw_low == "this ability" {
            program.activated.last().map(|ab| ab.id)
        } else {
            let mut equip_ids = program.activated.iter().filter_map(|ab| {
                ab.body
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::Attach(AttachAbility::Equip)))
                    .then_some(ab.id)
            });
            match (equip_ids.next(), equip_ids.next()) {
                (Some(id), None) => Some(id),
                _ => None,
            }
        }
        .ok_or("oracle:orphan-ability-cost-modifier")?;
        let delta_n = if scale_expr.is_some() && delta.trim().eq_ignore_ascii_case("{x}") {
            1
        } else {
            parse_generic_mana_amount(delta)
                .filter(|&n| n > 0)
                .ok_or("oracle:unsupported-ability-cost-delta")?
        };
        if let (None, Some(ConditionAst::TargetsFilter(flt))) = (scale_expr, condition) {
            let obj_flt = lower_object_filter(flt, vocabulary)?;
            if program
                .activation_target_cost_reductions
                .contains_key(&ability_id)
            {
                return Err("oracle:duplicate-ability-target-cost-reduction");
            }
            program
                .activation_target_cost_reductions
                .insert(ability_id, (obj_flt, delta_n));
            return Ok(());
        }
        let reduction_expr = match (scale_expr, condition) {
            (Some(se), None) => {
                let base = lower_static_value_expr(se, expr_ctx, vocabulary)?;
                let scaled = if delta_n == 1 {
                    base
                } else {
                    scale_value_expr(base, i64::from(delta_n))
                };
                if !scaled.is_world_evaluable() {
                    return Err("oracle:unsupported-ability-cost-scale");
                }
                scaled
            }
            (None, Some(cond)) => {
                let mut tmp_builder = BodyBuilder::new();
                let rc =
                    lower_resolution_condition(cond, &mut tmp_builder, vocabulary, false, None)?;
                if tmp_builder.target != BodyTarget::None || !rc.is_world_evaluable() {
                    return Err("oracle:unsupported-ability-cost-cond");
                }
                let cond_expr = ValueExpr::conditional(
                    rc.left,
                    rc.comparison,
                    rc.right,
                    ValueExpr::literal(i64::from(delta_n)),
                    ValueExpr::literal(0_i64),
                );
                if !cond_expr.is_world_evaluable() {
                    return Err("oracle:unsupported-ability-cost-cond");
                }
                cond_expr
            }
            _ => return Err("oracle:unsupported-ability-cost-modifier"),
        };
        let combined = match program.activation_cost_reductions.remove(&ability_id) {
            Some(prev) => ValueExpr::Add(Box::new(prev), Box::new(reduction_expr)),
            None => reduction_expr,
        };
        program
            .activation_cost_reductions
            .insert(ability_id, combined);
        return Ok(());
    }

    // 2. Activated ability cost modifiers (Equip, Boast, Cycling, Ninjutsu, Unearth, Encore, and general scopes).
    //    CR 118.7a floor sentences are either captured in `CostModifierWithFloor` (`min_mana_floor`)
    //    or split out as an unhandled `CostReductionRider:...` static that fails closed.
    if per.is_none()
        && let Some((scope, controller)) = parse_activated_cost_anaphora(&raw_low, vocabulary)
    {
        let (delta_n, scale) = if let Some(se) = scale_expr {
            let base_delta = if delta.trim().eq_ignore_ascii_case("{x}") {
                1
            } else {
                parse_generic_mana_amount(delta)
                    .filter(|&n| n > 0)
                    .ok_or("oracle:non-generic-activated-cost")?
            };
            let ve = lower_static_value_expr(se, expr_ctx, vocabulary)?;
            if !ve.is_world_evaluable() {
                return Err("oracle:unsupported-activated-cost-scale");
            }
            (base_delta, Some(Box::new(ve)))
        } else {
            let base_delta = parse_generic_mana_amount(delta)
                .filter(|&n| n > 0)
                .ok_or("oracle:non-generic-activated-cost")?;
            (base_delta, None)
        };
        return push_conditional_static(
            program,
            StaticContinuousAbility::Modifier {
                id: ids.static_id(),
                rule: StaticModifierRule::ActivatedAbilityCost {
                    scope,
                    controller,
                    delta: delta_n,
                    less,
                    scale,
                    minimum_total_mana: min_mana_floor.unwrap_or(0),
                },
            },
            condition,
            vocabulary,
        );
    }

    if min_mana_floor.is_some() {
        return Err("oracle:unsupported-cost-modifier");
    }

    // 3. Keyword cast cost modifiers (Dash, Blitz, Warp, Flashback, Buyback, Bestow, Evoke,
    //    Mutate, Escape, Foretell, Madness, Kicker, Spectacle, Prowl, Surge).
    if per.is_none()
        && let Some((keyword, controller)) = parse_keyword_cast_cost_anaphora(&raw_low)
    {
        if scale_expr.is_some() {
            return Err("oracle:unsupported-keyword-cast-cost-scale");
        }
        let delta_n = parse_generic_mana_amount(delta)
            .filter(|&n| n > 0)
            .ok_or("oracle:non-generic-keyword-cast-cost")?;
        return push_conditional_static(
            program,
            StaticContinuousAbility::CostModifier {
                id: ids.static_id(),
                modifier: SpellCostModifier::KeywordCastCost {
                    keyword,
                    controller,
                    delta: delta_n,
                    less,
                },
            },
            condition,
            vocabulary,
        );
    }

    // 4. Scoped spell anaphora subjects emitted by clauses.rs:24290-24324.
    if less && per.is_none() && scale_expr.is_none() && raw_low.starts_with("the first ") {
        return lower_first_spell_discount_modifier(
            &SubjectRefAst::Anaphora(raw.to_string()),
            delta,
            condition,
            program,
            vocabulary,
            ids,
        );
    }
    let turn_scoped = if let Some(sp) = raw_low
        .strip_suffix(" your opponents cast during your turn")
        .or_else(|| raw_low.strip_suffix(" your opponents cast on your turn"))
        .or_else(|| raw_low.strip_suffix(" opponents cast during your turn"))
        .or_else(|| raw_low.strip_suffix(" opponents cast on your turn"))
    {
        Some((sp.trim(), false, true, ConditionAst::IsYourTurn))
    } else if let Some(sp) = raw_low
        .strip_suffix(" you cast during your turn")
        .or_else(|| raw_low.strip_suffix(" you cast on your turn"))
    {
        Some((sp.trim(), false, false, ConditionAst::IsYourTurn))
    } else if let Some(sp) = raw_low
        .strip_suffix(" you cast during opponents' turns")
        .or_else(|| raw_low.strip_suffix(" you cast during an opponent's turn"))
        .or_else(|| raw_low.strip_suffix(" you cast during each opponent's turn"))
    {
        Some((
            sp.trim(),
            false,
            false,
            ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)),
        ))
    } else {
        raw_low
            .strip_suffix(" cast during your turn")
            .or_else(|| raw_low.strip_suffix(" cast on your turn"))
            .map(|sp| (sp.trim(), true, false, ConditionAst::IsYourTurn))
    };
    if let Some((spell_part, any_player, opponents_only, turn_cond)) = turn_scoped {
        let generic_n = if less
            && delta.trim().eq_ignore_ascii_case("{x}")
            && scale_expr.is_some()
            && per.is_none()
        {
            1
        } else {
            parse_generic_mana_amount(delta).ok_or("oracle:non-generic-cost-mod")?
        };
        let amount = NonZeroU8::new(generic_n).ok_or("oracle:zero-cost-mod")?;
        let matching = if matches!(spell_part, "spells" | "spell") {
            SpellFilter::any()
        } else {
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            let parsed =
                spellwake_oracle_compiler::filter::parse_object_filter(spell_part, &compiler_vocab)
                    .map_err(|_| "oracle:unsupported-cost-modifier")?;
            if parsed.controller.is_some() {
                return Err("oracle:unsupported-cost-modifier");
            }
            lower_spell_filter(&parsed, vocabulary)?
        };
        let combined_cond = match condition {
            Some(c) => ConditionAst::And(Box::new(turn_cond), Box::new(c.clone())),
            None => turn_cond,
        };
        let modifier = if less {
            if opponents_only {
                return Err("oracle:unsupported-cost-modifier");
            }
            let discount = match lower_static_spell_discount_amount(amount, per, scale_expr) {
                Ok(a) => a,
                Err(_) => lower_dynamic_static_spell_discount(
                    amount, per, scale_expr, any_player, program, vocabulary,
                )?,
            };
            SpellCostModifier::Discount {
                any_player,
                matching,
                amount: discount,
            }
        } else {
            if scale_expr.is_some() || per.is_some() {
                return Err("oracle:unsupported-cost-modifier");
            }
            SpellCostModifier::Increase {
                any_player,
                opponents_only,
                matching,
                amount,
            }
        };
        return push_conditional_static(
            program,
            StaticContinuousAbility::CostModifier {
                id: ids.static_id(),
                modifier,
            },
            Some(&combined_cond),
            vocabulary,
        );
    }

    Err("oracle:unsupported-cost-modifier")
}

fn is_self_colors_count_scale(scale: Option<&ValueExprAst>) -> bool {
    match scale {
        Some(ValueExprAst::SelfColorsCount) => true,
        Some(ValueExprAst::EventCountThisTurn {
            event,
            player: None,
        }) => matches!(
            event.trim().to_ascii_lowercase().as_str(),
            "the number of colors it is" | "of its colors" | "of ~'s colors"
        ),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaticSelfAnchor {
    None,
    Source,
    AttachedHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StaticExprContext {
    anchor: StaticSelfAnchor,
    has_chosen_player_entry: bool,
}

impl StaticExprContext {
    fn from_subject_and_program(subject: &SubjectRefAst, program: &Program) -> Self {
        let anchor = match subject {
            SubjectRefAst::SelfObject => StaticSelfAnchor::Source,
            SubjectRefAst::AttachedHost(_) => StaticSelfAnchor::AttachedHost,
            _ => StaticSelfAnchor::None,
        };
        let has_chosen_player_entry = matches!(
            program.permanent().and_then(|p| p.entry_tap),
            Some(EntryTap::ChooseOpponent | EntryTap::ChoosePlayer)
        );
        Self {
            anchor,
            has_chosen_player_entry,
        }
    }

    fn for_source(program: &Program) -> Self {
        Self {
            anchor: StaticSelfAnchor::Source,
            has_chosen_player_entry: matches!(
                program.permanent().and_then(|p| p.entry_tap),
                Some(EntryTap::ChooseOpponent | EntryTap::ChoosePlayer)
            ),
        }
    }

    fn is_attached_host(self) -> bool {
        self.anchor == StaticSelfAnchor::AttachedHost
    }
}

fn resolve_static_counter_subject(
    subject: &SubjectRefAst,
    anchor: StaticSelfAnchor,
) -> Option<spellwake_engine::program::CounterSubjectRef> {
    match subject {
        SubjectRefAst::SelfObject => {
            Some(spellwake_engine::program::CounterSubjectRef::AbilitySource)
        }
        SubjectRefAst::AttachedHost(_) => {
            Some(spellwake_engine::program::CounterSubjectRef::AttachedHost)
        }
        SubjectRefAst::Anaphora(raw) => {
            let low = raw.trim().to_ascii_lowercase();
            if matches!(
                low.as_str(),
                "it" | "~"
                    | "this creature"
                    | "this permanent"
                    | "this artifact"
                    | "this enchantment"
            ) {
                match anchor {
                    StaticSelfAnchor::Source => {
                        Some(spellwake_engine::program::CounterSubjectRef::AbilitySource)
                    }
                    StaticSelfAnchor::AttachedHost => {
                        Some(spellwake_engine::program::CounterSubjectRef::AttachedHost)
                    }
                    StaticSelfAnchor::None => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_static_counter_kind(counter_kind: &str) -> Result<Option<CounterKind>, &'static str> {
    let ck_low = counter_kind.trim().to_ascii_lowercase();
    let is_any = ck_low.is_empty() || matches!(ck_low.as_str(), "any" | "all" | "a" | "each");
    if is_any {
        return Ok(None);
    }
    let kind = parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?;
    if !kind.is_permanent() {
        return Err("oracle:unsupported-counter-kind");
    }
    Ok(Some(kind))
}

fn lower_modify_pt_part(
    dx: &str,
    dy: &str,
    scale: Option<&ValueExprAst>,
    expr_ctx: StaticExprContext,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StaticPart, &'static str> {
    if is_self_colors_count_scale(scale) {
        let p_term = parse_pt_delta_term(dx).ok_or("oracle:non-numeric-pt-delta")?;
        let t_term = parse_pt_delta_term(dy).ok_or("oracle:non-numeric-pt-delta")?;
        let (p_per_color, t_per_color) = match (p_term, t_term) {
            (PtDeltaTerm::Literal(p), PtDeltaTerm::Literal(t))
            | (PtDeltaTerm::ScaledX(p), PtDeltaTerm::ScaledX(t)) => (p, t),
            (PtDeltaTerm::ScaledX(p), PtDeltaTerm::Literal(0)) => (p, 0),
            (PtDeltaTerm::Literal(0), PtDeltaTerm::ScaledX(t)) => (0, t),
            _ => return Err("oracle:unsupported-scaled-static-pt"),
        };
        return Ok(StaticPart::ModifyPTPerOwnColor {
            power: p_per_color.into(),
            toughness: t_per_color.into(),
        });
    }
    let (power, toughness) = lower_pt_delta_pair_with_context(dx, dy, scale, expr_ctx, vocabulary)?;
    Ok(StaticPart::ModifyPT(NumericPair { power, toughness }))
}

fn lower_static_player_ref(
    player: PlayerRefAst,
    expr_ctx: StaticExprContext,
) -> Option<ResolvingPlayer> {
    match player {
        PlayerRefAst::You => Some(ResolvingPlayer::Controller),
        PlayerRefAst::EnchantedPlayer => Some(ResolvingPlayer::EnchantedPlayer),
        PlayerRefAst::DefendingPlayer => Some(ResolvingPlayer::DefendingPlayer),
        PlayerRefAst::ActivePlayer => Some(ResolvingPlayer::ActivePlayer),
        PlayerRefAst::ChosenPlayer => Some(ResolvingPlayer::SourceChosenPlayer),
        PlayerRefAst::ThatPlayer if expr_ctx.has_chosen_player_entry => {
            Some(ResolvingPlayer::SourceChosenPlayer)
        }
        PlayerRefAst::ItsController | PlayerRefAst::ThatObjectController
            if expr_ctx.is_attached_host() =>
        {
            Some(ResolvingPlayer::AttachedHostController)
        }
        _ => None,
    }
}

fn lower_other_controlled_creature_clause(norm: &str) -> Option<ObjectClause> {
    let base = ObjectClause::creature()
        .controlled()
        .other_than_ability_source();
    match norm {
        "other creature you control" => Some(base),
        "other white creature you control" => {
            Some(base.with_color(ObjectColorFilter::has(Color::White)))
        }
        "other blue creature you control" => {
            Some(base.with_color(ObjectColorFilter::has(Color::Blue)))
        }
        "other black creature you control" => {
            Some(base.with_color(ObjectColorFilter::has(Color::Black)))
        }
        "other red creature you control" => {
            Some(base.with_color(ObjectColorFilter::has(Color::Red)))
        }
        "other green creature you control" => {
            Some(base.with_color(ObjectColorFilter::has(Color::Green)))
        }
        _ => None,
    }
}

fn lower_static_value_expr(
    expr: &ValueExprAst,
    expr_ctx: StaticExprContext,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ValueExpr, &'static str> {
    match expr {
        ValueExprAst::TotalCardsInAllHands => Ok(ValueExpr::Count {
            zone: ValueZone::AllHands,
            filter: Box::new(ObjectFilter::any()),
            controller_scope: None,
        }),
        ValueExprAst::PlayerStat { player, stat } => {
            if let Some(role) = lower_static_player_ref(*player, expr_ctx) {
                return Ok(match stat {
                    PlayerStatKind::LifeTotal if role == ResolvingPlayer::Controller => {
                        ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife)
                    }
                    PlayerStatKind::LifeTotal => {
                        ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(role))
                    }
                    PlayerStatKind::CardsInHand => {
                        ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(role))
                    }
                    PlayerStatKind::CardsInGraveyard => {
                        ValueExpr::PlayerStat(PlayerStatExpr::PlayerGraveyardSize(role))
                    }
                    PlayerStatKind::CardsInLibrary => {
                        ValueExpr::PlayerStat(PlayerStatExpr::PlayerLibrarySize(role))
                    }
                });
            }
            lower_value_expr(expr, vocabulary)
        }
        ValueExprAst::Count(f)
            if (f.zone == Some(ZoneFilter::YourGraveyardOrExile) && f.extra_zones.is_empty()
                || (f.zone == Some(ZoneFilter::Exile)
                    && f.extra_zones.as_slice() == [ZoneFilter::YourGraveyard]))
                && f.controller.is_none()
                && matches!(f.owner, None | Some(ControllerRelation::You)) =>
        {
            let exile_owner = if f.owner == Some(ControllerRelation::You) {
                Some(ResolvingPlayer::Controller)
            } else {
                None
            };
            let mut inner = f.as_ref().clone();
            inner.zone = None;
            inner.extra_zones.clear();
            inner.owner = None;
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            Ok(ValueExpr::Add(
                Box::new(ValueExpr::Count {
                    zone: spellwake_engine::program::ValueZone::Exile(exile_owner),
                    filter: Box::new(obj),
                    controller_scope: None,
                }),
                Box::new(ValueExpr::Count {
                    zone: spellwake_engine::program::ValueZone::Graveyard(
                        ResolvingPlayer::Controller,
                    ),
                    filter: Box::new(obj),
                    controller_scope: None,
                }),
            ))
        }
        ValueExprAst::Count(f)
            if expr_ctx.has_chosen_player_entry
                && f.extra_zones.is_empty()
                && matches!(
                    f.zone,
                    Some(
                        ZoneFilter::ThatPlayerGraveyard
                            | ZoneFilter::ThatPlayerHand
                            | ZoneFilter::ThatPlayerLibrary
                    )
                ) =>
        {
            let zone = match f.zone {
                Some(ZoneFilter::ThatPlayerGraveyard) => {
                    spellwake_engine::program::ValueZone::Graveyard(
                        ResolvingPlayer::SourceChosenPlayer,
                    )
                }
                Some(ZoneFilter::ThatPlayerHand) => {
                    spellwake_engine::program::ValueZone::Hand(ResolvingPlayer::SourceChosenPlayer)
                }
                Some(ZoneFilter::ThatPlayerLibrary) => {
                    spellwake_engine::program::ValueZone::Library(
                        ResolvingPlayer::SourceChosenPlayer,
                    )
                }
                _ => return Err("oracle:unsupported-numeric-expr"),
            };
            let mut inner = f.as_ref().clone();
            inner.zone = None;
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            Ok(ValueExpr::Count {
                zone,
                filter: Box::new(obj),
                controller_scope: None,
            })
        }
        ValueExprAst::Count(f)
            if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                && f.extra_zones.is_empty()
                && (f.controller == Some(ControllerRelation::ChosenPlayer)
                    || (expr_ctx.has_chosen_player_entry
                        && f.controller == Some(ControllerRelation::ThatPlayer))) =>
        {
            let mut inner = f.as_ref().clone();
            inner.zone = None;
            inner.controller = None;
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            Ok(ValueExpr::Count {
                zone: spellwake_engine::program::ValueZone::Battlefield,
                filter: Box::new(obj),
                controller_scope: Some(ResolvingPlayer::SourceChosenPlayer),
            })
        }
        ValueExprAst::Count(f)
            if expr_ctx.is_attached_host()
                && matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                && f.controller == Some(ControllerRelation::ItsController) =>
        {
            let mut inner = f.as_ref().clone();
            inner.zone = None;
            inner.controller = None;
            let obj = lower_object_filter(&inner, vocabulary)?;
            Ok(ValueExpr::Count {
                zone: spellwake_engine::program::ValueZone::Battlefield,
                filter: Box::new(obj),
                controller_scope: Some(ResolvingPlayer::AttachedHostController),
            })
        }
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } => {
            if let Some(subj_ref) =
                resolve_static_counter_subject(subject.as_ref(), expr_ctx.anchor)
            {
                let kind = parse_static_counter_kind(counter_kind)?;
                return Ok(ValueExpr::CounterCount {
                    subject: subj_ref,
                    kind,
                });
            }
            lower_value_expr(expr, vocabulary)
        }
        ValueExprAst::AggregateOverPlayers { op, metric } => {
            match (op, metric.as_ref()) {
                (
                    spellwake_oracle_compiler::ast::AggregateOp::Max,
                    ValueExprAst::PlayerStat {
                        player: PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer,
                        stat: PlayerStatKind::LifeTotal,
                    },
                ) => {
                    let ctrl = ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife);
                    let opp = ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife);
                    return Ok(ValueExpr::conditional(
                        ctrl.clone(),
                        Comparison::GreaterOrEqual,
                        opp.clone(),
                        ctrl,
                        opp,
                    ));
                }
                (
                    spellwake_oracle_compiler::ast::AggregateOp::Min,
                    ValueExprAst::PlayerStat {
                        player: PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer,
                        stat: PlayerStatKind::LifeTotal,
                    },
                ) => {
                    let ctrl = ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife);
                    let opp = ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife);
                    return Ok(ValueExpr::conditional(
                        ctrl.clone(),
                        Comparison::LessOrEqual,
                        opp.clone(),
                        ctrl,
                        opp,
                    ));
                }
                (
                    spellwake_oracle_compiler::ast::AggregateOp::Max,
                    ValueExprAst::PlayerStat {
                        player: PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent,
                        stat: PlayerStatKind::CardsInHand,
                    },
                ) => {
                    return Ok(ValueExpr::OpponentMaxCount {
                        zone: spellwake_engine::program::ValueZone::Hand(
                            ResolvingPlayer::Controller,
                        ),
                        filter: Box::new(ObjectFilter::any()),
                    });
                }
                _ => {}
            }
            lower_value_expr(expr, vocabulary)
        }
        ValueExprAst::EventCountThisTurn {
            event,
            player: None,
        } => {
            let norm = event.trim().to_ascii_lowercase();
            if matches!(
                norm.as_str(),
                "aura and equipment attached to ~" | "aura or equipment attached to ~"
            ) {
                return Ok(ValueExpr::AttachedCount(
                    AttachedCountKind::AurasAndEquipmentOnSource,
                ));
            }
            if let Some(clause) = lower_other_controlled_creature_clause(&norm) {
                return Ok(ValueExpr::count_battlefield(ObjectFilter::Clause(clause)));
            }
            match norm.as_str() {
                "spell you've cast this turn" | "spell you cast this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::ControllerSpellsCastThisTurn,
                    ));
                }
                "spell cast this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::SpellsCastMatchingThisTurn {
                            players: spellwake_engine::program::AffectedPlayers::EachPlayer,
                            filter: Box::new(ObjectFilter::any()),
                            from_zone: None,
                        },
                    ));
                }
                "spell your opponents have cast this turn"
                | "spell your opponents cast this turn"
                | "spell an opponent has cast this turn"
                | "spell an opponent cast this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::SpellsCastMatchingThisTurn {
                            players: AffectedPlayers::EachOpponent,
                            filter: Box::new(ObjectFilter::any()),
                            from_zone: None,
                        },
                    ));
                }
                "card you've drawn this turn" | "card you drew this turn" => {
                    return Ok(ValueExpr::PlayerStat(PlayerStatExpr::CardsDrawnThisTurn));
                }
                "life you gained this turn"
                | "1 life you gained this turn"
                | "life you've gained this turn"
                | "1 life you've gained this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::ControllerLifeGainedThisTurn,
                    ));
                }
                "life you lost this turn"
                | "1 life you lost this turn"
                | "life you've lost this turn"
                | "1 life you've lost this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::ControllerLifeLostThisTurn,
                    ));
                }
                "1 life your opponents have lost this turn"
                | "life your opponents have lost this turn"
                | "1 life your opponents lost this turn"
                | "life your opponents lost this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::OpponentsLifeLostThisTurn,
                    ));
                }
                "card you've discarded this turn" | "card you discarded this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::ControllerDiscardedThisTurn,
                    ));
                }
                "card that left your graveyard this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::CardsLeftGraveyardThisTurn,
                    ));
                }
                "card put into your graveyard this turn"
                | "card that entered your graveyard this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::CardsEnteredGraveyardThisTurn,
                    ));
                }
                "creature that died this turn" => {
                    return Ok(ValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn));
                }
                "creature you control that died this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::ControlledCreatureDiedThisTurn,
                    ));
                }
                "permanent you sacrificed this turn" | "permanent you've sacrificed this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::ControllerSacrificedThisTurn,
                    ));
                }
                "permanent that left the battlefield under your control this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::PermanentLeftBattlefieldThisTurn,
                    ));
                }
                "nonland permanent that left the battlefield under your control this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::NonlandPermanentLeftBattlefieldThisTurn,
                    ));
                }
                "token you created this turn" | "token you've created this turn" => {
                    return Ok(ValueExpr::PlayerStat(PlayerStatExpr::CreatedTokensThisTurn));
                }
                "land you played this turn" | "land you've played this turn" => {
                    return Ok(ValueExpr::PlayerStat(PlayerStatExpr::PlayedLandThisTurn));
                }
                "land that entered the battlefield under your control this turn"
                | "land that entered under your control this turn" => {
                    return Ok(ValueExpr::PlayerStat(PlayerStatExpr::LandsEnteredThisTurn(
                        ResolvingPlayer::Controller,
                    )));
                }
                "noncreature spell you've cast this turn"
                | "noncreature spell you cast this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::CastNoncreatureSpellsThisTurn,
                    ));
                }
                "spell you've cast from your hand this turn"
                | "spell you cast from your hand this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::CastSpellsFromHandThisTurn,
                    ));
                }
                "other spell you've cast this turn" | "other spell you cast this turn" => {
                    return Ok(ValueExpr::PlayerStat(
                        PlayerStatExpr::OtherSpellsCastThisTurn,
                    ));
                }
                "instant and sorcery spell you've cast this turn"
                | "instant or sorcery spell you've cast this turn"
                | "instant and sorcery spell you cast this turn"
                | "instant or sorcery spell you cast this turn" => {
                    return Ok(ValueExpr::Add(
                        Box::new(ValueExpr::PlayerStat(
                            PlayerStatExpr::CastSpellTypeThisTurn(CardType::Instant),
                        )),
                        Box::new(ValueExpr::PlayerStat(
                            PlayerStatExpr::CastSpellTypeThisTurn(CardType::Sorcery),
                        )),
                    ));
                }
                "time you've rolled a die this turn"
                | "die you've rolled this turn"
                | "die you rolled this turn" => {
                    return Ok(ValueExpr::PlayerStat(PlayerStatExpr::DiceRolledThisTurn));
                }
                "dungeon you've completed" | "dungeon you completed" => {
                    return Ok(ValueExpr::PlayerStat(PlayerStatExpr::CompletedDungeons));
                }
                "creature attacking you" => {
                    return Ok(ValueExpr::CountAttackingController);
                }
                "other creature you control with a +1/+1 counter on it" => {
                    return Ok(ValueExpr::count_battlefield(ObjectFilter::Clause(
                        ObjectClause::creature()
                            .controlled()
                            .other_than_ability_source()
                            .with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER),
                    )));
                }
                "other creature you control with a counter on it" => {
                    return Ok(ValueExpr::count_battlefield(ObjectFilter::Clause(
                        ObjectClause::creature()
                            .controlled()
                            .other_than_ability_source()
                            .with_flags(ObjectFilterFlags::WITH_ANY_COUNTER),
                    )));
                }
                "card in your opponents' hands"
                | "card in opponents' hands"
                | "card in each opponent's hand" => {
                    return Ok(ValueExpr::Count {
                        zone: ValueZone::OpponentsHands,
                        filter: Box::new(ObjectFilter::any()),
                        controller_scope: None,
                    });
                }
                "card in all players' hands" | "card in each player's hand" => {
                    return Ok(ValueExpr::Count {
                        zone: ValueZone::AllHands,
                        filter: Box::new(ObjectFilter::any()),
                        controller_scope: None,
                    });
                }
                _ => {}
            }
            lower_value_expr(expr, vocabulary)
        }
        ValueExprAst::AggregateStat { op, stat, filter }
            if matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                && filter.extra_zones.is_empty() =>
        {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary)?
            };
            let agg_op = match op {
                AggregateOp::Max => StatAggregate::Max,
                AggregateOp::Min => StatAggregate::Min,
                AggregateOp::Sum => StatAggregate::Sum,
            };
            Ok(ValueExpr::Aggregate {
                filter: Box::new(obj),
                stat: static_lower_stat_kind(*stat),
                op: agg_op,
            })
        }
        ValueExprAst::DistinctCountAmong { category, filter }
            if category == "different_mana_values"
                && filter.zone == Some(ZoneFilter::YourGraveyard)
                && filter.extra_zones.is_empty() =>
        {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            if inner.any_card || is_unrestricted_filter(&inner) {
                Ok(ValueExpr::DistinctManaValuesInGraveyard(
                    ResolvingPlayer::Controller,
                ))
            } else {
                lower_value_expr(expr, vocabulary)
            }
        }
        ValueExprAst::Add(a, b) => Ok(ValueExpr::Add(
            Box::new(lower_static_value_expr(a, expr_ctx, vocabulary)?),
            Box::new(lower_static_value_expr(b, expr_ctx, vocabulary)?),
        )),
        ValueExprAst::Sub(a, b) => Ok(ValueExpr::Sub(
            Box::new(lower_static_value_expr(a, expr_ctx, vocabulary)?),
            Box::new(lower_static_value_expr(b, expr_ctx, vocabulary)?),
        )),
        ValueExprAst::Mul(a, b) => match (a.as_ref(), b.as_ref()) {
            (ValueExprAst::Const(k), other) | (other, ValueExprAst::Const(k)) => {
                Ok(scale_value_expr(
                    lower_static_value_expr(other, expr_ctx, vocabulary)?,
                    i64::from(*k),
                ))
            }
            _ => lower_value_expr(expr, vocabulary),
        },
        _ => lower_value_expr(expr, vocabulary),
    }
}

fn lower_pt_delta_pair_with_context(
    dx: &str,
    dy: &str,
    scale: Option<&ValueExprAst>,
    expr_ctx: StaticExprContext,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(NumericExpression, NumericExpression), &'static str> {
    let p_term = parse_pt_delta_term(dx).ok_or("oracle:non-numeric-pt-delta")?;
    let t_term = parse_pt_delta_term(dy).ok_or("oracle:non-numeric-pt-delta")?;
    match (p_term, t_term) {
        (PtDeltaTerm::Literal(p_num), PtDeltaTerm::Literal(t_num)) => {
            if let Some(scale_expr) = scale {
                let base = lower_numeric_expr_with_context(scale_expr, expr_ctx, vocabulary)?;
                let scale_one = |coeff: i64| -> Result<NumericExpression, &'static str> {
                    if coeff == 0 {
                        return Ok(NumericExpression::Literal(0.into()));
                    }
                    match &base {
                        NumericExpression::Count {
                            selector,
                            multiplier,
                            offset,
                        } if *offset == 0.into() => Ok(NumericExpression::Count {
                            selector: selector.clone(),
                            multiplier: multiplier.times(&coeff.into()),
                            offset: 0.into(),
                        }),
                        NumericExpression::AttachedAurasAndEquipment { multiplier } => {
                            Ok(NumericExpression::AttachedAurasAndEquipment {
                                multiplier: multiplier.times(&coeff.into()),
                            })
                        }
                        NumericExpression::Eval(ve) => Ok(NumericExpression::Eval(Box::new(
                            scale_value_expr((**ve).clone(), coeff),
                        ))),
                        _ if coeff == 1 => Ok(base.clone()),
                        _ => Ok(NumericExpression::Scale {
                            expr: Box::new(base.clone()),
                            multiplier: coeff.into(),
                            offset: 0.into(),
                        }),
                    }
                };
                Ok((scale_one(p_num)?, scale_one(t_num)?))
            } else {
                Ok((
                    NumericExpression::Literal(p_num.into()),
                    NumericExpression::Literal(t_num.into()),
                ))
            }
        }
        (p_term, t_term) => {
            let Some(scale_expr) = scale else {
                return Err("oracle:non-numeric-pt-delta");
            };
            if let Ok(ve) = lower_static_value_expr(scale_expr, expr_ctx, vocabulary)
                && ve.is_world_evaluable()
            {
                let resolve_term = |term: PtDeltaTerm| -> NumericExpression {
                    match term {
                        PtDeltaTerm::Literal(n) => NumericExpression::Literal(n.into()),
                        PtDeltaTerm::ScaledX(k) => {
                            if k == 0 {
                                NumericExpression::Literal(0.into())
                            } else {
                                NumericExpression::Eval(Box::new(scale_value_expr(ve.clone(), k)))
                            }
                        }
                    }
                };
                return Ok((resolve_term(p_term), resolve_term(t_term)));
            }
            let base = lower_numeric_expr_with_context(scale_expr, expr_ctx, vocabulary)
                .map_err(|_| "oracle:unsupported-scaled-static-pt")?;
            let resolve_term = |term: PtDeltaTerm| -> NumericExpression {
                match term {
                    PtDeltaTerm::Literal(n) => NumericExpression::Literal(n.into()),
                    PtDeltaTerm::ScaledX(k) => {
                        if k == 0 {
                            NumericExpression::Literal(0.into())
                        } else if k == 1 {
                            base.clone()
                        } else {
                            NumericExpression::Scale {
                                expr: Box::new(base.clone()),
                                multiplier: k.into(),
                                offset: 0.into(),
                            }
                        }
                    }
                }
            };
            Ok((resolve_term(p_term), resolve_term(t_term)))
        }
    }
}

fn lower_numeric_expr_with_context(
    expr: &ValueExprAst,
    expr_ctx: StaticExprContext,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<NumericExpression, &'static str> {
    let subject_is_attached_host = expr_ctx.is_attached_host();
    match expr {
        ValueExprAst::Const(n) => Ok(NumericExpression::Literal(i64::from(*n).into())),
        ValueExprAst::Count(f) => {
            if f.zone == Some(ZoneFilter::YourGraveyard)
                && f.any_types.is_empty()
                && f.required_keywords.len() == 1
                && f.required_keywords[0].eq_ignore_ascii_case("cycling")
                && filter_only_has(
                    f,
                    FilterAllow {
                        zone: true,
                        req_keywords: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                return Ok(NumericExpression::PlayerStat(
                    StaticPlayerStat::OwnGraveyardWithCycling,
                ));
            }
            if f.zone == Some(ZoneFilter::YourGraveyard)
                && filter_only_has(
                    f,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                let types = lower_card_type_mask(f.any_types)?;
                return Ok(NumericExpression::ZoneCount {
                    zone: StaticZone::OwnGraveyard,
                    types,
                });
            }
            if matches!(
                f.zone,
                Some(ZoneFilter::AllGraveyards | ZoneFilter::AnyGraveyard)
            ) && filter_only_has(
                f,
                FilterAllow {
                    types: true,
                    zone: true,
                    ..FilterAllow::NONE
                },
            ) {
                let types = lower_card_type_mask(f.any_types)?;
                return Ok(NumericExpression::ZoneCount {
                    zone: StaticZone::AllGraveyards,
                    types,
                });
            }
            if f.zone == Some(ZoneFilter::YourHand)
                && filter_only_has(
                    f,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                let types = lower_card_type_mask(f.any_types)?;
                return Ok(NumericExpression::ZoneCount {
                    zone: StaticZone::OwnHand,
                    types,
                });
            }
            if f.zone == Some(ZoneFilter::Exile)
                && matches!(
                    f.owner,
                    Some(ControllerRelation::You | ControllerRelation::OwnerYou)
                )
            {
                let mut stripped = f.as_ref().clone();
                stripped.owner = None;
                if filter_only_has(
                    &stripped,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                ) {
                    let types = lower_card_type_mask(f.any_types)?;
                    return Ok(NumericExpression::ZoneCount {
                        zone: StaticZone::OwnExile,
                        types,
                    });
                }
            }
            if (f.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0
                && matches!(f.zone, None | Some(ZoneFilter::Exile))
            {
                let mut stripped = f.as_ref().clone();
                stripped.flags =
                    AstFilterFlags(stripped.flags.0 & !AstFilterFlags::EXILED_WITH_SELF.0);
                if filter_only_has(
                    &stripped,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                ) {
                    let types = lower_card_type_mask(f.any_types)?;
                    return Ok(NumericExpression::ZoneCount {
                        zone: StaticZone::LinkedExile,
                        types,
                    });
                }
            }
            if matches!(f.zone, None | Some(ZoneFilter::Battlefield))
                && let Ok(selector) = lower_static_filter_to_permanent_selector_with_host(
                    f,
                    subject_is_attached_host,
                    vocabulary,
                )
            {
                return Ok(NumericExpression::count(selector));
            }
            if let Ok(ve) = lower_static_value_expr(expr, expr_ctx, vocabulary)
                && ve.is_world_evaluable()
            {
                return Ok(NumericExpression::Eval(Box::new(ve)));
            }
            if matches!(f.zone, None | Some(ZoneFilter::Battlefield)) {
                let selector = lower_static_filter_to_permanent_selector_with_host(
                    f,
                    subject_is_attached_host,
                    vocabulary,
                )?;
                return Ok(NumericExpression::count(selector));
            }
            Err("oracle:unsupported-numeric-expr")
        }
        ValueExprAst::Add(a, b) => {
            if let (
                Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset,
                }),
                ValueExprAst::Const(k),
            ) = (
                lower_numeric_expr_with_context(a, expr_ctx, vocabulary),
                b.as_ref(),
            ) {
                return Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset: offset.plus(&i64::from(*k).into()),
                });
            }
            if let (
                ValueExprAst::Const(k),
                Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset,
                }),
            ) = (
                a.as_ref(),
                lower_numeric_expr_with_context(b, expr_ctx, vocabulary),
            ) {
                return Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset: offset.plus(&i64::from(*k).into()),
                });
            }
            if let Ok(ve) = lower_static_value_expr(expr, expr_ctx, vocabulary)
                && ve.is_world_evaluable()
            {
                return Ok(NumericExpression::Eval(Box::new(ve)));
            }
            if let (Ok(na), Ok(nb)) = (
                lower_numeric_expr_with_context(a, expr_ctx, vocabulary),
                lower_numeric_expr_with_context(b, expr_ctx, vocabulary),
            ) {
                return Ok(NumericExpression::Add(Box::new(na), Box::new(nb)));
            }
            Err("oracle:unsupported-numeric-expr")
        }
        ValueExprAst::Sub(a, b) => {
            if let (
                Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset,
                }),
                ValueExprAst::Const(k),
            ) = (
                lower_numeric_expr_with_context(a, expr_ctx, vocabulary),
                b.as_ref(),
            ) {
                return Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset: offset.minus(&i64::from(*k).into()),
                });
            }
            if let Ok(ve) = lower_static_value_expr(expr, expr_ctx, vocabulary)
                && ve.is_world_evaluable()
            {
                return Ok(NumericExpression::Eval(Box::new(ve)));
            }
            if let (Ok(na), Ok(nb)) = (
                lower_numeric_expr_with_context(a, expr_ctx, vocabulary),
                lower_numeric_expr_with_context(b, expr_ctx, vocabulary),
            ) {
                return Ok(NumericExpression::Add(
                    Box::new(na),
                    Box::new(NumericExpression::Scale {
                        expr: Box::new(nb),
                        multiplier: (-1_i64).into(),
                        offset: 0.into(),
                    }),
                ));
            }
            Err("oracle:unsupported-numeric-expr")
        }
        ValueExprAst::Mul(a, b) => {
            if let (
                Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset,
                }),
                ValueExprAst::Const(k),
            ) = (
                lower_numeric_expr_with_context(a, expr_ctx, vocabulary),
                b.as_ref(),
            ) {
                let k_ri: RulesInteger = i64::from(*k).into();
                return Ok(NumericExpression::Count {
                    selector,
                    multiplier: multiplier.times(&k_ri),
                    offset: offset.times(&k_ri),
                });
            }
            if let (
                ValueExprAst::Const(k),
                Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset,
                }),
            ) = (
                a.as_ref(),
                lower_numeric_expr_with_context(b, expr_ctx, vocabulary),
            ) {
                let k_ri: RulesInteger = i64::from(*k).into();
                return Ok(NumericExpression::Count {
                    selector,
                    multiplier: multiplier.times(&k_ri),
                    offset: offset.times(&k_ri),
                });
            }
            if let Ok(ve) = lower_static_value_expr(expr, expr_ctx, vocabulary)
                && ve.is_world_evaluable()
            {
                return Ok(NumericExpression::Eval(Box::new(ve)));
            }
            if let (ValueExprAst::Const(k), other) | (other, ValueExprAst::Const(k)) =
                (a.as_ref(), b.as_ref())
                && let Ok(inner) = lower_numeric_expr_with_context(other, expr_ctx, vocabulary)
            {
                return Ok(NumericExpression::Scale {
                    expr: Box::new(inner),
                    multiplier: i64::from(*k).into(),
                    offset: 0.into(),
                });
            }
            Err("oracle:unsupported-numeric-expr")
        }
        ValueExprAst::PlayerStat {
            player: PlayerRefAst::You,
            stat: PlayerStatKind::CardsInHand,
        } => Ok(NumericExpression::ZoneCount {
            zone: StaticZone::OwnHand,
            types: CardTypeMask::EMPTY,
        }),
        ValueExprAst::PlayerStat {
            player: PlayerRefAst::You,
            stat: PlayerStatKind::CardsInGraveyard,
        } => Ok(NumericExpression::ZoneCount {
            zone: StaticZone::OwnGraveyard,
            types: CardTypeMask::EMPTY,
        }),
        ValueExprAst::EventCountThisTurn {
            event,
            player: None,
        } => {
            let norm = event.trim().to_ascii_lowercase();
            if matches!(
                norm.as_str(),
                "aura and equipment attached to ~" | "aura or equipment attached to ~"
            ) {
                return Ok(NumericExpression::AttachedAurasAndEquipment {
                    multiplier: 1.into(),
                });
            }
            if norm == "unspent mana you have" {
                return Ok(NumericExpression::PlayerStat(
                    StaticPlayerStat::TotalUnspentMana,
                ));
            }
            if let Some(mana_kind) = match norm.as_str() {
                "unspent white mana you have" => Some(ManaType::White),
                "unspent blue mana you have" => Some(ManaType::Blue),
                "unspent black mana you have" => Some(ManaType::Black),
                "unspent red mana you have" => Some(ManaType::Red),
                "unspent green mana you have" => Some(ManaType::Green),
                "unspent colorless mana you have" => Some(ManaType::Colourless),
                _ => None,
            } {
                return Ok(NumericExpression::PlayerStat(StaticPlayerStat::ManaInPool(
                    mana_kind,
                )));
            }
            if let Some(clause) = lower_other_controlled_creature_clause(&norm) {
                return Ok(NumericExpression::Eval(Box::new(
                    ValueExpr::count_battlefield(ObjectFilter::Clause(clause)),
                )));
            }
            if let Ok(ve) = lower_static_value_expr(expr, expr_ctx, vocabulary)
                && ve.is_world_evaluable()
            {
                Ok(NumericExpression::Eval(Box::new(ve)))
            } else {
                Err("oracle:unsupported-numeric-expr")
            }
        }
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } => {
            if let SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) =
                subject.as_ref()
                && f.any_types == AstCardTypeMask::CREATURE
                && f.controller == Some(ControllerRelation::You)
                && f.flags == AstFilterFlags::EXCLUDE_SELF
            {
                let mut stripped = f.as_ref().clone();
                stripped.flags = AstFilterFlags::EMPTY;
                if filter_only_has(
                    &stripped,
                    FilterAllow {
                        types: true,
                        controller: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                ) && let Some(kind) = parse_static_counter_kind(counter_kind)?
                {
                    return Ok(NumericExpression::PlayerStat(
                        StaticPlayerStat::OtherControlledCreatureCounters(kind),
                    ));
                }
            }
            let Some(subj_ref) = resolve_static_counter_subject(subject.as_ref(), expr_ctx.anchor)
            else {
                return Err("oracle:unsupported-numeric-expr");
            };
            let parsed_kind = parse_static_counter_kind(counter_kind)?;
            match (subj_ref, parsed_kind) {
                (spellwake_engine::program::CounterSubjectRef::AbilitySource, Some(kind)) => {
                    Ok(NumericExpression::Counters { source: None, kind })
                }
                (subject, kind) => Ok(NumericExpression::Eval(Box::new(ValueExpr::CounterCount {
                    subject,
                    kind,
                }))),
            }
        }
        ValueExprAst::Devotion { colors } => {
            let low = colors.trim().to_ascii_lowercase();
            if let Some(color) = parse_color_word(&low) {
                return Ok(NumericExpression::PlayerStat(StaticPlayerStat::Devotion(
                    color,
                )));
            }
            let mut mask = ColorMask::EMPTY;
            for raw_tok in low
                .replace(" and ", " ")
                .replace(" or ", " ")
                .replace(',', " ")
                .split_whitespace()
            {
                let color = parse_color_word(raw_tok).ok_or("oracle:multi-color-devotion")?;
                mask = mask.with(color);
            }
            if mask.is_empty() {
                return Err("oracle:multi-color-devotion");
            }
            Ok(NumericExpression::Eval(Box::new(ValueExpr::Devotion(mask))))
        }
        _ => {
            if let Ok(ve) = lower_static_value_expr(expr, expr_ctx, vocabulary)
                && ve.is_world_evaluable()
            {
                Ok(NumericExpression::Eval(Box::new(ve)))
            } else {
                Err("oracle:unsupported-numeric-expr")
            }
        }
    }
}

fn is_permanent_attachment_program(program: &Program) -> bool {
    matches!(
        program.permanent().and_then(|p| p.attachment),
        Some(AttachmentKind::Equipment | AttachmentKind::Fortification)
    ) || matches!(
        program.permanent().and_then(|p| p.attachment),
        Some(AttachmentKind::Aura(r)) if !matches!(r, EnchantRestriction::Player)
    )
}

fn selector_guaranteed_source_controller(selector: &PermanentSelector, program: &Program) -> bool {
    if matches!(
        selector.controller,
        ControllerSelector::SourceItself
            | ControllerSelector::SelfAtMaxSpeed
            | ControllerSelector::SourcePair
            | ControllerSelector::SourceController
    ) {
        return true;
    }
    if selector.attached_to_source {
        return match program.permanent().and_then(|p| p.attachment) {
            Some(AttachmentKind::Equipment) => true,
            Some(AttachmentKind::Aura(EnchantRestriction::CreatureYouControl)) => true,
            Some(AttachmentKind::Aura(EnchantRestriction::Matching(ObjectFilter::Clause(c)))) => {
                c.controller == ControllerFilter::You
            }
            Some(AttachmentKind::Aura(EnchantRestriction::Matching(ObjectFilter::Either(
                a,
                b,
            )))) => a.controller == ControllerFilter::You && b.controller == ControllerFilter::You,
            _ => false,
        };
    }
    false
}

fn parse_nonneg_u8_word(raw: &str) -> Option<u8> {
    let trimmed = raw.trim();
    if trimmed == "zero" {
        return Some(0);
    }
    trimmed.parse::<u8>().ok().or_else(|| {
        spellwake_oracle_compiler::vocab::parse_number_word(trimmed)
            .and_then(|v| u8::try_from(v).ok())
    })
}

fn parse_all_players_additional_land_plays(raw: &str) -> Option<NonZeroU8> {
    let trimmed = raw.trim().trim_end_matches('.').trim();
    let rest = trimmed.strip_prefix("each player may play ")?;
    let body = rest
        .strip_suffix(" on each of their turns")
        .or_else(|| rest.strip_suffix(" during each of their turns"))?
        .trim();
    let count_raw = body
        .strip_suffix(" additional lands")
        .or_else(|| body.strip_suffix(" additional land"))?
        .trim();
    let count = if count_raw == "an" {
        1
    } else {
        parse_nonneg_u8_word(count_raw)?
    };
    NonZeroU8::new(count)
}

fn parse_color_word_mask(raw: &str) -> Option<ColorMask> {
    let mut mask = ColorMask::EMPTY;
    for part in raw.split(" or ") {
        let color = parse_color_word(part.trim())?;
        mask = mask.with(color);
    }
    (!mask.is_empty()).then_some(mask)
}

fn parse_cant_be_targeted_by_color_mask(ts: &str) -> Option<ColorMask> {
    let (lhs_raw, rhs_raw) = ts.split_once(" spells or abilities from ")?;
    let rhs_colors_raw = rhs_raw.strip_suffix(" sources")?.trim();
    let lhs_mask = parse_color_word_mask(lhs_raw.trim())?;
    let rhs_mask = parse_color_word_mask(rhs_colors_raw)?;
    (!lhs_mask.is_empty() && lhs_mask == rhs_mask).then_some(lhs_mask)
}

fn spell_filter_supported_by_matches_characteristics(sf: &SpellFilter) -> bool {
    use spellwake_engine::program::{SpellCastZoneFilter, SpellSpecialFilter, SpellTargetShape};
    const ALLOWED_FLAGS: ObjectFilterFlags = ObjectFilterFlags::HISTORIC
        .union(ObjectFilterFlags::NON_HISTORIC)
        .union(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
        .union(ObjectFilterFlags::SADDLED)
        .union(ObjectFilterFlags::HAS_ADVENTURE)
        .union(ObjectFilterFlags::HAS_X_IN_COST)
        .union(ObjectFilterFlags::HAS_TOXIC)
        .union(ObjectFilterFlags::NO_ABILITIES)
        .union(ObjectFilterFlags::COMMANDER_ONLY)
        .union(ObjectFilterFlags::NOT_COMMANDER)
        .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_TYPE);
    if !matches!(sf.cast_zone, SpellCastZoneFilter::Any)
        || matches!(sf.cast_zone, SpellCastZoneFilter::LinkedExile)
        || sf.target_shape != SpellTargetShape::Any
        || sf.ordinal_this_turn.is_some()
    {
        return false;
    }
    match sf.special {
        SpellSpecialFilter::AuraOrEquipment | SpellSpecialFilter::ChosenEntryCreatureType => {
            return sf.object == ObjectFilter::any();
        }
        SpellSpecialFilter::None => {}
        SpellSpecialFilter::AboveExperience | SpellSpecialFilter::CopyableInstruction => {
            return false;
        }
    }
    let clause_ok = |c: ObjectClause| {
        c.required_supertypes.is_empty()
            && c.forbidden_supertypes.is_empty()
            && matches!(
                c.subtypes,
                SubtypeFilter::Any
                    | SubtypeFilter::Has(_)
                    | SubtypeFilter::Lacks(_)
                    | SubtypeFilter::AnyOf(_)
                    | SubtypeFilter::NoneOf(_)
                    | SubtypeFilter::AnyOfList(_)
                    | SubtypeFilter::AllOfList(_)
                    | SubtypeFilter::NoneOfList(_)
                    | SubtypeFilter::Land(_)
                    | SubtypeFilter::NotLand(_)
                    | SubtypeFilter::Enchantment(_)
                    | SubtypeFilter::Spell(_)
                    | SubtypeFilter::Artifact(_)
                    | SubtypeFilter::Planeswalker(_)
                    | SubtypeFilter::Aura
                    | SubtypeFilter::AnyLandMask(_)
                    | SubtypeFilter::NotLandMask(_)
            )
            && c.controller == ControllerFilter::Any
            && c.owner == ControllerFilter::Any
            && c.source_relation == SourceRelation::Any
            && c.combat == CombatFilter::Any
            && matches!(c.power, PowerFilter::Any | PowerFilter::AtLeast(_))
            && c.toughness == ToughnessFilter::Any
            && c.mana_value == ManaValueFilter::Any
            && c.keyword.is_none()
            && c.forbidden_keyword.is_none()
            && c.tapped.is_none()
            && c.token.is_none()
            && c.required_counter_kind.is_none()
            && c.forbidden_counter_kind.is_none()
            && ALLOWED_FLAGS.union(c.flags) == ALLOWED_FLAGS
    };
    match sf.object {
        ObjectFilter::Clause(c) => clause_ok(c),
        ObjectFilter::Either(a, b) => clause_ok(a) && clause_ok(b),
    }
}

fn spell_filter_supported_by_prohibit_casting(sf: &SpellFilter) -> bool {
    use spellwake_engine::program::{SpellSpecialFilter, SpellTargetShape};
    const ALLOWED_FLAGS: ObjectFilterFlags = ObjectFilterFlags::HISTORIC
        .union(ObjectFilterFlags::NON_HISTORIC)
        .union(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
        .union(ObjectFilterFlags::SADDLED)
        .union(ObjectFilterFlags::HAS_ADVENTURE)
        .union(ObjectFilterFlags::HAS_X_IN_COST)
        .union(ObjectFilterFlags::HAS_TOXIC)
        .union(ObjectFilterFlags::NO_ABILITIES)
        .union(ObjectFilterFlags::COMMANDER_ONLY)
        .union(ObjectFilterFlags::NOT_COMMANDER)
        .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_TYPE)
        .union(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_COLOR)
        .union(ObjectFilterFlags::MATCHES_CHOSEN_NAME)
        .union(ObjectFilterFlags::MATCHES_CHOSEN_CARD_TYPE);
    if sf.target_shape != SpellTargetShape::Any || sf.ordinal_this_turn.is_some() {
        return false;
    }
    match sf.special {
        SpellSpecialFilter::AuraOrEquipment | SpellSpecialFilter::ChosenEntryCreatureType => {
            return sf.object == ObjectFilter::any();
        }
        SpellSpecialFilter::None => {}
        SpellSpecialFilter::AboveExperience | SpellSpecialFilter::CopyableInstruction => {
            return false;
        }
    }
    let clause_ok = |c: ObjectClause| {
        matches!(
            c.subtypes,
            SubtypeFilter::Any
                | SubtypeFilter::Has(_)
                | SubtypeFilter::Lacks(_)
                | SubtypeFilter::AnyOf(_)
                | SubtypeFilter::NoneOf(_)
                | SubtypeFilter::AnyOfList(_)
                | SubtypeFilter::AllOfList(_)
                | SubtypeFilter::NoneOfList(_)
                | SubtypeFilter::Land(_)
                | SubtypeFilter::NotLand(_)
                | SubtypeFilter::Enchantment(_)
                | SubtypeFilter::Spell(_)
                | SubtypeFilter::Artifact(_)
                | SubtypeFilter::Planeswalker(_)
                | SubtypeFilter::Aura
                | SubtypeFilter::AnyLandMask(_)
                | SubtypeFilter::NotLandMask(_)
        ) && c.controller == ControllerFilter::Any
            && c.owner == ControllerFilter::Any
            && c.source_relation == SourceRelation::Any
            && c.combat == CombatFilter::Any
            && matches!(c.power, PowerFilter::Any | PowerFilter::AtLeast(_))
            && c.toughness == ToughnessFilter::Any
            && matches!(
                c.mana_value,
                ManaValueFilter::Any
                    | ManaValueFilter::AtMost(_)
                    | ManaValueFilter::AtLeast(_)
                    | ManaValueFilter::Equal(_)
                    | ManaValueFilter::Even
                    | ManaValueFilter::Odd
            )
            && c.keyword.is_none()
            && c.forbidden_keyword.is_none()
            && c.tapped.is_none()
            && c.token.is_none()
            && c.required_counter_kind.is_none()
            && c.forbidden_counter_kind.is_none()
            && ALLOWED_FLAGS.union(c.flags) == ALLOWED_FLAGS
    };
    match sf.object {
        ObjectFilter::Clause(c) => clause_ok(c),
        ObjectFilter::Either(a, b) => clause_ok(a) && clause_ok(b),
    }
}

fn parse_library_top_cast_filter(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let trimmed = raw.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() {
        return None;
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    let try_single = |text: &str| -> Option<ObjectFilter> {
        let parsed =
            spellwake_oracle_compiler::filter::parse_object_filter(text, &compiler_vocab).ok()?;
        if parsed.zone.is_some() || parsed.controller.is_some() || parsed.owner.is_some() {
            return None;
        }
        let lowered = lower_object_filter(&parsed, vocabulary).ok()?;
        let clause_ok = |c: ObjectClause| {
            c.controller == ControllerFilter::Any && c.source_relation == SourceRelation::Any
        };
        let ok = match lowered {
            ObjectFilter::Clause(c) => clause_ok(c),
            ObjectFilter::Either(a, b) => clause_ok(a) && clause_ok(b),
        };
        ok.then_some(lowered)
    };
    if let Some(obj) = try_single(trimmed) {
        return Some(obj);
    }
    if let Some(stem) = trimmed.strip_suffix(" spells") {
        if let Some((lhs, rhs)) = stem
            .split_once(" spells and ")
            .or_else(|| stem.split_once(" spells or "))
            && !lhs.contains(',')
            && !rhs.contains(',')
            && !rhs.contains(" and ")
            && !rhs.contains(" or ")
        {
            let lhs_obj = try_single(&format!("{} spells", lhs.trim()))?;
            let rhs_obj = try_single(&format!("{} spells", rhs.trim()))?;
            if let (ObjectFilter::Clause(a), ObjectFilter::Clause(b)) = (lhs_obj, rhs_obj) {
                return Some(ObjectFilter::Either(a, b));
            }
        }
        if let Some((lhs, rhs)) = stem.split_once(" and ")
            && !lhs.contains(',')
            && !rhs.contains(',')
            && !rhs.contains(" and ")
        {
            let lhs_obj = try_single(&format!("{} spells", lhs.trim()))?;
            let rhs_obj = try_single(&format!("{} spells", rhs.trim()))?;
            if let (ObjectFilter::Clause(a), ObjectFilter::Clause(b)) = (lhs_obj, rhs_obj) {
                return Some(ObjectFilter::Either(a, b));
            }
        }
        if stem.contains(',') {
            let mut subtypes = Vec::new();
            let mut all_resolved = true;
            for piece in stem.split(',') {
                let p = piece.trim();
                let p = p
                    .strip_prefix("and ")
                    .or_else(|| p.strip_prefix("or "))
                    .unwrap_or(p)
                    .trim();
                if p.is_empty() {
                    continue;
                }
                if let Some(ct) = vocabulary.resolve_label(p) {
                    subtypes.push(spellwake_engine::program::SingleSubtype::Creature(ct));
                } else {
                    all_resolved = false;
                    break;
                }
            }
            if all_resolved
                && subtypes.len() >= 2
                && let Some(list) = spellwake_engine::program::SubtypeList::from_slice(&subtypes)
            {
                return Some(ObjectFilter::Clause(ObjectClause {
                    subtypes: SubtypeFilter::AnyOfList(list),
                    ..ObjectClause::any()
                }));
            }
        }
    }
    None
}

fn lower_loses_all_other_abilities(
    subject: Option<&SubjectRefAst>,
    condition: Option<&ConditionAst>,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    let mut selectors = match subject {
        Some(s) => lower_static_permanent_selectors_with_context(s, condition, vocabulary)?,
        None => vec![PermanentSelector {
            controller: ControllerSelector::SourceItself,
            ..PermanentSelector::default()
        }],
    };
    for sel in &mut selectors {
        if let Some(cond) = condition {
            apply_static_selector_condition(sel, cond, vocabulary)?;
        }
    }
    #[derive(Clone, Copy)]
    enum MergeMode {
        AppendNew,
        MergeAbilitiesPart,
        PrependRemovalToGrant,
    }
    // Verify before mutating so any rejected selector leaves `program` untouched.
    let mut merge_modes: Vec<MergeMode> = Vec::with_capacity(selectors.len());
    for sel in &selectors {
        let mut abilities_part_count = 0usize;
        let mut can_merge_in_place = false;
        let mut grant_ability_count = 0usize;
        let mut can_prepend_to_grant = false;
        for ab in program
            .continuous
            .iter()
            .chain(program.additional_battlefield_abilities.iter())
        {
            if let StaticContinuousAbility::Battlefield {
                id,
                selector: existing_sel,
                parts,
            } = ab
                && existing_sel == sel
            {
                let mut ab_has_grant = false;
                let mut ab_has_abilities = false;
                let mut ab_has_inherit = false;
                for part in &parts.0 {
                    match part {
                        StaticPart::GrantTrigger(_)
                        | StaticPart::GrantActivated(_)
                        | StaticPart::GrantActivatedRestricted { .. }
                        | StaticPart::GrantLoyalty { .. }
                        | StaticPart::GrantAnyColorMana
                        | StaticPart::GrantFixedColorMana(_)
                        | StaticPart::GrantMana(_)
                        | StaticPart::GrantSubtypeProtection { .. }
                        | StaticPart::GrantProtectionChosenColor
                        | StaticPart::GrantProtectionChosenPlayer => {
                            ab_has_grant = true;
                        }
                        StaticPart::InheritCounteredCreatureAbilities
                        | StaticPart::InheritZoneAbilities { .. }
                        | StaticPart::InheritExiledCreatureAbilities => {
                            ab_has_inherit = true;
                        }
                        StaticPart::Abilities(change) => {
                            abilities_part_count += 1;
                            ab_has_abilities = true;
                            if condition.is_none()
                                && !program.static_conditions.contains_key(id)
                                && matches!(change.removal, None | Some(AbilityRemoval::All))
                            {
                                can_merge_in_place = true;
                            } else {
                                return Err("oracle:unsupported-rule-restriction");
                            }
                        }
                        _ => {}
                    }
                }
                if ab_has_inherit {
                    return Err("oracle:unsupported-rule-restriction");
                }
                if ab_has_grant {
                    grant_ability_count += 1;
                    if !ab_has_abilities
                        && condition.is_none()
                        && !program.static_conditions.contains_key(id)
                    {
                        can_prepend_to_grant = true;
                    } else {
                        return Err("oracle:unsupported-rule-restriction");
                    }
                }
            }
        }
        if abilities_part_count > 1
            || grant_ability_count > 1
            || (abilities_part_count > 0 && grant_ability_count > 0)
        {
            return Err("oracle:unsupported-rule-restriction");
        }
        let mode = if abilities_part_count == 1 && can_merge_in_place {
            MergeMode::MergeAbilitiesPart
        } else if grant_ability_count == 1 && can_prepend_to_grant {
            MergeMode::PrependRemovalToGrant
        } else {
            MergeMode::AppendNew
        };
        merge_modes.push(mode);
    }
    for (sel, mode) in selectors.into_iter().zip(merge_modes) {
        match mode {
            MergeMode::MergeAbilitiesPart => {
                for ab in program
                    .continuous
                    .iter_mut()
                    .chain(program.additional_battlefield_abilities.iter_mut())
                {
                    if let StaticContinuousAbility::Battlefield {
                        selector: existing_sel,
                        parts,
                        ..
                    } = ab
                        && existing_sel == &sel
                    {
                        for part in &mut parts.0 {
                            if let StaticPart::Abilities(change) = part {
                                change.removal = Some(AbilityRemoval::All);
                            }
                        }
                    }
                }
            }
            MergeMode::PrependRemovalToGrant => {
                for ab in program
                    .continuous
                    .iter_mut()
                    .chain(program.additional_battlefield_abilities.iter_mut())
                {
                    if let StaticContinuousAbility::Battlefield {
                        selector: existing_sel,
                        parts,
                        ..
                    } = ab
                        && existing_sel == &sel
                        && parts.0.iter().any(|part| {
                            matches!(
                                part,
                                StaticPart::GrantTrigger(_)
                                    | StaticPart::GrantActivated(_)
                                    | StaticPart::GrantActivatedRestricted { .. }
                                    | StaticPart::GrantLoyalty { .. }
                                    | StaticPart::GrantAnyColorMana
                                    | StaticPart::GrantFixedColorMana(_)
                                    | StaticPart::GrantMana(_)
                                    | StaticPart::GrantSubtypeProtection { .. }
                                    | StaticPart::GrantProtectionChosenColor
                                    | StaticPart::GrantProtectionChosenPlayer
                            )
                        })
                    {
                        parts.0.insert(
                            0,
                            StaticPart::Abilities(AbilityChange::remove(AbilityRemoval::All)),
                        );
                        break;
                    }
                }
            }
            MergeMode::AppendNew => {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: sel,
                        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::remove(
                            AbilityRemoval::All,
                        ))]),
                    },
                )?;
            }
        }
    }
    Ok(())
}

fn lower_static_base_pt_part(
    p_raw: &str,
    t_raw: &str,
    vocabulary: &CreatureTypeVocabulary,
    err: &'static str,
) -> Result<StaticPart, &'static str> {
    let p_trim = p_raw.trim();
    let t_trim = t_raw.trim();
    if p_trim.eq_ignore_ascii_case("its mana value")
        && t_trim.eq_ignore_ascii_case("its mana value")
    {
        return Ok(StaticPart::SetPTToOwnManaValue);
    }
    let parse_single = |raw: &str| -> Option<NumericExpression> {
        if let Ok(n) = raw.parse::<i64>() {
            return Some(NumericExpression::Literal(n.into()));
        }
        let ve = spellwake_oracle_compiler::filter::parse_value_expr(
            raw,
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        lower_numeric_expr_with_context(
            &ve,
            StaticExprContext {
                anchor: StaticSelfAnchor::Source,
                has_chosen_player_entry: false,
            },
            vocabulary,
        )
        .ok()
    };
    if t_trim.eq_ignore_ascii_case("keep") {
        let expr = parse_single(p_trim).ok_or(err)?;
        return Ok(StaticPart::SetPower(expr));
    }
    if p_trim.eq_ignore_ascii_case("keep") {
        let expr = parse_single(t_trim).ok_or(err)?;
        return Ok(StaticPart::SetToughness(expr));
    }
    let p: i64 = p_trim.parse().map_err(|_| err)?;
    let t: i64 = t_trim.parse().map_err(|_| err)?;
    Ok(StaticPart::SetPT(NumericPair {
        power: NumericExpression::Literal(p.into()),
        toughness: NumericExpression::Literal(t.into()),
    }))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct StaticAbilityLock {
    doesnt_untap: bool,
    suppressed_activations: Option<ActivationSuppression>,
    cannot_attack_or_block: bool,
    cannot_crew_vehicles: bool,
    requires_source_controller: bool,
    singular_attached_fallback: bool,
    allow_none_as_self: bool,
}

fn parse_static_ability_lock(rule: &str, has_condition: bool) -> Option<StaticAbilityLock> {
    if rule == "DoesntUntap" {
        return Some(StaticAbilityLock {
            doesnt_untap: true,
            suppressed_activations: None,
            cannot_attack_or_block: false,
            cannot_crew_vehicles: false,
            requires_source_controller: false,
            singular_attached_fallback: false,
            allow_none_as_self: true,
        });
    }
    if rule == "DoesntUntapIf" {
        return has_condition.then_some(StaticAbilityLock {
            doesnt_untap: true,
            suppressed_activations: None,
            cannot_attack_or_block: false,
            cannot_crew_vehicles: false,
            requires_source_controller: false,
            singular_attached_fallback: false,
            allow_none_as_self: true,
        });
    }
    let r_low = rule
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
    match r_low.as_str() {
        "doesn't untap during its controller's untap step"
        | "don't untap during their controllers' untap steps"
        | "~ doesn't untap during its controller's untap step" => {
            return Some(StaticAbilityLock {
                doesnt_untap: true,
                suppressed_activations: None,
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: false,
                allow_none_as_self: true,
            });
        }
        "enchanted creature doesn't untap during its controller's untap step"
        | "enchanted permanent doesn't untap during its controller's untap step" => {
            return Some(StaticAbilityLock {
                doesnt_untap: true,
                suppressed_activations: None,
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: true,
                allow_none_as_self: false,
            });
        }
        "doesn't untap during your untap step" | "~ doesn't untap during your untap step" => {
            return Some(StaticAbilityLock {
                doesnt_untap: true,
                suppressed_activations: None,
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: true,
                singular_attached_fallback: false,
                allow_none_as_self: true,
            });
        }
        "activatedabilitiescantbeactivated"
        | "cantactivateabilities"
        | "activated_abilities_cant_be_activated" => {
            return Some(StaticAbilityLock {
                doesnt_untap: false,
                suppressed_activations: Some(ActivationSuppression::All),
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: false,
                allow_none_as_self: false,
            });
        }
        "nonmanaactivatedabilitiescantbeactivated" => {
            return Some(StaticAbilityLock {
                doesnt_untap: false,
                suppressed_activations: Some(ActivationSuppression::NonMana),
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: false,
                allow_none_as_self: false,
            });
        }
        "cant_activate_nonmana_abilities" => {
            return Some(StaticAbilityLock {
                doesnt_untap: false,
                suppressed_activations: Some(ActivationSuppression::NonMana),
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: true,
                allow_none_as_self: false,
            });
        }
        "can't attack or block, and its activated abilities can't be activated"
        | "can't attack or block and its activated abilities can't be activated"
        | "can't attack or block, and their activated abilities can't be activated"
        | "can't attack or block and their activated abilities can't be activated" => {
            return Some(StaticAbilityLock {
                doesnt_untap: false,
                suppressed_activations: Some(ActivationSuppression::All),
                cannot_attack_or_block: true,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: false,
                allow_none_as_self: false,
            });
        }
        "enchanted creature can't attack, block, or crew vehicles" => {
            return Some(StaticAbilityLock {
                cannot_attack_or_block: true,
                cannot_crew_vehicles: true,
                singular_attached_fallback: true,
                ..StaticAbilityLock::default()
            });
        }
        "can't attack, block, or crew vehicles" => {
            return Some(StaticAbilityLock {
                cannot_attack_or_block: true,
                cannot_crew_vehicles: true,
                ..StaticAbilityLock::default()
            });
        }
        "can't attack, block, or crew vehicles, and its activated abilities can't be activated"
        | "can't attack, block, or crew vehicles, and their activated abilities can't be activated" =>
        {
            return Some(StaticAbilityLock {
                suppressed_activations: Some(ActivationSuppression::All),
                cannot_attack_or_block: true,
                cannot_crew_vehicles: true,
                ..StaticAbilityLock::default()
            });
        }
        "can't attack, block, or crew vehicles, and its activated abilities can't be activated unless they're mana abilities"
        | "can't attack, block, or crew vehicles, and their activated abilities can't be activated unless they're mana abilities" =>
        {
            return Some(StaticAbilityLock {
                suppressed_activations: Some(ActivationSuppression::NonMana),
                cannot_attack_or_block: true,
                cannot_crew_vehicles: true,
                ..StaticAbilityLock::default()
            });
        }
        "can't attack or block, and its activated abilities can't be activated unless they're mana abilities"
        | "can't attack or block and its activated abilities can't be activated unless they're mana abilities"
        | "can't attack or block, and their activated abilities can't be activated unless they're mana abilities"
        | "can't attack or block and their activated abilities can't be activated unless they're mana abilities" =>
        {
            return Some(StaticAbilityLock {
                doesnt_untap: false,
                suppressed_activations: Some(ActivationSuppression::NonMana),
                cannot_attack_or_block: true,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: false,
                allow_none_as_self: false,
            });
        }
        "doesn't untap during its controller's untap step, and its activated abilities can't be activated"
        | "doesn't untap during its controller's untap step and its activated abilities can't be activated"
        | "don't untap during their controllers' untap steps, and their activated abilities can't be activated"
        | "don't untap during their controllers' untap steps and their activated abilities can't be activated" =>
        {
            return Some(StaticAbilityLock {
                doesnt_untap: true,
                suppressed_activations: Some(ActivationSuppression::All),
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: false,
                allow_none_as_self: false,
            });
        }
        "doesn't untap during its controller's untap step, and its activated abilities can't be activated unless they're mana abilities"
        | "doesn't untap during its controller's untap step and its activated abilities can't be activated unless they're mana abilities"
        | "don't untap during their controllers' untap steps, and their activated abilities can't be activated unless they're mana abilities"
        | "don't untap during their controllers' untap steps and their activated abilities can't be activated unless they're mana abilities" =>
        {
            return Some(StaticAbilityLock {
                doesnt_untap: true,
                suppressed_activations: Some(ActivationSuppression::NonMana),
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: false,
                allow_none_as_self: false,
            });
        }
        _ => {}
    }
    let lock_raw = r_low
        .strip_prefix("abilitytimingorlock:")
        .unwrap_or(&r_low)
        .trim()
        .trim_end_matches('.')
        .trim();
    match lock_raw {
        "its activated abilities can't be activated" => Some(StaticAbilityLock {
            doesnt_untap: false,
            suppressed_activations: Some(ActivationSuppression::All),
            cannot_attack_or_block: false,
            cannot_crew_vehicles: false,
            requires_source_controller: false,
            singular_attached_fallback: true,
            allow_none_as_self: false,
        }),
        "their activated abilities can't be activated" => Some(StaticAbilityLock {
            doesnt_untap: false,
            suppressed_activations: Some(ActivationSuppression::All),
            cannot_attack_or_block: false,
            cannot_crew_vehicles: false,
            requires_source_controller: false,
            singular_attached_fallback: false,
            allow_none_as_self: false,
        }),
        "its activated abilities can't be activated unless they're mana abilities" => {
            Some(StaticAbilityLock {
                doesnt_untap: false,
                suppressed_activations: Some(ActivationSuppression::NonMana),
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: true,
                allow_none_as_self: false,
            })
        }
        "their activated abilities can't be activated unless they're mana abilities" => {
            Some(StaticAbilityLock {
                doesnt_untap: false,
                suppressed_activations: Some(ActivationSuppression::NonMana),
                cannot_attack_or_block: false,
                cannot_crew_vehicles: false,
                requires_source_controller: false,
                singular_attached_fallback: false,
                allow_none_as_self: false,
            })
        }
        _ => None,
    }
}

fn clause_supported_by_blocking_static_characteristics(c: &ObjectClause) -> bool {
    const ALLOWED_FLAGS: ObjectFilterFlags = ObjectFilterFlags::HISTORIC
        .union(ObjectFilterFlags::NON_HISTORIC)
        .union(ObjectFilterFlags::ENCHANTED)
        .union(ObjectFilterFlags::EQUIPPED)
        .union(ObjectFilterFlags::MODIFIED)
        .union(ObjectFilterFlags::WITHOUT_COUNTERS)
        .union(ObjectFilterFlags::WITH_ANY_COUNTER)
        .union(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
        .union(ObjectFilterFlags::WITH_MINUS_ONE_COUNTER)
        .union(ObjectFilterFlags::FACE_DOWN)
        .union(ObjectFilterFlags::COMMANDER_ONLY)
        .union(ObjectFilterFlags::NOT_COMMANDER)
        .union(ObjectFilterFlags::ENTERED_THIS_TURN)
        .union(ObjectFilterFlags::ATTACKED_THIS_TURN)
        .union(ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN)
        .union(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN)
        .union(ObjectFilterFlags::KICKED)
        .union(ObjectFilterFlags::RENOWNED)
        .union(ObjectFilterFlags::SADDLED)
        .union(ObjectFilterFlags::MONSTROUS)
        .union(ObjectFilterFlags::SUSPECTED)
        .union(ObjectFilterFlags::GOADED)
        .union(ObjectFilterFlags::SOLVED)
        .union(ObjectFilterFlags::PREPARED)
        .union(ObjectFilterFlags::EXERTED)
        .union(ObjectFilterFlags::HAS_TOXIC)
        .union(ObjectFilterFlags::NO_ABILITIES);
    !c.forbidden_types.contains(CardType::Creature)
        && c.controller == ControllerFilter::Any
        && c.source_relation == SourceRelation::Any
        && c.combat == CombatFilter::Any
        && ALLOWED_FLAGS.union(c.flags) == ALLOWED_FLAGS
        && !matches!(
            c.subtypes,
            SubtypeFilter::ExceptChosenSlot(_)
                | SubtypeFilter::MatchesChosenSlot(_)
                | SubtypeFilter::OutsideNamedSlots
        )
        && matches!(
            c.power,
            PowerFilter::Any
                | PowerFilter::AtLeast(_)
                | PowerFilter::AtMost(_)
                | PowerFilter::Equal(_)
                | PowerFilter::PowerOrToughnessAtMost(_)
                | PowerFilter::CompareAnchor {
                    anchor: DynamicStatAnchor::SourcePower
                        | DynamicStatAnchor::SourceToughness
                        | DynamicStatAnchor::SourceManaValue,
                    ..
                }
        )
        && matches!(
            c.toughness,
            ToughnessFilter::Any
                | ToughnessFilter::AtLeast(_)
                | ToughnessFilter::AtMost(_)
                | ToughnessFilter::Equal(_)
                | ToughnessFilter::CompareAnchor {
                    anchor: DynamicStatAnchor::SourcePower
                        | DynamicStatAnchor::SourceToughness
                        | DynamicStatAnchor::SourceManaValue,
                    ..
                }
        )
        && matches!(
            c.mana_value,
            ManaValueFilter::Any
                | ManaValueFilter::AtMost(_)
                | ManaValueFilter::AtLeast(_)
                | ManaValueFilter::Equal(_)
                | ManaValueFilter::Even
                | ManaValueFilter::Odd
                | ManaValueFilter::CompareAnchor {
                    anchor: DynamicStatAnchor::SourcePower
                        | DynamicStatAnchor::SourceToughness
                        | DynamicStatAnchor::SourceManaValue,
                    ..
                }
        )
}

fn normalize_blocker_filter_ast(
    flt: &ObjectFilterAst,
) -> Option<(Vec<ObjectFilterAst>, CardTypeMask)> {
    if flt.zone.is_some()
        || !flt.extra_zones.is_empty()
        || flt.owner.is_some()
        || flt.any_permanent
        || flt.any_card
        || flt.any_spell
        || flt.is_ability
        || flt.named.is_some()
    {
        return None;
    }
    let req_type = if flt.any_types.is_empty() || flt.any_types == AstCardTypeMask::CREATURE {
        CardTypeMask::EMPTY
    } else if flt.any_types == AstCardTypeMask::ARTIFACT
        || flt.any_types == AstCardTypeMask::ARTIFACT.union(AstCardTypeMask::CREATURE)
    {
        CardTypeMask::ARTIFACT
    } else if flt.any_types == AstCardTypeMask::ENCHANTMENT
        || flt.any_types == AstCardTypeMask::ENCHANTMENT.union(AstCardTypeMask::CREATURE)
    {
        CardTypeMask::ENCHANTMENT
    } else if flt.any_types == AstCardTypeMask::LAND
        || flt.any_types == AstCardTypeMask::LAND.union(AstCardTypeMask::CREATURE)
    {
        CardTypeMask::LAND
    } else {
        return None;
    };
    let mut norm = flt.clone();
    norm.any_types = AstCardTypeMask::CREATURE;
    if norm.required_keywords.len() == 1
        && norm.required_keywords[0]
            .trim()
            .eq_ignore_ascii_case("flying or reach")
    {
        let mut with_flying = norm.clone();
        with_flying.required_keywords = vec!["flying".to_string()];
        let mut with_reach = norm;
        with_reach.required_keywords = vec!["reach".to_string()];
        return Some((vec![with_flying, with_reach], req_type));
    }
    Some((vec![norm], req_type))
}

fn merge_disjunctive_blocker_clauses(clauses: Vec<ObjectClause>) -> Vec<ObjectClause> {
    let mut unique: Vec<ObjectClause> = Vec::with_capacity(clauses.len());
    for clause in clauses {
        if !unique.contains(&clause) {
            unique.push(clause);
        }
    }
    let mut color_merged: Vec<ObjectClause> = Vec::with_capacity(unique.len());
    for clause in unique {
        if let ObjectColorFilter::Mask { any_of, none_of } = clause.color
            && !any_of.is_empty()
            && none_of.is_empty()
        {
            let mut key = clause;
            key.color = ObjectColorFilter::Any;
            if let Some(prev) = color_merged.iter_mut().find(|prev| {
                if let ObjectColorFilter::Mask {
                    any_of: prev_any,
                    none_of: prev_none,
                } = prev.color
                    && !prev_any.is_empty()
                    && prev_none.is_empty()
                {
                    let mut prev_key = **prev;
                    prev_key.color = ObjectColorFilter::Any;
                    prev_key == key
                } else {
                    false
                }
            }) && let ObjectColorFilter::Mask {
                any_of: prev_any, ..
            } = prev.color
            {
                prev.color = ObjectColorFilter::Mask {
                    any_of: prev_any.union(any_of),
                    none_of: ColorMask::EMPTY,
                };
                continue;
            }
        }
        color_merged.push(clause);
    }
    let mut out: Vec<ObjectClause> = Vec::with_capacity(color_merged.len());
    let mut subtype_groups: Vec<(ObjectClause, Vec<CreatureTypeId>)> = Vec::new();
    for clause in color_merged {
        let group_types = match clause.subtypes {
            SubtypeFilter::Has(ct) => Some(vec![ct]),
            SubtypeFilter::AnyOf(cts) => Some(cts.to_vec()),
            _ => None,
        };
        if let Some(cts) = group_types {
            let mut key = clause;
            key.subtypes = SubtypeFilter::Any;
            if let Some((_, existing)) = subtype_groups.iter_mut().find(|(k, _)| *k == key) {
                for ct in cts {
                    if !existing.contains(&ct) {
                        existing.push(ct);
                    }
                }
            } else {
                let mut deduped = Vec::new();
                for ct in cts {
                    if !deduped.contains(&ct) {
                        deduped.push(ct);
                    }
                }
                subtype_groups.push((key, deduped));
            }
        } else {
            out.push(clause);
        }
    }
    for (base, cts) in subtype_groups {
        for chunk in cts.chunks(3) {
            let mut built = base;
            built.subtypes = match chunk {
                [a] => SubtypeFilter::Has(*a),
                [a, b] => SubtypeFilter::AnyOf([*a, *b, *a]),
                [a, b, c] => SubtypeFilter::AnyOf([*a, *b, *c]),
                _ => continue,
            };
            out.push(built);
        }
    }
    out
}

fn lower_cant_be_blocked_except_by_any(
    allowed_filters: &[ObjectFilterAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Option<BlockingRestriction> {
    if allowed_filters.is_empty() {
        return None;
    }
    let mut clauses = Vec::with_capacity(allowed_filters.len());
    for flt in allowed_filters {
        let (expanded, req_type) = normalize_blocker_filter_ast(flt)?;
        for norm in expanded {
            let lowered = lower_object_filter(&norm, vocabulary).ok()?;
            match lowered {
                ObjectFilter::Clause(mut c) => {
                    c.required_types = c.required_types.union(req_type);
                    if !clause_supported_by_blocking_static_characteristics(&c) {
                        return None;
                    }
                    clauses.push(c);
                }
                ObjectFilter::Either(mut a, mut b) => {
                    a.required_types = a.required_types.union(req_type);
                    b.required_types = b.required_types.union(req_type);
                    if !clause_supported_by_blocking_static_characteristics(&a)
                        || !clause_supported_by_blocking_static_characteristics(&b)
                    {
                        return None;
                    }
                    clauses.push(a);
                    clauses.push(b);
                }
            }
        }
    }
    let merged = merge_disjunctive_blocker_clauses(clauses);
    let single_color_of = |c: &ObjectClause| -> Option<Color> {
        let ObjectColorFilter::Mask { any_of, none_of } = c.color else {
            return None;
        };
        if !none_of.is_empty() {
            return None;
        }
        let color = if any_of == ColorMask::WHITE {
            Color::White
        } else if any_of == ColorMask::BLUE {
            Color::Blue
        } else if any_of == ColorMask::BLACK {
            Color::Black
        } else if any_of == ColorMask::RED {
            Color::Red
        } else if any_of == ColorMask::GREEN {
            Color::Green
        } else {
            return None;
        };
        let mut stripped = *c;
        stripped.color = ObjectColorFilter::Any;
        (stripped == ObjectClause::creature()).then_some(color)
    };
    let is_plain_artifact = |c: &ObjectClause| -> bool {
        let mut stripped = *c;
        stripped.required_types = CardTypeMask::EMPTY;
        c.required_types == CardTypeMask::ARTIFACT && stripped == ObjectClause::creature()
    };
    let plain_keyword_of = |c: &ObjectClause| -> Option<KeywordAbility> {
        let kw = c.keyword?;
        let mut stripped = *c;
        stripped.keyword = None;
        (stripped == ObjectClause::creature()).then_some(kw)
    };
    let plain_subtype_of = |c: &ObjectClause| -> Option<CreatureTypeId> {
        let SubtypeFilter::Has(ct) = c.subtypes else {
            return None;
        };
        let mut stripped = *c;
        stripped.subtypes = SubtypeFilter::Any;
        (stripped == ObjectClause::creature()).then_some(ct)
    };
    match merged.as_slice() {
        [only] => {
            if is_plain_artifact(only) {
                Some(BlockingRestriction::OnlyBlockedByArtifact)
            } else if let Some(color) = single_color_of(only) {
                Some(BlockingRestriction::OnlyBlockedByColor(color))
            } else if let Some(ct) = plain_subtype_of(only) {
                Some(BlockingRestriction::OnlyBlockedBySubtype(ct))
            } else {
                Some(BlockingRestriction::OnlyBlockedByMatching(
                    ObjectFilter::Clause(*only),
                ))
            }
        }
        [a, b] => {
            if matches!(
                (plain_keyword_of(a), plain_keyword_of(b)),
                (Some(KeywordAbility::Flying), Some(KeywordAbility::Reach))
                    | (Some(KeywordAbility::Reach), Some(KeywordAbility::Flying))
            ) {
                Some(BlockingRestriction::OnlyBlockedByFlyingOrReach)
            } else if is_plain_artifact(a)
                && let Some(color) = single_color_of(b)
            {
                Some(BlockingRestriction::OnlyBlockedByColorOrArtifact(color))
            } else if is_plain_artifact(b)
                && let Some(color) = single_color_of(a)
            {
                Some(BlockingRestriction::OnlyBlockedByColorOrArtifact(color))
            } else {
                Some(BlockingRestriction::OnlyBlockedByMatching(
                    ObjectFilter::Either(*a, *b),
                ))
            }
        }
        _ => None,
    }
}

/// Preserve the engine's live cost-discount quantities instead of snapshotting them at resolution.
fn lower_static_spell_discount_amount(
    unit: NonZeroU8,
    per: Option<&ObjectFilterAst>,
    scale: Option<&ValueExprAst>,
) -> Result<SpellDiscount, &'static str> {
    match (per, scale) {
        (None, None) => Ok(SpellDiscount::Fixed(unit)),
        (Some(filter), None)
            if unit.get() == 1
                && lower_cast_cost_count(filter)
                    == Ok(CastCostCount::ControlledCreaturesWithPlusOneCounters) =>
        {
            Ok(SpellDiscount::ControlledCreaturesWithPlusOneCounters)
        }
        (
            None,
            Some(ValueExprAst::CounterCount {
                subject,
                counter_kind,
            }),
        ) if unit.get() == 1
            && matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            && counter_kind.eq_ignore_ascii_case("+1/+1") =>
        {
            Ok(SpellDiscount::SourcePlusOneCounters)
        }
        (
            None,
            Some(ValueExprAst::PlayerCounterCount {
                player: PlayerRefAst::You,
                counter_kind,
            }),
        ) if counter_kind.eq_ignore_ascii_case("experience") => {
            Ok(SpellDiscount::PerExperienceCounter(unit))
        }
        _ => Err("oracle:unsupported-cost-modifier"),
    }
}

fn is_controller_agnostic_world_discount(expr: &ValueExpr) -> bool {
    match expr {
        ValueExpr::Literal(_) => true,
        ValueExpr::CounterCount {
            subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
            ..
        } => true,
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            is_controller_agnostic_world_discount(inner)
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            is_controller_agnostic_world_discount(a) && is_controller_agnostic_world_discount(b)
        }
        _ => false,
    }
}

fn lower_dynamic_static_spell_discount(
    unit: NonZeroU8,
    per: Option<&ObjectFilterAst>,
    scale_expr: Option<&ValueExprAst>,
    any_player: bool,
    program: &Program,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SpellDiscount, &'static str> {
    let expr_ctx = StaticExprContext::for_source(program);
    let base_expr = match (per, scale_expr) {
        (Some(per_filter), None) => lower_static_value_expr(
            &ValueExprAst::Count(Box::new(per_filter.clone())),
            expr_ctx,
            vocabulary,
        )?,
        (None, Some(se)) => lower_static_value_expr(se, expr_ctx, vocabulary)?,
        _ => return Err("oracle:unsupported-cost-modifier"),
    };
    let scaled = if unit.get() == 1 {
        base_expr
    } else {
        scale_value_expr(base_expr, i64::from(unit.get()))
    };
    if scaled.validate_world_expr(vocabulary).is_err() {
        return Err("oracle:unsupported-cost-modifier");
    }
    if any_player && !is_controller_agnostic_world_discount(&scaled) {
        return Err("oracle:unsupported-cost-modifier");
    }
    Ok(SpellDiscount::Dynamic(Box::new(scaled)))
}

#[cfg(test)]
mod wave10_discount_tests {
    use super::*;

    #[test]
    fn wave10_static_discount_reads_source_plus_counters_only() {
        let unit = NonZeroU8::new(1).unwrap();
        let source = ValueExprAst::CounterCount {
            subject: Box::new(SubjectRefAst::SelfObject),
            counter_kind: "+1/+1".into(),
        };
        assert_eq!(
            lower_static_spell_discount_amount(unit, None, Some(&source)),
            Ok(SpellDiscount::SourcePlusOneCounters)
        );
        assert!(
            lower_static_spell_discount_amount(NonZeroU8::new(2).unwrap(), None, Some(&source))
                .is_err()
        );
        let oil = ValueExprAst::CounterCount {
            subject: Box::new(SubjectRefAst::SelfObject),
            counter_kind: "oil".into(),
        };
        assert!(lower_static_spell_discount_amount(unit, None, Some(&oil)).is_err());
    }

    #[test]
    fn wave10_static_discount_preserves_experience_multiplier_and_player() {
        let unit = NonZeroU8::new(2).unwrap();
        let experience = ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::You,
            counter_kind: "experience".into(),
        };
        assert_eq!(
            lower_static_spell_discount_amount(unit, None, Some(&experience)),
            Ok(SpellDiscount::PerExperienceCounter(unit))
        );
        let other_player = ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::TargetPlayer,
            counter_kind: "experience".into(),
        };
        assert!(lower_static_spell_discount_amount(unit, None, Some(&other_player)).is_err());
    }

    #[test]
    fn wave10_static_discount_counts_creatures_not_counter_quantity() {
        let unit = NonZeroU8::new(1).unwrap();
        let mut creatures = ObjectFilterAst {
            any_types: AstCardTypeMask::CREATURE,
            controller: Some(ControllerRelation::You),
            counter_kind: Some("+1/+1".into()),
            ..Default::default()
        };
        assert_eq!(
            lower_static_spell_discount_amount(unit, Some(&creatures), None),
            Ok(SpellDiscount::ControlledCreaturesWithPlusOneCounters)
        );
        creatures.tapped = Some(true);
        assert!(lower_static_spell_discount_amount(unit, Some(&creatures), None).is_err());
    }
}

// Typed station bands must consume every nested semantic channel. Raw payload text
// is diagnostic provenance only once the parser has supplied a compiled face.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_compiled_station_band(
    source_face: &FaceAst,
    power: Option<&str>,
    toughness: Option<&str>,
    compiled: Option<&FaceAst>,
    condition: StaticSourceCondition,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    match (power, toughness, compiled) {
        (None, None, Some(face)) => {
            if !face.failures.is_empty()
                || !face.spell_effects.is_empty()
                || !face.level_bands.is_empty()
                || !face.class_levels.is_empty()
                || !face.case_solved.is_empty()
            {
                return Err("oracle:unsupported-station-band");
            }
            lower_gated_section(
                source_face,
                &face.keywords,
                &face.statics,
                &face.abilities,
                &face.replacements,
                GatedSectionMode::LevelerBand(condition),
                program,
                vocabulary,
                ids,
            )
        }
        (Some(power), Some(toughness), None) => {
            let power = power
                .trim()
                .parse::<i64>()
                .map_err(|_| "oracle:station-band-bad-pt")?;
            let toughness = toughness
                .trim()
                .parse::<i64>()
                .map_err(|_| "oracle:station-band-bad-pt")?;
            push_static_ability(
                program,
                StaticContinuousAbility::Battlefield {
                    id: ids.static_id(),
                    selector: PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        source_condition: Some(condition),
                        ..PermanentSelector::default()
                    },
                    parts: StaticParts(vec![
                        StaticPart::Type(spellwake_engine::characteristics::StaticTypeChange {
                            supertypes: TypeOperation::Keep,
                            types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                            subtypes: SubtypeOperation::Keep,
                            land_subtypes: SubtypeOperation::Keep,
                        }),
                        StaticPart::SetPT(NumericPair {
                            power: NumericExpression::Literal(power.into()),
                            toughness: NumericExpression::Literal(toughness.into()),
                        }),
                    ]),
                },
            )
        }
        _ => Err("oracle:unsupported-station-band"),
    }
}

#[cfg(test)]
mod wave12_station_tests {
    use super::*;

    fn vocabulary() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new("wave12-station".into(), [0; 32], vec!["Robot".into()])
            .expect("test vocabulary")
    }

    fn station_face() -> FaceAst {
        compile_card(
            &CardInput {
                oracle_id: "",
                name: "Station fixture",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Station fixture",
                    type_line: "Artifact",
                    oracle_text: "Flying",
                }],
            },
            &cached_compiler_vocab(&vocabulary()),
        )
        .faces
        .remove(0)
    }

    #[test]
    fn wave12_station_compiled_keywords_keep_charge_threshold() {
        let card = InventoryCard {
            scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
            oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
            name: "Wedgelight Rammer".into(), layout: "normal".into(),
            faces: vec![spellwake_content::CatalogFace {
                name: "Wedgelight Rammer".into(), type_line: Some("Artifact \u{2014} Spacecraft".into()),
                oracle_text: Some("When this Spacecraft enters, create a 2/2 colorless Robot artifact creature token.\nStation (Tap another creature you control: Put charge counters equal to its power on this Spacecraft. Station only as a sorcery. It's an artifact creature at 9+.)\n9+ | Flying, first strike".into()),
                mana_cost: Some("{3}{W}".into()), power: Some("3".into()), toughness: Some("4".into()),
            }],
            legalities: BTreeMap::new(), eligible_formats: Vec::new(),
            required_by_base_formats: true, program_evidence: "test",
        };
        let program = compile_with_reason(&card, &vocabulary()).expect("typed station card");
        assert!(!program.printed_keywords.contains(&KeywordAbility::Flying));
        assert!(program.continuous.iter().chain(program.additional_battlefield_abilities.iter()).any(|ability| matches!(ability,
            StaticContinuousAbility::Battlefield { selector, parts, .. }
                if selector.source_condition == Some(StaticSourceCondition::SourceCounterAtLeast {
                    kind: CounterKind::Charge, minimum: 9,
                }) && parts.0.iter().any(|part| matches!(part,
                    StaticPart::Abilities(change) if change.granted_keywords.contains(&KeywordAbility::Flying))))));
    }

    #[test]
    fn wave12_station_rejects_partial_pt_and_unconsumed_nested_effects() {
        let face = station_face();
        let condition = StaticSourceCondition::SourceCounterAtLeast {
            kind: CounterKind::Charge,
            minimum: 9,
        };
        let mut program = Program::artifact(Some(ManaCost::free()), vec![]);
        let mut ids = LoweringIds::default();
        assert!(
            lower_compiled_station_band(
                &face,
                Some("3"),
                None,
                None,
                condition.clone(),
                &mut program,
                &vocabulary(),
                &mut ids
            )
            .is_err()
        );
        let mut invalid = face.clone();
        invalid
            .spell_effects
            .push(InstructionAst::AbilityRestriction {
                restriction: "unsupported station payload".into(),
                condition: None,
            });
        assert!(
            lower_compiled_station_band(
                &face,
                None,
                None,
                Some(&invalid),
                condition,
                &mut program,
                &vocabulary(),
                &mut ids
            )
            .is_err()
        );
        assert!(program.continuous.is_none());
        assert!(program.additional_battlefield_abilities.is_empty());
    }
    #[test]
    fn wave12_station_mana_preserves_plain_tap_and_rejects_extra_costs() {
        for (text, supported) in [
            ("{T}: Add {G}.", true),
            ("{1}, {T}: Add {G}.", false),
            ("{T}, Sacrifice this artifact: Add {G}.", false),
            ("{T}: Add {G}. You gain 1 life.", false),
        ] {
            let compiled = compile_card(
                &CardInput {
                    oracle_id: "",
                    name: "Station fixture",
                    layout: "normal",
                    faces: vec![FaceInput {
                        name: "Station fixture",
                        type_line: "Artifact",
                        oracle_text: text,
                    }],
                },
                &cached_compiler_vocab(&vocabulary()),
            );
            assert!(compiled.strictly_compiled, "{text}");
            let mut program = Program::artifact(Some(ManaCost::free()), vec![]);
            let result = lower_compiled_station_band(
                &compiled.faces[0],
                None,
                None,
                Some(&compiled.faces[0]),
                StaticSourceCondition::SourceCounterAtLeast {
                    kind: CounterKind::Charge,
                    minimum: 3,
                },
                &mut program,
                &vocabulary(),
                &mut LoweringIds::default(),
            );
            assert_eq!(result.is_ok(), supported, "{text}: {result:?}");
            if supported {
                assert!(program.continuous.iter().chain(program.additional_battlefield_abilities.iter())
                    .any(|ability| matches!(ability, StaticContinuousAbility::Battlefield { parts, .. }
                        if matches!(parts.0.as_slice(), [StaticPart::GrantFixedColorMana(ManaType::Green)]))));
            }
        }
    }
}
