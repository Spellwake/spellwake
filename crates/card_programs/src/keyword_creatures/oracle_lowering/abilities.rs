use super::*;

pub(super) fn lower_ability(
    ab: &AbilityAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    match ab {
        AbilityAst::Activated {
            ability_word: _,
            zone,
            cost,
            body,
        } => {
            // Try lowering as a ManaAbility first (battlefield, untargeted AddMana)
            if zone.is_none()
                && let Some(mana_out) = try_lower_mana_ability(cost, body, vocabulary)
            {
                program.mana.push(ManaAbility {
                    id: ids.mana(),
                    output: mana_out,
                });
                return Ok(());
            }
            let (filtered_ins, mut restriction) = strip_activated_restrictions(body, vocabulary)?;
            let effective_zone = zone
                .as_deref()
                .or_else(|| body_returns_self_from_graveyard(&filtered_ins).then_some("graveyard"));
            let (mana_cost, source_cost, hybrids, extra_costs) =
                lower_activation_cost_full(cost, effective_zone, vocabulary)?;
            let anaphora_ref = if extra_costs.is_empty() {
                match source_cost {
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
                }
            } else {
                ValueObjectRef::AbilitySource
            };
            if let Some((pre, choice, modes, post, allow_repeat)) =
                extract_modal_with_wrap(&filtered_ins)
                && let Ok((min, raw_max, uniqueness)) = parse_modal_choice_with_uniqueness(choice)
                && modes.len() >= 2
            {
                let max = if allow_repeat {
                    raw_max
                } else {
                    raw_max.min(u8::try_from(modes.len()).unwrap_or(u8::MAX))
                };
                let mut mode_bodies = Vec::with_capacity(modes.len());
                for m in modes {
                    let (stripped, mode_res) = strip_activated_restrictions(&m.body, vocabulary)?;
                    if let Some(r) = mode_res {
                        restriction = Some(combine_activation_restrictions(restriction, r));
                    }
                    mode_bodies.push(stripped);
                }
                let allows_modal_split = (min, max) == (1, 1)
                    && !allow_repeat
                    && uniqueness == ModalUniqueness::None
                    && !restriction
                        .as_ref()
                        .is_some_and(|r| r.has_once_each_turn() || r.has_exhaust());
                if allows_modal_split {
                    for mb in &mode_bodies {
                        let mut full = Vec::with_capacity(pre.len() + mb.len() + post.len());
                        full.extend_from_slice(pre);
                        full.extend_from_slice(mb);
                        full.extend_from_slice(post);
                        let mut exec_body = lower_effect_body(&full, vocabulary, ids, false, true)?;
                        rewrite_body_anaphora_stats(&mut exec_body, anaphora_ref);
                        let id = ids.activated();
                        if let Some(res) = restriction.clone() {
                            program.activation_restrictions.insert(id, res);
                        }
                        if !hybrids.is_empty() {
                            program
                                .activation_hybrid_symbols
                                .insert(id, hybrids.clone());
                        }
                        if !extra_costs.is_empty() {
                            program
                                .extra_activation_costs
                                .insert(id, extra_costs.clone());
                        }
                        program.activated.push(ActivatedAbility {
                            id,
                            mana_cost: mana_cost.clone(),
                            source_cost,
                            body: exec_body,
                        });
                    }
                    return Ok(());
                }
                let prefix = if pre.is_empty() {
                    None
                } else {
                    let mut pb = lower_effect_body(pre, vocabulary, ids, false, true)?;
                    rewrite_body_anaphora_stats(&mut pb, anaphora_ref);
                    if pb.target != BodyTarget::None {
                        return Err("oracle:modal-prefix-target");
                    }
                    Some(Box::new(pb))
                };
                let suffix = if post.is_empty() {
                    None
                } else {
                    let mut sb = lower_effect_body(post, vocabulary, ids, false, true)?;
                    rewrite_body_anaphora_stats(&mut sb, anaphora_ref);
                    if sb.target != BodyTarget::None {
                        return Err("oracle:modal-suffix-target");
                    }
                    Some(Box::new(sb))
                };
                let mut lowered_modes = Vec::with_capacity(mode_bodies.len());
                for mb in &mode_bodies {
                    let mut exec_body = lower_effect_body(mb, vocabulary, ids, false, true)?;
                    rewrite_body_anaphora_stats(&mut exec_body, anaphora_ref);
                    lowered_modes.push(exec_body);
                }
                let first = lowered_modes.remove(0);
                let id = ids.activated();
                if let Some(res) = restriction {
                    program.activation_restrictions.insert(id, res);
                }
                if !hybrids.is_empty() {
                    program.activation_hybrid_symbols.insert(id, hybrids);
                }
                if !extra_costs.is_empty() {
                    program.extra_activation_costs.insert(id, extra_costs);
                }
                program.activation_modes.insert(
                    id,
                    SpellModes {
                        minimum: min,
                        maximum: max,
                        conditional_maximum: None,
                        allow_repeat,
                        distinct_targets: false,
                        uniqueness,
                        mode_costs: Vec::new(),
                        escalate_mana: None,
                        prefix,
                        suffix,
                        additional: lowered_modes,
                    },
                );
                program.activated.push(ActivatedAbility {
                    id,
                    mana_cost,
                    source_cost,
                    body: first,
                });
                return Ok(());
            }
            let mut exec_body = lower_effect_body(&filtered_ins, vocabulary, ids, false, true)?;
            rewrite_body_anaphora_stats(&mut exec_body, anaphora_ref);
            let id = ids.activated();
            if !hybrids.is_empty() {
                program.activation_hybrid_symbols.insert(id, hybrids);
            }
            if !extra_costs.is_empty() {
                program.extra_activation_costs.insert(id, extra_costs);
            }
            if let Some(res) = restriction {
                program.activation_restrictions.insert(id, res);
            }
            program.activated.push(ActivatedAbility {
                id,
                mana_cost,
                source_cost,
                body: exec_body,
            });
            Ok(())
        }
        AbilityAst::Triggered {
            ability_word: _,
            trigger,
            body,
        } => {
            let (mut spec, mut grouping, guard) = lower_trigger_spec(trigger, vocabulary)?;
            if spec.zone == spellwake_engine::program::TriggerZone::Battlefield
                && (body_returns_self_from_graveyard(body)
                    || matches!(
                        spec.event,
                        TriggerEvent::PlayerAction {
                            action: PlayerTriggerAction::DiscardedByOpponent(Some(filter)),
                            ..
                        } if filter == ObjectFilter::self_object()
                    )
                    || (matches!(program.kind, ProgramKind::Spell(_))
                        && (body_references_self_from_graveyard(body)
                            || matches!(
                                spec.event,
                                TriggerEvent::PlayerAction {
                                    action: PlayerTriggerAction::Discarded
                                        | PlayerTriggerAction::DiscardedMatching(_)
                                        | PlayerTriggerAction::DiscardedByOpponent(_),
                                    ..
                                }
                            )
                            || matches!(
                                &spec.event,
                                TriggerEvent::PutIntoGraveyard { subject, .. }
                                    if *subject == ObjectFilter::self_object()
                            ))))
            {
                spec.zone = spellwake_engine::program::TriggerZone::Graveyard;
            }
            let self_pronoun = trigger_subject_is_self(trigger);
            let stripped_body =
                strip_triggered_restrictions(body, &mut spec, &mut grouping, vocabulary);
            let body = &stripped_body;
            let mut exec_body = if let Some((pre, choice, modes, post, allow_repeat)) =
                extract_modal_with_wrap(body)
            {
                let (min, raw_max, uniqueness) = parse_modal_choice_with_uniqueness(choice)?;
                if modes.len() < 2 {
                    return Err("oracle:modal-too-few");
                }
                let max = if allow_repeat {
                    raw_max
                } else {
                    raw_max.min(u8::try_from(modes.len()).unwrap_or(u8::MAX))
                };
                let (prefix, suffix, embed_wrap) = if pre.is_empty() && post.is_empty() {
                    (None, None, false)
                } else if (min, max) == (1, 1) && !allow_repeat {
                    (None, None, true)
                } else {
                    let pb = if pre.is_empty() {
                        None
                    } else {
                        let b =
                            lower_trigger_effect_body(pre, vocabulary, ids, &spec, self_pronoun)?;
                        if b.target != BodyTarget::None {
                            return Err("oracle:modal-prefix-target");
                        }
                        Some(Box::new(b))
                    };
                    let sb = if post.is_empty() {
                        None
                    } else {
                        let b =
                            lower_trigger_effect_body(post, vocabulary, ids, &spec, self_pronoun)?;
                        if b.target != BodyTarget::None {
                            return Err("oracle:modal-suffix-target");
                        }
                        Some(Box::new(b))
                    };
                    (pb, sb, false)
                };
                let mut options = Vec::with_capacity(modes.len());
                for m in modes {
                    let m_stripped =
                        strip_triggered_restrictions(&m.body, &mut spec, &mut grouping, vocabulary);
                    let full = if embed_wrap {
                        let mut f = Vec::with_capacity(pre.len() + m_stripped.len() + post.len());
                        f.extend_from_slice(pre);
                        f.extend(m_stripped);
                        f.extend_from_slice(post);
                        f
                    } else {
                        m_stripped
                    };
                    let mut opt_body =
                        lower_trigger_effect_body(&full, vocabulary, ids, &spec, self_pronoun)?;
                    if let Some(cond) = guard {
                        prepend_instruction_at(
                            &mut opt_body.instructions,
                            0,
                            Instruction::RequireCondition(cond),
                        );
                    }
                    options.push(opt_body);
                }
                ExecutableBody::with_triggered_modes(TriggeredModes {
                    minimum: min,
                    maximum: max,
                    distinct_targets: false,
                    allow_repeat,
                    uniqueness,
                    prefix,
                    suffix,
                    options,
                })
            } else {
                let mut b = lower_trigger_effect_body(body, vocabulary, ids, &spec, self_pronoun)?;
                if let Some(cond) = guard {
                    prepend_instruction_at(
                        &mut b.instructions,
                        0,
                        Instruction::RequireCondition(cond),
                    );
                }
                b
            };
            apply_trigger_body_rewrites(&mut exec_body, &spec, grouping, self_pronoun);
            program.triggered.push(TriggeredAbility {
                id: ids.triggered(),
                condition: spec,
                grouping,
                body: exec_body,
            });
            Ok(())
        }
        AbilityAst::Loyalty { delta, body } => {
            let trimmed = delta.trim().replace('\u{2212}', "-");
            let parsed_delta: Option<i32> = if trimmed.eq_ignore_ascii_case("-x") {
                None
            } else {
                let n: i32 = trimmed
                    .strip_prefix('+')
                    .unwrap_or(&trimmed)
                    .parse()
                    .map_err(|_| "oracle:bad-loyalty-delta")?;
                Some(n)
            };
            let (loyalty_filtered, loyalty_res) = strip_activated_restrictions(body, vocabulary)?;
            if let Some((pre, choice, modes, post, allow_repeat)) =
                extract_modal_with_wrap(&loyalty_filtered)
                && let Ok((min, raw_max, uniqueness)) = parse_modal_choice_with_uniqueness(choice)
                && modes.len() >= 2
            {
                let max = if allow_repeat {
                    raw_max
                } else {
                    raw_max.min(u8::try_from(modes.len()).unwrap_or(u8::MAX))
                };
                if (min, max) == (1, 1) && !allow_repeat && uniqueness == ModalUniqueness::None {
                    for m in modes {
                        let (m_filtered, m_res) =
                            strip_activated_restrictions(&m.body, vocabulary)?;
                        let combined_res = match (loyalty_res.clone(), m_res) {
                            (a, Some(b)) => Some(combine_activation_restrictions(a, b)),
                            (a, None) => a,
                        };
                        let mut full =
                            Vec::with_capacity(pre.len() + m_filtered.len() + post.len());
                        full.extend_from_slice(pre);
                        full.extend(m_filtered);
                        full.extend_from_slice(post);
                        let id = ids.activated();
                        if let Some(n) = parsed_delta {
                            program.loyalty_costs.insert(id, n);
                        } else {
                            program.loyalty_costs.insert(id, 0);
                            program.variable_loyalty_costs.insert(id);
                        }
                        if let Some(res) = combined_res {
                            program.activation_restrictions.insert(id, res);
                        }
                        let exec_body = lower_effect_body(&full, vocabulary, ids, false, true)?;
                        program.activated.push(ActivatedAbility {
                            id,
                            mana_cost: ManaCost::free(),
                            source_cost: SourceCost::None,
                            body: exec_body,
                        });
                    }
                    return Ok(());
                }
                let prefix = if pre.is_empty() {
                    None
                } else {
                    let pb = lower_effect_body(pre, vocabulary, ids, false, true)?;
                    if pb.target != BodyTarget::None {
                        return Err("oracle:modal-prefix-target");
                    }
                    Some(Box::new(pb))
                };
                let suffix = if post.is_empty() {
                    None
                } else {
                    let sb = lower_effect_body(post, vocabulary, ids, false, true)?;
                    if sb.target != BodyTarget::None {
                        return Err("oracle:modal-suffix-target");
                    }
                    Some(Box::new(sb))
                };
                let mut combined_res = loyalty_res.clone();
                let mut lowered_modes = Vec::with_capacity(modes.len());
                for m in modes {
                    let (m_filtered, m_res) = strip_activated_restrictions(&m.body, vocabulary)?;
                    if let Some(r) = m_res {
                        combined_res = Some(combine_activation_restrictions(combined_res, r));
                    }
                    lowered_modes.push(lower_effect_body(
                        &m_filtered,
                        vocabulary,
                        ids,
                        false,
                        true,
                    )?);
                }
                let first = lowered_modes.remove(0);
                let id = ids.activated();
                if let Some(n) = parsed_delta {
                    program.loyalty_costs.insert(id, n);
                } else {
                    program.loyalty_costs.insert(id, 0);
                    program.variable_loyalty_costs.insert(id);
                }
                if let Some(res) = combined_res {
                    program.activation_restrictions.insert(id, res);
                }
                program.activation_modes.insert(
                    id,
                    SpellModes {
                        minimum: min,
                        maximum: max,
                        conditional_maximum: None,
                        allow_repeat,
                        distinct_targets: false,
                        uniqueness,
                        mode_costs: Vec::new(),
                        escalate_mana: None,
                        prefix,
                        suffix,
                        additional: lowered_modes,
                    },
                );
                program.activated.push(ActivatedAbility {
                    id,
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::None,
                    body: first,
                });
                return Ok(());
            }
            let id = ids.activated();
            if let Some(n) = parsed_delta {
                program.loyalty_costs.insert(id, n);
            } else {
                program.loyalty_costs.insert(id, 0);
                program.variable_loyalty_costs.insert(id);
            }
            if let Some(res) = loyalty_res {
                program.activation_restrictions.insert(id, res);
            }
            let exec_body = lower_effect_body(&loyalty_filtered, vocabulary, ids, false, true)?;
            program.activated.push(ActivatedAbility {
                id,
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body: exec_body,
            });
            Ok(())
        }
        AbilityAst::SagaChapter { chapters, body } => {
            if chapters.is_empty() {
                return Err("oracle:empty-saga-chapter");
            }
            let mut chapter_nums = Vec::with_capacity(chapters.len());
            for ch in chapters {
                let num: u8 = match ch.trim() {
                    "I" | "1" => 1,
                    "II" | "2" => 2,
                    "III" | "3" => 3,
                    "IV" | "4" => 4,
                    "V" | "5" => 5,
                    "VI" | "6" => 6,
                    _ => return Err("oracle:unsupported-saga-chapter"),
                };
                chapter_nums.push(num);
            }
            let mut spec_template = TriggerSpec::counters_changed(
                ObjectFilter::self_object(),
                CounterTriggerKind::SagaChapter(chapter_nums[0]),
            );
            let mut grouping = TriggerGrouping::EachOccurrence;
            let stripped_body =
                strip_triggered_restrictions(body, &mut spec_template, &mut grouping, vocabulary);
            let body = &stripped_body;
            let mut exec_body = if let Some((pre, choice, modes, post, allow_repeat)) =
                extract_modal_with_wrap(body)
            {
                let (min, raw_max, uniqueness) = parse_modal_choice_with_uniqueness(choice)?;
                if modes.len() < 2 {
                    return Err("oracle:modal-too-few");
                }
                let max = if allow_repeat {
                    raw_max
                } else {
                    raw_max.min(u8::try_from(modes.len()).unwrap_or(u8::MAX))
                };
                let (prefix, suffix, embed_wrap) = if pre.is_empty() && post.is_empty() {
                    (None, None, false)
                } else if (min, max) == (1, 1) && !allow_repeat {
                    (None, None, true)
                } else {
                    let pb = if pre.is_empty() {
                        None
                    } else {
                        let b = lower_effect_body(pre, vocabulary, ids, true, true)?;
                        if b.target != BodyTarget::None {
                            return Err("oracle:modal-prefix-target");
                        }
                        Some(Box::new(b))
                    };
                    let sb = if post.is_empty() {
                        None
                    } else {
                        let b = lower_effect_body(post, vocabulary, ids, true, true)?;
                        if b.target != BodyTarget::None {
                            return Err("oracle:modal-suffix-target");
                        }
                        Some(Box::new(b))
                    };
                    (pb, sb, false)
                };
                let mut mode_bodies = Vec::with_capacity(modes.len());
                for m in modes {
                    let m_stripped = strip_triggered_restrictions(
                        &m.body,
                        &mut spec_template,
                        &mut grouping,
                        vocabulary,
                    );
                    let full = if embed_wrap {
                        let mut f = Vec::with_capacity(pre.len() + m_stripped.len() + post.len());
                        f.extend_from_slice(pre);
                        f.extend(m_stripped);
                        f.extend_from_slice(post);
                        f
                    } else {
                        m_stripped
                    };
                    mode_bodies.push(full);
                }
                let guard = spec_template
                    .intervening_if
                    .as_ref()
                    .and_then(InterveningCondition::from_trigger_condition);
                let mut options = Vec::with_capacity(mode_bodies.len());
                for full in &mode_bodies {
                    let mut opt_body = lower_effect_body(full, vocabulary, ids, true, true)?;
                    if let Some(cond) = guard {
                        prepend_instruction_at(
                            &mut opt_body.instructions,
                            0,
                            Instruction::RequireCondition(cond),
                        );
                    }
                    options.push(opt_body);
                }
                ExecutableBody::with_triggered_modes(TriggeredModes {
                    minimum: min,
                    maximum: max,
                    distinct_targets: false,
                    allow_repeat,
                    uniqueness,
                    prefix,
                    suffix,
                    options,
                })
            } else {
                let guard = spec_template
                    .intervening_if
                    .as_ref()
                    .and_then(InterveningCondition::from_trigger_condition);
                let mut b = lower_effect_body(body, vocabulary, ids, true, true)?;
                if let Some(cond) = guard {
                    prepend_instruction_at(
                        &mut b.instructions,
                        0,
                        Instruction::RequireCondition(cond),
                    );
                }
                b
            };
            apply_trigger_body_rewrites(&mut exec_body, &spec_template, grouping, true);
            for num in chapter_nums {
                let mut cond = spec_template.clone();
                cond.event = TriggerEvent::CountersChanged {
                    subject: ObjectFilter::self_object(),
                    kind: CounterTriggerKind::SagaChapter(num),
                };
                program.triggered.push(TriggeredAbility {
                    id: ids.triggered(),
                    condition: cond,
                    grouping,
                    body: exec_body.clone(),
                });
            }
            Ok(())
        }
        AbilityAst::ClassLevel { .. } => Err("oracle:unsupported-ability-kind"),
    }
}

