use super::*;

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
            let mut selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let (p_expr, t_expr) = lower_pt_delta_pair(dx, dy, scale.as_deref(), vocabulary)?;
            let mut extra_prot_parts = Vec::new();
            let mut remaining_grant_kws = Vec::new();
            for kw in grant_keywords {
                let low = kw.trim().to_ascii_lowercase();
                if matches!(
                    low.as_str(),
                    "protection from the chosen color" | "protection from the color of your choice"
                ) {
                    extra_prot_parts.push(StaticPart::GrantProtectionChosenColor);
                } else if low == "protection from the chosen player" {
                    extra_prot_parts.push(StaticPart::GrantProtectionChosenPlayer);
                } else {
                    remaining_grant_kws.push(kw.clone());
                }
            }
            let ab_change = if !remaining_grant_kws.is_empty() {
                Some(lower_ability_change(&remaining_grant_kws, vocabulary)?)
            } else {
                None
            };
            let granted_parts = if let Some(raw_text) = grant_ability_text {
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
                if !synthetic.strictly_compiled || synthetic.faces.len() != 1 {
                    return Err("oracle:unsupported-grant-ability-text");
                }
                lower_granted_ability_face_parts(&synthetic.faces[0], vocabulary, ids)?
            } else {
                Vec::new()
            };
            for selector in &selectors {
                let mut parts = vec![StaticPart::ModifyPT(NumericPair {
                    power: p_expr.clone(),
                    toughness: t_expr.clone(),
                })];
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
            let mut selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let mut parts_vec = Vec::new();
            let mut remaining_kws = Vec::new();
            for kw in keywords {
                let low = kw.trim().to_ascii_lowercase();
                if matches!(
                    low.as_str(),
                    "protection from the chosen color" | "protection from the color of your choice"
                ) {
                    parts_vec.push(StaticPart::GrantProtectionChosenColor);
                } else if low == "protection from the chosen player" {
                    parts_vec.push(StaticPart::GrantProtectionChosenPlayer);
                } else {
                    remaining_kws.push(kw.clone());
                }
            }
            if !remaining_kws.is_empty() {
                let ab_change = lower_ability_change(&remaining_kws, vocabulary)?;
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
            ability_text: _,
            compiled: Some(compiled_face),
            condition,
        } => {
            let mut selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let parts = lower_granted_ability_face_parts(compiled_face, vocabulary, ids)?;
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
            Ok(())
        }
        StaticAbilityAst::SetOrAddType {
            subject,
            type_or_subtype,
            additive,
            condition,
        } => {
            let mut selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let type_part = parse_static_type_part(type_or_subtype, *additive, vocabulary)?;
            for selector in selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector,
                        parts: StaticParts(vec![type_part.clone()]),
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
            let mut selectors = lower_permanent_selectors(subject, vocabulary)?;
            for sel in &mut selectors {
                if let Some(cond) = condition {
                    apply_selector_condition(sel, cond, vocabulary)?;
                }
            }
            let mut parts = Vec::new();
            if let Some(desc) = set_types {
                parts.push(parse_static_type_part(desc, *additive_types, vocabulary)?);
            }
            if let Some(color_str) = set_colors {
                let mut colors = BTreeSet::new();
                let mut saw_colorless = false;
                for raw_tok in color_str
                    .replace(" and ", " ")
                    .replace(',', " ")
                    .split_whitespace()
                {
                    match raw_tok.to_ascii_lowercase().as_str() {
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
                        "a" | "an" => {}
                        _ => return Err("oracle:unsupported-layer-color"),
                    }
                }
                if colors.is_empty() && !saw_colorless {
                    return Err("oracle:unsupported-layer-color");
                }
                parts.push(StaticPart::Color(ColorOperation::Set(
                    StaticColorOperand::Literal(colors),
                )));
            }
            if *lose_all_abilities || !grant_keywords.is_empty() || !lose_keywords.is_empty() {
                let mut rem_kws = Vec::new();
                for kw in grant_keywords {
                    let low = kw.trim().to_ascii_lowercase();
                    if matches!(
                        low.as_str(),
                        "protection from the chosen color"
                            | "protection from the color of your choice"
                    ) {
                        parts.push(StaticPart::GrantProtectionChosenColor);
                    } else if low == "protection from the chosen player" {
                        parts.push(StaticPart::GrantProtectionChosenPlayer);
                    } else {
                        rem_kws.push(kw.clone());
                    }
                }
                if *lose_all_abilities || !rem_kws.is_empty() || !lose_keywords.is_empty() {
                    let mut change = if !rem_kws.is_empty() {
                        lower_ability_change(&rem_kws, vocabulary)?
                    } else {
                        AbilityChange::default()
                    };
                    if *lose_all_abilities {
                        change.removal = Some(AbilityRemoval::All);
                    }
                    if !lose_keywords.is_empty() {
                        change
                            .suppressed_keywords
                            .extend(lower_keyword_list(lose_keywords)?);
                    }
                    parts.push(StaticPart::Abilities(change));
                }
            }
            if let Some((p_raw, t_raw)) = base_pt {
                let p: i64 = p_raw
                    .trim()
                    .parse()
                    .map_err(|_| "oracle:unsupported-layer-pt")?;
                let t: i64 = t_raw
                    .trim()
                    .parse()
                    .map_err(|_| "oracle:unsupported-layer-pt")?;
                parts.push(StaticPart::SetPT(NumericPair {
                    power: NumericExpression::Literal(p.into()),
                    toughness: NumericExpression::Literal(t.into()),
                }));
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
        StaticAbilityAst::FollowUp(ins)
            if !ins.is_empty()
                && ins.iter().all(|op| {
                    matches!(
                        op,
                        InstructionAst::TemporaryReplacement {
                            duration: DurationAst::Permanent | DurationAst::Instant,
                            ..
                        }
                    )
                }) =>
        {
            for op in ins {
                if let InstructionAst::TemporaryReplacement { replacement, .. } = op {
                    lower_replacement(replacement, program, vocabulary, ids)?;
                }
            }
            Ok(())
        }
        StaticAbilityAst::Compound(items) => {
            let mut idx = 0usize;
            let mut prev_subject: Option<SubjectRefAst> = None;
            while idx < items.len() {
                let mut cur = items[idx].clone();
                if let Some(prev) = prev_subject.as_ref() {
                    resolve_static_anaphora_subject(&mut cur, prev);
                }
                if let Some(subj) = static_ast_subject(&cur)
                    && !matches!(subj, SubjectRefAst::Anaphora(_))
                {
                    prev_subject = Some(subj.clone());
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
                    idx += 2;
                    continue;
                }
                lower_static(&cur, program, vocabulary, ids, is_spell)?;
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
            let p = match power {
                Some(v) => Some(Box::new(lower_numeric_expr(v, vocabulary)?)),
                None => None,
            };
            let t = match toughness {
                Some(v) => Some(Box::new(lower_numeric_expr(v, vocabulary)?)),
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
                if let Some(se) = scale_expr
                    && per.is_none()
                    && condition.is_none()
                {
                    let generic_n = parse_generic_mana_amount(delta)
                        .ok_or("oracle:unsupported-self-discount-delta")?;
                    if generic_n == 0 {
                        return Err("oracle:unsupported-self-discount-delta");
                    }
                    let base_expr = lower_value_expr(se, vocabulary)?;
                    let scaled = if generic_n == 1 {
                        base_expr
                    } else {
                        scale_value_expr(base_expr, i64::from(generic_n))
                    };
                    if scaled.is_world_evaluable() {
                        program
                            .cast_reductions
                            .push(CastReduction::Dynamic(Box::new(scaled)));
                        return Ok(());
                    }
                    return Err("oracle:unsupported-self-discount");
                }
                if scale_expr.is_some() {
                    return Err("oracle:unsupported-self-discount");
                }
                let generic_n = parse_generic_mana_amount(delta)
                    .ok_or("oracle:unsupported-self-discount-delta")?;
                if generic_n == 0 {
                    return Err("oracle:unsupported-self-discount-delta");
                }
                if let Some(per_filter) = per
                    && condition.is_none()
                {
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
                            lower_filter_to_permanent_selector(per_filter, vocabulary)
                    {
                        sel.controller = ControllerSelector::SourceController;
                        for _ in 0..generic_n {
                            program
                                .cast_reductions
                                .push(CastReduction::Affinity(Box::new(sel.clone())));
                        }
                        return Ok(());
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
            // Battlefield permanent discounting spells
            if is_spell || per.is_some() {
                return Err("oracle:unsupported-cost-modifier");
            }
            let generic_n =
                parse_generic_mana_amount(delta).ok_or("oracle:non-generic-discount")?;
            let amount = NonZeroU8::new(generic_n).ok_or("oracle:zero-discount")?;
            let (any_player, matching) = lower_spell_subject_filter(subject, vocabulary)?;
            push_conditional_static(
                program,
                StaticContinuousAbility::CostModifier {
                    id: ids.static_id(),
                    modifier: SpellCostModifier::Discount {
                        any_player,
                        matching,
                        amount: SpellDiscount::Fixed(amount),
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
            scale_expr: None,
            condition,
        } if !is_spell => {
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
            let matching = lower_spell_filter(f, vocabulary)?;
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
                let matching = lower_spell_filter(filter, vocabulary)?;
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
                "cast_self_from_graveyard" | "you may cast ~ from your graveyard"
            ) {
                if let Some(cond) = condition.as_deref() {
                    let mut tmp_builder = BodyBuilder::new();
                    if let Ok(rc) =
                        lower_resolution_condition(cond, &mut tmp_builder, vocabulary, false, None)
                        && tmp_builder.target == BodyTarget::None
                        && rc.is_world_evaluable()
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
                    && matches!(program.kind, ProgramKind::Permanent(_))
                    && program.card_types.contains(&CardType::Creature)
                {
                    program.graveyard_self_cast =
                        Some(spellwake_engine::program::GraveyardSelfCast::Unconditional);
                    return Ok(());
                }
            }
            if let Some(permission) = try_lower_graveyard_matching_permission(
                &p_low, *cast_only, filter, false, vocabulary,
            ) {
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
            if let Some(permission) =
                try_lower_exile_matching_permission(&p_low, filter, vocabulary)
            {
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
            Err("oracle:unsupported-cast-permission")
        }
        StaticAbilityAst::AdditionalCastCost { cost } => match cost {
            CostAtomAst::Sacrifice { count, filter }
                if matches!(count.as_ref(), ValueExprAst::Const(1)) =>
            {
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
            CostAtomAst::Exile { count, filter }
                if filter.zone == Some(ZoneFilter::YourGraveyard) =>
            {
                match count.as_ref() {
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
                        let cnt = u8::try_from(*n)
                            .map_err(|_| "oracle:unsupported-additional-cast-cost")?;
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
                            return Err("oracle:unsupported-additional-cast-cost");
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
                        program.extra_cast_costs.push(extra);
                        Ok(())
                    }
                }
            }
            _ => {
                let extra = lower_extra_cost_atom(cost, vocabulary)
                    .map_err(|_| "oracle:unsupported-additional-cast-cost")?;
                program.extra_cast_costs.push(extra);
                Ok(())
            }
        },
        StaticAbilityAst::RuleOrCombatRestriction {
            subject,
            rule,
            filter,
            condition,
        } => {
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
                let mut body =
                    ExecutableBody::new(BodyTarget::None, vec![Instruction::SolveSourceCase]);
                if let Some(g) = guard {
                    prepend_instruction_at(
                        &mut body.instructions,
                        0,
                        Instruction::RequireCondition(g),
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
            if condition.is_none()
                && (rule.eq_ignore_ascii_case("cantbecountered")
                    || rule.eq_ignore_ascii_case("can't be countered"))
            {
                let spell_filter = filter.as_deref().or(match subject.as_deref() {
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f),
                    ) => Some(f),
                    _ => None,
                });
                match (subject.as_deref(), spell_filter) {
                    (None | Some(SubjectRefAst::SelfObject), None) => {
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
                            && f.flags.is_empty() =>
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
                            return push_static_ability(
                                program,
                                StaticContinuousAbility::CastPermission {
                                    id: ids.static_id(),
                                    permission:
                                        CastPermissionRule::ControllerSpellsCannotBeCountered(types),
                                },
                            );
                        }
                    }
                    (_, Some(f)) if f.controller == Some(ControllerRelation::You) => {
                        if let Ok(sf) = lower_spell_filter(f, vocabulary) {
                            return push_static_ability(
                                program,
                                StaticContinuousAbility::CastPermission {
                                    id: ids.static_id(),
                                    permission:
                                        CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(
                                            Box::new(sf),
                                        ),
                                },
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
                && rule == "EntersUntapped"
                && filter.is_none()
                && let Some(subj) = subject.as_deref()
                && let Ok(selectors) = lower_permanent_selectors(subj, vocabulary)
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
                && let Ok(selectors) = lower_permanent_selectors(subj, vocabulary)
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
            if condition.is_none() && rule == "DoesntUntap" && filter.is_none() {
                let untap_rule = match subject.as_deref() {
                    Some(SubjectRefAst::SelfObject) | None => StaticUntapRule::SourceSkipsUntap,
                    Some(SubjectRefAst::AttachedHost(_)) => {
                        StaticUntapRule::EnchantedCreatureSkipsUntap
                    }
                    _ => return Err("oracle:unsupported-untap-rule"),
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::UntapRule {
                        id: ids.static_id(),
                        rule: untap_rule,
                    },
                );
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
                        let selectors = lower_permanent_selectors(subj, vocabulary)?;
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
                    Some(s) => lower_permanent_selector(s, vocabulary)?,
                    None => PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    },
                };
                if let Some(cond) = condition {
                    apply_selector_condition(&mut selector, cond, vocabulary)?;
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
                    Some(s) => lower_permanent_selector(s, vocabulary)?,
                    None => PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    },
                };
                if let Some(cond) = condition {
                    apply_selector_condition(&mut selector, cond, vocabulary)?;
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
                    Some(s) => lower_permanent_selectors(s, vocabulary)?,
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
                    apply_selector_condition(&mut selector, &neg_cond, vocabulary)?;
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
            if let Some(br) = lower_rule_blocking_restriction(rule, filter.as_deref(), vocabulary) {
                let selectors = match subject.as_deref() {
                    Some(s) => lower_permanent_selectors(s, vocabulary)?,
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
                        apply_selector_condition(&mut selector, cond, vocabulary)?;
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
                && subject.is_none()
                && let Some(flt) = filter.as_deref()
            {
                let selector = lower_filter_to_permanent_selector(flt, vocabulary)?;
                return push_conditional_static(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventMatchingActivations(Box::new(selector)),
                    },
                    condition.as_deref(),
                    vocabulary,
                );
            }
            if rule.eq_ignore_ascii_case("entertriggerssuppressed") {
                let creature_only_or_none = filter.as_deref().is_none_or(|f| {
                    f.any_types == AstCardTypeMask::CREATURE
                        && f.forbidden_types.is_empty()
                        && f.subtypes.is_empty()
                        && f.any_colors.is_empty()
                        && f.forbidden_colors.is_empty()
                        && f.controller.is_none()
                        && f.stat_bounds.is_empty()
                        && f.flags.is_empty()
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
                    (_, Some(f))
                        if f.controller == Some(ControllerRelation::You)
                            && f.subtypes.is_empty()
                            && f.forbidden_types.is_empty()
                            && f.any_types.is_empty() =>
                    {
                        Some(StaticModifierRule::IgnoreLegendRuleForController)
                    }
                    (_, Some(f))
                        if f.subtypes.len() == 1
                            && f.forbidden_types.is_empty()
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
            if (rule.starts_with("CantCast:") || rule == "CantCastOrPlay" || rule == "CantBeCast")
                && let Some(flt) = filter.as_deref()
                && !flt.any_card
                && !(rule == "CantCastOrPlay" && (flt.any_types.0 & AstCardTypeMask::LAND.0 != 0))
            {
                let player_rel = if rule == "CantBeCast" && subject.is_none() {
                    Some(PlayerRelation::AnyParticipant)
                } else if let Some(SubjectRefAst::Player(p)) = subject.as_deref() {
                    match p {
                        PlayerRefAst::You => Some(PlayerRelation::Controller),
                        PlayerRefAst::EachOpponent | PlayerRefAst::AnyOpponent => {
                            Some(PlayerRelation::Opponent)
                        }
                        PlayerRefAst::EachPlayer | PlayerRefAst::AnyPlayer => {
                            Some(PlayerRelation::AnyParticipant)
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(player) = player_rel
                    && let Ok(sf) = lower_spell_filter(flt, vocabulary)
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
                    Some(s) => lower_permanent_selectors(s, vocabulary)?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for mut selector in selectors {
                    if let Some(cond) = condition {
                        apply_selector_condition(&mut selector, cond, vocabulary)?;
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
            if filter.is_some() {
                return Err("oracle:unsupported-rule-restriction");
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
            if let Some(target_src) = r_low.strip_prefix("cantbetargetof:") {
                let ts = target_src.trim();
                if ts == "spells or abilities your opponents control"
                    && matches!(
                        subject.as_deref(),
                        Some(SubjectRefAst::Player(PlayerRefAst::You))
                    )
                {
                    return push_static_ability(
                        program,
                        StaticContinuousAbility::Restriction {
                            id: ids.static_id(),
                            rule: StaticRestriction::ControllerHasHexproof,
                        },
                    );
                }
                let granted_kws: Option<BTreeSet<KeywordAbility>> = if ts == "spells or abilities" {
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
                    let selectors = match subject.as_deref() {
                        Some(s) => lower_permanent_selectors(s, vocabulary)?,
                        None => vec![PermanentSelector {
                            controller: ControllerSelector::SourceItself,
                            ..PermanentSelector::default()
                        }],
                    };
                    for selector in selectors {
                        push_static_ability(
                            program,
                            StaticContinuousAbility::Battlefield {
                                id: ids.static_id(),
                                selector,
                                parts: StaticParts(vec![StaticPart::Abilities(AbilityChange {
                                    granted_keywords: kws.clone(),
                                    ..AbilityChange::default()
                                })]),
                            },
                        )?;
                    }
                    return Ok(());
                }
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
                    Some(s) => lower_permanent_selectors(s, vocabulary)?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
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
            if r_low == "cantattackyou"
                || r_low == "can't attack you or planeswalkers you control"
                || r_low == "can't attack you or a planeswalker you control"
            {
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
                    Some(s) => lower_permanent_selectors(s, vocabulary)?,
                    None => vec![PermanentSelector {
                        controller: ControllerSelector::SourceItself,
                        ..PermanentSelector::default()
                    }],
                };
                for selector in selectors {
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
            if r_low == "activatedabilitiescantbeactivated"
                && let Some(subj) = subject.as_deref()
            {
                let selector = lower_permanent_selector(subj, vocabulary)?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventMatchingActivations(Box::new(selector)),
                    },
                );
            }
            if r_low == "can't attack or block, and its activated abilities can't be activated"
                && let Some(subj) = subject.as_deref()
            {
                let selector = lower_permanent_selector(subj, vocabulary)?;
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: selector.clone(),
                        parts: StaticParts(vec![StaticPart::BlockingRestriction(
                            BlockingRestriction::CannotAttackOrBlock,
                        )]),
                    },
                )?;
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::PreventMatchingActivations(Box::new(selector)),
                    },
                );
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
            if r_low == "you have no maximum hand size" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::DrawRule {
                        id: ids.static_id(),
                        rule: StaticDrawRule::NoMaximumHandSize,
                    },
                );
            }
            if r_low == "each player can't cast more than one spell each turn" {
                return push_static_ability(
                    program,
                    StaticContinuousAbility::Restriction {
                        id: ids.static_id(),
                        rule: StaticRestriction::OneSpellPerTurn,
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
            if matches!(
                r_low.as_str(),
                "doesn't untap during its controller's untap step"
                    | "doesn't untap during your untap step"
                    | "~ doesn't untap during your untap step"
                    | "~ doesn't untap during its controller's untap step"
                    | "enchanted creature doesn't untap during its controller's untap step"
                    | "enchanted permanent doesn't untap during its controller's untap step"
            ) {
                let untap_rule = match subject.as_deref() {
                    Some(SubjectRefAst::SelfObject) | None => StaticUntapRule::SourceSkipsUntap,
                    Some(SubjectRefAst::AttachedHost(_)) => {
                        StaticUntapRule::EnchantedCreatureSkipsUntap
                    }
                    _ => return Err("oracle:unsupported-untap-rule"),
                };
                return push_static_ability(
                    program,
                    StaticContinuousAbility::UntapRule {
                        id: ids.static_id(),
                        rule: untap_rule,
                    },
                );
            }
            if rule == "AttacksEachCombatIfAble"
                || r_low == "attacks each combat if able"
                || r_low == "attacks each turn if able"
            {
                let req = match subject.as_deref() {
                    Some(SubjectRefAst::SelfObject) | None => AttackRequirement::SourceAttacks,
                    Some(SubjectRefAst::AttachedHost(_)) => {
                        AttackRequirement::AttachedCreatureAttacks
                    }
                    Some(
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f),
                    ) if f.any_types == AstCardTypeMask::CREATURE
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                ..FilterAllow::NONE
                            },
                        ) =>
                    {
                        AttackRequirement::AllCreaturesAttack
                    }
                    _ => return Err("oracle:unsupported-attack-req"),
                };
                push_attack_requirement(program, req);
                return Ok(());
            }
            Err("oracle:unsupported-rule-restriction")
        }
        StaticAbilityAst::TriggerMultiplier {
            event_desc,
            multiplier,
            condition,
        } => {
            use spellwake_engine::characteristics::{
                TriggerMultiplierEvent, TriggerMultiplierRule,
            };
            let ev_low = event_desc.trim().to_ascii_lowercase();
            let obj_filter = if ev_low.contains("artifact or creature")
                || ev_low.contains("creature or artifact")
            {
                ObjectFilter::Either(ObjectClause::artifact(), ObjectClause::creature())
            } else if ev_low.contains("creature") {
                ObjectFilter::creature()
            } else if ev_low.contains("artifact") {
                ObjectFilter::artifact()
            } else if ev_low.contains("enchantment") {
                ObjectFilter::enchantment()
            } else if ev_low.contains("land") {
                ObjectFilter::land()
            } else {
                ObjectFilter::permanent()
            };
            let event = if ev_low.contains("enters") || ev_low.contains("entering") {
                TriggerMultiplierEvent::Enters(obj_filter)
            } else if ev_low.contains("dies") || ev_low.contains("dying") {
                TriggerMultiplierEvent::Dies(obj_filter)
            } else if ev_low.contains("leaves the battlefield") || ev_low.contains("leaving") {
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
            } else if ev_low.contains("deals damage") || ev_low.contains("combat damage") {
                TriggerMultiplierEvent::DealsDamage {
                    combat_only: ev_low.contains("combat"),
                    to_player_only: ev_low.contains("to a player"),
                }
            } else {
                TriggerMultiplierEvent::Any
            };
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
                        exclude_sacrifice_triggers: false,
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
        StaticAbilityAst::OncePerTurn(inner) => match inner.as_ref() {
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
                if let Some(perm) = try_lower_graveyard_matching_permission(
                    &p_low, *cast_only, filter, true, vocabulary,
                ) {
                    return push_conditional_static(
                        program,
                        StaticContinuousAbility::CastPermission {
                            id: ids.static_id(),
                            permission: perm,
                        },
                        condition.as_deref(),
                        vocabulary,
                    );
                }
                Err("oracle:unsupported-static")
            }
            _ => Err("oracle:unsupported-static"),
        },
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
        _ => Err("oracle:unsupported-static"),
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

fn push_conditional_static(
    program: &mut Program,
    ability: StaticContinuousAbility,
    condition: Option<&ConditionAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(), &'static str> {
    let sc = match condition {
        Some(cond) => Some(
            lower_static_source_condition(cond, vocabulary)
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

fn try_lower_graveyard_matching_permission(
    p_low: &str,
    cast_only: bool,
    filter: &ObjectFilterAst,
    once_each_turn: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<CastPermissionRule> {
    let is_gy_perm = p_low == "from your graveyard"
        || ((p_low.starts_with("you may cast ") || p_low.starts_with("you may play "))
            && p_low.ends_with(" from your graveyard")
            && !p_low.contains(" ~ ")
            && !p_low.contains("without paying")
            && !p_low.contains(" by ")
            && !p_low.contains("using its "));
    if !is_gy_perm || !matches!(filter.zone, None | Some(ZoneFilter::YourGraveyard)) {
        return None;
    }
    let mut inner = filter.clone();
    inner.zone = None;
    let obj = lower_object_filter(&inner, vocabulary).ok()?;
    Some(CastPermissionRule::GraveyardMatching {
        lands_allowed: !cast_only,
        filter: Box::new(obj),
        once_each_turn,
    })
}

fn try_lower_exile_matching_permission(
    p_low: &str,
    filter: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<CastPermissionRule> {
    let (linked_by_perm, rest) =
        if let Some(rest) = p_low.strip_prefix("from among cards exiled with ~") {
            (true, rest.trim())
        } else if let Some(rest) = p_low.strip_prefix("from among cards you own exiled with ~") {
            (true, rest.trim())
        } else {
            let rest = p_low.strip_prefix("from exile")?;
            (false, rest.trim())
        };
    if !matches!(filter.zone, None | Some(ZoneFilter::Exile)) {
        return None;
    }
    let (free, any_color) = match rest {
        "" => (false, false),
        "without paying their mana costs" | "without paying its mana cost" => (true, false),
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
        | ", and mana of any color can be spent to cast that spell" => (false, true),
        _ => return None,
    };
    let linked_to_source =
        linked_by_perm || (filter.flags.0 & AstFilterFlags::EXILED_WITH_SELF.0) != 0;
    let mut inner = filter.clone();
    inner.zone = None;
    inner.flags = AstFilterFlags(inner.flags.0 & !AstFilterFlags::EXILED_WITH_SELF.0);
    let obj = lower_object_filter(&inner, vocabulary).ok()?;
    Some(CastPermissionRule::ExileMatching {
        filter: Box::new(obj),
        linked_to_source,
        free,
        any_color,
    })
}

fn static_ast_subject(st: &StaticAbilityAst) -> Option<&SubjectRefAst> {
    match st {
        StaticAbilityAst::ContinuousModifyPT { subject, .. }
        | StaticAbilityAst::GrantKeywords { subject, .. }
        | StaticAbilityAst::GrantAbilityText { subject, .. }
        | StaticAbilityAst::SetOrAddType { subject, .. }
        | StaticAbilityAst::ContinuousLayerEffect { subject, .. } => Some(subject.as_ref()),
        StaticAbilityAst::RuleOrCombatRestriction { subject, .. } => subject.as_deref(),
        _ => None,
    }
}

fn resolve_static_anaphora_subject(st: &mut StaticAbilityAst, prev: &SubjectRefAst) {
    let slot: Option<&mut SubjectRefAst> = match st {
        StaticAbilityAst::ContinuousModifyPT { subject, .. }
        | StaticAbilityAst::GrantKeywords { subject, .. }
        | StaticAbilityAst::GrantAbilityText { subject, .. }
        | StaticAbilityAst::SetOrAddType { subject, .. }
        | StaticAbilityAst::ContinuousLayerEffect { subject, .. } => Some(subject.as_mut()),
        StaticAbilityAst::RuleOrCombatRestriction {
            subject: Some(subject),
            ..
        } => Some(subject.as_mut()),
        _ => None,
    };
    if let Some(s) = slot
        && let SubjectRefAst::Anaphora(raw) = s
    {
        let low = raw.trim().to_ascii_lowercase();
        if matches!(
            low.as_str(),
            "it" | "they"
                | "that creature"
                | "those creatures"
                | "that permanent"
                | "those permanents"
        ) {
            *s = prev.clone();
        }
    }
}

fn static_ast_condition(st: &StaticAbilityAst) -> Option<&ConditionAst> {
    match st {
        StaticAbilityAst::ContinuousModifyPT { condition, .. }
        | StaticAbilityAst::GrantKeywords { condition, .. }
        | StaticAbilityAst::GrantAbilityText { condition, .. }
        | StaticAbilityAst::SetOrAddType { condition, .. }
        | StaticAbilityAst::ContinuousLayerEffect { condition, .. }
        | StaticAbilityAst::CostModifier { condition, .. }
        | StaticAbilityAst::PlayOrCastPermission { condition, .. }
        | StaticAbilityAst::RuleOrCombatRestriction { condition, .. } => condition.as_deref(),
        _ => None,
    }
}

fn attach_ast_condition(st: &mut StaticAbilityAst, cond: ConditionAst) {
    let slot = match st {
        StaticAbilityAst::ContinuousModifyPT { condition, .. }
        | StaticAbilityAst::GrantKeywords { condition, .. }
        | StaticAbilityAst::GrantAbilityText { condition, .. }
        | StaticAbilityAst::SetOrAddType { condition, .. }
        | StaticAbilityAst::ContinuousLayerEffect { condition, .. }
        | StaticAbilityAst::CostModifier { condition, .. }
        | StaticAbilityAst::PlayOrCastPermission { condition, .. }
        | StaticAbilityAst::RuleOrCombatRestriction { condition, .. } => condition,
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
                && let Some(lost_raw) = rule.strip_prefix("Loses:")
            {
                let kws: Vec<String> = lost_raw
                    .replace(" and ", ",")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let suppressed = lower_keyword_list(&kws)?;
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
            let type_part = parse_static_type_part(type_or_subtype, *additive, vocabulary)?;
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
                        parts: StaticParts(vec![type_part.clone()]),
                    },
                )?;
            }
            Ok(())
        }
        StaticAbilityAst::GrantAbilityText {
            compiled: Some(face),
            condition: None,
            ..
        } => {
            let parts = lower_granted_ability_face_parts(face, vocabulary, ids)?;
            for sel in outer_selectors {
                for part in &parts {
                    push_static_ability(
                        program,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: sel.clone(),
                            parts: StaticParts(vec![part.clone()]),
                        },
                    )?;
                }
            }
            Ok(())
        }
        StaticAbilityAst::GrantKeywords {
            keywords,
            condition: None,
            paired_rule,
            ..
        } => {
            let ab_change = lower_ability_change(keywords, vocabulary)?;
            for sel in outer_selectors {
                push_static_ability(
                    program,
                    StaticContinuousAbility::Battlefield {
                        id: ids.static_id(),
                        selector: sel.clone(),
                        parts: StaticParts(vec![StaticPart::Abilities(ab_change.clone())]),
                    },
                )?;
            }
            if let Some(inner) = paired_rule {
                lower_paired_static(inner, outer_selectors, program, vocabulary, ids)?;
            }
            Ok(())
        }
        StaticAbilityAst::FollowUp(ins)
            if !ins.is_empty()
                && ins.iter().all(|op| {
                    matches!(
                        op,
                        InstructionAst::TemporaryReplacement {
                            duration: DurationAst::Permanent,
                            ..
                        }
                    )
                }) =>
        {
            for op in ins {
                if let InstructionAst::TemporaryReplacement { replacement, .. } = op {
                    lower_replacement(replacement, program, vocabulary, ids)?;
                }
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

fn lower_single_granted_ability(
    ability: &AbilityAst,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<StaticPart, &'static str> {
    match ability {
        AbilityAst::Activated {
            ability_word: _,
            zone: None,
            cost,
            body,
        } => {
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
                let (mana_cost, source_cost) = lower_activation_cost(cost, None, vocabulary)?;
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
                    Instruction::RequireCondition(cond),
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
        _ => Err("oracle:unsupported-grant-ability-text"),
    }
}

fn static_part_to_resolved_part_spec(part: StaticPart) -> Result<ResolvedPartSpec, &'static str> {
    match part {
        StaticPart::GrantTrigger(ability) => Ok(ResolvedPartSpec::GrantTrigger(ability)),
        StaticPart::GrantActivated(ability) => Ok(ResolvedPartSpec::GrantActivated(ability)),
        StaticPart::GrantActivatedRestricted {
            ability,
            restriction,
        } => Ok(ResolvedPartSpec::GrantActivatedRestricted {
            ability,
            restriction,
        }),
        StaticPart::GrantAnyColorMana => Ok(ResolvedPartSpec::GrantAnyColorMana),
        StaticPart::GrantFixedColorMana(color) => Ok(ResolvedPartSpec::GrantFixedColorMana(color)),
        StaticPart::GrantMana(m) => Ok(ResolvedPartSpec::GrantMana(m)),
        StaticPart::Abilities(change) => Ok(ResolvedPartSpec::Abilities(change)),
        _ => Err("oracle:unsupported-grant-ability-text"),
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
        || !compiled_face.statics.is_empty()
    {
        return Err("oracle:unsupported-grant-ability-text");
    }
    if compiled_face.keywords.is_empty() && compiled_face.abilities.len() == 1 {
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
        let ab_change = lower_ability_change(&kw_names, vocabulary)?;
        parts.push(StaticPart::Abilities(ab_change));
    }
    for ability in &compiled_face.abilities {
        parts.push(lower_single_granted_ability(ability, vocabulary, ids)?);
    }
    if parts.is_empty() {
        return Err("oracle:unsupported-grant-ability-text");
    }
    Ok(parts)
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
    let mut clean = raw
        .replace(" in addition to its other types", "")
        .replace(" in addition to their other types", "")
        .replace(" in addition to its other creature types", "")
        .replace(" in addition to their other creature types", "")
        .replace(" in addition to its other land types", "")
        .replace(" in addition to their other land types", "");
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
    for raw_tok in clean.split_whitespace() {
        let tok = raw_tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'');
        if tok.is_empty() {
            continue;
        }
        match tok.to_ascii_lowercase().as_str() {
            "a" | "an" | "the" | "type" | "types" | "is" | "are" | "becomes" | "become"
            | "also" | "still" | "other" => {}
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
                enchantment_subtypes.insert(EnchantmentSubtype::Shard);
            }
            other => {
                let singular = other.strip_suffix('s').unwrap_or(other);
                if let Some(lt) = parse_land_subtype(tok).or_else(|| {
                    let cap = {
                        let mut ch = singular.chars();
                        ch.next()
                            .map(|c| c.to_uppercase().collect::<String>() + ch.as_str())
                            .unwrap_or_default()
                    };
                    parse_land_subtype(&cap)
                }) {
                    land_subtypes.insert(lt);
                } else if let Some(ct) = vocabulary.resolve_label(tok).or_else(|| {
                    let cap = {
                        let mut ch = singular.chars();
                        ch.next()
                            .map(|c| c.to_uppercase().collect::<String>() + ch.as_str())
                            .unwrap_or_default()
                    };
                    vocabulary.resolve_label(&cap)
                }) {
                    creature_subtypes.insert(ct);
                } else {
                    return Err("oracle:unsupported-type-change");
                }
            }
        }
    }
    if !all_creature_types
        && supertypes.is_empty()
        && card_types.is_empty()
        && creature_subtypes.is_empty()
        && land_subtypes.is_empty()
        && artifact_subtypes.is_empty()
        && enchantment_subtypes.is_empty()
    {
        return Err("oracle:unsupported-type-change");
    }
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
            SubtypeOperation::Add(StaticSubtypeOperand::AllCreatureTypes)
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
        Ok(StaticPart::Type(base))
    } else {
        Ok(StaticPart::TypeWithSubtypes {
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
        })
    }
}

pub(super) fn lower_rule_blocking_restriction(
    rule: &str,
    filter: Option<&ObjectFilterAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<BlockingRestriction> {
    if let Some(flt) = filter {
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
        if let Ok(obj) = lower_object_filter(flt, vocabulary) {
            match rule {
                "CantBeBlockedBy" => {
                    return Some(BlockingRestriction::CannotBeBlockedByMatching(obj));
                }
                "CantBeBlockedExceptBy" | "CanOnlyBeBlockedBy" => {
                    return Some(BlockingRestriction::OnlyBlockedByMatching(obj));
                }
                "CantBlock" => {
                    return Some(BlockingRestriction::CannotBlockMatching(obj));
                }
                "AllAbleBlock" | "MustBeBlockedByAll" | "MustBeBlockedByAllMatching" => {
                    return Some(BlockingRestriction::MustBeBlockedByAllMatching(obj));
                }
                _ => {}
            }
        }
        return None;
    }
    lower_blocking_restriction(rule)
        .or_else(|| parse_must_be_blocked_by_all_matching(rule, vocabulary))
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
    let value = spellwake_oracle_compiler::vocab::parse_number_word(count_raw)?;
    let count = u8::try_from(value).ok()?;
    (count > 0).then_some(count)
}

fn parse_must_be_blocked_by_all_matching(
    rule: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<BlockingRestriction> {
    let norm = rule
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase()
        .replace(" this turn ", " ")
        .replace(" this combat ", " ");
    let norm = norm
        .strip_suffix(" this turn")
        .or_else(|| norm.strip_suffix(" this combat"))
        .or_else(|| norm.strip_suffix(" until end of turn"))
        .unwrap_or(&norm)
        .trim();
    if let Some(rest) = norm
        .strip_prefix("mustbeblockedbyall:")
        .or_else(|| norm.strip_prefix("allableblock:"))
    {
        let ast_f = spellwake_oracle_compiler::filter::parse_object_filter(
            rest.trim(),
            &cached_compiler_vocab(vocabulary),
        )
        .ok()?;
        let obj = lower_object_filter(&ast_f, vocabulary).ok()?;
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
    Some(BlockingRestriction::MustBeBlockedByAllMatching(obj))
}

pub(super) fn lower_combat_mod_blocking_restriction(
    rule: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<BlockingRestriction> {
    if let Some(br) = lower_blocking_restriction(rule) {
        return Some(br);
    }
    let trimmed = rule.trim().trim_end_matches('.');
    let low = trimmed
        .to_ascii_lowercase()
        .replace(" this turn ", " ")
        .replace(" this combat ", " ");
    let stripped = low
        .strip_suffix(" this turn")
        .or_else(|| low.strip_suffix(" this combat"))
        .or_else(|| low.strip_suffix(" until end of turn"))
        .unwrap_or(&low)
        .trim();
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
    if stripped == "can't block ~"
        || stripped == "can't block this creature"
        || stripped == "can't block this permanent"
    {
        return Some(BlockingRestriction::CannotBlockSource);
    }
    if stripped == "blocks ~ if able"
        || stripped == "blocks this creature if able"
        || stripped == "must block ~ if able"
        || stripped == "must block this creature if able"
        || stripped == "must block ~"
        || stripped == "must block this creature"
    {
        return Some(BlockingRestriction::MustBlockSource);
    }
    if stripped == "can't be blocked by ~"
        || stripped == "can't be blocked by this creature"
        || stripped == "can't be blocked by this permanent"
    {
        return Some(BlockingRestriction::CannotBeBlockedBySource);
    }
    if stripped == "can block an additional creature"
        || stripped == "can block an additional creature each combat"
    {
        return Some(BlockingRestriction::AdditionalBlock);
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
        if let Some(n_str) = rest
            .strip_prefix("creatures with power ")
            .and_then(|s| s.strip_suffix(" or less"))
            && let Ok(n) = n_str.trim().parse::<u16>()
        {
            return Some(BlockingRestriction::CannotBeBlockedByPowerAtMost(n));
        }
        if let Some(n_str) = rest
            .strip_prefix("creatures with power ")
            .and_then(|s| s.strip_suffix(" or greater"))
            && let Ok(n) = n_str.trim().parse::<u16>()
        {
            return Some(BlockingRestriction::CannotBeBlockedByPowerAtLeast(n));
        }
        if let Some(n_str) = rest
            .strip_prefix("creatures with toughness ")
            .and_then(|s| s.strip_suffix(" or greater"))
            && let Ok(n) = n_str.trim().parse::<u16>()
        {
            return Some(BlockingRestriction::CannotBeBlockedByToughnessAtLeast(n));
        }
        if let Some(color_word) = rest.strip_suffix(" creatures") {
            let color = match color_word.trim() {
                "white" => Some(Color::White),
                "blue" => Some(Color::Blue),
                "black" => Some(Color::Black),
                "red" => Some(Color::Red),
                "green" => Some(Color::Green),
                _ => None,
            };
            if let Some(c) = color {
                return Some(BlockingRestriction::CannotBeBlockedByColor(c));
            }
            if let Some(ct) = resolve_protection_subtype(color_word.trim(), vocabulary) {
                return Some(BlockingRestriction::CannotBeBlockedBySubtype(ct));
            }
        }
        if let Some(ct) = resolve_protection_subtype(rest, vocabulary) {
            return Some(BlockingRestriction::CannotBeBlockedBySubtype(ct));
        }
        if let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
            rest,
            &cached_compiler_vocab(vocabulary),
        ) && let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
        {
            return Some(BlockingRestriction::CannotBeBlockedByMatching(obj));
        }
    }
    if let Some(rest) = stripped
        .strip_prefix("can't be blocked except by ")
        .or_else(|| stripped.strip_prefix("cantbeblockedexceptby:"))
        .or_else(|| stripped.strip_prefix("canonlybeblockedby:"))
    {
        let rest = rest.trim();
        if rest == "creatures with flying or reach" {
            return Some(BlockingRestriction::OnlyBlockedByFlyingOrReach);
        }
        if rest == "artifact creatures" {
            return Some(BlockingRestriction::OnlyBlockedByArtifact);
        }
        if let Some(color_word) = rest.strip_suffix(" creatures") {
            let color = match color_word.trim() {
                "white" => Some(Color::White),
                "blue" => Some(Color::Blue),
                "black" => Some(Color::Black),
                "red" => Some(Color::Red),
                "green" => Some(Color::Green),
                _ => None,
            };
            if let Some(c) = color {
                return Some(BlockingRestriction::OnlyBlockedByColor(c));
            }
            if let Some(ct) = resolve_protection_subtype(color_word.trim(), vocabulary) {
                return Some(BlockingRestriction::OnlyBlockedBySubtype(ct));
            }
        }
        if let Some(ct) = resolve_protection_subtype(rest, vocabulary) {
            return Some(BlockingRestriction::OnlyBlockedBySubtype(ct));
        }
        if let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
            rest,
            &cached_compiler_vocab(vocabulary),
        ) && let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
        {
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
        if let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
            rest,
            &cached_compiler_vocab(vocabulary),
        ) && let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
        {
            return Some(BlockingRestriction::CannotBlockMatching(obj));
        }
    }
    if let Some(rest) = stripped
        .strip_prefix("can block only ")
        .or_else(|| stripped.strip_prefix("canblockonly:"))
    {
        let rest = rest.trim();
        if let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
            rest,
            &cached_compiler_vocab(vocabulary),
        ) && let Ok(obj) = lower_object_filter(&ast_f, vocabulary)
        {
            return Some(BlockingRestriction::CanBlockOnlyMatching(obj));
        }
    }
    if matches!(
        stripped,
        "can't attack you"
            | "can't attack you or planeswalkers you control"
            | "can't attack you or a planeswalker you control"
            | "cantattackyou"
    ) {
        return Some(BlockingRestriction::CannotAttackSourceController);
    }
    None
}

pub(super) fn lower_blocking_restriction(rule: &str) -> Option<BlockingRestriction> {
    let norm = rule
        .trim()
        .to_ascii_lowercase()
        .replace(" this turn ", " ")
        .replace(" this combat ", " ");
    let norm = norm
        .strip_suffix(" this turn")
        .or_else(|| norm.strip_suffix(" this combat"))
        .unwrap_or(&norm);
    match norm {
        "can't block" | "cant_block" | "cantblock" => Some(BlockingRestriction::CannotBlock),
        "can't attack" | "cant_attack" | "cantattack" => Some(BlockingRestriction::CannotAttack),
        "can't attack or block"
        | "cant_attack_or_block"
        | "cantattackorblock"
        | "cantattackorblockif"
        | "cant_attack_or_block_if" => Some(BlockingRestriction::CannotAttackOrBlock),
        "can't attack alone" | "cant_attack_alone" | "cantattackalone" => {
            Some(BlockingRestriction::CannotAttackAlone)
        }
        "can't block alone" | "cant_block_alone" | "cantblockalone" => {
            Some(BlockingRestriction::CannotBlockAlone)
        }
        "assigns combat damage equal to its toughness rather than its power"
        | "assign combat damage equal to their toughness rather than their power"
        | "assigns combat damage equal to its toughness rather than its power this turn" => {
            Some(BlockingRestriction::AssignDamageByToughness)
        }
        "can block any number of creatures" => Some(BlockingRestriction::CanBlockAnyNumber),
        "can attack as though they didn't have defender"
        | "can attack as though it didn't have defender" => {
            Some(BlockingRestriction::CanAttackAsThoughNoDefender)
        }
        "you may have ~ assign its combat damage as though it weren't blocked"
        | "you may have it assign its combat damage as though it weren't blocked"
        | "you may have them assign their combat damage as though they weren't blocked"
        | "assign its combat damage as though it weren't blocked"
        | "assign their combat damage as though they weren't blocked" => {
            Some(BlockingRestriction::AssignDamageAsThoughUnblocked)
        }
        "atmostoneblocker" | "at_most_one_blocker" => Some(BlockingRestriction::AtMostOneBlocker),
        "can't be blocked" | "cant_be_blocked" | "cantbeblocked" | "cantbeblockedif"
        | "unblockable" => Some(BlockingRestriction::CannotBeBlocked),
        "can block only creatures with flying" | "only_block_flying" => {
            Some(BlockingRestriction::CanBlockOnlyFlying)
        }
        "must be blocked if able" | "must_be_blocked" | "mustbeblocked" => {
            Some(BlockingRestriction::MustBeBlocked)
        }
        "allableblock" => Some(BlockingRestriction::AllDefendingCreaturesMustBlock),
        "minblockers:three or more creatures" => Some(BlockingRestriction::AtLeastThreeBlockers),
        "can block an additional creature each combat"
        | "additional_block"
        | "block_additional:creature each combat" => Some(BlockingRestriction::AdditionalBlock),
        other => {
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

pub(super) fn lower_keyword_list(kws: &[String]) -> Result<BTreeSet<KeywordAbility>, &'static str> {
    let mut out = BTreeSet::new();
    for k in kws {
        let ka = lower_keyword_ability_name(k).ok_or("oracle:unsupported-granted-keyword")?;
        out.insert(ka);
    }
    Ok(out)
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
        let low = raw.trim().to_ascii_lowercase();
        if let Some(ka) = lower_keyword_ability_name(&low) {
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
            let quals = parse_protection_keywords(prot, vocabulary)
                .ok_or("oracle:unsupported-granted-keyword")?;
            change.granted_keywords.extend(quals);
            continue;
        }
        match low.as_str() {
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
            "cascade" if change.granted_trigger.is_none() => {
                change.granted_trigger =
                    Some(Box::new(TriggeredAbility::cascade(TriggeredAbilityId(0))));
            }
            "extort" if change.granted_trigger.is_none() => {
                change.granted_trigger =
                    Some(Box::new(TriggeredAbility::extort(TriggeredAbilityId(0))));
            }
            "exploit" if change.granted_trigger.is_none() => {
                change.granted_trigger = Some(Box::new(TriggeredAbility {
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
                }));
            }
            "myriad" if change.granted_trigger.is_none() => {
                change.granted_trigger = Some(Box::new(TriggeredAbility {
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
                            Instruction::Myriad,
                            Instruction::ScheduleCreatedTokenExile(Step::EndCombat),
                        ],
                    ),
                }));
            }
            "evolve" if change.granted_trigger.is_none() => {
                change.granted_trigger = Some(Box::new(
                    GrowthKeyword::Evolve.ability(TriggeredAbilityId(0)),
                ));
            }
            "undying" if change.granted_trigger.is_none() => {
                change.granted_trigger = Some(Box::new(
                    CounterKeyword::Undying.ability(TriggeredAbilityId(0)),
                ));
            }
            "persist" if change.granted_trigger.is_none() => {
                change.granted_trigger = Some(Box::new(
                    CounterKeyword::Persist.ability(TriggeredAbilityId(0)),
                ));
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
                } else if let Some(n_str) = low.strip_prefix("annihilator ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n > 0
                    && change.granted_trigger.is_none()
                {
                    change.granted_trigger = Some(Box::new(TriggeredAbility {
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
                    }));
                } else if let Some(n_str) = low.strip_prefix("afterlife ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n >= 0
                    && change.granted_trigger.is_none()
                    && let Ok(tr) = spellwake_engine::program::TokenKeyword::Afterlife.ability(
                        TriggeredAbilityId(0),
                        n.into(),
                        vocabulary,
                    )
                {
                    change.granted_trigger = Some(Box::new(tr));
                } else if let Some(n_str) = low.strip_prefix("fabricate ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n >= 0
                    && change.granted_trigger.is_none()
                    && let Ok(tr) = spellwake_engine::program::TokenKeyword::Fabricate.ability(
                        TriggeredAbilityId(0),
                        n.into(),
                        vocabulary,
                    )
                {
                    change.granted_trigger = Some(Box::new(tr));
                } else if let Some(n_str) = low.strip_prefix("firebending ")
                    && let Ok(n) = n_str.trim().parse::<i64>()
                    && n > 0
                    && change.granted_trigger.is_none()
                {
                    change.granted_trigger = Some(Box::new(TriggeredAbility {
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
                            vec![Instruction::AddMana(
                                EffectManaYield::RetainedUntilEndOfCombat {
                                    kind: ManaType::Red,
                                    amount: ValueExpr::literal(n),
                                },
                            )],
                        ),
                    }));
                } else if let Some(cost_str) = low.strip_prefix("ward ")
                    && change.granted_trigger.is_none()
                    && let Some(cost) = parse_fixed_mana_cost(cost_str.trim())
                {
                    change.granted_trigger = Some(Box::new(TriggeredAbility::ward(
                        TriggeredAbilityId(0),
                        cost,
                    )));
                } else {
                    return Err("oracle:unsupported-granted-keyword");
                }
            }
        }
    }
    Ok(change)
}

pub(super) fn parse_signed_pt_delta(raw: &str) -> Option<i64> {
    let t = raw.trim().replace('\u{2212}', "-");
    let t = t.strip_prefix('+').unwrap_or(&t);
    t.parse::<i64>().ok()
}

pub(super) fn lower_pt_delta_pair(
    dx: &str,
    dy: &str,
    scale: Option<&ValueExprAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(NumericExpression, NumericExpression), &'static str> {
    let p_num = parse_signed_pt_delta(dx).ok_or("oracle:non-numeric-pt-delta")?;
    let t_num = parse_signed_pt_delta(dy).ok_or("oracle:non-numeric-pt-delta")?;
    if let Some(scale_expr) = scale {
        let base = lower_numeric_expr(scale_expr, vocabulary)?;
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
                _ => {
                    if let Ok(ve) = lower_value_expr(scale_expr, vocabulary)
                        && ve.is_world_evaluable()
                    {
                        Ok(NumericExpression::Eval(Box::new(scale_value_expr(
                            ve, coeff,
                        ))))
                    } else {
                        Err("oracle:unsupported-scaled-static-pt")
                    }
                }
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

pub(super) fn lower_numeric_expr(
    expr: &ValueExprAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<NumericExpression, &'static str> {
    match expr {
        ValueExprAst::Const(n) => Ok(NumericExpression::Literal(i64::from(*n).into())),
        ValueExprAst::Count(f) => {
            if f.zone == Some(ZoneFilter::YourGraveyard) {
                let types = lower_card_type_mask(f.any_types)?;
                if !f.forbidden_types.is_empty()
                    || !f.subtypes.is_empty()
                    || !f.any_colors.is_empty()
                    || f.color_arity.is_some()
                {
                    return Err("oracle:complex-graveyard-numeric-count");
                }
                return Ok(NumericExpression::ZoneCount {
                    zone: StaticZone::OwnGraveyard,
                    types,
                });
            }
            if f.zone == Some(ZoneFilter::YourHand)
                && f.forbidden_types.is_empty()
                && f.subtypes.is_empty()
                && f.any_colors.is_empty()
                && f.color_arity.is_none()
            {
                let types = lower_card_type_mask(f.any_types)?;
                return Ok(NumericExpression::ZoneCount {
                    zone: StaticZone::OwnHand,
                    types,
                });
            }
            let selector = lower_filter_to_permanent_selector(f, vocabulary)?;
            Ok(NumericExpression::count(selector))
        }
        ValueExprAst::Add(a, b) => {
            if let (
                Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset,
                }),
                ValueExprAst::Const(k),
            ) = (lower_numeric_expr(a, vocabulary), b.as_ref())
            {
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
            ) = (a.as_ref(), lower_numeric_expr(b, vocabulary))
            {
                return Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset: offset.plus(&i64::from(*k).into()),
                });
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
            ) = (lower_numeric_expr(a, vocabulary), b.as_ref())
            {
                return Ok(NumericExpression::Count {
                    selector,
                    multiplier,
                    offset: offset.minus(&i64::from(*k).into()),
                });
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
            ) = (lower_numeric_expr(a, vocabulary), b.as_ref())
            {
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
            ) = (a.as_ref(), lower_numeric_expr(b, vocabulary))
            {
                let k_ri: RulesInteger = i64::from(*k).into();
                return Ok(NumericExpression::Count {
                    selector,
                    multiplier: multiplier.times(&k_ri),
                    offset: offset.times(&k_ri),
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
        ValueExprAst::CounterCount {
            subject,
            counter_kind,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let kind = parse_counter_kind(counter_kind).ok_or("oracle:unsupported-counter-kind")?;
            Ok(NumericExpression::Counters { source: None, kind })
        }
        ValueExprAst::Devotion { colors } => {
            let color = match colors.to_ascii_lowercase().as_str() {
                "white" => Color::White,
                "blue" => Color::Blue,
                "black" => Color::Black,
                "red" => Color::Red,
                "green" => Color::Green,
                _ => return Err("oracle:multi-color-devotion"),
            };
            Ok(NumericExpression::PlayerStat(StaticPlayerStat::Devotion(
                color,
            )))
        }
        _ => {
            if let Ok(ve) = lower_value_expr(expr, vocabulary)
                && ve.is_world_evaluable()
            {
                Ok(NumericExpression::Eval(Box::new(ve)))
            } else {
                Err("oracle:unsupported-numeric-expr")
            }
        }
    }
}