pub(super) fn try_lower_mana_ability(
    cost: &[CostAtomAst],
    body: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ManaOutput> {
    fn restriction_has_any_player(r: &ActivationRestriction) -> bool {
        match r {
            ActivationRestriction::AnyPlayer => true,
            ActivationRestriction::All(items) => items.iter().any(restriction_has_any_player),
            _ => false,
        }
    }
    let (filtered_body, act_restriction) = strip_activated_restrictions(body, vocabulary).ok()?;
    if act_restriction
        .as_ref()
        .is_some_and(restriction_has_any_player)
    {
        return None;
    }
    // Extract AddMana + optional SpendRestriction / SpendEffect + optional Life/Damage side effect
    let mut mana_spec: Option<&ManaSpecAst> = None;
    let mut restriction: Option<spellwake_engine::mana::ManaOutputRestriction> = None;
    let mut spend_effect: Option<spellwake_engine::mana::ManaSpendEffect> = None;
    let mut side_effect: Option<ManaSideEffect> = None;
    for ins in &filtered_body {
        match ins {
            InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana,
            } => {
                if mana_spec.is_some() {
                    return None;
                }
                mana_spec = Some(mana);
            }
            InstructionAst::RetainManaUntilEndOfTurn => {
                if spend_effect.is_some() {
                    return None;
                }
                spend_effect = Some(spellwake_engine::mana::ManaSpendEffect::RetainUntilCleanup);
            }
            InstructionAst::ManaSpendRestriction(r) => {
                if let Some(effect) = parse_mana_spend_effect(r) {
                    if spend_effect.is_some() {
                        return None;
                    }
                    spend_effect = Some(effect);
                } else {
                    if restriction.is_some() {
                        return None;
                    }
                    if let Some(scope) = parse_chosen_type_mana_scope(r) {
                        restriction = Some(
                            spellwake_engine::mana::ManaOutputRestriction::ChosenCreatureType(
                                scope,
                            ),
                        );
                    } else {
                        restriction = Some(spellwake_engine::mana::ManaOutputRestriction::Fixed(
                            parse_mana_restriction(r, vocabulary)?,
                        ));
                    }
                }
            }
            InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                delta,
                gain: true,
            } => {
                let ValueExprAst::Const(n) = delta.as_ref() else {
                    return None;
                };
                if *n <= 0 || side_effect.is_some() {
                    return None;
                }
                side_effect = Some(ManaSideEffect::GainLife(i64::from(*n).into()));
            }
            InstructionAst::DealDamage {
                source,
                target,
                amount,
            } if matches!(source.as_ref(), SubjectRefAst::SelfObject)
                && matches!(target.as_ref(), SubjectRefAst::Player(PlayerRefAst::You)) =>
            {
                let ValueExprAst::Const(n) = amount.as_ref() else {
                    return None;
                };
                if *n <= 0 || side_effect.is_some() {
                    return None;
                }
                side_effect = Some(ManaSideEffect::DamageController(i64::from(*n).into()));
            }
            _ => return None,
        }
    }
    let spec = mana_spec?;
    let mut output = match spec {
        ManaSpecAst::Fixed(raw) => ManaOutput::Fixed(parse_mana_bundle(raw)?),
        ManaSpecAst::Choice(opts) => {
            let mut colors = BTreeSet::new();
            for o in opts {
                let mt = parse_single_mana_type(o)?;
                if mt == ManaType::Colourless {
                    return None;
                }
                colors.insert(mt);
            }
            if colors.is_empty() {
                return None;
            }
            ManaOutput::ChooseColor {
                amount: 1.into(),
                colors,
            }
        }
        ManaSpecAst::AnyColor {
            amount,
            single_color: true,
        } => {
            let ValueExprAst::Const(n) = amount.as_ref() else {
                return None;
            };
            if *n <= 0 {
                return None;
            }
            ManaOutput::ChooseColor {
                amount: i64::from(*n).into(),
                colors: ManaOutput::all_colors(),
            }
        }
        ManaSpecAst::DifferentColors { amount }
            if matches!(amount.as_ref(), ValueExprAst::Const(2)) =>
        {
            ManaOutput::TwoColors { distinct: true }
        }
        ManaSpecAst::ScaledSymbol { symbol, amount } => {
            let mt = parse_single_mana_type(symbol)?;
            let ValueExprAst::Count(filter) = amount.as_ref() else {
                return None;
            };
            let permanents = lower_filter_to_permanent_selector(filter, vocabulary).ok()?;
            ManaOutput::Counted {
                mana_type: Some(mt),
                permanents,
            }
        }
        ManaSpecAst::CommanderColorIdentity => {
            ManaOutput::SpecialColor(SpecialColorYield::CommanderColorIdentity)
        }
        ManaSpecAst::AnyLandCouldProduce {
            controller: ControllerRelation::Opponent | ControllerRelation::Opponents,
        } => ManaOutput::SpecialColor(SpecialColorYield::OpponentsLandColors),
        _ => return None,
    };

    // Parse cost atoms for mana ability
    let mut has_tap = false;
    let mut pay_mana: Option<ManaCost> = None;
    let mut sac_self = false;
    let mut sac_creature = false;
    let mut pay_life: Option<i64> = None;
    for atom in cost {
        match atom {
            CostAtomAst::TapSelf => {
                if has_tap {
                    return None;
                }
                has_tap = true;
            }
            CostAtomAst::PayMana(raw) => {
                if pay_mana.is_some() {
                    return None;
                }
                pay_mana = Some(parse_fixed_mana_cost(raw)?);
            }
            CostAtomAst::SacrificeSelf => {
                if sac_self {
                    return None;
                }
                sac_self = true;
            }
            CostAtomAst::Sacrifice {
                count,
                filter,
                up_to: false,
                min_one: false,
            } if matches!(count.as_ref(), ValueExprAst::Const(1))
                && filter.any_types == AstCardTypeMask::CREATURE
                && filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        controller: true,
                        ..FilterAllow::NONE
                    },
                )
                && matches!(filter.controller, None | Some(ControllerRelation::You)) =>
            {
                if sac_creature {
                    return None;
                }
                sac_creature = true;
            }
            CostAtomAst::PayLife(expr) => {
                let ValueExprAst::Const(n) = expr.as_ref() else {
                    return None;
                };
                if *n <= 0 || pay_life.is_some() {
                    return None;
                }
                pay_life = Some(i64::from(*n));
            }
            _ => return None,
        }
    }

    if pay_life.is_some() {
        // Engine ManaSideEffect models damage-to-controller, not pay-life cost;
        // let paid-life mana abilities fall back cleanly unless exact.
        return None;
    }
    if (sac_self || sac_creature)
        && (pay_mana.is_some() || side_effect.is_some() || spend_effect.is_some())
    {
        // Engine ManaOutput::valid_for_sacrifice_inner and mana_activation reject combining
        // ManaOutputModifier::Sacrifice with PayMana, SideEffect, or SpendEffect; fall back to ActivatedAbility.
        return None;
    }
    if let Some(ref se) = side_effect {
        // Engine mana_activation applies GainLife only on Fixed yields and DamageController
        // only on ChooseColor yields; fall back to ActivatedAbility otherwise so the side
        // effect is never dropped at runtime.
        let side_effect_supported = match se {
            ManaSideEffect::GainLife(_) => matches!(output.base(), ManaOutput::Fixed(_)),
            ManaSideEffect::DamageController(_) => {
                matches!(output.base(), ManaOutput::ChooseColor { .. })
            }
        };
        if !side_effect_supported {
            return None;
        }
    }
    if let Some(ref mc) = pay_mana
        && !mc.total().is_positive()
        && !matches!(
            output.base(),
            ManaOutput::Dynamic(spellwake_engine::mana::DynamicManaYield::GreatestCreaturePower)
                | ManaOutput::SpecialColor(SpecialColorYield::CommanderColorIdentity)
        )
    {
        return None;
    }

    if let Some(res) = restriction {
        output = output.with_modifier(ManaOutputModifier::Restrict(res));
    }
    if let Some(se) = spend_effect {
        output = output.with_modifier(ManaOutputModifier::SpendEffect(se));
    }
    if let Some(se) = side_effect {
        output = output.with_modifier(ManaOutputModifier::SideEffect(se));
    }
    if let Some(mc) = pay_mana {
        output = output.paid(mc);
    }
    let valid_sacrifice_inner = |out: &ManaOutput| -> bool {
        matches!(out, ManaOutput::Fixed(_) | ManaOutput::ChooseColor { .. })
            || matches!(
                out,
                ManaOutput::Modified {
                    modifier: ManaOutputModifier::Restrict(
                        spellwake_engine::mana::ManaOutputRestriction::Fixed(_)
                    ),
                    output: inner,
                } if matches!(inner.as_ref(), ManaOutput::Fixed(_) | ManaOutput::ChooseColor { .. })
            )
    };
    if sac_self {
        if !valid_sacrifice_inner(&output) {
            return None;
        }
        output = output.with_modifier(ManaOutputModifier::Sacrifice(ManaSacrificeCost::Source {
            tap: has_tap,
        }));
    } else if sac_creature && !has_tap {
        if !valid_sacrifice_inner(&output) {
            return None;
        }
        output = output.with_modifier(ManaOutputModifier::Sacrifice(
            ManaSacrificeCost::ControlledCreature,
        ));
    } else if !has_tap {
        return None;
    }
    if let Some(cond) = act_restriction {
        output = output.with_activation_condition(cond);
    }
    Some(output)
}

fn normalize_mana_clause(raw: &str) -> String {
    let mut trimmed = raw.trim().to_ascii_lowercase();
    loop {
        let mut next = trimmed.trim().trim_matches('"').trim();
        if !next.contains('(') {
            next = next.trim_end_matches(')');
        }
        next = next
            .trim_end_matches('.')
            .trim_end_matches('"')
            .trim_end_matches('.')
            .trim();
        if !next.contains('(') {
            next = next.trim_end_matches(')').trim();
        }
        if next.len() == trimmed.len() {
            break;
        }
        trimmed = next.to_owned();
    }
    if let Some(rest) = trimmed
        .strip_prefix("you may spend this mana only ")
        .or_else(|| trimmed.strip_prefix("you can spend this mana only "))
        .or_else(|| trimmed.strip_prefix("this mana can be spent only "))
        .or_else(|| trimmed.strip_prefix("this mana can only be spent "))
    {
        format!("spend this mana only {rest}")
    } else {
        trimmed
    }
}

fn parse_mana_spend_effect(raw: &str) -> Option<spellwake_engine::mana::ManaSpendEffect> {
    use spellwake_engine::mana::ManaSpendEffect;
    match normalize_mana_clause(raw).as_str() {
        "you don't lose this mana as steps and phases end"
        | "until end of turn, you don't lose this mana as steps and phases end"
        | "you don't lose this mana as steps and phases end until end of turn"
        | "this mana doesn't empty from your mana pool as steps and phases end"
        | "until end of turn, this mana doesn't empty from your mana pool as steps and phases end" => {
            Some(ManaSpendEffect::RetainUntilCleanup)
        }
        "until end of combat, you don't lose this mana as steps and phases end"
        | "you don't lose this mana as steps and phases end until end of combat" => {
            Some(ManaSpendEffect::RetainUntilEndOfCombat)
        }
        _ => None,
    }
}

fn parse_chosen_type_mana_scope(raw: &str) -> Option<spellwake_engine::mana::ChosenTypeManaScope> {
    use spellwake_engine::mana::ChosenTypeManaScope;
    let low = normalize_mana_clause(raw);
    if !low.contains("of the chosen type") {
        return None;
    }
    if low.contains("can't be countered") {
        Some(ChosenTypeManaScope::CreatureSpellUncounterable)
    } else if low.contains("activate") {
        Some(ChosenTypeManaScope::CreatureSpellOrAbility)
    } else if low.contains("cast") {
        Some(ChosenTypeManaScope::CreatureSpell)
    } else {
        None
    }
}

pub(super) fn parse_mana_restriction(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ManaRestriction> {
    let low = normalize_mana_clause(raw);
    match low.as_str() {
        "spend this mana only to cast a creature spell"
        | "spend this mana only to cast creature spells" => Some(ManaRestriction::CreatureSpell),
        "spend this mana only to cast a noncreature spell"
        | "spend this mana only to cast noncreature spells" => {
            Some(ManaRestriction::NoncreatureSpell)
        }
        "spend this mana only to cast an artifact spell or activate an ability of an artifact"
        | "spend this mana only to cast an artifact spell or to activate an ability of an artifact"
        | "spend this mana only to cast an artifact spell or activate an ability of an artifact source"
        | "spend this mana only to cast an artifact spell or to activate an ability of an artifact source"
        | "spend this mana only to cast artifact spells or activate abilities of artifacts"
        | "spend this mana only to cast artifact spells or to activate abilities of artifacts"
        | "spend this mana only to cast artifact spells or activate abilities of artifact sources"
        | "spend this mana only to cast artifact spells or to activate abilities of artifact sources"
        | "spend this mana only to cast artifact spells and activate abilities of artifacts"
        | "spend this mana only to cast artifact spells and to activate abilities of artifacts"
        | "spend this mana only to cast artifact spells and activate abilities of artifact sources"
        | "spend this mana only to cast artifact spells and to activate abilities of artifact sources"
        | "spend this mana only to cast an artifact spell or activate abilities of artifacts"
        | "spend this mana only to cast artifact spells or activate an ability of an artifact"
        | "spend this mana only to activate an ability of an artifact or cast an artifact spell"
        | "spend this mana only to activate an ability of an artifact or to cast an artifact spell"
        | "spend this mana only to activate an ability of an artifact source or cast an artifact spell"
        | "spend this mana only to activate abilities of artifacts or cast artifact spells"
        | "spend this mana only to activate abilities of artifact sources or cast artifact spells" => {
            Some(ManaRestriction::ArtifactSpellOrAbility)
        }
        "spend this mana only to cast an artifact spell or activate an ability"
        | "spend this mana only to cast an artifact spell or to activate an ability"
        | "spend this mana only to cast artifact spells or activate abilities"
        | "spend this mana only to cast artifact spells or to activate abilities"
        | "spend this mana only to cast artifact spells and activate abilities"
        | "spend this mana only to cast artifact spells and to activate abilities"
        | "spend this mana only to cast an artifact spell or activate abilities"
        | "spend this mana only to cast an artifact spell or to activate abilities"
        | "spend this mana only to cast artifact spells or activate an ability"
        | "spend this mana only to cast artifact spells or to activate an ability"
        | "spend this mana only to activate an ability or cast an artifact spell"
        | "spend this mana only to activate an ability or to cast an artifact spell"
        | "spend this mana only to activate abilities or cast artifact spells"
        | "spend this mana only to activate abilities or to cast artifact spells"
        | "spend this mana only to activate an ability or cast artifact spells"
        | "spend this mana only to activate abilities or cast an artifact spell" => {
            Some(ManaRestriction::ArtifactSpellOrAnyAbility)
        }
        "spend this mana only to cast an artifact spell"
        | "spend this mana only to cast artifact spells" => Some(ManaRestriction::ArtifactSpell),
        "spend this mana only to activate abilities of artifacts"
        | "spend this mana only to activate an ability of an artifact"
        | "spend this mana only to activate abilities of artifact sources"
        | "spend this mana only to activate an ability of an artifact source" => {
            Some(ManaRestriction::ArtifactAbility)
        }
        "spend this mana only to activate abilities"
        | "spend this mana only to activate an ability" => Some(ManaRestriction::ActivatedAbility),
        "spend this mana only to cast an instant or sorcery spell"
        | "spend this mana only to cast an instant or a sorcery spell"
        | "spend this mana only to cast an instant and/or sorcery spell"
        | "spend this mana only to cast instant or sorcery spells"
        | "spend this mana only to cast instant and sorcery spells"
        | "spend this mana only to cast instant and/or sorcery spells"
        | "spend this mana only to cast a sorcery or instant spell"
        | "spend this mana only to cast sorcery or instant spells"
        | "spend this mana only to cast sorcery and instant spells" => {
            Some(ManaRestriction::InstantOrSorcerySpell)
        }
        "spend this mana only to foretell a card from your hand or cast an instant or sorcery spell"
        | "spend this mana only to foretell cards from your hand or cast instant or sorcery spells"
        | "spend this mana only to cast an instant or sorcery spell or foretell a card from your hand"
        | "spend this mana only to cast instant or sorcery spells or foretell cards from your hand" => {
            Some(ManaRestriction::InstantOrSorceryOrForetell)
        }
        "spend this mana only to cast an enchantment spell"
        | "spend this mana only to cast enchantment spells" => {
            Some(ManaRestriction::EnchantmentSpell)
        }
        "spend this mana only to cast a legendary spell"
        | "spend this mana only to cast legendary spells" => Some(ManaRestriction::LegendarySpell),
        "spend this mana only to cast a multicolored spell"
        | "spend this mana only to cast multicolored spells" => {
            Some(ManaRestriction::MulticoloredSpell)
        }
        "spend this mana only to cast a creature spell or activate an ability of a creature"
        | "spend this mana only to cast a creature spell or to activate an ability of a creature"
        | "spend this mana only to cast creature spells or activate abilities of creatures"
        | "spend this mana only to cast creature spells or to activate abilities of creatures"
        | "spend this mana only to cast creature spells and activate abilities of creatures"
        | "spend this mana only to cast creature spells and to activate abilities of creatures"
        | "spend this mana only to cast a creature spell or activate abilities of creatures"
        | "spend this mana only to cast creature spells or activate an ability of a creature"
        | "spend this mana only to cast a creature spell or activate an ability of a creature source"
        | "spend this mana only to cast a creature spell or to activate an ability of a creature source"
        | "spend this mana only to cast creature spells or activate abilities of creature sources"
        | "spend this mana only to cast creature spells or to activate abilities of creature sources"
        | "spend this mana only to cast creature spells and activate abilities of creature sources"
        | "spend this mana only to activate an ability of a creature or cast a creature spell"
        | "spend this mana only to activate an ability of a creature or to cast a creature spell"
        | "spend this mana only to activate abilities of creatures or cast creature spells"
        | "spend this mana only to activate abilities of creature sources or cast creature spells" => {
            Some(ManaRestriction::CreatureSpellOrAbility)
        }
        "spend this mana only to cast a spell that's one or more colors without {x} in its mana cost"
        | "spend this mana only to cast spells that are one or more colors without {x} in their mana costs" => {
            Some(ManaRestriction::ColoredSpellWithoutX)
        }
        "spend this mana only to cast spells" | "spend this mana only to cast a spell" => {
            Some(ManaRestriction::CastSpell)
        }
        "spend this mana only on costs that contain {x}"
        | "spend this mana only on a cost that contains {x}"
        | "spend this mana only on costs containing {x}"
        | "spend this mana only to pay costs that contain {x}"
        | "spend this mana only to pay a cost that contains {x}"
        | "spend this mana only to pay costs containing {x}" => {
            Some(ManaRestriction::CostContainingX)
        }
        "this mana can't be spent to cast a nonartifact spell"
        | "this mana can't be spent to cast nonartifact spells" => {
            Some(ManaRestriction::NotNonartifactSpell)
        }
        "this mana can't be spent to cast spells from your hand"
        | "this mana can't be spent to cast a spell from your hand" => {
            Some(ManaRestriction::NotSpellFromHand)
        }
        "spend this mana only to cast a planeswalker spell"
        | "spend this mana only to cast planeswalker spells" => {
            Some(ManaRestriction::PlaneswalkerSpell)
        }
        "spend this mana only to cast a colorless spell"
        | "spend this mana only to cast colorless spells" => Some(ManaRestriction::ColorlessSpell),
        "spend this mana only to cast your commander"
        | "spend this mana only to cast a commander spell"
        | "spend this mana only to cast commander spells" => Some(ManaRestriction::CommanderSpell),
        "spend this mana only to cast a historic spell"
        | "spend this mana only to cast historic spells" => Some(ManaRestriction::HistoricSpell),
        "spend this mana only to cast a creature or planeswalker spell"
        | "spend this mana only to cast creature or planeswalker spells"
        | "spend this mana only to cast creature and planeswalker spells"
        | "spend this mana only to cast creature and/or planeswalker spells"
        | "spend this mana only to cast a planeswalker or creature spell"
        | "spend this mana only to cast planeswalker or creature spells"
        | "spend this mana only to cast planeswalker and creature spells" => {
            Some(ManaRestriction::CreatureOrPlaneswalkerSpell)
        }
        "spend this mana only to cast a creature or enchantment spell"
        | "spend this mana only to cast creature or enchantment spells"
        | "spend this mana only to cast creature and enchantment spells"
        | "spend this mana only to cast creature and/or enchantment spells"
        | "spend this mana only to cast an enchantment or creature spell"
        | "spend this mana only to cast enchantment or creature spells"
        | "spend this mana only to cast enchantment and creature spells" => {
            Some(ManaRestriction::CreatureOrEnchantmentSpell)
        }
        "spend this mana only to activate abilities of creatures"
        | "spend this mana only to activate an ability of a creature"
        | "spend this mana only to activate abilities of creature sources"
        | "spend this mana only to activate an ability of a creature source" => {
            Some(ManaRestriction::CreatureAbility)
        }
        "spend this mana only to cast a permanent spell"
        | "spend this mana only to cast permanent spells" => Some(ManaRestriction::PermanentSpell),
        "spend this mana only to cast spells from your graveyard or exile"
        | "spend this mana only to cast a spell from your graveyard or exile"
        | "spend this mana only to cast spells from exile or your graveyard"
        | "spend this mana only to cast a spell from exile or your graveyard"
        | "spend this mana only to cast spells from your graveyard or from exile"
        | "spend this mana only to cast a spell from your graveyard or from exile"
        | "spend this mana only to cast spells from exile or from your graveyard"
        | "spend this mana only to cast a spell from exile or from your graveyard" => {
            Some(ManaRestriction::SpellFromGraveyardOrExile)
        }
        "spend this mana only to cast an equipment spell or activate an equip ability"
        | "spend this mana only to cast an equipment spell or to activate an equip ability"
        | "spend this mana only to cast equipment spells or activate equip abilities"
        | "spend this mana only to cast equipment spells or to activate equip abilities"
        | "spend this mana only to cast equipment spells and activate equip abilities"
        | "spend this mana only to cast equipment spells or activate abilities of equipment"
        | "spend this mana only to cast equipment spells or to activate abilities of equipment"
        | "spend this mana only to cast an equipment spell or activate an ability of an equipment"
        | "spend this mana only to cast an equipment spell or to activate an ability of an equipment" => {
            Some(ManaRestriction::EquipmentOrEquipAbility)
        }
        "spend this mana only to cast spells with {x} in their mana costs"
        | "spend this mana only to cast a spell with {x} in its mana cost"
        | "spend this mana only to cast x spells" => Some(ManaRestriction::CostContainingX),
        "spend this mana only to cast colorless eldrazi spells or activate abilities of colorless eldrazi"
        | "spend this mana only to cast colorless eldrazi spells or to activate abilities of colorless eldrazi"
        | "spend this mana only to cast colorless eldrazi spells and activate abilities of colorless eldrazi"
        | "spend this mana only to cast a colorless eldrazi spell or activate an ability of a colorless eldrazi"
        | "spend this mana only to cast a colorless eldrazi spell or to activate an ability of a colorless eldrazi"
        | "spend this mana only to cast colorless eldrazi spells or activate abilities of colorless eldrazi sources"
        | "spend this mana only to cast colorless eldrazi spells or to activate abilities of colorless eldrazi sources"
        | "spend this mana only to cast a colorless eldrazi spell or activate an ability of a colorless eldrazi source"
        | "spend this mana only to cast a colorless eldrazi spell or to activate an ability of a colorless eldrazi source" => {
            Some(ManaRestriction::ColorlessEldraziSpellOrAbility)
        }
        "spend this mana only to cast a spell with flashback from a graveyard"
        | "spend this mana only to cast spells with flashback from a graveyard"
        | "spend this mana only to cast a spell with flashback from your graveyard"
        | "spend this mana only to cast spells with flashback from your graveyard" => {
            Some(ManaRestriction::SpellWithFlashbackFromGraveyard)
        }
        _ => {
            if let Some(rest) = low
                .strip_prefix("spend this mana only to cast creature spells with mana value ")
                .or_else(|| {
                    low.strip_prefix(
                        "spend this mana only to cast a creature spell with mana value ",
                    )
                })
                && let Some((num_part, tail)) = rest
                    .split_once(" or greater")
                    .or_else(|| rest.split_once(" or more"))
                && let Ok(min_mv) = num_part.trim().parse::<u8>()
            {
                let t = tail.trim();
                if t.is_empty() {
                    return Some(ManaRestriction::CreatureSpellWithMinManaValue(min_mv));
                }
                if t.contains("{x}") && t.contains("mana cost") {
                    return Some(ManaRestriction::CreatureSpellWithMinManaValueOrX(min_mv));
                }
            }
            if let Some(rest) = low
                .strip_prefix("spend this mana only to cast spells with mana value ")
                .or_else(|| {
                    low.strip_prefix("spend this mana only to cast a spell with mana value ")
                })
                && let Some((num_part, tail)) = rest
                    .split_once(" or greater")
                    .or_else(|| rest.split_once(" or more"))
                && let Ok(min_mv) = num_part.trim().parse::<u8>()
            {
                let t = tail.trim();
                if t.is_empty() {
                    return Some(ManaRestriction::SpellWithMinManaValue(min_mv));
                }
                if t.contains("{x}") && t.contains("mana cost") {
                    return Some(ManaRestriction::SpellWithMinManaValueOrX(min_mv));
                }
            }
            let resolve_subtype = |raw_sub: &str| {
                resolve_protection_subtype(raw_sub, vocabulary).or_else(|| {
                    raw_sub.trim().strip_suffix("ies").and_then(|stem| {
                        resolve_protection_subtype(&format!("{stem}y"), vocabulary)
                    })
                })
            };
            fn peel_spell_subtype(part: &str) -> Option<&str> {
                let p = part.trim();
                let no_art = p
                    .strip_prefix("a ")
                    .or_else(|| p.strip_prefix("an "))
                    .unwrap_or(p);
                no_art
                    .strip_suffix(" creature spells")
                    .or_else(|| no_art.strip_suffix(" creature spell"))
                    .or_else(|| no_art.strip_suffix(" spells"))
                    .or_else(|| no_art.strip_suffix(" spell"))
                    .map(str::trim)
            }
            let matches_subtype_ability = |ab_part: &str, ct: CreatureTypeId| -> bool {
                let ab_core = ab_part
                    .trim()
                    .strip_prefix("abilities of ")
                    .or_else(|| ab_part.trim().strip_prefix("an ability of "))
                    .map(str::trim);
                ab_core.is_some_and(|core| {
                    if core == "creatures of that type"
                        || core == "a creature of that type"
                        || core == "permanents of that type"
                        || core == "a permanent of that type"
                        || core == "sources of that type"
                        || core == "a source of that type"
                    {
                        return true;
                    }
                    let cleaned = core
                        .strip_prefix("a ")
                        .or_else(|| core.strip_prefix("an "))
                        .unwrap_or(core);
                    let cleaned = cleaned
                        .strip_suffix(" sources")
                        .or_else(|| cleaned.strip_suffix(" source"))
                        .or_else(|| cleaned.strip_suffix(" creatures"))
                        .or_else(|| cleaned.strip_suffix(" creature"))
                        .or_else(|| cleaned.strip_suffix(" permanents"))
                        .or_else(|| cleaned.strip_suffix(" permanent"))
                        .unwrap_or(cleaned)
                        .trim();
                    resolve_subtype(cleaned) == Some(ct)
                })
            };
            if let Some(rest) = low.strip_prefix("spend this mana only to cast ") {
                if let Some((spell_part, ab_part)) = rest
                    .split_once(" or to activate ")
                    .or_else(|| rest.split_once(" or activate "))
                    .or_else(|| rest.split_once(" and to activate "))
                    .or_else(|| rest.split_once(" and activate "))
                {
                    if let Some(sub_raw) = peel_spell_subtype(spell_part)
                        && let Some(ct) = resolve_subtype(sub_raw)
                        && matches_subtype_ability(ab_part, ct)
                    {
                        return Some(ManaRestriction::CreatureSpellOrAbilityOfType(Some(ct)));
                    }
                    return None;
                }
                if let Some(sub_raw) = peel_spell_subtype(rest)
                    && let Some(ct) = resolve_subtype(sub_raw)
                {
                    return Some(ManaRestriction::CreatureSpellOfType(Some(ct)));
                }
            }
            if let Some(rest) = low.strip_prefix("spend this mana only to activate ")
                && let Some((ab_part, spell_part)) = rest
                    .split_once(" or to cast ")
                    .or_else(|| rest.split_once(" or cast "))
                    .or_else(|| rest.split_once(" and to cast "))
                    .or_else(|| rest.split_once(" and cast "))
                && let Some(sub_raw) = peel_spell_subtype(spell_part)
                && let Some(ct) = resolve_subtype(sub_raw)
                && matches_subtype_ability(ab_part, ct)
            {
                return Some(ManaRestriction::CreatureSpellOrAbilityOfType(Some(ct)));
            }
            None
        }
    }
}

pub(super) type LoweredActivationCost = (
    ManaCost,
    SourceCost,
    Vec<spellwake_engine::mana::HybridSymbol>,
    Vec<ExtraActivationCost>,
);

pub(super) fn lower_extra_cost_atom(
    atom: &CostAtomAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ExtraActivationCost, &'static str> {
    let nz_u8 = |n: i32| -> Result<NonZeroU8, &'static str> {
        let u = u8::try_from(n).map_err(|_| "oracle:unsupported-activation-cost")?;
        NonZeroU8::new(u).ok_or("oracle:unsupported-activation-cost")
    };
    let opt_count = |expr: &ValueExprAst| -> Result<Option<NonZeroU8>, &'static str> {
        match expr {
            ValueExprAst::Const(n) if *n > 0 => Ok(Some(nz_u8(*n)?)),
            ValueExprAst::ChosenX => Ok(None),
            _ => Err("oracle:unsupported-activation-cost"),
        }
    };
    let lower_filter_any = |f: &ObjectFilterAst| -> Result<ObjectFilter, &'static str> {
        let mut inner = f.clone();
        inner.zone = None;
        if inner.is_empty() || inner.any_card {
            Ok(ObjectFilter::any())
        } else {
            lower_object_filter(&inner, vocabulary)
        }
    };
    match atom {
        CostAtomAst::TapSelf => Ok(ExtraActivationCost::TapSource),
        CostAtomAst::UntapSelf => Ok(ExtraActivationCost::UntapSource),
        CostAtomAst::SacrificeSelf => Ok(ExtraActivationCost::SacrificeSource),
        CostAtomAst::ExileSelf {
            from_graveyard,
            from_hand,
        } => Ok(ExtraActivationCost::ExileSource {
            from_graveyard: *from_graveyard,
            from_hand: *from_hand,
        }),
        CostAtomAst::DiscardSelf => Ok(ExtraActivationCost::DiscardSource),
        CostAtomAst::ExertSelf => Ok(ExtraActivationCost::ExertSource),
        CostAtomAst::PayLife(expr) => match expr.as_ref() {
            ValueExprAst::Const(n) if *n > 0 => {
                Ok(ExtraActivationCost::PayLife(LifeCostAmount::Fixed(
                    u16::try_from(*n).map_err(|_| "oracle:unsupported-activation-cost")?,
                )))
            }
            _ => Err("oracle:unsupported-activation-cost"),
        },
        CostAtomAst::PayHalfLife => Ok(ExtraActivationCost::PayHalfLife),
        CostAtomAst::PayEnergy(n) if *n > 0 => Ok(ExtraActivationCost::PayEnergy(*n)),
        CostAtomAst::PayEnergyExpr(expr) => {
            let lowered = lower_value_expr(expr, vocabulary)?;
            if !lowered.is_chosen_x() && !lowered.is_world_evaluable() {
                return Err("oracle:unsupported-activation-cost");
            }
            Ok(ExtraActivationCost::PayEnergyExpr(lowered))
        }
        CostAtomAst::Mill(expr) => match expr.as_ref() {
            ValueExprAst::Const(n) if *n > 0 => Ok(ExtraActivationCost::Mill(nz_u8(*n)?)),
            _ => Err("oracle:unsupported-activation-cost"),
        },
        CostAtomAst::RemoveCounters {
            all,
            count,
            counter_kind,
            subject,
        } => {
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                let kind = parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?;
                let delta = if *all {
                    CounterCostDelta::RemoveAll
                } else if let Some(c) = count
                    && let ValueExprAst::Const(n) = c.as_ref()
                    && *n > 0
                {
                    CounterCostDelta::Remove(nz_u8(*n)?)
                } else {
                    return Err("oracle:unsupported-activation-cost");
                };
                return Ok(ExtraActivationCost::SourceCounter { kind, delta });
            }
            let kind = {
                let ck = counter_kind.trim().to_ascii_lowercase();
                if ck.is_empty()
                    || matches!(ck.as_str(), "a" | "an" | "any" | "counter" | "counters")
                {
                    None
                } else {
                    Some(parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?)
                }
            };
            let cnt = if *all {
                None
            } else if let Some(c) = count {
                opt_count(c)?
            } else {
                Some(NonZeroU8::MIN)
            };
            let filter = match subject.as_ref() {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                    lower_filter_any(f)?
                }
                _ => return Err("oracle:unsupported-activation-cost"),
            };
            Ok(ExtraActivationCost::RemovePermanentCounters {
                kind,
                count: cnt,
                all: *all,
                filter,
            })
        }
        CostAtomAst::PutCounters {
            count,
            counter_kind,
            subject,
        } => {
            let kind = parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?;
            let ValueExprAst::Const(n) = count.as_ref() else {
                return Err("oracle:unsupported-activation-cost");
            };
            let nz = nz_u8(*n)?;
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                return Ok(ExtraActivationCost::SourceCounter {
                    kind,
                    delta: CounterCostDelta::Add(nz),
                });
            }
            let filter = match subject.as_ref() {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                    lower_filter_any(f)?
                }
                _ => return Err("oracle:unsupported-activation-cost"),
            };
            Ok(ExtraActivationCost::PutPermanentCounters {
                kind,
                count: nz,
                filter,
            })
        }
        CostAtomAst::Sacrifice {
            count,
            filter,
            up_to: false,
            min_one: false,
        } => Ok(ExtraActivationCost::SacrificePermanent {
            filter: lower_filter_any(filter)?,
            count: opt_count(count)?,
        }),
        CostAtomAst::TapPermanent {
            count,
            filter,
            any_number: false,
        } => Ok(ExtraActivationCost::TapPermanent {
            filter: lower_filter_any(filter)?,
            count: opt_count(count)?,
            exclude_source: filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0,
        }),
        CostAtomAst::UntapPermanent { count, filter } => {
            let ValueExprAst::Const(n) = count.as_ref() else {
                return Err("oracle:unsupported-activation-cost");
            };
            Ok(ExtraActivationCost::UntapPermanent {
                filter: lower_filter_any(filter)?,
                count: nz_u8(*n)?,
                exclude_source: filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0,
            })
        }
        CostAtomAst::BounceToHand(subject) => match subject.as_ref() {
            SubjectRefAst::SelfObject => Ok(ExtraActivationCost::ReturnSourceToHand),
            SubjectRefAst::AttachedHost(_) => Ok(ExtraActivationCost::BouncePermanent {
                filter: ObjectFilter::any().attached_to_self(),
                count: NonZeroU8::MIN,
                exclude_source: false,
            }),
            SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                Ok(ExtraActivationCost::BouncePermanent {
                    filter: lower_filter_any(f)?,
                    count: NonZeroU8::MIN,
                    exclude_source: f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0,
                })
            }
            SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(n),
            } => Ok(ExtraActivationCost::BouncePermanent {
                filter: lower_filter_any(filter)?,
                count: nz_u8(i32::from(*n))?,
                exclude_source: filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0,
            }),
            _ => Err("oracle:unsupported-activation-cost"),
        },
        CostAtomAst::DiscardHand => Ok(ExtraActivationCost::DiscardCards {
            filter: ObjectFilter::any(),
            count: None,
            whole_hand: true,
            random: false,
        }),
        CostAtomAst::Discard {
            count,
            filter,
            random,
            up_to: false,
            any_number: false,
        } => Ok(ExtraActivationCost::DiscardCards {
            filter: lower_filter_any(filter)?,
            count: opt_count(count)?,
            whole_hand: false,
            random: *random,
        }),
        CostAtomAst::Reveal(subject) => {
            let inner = match subject.as_ref() {
                SubjectRefAst::InZone {
                    subject: inner,
                    zone: ZoneFilter::YourHand,
                } => inner.as_ref(),
                other => other,
            };
            match inner {
                SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                    Ok(ExtraActivationCost::RevealHand {
                        filter: lower_filter_any(f)?,
                        count: NonZeroU8::MIN,
                        allow_controlled: false,
                    })
                }
                SubjectRefAst::TargetObject {
                    filter,
                    count: TargetCountAst::Exact(n),
                } => Ok(ExtraActivationCost::RevealHand {
                    filter: lower_filter_any(filter)?,
                    count: nz_u8(i32::from(*n))?,
                    allow_controlled: false,
                }),
                _ => Err("oracle:unsupported-activation-cost"),
            }
        }
        CostAtomAst::Behold(filter) => Ok(ExtraActivationCost::RevealHand {
            filter: lower_filter_any(filter)?,
            count: NonZeroU8::MIN,
            allow_controlled: true,
        }),
        CostAtomAst::Exile {
            count,
            filter,
            face_down: false,
            face_up: _,
            single_graveyard: false,
            any_number: false,
            min_one: false,
            total_mv: None,
        } => {
            let zone = match filter.zone {
                Some(ZoneFilter::YourGraveyard) => CostExileZone::Graveyard,
                Some(ZoneFilter::YourHand) => CostExileZone::Hand,
                None | Some(ZoneFilter::Battlefield) => CostExileZone::Battlefield,
                _ => return Err("oracle:unsupported-activation-cost"),
            };
            Ok(ExtraActivationCost::ExileObjects {
                zone,
                filter: lower_filter_any(filter)?,
                count: opt_count(count)?,
                exclude_source: filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0,
            })
        }
        CostAtomAst::CollectEvidence(expr) => match expr.as_ref() {
            ValueExprAst::Const(n) if *n > 0 => Ok(ExtraActivationCost::CollectEvidence(
                u16::try_from(*n).map_err(|_| "oracle:unsupported-activation-cost")?,
            )),
            _ => Err("oracle:unsupported-activation-cost"),
        },
        CostAtomAst::Forage => Ok(ExtraActivationCost::Forage),
        CostAtomAst::Waterbend(raw) => {
            let mc = parse_fixed_mana_cost(raw).ok_or("oracle:unsupported-activation-cost")?;
            Ok(ExtraActivationCost::Waterbend(mc))
        }
        CostAtomAst::Blight(expr) => match expr.as_ref() {
            ValueExprAst::Const(n) if *n > 0 => Ok(ExtraActivationCost::Blight(nz_u8(*n)?)),
            _ => Err("oracle:unsupported-activation-cost"),
        },
        CostAtomAst::ProcessExiledCard => Ok(ExtraActivationCost::ProcessExiledCard),
        CostAtomAst::PayMana(raw) => {
            let mc = parse_fixed_mana_cost(raw).ok_or("oracle:unsupported-activation-cost")?;
            Ok(ExtraActivationCost::PayMana(mc))
        }
        CostAtomAst::Or(a, b) => Ok(ExtraActivationCost::Either(
            Box::new(lower_extra_cost_atom(a, vocabulary)?),
            Box::new(lower_extra_cost_atom(b, vocabulary)?),
        )),
        CostAtomAst::Unattach { subject, .. } => match subject.as_ref() {
            SubjectRefAst::SelfObject => Ok(ExtraActivationCost::Unattach { self_source: true }),
            SubjectRefAst::Group(_) | SubjectRefAst::All(_) | SubjectRefAst::Each(_) => {
                Ok(ExtraActivationCost::Unattach { self_source: false })
            }
            _ => Err("oracle:unsupported-activation-cost"),
        },
        _ => Err("oracle:unsupported-activation-cost"),
    }
}

pub(super) fn lower_activation_cost_composite(
    cost: &[CostAtomAst],
    zone: Option<&str>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<LoweredActivationCost, &'static str> {
    let mut mana_cost = ManaCost::free();
    let mut hybrids = Vec::new();
    let mut x_symbols: u8 = 0;
    let mut extra = Vec::new();
    for atom in cost {
        match atom {
            CostAtomAst::PayMana(raw) => {
                let (mc, x, hybs) = parse_mana_cost_full(raw).ok_or("oracle:act-mana-cost")?;
                mana_cost.generic = mana_cost.generic.plus(&mc.generic);
                for (k, v) in mc.required {
                    let entry = mana_cost.required.entry(k).or_insert_with(|| 0.into());
                    *entry = entry.plus(&v);
                }
                hybrids.extend(hybs);
                x_symbols = x_symbols.checked_add(x).ok_or("oracle:x-ovf")?;
            }
            other => {
                extra.push(lower_extra_cost_atom(other, vocabulary)?);
            }
        }
    }
    if zone == Some("hand") && !extra.iter().any(ExtraActivationCost::is_from_hand) {
        return Err("oracle:unsupported-hand-activation");
    }
    if zone == Some("graveyard")
        && x_symbols > 0
        && !extra.iter().any(ExtraActivationCost::is_from_graveyard)
    {
        return Err("oracle:unsupported-graveyard-x-activation");
    }
    let source_cost = if x_symbols > 0 {
        SourceCost::GenericX {
            symbols: x_symbols,
            tap_source: false,
        }
    } else if zone == Some("graveyard") && !extra.iter().any(ExtraActivationCost::is_from_graveyard)
    {
        SourceCost::GraveyardSelf { unearth: false }
    } else {
        SourceCost::None
    };
    Ok((mana_cost, source_cost, hybrids, extra))
}

pub(super) fn lower_activation_cost_full(
    cost: &[CostAtomAst],
    zone: Option<&str>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<LoweredActivationCost, &'static str> {
    if let Ok((mc, sc, hybs)) = lower_activation_cost_fast(cost, zone, vocabulary) {
        return Ok((mc, sc, hybs, Vec::new()));
    }
    lower_activation_cost_composite(cost, zone, vocabulary)
}

pub(super) fn lower_activation_cost_fast(
    cost: &[CostAtomAst],
    zone: Option<&str>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<
    (
        ManaCost,
        SourceCost,
        Vec<spellwake_engine::mana::HybridSymbol>,
    ),
    &'static str,
> {
    let mut mana_cost = ManaCost::free();
    let mut hybrid_symbols: Vec<spellwake_engine::mana::HybridSymbol> = Vec::new();
    let mut x_symbols: u8 = 0;
    let mut tap_self = false;
    let mut sac_self = false;
    let mut exile_self = false;
    let mut exile_from_gy = false;
    let mut pay_life: Option<u16> = None;
    let mut discard_self = false;
    let mut discard_mode: Option<DiscardCostMode> = None;
    let mut sac_selected: Option<(SelectedSacrificeFilter, u8)> = None;
    let mut tap_perms: Option<(TapCostFilter, TapCostCount, bool)> = None;
    let mut counter_cost: Option<(CounterKind, CounterCostDelta)> = None;
    let mut mill_count: Option<NonZeroU8> = None;
    let mut exile_gy_cost: Option<GraveyardExileCost> = None;
    let mut pay_energy: Option<u16> = None;
    let mut untap_self = false;
    let mut return_self_to_hand = false;

    for atom in cost {
        match atom {
            CostAtomAst::PayMana(raw) => {
                let (mc, x, hybs) = parse_mana_cost_full(raw).ok_or("oracle:act-mana-cost")?;
                mana_cost.generic = mana_cost.generic.plus(&mc.generic);
                for (k, v) in mc.required {
                    let entry = mana_cost.required.entry(k).or_insert_with(|| 0.into());
                    *entry = entry.plus(&v);
                }
                hybrid_symbols.extend(hybs);
                x_symbols = x_symbols.checked_add(x).ok_or("oracle:x-ovf")?;
            }
            CostAtomAst::TapSelf => tap_self = true,
            CostAtomAst::UntapSelf => untap_self = true,
            CostAtomAst::PayEnergy(n) if *n > 0 => {
                pay_energy = Some(pay_energy.unwrap_or(0).saturating_add(*n));
            }
            CostAtomAst::BounceToHand(subj)
                if matches!(subj.as_ref(), SubjectRefAst::SelfObject) =>
            {
                return_self_to_hand = true;
            }
            CostAtomAst::SacrificeSelf => sac_self = true,
            CostAtomAst::ExileSelf {
                from_graveyard,
                from_hand: false,
            } => {
                exile_self = true;
                exile_from_gy = *from_graveyard;
            }
            CostAtomAst::Exile {
                count,
                filter,
                face_down: false,
                face_up: _,
                single_graveyard: false,
                any_number: false,
                min_one: false,
                total_mv: None,
            } if filter.zone == Some(ZoneFilter::YourGraveyard) => {
                if matches!(count.as_ref(), ValueExprAst::Const(1))
                    && filter.any_types == AstCardTypeMask::CREATURE
                    && filter.forbidden_types.is_empty()
                    && filter.required_supertypes.is_empty()
                    && filter.forbidden_supertypes.is_empty()
                    && filter.subtypes.is_empty()
                    && filter.forbidden_subtypes.is_empty()
                    && filter.any_colors.is_empty()
                    && filter.forbidden_colors.is_empty()
                    && filter.color_arity.is_none()
                    && filter.required_keywords.is_empty()
                    && filter.forbidden_keywords.is_empty()
                    && filter.stat_bounds.is_empty()
                    && filter.counter_kind.is_none()
                    && filter.flags.0 == 0
                    && filter.predicates.is_empty()
                {
                    exile_gy_cost = Some(GraveyardExileCost::Creature {
                        pay_mana_cost: false,
                    });
                } else if matches!(count.as_ref(), ValueExprAst::ChosenX)
                    && (filter.any_card || filter.any_types.is_empty())
                    && filter.forbidden_types.is_empty()
                    && filter.subtypes.is_empty()
                    && filter.any_colors.is_empty()
                    && filter.forbidden_colors.is_empty()
                    && filter.stat_bounds.is_empty()
                    && filter.flags.0 == 0
                    && filter.predicates.is_empty()
                {
                    exile_gy_cost = Some(GraveyardExileCost::AnyCountX);
                } else if let ValueExprAst::Const(n) = count.as_ref()
                    && *n > 0
                    && let Ok(u) = u8::try_from(*n)
                    && let Some(nz) = NonZeroU8::new(u)
                {
                    let mut inner = filter.as_ref().clone();
                    inner.zone = None;
                    let obj = if inner.is_empty()
                        || (inner.any_card && inner.predicates.is_empty() && inner.flags.0 == 0)
                    {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(&inner, vocabulary)?
                    };
                    exile_gy_cost = Some(GraveyardExileCost::Filtered {
                        filter: obj,
                        count: nz,
                    });
                } else {
                    return Err("oracle:unsupported-activation-cost");
                }
            }
            CostAtomAst::DiscardSelf => discard_self = true,
            CostAtomAst::DiscardHand => discard_mode = Some(DiscardCostMode::WholeHand),
            CostAtomAst::Discard {
                count,
                filter,
                random: false,
                up_to: false,
                any_number: false,
            } if matches!(count.as_ref(), ValueExprAst::Const(n) if *n > 0) => {
                let ValueExprAst::Const(n) = count.as_ref() else {
                    unreachable!()
                };
                let n_u8 = u8::try_from(*n).map_err(|_| "oracle:discard-ovf")?;
                let nz = NonZeroU8::new(n_u8).ok_or("oracle:discard-zero")?;
                let plain_any = filter.any_types.is_empty()
                    && filter.forbidden_types.is_empty()
                    && filter.required_supertypes.is_empty()
                    && filter.forbidden_supertypes.is_empty()
                    && filter.subtypes.is_empty()
                    && filter.forbidden_subtypes.is_empty()
                    && filter.any_colors.is_empty()
                    && filter.forbidden_colors.is_empty()
                    && filter.color_arity.is_none()
                    && filter.required_keywords.is_empty()
                    && filter.forbidden_keywords.is_empty()
                    && filter.stat_bounds.is_empty()
                    && filter.counter_kind.is_none()
                    && filter.flags.0 == 0
                    && filter.predicates.is_empty();
                if n_u8 == 1 && plain_any {
                    discard_mode = Some(DiscardCostMode::SelectedOne);
                } else {
                    let obj = if plain_any {
                        ObjectFilter::any()
                    } else {
                        lower_object_filter(filter, vocabulary)?
                    };
                    discard_mode = Some(DiscardCostMode::Filtered {
                        filter: obj,
                        count: nz,
                    });
                }
            }
            CostAtomAst::PayLife(expr) => {
                let ValueExprAst::Const(n) = expr.as_ref() else {
                    return Err("oracle:act-dynamic-life-cost");
                };
                if *n <= 0 {
                    return Err("oracle:act-nonpos-life");
                }
                pay_life = Some(u16::try_from(*n).map_err(|_| "oracle:life-ovf")?);
            }
            CostAtomAst::Sacrifice {
                count,
                filter,
                up_to: false,
                min_one: false,
            } => {
                let ValueExprAst::Const(n) = count.as_ref() else {
                    return Err("oracle:act-dynamic-sac");
                };
                let n_u8 = u8::try_from(*n).map_err(|_| "oracle:sac-ovf")?;
                if n_u8 == 0 {
                    return Err("oracle:sac-zero");
                }
                let ssf = lower_selected_sacrifice_filter(filter, vocabulary)?;
                sac_selected = Some((ssf, n_u8));
            }
            CostAtomAst::TapPermanent {
                count,
                filter,
                any_number: false,
            } => {
                let tc = match count.as_ref() {
                    ValueExprAst::Const(n) if *n > 0 => TapCostCount::Fixed(
                        NonZeroU8::new(u8::try_from(*n).map_err(|_| "oracle:tap-ovf")?)
                            .ok_or("oracle:tap-zero")?,
                    ),
                    ValueExprAst::ChosenX => TapCostCount::ChosenX,
                    _ => return Err("oracle:act-dynamic-tap"),
                };
                let tcf = lower_tap_cost_filter(filter, vocabulary)?;
                let exclude_self = filter.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0;
                tap_perms = Some((tcf, tc, exclude_self));
            }
            CostAtomAst::RemoveCounters {
                all,
                count,
                counter_kind,
                subject,
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
                let ck = parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?;
                let delta = if *all {
                    CounterCostDelta::RemoveAll
                } else if let Some(c) = count {
                    let ValueExprAst::Const(n) = c.as_ref() else {
                        return Err("oracle:act-dynamic-counter");
                    };
                    CounterCostDelta::Remove(
                        NonZeroU8::new(u8::try_from(*n).map_err(|_| "oracle:cnt-ovf")?)
                            .ok_or("oracle:cnt-zero")?,
                    )
                } else {
                    return Err("oracle:act-counter-missing-count");
                };
                counter_cost = Some((ck, delta));
            }
            CostAtomAst::PutCounters {
                count,
                counter_kind,
                subject,
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
                let ck = parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?;
                let ValueExprAst::Const(n) = count.as_ref() else {
                    return Err("oracle:act-dynamic-counter");
                };
                let delta = CounterCostDelta::Add(
                    NonZeroU8::new(u8::try_from(*n).map_err(|_| "oracle:cnt-ovf")?)
                        .ok_or("oracle:cnt-zero")?,
                );
                counter_cost = Some((ck, delta));
            }
            CostAtomAst::Mill(expr) => {
                let ValueExprAst::Const(n) = expr.as_ref() else {
                    return Err("oracle:act-dynamic-mill");
                };
                mill_count = Some(
                    NonZeroU8::new(u8::try_from(*n).map_err(|_| "oracle:mill-ovf")?)
                        .ok_or("oracle:mill-zero")?,
                );
            }
            _ => return Err("oracle:unsupported-activation-cost"),
        }
    }

    let from_gy = zone == Some("graveyard") || exile_from_gy;
    let from_hand = zone == Some("hand");

    // Count non-tap/non-mana extra cost components to make sure they combine cleanly
    let non_mana_count = usize::from(exile_self)
        + usize::from(exile_gy_cost.is_some())
        + usize::from(discard_self)
        + usize::from(discard_mode.is_some())
        + usize::from(tap_perms.is_some())
        + usize::from(counter_cost.is_some())
        + usize::from(mill_count.is_some())
        + usize::from(pay_energy.is_some())
        + usize::from(untap_self)
        + usize::from(return_self_to_hand)
        + usize::from(x_symbols > 0);
    if non_mana_count > 1 {
        return Err("oracle:compound-activation-cost");
    }

    if let Some(amount) = pay_energy {
        if sac_self || pay_life.is_some() || sac_selected.is_some() || from_gy || from_hand {
            return Err("oracle:compound-energy-cost");
        }
        return Ok((
            mana_cost,
            SourceCost::PayEnergy {
                amount,
                tap_source: tap_self,
            },
            hybrid_symbols,
        ));
    }
    if untap_self {
        if tap_self
            || sac_self
            || pay_life.is_some()
            || sac_selected.is_some()
            || from_gy
            || from_hand
        {
            return Err("oracle:compound-untap-cost");
        }
        return Ok((mana_cost, SourceCost::Untap, hybrid_symbols));
    }
    if return_self_to_hand {
        if sac_self || pay_life.is_some() || sac_selected.is_some() || from_gy || from_hand {
            return Err("oracle:compound-bounce-cost");
        }
        return Ok((
            mana_cost,
            SourceCost::ReturnSelfToHand {
                tap_source: tap_self,
            },
            hybrid_symbols,
        ));
    }

    if let Some(filter) = exile_gy_cost {
        if sac_self || pay_life.is_some() || sac_selected.is_some() || from_hand {
            return Err("oracle:compound-exile-gy-cost");
        }
        if from_gy {
            if tap_self {
                return Err("oracle:compound-exile-gy-cost");
            }
            return Ok((
                mana_cost,
                SourceCost::GraveyardExileOther { filter },
                hybrid_symbols,
            ));
        }
        return Ok((
            mana_cost,
            SourceCost::ExileGraveyard {
                filter,
                tap_source: tap_self,
            },
            hybrid_symbols,
        ));
    }

    if discard_self {
        if tap_self || sac_self || pay_life.is_some() || sac_selected.is_some() || from_gy {
            return Err("oracle:compound-discard-self");
        }
        return Ok((
            mana_cost,
            SourceCost::HandDiscardSelf(HandDiscardKeyword::Channel),
            hybrid_symbols,
        ));
    }
    if from_hand {
        return Err("oracle:unsupported-hand-activation");
    }
    if exile_self {
        if sac_self || pay_life.is_some() || sac_selected.is_some() {
            return Err("oracle:compound-exile-self");
        }
        return Ok((
            mana_cost,
            SourceCost::Exile {
                tap_source: tap_self,
                from_graveyard: from_gy,
            },
            hybrid_symbols,
        ));
    }
    if let Some((kind, delta)) = counter_cost {
        if pay_life.is_some() || sac_selected.is_some() || from_gy {
            return Err("oracle:compound-counter-cost");
        }
        return Ok((
            mana_cost,
            SourceCost::Counter {
                kind,
                delta,
                tap_source: tap_self,
                sacrifice_source: sac_self,
            },
            hybrid_symbols,
        ));
    }
    if let Some(amount) = pay_life {
        if sac_self {
            return Err("oracle:compound-life-cost");
        }
        if from_gy {
            if tap_self || sac_selected.is_some() {
                return Err("oracle:compound-life-cost");
            }
            return Ok((
                mana_cost,
                SourceCost::GraveyardPayLife {
                    amount: LifeCostAmount::Fixed(amount),
                },
                hybrid_symbols,
            ));
        }
        let sac = match sac_selected {
            None => None,
            Some((f, 1)) => Some(f),
            Some(_) => return Err("oracle:life-multi-sac"),
        };
        return Ok((
            mana_cost,
            SourceCost::PayLife {
                amount: LifeCostAmount::Fixed(amount),
                tap_source: tap_self,
                sacrifice: sac,
            },
            hybrid_symbols,
        ));
    }
    if x_symbols > 0 {
        if sac_self || sac_selected.is_some() || from_gy {
            return Err("oracle:compound-x-cost");
        }
        return Ok((
            mana_cost,
            SourceCost::GenericX {
                symbols: x_symbols,
                tap_source: tap_self,
            },
            hybrid_symbols,
        ));
    }
    if let Some((filter, count, exclude_source)) = tap_perms {
        if sac_self || sac_selected.is_some() || from_gy {
            return Err("oracle:compound-tap-perms");
        }
        return Ok((
            mana_cost,
            SourceCost::TapPermanents {
                filter,
                count,
                tap_source: tap_self,
                exclude_source,
            },
            hybrid_symbols,
        ));
    }
    if let Some((filter, count)) = sac_selected {
        if sac_self {
            return Err("oracle:compound-sac");
        }
        return Ok((
            mana_cost,
            SourceCost::SacrificeSelected {
                filter,
                count,
                tap_source: tap_self,
                from_graveyard: from_gy,
            },
            hybrid_symbols,
        ));
    }
    if let Some(mode) = discard_mode {
        if from_gy {
            if tap_self || sac_self {
                return Err("oracle:compound-discard");
            }
            return Ok((
                mana_cost,
                SourceCost::GraveyardDiscard { mode },
                hybrid_symbols,
            ));
        }
        return Ok((
            mana_cost,
            SourceCost::Discard {
                mode,
                tap_source: tap_self,
                sacrifice_source: sac_self,
            },
            hybrid_symbols,
        ));
    }
    if let Some(count) = mill_count {
        if sac_self || from_gy {
            return Err("oracle:compound-mill");
        }
        return Ok((
            mana_cost,
            SourceCost::Mill {
                count,
                tap_source: tap_self,
            },
            hybrid_symbols,
        ));
    }
    if from_gy {
        if tap_self || sac_self {
            return Err("oracle:gy-tap-or-sac");
        }
        return Ok((
            mana_cost,
            SourceCost::GraveyardSelf { unearth: false },
            hybrid_symbols,
        ));
    }

    let source_cost = match (tap_self, sac_self) {
        (false, false) => SourceCost::None,
        (true, false) => SourceCost::Tap,
        (false, true) => SourceCost::Sacrifice,
        (true, true) => SourceCost::TapSacrifice,
    };
    Ok((mana_cost, source_cost, hybrid_symbols))
}

pub(super) fn lower_selected_sacrifice_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SelectedSacrificeFilter, &'static str> {
    let exclude_self = f.flags == AstFilterFlags::EXCLUDE_SELF;
    let plain_flags = f.flags.is_empty() || exclude_self;
    if plain_flags
        && filter_only_has(
            f,
            FilterAllow {
                types: true,
                subtypes: true,
                controller: true,
                flags: true,
                ..FilterAllow::NONE
            },
        )
        && matches!(f.controller, None | Some(ControllerRelation::You))
    {
        if f.subtypes.len() == 1
            && !exclude_self
            && (f.any_types.is_empty()
                || f.any_types == AstCardTypeMask::CREATURE
                || f.any_types == AstCardTypeMask::LAND)
        {
            let st = &f.subtypes[0];
            if st.eq_ignore_ascii_case("desert") && f.any_types != AstCardTypeMask::CREATURE {
                return Ok(SelectedSacrificeFilter::Desert);
            }
            if f.any_types != AstCardTypeMask::LAND
                && let Some(ct) = vocabulary.resolve_label(st)
            {
                return Ok(SelectedSacrificeFilter::Subtype(ct));
            }
        }
        if f.subtypes.is_empty() {
            if f.any_types == AstCardTypeMask::CREATURE {
                return Ok(if exclude_self {
                    SelectedSacrificeFilter::OtherCreature
                } else {
                    SelectedSacrificeFilter::Creature
                });
            }
            if f.any_types == AstCardTypeMask::ARTIFACT && !exclude_self {
                return Ok(SelectedSacrificeFilter::Artifact);
            }
            if f.any_types == AstCardTypeMask::ENCHANTMENT && exclude_self {
                return Ok(SelectedSacrificeFilter::OtherEnchantment);
            }
            if f.any_types == AstCardTypeMask::ARTIFACT.union(AstCardTypeMask::CREATURE)
                && !exclude_self
            {
                return Ok(SelectedSacrificeFilter::ArtifactOrCreature);
            }
            if f.any_types == AstCardTypeMask::CREATURE.union(AstCardTypeMask::ENCHANTMENT)
                && !exclude_self
            {
                return Ok(SelectedSacrificeFilter::CreatureOrEnchantment);
            }
        }
    }
    let obj = lower_object_filter(f, vocabulary)?;
    Ok(SelectedSacrificeFilter::Matching(obj))
}

pub(super) fn lower_tap_cost_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<TapCostFilter, &'static str> {
    if filter_only_has(
        f,
        FilterAllow {
            types: true,
            req_supertypes: true,
            subtypes: true,
            controller: true,
            ..FilterAllow::NONE
        },
    ) && matches!(f.controller, None | Some(ControllerRelation::You))
    {
        if f.subtypes.len() == 1
            && f.required_supertypes.is_empty()
            && (f.any_types.is_empty() || f.any_types == AstCardTypeMask::CREATURE)
            && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0])
        {
            return Ok(TapCostFilter::Subtype(ct));
        }
        if f.subtypes.is_empty() {
            if f.required_supertypes == AstSupertypeMask::LEGENDARY && f.any_types.is_empty() {
                return Ok(TapCostFilter::Legendary);
            }
            if f.required_supertypes.is_empty() && f.any_types == AstCardTypeMask::CREATURE {
                return Ok(TapCostFilter::Creature);
            }
            if f.required_supertypes.is_empty() && f.any_types == AstCardTypeMask::ARTIFACT {
                return Ok(TapCostFilter::Artifact);
            }
        }
    }
    let obj = lower_object_filter(f, vocabulary)?;
    Ok(TapCostFilter::Matching(obj))
}

pub(super) fn lower_trigger_spec(
    trig: &TriggerSpecAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(TriggerSpec, TriggerGrouping, Option<InterveningCondition>), &'static str> {
    if trig.events.is_empty() {
        return Err("oracle:empty-trigger");
    }
    let mut grouping = TriggerGrouping::EachOccurrence;
    let mut events = Vec::with_capacity(trig.events.len());
    let mut extra_cond: Option<TriggerCondition> = None;
    let own_main_phase = || {
        TriggerCondition::And(
            Box::new(TriggerCondition::ControllersTurn),
            Box::new(TriggerCondition::Or(
                Box::new(TriggerCondition::DuringStep(Step::PrecombatMain)),
                Box::new(TriggerCondition::DuringStep(Step::PostcombatMain)),
            )),
        )
    };
    let mut handled_single = false;
    if trig.events.len() == 1 {
        match &trig.events[0] {
            TriggerEventAst::KeywordActionPerformed { action, .. } => {
                let low = action.trim().to_ascii_lowercase();
                if let Some(cond_raw) = low.strip_prefix("~ attacks while ") {
                    let cond_trimmed = cond_raw.trim();
                    let while_cond = match cond_trimmed {
                        "you have the most life or are tied for most life"
                        | "you have the most life or are tied for the most life" => {
                            TriggerCondition::Not(Box::new(TriggerCondition::OpponentHasMoreLife))
                        }
                        _ => {
                            let compiler_vocab = cached_compiler_vocab(vocabulary);
                            let cond_ast = spellwake_oracle_compiler::filter::parse_condition(
                                cond_trimmed,
                                &compiler_vocab,
                            )
                            .map_err(|_| "oracle:unsupported-trigger-event")?;
                            lower_trigger_condition(&cond_ast, vocabulary)?
                        }
                    };
                    events.push(TriggerEvent::Attacks {
                        subject: ObjectFilter::self_object(),
                        mode: AttackTriggerMode::Individual {
                            capture_defender: false,
                        },
                    });
                    extra_cond = Some(while_cond);
                    handled_single = true;
                } else {
                    match low.as_str() {
                        "you cast an instant spell during your main phase" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::from_object(ObjectFilter::Clause(
                                    ObjectClause::instant(),
                                )),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(own_main_phase());
                            handled_single = true;
                        }
                        "you cast a sorcery spell during your main phase" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::from_object(ObjectFilter::Clause(
                                    ObjectClause::sorcery(),
                                )),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(own_main_phase());
                            handled_single = true;
                        }
                        "an instant or sorcery spell is cast during your turn" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(
                                    PlayerRelation::AnyParticipant,
                                ),
                                filter: SpellFilter::from_object(ObjectFilter::Clause(
                                    ObjectClause::any().any_types(
                                        CardTypeMask::INSTANT.union(CardTypeMask::SORCERY),
                                    ),
                                )),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(TriggerCondition::ControllersTurn);
                            handled_single = true;
                        }
                        "you cast your first spell during each of your turns" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::any().ordinal_this_turn(NonZeroU32::MIN),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(TriggerCondition::ControllersTurn);
                            handled_single = true;
                        }
                        "you cast a spell during your turn other than your first spell that turn" =>
                        {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::any(),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(TriggerCondition::And(
                                Box::new(TriggerCondition::ControllersTurn),
                                Box::new(TriggerCondition::ControllerCastSpellsThisTurn(2)),
                            ));
                            handled_single = true;
                        }
                        "~ becomes untapped during your untap step" => {
                            events.push(TriggerEvent::StatusChanged {
                                subject: ObjectFilter::self_object(),
                                kind: StatusTriggerKind::BecomesUntapped,
                            });
                            extra_cond = Some(TriggerCondition::And(
                                Box::new(TriggerCondition::ControllersTurn),
                                Box::new(TriggerCondition::DuringStep(Step::Untap)),
                            ));
                            handled_single = true;
                        }
                        "you gain life for the first time during each of your turns" => {
                            events.push(TriggerEvent::LifeChanged {
                                kind: LifeChangeKind::Gain,
                                player: PlayerRelation::Controller,
                            });
                            grouping = TriggerGrouping::OneOrMoreOnceEachTurn;
                            extra_cond = Some(TriggerCondition::ControllersTurn);
                            handled_single = true;
                        }
                        _ => {}
                    }
                }
            }
            TriggerEventAst::SpellCast {
                caster,
                spell: Some(f),
                self_spell: false,
                targets: None,
                nth_each_turn,
                include_copy,
            } if f.stat_bounds.len() == 1
                && f.stat_bounds[0].stat == StatKind::ManaValue
                && !matches!(
                    (f.stat_bounds[0].op, f.stat_bounds[0].value.as_ref()),
                    (CompareOp::Ge, ValueExprAst::Const(v)) if *v >= 0
                )
                && !matches!(
                    (f.stat_bounds[0].op, f.stat_bounds[0].value.as_ref()),
                    (CompareOp::Gt, ValueExprAst::Const(v)) if *v >= 0
                ) =>
            {
                let sb = &f.stat_bounds[0];
                if let Ok(rhs) = lower_value_expr(sb.value.as_ref(), vocabulary)
                    && rhs.is_trigger_world_evaluable()
                {
                    let mut stripped_filter = f.as_ref().clone();
                    stripped_filter.stat_bounds.clear();
                    let stripped_ev = TriggerEventAst::SpellCast {
                        caster: *caster,
                        spell: Some(Box::new(stripped_filter)),
                        self_spell: false,
                        targets: None,
                        nth_each_turn: *nth_each_turn,
                        include_copy: *include_copy,
                    };
                    let (e, g) = lower_trigger_event(&stripped_ev, vocabulary)?;
                    if g != TriggerGrouping::EachOccurrence {
                        grouping = g;
                    }
                    events.push(e);
                    extra_cond = Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::ObjectStat {
                            subject: ValueObjectRef::TriggeringSpell,
                            stat: ObjectStatKind::ManaValue,
                        },
                        comparison: lower_comparison(sb.op),
                        right: rhs,
                    })));
                    handled_single = true;
                }
            }
            _ => {}
        }
    }
    if !handled_single {
        for ev in &trig.events {
            let (e, g) = lower_trigger_event(ev, vocabulary)?;
            if g != TriggerGrouping::EachOccurrence {
                grouping = g;
            }
            events.push(e);
        }
    }
    let mut event = if events.len() == 1 {
        events.pop().ok_or("oracle:unreachable")?
    } else {
        TriggerEvent::AnyOf(events)
    };
    let mut consumed_if = false;
    if let (
        TriggerEvent::Enters {
            cast_origin: origin @ None,
            ..
        },
        Some(cond),
    ) = (&mut event, &trig.intervening_if)
    {
        match cond {
            ConditionAst::WasKicked => {
                *origin = Some(CastEntryKind::Kicked);
                consumed_if = true;
            }
            ConditionAst::WasCast { from_hand, .. } => {
                *origin = Some(CastEntryKind::Cast {
                    from_hand: *from_hand,
                });
                consumed_if = true;
            }
            ConditionAst::Marker(GameMarker::HasNotPlayedOrCast) => {
                *origin = Some(CastEntryKind::NotCast);
                consumed_if = true;
            }
            _ => {}
        }
    }
    if !consumed_if
        && let (
            TriggerEvent::DealsDamage {
                source,
                recipient: DamageTriggerRecipient::Opponent,
                combat_only: true,
            },
            Some(ConditionAst::PlayerActionThisTurn { action, .. }),
        ) = (&mut event, &trig.intervening_if)
        && *source == ObjectFilter::any()
        && let Some(flt_raw) = action
            .trim()
            .to_ascii_lowercase()
            .strip_prefix("any of that damage was dealt by ")
            .map(str::to_owned)
    {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        let parsed =
            spellwake_oracle_compiler::filter::parse_object_filter(flt_raw.trim(), &compiler_vocab)
                .map_err(|_| "oracle:unsupported-trigger-condition")?;
        *source = lower_object_filter(&parsed, vocabulary)?;
        grouping = TriggerGrouping::OneOrMoreTogether;
        consumed_if = true;
    }
    for ev in &trig.events {
        if let TriggerEventAst::Attacks {
            qualifier: Some(q), ..
        } = ev
        {
            let q_low = q.trim().to_ascii_lowercase();
            let parsed_cond = if q_low == "while_saddled" {
                Some(TriggerCondition::SourceSaddled)
            } else if let Some(cond_raw) = q_low.strip_prefix("while:") {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let cond_ast = spellwake_oracle_compiler::filter::parse_condition(
                    cond_raw.trim(),
                    &compiler_vocab,
                )
                .map_err(|_| "oracle:unsupported-attack-qualifier")?;
                Some(lower_trigger_condition(&cond_ast, vocabulary)?)
            } else {
                None
            };
            if let Some(c) = parsed_cond {
                extra_cond = Some(match extra_cond {
                    Some(prev) => TriggerCondition::And(Box::new(prev), Box::new(c)),
                    None => c,
                });
            }
        }
    }
    let mut spec = TriggerSpec::new(event);
    let mut guard = None;
    if let Some(ec) = extra_cond {
        spec = spec.only_if(ec);
    }
    if !consumed_if && let Some(cond) = &trig.intervening_if {
        let is_self_in_gy = |c: &ConditionAst| {
            matches!(
                c,
                ConditionAst::Marker(GameMarker::InZone {
                    subject,
                    zone: ZoneFilter::YourGraveyard,
                }) if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            )
        };
        let remaining = if is_self_in_gy(cond) {
            spec = spec.in_zone(spellwake_engine::program::TriggerZone::Graveyard);
            None
        } else if let ConditionAst::And(a, b) = cond {
            if is_self_in_gy(a) {
                spec = spec.in_zone(spellwake_engine::program::TriggerZone::Graveyard);
                Some(b.as_ref())
            } else if is_self_in_gy(b) {
                spec = spec.in_zone(spellwake_engine::program::TriggerZone::Graveyard);
                Some(a.as_ref())
            } else {
                Some(cond)
            }
        } else {
            Some(cond)
        };
        if let Some(rem) = remaining {
            let tc = lower_trigger_condition(rem, vocabulary)?;
            guard = InterveningCondition::from_trigger_condition(&tc);
            let combined = match spec.intervening_if.take() {
                Some(prev) => TriggerCondition::And(Box::new(prev), Box::new(tc)),
                None => tc,
            };
            spec = spec.only_if(combined);
        }
    }
    Ok((spec, grouping, guard))
}

pub(super) fn lower_trigger_event(
    ev: &TriggerEventAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(TriggerEvent, TriggerGrouping), &'static str> {
    match ev {
        TriggerEventAst::ZoneChange {
            from,
            to,
            subject,
            grouped,
            entering_controller,
        } => {
            let grouping = if *grouped {
                TriggerGrouping::OneOrMoreTogether
            } else {
                TriggerGrouping::EachOccurrence
            };
            let mut filter = lower_trigger_subject_filter(subject, vocabulary)?;
            if let Some(ctrl) = entering_controller {
                let cf = match ctrl {
                    ControllerRelation::You => ControllerFilter::You,
                    ControllerRelation::Opponent | ControllerRelation::Opponents => {
                        ControllerFilter::Opponent
                    }
                    ControllerRelation::NotYou => ControllerFilter::NotYou,
                    ControllerRelation::ThatPlayer | ControllerRelation::DefendingPlayer => {
                        ControllerFilter::RelatedPlayer
                    }
                    _ => return Err("oracle:trigger-entering-controller"),
                };
                match &mut filter {
                    ObjectFilter::Clause(c) => {
                        c.controller = cf;
                    }
                    ObjectFilter::Either(c1, c2) => {
                        c1.controller = cf;
                        c2.controller = cf;
                    }
                }
            }
            let from_norm = from.as_deref().map(|s| {
                let low = s.trim().to_ascii_lowercase();
                match low.as_str() {
                    "the battlefield" | "play" => "battlefield".to_owned(),
                    "anywhere" => "any".to_owned(),
                    "your graveyard" => "your_graveyard".to_owned(),
                    "opponent's graveyard"
                    | "an opponent's graveyard"
                    | "opponents' graveyards"
                    | "your opponents' graveyards" => "opponent_graveyard".to_owned(),
                    "a graveyard"
                    | "a player's graveyard"
                    | "any graveyard"
                    | "graveyards"
                    | "its owner's graveyard"
                    | "their graveyard"
                    | "their graveyards"
                    | "their owner's graveyard"
                    | "their owners' graveyards" => "graveyard".to_owned(),
                    "your hand" => "your_hand".to_owned(),
                    "an opponent's hand" | "opponent's hand" => "opponent_hand".to_owned(),
                    "a hand"
                    | "a player's hand"
                    | "their hand"
                    | "its owner's hand"
                    | "their owner's hand"
                    | "their owners' hands" => "hand".to_owned(),
                    "your library"
                    | "your_library"
                    | "opponent_library"
                    | "any_library"
                    | "a library"
                    | "any library"
                    | "a player's library"
                    | "their library"
                    | "their libraries"
                    | "an opponent's library"
                    | "its owner's library"
                    | "their owner's library"
                    | "their owners' libraries" => "library".to_owned(),
                    "exile" | "the exile zone" => "exile".to_owned(),
                    "the stack" | "stack" => "stack".to_owned(),
                    "anywhere other than a graveyard or exile" | "not_graveyard_or_exile" => {
                        "not_graveyard_or_exile".to_owned()
                    }
                    _ => low,
                }
            });
            let to_norm = to.as_deref().map(|s| {
                let low = s.trim().to_ascii_lowercase();
                match low.as_str() {
                    "the battlefield" | "play" => "battlefield".to_owned(),
                    "your graveyard" => "your_graveyard".to_owned(),
                    "opponent's graveyard"
                    | "an opponent's graveyard"
                    | "opponents' graveyards"
                    | "your opponents' graveyards" => "opponent_graveyard".to_owned(),
                    "a graveyard"
                    | "a player's graveyard"
                    | "any graveyard"
                    | "graveyards"
                    | "its owner's graveyard"
                    | "their graveyard"
                    | "their graveyards"
                    | "their owner's graveyard"
                    | "their owners' graveyards" => "graveyard".to_owned(),
                    "your hand" => "your_hand".to_owned(),
                    "an opponent's hand" | "opponent's hand" => "opponent_hand".to_owned(),
                    "its owner's hand"
                    | "their owner's hand"
                    | "their owners' hands"
                    | "their hand"
                    | "a hand"
                    | "a player's hand"
                    | "owner's hand"
                    | "hands" => "hand".to_owned(),
                    "your library"
                    | "your_library"
                    | "a library"
                    | "any library"
                    | "a player's library"
                    | "an opponent's library"
                    | "its owner's library"
                    | "their owner's library"
                    | "their owners' libraries"
                    | "their library"
                    | "their libraries" => "library".to_owned(),
                    "exile" | "the exile zone" => "exile".to_owned(),
                    _ => low,
                }
            });
            if matches!(
                from_norm.as_deref(),
                Some("hand" | "your_hand" | "opponent_hand")
            ) && matches!(
                to_norm.as_deref(),
                Some("graveyard" | "any_graveyard" | "your_graveyard" | "opponent_graveyard")
            ) && entering_controller.is_none()
                && let SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) =
                    subject.as_ref()
                && f.zone.is_none()
            {
                let mut stripped = f.as_ref().clone();
                let ctrl_rel = stripped.controller;
                stripped.controller = None;
                stripped.any_card = false;
                let from_rel = match from_norm.as_deref() {
                    Some("your_hand") => Some(PlayerRelation::Controller),
                    Some("opponent_hand") => Some(PlayerRelation::Opponent),
                    _ => None,
                };
                let to_rel = match to_norm.as_deref() {
                    Some("your_graveyard") => Some(PlayerRelation::Controller),
                    Some("opponent_graveyard") => Some(PlayerRelation::Opponent),
                    _ => None,
                };
                let subj_rel = match ctrl_rel {
                    Some(ControllerRelation::You) => Some(PlayerRelation::Controller),
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        Some(PlayerRelation::Opponent)
                    }
                    None => None,
                    _ => return Err("oracle:unsupported-zone-trigger"),
                };
                let mut chosen: Option<PlayerRelation> = None;
                let mut conflict = false;
                for rel in [from_rel, to_rel, subj_rel].into_iter().flatten() {
                    if let Some(prev) = chosen {
                        if prev != rel {
                            conflict = true;
                        }
                    } else {
                        chosen = Some(rel);
                    }
                }
                if !conflict {
                    let player = chosen.unwrap_or(PlayerRelation::AnyParticipant);
                    if is_unrestricted_filter(&stripped) {
                        return Ok((
                            TriggerEvent::PlayerAction {
                                player,
                                action: PlayerTriggerAction::Discarded,
                            },
                            grouping,
                        ));
                    }
                    if let Ok(obj) = lower_object_filter(&stripped, vocabulary) {
                        return Ok((
                            TriggerEvent::PlayerAction {
                                player,
                                action: PlayerTriggerAction::DiscardedMatching(obj),
                            },
                            grouping,
                        ));
                    }
                }
            }
            match (from_norm.as_deref(), to_norm.as_deref()) {
                (None | Some("any"), Some("battlefield" | "battlefield_transformed")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Any,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("your_graveyard"), Some("battlefield" | "battlefield_transformed")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::ControllerGraveyard,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("opponent_graveyard"), Some("battlefield" | "battlefield_transformed")) => {
                    Ok((
                        TriggerEvent::Enters {
                            subject: filter,
                            from_zone: TriggerFromZone::OpponentGraveyard,
                            cast_origin: None,
                            attached_to_creature: false,
                        },
                        grouping,
                    ))
                }
                (
                    Some("graveyard" | "any_graveyard"),
                    Some("battlefield" | "battlefield_transformed"),
                ) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Graveyard,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (
                    Some("hand" | "your_hand" | "opponent_hand"),
                    Some("battlefield" | "battlefield_transformed"),
                ) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Hand,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("exile"), Some("battlefield" | "battlefield_transformed")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Exile,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("library"), Some("battlefield" | "battlefield_transformed")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::Library,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (
                    Some("not_graveyard_or_exile"),
                    Some("battlefield" | "battlefield_transformed"),
                ) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::NotGraveyardOrExile,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (Some("battlefield"), None) => Ok((
                    TriggerEvent::LeavesBattlefield { subject: filter },
                    grouping,
                )),
                (Some("battlefield"), Some("not_graveyard")) => Ok((
                    TriggerEvent::LeavesBattlefieldWithoutDying { subject: filter },
                    grouping,
                )),
                (Some("battlefield"), Some("graveyard_or_exile")) => {
                    let mode = if filter.guarantees_creature() {
                        DiesTriggerMode::Creature
                    } else {
                        DiesTriggerMode::Permanent
                    };
                    Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::Dies {
                                subject: filter,
                                mode,
                            },
                            TriggerEvent::Exiled {
                                subject: filter,
                                from_zone: TriggerFromZone::Battlefield,
                            },
                        ]),
                        grouping,
                    ))
                }
                (Some("your_graveyard"), None) => Ok((
                    TriggerEvent::LeavesGraveyard {
                        subject: filter,
                        player: PlayerRelation::Controller,
                    },
                    grouping,
                )),
                (Some("opponent_graveyard"), None) => Ok((
                    TriggerEvent::LeavesGraveyard {
                        subject: filter,
                        player: PlayerRelation::Opponent,
                    },
                    grouping,
                )),
                (Some("any_graveyard" | "graveyard"), None) => Ok((
                    TriggerEvent::LeavesGraveyard {
                        subject: filter,
                        player: PlayerRelation::AnyParticipant,
                    },
                    grouping,
                )),
                (Some("battlefield"), Some("hand" | "your_hand" | "opponent_hand")) => {
                    Ok((TriggerEvent::ReturnedToHand { subject: filter }, grouping))
                }
                (from_z, Some("exile")) => {
                    let fz = match from_z {
                        None | Some("any") => TriggerFromZone::Any,
                        Some("battlefield") => TriggerFromZone::Battlefield,
                        Some("your_graveyard") => TriggerFromZone::ControllerGraveyard,
                        Some("opponent_graveyard") => TriggerFromZone::OpponentGraveyard,
                        Some("graveyard" | "any_graveyard") => TriggerFromZone::Graveyard,
                        Some("hand" | "your_hand" | "opponent_hand") => TriggerFromZone::Hand,
                        Some("library") => TriggerFromZone::Library,
                        Some("not_graveyard_or_exile") => TriggerFromZone::NotGraveyardOrExile,
                        _ => return Err("oracle:unsupported-zone-trigger"),
                    };
                    Ok((
                        TriggerEvent::Exiled {
                            subject: filter,
                            from_zone: fz,
                        },
                        grouping,
                    ))
                }
                (Some("battlefield"), Some("graveyard_or_command")) => Ok((
                    TriggerEvent::Dies {
                        subject: filter,
                        mode: DiesTriggerMode::OrCommandZone,
                    },
                    grouping,
                )),
                (Some("battlefield"), Some("your_graveyard")) => {
                    let mode = if filter.guarantees_creature() {
                        DiesTriggerMode::ToControllerGraveyard
                    } else {
                        DiesTriggerMode::Permanent
                    };
                    Ok((
                        TriggerEvent::Dies {
                            subject: filter,
                            mode,
                        },
                        grouping,
                    ))
                }
                (Some("battlefield"), Some("opponent_graveyard")) => {
                    let mut opp_filter = filter;
                    match &mut opp_filter {
                        ObjectFilter::Clause(c) => {
                            if c.controller == ControllerFilter::Any {
                                c.controller = ControllerFilter::OwnerOpponent;
                            }
                        }
                        ObjectFilter::Either(c1, c2) => {
                            if c1.controller == ControllerFilter::Any {
                                c1.controller = ControllerFilter::OwnerOpponent;
                            }
                            if c2.controller == ControllerFilter::Any {
                                c2.controller = ControllerFilter::OwnerOpponent;
                            }
                        }
                    }
                    let mode = if opp_filter.guarantees_creature() {
                        DiesTriggerMode::Creature
                    } else {
                        DiesTriggerMode::Permanent
                    };
                    Ok((
                        TriggerEvent::Dies {
                            subject: opp_filter,
                            mode,
                        },
                        grouping,
                    ))
                }
                (Some("battlefield"), Some("graveyard" | "any_graveyard")) => {
                    let mode = if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                        DiesTriggerMode::Creature
                    } else if let ObjectFilter::Clause(c) = filter
                        && !c.any_types.contains(CardType::Creature)
                        && !c.any_types.is_empty()
                    {
                        DiesTriggerMode::Permanent
                    } else {
                        DiesTriggerMode::Creature
                    };
                    Ok((
                        TriggerEvent::Dies {
                            subject: filter,
                            mode,
                        },
                        grouping,
                    ))
                }
                (
                    Some("library"),
                    Some(
                        to_gy @ ("graveyard" | "any_graveyard" | "your_graveyard"
                        | "opponent_graveyard"),
                    ),
                ) => {
                    let player = match to_gy {
                        "your_graveyard" => PlayerRelation::Controller,
                        "opponent_graveyard" => PlayerRelation::Opponent,
                        _ => PlayerRelation::AnyParticipant,
                    };
                    Ok((
                        TriggerEvent::PutIntoGraveyard {
                            subject: filter,
                            player,
                            from_library: true,
                            own_turn_only: false,
                        },
                        grouping,
                    ))
                }
                (
                    None | Some("any"),
                    Some(
                        to_gy @ ("graveyard" | "any_graveyard" | "your_graveyard"
                        | "opponent_graveyard"),
                    ),
                ) => {
                    let player = match to_gy {
                        "your_graveyard" => PlayerRelation::Controller,
                        "opponent_graveyard" => PlayerRelation::Opponent,
                        _ => PlayerRelation::AnyParticipant,
                    };
                    Ok((
                        TriggerEvent::PutIntoGraveyard {
                            subject: filter,
                            player,
                            from_library: false,
                            own_turn_only: false,
                        },
                        grouping,
                    ))
                }
                (_, Some("library")) => {
                    Ok((TriggerEvent::PutIntoLibrary { subject: filter }, grouping))
                }
                _ => Err("oracle:unsupported-zone-trigger"),
            }
        }
        TriggerEventAst::Exiled { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::Exiled {
                    subject: subj,
                    from_zone: TriggerFromZone::Any,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::PlayerAttacked { player } => {
            let mode = match player {
                PlayerRefAst::You => AttackTriggerMode::AtController,
                PlayerRefAst::EnchantedPlayer => AttackTriggerMode::AtEnchantedPlayer,
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    AttackTriggerMode::AtOpponent
                }
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => AttackTriggerMode::AtPlayer,
                _ => return Err("oracle:unsupported-trigger-event"),
            };
            let grouping = if mode == AttackTriggerMode::AtEnchantedPlayer {
                TriggerGrouping::OneOrMoreByPlayer
            } else {
                TriggerGrouping::OneOrMoreTogether
            };
            Ok((
                TriggerEvent::Attacks {
                    subject: ObjectFilter::creature(),
                    mode,
                },
                grouping,
            ))
        }
        TriggerEventAst::StepBegan { step, whose } => {
            let whose_low = whose.trim().to_ascii_lowercase();
            match whose_low.as_str() {
                "each of your precombat main phases"
                | "your precombat main phase"
                | "your first main phase"
                | "each of your first main phases" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::PrecombatMain,
                            player: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "each of your postcombat main phases"
                | "your postcombat main phase"
                | "your second main phase"
                | "each of your second main phases" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::PostcombatMain,
                            player: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "each of your main phases" | "your main phase" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::StepBegan {
                                step: Step::PrecombatMain,
                                player: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                            },
                            TriggerEvent::StepBegan {
                                step: Step::PostcombatMain,
                                player: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "each precombat main phase"
                | "each player's precombat main phase"
                | "each player's first main phase" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::PrecombatMain,
                            player: TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "each postcombat main phase"
                | "each player's postcombat main phase"
                | "each player's second main phase" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::PostcombatMain,
                            player: TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "each of enchanted player's postcombat main phases"
                | "enchanted player's postcombat main phase" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::PostcombatMain,
                            player: TriggerPlayerScope::EnchantedPlayer,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "combat on your turn" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::BeginningCombat,
                            player: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "combat on each player's turn" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::BeginningCombat,
                            player: TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "combat on each opponent's turn" | "combat on an opponent's turn" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::BeginningCombat,
                            player: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "combat on enchanted player's turn" | "combat on enchanted opponent's turn" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::BeginningCombat,
                            player: TriggerPlayerScope::EnchantedPlayer,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                _ => {}
            }
            let scope = match whose_low.as_str() {
                "your" | "you" | "your_next" | "your turn" | "each of your" => {
                    TriggerPlayerScope::Relation(PlayerRelation::Controller)
                }
                "each" | "each_player" | "each player's" | "any" | "next" | "every"
                | "each turn" | "each player's turn" => {
                    TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant)
                }
                "each_opponent"
                | "each opponent's"
                | "your opponents'"
                | "opponents'"
                | "an opponent's"
                | "opponent"
                | "opponent's"
                | "each_other_player"
                | "each other player's"
                | "each opponent's turn"
                | "an opponent's turn" => TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                "enchanted_player"
                | "enchanted player"
                | "enchanted player's"
                | "enchanted player's turn"
                | "enchanted_opponent"
                | "enchanted opponent"
                | "enchanted opponent's"
                | "enchanted opponent's turn" => TriggerPlayerScope::EnchantedPlayer,
                "enchanted creature's controller"
                | "enchanted creature's controller's"
                | "enchanted permanent's controller"
                | "enchanted permanent's controller's"
                | "enchanted land's controller"
                | "enchanted land's controller's"
                | "enchanted artifact's controller"
                | "enchanted artifact's controller's"
                | "enchanted enchantment's controller"
                | "enchanted enchantment's controller's"
                | "enchanted planeswalker's controller"
                | "enchanted planeswalker's controller's"
                | "enchanted battle's controller"
                | "enchanted battle's controller's"
                | "equipped creature's controller"
                | "equipped creature's controller's"
                | "the controller of enchanted creature"
                | "the controller of enchanted permanent"
                | "the controller of enchanted land"
                | "the controller of enchanted artifact"
                | "the controller of enchanted enchantment"
                | "the controller of enchanted planeswalker"
                | "the controller of equipped creature"
                | "attached_host_controller" => TriggerPlayerScope::AttachedHostController,
                "monarch" | "the monarch" | "the monarch's" => TriggerPlayerScope::Monarch,
                _ => return Err("oracle:unsupported-step-whose"),
            };
            if matches!(step, TurnStepAst::MainPhase) {
                return Ok((
                    TriggerEvent::AnyOf(vec![
                        TriggerEvent::StepBegan {
                            step: Step::PrecombatMain,
                            player: scope,
                        },
                        TriggerEvent::StepBegan {
                            step: Step::PostcombatMain,
                            player: scope,
                        },
                    ]),
                    TriggerGrouping::EachOccurrence,
                ));
            }
            let st = match step {
                TurnStepAst::UntapStep => Step::Untap,
                TurnStepAst::Upkeep => Step::Upkeep,
                TurnStepAst::DrawStep => Step::Draw,
                TurnStepAst::BeginningOfCombat => Step::BeginningCombat,
                TurnStepAst::EndOfCombat => Step::EndCombat,
                TurnStepAst::EndStep => Step::End,
                TurnStepAst::CleanupStep => Step::Cleanup,
                TurnStepAst::PrecombatMain => Step::PrecombatMain,
                TurnStepAst::PostcombatMain => Step::PostcombatMain,
                _ => return Err("oracle:unsupported-trigger-step"),
            };
            Ok((
                TriggerEvent::StepBegan {
                    step: st,
                    player: scope,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::CastFromOutsideHand { player } => {
            let rel = lower_player_relation(*player)?;
            Ok((
                TriggerEvent::SpellCast {
                    caster: TriggerPlayerScope::Relation(rel),
                    filter: SpellFilter::any().not_from_owners_hand(),
                    self_cast: false,
                    include_copy: false,
                    min_mana_value: None,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::SpellCast {
            caster,
            spell,
            self_spell,
            targets,
            nth_each_turn,
            include_copy,
        } => {
            let scope = match caster {
                PlayerRefAst::You => TriggerPlayerScope::Relation(PlayerRelation::Controller),
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    TriggerPlayerScope::Relation(PlayerRelation::Opponent)
                }
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => {
                    TriggerPlayerScope::Relation(PlayerRelation::AnyParticipant)
                }
                _ => return Err("oracle:unsupported-spell-caster"),
            };
            let (sf, min_mv) = match spell {
                Some(f) => {
                    let mut cloned = f.as_ref().clone();
                    let mut min_mv = None;
                    if cloned.stat_bounds.len() == 1
                        && cloned.stat_bounds[0].stat == StatKind::ManaValue
                        && let ValueExprAst::Const(v) = cloned.stat_bounds[0].value.as_ref()
                    {
                        match cloned.stat_bounds[0].op {
                            CompareOp::Ge => {
                                if *v == 0 {
                                    cloned.stat_bounds.clear();
                                } else if let Ok(u) = u16::try_from(*v)
                                    && u > 0
                                {
                                    min_mv = Some(u);
                                    cloned.stat_bounds.clear();
                                }
                            }
                            CompareOp::Gt => {
                                if *v >= 0
                                    && let Some(next_v) = v.checked_add(1)
                                    && let Ok(u) = u16::try_from(next_v)
                                    && u > 0
                                {
                                    min_mv = Some(u);
                                    cloned.stat_bounds.clear();
                                }
                            }
                            _ => {}
                        }
                    }
                    (lower_spell_filter(&cloned, vocabulary)?, min_mv)
                }
                None => (SpellFilter::any(), None),
            };
            let mut sf = sf;
            match targets {
                None => {}
                Some(t) if matches!(t.as_ref(), SubjectRefAst::SelfObject) => {
                    sf = sf.targeting_only_source();
                }
                Some(t) if !*self_spell && nth_each_turn.is_none() && min_mv.is_none() => {
                    let inst_sorc_only = match spell {
                        None => Some(false),
                        Some(f) => {
                            let mut stripped = f.as_ref().clone();
                            stripped.any_spell = false;
                            if is_unrestricted_filter(&stripped) {
                                Some(false)
                            } else if stripped.any_types
                                == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY)
                                && filter_only_has(
                                    &stripped,
                                    FilterAllow {
                                        types: true,
                                        ..FilterAllow::NONE
                                    },
                                )
                            {
                                Some(true)
                            } else {
                                None
                            }
                        }
                    };
                    if let Some(inst_sorc) = inst_sorc_only
                        && let Ok(tgt_filter) = lower_trigger_subject_filter(t, vocabulary)
                    {
                        let event = match caster {
                            PlayerRefAst::You => TriggerEvent::TargetedByController {
                                subject: tgt_filter,
                                spell_only: true,
                                instant_or_sorcery_only: inst_sorc,
                            },
                            PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                                TriggerEvent::Targeted {
                                    subject: tgt_filter,
                                    by_opponent: true,
                                    spell_only: true,
                                    instant_or_sorcery_only: inst_sorc,
                                }
                            }
                            PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => {
                                TriggerEvent::Targeted {
                                    subject: tgt_filter,
                                    by_opponent: false,
                                    spell_only: true,
                                    instant_or_sorcery_only: inst_sorc,
                                }
                            }
                            _ => return Err("oracle:unsupported-spell-target"),
                        };
                        return Ok((event, TriggerGrouping::OneOrMoreTogether));
                    }
                    return Err("oracle:unsupported-spell-target");
                }
                Some(_) => return Err("oracle:unsupported-spell-target"),
            }
            if let Some(nth) = nth_each_turn {
                let nz = NonZeroU32::new(u32::from(*nth)).ok_or("oracle:zero-nth")?;
                sf = sf.ordinal_this_turn(nz);
            }
            Ok((
                TriggerEvent::SpellCast {
                    caster: scope,
                    filter: sf,
                    self_cast: *self_spell,
                    include_copy: *include_copy,
                    min_mana_value: min_mv,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Attacks {
            subject,
            actor,
            qualifier,
            grouped,
        } => {
            let mut subj = match (subject, actor) {
                (Some(s), _) => lower_trigger_subject_filter(s, vocabulary)?,
                (None, None | Some(PlayerRefAst::You)) => ObjectFilter::creature().controlled(),
                (None, Some(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent)) => {
                    ObjectFilter::creature().opposing()
                }
                (None, Some(PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer)) => {
                    ObjectFilter::creature()
                }
                _ => return Err("oracle:unsupported-trigger-event"),
            };
            let mut grouping = if *grouped {
                TriggerGrouping::OneOrMoreTogether
            } else {
                TriggerGrouping::EachOccurrence
            };
            let q_low = qualifier.as_deref().map(|q| q.trim().to_ascii_lowercase());
            let mode = match q_low.as_deref() {
                None => AttackTriggerMode::Individual {
                    capture_defender: false,
                },
                Some("while_saddled") => AttackTriggerMode::Individual {
                    capture_defender: false,
                },
                Some(q) if q.starts_with("while:") => AttackTriggerMode::Individual {
                    capture_defender: false,
                },
                Some("alone") => AttackTriggerMode::Alone,
                Some("you_or_planeswalker" | "you or a planeswalker you control") => {
                    AttackTriggerMode::AtControllerOrPlaneswalker
                }
                Some(
                    "opponent_or_planeswalker"
                    | "one of your opponents or a planeswalker an opponent controls"
                    | "one of your opponents or a planeswalker they control",
                ) => AttackTriggerMode::AtOpponentOrPlaneswalker,
                Some("you") => AttackTriggerMode::AtController,
                Some("opponent" | "an opponent" | "one of your opponents") => {
                    AttackTriggerMode::AtOpponent
                }
                Some("player" | "a player" | "one or more players") => AttackTriggerMode::AtPlayer,
                Some("planeswalker" | "a planeswalker") => AttackTriggerMode::AtPlaneswalker,
                Some("enchanted_player" | "enchanted player") => {
                    AttackTriggerMode::AtEnchantedPlayer
                }
                Some("first_time_each_turn") => AttackTriggerMode::FirstThisTurn {
                    highest_life: false,
                },
                Some("two_or_more" | "two or more creatures" | "2 or more creatures") => {
                    AttackTriggerMode::GroupAtLeast(2)
                }
                Some("three or more creatures" | "3 or more creatures") => {
                    AttackTriggerMode::GroupAtLeast(3)
                }
                Some("four or more creatures" | "4 or more creatures") => {
                    AttackTriggerMode::GroupAtLeast(4)
                }
                Some("five or more creatures" | "5 or more creatures") => {
                    AttackTriggerMode::GroupAtLeast(5)
                }
                Some("one or more equipped creatures") => AttackTriggerMode::EquippedAtPlayer,
                Some(
                    "the most life"
                    | "the most life or tied for most life"
                    | "the most life or tied for the most life",
                ) => AttackTriggerMode::HighestLifeDefender,
                Some(other) => {
                    if let Some(n_str) = other
                        .strip_prefix("creatures with total power ")
                        .and_then(|r| r.strip_suffix(" or greater"))
                        && let Ok(n) = n_str.trim().parse::<u16>()
                        && n > 0
                    {
                        AttackTriggerMode::GroupTotalPowerAtLeast(n)
                    } else if let Some((def_raw, with_raw)) = other.split_once(" with one or more ")
                    {
                        let def_mode = match def_raw.trim() {
                            "enchanted player" => AttackTriggerMode::AtEnchantedPlayer,
                            "you" => AttackTriggerMode::AtController,
                            "you or a planeswalker you control" => {
                                AttackTriggerMode::AtControllerOrPlaneswalker
                            }
                            "an opponent" | "one of your opponents" => {
                                AttackTriggerMode::AtOpponent
                            }
                            "a player" => AttackTriggerMode::AtPlayer,
                            _ => return Err("oracle:unsupported-attack-qualifier"),
                        };
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        if with_raw.trim() != "creatures" {
                            let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                                with_raw.trim(),
                                &compiler_vocab,
                            )
                            .map_err(|_| "oracle:unsupported-attack-qualifier")?;
                            let mut f = lower_object_filter(&parsed, vocabulary)
                                .map_err(|_| "oracle:unsupported-attack-qualifier")?;
                            if matches!(actor, Some(PlayerRefAst::You)) {
                                f = f.controlled();
                            } else if matches!(
                                actor,
                                Some(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent)
                            ) {
                                f = f.opposing();
                            }
                            subj = f;
                        }
                        grouping = if matches!(
                            def_mode,
                            AttackTriggerMode::AtPlayer
                                | AttackTriggerMode::AtOpponent
                                | AttackTriggerMode::AtEnchantedPlayer
                                | AttackTriggerMode::AtOpponentOrPlaneswalker
                        ) {
                            TriggerGrouping::OneOrMoreByPlayer
                        } else {
                            TriggerGrouping::OneOrMoreTogether
                        };
                        def_mode
                    } else if let Some((count_prefix, flt_raw)) = [
                        ("two or more ", 2u8),
                        ("2 or more ", 2u8),
                        ("three or more ", 3u8),
                        ("3 or more ", 3u8),
                        ("four or more ", 4u8),
                        ("4 or more ", 4u8),
                        ("five or more ", 5u8),
                        ("5 or more ", 5u8),
                    ]
                    .into_iter()
                    .find_map(|(pfx, n)| other.strip_prefix(pfx).map(|r| (n, r.trim())))
                    {
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                            flt_raw,
                            &compiler_vocab,
                        )
                        .map_err(|_| "oracle:unsupported-attack-qualifier")?;
                        let mut f = lower_object_filter(&parsed, vocabulary)
                            .map_err(|_| "oracle:unsupported-attack-qualifier")?;
                        if matches!(actor, None | Some(PlayerRefAst::You)) {
                            f = f.controlled();
                        } else if matches!(
                            actor,
                            Some(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent)
                        ) {
                            f = f.opposing();
                        }
                        subj = f;
                        grouping = TriggerGrouping::OneOrMoreTogether;
                        AttackTriggerMode::GroupAtLeast(count_prefix)
                    } else {
                        return Err("oracle:unsupported-attack-qualifier");
                    }
                }
            };
            if *grouped
                && matches!(
                    mode,
                    AttackTriggerMode::AtPlayer
                        | AttackTriggerMode::AtOpponent
                        | AttackTriggerMode::AtEnchantedPlayer
                        | AttackTriggerMode::AtOpponentOrPlaneswalker
                )
            {
                grouping = TriggerGrouping::OneOrMoreByPlayer;
            }
            Ok((
                TriggerEvent::Attacks {
                    subject: subj,
                    mode,
                },
                grouping,
            ))
        }
        TriggerEventAst::AttacksWithOthers {
            subject,
            min_others,
            other_filter,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            && (other_filter.is_empty()
                || (other_filter.any_types == AstCardTypeMask::CREATURE
                    && filter_only_has(
                        other_filter,
                        FilterAllow {
                            types: true,
                            controller: true,
                            flags: true,
                            ..FilterAllow::NONE
                        },
                    )
                    && matches!(
                        other_filter.controller,
                        None | Some(ControllerRelation::You)
                    )
                    && (other_filter.flags.is_empty()
                        || other_filter.flags == AstFilterFlags::EXCLUDE_SELF))) =>
        {
            Ok((
                TriggerEvent::Attacks {
                    subject: ObjectFilter::self_object(),
                    mode: AttackTriggerMode::WithAtLeastOthers(*min_others),
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::AttacksUnblocked { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind: BlockTriggerKind::Unblocked,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Blocks { subject, attacker } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            let kind = match attacker {
                None => BlockTriggerKind::Blocks,
                Some(atk) => BlockTriggerKind::BlocksMatching(Box::new(
                    lower_trigger_subject_filter(atk, vocabulary)?,
                )),
            };
            Ok((
                TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::BecomesBlocked { subject, blocker } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            let kind = match blocker {
                None => BlockTriggerKind::BecomesBlocked {
                    capture_defender: false,
                },
                Some(blk) => BlockTriggerKind::BlockedByMatching(Box::new(
                    lower_trigger_subject_filter(blk, vocabulary)?,
                )),
            };
            Ok((
                TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::BlocksOrBecomesBlockedBy {
            subject,
            other_filter,
            blocks,
            becomes_blocked,
            min_count,
        } if *min_count == 1 && other_filter.zone.is_none() => {
            let subj = match subject.as_ref() {
                SubjectRefAst::SelfObject => ObjectFilter::self_object(),
                SubjectRefAst::AttachedHost(_) => ObjectFilter::any().attached_to_self(),
                _ => return Err("oracle:unsupported-trigger-event"),
            };
            let obj = lower_object_filter(other_filter, vocabulary)?;
            let event = match (*blocks, *becomes_blocked) {
                (true, false) => TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind: BlockTriggerKind::BlocksMatching(Box::new(obj)),
                },
                (false, true) => TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind: BlockTriggerKind::BlockedByMatching(Box::new(obj)),
                },
                (true, true) => {
                    let simple_subtype = if let ObjectFilter::Clause(c) = obj
                        && (c.any_types.is_empty() || c.any_types == CardTypeMask::CREATURE)
                    {
                        let base = ObjectClause {
                            any_types: c.any_types,
                            subtypes: c.subtypes,
                            ..ObjectClause::any()
                        };
                        if c == base {
                            match c.subtypes {
                                SubtypeFilter::Any => Some(None),
                                SubtypeFilter::Has(st) => Some(Some(st)),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(required_subtype) = simple_subtype {
                        TriggerEvent::BlocksOrBlocked {
                            subject: subj,
                            kind: BlockTriggerKind::BlocksOrBlockedByEach { required_subtype },
                        }
                    } else {
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::BlocksOrBlocked {
                                subject: subj,
                                kind: BlockTriggerKind::BlocksMatching(Box::new(obj)),
                            },
                            TriggerEvent::BlocksOrBlocked {
                                subject: subj,
                                kind: BlockTriggerKind::BlockedByMatching(Box::new(obj)),
                            },
                        ])
                    }
                }
                (false, false) => return Err("oracle:unsupported-trigger-event"),
            };
            Ok((event, TriggerGrouping::EachOccurrence))
        }
        TriggerEventAst::DamageDealt {
            combat_only,
            noncombat_only,
            source,
            recipient,
            grouped,
            excess_only: false,
        } => {
            // "Whenever ~ / a creature you control is dealt damage..." -> TriggerEvent::DealtDamage
            if source.is_none()
                && !*combat_only
                && !*noncombat_only
                && !*grouped
                && let Some(rec_subj) = recipient.as_deref()
                && !matches!(
                    rec_subj,
                    SubjectRefAst::Player(_) | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                )
                && let Ok(subj_filter) = lower_trigger_subject_filter(rec_subj, vocabulary)
            {
                return Ok((
                    TriggerEvent::DealtDamage {
                        subject: subj_filter,
                        linked_player: false,
                    },
                    TriggerGrouping::EachOccurrence,
                ));
            }
            let src = match source {
                Some(s) => lower_trigger_subject_filter(s, vocabulary)?,
                None => ObjectFilter::any(),
            };
            let rec = match recipient.as_deref() {
                None => DamageTriggerRecipient::Any,
                Some(SubjectRefAst::SelfObject) => DamageTriggerRecipient::SelfObject,
                Some(SubjectRefAst::Player(
                    PlayerRefAst::AnyPlayer
                    | PlayerRefAst::EachPlayer
                    | PlayerRefAst::TargetPlayer
                    | PlayerRefAst::ThatPlayer,
                )) => DamageTriggerRecipient::AnyPlayer,
                Some(SubjectRefAst::Player(
                    PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
                    | PlayerRefAst::DefendingPlayer,
                )) => DamageTriggerRecipient::Opponent,
                Some(SubjectRefAst::Player(PlayerRefAst::You)) => {
                    DamageTriggerRecipient::Controller
                }
                Some(SubjectRefAst::Player(PlayerRefAst::EnchantedPlayer)) => {
                    DamageTriggerRecipient::EnchantedPlayer
                }
                Some(SubjectRefAst::TargetPlayerOrPlaneswalker {
                    opponent_only: true,
                }) => DamageTriggerRecipient::OpponentOrPlaneswalker,
                Some(SubjectRefAst::TargetPlayerOrPlaneswalker {
                    opponent_only: false,
                }) => DamageTriggerRecipient::PlayerOrPlaneswalker,
                Some(SubjectRefAst::Compound(items))
                    if items.len() == 2
                        && items.iter().any(|s| {
                            matches!(
                                s,
                                SubjectRefAst::Group(f)
                                    | SubjectRefAst::Each(f)
                                    | SubjectRefAst::All(f)
                                    if f.any_types == AstCardTypeMask::PLANESWALKER
                            )
                        }) =>
                {
                    if items
                        .iter()
                        .any(|s| matches!(s, SubjectRefAst::Player(PlayerRefAst::You)))
                    {
                        DamageTriggerRecipient::ControllerOrPlaneswalker
                    } else if items.iter().any(|s| {
                        matches!(
                            s,
                            SubjectRefAst::Player(
                                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent
                            )
                        )
                    }) {
                        DamageTriggerRecipient::OpponentOrPlaneswalker
                    } else {
                        DamageTriggerRecipient::PlayerOrPlaneswalker
                    }
                }
                Some(SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f))
                    if f.any_types == AstCardTypeMask::CREATURE
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                ..FilterAllow::NONE
                            },
                        ) =>
                {
                    DamageTriggerRecipient::Creature
                }
                Some(SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f))
                    if f.any_types == AstCardTypeMask::BATTLE =>
                {
                    DamageTriggerRecipient::Battle
                }
                _ => return Err("oracle:unsupported-damage-recipient"),
            };
            let grouping = if *grouped {
                TriggerGrouping::OneOrMoreByPlayer
            } else {
                TriggerGrouping::EachOccurrence
            };
            if *noncombat_only {
                Ok((
                    TriggerEvent::DealsNoncombatDamage {
                        source: src,
                        recipient: rec,
                    },
                    grouping,
                ))
            } else {
                Ok((
                    TriggerEvent::DealsDamage {
                        source: src,
                        recipient: rec,
                        combat_only: *combat_only,
                    },
                    grouping,
                ))
            }
        }
        TriggerEventAst::LifeChanged {
            player,
            gained,
            first_each_turn,
        } => {
            let rel = lower_player_relation(*player)?;
            let kind = if *gained {
                LifeChangeKind::Gain
            } else {
                LifeChangeKind::Loss
            };
            let grouping = if *first_each_turn {
                TriggerGrouping::OneOrMoreOnceEachTurn
            } else {
                TriggerGrouping::EachOccurrence
            };
            Ok((TriggerEvent::LifeChanged { kind, player: rel }, grouping))
        }
        TriggerEventAst::CardDrawn { player, qualifier } => {
            let rel = lower_player_relation(*player)?;
            let nth = match qualifier.trim().to_ascii_lowercase().as_str() {
                "" | "any" | "a card" | "cards" | "one or more cards" => None,
                "first"
                | "1"
                | "your first card each turn"
                | "their first card each turn"
                | "the first card they draw each turn" => NonZeroU16::new(1),
                "second"
                | "2"
                | "your second card each turn"
                | "their second card each turn"
                | "a second card each turn"
                | "their second card" => NonZeroU16::new(2),
                "third"
                | "3"
                | "your third card each turn"
                | "their third card each turn"
                | "a third card each turn" => NonZeroU16::new(3),
                "fourth" | "4" | "your fourth card each turn" | "their fourth card each turn" => {
                    NonZeroU16::new(4)
                }
                _ => return Err("oracle:unsupported-draw-qualifier"),
            };
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::DrewCard { nth_each_turn: nth },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::CardMilled { player, filter } => {
            let rel = lower_player_relation(*player)?;
            let subj = if is_unrestricted_filter(filter) {
                ObjectFilter::any()
            } else {
                lower_object_filter(filter, vocabulary)?
            };
            Ok((
                TriggerEvent::PutIntoGraveyard {
                    subject: subj,
                    player: rel,
                    from_library: true,
                    own_turn_only: false,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::CardDiscarded { player, filter } => {
            let rel = lower_player_relation(*player)?;
            let action = if is_unrestricted_filter(filter) {
                PlayerTriggerAction::Discarded
            } else {
                let mut obj = lower_object_filter(filter, vocabulary)?;
                if filter.any_permanent && filter.any_types.is_empty() {
                    match &mut obj {
                        ObjectFilter::Clause(c) => {
                            c.any_types = CardTypeMask::PERMANENT;
                        }
                        ObjectFilter::Either(c1, c2) => {
                            c1.any_types = CardTypeMask::PERMANENT;
                            c2.any_types = CardTypeMask::PERMANENT;
                        }
                    }
                }
                PlayerTriggerAction::DiscardedMatching(obj)
            };
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::PermanentSacrificed { player, subject } => {
            let rel = lower_player_relation(*player)?;
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::Sacrificed(subj),
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Cycled { subject } => {
            let self_only = match subject.as_ref() {
                SubjectRefAst::SelfObject => true,
                SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f)
                    if filter_only_has(
                        f,
                        FilterAllow {
                            controller: true,
                            ..FilterAllow::NONE
                        },
                    ) && matches!(f.controller, None | Some(ControllerRelation::You)) =>
                {
                    false
                }
                _ => return Err("oracle:unsupported-cycled-subject"),
            };
            Ok((
                TriggerEvent::PlayerAction {
                    player: PlayerRelation::Controller,
                    action: PlayerTriggerAction::Cycled { self_only },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::TapStateChanged {
            subject,
            tapped,
            for_mana: false,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: if *tapped {
                        StatusTriggerKind::BecomesTapped
                    } else {
                        StatusTriggerKind::BecomesUntapped
                    },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::CrimeCommitted { player } => {
            let rel = lower_player_relation(*player)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::CommittedCrime,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::TurnedFaceUp { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::TurnsFaceUp,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Transforms { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::Transforms,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::PhaseChanged {
            subject,
            phased_out,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::Phased { out: *phased_out },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Mutates { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::Mutated,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::ClassLevelReached { level } => Ok((
            TriggerEvent::StatusChanged {
                subject: ObjectFilter::self_object(),
                kind: StatusTriggerKind::ClassLevelReached(*level),
            },
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::CaseSolved => Ok((
            TriggerEvent::StatusChanged {
                subject: ObjectFilter::self_object(),
                kind: StatusTriggerKind::CaseSolved,
            },
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::BecomesMonstrous { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::BecomesMonstrous,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::KeywordActionPerformed { player, action } => {
            let rel = lower_player_relation(*player)?;
            let low = action.trim().to_ascii_lowercase();
            match low.as_str() {
                "loses_game"
                | "an opponent loses the game"
                | "another player loses the game"
                | "a player loses the game" => {
                    let eff_rel = match low.as_str() {
                        "an opponent loses the game" | "another player loses the game" => {
                            PlayerRelation::Opponent
                        }
                        "a player loses the game" => PlayerRelation::AnyParticipant,
                        _ => rel,
                    };
                    return Ok((
                        TriggerEvent::PlayerLosesGame(eff_rel),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "a creature dealt damage by equipped creature this turn dies"
                | "a creature dealt damage by enchanted creature this turn dies"
                | "a creature that was dealt damage by equipped creature this turn dies"
                | "a creature that was dealt damage by enchanted creature this turn dies" => {
                    return Ok((
                        TriggerEvent::Dies {
                            subject: ObjectFilter::creature(),
                            mode: DiesTriggerMode::DamagedByAttachedHost,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ enters from anywhere other than a graveyard or exile"
                | "~ enters the battlefield from anywhere other than a graveyard or exile" => {
                    return Ok((
                        TriggerEvent::Enters {
                            subject: ObjectFilter::self_object(),
                            from_zone: TriggerFromZone::NotGraveyardOrExile,
                            cast_origin: None,
                            attached_to_creature: false,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you solve a case" => {
                    return Ok((
                        TriggerEvent::StatusChanged {
                            subject: ObjectFilter::permanent().controlled(),
                            kind: StatusTriggerKind::CaseSolved,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "combat damage is dealt to you or a planeswalker you control" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::any(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::ControllerOrPlaneswalker,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "a creature you control deals combat damage to a player" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::creature().controlled(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::AnyPlayer,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more creatures you control deal combat damage to a player" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::creature().controlled(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::AnyPlayer,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "you gain or lose life" | "gain or lose life" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::LifeChanged {
                                kind: LifeChangeKind::Gain,
                                player: rel,
                            },
                            TriggerEvent::LifeChanged {
                                kind: LifeChangeKind::Loss,
                                player: rel,
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ enters attached to a creature" => {
                    return Ok((
                        TriggerEvent::Enters {
                            subject: ObjectFilter::self_object(),
                            from_zone: TriggerFromZone::Any,
                            cast_origin: None,
                            attached_to_creature: true,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ phases out" => {
                    return Ok((
                        TriggerEvent::StatusChanged {
                            subject: ObjectFilter::self_object(),
                            kind: StatusTriggerKind::Phased { out: true },
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ phases out or leaves the battlefield" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::StatusChanged {
                                subject: ObjectFilter::self_object(),
                                kind: StatusTriggerKind::Phased { out: true },
                            },
                            TriggerEvent::LeavesBattlefield {
                                subject: ObjectFilter::self_object(),
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ enters from your hand" => {
                    return Ok((
                        TriggerEvent::Enters {
                            subject: ObjectFilter::self_object(),
                            from_zone: TriggerFromZone::Hand,
                            cast_origin: None,
                            attached_to_creature: false,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ transforms into ~" => {
                    return Ok((
                        TriggerEvent::StatusChanged {
                            subject: ObjectFilter::self_object(),
                            kind: StatusTriggerKind::Transforms,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ transforms into ~ or dies" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::StatusChanged {
                                subject: ObjectFilter::self_object(),
                                kind: StatusTriggerKind::Transforms,
                            },
                            TriggerEvent::Dies {
                                subject: ObjectFilter::self_object(),
                                mode: DiesTriggerMode::Creature,
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "combat damage is dealt to you" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::any(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::Controller,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "one or more opponents lose life" => {
                    return Ok((
                        TriggerEvent::LifeChanged {
                            kind: LifeChangeKind::Loss,
                            player: PlayerRelation::Opponent,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "you turn a permanent face up" => {
                    return Ok((
                        TriggerEvent::StatusChanged {
                            subject: ObjectFilter::permanent().controlled(),
                            kind: StatusTriggerKind::TurnsFaceUp,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you cycle ~ or cycle another card" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::PlayerAction {
                                player: PlayerRelation::Controller,
                                action: PlayerTriggerAction::Cycled { self_only: true },
                            },
                            TriggerEvent::PlayerAction {
                                player: PlayerRelation::Controller,
                                action: PlayerTriggerAction::Cycled { self_only: false },
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ attacks a player and isn't blocked" => {
                    return Ok((
                        TriggerEvent::BlocksOrBlocked {
                            subject: ObjectFilter::self_object(),
                            kind: BlockTriggerKind::Unblocked,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "a creature attacks and isn't blocked this combat" => {
                    return Ok((
                        TriggerEvent::BlocksOrBlocked {
                            subject: ObjectFilter::creature(),
                            kind: BlockTriggerKind::Unblocked,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you attack enchanted player" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::creature().controlled(),
                            mode: AttackTriggerMode::AtEnchantedPlayer,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "you attack a player with one or more equipped creatures" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::creature().controlled(),
                            mode: AttackTriggerMode::EquippedAtPlayer,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "you remove a time counter from ~" => {
                    return Ok((
                        TriggerEvent::CountersChanged {
                            subject: ObjectFilter::self_object(),
                            kind: CounterTriggerKind::CounterChanged {
                                kind: Some(CounterKind::Time),
                                added: false,
                                by_controller_only: true,
                            },
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more other permanents phase out" => {
                    return Ok((
                        TriggerEvent::StatusChanged {
                            subject: ObjectFilter::permanent().other_than_ability_source(),
                            kind: StatusTriggerKind::Phased { out: true },
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "regenerated" | "is regenerated" | "regenerates" => {
                    return Ok((
                        TriggerEvent::StatusChanged {
                            subject: ObjectFilter::self_object(),
                            kind: StatusTriggerKind::Regenerated,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "another player casts a spell from anywhere other than their hand"
                | "an opponent casts a spell from anywhere other than their hand" => {
                    return Ok((
                        TriggerEvent::SpellCast {
                            caster: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                            filter: SpellFilter::any().not_from_owners_hand(),
                            self_cast: false,
                            include_copy: false,
                            min_mana_value: None,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you attack a player with one or more creatures with power 4 or greater" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::creature()
                                .controlled()
                                .with_power(PowerFilter::AtLeast(4)),
                            mode: AttackTriggerMode::AtPlayer,
                        },
                        TriggerGrouping::OneOrMoreByPlayer,
                    ));
                }
                "three or more creatures you control with flying attack" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::creature()
                                .controlled()
                                .with_keyword(KeywordAbility::Flying),
                            mode: AttackTriggerMode::GroupAtLeast(3),
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "another player attacks with two or more creatures" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::creature().opposing(),
                            mode: AttackTriggerMode::GroupAtLeast(2),
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "one or more of your opponents are attacked" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::creature(),
                            mode: AttackTriggerMode::AtOpponent,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "one or more players sacrifice one or more creatures" => {
                    return Ok((
                        TriggerEvent::PlayerAction {
                            player: PlayerRelation::AnyParticipant,
                            action: PlayerTriggerAction::Sacrificed(ObjectFilter::creature()),
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "one or more players discard one or more cards" => {
                    return Ok((
                        TriggerEvent::PlayerAction {
                            player: PlayerRelation::AnyParticipant,
                            action: PlayerTriggerAction::Discarded,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "one or more opponents are dealt noncombat damage" => {
                    return Ok((
                        TriggerEvent::DealsNoncombatDamage {
                            source: ObjectFilter::any(),
                            recipient: DamageTriggerRecipient::Opponent,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "damage is dealt to one of the chosen players" => {
                    return Ok((
                        TriggerEvent::DealtDamage {
                            subject: ObjectFilter::any(),
                            linked_player: true,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "a permanent other than a basic land enters" => {
                    return Ok((
                        TriggerEvent::Enters {
                            subject: ObjectFilter::Either(
                                ObjectClause::permanent().forbid_types(CardTypeMask::LAND),
                                ObjectClause {
                                    forbidden_supertypes: SupertypeMask::BASIC,
                                    ..ObjectClause::land()
                                },
                            ),
                            from_zone: TriggerFromZone::Any,
                            cast_origin: None,
                            attached_to_creature: false,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "a permanent owned by another player is put into a graveyard from the battlefield" =>
                {
                    return Ok((
                        TriggerEvent::Dies {
                            subject: ObjectFilter::Clause(ObjectClause {
                                controller: ControllerFilter::OwnerOpponent,
                                ..ObjectClause::permanent()
                            }),
                            mode: DiesTriggerMode::Permanent,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you put one or more +1/+1 counters on one or more other heroes you control" => {
                    if let Some(hero) = vocabulary.resolve_label("Hero") {
                        return Ok((
                            TriggerEvent::CountersChanged {
                                subject: ObjectFilter::permanent()
                                    .controlled()
                                    .other_than_ability_source()
                                    .with_subtype(hero),
                                kind: CounterTriggerKind::PlusOnePlaced {
                                    by_controller_only: true,
                                },
                            },
                            TriggerGrouping::OneOrMoreTogether,
                        ));
                    }
                }
                "~ or another creature is returned to your hand from the battlefield" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::ReturnedToHand {
                                subject: ObjectFilter::Clause(ObjectClause {
                                    controller: ControllerFilter::OwnerYou,
                                    ..ObjectClause::any().self_object()
                                }),
                            },
                            TriggerEvent::ReturnedToHand {
                                subject: ObjectFilter::Clause(ObjectClause {
                                    controller: ControllerFilter::OwnerYou,
                                    ..ObjectClause::creature()
                                })
                                .other_than_ability_source(),
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more noncreature permanents are returned to hand" => {
                    return Ok((
                        TriggerEvent::ReturnedToHand {
                            subject: ObjectFilter::Clause(ObjectClause::permanent().noncreature()),
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "you activate an ability" => {
                    return Ok((
                        TriggerEvent::AbilityActivated {
                            player: PlayerRelation::Controller,
                            source_filter: ObjectFilter::any(),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you cast an instant or sorcery spell or activate an ability" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::from_object(ObjectFilter::Clause(
                                    ObjectClause::any().any_types(
                                        CardTypeMask::INSTANT.union(CardTypeMask::SORCERY),
                                    ),
                                )),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            },
                            TriggerEvent::AbilityActivated {
                                player: PlayerRelation::Controller,
                                source_filter: ObjectFilter::any(),
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "an opponent casts a spell or activates an ability" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                                filter: SpellFilter::any(),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            },
                            TriggerEvent::AbilityActivated {
                                player: PlayerRelation::Opponent,
                                source_filter: ObjectFilter::any(),
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "an opponent sacrifices a nontoken permanent or discards a permanent card" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::PlayerAction {
                                player: PlayerRelation::Opponent,
                                action: PlayerTriggerAction::Sacrificed(
                                    ObjectFilter::permanent().nontoken(),
                                ),
                            },
                            TriggerEvent::PlayerAction {
                                player: PlayerRelation::Opponent,
                                action: PlayerTriggerAction::DiscardedMatching(
                                    ObjectFilter::permanent(),
                                ),
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "a spell or ability causes its controller to shuffle their library" => {
                    return Ok((
                        TriggerEvent::PlayerAction {
                            player: PlayerRelation::AnyParticipant,
                            action: PlayerTriggerAction::ShuffledLibrary,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you play a card" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::PlayerAction {
                                player: PlayerRelation::Controller,
                                action: PlayerTriggerAction::LandPlayed,
                            },
                            TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::any(),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                _ => {}
            }
            if let Some(ord_raw) = low.strip_prefix("you cast your ").and_then(|r| {
                r.strip_suffix(" spell each turn")
                    .or_else(|| r.strip_suffix(" spell in a turn"))
            }) && let Some(ord_n) = match ord_raw.trim() {
                "first" => Some(1u32),
                "second" => Some(2u32),
                "third" => Some(3u32),
                "fourth" => Some(4u32),
                "fifth" => Some(5u32),
                _ => None,
            } && let Some(nz) = NonZeroU32::new(ord_n)
            {
                return Ok((
                    TriggerEvent::SpellCast {
                        caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                        filter: SpellFilter::any().ordinal_this_turn(nz),
                        self_cast: false,
                        include_copy: false,
                        min_mana_value: None,
                    },
                    TriggerGrouping::EachOccurrence,
                ));
            }
            if let Some(rest) = low.strip_prefix("~ and at least ")
                && let Some(cnt_raw) = rest
                    .strip_suffix(" other creatures attack")
                    .or_else(|| rest.strip_suffix(" other creatures you control attack"))
                    .or_else(|| rest.strip_suffix(" other creature attack"))
                    .or_else(|| rest.strip_suffix(" other creature you control attack"))
                && let Some(min_others) = match cnt_raw.trim() {
                    "one" | "1" => Some(1u8),
                    "two" | "2" => Some(2u8),
                    "three" | "3" => Some(3u8),
                    "four" | "4" => Some(4u8),
                    "five" | "5" => Some(5u8),
                    _ => None,
                }
            {
                return Ok((
                    TriggerEvent::Attacks {
                        subject: ObjectFilter::self_object(),
                        mode: AttackTriggerMode::WithAtLeastOthers(min_others),
                    },
                    TriggerGrouping::EachOccurrence,
                ));
            }
            if let Some(raw_rest) = action
                .trim()
                .strip_prefix("regenerates:")
                .or_else(|| low.strip_prefix("regenerates:"))
            {
                let subj = lower_regenerates_subject(raw_rest, vocabulary)
                    .ok_or("oracle:unsupported-trigger-event")?;
                return Ok((
                    TriggerEvent::StatusChanged {
                        subject: subj,
                        kind: StatusTriggerKind::Regenerated,
                    },
                    TriggerGrouping::EachOccurrence,
                ));
            }
            if let Some(discard_raw) = low.strip_prefix("opponent_caused:discard ").or_else(|| {
                low.strip_prefix("a spell or ability an opponent controls causes you to discard ")
            }) {
                let discard_raw = discard_raw.trim();
                let (core, grouping) = if let Some(rest) = discard_raw.strip_prefix("one or more ")
                {
                    (rest.trim(), TriggerGrouping::OneOrMoreTogether)
                } else {
                    (discard_raw, TriggerGrouping::EachOccurrence)
                };
                let filter = match core {
                    "~" | "this card" => Some(ObjectFilter::self_object()),
                    "a card" | "card" | "cards" => None,
                    other => {
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                            other,
                            &compiler_vocab,
                        )
                        .map_err(|_| "oracle:unsupported-trigger-event")?;
                        if is_unrestricted_filter(&parsed) {
                            None
                        } else {
                            let mut obj = lower_object_filter(&parsed, vocabulary)
                                .map_err(|_| "oracle:unsupported-trigger-event")?;
                            if parsed.any_permanent && parsed.any_types.is_empty() {
                                match &mut obj {
                                    ObjectFilter::Clause(c) => {
                                        c.any_types = CardTypeMask::PERMANENT;
                                    }
                                    ObjectFilter::Either(c1, c2) => {
                                        c1.any_types = CardTypeMask::PERMANENT;
                                        c2.any_types = CardTypeMask::PERMANENT;
                                    }
                                }
                            }
                            Some(obj)
                        }
                    }
                };
                return Ok((
                    TriggerEvent::PlayerAction {
                        player: PlayerRelation::Controller,
                        action: PlayerTriggerAction::DiscardedByOpponent(filter),
                    },
                    grouping,
                ));
            }
            if let Some(subj_raw) = low
                .strip_suffix(" leaves the battlefield without dying")
                .or_else(|| low.strip_suffix(" leave the battlefield without dying"))
            {
                let subj_raw = subj_raw.trim();
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                if let Some(other_raw) = subj_raw.strip_prefix("~ or ") {
                    let parsed = spellwake_oracle_compiler::filter::parse_subject_or_target(
                        other_raw.trim(),
                        &compiler_vocab,
                    )
                    .map_err(|_| "oracle:unsupported-trigger-event")?;
                    let other_subj = lower_trigger_subject_filter(&parsed, vocabulary)
                        .map_err(|_| "oracle:unsupported-trigger-event")?;
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::LeavesBattlefieldWithoutDying {
                                subject: ObjectFilter::self_object(),
                            },
                            TriggerEvent::LeavesBattlefieldWithoutDying {
                                subject: other_subj,
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                let (core, grouping) = if let Some(rest) = subj_raw.strip_prefix("one or more ") {
                    (rest.trim(), TriggerGrouping::OneOrMoreTogether)
                } else {
                    (subj_raw, TriggerGrouping::EachOccurrence)
                };
                let parsed = spellwake_oracle_compiler::filter::parse_subject_or_target(
                    core,
                    &compiler_vocab,
                )
                .map_err(|_| "oracle:unsupported-trigger-event")?;
                let subject = lower_trigger_subject_filter(&parsed, vocabulary)
                    .map_err(|_| "oracle:unsupported-trigger-event")?;
                return Ok((
                    TriggerEvent::LeavesBattlefieldWithoutDying { subject },
                    grouping,
                ));
            }
            let act = match low.as_str() {
                "scry" | "scries" | "you scry" => PlayerTriggerAction::Scried,
                "surveil" | "surveils" | "you surveil" => PlayerTriggerAction::Surveilled,
                "cycle" | "cycles" | "cycle a card" | "cycles a card" | "you cycle a card" => {
                    PlayerTriggerAction::Cycled { self_only: false }
                }
                "discard" | "discards" | "discard a card" | "discards a card"
                | "you discard a card" => PlayerTriggerAction::Discarded,
                "commit a crime" | "commits a crime" | "you commit a crime" => {
                    PlayerTriggerAction::CommittedCrime
                }
                "draw a card" | "draws a card" | "you draw a card" => {
                    PlayerTriggerAction::DrewCard {
                        nth_each_turn: None,
                    }
                }
                "the final chapter ability of a saga you control resolves" => {
                    PlayerTriggerAction::SagaFinalChapterResolved
                }
                "create a token" | "creates a token" | "create_token" | "token_created" => {
                    PlayerTriggerAction::TokenCreated
                }
                "proliferate" | "proliferates" | "you proliferate" => {
                    PlayerTriggerAction::Proliferated
                }
                "explore" | "explores" | "you explore" => PlayerTriggerAction::Explored,
                "shuffle_library"
                | "shuffle your library"
                | "you shuffle your library"
                | "shuffles their library"
                | "shuffle a library"
                | "shuffles a library" => PlayerTriggerAction::ShuffledLibrary,
                "search your library"
                | "you search your library"
                | "searches their library"
                | "search a library"
                | "searches a library" => PlayerTriggerAction::SearchedLibrary,
                "become_monarch"
                | "become the monarch"
                | "becomes the monarch"
                | "you become the monarch" => PlayerTriggerAction::BecameMonarch,
                "you get one or more {e}" | "gain_energy" | "get one or more {e}" => {
                    PlayerTriggerAction::GainedEnergy
                }
                "a spell or ability you control counters a spell"
                | "counter a spell"
                | "counters a spell" => PlayerTriggerAction::SpellCountered,
                "investigate" | "investigates" | "you investigate" => {
                    PlayerTriggerAction::Investigated
                }
                "connive" | "connives" | "you connive" => PlayerTriggerAction::Connived,
                "forage" | "forages" | "you forage" => PlayerTriggerAction::Foraged,
                "you collect evidence" | "collect evidence" | "collects evidence" => {
                    PlayerTriggerAction::CollectedEvidence
                }
                "foretell" | "foretells" | "you foretell a card" => PlayerTriggerAction::Foretold,
                "~ becomes saddled" | "saddle" | "saddles" => PlayerTriggerAction::Saddled,
                "exert" | "exerts" | "you exert" => PlayerTriggerAction::Exerted,
                "~ becomes plotted" | "plot" | "plots" => PlayerTriggerAction::Plotted,
                "clash" | "clashes" | "you clash" => PlayerTriggerAction::Clashed,
                "exhaust" | "exhausts" | "you exhaust" => PlayerTriggerAction::Exhausted,
                "evolve" | "evolves" | "you evolve" => PlayerTriggerAction::Evolved,
                "ring_tempts_you" | "the ring tempts you" => PlayerTriggerAction::RingTemptsYou,
                "populate" | "populates" | "you populate" => PlayerTriggerAction::Populated,
                "discover" | "discovers" | "you discover" => PlayerTriggerAction::Discovered,
                "venture"
                | "ventures"
                | "venture into the dungeon"
                | "ventures into the dungeon"
                | "you venture into the dungeon" => PlayerTriggerAction::Ventured,
                "complete_dungeon"
                | "complete a dungeon"
                | "completes a dungeon"
                | "you complete a dungeon" => PlayerTriggerAction::CompletedDungeon,
                "manifest_dread" | "manifest dread" | "manifests dread" | "you manifest dread" => {
                    PlayerTriggerAction::ManifestedDread
                }
                "pay life" | "pays life" | "you pay life" => PlayerTriggerAction::PaidLife,
                "give a gift" | "gives a gift" | "you give a gift" => PlayerTriggerAction::GaveGift,
                "win a clash"
                | "wins a clash"
                | "you win a clash"
                | "whenever you win a clash"
                | "you clash and win" => PlayerTriggerAction::WonClash,
                "copy a spell"
                | "copies a spell"
                | "you copy a spell"
                | "you copy a spell or ability"
                | "you copy an instant or sorcery spell" => {
                    PlayerTriggerAction::CopiedSpellOrAbility
                }
                "enlist" | "enlists" | "you enlist" => PlayerTriggerAction::Enlisted,
                "train" | "trains" | "you train" => PlayerTriggerAction::Trained,
                "kick_spell" | "kick a spell" | "kicks a spell" | "you kick a spell" => {
                    PlayerTriggerAction::KickedSpell
                }
                "scry_to_bottom" | "scry_bottom" => PlayerTriggerAction::ScryToBottom,
                "waterbend" | "waterbends" | "you waterbend" => PlayerTriggerAction::Waterbended,
                _ => return Err("oracle:unsupported-trigger-event"),
            };
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: act,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::BecomesTargeted {
            subject,
            by_opponent,
            by_text,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            let mut grouping = TriggerGrouping::EachOccurrence;
            let mut opp = *by_opponent;
            let mut by_controller = false;
            let (spell_only, inst_sorc) = if let Some(raw) = by_text.as_deref() {
                let low = raw.trim().to_ascii_lowercase();
                let mut rest = low.as_str();
                if let Some(stripped) = rest.strip_suffix(" for the first time each turn") {
                    rest = stripped.trim_end();
                    grouping = TriggerGrouping::OneOrMoreOnceEachTurn;
                }
                if let Some(stripped) = rest
                    .strip_suffix(" an opponent controls")
                    .or_else(|| rest.strip_suffix(" your opponents control"))
                    .or_else(|| rest.strip_suffix(" opponents control"))
                    .or_else(|| rest.strip_suffix(" controlled by an opponent"))
                    .or_else(|| rest.strip_suffix(" controlled by your opponents"))
                {
                    rest = stripped.trim_end();
                    opp = true;
                } else if let Some(stripped) = rest
                    .strip_suffix(" you control")
                    .or_else(|| rest.strip_suffix(" controlled by you"))
                {
                    rest = stripped.trim_end();
                    by_controller = true;
                }
                if opp && by_controller {
                    return Err("oracle:unsupported-targeted-by");
                }
                let rest_trimmed = rest.trim();
                let normalized = rest_trimmed
                    .strip_prefix("an ")
                    .or_else(|| rest_trimmed.strip_prefix("a "))
                    .unwrap_or(rest_trimmed)
                    .trim();
                match normalized {
                    "" | "spell or ability" | "spell or an ability" => (false, false),
                    "spell" => (true, false),
                    "instant or sorcery spell" => (true, true),
                    _ => return Err("oracle:unsupported-targeted-by"),
                }
            } else {
                (false, false)
            };
            let event = if by_controller {
                TriggerEvent::TargetedByController {
                    subject: subj,
                    spell_only,
                    instant_or_sorcery_only: inst_sorc,
                }
            } else {
                TriggerEvent::Targeted {
                    subject: subj,
                    by_opponent: opp,
                    spell_only,
                    instant_or_sorcery_only: inst_sorc,
                }
            };
            Ok((event, grouping))
        }
        TriggerEventAst::CountersChanged {
            subject,
            counter_kind,
            added,
            by_you,
        } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            let ck_low = counter_kind.trim().to_ascii_lowercase();
            let kind = if *added && ck_low == "+1/+1" {
                CounterTriggerKind::PlusOnePlaced {
                    by_controller_only: *by_you,
                }
            } else {
                let parsed = if ck_low.is_empty()
                    || ck_low == "any"
                    || ck_low == "a"
                    || ck_low == "one or more"
                {
                    None
                } else {
                    Some(
                        parse_counter_kind(counter_kind)
                            .ok_or("oracle:unsupported-trigger-event")?,
                    )
                };
                CounterTriggerKind::CounterChanged {
                    kind: parsed,
                    added: *added,
                    by_controller_only: *by_you,
                }
            };
            Ok((
                TriggerEvent::CountersChanged {
                    subject: subj,
                    kind,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::TapStateChanged {
            subject,
            tapped: true,
            for_mana: true,
        } => {
            if let SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) =
                subject.as_ref()
                && f.any_types == AstCardTypeMask::ARTIFACT
                && f.forbidden_types.is_empty()
                && f.subtypes.is_empty()
            {
                let rel = match f.controller {
                    None => PlayerRelation::AnyParticipant,
                    Some(ControllerRelation::You) => PlayerRelation::Controller,
                    Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                        PlayerRelation::Opponent
                    }
                    _ => return Err("oracle:unsupported-trigger-event"),
                };
                return Ok((
                    TriggerEvent::PlayerAction {
                        player: rel,
                        action: PlayerTriggerAction::ArtifactTappedForMana,
                    },
                    TriggerGrouping::EachOccurrence,
                ));
            }
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            let rel = match subject.as_ref() {
                SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) => {
                    match f.controller {
                        None => PlayerRelation::AnyParticipant,
                        Some(ControllerRelation::You) => PlayerRelation::Controller,
                        Some(ControllerRelation::Opponent | ControllerRelation::Opponents) => {
                            PlayerRelation::Opponent
                        }
                        _ => PlayerRelation::AnyParticipant,
                    }
                }
                _ => PlayerRelation::AnyParticipant,
            };
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::PermanentTappedForMana(subj),
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::StateTrigger(cond) => match cond.as_ref() {
            ConditionAst::CounterCompare {
                subject,
                counter_kind,
                op,
                value,
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
                let kind =
                    parse_counter_kind(counter_kind).ok_or("oracle:unsupported-trigger-event")?;
                let amount =
                    u8::try_from(*value).map_err(|_| "oracle:unsupported-trigger-event")?;
                Ok((
                    TriggerEvent::StateThreshold(StateThresholdKind::SourceCounters {
                        kind,
                        comparison: lower_comparison(*op),
                        amount,
                    }),
                    TriggerGrouping::EachOccurrence,
                ))
            }
            ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter,
                op: CompareOp::Ge,
                value,
            } if *value > 0
                && filter.subtypes.len() == 1
                && filter.forbidden_types.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty() =>
            {
                let ct = vocabulary
                    .resolve_label(&filter.subtypes[0])
                    .ok_or("oracle:unsupported-trigger-event")?;
                Ok((
                    TriggerEvent::StateThreshold(StateThresholdKind::ControlledSubtype {
                        subtype: ct,
                        minimum: *value,
                    }),
                    TriggerGrouping::EachOccurrence,
                ))
            }
            other_cond => {
                let mut tmp = BodyBuilder::new();
                let rc = lower_resolution_condition(other_cond, &mut tmp, vocabulary, false, None)
                    .map_err(|_| "oracle:unsupported-trigger-event")?;
                if tmp.target == BodyTarget::None && rc.is_world_evaluable() {
                    Ok((
                        TriggerEvent::StateThreshold(StateThresholdKind::Eval(Box::new(rc))),
                        TriggerGrouping::EachOccurrence,
                    ))
                } else {
                    Err("oracle:unsupported-trigger-event")
                }
            }
        },
        TriggerEventAst::AbilityActivated {
            player,
            source_filter,
        } => {
            let rel = lower_player_relation(*player)?;
            let sf = if is_unrestricted_filter(source_filter) {
                ObjectFilter::any()
            } else {
                lower_object_filter(source_filter, vocabulary)?
            };
            Ok((
                TriggerEvent::AbilityActivated {
                    player: rel,
                    source_filter: sf,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::BecomesAttached { subject, .. } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::BecomesAttached { subject: subj },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Exploits { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::Exploits { subject: subj },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::DayNightChanged => Ok((
            TriggerEvent::DayNightChanged,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::LandPlayed { player } => {
            let rel = lower_player_relation(*player)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::LandPlayed,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Expend { player, amount } => {
            let rel = lower_player_relation(*player)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::Expend(*amount),
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::RollDie {
            player,
            exact_result,
        } => {
            let rel = lower_player_relation(*player)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::RolledDie {
                        exact: *exact_result,
                    },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::FlipCoin { player, won } => {
            let rel = lower_player_relation(*player)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::FlippedCoin { won: *won },
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::LibrarySearched { player } => {
            let rel = lower_player_relation(*player)?;
            Ok((
                TriggerEvent::PlayerAction {
                    player: rel,
                    action: PlayerTriggerAction::SearchedLibrary,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Crewed { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::Crewed,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Exerted { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::Exerted,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Explores { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::StatusChanged {
                    subject: subj,
                    kind: StatusTriggerKind::Explored,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::Connives { .. } => Ok((
            TriggerEvent::PlayerAction {
                player: PlayerRelation::Controller,
                action: PlayerTriggerAction::Connived,
            },
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::UnlockDoor { subject } => {
            let subj = lower_trigger_subject_filter(subject, vocabulary)?;
            Ok((
                TriggerEvent::DoorUnlocked {
                    subject: subj,
                    fully_unlocked_only: false,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::RoomFullyUnlocked { player } => {
            let subj = match player {
                PlayerRefAst::You => ObjectFilter::permanent().controlled(),
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    ObjectFilter::Clause(ObjectClause {
                        controller: ControllerFilter::Opponent,
                        ..ObjectClause::permanent()
                    })
                }
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => ObjectFilter::permanent(),
                _ => return Err("oracle:unsupported-trigger-event"),
            };
            Ok((
                TriggerEvent::DoorUnlocked {
                    subject: subj,
                    fully_unlocked_only: true,
                },
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::PlaneswalkToSelf => Ok((
            TriggerEvent::Planar(PlanarTriggerEvent::PlaneswalkToSelf),
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::PlaneswalkAwayFromSelf => Ok((
            TriggerEvent::Planar(PlanarTriggerEvent::PlaneswalkAwayFromSelf),
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::ChaosEnsues => Ok((
            TriggerEvent::Planar(PlanarTriggerEvent::ChaosEnsues),
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::EncounterPhenomenon => Ok((
            TriggerEvent::Planar(PlanarTriggerEvent::EncounterPhenomenon),
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::SchemeSetInMotion => Ok((
            TriggerEvent::SchemeSetInMotion,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::BattleDefeated => Ok((
            TriggerEvent::CountersChanged {
                subject: ObjectFilter::self_object(),
                kind: CounterTriggerKind::LastDefenseRemoved,
            },
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::ManualTabletopClaim { .. } => {
            Err("oracle:tabletop-honor-excluded-from-constructed")
        }
        _ => Err("oracle:unsupported-trigger-event"),
    }
}

fn lower_regenerates_subject(
    raw_rest: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ObjectFilter> {
    let trimmed = raw_rest.trim();
    if trimmed.eq_ignore_ascii_case("selfobject") || trimmed == "~" {
        return Some(ObjectFilter::self_object());
    }
    let low = trimmed.to_ascii_lowercase();
    if low.starts_with("attachedhost(")
        || matches!(
            low.as_str(),
            "enchanted creature" | "equipped creature" | "enchanted permanent"
        )
    {
        return Some(ObjectFilter::Clause(ObjectClause::any().attached_to_self()));
    }
    let candidates: [(ObjectFilterAst, ObjectFilter); 6] = [
        (
            ObjectFilterAst {
                any_types: AstCardTypeMask::CREATURE,
                ..ObjectFilterAst::default()
            },
            ObjectFilter::creature(),
        ),
        (
            ObjectFilterAst {
                any_types: AstCardTypeMask::CREATURE,
                controller: Some(ControllerRelation::You),
                ..ObjectFilterAst::default()
            },
            ObjectFilter::creature().controlled(),
        ),
        (
            ObjectFilterAst {
                any_types: AstCardTypeMask::CREATURE,
                controller: Some(ControllerRelation::Opponent),
                ..ObjectFilterAst::default()
            },
            ObjectFilter::creature().opposing(),
        ),
        (
            ObjectFilterAst {
                any_types: AstCardTypeMask::CREATURE,
                flags: AstFilterFlags::EXCLUDE_SELF,
                ..ObjectFilterAst::default()
            },
            ObjectFilter::creature().other_than_ability_source(),
        ),
        (
            ObjectFilterAst {
                any_types: AstCardTypeMask::CREATURE,
                controller: Some(ControllerRelation::You),
                flags: AstFilterFlags::EXCLUDE_SELF,
                ..ObjectFilterAst::default()
            },
            ObjectFilter::creature()
                .controlled()
                .other_than_ability_source(),
        ),
        (
            ObjectFilterAst {
                any_permanent: true,
                ..ObjectFilterAst::default()
            },
            ObjectFilter::permanent(),
        ),
    ];
    for (ast_flt, lowered) in candidates {
        let group = SubjectRefAst::Group(Box::new(ast_flt));
        if format!("{group:?}") == trimmed {
            return Some(lowered);
        }
    }
    if !trimmed.contains("ObjectFilterAst") {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        if let Ok(parsed) =
            spellwake_oracle_compiler::filter::parse_subject_or_target(trimmed, &compiler_vocab)
            && let Ok(lowered) = lower_trigger_subject_filter(&parsed, vocabulary)
        {
            return Some(lowered);
        }
    }
    None
}

pub(super) fn lower_trigger_subject_filter(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ObjectFilter, &'static str> {
    match subject {
        SubjectRefAst::SelfObject => Ok(ObjectFilter::self_object()),
        SubjectRefAst::AttachedHost(_) => {
            Ok(ObjectFilter::Clause(ObjectClause::any().attached_to_self()))
        }
        SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) => {
            lower_object_filter(f, vocabulary)
        }
        _ => Err("oracle:unsupported-trigger-subject"),
    }
}

pub(super) fn lower_player_relation(p: PlayerRefAst) -> Result<PlayerRelation, &'static str> {
    match p {
        PlayerRefAst::You => Ok(PlayerRelation::Controller),
        PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => Ok(PlayerRelation::Opponent),
        PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => Ok(PlayerRelation::AnyParticipant),
        _ => Err("oracle:unsupported-player-relation"),
    }
}

fn try_lower_specialized_trigger_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<TriggerCondition> {
    match cond {
        ConditionAst::IsYourTurn => Some(TriggerCondition::ControllersTurn),
        ConditionAst::Not(inner) if matches!(inner.as_ref(), ConditionAst::IsYourTurn) => {
            Some(TriggerCondition::OpponentsTurn)
        }
        ConditionAst::Delirium => Some(TriggerCondition::Delirium),
        ConditionAst::Metalcraft => Some(TriggerCondition::Metalcraft),
        ConditionAst::Threshold => Some(TriggerCondition::Threshold),
        ConditionAst::Morbid {
            your_control_only: false,
        } => Some(TriggerCondition::Morbid),
        ConditionAst::ControlsCommander => Some(TriggerCondition::ControlsOwnCommander),
        ConditionAst::Marker(GameMarker::CitysBlessing) => {
            Some(TriggerCondition::ControllerHasCityBlessing)
        }
        ConditionAst::Marker(GameMarker::Monarch(PlayerRefAst::You)) => {
            Some(TriggerCondition::Monarch { opponent: false })
        }
        ConditionAst::Marker(GameMarker::Monarch(
            PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
        )) => Some(TriggerCondition::Monarch { opponent: true }),
        ConditionAst::OpponentLostLifeThisTurn => {
            Some(TriggerCondition::OpponentLostLifeAtLeast(1))
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Eq,
            value: 0,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.flags == AstFilterFlags::EXCLUDE_SELF
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    controller: true,
                    flags: true,
                    ..FilterAllow::NONE
                },
            )
            && matches!(filter.controller, None | Some(ControllerRelation::You)) =>
        {
            Some(TriggerCondition::NoOtherControlledCreatures)
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    controller: true,
                    stat_bounds: true,
                    ..FilterAllow::NONE
                },
            )
            && matches!(filter.controller, None | Some(ControllerRelation::You))
            && filter.stat_bounds.len() == 1
            && filter.stat_bounds[0].stat == StatKind::Power
            && filter.stat_bounds[0].op == CompareOp::Ge
            && matches!(filter.stat_bounds[0].value.as_ref(), ValueExprAst::Const(p) if *p > 0) =>
        {
            let ValueExprAst::Const(p) = filter.stat_bounds[0].value.as_ref() else {
                return None;
            };
            Some(TriggerCondition::ControlledCreaturePowerAtLeast(
                u16::try_from(*p).ok()?,
            ))
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.any_types == AstCardTypeMask::CREATURE
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    controller: true,
                    ..FilterAllow::NONE
                },
            )
            && matches!(filter.controller, None | Some(ControllerRelation::You)) =>
        {
            Some(TriggerCondition::ControlledCreatureCount(*value))
        }
        ConditionAst::TotalStatCompare {
            filter,
            stat: StatKind::Power,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.any_types == AstCardTypeMask::CREATURE
            && filter.controller == Some(ControllerRelation::You)
            && filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    controller: true,
                    ..FilterAllow::NONE
                },
            ) =>
        {
            Some(TriggerCondition::TotalControlledPowerAtLeast(
                u16::try_from(*value).ok()?,
            ))
        }
        ConditionAst::OpponentMoreThanYou {
            metric,
            filter: None,
        } => match metric.to_ascii_lowercase().as_str() {
            "lands" => Some(TriggerCondition::OpponentHasMoreLands),
            "life" => Some(TriggerCondition::OpponentHasMoreLife),
            "creatures" => Some(TriggerCondition::OpponentControlsMoreCreatures),
            "cards in hand" | "cards_in_hand" | "cards" => {
                Some(TriggerCondition::OpponentHasMoreCardsInHand)
            }
            _ => None,
        },
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op,
            value,
        } => match (op, *value) {
            (CompareOp::Le, v) => Some(TriggerCondition::HandSizeAtMost(u8::try_from(v).ok()?)),
            (CompareOp::Lt, v) if v > 0 => {
                Some(TriggerCondition::HandSizeAtMost(u8::try_from(v - 1).ok()?))
            }
            (CompareOp::Eq, 0) => Some(TriggerCondition::HandSizeAtMost(0)),
            (CompareOp::Ge, v) => Some(TriggerCondition::HandSizeAtLeast(u8::try_from(v).ok()?)),
            (CompareOp::Gt, v) => Some(TriggerCondition::HandSizeAtLeast(
                v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
            )),
            (CompareOp::Eq, v) if v > 0 => {
                let n = u8::try_from(v).ok()?;
                Some(TriggerCondition::And(
                    Box::new(TriggerCondition::HandSizeAtLeast(n)),
                    Box::new(TriggerCondition::HandSizeAtMost(n)),
                ))
            }
            _ => None,
        },
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            match status.trim().to_ascii_lowercase().as_str() {
                "saddled" => Some(TriggerCondition::SourceSaddled),
                "tapped" => Some(TriggerCondition::SourceIsTapped),
                "untapped" => Some(TriggerCondition::SourceIsUntapped),
                "prepared" => Some(TriggerCondition::SourceIsPrepared),
                "attacking" => Some(TriggerCondition::SourceIsAttacking),
                "blocking" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        combat: CombatFilter::Blocking,
                        ..ObjectClause::any()
                    }),
                ))),
                "attacking or blocking" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        combat: CombatFilter::AttackingOrBlocking,
                        ..ObjectClause::any()
                    }),
                ))),
                "blocked" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        combat: CombatFilter::Blocked,
                        ..ObjectClause::any()
                    }),
                ))),
                "unblocked" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        combat: CombatFilter::Unblocked,
                        ..ObjectClause::any()
                    }),
                ))),
                "enchanted" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        flags: ObjectFilterFlags::ENCHANTED,
                        ..ObjectClause::any()
                    }),
                ))),
                "equipped" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        flags: ObjectFilterFlags::EQUIPPED,
                        ..ObjectClause::any()
                    }),
                ))),
                "modified" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        flags: ObjectFilterFlags::MODIFIED,
                        ..ObjectClause::any()
                    }),
                ))),
                "face down" | "face-down" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        flags: ObjectFilterFlags::FACE_DOWN,
                        ..ObjectClause::any()
                    }),
                ))),
                "solved" => Some(TriggerCondition::SourceIsSolved),
                "unsolved" => Some(TriggerCondition::SourceIsUnsolved),
                "unrenowned" | "not renowned" => Some(TriggerCondition::UnrenownedSource),
                other => {
                    if let Some(flt_raw) = other.strip_prefix("is:") {
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                            flt_raw.trim(),
                            &compiler_vocab,
                        )
                        .ok()?;
                        let obj = lower_object_filter(&parsed, vocabulary).ok()?;
                        Some(TriggerCondition::SourceMatches(Box::new(obj)))
                    } else {
                        None
                    }
                }
            }
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            let kind = parse_counter_kind(counter_kind)?;
            match (op, *value) {
                (CompareOp::Ge, v) if v > 0 => Some(TriggerCondition::SourceCounterAtLeast {
                    kind,
                    minimum: u8::try_from(v).ok()?,
                }),
                (CompareOp::Eq, 0) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::SourceCounterAtLeast { kind, minimum: 1 },
                ))),
                (CompareOp::Gt, v) => Some(TriggerCondition::SourceCounterAtLeast {
                    kind,
                    minimum: v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
                }),
                (CompareOp::Le, v) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::SourceCounterAtLeast {
                        kind,
                        minimum: v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
                    },
                ))),
                (CompareOp::Lt, v) if v > 0 => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::SourceCounterAtLeast {
                        kind,
                        minimum: u8::try_from(v).ok()?,
                    },
                ))),
                _ => None,
            }
        }
        ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
            subject,
            or_entered_this_turn: false,
        }) if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
            Some(TriggerCondition::SourceDidNotAttackThisTurn)
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op,
            value,
        } if *value >= 0 => match op {
            CompareOp::Ge => Some(TriggerCondition::ControllerLifeAtLeast(
                u16::try_from(*value).ok()?,
            )),
            CompareOp::Le => Some(TriggerCondition::ControllerLifeAtMost(
                u16::try_from(*value).ok()?,
            )),
            CompareOp::Gt => Some(TriggerCondition::ControllerLifeAtLeast(
                value.checked_add(1).and_then(|n| u16::try_from(n).ok())?,
            )),
            CompareOp::Lt if *value > 0 => Some(TriggerCondition::ControllerLifeAtMost(
                u16::try_from(*value - 1).ok()?,
            )),
            _ => None,
        },
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0 => {
            let min_u8 = u8::try_from(*value).ok()?;
            if filter_only_has(
                filter,
                FilterAllow {
                    types: true,
                    subtypes: true,
                    controller: true,
                    ..FilterAllow::NONE
                },
            ) && matches!(filter.controller, None | Some(ControllerRelation::You))
            {
                if filter.subtypes.len() == 1
                    && (filter.any_types.is_empty()
                        || filter.any_types == AstCardTypeMask::CREATURE)
                    && let Some(ct) = vocabulary.resolve_label(&filter.subtypes[0])
                {
                    return Some(TriggerCondition::ControlsSubtype {
                        kind: ct,
                        minimum: min_u8,
                    });
                }
                if filter.subtypes.is_empty()
                    && let Ok(mut types) = card_type_mask_to_set(filter.any_types)
                    && types.len() == 1
                    && let Some(kind) = types.pop_first()
                {
                    return Some(TriggerCondition::ControlsCardType {
                        kind,
                        minimum: min_u8,
                    });
                }
            }
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            Some(TriggerCondition::ControlsMatching {
                filter: obj,
                minimum: min_u8,
            })
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op,
            value,
        } => {
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            match (op, *value) {
                (CompareOp::Eq, 0) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::ControlsMatching {
                        filter: obj,
                        minimum: 1,
                    },
                ))),
                (CompareOp::Gt, v) => Some(TriggerCondition::ControlsMatching {
                    filter: obj,
                    minimum: v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
                }),
                (CompareOp::Le, v) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::ControlsMatching {
                        filter: obj,
                        minimum: v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
                    },
                ))),
                (CompareOp::Lt, v) if v > 0 => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::ControlsMatching {
                        filter: obj,
                        minimum: u8::try_from(v).ok()?,
                    },
                ))),
                (CompareOp::Eq, v) if v > 0 => {
                    let min = u8::try_from(v).ok()?;
                    let over = v.checked_add(1).and_then(|n| u8::try_from(n).ok())?;
                    Some(TriggerCondition::And(
                        Box::new(TriggerCondition::ControlsMatching {
                            filter: obj,
                            minimum: min,
                        }),
                        Box::new(TriggerCondition::Not(Box::new(
                            TriggerCondition::ControlsMatching {
                                filter: obj,
                                minimum: over,
                            },
                        ))),
                    ))
                }
                _ => None,
            }
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            filter,
            op,
            value,
        } => {
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            match (op, *value) {
                (CompareOp::Ge, v) if v > 0 => Some(TriggerCondition::OpponentControlsMatching {
                    filter: obj,
                    minimum: u8::try_from(v).ok()?,
                }),
                (CompareOp::Gt, v) => Some(TriggerCondition::OpponentControlsMatching {
                    filter: obj,
                    minimum: v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
                }),
                (CompareOp::Eq, 0) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::OpponentControlsMatching {
                        filter: obj,
                        minimum: 1,
                    },
                ))),
                (CompareOp::Le, v) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::OpponentControlsMatching {
                        filter: obj,
                        minimum: v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
                    },
                ))),
                (CompareOp::Lt, v) if v > 0 => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::OpponentControlsMatching {
                        filter: obj,
                        minimum: u8::try_from(v).ok()?,
                    },
                ))),
                (CompareOp::Eq, v) if v > 0 => {
                    let rc = ResolutionCondition {
                        left: ValueExpr::count_battlefield(obj.opposing()),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(i64::from(v)),
                    };
                    rc.is_trigger_world_evaluable()
                        .then(|| TriggerCondition::Eval(Box::new(rc)))
                }
                _ => None,
            }
        }
        ConditionAst::ControlsCount {
            player: p @ (PlayerRefAst::DefendingPlayer | PlayerRefAst::ThatPlayer),
            filter,
            op,
            value,
        } if matches!(
            filter.controller,
            None | Some(ControllerRelation::DefendingPlayer | ControllerRelation::ThatPlayer)
        ) =>
        {
            let mut obj = lower_object_filter(filter, vocabulary).ok()?;
            match &mut obj {
                ObjectFilter::Clause(c) => {
                    c.controller = ControllerFilter::RelatedPlayer;
                }
                ObjectFilter::Either(c1, c2) => {
                    c1.controller = ControllerFilter::RelatedPlayer;
                    c2.controller = ControllerFilter::RelatedPlayer;
                }
            }
            let rp = if matches!(p, PlayerRefAst::DefendingPlayer) {
                ResolvingPlayer::DefendingPlayer
            } else {
                ResolvingPlayer::UniqueRelatedPlayer
            };
            let rc = ResolutionCondition {
                left: ValueExpr::count_player_battlefield(rp, obj),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            };
            rc.is_trigger_world_evaluable()
                .then(|| TriggerCondition::Eval(Box::new(rc)))
        }
        ConditionAst::GraveyardCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } => Some(TriggerCondition::GraveyardCardTypeCount {
            kind: None,
            minimum: u8::try_from(*value).ok()?,
        }),
        ConditionAst::ZoneObjectCount { filter, op, value }
            if filter.zone == Some(ZoneFilter::YourGraveyard) =>
        {
            if *op == CompareOp::Ge
                && filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        zone: true,
                        ..FilterAllow::NONE
                    },
                )
                && let Ok(min_u8) = u8::try_from(*value)
            {
                if filter.any_types.is_empty() {
                    return Some(TriggerCondition::GraveyardCardTypeCount {
                        kind: None,
                        minimum: min_u8,
                    });
                }
                if let Ok(mut types) = card_type_mask_to_set(filter.any_types)
                    && types.len() == 1
                    && let Some(kind) = types.pop_first()
                {
                    return Some(TriggerCondition::GraveyardCardTypeCount {
                        kind: Some(kind),
                        minimum: min_u8,
                    });
                }
            }
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let obj = if is_unrestricted_filter(&inner) {
                ObjectFilter::any()
            } else {
                lower_object_filter(&inner, vocabulary).ok()?
            };
            match (op, *value) {
                (CompareOp::Ge, v) if v > 0 => Some(TriggerCondition::GraveyardMatching {
                    filter: Box::new(obj),
                    minimum: u8::try_from(v).ok()?,
                }),
                (CompareOp::Eq, 0) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::GraveyardMatching {
                        filter: Box::new(obj),
                        minimum: 1,
                    },
                ))),
                (CompareOp::Le, v) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::GraveyardMatching {
                        filter: Box::new(obj),
                        minimum: v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
                    },
                ))),
                _ => None,
            }
        }
        ConditionAst::GainedLifeThisTurn => Some(TriggerCondition::ControllerGainedLifeThisTurn),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount: 1,
        } => Some(TriggerCondition::ControllerLostLifeThisTurn),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            min_amount,
        } if *min_amount > 0 => Some(TriggerCondition::OpponentLostLifeAtLeast(
            u8::try_from(*min_amount).ok()?,
        )),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::AnyPlayer,
            min_amount,
        } if *min_amount > 0 => Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::MaxPlayerLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }))),
        ConditionAst::AttackedThisTurn => Some(TriggerCondition::ControllerAttackedThisTurn),
        ConditionAst::EnteredUnderYourControlThisTurn {
            filter,
            min_count: 1,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            Some(TriggerCondition::CreatureEnteredUnderControlThisTurn)
        }
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.is_empty() => Some(TriggerCondition::ControllerCastSpellsThisTurn(1)),
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } => match action.as_str() {
            "gained_life" | "gain_life" | "you gained life this turn" => {
                Some(TriggerCondition::ControllerGainedLifeThisTurn)
            }
            "lost_life" | "lose_life" | "you lost life this turn" => {
                Some(TriggerCondition::ControllerLostLifeThisTurn)
            }
            "attacked" | "attack" | "you attacked this turn" => {
                Some(TriggerCondition::ControllerAttackedThisTurn)
            }
            "discard" | "discarded" | "you discarded a card this turn" => {
                Some(TriggerCondition::ControllerDiscardedThisTurn)
            }
            "sacrifice" | "sacrificed" | "you sacrificed a permanent this turn" => {
                Some(TriggerCondition::ControllerSacrificedThisTurn)
            }
            "revolt"
            | "permanent_left_battlefield"
            | "a permanent you controlled left the battlefield this turn" => {
                Some(TriggerCondition::PermanentLeftBattlefieldThisTurn)
            }
            "descended" | "you descended this turn" => Some(TriggerCondition::DescendedThisTurn),
            "crime" | "committed_crime" | "you committed a crime this turn" => {
                Some(TriggerCondition::CommittedCrimeThisTurn)
            }
            "cast_two_spells" | "you've cast two or more spells this turn" => {
                Some(TriggerCondition::ControllerCastSpellsThisTurn(2))
            }
            "drew_two_cards" | "you've drawn two or more cards this turn" => {
                Some(TriggerCondition::ControllerDrewCardsThisTurn(2))
            }
            _ => None,
        },
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            Some(TriggerCondition::SourceMatches(Box::new(obj)))
        }
        ConditionAst::HasKeyword { subject, keyword }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            let kw = lower_keyword_ability_name(keyword)?;
            Some(TriggerCondition::SourceMatches(Box::new(
                ObjectFilter::any().with_keyword(kw),
            )))
        }
        ConditionAst::Marker(GameMarker::NotManaAbility) => {
            Some(TriggerCondition::TriggeringAbilityNotMana)
        }
        ConditionAst::Marker(GameMarker::DescendedThisTurn) => {
            Some(TriggerCondition::DescendedThisTurn)
        }
        ConditionAst::Marker(GameMarker::CommittedCrimeThisTurn) => {
            Some(TriggerCondition::CommittedCrimeThisTurn)
        }
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
            from_hand_only: false,
        }) if *value > 0 => Some(TriggerCondition::ControllerCastSpellsThisTurn(*value)),
        ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        }) if *value > 0 => Some(TriggerCondition::ControllerDrewCardsThisTurn(*value)),
        ConditionAst::SpellsCastLastTurn {
            any_player_single: false,
            op: CompareOp::Eq,
            value: 0,
        } => Some(TriggerCondition::PreviousTurnCasts(
            spellwake_engine::program::PreviousTurnCastCondition::NoSpells,
        )),
        ConditionAst::SpellsCastLastTurn {
            any_player_single: true,
            op: CompareOp::Ge,
            value: 2,
        } => Some(TriggerCondition::PreviousTurnCasts(
            spellwake_engine::program::PreviousTurnCastCondition::OnePlayerCastAtLeastTwo,
        )),
        _ => None,
    }
}

pub(super) fn lower_trigger_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<TriggerCondition, &'static str> {
    if let Some(specialized) = try_lower_specialized_trigger_condition(cond, vocabulary) {
        return Ok(specialized);
    }
    match cond {
        ConditionAst::Not(inner) => Ok(TriggerCondition::Not(Box::new(lower_trigger_condition(
            inner, vocabulary,
        )?))),
        ConditionAst::And(a, b) => Ok(TriggerCondition::And(
            Box::new(lower_trigger_condition(a, vocabulary)?),
            Box::new(lower_trigger_condition(b, vocabulary)?),
        )),
        ConditionAst::Or(a, b) => Ok(TriggerCondition::Or(
            Box::new(lower_trigger_condition(a, vocabulary)?),
            Box::new(lower_trigger_condition(b, vocabulary)?),
        )),
        other => {
            let mut tmp_builder = BodyBuilder::new();
            let mut rc =
                lower_resolution_condition(other, &mut tmp_builder, vocabulary, true, None)
                    .map_err(|_| "oracle:unsupported-trigger-condition")?;
            if tmp_builder.target == BodyTarget::None {
                rewrite_value_expr_anaphora(&mut rc.left, ValueObjectRef::TriggerSubject);
                rewrite_value_expr_anaphora(&mut rc.right, ValueObjectRef::TriggerSubject);
            }
            if tmp_builder.target == BodyTarget::None && rc.is_trigger_world_evaluable() {
                Ok(TriggerCondition::Eval(Box::new(rc)))
            } else {
                Err("oracle:unsupported-trigger-condition")
            }
        }
    }
}
